/*
 * Safe Rust bridge for:
 *   src/core/SkBlurEngine.cpp
 * (Phase 1 Layer 4.3 skia-rs-effects, GN target :core).
 */

#include "src/core/SkBlurEngine.h"

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkClipOp.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTileMode.h"
#include "include/effects/SkRuntimeEffect.h"
#include "include/private/SkAssert.h"
#include "src/core/SkBitmapDevice.h"
#include "src/core/SkDevice.h"
#include "src/core/SkKnownRuntimeEffects.h"
#include "src/core/SkSpecialImage.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <utility>

namespace {

struct RsRenderBlurPlan {
    SkIRect fast_dst_rect;
    int32_t fast_tile_mode;
    int32_t strict_border_count;
    SkIRect strict_borders[4];
};

static_assert(sizeof(SkSize) == 8);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkV4) == 16);
static_assert(sizeof(RsRenderBlurPlan) == 88);

}  // namespace

extern "C" {
void skia_rs_blur_engine_compute_2d_kernel(SkSize sigma,
                                           SkISize radius,
                                           float* kernel,
                                           size_t kernel_len);
void skia_rs_blur_engine_compute_2d_offsets(SkISize radius, SkV4* offsets);
void skia_rs_blur_engine_compute_1d_linear_kernel(float sigma,
                                                  int32_t radius,
                                                  SkV4* offsets_and_kernel);
uint32_t skia_rs_blur_engine_stable_key_offset(int32_t kernel_width);
RsRenderBlurPlan skia_rs_blur_engine_plan_render_blur_rects(SkIRect src_rect,
                                                            SkISize backing_dims,
                                                            SkISize radii,
                                                            SkIRect dst_rect,
                                                            int32_t tile_mode);
bool skia_rs_blur_engine_plan_intermediate_dst(SkIRect src_rect,
                                               SkIRect dst_rect,
                                               int32_t radius_y,
                                               int32_t tile_mode,
                                               SkIRect* out_rect);
int32_t skia_rs_blur_engine_raster_a8_window(float sigma);
int32_t skia_rs_blur_engine_raster_8888_window(float sigma);
void skia_rs_blur_engine_eval_a8(SkSize sigma,
                                 const uint8_t* src,
                                 size_t src_len,
                                 size_t src_row_pixels,
                                 SkIRect orig_src_bounds,
                                 uint8_t* dst,
                                 size_t dst_len,
                                 size_t dst_row_pixels,
                                 SkIRect alloc_dst_bounds,
                                 SkIRect orig_dst_bounds);
void skia_rs_blur_engine_eval_8888(SkSize sigma,
                                   const uint8_t* src,
                                   size_t src_len,
                                   size_t src_row_pixels,
                                   SkIRect orig_src_bounds,
                                   uint8_t* dst,
                                   size_t dst_len,
                                   size_t dst_row_pixels,
                                   SkIRect alloc_dst_bounds,
                                   SkIRect orig_dst_bounds);
}  // extern "C"

namespace {

static sk_sp<SkSpecialImage> eval_raster_blur(SkSize sigma,
                                              sk_sp<SkSpecialImage> input,
                                              const SkIRect& originalSrcBounds,
                                              const SkIRect& originalDstBounds,
                                              bool isA8) {
    SkBitmap src;
    if (!SkSpecialImages::AsBitmap(input.get(), &src)) {
        return nullptr;
    }

    const int32_t winX = isA8 ? skia_rs_blur_engine_raster_a8_window(sigma.width())
                              : skia_rs_blur_engine_raster_8888_window(sigma.width());

    SkIRect dstBounds = originalDstBounds;
    if (winX > 1) {
        dstBounds.outset(0, SkBlurEngine::SigmaToRadius(sigma.height()));
    }

    SkBitmap dst;
    const SkIPoint dstOrigin = dstBounds.topLeft();
    SkImageInfo dstII = src.info().makeWH(dstBounds.width(), dstBounds.height());
    if (dstII.alphaType() == kOpaque_SkAlphaType) {
        dstII = dstII.makeAlphaType(kPremul_SkAlphaType);
    }
    if (!dst.tryAllocPixels(dstII)) {
        return nullptr;
    }
    dst.eraseColor(SK_ColorTRANSPARENT);

    const size_t srcLen = src.height() > 0 ? src.computeByteSize() : 0;
    const size_t dstLen = dst.height() > 0 ? dst.computeByteSize() : 0;

    if (isA8) {
        skia_rs_blur_engine_eval_a8(sigma,
                                    static_cast<const uint8_t*>(src.getPixels()),
                                    srcLen,
                                    src.rowBytesAsPixels(),
                                    originalSrcBounds,
                                    static_cast<uint8_t*>(dst.getPixels()),
                                    dstLen,
                                    dst.rowBytesAsPixels(),
                                    dstBounds,
                                    originalDstBounds);
    } else {
        skia_rs_blur_engine_eval_8888(sigma,
                                      static_cast<const uint8_t*>(src.getPixels()),
                                      srcLen,
                                      src.rowBytesAsPixels(),
                                      originalSrcBounds,
                                      static_cast<uint8_t*>(dst.getPixels()),
                                      dstLen,
                                      dst.rowBytesAsPixels(),
                                      dstBounds,
                                      originalDstBounds);
    }

    dst.setImmutable();
    SkIRect finalSubset = originalDstBounds.makeOffset(-dstOrigin);
    return SkSpecialImages::MakeFromRaster(finalSubset, dst, SkSurfaceProps{});
}

class RasterA8BlurAlgorithm : public SkBlurEngine::Algorithm {
public:
    float maxSigma() const override {
        static constexpr float kMaxSigma = 135.f;
        SkASSERT(SkBlurEngine::BoxBlurWindow(kMaxSigma) <= 255);
        return kMaxSigma;
    }

    bool supportsOnlyDecalTiling() const override { return true; }

    sk_sp<SkSpecialImage> blur(SkSize sigma,
                               sk_sp<SkSpecialImage> input,
                               const SkIRect& originalSrcBounds,
                               SkTileMode tileMode,
                               const SkIRect& originalDstBounds) const override {
        SkASSERT(tileMode == SkTileMode::kDecal);
        SkASSERT(SkIRect::MakeSize(input->dimensions()).contains(originalSrcBounds));
        return eval_raster_blur(sigma, std::move(input), originalSrcBounds, originalDstBounds,
                                /*isA8=*/true);
    }
};

class Raster8888BlurAlgorithm : public SkBlurEngine::Algorithm {
public:
    float maxSigma() const override {
        static constexpr float kMaxSigma = 135.f;
        SkASSERT(SkBlurEngine::BoxBlurWindow(kMaxSigma) <= 255);
        return kMaxSigma;
    }

    bool supportsOnlyDecalTiling() const override { return true; }

    sk_sp<SkSpecialImage> blur(SkSize sigma,
                               sk_sp<SkSpecialImage> input,
                               const SkIRect& originalSrcBounds,
                               SkTileMode tileMode,
                               const SkIRect& originalDstBounds) const override {
        SkASSERT(tileMode == SkTileMode::kDecal);
        SkASSERT(SkIRect::MakeSize(input->dimensions()).contains(originalSrcBounds));
        return eval_raster_blur(sigma, std::move(input), originalSrcBounds, originalDstBounds,
                                /*isA8=*/false);
    }
};

class RasterShaderBlurAlgorithm : public SkShaderBlurAlgorithm {
public:
    sk_sp<SkDevice> makeDevice(const SkImageInfo& imageInfo) const override {
        return SkBitmapDevice::Create(imageInfo, SkSurfaceProps{});
    }
};

class RasterBlurEngine : public SkBlurEngine {
public:
    const Algorithm* findAlgorithm(SkSize, SkColorType colorType) const override {
        const bool rgba8Blur = colorType == kRGBA_8888_SkColorType ||
                               colorType == kBGRA_8888_SkColorType;
        const bool a8Blur = colorType == kAlpha_8_SkColorType;
        if (a8Blur) {
            return &fA8BlurAlgorithm;
        } else if (rgba8Blur) {
            return &fRGBA8BlurAlgorithm;
        } else {
            return &fShaderBlurAlgorithm;
        }
    }

private:
    RasterShaderBlurAlgorithm fShaderBlurAlgorithm;
    Raster8888BlurAlgorithm fRGBA8BlurAlgorithm;
    RasterA8BlurAlgorithm fA8BlurAlgorithm;
};

}  // namespace

const SkBlurEngine* SkBlurEngine::GetRasterBlurEngine() {
    static const RasterBlurEngine kInstance;
    return &kInstance;
}

void SkShaderBlurAlgorithm::Compute2DBlurKernel(SkSize sigma,
                                                SkISize radius,
                                                SkSpan<float> kernel) {
    SkASSERT(SkBlurEngine::SigmaToRadius(sigma.width()) == radius.width() &&
             SkBlurEngine::SigmaToRadius(sigma.height()) == radius.height());
    skia_rs_blur_engine_compute_2d_kernel(sigma, radius, kernel.data(), kernel.size());
}

void SkShaderBlurAlgorithm::Compute2DBlurKernel(SkSize sigma,
                                                SkISize radii,
                                                std::array<SkV4, kMaxSamples / 4>& kernel) {
    static_assert(sizeof(kernel) == sizeof(std::array<float, kMaxSamples>));
    static_assert(alignof(float) == alignof(SkV4));
    float* data = kernel[0].ptr();
    Compute2DBlurKernel(sigma, radii, SkSpan<float>(data, kMaxSamples));
}

void SkShaderBlurAlgorithm::Compute2DBlurOffsets(SkISize radius,
                                                 std::array<SkV4, kMaxSamples / 2>& offsets) {
    skia_rs_blur_engine_compute_2d_offsets(radius, offsets.data());
}

void SkShaderBlurAlgorithm::Compute1DBlurLinearKernel(
        float sigma,
        int radius,
        std::array<SkV4, kMaxSamples / 2>& offsetsAndKernel) {
    SkASSERT(sigma <= kMaxLinearSigma);
    SkASSERT(radius == SkBlurEngine::SigmaToRadius(sigma));
    SkASSERT(LinearKernelWidth(radius) <= kMaxSamples);
    skia_rs_blur_engine_compute_1d_linear_kernel(sigma, radius, offsetsAndKernel.data());
}

static SkKnownRuntimeEffects::StableKey to_stablekey(int kernelWidth, uint32_t baseKey) {
    SkASSERT(kernelWidth >= 2 && kernelWidth <= SkShaderBlurAlgorithm::kMaxSamples);
    return static_cast<SkKnownRuntimeEffects::StableKey>(
            baseKey + skia_rs_blur_engine_stable_key_offset(kernelWidth));
}

const SkRuntimeEffect* SkShaderBlurAlgorithm::GetLinearBlur1DEffect(int radius) {
    return GetKnownRuntimeEffect(
            to_stablekey(LinearKernelWidth(radius),
                         static_cast<uint32_t>(SkKnownRuntimeEffects::StableKey::k1DBlurBase)));
}

const SkRuntimeEffect* SkShaderBlurAlgorithm::GetBlur2DEffect(const SkISize& radii) {
    int kernelArea = KernelWidth(radii.width()) * KernelWidth(radii.height());
    return GetKnownRuntimeEffect(
            to_stablekey(kernelArea,
                         static_cast<uint32_t>(SkKnownRuntimeEffects::StableKey::k2DBlurBase)));
}

sk_sp<SkSpecialImage> SkShaderBlurAlgorithm::renderBlur(SkRuntimeShaderBuilder* blurEffectBuilder,
                                                        SkFilterMode filter,
                                                        SkISize radii,
                                                        sk_sp<SkSpecialImage> input,
                                                        const SkIRect& srcRect,
                                                        SkTileMode tileMode,
                                                        const SkIRect& dstRect) const {
    SkImageInfo outII = SkImageInfo::Make({dstRect.width(), dstRect.height()},
                                          input->colorType(),
                                          kPremul_SkAlphaType,
                                          input->colorInfo().refColorSpace());
    sk_sp<SkDevice> device = this->makeDevice(outII);
    if (!device) {
        return nullptr;
    }

    SkIRect subset = SkIRect::MakeSize(dstRect.size());
    device->clipRect(SkRect::Make(subset), SkClipOp::kIntersect, /*aa=*/false);
    device->setLocalToDevice(SkM44::Translate(-dstRect.left(), -dstRect.top()));

    SkPaint paint;
    paint.setBlendMode(SkBlendMode::kSrc);

    RsRenderBlurPlan plan = skia_rs_blur_engine_plan_render_blur_rects(
            srcRect, input->backingStoreDimensions(), radii, dstRect, static_cast<int32_t>(tileMode));

    if (!plan.fast_dst_rect.isEmpty()) {
        SkTileMode fastTileMode = static_cast<SkTileMode>(plan.fast_tile_mode);
        blurEffectBuilder->child("child") =
                input->asShader(fastTileMode, filter, SkMatrix::I(), /*strict=*/false);
        paint.setShader(blurEffectBuilder->makeShader());
        device->drawRect(SkRect::Make(plan.fast_dst_rect), paint);
    }

    if (plan.strict_border_count > 0) {
        blurEffectBuilder->child("child") = input->makeSubset(srcRect)->asShader(
                tileMode, filter, SkMatrix::Translate(srcRect.left(), srcRect.top()));
        paint.setShader(blurEffectBuilder->makeShader());
        for (int i = 0; i < plan.strict_border_count; ++i) {
            device->drawRect(SkRect::Make(plan.strict_borders[i]), paint);
        }
    }

    device->setImmutable();
    return device->snapSpecial(subset);
}

sk_sp<SkSpecialImage> SkShaderBlurAlgorithm::evalBlur2D(SkSize sigma,
                                                        SkISize radii,
                                                        sk_sp<SkSpecialImage> input,
                                                        const SkIRect& srcRect,
                                                        SkTileMode tileMode,
                                                        const SkIRect& dstRect) const {
    std::array<SkV4, kMaxSamples / 4> kernel;
    std::array<SkV4, kMaxSamples / 2> offsets;
    Compute2DBlurKernel(sigma, radii, kernel);
    Compute2DBlurOffsets(radii, offsets);

    SkRuntimeShaderBuilder builder{sk_ref_sp(GetBlur2DEffect(radii))};
    builder.uniform("kernel") = kernel;
    builder.uniform("offsets") = offsets;
    return this->renderBlur(&builder, SkFilterMode::kNearest, radii,
                            std::move(input), srcRect, tileMode, dstRect);
}

sk_sp<SkSpecialImage> SkShaderBlurAlgorithm::evalBlur1D(float sigma,
                                                        int radius,
                                                        SkV2 dir,
                                                        sk_sp<SkSpecialImage> input,
                                                        SkIRect srcRect,
                                                        SkTileMode tileMode,
                                                        SkIRect dstRect) const {
    std::array<SkV4, kMaxSamples / 2> offsetsAndKernel;
    Compute1DBlurLinearKernel(sigma, radius, offsetsAndKernel);

    SkRuntimeShaderBuilder builder{sk_ref_sp(GetLinearBlur1DEffect(radius))};
    builder.uniform("offsetsAndKernel") = offsetsAndKernel;
    builder.uniform("dir") = dir;
    SkISize radii{dir.x ? radius : 0, dir.y ? radius : 0};
    return this->renderBlur(&builder, SkFilterMode::kLinear, radii,
                            std::move(input), srcRect, tileMode, dstRect);
}

sk_sp<SkSpecialImage> SkShaderBlurAlgorithm::blur(SkSize sigma,
                                                  sk_sp<SkSpecialImage> src,
                                                  const SkIRect& srcRect,
                                                  SkTileMode tileMode,
                                                  const SkIRect& dstRect) const {
    SkASSERT(sigma.width() <= kMaxLinearSigma && sigma.height() <= kMaxLinearSigma);

    int radiusX = SkBlurEngine::SigmaToRadius(sigma.width());
    int radiusY = SkBlurEngine::SigmaToRadius(sigma.height());
    const int kernelArea = KernelWidth(radiusX) * KernelWidth(radiusY);
    if (kernelArea <= kMaxSamples && radiusX > 0 && radiusY > 0) {
        return this->evalBlur2D(sigma,
                                {radiusX, radiusY},
                                std::move(src),
                                srcRect,
                                tileMode,
                                dstRect);
    }

    SkIRect intermediateSrcRect = srcRect;
    SkIRect intermediateDstRect = dstRect;
    if (radiusX > 0) {
        if (radiusY > 0) {
            if (!skia_rs_blur_engine_plan_intermediate_dst(
                        srcRect, dstRect, radiusY, static_cast<int32_t>(tileMode),
                        &intermediateDstRect)) {
                return nullptr;
            }
        }
        src = this->evalBlur1D(sigma.width(),
                               radiusX,
                               /*dir=*/{1.f, 0.f},
                               std::move(src),
                               srcRect,
                               tileMode,
                               intermediateDstRect);
        if (!src) {
            return nullptr;
        }
        intermediateSrcRect = SkIRect::MakeWH(src->width(), src->height());
        intermediateDstRect = dstRect.makeOffset(-intermediateDstRect.left(),
                                                 -intermediateDstRect.top());
    }

    if (radiusY > 0) {
        src = this->evalBlur1D(sigma.height(),
                               radiusY,
                               /*dir=*/{0.f, 1.f},
                               std::move(src),
                               intermediateSrcRect,
                               tileMode,
                               intermediateDstRect);
    }

    return src;
}
