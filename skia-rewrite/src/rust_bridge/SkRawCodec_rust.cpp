/*
 * Safe Rust bridge for:
 *   src/codec/SkRawCodec.cpp
 * (Phase 2 Module P2.8: skia-rs-codec-raw, GN optional target: raw).
 *
 * Keeps thin third_party/dng_sdk (`dng_host`, `dng_info`, `dng_negative`,
 * `dng_render`, `dng_stream`, `dng_image`, C++ `try/catch` around
 * `dng_exception`) and third_party/piex (`piex::IsRaw`,
 * `piex::GetPreviewImageData`) boundary calls while delegating TIFF header
 * validation, EXIF orientation mapping, PIEX preview-vs-DNG selection, RAW
 * metadata/scaling/dimension validation, `dng_render` sizing, `dng_pixel_buffer`
 * layout & `onGetPixels` planning, buffered/asset stream state machines, and
 * `SkDngHost` tile/area task partitioning to `#![forbid(unsafe_code)]` safe
 * Rust in `skia-rs-codec-raw`.
 */

#include "src/codec/SkRawCodec.h"

#include "include/codec/SkCodec.h"
#include "include/codec/SkEncodedImageFormat.h"
#include "include/codec/SkEncodedOrigin.h"
#include "include/codec/SkRawDecoder.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/core/SkTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkMutex.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkJpegCodec.h"
#include "src/core/SkTaskGroup.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <mutex>
#include <type_traits>
#include <utility>
#include <vector>

#include "dng_area_task.h"     // NO_G3_REWRITE
#include "dng_color_space.h"   // NO_G3_REWRITE
#include "dng_errors.h"        // NO_G3_REWRITE
#include "dng_exceptions.h"    // NO_G3_REWRITE
#include "dng_host.h"          // NO_G3_REWRITE
#include "dng_image.h"         // NO_G3_REWRITE
#include "dng_info.h"          // NO_G3_REWRITE
#include "dng_memory.h"        // NO_G3_REWRITE
#include "dng_mosaic_info.h"   // NO_G3_REWRITE
#include "dng_negative.h"      // NO_G3_REWRITE
#include "dng_pixel_buffer.h"  // NO_G3_REWRITE
#include "dng_point.h"         // NO_G3_REWRITE
#include "dng_rational.h"      // NO_G3_REWRITE
#include "dng_rect.h"          // NO_G3_REWRITE
#include "dng_render.h"        // NO_G3_REWRITE
#include "dng_sdk_limits.h"    // NO_G3_REWRITE
#include "dng_stream.h"        // NO_G3_REWRITE
#include "dng_tag_types.h"     // NO_G3_REWRITE
#include "dng_types.h"         // NO_G3_REWRITE
#include "dng_utils.h"         // NO_G3_REWRITE
#if qDNGUseXMP
#include "dng_xmp_sdk.h"       // NO_G3_REWRITE
#endif

#include "src/piex.h"          // NO_G3_REWRITE
#include "src/piex_types.h"    // NO_G3_REWRITE

#ifndef SK_DNG_VERSION
#define SK_DNG_VERSION 0x01040000
#endif

#if SK_DNG_VERSION <= 0x01040000
#define OPT_PROGRESS_ARG
#else
#define OPT_PROGRESS_ARG , dng_area_task_progress*
#endif

using namespace skia_private;

template <typename T> struct sk_is_trivially_relocatable;
template <> struct sk_is_trivially_relocatable<dng_exception> : std::true_type {};

namespace {

// ============================================================================
// Discriminant assertions shared across FFI with `skia-rs-codec-raw`.
// ============================================================================

static_assert(static_cast<int>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int>(kAlpha_8_SkColorType) == 1);
static_assert(static_cast<int>(kRGB_565_SkColorType) == 2);
static_assert(static_cast<int>(kARGB_4444_SkColorType) == 3);
static_assert(static_cast<int>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int>(kRGB_888x_SkColorType) == 5);
static_assert(static_cast<int>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int>(kRGBA_1010102_SkColorType) == 7);
static_assert(static_cast<int>(kBGR_101010x_XR_SkColorType) == 11);
static_assert(static_cast<int>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int>(kRGBA_F16_SkColorType) == 16);

static_assert(static_cast<int>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int>(kUnpremul_SkAlphaType) == 3);

static_assert(static_cast<int>(SkCodec::kSuccess) == 0);
static_assert(static_cast<int>(SkCodec::kIncompleteInput) == 1);
static_assert(static_cast<int>(SkCodec::kErrorInInput) == 2);
static_assert(static_cast<int>(SkCodec::kInvalidConversion) == 3);
static_assert(static_cast<int>(SkCodec::kInvalidScale) == 4);
static_assert(static_cast<int>(SkCodec::kInvalidParameters) == 5);
static_assert(static_cast<int>(SkCodec::kInvalidInput) == 6);
static_assert(static_cast<int>(SkCodec::kCouldNotRewind) == 7);
static_assert(static_cast<int>(SkCodec::kInternalError) == 8);
static_assert(static_cast<int>(SkCodec::kUnimplemented) == 9);
static_assert(static_cast<int>(SkCodec::kOutOfMemory) == 10);

static_assert(static_cast<int>(SkEncodedInfo::kGray_Color) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kRGB_Color) == 5);
static_assert(static_cast<int>(SkEncodedInfo::kRGBA_Color) == 6);
static_assert(static_cast<int>(SkEncodedInfo::kOpaque_Alpha) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kUnpremul_Alpha) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kBinary_Alpha) == 2);

static_assert(static_cast<int>(SkEncodedImageFormat::kDNG) == 10);

static_assert(static_cast<int>(kTopLeft_SkEncodedOrigin) == 1);
static_assert(static_cast<int>(kTopRight_SkEncodedOrigin) == 2);
static_assert(static_cast<int>(kBottomRight_SkEncodedOrigin) == 3);
static_assert(static_cast<int>(kBottomLeft_SkEncodedOrigin) == 4);
static_assert(static_cast<int>(kLeftTop_SkEncodedOrigin) == 5);
static_assert(static_cast<int>(kRightTop_SkEncodedOrigin) == 6);
static_assert(static_cast<int>(kRightBottom_SkEncodedOrigin) == 7);
static_assert(static_cast<int>(kLeftBottom_SkEncodedOrigin) == 8);
static_assert(static_cast<int>(kDefault_SkEncodedOrigin) == 1);
static_assert(static_cast<int>(kLast_SkEncodedOrigin) == 8);

static_assert(static_cast<int>(skcms_PixelFormat_G_8) == 2);
static_assert(static_cast<int>(skcms_PixelFormat_BGR_565) == 7);
static_assert(static_cast<int>(skcms_PixelFormat_RGB_888) == 10);
static_assert(static_cast<int>(skcms_PixelFormat_RGBA_8888) == 12);
static_assert(static_cast<int>(skcms_PixelFormat_BGRA_8888) == 13);
static_assert(static_cast<int>(skcms_PixelFormat_RGBA_1010102) == 16);
static_assert(static_cast<int>(skcms_PixelFormat_RGBA_hhhh) == 32);
static_assert(static_cast<int>(skcms_PixelFormat_BGR_101010x_XR) == 39);

static_assert(static_cast<int>(skcms_AlphaFormat_Opaque) == 0);
static_assert(static_cast<int>(skcms_AlphaFormat_Unpremul) == 1);
static_assert(static_cast<int>(skcms_AlphaFormat_PremulAsEncoded) == 2);

static_assert(static_cast<int>(::piex::Error::kOk) == 0);
static_assert(static_cast<int>(::piex::Error::kFail) == 1);
static_assert(static_cast<int>(::piex::Error::kUnsupported) == 2);

static_assert(static_cast<int>(::piex::Image::kJpegCompressed) == 0);
static_assert(static_cast<int>(::piex::Image::kUncompressedRgb) == 1);

static_assert(static_cast<int>(::piex::PreviewImageData::kSrgb) == 0);
static_assert(static_cast<int>(::piex::PreviewImageData::kAdobeRgb) == 1);

static_assert(static_cast<uint32_t>(ttByte) == 1);

// ============================================================================
// Rust FFI declarations (`skia-rs-ffi::codec_raw`).
// ============================================================================

struct SkiaRsDngPoint {
    int32_t v;
    int32_t h;
};

struct SkiaRsDngRect {
    int32_t t;
    int32_t l;
    int32_t b;
    int32_t r;
};

struct SkiaRsPiexPreviewPlan {
    int32_t decision;
    int32_t result;
    uint32_t preview_offset;
    uint32_t preview_length;
    bool use_adobe_rgb;
    int32_t origin;
};

struct SkiaRsRawImageMeta {
    int32_t width;
    int32_t height;
    bool is_scalable;
    bool is_xtrans_image;
    bool valid;
};

struct SkiaRsRawEncodedInfoSpec {
    int32_t width;
    int32_t height;
    int32_t encoded_color;
    int32_t encoded_alpha;
    uint8_t bits_per_component;
    int32_t codec_xform_format;
    int32_t encoded_format;
    bool uses_color_xform;
};

struct SkiaRsDngPixelBufferLayout {
    uint32_t plane;
    uint32_t planes;
    int32_t col_step;
    int32_t plane_step;
    uint32_t pixel_type;
    uint32_t pixel_size;
    int32_t row_step;
    size_t src_row_bytes;
};

struct SkiaRsRawGetPixelsPlan {
    int32_t result;
    int32_t src_format;
    int32_t dst_format;
    int32_t src_alpha_format;
    int32_t dst_alpha_format;
    int32_t row_count;
    int32_t pixel_count;
    SkiaRsDngPixelBufferLayout buffer_layout;
};

struct SkiaRsBufferMorePlan {
    int32_t kind;
    size_t size_requested;
    size_t size_to_read;
};

struct SkiaRsBufferedTransferPlan {
    int32_t kind;
    size_t skip_length;
    size_t copy_offset;
    size_t already_buffered;
    size_t remaining;
};

struct SkiaRsRawBufferedStream;
struct SkiaRsRawTaskAreas;

enum : int32_t {
    kPiexDecisionFail = 0,
    kPiexDecisionUseJpegPreview = 1,
    kPiexDecisionFallbackToDng = 2,
};

enum : int32_t {
    kBufferMoreAlreadySatisfied = 0,
    kBufferMoreFailed = 1,
    kBufferMoreReadToEnd = 2,
    kBufferMoreReadChunk = 3,
};

enum : int32_t {
    kBufferedTransferSkipAndRead = 0,
    kBufferedTransferCopyAndRead = 1,
};

extern "C" {
bool skia_rs_raw_is_tiff_header_valid(const uint8_t* header_ptr, size_t len);
bool skia_rs_raw_evaluate_piex_preview(int32_t error,
                                       int32_t color_space,
                                       uint32_t preview_offset,
                                       uint32_t preview_length,
                                       int32_t preview_format,
                                       uint32_t exif_orientation,
                                       SkiaRsPiexPreviewPlan* out_plan);
bool skia_rs_raw_init_image_meta(int32_t width,
                                 int32_t height,
                                 int32_t cfa_pattern_v,
                                 int32_t cfa_pattern_h,
                                 SkiaRsRawImageMeta* out_meta);
bool skia_rs_raw_encoded_info_spec(int32_t width,
                                   int32_t height,
                                   SkiaRsRawEncodedInfoSpec* out_spec);

int32_t skia_rs_raw_preferred_render_size(int32_t width, int32_t height);
int32_t skia_rs_raw_max_render_size(int32_t stage3_h, int32_t stage3_v);
bool skia_rs_raw_get_scaled_dimensions(int32_t width,
                                       int32_t height,
                                       float desired_scale,
                                       bool is_scalable,
                                       bool is_xtrans_image,
                                       int32_t* out_width,
                                       int32_t* out_height);
bool skia_rs_raw_dimensions_supported(int32_t full_width,
                                      int32_t full_height,
                                      int32_t target_width,
                                      int32_t target_height,
                                      bool is_scalable,
                                      bool is_xtrans_image);
bool skia_rs_raw_plan_get_pixels(int32_t rendered_h,
                                 int32_t rendered_v,
                                 int32_t dst_width,
                                 int32_t dst_height,
                                 int32_t dst_color_type,
                                 bool pmcolor_is_rgba,
                                 SkiaRsRawGetPixelsPlan* out_plan);
bool skia_rs_raw_row_area(int32_t row_index, int32_t width, SkiaRsDngRect* out_rect);

bool skia_rs_raw_is_asset_stream(bool has_length, bool has_position);
bool skia_rs_raw_safe_add_usize(size_t arg1, size_t arg2, size_t* out_sum);
bool skia_rs_raw_validate_dng_read(uint32_t count,
                                   uint64_t offset,
                                   size_t* out_offset,
                                   size_t* out_count);
bool skia_rs_raw_validate_asset_read(size_t offset, size_t length);
bool skia_rs_raw_plan_asset_transfer(size_t stream_length,
                                     size_t offset,
                                     size_t size,
                                     size_t* out_bytes_to_read);

SkiaRsRawBufferedStream* skia_rs_raw_buffered_stream_new();
void skia_rs_raw_buffered_stream_free(SkiaRsRawBufferedStream* state);
size_t skia_rs_raw_buffered_stream_bytes_written(const SkiaRsRawBufferedStream* state);
bool skia_rs_raw_buffered_stream_write(SkiaRsRawBufferedStream* state,
                                       const uint8_t* data,
                                       size_t len);
void skia_rs_raw_buffered_stream_mark_eof(SkiaRsRawBufferedStream* state);
bool skia_rs_raw_buffered_stream_read_buffered(const SkiaRsRawBufferedStream* state,
                                               size_t offset,
                                               uint8_t* dst,
                                               size_t len);
bool skia_rs_raw_buffered_stream_plan_buffer_more(const SkiaRsRawBufferedStream* state,
                                                  size_t new_size,
                                                  SkiaRsBufferMorePlan* out_plan);
bool skia_rs_raw_buffered_stream_commit_chunk(SkiaRsRawBufferedStream* state,
                                              const uint8_t* data,
                                              size_t len,
                                              size_t size_requested);
bool skia_rs_raw_buffered_stream_plan_transfer(const SkiaRsRawBufferedStream* state,
                                               size_t offset,
                                               size_t size,
                                               SkiaRsBufferedTransferPlan* out_plan);
bool skia_rs_raw_buffered_stream_finalize_transfer_size(size_t already_buffered,
                                                        size_t remaining,
                                                        size_t bytes_read,
                                                        size_t* out_size);

uint32_t skia_rs_raw_perform_area_task_threads(bool is_android, uint32_t max_mp_threads);
SkiaRsRawTaskAreas* skia_rs_raw_task_areas_compute(int32_t max_tasks,
                                                   SkiaRsDngRect area,
                                                   SkiaRsDngPoint tile_size);
size_t skia_rs_raw_task_areas_len(const SkiaRsRawTaskAreas* areas);
const SkiaRsDngRect* skia_rs_raw_task_areas_data(const SkiaRsRawTaskAreas* areas);
void skia_rs_raw_task_areas_free(SkiaRsRawTaskAreas* areas);
}  // extern "C"

struct BufferedStreamDeleter {
    void operator()(SkiaRsRawBufferedStream* p) const {
        skia_rs_raw_buffered_stream_free(p);
    }
};

struct TaskAreasDeleter {
    void operator()(SkiaRsRawTaskAreas* p) const {
        skia_rs_raw_task_areas_free(p);
    }
};

constexpr bool kPmColorIsRgba =
#ifdef SK_PMCOLOR_IS_RGBA
        true;
#else
        false;
#endif

constexpr bool kIsAndroidBuild =
#ifdef SK_BUILD_FOR_ANDROID
        true;
#else
        false;
#endif

bool is_asset_stream(const SkStream& stream) {
    return skia_rs_raw_is_asset_stream(stream.hasLength(), stream.hasPosition());
}

SkiaRsRawEncodedInfoSpec query_encoded_info_spec(int width, int height) {
    SkiaRsRawEncodedInfoSpec spec{};
    skia_rs_raw_encoded_info_spec(width, height, &spec);
    return spec;
}

class SkDngHost : public dng_host {
public:
    explicit SkDngHost(dng_memory_allocator* allocator) : dng_host(allocator) {}

    void PerformAreaTask(dng_area_task& task, const dng_rect& area OPT_PROGRESS_ARG) override {
        SkTaskGroup taskGroup;

        // tileSize is typically 256x256
        const dng_point tileSize(task.FindTileSize(area));
        std::unique_ptr<SkiaRsRawTaskAreas, TaskAreasDeleter> rawAreas(
                skia_rs_raw_task_areas_compute(static_cast<int32_t>(this->PerformAreaTaskThreads()),
                                               SkiaRsDngRect{area.t, area.l, area.b, area.r},
                                               SkiaRsDngPoint{tileSize.v, tileSize.h}));
        if (!rawAreas) {
            ThrowProgramError("num_tiles_per_task calculation is wrong.");
        }

        const size_t areaCount = skia_rs_raw_task_areas_len(rawAreas.get());
        const SkiaRsDngRect* areaData = skia_rs_raw_task_areas_data(rawAreas.get());
        std::vector<dng_rect> taskAreas;
        taskAreas.reserve(areaCount);
        for (size_t i = 0; i < areaCount; ++i) {
            taskAreas.emplace_back(areaData[i].t, areaData[i].l, areaData[i].b, areaData[i].r);
        }
        const int numTasks = static_cast<int>(taskAreas.size());

        SkMutex mutex;
        TArray<dng_exception> exceptions;
#if SK_DNG_VERSION <= 0x01040000
        task.Start(numTasks, tileSize, &Allocator(), Sniffer());
#else
        task.Start(numTasks, area, tileSize, &Allocator(), Sniffer());
#endif
        for (int taskIndex = 0; taskIndex < numTasks; ++taskIndex) {
            taskGroup.add([&mutex, &exceptions, &task, this, taskIndex, taskAreas, tileSize] {
                try {
#if SK_DNG_VERSION <= 0x01040000
                    task.ProcessOnThread(taskIndex, taskAreas[taskIndex], tileSize,
                                         this->Sniffer());
#else
                    task.ProcessOnThread(taskIndex, taskAreas[taskIndex], tileSize,
                                         this->Sniffer(), nullptr);
#endif
                } catch (dng_exception& exception) {
                    SkAutoMutexExclusive lock(mutex);
                    exceptions.push_back(exception);
                } catch (...) {
                    SkAutoMutexExclusive lock(mutex);
                    exceptions.push_back(dng_exception(dng_error_unknown));
                }
            });
        }

        taskGroup.wait();
        task.Finish(numTasks);

        // We only re-throw the first exception.
        if (!exceptions.empty()) {
            Throw_dng_error(exceptions.front().ErrorCode(), nullptr, nullptr);
        }
    }

    uint32 PerformAreaTaskThreads() override {
        return skia_rs_raw_perform_area_task_threads(kIsAndroidBuild, kMaxMPThreads);
    }

private:
    using INHERITED = dng_host;
};

}  // namespace

class SkRawStream {
public:
    virtual ~SkRawStream() {}

    virtual uint64 getLength() = 0;
    virtual bool read(void* data, size_t offset, size_t length) = 0;
    virtual std::unique_ptr<SkMemoryStream> transferBuffer(size_t offset, size_t size) = 0;
};

// Note: the maximum buffer size is 100MB (enforced by safe Rust `RawBufferedStream`).
class SkRawBufferedStream : public SkRawStream {
public:
    explicit SkRawBufferedStream(std::unique_ptr<SkStream> stream)
            : fStream(std::move(stream))
            , fState(skia_rs_raw_buffered_stream_new()) {
        // Only use SkRawBufferedStream when the stream is not an asset stream.
        SkASSERT(!is_asset_stream(*fStream));
    }

    ~SkRawBufferedStream() override {}

    uint64 getLength() override {
        if (!this->bufferMoreData(kReadToEnd)) {  // read whole stream
            ThrowReadFile();
        }
        return skia_rs_raw_buffered_stream_bytes_written(fState.get());
    }

    bool read(void* data, size_t offset, size_t length) override {
        if (length == 0) {
            return true;
        }

        size_t sum = 0;
        if (!skia_rs_raw_safe_add_usize(offset, length, &sum)) {
            return false;
        }

        return this->bufferMoreData(sum) &&
               skia_rs_raw_buffered_stream_read_buffered(
                       fState.get(), offset, static_cast<uint8_t*>(data), length);
    }

    std::unique_ptr<SkMemoryStream> transferBuffer(size_t offset, size_t size) override {
        sk_sp<SkData> data(SkData::MakeUninitialized(size));
        SkiaRsBufferedTransferPlan plan{};
        skia_rs_raw_buffered_stream_plan_transfer(fState.get(), offset, size, &plan);

        if (plan.kind == kBufferedTransferSkipAndRead) {
            // If the offset is not buffered, read from fStream directly and skip the buffering.
            if (fStream->skip(plan.skip_length) != plan.skip_length) {
                return nullptr;
            }
            const size_t bytesRead = fStream->read(data->writable_data(), plan.remaining);
            if (bytesRead < plan.remaining) {
                data = SkData::MakeSubset(data.get(), 0, bytesRead);
            }
        } else {
            if (plan.already_buffered > 0 &&
                !skia_rs_raw_buffered_stream_read_buffered(
                        fState.get(),
                        plan.copy_offset,
                        static_cast<uint8_t*>(data->writable_data()),
                        plan.already_buffered)) {
                return nullptr;
            }

            if (plan.remaining > 0) {
                auto* dst = static_cast<uint8_t*>(data->writable_data()) + plan.already_buffered;
                const size_t bytesRead = fStream->read(dst, plan.remaining);
                if (bytesRead < plan.remaining) {
                    size_t newSize = 0;
                    if (!skia_rs_raw_buffered_stream_finalize_transfer_size(
                                plan.already_buffered, plan.remaining, bytesRead, &newSize)) {
                        return nullptr;
                    }
                    data = SkData::MakeSubset(data.get(), 0, newSize);
                }
            }
        }
        return SkMemoryStream::Make(data);
    }

private:
    // Note: if newSize == kReadToEnd (0), this function will read to the end of stream.
    bool bufferMoreData(size_t newSize) {
        SkiaRsBufferMorePlan plan{};
        skia_rs_raw_buffered_stream_plan_buffer_more(fState.get(), newSize, &plan);

        switch (plan.kind) {
            case kBufferMoreAlreadySatisfied:
                return true;
            case kBufferMoreFailed:
                return false;
            case kBufferMoreReadToEnd: {
                AutoSTMalloc<8192, uint8_t> scratch(plan.size_to_read);
                while (true) {
                    const size_t bytesRead = fStream->read(scratch.get(), plan.size_to_read);
                    if (bytesRead == 0) {
                        skia_rs_raw_buffered_stream_mark_eof(fState.get());
                        return true;
                    }
                    if (!skia_rs_raw_buffered_stream_write(fState.get(), scratch.get(),
                                                           bytesRead)) {
                        SkCodecPrintf("Error: Stream size exceeds the limit.\n");
                        return false;
                    }
                }
            }
            case kBufferMoreReadChunk: {
                AutoSTMalloc<8192, uint8_t> tempBuffer(plan.size_to_read);
                const size_t bytesRead = fStream->read(tempBuffer.get(), plan.size_to_read);
                if (!skia_rs_raw_buffered_stream_commit_chunk(
                            fState.get(), tempBuffer.get(), bytesRead, plan.size_requested)) {
                    if (bytesRead >= plan.size_requested) {
                        SkCodecPrintf("Error: Stream size exceeds the limit.\n");
                    }
                    return false;
                }
                return true;
            }
            default:
                return false;
        }
    }

    std::unique_ptr<SkStream> fStream;
    std::unique_ptr<SkiaRsRawBufferedStream, BufferedStreamDeleter> fState;

    static constexpr size_t kReadToEnd = 0;
};

class SkRawAssetStream : public SkRawStream {
public:
    explicit SkRawAssetStream(std::unique_ptr<SkStream> stream)
            : fStream(std::move(stream)) {
        // Only use SkRawAssetStream when the stream is an asset stream.
        SkASSERT(is_asset_stream(*fStream));
    }

    ~SkRawAssetStream() override {}

    uint64 getLength() override { return fStream->getLength(); }

    bool read(void* data, size_t offset, size_t length) override {
        if (length == 0) {
            return true;
        }

        if (!skia_rs_raw_validate_asset_read(offset, length)) {
            return false;
        }

        return fStream->seek(offset) && (fStream->read(data, length) == length);
    }

    std::unique_ptr<SkMemoryStream> transferBuffer(size_t offset, size_t size) override {
        size_t bytesToRead = 0;
        if (!skia_rs_raw_plan_asset_transfer(fStream->getLength(), offset, size, &bytesToRead)) {
            return nullptr;
        }

        if (fStream->getMemoryBase()) {  // directly copy if getMemoryBase() is available.
            sk_sp<SkData> data(SkData::MakeWithCopy(
                    static_cast<const uint8_t*>(fStream->getMemoryBase()) + offset, bytesToRead));
            fStream.reset();
            return SkMemoryStream::Make(data);
        } else {
            sk_sp<SkData> data(SkData::MakeUninitialized(bytesToRead));
            if (!fStream->seek(offset)) {
                return nullptr;
            }
            const size_t bytesRead = fStream->read(data->writable_data(), bytesToRead);
            if (bytesRead < bytesToRead) {
                data = SkData::MakeSubset(data.get(), 0, bytesRead);
            }
            return SkMemoryStream::Make(data);
        }
    }

private:
    std::unique_ptr<SkStream> fStream;
};

class SkPiexStream : public ::piex::StreamInterface {
public:
    // Will NOT take the ownership of the stream.
    explicit SkPiexStream(SkRawStream* stream) : fStream(stream) {}

    ~SkPiexStream() override {}

    ::piex::Error GetData(const size_t offset, const size_t length, uint8* data) override {
        return fStream->read(static_cast<void*>(data), offset, length) ? ::piex::Error::kOk
                                                                       : ::piex::Error::kFail;
    }

private:
    SkRawStream* fStream;
};

class SkDngStream : public dng_stream {
public:
    // Will NOT take the ownership of the stream.
    SkDngStream(SkRawStream* stream)
            // The default constructor sets offsetInOriginalFile to invalid (-1), however
            // this results in dng_negative hitting a bug path that causes a crash due to
            // unsigned overflow occuring. This offset doesn't seem to serve any purpose,
            // so just pretend we're always at the start of the file to avoid the crash.
            : dng_stream((dng_abort_sniffer*)nullptr, dng_stream::kDefaultBufferSize, 0)
            , fStream(stream) {}

    ~SkDngStream() override {}

    uint64 DoGetLength() override { return fStream->getLength(); }

    void DoRead(void* data, uint32 count, uint64 offset) override {
        size_t safeOffset = 0;
        size_t safeCount = 0;
        if (!skia_rs_raw_validate_dng_read(count, offset, &safeOffset, &safeCount) ||
            !fStream->read(data, safeOffset, safeCount)) {
            ThrowReadFile();
        }
    }

private:
    SkRawStream* fStream;
};

class SkDngImage {
public:
    static SkDngImage* NewFromStream(SkRawStream* stream) {
        std::unique_ptr<SkDngImage> dngImage(new SkDngImage(stream));
#if defined(SK_BUILD_FOR_LIBFUZZER)
        // Libfuzzer easily runs out of memory after here. To avoid that
        // We just pretend all streams are invalid. Our AFL-fuzzer
        // should still exercise this code; it's more resistant to OOM.
        return nullptr;
#else
        if (!dngImage->initFromPiex() && !dngImage->readDng()) {
            return nullptr;
        }

        return dngImage.release();
#endif
    }

    dng_image* render(int width, int height) {
        if (!fHost || !fInfo || !fNegative || !fDngStream) {
            if (!this->readDng()) {
                return nullptr;
            }
        }

        // DNG SDK preserves the aspect ratio, so it only needs to know the longer dimension.
        const int preferredSize = skia_rs_raw_preferred_render_size(width, height);
        try {
            // render() takes ownership of fHost, fInfo, fNegative and fDngStream when available.
            std::unique_ptr<dng_host> host(fHost.release());
            std::unique_ptr<dng_info> info(fInfo.release());
            std::unique_ptr<dng_negative> negative(fNegative.release());
            std::unique_ptr<dng_stream> dngStream(fDngStream.release());

            host->SetPreferredSize(preferredSize);
            host->ValidateSizes();

            negative->ReadStage1Image(*host, *dngStream, *info);

            if (info->fMaskIndex != -1) {
                negative->ReadTransparencyMask(*host, *dngStream, *info);
            }

            negative->ValidateRawImageDigest(*host);
            if (negative->IsDamaged()) {
                return nullptr;
            }

            const int32 kMosaicPlane = -1;
            negative->BuildStage2Image(*host);
            negative->BuildStage3Image(*host, kMosaicPlane);

            dng_render render(*host, *negative);
            render.SetFinalSpace(dng_space_sRGB::Get());
            render.SetFinalPixelType(ttByte);

            dng_point stage3_size = negative->Stage3Image()->Size();
            render.SetMaximumSize(skia_rs_raw_max_render_size(stage3_size.h, stage3_size.v));

            return render.Render();
        } catch (...) {
            return nullptr;
        }
    }

    int width() const { return fMeta.width; }
    int height() const { return fMeta.height; }
    bool isScalable() const { return fMeta.is_scalable; }
    bool isXtransImage() const { return fMeta.is_xtrans_image; }

    // Quick check if the image contains a valid TIFF header as requested by DNG format.
    // Does not affect ownership of stream.
    static bool IsTiffHeaderValid(SkRawStream* stream) {
        constexpr size_t kHeaderSize = 4;
        uint8_t header[kHeaderSize];
        if (!stream->read(header, 0 /* offset */, kHeaderSize)) {
            return false;
        }
        return skia_rs_raw_is_tiff_header_valid(header, kHeaderSize);
    }

private:
    bool init(int width, int height, const dng_point& cfaPatternSize) {
        return skia_rs_raw_init_image_meta(width, height, cfaPatternSize.v, cfaPatternSize.h,
                                           &fMeta);
    }

    bool initFromPiex() {
        // Does not take the ownership of rawStream.
        SkPiexStream piexStream(fStream.get());
        ::piex::PreviewImageData imageData;
        if (::piex::IsRaw(&piexStream) &&
            ::piex::GetPreviewImageData(&piexStream, &imageData) == ::piex::Error::kOk) {
            dng_point cfaPatternSize(imageData.cfa_pattern_dim[1], imageData.cfa_pattern_dim[0]);
            return this->init(static_cast<int>(imageData.full_width),
                              static_cast<int>(imageData.full_height),
                              cfaPatternSize);
        }
        return false;
    }

    bool readDng() {
        try {
#if qDNGUseXMP
            static std::once_flag fInitializeDngSDK;
            std::call_once(fInitializeDngSDK, []() { dng_xmp_sdk::InitializeSDK(); });
#endif
            // Due to the limit of DNG SDK, we need to reset host and info.
            fHost = std::make_unique<SkDngHost>(&fAllocator);
            fInfo = std::make_unique<dng_info>();
            fDngStream = std::make_unique<SkDngStream>(fStream.get());

            fHost->ValidateSizes();
            fInfo->Parse(*fHost, *fDngStream);
            fInfo->PostParse(*fHost);
            if (!fInfo->IsValidDNG()) {
                return false;
            }

            fNegative.reset(fHost->Make_dng_negative());
            fNegative->Parse(*fHost, *fDngStream, *fInfo);
            fNegative->PostParse(*fHost, *fDngStream, *fInfo);
            fNegative->SynchronizeMetadata();

            dng_point cfaPatternSize(0, 0);
            if (fNegative->GetMosaicInfo() != nullptr) {
                cfaPatternSize = fNegative->GetMosaicInfo()->fCFAPatternSize;
            }
            return this->init(static_cast<int>(fNegative->DefaultCropSizeH().As_real64()),
                              static_cast<int>(fNegative->DefaultCropSizeV().As_real64()),
                              cfaPatternSize);
        } catch (...) {
            return false;
        }
    }

    explicit SkDngImage(SkRawStream* stream) : fStream(stream) {}

    dng_memory_allocator fAllocator;
    std::unique_ptr<SkRawStream> fStream;
    std::unique_ptr<dng_host> fHost;
    std::unique_ptr<dng_info> fInfo;
    std::unique_ptr<dng_negative> fNegative;
    std::unique_ptr<dng_stream> fDngStream;

    SkiaRsRawImageMeta fMeta{};
};

/*
 * Tries to handle the image with PIEX. If PIEX returns kOk and finds the preview image, create a
 * SkJpegCodec. If PIEX returns kFail, then the file is invalid, return nullptr. In other cases,
 * fallback to create SkRawCodec for DNG images.
 */
std::unique_ptr<SkCodec> SkRawCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                    Result* result) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }
    std::unique_ptr<SkRawStream> rawStream;
    if (is_asset_stream(*stream)) {
        rawStream = std::make_unique<SkRawAssetStream>(std::move(stream));
    } else {
        rawStream = std::make_unique<SkRawBufferedStream>(std::move(stream));
    }

    // Does not take the ownership of rawStream.
    SkPiexStream piexStream(rawStream.get());
    ::piex::PreviewImageData imageData;
    if (::piex::IsRaw(&piexStream)) {
        ::piex::Error error = ::piex::GetPreviewImageData(&piexStream, &imageData);
        SkiaRsPiexPreviewPlan plan{};
        skia_rs_raw_evaluate_piex_preview(static_cast<int32_t>(error),
                                          static_cast<int32_t>(imageData.color_space),
                                          imageData.preview.offset,
                                          imageData.preview.length,
                                          static_cast<int32_t>(imageData.preview.format),
                                          imageData.exif_orientation,
                                          &plan);

        if (plan.decision == kPiexDecisionFail) {
            *result = static_cast<Result>(plan.result);
            return nullptr;
        }

        if (plan.decision == kPiexDecisionUseJpegPreview) {
            std::unique_ptr<SkCodecs::ColorProfile> profile;
            if (plan.use_adobe_rgb) {
                profile = SkCodecs::ColorProfile::Make(SkNamedTransferFn::k2Dot2,
                                                       SkNamedGamut::kAdobeRGB);
            }

            // transferBuffer() is destructive to the rawStream. Abandon the rawStream after this
            // function call.
            auto memoryStream =
                    rawStream->transferBuffer(plan.preview_offset, plan.preview_length);
            if (!memoryStream) {
                *result = kInvalidInput;
                return nullptr;
            }
            const auto origin = static_cast<SkEncodedOrigin>(plan.origin);
            return SkJpegCodec::MakeFromStream(std::move(memoryStream), result, std::move(profile),
                                               origin);
        }
    }

    if (!SkDngImage::IsTiffHeaderValid(rawStream.get())) {
        *result = kUnimplemented;
        return nullptr;
    }

    // Takes the ownership of the rawStream.
    std::unique_ptr<SkDngImage> dngImage(SkDngImage::NewFromStream(rawStream.release()));
    if (!dngImage) {
        *result = kInvalidInput;
        return nullptr;
    }

    *result = kSuccess;
    return std::unique_ptr<SkCodec>(new SkRawCodec(dngImage.release()));
}

SkCodec::Result SkRawCodec::onGetPixels(const SkImageInfo& dstInfo,
                                        void* dst,
                                        size_t dstRowBytes,
                                        const Options&,
                                        int* rowsDecoded) {
    const int width = dstInfo.width();
    const int height = dstInfo.height();
    std::unique_ptr<dng_image> image(fDngImage->render(width, height));
    if (!image) {
        return kInvalidInput;
    }

    const dng_point& imageSize = image->Size();
    SkiaRsRawGetPixelsPlan plan{};
    skia_rs_raw_plan_get_pixels(imageSize.h,
                                imageSize.v,
                                width,
                                height,
                                static_cast<int32_t>(dstInfo.colorType()),
                                kPmColorIsRgba,
                                &plan);
    if (plan.result != static_cast<int32_t>(kSuccess)) {
        return static_cast<Result>(plan.result);
    }

    void* dstRow = dst;
    AutoTMalloc<uint8_t> srcRow(plan.buffer_layout.src_row_bytes);

    dng_pixel_buffer buffer;
    buffer.fData = &srcRow[0];
    buffer.fPlane = plan.buffer_layout.plane;
    buffer.fPlanes = plan.buffer_layout.planes;
    buffer.fColStep = plan.buffer_layout.col_step;
    buffer.fPlaneStep = plan.buffer_layout.plane_step;
    buffer.fPixelType = plan.buffer_layout.pixel_type;
    buffer.fPixelSize = plan.buffer_layout.pixel_size;
    buffer.fRowStep = plan.buffer_layout.row_step;

    const auto srcFormat = static_cast<skcms_PixelFormat>(plan.src_format);
    const auto dstFormat = static_cast<skcms_PixelFormat>(plan.dst_format);
    const auto srcAlpha = static_cast<skcms_AlphaFormat>(plan.src_alpha_format);
    const auto dstAlpha = static_cast<skcms_AlphaFormat>(plan.dst_alpha_format);

    const skcms_ICCProfile* const srcProfile = this->getEncodedInfo().profile();
    skcms_ICCProfile dstProfileStorage;
    const skcms_ICCProfile* dstProfile = nullptr;
    if (auto cs = dstInfo.colorSpace()) {
        cs->toProfile(&dstProfileStorage);
        dstProfile = &dstProfileStorage;
    }

    for (int i = 0; i < plan.row_count; ++i) {
        SkiaRsDngRect area{};
        skia_rs_raw_row_area(i, plan.pixel_count, &area);
        buffer.fArea = dng_rect(area.t, area.l, area.b, area.r);

        try {
            image->Get(buffer, dng_image::edge_zero);
        } catch (...) {
            *rowsDecoded = i;
            return kIncompleteInput;
        }

        if (!skcms_Transform(&srcRow[0], srcFormat, srcAlpha, srcProfile,
                             dstRow,     dstFormat, dstAlpha, dstProfile,
                             static_cast<size_t>(plan.pixel_count))) {
            SkDebugf("failed to transform\n");
            *rowsDecoded = i;
            return kInternalError;
        }

        dstRow = SkTAddOffset<void>(dstRow, dstRowBytes);
    }
    return kSuccess;
}

SkISize SkRawCodec::onGetScaledDimensions(float desiredScale) const {
    SkASSERT(desiredScale <= 1.f);

    const SkISize dim = this->dimensions();
    SkASSERT(dim.fWidth != 0 && dim.fHeight != 0);

    int32_t outWidth = dim.fWidth;
    int32_t outHeight = dim.fHeight;
    skia_rs_raw_get_scaled_dimensions(dim.fWidth,
                                      dim.fHeight,
                                      desiredScale,
                                      fDngImage->isScalable(),
                                      fDngImage->isXtransImage(),
                                      &outWidth,
                                      &outHeight);
    return SkISize::Make(outWidth, outHeight);
}

bool SkRawCodec::onDimensionsSupported(const SkISize& dim) {
    const SkISize fullDim = this->dimensions();
    return skia_rs_raw_dimensions_supported(fullDim.fWidth,
                                            fullDim.fHeight,
                                            dim.fWidth,
                                            dim.fHeight,
                                            fDngImage->isScalable(),
                                            fDngImage->isXtransImage());
}

SkRawCodec::~SkRawCodec() {}

SkRawCodec::SkRawCodec(SkDngImage* dngImage)
        : INHERITED(
                  [dngImage]() {
                      const SkiaRsRawEncodedInfoSpec spec =
                              query_encoded_info_spec(dngImage->width(), dngImage->height());
                      return SkEncodedInfo::Make(
                              spec.width,
                              spec.height,
                              static_cast<SkEncodedInfo::Color>(spec.encoded_color),
                              static_cast<SkEncodedInfo::Alpha>(spec.encoded_alpha),
                              spec.bits_per_component);
                  }(),
                  static_cast<skcms_PixelFormat>(
                          query_encoded_info_spec(dngImage->width(), dngImage->height())
                                  .codec_xform_format),
                  nullptr)
        , fDngImage(dngImage) {}

namespace SkRawDecoder {

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    return SkRawCodec::MakeFromStream(std::move(stream), outResult);
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

}  // namespace SkRawDecoder
