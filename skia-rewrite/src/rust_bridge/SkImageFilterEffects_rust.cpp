/*
 * Safe Rust bridge for:
 *   src/effects/imagefilters/SkDisplacementMapImageFilter.cpp
 *   src/effects/imagefilters/SkLightingImageFilter.cpp
 *   src/effects/imagefilters/SkMagnifierImageFilter.cpp
 *   src/effects/imagefilters/SkMatrixConvolutionImageFilter.cpp
 *   src/effects/imagefilters/SkMatrixTransformImageFilter.cpp
 *   src/effects/imagefilters/SkMorphologyImageFilter.cpp
 * (Phase 1 Layer 4.4 skia-rs-imagefilters, GN target :skia).
 */

#include "include/effects/SkImageFilters.h"
#include "src/effects/imagefilters/SkMatrixConvolutionImageFilter.h"

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSize.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkRuntimeEffect.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkMath.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTemplates.h"
#include "src/core/SkImageFilterTypes.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkKnownRuntimeEffects.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkRectPriv.h"
#include "src/core/SkSamplingPriv.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/SkEmbossMaskFilter.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkIPoint) == 8);
static_assert(sizeof(SkPoint3) == 12);
static_assert(sizeof(SkSize) == 8);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(skif::Mapping) == 192);
static_assert(sizeof(skif::Vector) == 8);

namespace {

struct RsLightingUniforms {
    SkRect normalEdgeBounds;
    float negSurfaceDepth;
    float materialAndLightType[4];
    float lightPosAndSpotFalloff[4];
    float lightDirAndSpotCutoff[4];
    float lightColor[3];
};

struct RsMagnifierFilterPlan {
    int32_t action;  // 0=Empty, 1=DirectTransform, 2=Shader
    SkRect lensBounds;
    SkIRect lensBoundsRoundOut;
    SkIRect srcRectRoundOut;
    SkIRect expectedChildRoundOut;
    SkMatrix invZoomXform;
    float zoomXformUniform[4];
    float invInsetUniform[2];
};

struct RsMorphPassStep {
    bool isLinear;
    int32_t stepRadius;
    SkIRect sampleBounds;
    SkIRect stepDesiredOutput;
};

}  // namespace

extern "C" {
bool skia_rs_if_displacement_validate(int32_t x_channel, int32_t y_channel, float scale);
void skia_rs_if_displacement_channel_selector(int32_t channel, float out_v4[4]);
SkIRect skia_rs_if_displacement_outset_bounds(
        const skif::Mapping* mapping, float scale, SkIRect bounds);
skif::Vector skia_rs_if_displacement_layer_scale(const skif::Mapping* mapping, float scale);
SkRect skia_rs_if_displacement_fast_bounds(SkRect color_bounds, float scale);

bool skia_rs_if_lighting_validate(
        float k,
        float shininess,
        float surface_depth,
        SkPoint loc_xy,
        float loc_z,
        skif::Vector dir_xy,
        float dir_z,
        float falloff_exponent,
        float cos_cutoff_angle);
float skia_rs_if_lighting_spot_dir_and_cos(
        SkPoint3 location, SkPoint3 target, float cutoff_angle_deg, SkPoint3* out_dir);
SkIRect skia_rs_if_lighting_clamp_rect(
        SkIRect child_layer_bounds, SkIRect required_input, SkIRect desired_output);
SkIRect skia_rs_if_lighting_required_input(SkIRect desired_output);
void skia_rs_if_lighting_compute_uniforms(
        const skif::Mapping* mapping,
        SkIRect clamp_rect,
        int32_t light_type,
        uint32_t light_color_argb,
        SkPoint loc_xy,
        float loc_z,
        skif::Vector dir_xy,
        float dir_z,
        float falloff_exponent,
        float cos_cutoff_angle,
        int32_t mat_type,
        float surface_depth,
        float k,
        float shininess,
        RsLightingUniforms* out_uniforms);

int32_t skia_rs_if_magnifier_validate(SkRect lens_bounds, float zoom_amount, float inset);
void skia_rs_if_magnifier_plan_filter(
        const skif::Mapping* mapping,
        SkIRect desired_output,
        SkRect param_lens_bounds,
        float zoom_amount,
        float param_inset,
        bool has_child_output,
        SkIRect child_output,
        RsMagnifierFilterPlan* out_plan);
SkIRect skia_rs_if_magnifier_output_bounds(
        const skif::Mapping* mapping,
        SkRect param_lens_bounds,
        bool has_child_output,
        SkIRect child_output);
SkRect skia_rs_if_magnifier_fast_bounds(SkRect child_bounds, SkRect param_lens_bounds);

bool skia_rs_if_matconv_validate(SkISize kernel_size, bool has_kernel, SkIPoint kernel_offset);
int32_t skia_rs_if_matconv_quantize(int32_t kernel_length, int32_t* out_quantized_size);
void skia_rs_if_matconv_encode_kernel_a8(
        const float* kernel,
        size_t kernel_len,
        uint8_t* out_pixels,
        size_t quantized_size,
        float* out_inner_gain,
        float* out_inner_bias);
SkIRect skia_rs_if_matconv_bounds_sampled(
        SkIRect bounds, SkISize kernel_size, SkIPoint kernel_offset);
SkIRect skia_rs_if_matconv_bounds_affected(
        SkIRect bounds, SkISize kernel_size, SkIPoint kernel_offset);
void skia_rs_if_matconv_gain_and_bias(float gain, float bias, float out_v2[2]);

SkIRect skia_rs_if_matxform_required_input(
        const skif::Mapping* mapping,
        const SkMatrix* param_transform,
        SkIRect desired_output,
        bool is_default_nearest);
bool skia_rs_if_matxform_output_bounds(
        const skif::Mapping* mapping,
        const SkMatrix* param_transform,
        bool has_child,
        SkIRect child_output,
        SkIRect* out_rect);

int32_t skia_rs_if_morph_validate(SkSize radii);
SkISize skia_rs_if_morph_map_radii(const skif::Mapping* mapping, SkSize param_radii);
SkIRect skia_rs_if_morph_required_input(
        const skif::Mapping* mapping, SkSize param_radii, SkIRect bounds);
SkIRect skia_rs_if_morph_kernel_output_bounds(
        int32_t morph_type, const skif::Mapping* mapping, SkSize param_radii, SkIRect bounds);
SkRect skia_rs_if_morph_fast_bounds(int32_t morph_type, SkSize param_radii, SkRect child_bounds);
size_t skia_rs_if_morph_plan_passes(
        int32_t dir,
        int32_t radius,
        SkIRect desired_output,
        RsMorphPassStep* out_steps,
        size_t max_steps);
}  // extern "C"

///////////////////////////////////////////////////////////////////////////////////////////////////
// 1. SkDisplacementMapImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkDisplacementMapImageFilter final : public SkImageFilter_Base {
    static constexpr int kDisplacement = 0;
    static constexpr int kColor = 1;
    static constexpr SkSamplingOptions kDisplacementSampling{SkFilterMode::kNearest};

public:
    SkDisplacementMapImageFilter(SkColorChannel xChannel,
                                 SkColorChannel yChannel,
                                 SkScalar scale,
                                 sk_sp<SkImageFilter> inputs[2])
            : SkImageFilter_Base(inputs, 2)
            , fXChannel(xChannel)
            , fYChannel(yChannel)
            , fScale(scale) {}

    SkRect computeFastBounds(const SkRect& src) const override;

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterDisplacementMapImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkDisplacementMapImageFilter)

    skif::FilterResult onFilterImage(const skif::Context&) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override;

    skif::LayerSpace<SkIRect> outsetByMaxDisplacement(const skif::Mapping& mapping,
                                                      skif::LayerSpace<SkIRect> bounds) const {
        return skif::LayerSpace<SkIRect>(
                skia_rs_if_displacement_outset_bounds(&mapping, fScale, SkIRect(bounds)));
    }

    SkColorChannel fXChannel;
    SkColorChannel fYChannel;
    SkScalar fScale;
};

sk_sp<SkShader> make_displacement_shader(
        sk_sp<SkShader> displacement,
        sk_sp<SkShader> color,
        skif::LayerSpace<skif::Vector> scale,
        SkColorChannel xChannel,
        SkColorChannel yChannel) {
    if (!color) {
        return nullptr;
    }
    if (!displacement) {
        displacement = SkShaders::Color(SK_ColorTRANSPARENT);
    }

    const SkRuntimeEffect* displacementEffect =
            GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kDisplacement);

    float xSel[4], ySel[4];
    skia_rs_if_displacement_channel_selector(static_cast<int32_t>(xChannel), xSel);
    skia_rs_if_displacement_channel_selector(static_cast<int32_t>(yChannel), ySel);

    SkRuntimeShaderBuilder builder(sk_ref_sp(displacementEffect));
    builder.child("displMap") = std::move(displacement);
    builder.child("colorMap") = std::move(color);
    builder.uniform("scale") = SkV2{scale.x(), scale.y()};
    builder.uniform("xSelect") = SkV4{xSel[0], xSel[1], xSel[2], xSel[3]};
    builder.uniform("ySelect") = SkV4{ySel[0], ySel[1], ySel[2], ySel[3]};
    return builder.makeShader();
}

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::DisplacementMap(
        SkColorChannel xChannelSelector, SkColorChannel yChannelSelector, SkScalar scale,
        sk_sp<SkImageFilter> displacement, sk_sp<SkImageFilter> color, const CropRect& cropRect) {
    if (!skia_rs_if_displacement_validate(static_cast<int32_t>(xChannelSelector),
                                          static_cast<int32_t>(yChannelSelector),
                                          scale)) {
        return nullptr;
    }

    sk_sp<SkImageFilter> inputs[2] = { std::move(displacement), std::move(color) };
    sk_sp<SkImageFilter> filter(new SkDisplacementMapImageFilter(
            xChannelSelector, yChannelSelector, scale, inputs));
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    return filter;
}

void SkRegisterDisplacementMapImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkDisplacementMapImageFilter);
    SkFlattenable::Register("SkDisplacementMapEffect", SkDisplacementMapImageFilter::CreateProc);
    SkFlattenable::Register("SkDisplacementMapEffectImpl",
                            SkDisplacementMapImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkDisplacementMapImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 2);
    SkColorChannel xsel = buffer.read32LE(SkColorChannel::kLastEnum);
    SkColorChannel ysel = buffer.read32LE(SkColorChannel::kLastEnum);
    SkScalar scale = buffer.readScalar();
    return SkImageFilters::DisplacementMap(xsel, ysel, scale, common.getInput(0),
                                           common.getInput(1), common.cropRect());
}

void SkDisplacementMapImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeInt((int) fXChannel);
    buffer.writeInt((int) fYChannel);
    buffer.writeScalar(fScale);
}

skif::FilterResult SkDisplacementMapImageFilter::onFilterImage(const skif::Context& ctx) const {
    skif::LayerSpace<SkIRect> requiredColorInput =
            this->outsetByMaxDisplacement(ctx.mapping(), ctx.desiredOutput());
    skif::FilterResult colorOutput =
            this->getChildOutput(kColor, ctx.withNewDesiredOutput(requiredColorInput));
    if (!colorOutput) {
        return {};
    }

    skif::LayerSpace<SkIRect> outputBounds =
            this->outsetByMaxDisplacement(ctx.mapping(), colorOutput.layerBounds());
    if (!outputBounds.intersect(ctx.desiredOutput())) {
        return {};
    }

    skif::FilterResult displacementOutput =
            this->getChildOutput(kDisplacement, ctx.withNewDesiredOutput(outputBounds)
                                                   .withNewColorSpace(/*cs=*/nullptr));

    const skif::LayerSpace<skif::Vector> scale{
            skia_rs_if_displacement_layer_scale(&ctx.mapping(), fScale)};
    if (!displacementOutput) {
        skif::LayerSpace<SkMatrix> constantDisplacement{SkMatrix::Translate(-0.5f * scale.x(),
                                                                            -0.5f * scale.y())};
        return colorOutput.applyTransform(ctx, constantDisplacement, kDisplacementSampling);
    }

    using ShaderFlags = skif::FilterResult::ShaderFlags;
    skif::FilterResult::Builder builder{ctx};
    builder.add(displacementOutput, /*sampleBounds=*/outputBounds);
    builder.add(colorOutput,
                /*sampleBounds=*/requiredColorInput,
                ShaderFlags::kNonTrivialSampling,
                kDisplacementSampling);
    return builder.eval(
            [&](SkSpan<sk_sp<SkShader>> inputs) {
                return make_displacement_shader(inputs[kDisplacement], inputs[kColor],
                                                scale, fXChannel, fYChannel);
            }, outputBounds);
}

skif::LayerSpace<SkIRect> SkDisplacementMapImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    skif::LayerSpace<SkIRect> requiredInput = this->outsetByMaxDisplacement(mapping, desiredOutput);
    requiredInput = this->getChildInputLayerBounds(kColor, mapping, requiredInput, contentBounds);
    requiredInput.join(this->getChildInputLayerBounds(
            kDisplacement, mapping, desiredOutput, contentBounds));
    return requiredInput;
}

std::optional<skif::LayerSpace<SkIRect>> SkDisplacementMapImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    auto colorOutput = this->getChildOutputLayerBounds(kColor, mapping, contentBounds);
    if (colorOutput) {
        return this->outsetByMaxDisplacement(mapping, *colorOutput);
    } else {
        return skif::LayerSpace<SkIRect>::Unbounded();
    }
}

SkRect SkDisplacementMapImageFilter::computeFastBounds(const SkRect& src) const {
    SkRect colorBounds = this->getInput(kColor) ? this->getInput(kColor)->computeFastBounds(src)
                                                : src;
    return skia_rs_if_displacement_fast_bounds(colorBounds, fScale);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 2. SkLightingImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

struct Light {
    enum class Type {
        kDistant,
        kPoint,
        kSpot,
        kLast = kSpot
    };

    Type fType;
    SkColor fLightColor;
    SkPoint fLocationXY;
    float fLocationZ;
    skif::Vector fDirectionXY;
    float fDirectionZ;
    float fFalloffExponent;
    float fCosCutoffAngle;

    static Light Point(SkColor color, const SkPoint3& location) {
        return {Type::kPoint, color, {location.fX, location.fY}, location.fZ,
                {0.f, 0.f}, 0.f, 0.f, 0.f};
    }

    static Light Distant(SkColor color, const SkPoint3& direction) {
        return {Type::kDistant, color, {0.f, 0.f}, 0.f,
                {direction.fX, direction.fY}, direction.fZ, 0.f, 0.f};
    }

    static Light Spot(SkColor color, const SkPoint3& location, const SkPoint3& direction,
                      float falloffExponent, float cosCutoffAngle) {
        return {Type::kSpot, color, {location.fX, location.fY}, location.fZ,
                {direction.fX, direction.fY}, direction.fZ, falloffExponent, cosCutoffAngle};
    }
};

struct Material {
    enum class Type {
        kDiffuse,
        kSpecular,
        kEmbossSpecular,
        kLast = kEmbossSpecular
    };

    Type fType;
    float fSurfaceDepth;
    float fK;
    float fShininess;

    static Material Diffuse(float k, float surfaceDepth) {
        return {Type::kDiffuse, surfaceDepth, k, 0.f};
    }
    static Material Specular(float k, float shininess, float surfaceDepth) {
        return {Type::kSpecular, surfaceDepth, k, shininess};
    }
    static Material EmbossSpecular(float k, float shininess, float surfaceDepth) {
        return {Type::kEmbossSpecular, surfaceDepth, k, shininess};
    }
};

class SkLightingImageFilter final : public SkImageFilter_Base {
public:
    SkLightingImageFilter(const Light& light, const Material& material, sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fLight(light)
            , fMaterial(material) {}

    SkRect computeFastBounds(const SkRect&) const override {
        return SkRectPriv::MakeLargeS32();
    }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterLightingImageFilterFlattenables();
    SK_FLATTENABLE_HOOKS(SkLightingImageFilter)
    static Light LegacyDeserializeLight(SkReadBuffer& buffer);
    static sk_sp<SkFlattenable> LegacyDiffuseCreateProc(SkReadBuffer& buffer);
    static sk_sp<SkFlattenable> LegacySpecularCreateProc(SkReadBuffer& buffer);

    bool onAffectsTransparentBlack() const override { return true; }

    skif::FilterResult onFilterImage(const skif::Context&) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        skif::LayerSpace<SkIRect> req{skia_rs_if_lighting_required_input(SkIRect(desiredOutput))};
        return this->getChildInputLayerBounds(0, mapping, req, contentBounds);
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping&,
            std::optional<skif::LayerSpace<SkIRect>>) const override {
        return skif::LayerSpace<SkIRect>::Unbounded();
    }

    Light fLight;
    Material fMaterial;
};

sk_sp<SkImageFilter> make_lighting(const Light& light,
                                   const Material& material,
                                   sk_sp<SkImageFilter> input,
                                   const SkImageFilters::CropRect& cropRect) {
    if (!skia_rs_if_lighting_validate(
                material.fK, material.fShininess, material.fSurfaceDepth,
                light.fLocationXY, light.fLocationZ,
                light.fDirectionXY, light.fDirectionZ,
                light.fFalloffExponent, light.fCosCutoffAngle)) {
        return nullptr;
    }

    sk_sp<SkImageFilter> filter = std::move(input);
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    filter = sk_sp<SkImageFilter>(
            new SkLightingImageFilter(light, material, std::move(filter)));
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    return filter;
}

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::DistantLitDiffuse(
        const SkPoint3& direction, SkColor lightColor, SkScalar surfaceScale, SkScalar kd,
        sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    return make_lighting(Light::Distant(lightColor, direction),
                         Material::Diffuse(kd, surfaceScale),
                         std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::PointLitDiffuse(
        const SkPoint3& location, SkColor lightColor, SkScalar surfaceScale, SkScalar kd,
        sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    return make_lighting(Light::Point(lightColor, location),
                         Material::Diffuse(kd, surfaceScale),
                         std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::SpotLitDiffuse(
        const SkPoint3& location, const SkPoint3& target, SkScalar falloffExponent,
        SkScalar cutoffAngle, SkColor lightColor, SkScalar surfaceScale, SkScalar kd,
        sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    SkPoint3 dir;
    float cosCutoffAngle = skia_rs_if_lighting_spot_dir_and_cos(location, target, cutoffAngle, &dir);
    return make_lighting(Light::Spot(lightColor, location, dir, falloffExponent, cosCutoffAngle),
                         Material::Diffuse(kd, surfaceScale),
                         std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkEmbossMaskFilter::LegacySpecular(
        const SkPoint3& direction, SkColor lightColor, SkScalar surfaceScale, SkScalar ks,
        SkScalar shininess, sk_sp<SkImageFilter> input) {
    return make_lighting(::Light::Distant(lightColor, direction),
                         Material::EmbossSpecular(ks, shininess, surfaceScale),
                         std::move(input), {});
}

sk_sp<SkImageFilter> SkImageFilters::DistantLitSpecular(
        const SkPoint3& direction, SkColor lightColor, SkScalar surfaceScale, SkScalar ks,
        SkScalar shininess, sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    return make_lighting(Light::Distant(lightColor, direction),
                         Material::Specular(ks, shininess, surfaceScale),
                         std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::PointLitSpecular(
        const SkPoint3& location, SkColor lightColor, SkScalar surfaceScale, SkScalar ks,
        SkScalar shininess, sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    return make_lighting(Light::Point(lightColor, location),
                         Material::Specular(ks, shininess, surfaceScale),
                         std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::SpotLitSpecular(
        const SkPoint3& location, const SkPoint3& target, SkScalar falloffExponent,
        SkScalar cutoffAngle, SkColor lightColor, SkScalar surfaceScale, SkScalar ks,
        SkScalar shininess, sk_sp<SkImageFilter> input, const CropRect& cropRect) {
    SkPoint3 dir;
    float cosCutoffAngle = skia_rs_if_lighting_spot_dir_and_cos(location, target, cutoffAngle, &dir);
    return make_lighting(Light::Spot(lightColor, location, dir, falloffExponent, cosCutoffAngle),
                         Material::Specular(ks, shininess, surfaceScale),
                         std::move(input), cropRect);
}

void SkRegisterLightingImageFilterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkLightingImageFilter);
    SkFlattenable::Register("SkDiffuseLightingImageFilter",
                            SkLightingImageFilter::LegacyDiffuseCreateProc);
    SkFlattenable::Register("SkSpecularLightingImageFilter",
                            SkLightingImageFilter::LegacySpecularCreateProc);
}

sk_sp<SkFlattenable> SkLightingImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);

    Light light;
    light.fType = buffer.read32LE(Light::Type::kLast);
    light.fLightColor = buffer.readColor();

    SkPoint3 lightPos, lightDir;
    buffer.readPoint3(&lightPos);
    light.fLocationXY = {lightPos.fX, lightPos.fY};
    light.fLocationZ = lightPos.fZ;

    buffer.readPoint3(&lightDir);
    light.fDirectionXY = {lightDir.fX, lightDir.fY};
    light.fDirectionZ = lightDir.fZ;

    light.fFalloffExponent = buffer.readScalar();
    light.fCosCutoffAngle = buffer.readScalar();

    Material material;
    material.fType = buffer.read32LE(Material::Type::kLast);
    material.fSurfaceDepth = buffer.readScalar();
    material.fK = buffer.readScalar();
    material.fShininess = buffer.readScalar();

    if (!buffer.isValid()) {
        return nullptr;
    }

    return make_lighting(light, material, common.getInput(0), common.cropRect());
}

Light SkLightingImageFilter::LegacyDeserializeLight(SkReadBuffer& buffer) {
    Light::Type lightType = buffer.read32LE(Light::Type::kLast);
    if (!buffer.isValid()) {
        return {};
    }

    SkColor lightColor = SkColorSetARGB(255,
                                        (U8CPU) buffer.readScalar(),
                                        (U8CPU) buffer.readScalar(),
                                        (U8CPU) buffer.readScalar());
    switch (lightType) {
        case Light::Type::kDistant: {
            SkPoint3 dir = {buffer.readScalar(), buffer.readScalar(), buffer.readScalar()};
            return Light::Distant(lightColor, dir);
        }
        case Light::Type::kPoint: {
            SkPoint3 loc = {buffer.readScalar(), buffer.readScalar(), buffer.readScalar()};
            return Light::Point(lightColor, loc);
        }
        case Light::Type::kSpot: {
            SkPoint3 loc = {buffer.readScalar(), buffer.readScalar(), buffer.readScalar()};
            SkPoint3 target = {buffer.readScalar(), buffer.readScalar(), buffer.readScalar()};
            float falloffExponent = buffer.readScalar();
            float cosOuterConeAngle = buffer.readScalar();
            buffer.readScalar();
            buffer.readScalar();
            buffer.readScalar();
            buffer.readScalar();
            buffer.readScalar();
            return Light::Spot(lightColor, loc, target - loc, falloffExponent, cosOuterConeAngle);
        }
    }
    SkUNREACHABLE;
}

sk_sp<SkFlattenable> SkLightingImageFilter::LegacyDiffuseCreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    Light light = LegacyDeserializeLight(buffer);
    float surfaceScale = buffer.readScalar();
    float kd = buffer.readScalar();
    Material material = Material::Diffuse(kd, surfaceScale);
    return make_lighting(light, material, common.getInput(0), common.cropRect());
}

sk_sp<SkFlattenable> SkLightingImageFilter::LegacySpecularCreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    Light light = LegacyDeserializeLight(buffer);
    float surfaceScale = buffer.readScalar();
    float ks = buffer.readScalar();
    float shininess = buffer.readScalar();
    Material material = Material::Specular(ks, shininess, surfaceScale);
    return make_lighting(light, material, common.getInput(0), common.cropRect());
}

void SkLightingImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeInt((int) fLight.fType);
    buffer.writeColor(fLight.fLightColor);
    buffer.writePoint(fLight.fLocationXY);
    buffer.writeScalar(fLight.fLocationZ);
    buffer.writePoint(SkPoint{fLight.fDirectionXY.fX, fLight.fDirectionXY.fY});
    buffer.writeScalar(fLight.fDirectionZ);
    buffer.writeScalar(fLight.fFalloffExponent);
    buffer.writeScalar(fLight.fCosCutoffAngle);
    buffer.writeInt((int) fMaterial.fType);
    buffer.writeScalar(fMaterial.fSurfaceDepth);
    buffer.writeScalar(fMaterial.fK);
    buffer.writeScalar(fMaterial.fShininess);
}

skif::FilterResult SkLightingImageFilter::onFilterImage(const skif::Context& ctx) const {
    using ShaderFlags = skif::FilterResult::ShaderFlags;

    skif::LayerSpace<SkIRect> requiredInput{
            skia_rs_if_lighting_required_input(SkIRect(ctx.desiredOutput()))};
    skif::FilterResult childOutput =
            this->getChildOutput(0, ctx.withNewDesiredOutput(requiredInput));

    skif::LayerSpace<SkIRect> clampRect{skia_rs_if_lighting_clamp_rect(
            SkIRect(childOutput.layerBounds()),
            SkIRect(requiredInput),
            SkIRect(ctx.desiredOutput()))};

    RsLightingUniforms uniforms;
    skia_rs_if_lighting_compute_uniforms(
            &ctx.mapping(),
            SkIRect(clampRect),
            static_cast<int32_t>(fLight.fType),
            fLight.fLightColor,
            fLight.fLocationXY,
            fLight.fLocationZ,
            fLight.fDirectionXY,
            fLight.fDirectionZ,
            fLight.fFalloffExponent,
            fLight.fCosCutoffAngle,
            static_cast<int32_t>(fMaterial.fType),
            fMaterial.fSurfaceDepth,
            fMaterial.fK,
            fMaterial.fShininess,
            &uniforms);

    skif::FilterResult::Builder builder{ctx};
    builder.add(childOutput, /*sampleBounds=*/clampRect, ShaderFlags::kSampledRepeatedly);
    return builder.eval([&](SkSpan<sk_sp<SkShader>> input) {
        const SkRuntimeEffect* normalEffect =
                GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kNormal);
        SkRuntimeShaderBuilder normalBuilder(sk_ref_sp(normalEffect));
        normalBuilder.child("alphaMap") = std::move(input[0]);
        normalBuilder.uniform("edgeBounds") = uniforms.normalEdgeBounds;
        normalBuilder.uniform("negSurfaceDepth") = uniforms.negSurfaceDepth;
        sk_sp<SkShader> normals = normalBuilder.makeShader();

        const SkRuntimeEffect* lightingEffect =
                GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kLighting);
        SkRuntimeShaderBuilder lightBuilder(sk_ref_sp(lightingEffect));
        lightBuilder.child("normalMap") = std::move(normals);
        lightBuilder.uniform("materialAndLightType") = SkV4{
                uniforms.materialAndLightType[0], uniforms.materialAndLightType[1],
                uniforms.materialAndLightType[2], uniforms.materialAndLightType[3]};
        lightBuilder.uniform("lightPosAndSpotFalloff") = SkV4{
                uniforms.lightPosAndSpotFalloff[0], uniforms.lightPosAndSpotFalloff[1],
                uniforms.lightPosAndSpotFalloff[2], uniforms.lightPosAndSpotFalloff[3]};
        lightBuilder.uniform("lightDirAndSpotCutoff") = SkV4{
                uniforms.lightDirAndSpotCutoff[0], uniforms.lightDirAndSpotCutoff[1],
                uniforms.lightDirAndSpotCutoff[2], uniforms.lightDirAndSpotCutoff[3]};
        lightBuilder.uniform("lightColor") = SkV3{
                uniforms.lightColor[0], uniforms.lightColor[1], uniforms.lightColor[2]};
        return lightBuilder.makeShader();
    });
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 3. SkMagnifierImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkMagnifierImageFilter final : public SkImageFilter_Base {
public:
    SkMagnifierImageFilter(const SkRect& lensBounds,
                           float zoomAmount,
                           float inset,
                           const SkSamplingOptions& sampling,
                           sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fLensBounds(lensBounds)
            , fZoomAmount(zoomAmount)
            , fInset(inset)
            , fSampling(sampling) {}

    SkRect computeFastBounds(const SkRect& src) const override {
        SkRect bounds = this->getInput(0) ? this->getInput(0)->computeFastBounds(src) : src;
        return skia_rs_if_magnifier_fast_bounds(bounds, SkRect(fLensBounds));
    }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterMagnifierImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkMagnifierImageFilter)

    skif::FilterResult onFilterImage(const skif::Context& context) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>&,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        skif::LayerSpace<SkIRect> requiredInput = mapping.paramToLayer(fLensBounds).roundOut();
        return this->getChildInputLayerBounds(0, mapping, requiredInput, contentBounds);
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        auto output = this->getChildOutputLayerBounds(0, mapping, contentBounds);
        return skif::LayerSpace<SkIRect>(skia_rs_if_magnifier_output_bounds(
                &mapping,
                SkRect(fLensBounds),
                output.has_value(),
                output ? SkIRect(*output) : SkIRect::MakeEmpty()));
    }

    skif::ParameterSpace<SkRect> fLensBounds;
    float fZoomAmount;
    float fInset;
    SkSamplingOptions fSampling;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Magnifier(const SkRect& lensBounds,
                                               SkScalar zoomAmount,
                                               SkScalar inset,
                                               const SkSamplingOptions& sampling,
                                               sk_sp<SkImageFilter> input,
                                               const CropRect& cropRect) {
    int32_t valid = skia_rs_if_magnifier_validate(lensBounds, zoomAmount, inset);
    if (valid == 0) {
        return nullptr;
    }
    if (cropRect) {
        input = SkImageFilters::Crop(*cropRect, std::move(input));
    }
    if (valid == 2) {
        return sk_sp<SkImageFilter>(new SkMagnifierImageFilter(
                lensBounds, zoomAmount, inset, sampling, std::move(input)));
    }
    return input;
}

void SkRegisterMagnifierImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkMagnifierImageFilter);
}

sk_sp<SkFlattenable> SkMagnifierImageFilter::CreateProc(SkReadBuffer& buffer) {
    if (buffer.isVersionLT(SkPicturePriv::kRevampMagnifierFilter)) {
        return nullptr;
    }
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkRect lensBounds;
    buffer.readRect(&lensBounds);
    SkScalar zoomAmount = buffer.readScalar();
    SkScalar inset = buffer.readScalar();
    SkSamplingOptions sampling = buffer.readSampling();
    return SkImageFilters::Magnifier(lensBounds, zoomAmount, inset, sampling, common.getInput(0));
}

void SkMagnifierImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeRect(SkRect(fLensBounds));
    buffer.writeScalar(fZoomAmount);
    buffer.writeScalar(fInset);
    buffer.writeSampling(fSampling);
}

skif::FilterResult SkMagnifierImageFilter::onFilterImage(const skif::Context& context) const {
    std::optional<skif::LayerSpace<SkIRect>> childOut =
            this->getChildOutputLayerBounds(0, context.mapping(), context.source().layerBounds());

    RsMagnifierFilterPlan plan;
    skia_rs_if_magnifier_plan_filter(
            &context.mapping(),
            SkIRect(context.desiredOutput()),
            SkRect(fLensBounds),
            fZoomAmount,
            fInset,
            childOut.has_value(),
            childOut ? SkIRect(*childOut) : SkIRect::MakeEmpty(),
            &plan);

    if (plan.action == 0) {
        return {};
    }
    if (plan.action == 1) {
        skif::FilterResult childOutput = this->getChildOutput(
                0, context.withNewDesiredOutput(skif::LayerSpace<SkIRect>(plan.srcRectRoundOut)));
        return childOutput
                .applyTransform(context, skif::LayerSpace<SkMatrix>(plan.invZoomXform), fSampling)
                .applyCrop(context, skif::LayerSpace<SkIRect>(plan.lensBoundsRoundOut));
    }

    using ShaderFlags = skif::FilterResult::ShaderFlags;
    skif::FilterResult::Builder builder{context};
    auto actualOutput = this->getChildOutput(
            0, context.withNewDesiredOutput(skif::LayerSpace<SkIRect>(plan.expectedChildRoundOut)));

    builder.add(actualOutput, {}, ShaderFlags::kNonTrivialSampling, fSampling);
    return builder.eval([&](SkSpan<sk_sp<SkShader>> inputs) -> sk_sp<SkShader> {
        if (!inputs[0]) {
            return nullptr;
        }
        const SkRuntimeEffect* magEffect =
                GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kMagnifier);
        SkRuntimeShaderBuilder rb(sk_ref_sp(magEffect));
        rb.child("src") = std::move(inputs[0]);
        rb.uniform("lensBounds") = plan.lensBounds;
        rb.uniform("zoomXform") = SkV4{plan.zoomXformUniform[0], plan.zoomXformUniform[1],
                                       plan.zoomXformUniform[2], plan.zoomXformUniform[3]};
        rb.uniform("invInset") = SkV2{plan.invInsetUniform[0], plan.invInsetUniform[1]};
        return rb.makeShader();
    }, skif::LayerSpace<SkIRect>(plan.lensBoundsRoundOut));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 4. SkMatrixConvolutionImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

using namespace skia_private;
using namespace MatrixConvolutionImageFilter;

SkBitmap create_kernel_bitmap(const SkISize& kernelSize, const float* kernel,
                              float* innerGain, float* innerBias) {
    int length = kernelSize.fWidth * kernelSize.fHeight;
    int32_t quantizedKernelSize = 0;
    int32_t tier = skia_rs_if_matconv_quantize(length, &quantizedKernelSize);
    if (tier == 0) {
        *innerGain = 1.f;
        *innerBias = 0.f;
        return {};
    }

    SkBitmap kernelBM;
    if (!kernelBM.tryAllocPixels(SkImageInfo::Make({quantizedKernelSize, 1},
                                                   kAlpha_8_SkColorType,
                                                   kPremul_SkAlphaType))) {
        return {};
    }

    skia_rs_if_matconv_encode_kernel_a8(
            kernel, static_cast<size_t>(length),
            kernelBM.getAddr8(0, 0), static_cast<size_t>(quantizedKernelSize),
            innerGain, innerBias);

    kernelBM.setImmutable();
    return kernelBM;
}

class SkMatrixConvolutionImageFilter final : public SkImageFilter_Base {
public:
    SkMatrixConvolutionImageFilter(const SkISize& kernelSize, const SkScalar* kernel,
                                   SkScalar gain, SkScalar bias, const SkIPoint& kernelOffset,
                                   bool convolveAlpha, sk_sp<SkImageFilter> const* input)
            : SkImageFilter_Base(input, 1)
            , fKernel(kernel, kernelSize.width() * kernelSize.height())
            , fKernelSize(kernelSize)
            , fKernelOffset({kernelOffset.fX, kernelOffset.fY})
            , fGain(gain)
            , fBias(bias)
            , fConvolveAlpha(convolveAlpha) {
        fKernelBitmap = create_kernel_bitmap(kernelSize, kernel, &fInnerGain, &fInnerBias);
    }

    SkRect computeFastBounds(const SkRect&) const override {
        return SkRectPriv::MakeLargeS32();
    }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterMatrixConvolutionImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkMatrixConvolutionImageFilter)

    bool onAffectsTransparentBlack() const override { return true; }

    skif::FilterResult onFilterImage(const skif::Context& context) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        skif::LayerSpace<SkIRect> req = this->boundsSampledByKernel(desiredOutput);
        return this->getChildInputLayerBounds(0, mapping, req, contentBounds);
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        if (fConvolveAlpha && fBias != 0.f) {
            return skif::LayerSpace<SkIRect>::Unbounded();
        }
        auto outputBounds = this->getChildOutputLayerBounds(0, mapping, contentBounds);
        if (outputBounds) {
            return this->boundsAffectedByKernel(*outputBounds);
        } else {
            return skif::LayerSpace<SkIRect>::Unbounded();
        }
    }

    skif::LayerSpace<SkIRect> boundsSampledByKernel(const skif::LayerSpace<SkIRect>& bounds) const {
        return skif::LayerSpace<SkIRect>(skia_rs_if_matconv_bounds_sampled(
                SkIRect(bounds), SkISize(fKernelSize), {fKernelOffset.x(), fKernelOffset.y()}));
    }

    skif::LayerSpace<SkIRect> boundsAffectedByKernel(const skif::LayerSpace<SkIRect>& bounds) const {
        return skif::LayerSpace<SkIRect>(skia_rs_if_matconv_bounds_affected(
                SkIRect(bounds), SkISize(fKernelSize), {fKernelOffset.x(), fKernelOffset.y()}));
    }

    sk_sp<SkShader> createShader(const skif::Context& ctx, sk_sp<SkShader> input) const;

    TArray<float> fKernel;
    skif::LayerSpace<SkISize> fKernelSize;
    skif::LayerSpace<skif::IVector> fKernelOffset;
    float fGain;
    float fBias;
    bool fConvolveAlpha;
    SkBitmap fKernelBitmap;
    float fInnerBias;
    float fInnerGain;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::MatrixConvolution(const SkISize& kernelSize,
                                                       const SkScalar kernel[],
                                                       SkScalar gain,
                                                       SkScalar bias,
                                                       const SkIPoint& kernelOffset,
                                                       SkTileMode tileMode,
                                                       bool convolveAlpha,
                                                       sk_sp<SkImageFilter> input,
                                                       const CropRect& cropRect) {
    if (!skia_rs_if_matconv_validate(kernelSize, kernel != nullptr, kernelOffset)) {
        return nullptr;
    }

    sk_sp<SkImageFilter> filter = std::move(input);
    if (cropRect && tileMode != SkTileMode::kDecal) {
        filter = SkImageFilters::Crop(*cropRect, tileMode, std::move(filter));
    }
    filter = sk_sp<SkImageFilter>(new SkMatrixConvolutionImageFilter(
            kernelSize, kernel, gain, bias, kernelOffset, convolveAlpha, &filter));
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, SkTileMode::kDecal, std::move(filter));
    }
    return filter;
}

void SkRegisterMatrixConvolutionImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkMatrixConvolutionImageFilter);
    SkFlattenable::Register("SkMatrixConvolutionImageFilterImpl",
                            SkMatrixConvolutionImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkMatrixConvolutionImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);

    SkISize kernelSize;
    kernelSize.fWidth = buffer.readInt();
    kernelSize.fHeight = buffer.readInt();
    const int count = buffer.getArrayCount();

    const int64_t kernelArea = sk_64_mul(kernelSize.width(), kernelSize.height());
    if (!buffer.validate(kernelArea == count)) {
        return nullptr;
    }
    if (!buffer.validateCanReadN<SkScalar>(count)) {
        return nullptr;
    }
    AutoSTArray<16, SkScalar> kernel(count);
    if (!buffer.readScalarArray(kernel)) {
        return nullptr;
    }
    SkScalar gain = buffer.readScalar();
    SkScalar bias = buffer.readScalar();
    SkIPoint kernelOffset;
    kernelOffset.fX = buffer.readInt();
    kernelOffset.fY = buffer.readInt();

    SkTileMode tileMode = SkTileMode::kDecal;
    if (buffer.isVersionLT(SkPicturePriv::kConvolutionImageFilterTilingUpdate)) {
        tileMode = buffer.read32LE(SkTileMode::kLastTileMode);
    }
    bool convolveAlpha = buffer.readBool();

    if (!buffer.isValid()) {
        return nullptr;
    }
    return SkImageFilters::MatrixConvolution(
            kernelSize, kernel.get(), gain, bias, kernelOffset, tileMode,
            convolveAlpha, common.getInput(0), common.cropRect());
}

void SkMatrixConvolutionImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeInt(fKernelSize.width());
    buffer.writeInt(fKernelSize.height());
    buffer.writeScalarArray(fKernel);
    buffer.writeScalar(fGain);
    buffer.writeScalar(fBias);
    buffer.writeInt(fKernelOffset.x());
    buffer.writeInt(fKernelOffset.y());
    buffer.writeBool(fConvolveAlpha);
}

sk_sp<SkShader> SkMatrixConvolutionImageFilter::createShader(const skif::Context& ctx,
                                                             sk_sp<SkShader> input) const {
    const int kernelLength = fKernelSize.width() * fKernelSize.height();
    int32_t tier = skia_rs_if_matconv_quantize(kernelLength, nullptr);
    SkKnownRuntimeEffects::StableKey key =
            tier == 0 ? SkKnownRuntimeEffects::StableKey::kMatrixConvUniforms
                      : (tier == 1 ? SkKnownRuntimeEffects::StableKey::kMatrixConvTexSm
                                   : SkKnownRuntimeEffects::StableKey::kMatrixConvTexLg);
    const bool useTextureShader = (tier != 0);
    if (useTextureShader && fKernelBitmap.empty()) {
        return nullptr;
    }

    const SkRuntimeEffect* matrixConvEffect = GetKnownRuntimeEffect(key);
    SkRuntimeShaderBuilder builder(sk_ref_sp(matrixConvEffect));
    builder.child("child") = std::move(input);

    if (useTextureShader) {
        sk_sp<SkImage> cachedKernel = ctx.backend()->getCachedBitmap(fKernelBitmap);
        if (!cachedKernel) {
            return nullptr;
        }
        builder.child("kernel") = cachedKernel->makeRawShader(SkFilterMode::kNearest);
        builder.uniform("innerGainAndBias") = SkV2{fInnerGain, fInnerBias};
    } else {
        std::array<float, kMaxUniformKernelSize> paddedKernel{};
        SkASSERT_RELEASE(kernelLength <= kMaxUniformKernelSize);
        std::copy(fKernel.begin(), fKernel.end(), paddedKernel.begin());
        builder.uniform("kernel").set(paddedKernel.data(), kMaxUniformKernelSize);
    }

    builder.uniform("size") = SkISize(fKernelSize);
    builder.uniform("offset") = skif::IVector(fKernelOffset);
    float gb[2];
    skia_rs_if_matconv_gain_and_bias(fGain, fBias, gb);
    builder.uniform("gainAndBias") = SkV2{gb[0], gb[1]};
    builder.uniform("convolveAlpha") = fConvolveAlpha ? 1 : 0;
    return builder.makeShader();
}

skif::FilterResult SkMatrixConvolutionImageFilter::onFilterImage(
        const skif::Context& context) const {
    using ShaderFlags = skif::FilterResult::ShaderFlags;

    skif::LayerSpace<SkIRect> requiredInput = this->boundsSampledByKernel(context.desiredOutput());
    skif::FilterResult childOutput =
            this->getChildOutput(0, context.withNewDesiredOutput(requiredInput));

    skif::LayerSpace<SkIRect> outputBounds;
    if (fConvolveAlpha && fBias != 0.f) {
        outputBounds = context.desiredOutput();
    } else {
        outputBounds = this->boundsAffectedByKernel(childOutput.layerBounds());
        if (!outputBounds.intersect(context.desiredOutput())) {
            return {};
        }
    }

    skif::FilterResult::Builder builder{context};
    builder.add(childOutput,
                this->boundsSampledByKernel(outputBounds),
                ShaderFlags::kSampledRepeatedly);
    return builder.eval([&](SkSpan<sk_sp<SkShader>> inputs) {
        return this->createShader(context, inputs[0]);
    }, outputBounds);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 5. SkMatrixTransformImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

class SkMatrixTransformImageFilter final : public SkImageFilter_Base {
public:
    SkMatrixTransformImageFilter(const SkMatrix& transform,
                                 const SkSamplingOptions& sampling,
                                 sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fTransform(transform)
            , fSampling(sampling) {
        (void) static_cast<const SkMatrix&>(fTransform).getType();
    }

    SkRect computeFastBounds(const SkRect& src) const override {
        SkRect bounds = this->getInput(0) ? this->getInput(0)->computeFastBounds(src) : src;
        return static_cast<const SkMatrix&>(fTransform).mapRect(bounds);
    }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterMatrixTransformImageFilterFlattenable();
    SK_FLATTENABLE_HOOKS(SkMatrixTransformImageFilter)
    static sk_sp<SkFlattenable> LegacyOffsetCreateProc(SkReadBuffer& buffer);

    MatrixCapability onGetCTMCapability() const override { return MatrixCapability::kComplex; }

    skif::FilterResult onFilterImage(const skif::Context& context) const override {
        skif::LayerSpace<SkIRect> requiredInput =
                this->requiredInput(context.mapping(), context.desiredOutput());
        skif::FilterResult childOutput =
                this->getChildOutput(0, context.withNewDesiredOutput(requiredInput));

        skif::LayerSpace<SkMatrix> transform = context.mapping().paramToLayer(fTransform);
        return childOutput.applyTransform(context, transform, fSampling);
    }

    skif::LayerSpace<SkIRect> requiredInput(const skif::Mapping& mapping,
                                            const skif::LayerSpace<SkIRect>& desiredOutput) const {
        const SkMatrix& xform = static_cast<const SkMatrix&>(fTransform);
        return skif::LayerSpace<SkIRect>(skia_rs_if_matxform_required_input(
                &mapping, &xform, SkIRect(desiredOutput), fSampling == SkSamplingOptions()));
    }

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        skif::LayerSpace<SkIRect> requiredInput = this->requiredInput(mapping, desiredOutput);
        return this->getChildInputLayerBounds(0, mapping, requiredInput, contentBounds);
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        auto childOutput = this->getChildOutputLayerBounds(0, mapping, contentBounds);
        const SkMatrix& xform = static_cast<const SkMatrix&>(fTransform);
        SkIRect outRect;
        if (skia_rs_if_matxform_output_bounds(
                    &mapping,
                    &xform,
                    childOutput.has_value(),
                    childOutput ? SkIRect(*childOutput) : SkIRect::MakeEmpty(),
                    &outRect)) {
            return skif::LayerSpace<SkIRect>(outRect);
        }
        return skif::LayerSpace<SkIRect>::Unbounded();
    }

    skif::ParameterSpace<SkMatrix> fTransform;
    SkSamplingOptions fSampling;
};

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::MatrixTransform(const SkMatrix& transform,
                                                     const SkSamplingOptions& sampling,
                                                     sk_sp<SkImageFilter> input) {
    if (!transform.invert(/*inverse=*/nullptr)) {
        return nullptr;
    }
    return sk_sp<SkImageFilter>(new SkMatrixTransformImageFilter(transform,
                                                                 sampling,
                                                                 std::move(input)));
}

sk_sp<SkImageFilter> SkImageFilters::Offset(SkScalar dx, SkScalar dy,
                                            sk_sp<SkImageFilter> input,
                                            const CropRect& cropRect) {
    sk_sp<SkImageFilter> offset = SkImageFilters::MatrixTransform(
            SkMatrix::Translate(dx, dy),
            SkFilterMode::kNearest,
            std::move(input));
    if (cropRect) {
        offset = SkImageFilters::Crop(*cropRect, std::move(offset));
    }
    return offset;
}

void SkRegisterMatrixTransformImageFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkMatrixTransformImageFilter);
    SkFlattenable::Register("SkMatrixImageFilter", SkMatrixTransformImageFilter::CreateProc);
    SkFlattenable::Register("SkOffsetImageFilter",
                            SkMatrixTransformImageFilter::LegacyOffsetCreateProc);
    SkFlattenable::Register("SkOffsetImageFilterImpl",
                            SkMatrixTransformImageFilter::LegacyOffsetCreateProc);
}

sk_sp<SkFlattenable> SkMatrixTransformImageFilter::LegacyOffsetCreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkPoint offset;
    buffer.readPoint(&offset);
    return SkImageFilters::Offset(offset.x(), offset.y(), common.getInput(0), common.cropRect());
}

sk_sp<SkFlattenable> SkMatrixTransformImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkMatrix matrix;
    buffer.readMatrix(&matrix);

    auto sampling = [&]() {
        if (buffer.isVersionLT(SkPicturePriv::kMatrixImageFilterSampling_Version)) {
            return SkSamplingPriv::FromFQ(buffer.read32LE(kLast_SkLegacyFQ), kLinear_SkMediumAs);
        } else {
            return buffer.readSampling();
        }
    }();
    return SkImageFilters::MatrixTransform(matrix, sampling, common.getInput(0));
}

void SkMatrixTransformImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeMatrix(SkMatrix(fTransform));
    buffer.writeSampling(fSampling);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// 6. SkMorphologyImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

enum class MorphType {
    kErode,
    kDilate,
    kLastType = kDilate
};

enum class MorphDirection { kX, kY };

class SkMorphologyImageFilter final : public SkImageFilter_Base {
public:
    SkMorphologyImageFilter(MorphType type, SkSize radii, sk_sp<SkImageFilter> input)
            : SkImageFilter_Base(&input, 1)
            , fType(type)
            , fRadii(radii) {}

    SkRect computeFastBounds(const SkRect& src) const override {
        SkRect bounds = this->getInput(0) ? this->getInput(0)->computeFastBounds(src) : src;
        return skia_rs_if_morph_fast_bounds(static_cast<int32_t>(fType), SkSize(fRadii), bounds);
    }

protected:
    void flatten(SkWriteBuffer&) const override;

private:
    friend void ::SkRegisterMorphologyImageFilterFlattenables();
    SK_FLATTENABLE_HOOKS(SkMorphologyImageFilter)

    skif::FilterResult onFilterImage(const skif::Context&) const override;

    skif::LayerSpace<SkIRect> onGetInputLayerBounds(
            const skif::Mapping& mapping,
            const skif::LayerSpace<SkIRect>& desiredOutput,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        skif::LayerSpace<SkIRect> req{
                skia_rs_if_morph_required_input(&mapping, SkSize(fRadii), SkIRect(desiredOutput))};
        return this->getChildInputLayerBounds(0, mapping, req, contentBounds);
    }

    std::optional<skif::LayerSpace<SkIRect>> onGetOutputLayerBounds(
            const skif::Mapping& mapping,
            std::optional<skif::LayerSpace<SkIRect>> contentBounds) const override {
        auto childOutput = this->getChildOutputLayerBounds(0, mapping, contentBounds);
        if (childOutput) {
            return skif::LayerSpace<SkIRect>(skia_rs_if_morph_kernel_output_bounds(
                    static_cast<int32_t>(fType), &mapping, SkSize(fRadii), SkIRect(*childOutput)));
        } else {
            return skif::LayerSpace<SkIRect>::Unbounded();
        }
    }

    MorphType fType;
    skif::ParameterSpace<SkSize> fRadii;
};

sk_sp<SkImageFilter> make_morphology(MorphType type,
                                     SkSize radii,
                                     sk_sp<SkImageFilter> input,
                                     const SkImageFilters::CropRect& cropRect) {
    int32_t valid = skia_rs_if_morph_validate(radii);
    if (valid == 0) {
        return nullptr;
    }
    sk_sp<SkImageFilter> filter = std::move(input);
    if (valid == 2) {
        filter = sk_sp<SkImageFilter>(new SkMorphologyImageFilter(type, radii, std::move(filter)));
    }
    if (cropRect) {
        filter = SkImageFilters::Crop(*cropRect, std::move(filter));
    }
    return filter;
}

sk_sp<SkShader> make_linear_morphology(sk_sp<SkShader> input,
                                       MorphType type,
                                       MorphDirection direction,
                                       int radius) {
    const SkRuntimeEffect* linearMorphologyEffect =
            GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kLinearMorphology);
    SkRuntimeShaderBuilder builder(sk_ref_sp(linearMorphologyEffect));
    builder.child("child") = std::move(input);
    builder.uniform("offset") = direction == MorphDirection::kX ? SkV2{1.f, 0.f} : SkV2{0.f, 1.f};
    builder.uniform("flip") = (type == MorphType::kDilate) ? 1.f : -1.f;
    builder.uniform("radius") = (int32_t)radius;
    return builder.makeShader();
}

sk_sp<SkShader> make_sparse_morphology(sk_sp<SkShader> input,
                                       MorphType type,
                                       MorphDirection direction,
                                       int radius) {
    const SkRuntimeEffect* sparseMorphologyEffect =
            GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kSparseMorphology);
    SkRuntimeShaderBuilder builder(sk_ref_sp(sparseMorphologyEffect));
    builder.child("child") = std::move(input);
    builder.uniform("offset") = direction == MorphDirection::kX ? SkV2{(float)radius, 0.f}
                                                                : SkV2{0.f, (float)radius};
    builder.uniform("flip") = (type == MorphType::kDilate) ? 1.f : -1.f;
    return builder.makeShader();
}

skif::FilterResult morphology_pass(const skif::Context& ctx, const skif::FilterResult& input,
                                   MorphType type, MorphDirection dir, int radius) {
    using ShaderFlags = skif::FilterResult::ShaderFlags;
    RsMorphPassStep steps[16];
    size_t count = skia_rs_if_morph_plan_passes(
            static_cast<int32_t>(dir), radius, SkIRect(ctx.desiredOutput()), steps, 16);
    SkASSERT(count <= 16);

    skif::FilterResult childOutput = input;
    for (size_t i = 0; i < count; ++i) {
        if (!childOutput) {
            return {};
        }
        skif::Context stepCtx =
                ctx.withNewDesiredOutput(skif::LayerSpace<SkIRect>(steps[i].stepDesiredOutput));
        skif::FilterResult::Builder builder{stepCtx};
        builder.add(childOutput,
                    skif::LayerSpace<SkIRect>(steps[i].sampleBounds),
                    ShaderFlags::kSampledRepeatedly);
        childOutput = builder.eval([&](SkSpan<sk_sp<SkShader>> inputs) {
            if (steps[i].isLinear) {
                return make_linear_morphology(inputs[0], type, dir, steps[i].stepRadius);
            } else {
                return make_sparse_morphology(inputs[0], type, dir, steps[i].stepRadius);
            }
        });
    }
    return childOutput;
}

}  // namespace

sk_sp<SkImageFilter> SkImageFilters::Dilate(SkScalar radiusX, SkScalar radiusY,
                                            sk_sp<SkImageFilter> input,
                                            const CropRect& cropRect) {
    return make_morphology(MorphType::kDilate, {radiusX, radiusY}, std::move(input), cropRect);
}

sk_sp<SkImageFilter> SkImageFilters::Erode(SkScalar radiusX, SkScalar radiusY,
                                           sk_sp<SkImageFilter> input,
                                           const CropRect& cropRect) {
    return make_morphology(MorphType::kErode, {radiusX, radiusY}, std::move(input), cropRect);
}

void SkRegisterMorphologyImageFilterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkMorphologyImageFilter);
    SkFlattenable::Register("SkMorphologyImageFilterImpl", SkMorphologyImageFilter::CreateProc);
}

sk_sp<SkFlattenable> SkMorphologyImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkScalar width = buffer.readScalar();
    SkScalar height = buffer.readScalar();
    MorphType filterType = buffer.read32LE(MorphType::kLastType);
    if (filterType == MorphType::kDilate) {
        return SkImageFilters::Dilate(width, height, common.getInput(0), common.cropRect());
    } else if (filterType == MorphType::kErode) {
        return SkImageFilters::Erode(width, height, common.getInput(0), common.cropRect());
    } else {
        return nullptr;
    }
}

void SkMorphologyImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeScalar(SkSize(fRadii).width());
    buffer.writeScalar(SkSize(fRadii).height());
    buffer.writeInt(static_cast<int>(fType));
}

skif::FilterResult SkMorphologyImageFilter::onFilterImage(const skif::Context& ctx) const {
    skif::LayerSpace<SkIRect> requiredInput{skia_rs_if_morph_required_input(
            &ctx.mapping(), SkSize(fRadii), SkIRect(ctx.desiredOutput()))};
    skif::FilterResult childOutput =
            this->getChildOutput(0, ctx.withNewDesiredOutput(requiredInput));

    skif::LayerSpace<SkIRect> maxOutput{skia_rs_if_morph_kernel_output_bounds(
            static_cast<int32_t>(fType), &ctx.mapping(), SkSize(fRadii),
            SkIRect(childOutput.layerBounds()))};
    if (!maxOutput.intersect(ctx.desiredOutput())) {
        return {};
    }

    skif::LayerSpace<SkISize> radii{skia_rs_if_morph_map_radii(&ctx.mapping(), SkSize(fRadii))};
    skif::LayerSpace<SkIRect> maxOutputX = maxOutput;
    maxOutputX.outset(skif::LayerSpace<SkISize>({0, radii.height()}));
    childOutput = morphology_pass(ctx.withNewDesiredOutput(maxOutputX), childOutput, fType,
                                  MorphDirection::kX, radii.width());
    childOutput = morphology_pass(ctx.withNewDesiredOutput(maxOutput), childOutput, fType,
                                  MorphDirection::kY, radii.height());
    return childOutput;
}
