/*
 * Safe Rust bridge for:
 *   src/effects/imagefilters/SkBlendImageFilter.cpp
 *   src/effects/imagefilters/SkBlurImageFilter.cpp
 *   src/effects/imagefilters/SkColorFilterImageFilter.cpp
 *   src/effects/imagefilters/SkComposeImageFilter.cpp
 *   src/effects/imagefilters/SkCropImageFilter.cpp
 *   src/effects/imagefilters/SkDropShadowImageFilter.cpp
 *   src/effects/imagefilters/SkImageImageFilter.cpp
 *   src/effects/imagefilters/SkMergeImageFilter.cpp
 *   src/effects/imagefilters/SkPictureImageFilter.cpp
 *   src/effects/imagefilters/SkShaderImageFilter.cpp
 * (Phase 1 Layer 4.4 skia-rs-imagefilters, GN target :skia).
 */

#include "include/effects/SkImageFilters.h"

#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSize.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkBlenders.h"
#include "include/private/SkAssert.h"
#include "include/private/SkTo.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkImageFilterTypes.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkRectPriv.h"
#include "src/core/SkSamplingPriv.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"

#include <array>
#include <cstdint>
#include <optional>
#include <utility>

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkSize) == 8);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(skif::Mapping) == 192);

extern "C" {
int32_t skia_rs_if_blend_classify_factory(int32_t mode);
bool skia_rs_if_blend_affects_transparent_black(bool has_blend_mode, bool has_arith, float k3);
int32_t skia_rs_if_blend_null_shader_action(
        bool has_bg, bool has_fg, bool affects_transparent_black, bool has_blend_mode, int32_t mode);
int32_t skia_rs_if_blend_classify_bounds_rule(
        bool has_blend_mode, int32_t mode, bool has_arith, const float* arith_k, bool include_sc);
bool skia_rs_if_blend_combine_output_bounds(
        int32_t rule, bool has_fg, SkIRect fg, bool has_bg, SkIRect bg, SkIRect* out_rect);
SkRect skia_rs_if_blend_combine_fast_bounds(int32_t rule, SkRect fg, SkRect bg);

bool skia_rs_if_blur_validate(float sigma_x, float sigma_y);
SkSize skia_rs_if_blur_map_sigma(const skif::Mapping* mapping, SkSize param_sigma);
SkIRect skia_rs_if_blur_kernel_bounds(
        const skif::Mapping* mapping, SkSize param_sigma, SkIRect bounds);
SkRect skia_rs_if_blur_fast_bounds(SkRect child_bounds, SkSize param_sigma);

bool skia_rs_if_crop_validate_rect(SkRect rect);
SkIRect skia_rs_if_crop_layer_rect(
        const skif::Mapping* mapping, SkRect param_crop, int32_t tile_mode);
SkIRect skia_rs_if_crop_required_input(
        const skif::Mapping* mapping, SkRect param_crop, int32_t tile_mode, SkIRect output_bounds);
bool skia_rs_if_crop_output_bounds(
        const skif::Mapping* mapping,
        SkRect param_crop,
        int32_t tile_mode,
        bool has_child,
        SkIRect child_output,
        SkIRect* out_rect);
SkRect skia_rs_if_crop_fast_bounds(SkRect input_bounds, SkRect param_crop, int32_t tile_mode);

bool skia_rs_if_image_adjust_rects(
        SkISize image_dims, SkRect src_rect, SkRect dst_rect, SkRect* out_src, SkRect* out_dst);
bool skia_rs_if_picture_intersect_cull_rect(
        SkRect pic_cull, SkRect target_rect, SkRect* out_rect);
}  // extern "C"

///////////////////////////////////////////////////////////////////////////////////////////////////
// 1. SkBlendImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkBlendImageFilter : public SkImageFilter_Base {
    static constexpr int kBackground = 0;
    static constexpr int kForeground = 1;

public:
    SkBlendImageFilter(sk_sp<SkBlender> blender,
                       const std::optional<SkV4>& coefficients,
                       bool enforcePremul,
                       sk_sp<SkImageFilter> inputs[2])
            : SkImageFilter_Base(inputs, 2)
            , fBlender(std::move(blender))
            , fArithmeticCoefficients(coefficients)
            , fEnforcePremul(enforcePremul) {
        SkASSERT(fBlender);
    }

    SkRect computeFastBounds(const SkRect& bounds) const override;

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    static constexpr uint32_t kArithmetic_SkBlendMode = kCustom_SkBlendMode + 1;

    friend void ::SkRegisterBlendImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkBlendImageFilter)
    static sk_sp<SkFlattenable> LegacyArithmeticCreateProc(SkReadBuffer& buffer);

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    bool onAffectsTransparentBlack() const override {
        return skia_rs_if_blend_affects_transparent_black(
                as_BB(fBlender)->asBlendMode().has_value(),
                fArithmeticCoefficients.has_value(),
                fArithmeticCoefficients.has_value() ? (*fArithmeticCoefficients)[3] : 0.f);
    }

    skif::FilterResult onFilterImage(const skif::Context&) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    sk_sp<SkShader> makeBlendShader(sk_sp<SkShader> bg, sk_sp<SkShader> fg) const;

    sk_sp<SkBlender> fBlender;
    std::optional<SkV4> fArithmeticCoefficients;
    bool fEnforcePremul;
};

sk_sp<SkImageFilter> make_blend(sk_sp<SkBlender> blender,
                                sk_sp<SkImageFilter> background,
                                sk_sp<SkImageFilter> foreground,
                                const SkImageFilters::CropRect& cropRect,
                                std::optional<SkV4> coefficients = {},
                                bool enforcePremul = false) {
    if (!blender) {
        blender = SkBlender::Mode(SkBlendMode::kSrcOver);
    }

    auto cropped = [cropRect](sk_sp<SkImageFilter> filter) {
        if (cropRect) {
            filter = SkImageFilters::Crop(*cropRect, std::move(filter));
        }
        return filter;
    };

    if (auto bm = as_BB(blender)->asBlendMode()) {
        int32_t action = skia_rs_if_blend_classify_factory(static_cast<int32_t>(*bm));
        if (action == 1) {
            return cropped(std::move(foreground));
        } else if (action == 2) {
            return cropped(std::move(background));
        } else if (action == 3) {
            return SkImageFilters::Empty();
        }
    }

    sk_sp<SkImageFilter> inputs[2] = { std::move(background), std::move(foreground) };
    sk_sp<SkImageFilter> filter{new SkBlendImageFilter(blender, coefficients,
                                                       enforcePremul, inputs)};
    return cropped(std::move(filter));
}

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Blend(SkBlendMode mode,
                                           sk_sp<SkImageFilter> background,
                                           sk_sp<SkImageFilter> foreground,
                                           const CropRect& cropRect) {
    return make_blend(SkBlender::Mode(mode),
                      std::move(background),
                      std::move(foreground),
                      cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::Blend(sk_sp<SkBlender> blender,
                                           sk_sp<SkImageFilter> background,
                                           sk_sp<SkImageFilter> foreground,
                                           const CropRect& cropRect) {
    return make_blend(std::move(blender), std::move(background), std::move(foreground), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::Arithmetic(SkScalar k1,
                                                SkScalar k2,
                                                SkScalar k3,
                                                SkScalar k4,
                                                bool enforcePMColor,
                                                sk_sp<SkImageFilter> background,
                                                sk_sp<SkImageFilter> foreground,
                                                const CropRect& cropRect) {
    auto blender = SkBlenders::Arithmetic(k1, k2, k3, k4, enforcePMColor);
    if (!blender) {
        return nullptr;
    }
    return make_blend(std::move(blender),
                      std::move(background),
                      std::move(foreground),
                      cropRect,
                      SkV4{k1, k2, k3, k4},
                      enforcePMColor);
}

void SkRegisterBlendImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkBlendImageFilter);
    SkFlattenable::Register("SkXfermodeImageFilter_Base", SkBlendImageFilter::CreateProc);
    SkFlattenable::Register("SkXfermodeImageFilterImpl", SkBlendImageFilter::CreateProc);
    SkFlattenable::Register("ArithmeticImageFilterImpl",
                            SkBlendImageFilter::LegacyArithmeticCreateProc);
    SkFlattenable::Register("SkArithmeticImageFilter",
                            SkBlendImageFilter::LegacyArithmeticCreateProc);
}

sk_sp<SkFlattenable> SkBlendImageFilter::LegacyArithmeticCreateProc(SkReadBuffer& buffer) {
    if (!buffer.validate(buffer.isVersionLT(SkPicturePriv::kCombineBlendArithmeticFilters))) {
        SkASSERT(false);
        return nullptr;
    }

    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 2);
    std::array<float, 4> k;
    for (int i = 0; i < 4; ++i) {
        k[i] = buffer.readScalar();
    }
    const bool enforcePremul = buffer.readBool();
    return SkImageFilters::Arithmetic(k[0], k[1], k[2], k[3], enforcePremul,
                                      common.getInput(0), common.getInput(1), common.cropRect());
}

sk_sp<SkFlattenable> SkBlendImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 2);

    sk_sp<SkBlender> blender;
    std::optional<SkV4> coefficients;
    bool enforcePremul = false;

    const uint32_t mode = buffer.read32();
    if (mode == kArithmetic_SkBlendMode) {
        if (buffer.validate(!buffer.isVersionLT(SkPicturePriv::kCombineBlendArithmeticFilters))) {
            SkV4 k;
            for (int i = 0; i < 4; ++i) {
                k[i] = buffer.readScalar();
            }
            coefficients = k;
            enforcePremul = buffer.readBool();
            blender = SkBlenders::Arithmetic(k.x, k.y, k.z, k.w, enforcePremul);
            if (!buffer.validate(SkToBool(blender))) {
                return nullptr;
            }
        }
    } else if (mode == kCustom_SkBlendMode) {
        blender = buffer.readBlender();
    } else {
        if (!buffer.validate(mode <= (unsigned) SkBlendMode::kLastMode)) {
            return nullptr;
        }
        blender = SkBlender::Mode((SkBlendMode)mode);
    }

    return make_blend(std::move(blender),
                      common.getInput(kBackground),
                      common.getInput(kForeground),
                      common.cropRect(),
                      coefficients,
                      enforcePremul);
}

void SkBlendImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    if (fArithmeticCoefficients.has_value()) {
        buffer.write32(kArithmetic_SkBlendMode);
        const SkV4& k = *fArithmeticCoefficients;
        buffer.writeScalar(k[0]);
        buffer.writeScalar(k[1]);
        buffer.writeScalar(k[2]);
        buffer.writeScalar(k[3]);
        buffer.writeBool(fEnforcePremul);
    } else if (auto bm = as_BB(fBlender)->asBlendMode()) {
        buffer.write32((unsigned)bm.value());
    } else {
        buffer.write32(kCustom_SkBlendMode);
        buffer.writeFlattenable(fBlender.get());
    }
}

sk_sp<SkShader> SkBlendImageFilter::makeBlendShader(sk_sp<SkShader> bg, sk_sp<SkShader> fg) const {
    auto bm = as_BB(fBlender)->asBlendMode();
    int32_t action = skia_rs_if_blend_null_shader_action(
            bg != nullptr, fg != nullptr, this->onAffectsTransparentBlack(),
            bm.has_value(), bm.has_value() ? static_cast<int32_t>(*bm) : 0);
    if (action == 0) {
        return nullptr;
    } else if (action == 1) {
        return bg;
    } else if (action == 2) {
        return fg;
    }
    if (!bg) { bg = SkShaders::Color(SK_ColorTRANSPARENT); }
    if (!fg) { fg = SkShaders::Color(SK_ColorTRANSPARENT); }
    return SkShaders::Blend(fBlender, std::move(bg), std::move(fg));
}

skif::FilterResult SkBlendImageFilter::onFilterImage(const skif::Context& ctx) const {
    auto requiredInput = this->onGetOutputLayerBounds(ctx.mapping(), ctx.source().layerBounds());
    if (requiredInput) {
        if (!requiredInput->intersect(ctx.desiredOutput())) {
            return {};
        }
    } else {
        requiredInput = ctx.desiredOutput();
    }

    skif::Context inputCtx = ctx.withNewDesiredOutput(*requiredInput);
    skif::FilterResult::Builder builder{ctx};
    builder.add(this->getChildOutput(kBackground, inputCtx));
    builder.add(this->getChildOutput(kForeground, inputCtx));
    return builder.eval(
            [&](SkSpan<sk_sp<SkShader>> inputs) -> sk_sp<SkShader> {
                return this->makeBlendShader(inputs[kBackground], inputs[kForeground]);
            }, requiredInput);
}

skif::LayerSpace<SkIRect> SkBlendImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    skif::LayerSpace<SkIRect> requiredInput;
    std::optional<skif::LayerSpace<SkIRect>> maxOutput;
    if (contentBounds && (maxOutput = this->onGetOutputLayerBounds(mapping, *contentBounds))) {
        requiredInput = *maxOutput;
        if (!requiredInput.intersect(desiredOutput)) {
            return skif::LayerSpace<SkIRect>::Empty();
        }
    } else {
        requiredInput = desiredOutput;
    }

    skif::LayerSpace<SkIRect> bgInput =
            this->getChildInputLayerBounds(kBackground, mapping, requiredInput, contentBounds);
    skif::LayerSpace<SkIRect> fgInput =
            this->getChildInputLayerBounds(kForeground, mapping, requiredInput, contentBounds);

    bgInput.join(fgInput);
    return bgInput;
}

std::optional<skif::LayerSpace<SkIRect>> SkBlendImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    auto bm = as_BB(fBlender)->asBlendMode();
    const float* arithPtr = fArithmeticCoefficients.has_value()
                                    ? fArithmeticCoefficients->ptr()
                                    : nullptr;
    int32_t rule = skia_rs_if_blend_classify_bounds_rule(
            bm.has_value(),
            bm.has_value() ? static_cast<int32_t>(*bm) : 0,
            fArithmeticCoefficients.has_value(),
            arithPtr,
            /*include_sc=*/true);
    if (rule == 0) {
        return skif::LayerSpace<SkIRect>::Unbounded();
    }

    auto foregroundBounds = this->getChildOutputLayerBounds(kForeground, mapping, contentBounds);
    auto backgroundBounds = this->getChildOutputLayerBounds(kBackground, mapping, contentBounds);
    SkIRect outRect;
    if (skia_rs_if_blend_combine_output_bounds(
                rule,
                foregroundBounds.has_value(),
                foregroundBounds ? SkIRect(*foregroundBounds) : SkIRect::MakeEmpty(),
                backgroundBounds.has_value(),
                backgroundBounds ? SkIRect(*backgroundBounds) : SkIRect::MakeEmpty(),
                &outRect)) {
        return skif::LayerSpace<SkIRect>(outRect);
    }
    return skif::LayerSpace<SkIRect>::Unbounded();
}

SkRect SkBlendImageFilter::computeFastBounds(const SkRect& bounds) const {
    auto bm = as_BB(fBlender)->asBlendMode();
    const float* arithPtr = fArithmeticCoefficients.has_value()
                                    ? fArithmeticCoefficients->ptr()
                                    : nullptr;
    int32_t rule = skia_rs_if_blend_classify_bounds_rule(
            bm.has_value(),
            bm.has_value() ? static_cast<int32_t>(*bm) : 0,
            fArithmeticCoefficients.has_value(),
            arithPtr,
            /*include_sc=*/false);
    if (rule == 0) {
        return SkRectPriv::MakeLargeS32();
    }

    SkRect foregroundBounds = this->getInput(kForeground) ?
            this->getInput(kForeground)->computeFastBounds(bounds) : bounds;
    SkRect backgroundBounds = this->getInput(kBackground) ?
            this->getInput(kBackground)->computeFastBounds(bounds) : bounds;
    return skia_rs_if_blend_combine_fast_bounds(rule, foregroundBounds, backgroundBounds);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 2. SkBlurImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkBlurImageFilter final : public SkImageFilter_Base {
public:
    SkBlurImageFilter(SkSize sigma, sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fSigma{sigma} {}

    SkBlurImageFilter(SkSize sigma, SkTileMode legacyTileMode, sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fSigma(sigma)
            , fLegacyTileMode(legacyTileMode) {}

    SkRect computeFastBounds(const SkRect&) const override;

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterBlurImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkBlurImageFilter)

    skif::FilterResult onFilterImage(const skif::Context& context) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    skif::LayerSpace<SkSize> mapSigma(const skif::Mapping& mapping) const {
        return skif::LayerSpace<SkSize>(skia_rs_if_blur_map_sigma(&mapping, SkSize(fSigma)));
    }

    skif::LayerSpace<SkIRect> kernelBounds(const skif::Mapping& mapping,
                                           skif::LayerSpace<SkIRect> bounds) const {
        return skif::LayerSpace<SkIRect>(
                skia_rs_if_blur_kernel_bounds(&mapping, SkSize(fSigma), SkIRect(bounds)));
    }

    skif::ParameterSpace<SkSize> fSigma;
    SkTileMode fLegacyTileMode = SkTileMode::kDecal;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Blur(
        SkScalar sigmaX, SkScalar sigmaY, SkTileMode tileMode, sk_sp<SkImageFilter> input,
        const CropRect& cropRect) {
    if (!skia_rs_if_blur_validate(sigmaX, sigmaY)) {
        return nullptr;
    }

    if (tileMode != SkTileMode::kDecal && !cropRect) {
        return sk_make_sp<SkBlurImageFilter>(SkSize{sigmaX, sigmaY}, tileMode, std::move(input));
    }

    sk_sp<SkImageFilter> filter = std::move(input);
    if (tileMode != SkTileMode::kDecal && cropRect) {
        filter = SkImageFilters::Crop(*cropRect, tileMode, std::move(filter));
    }

    filter = sk_make_sp<SkBlurImageFilter>(SkSize{sigmaX, sigmaY}, std::move(filter));
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, SkTileMode::kDecal, std::move(filter));
    }
    return filter;
}

void SkRegisterBlurImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkBlurImageFilter);
    SkFlattenable::Register("SkBlurImageFilterImpl", SkBlurImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkBlurImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkScalar sigmaX = buffer.readScalar();
    SkScalar sigmaY = buffer.readScalar();
    SkTileMode tileMode = buffer.read32LE(SkTileMode::kLastTileMode);
    return SkImageFilters::Blur(
            sigmaX, sigmaY, tileMode, common.getInput(0), common.cropRect());
}

void SkBlurImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeScalar(SkSize(fSigma).fWidth);
    buffer.writeScalar(SkSize(fSigma).fHeight);
    buffer.writeInt(static_cast<int>(fLegacyTileMode));
}

skif::FilterResult SkBlurImageFilter::onFilterImage(const skif::Context& ctx) const {
    skif::Context inputCtx = ctx.withNewDesiredOutput(
            this->kernelBounds(ctx.mapping(), ctx.desiredOutput()));

    skif::FilterResult childOutput = this->getChildOutput(0, inputCtx);
    skif::LayerSpace<SkSize> sigma = this->mapSigma(ctx.mapping());
    if (sigma.width() == 0.f && sigma.height() == 0.f) {
        return childOutput;
    }

    skif::LayerSpace<SkIRect> maxOutput = ctx.desiredOutput();
    if (fLegacyTileMode != SkTileMode::kDecal) {
        maxOutput = this->kernelBounds(ctx.mapping(), childOutput.layerBounds());
        if (!maxOutput.intersect(ctx.desiredOutput())) {
            return {};
        }
        childOutput = childOutput.applyCrop(inputCtx,
                                            childOutput.layerBounds(),
                                            fLegacyTileMode);
    }

    skif::Context croppedOutput = ctx.withNewDesiredOutput(maxOutput);
    skif::FilterResult::Builder builder{croppedOutput};
    builder.add(childOutput);
    return builder.blur(sigma);
}

skif::LayerSpace<SkIRect> SkBlurImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    skif::LayerSpace<SkIRect> requiredInput = this->kernelBounds(mapping, desiredOutput);
    return this->getChildInputLayerBounds(0, mapping, requiredInput, contentBounds);
}

std::optional<skif::LayerSpace<SkIRect>> SkBlurImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    auto childOutput = this->getChildOutputLayerBounds(0, mapping, contentBounds);
    if (childOutput) {
        return this->kernelBounds(mapping, *childOutput);
    } else {
        return skif::LayerSpace<SkIRect>::Unbounded();
    }
}

SkRect SkBlurImageFilter::computeFastBounds(const SkRect& src) const {
    SkRect bounds = this->getInput(0) ? this->getInput(0)->computeFastBounds(src) : src;
    return skia_rs_if_blur_fast_bounds(bounds, SkSize(fSigma));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 3. SkColorFilterImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkColorFilterImageFilter final : public SkImageFilter_Base {
public:
    SkColorFilterImageFilter(sk_sp<SkColorFilter> cf, sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fColorFilter(std::move(cf)) {}

    SkRect computeFastBounds(const SkRect& bounds) const override;

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterColorFilterImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkColorFilterImageFilter)

    skif::FilterResult onFilterImage(const skif::Context&) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    bool onAffectsTransparentBlack() const override {
        return as_CFB(fColorFilter)->affectsTransparentBlack();
    }

    bool onIsColorFilterNode(SkColorFilter** filter) const override {
        SkASSERT(1 == this->countInputs());
        if (filter) {
            *filter = SkRef(fColorFilter.get());
        }
        return true;
    }

    sk_sp<SkColorFilter> fColorFilter;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::ColorFilter(sk_sp<SkColorFilter> cf,
                                                 sk_sp<SkImageFilter> input,
                                                 const CropRect& cropRect) {
    if (cf) {
        SkColorFilter* inputCF;
        if (input && input->isColorFilterNode(&inputCF)) {
            cf = cf->makeComposed(sk_sp<SkColorFilter>(inputCF));
            input = sk_ref_sp(input->getInput(0));
        }
    }

    sk_sp<SkImageFilter> filter = std::move(input);
    if (cf) {
        filter = sk_sp<SkImageFilter>(
                new SkColorFilterImageFilter(std::move(cf), std::move(filter)));
    }
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    return filter;
}

void SkRegisterColorFilterImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkColorFilterImageFilter);
    SkFlattenable::Register("SkColorFilterImageFilterImpl", SkColorFilterImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkColorFilterImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    sk_sp<SkColorFilter> cf(buffer.readColorFilter());
    return SkImageFilters::ColorFilter(std::move(cf), common.getInput(0), common.cropRect());
}

void SkColorFilterImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeFlattenable(fColorFilter.get());
}

skif::FilterResult SkColorFilterImageFilter::onFilterImage(const skif::Context& ctx) const {
    return this->getChildOutput(0, ctx).applyColorFilter(ctx, fColorFilter);
}

skif::LayerSpace<SkIRect> SkColorFilterImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    return this->getChildInputLayerBounds(0, mapping, desiredOutput, contentBounds);
}

std::optional<skif::LayerSpace<SkIRect>> SkColorFilterImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    if (as_CFB(fColorFilter)->affectsTransparentBlack()) {
        return skif::LayerSpace<SkIRect>::Unbounded();
    } else {
        return this->getChildOutputLayerBounds(0, mapping, contentBounds);
    }
}

SkRect SkColorFilterImageFilter::computeFastBounds(const SkRect& bounds) const {
    if (as_CFB(fColorFilter)->affectsTransparentBlack()) {
        return SkRectPriv::MakeLargeS32();
    } else if (this->getInput(0)) {
        return this->getInput(0)->computeFastBounds(bounds);
    } else {
        return bounds;
    }
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 4. SkComposeImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkComposeImageFilter final : public SkImageFilter_Base {
    static constexpr int kOuter = 0;
    static constexpr int kInner = 1;

public:
    explicit SkComposeImageFilter(sk_sp<SkImageFilter> inputs[2])
            : SkImageFilter_Base(inputs, 2,
                                 inputs[kInner] ? as_IFB(inputs[kInner])->usesSource() : false) {
        SkASSERT(inputs[kOuter].get());
        SkASSERT(inputs[kInner].get());
    }

    SkRect computeFastBounds(const SkRect& src) const override;

private:
    friend void ::SkRegisterComposeImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkComposeImageFilter)

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    skif::FilterResult onFilterImage(const skif::Context& context) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Compose(sk_sp<SkImageFilter> outer,
                                             sk_sp<SkImageFilter> inner) {
    if (!outer) {
        return inner;
    }
    if (!inner) {
        return outer;
    }
    sk_sp<SkImageFilter> inputs[2] = { std::move(outer), std::move(inner) };
    return sk_sp<SkImageFilter>(new SkComposeImageFilter(inputs));
}

void SkRegisterComposeImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkComposeImageFilter);
    SkFlattenable::Register("SkComposeImageFilterImpl", SkComposeImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkComposeImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 2);
    return SkImageFilters::Compose(common.getInput(kOuter), common.getInput(kInner));
}

skif::FilterResult SkComposeImageFilter::onFilterImage(const skif::Context& ctx) const {
    auto innerOutputBounds =
            this->getChildOutputLayerBounds(kInner, ctx.mapping(), ctx.source().layerBounds());
    skif::LayerSpace<SkIRect> outerRequiredInput =
            this->getChildInputLayerBounds(kOuter,
                                           ctx.mapping(),
                                           ctx.desiredOutput(),
                                           innerOutputBounds);

    skif::FilterResult innerResult =
            this->getChildOutput(kInner, ctx.withNewDesiredOutput(outerRequiredInput));
    return this->getChildOutput(kOuter, ctx.withNewSource(innerResult));
}

skif::LayerSpace<SkIRect> SkComposeImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    std::optional<skif::LayerSpace<SkIRect>> outerContentBounds;
    if (contentBounds) {
        outerContentBounds = this->getChildOutputLayerBounds(kInner, mapping, *contentBounds);
    }
    skif::LayerSpace<SkIRect> innerDesiredOutput =
            this->getChildInputLayerBounds(kOuter, mapping, desiredOutput, outerContentBounds);
    return this->getChildInputLayerBounds(kInner, mapping, innerDesiredOutput, contentBounds);
}

std::optional<skif::LayerSpace<SkIRect>> SkComposeImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    auto innerBounds = this->getChildOutputLayerBounds(kInner, mapping, contentBounds);
    return this->getChildOutputLayerBounds(kOuter, mapping, innerBounds);
}

SkRect SkComposeImageFilter::computeFastBounds(const SkRect& src) const {
    return this->getInput(kOuter)->computeFastBounds(
            this->getInput(kInner)->computeFastBounds(src));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 5. SkCropImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkCropImageFilter final : public SkImageFilter_Base {
public:
    SkCropImageFilter(const SkRect& cropRect, SkTileMode tileMode, sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fCropRect(cropRect)
            , fTileMode(tileMode) {
        SkASSERT(cropRect.isFinite());
        SkASSERT(cropRect.isSorted());
    }

    SkRect computeFastBounds(const SkRect& bounds) const override;

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterCropImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkCropImageFilter)
    static sk_sp<SkFlattenable> LegacyTileCreateProc(SkReadBuffer&);

    bool onAffectsTransparentBlack() const override { return fTileMode != SkTileMode::kDecal; }
    bool ignoreInputsAffectsTransparentBlack() const override { return true; }

    skif::FilterResult onFilterImage(const skif::Context& context) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    skif::LayerSpace<SkIRect> cropRect(const skif::Mapping& mapping) const {
        return skif::LayerSpace<SkIRect>(skia_rs_if_crop_layer_rect(
                &mapping, SkRect(fCropRect), static_cast<int32_t>(fTileMode)));
    }

    skif::LayerSpace<SkIRect> requiredInput(const skif::Mapping& mapping,
                                            const skif::LayerSpace<SkIRect>& outputBounds) const {
        return skif::LayerSpace<SkIRect>(skia_rs_if_crop_required_input(
                &mapping, SkRect(fCropRect), static_cast<int32_t>(fTileMode),
                SkIRect(outputBounds)));
    }

    skif::ParameterSpace<SkRect> fCropRect;
    SkTileMode fTileMode;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Crop(const SkRect& rect,
                                          SkTileMode tileMode,
                                          sk_sp<SkImageFilter> input) {
    if (!skia_rs_if_crop_validate_rect(rect)) {
        return nullptr;
    }
    return sk_sp<SkImageFilter>(new SkCropImageFilter(rect, tileMode, std::move(input)));
}

sk_sp<SkImageFilter> SkImageFilters::Empty() {
    return SkImageFilters::Crop(SkRect::MakeEmpty(), SkTileMode::kDecal, nullptr);
}

sk_sp<SkImageFilter> SkImageFilters::Tile(const SkRect& src,
                                          const SkRect& dst,
                                          sk_sp<SkImageFilter> input) {
    sk_sp<SkImageFilter> filter = SkImageFilters::Crop(src, SkTileMode::kRepeat, std::move(input));
    filter = SkImageFilters::Crop(dst, SkTileMode::kDecal, std::move(filter));
    return filter;
}

void SkRegisterCropImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkCropImageFilter);
    SkFlattenable::Register("SkTileImageFilter", SkCropImageFilter::LegacyTileCreateProc);
    SkFlattenable::Register("SkTileImageFilterImpl", SkCropImageFilter::LegacyTileCreateProc);
}

sk_sp<SkFlattenable> SkCropImageFilter::LegacyTileCreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkRect src, dst;
    buffer.readRect(&src);
    buffer.readRect(&dst);
    return SkImageFilters::Tile(src, dst, common.getInput(0));
}

sk_sp<SkFlattenable> SkCropImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkRect cropRect = buffer.readRect();
    if (!buffer.isValid() || !buffer.validate(skia_rs_if_crop_validate_rect(cropRect))) {
        return nullptr;
    }

    SkTileMode tileMode = SkTileMode::kDecal;
    if (!buffer.isVersionLT(SkPicturePriv::kCropImageFilterSupportsTiling)) {
        tileMode = buffer.read32LE(SkTileMode::kLastTileMode);
    }
    return SkImageFilters::Crop(cropRect, tileMode, common.getInput(0));
}

void SkCropImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeRect(SkRect(fCropRect));
    buffer.writeInt(static_cast<int32_t>(fTileMode));
}

skif::FilterResult SkCropImageFilter::onFilterImage(const skif::Context& context) const {
    skif::LayerSpace<SkIRect> cropInput = this->requiredInput(context.mapping(),
                                                              context.desiredOutput());
    skif::FilterResult childOutput =
            this->getChildOutput(0, context.withNewDesiredOutput(cropInput));
    return childOutput.applyCrop(context, this->cropRect(context.mapping()), fTileMode);
}

skif::LayerSpace<SkIRect> SkCropImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    skif::LayerSpace<SkIRect> requiredInput = this->requiredInput(mapping, desiredOutput);
    return this->getChildInputLayerBounds(0, mapping, requiredInput, contentBounds);
}

std::optional<skif::LayerSpace<SkIRect>> SkCropImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    auto childOutput = this->getChildOutputLayerBounds(0, mapping, contentBounds);
    SkIRect outRect;
    if (skia_rs_if_crop_output_bounds(
                &mapping,
                SkRect(fCropRect),
                static_cast<int32_t>(fTileMode),
                childOutput.has_value(),
                childOutput ? SkIRect(*childOutput) : SkIRect::MakeEmpty(),
                &outRect)) {
        return skif::LayerSpace<SkIRect>(outRect);
    }
    return skif::LayerSpace<SkIRect>::Unbounded();
}

SkRect SkCropImageFilter::computeFastBounds(const SkRect& bounds) const {
    SkRect inputBounds = this->getInput(0) ? this->getInput(0)->computeFastBounds(bounds) : bounds;
    return skia_rs_if_crop_fast_bounds(
            inputBounds, SkRect(fCropRect), static_cast<int32_t>(fTileMode));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 6. SkDropShadowImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

sk_sp<SkImageFilter> make_drop_shadow_graph(SkVector offset,
                                            SkSize sigma,
                                            SkColor4f color,
                                            sk_sp<SkColorSpace> colorSpace,
                                            bool shadowOnly,
                                            sk_sp<SkImageFilter> input,
                                            const std::optional<SkRect>& crop) {
    sk_sp<SkImageFilter> filter = input;
    filter = SkImageFilters::Blur(sigma.fWidth, sigma.fHeight, std::move(filter));
    filter = SkImageFilters::ColorFilter(
            SkColorFilters::Blend(color, std::move(colorSpace), SkBlendMode::kSrcIn),
            std::move(filter));
    filter = SkImageFilters::MatrixTransform(SkMatrix::Translate(offset.fX, offset.fY),
                                             SkFilterMode::kLinear,
                                             std::move(filter));
    if (!shadowOnly) {
#if defined(SK_LEGACY_BLEND_FOR_DROP_SHADOWS)
        filter = SkImageFilters::Blend(
                SkBlendMode::kSrcOver, std::move(filter), std::move(input));
#else
        filter = SkImageFilters::Merge(std::move(filter), std::move(input));
#endif
    }
    if (crop) {
        filter = SkImageFilters::Crop(*crop, std::move(filter));
    }
    return filter;
}

sk_sp<SkFlattenable> legacy_drop_shadow_create_proc(SkReadBuffer& buffer) {
    if (!buffer.isVersionLT(SkPicturePriv::Version::kDropShadowImageFilterComposition)) {
        return nullptr;
    }

    auto [child, cropRect] = SkImageFilter_Base::Unflatten(buffer);

    SkScalar dx = buffer.readScalar();
    SkScalar dy = buffer.readScalar();
    SkScalar sigmaX = buffer.readScalar();
    SkScalar sigmaY = buffer.readScalar();
    SkColor4f color = SkColor4f::FromColor(buffer.readColor());
    bool shadowOnly = SkToBool(buffer.read32LE(1));
    return make_drop_shadow_graph({dx, dy}, {sigmaX, sigmaY}, color, /*colorSpace=*/nullptr,
                                  shadowOnly, std::move(child), cropRect);
}

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::DropShadow(
        SkScalar dx, SkScalar dy, SkScalar sigmaX, SkScalar sigmaY, SkColor4f color,
        sk_sp<SkColorSpace> colorSpace,
        sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    return make_drop_shadow_graph({dx, dy}, {sigmaX, sigmaY}, color, std::move(colorSpace),
                                  /*shadowOnly=*/false, std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::DropShadowOnly(
        SkScalar dx, SkScalar dy, SkScalar sigmaX, SkScalar sigmaY, SkColor4f color,
        sk_sp<SkColorSpace> colorSpace, sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    return make_drop_shadow_graph({dx, dy}, {sigmaX, sigmaY}, color, std::move(colorSpace),
                                  /*shadowOnly=*/true, std::move(input), cropRect);
}

void SkRegisterLegacyDropShadowImageFilterFlattenable() {
    SkFlattenable::Register("SkDropShadowImageFilter", legacy_drop_shadow_create_proc);
    SkFlattenable::Register("SkDropShadowImageFilterImpl", legacy_drop_shadow_create_proc);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 7. SkImageImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkImageImageFilter final : public SkImageFilter_Base {
public:
    SkImageImageFilter(sk_sp<SkImage> image,
                       const SkRect& srcRect,
                       const SkRect& dstRect,
                       const SkSamplingOptions& sampling)
            : SkImageFilter_Base(nullptr, 0)
            , fImage(std::move(image))
            , fSrcRect(srcRect)
            , fDstRect(dstRect)
            , fSampling(sampling) {
        SkASSERT(fImage && !dstRect.isEmpty());
    }

    SkRect computeFastBounds(const SkRect&) const override { return SkRect(fDstRect); }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterImageImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkImageImageFilter)

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    skif::FilterResult onFilterImage(const skif::Context& ctx) const override {
        return skif::FilterResult::MakeFromImage(ctx, fImage, fSrcRect, fDstRect, fSampling);
    }

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping&,
            const skif::LayerSpace<SkIRect>&,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return skif::LayerSpace<SkIRect>::Empty();
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return mapping.paramToLayer(fDstRect).roundOut();
    }

    sk_sp<SkImage> fImage;
    SkRect fSrcRect;
    skif::ParameterSpace<SkRect> fDstRect;
    SkSamplingOptions fSampling;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Image(sk_sp<SkImage> image,
                                           const SkRect& srcRect,
                                           const SkRect& dstRect,
                                           const SkSamplingOptions& sampling) {
    if (!image) {
        return SkImageFilters::Empty();
    }
    SkRect adjSrc, adjDst;
    if (!skia_rs_if_image_adjust_rects(image->dimensions(), srcRect, dstRect, &adjSrc, &adjDst)) {
        return SkImageFilters::Empty();
    }
    return sk_sp<SkImageFilter>(new SkImageImageFilter(
            std::move(image), adjSrc, adjDst, sampling));
}

void SkRegisterImageImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkImageImageFilter);
    SkFlattenable::Register("SkImageSourceImpl", SkImageImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkImageImageFilter::CreateProc(SkReadBuffer& buffer) {
    SkSamplingOptions sampling;
    if (buffer.isVersionLT(SkPicturePriv::kImageFilterImageSampling_Version)) {
        sampling = SkSamplingPriv::FromFQ(buffer.checkFilterQuality(), kLinear_SkMediumAs);
    } else {
        sampling = buffer.readSampling();
    }

    SkRect src, dst;
    buffer.readRect(&src);
    buffer.readRect(&dst);

    sk_sp<SkImage> image(buffer.readImage());
    if (!image) {
        return nullptr;
    }

    return SkImageFilters::Image(std::move(image), src, dst, sampling);
}

void SkImageImageFilter::flatten(SkWriteBuffer& buffer) const {
    buffer.writeSampling(fSampling);
    buffer.writeRect(fSrcRect);
    buffer.writeRect(SkRect(fDstRect));
    buffer.writeImage(fImage.get());
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 8. SkMergeImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkMergeImageFilter final : public SkImageFilter_Base {
public:
    SkMergeImageFilter(sk_sp<SkImageFilter>* const filters, int count)
            : SkImageFilter_Base(filters, count) {
        SkASSERT(filters && count > 0);
    }

    SkRect computeFastBounds(const SkRect& rect) const override {
        return SkImageFilter_Base::computeFastBounds(rect);
    }

private:
    friend void ::SkRegisterMergeImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkMergeImageFilter)

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    skif::FilterResult onFilterImage(const skif::Context& ctx) const override {
        const int inputCount = this->countInputs();
        skif::FilterResult::Builder builder{ctx};
        for (int i = 0; i < inputCount; ++i) {
            builder.add(this->getChildOutput(i, ctx));
        }
        return builder.merge();
    }

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        const int inputCount = this->countInputs();
        return skif::LayerSpace<SkIRect>::Union(
                inputCount,
                [&](int i) {
                    return this->getChildInputLayerBounds(i, mapping, desiredOutput, contentBounds);
                });
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        const int inputCount = this->countInputs();
        bool childIsUnbounded = false;
        auto childOutput = skif::LayerSpace<SkIRect>::Union(
                inputCount,
                [&](int i) {
                    auto o = this->getChildOutputLayerBounds(i, mapping, contentBounds);
                    if (o) {
                        return *o;
                    } else {
                        childIsUnbounded = true;
                        return skif::LayerSpace<SkIRect>::Empty();
                    }
                });
        if (childIsUnbounded) {
            return skif::LayerSpace<SkIRect>::Unbounded();
        } else {
            return childOutput;
        }
    }
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Merge(sk_sp<SkImageFilter>* const filters, int count,
                                           const CropRect& cropRect) {
    if (count <= 0 || !filters) {
        return SkImageFilters::Empty();
    }

    sk_sp<SkImageFilter> filter{new SkMergeImageFilter(filters, count)};
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    return filter;
}

void SkRegisterMergeImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkMergeImageFilter);
    SkFlattenable::Register("SkMergeImageFilterImpl", SkMergeImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkMergeImageFilter::CreateProc(SkReadBuffer& buffer) {
    Common common;
    if (!common.unflatten(buffer, -1) || !buffer.isValid()) {
        return nullptr;
    }
    return SkImageFilters::Merge(common.inputs(), common.inputCount(), common.cropRect());
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 9. SkPictureImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkPictureImageFilter final : public SkImageFilter_Base {
public:
    SkPictureImageFilter(sk_sp<SkPicture> picture, const SkRect& cullRect)
            : SkImageFilter_Base(nullptr, 0)
            , fPicture(std::move(picture))
            , fCullRect(cullRect) {
        SkASSERT(fPicture && fPicture->cullRect().contains(cullRect));
    }

    SkRect computeFastBounds(const SkRect&) const override { return SkRect(fCullRect); }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterPictureImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkPictureImageFilter)

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    skif::FilterResult onFilterImage(const skif::Context& ctx) const override {
        return skif::FilterResult::MakeFromPicture(ctx, fPicture, fCullRect);
    }

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping&,
            const skif::LayerSpace<SkIRect>&,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return skif::LayerSpace<SkIRect>::Empty();
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return mapping.paramToLayer(fCullRect).roundOut();
    }

    sk_sp<SkPicture> fPicture;
    skif::ParameterSpace<SkRect> fCullRect;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Picture(sk_sp<SkPicture> pic, const SkRect& targetRect) {
    if (pic) {
        SkRect cullRect;
        if (skia_rs_if_picture_intersect_cull_rect(pic->cullRect(), targetRect, &cullRect)) {
            return sk_sp<SkImageFilter>(new SkPictureImageFilter(std::move(pic), cullRect));
        }
    }
    return SkImageFilters::Empty();
}

void SkRegisterPictureImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkPictureImageFilter);
    SkFlattenable::Register("SkPictureImageFilterImpl", SkPictureImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkPictureImageFilter::CreateProc(SkReadBuffer& buffer) {
    sk_sp<SkPicture> picture;
    if (buffer.readBool()) {
        picture = SkPicturePriv::MakeFromBuffer(buffer);
    }
    SkRect cullRect;
    buffer.readRect(&cullRect);
    return SkImageFilters::Picture(std::move(picture), cullRect);
}

void SkPictureImageFilter::flatten(SkWriteBuffer& buffer) const {
    buffer.writeBool(SkToBool(fPicture));
    if (fPicture) {
        SkPicturePriv::Flatten(fPicture, buffer);
    }
    buffer.writeRect(SkRect(fCullRect));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 10. SkShaderImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkShaderImageFilter final : public SkImageFilter_Base {
public:
    SkShaderImageFilter(sk_sp<SkShader> shader, SkImageFilters::Dither dither)
            : SkImageFilter_Base(nullptr, 0)
            , fShader(std::move(shader))
            , fDither(dither) {
        SkASSERT(fShader);
    }

    SkRect computeFastBounds(const SkRect&) const override {
        return SkRectPriv::MakeLargeS32();
    }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterShaderImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkShaderImageFilter)

    bool onAffectsTransparentBlack() const override { return true; }
    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    skif::FilterResult onFilterImage(const skif::Context& ctx) const override {
        const bool dither = fDither == SkImageFilters::Dither::kYes;
        return skif::FilterResult::MakeFromShader(ctx, fShader, dither);
    }

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping&,
            const skif::LayerSpace<SkIRect>&,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return skif::LayerSpace<SkIRect>::Empty();
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping&,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return skif::LayerSpace<SkIRect>::Unbounded();
    }

    sk_sp<SkShader> fShader;
    SkImageFilters::Dither fDither;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Shader(sk_sp<SkShader> shader,
                                            Dither dither,
                                            const CropRect& cropRect) {
    if (!shader) {
        return SkImageFilters::Empty();
    }

    sk_sp<SkImageFilter> filter{new SkShaderImageFilter(std::move(shader), dither)};
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    return filter;
}

void SkRegisterShaderImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkShaderImageFilter);
    SkFlattenable::Register("SkPaintImageFilter", SkShaderImageFilter::CreateProc);
    SkFlattenable::Register("SkPaintImageFilterImpl", SkShaderImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkShaderImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 0);
    sk_sp<SkShader> shader;
    bool dither;
    if (buffer.isVersionLT(SkPicturePriv::kShaderImageFilterSerializeShader)) {
        SkPaint paint = buffer.readPaint();
        shader = paint.getShader() ? paint.refShader()
                                   : SkShaders::Color(paint.getColor4f(), nullptr);
        dither = paint.isDither();
    } else {
        shader = buffer.readShader();
        dither = buffer.readBool();
    }
    return SkImageFilters::Shader(std::move(shader),
                                  SkImageFilters::Dither(dither),
                                  common.cropRect());
}

void SkShaderImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeFlattenable(fShader.get());
    buffer.writeBool(fDither == SkImageFilters::Dither::kYes);
}
