/*
 * Safe Rust bridge for:
 *   src/shaders/gradients/SkGradientBaseShader.cpp
 *   src/shaders/gradients/SkLinearGradient.cpp
 *   src/shaders/gradients/SkRadialGradient.cpp
 *   src/shaders/gradients/SkSweepGradient.cpp
 *   src/shaders/gradients/SkConicalGradient.cpp
 * (Phase 1 Layer 4.1 skia-rs-shaders, GN target :skia).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTileMode.h"
#include "include/effects/SkGradient.h"
#include "include/private/SkAssert.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkOnce.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTo.h"
#include "modules/skcms/skcms.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkWriteBuffer.h"
#include "src/shaders/SkShaderBase.h"
#include "src/shaders/gradients/SkConicalGradient.h"
#include "src/shaders/gradients/SkGradientBaseShader.h"
#include "src/shaders/gradients/SkLinearGradient.h"
#include "src/shaders/gradients/SkRadialGradient.h"
#include "src/shaders/gradients/SkSweepGradient.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

using namespace skia_private;

namespace {

struct SkiaRsUnpackedGradientFlags {
    uint32_t tile_mode;
    SkGradient::Interpolation interpolation;
    bool has_positions;
    bool has_color_space;
    bool has_legacy_local_matrix;
};

struct SkiaRsPreparedStopsMeta {
    size_t color_count;
    bool has_positions;
    bool first_stop_is_implicit;
    bool last_stop_is_implicit;
    bool colors_are_opaque;
};

struct SkiaRsInterpolatedToDstPlan {
    int32_t unpremul;
    int32_t color_op;
    SkAlphaType intermediate_alpha_type;
    SkAlphaType dst_alpha_type;
};

struct SkiaRsXformColorsResult {
    size_t out_count;
    bool has_out_positions;
};

struct SkiaRsDegenerateGradientResult {
    int32_t kind;
    SkColor4f color;
};

struct SkiaRsSweepTCoeff {
    float t_bias;
    float t_scale;
};

struct SkiaRsSweepAngles {
    float start_angle;
    float end_angle;
};

struct SkiaRsSweepGradientPlan {
    int32_t action;
    float t0;
    float t1;
    uint32_t effective_tile_mode;
};

struct SkiaRsConicalCreateResult {
    bool valid;
    SkConicalGradient::Type conical_type;
    SkMatrix gradient_matrix;
    SkConicalGradient::FocalData focal_data;
};

struct SkiaRsConicalFocalStagePlan {
    float p0;
    float p1;
    int32_t focal_op;
    bool mask_degenerates;
    bool negate_x;
    bool compensate_focal;
    bool unswap;
    bool apply_post_mask;
};

}  // namespace

static_assert(sizeof(SkGradient::Interpolation) == 3);
static_assert(sizeof(SkConicalGradient::FocalData) == 12);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkColor4f) == 16);
static_assert(sizeof(SkPMColor4f) == 16);

extern "C" {
uint32_t skia_rs_gradient_pack_flags(
        bool has_positions, bool has_color_space, uint32_t tile_mode,
        SkGradient::Interpolation interp);
SkiaRsUnpackedGradientFlags skia_rs_gradient_unpack_flags(uint32_t flags);
void skia_rs_gradient_serialization_slice(
        size_t color_count, bool first_stop_is_implicit, bool last_stop_is_implicit,
        size_t* out_start, size_t* out_count);
bool skia_rs_gradient_valid(
        bool has_colors_data, size_t color_count, uint32_t tile_mode,
        SkGradient::Interpolation interp);
bool skia_rs_gradient_is_opaque(bool colors_are_opaque, int32_t tile_mode);
SkColor4f skia_rs_gradient_luminance_color(const SkColor4f* colors, size_t count);
size_t skia_rs_gradient_max_stop_count(size_t color_count, const float* pos, size_t pos_count);
void skia_rs_gradient_prepare_stops(
        const SkColor4f* colors, size_t color_count, const float* pos, size_t pos_count,
        int32_t tile_mode, SkColor4f* out_colors, float* out_pos,
        SkiaRsPreparedStopsMeta* out_meta);
void skia_rs_gradient_compute_2_stop(
        SkPMColor4f c_l, SkPMColor4f c_r, float out_factor[4], float out_bias[4]);
size_t skia_rs_gradient_fill_evenly_spaced(
        const SkPMColor4f* pm_colors, size_t count,
        float* factor_r, float* factor_g, float* factor_b, float* factor_a,
        float* bias_r, float* bias_g, float* bias_b, float* bias_a,
        size_t capacity);
size_t skia_rs_gradient_fill_arbitrary(
        const SkPMColor4f* pm_colors, const float* positions, size_t count,
        float* factor_r, float* factor_g, float* factor_b, float* factor_a,
        float* bias_r, float* bias_g, float* bias_b, float* bias_a,
        float* ts, size_t capacity, size_t ts_capacity);
SkiaRsInterpolatedToDstPlan skia_rs_gradient_plan_interpolated_to_dst(
        bool colors_are_opaque, SkGradient::Interpolation interp);
float skia_rs_gradient_decal_limit_x();
int32_t skia_rs_gradient_intermediate_cs_kind(uint8_t color_space);
SkiaRsXformColorsResult skia_rs_gradient_xform_colors(
        const SkPMColor4f* in_colors, size_t in_count, const float* orig_positions,
        SkGradient::Interpolation interp, bool first_stop_is_implicit, bool last_stop_is_implicit,
        bool force_explicit_positions, SkPMColor4f* out_colors, float* out_positions,
        size_t out_capacity);
SkiaRsDegenerateGradientResult skia_rs_gradient_classify_degenerate(
        const SkColor4f* colors, size_t color_count, const float* pos, size_t pos_count,
        int32_t tile_mode);

void skia_rs_linear_gradient_unit_matrix(SkPoint p0, SkPoint p1, SkMatrix* out);
int32_t skia_rs_linear_gradient_classify(
        bool has_pts, SkPoint p0, SkPoint p1, bool has_colors_data, size_t color_count,
        uint32_t tile_mode, SkGradient::Interpolation interp, bool has_lm, bool lm_invertible);

void skia_rs_radial_gradient_unit_matrix(SkPoint center, float radius, SkMatrix* out);
int32_t skia_rs_radial_gradient_classify(
        float radius, bool has_colors_data, size_t color_count,
        uint32_t tile_mode, SkGradient::Interpolation interp, bool has_lm, bool lm_invertible);

void skia_rs_sweep_gradient_unit_matrix(SkPoint center, SkMatrix* out);
SkiaRsSweepTCoeff skia_rs_sweep_gradient_t_coeff(float t0, float t1);
SkiaRsSweepAngles skia_rs_sweep_gradient_angles_from_t(float t_bias, float t_scale);
void skia_rs_sweep_gradient_stage_matrix(float t_bias, float t_scale, SkMatrix* out);
SkiaRsSweepGradientPlan skia_rs_sweep_gradient_classify(
        float start_angle, float end_angle, bool has_colors_data, size_t color_count,
        uint32_t tile_mode, SkGradient::Interpolation interp, bool has_lm, bool lm_invertible);

bool skia_rs_conical_focal_data_set(
        SkConicalGradient::FocalData* focal_data, float r0, float r1, SkMatrix* matrix);
bool skia_rs_conical_map_to_unit_x(SkPoint start_center, SkPoint end_center, SkMatrix* out);
void skia_rs_conical_create_params(
        SkPoint c0, float r0, SkPoint c1, float r1, SkiaRsConicalCreateResult* out);
int32_t skia_rs_conical_classify_two_point(
        SkPoint start, float start_radius, SkPoint end, float end_radius,
        bool has_colors_data, size_t color_count, uint32_t tile_mode,
        SkGradient::Interpolation interp, bool has_lm, bool lm_invertible);
void skia_rs_conical_radial_stage_matrix(float r1, float r2, SkMatrix* out);
float skia_rs_conical_strip_p0(float r1, SkPoint c1, SkPoint c2);
void skia_rs_conical_plan_focal_stages(
        const SkConicalGradient::FocalData* focal_data, SkiaRsConicalFocalStagePlan* out);
}  // extern "C"

// ============================================================================
// SkGradientBaseShader.cpp
// ============================================================================

void SkGradientBaseShader::flatten(SkWriteBuffer& buffer) const {
    sk_sp<SkData> colorSpaceData = fColorSpace ? fColorSpace->serialize() : nullptr;
    uint32_t flags = skia_rs_gradient_pack_flags(
            fPositions != nullptr,
            colorSpaceData != nullptr,
            static_cast<uint32_t>(fTileMode),
            fInterpolation);
    buffer.writeUInt(flags);

    size_t start = 0;
    size_t colorCount = 0;
    skia_rs_gradient_serialization_slice(
            fColorCount, fFirstStopIsImplicit, fLastStopIsImplicit, &start, &colorCount);

    const SkColor4f* colors = fColors + start;
    const SkScalar* positions = fPositions ? fPositions + start : nullptr;

    buffer.writeColor4fArray({colors, colorCount});
    if (colorSpaceData) {
        buffer.writeDataAsByteArray(colorSpaceData.get());
    }
    if (positions) {
        buffer.writeScalarArray({positions, colorCount});
    }
}

template <int N, typename T, bool MEM_MOVE>
static bool validate_array(SkReadBuffer& buffer, size_t count, STArray<N, T, MEM_MOVE>* array) {
    if (!buffer.validateCanReadN<T>(count)) {
        return false;
    }
    array->resize_back(count);
    return true;
}

std::optional<SkGradient> SkGradientScope::unflatten(SkReadBuffer& buffer,
                                                     SkMatrix* legacyLocalMatrix) {
    uint32_t flags = buffer.readUInt();
    SkiaRsUnpackedGradientFlags unpacked = skia_rs_gradient_unpack_flags(flags);

    auto tm = static_cast<SkTileMode>(unpacked.tile_mode);

    const size_t count = buffer.getArrayCount();
    if (!(validate_array(buffer, count, &fColorStorage) &&
          buffer.readColor4fArray({fColorStorage.data(), count}))) {
        return {};
    }
    SkSpan<const SkColor4f> colors = {fColorStorage.begin(), count};

    sk_sp<SkColorSpace> colorSpace;
    if (unpacked.has_color_space) {
        if (auto data = buffer.readByteArrayAsData()) {
            colorSpace = SkColorSpace::Deserialize(data->data(), data->size());
        }
    }

    SkSpan<const float> pos;
    if (unpacked.has_positions) {
        if (!(validate_array(buffer, count, &fPositionStorage) &&
              buffer.readScalarArray({fPositionStorage.data(), count}))) {
            return {};
        }
        pos = {fPositionStorage.begin(), count};
    }

    if (unpacked.has_legacy_local_matrix) {
        SkASSERT(buffer.isVersionLT(SkPicturePriv::Version::kNoShaderLocalMatrix));
        buffer.readMatrix(legacyLocalMatrix);
    } else {
        *legacyLocalMatrix = SkMatrix::I();
    }

    if (buffer.isValid()) {
        return SkGradient({colors, pos, tm, std::move(colorSpace)}, unpacked.interpolation);
    }
    return {};
}

// Non-static to preserve exact symbol parity with SkGradientBaseShader.cpp
sk_sp<SkColorSpace> srgb_if_null(sk_sp<SkColorSpace> cs) {
    return cs ? cs : SkColorSpace::MakeSRGB();
}

SkGradientBaseShader::SkGradientBaseShader(const SkGradient& desc, const SkMatrix& ptsToUnit)
        : fPtsToUnit(ptsToUnit)
        , fColorSpace(srgb_if_null(desc.colors().colorSpace()))
        , fFirstStopIsImplicit(false)
        , fLastStopIsImplicit(false)
        , fColorsAreOpaque(true) {
    auto colors = desc.colors().colors();
    auto pos = desc.colors().positions();

    fPtsToUnit.getType();
    SkASSERT(colors.size() > 1);

    fInterpolation = desc.interpolation();
    SkASSERT((unsigned)desc.colors().tileMode() < kSkTileModeCount);
    fTileMode = desc.colors().tileMode();

    size_t maxStopCount = skia_rs_gradient_max_stop_count(colors.size(), pos.data(), pos.size());
    size_t storageSize =
            maxStopCount * (sizeof(SkColor4f) + (pos.size() > 0 ? sizeof(SkScalar) : 0));
    fColors = reinterpret_cast<SkColor4f*>(fStorage.reset(storageSize));
    fPositions = pos.size() > 0 ? reinterpret_cast<float*>(fColors + maxStopCount) : nullptr;

    SkiaRsPreparedStopsMeta meta;
    skia_rs_gradient_prepare_stops(
            colors.data(), colors.size(),
            pos.data(), pos.size(),
            static_cast<int32_t>(fTileMode),
            fColors, fPositions, &meta);

    fColorCount = meta.color_count;
    fFirstStopIsImplicit = meta.first_stop_is_implicit;
    fLastStopIsImplicit = meta.last_stop_is_implicit;
    fColorsAreOpaque = meta.colors_are_opaque;
    if (!meta.has_positions) {
        fPositions = nullptr;
    }
}

SkGradientBaseShader::~SkGradientBaseShader() {}

void SkGradientBaseShader::AppendGradientFillStages(SkRasterPipeline* p,
                                                    SkArenaAlloc* alloc,
                                                    const SkPMColor4f* pmColors,
                                                    const SkScalar* positions,
                                                    int count) {
    if (count == 2 && positions == nullptr) {
        auto ctx = alloc->make<SkRasterPipelineContexts::EvenlySpaced2StopGradientCtx>();
        skia_rs_gradient_compute_2_stop(pmColors[0], pmColors[1], ctx->factor, ctx->bias);
        p->append(SkRasterPipelineOp::evenly_spaced_2_stop_gradient, ctx);
        return;
    }

    auto* ctx = alloc->make<SkRasterPipelineContexts::GradientCtx>();
    constexpr int kMaxRegisterSize = 8;
    const size_t factorBiasFloats = std::max(count + 1, kMaxRegisterSize);
    const size_t tsForArbitraryStops = count + 1;
    using SkRasterPipelineContexts::kRGBAChannels;

    const size_t toAlloc = 2 * kRGBAChannels * factorBiasFloats + tsForArbitraryStops;
    float* gradientCtxBuffer = alloc->makeArray<float>(toAlloc);
    for (size_t i = 0; i < kRGBAChannels; i++) {
        ctx->factors[i] = gradientCtxBuffer;
        gradientCtxBuffer += factorBiasFloats;
        ctx->biases[i] = gradientCtxBuffer;
        gradientCtxBuffer += factorBiasFloats;
    }

    if (positions == nullptr) {
        ctx->stopCount = skia_rs_gradient_fill_evenly_spaced(
                pmColors, static_cast<size_t>(count),
                ctx->factors[0], ctx->factors[1], ctx->factors[2], ctx->factors[3],
                ctx->biases[0], ctx->biases[1], ctx->biases[2], ctx->biases[3],
                factorBiasFloats);
        p->append(SkRasterPipelineOp::evenly_spaced_gradient, ctx);
        return;
    }

    ctx->ts = gradientCtxBuffer;
    ctx->stopCount = skia_rs_gradient_fill_arbitrary(
            pmColors, positions, static_cast<size_t>(count),
            ctx->factors[0], ctx->factors[1], ctx->factors[2], ctx->factors[3],
            ctx->biases[0], ctx->biases[1], ctx->biases[2], ctx->biases[3],
            ctx->ts, factorBiasFloats, tsForArbitraryStops);
    p->append(SkRasterPipelineOp::gradient, ctx);
}

void SkGradientBaseShader::AppendInterpolatedToDstStages(SkRasterPipeline* p,
                                                         SkArenaAlloc* alloc,
                                                         bool colorsAreOpaque,
                                                         const Interpolation& interpolation,
                                                         const SkColorSpace* intermediateColorSpace,
                                                         const SkColorSpace* dstColorSpace) {
    SkiaRsInterpolatedToDstPlan plan =
            skia_rs_gradient_plan_interpolated_to_dst(colorsAreOpaque, interpolation);

    switch (plan.unpremul) {
        case 1: p->append(SkRasterPipelineOp::unpremul);       break;
        case 2: p->append(SkRasterPipelineOp::unpremul_polar); break;
        default: break;
    }

    switch (plan.color_op) {
        case 1:
            p->append(SkRasterPipelineOp::css_lab_to_xyz);
            break;
        case 2:
            p->append(SkRasterPipelineOp::css_oklab_to_linear_srgb);
            break;
        case 3:
            p->append(SkRasterPipelineOp::css_oklab_gamut_map_to_linear_srgb);
            break;
        case 4:
            p->append(SkRasterPipelineOp::css_hcl_to_lab);
            p->append(SkRasterPipelineOp::css_lab_to_xyz);
            break;
        case 5:
            p->append(SkRasterPipelineOp::css_hcl_to_lab);
            p->append(SkRasterPipelineOp::css_oklab_to_linear_srgb);
            break;
        case 6:
            p->append(SkRasterPipelineOp::css_hcl_to_lab);
            p->append(SkRasterPipelineOp::css_oklab_gamut_map_to_linear_srgb);
            break;
        case 7:
            p->append(SkRasterPipelineOp::css_hsl_to_srgb);
            break;
        case 8:
            p->append(SkRasterPipelineOp::css_hwb_to_srgb);
            break;
        default:
            break;
    }

    if (!dstColorSpace) {
        dstColorSpace = sk_srgb_singleton();
    }
    alloc->make<SkColorSpaceXformSteps>(
                 intermediateColorSpace, plan.intermediate_alpha_type,
                 dstColorSpace, plan.dst_alpha_type)
            ->apply(p);
}

bool SkGradientBaseShader::appendStages(const SkStageRec& rec,
                                        const SkShaders::MatrixRec& mRec) const {
    SkRasterPipeline* p = rec.fPipeline;
    SkArenaAlloc* alloc = rec.fAlloc;
    SkRasterPipelineContexts::DecalTileCtx* decal_ctx = nullptr;

    std::optional<SkShaders::MatrixRec> newMRec = mRec.apply(rec, fPtsToUnit);
    if (!newMRec.has_value()) {
        return false;
    }

    SkRasterPipeline_<256> postPipeline;
    this->appendGradientStages(alloc, p, &postPipeline);

    switch (fTileMode) {
        case SkTileMode::kMirror:
            p->append(SkRasterPipelineOp::mirror_x_1);
            break;
        case SkTileMode::kRepeat:
            p->append(SkRasterPipelineOp::repeat_x_1);
            break;
        case SkTileMode::kDecal:
            decal_ctx = alloc->make<SkRasterPipelineContexts::DecalTileCtx>();
            decal_ctx->limit_x = skia_rs_gradient_decal_limit_x();
            p->append(SkRasterPipelineOp::decal_x, decal_ctx);
            [[fallthrough]];
        case SkTileMode::kClamp:
            if (!fPositions) {
                p->append(SkRasterPipelineOp::clamp_x_1);
            }
            break;
    }

    SkColor4fXformer xformedColors(this, rec.fDstCS);
    AppendGradientFillStages(p, alloc,
                             xformedColors.fColors.begin(),
                             xformedColors.fPositions,
                             xformedColors.fColors.size());
    AppendInterpolatedToDstStages(p, alloc, fColorsAreOpaque, fInterpolation,
                                  xformedColors.fIntermediateColorSpace.get(), rec.fDstCS);

    if (decal_ctx) {
        p->append(SkRasterPipelineOp::check_decal_mask, decal_ctx);
    }

    p->extend(postPipeline);
    return true;
}

bool SkGradientBaseShader::isOpaque() const {
    return skia_rs_gradient_is_opaque(fColorsAreOpaque, static_cast<int32_t>(this->getTileMode()));
}

bool SkGradientBaseShader::onAsLuminanceColor(SkColor4f* lum) const {
    *lum = skia_rs_gradient_luminance_color(fColors, fColorCount);
    return true;
}

static sk_sp<SkColorSpace> intermediate_color_space(SkGradient::Interpolation::ColorSpace cs,
                                                    SkColorSpace* dst) {
    switch (skia_rs_gradient_intermediate_cs_kind(static_cast<uint8_t>(cs))) {
        case 0: return sk_ref_sp(dst);
        case 1: return SkColorSpace::MakeSRGBLinear();
        case 2: return SkColorSpace::MakeSRGB();
        case 3: return SkColorSpace::MakeRGB(SkNamedTransferFn::kLinear, SkNamedGamut::kXYZ);
        case 4: return SkColorSpace::MakeRGB(SkNamedTransferFn::kSRGB, SkNamedGamut::kDisplayP3);
        case 5: return SkColorSpace::MakeRGB(SkNamedTransferFn::kRec2020, SkNamedGamut::kRec2020);
        case 6: {
            static SkOnce once;
            static skcms_Matrix3x3 lin_proPhoto_to_XYZ_D50;
            once([] { SkNamedPrimaries::kProPhotoRGB.toXYZD50(&lin_proPhoto_to_XYZ_D50); });
            return SkColorSpace::MakeRGB(SkNamedTransferFn::kProPhotoRGB, lin_proPhoto_to_XYZ_D50);
        }
        case 7: return SkColorSpace::MakeRGB(SkNamedTransferFn::kA98RGB, SkNamedGamut::kAdobeRGB);
        default: SkUNREACHABLE;
    }
}

SkColor4fXformer::SkColor4fXformer(const SkGradientBaseShader* shader,
                                   SkColorSpace* dst,
                                   bool forceExplicitPositions) {
    size_t colorCount = shader->colors().size();
    const auto& interpolation = shader->interpolation();

    fPositions = shader->positions().data();
    fIntermediateColorSpace = intermediate_color_space(interpolation.fColorSpace, dst);

    auto info = SkImageInfo::Make(colorCount, 1, kRGBA_F32_SkColorType, kUnpremul_SkAlphaType);
    auto dstInfo = info.makeColorSpace(fIntermediateColorSpace);
    auto srcInfo = info.makeColorSpace(shader->colorSpace());

    ColorStorage convertedColors;
    convertedColors.reset(colorCount);
    SkAssertResult(SkConvertPixels(dstInfo,
                                   convertedColors.begin(),
                                   info.minRowBytes(),
                                   srcInfo,
                                   shader->colors().data(),
                                   info.minRowBytes()));

    size_t maxOut = colorCount * 2;
    fColors.reset(maxOut);
    fPositionStorage.reset(maxOut);

    SkiaRsXformColorsResult res = skia_rs_gradient_xform_colors(
            convertedColors.data(), colorCount,
            shader->positions().data(),
            interpolation,
            shader->firstStopIsImplicit(),
            shader->lastStopIsImplicit(),
            forceExplicitPositions,
            fColors.data(),
            fPositionStorage.data(),
            maxOut);

    fColors.resize(res.out_count);
    if (res.has_out_positions) {
        fPositionStorage.resize(res.out_count);
        fPositions = fPositionStorage.data();
    } else {
        fPositionStorage.clear();
    }
}

void SkGradientBaseShader::commonAsAGradient(GradientInfo* info) const {
    if (info) {
        const int colorCount = SkToInt(fColorCount);
        if (info->fColorCount >= colorCount) {
            if (info->fColors) {
                for (int i = 0; i < colorCount; ++i) {
                    info->fColors[i] = fColors[i];
                }
            }
            if (info->fColorOffsets) {
                for (int i = 0; i < colorCount; ++i) {
                    info->fColorOffsets[i] = this->getPos(i);
                }
            }
        }
        info->fColorCount = colorCount;
        info->fTileMode = fTileMode;
        info->fPremulInterp = this->interpolateInPremul();
    }
}

bool SkGradientBaseShader::ValidGradient(SkSpan<const SkColor4f> colors,
                                         SkTileMode tileMode,
                                         const Interpolation& interpolation) {
    return skia_rs_gradient_valid(
            colors.data() != nullptr, colors.size(),
            static_cast<uint32_t>(tileMode), interpolation);
}

sk_sp<SkShader> SkGradientBaseShader::MakeDegenerateGradient(const SkGradient::Colors& c) {
    SkiaRsDegenerateGradientResult res = skia_rs_gradient_classify_degenerate(
            c.colors().data(), c.colors().size(),
            c.positions().data(), c.positions().size(),
            static_cast<int32_t>(c.tileMode()));
    switch (res.kind) {
        case 1: return SkShaders::Empty();
        case 2: return SkShaders::Color(res.color, c.colorSpace());
        default:
            SkDEBUGFAIL("Should not be reached");
            return nullptr;
    }
}

// ============================================================================
// SkLinearGradient.cpp
// ============================================================================

static SkMatrix pts_to_unit_matrix(const SkPoint pts[2]) {
    SkMatrix matrix;
    skia_rs_linear_gradient_unit_matrix(pts[0], pts[1], &matrix);
    return matrix;
}

SkLinearGradient::SkLinearGradient(const SkPoint pts[2], const SkGradient& desc)
        : SkGradientBaseShader(desc, pts_to_unit_matrix(pts)), fStart(pts[0]), fEnd(pts[1]) {}

sk_sp<SkFlattenable> SkLinearGradient::CreateProc(SkReadBuffer& buffer) {
    SkGradientScope scope;
    SkMatrix legacyLocalMatrix, *lmPtr = nullptr;
    auto grad = scope.unflatten(buffer, &legacyLocalMatrix);
    if (!grad) {
        return nullptr;
    }
    if (!legacyLocalMatrix.isIdentity()) {
        lmPtr = &legacyLocalMatrix;
    }
    SkPoint pts[2];
    pts[0] = buffer.readPoint();
    pts[1] = buffer.readPoint();
    return SkShaders::LinearGradient(pts, *grad, lmPtr);
}

void SkLinearGradient::flatten(SkWriteBuffer& buffer) const {
    this->INHERITED::flatten(buffer);
    buffer.writePoint(fStart);
    buffer.writePoint(fEnd);
}

void SkLinearGradient::appendGradientStages(SkArenaAlloc*, SkRasterPipeline*,
                                            SkRasterPipeline*) const {
    // No extra stage needed for linear gradients.
}

SkShaderBase::GradientType SkLinearGradient::asGradient(GradientInfo* info,
                                                        SkMatrix* localMatrix) const {
    if (info) {
        commonAsAGradient(info);
        info->fPoint[0] = fStart;
        info->fPoint[1] = fEnd;
    }
    if (localMatrix) {
        *localMatrix = SkMatrix::I();
    }
    return GradientType::kLinear;
}

sk_sp<SkShader> SkShaders::LinearGradient(const SkPoint pts[2], const SkGradient& grad,
                                          const SkMatrix* lm) {
    const auto& colors = grad.colors();
    SkPoint p0 = pts ? pts[0] : SkPoint{0, 0};
    SkPoint p1 = pts ? pts[1] : SkPoint{0, 0};
    bool lmInv = !lm || lm->invert(nullptr);
    switch (skia_rs_linear_gradient_classify(
            pts != nullptr, p0, p1,
            colors.colors().data() != nullptr, colors.colors().size(),
            static_cast<uint32_t>(colors.tileMode()), grad.interpolation(),
            lm != nullptr, lmInv)) {
        case 0: return nullptr;
        case 1: return SkShaders::Color(colors.colors().front(), colors.colorSpace());
        case 2: return SkGradientBaseShader::MakeDegenerateGradient(colors);
        default: {
            sk_sp<SkShader> s = sk_make_sp<SkLinearGradient>(pts, grad);
            return s->makeWithLocalMatrix(lm ? *lm : SkMatrix::I());
        }
    }
}

void SkRegisterLinearGradientShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkLinearGradient);
}

// ============================================================================
// SkRadialGradient.cpp
// ============================================================================

static SkMatrix rad_to_unit_matrix(const SkPoint& center, SkScalar radius) {
    SkMatrix matrix;
    skia_rs_radial_gradient_unit_matrix(center, radius, &matrix);
    return matrix;
}

SkRadialGradient::SkRadialGradient(const SkPoint& center, SkScalar radius, const SkGradient& desc)
        : SkGradientBaseShader(desc, rad_to_unit_matrix(center, radius))
        , fCenter(center)
        , fRadius(radius) {}

SkShaderBase::GradientType SkRadialGradient::asGradient(GradientInfo* info,
                                                        SkMatrix* localMatrix) const {
    if (info) {
        commonAsAGradient(info);
        info->fPoint[0] = fCenter;
        info->fRadius[0] = fRadius;
    }
    if (localMatrix) {
        *localMatrix = SkMatrix::I();
    }
    return GradientType::kRadial;
}

sk_sp<SkFlattenable> SkRadialGradient::CreateProc(SkReadBuffer& buffer) {
    SkGradientScope scope;
    SkMatrix legacyLocalMatrix, *lmPtr = nullptr;
    auto grad = scope.unflatten(buffer, &legacyLocalMatrix);
    if (!grad) {
        return nullptr;
    }
    if (!legacyLocalMatrix.isIdentity()) {
        lmPtr = &legacyLocalMatrix;
    }
    const SkPoint center = buffer.readPoint();
    const SkScalar radius = buffer.readScalar();
    return SkShaders::RadialGradient(center, radius, *grad, lmPtr);
}

void SkRadialGradient::flatten(SkWriteBuffer& buffer) const {
    this->SkGradientBaseShader::flatten(buffer);
    buffer.writePoint(fCenter);
    buffer.writeScalar(fRadius);
}

void SkRadialGradient::appendGradientStages(SkArenaAlloc*, SkRasterPipeline* p,
                                            SkRasterPipeline*) const {
    p->append(SkRasterPipelineOp::xy_to_radius);
}

sk_sp<SkShader> SkShaders::RadialGradient(SkPoint center, float radius,
                                          const SkGradient& grad, const SkMatrix* lm) {
    const auto& colors = grad.colors();
    bool lmInv = !lm || lm->invert(nullptr);
    switch (skia_rs_radial_gradient_classify(
            radius,
            colors.colors().data() != nullptr, colors.colors().size(),
            static_cast<uint32_t>(colors.tileMode()), grad.interpolation(),
            lm != nullptr, lmInv)) {
        case 0: return nullptr;
        case 1: return SkShaders::Color(colors.colors().front(), colors.colorSpace());
        case 2: return SkGradientBaseShader::MakeDegenerateGradient(colors);
        default: {
            sk_sp<SkShader> s = sk_make_sp<SkRadialGradient>(center, radius, grad);
            return s->makeWithLocalMatrix(lm ? *lm : SkMatrix::I());
        }
    }
}

void SkRegisterRadialGradientShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkRadialGradient);
}

// ============================================================================
// SkSweepGradient.cpp
// ============================================================================

static SkMatrix sweep_unit_matrix(const SkPoint& center) {
    SkMatrix m;
    skia_rs_sweep_gradient_unit_matrix(center, &m);
    return m;
}

SkSweepGradient::SkSweepGradient(const SkPoint& center,
                                 SkScalar t0,
                                 SkScalar t1,
                                 const SkGradient& desc)
        : SkGradientBaseShader(desc, sweep_unit_matrix(center))
        , fCenter(center)
        , fTBias(skia_rs_sweep_gradient_t_coeff(t0, t1).t_bias)
        , fTScale(skia_rs_sweep_gradient_t_coeff(t0, t1).t_scale) {
    SkASSERT(t0 < t1);
}

SkShaderBase::GradientType SkSweepGradient::asGradient(GradientInfo* info,
                                                       SkMatrix* localMatrix) const {
    if (info) {
        commonAsAGradient(info);
        info->fPoint[0] = fCenter;
        info->fPoint[1].fX = fTScale;
        info->fPoint[1].fY = fTBias;
    }
    if (localMatrix) {
        *localMatrix = SkMatrix::I();
    }
    return GradientType::kSweep;
}

sk_sp<SkFlattenable> SkSweepGradient::CreateProc(SkReadBuffer& buffer) {
    SkGradientScope scope;
    SkMatrix legacyLocalMatrix, *lmPtr = nullptr;
    auto grad = scope.unflatten(buffer, &legacyLocalMatrix);
    if (!grad) {
        return nullptr;
    }
    if (!legacyLocalMatrix.isIdentity()) {
        lmPtr = &legacyLocalMatrix;
    }
    const SkPoint center = buffer.readPoint();
    const auto tBias  = buffer.readScalar(),
               tScale = buffer.readScalar();
    SkiaRsSweepAngles angles = skia_rs_sweep_gradient_angles_from_t(tBias, tScale);
    return SkShaders::SweepGradient(center, angles.start_angle, angles.end_angle, *grad, lmPtr);
}

void SkSweepGradient::flatten(SkWriteBuffer& buffer) const {
    this->SkGradientBaseShader::flatten(buffer);
    buffer.writePoint(fCenter);
    buffer.writeScalar(fTBias);
    buffer.writeScalar(fTScale);
}

void SkSweepGradient::appendGradientStages(SkArenaAlloc* alloc, SkRasterPipeline* p,
                                           SkRasterPipeline*) const {
    p->append(SkRasterPipelineOp::xy_to_unit_angle);
    SkMatrix m;
    skia_rs_sweep_gradient_stage_matrix(fTBias, fTScale, &m);
    p->appendMatrix(alloc, m);
}

sk_sp<SkShader> SkShaders::SweepGradient(SkPoint center, float startAngle, float endAngle,
                                         const SkGradient& grad, const SkMatrix* lm) {
    const auto& colors = grad.colors();
    const auto& interp = grad.interpolation();
    bool lmInv = !lm || lm->invert(nullptr);
    SkiaRsSweepGradientPlan plan = skia_rs_sweep_gradient_classify(
            startAngle, endAngle,
            colors.colors().data() != nullptr, colors.colors().size(),
            static_cast<uint32_t>(colors.tileMode()), interp,
            lm != nullptr, lmInv);

    switch (plan.action) {
        case 0: return nullptr;
        case 1: return SkShaders::Color(colors.colors().front(), colors.colorSpace());
        case 2: return SkGradientBaseShader::MakeDegenerateGradient(colors);
        case 3: {
            static constexpr float clampPos[3] = {0, 1, 1};
            SkSpan<const SkColor4f> srcColors = colors.colors();
            const SkColor4f reColors[3] = {
                srcColors.front(), srcColors.front(), srcColors.back()
            };
            SkGradient::Colors newColors = {
                reColors, clampPos, colors.tileMode(), colors.colorSpace()
            };
            return SkShaders::SweepGradient(center, 0, endAngle, {newColors, interp}, lm);
        }
        default: {
            const SkGradient newGrad{
                {colors.colors(), colors.positions(),
                 static_cast<SkTileMode>(plan.effective_tile_mode), colors.colorSpace()},
                interp
            };
            sk_sp<SkShader> s = sk_make_sp<SkSweepGradient>(center, plan.t0, plan.t1, newGrad);
            return s->makeWithLocalMatrix(lm ? *lm : SkMatrix::I());
        }
    }
}

void SkRegisterSweepGradientShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkSweepGradient);
}

// ============================================================================
// SkConicalGradient.cpp
// ============================================================================

bool SkConicalGradient::FocalData::set(SkScalar r0, SkScalar r1, SkMatrix* matrix) {
    return skia_rs_conical_focal_data_set(this, r0, r1, matrix);
}

std::optional<SkMatrix> SkConicalGradient::MapToUnitX(const SkPoint& startCenter,
                                                      const SkPoint& endCenter) {
    SkMatrix out;
    if (skia_rs_conical_map_to_unit_x(startCenter, endCenter, &out)) {
        return out;
    }
    return std::nullopt;
}

sk_sp<SkShader> SkConicalGradient::Create(const SkPoint& c0,
                                          SkScalar r0,
                                          const SkPoint& c1,
                                          SkScalar r1,
                                          const SkGradient& desc,
                                          const SkMatrix* localMatrix) {
    SkiaRsConicalCreateResult res;
    skia_rs_conical_create_params(c0, r0, c1, r1, &res);
    if (!res.valid) {
        return nullptr;
    }

    sk_sp<SkShader> s = sk_make_sp<SkConicalGradient>(
            c0, r0, c1, r1, desc, res.conical_type, res.gradient_matrix, res.focal_data);
    return s->makeWithLocalMatrix(localMatrix ? *localMatrix : SkMatrix::I());
}

SkConicalGradient::SkConicalGradient(const SkPoint& start,
                                     SkScalar startRadius,
                                     const SkPoint& end,
                                     SkScalar endRadius,
                                     const SkGradient& desc,
                                     Type type,
                                     const SkMatrix& gradientMatrix,
                                     const FocalData& data)
        : SkGradientBaseShader(desc, gradientMatrix)
        , fCenter1(start)
        , fCenter2(end)
        , fRadius1(startRadius)
        , fRadius2(endRadius)
        , fType(type) {
    SkASSERT(fCenter1 != fCenter2 || fRadius1 != fRadius2);
    if (type == Type::kFocal) {
        fFocalData = data;
    }
}

bool SkConicalGradient::isOpaque() const {
    return false;
}

SkShaderBase::GradientType SkConicalGradient::asGradient(GradientInfo* info,
                                                         SkMatrix* localMatrix) const {
    if (info) {
        commonAsAGradient(info);
        info->fPoint[0] = fCenter1;
        info->fPoint[1] = fCenter2;
        info->fRadius[0] = fRadius1;
        info->fRadius[1] = fRadius2;
    }
    if (localMatrix) {
        *localMatrix = SkMatrix::I();
    }
    return GradientType::kConical;
}

sk_sp<SkFlattenable> SkConicalGradient::CreateProc(SkReadBuffer& buffer) {
    SkGradientScope scope;
    SkMatrix legacyLocalMatrix, *lmPtr = nullptr;
    auto grad = scope.unflatten(buffer, &legacyLocalMatrix);
    if (!grad) {
        return nullptr;
    }
    if (!legacyLocalMatrix.isIdentity()) {
        lmPtr = &legacyLocalMatrix;
    }
    SkPoint c1 = buffer.readPoint();
    SkPoint c2 = buffer.readPoint();
    SkScalar r1 = buffer.readScalar();
    SkScalar r2 = buffer.readScalar();

    if (!buffer.isValid()) {
        return nullptr;
    }
    return SkShaders::TwoPointConicalGradient(c1, r1, c2, r2, *grad, lmPtr);
}

void SkConicalGradient::flatten(SkWriteBuffer& buffer) const {
    this->SkGradientBaseShader::flatten(buffer);
    buffer.writePoint(fCenter1);
    buffer.writePoint(fCenter2);
    buffer.writeScalar(fRadius1);
    buffer.writeScalar(fRadius2);
}

void SkConicalGradient::appendGradientStages(SkArenaAlloc* alloc,
                                             SkRasterPipeline* p,
                                             SkRasterPipeline* postPipeline) const {
    if (fType == Type::kRadial) {
        p->append(SkRasterPipelineOp::xy_to_radius);
        SkMatrix m;
        skia_rs_conical_radial_stage_matrix(fRadius1, fRadius2, &m);
        p->appendMatrix(alloc, m);
        return;
    }

    if (fType == Type::kStrip) {
        auto* ctx = alloc->make<SkRasterPipelineContexts::Conical2PtCtx>();
        ctx->fP0 = skia_rs_conical_strip_p0(fRadius1, fCenter1, fCenter2);
        p->append(SkRasterPipelineOp::xy_to_2pt_conical_strip, ctx);
        p->append(SkRasterPipelineOp::mask_2pt_conical_nan, ctx);
        postPipeline->append(SkRasterPipelineOp::apply_vector_mask, &ctx->fMask);
        return;
    }

    SkiaRsConicalFocalStagePlan plan;
    skia_rs_conical_plan_focal_stages(&fFocalData, &plan);

    auto* ctx = alloc->make<SkRasterPipelineContexts::Conical2PtCtx>();
    ctx->fP0 = plan.p0;
    ctx->fP1 = plan.p1;

    switch (plan.focal_op) {
        case 0: p->append(SkRasterPipelineOp::xy_to_2pt_conical_focal_on_circle); break;
        case 1: p->append(SkRasterPipelineOp::xy_to_2pt_conical_well_behaved, ctx); break;
        case 2: p->append(SkRasterPipelineOp::xy_to_2pt_conical_smaller, ctx); break;
        default: p->append(SkRasterPipelineOp::xy_to_2pt_conical_greater, ctx); break;
    }

    if (plan.mask_degenerates) {
        p->append(SkRasterPipelineOp::mask_2pt_conical_degenerates, ctx);
    }
    if (plan.negate_x) {
        p->append(SkRasterPipelineOp::negate_x);
    }
    if (plan.compensate_focal) {
        p->append(SkRasterPipelineOp::alter_2pt_conical_compensate_focal, ctx);
    }
    if (plan.unswap) {
        p->append(SkRasterPipelineOp::alter_2pt_conical_unswap);
    }
    if (plan.apply_post_mask) {
        postPipeline->append(SkRasterPipelineOp::apply_vector_mask, &ctx->fMask);
    }
}

sk_sp<SkShader> SkShaders::TwoPointConicalGradient(SkPoint start, float startRadius,
                                                   SkPoint end, float endRadius,
                                                   const SkGradient& grad, const SkMatrix* lm) {
    const auto& colors = grad.colors();
    const auto& interp = grad.interpolation();
    bool lmInv = !lm || lm->invert(nullptr);
    switch (skia_rs_conical_classify_two_point(
            start, startRadius, end, endRadius,
            colors.colors().data() != nullptr, colors.colors().size(),
            static_cast<uint32_t>(colors.tileMode()), interp,
            lm != nullptr, lmInv)) {
        case 0: return nullptr;
        case 1: {
            static constexpr SkScalar circlePos[3] = {0, 1, 1};
            SkColor4f front = colors.colors().front();
            SkColor4f reColors[3] = {front, front, colors.colors().back()};
            SkGradient::Colors newColors = {
                reColors, circlePos, colors.tileMode(), colors.colorSpace()
            };
            return SkShaders::RadialGradient(start, endRadius, {newColors, interp}, lm);
        }
        case 2: return SkGradientBaseShader::MakeDegenerateGradient(colors);
        case 3: return SkShaders::RadialGradient(start, endRadius, grad, lm);
        default: break;
    }

    SkColor4f tmp2Colors[2];
    SkSpan<const SkColor4f> c4 = colors.colors();
    SkSpan<const float> pos = colors.positions();
    if (c4.size() == 1) {
        tmp2Colors[0] = tmp2Colors[1] = c4[0];
        c4 = {tmp2Colors};
        pos = {};
    }
    SkGradient::Colors newColors = {
        c4, pos, colors.tileMode(), colors.colorSpace()
    };
    return SkConicalGradient::Create(start, startRadius, end, endRadius, {newColors, interp}, lm);
}

void SkRegisterConicalGradientShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkConicalGradient);
    SkFlattenable::Register("SkTwoPointConicalGradient", SkConicalGradient::CreateProc);
}
