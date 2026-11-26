/*
 * Safe Rust bridge for:
 *   src/effects/SkColorMatrix.cpp
 *   src/effects/SkColorMatrixFilter.cpp
 *   src/effects/SkHighContrastFilter.cpp
 * (Phase 1 Layer 4.2 skia-rs-colorfilters, GN target :skia).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkColorMatrix.h"
#include "include/effects/SkHighContrastFilter.h"
#include "include/effects/SkRuntimeEffect.h"
#include "src/core/SkColorFilterPriv.h"
#include "src/core/SkKnownRuntimeEffects.h"

#include <cstddef>
#include <cstdint>

enum SkYUVColorSpace : int;

namespace {
struct SkiaRsColorMatrix {
    float mat[20];
};

struct SkiaRsLightingResult {
    bool is_blend;
    uint32_t blend_color;
    SkiaRsColorMatrix matrix;
};

struct SkiaRsHighContrastUniforms {
    float grayscale;
    float invert_style;
    float contrast;
};
}  // namespace

static_assert(sizeof(SkColorMatrix) == 80);
static_assert(sizeof(SkiaRsColorMatrix) == 80);
static_assert(sizeof(SkHighContrastConfig) == 12);
static_assert(offsetof(SkHighContrastConfig, fGrayscale) == 0);
static_assert(offsetof(SkHighContrastConfig, fInvertStyle) == 4);
static_assert(offsetof(SkHighContrastConfig, fContrast) == 8);
static_assert(sizeof(SkiaRsHighContrastUniforms) == 12);

#if defined(SK_YUV_COLOR_SPACE_HIGH_PRECISION)
static constexpr bool kYUVHighPrecision = true;
#else
static constexpr bool kYUVHighPrecision = false;
#endif

extern "C" {
void skia_rs_color_matrix_rgb_to_yuv(int cs, bool high_precision, float* out_mat);
void skia_rs_color_matrix_yuv_to_rgb(int cs, bool high_precision, float* out_mat);
void skia_rs_color_matrix_set_identity(float* mat);
void skia_rs_color_matrix_set_scale(float* mat, float r_scale, float g_scale,
                                    float b_scale, float a_scale);
void skia_rs_color_matrix_post_translate(float* mat, float dr, float dg, float db, float da);
void skia_rs_color_matrix_set_concat(float* result, const float* outer, const float* inner);
void skia_rs_color_matrix_set_saturation(float* mat, float sat);

SkiaRsLightingResult skia_rs_color_filter_classify_lighting(uint32_t mul, uint32_t add);

bool skia_rs_high_contrast_compute_uniforms(const SkHighContrastConfig* config,
                                            SkiaRsHighContrastUniforms* out_uniforms);
}  // extern "C"

// ============================================================================
// SkColorMatrix.cpp
// ============================================================================

SkColorMatrix SkColorMatrix::RGBtoYUV(SkYUVColorSpace cs) {
    SkColorMatrix m;
    skia_rs_color_matrix_rgb_to_yuv(static_cast<int>(cs), kYUVHighPrecision, m.fMat.data());
    return m;
}

SkColorMatrix SkColorMatrix::YUVtoRGB(SkYUVColorSpace cs) {
    SkColorMatrix m;
    skia_rs_color_matrix_yuv_to_rgb(static_cast<int>(cs), kYUVHighPrecision, m.fMat.data());
    return m;
}

void SkColorMatrix::setIdentity() {
    skia_rs_color_matrix_set_identity(fMat.data());
}

void SkColorMatrix::setScale(float rScale, float gScale, float bScale, float aScale) {
    skia_rs_color_matrix_set_scale(fMat.data(), rScale, gScale, bScale, aScale);
}

void SkColorMatrix::postTranslate(float dr, float dg, float db, float da) {
    skia_rs_color_matrix_post_translate(fMat.data(), dr, dg, db, da);
}

void SkColorMatrix::setConcat(const SkColorMatrix& matA, const SkColorMatrix& matB) {
    skia_rs_color_matrix_set_concat(fMat.data(), matA.fMat.data(), matB.fMat.data());
}

void SkColorMatrix::setSaturation(float sat) {
    skia_rs_color_matrix_set_saturation(fMat.data(), sat);
}

// ============================================================================
// SkColorMatrixFilter.cpp
// ============================================================================

sk_sp<SkColorFilter> SkColorFilters::Lighting(SkColor mul, SkColor add) {
    SkiaRsLightingResult res = skia_rs_color_filter_classify_lighting(mul, add);
    if (res.is_blend) {
        return SkColorFilters::Blend(res.blend_color, SkBlendMode::kModulate);
    }
    return SkColorFilters::Matrix(res.matrix.mat);
}

// ============================================================================
// SkHighContrastFilter.cpp
// ============================================================================

sk_sp<SkColorFilter> SkHighContrastFilter::Make(const SkHighContrastConfig& config) {
    SkiaRsHighContrastUniforms uniforms;
    if (!skia_rs_high_contrast_compute_uniforms(&config, &uniforms)) {
        return nullptr;
    }

    const SkRuntimeEffect* highContrastEffect =
            GetKnownRuntimeEffect(SkKnownRuntimeEffects::StableKey::kHighContrast);

    const SkAlphaType kUnpremul = kUnpremul_SkAlphaType;
    return SkColorFilterPriv::WithWorkingFormat(
            highContrastEffect->makeColorFilter(SkData::MakeWithCopy(&uniforms, sizeof(uniforms))),
            &SkNamedTransferFn::kLinear,
            /* gamut= */ nullptr,           // use the dst gamut
            &kUnpremul);
}
