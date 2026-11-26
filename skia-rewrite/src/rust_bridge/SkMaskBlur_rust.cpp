/*
 * Safe Rust bridge for Layer 2.4 skia-rs-core-types:
 *   SkMask.cpp, SkBlurMask.cpp, SkGaussFilter.cpp, SkMaskGamma.cpp, SkMaskCache.cpp.
 */

#include "include/core/SkBlurTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRect.h"
#include "include/core/SkSize.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkMath.h"
#include "include/private/SkSafe32.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTPin.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkBlurMask.h"
#include "src/core/SkCachedData.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkGaussFilter.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskBlurFilter.h"
#include "src/core/SkMaskCache.h"
#include "src/core/SkMaskGamma.h"
#include "src/core/SkMathPriv.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkTLazy.h"
#include "src/partition_alloc/raw_ptr.h"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>

class SkDiscardableMemory;

extern "C" {

// SkMask & SkMaskCache
struct SkiaRsPreparedMaskDest {
    SkIRect  bounds;
    uint32_t row_bytes;
    size_t   to_alloc;
    bool     valid;
};

size_t skia_rs_mask_compute_image_size(const SkIRect* bounds, uint32_t row_bytes);
size_t skia_rs_mask_compute_total_image_size(const SkIRect* bounds, uint32_t row_bytes,
                                             uint8_t format);
size_t skia_rs_mask_alloc_image_aligned_size(size_t size);
bool skia_rs_mask_get_addr_offset(const SkIRect* bounds, uint32_t row_bytes, uint8_t format,
                                  int32_t x, int32_t y, ptrdiff_t* out_offset);
SkiaRsPreparedMaskDest skia_rs_mask_prepare_destination(int32_t radius_x, int32_t radius_y,
                                                        const SkIRect* src_bounds);
void skia_rs_mask_rects_blur_key_sizes(const SkRect* rects, size_t count, SkSize out_sizes[4]);

// SkBlurMask
float skia_rs_blur_mask_convert_radius_to_sigma(float radius);
float skia_rs_blur_mask_convert_sigma_to_radius(float sigma);
void skia_rs_blur_mask_merge_src_with_blur(uint8_t* dst, size_t dst_rb,
                                           const uint8_t* src, size_t src_rb, uint8_t src_format,
                                           const uint8_t* blur, size_t blur_rb,
                                           size_t sw, size_t sh, uint32_t a32_shift);
void skia_rs_blur_mask_clamp_solid(uint8_t* dst, size_t dst_rb,
                                   const uint8_t* src, size_t src_rb, uint8_t src_format,
                                   size_t sw, size_t sh, uint32_t a32_shift);
void skia_rs_blur_mask_clamp_outer(uint8_t* dst, size_t dst_rb,
                                   const uint8_t* src, size_t src_rb, uint8_t src_format,
                                   size_t sw, size_t sh, uint32_t a32_shift);
void skia_rs_blur_mask_compute_blur_profile(uint8_t* profile, size_t size, float sigma);
uint8_t skia_rs_blur_mask_profile_lookup(const uint8_t* profile, int32_t loc,
                                         int32_t blurred_width, int32_t sharp_width);
void skia_rs_blur_mask_compute_blurred_scanline(uint8_t* pixels, const uint8_t* profile,
                                                size_t width, float sigma);
void skia_rs_blur_mask_blur_rect_render_full(uint8_t* dst, size_t dst_w, size_t dst_h,
                                             float sigma, int32_t style, size_t pad, size_t sw);
void skia_rs_blur_mask_crop_inner_blur_rect(const uint8_t* full_blur, size_t dst_w, size_t dst_h,
                                            size_t pad, uint8_t* inner_dst, size_t sw, size_t sh);
int32_t skia_rs_blur_mask_ground_truth_pad(float sigma);
void skia_rs_blur_mask_ground_truth_convolve(float sigma,
                                             const uint8_t* src, size_t src_w, size_t src_h,
                                             size_t src_rb,
                                             uint8_t* dst, size_t dst_w, size_t dst_h,
                                             int32_t style);

// SkGaussFilter
int32_t skia_rs_gauss_filter_calculate_bessel_factors(double sigma, double basis[6]);

// SkMaskGamma
uint8_t skia_rs_mask_gamma_mode_from_gamma(float gamma);
float skia_rs_mask_gamma_to_luma(uint8_t mode, float gamma, float luminance);
float skia_rs_mask_gamma_from_luma(uint8_t mode, float gamma, float luma);
float skia_rs_mask_gamma_apply_contrast(float srca, float contrast);
void skia_rs_mask_gamma_build_correcting_lut(uint8_t table[256], uint8_t src_i, float contrast,
                                             uint8_t mode, float dst_gamma);

}  // extern "C"

static_assert(sizeof(SkIRect) == 16, "SkIRect layout mismatch");
static_assert(sizeof(SkRect) == 16, "SkRect layout mismatch");
static_assert(sizeof(SkSize) == 8, "SkSize layout mismatch");
static_assert(SkGaussFilter::kGaussArrayMax == 6, "SkGaussFilter::kGaussArrayMax mismatch");
static_assert(offsetof(SkiaRsPreparedMaskDest, bounds) == 0, "PreparedMaskDest::bounds offset");
static_assert(offsetof(SkiaRsPreparedMaskDest, row_bytes) == 16, "PreparedMaskDest::row_bytes offset");

// ============================================================================
// SkMask & SkMaskBuilder
// ============================================================================

size_t SkMask::computeImageSize() const {
    return skia_rs_mask_compute_image_size(&fBounds, fRowBytes);
}

size_t SkMask::computeTotalImageSize() const {
    return skia_rs_mask_compute_total_image_size(&fBounds, fRowBytes, static_cast<uint8_t>(fFormat));
}

uint8_t* SkMaskBuilder::AllocImage(size_t size, AllocType at) {
    size_t aligned_size = skia_rs_mask_alloc_image_aligned_size(size);
    unsigned flags = SK_MALLOC_THROW;
    if (at == kZeroInit_Alloc) {
        flags |= SK_MALLOC_ZERO_INITIALIZE;
    }
    return static_cast<uint8_t*>(sk_malloc_flags(aligned_size, flags));
}

void SkMaskBuilder::FreeImage(void* image) {
    sk_free(image);
}

SkMaskBuilder SkMaskBuilder::PrepareDestination(int radiusX, int radiusY, const SkMask& src) {
    SkMaskBuilder dst;
    dst.image() = nullptr;
    dst.format() = SkMask::kA8_Format;

    SkiaRsPreparedMaskDest prepared =
            skia_rs_mask_prepare_destination(radiusX, radiusY, &src.fBounds);
    if (!prepared.valid) {
        dst.bounds().setEmpty();
        dst.rowBytes() = 0;
        return dst;
    }

    dst.bounds() = prepared.bounds;
    dst.rowBytes() = prepared.row_bytes;

    if (src.fImage != nullptr) {
        dst.image() = SkMaskBuilder::AllocImage(prepared.to_alloc);
    }

    return dst;
}

const void* SkMask::getAddr(int x, int y) const {
    SkASSERT(kBW_Format != fFormat);
    SkASSERT(fBounds.contains(x, y));
    SkASSERT(fImage);

    ptrdiff_t offset = 0;
    SkAssertResult(skia_rs_mask_get_addr_offset(
            &fBounds, fRowBytes, static_cast<uint8_t>(fFormat), x, y, &offset));
    return reinterpret_cast<const char*>(fImage) + offset;
}

// ============================================================================
// SkBlurMask
// ============================================================================

SkScalar SkBlurMask::ConvertRadiusToSigma(SkScalar radius) {
    return skia_rs_blur_mask_convert_radius_to_sigma(radius);
}

SkScalar SkBlurMask::ConvertSigmaToRadius(SkScalar sigma) {
    return skia_rs_blur_mask_convert_sigma_to_radius(sigma);
}

bool SkBlurMask::BoxBlur(SkMaskBuilder* dst,
                         const SkMask& src,
                         SkScalar sigma,
                         SkBlurStyle style,
                         SkIVector* margin) {
    SkASSERT(dst);
    if (src.fFormat != SkMask::kBW_Format &&
        src.fFormat != SkMask::kA8_Format &&
        src.fFormat != SkMask::kARGB32_Format &&
        src.fFormat != SkMask::kLCD16_Format) {
        return false;
    }

    SkMaskBlurFilter blurFilter{sigma, sigma};
    if (blurFilter.hasNoBlur()) {
        if (style == kOuter_SkBlurStyle) {
            dst->image() = nullptr;
            dst->bounds() = SkIRect::MakeEmpty();
            dst->rowBytes() = dst->fBounds.width();
            dst->format() = SkMask::kA8_Format;
            if (margin != nullptr) {
                *margin = SkIVector{0, 0};
            }
            return true;
        }
        return false;
    }
    const SkIVector border = blurFilter.blur(src, dst);

    if (src.fImage != nullptr && dst->fImage == nullptr) {
        return false;
    }

    if (margin != nullptr) {
        *margin = border;
    }

    if (src.fImage == nullptr) {
        if (style == kInner_SkBlurStyle) {
            dst->bounds() = src.fBounds;
            dst->rowBytes() = dst->fBounds.width();
        }
        return true;
    }

    const size_t sw = static_cast<size_t>(src.fBounds.width());
    const size_t sh = static_cast<size_t>(src.fBounds.height());
    const uint8_t srcFmt = static_cast<uint8_t>(src.fFormat);

    switch (style) {
        case kNormal_SkBlurStyle:
            break;
        case kSolid_SkBlurStyle: {
            auto dstStart = &dst->image()[border.x() + border.y() * dst->fRowBytes];
            skia_rs_blur_mask_clamp_solid(
                    dstStart, dst->fRowBytes, src.fImage, src.fRowBytes, srcFmt,
                    sw, sh, SK_A32_SHIFT);
        } break;
        case kOuter_SkBlurStyle: {
            auto dstStart = &dst->image()[border.x() + border.y() * dst->fRowBytes];
            skia_rs_blur_mask_clamp_outer(
                    dstStart, dst->fRowBytes, src.fImage, src.fRowBytes, srcFmt,
                    sw, sh, SK_A32_SHIFT);
        } break;
        case kInner_SkBlurStyle: {
            SkMaskBuilder blur = std::move(*dst);
            SkAutoMaskFreeImage autoFreeBlurMask(blur.image());

            *dst = SkMaskBuilder(nullptr, src.fBounds, src.fBounds.width(), blur.format());
            size_t dstSize = dst->computeImageSize();
            if (0 == dstSize) {
                return false;
            }
            dst->image() = SkMaskBuilder::AllocImage(dstSize);

            auto blurStart = &blur.image()[border.x() + border.y() * blur.fRowBytes];
            skia_rs_blur_mask_merge_src_with_blur(
                    dst->image(), dst->fRowBytes,
                    src.fImage, src.fRowBytes, srcFmt,
                    blurStart, blur.fRowBytes,
                    sw, sh, SK_A32_SHIFT);
        } break;
    }

    return true;
}

void SkBlurMask::ComputeBlurProfile(uint8_t* profile, int size, SkScalar sigma) {
    SkASSERT(SkScalarCeilToInt(6 * sigma) == size);
    skia_rs_blur_mask_compute_blur_profile(profile, static_cast<size_t>(size), sigma);
}

uint8_t SkBlurMask::ProfileLookup(const uint8_t* profile, int loc,
                                  int blurredWidth, int sharpWidth) {
    return skia_rs_blur_mask_profile_lookup(profile, loc, blurredWidth, sharpWidth);
}

void SkBlurMask::ComputeBlurredScanline(uint8_t* pixels, const uint8_t* profile,
                                        unsigned int width, SkScalar sigma) {
    skia_rs_blur_mask_compute_blurred_scanline(pixels, profile, static_cast<size_t>(width), sigma);
}

bool SkBlurMask::BlurRect(SkScalar sigma,
                          SkMaskBuilder* dst,
                          const SkRect& src,
                          SkBlurStyle style,
                          SkIVector* margin,
                          SkMaskBuilder::CreateMode createMode) {
    int profileSize = SkScalarCeilToInt(6 * sigma);
    if (profileSize <= 0) {
        return false;
    }

    int pad = profileSize / 2;
    if (margin) {
        margin->set(pad, pad);
    }

    dst->bounds().setLTRB(SkScalarRoundToInt(src.fLeft - pad),
                          SkScalarRoundToInt(src.fTop - pad),
                          SkScalarRoundToInt(src.fRight + pad),
                          SkScalarRoundToInt(src.fBottom + pad));

    dst->rowBytes() = dst->fBounds.width();
    dst->format() = SkMask::kA8_Format;
    dst->image() = nullptr;

    int sw = SkScalarFloorToInt(src.width());
    int sh = SkScalarFloorToInt(src.height());

    if (createMode == SkMaskBuilder::kJustComputeBounds_CreateMode) {
        if (style == kInner_SkBlurStyle) {
            dst->bounds() = src.round();
            dst->rowBytes() = sw;
        }
        return true;
    }

    size_t dstSize = dst->computeImageSize();
    if (0 == dstSize) {
        return false;
    }

    uint8_t* dp = SkMaskBuilder::AllocImage(dstSize);
    dst->image() = dp;

    int dstHeight = dst->fBounds.height();
    int dstWidth = dst->fBounds.width();

    skia_rs_blur_mask_blur_rect_render_full(
            dp,
            static_cast<size_t>(dstWidth),
            static_cast<size_t>(dstHeight),
            sigma,
            static_cast<int32_t>(style),
            static_cast<size_t>(pad),
            sw > 0 ? static_cast<size_t>(sw) : 0);

    if (style == kInner_SkBlurStyle) {
        size_t srcSize = (size_t)(src.width() * src.height());
        if (0 == srcSize) {
            SkMaskBuilder::FreeImage(dp);
            dst->image() = nullptr;
            return false;
        }
        dst->image() = SkMaskBuilder::AllocImage(srcSize);
        if (sw > 0 && sh > 0) {
            skia_rs_blur_mask_crop_inner_blur_rect(
                    dp,
                    static_cast<size_t>(dstWidth),
                    static_cast<size_t>(dstHeight),
                    static_cast<size_t>(pad),
                    dst->image(),
                    static_cast<size_t>(sw),
                    static_cast<size_t>(sh));
        }
        SkMaskBuilder::FreeImage(dp);

        dst->bounds() = src.round();
        dst->rowBytes() = sw;
    }

    return true;
}

bool SkBlurMask::BlurGroundTruth(SkScalar sigma,
                                 SkMaskBuilder* dst,
                                 const SkMask& src,
                                 SkBlurStyle style,
                                 SkIVector* margin) {
    if (src.fFormat != SkMask::kA8_Format) {
        return false;
    }

    int pad = skia_rs_blur_mask_ground_truth_pad(sigma);
    if (margin) {
        margin->set(pad, pad);
    }

    dst->bounds() = src.fBounds;
    dst->bounds().outset(pad, pad);

    dst->rowBytes() = dst->fBounds.width();
    dst->format() = SkMask::kA8_Format;
    dst->image() = nullptr;

    if (src.fImage) {
        size_t dstSize = dst->computeImageSize();
        if (0 == dstSize) {
            return false;
        }

        int srcWidth = src.fBounds.width();
        int srcHeight = src.fBounds.height();
        int dstWidth = dst->fBounds.width();
        int dstHeight = dst->fBounds.height();

        const uint8_t* srcPixels = src.fImage;
        uint8_t* dstPixels = SkMaskBuilder::AllocImage(dstSize);
        SkAutoMaskFreeImage autoFreeDstPixels(dstPixels);

        skia_rs_blur_mask_ground_truth_convolve(
                sigma,
                srcPixels,
                static_cast<size_t>(srcWidth),
                static_cast<size_t>(srcHeight),
                src.fRowBytes,
                dstPixels,
                static_cast<size_t>(dstWidth),
                static_cast<size_t>(dstHeight),
                static_cast<int32_t>(style));

        dst->image() = dstPixels;
        if (style == kInner_SkBlurStyle) {
            size_t srcSize = src.computeImageSize();
            if (0 == srcSize) {
                dst->image() = nullptr;
                return false;
            }
            dst->image() = SkMaskBuilder::AllocImage(srcSize);
            skia_rs_blur_mask_merge_src_with_blur(
                    dst->image(), src.fRowBytes,
                    srcPixels, src.fRowBytes, static_cast<uint8_t>(SkMask::kA8_Format),
                    dstPixels + pad * dst->fRowBytes + pad, dst->fRowBytes,
                    static_cast<size_t>(srcWidth), static_cast<size_t>(srcHeight),
                    SK_A32_SHIFT);
            SkMaskBuilder::FreeImage(dstPixels);
        }
        autoFreeDstPixels.release();
    }

    if (style == kInner_SkBlurStyle) {
        dst->bounds() = src.fBounds;
        dst->rowBytes() = src.fRowBytes;
    }

    return true;
}

// ============================================================================
// SkGaussFilter
// ============================================================================

SkGaussFilter::SkGaussFilter(double sigma) {
    SkASSERT(0 <= sigma && sigma < 2);
    fN = skia_rs_gauss_filter_calculate_bessel_factors(sigma, fBasis.data());
}

// ============================================================================
// SkMaskGamma
// ============================================================================

namespace {

constexpr uint8_t kLuminanceMode_SRgb   = 0;
constexpr uint8_t kLuminanceMode_Linear = 1;
constexpr uint8_t kLuminanceMode_Gamma  = 2;

class SkLinearColorSpaceLuminance : public SkColorSpaceLuminance {
    SkScalar toLuma(SkScalar SkDEBUGCODE(gamma), SkScalar luminance) const override {
        SkASSERT(SK_Scalar1 == gamma);
        return skia_rs_mask_gamma_to_luma(kLuminanceMode_Linear, 1.0f, luminance);
    }
    SkScalar fromLuma(SkScalar SkDEBUGCODE(gamma), SkScalar luma) const override {
        SkASSERT(SK_Scalar1 == gamma);
        return skia_rs_mask_gamma_from_luma(kLuminanceMode_Linear, 1.0f, luma);
    }
};

class SkGammaColorSpaceLuminance : public SkColorSpaceLuminance {
    SkScalar toLuma(SkScalar gamma, SkScalar luminance) const override {
        return skia_rs_mask_gamma_to_luma(kLuminanceMode_Gamma, gamma, luminance);
    }
    SkScalar fromLuma(SkScalar gamma, SkScalar luma) const override {
        return skia_rs_mask_gamma_from_luma(kLuminanceMode_Gamma, gamma, luma);
    }
};

class SkSRGBColorSpaceLuminance : public SkColorSpaceLuminance {
    SkScalar toLuma(SkScalar SkDEBUGCODE(gamma), SkScalar luminance) const override {
        SkASSERT(0 == gamma);
        return skia_rs_mask_gamma_to_luma(kLuminanceMode_SRgb, 0.0f, luminance);
    }
    SkScalar fromLuma(SkScalar SkDEBUGCODE(gamma), SkScalar luma) const override {
        SkASSERT(0 == gamma);
        return skia_rs_mask_gamma_from_luma(kLuminanceMode_SRgb, 0.0f, luma);
    }
};

static SkLinearColorSpaceLuminance gSkLinearColorSpaceLuminance;
static SkGammaColorSpaceLuminance gSkGammaColorSpaceLuminance;
static SkSRGBColorSpaceLuminance gSkSRGBColorSpaceLuminance;

}  // namespace

/*static*/ const SkColorSpaceLuminance& SkColorSpaceLuminance::Fetch(SkScalar gamma) {
    uint8_t mode = skia_rs_mask_gamma_mode_from_gamma(gamma);
    if (mode == kLuminanceMode_SRgb) {
        return gSkSRGBColorSpaceLuminance;
    } else if (mode == kLuminanceMode_Linear) {
        return gSkLinearColorSpaceLuminance;
    } else {
        return gSkGammaColorSpaceLuminance;
    }
}

void SkTMaskGamma_build_correcting_lut(uint8_t* table, U8CPU srcI, SkScalar contrast,
                                       const SkColorSpaceLuminance& dstConvert, SkScalar dstGamma) {
    if (&dstConvert == &gSkSRGBColorSpaceLuminance) {
        skia_rs_mask_gamma_build_correcting_lut(
                table, SkToU8(srcI), contrast, kLuminanceMode_SRgb, dstGamma);
        return;
    }
    if (&dstConvert == &gSkLinearColorSpaceLuminance) {
        skia_rs_mask_gamma_build_correcting_lut(
                table, SkToU8(srcI), contrast, kLuminanceMode_Linear, dstGamma);
        return;
    }
    if (&dstConvert == &gSkGammaColorSpaceLuminance) {
        skia_rs_mask_gamma_build_correcting_lut(
                table, SkToU8(srcI), contrast, kLuminanceMode_Gamma, dstGamma);
        return;
    }

    // Fallback for custom SkColorSpaceLuminance subclasses.
    const float src = (float)srcI / 255.0f;
    const float linSrc = dstConvert.toLuma(dstGamma, src);
    const float dst = 1.0f - src;
    const float linDst = dstConvert.toLuma(dstGamma, dst);
    const float adjustedContrast = contrast * linDst;

    if (fabs(src - dst) < (1.0f / 256.0f)) {
        float ii = 0.0f;
        for (int i = 0; i < 256; ++i, ii += 1.0f) {
            float rawSrca = ii / 255.0f;
            float srca = skia_rs_mask_gamma_apply_contrast(rawSrca, adjustedContrast);
            table[i] = SkToU8(sk_float_round2int(255.0f * srca));
        }
    } else {
        float ii = 0.0f;
        for (int i = 0; i < 256; ++i, ii += 1.0f) {
            float rawSrca = ii / 255.0f;
            float srca = skia_rs_mask_gamma_apply_contrast(rawSrca, adjustedContrast);
            float dsta = 1.0f - srca;
            float linOut = linSrc * srca + dsta * linDst;
            float out = dstConvert.fromLuma(dstGamma, linOut);
            float result = (out - dst) / (src - dst);
            table[i] = SkToU8(sk_float_round2int(255.0f * result));
        }
    }
}

// ============================================================================
// SkMaskCache
// ============================================================================

#define CHECK_LOCAL(localCache, localName, globalName, ...) \
    ((localCache) ? localCache->localName(__VA_ARGS__) : SkResourceCache::globalName(__VA_ARGS__))

namespace {

struct MaskValue {
    SkMask                                   fMask;
    raw_ptr<SkCachedData, DanglingUntriaged> fData;
};

static unsigned gRRectBlurKeyNamespaceLabel;

struct RRectBlurKey : public SkResourceCache::Key {
public:
    RRectBlurKey(SkScalar sigma, const SkRRect& rrect, SkBlurStyle style)
        : fSigma(sigma)
        , fStyle(style)
        , fRRect(rrect)
    {
        this->init(&gRRectBlurKeyNamespaceLabel, 0,
                   sizeof(fSigma) + sizeof(fStyle) + sizeof(fRRect));
    }

    SkScalar fSigma;
    int32_t  fStyle;
    SkRRect  fRRect;
};

struct RRectBlurRec : public SkResourceCache::Rec {
    RRectBlurRec(RRectBlurKey key, const SkMask& mask, SkCachedData* data)
        : fKey(key), fValue({{nullptr, mask.fBounds, mask.fRowBytes, mask.fFormat}, data})
    {
        fValue.fData->attachToCacheAndRef();
    }
    ~RRectBlurRec() override {
        fValue.fData->detachFromCacheAndUnref();
    }

    RRectBlurKey fKey;
    MaskValue    fValue;

    const Key& getKey() const override { return fKey; }
    size_t bytesUsed() const override { return sizeof(*this) + fValue.fData->size(); }
    const char* getCategory() const override { return "rrect-blur"; }
    SkDiscardableMemory* diagnostic_only_getDiscardable() const override {
        return fValue.fData->diagnostic_only_getDiscardable();
    }

    static bool Visitor(const SkResourceCache::Rec& baseRec, void* contextData) {
        const RRectBlurRec& rec = static_cast<const RRectBlurRec&>(baseRec);
        SkTLazy<MaskValue>* result = (SkTLazy<MaskValue>*)contextData;

        SkCachedData* tmpData = rec.fValue.fData;
        tmpData->ref();
        if (nullptr == tmpData->data()) {
            tmpData->unref();
            return false;
        }
        result->init(rec.fValue);
        return true;
    }
};

static unsigned gRectsBlurKeyNamespaceLabel;

struct RectsBlurKey : public SkResourceCache::Key {
public:
    RectsBlurKey(SkScalar sigma, SkBlurStyle style, SkSpan<const SkRect> rects)
            : fSigma(sigma), fStyle(style) {
        SkASSERT(rects.size() == 1 || rects.size() == 2);
        skia_rs_mask_rects_blur_key_sizes(rects.data(), rects.size(), fSizes);
        this->init(&gRectsBlurKeyNamespaceLabel, 0,
                   sizeof(fSigma) + sizeof(fStyle) + sizeof(fSizes));
    }

    SkScalar fSigma;
    int32_t  fStyle;
    SkSize   fSizes[4];
};

struct RectsBlurRec : public SkResourceCache::Rec {
    RectsBlurRec(RectsBlurKey key, const SkMask& mask, SkCachedData* data)
        : fKey(key), fValue({{nullptr, mask.fBounds, mask.fRowBytes, mask.fFormat}, data})
    {
        fValue.fData->attachToCacheAndRef();
    }
    ~RectsBlurRec() override {
        fValue.fData->detachFromCacheAndUnref();
    }

    RectsBlurKey fKey;
    MaskValue    fValue;

    const Key& getKey() const override { return fKey; }
    size_t bytesUsed() const override { return sizeof(*this) + fValue.fData->size(); }
    const char* getCategory() const override { return "rects-blur"; }
    SkDiscardableMemory* diagnostic_only_getDiscardable() const override {
        return fValue.fData->diagnostic_only_getDiscardable();
    }

    static bool Visitor(const SkResourceCache::Rec& baseRec, void* contextData) {
        const RectsBlurRec& rec = static_cast<const RectsBlurRec&>(baseRec);
        SkTLazy<MaskValue>* result = static_cast<SkTLazy<MaskValue>*>(contextData);

        SkCachedData* tmpData = rec.fValue.fData;
        tmpData->ref();
        if (nullptr == tmpData->data()) {
            tmpData->unref();
            return false;
        }
        result->init(rec.fValue);
        return true;
    }
};

}  // namespace

sk_sp<SkCachedData> SkMaskCache::Find(SkScalar sigma,
                                      SkBlurStyle style,
                                      const SkRRect& rrect,
                                      SkTLazy<SkMask>* mask,
                                      SkResourceCache* localCache) {
    SkTLazy<MaskValue> result;
    RRectBlurKey key(sigma, rrect, style);
    if (!CHECK_LOCAL(localCache, find, Find, key, RRectBlurRec::Visitor, &result)) {
        return nullptr;
    }

    mask->init(static_cast<const uint8_t*>(result->fData->data()),
               result->fMask.fBounds, result->fMask.fRowBytes, result->fMask.fFormat);
    return sk_sp<SkCachedData>(result->fData);
}

void SkMaskCache::Add(SkScalar sigma,
                      SkBlurStyle style,
                      const SkRRect& rrect,
                      const SkMask& mask,
                      sk_sp<SkCachedData> data,
                      SkResourceCache* localCache) {
    RRectBlurKey key(sigma, rrect, style);
    return CHECK_LOCAL(localCache, add, Add, new RRectBlurRec(key, mask, data.get()));
}

sk_sp<SkCachedData> SkMaskCache::Find(SkScalar sigma,
                                      SkBlurStyle style,
                                      SkSpan<const SkRect> rects,
                                      SkTLazy<SkMask>* mask,
                                      SkResourceCache* localCache) {
    SkTLazy<MaskValue> result;
    RectsBlurKey key(sigma, style, rects);
    if (!CHECK_LOCAL(localCache, find, Find, key, RectsBlurRec::Visitor, &result)) {
        return nullptr;
    }

    mask->init(static_cast<const uint8_t*>(result->fData->data()),
               result->fMask.fBounds, result->fMask.fRowBytes, result->fMask.fFormat);
    return sk_sp<SkCachedData>(result->fData);
}

void SkMaskCache::Add(SkScalar sigma,
                      SkBlurStyle style,
                      SkSpan<const SkRect> rects,
                      const SkMask& mask,
                      sk_sp<SkCachedData> data,
                      SkResourceCache* localCache) {
    RectsBlurKey key(sigma, style, rects);
    return CHECK_LOCAL(localCache, add, Add, new RectsBlurRec(key, mask, data.get()));
}
