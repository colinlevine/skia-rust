/*
 * Safe Rust bridge for:
 *   src/codec/SkBmpBaseCodec.cpp
 *   src/codec/SkBmpCodec.cpp
 *   src/codec/SkBmpMaskCodec.cpp
 *   src/codec/SkBmpRLECodec.cpp
 *   src/codec/SkBmpStandardCodec.cpp
 *   src/codec/SkWbmpCodec.cpp
 * (Phase 2 Module P2.3: skia-rs-codec-bmp).
 */

#include "include/codec/SkBmpDecoder.h"
#include "include/codec/SkCodec.h"
#include "include/codec/SkEncodedImageFormat.h"
#include "include/codec/SkWbmpDecoder.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkBmpBaseCodec.h"
#include "src/codec/SkBmpCodec.h"
#include "src/codec/SkBmpMaskCodec.h"
#include "src/codec/SkBmpRLECodec.h"
#include "src/codec/SkBmpStandardCodec.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkColorPalette.h"
#include "src/codec/SkMaskSwizzler.h"
#include "src/codec/SkSampler.h"
#include "src/codec/SkSwizzler.h"
#include "src/codec/SkWbmpCodec.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkMasks.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>

namespace {

#ifdef SK_PMCOLOR_IS_BGRA
constexpr bool kPmColorIsBgra = true;
#else
constexpr bool kPmColorIsBgra = false;
#endif

// Compile-time verification of every enum discriminant shared across FFI.
static_assert(SkCodec::kSuccess == 0);
static_assert(SkCodec::kIncompleteInput == 1);
static_assert(SkCodec::kErrorInInput == 2);
static_assert(SkCodec::kInvalidConversion == 3);
static_assert(SkCodec::kInvalidScale == 4);
static_assert(SkCodec::kInvalidParameters == 5);
static_assert(SkCodec::kInvalidInput == 6);
static_assert(SkCodec::kCouldNotRewind == 7);
static_assert(SkCodec::kInternalError == 8);
static_assert(SkCodec::kUnimplemented == 9);
static_assert(SkCodec::kOutOfMemory == 10);

static_assert(SkEncodedInfo::kOpaque_Alpha == 0);
static_assert(SkEncodedInfo::kUnpremul_Alpha == 1);
static_assert(SkEncodedInfo::kBinary_Alpha == 2);

static_assert(SkEncodedInfo::kGray_Color == 0);
static_assert(SkEncodedInfo::kPalette_Color == 4);
static_assert(SkEncodedInfo::kBGR_Color == 7);
static_assert(SkEncodedInfo::kBGRX_Color == 8);
static_assert(SkEncodedInfo::kBGRA_Color == 9);

static_assert(SkCodec::kTopDown_SkScanlineOrder == 0);
static_assert(SkCodec::kBottomUp_SkScanlineOrder == 1);

static_assert(kUnknown_SkColorType == 0);
static_assert(kAlpha_8_SkColorType == 1);
static_assert(kRGB_565_SkColorType == 2);
static_assert(kRGBA_8888_SkColorType == 4);
static_assert(kBGRA_8888_SkColorType == 6);
static_assert(kGray_8_SkColorType == 14);
static_assert(kRGBA_F16_SkColorType == 16);

static_assert(kUnknown_SkAlphaType == 0);
static_assert(kOpaque_SkAlphaType == 1);
static_assert(kPremul_SkAlphaType == 2);
static_assert(kUnpremul_SkAlphaType == 3);

static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkMasks::InputMasks) == 16);

constexpr int32_t kBmpInputFormatStandard = 0;
constexpr int32_t kBmpInputFormatRle = 1;
constexpr int32_t kBmpInputFormatBitMask = 2;

struct RustBmpHeaderInfo {
    int32_t input_format;
    int32_t width;
    int32_t height;
    int32_t encoded_color;
    int32_t encoded_alpha;
    uint8_t bits_per_component;
    uint16_t bits_per_pixel;
    int32_t row_order;
    uint32_t num_colors;
    uint32_t bytes_per_color;
    uint32_t pixel_data_offset;
    bool is_opaque;
    SkMasks::InputMasks input_masks;
};

struct RustStandardSwizzlerEncodedInfo {
    int32_t color;
    int32_t alpha;
    uint8_t bits_per_component;
};

struct RustMaskSwizzlerTargetInfo {
    int32_t color_type;
    int32_t alpha_type;
};

struct RustRleSkipPlan {
    int32_t rows_advanced;
    int32_t remaining_height;
    bool fully_skipped;
};

using RustReadStreamFn = size_t (*)(void* ctx, uint8_t* dst, size_t size);
using RustSkipStreamFn = size_t (*)(void* ctx, size_t count);

size_t sk_stream_read_cb(void* ctx, uint8_t* dst, size_t size) {
    return static_cast<SkStream*>(ctx)->read(dst, size);
}

size_t sk_stream_skip_cb(void* ctx, size_t count) {
    return static_cast<SkStream*>(ctx)->skip(count);
}

}  // namespace

extern "C" {

// SkBmpCodec
bool skia_rs_bmp_is_bmp(const uint8_t* buf, size_t len);
int32_t skia_rs_bmp_read_header(void* ctx,
                                RustReadStreamFn read_fn,
                                RustSkipStreamFn skip_fn,
                                bool in_ico,
                                bool validate_masks,
                                RustBmpHeaderInfo* out_info);
size_t skia_rs_bmp_src_row_bytes(int32_t width, uint16_t bits_per_pixel);
size_t skia_rs_bmp_and_mask_row_bytes(int32_t width);
int32_t skia_rs_bmp_get_dst_row(int32_t row_order, int32_t y, int32_t height);

// SkBmpStandardCodec
int32_t skia_rs_bmp_standard_validate_get_pixels(bool has_subset,
                                                 SkISize dst_dims,
                                                 SkISize src_dims);
bool skia_rs_bmp_standard_build_color_table(uint16_t bits_per_pixel,
                                            uint32_t num_colors,
                                            uint32_t bytes_per_color,
                                            uint32_t offset,
                                            bool is_opaque,
                                            bool in_ico,
                                            int32_t dst_color_type,
                                            int32_t dst_alpha_type,
                                            bool has_color_xform,
                                            bool pmcolor_is_bgra,
                                            void* ctx,
                                            RustReadStreamFn read_fn,
                                            RustSkipStreamFn skip_fn,
                                            uint32_t* out_colors,
                                            uint32_t* out_max_colors);
bool skia_rs_bmp_standard_swizzler_info(bool in_ico,
                                        uint16_t bits_per_pixel,
                                        int32_t encoded_color,
                                        int32_t encoded_alpha,
                                        uint8_t encoded_bpc,
                                        RustStandardSwizzlerEncodedInfo* out_info);
bool skia_rs_bmp_standard_ico_and_mask_substream(int32_t total_height,
                                                 int32_t start_scanline,
                                                 int32_t count,
                                                 size_t src_row_bytes,
                                                 size_t and_mask_row_bytes,
                                                 size_t curr_position,
                                                 size_t stream_length,
                                                 size_t* out_start,
                                                 size_t* out_length);
void skia_rs_bmp_standard_decode_ico_mask(void* ctx,
                                          RustReadStreamFn read_fn,
                                          RustSkipStreamFn skip_fn,
                                          uint8_t* mask_buf,
                                          size_t and_mask_row_bytes,
                                          uint8_t* dst,
                                          size_t dst_row_bytes,
                                          int32_t dst_height,
                                          int32_t dst_color_type,
                                          int32_t row_order,
                                          int32_t sample_x,
                                          int32_t swizzle_width,
                                          int32_t src_offset);

// SkBmpMaskCodec
int32_t skia_rs_bmp_mask_validate_get_pixels(bool has_subset,
                                             SkISize dst_dims,
                                             SkISize src_dims);
bool skia_rs_bmp_mask_prepare_swizzler_info(int32_t dst_color_type,
                                            int32_t dst_alpha_type,
                                            bool has_color_xform,
                                            RustMaskSwizzlerTargetInfo* out_info);

// SkBmpRLECodec
bool skia_rs_bmp_rle_build_color_table(uint16_t bits_per_pixel,
                                       uint32_t num_colors,
                                       uint32_t bytes_per_color,
                                       uint32_t offset,
                                       int32_t dst_color_type,
                                       bool has_color_xform,
                                       bool pmcolor_is_bgra,
                                       void* ctx,
                                       RustReadStreamFn read_fn,
                                       RustSkipStreamFn skip_fn,
                                       uint32_t* out_colors,
                                       uint32_t* out_max_colors);
bool skia_rs_bmp_rle_init_stream_buffer(void* ctx,
                                        RustReadStreamFn read_fn,
                                        RustSkipStreamFn skip_fn,
                                        uint8_t* stream_buf,
                                        size_t* bytes_buffered,
                                        uint32_t* curr_rle_byte);
size_t skia_rs_bmp_rle_check_for_more_data(void* ctx,
                                           RustReadStreamFn read_fn,
                                           RustSkipStreamFn skip_fn,
                                           uint8_t* stream_buf,
                                           size_t* bytes_buffered,
                                           uint32_t* curr_rle_byte);
RustRleSkipPlan skia_rs_bmp_rle_consume_lines_to_skip(int32_t height, int32_t* lines_to_skip);
bool skia_rs_bmp_rle_compute_f16_xform_layout(int32_t dst_width,
                                              int32_t height,
                                              int32_t* out_count,
                                              size_t* out_byte_size,
                                              size_t* out_row_bytes);
void skia_rs_bmp_rle_set_pixel(uint8_t* dst,
                               size_t dst_row_bytes,
                               int32_t dst_width,
                               int32_t dst_height,
                               int32_t dst_color_type,
                               int32_t row_order,
                               int32_t sample_x,
                               const uint32_t* color_table,
                               size_t color_table_len,
                               bool pmcolor_is_bgra,
                               int32_t x,
                               int32_t y,
                               uint8_t index);
void skia_rs_bmp_rle_set_rgb_pixel(uint8_t* dst,
                                   size_t dst_row_bytes,
                                   int32_t dst_width,
                                   int32_t dst_height,
                                   int32_t dst_color_type,
                                   int32_t row_order,
                                   int32_t sample_x,
                                   int32_t x,
                                   int32_t y,
                                   uint8_t red,
                                   uint8_t green,
                                   uint8_t blue);
int32_t skia_rs_bmp_rle_decode(void* ctx,
                               RustReadStreamFn read_fn,
                               RustSkipStreamFn skip_fn,
                               uint8_t* stream_buf,
                               size_t* bytes_buffered,
                               uint32_t* curr_rle_byte,
                               int32_t* lines_to_skip,
                               int32_t orig_width,
                               uint16_t bits_per_pixel,
                               int32_t row_order,
                               int32_t sample_x,
                               const uint32_t* color_table,
                               size_t color_table_len,
                               bool pmcolor_is_bgra,
                               uint8_t* dst,
                               int32_t dst_width,
                               int32_t dst_height,
                               size_t dst_row_bytes,
                               int32_t dst_color_type);
int32_t skia_rs_bmp_rle_fill_width(int32_t orig_width, int32_t sample_x);

// SkWbmpCodec
bool skia_rs_wbmp_is_wbmp(const uint8_t* buf, size_t len);
bool skia_rs_wbmp_read_header(void* ctx,
                              RustReadStreamFn read_fn,
                              RustSkipStreamFn skip_fn,
                              SkISize* out_size);
size_t skia_rs_wbmp_src_row_bytes(int32_t width);
bool skia_rs_wbmp_conversion_supported(int32_t dst_color_type,
                                       int32_t dst_alpha_type,
                                       bool has_color_space,
                                       bool src_is_opaque);

}  // extern "C"

// ============================================================================
// SkBmpBaseCodec
// ============================================================================

SkBmpBaseCodec::~SkBmpBaseCodec() {}

SkBmpBaseCodec::SkBmpBaseCodec(SkEncodedInfo&& info,
                               std::unique_ptr<SkStream> stream,
                               uint16_t bitsPerPixel,
                               SkCodec::SkScanlineOrder rowOrder)
        : INHERITED(std::move(info), std::move(stream), bitsPerPixel, rowOrder)
        , fSrcBuffer(sk_malloc_canfail(this->srcRowBytes())) {}

// ============================================================================
// SkBmpCodec
// ============================================================================

bool SkBmpCodec::IsBmp(const void* buffer, size_t bytesRead) {
    return skia_rs_bmp_is_bmp(static_cast<const uint8_t*>(buffer), bytesRead);
}

std::unique_ptr<SkCodec> SkBmpCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                    Result* result) {
    return SkBmpCodec::MakeFromStream(std::move(stream), result, false);
}

std::unique_ptr<SkCodec> SkBmpCodec::MakeFromIco(std::unique_ptr<SkStream> stream, Result* result) {
    return SkBmpCodec::MakeFromStream(std::move(stream), result, true);
}

SkCodec::Result SkBmpCodec::ReadHeader(SkStream* stream,
                                       bool inIco,
                                       std::unique_ptr<SkCodec>* codecOut) {
    RustBmpHeaderInfo hdr{};
    const auto result = static_cast<SkCodec::Result>(skia_rs_bmp_read_header(
            stream, sk_stream_read_cb, sk_stream_skip_cb, inIco, codecOut != nullptr, &hdr));
    if (result != kSuccess || !codecOut) {
        return result;
    }

    auto info = SkEncodedInfo::Make(hdr.width,
                                    hdr.height,
                                    static_cast<SkEncodedInfo::Color>(hdr.encoded_color),
                                    static_cast<SkEncodedInfo::Alpha>(hdr.encoded_alpha),
                                    hdr.bits_per_component);
    const auto rowOrder = static_cast<SkScanlineOrder>(hdr.row_order);

    switch (hdr.input_format) {
        case kBmpInputFormatStandard: {
            SkASSERT(!inIco || nullptr != stream->getMemoryBase());
            *codecOut = std::make_unique<SkBmpStandardCodec>(std::move(info),
                                                             std::unique_ptr<SkStream>(stream),
                                                             hdr.bits_per_pixel,
                                                             hdr.num_colors,
                                                             hdr.bytes_per_color,
                                                             hdr.pixel_data_offset,
                                                             rowOrder,
                                                             hdr.is_opaque,
                                                             inIco);
            return static_cast<SkBmpStandardCodec*>(codecOut->get())->didCreateSrcBuffer()
                           ? kSuccess
                           : kInvalidInput;
        }
        case kBmpInputFormatBitMask: {
            SkASSERT(hdr.bits_per_pixel % 8 == 0);
            std::unique_ptr<SkMasks> masks(
                    SkMasks::CreateMasks(hdr.input_masks, hdr.bits_per_pixel / 8));
            if (!masks) {
                return kInvalidInput;
            }
            *codecOut = std::make_unique<SkBmpMaskCodec>(std::move(info),
                                                         std::unique_ptr<SkStream>(stream),
                                                         hdr.bits_per_pixel,
                                                         masks.release(),
                                                         rowOrder);
            return static_cast<SkBmpMaskCodec*>(codecOut->get())->didCreateSrcBuffer()
                           ? kSuccess
                           : kInvalidInput;
        }
        case kBmpInputFormatRle: {
            *codecOut = std::make_unique<SkBmpRLECodec>(std::move(info),
                                                        std::unique_ptr<SkStream>(stream),
                                                        hdr.bits_per_pixel,
                                                        hdr.num_colors,
                                                        hdr.bytes_per_color,
                                                        hdr.pixel_data_offset,
                                                        rowOrder);
            return kSuccess;
        }
        default:
            SkASSERT(false);
            return kInvalidInput;
    }
}

std::unique_ptr<SkCodec> SkBmpCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                    Result* result,
                                                    bool inIco) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }
    std::unique_ptr<SkCodec> codec;
    *result = ReadHeader(stream.get(), inIco, &codec);
    if (codec) {
        stream.release();
    }
    return kSuccess == *result ? std::move(codec) : nullptr;
}

SkBmpCodec::SkBmpCodec(SkEncodedInfo&& info,
                       std::unique_ptr<SkStream> stream,
                       uint16_t bitsPerPixel,
                       SkCodec::SkScanlineOrder rowOrder)
        : INHERITED(std::move(info), kXformSrcColorFormat, std::move(stream))
        , fBitsPerPixel(bitsPerPixel)
        , fRowOrder(rowOrder)
        , fSrcRowBytes(skia_rs_bmp_src_row_bytes(this->dimensions().width(), fBitsPerPixel))
        , fXformBuffer(nullptr) {}

bool SkBmpCodec::onRewind() {
    if (!this->rewindStream()) {
        return false;
    }
    return SkBmpCodec::ReadHeader(this->stream(), this->inIco(), nullptr) == kSuccess;
}

int32_t SkBmpCodec::getDstRow(int32_t y, int32_t height) const {
    return skia_rs_bmp_get_dst_row(static_cast<int32_t>(fRowOrder), y, height);
}

SkCodec::Result SkBmpCodec::prepareToDecode(const SkImageInfo& dstInfo,
                                            const SkCodec::Options& options) {
    return this->onPrepareToDecode(dstInfo, options);
}

SkCodec::Result SkBmpCodec::onStartScanlineDecode(const SkImageInfo& dstInfo,
                                                  const SkCodec::Options& options) {
    return prepareToDecode(dstInfo, options);
}

int SkBmpCodec::onGetScanlines(void* dst, int count, size_t rowBytes) {
    SkImageInfo rowInfo = this->dstInfo().makeWH(this->dstInfo().width(), count);
    return this->decodeRows(rowInfo, dst, rowBytes, this->options());
}

bool SkBmpCodec::skipRows(int count) {
    const size_t bytesToSkip = count * fSrcRowBytes;
    return this->stream()->skip(bytesToSkip) == bytesToSkip;
}

bool SkBmpCodec::onSkipScanlines(int count) {
    return this->skipRows(count);
}

namespace SkBmpDecoder {
bool IsBmp(const void* data, size_t len) {
    return SkBmpCodec::IsBmp(data, len);
}

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    return SkBmpCodec::MakeFromStream(std::move(stream), outResult);
}

std::unique_ptr<SkCodec> Decode(sk_sp<const SkData> data,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    if (!data) {
        if (outResult) {
            *outResult = SkCodec::kInvalidInput;
        }
        return nullptr;
    }
    return Decode(SkMemoryStream::Make(std::move(data)), outResult, nullptr);
}
}  // namespace SkBmpDecoder

// ============================================================================
// SkBmpMaskCodec
// ============================================================================

SkBmpMaskCodec::SkBmpMaskCodec(SkEncodedInfo&& info,
                               std::unique_ptr<SkStream> stream,
                               uint16_t bitsPerPixel,
                               SkMasks* masks,
                               SkCodec::SkScanlineOrder rowOrder)
        : INHERITED(std::move(info), std::move(stream), bitsPerPixel, rowOrder)
        , fMasks(masks)
        , fMaskSwizzler(nullptr) {}

SkCodec::Result SkBmpMaskCodec::onGetPixels(const SkImageInfo& dstInfo,
                                            void* dst,
                                            size_t dstRowBytes,
                                            const Options& opts,
                                            int* rowsDecoded) {
    const auto check = static_cast<Result>(skia_rs_bmp_mask_validate_get_pixels(
            opts.fSubset != nullptr, dstInfo.dimensions(), this->dimensions()));
    if (kSuccess != check) {
        return check;
    }

    Result result = this->prepareToDecode(dstInfo, opts);
    if (kSuccess != result) {
        return result;
    }

    int rows = this->decodeRows(dstInfo, dst, dstRowBytes, opts);
    if (rows != dstInfo.height()) {
        *rowsDecoded = rows;
        return kIncompleteInput;
    }
    return kSuccess;
}

SkCodec::Result SkBmpMaskCodec::onPrepareToDecode(const SkImageInfo& dstInfo,
                                                  const SkCodec::Options& options) {
    if (this->colorXform()) {
        this->resetXformBuffer(dstInfo.width());
    }

    RustMaskSwizzlerTargetInfo target{};
    skia_rs_bmp_mask_prepare_swizzler_info(static_cast<int32_t>(dstInfo.colorType()),
                                           static_cast<int32_t>(dstInfo.alphaType()),
                                           this->colorXform(),
                                           &target);
    SkImageInfo swizzlerInfo =
            dstInfo.makeColorType(static_cast<SkColorType>(target.color_type))
                    .makeAlphaType(static_cast<SkAlphaType>(target.alpha_type));

    bool srcIsOpaque = this->getEncodedInfo().opaque();
    fMaskSwizzler.reset(SkMaskSwizzler::CreateMaskSwizzler(
            swizzlerInfo, srcIsOpaque, fMasks.get(), this->bitsPerPixel(), options));
    SkASSERT(fMaskSwizzler);

    return SkCodec::kSuccess;
}

int SkBmpMaskCodec::decodeRows(const SkImageInfo& dstInfo,
                               void* dst,
                               size_t dstRowBytes,
                               const Options&) {
    uint8_t* srcRow = this->srcBuffer();
    const int height = dstInfo.height();
    for (int y = 0; y < height; y++) {
        if (this->stream()->read(srcRow, this->srcRowBytes()) != this->srcRowBytes()) {
            return y;
        }
        uint32_t row = this->getDstRow(y, height);
        void* dstRow = SkTAddOffset<void>(dst, row * dstRowBytes);
        if (this->colorXform()) {
            fMaskSwizzler->swizzle(this->xformBuffer(), srcRow);
            this->applyColorXform(dstRow, this->xformBuffer(), fMaskSwizzler->swizzleWidth());
        } else {
            fMaskSwizzler->swizzle(dstRow, srcRow);
        }
    }
    return height;
}

// ============================================================================
// SkBmpStandardCodec
// ============================================================================

SkBmpStandardCodec::SkBmpStandardCodec(SkEncodedInfo&& info,
                                       std::unique_ptr<SkStream> stream,
                                       uint16_t bitsPerPixel,
                                       uint32_t numColors,
                                       uint32_t bytesPerColor,
                                       uint32_t offset,
                                       SkCodec::SkScanlineOrder rowOrder,
                                       bool isOpaque,
                                       bool inIco)
        : INHERITED(std::move(info), std::move(stream), bitsPerPixel, rowOrder)
        , fColorTable(nullptr)
        , fNumColors(numColors)
        , fBytesPerColor(bytesPerColor)
        , fOffset(offset)
        , fSwizzler(nullptr)
        , fIsOpaque(isOpaque)
        , fInIco(inIco)
        , fAndMaskRowBytes(
                  fInIco ? skia_rs_bmp_and_mask_row_bytes(this->dimensions().width()) : 0) {}

SkCodec::Result SkBmpStandardCodec::onGetPixels(const SkImageInfo& dstInfo,
                                                void* dst,
                                                size_t dstRowBytes,
                                                const Options& opts,
                                                int* rowsDecoded) {
    const auto check = static_cast<Result>(skia_rs_bmp_standard_validate_get_pixels(
            opts.fSubset != nullptr, dstInfo.dimensions(), this->dimensions()));
    if (kSuccess != check) {
        return check;
    }

    Result result = this->prepareToDecode(dstInfo, opts);
    if (kSuccess != result) {
        return result;
    }
    int rows = this->decodeRows(dstInfo, dst, dstRowBytes, opts);
    if (rows != dstInfo.height()) {
        *rowsDecoded = rows;
        return kIncompleteInput;
    }
    return kSuccess;
}

bool SkBmpStandardCodec::createColorTable(SkColorType dstColorType, SkAlphaType dstAlphaType) {
    SkPMColor colorTable[256];
    uint32_t maxColors = 0;
    if (!skia_rs_bmp_standard_build_color_table(this->bitsPerPixel(),
                                                fNumColors,
                                                fBytesPerColor,
                                                fOffset,
                                                fIsOpaque,
                                                fInIco,
                                                static_cast<int32_t>(dstColorType),
                                                static_cast<int32_t>(dstAlphaType),
                                                this->colorXform(),
                                                kPmColorIsBgra,
                                                this->stream(),
                                                sk_stream_read_cb,
                                                sk_stream_skip_cb,
                                                colorTable,
                                                &maxColors)) {
        return false;
    }

    if (maxColors > 0) {
        if (this->colorXform() && !this->xformOnDecode()) {
            this->applyColorXform(colorTable, colorTable, maxColors);
        }
        fColorTable.reset(new SkColorPalette(colorTable, maxColors));
    }
    return true;
}

SkEncodedInfo SkBmpStandardCodec::swizzlerInfo() const {
    const auto& info = this->getEncodedInfo();
    RustStandardSwizzlerEncodedInfo out{};
    skia_rs_bmp_standard_swizzler_info(fInIco,
                                       this->bitsPerPixel(),
                                       static_cast<int32_t>(info.color()),
                                       static_cast<int32_t>(info.alpha()),
                                       info.bitsPerComponent(),
                                       &out);
    return SkEncodedInfo::Make(0,
                               0,
                               static_cast<SkEncodedInfo::Color>(out.color),
                               static_cast<SkEncodedInfo::Alpha>(out.alpha),
                               out.bits_per_component);
}

void SkBmpStandardCodec::initializeSwizzler(const SkImageInfo& dstInfo, const Options& opts) {
    SkEncodedInfo encodedInfo = this->swizzlerInfo();
    const SkPMColor* colorPtr = SkCodecPriv::GetColorPtr(fColorTable.get());

    SkImageInfo swizzlerInfo = dstInfo;
    SkCodec::Options swizzlerOptions = opts;
    if (this->xformOnDecode()) {
        swizzlerInfo = swizzlerInfo.makeColorType(kXformSrcColorType);
        if (kPremul_SkAlphaType == dstInfo.alphaType()) {
            swizzlerInfo = swizzlerInfo.makeAlphaType(kUnpremul_SkAlphaType);
        }
        swizzlerOptions.fZeroInitialized = kNo_ZeroInitialized;
    }

    fSwizzler = SkSwizzler::Make(encodedInfo, colorPtr, swizzlerInfo, swizzlerOptions);
    SkASSERT(fSwizzler);
}

SkCodec::Result SkBmpStandardCodec::onPrepareToDecode(const SkImageInfo& dstInfo,
                                                      const SkCodec::Options& options) {
    if (this->xformOnDecode()) {
        this->resetXformBuffer(dstInfo.width());
    }
    if (!this->createColorTable(dstInfo.colorType(), dstInfo.alphaType())) {
        return SkCodec::kInvalidInput;
    }
    this->initializeSwizzler(dstInfo, options);
    return SkCodec::kSuccess;
}

int SkBmpStandardCodec::decodeRows(const SkImageInfo& dstInfo,
                                   void* dst,
                                   size_t dstRowBytes,
                                   const Options&) {
    const int height = dstInfo.height();
    for (int y = 0; y < height; y++) {
        if (this->stream()->read(this->srcBuffer(), this->srcRowBytes()) != this->srcRowBytes()) {
            return y;
        }
        uint32_t row = this->getDstRow(y, dstInfo.height());
        void* dstRow = SkTAddOffset<void>(dst, row * dstRowBytes);
        if (this->xformOnDecode()) {
            SkASSERT(this->colorXform());
            fSwizzler->swizzle(this->xformBuffer(), this->srcBuffer());
            this->applyColorXform(dstRow, this->xformBuffer(), fSwizzler->swizzleWidth());
        } else {
            fSwizzler->swizzle(dstRow, this->srcBuffer());
        }
    }

    if (fInIco && fIsOpaque) {
        const int startScanline = this->currScanline();
        if (startScanline < 0) {
            decodeIcoMask(this->stream(), dstInfo, dst, dstRowBytes);
            return height;
        }

        const void* memoryBase = this->stream()->getMemoryBase();
        SkASSERT(nullptr != memoryBase);
        SkASSERT(this->stream()->hasLength());
        SkASSERT(this->stream()->hasPosition());

        size_t subStreamStart = 0;
        size_t subStreamLength = 0;
        if (!skia_rs_bmp_standard_ico_and_mask_substream(this->dimensions().height(),
                                                         startScanline,
                                                         height,
                                                         this->srcRowBytes(),
                                                         fAndMaskRowBytes,
                                                         this->stream()->getPosition(),
                                                         this->stream()->getLength(),
                                                         &subStreamStart,
                                                         &subStreamLength)) {
            return height;
        }

        const void* subStreamMemoryBase = SkTAddOffset<const void>(memoryBase, subStreamStart);
        SkMemoryStream subStream(subStreamMemoryBase, subStreamLength, false);
        decodeIcoMask(&subStream, dstInfo, dst, dstRowBytes);
    }

    return height;
}

void SkBmpStandardCodec::decodeIcoMask(SkStream* stream,
                                       const SkImageInfo& dstInfo,
                                       void* dst,
                                       size_t dstRowBytes) {
    SkASSERT(kRGBA_8888_SkColorType == dstInfo.colorType() ||
             kBGRA_8888_SkColorType == dstInfo.colorType() ||
             kRGBA_F16_SkColorType == dstInfo.colorType());

    skia_rs_bmp_standard_decode_ico_mask(stream,
                                         sk_stream_read_cb,
                                         sk_stream_skip_cb,
                                         this->srcBuffer(),
                                         fAndMaskRowBytes,
                                         static_cast<uint8_t*>(dst),
                                         dstRowBytes,
                                         dstInfo.height(),
                                         static_cast<int32_t>(dstInfo.colorType()),
                                         static_cast<int32_t>(this->getScanlineOrder()),
                                         fSwizzler->sampleX(),
                                         fSwizzler->swizzleWidth(),
                                         fSwizzler->srcOffset());
}

// ============================================================================
// SkBmpRLECodec
// ============================================================================

SkBmpRLECodec::SkBmpRLECodec(SkEncodedInfo&& info,
                             std::unique_ptr<SkStream> stream,
                             uint16_t bitsPerPixel,
                             uint32_t numColors,
                             uint32_t bytesPerColor,
                             uint32_t offset,
                             SkCodec::SkScanlineOrder rowOrder)
        : INHERITED(std::move(info), std::move(stream), bitsPerPixel, rowOrder)
        , fColorTable(nullptr)
        , fNumColors(numColors)
        , fBytesPerColor(bytesPerColor)
        , fOffset(offset)
        , fBytesBuffered(0)
        , fCurrRLEByte(0)
        , fSampleX(1)
        , fLinesToSkip(0) {
    static_assert(kBufferSize == 4096);
}

SkCodec::Result SkBmpRLECodec::onGetPixels(const SkImageInfo& dstInfo,
                                           void* dst,
                                           size_t dstRowBytes,
                                           const Options& opts,
                                           int* rowsDecoded) {
    if (opts.fSubset) {
        return kUnimplemented;
    }

    Result result = this->prepareToDecode(dstInfo, opts);
    if (kSuccess != result) {
        return result;
    }

    int rows = this->decodeRows(dstInfo, dst, dstRowBytes, opts);
    if (rows != dstInfo.height()) {
        *rowsDecoded = dstInfo.height();
        return kIncompleteInput;
    }
    return kSuccess;
}

bool SkBmpRLECodec::createColorTable(SkColorType dstColorType) {
    SkPMColor colorTable[256];
    uint32_t maxColors = 0;
    if (!skia_rs_bmp_rle_build_color_table(this->bitsPerPixel(),
                                           fNumColors,
                                           fBytesPerColor,
                                           fOffset,
                                           static_cast<int32_t>(dstColorType),
                                           false,
                                           kPmColorIsBgra,
                                           this->stream(),
                                           sk_stream_read_cb,
                                           sk_stream_skip_cb,
                                           colorTable,
                                           &maxColors)) {
        return false;
    }
    if (maxColors > 0) {
        fColorTable.reset(new SkColorPalette(colorTable, maxColors));
    }
    return true;
}

bool SkBmpRLECodec::initializeStreamBuffer() {
    return skia_rs_bmp_rle_init_stream_buffer(this->stream(),
                                              sk_stream_read_cb,
                                              sk_stream_skip_cb,
                                              fStreamBuffer,
                                              &fBytesBuffered,
                                              &fCurrRLEByte);
}

size_t SkBmpRLECodec::checkForMoreData() {
    return skia_rs_bmp_rle_check_for_more_data(this->stream(),
                                               sk_stream_read_cb,
                                               sk_stream_skip_cb,
                                               fStreamBuffer,
                                               &fBytesBuffered,
                                               &fCurrRLEByte);
}

void SkBmpRLECodec::setPixel(void* dst,
                             size_t dstRowBytes,
                             const SkImageInfo& dstInfo,
                             uint32_t x,
                             uint32_t y,
                             uint8_t index) {
    const uint32_t* colors = fColorTable ? fColorTable->readColors() : nullptr;
    const size_t colorCount = fColorTable ? static_cast<size_t>(fColorTable->count()) : 0;
    skia_rs_bmp_rle_set_pixel(static_cast<uint8_t*>(dst),
                              dstRowBytes,
                              dstInfo.width(),
                              dstInfo.height(),
                              static_cast<int32_t>(dstInfo.colorType()),
                              static_cast<int32_t>(this->getScanlineOrder()),
                              fSampleX,
                              colors,
                              colorCount,
                              kPmColorIsBgra,
                              static_cast<int32_t>(x),
                              static_cast<int32_t>(y),
                              index);
}

void SkBmpRLECodec::setRGBPixel(void* dst,
                                size_t dstRowBytes,
                                const SkImageInfo& dstInfo,
                                uint32_t x,
                                uint32_t y,
                                uint8_t red,
                                uint8_t green,
                                uint8_t blue) {
    skia_rs_bmp_rle_set_rgb_pixel(static_cast<uint8_t*>(dst),
                                  dstRowBytes,
                                  dstInfo.width(),
                                  dstInfo.height(),
                                  static_cast<int32_t>(dstInfo.colorType()),
                                  static_cast<int32_t>(this->getScanlineOrder()),
                                  fSampleX,
                                  static_cast<int32_t>(x),
                                  static_cast<int32_t>(y),
                                  red,
                                  green,
                                  blue);
}

SkCodec::Result SkBmpRLECodec::onPrepareToDecode(const SkImageInfo& dstInfo,
                                                 const SkCodec::Options& options) {
    if (options.fSubset) {
        return kUnimplemented;
    }

    fSampleX = 1;
    fLinesToSkip = 0;

    SkColorType colorTableColorType =
            this->colorXform() ? kBGRA_8888_SkColorType : dstInfo.colorType();

    if (!this->createColorTable(colorTableColorType)) {
        return SkCodec::kInvalidInput;
    }

    if (!this->initializeStreamBuffer()) {
        return SkCodec::kInvalidInput;
    }

    return SkCodec::kSuccess;
}

int SkBmpRLECodec::decodeRows(const SkImageInfo& info,
                              void* dst,
                              size_t dstRowBytes,
                              const Options& opts) {
    int height = info.height();
    SkImageInfo dstInfo = info.makeWH(this->fillWidth(), height);

    if (dst) {
        SkSampler::Fill(dstInfo, dst, dstRowBytes, opts.fZeroInitialized);
    }

    const RustRleSkipPlan plan = skia_rs_bmp_rle_consume_lines_to_skip(height, &fLinesToSkip);
    if (plan.fully_skipped) {
        return plan.remaining_height;
    }
    if (plan.rows_advanced > 0 && dst) {
        dst = SkTAddOffset<void>(dst, static_cast<size_t>(plan.rows_advanced) * dstRowBytes);
    }
    height = plan.remaining_height;
    dstInfo = dstInfo.makeWH(dstInfo.width(), height);

    void* decodeDst = dst;
    size_t decodeRowBytes = dstRowBytes;
    SkImageInfo decodeInfo = dstInfo;
    if (decodeDst && this->colorXform()) {
        decodeInfo = decodeInfo.makeColorType(kXformSrcColorType);
        if (kRGBA_F16_SkColorType == dstInfo.colorType()) {
            int32_t count = 0;
            size_t xformBufferSize = 0;
            size_t rowBytes = 0;
            if (!skia_rs_bmp_rle_compute_f16_xform_layout(
                        dstInfo.width(), height, &count, &xformBufferSize, &rowBytes)) {
                return 0;
            }
            this->resetXformBuffer(count);
            sk_bzero(this->xformBuffer(), xformBufferSize);
            decodeDst = this->xformBuffer();
            decodeRowBytes = rowBytes;
        }
    }

    int decodedHeight = this->decodeRLE(decodeInfo, decodeDst, decodeRowBytes);
    if (this->colorXform() && decodeDst) {
        for (int y = 0; y < decodedHeight; y++) {
            this->applyColorXform(dst, decodeDst, dstInfo.width());
            decodeDst = SkTAddOffset<void>(decodeDst, decodeRowBytes);
            dst = SkTAddOffset<void>(dst, dstRowBytes);
        }
    }

    return decodedHeight;
}

int SkBmpRLECodec::decodeRLE(const SkImageInfo& dstInfo, void* dst, size_t dstRowBytes) {
    const uint32_t* colors = fColorTable ? fColorTable->readColors() : nullptr;
    const size_t colorCount = fColorTable ? static_cast<size_t>(fColorTable->count()) : 0;
    return skia_rs_bmp_rle_decode(this->stream(),
                                  sk_stream_read_cb,
                                  sk_stream_skip_cb,
                                  fStreamBuffer,
                                  &fBytesBuffered,
                                  &fCurrRLEByte,
                                  &fLinesToSkip,
                                  this->dimensions().width(),
                                  this->bitsPerPixel(),
                                  static_cast<int32_t>(this->getScanlineOrder()),
                                  fSampleX,
                                  colors,
                                  colorCount,
                                  kPmColorIsBgra,
                                  static_cast<uint8_t*>(dst),
                                  dstInfo.width(),
                                  dstInfo.height(),
                                  dstRowBytes,
                                  static_cast<int32_t>(dstInfo.colorType()));
}

bool SkBmpRLECodec::skipRows(int count) {
    const SkImageInfo rowInfo = SkImageInfo::Make(
            this->dimensions().width(), count, kN32_SkColorType, kUnpremul_SkAlphaType);
    return count == this->decodeRows(rowInfo, nullptr, 0, this->options());
}

class SkBmpRLESampler : public SkSampler {
public:
    explicit SkBmpRLESampler(SkBmpRLECodec* codec) : fCodec(codec) {
        SkASSERT(fCodec);
    }

    int fillWidth() const override {
        return fCodec->fillWidth();
    }

private:
    int onSetSampleX(int sampleX) override {
        return fCodec->setSampleX(sampleX);
    }

    SkBmpRLECodec* fCodec;
};

SkSampler* SkBmpRLECodec::getSampler(bool createIfNecessary) {
    if (!fSampler && createIfNecessary) {
        fSampler = std::make_unique<SkBmpRLESampler>(this);
    }
    return fSampler.get();
}

int SkBmpRLECodec::setSampleX(int sampleX) {
    fSampleX = sampleX;
    return this->fillWidth();
}

int SkBmpRLECodec::fillWidth() const {
    return skia_rs_bmp_rle_fill_width(this->dimensions().width(), fSampleX);
}

// ============================================================================
// SkWbmpCodec
// ============================================================================

bool SkWbmpCodec::onRewind() {
    if (!this->rewindStream()) {
        return false;
    }
    return skia_rs_wbmp_read_header(this->stream(), sk_stream_read_cb, sk_stream_skip_cb, nullptr);
}

bool SkWbmpCodec::readRow(uint8_t* row) {
    return this->stream()->read(row, fSrcRowBytes) == fSrcRowBytes;
}

SkWbmpCodec::SkWbmpCodec(SkEncodedInfo&& info, std::unique_ptr<SkStream> stream)
        : INHERITED(std::move(info), skcms_PixelFormat(), std::move(stream))
        , fSrcRowBytes(skia_rs_wbmp_src_row_bytes(this->dimensions().width()))
        , fSwizzler(nullptr) {}

SkEncodedImageFormat SkWbmpCodec::onGetEncodedFormat() const {
    return SkEncodedImageFormat::kWBMP;
}

bool SkWbmpCodec::conversionSupported(const SkImageInfo& dst,
                                      bool srcIsOpaque,
                                      bool /*needsColorXform*/) {
    return skia_rs_wbmp_conversion_supported(static_cast<int32_t>(dst.colorType()),
                                             static_cast<int32_t>(dst.alphaType()),
                                             dst.colorSpace() != nullptr,
                                             srcIsOpaque);
}

SkCodec::Result SkWbmpCodec::onGetPixels(const SkImageInfo& info,
                                         void* dst,
                                         size_t rowBytes,
                                         const Options& options,
                                         int* rowsDecoded) {
    if (options.fSubset) {
        return kUnimplemented;
    }

    std::unique_ptr<SkSwizzler> swizzler =
            SkSwizzler::Make(this->getEncodedInfo(), nullptr, info, options);
    SkASSERT(swizzler);

    SkISize size = info.dimensions();
    skia_private::AutoTMalloc<uint8_t> src(fSrcRowBytes);
    void* dstRow = dst;
    for (int y = 0; y < size.height(); ++y) {
        if (!this->readRow(src.get())) {
            *rowsDecoded = y;
            return kIncompleteInput;
        }
        swizzler->swizzle(dstRow, src.get());
        dstRow = SkTAddOffset<void>(dstRow, rowBytes);
    }
    return kSuccess;
}

bool SkWbmpCodec::IsWbmp(const void* buffer, size_t bytesRead) {
    return skia_rs_wbmp_is_wbmp(static_cast<const uint8_t*>(buffer), bytesRead);
}

std::unique_ptr<SkCodec> SkWbmpCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                     Result* result) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }
    SkISize size{};
    if (!skia_rs_wbmp_read_header(stream.get(), sk_stream_read_cb, sk_stream_skip_cb, &size)) {
        *result = kCouldNotRewind;
        return nullptr;
    }
    *result = kSuccess;
    auto info = SkEncodedInfo::Make(
            size.width(), size.height(), SkEncodedInfo::kGray_Color, SkEncodedInfo::kOpaque_Alpha, 1);
    return std::unique_ptr<SkCodec>(new SkWbmpCodec(std::move(info), std::move(stream)));
}

int SkWbmpCodec::onGetScanlines(void* dst, int count, size_t dstRowBytes) {
    void* dstRow = dst;
    for (int y = 0; y < count; ++y) {
        if (!this->readRow(fSrcBuffer.get())) {
            return y;
        }
        fSwizzler->swizzle(dstRow, fSrcBuffer.get());
        dstRow = SkTAddOffset<void>(dstRow, dstRowBytes);
    }
    return count;
}

bool SkWbmpCodec::onSkipScanlines(int count) {
    const size_t bytesToSkip = count * fSrcRowBytes;
    return this->stream()->skip(bytesToSkip) == bytesToSkip;
}

SkCodec::Result SkWbmpCodec::onStartScanlineDecode(const SkImageInfo& dstInfo,
                                                   const Options& options) {
    if (options.fSubset) {
        return kUnimplemented;
    }

    fSwizzler = SkSwizzler::Make(this->getEncodedInfo(), nullptr, dstInfo, options);
    SkASSERT(fSwizzler);
    fSrcBuffer.reset(fSrcRowBytes);
    return kSuccess;
}

namespace SkWbmpDecoder {
bool IsWbmp(const void* data, size_t len) {
    return SkWbmpCodec::IsWbmp(data, len);
}

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    return SkWbmpCodec::MakeFromStream(std::move(stream), outResult);
}

std::unique_ptr<SkCodec> Decode(sk_sp<const SkData> data,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    if (!data) {
        if (outResult) {
            *outResult = SkCodec::kInvalidInput;
        }
        return nullptr;
    }
    return Decode(SkMemoryStream::Make(std::move(data)), outResult, nullptr);
}
}  // namespace SkWbmpDecoder
