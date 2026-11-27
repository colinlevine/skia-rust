/*
 * Safe Rust bridge for:
 *   src/codec/SkPngCodec.cpp
 * (Phase 2 Module P2.4: skia-rs-codec-png, GN target: png_decode_libpng).
 *
 * Keeps thin third_party/libpng boundary calls (`png_*` and `setjmp`) while
 * delegating chunk header parsing/encoding, color profile planning, IHDR/tRNS/sBIT
 * transformation planning, and normal/interlaced decode state machines to safe Rust.
 */

#include "src/codec/SkPngCodec.h"

#include "include/codec/SkEncodedOrigin.h"
#include "include/codec/SkPngChunkReader.h"
#include "include/codec/SkPngDecoder.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRect.h"
#include "include/core/SkSize.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/core/SkTypes.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkNoncopyable.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkPngCompositeChunkReader.h"
#include "src/codec/SkPngPriv.h"
#include "src/codec/SkSwizzler.h"

#include <setjmp.h>
#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>

#include <png.h>
#include <pngconf.h>

using namespace skia_private;

#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
#include "include/android/SkAndroidFrameworkUtils.h"
#endif

#if defined(__GNUC__) && !defined(__clang__)
#pragma GCC diagnostic ignored "-Wclobbered"
#endif

#define PNG_JMPBUF(x) png_jmpbuf((png_structp)x)

void sk_warning_fn(png_structp, png_const_charp msg) {
    (void)msg;
    SkCodecPrintf("----- png warning %s\n", msg);
}

namespace {

// Discriminant & layout assertions shared across FFI with `skia-rs-codec-png`.
static_assert(PNG_COLOR_TYPE_GRAY == 0);
static_assert(PNG_COLOR_TYPE_RGB == 2);
static_assert(PNG_COLOR_TYPE_PALETTE == 3);
static_assert(PNG_COLOR_TYPE_GRAY_ALPHA == 4);
static_assert(PNG_COLOR_TYPE_RGBA == 6);
static_assert(kGraySigBit_GrayAlphaIsJustAlpha == 1);

static_assert(static_cast<int>(SkEncodedInfo::kGray_Color) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kGrayAlpha_Color) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kXAlpha_Color) == 2);
static_assert(static_cast<int>(SkEncodedInfo::k565_Color) == 3);
static_assert(static_cast<int>(SkEncodedInfo::kPalette_Color) == 4);
static_assert(static_cast<int>(SkEncodedInfo::kRGB_Color) == 5);
static_assert(static_cast<int>(SkEncodedInfo::kRGBA_Color) == 6);

static_assert(static_cast<int>(SkEncodedInfo::kOpaque_Alpha) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kUnpremul_Alpha) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kBinary_Alpha) == 2);

static_assert(static_cast<int>(SkCodec::kSuccess) == 0);
static_assert(static_cast<int>(SkCodec::kIncompleteInput) == 1);
static_assert(static_cast<int>(SkCodec::kErrorInInput) == 2);
static_assert(static_cast<int>(SkCodec::kInvalidInput) == 6);
static_assert(static_cast<int>(SkCodec::kInternalError) == 8);

static_assert(sizeof(skcms_TransferFunction) == 7 * sizeof(float));
static_assert(sizeof(skcms_Matrix3x3) == 9 * sizeof(float));

enum SkiaRsPngColorProfileKind : int32_t {
    kColorProfile_UseIcc = 0,
    kColorProfile_UseSrgbNull = 1,
    kColorProfile_UseCustomTfAndMatrix = 2,
};

struct SkiaRsPngColorProfilePlan {
    int32_t kind;
    skcms_TransferFunction transfer_fn;
    skcms_Matrix3x3 to_xyzd50;
};

struct SkiaRsPngDecodeHeaderPlan {
    bool strip_16;
    bool set_packing;
    bool expand_gray_1_2_4_to_8;
    bool trns_to_alpha;
    int32_t output_bit_depth;
    int32_t profile_check_color;
    int32_t final_encoded_color;
    int32_t encoded_alpha;
    bool android_565_sbit_warning;
};

enum SkiaRsPngNormalRowAction : int32_t {
    kNormalRow_Ignore = 0,
    kNormalRow_TransformAndContinue = 1,
    kNormalRow_TransformAndStop = 2,
    kNormalRow_Stop = 3,
};

struct SkiaRsPngNormalDecodeState {
    int32_t rows_written;
    int32_t first_row;
    int32_t last_row;
    int32_t rows_needed;
};

struct SkiaRsPngInterlacedDecodeState {
    int32_t number_passes;
    int32_t image_height;
    int32_t first_row;
    int32_t last_row;
    int32_t lines_decoded;
    bool interlaced_complete;
    size_t png_rowbytes;
};

struct SkiaRsPngInterlacedRowAction {
    bool ignore;
    size_t buffer_offset;
    bool stop_decoding;
};

struct SkiaRsPngInterlacedOutputSchedule {
    bool has_lines;
    int32_t start_src_row;
    int32_t step;
    int32_t rows_to_write;
};

extern "C" {
bool skia_rs_png_parse_chunk_header(const uint8_t* bytes,
                                    size_t len,
                                    uint32_t* out_length,
                                    bool* out_is_idat,
                                    bool* out_is_iend,
                                    size_t* out_payload_plus_crc);
void skia_rs_png_encode_idat_header(uint32_t length, uint8_t* out_buf);
void skia_rs_png_plan_color_profile(bool has_iccp,
                                    bool has_srgb,
                                    bool has_chrm,
                                    const int32_t* chrm_fixed,
                                    bool has_gama,
                                    int32_t gama_fixed,
                                    SkiaRsPngColorProfilePlan* out_plan);
void skia_rs_png_plan_decode_header(int32_t encoded_color_type,
                                    int32_t bit_depth,
                                    bool has_trns,
                                    bool has_sbit,
                                    uint8_t sbit_red,
                                    uint8_t sbit_green,
                                    uint8_t sbit_blue,
                                    uint8_t sbit_gray,
                                    uint8_t sbit_alpha,
                                    SkiaRsPngDecodeHeaderPlan* out_plan);

void skia_rs_png_normal_start_all_rows(SkiaRsPngNormalDecodeState* state, int32_t height);
void skia_rs_png_normal_on_all_rows(SkiaRsPngNormalDecodeState* state, int32_t row_num);
int32_t skia_rs_png_normal_finish_all_rows(const SkiaRsPngNormalDecodeState* state,
                                           bool process_succeeded,
                                           int32_t height,
                                           int32_t* out_rows_decoded);
void skia_rs_png_normal_set_range(SkiaRsPngNormalDecodeState* state,
                                  int32_t first_row,
                                  int32_t last_row);
void skia_rs_png_normal_prepare_decode(SkiaRsPngNormalDecodeState* state,
                                       bool has_swizzler,
                                       int32_t sample_y);
bool skia_rs_png_normal_relative_row(const SkiaRsPngNormalDecodeState* state,
                                     int32_t row_num,
                                     int32_t* out_rel_row);
int32_t skia_rs_png_normal_on_row(SkiaRsPngNormalDecodeState* state,
                                  int32_t row_num,
                                  bool row_needed);
int32_t skia_rs_png_normal_finish_decode(const SkiaRsPngNormalDecodeState* state,
                                         bool process_succeeded,
                                         int32_t* out_rows_decoded);

void skia_rs_png_interlaced_init(SkiaRsPngInterlacedDecodeState* state,
                                 int32_t number_passes,
                                 int32_t image_height);
bool skia_rs_png_interlaced_buffer_size(size_t png_rowbytes, int32_t height, size_t* out_size);
void skia_rs_png_interlaced_reset_range(SkiaRsPngInterlacedDecodeState* state,
                                        int32_t first_row,
                                        int32_t last_row,
                                        size_t png_rowbytes);
void skia_rs_png_interlaced_on_row(SkiaRsPngInterlacedDecodeState* state,
                                   int32_t row_num,
                                   int32_t pass,
                                   int32_t sample_y,
                                   SkiaRsPngInterlacedRowAction* out_action);
void skia_rs_png_interlaced_output_schedule(const SkiaRsPngInterlacedDecodeState* state,
                                            int32_t sample_y,
                                            SkiaRsPngInterlacedOutputSchedule* out_schedule);
int32_t skia_rs_png_interlaced_finish(const SkiaRsPngInterlacedDecodeState* state,
                                      bool process_succeeded,
                                      int32_t rows_written,
                                      int32_t* out_rows_decoded);
}

constexpr int kSetJmpOkay = 0;
constexpr int kPngError = 1;
constexpr int kStopDecoding = 2;

void sk_error_fn(png_structp png_ptr, png_const_charp msg) {
    (void)msg;
    SkCodecPrintf("------ png error %s\n", msg);
    longjmp(PNG_JMPBUF(png_ptr), kPngError);
}

#ifdef PNG_READ_UNKNOWN_CHUNKS_SUPPORTED
int sk_read_user_chunk(png_structp png_ptr, png_unknown_chunkp chunk) {
    SkPngChunkReader* chunkReader = (SkPngChunkReader*)png_get_user_chunk_ptr(png_ptr);
    return chunkReader->readChunk((const char*)chunk->name, chunk->data, chunk->size) ? 1 : -1;
}
#endif

inline bool process_stream_bytes(png_structp png_ptr,
                                 png_infop info_ptr,
                                 SkStream* stream,
                                 void* buffer,
                                 size_t bufferSize,
                                 size_t length) {
    while (length > 0) {
        const size_t bytesToProcess = std::min(bufferSize, length);
        const size_t bytesRead = stream->read(buffer, bytesToProcess);
        png_process_data(png_ptr, info_ptr, static_cast<png_bytep>(buffer), bytesRead);
        if (bytesRead < bytesToProcess) {
            return false;
        }
        length -= bytesToProcess;
    }
    return true;
}

SkCodec::Result maybe_log_android_error(SkCodec::Result res) {
#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
    if (res == SkCodec::kErrorInInput) {
        SkAndroidFrameworkUtils::SafetyNetLog("117838472");
    }
#endif
    return res;
}

std::unique_ptr<SkCodecs::ColorProfile> read_color_profile(png_structp png_ptr,
                                                           png_infop info_ptr) {
#if (PNG_LIBPNG_VER_MAJOR > 1) || (PNG_LIBPNG_VER_MAJOR == 1 && PNG_LIBPNG_VER_MINOR >= 6)
    png_bytep profile = nullptr;
    png_uint_32 length = 0;
    png_charp name = nullptr;
    int compression = 0;
    const bool hasIccp =
            (PNG_INFO_iCCP == png_get_iCCP(png_ptr, info_ptr, &name, &compression, &profile, &length));
    const bool hasSrgb = !hasIccp && (png_get_valid(png_ptr, info_ptr, PNG_INFO_sRGB) != 0);

    png_fixed_point chrm[8] = {};
    bool hasChrm = false;
    png_fixed_point gamma = 0;
    bool hasGama = false;
    if (!hasIccp && !hasSrgb) {
        hasChrm = png_get_cHRM_fixed(png_ptr,
                                     info_ptr,
                                     &chrm[0],
                                     &chrm[1],
                                     &chrm[2],
                                     &chrm[3],
                                     &chrm[4],
                                     &chrm[5],
                                     &chrm[6],
                                     &chrm[7]) != 0;
        hasGama = (PNG_INFO_gAMA == png_get_gAMA_fixed(png_ptr, info_ptr, &gamma));
    }

    int32_t chrmI32[8];
    for (int i = 0; i < 8; ++i) {
        chrmI32[i] = static_cast<int32_t>(chrm[i]);
    }

    SkiaRsPngColorProfilePlan plan{};
    skia_rs_png_plan_color_profile(
            hasIccp, hasSrgb, hasChrm, chrmI32, hasGama, static_cast<int32_t>(gamma), &plan);

    switch (static_cast<SkiaRsPngColorProfileKind>(plan.kind)) {
        case kColorProfile_UseIcc: {
            auto data = SkData::MakeWithCopy(profile, length);
            return SkCodecs::ColorProfile::MakeICCProfile(std::move(data));
        }
        case kColorProfile_UseSrgbNull:
            return nullptr;
        case kColorProfile_UseCustomTfAndMatrix:
            return SkCodecs::ColorProfile::Make(plan.transfer_fn, plan.to_xyzd50);
    }
    return nullptr;
#else
    return nullptr;
#endif
}

class SkPngNormalDecoder final : public SkPngCodec {
public:
    SkPngNormalDecoder(SkEncodedInfo&& info,
                       std::unique_ptr<SkStream> stream,
                       sk_sp<SkPngCompositeChunkReader> reader,
                       png_structp png_ptr,
                       png_infop info_ptr,
                       std::unique_ptr<SkStream> gainmapStream,
                       std::optional<SkGainmapInfo> gainmapInfo)
            : SkPngCodec(std::move(info),
                         std::move(stream),
                         std::move(reader),
                         png_ptr,
                         info_ptr,
                         std::move(gainmapStream),
                         gainmapInfo)
            , fState{}
            , fDst(nullptr)
            , fRowBytes(0) {}

    static void AllRowsCallback(png_structp png_ptr,
                                png_bytep row,
                                png_uint_32 rowNum,
                                int /*pass*/) {
        GetDecoder(png_ptr)->allRowsCallback(row, static_cast<int>(rowNum));
    }

    static void RowCallback(png_structp png_ptr,
                            png_bytep row,
                            png_uint_32 rowNum,
                            int /*pass*/) {
        GetDecoder(png_ptr)->rowCallback(row, static_cast<int>(rowNum));
    }

private:
    SkiaRsPngNormalDecodeState fState;
    void* fDst;
    size_t fRowBytes;

    static SkPngNormalDecoder* GetDecoder(png_structp png_ptr) {
        return static_cast<SkPngNormalDecoder*>(png_get_progressive_ptr(png_ptr));
    }

    Result decodeAllRows(void* dst, size_t rowBytes, int* rowsDecoded) override {
        const int height = this->dimensions().height();
        png_set_progressive_read_fn(this->png_ptr(), this, nullptr, AllRowsCallback, nullptr);
        fDst = dst;
        fRowBytes = rowBytes;
        skia_rs_png_normal_start_all_rows(&fState, height);

        const bool success = this->processData();
        int32_t written = 0;
        const auto res = static_cast<Result>(
                skia_rs_png_normal_finish_all_rows(&fState, success, height, &written));
        if (res != kSuccess && rowsDecoded) {
            *rowsDecoded = written;
        }
        return maybe_log_android_error(res);
    }

    void allRowsCallback(png_bytep row, int rowNum) {
        SkASSERT(rowNum == fState.rows_written);
        skia_rs_png_normal_on_all_rows(&fState, rowNum);
        this->applyXformRow(fDst, row);
        fDst = SkTAddOffset<void>(fDst, fRowBytes);
    }

    Result setRange(int firstRow, int lastRow, void* dst, size_t rowBytes) override {
        png_set_progressive_read_fn(this->png_ptr(), this, nullptr, RowCallback, nullptr);
        fDst = dst;
        fRowBytes = rowBytes;
        skia_rs_png_normal_set_range(&fState, firstRow, lastRow);
        return kSuccess;
    }

    Result decode(int* rowsDecoded) override {
        const bool hasSwizzler = (this->swizzler() != nullptr);
        const int sampleY = hasSwizzler ? this->swizzler()->sampleY() : 1;
        skia_rs_png_normal_prepare_decode(&fState, hasSwizzler, sampleY);

        const bool success = this->processData();
        int32_t written = 0;
        const auto res = static_cast<Result>(
                skia_rs_png_normal_finish_decode(&fState, success, &written));
        if (res != kSuccess && rowsDecoded) {
            *rowsDecoded = written;
        }
        return maybe_log_android_error(res);
    }

    void rowCallback(png_bytep row, int rowNum) {
        int32_t relRow = 0;
        if (!skia_rs_png_normal_relative_row(&fState, rowNum, &relRow)) {
            return;
        }

        const bool rowNeeded = !this->swizzler() || this->swizzler()->rowNeeded(relRow);
        const auto action = static_cast<SkiaRsPngNormalRowAction>(
                skia_rs_png_normal_on_row(&fState, rowNum, rowNeeded));
        switch (action) {
            case kNormalRow_Ignore:
                break;
            case kNormalRow_TransformAndContinue:
                this->applyXformRow(fDst, row);
                fDst = SkTAddOffset<void>(fDst, fRowBytes);
                break;
            case kNormalRow_TransformAndStop:
                this->applyXformRow(fDst, row);
                fDst = SkTAddOffset<void>(fDst, fRowBytes);
                longjmp(PNG_JMPBUF(this->png_ptr()), kStopDecoding);
            case kNormalRow_Stop:
                longjmp(PNG_JMPBUF(this->png_ptr()), kStopDecoding);
        }
    }
};

class SkPngInterlacedDecoder final : public SkPngCodec {
public:
    SkPngInterlacedDecoder(SkEncodedInfo&& info,
                           std::unique_ptr<SkStream> stream,
                           sk_sp<SkPngCompositeChunkReader> reader,
                           png_structp png_ptr,
                           png_infop info_ptr,
                           int numberPasses,
                           std::unique_ptr<SkStream> gainmapStream,
                           std::optional<SkGainmapInfo> gainmapInfo)
            : SkPngCodec(std::move(info),
                         std::move(stream),
                         std::move(reader),
                         png_ptr,
                         info_ptr,
                         std::move(gainmapStream),
                         gainmapInfo)
            , fState{}
            , fDst(nullptr)
            , fRowBytes(0) {
        skia_rs_png_interlaced_init(&fState, numberPasses, this->dimensions().height());
    }

    static void InterlacedRowCallback(png_structp png_ptr,
                                      png_bytep row,
                                      png_uint_32 rowNum,
                                      int pass) {
        auto* decoder = static_cast<SkPngInterlacedDecoder*>(png_get_progressive_ptr(png_ptr));
        decoder->interlacedRowCallback(row, static_cast<int>(rowNum), pass);
    }

private:
    SkiaRsPngInterlacedDecodeState fState;
    void* fDst;
    size_t fRowBytes;
    std::unique_ptr<png_byte, SkOverloadedFunctionObject<void(void*), sk_free>> fInterlaceBuffer;

    void interlacedRowCallback(png_bytep row, int rowNum, int pass) {
        const int sampleY = this->swizzler() ? this->swizzler()->sampleY() : 1;
        SkiaRsPngInterlacedRowAction action{};
        skia_rs_png_interlaced_on_row(&fState, rowNum, pass, sampleY, &action);
        if (action.ignore) {
            return;
        }

        png_bytep oldRow = fInterlaceBuffer.get() + action.buffer_offset;
        png_progressive_combine_row(this->png_ptr(), oldRow, row);

        if (action.stop_decoding) {
            longjmp(PNG_JMPBUF(this->png_ptr()), kStopDecoding);
        }
    }

    Result setUpInterlaceBuffer(int firstRow, int lastRow) {
        const size_t pngRowBytes = png_get_rowbytes(this->png_ptr(), this->info_ptr());
        const int height = lastRow - firstRow + 1;
        size_t interlaceBufferSize = 0;
        if (!skia_rs_png_interlaced_buffer_size(pngRowBytes, height, &interlaceBufferSize)) {
            return kInternalError;
        }
        void* interlaceBufferRaw = nullptr;
        if (interlaceBufferSize) {
            interlaceBufferRaw = sk_malloc_canfail(interlaceBufferSize, sizeof(png_byte));
            if (!interlaceBufferRaw) {
                return kInternalError;
            }
        }
        fInterlaceBuffer.reset(reinterpret_cast<png_byte*>(interlaceBufferRaw));
        skia_rs_png_interlaced_reset_range(&fState, firstRow, lastRow, pngRowBytes);
        return kSuccess;
    }

    Result decodeAllRows(void* dst, size_t rowBytes, int* rowsDecoded) override {
        const int height = this->dimensions().height();
        Result res = this->setUpInterlaceBuffer(0, height - 1);
        if (res != kSuccess) {
            return res;
        }
        png_set_progressive_read_fn(
                this->png_ptr(), this, nullptr, InterlacedRowCallback, nullptr);

        const bool success = this->processData();
        png_bytep srcRow = fInterlaceBuffer.get();
        for (int rowNum = 0; rowNum < fState.lines_decoded; rowNum++) {
            this->applyXformRow(dst, srcRow);
            dst = SkTAddOffset<void>(dst, rowBytes);
            srcRow = SkTAddOffset<png_byte>(srcRow, fState.png_rowbytes);
        }

        int32_t written = 0;
        const auto finalRes = static_cast<Result>(skia_rs_png_interlaced_finish(
                &fState, success, fState.lines_decoded, &written));
        if (finalRes != kSuccess && rowsDecoded) {
            *rowsDecoded = written;
        }
        return maybe_log_android_error(finalRes);
    }

    Result setRange(int firstRow, int lastRow, void* dst, size_t rowBytes) override {
        Result res = this->setUpInterlaceBuffer(firstRow, lastRow);
        if (res != kSuccess) {
            return res;
        }
        png_set_progressive_read_fn(
                this->png_ptr(), this, nullptr, InterlacedRowCallback, nullptr);
        fDst = dst;
        fRowBytes = rowBytes;
        return kSuccess;
    }

    Result decode(int* rowsDecoded) override {
        const bool success = this->processData();

        const int sampleY = this->swizzler() ? this->swizzler()->sampleY() : 1;
        SkiaRsPngInterlacedOutputSchedule sched{};
        skia_rs_png_interlaced_output_schedule(&fState, sampleY, &sched);

        if (!sched.has_lines) {
            if (rowsDecoded) {
                *rowsDecoded = 0;
            }
            return maybe_log_android_error(success ? kIncompleteInput : kErrorInInput);
        }

        int srcRow = sched.start_src_row;
        void* dst = fDst;
        for (int i = 0; i < sched.rows_to_write; ++i) {
            png_bytep src = SkTAddOffset<png_byte>(
                    fInterlaceBuffer.get(), fState.png_rowbytes * static_cast<size_t>(srcRow));
            this->applyXformRow(dst, src);
            dst = SkTAddOffset<void>(dst, fRowBytes);
            srcRow += sched.step;
        }

        int32_t written = 0;
        const auto finalRes = static_cast<Result>(skia_rs_png_interlaced_finish(
                &fState, success, sched.rows_to_write, &written));
        if (finalRes != kSuccess && rowsDecoded) {
            *rowsDecoded = written;
        }
        return maybe_log_android_error(finalRes);
    }
};

class AutoCleanPng : public SkNoncopyable {
public:
    AutoCleanPng(png_structp png_ptr,
                 SkStream* stream,
                 sk_sp<SkPngCompositeChunkReader> reader,
                 SkCodec** codecPtr)
            : fPng_ptr(png_ptr)
            , fInfo_ptr(nullptr)
            , fStream(stream)
            , fChunkReader(std::move(reader))
            , fOutCodec(codecPtr) {}

    ~AutoCleanPng() {
        if (fPng_ptr) {
            png_infopp info_pp = fInfo_ptr ? &fInfo_ptr : nullptr;
            png_destroy_read_struct(&fPng_ptr, info_pp, nullptr);
        }
    }

    void setInfoPtr(png_infop info_ptr) {
        SkASSERT(nullptr == fInfo_ptr);
        fInfo_ptr = info_ptr;
    }

    bool decodeBounds();

private:
    png_structp fPng_ptr;
    png_infop fInfo_ptr;
    SkStream* fStream;
    sk_sp<SkPngCompositeChunkReader> fChunkReader;
    SkCodec** fOutCodec;

    void infoCallback(size_t idatLength);

    void releasePngPtrs() {
        fPng_ptr = nullptr;
        fInfo_ptr = nullptr;
    }
};

bool AutoCleanPng::decodeBounds() {
    SkASSERT(fStream);
    if (setjmp(PNG_JMPBUF(fPng_ptr))) {
        return false;
    }

    png_set_progressive_read_fn(fPng_ptr, nullptr, nullptr, nullptr, nullptr);

    constexpr size_t kBufferSize = 4096;
    char buffer[kBufferSize];

    {
        if (fStream->read(buffer, 8) < 8) {
            return false;
        }
        png_process_data(fPng_ptr, fInfo_ptr, reinterpret_cast<png_bytep>(buffer), 8);
    }

    while (true) {
        if (fStream->read(buffer, 8) < 8) {
            break;
        }

        png_byte* chunk = reinterpret_cast<png_byte*>(buffer);
        uint32_t length = 0;
        bool isIdat = false;
        bool isIend = false;
        size_t payloadPlusCrc = 0;
        if (!skia_rs_png_parse_chunk_header(
                    chunk, 8, &length, &isIdat, &isIend, &payloadPlusCrc)) {
            return false;
        }

        if (isIdat) {
            this->infoCallback(static_cast<size_t>(length));
            return true;
        }

        png_process_data(fPng_ptr, fInfo_ptr, chunk, 8);
        if (!process_stream_bytes(
                    fPng_ptr, fInfo_ptr, fStream, buffer, kBufferSize, payloadPlusCrc)) {
            return false;
        }
    }

    return false;
}

void AutoCleanPng::infoCallback(size_t idatLength) {
    png_uint_32 origWidth, origHeight;
    int bitDepth, encodedColorType;
    png_get_IHDR(fPng_ptr,
                 fInfo_ptr,
                 &origWidth,
                 &origHeight,
                 &bitDepth,
                 &encodedColorType,
                 nullptr,
                 nullptr,
                 nullptr);

    const bool hasTrns = (png_get_valid(fPng_ptr, fInfo_ptr, PNG_INFO_tRNS) != 0);
    png_color_8p sigBits = nullptr;
    const bool hasSbit = (png_get_sBIT(fPng_ptr, fInfo_ptr, &sigBits) != 0) && (sigBits != nullptr);

    SkiaRsPngDecodeHeaderPlan plan{};
    skia_rs_png_plan_decode_header(encodedColorType,
                                   bitDepth,
                                   hasTrns,
                                   hasSbit,
                                   hasSbit ? sigBits->red : 0,
                                   hasSbit ? sigBits->green : 0,
                                   hasSbit ? sigBits->blue : 0,
                                   hasSbit ? sigBits->gray : 0,
                                   hasSbit ? sigBits->alpha : 0,
                                   &plan);

    if (plan.strip_16) {
        png_set_strip_16(fPng_ptr);
    }
    if (plan.set_packing) {
        png_set_packing(fPng_ptr);
    }
    if (plan.expand_gray_1_2_4_to_8) {
        png_set_expand_gray_1_2_4_to_8(fPng_ptr);
    }
    if (plan.trns_to_alpha) {
        png_set_tRNS_to_alpha(fPng_ptr);
    }

    const int numberPasses = png_set_interlace_handling(fPng_ptr);

    if (fOutCodec) {
        SkASSERT(nullptr == *fOutCodec);
        auto profile = read_color_profile(fPng_ptr, fInfo_ptr);
        const auto profileCheckColor = static_cast<SkEncodedInfo::Color>(plan.profile_check_color);
        if (!SkPngCodecBase::isCompatibleColorProfileAndType(profile.get(), profileCheckColor)) {
            profile = nullptr;
        }

#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
        if (plan.android_565_sbit_warning) {
            SkAndroidFrameworkUtils::SafetyNetLog("190188264");
        }
#endif

        const auto finalColor = static_cast<SkEncodedInfo::Color>(plan.final_encoded_color);
        const auto finalAlpha = static_cast<SkEncodedInfo::Alpha>(plan.encoded_alpha);
        SkEncodedInfo encodedInfo = SkEncodedInfo::Make(
                origWidth,
                origHeight,
                finalColor,
                finalAlpha,
                plan.output_bit_depth,
                plan.output_bit_depth,
                std::move(profile),
                fChunkReader ? fChunkReader->getHdrMetadata() : skhdr::Metadata::MakeEmpty());

        std::unique_ptr<SkStream> gainmapStream =
                fChunkReader ? fChunkReader->takeGainmapStream() : nullptr;
        std::optional<SkGainmapInfo> gainmapInfo =
                fChunkReader ? fChunkReader->getGainmapInfo() : std::nullopt;

        if (1 == numberPasses) {
            *fOutCodec = new SkPngNormalDecoder(std::move(encodedInfo),
                                                std::unique_ptr<SkStream>(fStream),
                                                fChunkReader,
                                                fPng_ptr,
                                                fInfo_ptr,
                                                std::move(gainmapStream),
                                                gainmapInfo);
        } else {
            *fOutCodec = new SkPngInterlacedDecoder(std::move(encodedInfo),
                                                    std::unique_ptr<SkStream>(fStream),
                                                    fChunkReader,
                                                    fPng_ptr,
                                                    fInfo_ptr,
                                                    numberPasses,
                                                    std::move(gainmapStream),
                                                    gainmapInfo);
        }
        static_cast<SkPngCodec*>(*fOutCodec)->setIdatLength(idatLength);
    }

    this->releasePngPtrs();
}

SkCodec::Result read_header(SkStream* stream,
                            const sk_sp<SkPngCompositeChunkReader>& chunkReader,
                            SkCodec** outCodec,
                            png_structp* png_ptrp,
                            png_infop* info_ptrp) {
    png_structp png_ptr =
            png_create_read_struct(PNG_LIBPNG_VER_STRING, nullptr, sk_error_fn, ::sk_warning_fn);
    if (!png_ptr) {
        return SkCodec::kInternalError;
    }

#ifdef PNG_SET_OPTION_SUPPORTED
    png_set_option(png_ptr, PNG_MAXIMUM_INFLATE_WINDOW, PNG_OPTION_ON);
#endif

    AutoCleanPng autoClean(png_ptr, stream, chunkReader, outCodec);

    png_infop info_ptr = png_create_info_struct(png_ptr);
    if (info_ptr == nullptr) {
        return SkCodec::kInternalError;
    }

    autoClean.setInfoPtr(info_ptr);

    if (setjmp(PNG_JMPBUF(png_ptr))) {
        return SkCodec::kInvalidInput;
    }

#ifdef PNG_READ_UNKNOWN_CHUNKS_SUPPORTED
    if (chunkReader) {
        png_set_keep_unknown_chunks(png_ptr, PNG_HANDLE_CHUNK_ALWAYS, (png_const_bytep)"", 0);
        png_set_read_user_chunk_fn(png_ptr, (png_voidp)chunkReader.get(), sk_read_user_chunk);
    }
#endif

    const bool decodedBounds = autoClean.decodeBounds();
    if (!decodedBounds) {
        return SkCodec::kIncompleteInput;
    }

    if (png_ptrp) {
        *png_ptrp = png_ptr;
    }
    if (info_ptrp) {
        *info_ptrp = info_ptr;
    }
    if (outCodec) {
        SkASSERT(*outCodec);
    }
    return SkCodec::kSuccess;
}

constexpr SkEncodedOrigin kDefaultEncodedOrigin = kTopLeft_SkEncodedOrigin;

}  // namespace

bool SkPngCodec::processData() {
    switch (setjmp(PNG_JMPBUF(fPng_ptr))) {
        case kPngError:
            return false;
        case kStopDecoding:
            return true;
        case kSetJmpOkay:
            break;
        default:
            SkASSERT(false);
    }

    constexpr size_t kBufferSize = 4096;
    char buffer[kBufferSize];

    bool iend = false;
    while (true) {
        size_t payloadPlusCrc = 0;
        if (fDecodedIdat) {
            if (this->stream()->read(buffer, 8) < 8) {
                break;
            }
            png_byte* chunk = reinterpret_cast<png_byte*>(buffer);
            png_process_data(fPng_ptr, fInfo_ptr, chunk, 8);

            uint32_t length = 0;
            bool isIdat = false;
            if (!skia_rs_png_parse_chunk_header(
                        chunk, 8, &length, &isIdat, &iend, &payloadPlusCrc)) {
                return false;
            }
        } else {
            uint8_t idat[8];
            skia_rs_png_encode_idat_header(static_cast<uint32_t>(fIdatLength), idat);
            png_process_data(fPng_ptr, fInfo_ptr, idat, 8);
            fDecodedIdat = true;
            payloadPlusCrc = fIdatLength + 4;
        }

        if (!process_stream_bytes(
                    fPng_ptr, fInfo_ptr, this->stream(), buffer, kBufferSize, payloadPlusCrc) ||
            iend) {
            break;
        }
    }

    return true;
}

std::optional<SkSpan<const SkPngCodecBase::PaletteColorEntry>> SkPngCodec::onTryGetPlteChunk() {
    int numColors;
    png_color* palette;
    if (!png_get_PLTE(fPng_ptr, fInfo_ptr, &palette, &numColors)) {
        return std::nullopt;
    }

    static_assert(sizeof(png_color) == sizeof(PaletteColorEntry));
    return SkSpan(reinterpret_cast<const PaletteColorEntry*>(palette), numColors);
}

std::optional<SkSpan<const uint8_t>> SkPngCodec::onTryGetTrnsChunk() {
    png_bytep alphas;
    int numColorsWithAlpha = 0;
    if (!png_get_tRNS(fPng_ptr, fInfo_ptr, &alphas, &numColorsWithAlpha, nullptr)) {
        return std::nullopt;
    }
    return SkSpan(alphas, numColorsWithAlpha);
}

SkPngCodec::SkPngCodec(SkEncodedInfo&& encodedInfo,
                       std::unique_ptr<SkStream> stream,
                       sk_sp<SkPngCompositeChunkReader> chunkReader,
                       void* png_ptr,
                       void* info_ptr,
                       std::unique_ptr<SkStream> gainmapStream,
                       std::optional<SkGainmapInfo> gainmapInfo)
        : SkPngCodecBase(std::move(encodedInfo),
                         std::move(stream),
                         kDefaultEncodedOrigin,
                         std::move(chunkReader),
                         std::move(gainmapStream),
                         gainmapInfo)
        , fPng_ptr(png_ptr)
        , fInfo_ptr(info_ptr)
        , fIdatLength(0)
        , fDecodedIdat(false) {}

SkPngCodec::~SkPngCodec() { this->destroyReadStruct(); }

void SkPngCodec::destroyReadStruct() {
    if (fPng_ptr) {
        SkASSERT(fInfo_ptr);
        png_destroy_read_struct((png_struct**)&fPng_ptr, (png_info**)&fInfo_ptr, nullptr);
        fPng_ptr = nullptr;
        fInfo_ptr = nullptr;
    }
}

SkCodec::Result SkPngCodec::initializeXforms(const SkImageInfo& dstInfo, const Options& options) {
    if (setjmp(PNG_JMPBUF((png_struct*)fPng_ptr))) {
        SkCodecPrintf("Failed on png_read_update_info.\n");
        return kInvalidInput;
    }
    png_read_update_info(fPng_ptr, fInfo_ptr);

    int frameWidth = dstInfo.width();
    return SkPngCodecBase::initializeXforms(dstInfo, options, frameWidth);
}

bool SkPngCodec::onRewind() {
    if (!this->rewindStream()) {
        return false;
    }
    this->destroyReadStruct();

    png_structp png_ptr;
    png_infop info_ptr;
    if (kSuccess != read_header(this->stream(), fPngChunkReader, nullptr, &png_ptr, &info_ptr)) {
        return false;
    }

    fPng_ptr = png_ptr;
    fInfo_ptr = info_ptr;
    fDecodedIdat = false;
    return true;
}

SkCodec::Result SkPngCodec::onGetPixels(const SkImageInfo& dstInfo,
                                        void* dst,
                                        size_t rowBytes,
                                        const Options& options,
                                        int* rowsDecoded) {
    Result result = this->initializeXforms(dstInfo, options);
    if (kSuccess != result) {
        return result;
    }

    if (options.fSubset) {
        return kUnimplemented;
    }

    this->initializeXformParams();
    return this->decodeAllRows(dst, rowBytes, rowsDecoded);
}

SkCodec::Result SkPngCodec::onStartIncrementalDecode(const SkImageInfo& dstInfo,
                                                     void* dst,
                                                     size_t rowBytes,
                                                     const SkCodec::Options& options) {
    Result result = this->initializeXforms(dstInfo, options);
    if (kSuccess != result) {
        return result;
    }

    int firstRow, lastRow;
    if (options.fSubset) {
        firstRow = options.fSubset->top();
        lastRow = options.fSubset->bottom() - 1;
    } else {
        firstRow = 0;
        lastRow = dstInfo.height() - 1;
    }
    return this->setRange(firstRow, lastRow, dst, rowBytes);
}

SkCodec::Result SkPngCodec::onIncrementalDecode(int* rowsDecoded) {
    this->initializeXformParams();
    return this->decode(rowsDecoded);
}

std::unique_ptr<SkCodec> SkPngCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                    Result* result,
                                                    SkPngChunkReader* chunkReader) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }
    SkCodec* outCodec = nullptr;
    auto compositeReader = sk_make_sp<SkPngCompositeChunkReader>(chunkReader);
    *result = read_header(stream.get(), compositeReader, &outCodec, nullptr, nullptr);
    if (kSuccess == *result) {
        SkASSERT(outCodec);
        stream.release();
    }
    return std::unique_ptr<SkCodec>(outCodec);
}

std::unique_ptr<SkCodec> SkPngCodec::onDecodeGainmap(std::unique_ptr<SkStream> stream,
                                                     SkCodec::Result* result) {
    return SkPngCodec::MakeFromStream(std::move(stream), result, fPngChunkReader.get());
}

namespace SkPngDecoder {
bool IsPng(const void* data, size_t len) { return SkPngCodecBase::IsPng(data, len); }

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext ctx) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    SkPngChunkReader* chunkReader = nullptr;
    if (ctx) {
        chunkReader = static_cast<SkPngChunkReader*>(ctx);
    }
    return SkPngCodec::MakeFromStream(std::move(stream), outResult, chunkReader);
}

std::unique_ptr<SkCodec> Decode(sk_sp<const SkData> data,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext ctx) {
    if (!data) {
        if (outResult) {
            *outResult = SkCodec::kInvalidInput;
        }
        return nullptr;
    }
    return Decode(SkMemoryStream::Make(std::move(data)), outResult, ctx);
}
}  // namespace SkPngDecoder
