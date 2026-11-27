/*
 * Safe Rust bridge for:
 *   src/codec/SkCodec.cpp
 *   src/codec/SkEncodedInfo.cpp
 *   src/codec/SkPixmapUtils.cpp
 *   src/codec/SkCodecImageGenerator.cpp
 *   src/codec/SkImageGenerator_FromEncoded.cpp
 *   src/encode/SkEncoder.cpp
 * (Phase 2 Module P2.1: skia-rs-codec-core).
 */

#include "include/codec/SkCodec.h"
#include "include/codec/SkCodecAnimation.h"
#include "include/codec/SkEncodedOrigin.h"
#include "include/codec/SkPixmapUtils.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkGraphics.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageGenerator.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkStream.h"
#include "include/core/SkSurface.h"
#include "include/core/SkTypes.h"
#include "include/encode/SkEncoder.h"
#include "include/private/SkAssert.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecImageGenerator.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkFrameHolder.h"
#include "src/codec/SkPixmapUtilsPriv.h"
#include "src/codec/SkSampler.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkNoDestructor.h"
#include "src/core/SkStreamPriv.h"
#include "src/image/SkImageGeneratorPriv.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <tuple>
#include <utility>
#include <vector>

#if !defined(SK_DISABLE_LEGACY_INIT_DECODERS)
#include "include/private/SkOnce.h"

#if defined(SK_CODEC_DECODES_AVIF)
#include "include/codec/SkAvifDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_BMP_WITH_RUST)
#include "include/codec/SkBmpRustDecoder.h"
#elif defined(SK_CODEC_DECODES_BMP)
#include "include/codec/SkBmpDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_GIF) || defined(SK_HAS_WUFFS_LIBRARY)
#include "include/codec/SkGifDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_ICO_WITH_RUST)
#include "experimental/rust_ico/decoder/SkIcoRustDecoder.h"
#elif defined(SK_CODEC_DECODES_ICO)
#include "include/codec/SkIcoDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_JPEG_WITH_RUST)
#include "experimental/rust_jpeg/decoder/SkJpegRustDecoder.h"
#elif defined(SK_CODEC_DECODES_JPEG)
#include "include/codec/SkJpegDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_JPEGXL)
#include "include/codec/SkJpegxlDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_PNG_WITH_LIBPNG)
#include "include/codec/SkPngDecoder.h"
#elif defined(SK_CODEC_DECODES_PNG_WITH_RUST)
#include "include/codec/SkPngRustDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_RAW)
#include "include/codec/SkRawDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_WBMP)
#include "include/codec/SkWbmpDecoder.h"
#endif

#if defined(SK_CODEC_DECODES_WEBP)
#include "include/codec/SkWebpDecoder.h"
#endif
#endif  // !defined(SK_DISABLE_LEGACY_INIT_DECODERS)

namespace {

#ifdef SK_PMCOLOR_IS_RGBA
constexpr bool kPmColorIsRgba = true;
constexpr bool kPmColorIsBgra = false;
#else
constexpr bool kPmColorIsRgba = false;
constexpr bool kPmColorIsBgra = true;
#endif

static_assert(skcms_PixelFormat_G_8 == 2);
static_assert(skcms_PixelFormat_BGR_565 == 7);
static_assert(skcms_PixelFormat_RGBA_8888 == 12);
static_assert(skcms_PixelFormat_BGRA_8888 == 13);
static_assert(skcms_PixelFormat_RGBA_1010102 == 16);
static_assert(skcms_PixelFormat_RGBA_hhhh == 32);
static_assert(skcms_PixelFormat_BGR_101010x_XR == 39);
static_assert(skcms_AlphaFormat_Opaque == 0);
static_assert(skcms_AlphaFormat_Unpremul == 1);
static_assert(skcms_AlphaFormat_PremulAsEncoded == 2);

struct RustColorXformPlan {
    bool success;
    int32_t xform_time;
    int32_t dst_xform_format;
    int32_t dst_xform_alpha_format;
};

struct RustZeroRectPlan {
    bool ok;
    bool should_fill;
    SkIRect rect;
    size_t byte_offset;
};

struct RustFillIncompletePlan {
    bool should_fill;
    int32_t fill_width;
    int32_t lines_remaining;
    size_t byte_offset;
};

struct RustFrameSummary {
    int32_t frame_id;
    int32_t reported_alpha;
    bool has_alpha;
    int32_t required_frame;
    int32_t disposal_method;
    int32_t blend;
    SkIRect frame_rect;
};

struct RustFrameInfoFields {
    int32_t required_frame;
    int32_t duration;
    bool fully_received;
    int32_t alpha_type;
    bool has_alpha_within_bounds;
    int32_t disposal_method;
    int32_t blend;
    SkIRect frame_rect;
};

struct RustFrameDependencyResult {
    bool has_alpha;
    int32_t required_frame;
};

struct RustGeneratorInfoAdjust {
    int32_t width;
    int32_t height;
    int32_t alpha_type;
};

using RustGetFrameSummaryFn = RustFrameSummary (*)(const void* ctx, int32_t id);

}  // namespace

extern "C" {

bool skia_rs_encoded_info_make_image_info_types(int32_t color,
                                                int32_t alpha,
                                                bool pmcolor_is_bgra,
                                                int32_t* out_ct,
                                                int32_t* out_at);
bool skia_rs_encoded_info_verify_color(int32_t color, int32_t alpha, int32_t bits_per_component);

const char* skia_rs_codec_result_to_string(int32_t result);
bool skia_rs_codec_is_valid_selection_policy(int32_t policy);
int32_t skia_rs_codec_classify_decoder_id(const uint8_t* id, size_t len);
int32_t skia_rs_codec_no_decoder_match_result(size_t bytes_read);
bool skia_rs_codec_conversion_supported(int32_t encoded_color,
                                        int32_t dst_ct,
                                        int32_t dst_at,
                                        bool src_is_opaque);
bool skia_rs_codec_select_xform_format(int32_t color_type,
                                       bool for_color_table,
                                       bool pmcolor_is_rgba,
                                       int32_t* out_format);
bool skia_rs_codec_color_type_forces_xform(int32_t dst_ct);
RustColorXformPlan skia_rs_codec_finalize_color_xform(int32_t encoded_color,
                                                      int32_t encoded_alpha,
                                                      int32_t dst_ct,
                                                      int32_t dst_at,
                                                      bool pmcolor_is_rgba);
bool skia_rs_codec_allocate_from_budget(size_t* budget, size_t num_bytes);
RustZeroRectPlan skia_rs_codec_compute_zero_rect(SkISize dst_dims,
                                                 SkISize src_dims,
                                                 SkIRect prev_rect,
                                                 size_t bpp,
                                                 size_t row_bytes);
int32_t skia_rs_codec_validate_get_pixels(int32_t dst_ct,
                                          bool has_pixels,
                                          size_t row_bytes,
                                          size_t min_row_bytes);
int32_t skia_rs_codec_validate_start_incremental(bool supports_incremental,
                                                 int32_t dst_ct,
                                                 bool has_pixels,
                                                 SkISize dst_dims,
                                                 const SkIRect* subset);
int32_t skia_rs_codec_validate_start_scanline(SkISize dst_dims,
                                              const SkIRect* subset,
                                              int32_t frame_index);
bool skia_rs_codec_validate_get_scanlines(int32_t curr_scanline,
                                          int32_t dst_height,
                                          int32_t count_lines);
bool skia_rs_codec_validate_skip_scanlines(int32_t curr_scanline,
                                           int32_t dst_height,
                                           int32_t count_lines);
int32_t skia_rs_codec_output_scanline(int32_t order, int32_t encoded_height, int32_t input_scanline);
int32_t skia_rs_codec_validate_nonzero_frame_index(int32_t frame_index,
                                                   bool has_subset,
                                                   int32_t frame_count);
bool skia_rs_codec_validate_prior_frame_range(int32_t prior_frame,
                                              int32_t required_frame,
                                              int32_t frame_index);
int32_t skia_rs_codec_classify_prior_frame_disposal(int32_t disposal,
                                                    int32_t prepped_frame_id,
                                                    int32_t required_frame);
RustFillIncompletePlan skia_rs_codec_plan_fill_incomplete_image(int32_t dst_ct,
                                                                int32_t dst_width,
                                                                size_t row_bytes,
                                                                int32_t zero_init,
                                                                int32_t lines_requested,
                                                                int32_t lines_decoded,
                                                                bool has_frame,
                                                                int32_t frame_required,
                                                                bool frame_rect_matches_bounds,
                                                                int32_t sampler_fill_width,
                                                                int32_t subset_width,
                                                                int32_t scanline_order);
RustFrameInfoFields skia_rs_frame_fill_in(int32_t required_frame,
                                          int32_t duration,
                                          bool fully_received,
                                          bool has_alpha,
                                          int32_t reported_alpha,
                                          int32_t disposal_method,
                                          int32_t blend,
                                          SkIRect frame_rect);
RustFrameDependencyResult skia_rs_frame_holder_compute_dependency(int32_t screen_width,
                                                                  int32_t screen_height,
                                                                  RustFrameSummary frame,
                                                                  const void* ctx,
                                                                  RustGetFrameSummaryFn get_frame);
RustGeneratorInfoAdjust skia_rs_codec_adjust_generator_info(int32_t width,
                                                            int32_t height,
                                                            int32_t alpha_type,
                                                            int32_t origin,
                                                            bool has_requested_alpha,
                                                            int32_t requested_alpha);
bool skia_rs_codec_is_partial_or_full_success(int32_t result);

int32_t skia_rs_pixmap_utils_validate_orient(int32_t src_ct,
                                             int32_t dst_ct,
                                             int32_t src_w,
                                             int32_t src_h,
                                             int32_t dst_w,
                                             int32_t dst_h,
                                             int32_t origin,
                                             bool same_addr);
SkISize skia_rs_pixmap_utils_swap_wh(SkISize size);

bool skia_rs_encoder_clamp_rows(int32_t curr_row,
                                int32_t height,
                                int32_t num_rows,
                                int32_t* out_clamped);

}  // extern "C"

// ============================================================================
// SkEncodedInfo.cpp
// ============================================================================

SkImageInfo SkEncodedInfo::makeImageInfo() const {
    int32_t ct = static_cast<int32_t>(kN32_SkColorType);
    int32_t at = static_cast<int32_t>(kUnpremul_SkAlphaType);
    skia_rs_encoded_info_make_image_info_types(static_cast<int32_t>(fColor),
                                               static_cast<int32_t>(fAlpha),
                                               kPmColorIsBgra,
                                               &ct,
                                               &at);
    auto cs = fColorProfile ? fColorProfile->getExactColorSpace() : SkColorSpace::MakeSRGB();
    if (!cs) {
        cs = SkColorSpace::MakeSRGB();
    }
    return SkImageInfo::Make(fWidth,
                             fHeight,
                             static_cast<SkColorType>(ct),
                             static_cast<SkAlphaType>(at),
                             std::move(cs));
}

SkEncodedInfo SkEncodedInfo::Make(
        int width, int height, Color color, Alpha alpha, int bitsPerComponent) {
    return Make(width, height, color, alpha, bitsPerComponent, nullptr);
}

SkEncodedInfo SkEncodedInfo::Make(int width,
                                  int height,
                                  Color color,
                                  Alpha alpha,
                                  int bitsPerComponent,
                                  std::unique_ptr<SkCodecs::ColorProfile> profile) {
    return Make(width,
                height,
                color,
                alpha,
                bitsPerComponent,
                std::move(profile),
                bitsPerComponent);
}

SkEncodedInfo SkEncodedInfo::Make(int width,
                                  int height,
                                  Color color,
                                  Alpha alpha,
                                  int bitsPerComponent,
                                  std::unique_ptr<SkCodecs::ColorProfile> profile,
                                  int colorDepth) {
    return Make(width,
                height,
                color,
                alpha,
                bitsPerComponent,
                colorDepth,
                std::move(profile),
                skhdr::Metadata::MakeEmpty());
}

SkEncodedInfo SkEncodedInfo::Make(int width,
                                  int height,
                                  Color color,
                                  Alpha alpha,
                                  int bitsPerComponent,
                                  int colorDepth,
                                  std::unique_ptr<SkCodecs::ColorProfile> profile,
                                  const skhdr::Metadata& hdrMetadata) {
    SkASSERT(skia_rs_encoded_info_verify_color(
            static_cast<int32_t>(color), static_cast<int32_t>(alpha), bitsPerComponent));
    VerifyColor(color, alpha, bitsPerComponent);
    return SkEncodedInfo(width,
                         height,
                         color,
                         alpha,
                         SkToU8(bitsPerComponent),
                         SkToU8(colorDepth),
                         std::move(profile),
                         hdrMetadata);
}

const skcms_ICCProfile* SkEncodedInfo::profile() const {
    if (!fColorProfile) {
        return nullptr;
    }
    return fColorProfile->profile();
}

sk_sp<const SkData> SkEncodedInfo::profileData() const {
    if (!fColorProfile) {
        return nullptr;
    }
    return fColorProfile->data();
}

SkEncodedInfo SkEncodedInfo::copy() const {
    return SkEncodedInfo(fWidth,
                         fHeight,
                         fColor,
                         fAlpha,
                         fBitsPerComponent,
                         fColorDepth,
                         fColorProfile ? fColorProfile->clone() : nullptr,
                         fHdrMetadata);
}

SkEncodedInfo::SkEncodedInfo(SkEncodedInfo&& orig) = default;
SkEncodedInfo& SkEncodedInfo::operator=(SkEncodedInfo&&) = default;
SkEncodedInfo::~SkEncodedInfo() = default;

SkEncodedInfo::SkEncodedInfo(int width,
                             int height,
                             Color color,
                             Alpha alpha,
                             uint8_t bitsPerComponent,
                             uint8_t colorDepth,
                             std::unique_ptr<SkCodecs::ColorProfile> profile,
                             const skhdr::Metadata& hdrMetadata)
        : fWidth(width)
        , fHeight(height)
        , fColor(color)
        , fAlpha(alpha)
        , fBitsPerComponent(bitsPerComponent)
        , fColorDepth(colorDepth)
        , fColorProfile(std::move(profile))
        , fHdrMetadata(hdrMetadata) {}

// ============================================================================
// SkCodec.cpp — Decoder Registry & Factory
// ============================================================================

namespace SkCodecs {

static std::vector<Decoder>* get_decoders_for_editing() {
    static SkNoDestructor<std::vector<Decoder>> decoders;
#if !defined(SK_DISABLE_LEGACY_INIT_DECODERS)
    static SkOnce once;
    once([] {
        if (decoders->empty()) {
#if defined(SK_CODEC_DECODES_PNG_WITH_LIBPNG)
            decoders->push_back(SkPngDecoder::Decoder());
#elif defined(SK_CODEC_DECODES_PNG_WITH_RUST)
            decoders->push_back(SkPngRustDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_JPEG_WITH_RUST)
            decoders->push_back(SkJpegRustDecoder::Decoder());
#elif defined(SK_CODEC_DECODES_JPEG)
            decoders->push_back(SkJpegDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_WEBP)
            decoders->push_back(SkWebpDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_GIF) || defined(SK_HAS_WUFFS_LIBRARY)
            decoders->push_back(SkGifDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_ICO_WITH_RUST)
            decoders->push_back(SkIcoRustDecoder::Decoder());
#elif defined(SK_CODEC_DECODES_ICO)
            decoders->push_back(SkIcoDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_BMP_WITH_RUST)
            decoders->push_back(SkBmpRustDecoder::Decoder());
#elif defined(SK_CODEC_DECODES_BMP)
            decoders->push_back(SkBmpDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_WBMP)
            decoders->push_back(SkWbmpDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_AVIF)
            decoders->push_back(SkAvifDecoder::CrabbyAvif::Decoder());
#endif
#if defined(SK_CODEC_DECODES_JPEGXL)
            decoders->push_back(SkJpegxlDecoder::Decoder());
#endif
#if defined(SK_CODEC_DECODES_RAW)
            decoders->push_back(SkRawDecoder::Decoder());
#endif
        }
    });
#endif  // !defined(SK_DISABLE_LEGACY_INIT_DECODERS)
    return decoders.get();
}

const std::vector<Decoder>& get_decoders() {
    auto decoders = get_decoders_for_editing();
    return *decoders;
}

void Register(Decoder d) {
    auto decoders = get_decoders_for_editing();
    for (size_t i = 0; i < decoders->size(); i++) {
        if ((*decoders)[i].id == d.id) {
            (*decoders)[i] = d;
            return;
        }
    }
    decoders->push_back(d);
}

bool HasDecoder(std::string_view id) {
    for (const SkCodecs::Decoder& decoder : get_decoders()) {
        if (decoder.id == id) {
            return true;
        }
    }
    return false;
}

}  // namespace SkCodecs

std::unique_ptr<SkCodec> SkCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                 Result* outResult,
                                                 SkPngChunkReader* chunkReader,
                                                 SelectionPolicy selectionPolicy) {
    return MakeFromStream(
            std::move(stream), SkCodecs::get_decoders(), outResult, chunkReader, selectionPolicy);
}

std::unique_ptr<SkCodec> SkCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                 SkSpan<const SkCodecs::Decoder> decoders,
                                                 Result* outResult,
                                                 SkPngChunkReader* chunkReader,
                                                 SelectionPolicy selectionPolicy) {
    Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }

    if (!stream) {
        *outResult = kInvalidInput;
        return nullptr;
    }

    if (!skia_rs_codec_is_valid_selection_policy(static_cast<int32_t>(selectionPolicy))) {
        *outResult = kInvalidParameters;
        return nullptr;
    }

    constexpr size_t bytesToRead = MinBufferedBytesNeeded();
    char buffer[bytesToRead];
    size_t bytesRead = stream->peek(buffer, bytesToRead);
    if (0 == bytesRead) {
        bytesRead = stream->read(buffer, bytesToRead);
        if (!stream->rewind()) {
            SkCodecPrintf("Encoded image data could not peek or rewind to determine format!\n");
            *outResult = kCouldNotRewind;
            return nullptr;
        }
    }

    SkCodecs::MakeFromStreamCallback rawFallback = nullptr;
    for (const SkCodecs::Decoder& proc : decoders) {
        if (proc.isFormat(buffer, bytesRead)) {
            int32_t kind = skia_rs_codec_classify_decoder_id(
                    reinterpret_cast<const uint8_t*>(proc.id.data()), proc.id.size());
            if (kind == 1) {  // PngChunkReader
                return proc.makeFromStream(std::move(stream), outResult, chunkReader);
            } else if (kind == 2) {  // SelectionPolicy
                return proc.makeFromStream(std::move(stream), outResult, &selectionPolicy);
            } else if (kind == 3) {  // RawFallback
                rawFallback = proc.makeFromStream;
                continue;
            }
            return proc.makeFromStream(std::move(stream), outResult, nullptr);
        }
    }
    if (rawFallback != nullptr) {
        return rawFallback(std::move(stream), outResult, nullptr);
    }

    *outResult = static_cast<Result>(skia_rs_codec_no_decoder_match_result(bytesRead));
    return nullptr;
}

std::unique_ptr<SkCodec> SkCodec::MakeFromData(sk_sp<const SkData> data, SkPngChunkReader* reader) {
    return MakeFromData(std::move(data), SkCodecs::get_decoders(), reader);
}

std::unique_ptr<SkCodec> SkCodec::MakeFromData(sk_sp<const SkData> data,
                                               SkSpan<const SkCodecs::Decoder> decoders,
                                               SkPngChunkReader* reader) {
    if (!data) {
        return nullptr;
    }
    return MakeFromStream(SkMemoryStream::Make(std::move(data)), decoders, nullptr, reader);
}

// ============================================================================
// SkCodec.cpp — Core Methods
// ============================================================================

SkCodec::SkCodec(SkEncodedInfo&& info,
                 XformFormat srcFormat,
                 std::unique_ptr<SkStream> stream,
                 SkEncodedOrigin origin)
        : fEncodedInfo(std::move(info))
        , fSrcXformFormat(srcFormat)
        , fStream(std::move(stream))
        , fOrigin(origin)
        , fDstInfo()
        , fOptions() {}

SkCodec::~SkCodec() {}

bool SkCodec::queryYUVAInfo(const SkYUVAPixmapInfo::SupportedDataTypes& supportedDataTypes,
                            SkYUVAPixmapInfo* yuvaPixmapInfo) const {
    if (!yuvaPixmapInfo) {
        return false;
    }
    return this->onQueryYUVAInfo(supportedDataTypes, yuvaPixmapInfo) &&
           yuvaPixmapInfo->isSupported(supportedDataTypes);
}

SkCodec::Result SkCodec::getYUVAPlanes(const SkYUVAPixmaps& yuvaPixmaps) {
    if (!yuvaPixmaps.isValid()) {
        return kInvalidInput;
    }
    if (!this->rewindIfNeeded()) {
        return kCouldNotRewind;
    }
    return this->onGetYUVAPlanes(yuvaPixmaps);
}

bool SkCodec::conversionSupported(const SkImageInfo& dst, bool srcIsOpaque, bool needsColorXform) {
    return skia_rs_codec_conversion_supported(static_cast<int32_t>(fEncodedInfo.color()),
                                              static_cast<int32_t>(dst.colorType()),
                                              static_cast<int32_t>(dst.alphaType()),
                                              srcIsOpaque);
}

bool SkCodec::rewindStream() {
    if (fStream && !fStream->rewind()) {
        return false;
    }
    return true;
}

bool SkCodec::rewindIfNeeded() {
    const bool needsRewind = fNeedsRewind;
    fNeedsRewind = true;
    if (!needsRewind) {
        return true;
    }
    fCurrScanline = -1;
    fStartedIncrementalDecode = false;
    return this->onRewind();
}

bool zero_rect(const SkImageInfo& dstInfo,
               void* pixels,
               size_t rowBytes,
               SkISize srcDimensions,
               SkIRect prevRect) {
    RustZeroRectPlan plan = skia_rs_codec_compute_zero_rect(
            dstInfo.dimensions(), srcDimensions, prevRect, dstInfo.bytesPerPixel(), rowBytes);
    if (!plan.ok) {
        return false;
    }
    if (!plan.should_fill) {
        return true;
    }
    const SkImageInfo info = dstInfo.makeDimensions(plan.rect.size());
    void* eraseDst = SkTAddOffset<void>(pixels, plan.byte_offset);
    SkSampler::Fill(info, eraseDst, rowBytes, SkCodec::kNo_ZeroInitialized);
    return true;
}

SkCodec::Result SkCodec::handleFrameIndex(const SkImageInfo& info,
                                          void* pixels,
                                          size_t rowBytes,
                                          const Options& options,
                                          GetPixelsCallback getPixelsFn) {
    if (getPixelsFn) {
        fUsingCallbackForHandleFrameIndex = true;
    } else if (fUsingCallbackForHandleFrameIndex) {
        return kSuccess;
    }

    if (!this->rewindIfNeeded()) {
        return kCouldNotRewind;
    }

    const int index = options.fFrameIndex;
    if (0 == index) {
        return this->initializeColorXform(info, fEncodedInfo.alpha(), fEncodedInfo.opaque())
                       ? kSuccess
                       : kInvalidConversion;
    }

    if (index < 0 || options.fSubset) {
        return kInvalidParameters;
    }

    const Result indexRes = static_cast<Result>(skia_rs_codec_validate_nonzero_frame_index(
            index, options.fSubset != nullptr, this->onGetFrameCount()));
    if (indexRes != kSuccess) {
        return indexRes;
    }

    const auto* frameHolder = this->getFrameHolder();
    SkASSERT(frameHolder);

    const auto* frame = frameHolder->getFrame(index);
    SkASSERT(frame);

    const int requiredFrame = frame->getRequiredFrame();
    if (requiredFrame != kNoFrame) {
        const SkFrame* preppedFrame = nullptr;
        if (options.fPriorFrame == kNoFrame) {
            Result result = kInternalError;
            if (getPixelsFn) {
                result = getPixelsFn(info, pixels, rowBytes, options, requiredFrame);
            } else {
                Options prevFrameOptions(options);
                prevFrameOptions.fFrameIndex = requiredFrame;
                result = this->getPixels(info, pixels, rowBytes, &prevFrameOptions);
            }
            if (result != kSuccess) {
                return result;
            }
            preppedFrame = frameHolder->getFrame(requiredFrame);
        } else {
            if (!skia_rs_codec_validate_prior_frame_range(
                        options.fPriorFrame, requiredFrame, index)) {
                return kInvalidParameters;
            }
            preppedFrame = frameHolder->getFrame(options.fPriorFrame);
        }

        SkASSERT(preppedFrame);
        int32_t action = skia_rs_codec_classify_prior_frame_disposal(
                static_cast<int32_t>(preppedFrame->getDisposalMethod()),
                preppedFrame->frameId(),
                requiredFrame);
        if (action == 1) {  // InvalidParameters (kRestorePrevious)
            SkASSERT(options.fPriorFrame != kNoFrame);
            return kInvalidParameters;
        } else if (action == 2) {  // ZeroRect (kRestoreBGColor)
            if (!zero_rect(info, pixels, rowBytes, this->dimensions(), preppedFrame->frameRect())) {
                return kInternalError;
            }
        }
    }

    return this->initializeColorXform(info, frame->reportedAlpha(), !frame->hasAlpha())
                   ? kSuccess
                   : kInvalidConversion;
}

static const SkFrame* get_frame(const SkFrameHolder* frameHolder, int frameIndex) {
    return frameHolder ? frameHolder->getFrame(frameIndex) : nullptr;
}

static bool dst_has_prior_frame(const SkCodec::Options& options, const SkFrameHolder* frameHolder) {
    if (options.fPriorFrame != SkCodec::kNoFrame) {
        return true;
    }
    const SkFrame* frame = get_frame(frameHolder, options.fFrameIndex);
    return frame && frame->getRequiredFrame() != SkCodec::kNoFrame;
}

bool SkCodec::allocateFromBudget(size_t numBytes) {
    return skia_rs_codec_allocate_from_budget(&fDecodeBudget, numBytes);
}

SkCodec::Result SkCodec::getPixels(const SkImageInfo& info,
                                   void* pixels,
                                   size_t rowBytes,
                                   const Options* options) {
    Options optsStorage;
    if (!options) {
        options = &optsStorage;
    }
    fDecodeBudget = options->fMaxDecodeMemory ? options->fMaxDecodeMemory : SIZE_MAX;
    return this->getPixelsBudgeted(info, pixels, rowBytes, options);
}

SkCodec::Result SkCodec::getPixelsBudgeted(const SkImageInfo& info,
                                           void* pixels,
                                           size_t rowBytes,
                                           const Options* options) {
    const Result paramResult = static_cast<Result>(skia_rs_codec_validate_get_pixels(
            static_cast<int32_t>(info.colorType()),
            pixels != nullptr,
            rowBytes,
            info.minRowBytes()));
    if (paramResult != kSuccess) {
        return paramResult;
    }
    SkASSERT(options);
    SkASSERT(fDecodeBudget > 0);

    if (options->fSubset) {
        SkIRect subset(*options->fSubset);
        if (!this->onGetValidSubset(&subset) || subset != *options->fSubset) {
            return kUnimplemented;
        }
    }

    const Result frameIndexResult = this->handleFrameIndex(info, pixels, rowBytes, *options);
    if (frameIndexResult != kSuccess) {
        return frameIndexResult;
    }

    if (!this->dimensionsSupported(info.dimensions())) {
        return kInvalidScale;
    }

    fDstInfo = info;
    fOptions = *options;
    if (dst_has_prior_frame(fOptions, this->getFrameHolder())) {
        fOptions.fZeroInitialized = kNo_ZeroInitialized;
    }

    int rowsDecoded = 0;
    const Result result = this->onGetPixels(info, pixels, rowBytes, fOptions, &rowsDecoded);
    if ((kIncompleteInput == result || kErrorInInput == result) && rowsDecoded != info.height()) {
        fOptions.fSubset = nullptr;
        this->fillIncompleteImage(
                info, pixels, rowBytes, fOptions.fZeroInitialized, info.height(), rowsDecoded);
    }
    return result;
}

std::tuple<sk_sp<SkImage>, SkCodec::Result> SkCodec::getImage(const SkImageInfo& info,
                                                              const Options* options) {
    Options optsStorage;
    if (!options) {
        options = &optsStorage;
    }
    fDecodeBudget = options->fMaxDecodeMemory ? options->fMaxDecodeMemory : SIZE_MAX;

    if (size_t size = info.computeByteSize(info.minRowBytes()); !this->allocateFromBudget(size)) {
        return {nullptr, kOutOfMemory};
    }

    SkBitmap bm;
    if (!bm.tryAllocPixels(info)) {
        return {nullptr, kInternalError};
    }

    Result result = kInternalError;
    auto decode = [this, options, &result](const SkPixmap& pm) {
        result = this->getPixelsBudgeted(pm.info(), pm.writable_addr(), pm.rowBytes(), options);
        return skia_rs_codec_is_partial_or_full_success(static_cast<int32_t>(result));
    };

    if (!SkPixmapUtils::Orient(bm.pixmap(), this->getOrigin(), decode)) {
        return {nullptr, result};
    }
    bm.setImmutable();
    return {SkImages::RasterFromBitmap(bm), kSuccess};
}

std::tuple<sk_sp<SkImage>, SkCodec::Result> SkCodec::getImage() {
    SkImageInfo info = this->getInfo();
    if (SkEncodedOriginSwapsWidthHeight(this->getOrigin())) {
        info = SkPixmapUtils::SwapWidthHeight(info);
    }
    return this->getImage(info, nullptr);
}

SkCodec::Result SkCodec::startIncrementalDecode(const SkImageInfo& info,
                                                void* pixels,
                                                size_t rowBytes,
                                                const SkCodec::Options* options) {
    fStartedIncrementalDecode = false;

    Options optsStorage;
    if (nullptr == options) {
        options = &optsStorage;
    }

    const Result check = static_cast<Result>(skia_rs_codec_validate_start_incremental(
            this->onSupportsIncrementalDecode(info),
            static_cast<int32_t>(info.colorType()),
            pixels != nullptr,
            info.dimensions(),
            options->fSubset));
    if (check != kSuccess) {
        return check;
    }

    fDecodeBudget = options->fMaxDecodeMemory ? options->fMaxDecodeMemory : SIZE_MAX;

    const Result frameIndexResult = this->handleFrameIndex(info, pixels, rowBytes, *options);
    if (frameIndexResult != kSuccess) {
        return frameIndexResult;
    }

    if (!this->dimensionsSupported(info.dimensions())) {
        return kInvalidScale;
    }

    fDstInfo = info;
    fOptions = *options;
    if (dst_has_prior_frame(fOptions, this->getFrameHolder())) {
        fOptions.fZeroInitialized = kNo_ZeroInitialized;
    }

    const Result result = this->onStartIncrementalDecode(info, pixels, rowBytes, fOptions);
    if (kSuccess == result) {
        fStartedIncrementalDecode = true;
    }
    return result;
}

SkCodec::Result SkCodec::startScanlineDecode(const SkImageInfo& info,
                                             const SkCodec::Options* options) {
    fCurrScanline = -1;

    Options optsStorage;
    if (!options) {
        options = &optsStorage;
    }
    fDecodeBudget = options->fMaxDecodeMemory ? options->fMaxDecodeMemory : SIZE_MAX;

    const Result check = static_cast<Result>(skia_rs_codec_validate_start_scanline(
            info.dimensions(), options->fSubset, options->fFrameIndex));
    if (check != kSuccess) {
        return check;
    }

    const Result frameIndexResult = this->handleFrameIndex(info, nullptr, 0, *options);
    if (frameIndexResult != kSuccess) {
        return frameIndexResult;
    }

    if (!this->dimensionsSupported(info.dimensions())) {
        return kInvalidScale;
    }

    const Result result = this->onStartScanlineDecode(info, *options);
    if (result != SkCodec::kSuccess) {
        return result;
    }

    fCurrScanline = 0;
    fDstInfo = info;
    fOptions = *options;
    return kSuccess;
}

int SkCodec::getScanlines(void* dst, int countLines, size_t rowBytes) {
    if (!skia_rs_codec_validate_get_scanlines(fCurrScanline, fDstInfo.height(), countLines)) {
        return 0;
    }
    SkASSERT(!fDstInfo.isEmpty());

    const int linesDecoded = this->onGetScanlines(dst, countLines, rowBytes);
    if (linesDecoded < countLines) {
        this->fillIncompleteImage(this->dstInfo(),
                                  dst,
                                  rowBytes,
                                  this->options().fZeroInitialized,
                                  countLines,
                                  linesDecoded);
    }
    fCurrScanline += countLines;
    return linesDecoded;
}

bool SkCodec::skipScanlines(int countLines) {
    if (!skia_rs_codec_validate_skip_scanlines(fCurrScanline, fDstInfo.height(), countLines)) {
        return false;
    }
    SkASSERT(!fDstInfo.isEmpty());

    bool result = this->onSkipScanlines(countLines);
    fCurrScanline += countLines;
    return result;
}

int SkCodec::outputScanline(int inputScanline) const {
    SkASSERT(0 <= inputScanline && inputScanline < fEncodedInfo.height());
    return this->onOutputScanline(inputScanline);
}

int SkCodec::onOutputScanline(int inputScanline) const {
    return skia_rs_codec_output_scanline(
            static_cast<int32_t>(this->getScanlineOrder()), fEncodedInfo.height(), inputScanline);
}

void SkCodec::fillIncompleteImage(const SkImageInfo& info,
                                  void* dst,
                                  size_t rowBytes,
                                  ZeroInitialized zeroInit,
                                  int linesRequested,
                                  int linesDecoded) {
    if (kYes_ZeroInitialized == zeroInit && SkSampler::FillIsZeroBytes(info.colorType())) {
        return;
    }

    const SkFrame* frame = get_frame(this->getFrameHolder(), fOptions.fFrameIndex);
    SkSampler* sampler = this->getSampler(false);

    RustFillIncompletePlan plan = skia_rs_codec_plan_fill_incomplete_image(
            static_cast<int32_t>(info.colorType()),
            info.width(),
            rowBytes,
            static_cast<int32_t>(zeroInit),
            linesRequested,
            linesDecoded,
            frame != nullptr,
            frame ? frame->getRequiredFrame() : kNoFrame,
            frame ? (frame->frameRect() == this->bounds()) : true,
            sampler ? sampler->fillWidth() : -1,
            fOptions.fSubset ? fOptions.fSubset->width() : -1,
            static_cast<int32_t>(this->getScanlineOrder()));
    if (!plan.should_fill) {
        return;
    }

    void* fillDst = SkTAddOffset<void>(dst, plan.byte_offset);
    const auto fillInfo = info.makeWH(plan.fill_width, plan.lines_remaining);
    SkSampler::Fill(fillInfo, fillDst, rowBytes, kNo_ZeroInitialized);
}

bool SkCodecPriv::SelectXformFormat(SkColorType colorType,
                                    bool forColorTable,
                                    skcms_PixelFormat* outFormat) {
    SkASSERT(outFormat);
    int32_t fmt = 0;
    if (!skia_rs_codec_select_xform_format(
                static_cast<int32_t>(colorType), forColorTable, kPmColorIsRgba, &fmt)) {
        return false;
    }
    *outFormat = static_cast<skcms_PixelFormat>(fmt);
    return true;
}

bool SkCodec::initializeColorXform(const SkImageInfo& dstInfo,
                                   SkEncodedInfo::Alpha encodedAlpha,
                                   bool srcIsOpaque) {
    fXformTime = kNo_XformTime;
    bool needsColorXform = false;
    if (this->usesColorXform()) {
        if (skia_rs_codec_color_type_forces_xform(static_cast<int32_t>(dstInfo.colorType()))) {
            needsColorXform = true;
            if (dstInfo.colorSpace()) {
                dstInfo.colorSpace()->toProfile(&fDstProfileStorage);
                fDstProfile = &fDstProfileStorage;
            } else {
                const auto* srcProfile = fEncodedInfo.profile();
                fDstProfile = srcProfile ? srcProfile : skcms_sRGB_profile();
            }
        } else if (dstInfo.colorSpace()) {
            dstInfo.colorSpace()->toProfile(&fDstProfileStorage);
            fDstProfile = &fDstProfileStorage;
            const auto* srcProfile = fEncodedInfo.profile();
            if (!srcProfile) {
                srcProfile = skcms_sRGB_profile();
            }
            if (!skcms_ApproximatelyEqualProfiles(srcProfile, fDstProfile)) {
                needsColorXform = true;
            }
        }
    }

    if (!this->conversionSupported(dstInfo, srcIsOpaque, needsColorXform)) {
        return false;
    }

    if (needsColorXform) {
        RustColorXformPlan plan = skia_rs_codec_finalize_color_xform(
                static_cast<int32_t>(fEncodedInfo.color()),
                static_cast<int32_t>(encodedAlpha),
                static_cast<int32_t>(dstInfo.colorType()),
                static_cast<int32_t>(dstInfo.alphaType()),
                kPmColorIsRgba);
        fXformTime = static_cast<XformTime>(plan.xform_time);
        if (!plan.success) {
            return false;
        }
        fDstXformFormat = static_cast<XformFormat>(plan.dst_xform_format);
        fDstXformAlphaFormat = static_cast<skcms_AlphaFormat>(plan.dst_xform_alpha_format);
    }
    return true;
}

void SkCodec::applyColorXform(void* dst, const void* src, int count) const {
    const auto* srcProfile = fEncodedInfo.profile();
    SkAssertResult(skcms_Transform(src,
                                   fSrcXformFormat,
                                   skcms_AlphaFormat_Unpremul,
                                   srcProfile,
                                   dst,
                                   fDstXformFormat,
                                   fDstXformAlphaFormat,
                                   fDstProfile,
                                   count));
}

std::vector<SkCodec::FrameInfo> SkCodec::getFrameInfo() {
    const int frameCount = this->getFrameCount();
    SkASSERT(frameCount >= 0);
    if (frameCount <= 0) {
        return std::vector<FrameInfo>{};
    }

    if (frameCount == 1 && !this->onGetFrameInfo(0, nullptr)) {
        return std::vector<FrameInfo>{};
    }

    std::vector<FrameInfo> result(frameCount);
    for (int i = 0; i < frameCount; ++i) {
        SkAssertResult(this->onGetFrameInfo(i, &result[i]));
    }
    return result;
}

const char* SkCodec::ResultToString(Result result) {
    return skia_rs_codec_result_to_string(static_cast<int32_t>(result));
}

void SkFrame::fillIn(SkCodec::FrameInfo* frameInfo, bool fullyReceived) const {
    SkASSERT(frameInfo);
    RustFrameInfoFields f = skia_rs_frame_fill_in(fRequiredFrame,
                                                  fDuration,
                                                  fullyReceived,
                                                  fHasAlpha,
                                                  static_cast<int32_t>(this->reportedAlpha()),
                                                  static_cast<int32_t>(fDisposalMethod),
                                                  static_cast<int32_t>(fBlend),
                                                  fRect);
    frameInfo->fRequiredFrame = f.required_frame;
    frameInfo->fDuration = f.duration;
    frameInfo->fFullyReceived = f.fully_received;
    frameInfo->fAlphaType = static_cast<SkAlphaType>(f.alpha_type);
    frameInfo->fHasAlphaWithinBounds = f.has_alpha_within_bounds;
    frameInfo->fDisposalMethod = static_cast<SkCodecAnimation::DisposalMethod>(f.disposal_method);
    frameInfo->fBlend = static_cast<SkCodecAnimation::Blend>(f.blend);
    frameInfo->fFrameRect = f.frame_rect;
}

static RustFrameSummary make_frame_summary(const SkFrame* frame, bool readRequiredFrame) {
    return RustFrameSummary{
            frame->frameId(),
            static_cast<int32_t>(frame->reportedAlpha()),
            frame->hasAlpha(),
            readRequiredFrame ? frame->getRequiredFrame() : SkCodec::kNoFrame,
            static_cast<int32_t>(frame->getDisposalMethod()),
            static_cast<int32_t>(frame->getBlend()),
            frame->frameRect(),
    };
}

static RustFrameSummary frame_holder_get_frame_cb(const void* ctx, int32_t id) {
    const auto* holder = static_cast<const SkFrameHolder*>(ctx);
    const SkFrame* prev = holder->getFrame(id);
    return make_frame_summary(prev, true);
}

void SkFrameHolder::setAlphaAndRequiredFrame(SkFrame* frame) {
    RustFrameSummary curr = make_frame_summary(frame, false);
    RustFrameDependencyResult res = skia_rs_frame_holder_compute_dependency(
            fScreenWidth, fScreenHeight, curr, this, &frame_holder_get_frame_cb);
    frame->setHasAlpha(res.has_alpha);
    frame->setRequiredFrame(res.required_frame);
}

sk_sp<const SkData> SkCodec::getEncodedData() const {
    SkASSERT(fStream);
    if (!fStream) {
        return nullptr;
    }
    sk_sp<const SkData> data = fStream->getData();
    if (data) {
        return data;
    }
    auto dStream = fStream->duplicate();
    if (!dStream->hasLength()) {
        return nullptr;
    }
    return SkData::MakeFromStream(dStream.get(), dStream->getLength());
}

// ============================================================================
// SkPixmapUtils.cpp
// ============================================================================

static bool draw_orientation(const SkPixmap& dst, const SkPixmap& src, SkEncodedOrigin origin) {
    auto surf = SkSurfaces::WrapPixels(dst.info(), dst.writable_addr(), dst.rowBytes());
    if (!surf) {
        return false;
    }

    SkBitmap bm;
    bm.installPixels(src);

    SkMatrix m = SkEncodedOriginToMatrix(origin, dst.width(), dst.height());

    SkPaint p;
    p.setBlendMode(SkBlendMode::kSrc);
    surf->getCanvas()->concat(m);
    surf->getCanvas()->drawImage(SkImages::RasterFromBitmap(bm), 0, 0, SkSamplingOptions(), &p);
    return true;
}

namespace SkPixmapUtils {

bool Orient(const SkPixmap& dst, const SkPixmap& src, SkEncodedOrigin origin) {
    int32_t action = skia_rs_pixmap_utils_validate_orient(static_cast<int32_t>(src.colorType()),
                                                          static_cast<int32_t>(dst.colorType()),
                                                          src.width(),
                                                          src.height(),
                                                          dst.width(),
                                                          dst.height(),
                                                          static_cast<int32_t>(origin),
                                                          src.addr() == dst.addr());
    if (action == 0) {
        return false;
    }
    if (action == 1) {
        return true;
    }
    return draw_orientation(dst, src, origin);
}

SkImageInfo SwapWidthHeight(const SkImageInfo& info) {
    SkISize swapped = skia_rs_pixmap_utils_swap_wh(info.dimensions());
    return info.makeWH(swapped.width(), swapped.height());
}

}  // namespace SkPixmapUtils

// ============================================================================
// SkCodecImageGenerator.cpp
// ============================================================================

std::unique_ptr<SkImageGenerator> SkCodecImageGenerator::MakeFromEncodedCodec(
        sk_sp<const SkData> data, std::optional<SkAlphaType> at) {
    auto codec = SkCodec::MakeFromData(data);
    if (codec == nullptr) {
        return nullptr;
    }
    return std::unique_ptr<SkImageGenerator>(new SkCodecImageGenerator(std::move(codec), at));
}

std::unique_ptr<SkImageGenerator> SkCodecImageGenerator::MakeFromCodec(
        std::unique_ptr<SkCodec> codec, std::optional<SkAlphaType> at) {
    return codec ? std::unique_ptr<SkImageGenerator>(
                           new SkCodecImageGenerator(std::move(codec), at))
                 : nullptr;
}

static SkImageInfo adjust_info(SkCodec* codec, std::optional<SkAlphaType> at) {
    SkASSERT(at != kOpaque_SkAlphaType);
    SkImageInfo info = codec->getInfo();
    RustGeneratorInfoAdjust adj = skia_rs_codec_adjust_generator_info(
            info.width(),
            info.height(),
            static_cast<int32_t>(info.alphaType()),
            static_cast<int32_t>(codec->getOrigin()),
            at.has_value(),
            at.has_value() ? static_cast<int32_t>(*at) : 0);
    return info.makeWH(adj.width, adj.height)
            .makeAlphaType(static_cast<SkAlphaType>(adj.alpha_type));
}

SkCodecImageGenerator::SkCodecImageGenerator(std::unique_ptr<SkCodec> codec,
                                             std::optional<SkAlphaType> at)
        : SkImageGenerator(adjust_info(codec.get(), at)), fCodec(std::move(codec)) {}

sk_sp<const SkData> SkCodecImageGenerator::onRefEncodedData() {
    SkASSERT(fCodec);
    if (!fCachedData) {
        fCachedData = SkCodecPriv::GetEncodedData(fCodec.get());
    }
    return fCachedData;
}

bool SkCodecImageGenerator::getPixels(const SkImageInfo& info,
                                      void* pixels,
                                      size_t rowBytes,
                                      const SkCodec::Options* options) {
    SkPixmap dst(info, pixels, rowBytes);
    auto decode = [this, options](const SkPixmap& pm) {
        SkCodec::Result result = fCodec->getPixels(pm, options);
        return skia_rs_codec_is_partial_or_full_success(static_cast<int32_t>(result));
    };
    return SkPixmapUtils::Orient(dst, fCodec->getOrigin(), decode);
}

bool SkCodecImageGenerator::onGetPixels(const SkImageInfo& requestInfo,
                                        void* requestPixels,
                                        size_t requestRowBytes,
                                        const Options& options) {
    return this->getPixels(requestInfo, requestPixels, requestRowBytes, nullptr);
}

bool SkCodecImageGenerator::onQueryYUVAInfo(
        const SkYUVAPixmapInfo::SupportedDataTypes& supportedDataTypes,
        SkYUVAPixmapInfo* yuvaPixmapInfo) const {
    return fCodec->queryYUVAInfo(supportedDataTypes, yuvaPixmapInfo);
}

bool SkCodecImageGenerator::onGetYUVAPlanes(const SkYUVAPixmaps& yuvaPixmaps) {
    return skia_rs_codec_is_partial_or_full_success(
            static_cast<int32_t>(fCodec->getYUVAPlanes(yuvaPixmaps)));
}

SkISize SkCodecImageGenerator::getScaledDimensions(float desiredScale) const {
    SkISize size = fCodec->getScaledDimensions(desiredScale);
    if (SkEncodedOriginSwapsWidthHeight(fCodec->getOrigin())) {
        size = skia_rs_pixmap_utils_swap_wh(size);
    }
    return size;
}

// ============================================================================
// SkImageGenerator_FromEncoded.cpp
// ============================================================================

static SkGraphics::ImageGeneratorFromEncodedDataFactory gFactory;

SkGraphics::ImageGeneratorFromEncodedDataFactory
SkGraphics::SetImageGeneratorFromEncodedDataFactory(
        ImageGeneratorFromEncodedDataFactory factory) {
    ImageGeneratorFromEncodedDataFactory prev = gFactory;
    gFactory = factory;
    return prev;
}

namespace SkImageGenerators {

std::unique_ptr<SkImageGenerator> MakeFromEncoded(sk_sp<const SkData> data,
                                                  std::optional<SkAlphaType> at) {
    if (!data || at == kOpaque_SkAlphaType) {
        return nullptr;
    }
    if (gFactory) {
        if (std::unique_ptr<SkImageGenerator> generator = gFactory(data)) {
            return generator;
        }
    }
    return SkCodecImageGenerator::MakeFromEncodedCodec(std::move(data), at);
}

}  // namespace SkImageGenerators

namespace SkImages {

sk_sp<SkImage> DeferredFromEncodedData(sk_sp<const SkData> encoded,
                                       std::optional<SkAlphaType> alphaType) {
    if (nullptr == encoded || encoded->empty()) {
        return nullptr;
    }
    return DeferredFromGenerator(SkImageGenerators::MakeFromEncoded(std::move(encoded), alphaType));
}

}  // namespace SkImages

namespace SkCodecs {

sk_sp<SkImage> DeferredImage(std::unique_ptr<SkCodec> codec,
                             std::optional<SkAlphaType> alphaType) {
    return SkImages::DeferredFromGenerator(
            SkCodecImageGenerator::MakeFromCodec(std::move(codec), alphaType));
}

}  // namespace SkCodecs

// ============================================================================
// SkEncoder.cpp
// ============================================================================

bool SkEncoder::encodeRows(int numRows) {
    SkASSERT(numRows > 0 && fCurrRow < fSrc.height());
    int32_t clamped = 0;
    if (!skia_rs_encoder_clamp_rows(fCurrRow, fSrc.height(), numRows, &clamped)) {
        return false;
    }

    if (!this->onEncodeRows(clamped)) {
        fCurrRow = fSrc.height();
        return false;
    }

    return true;
}
