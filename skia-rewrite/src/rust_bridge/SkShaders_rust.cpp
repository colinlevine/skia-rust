/*
 * Safe Rust bridge for:
 *   src/shaders/SkShader.cpp
 *   src/shaders/SkShaderBase.cpp
 *   src/shaders/SkColorShader.cpp
 *   src/shaders/SkEmptyShader.cpp
 *   src/shaders/SkBlendShader.cpp
 *   src/shaders/SkColorFilterShader.cpp
 *   src/shaders/SkCoordClampShader.cpp
 *   src/shaders/SkLocalMatrixShader.cpp
 *   src/shaders/SkTransformShader.cpp
 *   src/shaders/SkTriColorShader.cpp
 *   src/shaders/SkWorkingColorSpaceShader.cpp
 *   src/shaders/SkPictureShader.cpp
 *   src/shaders/SkPerlinNoiseShaderImpl.cpp
 *   src/shaders/SkGainmapShader.cpp
 * (Phase 1 Layer 4.1 skia-rs-shaders, GN target :core).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSize.h"
#include "include/core/SkString.h"
#include "include/core/SkSurface.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkPerlinNoiseShader.h"
#include "include/effects/SkRuntimeEffect.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/SkGainmapShader.h"
#include "include/private/SkTo.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkKnownRuntimeEffects.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"
#include "src/shaders/SkBlendShader.h"
#include "src/shaders/SkColorFilterShader.h"
#include "src/shaders/SkColorShader.h"
#include "src/shaders/SkCoordClampShader.h"
#include "src/shaders/SkEmptyShader.h"
#include "src/shaders/SkLocalMatrixShader.h"
#include "src/shaders/SkPerlinNoiseShaderImpl.h"
#include "src/shaders/SkPerlinNoiseShaderType.h"
#include "src/shaders/SkPictureShader.h"
#include "src/shaders/SkShaderBase.h"
#include "src/shaders/SkTransformShader.h"
#include "src/shaders/SkTriColorShader.h"
#include "src/shaders/SkWorkingColorSpaceShader.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <optional>
#include <tuple>
#include <utility>

class SkDiscardableMemory;

namespace {

constexpr bool kIsAndroidFramework =
#if defined(SK_BUILD_FOR_ANDROID_FRAMEWORK)
        true;
#else
        false;
#endif

struct SkiaRsMatrixRec {
    SkMatrix ctm;
    SkMatrix total_local_matrix;
    SkMatrix pending_local_matrix;
    bool total_matrix_is_valid;
    bool ctm_applied;
};

struct SkiaRsMatrixRecApplyResult {
    bool valid;
    bool seed_shader;
    SkMatrix total_matrix;
    SkiaRsMatrixRec new_rec;
};

struct SkiaRsMatrixRecFpResult {
    SkMatrix total_matrix;
    bool valid;
};

struct SkiaRsColorFilterShaderStagePlan {
    bool append_scale_alpha;
    bool filter_shader_is_opaque;
};

struct SkiaRsCoordClampCtx {
    float min_x, min_y;
    float max_x, max_y;
};

struct SkiaRsTriColorUpdateResult {
    SkMatrix m33;
    float m43[12];
    bool valid;
};

struct SkiaRsPictureCachedImageParams {
    bool success;
    SkISize tile_size;
    SkSize tile_scale;
    SkMatrix matrix_for_draw;
    int32_t img_color_type;
};

struct SkiaRsPerlinStitchData {
    int32_t width;
    int32_t wrap_x;
    int32_t height;
    int32_t wrap_y;
};

struct SkiaRsPerlinPaintingDataTables {
    int32_t seed;
    uint8_t lattice_selector[256];
    uint16_t noise[4][256][2];
    SkISize tile_size;
    float base_frequency_x;
    float base_frequency_y;
    SkiaRsPerlinStitchData stitch_data_init;
};

struct SkiaRsGainmapInfoInput {
    SkColor4f gainmap_ratio_min;
    SkColor4f gainmap_ratio_max;
    SkColor4f gainmap_gamma;
    SkColor4f epsilon_sdr;
    SkColor4f epsilon_hdr;
    float display_ratio_sdr;
    float display_ratio_hdr;
    int32_t base_image_type;
    int32_t gainmap_type;
};

struct SkiaRsGainmapUniforms {
    SkMatrix base_rect_to_dst;
    SkMatrix gainmap_rect_to_dst;
    float weight;
    SkColor4f log_ratio_min;
    SkColor4f log_ratio_max;
    SkColor4f gainmap_gamma;
    SkColor4f epsilon_base;
    SkColor4f epsilon_other;
    int32_t no_gamma;
    int32_t single_channel;
    int32_t gainmap_is_alpha;
    int32_t gainmap_is_red;
    int32_t is_apple;
    float apple_g;
    float apple_h;
};

}  // namespace

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkColor4f) == 16);
static_assert(sizeof(SkPMColor4f) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkSize) == 8);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkShaders::MatrixRec) == sizeof(SkiaRsMatrixRec));
static_assert(sizeof(SkiaRsMatrixRec) == 124);
static_assert(sizeof(SkRasterPipelineContexts::CoordClampCtx) == sizeof(SkiaRsCoordClampCtx));

extern "C" {
void skia_rs_shader_concat_local_matrices(
        const SkMatrix* parent_lm, const SkMatrix* child_lm, bool android_framework_order,
        SkMatrix* out);
void skia_rs_matrix_rec_apply(
        const SkiaRsMatrixRec* rec, const SkMatrix* post_inv, SkiaRsMatrixRecApplyResult* out);
void skia_rs_matrix_rec_apply_for_fp(
        const SkiaRsMatrixRec* rec, const SkMatrix* post_inv, SkiaRsMatrixRecFpResult* out);
void skia_rs_matrix_rec_applied(const SkiaRsMatrixRec* rec, SkiaRsMatrixRec* out);
void skia_rs_matrix_rec_concat(
        const SkiaRsMatrixRec* rec, const SkMatrix* m, bool android_framework_order,
        SkiaRsMatrixRec* out);
SkColor4f skia_rs_shader_finalize_luminance_color(SkColor4f color);
bool skia_rs_shader_context_matrix_is_eligible(const SkMatrix* total_matrix);
bool skia_rs_color_shader_validate_and_pin(SkColor4f color, SkColor4f* out_pinned);
SkColor4f skia_rs_color_shader_from_sk_color(uint32_t color);

int32_t skia_rs_blend_shader_classify(int32_t mode, bool has_dst, bool has_src);
bool skia_rs_blend_shader_is_opaque(int32_t mode, bool src_is_opaque, bool dst_is_opaque);
bool skia_rs_blend_shader_is_valid_mode(uint32_t mode);

int32_t skia_rs_color_filter_shader_classify(bool has_shader, bool has_filter);
bool skia_rs_color_filter_shader_is_opaque(
        bool shader_is_opaque, float alpha, bool filter_alpha_unchanged);
SkiaRsColorFilterShaderStagePlan skia_rs_color_filter_shader_plan_stages(
        float alpha, bool shader_is_opaque);

bool skia_rs_coord_clamp_is_valid(bool has_shader, SkRect subset);
SkiaRsCoordClampCtx skia_rs_coord_clamp_make_ctx(SkRect subset);

int32_t skia_rs_working_cs_shader_classify(
        bool has_shader, bool has_input_cs, bool has_output_cs, bool work_in_unpremul);
SkColor4f skia_rs_working_cs_make_opaque(SkColor4f color);

bool skia_rs_transform_shader_update(
        bool allow_perspective, const SkMatrix* matrix, float out_storage[9]);

void skia_rs_tri_color_shader_update(
        bool use_persp, const SkMatrix* ctm_inv, SkPoint p0, SkPoint p1, SkPoint p2,
        SkPMColor4f c0, SkPMColor4f c1, SkPMColor4f c2, SkiaRsTriColorUpdateResult* out);

bool skia_rs_picture_shader_should_be_empty(
        bool has_picture, SkRect cull_rect, bool has_tile, SkRect tile);
void skia_rs_picture_shader_cached_image_info(
        SkRect bounds, const SkMatrix* total_m, int32_t dst_color_type, int32_t max_texture_size,
        SkiaRsPictureCachedImageParams* out);
void skia_rs_picture_shader_tile_local_matrix(SkSize tile_scale, SkMatrix* out);

int32_t skia_rs_perlin_classify(
        int32_t noise_type, float base_x, float base_y, int32_t num_octaves, float seed,
        const SkISize* tile_size);
void skia_rs_perlin_compute_painting_data(
        SkISize tile_size, float seed, float base_freq_x, float base_freq_y,
        SkiaRsPerlinPaintingDataTables* out);

void skia_rs_gainmap_compute_uniforms(
        SkRect base_rect, SkRect gainmap_rect, SkRect dst_rect, int32_t gainmap_color_type,
        const SkiaRsGainmapInfoInput* info, float dst_hdr_ratio, SkiaRsGainmapUniforms* out);
}  // extern "C"

// ============================================================================
// SkShader.cpp
// ============================================================================

SkImage* SkShader::isAImage(SkMatrix* localMatrix, SkTileMode xy[2]) const {
    return as_SB(this)->onIsAImage(localMatrix, xy);
}

sk_sp<SkShader> SkShader::makeWithLocalMatrix(const SkMatrix& localMatrix) const {
    const SkMatrix* lm = &localMatrix;

    sk_sp<SkShader> baseShader;
    SkMatrix otherLocalMatrix;
    sk_sp<SkShader> proxy = as_SB(this)->makeAsALocalMatrixShader(&otherLocalMatrix);
    if (proxy) {
        skia_rs_shader_concat_local_matrices(
                &localMatrix, &otherLocalMatrix, kIsAndroidFramework, &otherLocalMatrix);
        lm = &otherLocalMatrix;
        baseShader = proxy;
    } else {
        baseShader = sk_ref_sp(const_cast<SkShader*>(this));
    }

    return sk_make_sp<SkLocalMatrixShader>(std::move(baseShader), *lm);
}

sk_sp<SkShader> SkShader::makeWithColorFilter(sk_sp<SkColorFilter> filter) const {
    return SkColorFilterShader::Make(sk_ref_sp(this), 1.0f, std::move(filter));
}

sk_sp<SkShader> SkShader::makeWithWorkingColorSpace(sk_sp<SkColorSpace> inputCS,
                                                    sk_sp<SkColorSpace> outputCS) const {
    return SkWorkingColorSpaceShader::Make(
            sk_ref_sp(this), std::move(inputCS), std::move(outputCS), /*workInUnpremul=*/false);
}

// ============================================================================
// SkShaderBase.cpp
// ============================================================================

namespace SkShaders {

MatrixRec::MatrixRec(const SkMatrix& ctm) : fCTM(ctm) {}

std::optional<MatrixRec> MatrixRec::apply(const SkStageRec& rec, const SkMatrix& postInv) const {
    SkiaRsMatrixRec in{fCTM, fTotalLocalMatrix, fPendingLocalMatrix,
                       fTotalMatrixIsValid, fCTMApplied};
    SkiaRsMatrixRecApplyResult out;
    skia_rs_matrix_rec_apply(&in, &postInv, &out);
    if (!out.valid) {
        return {};
    }
    if (out.seed_shader) {
        rec.fPipeline->append(SkRasterPipelineOp::seed_shader);
    }
    rec.fPipeline->appendMatrix(rec.fAlloc, out.total_matrix);
    return MatrixRec{out.new_rec.ctm,
                     out.new_rec.total_local_matrix,
                     out.new_rec.pending_local_matrix,
                     out.new_rec.total_matrix_is_valid,
                     out.new_rec.ctm_applied};
}

std::tuple<SkMatrix, bool> MatrixRec::applyForFragmentProcessor(const SkMatrix& postInv) const {
    SkASSERT(!fCTMApplied);
    SkiaRsMatrixRec in{fCTM, fTotalLocalMatrix, fPendingLocalMatrix,
                       fTotalMatrixIsValid, fCTMApplied};
    SkiaRsMatrixRecFpResult out;
    skia_rs_matrix_rec_apply_for_fp(&in, &postInv, &out);
    return {out.total_matrix, out.valid};
}

MatrixRec MatrixRec::applied() const {
    SkiaRsMatrixRec in{fCTM, fTotalLocalMatrix, fPendingLocalMatrix,
                       fTotalMatrixIsValid, fCTMApplied};
    SkiaRsMatrixRec out;
    skia_rs_matrix_rec_applied(&in, &out);
    return MatrixRec{out.ctm,
                     out.total_local_matrix,
                     out.pending_local_matrix,
                     out.total_matrix_is_valid,
                     out.ctm_applied};
}

MatrixRec MatrixRec::concat(const SkMatrix& m) const {
    SkiaRsMatrixRec in{fCTM, fTotalLocalMatrix, fPendingLocalMatrix,
                       fTotalMatrixIsValid, fCTMApplied};
    SkiaRsMatrixRec out;
    skia_rs_matrix_rec_concat(&in, &m, kIsAndroidFramework, &out);
    return MatrixRec{out.ctm,
                     out.total_local_matrix,
                     out.pending_local_matrix,
                     out.total_matrix_is_valid,
                     out.ctm_applied};
}

}  // namespace SkShaders

SkShaderBase::SkShaderBase() {}

SkShaderBase::~SkShaderBase() = default;

void SkShaderBase::flatten(SkWriteBuffer& buffer) const { this->INHERITED::flatten(buffer); }

bool SkShaderBase::asLuminanceColor(SkColor4f* colorPtr) const {
    SkColor4f storage;
    if (nullptr == colorPtr) {
        colorPtr = &storage;
    }
    if (this->onAsLuminanceColor(colorPtr)) {
        *colorPtr = skia_rs_shader_finalize_luminance_color(*colorPtr);
        return true;
    }
    return false;
}

SkShaderBase::Context* SkShaderBase::makeContext(const ContextRec& rec, SkArenaAlloc* alloc) const {
#ifdef SK_ENABLE_LEGACY_SHADERCONTEXT
    auto totalMatrix = rec.fMatrixRec.totalMatrix();
    if (!skia_rs_shader_context_matrix_is_eligible(&totalMatrix)) {
        return nullptr;
    }
    return this->onMakeContext(rec, alloc);
#else
    return nullptr;
#endif
}

SkShaderBase::Context::Context(const SkShaderBase& shader, const ContextRec& rec)
        : fShader(shader) {
    SkASSERT(!rec.fMatrixRec.totalMatrix().hasPerspective());
    fPaintAlpha = rec.fPaintAlpha;
}

SkShaderBase::Context::~Context() {}

bool SkShaderBase::ContextRec::isLegacyCompatible(SkColorSpace* shaderColorSpace) const {
    SkAlphaType shaderAT = kPremul_SkAlphaType, dstAT = kPremul_SkAlphaType;
    return 0 ==
           SkColorSpaceXformSteps{shaderColorSpace, shaderAT, fDstColorSpace, dstAT}.fFlags.mask();
}

sk_sp<SkShader> SkShaderBase::makeAsALocalMatrixShader(SkMatrix*) const { return nullptr; }

bool SkShaderBase::appendRootStages(const SkStageRec& rec, const SkMatrix& ctm) const {
    return this->appendStages(rec, SkShaders::MatrixRec(ctm));
}

sk_sp<SkShader> SkShaderBase::makeWithCTM(const SkMatrix& postM) const {
    return sk_sp<SkShader>(new SkCTMShader(sk_ref_sp(this), postM));
}

sk_sp<SkShader> SkShaderBase::makeInvertAlpha() const {
    return this->makeWithColorFilter(SkColorFilters::Blend(0xFFFFFFFF, SkBlendMode::kSrcOut));
}

// ============================================================================
// SkColorShader.cpp
// ============================================================================

namespace {

sk_sp<SkFlattenable> legacy_color4_create_proc(SkReadBuffer& buffer) {
    if (buffer.validate(buffer.isVersionLT(SkPicturePriv::Version::kCombineColorShaders))) {
        SkColor4f color;
        sk_sp<SkColorSpace> colorSpace;
        buffer.readColor4f(&color);
        if (buffer.readBool()) {
            sk_sp<SkData> data = buffer.readByteArrayAsData();
            colorSpace = data ? SkColorSpace::Deserialize(data->data(), data->size()) : nullptr;
        }
        return SkShaders::Color(color, std::move(colorSpace));
    }
    return nullptr;
}

}  // namespace

sk_sp<SkFlattenable> SkColorShader::CreateProc(SkReadBuffer& buffer) {
    if (buffer.isVersionLT(SkPicturePriv::Version::kCombineColorShaders)) {
        return SkShaders::Color(buffer.readColor());
    } else {
        SkColor4f color;
        buffer.readColor4f(&color);
        return SkShaders::Color(color, SkColorSpace::MakeSRGB());
    }
}

void SkColorShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeColor4f(fColor);
}

bool SkColorShader::appendStages(const SkStageRec& rec, const SkShaders::MatrixRec&) const {
    SkColor4f color = fColor;
    SkColorSpaceXformSteps(sk_srgb_singleton(), kUnpremul_SkAlphaType,
                           rec.fDstCS,          kPremul_SkAlphaType).apply(color.vec());
    rec.fPipeline->appendConstantColor(rec.fAlloc, color.vec());
    return true;
}

void SkRegisterColorShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkColorShader);
    SkFlattenable::Register("SkColorShader4", legacy_color4_create_proc);
}

namespace SkShaders {

sk_sp<SkShader> Color(SkColor color) {
    return Color(skia_rs_color_shader_from_sk_color(color), SkColorSpace::MakeSRGB());
}

sk_sp<SkShader> Color(const SkColor4f& color, sk_sp<SkColorSpace> space) {
    SkColor4f srgb;
    if (!skia_rs_color_shader_validate_and_pin(color, &srgb)) {
        return nullptr;
    }
    SkColorSpaceXformSteps(space.get(),         kUnpremul_SkAlphaType,
                           sk_srgb_singleton(), kUnpremul_SkAlphaType).apply(srgb.vec());
    return sk_make_sp<SkColorShader>(srgb);
}

}  // namespace SkShaders

// ============================================================================
// SkEmptyShader.cpp
// ============================================================================

sk_sp<SkFlattenable> SkEmptyShader::CreateProc(SkReadBuffer&) {
    return SkShaders::Empty();
}

sk_sp<SkShader> SkShaders::Empty() { return sk_make_sp<SkEmptyShader>(); }

void SkRegisterEmptyShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkEmptyShader);
}

// ============================================================================
// SkBlendShader.cpp
// ============================================================================

sk_sp<SkFlattenable> SkBlendShader::CreateProc(SkReadBuffer& buffer) {
    sk_sp<SkShader> dst(buffer.readShader());
    sk_sp<SkShader> src(buffer.readShader());
    if (!buffer.validate(dst && src)) {
        return nullptr;
    }

    unsigned mode = buffer.read32();
    if (mode == kCustom_SkBlendMode) {
        sk_sp<SkBlender> blender = buffer.readBlender();
        if (buffer.validate(blender != nullptr)) {
            return SkShaders::Blend(std::move(blender), std::move(dst), std::move(src));
        }
    } else {
        if (buffer.validate(skia_rs_blend_shader_is_valid_mode(mode))) {
            return SkShaders::Blend(static_cast<SkBlendMode>(mode), std::move(dst), std::move(src));
        }
    }
    return nullptr;
}

bool SkBlendShader::isOpaque() const {
    return skia_rs_blend_shader_is_opaque(
            static_cast<int32_t>(fMode), fSrc->isOpaque(), fDst->isOpaque());
}

void SkBlendShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fDst.get());
    buffer.writeFlattenable(fSrc.get());
    buffer.write32((int)fMode);
}

static float* append_two_shaders(const SkStageRec& rec,
                                 const SkShaders::MatrixRec& mRec,
                                 SkShader* s0,
                                 SkShader* s1) {
    struct Storage {
        float fCoords[2 * SkRasterPipelineContexts::kMaxStride];
        float fRes0[4 * SkRasterPipelineContexts::kMaxStride];
    };
    auto storage = rec.fAlloc->make<Storage>();

    if (mRec.rasterPipelineCoordsAreSeeded()) {
        rec.fPipeline->append(SkRasterPipelineOp::store_src_rg, storage->fCoords);
    }
    if (!as_SB(s0)->appendStages(rec, mRec)) {
        return nullptr;
    }
    rec.fPipeline->append(SkRasterPipelineOp::store_src, storage->fRes0);

    if (mRec.rasterPipelineCoordsAreSeeded()) {
        rec.fPipeline->append(SkRasterPipelineOp::load_src_rg, storage->fCoords);
    }
    if (!as_SB(s1)->appendStages(rec, mRec)) {
        return nullptr;
    }
    return storage->fRes0;
}

bool SkBlendShader::appendStages(const SkStageRec& rec, const SkShaders::MatrixRec& mRec) const {
    float* res0 = append_two_shaders(rec, mRec, fDst.get(), fSrc.get());
    if (!res0) {
        return false;
    }
    rec.fPipeline->append(SkRasterPipelineOp::load_dst, res0);
    SkBlendMode_AppendStages(fMode, rec.fPipeline);
    return true;
}

sk_sp<SkShader> SkShaders::Blend(SkBlendMode mode, sk_sp<SkShader> dst, sk_sp<SkShader> src) {
    switch (skia_rs_blend_shader_classify(static_cast<int32_t>(mode), dst != nullptr, src != nullptr)) {
        case 0: return nullptr;
        case 1: return Color(0);
        case 2: return dst;
        case 3: return src;
        default:
            return sk_sp<SkShader>(new SkBlendShader(mode, std::move(dst), std::move(src)));
    }
}

sk_sp<SkShader> SkShaders::Blend(sk_sp<SkBlender> blender,
                                 sk_sp<SkShader> dst,
                                 sk_sp<SkShader> src) {
    using namespace SkKnownRuntimeEffects;

    if (!src || !dst) {
        return nullptr;
    }
    if (!blender) {
        return SkShaders::Blend(SkBlendMode::kSrcOver, std::move(dst), std::move(src));
    }
    if (std::optional<SkBlendMode> mode = as_BB(blender)->asBlendMode()) {
        return SkShaders::Blend(*mode, std::move(dst), std::move(src));
    }

    const SkRuntimeEffect* blendEffect = GetKnownRuntimeEffect(StableKey::kBlend);
    SkRuntimeEffect::ChildPtr children[] = {std::move(src), std::move(dst), std::move(blender)};
    return blendEffect->makeShader(/*uniforms=*/{}, children);
}

void SkRegisterBlendShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkBlendShader);
    SkFlattenable::Register("SkShader_Blend", SkBlendShader::CreateProc);
}

// ============================================================================
// SkColorFilterShader.cpp
// ============================================================================

SkColorFilterShader::SkColorFilterShader(sk_sp<SkShader> shader,
                                         float alpha,
                                         sk_sp<SkColorFilter> filter)
        : fShader(std::move(shader))
        , fFilter(as_CFB_sp(std::move(filter)))
        , fAlpha(alpha) {
    SkASSERT(fShader);
    SkASSERT(fFilter);
}

sk_sp<SkShader> SkColorFilterShader::Make(sk_sp<SkShader> shader,
                                          float alpha,
                                          sk_sp<SkColorFilter> filter) {
    switch (skia_rs_color_filter_shader_classify(shader != nullptr, filter != nullptr)) {
        case 0: return nullptr;
        case 1: return shader;
        default:
            return sk_sp(new SkColorFilterShader(std::move(shader), alpha, std::move(filter)));
    }
}

sk_sp<SkFlattenable> SkColorFilterShader::CreateProc(SkReadBuffer& buffer) {
    auto shader = buffer.readShader();
    auto filter = buffer.readColorFilter();
    return Make(std::move(shader), 1.0f, std::move(filter));
}

bool SkColorFilterShader::isOpaque() const {
    return skia_rs_color_filter_shader_is_opaque(
            fShader->isOpaque(), fAlpha, as_CFB(fFilter)->isAlphaUnchanged());
}

void SkColorFilterShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fShader.get());
    SkASSERT(fAlpha == 1.0f);
    buffer.writeFlattenable(fFilter.get());
}

bool SkColorFilterShader::appendStages(const SkStageRec& rec,
                                       const SkShaders::MatrixRec& mRec) const {
    if (!as_SB(fShader)->appendStages(rec, mRec)) {
        return false;
    }
    SkiaRsColorFilterShaderStagePlan plan =
            skia_rs_color_filter_shader_plan_stages(fAlpha, fShader->isOpaque());
    if (plan.append_scale_alpha) {
        rec.fPipeline->append(SkRasterPipelineOp::scale_1_float, rec.fAlloc->make<float>(fAlpha));
    }
    if (!fFilter->appendStages(rec, plan.filter_shader_is_opaque)) {
        return false;
    }
    return true;
}

// ============================================================================
// SkCoordClampShader.cpp
// ============================================================================

sk_sp<SkFlattenable> SkCoordClampShader::CreateProc(SkReadBuffer& buffer) {
    sk_sp<SkShader> shader(buffer.readShader());
    SkRect subset = buffer.readRect();
    if (!buffer.validate(SkToBool(shader))) {
        return nullptr;
    }
    return SkShaders::CoordClamp(std::move(shader), subset);
}

void SkCoordClampShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fShader.get());
    buffer.writeRect(fSubset);
}

bool SkCoordClampShader::appendStages(const SkStageRec& rec,
                                      const SkShaders::MatrixRec& mRec) const {
    std::optional<SkShaders::MatrixRec> childMRec = mRec.apply(rec);
    if (!childMRec.has_value()) {
        return false;
    }
    SkiaRsCoordClampCtx ctx = skia_rs_coord_clamp_make_ctx(fSubset);
    auto clampCtx = rec.fAlloc->make<SkRasterPipelineContexts::CoordClampCtx>();
    *clampCtx = {ctx.min_x, ctx.min_y, ctx.max_x, ctx.max_y};
    rec.fPipeline->append(SkRasterPipelineOp::clamp_x_and_y, clampCtx);
    return as_SB(fShader)->appendStages(rec, *childMRec);
}

void SkRegisterCoordClampShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkCoordClampShader);
    SkFlattenable::Register("SkShader_CoordClamp", SkCoordClampShader::CreateProc);
}

sk_sp<SkShader> SkShaders::CoordClamp(sk_sp<SkShader> shader, const SkRect& subset) {
    if (!skia_rs_coord_clamp_is_valid(shader != nullptr, subset)) {
        return nullptr;
    }
    return sk_make_sp<SkCoordClampShader>(std::move(shader), subset);
}

// ============================================================================
// SkLocalMatrixShader.cpp
// ============================================================================

bool SkLocalMatrixShader::isConstant(SkColor4f* color) const {
    return as_SB(fWrappedShader)->isConstant(color);
}

SkShaderBase::GradientType SkLocalMatrixShader::asGradient(GradientInfo* info,
                                                           SkMatrix* localMatrix) const {
    GradientType type = as_SB(fWrappedShader)->asGradient(info, localMatrix);
    if (type != SkShaderBase::GradientType::kNone && localMatrix) {
        skia_rs_shader_concat_local_matrices(
                &fLocalMatrix, localMatrix, kIsAndroidFramework, localMatrix);
    }
    return type;
}

sk_sp<SkFlattenable> SkLocalMatrixShader::CreateProc(SkReadBuffer& buffer) {
    SkMatrix lm;
    buffer.readMatrix(&lm);
    auto baseShader(buffer.readShader());
    if (!baseShader) {
        return nullptr;
    }
    return baseShader->makeWithLocalMatrix(lm);
}

void SkLocalMatrixShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeMatrix(fLocalMatrix);
    buffer.writeFlattenable(fWrappedShader.get());
}

#ifdef SK_ENABLE_LEGACY_SHADERCONTEXT
SkShaderBase::Context* SkLocalMatrixShader::onMakeContext(const ContextRec& rec,
                                                          SkArenaAlloc* alloc) const {
    return as_SB(fWrappedShader)->makeContext(ContextRec::Concat(rec, fLocalMatrix), alloc);
}
#endif

SkImage* SkLocalMatrixShader::onIsAImage(SkMatrix* outMatrix, SkTileMode* mode) const {
    SkMatrix imageMatrix;
    SkImage* image = fWrappedShader->isAImage(&imageMatrix, mode);
    if (image && outMatrix) {
        skia_rs_shader_concat_local_matrices(
                &fLocalMatrix, &imageMatrix, kIsAndroidFramework, outMatrix);
    }
    return image;
}

bool SkLocalMatrixShader::onAsLuminanceColor(SkColor4f* color) const {
    return as_SB(fWrappedShader)->asLuminanceColor(color);
}

bool SkLocalMatrixShader::appendStages(const SkStageRec& rec,
                                       const SkShaders::MatrixRec& mRec) const {
    return as_SB(fWrappedShader)->appendStages(rec, mRec.concat(fLocalMatrix));
}

SkCTMShader::SkCTMShader(sk_sp<SkShader> proxy, const SkMatrix& ctm)
        : fProxyShader(std::move(proxy)), fCTM(ctm) {}

bool SkCTMShader::isConstant(SkColor4f* color) const {
    return as_SB(fProxyShader)->isConstant(color);
}

SkShaderBase::GradientType SkCTMShader::asGradient(GradientInfo* info,
                                                   SkMatrix* localMatrix) const {
    return as_SB(fProxyShader)->asGradient(info, localMatrix);
}

bool SkCTMShader::appendStages(const SkStageRec& rec, const SkShaders::MatrixRec&) const {
    return as_SB(fProxyShader)->appendRootStages(rec, fCTM);
}

sk_sp<SkFlattenable> SkCTMShader::CreateProc(SkReadBuffer&) {
    SkASSERT(false);
    return nullptr;
}

// ============================================================================
// SkTransformShader.cpp
// ============================================================================

SkTransformShader::SkTransformShader(const SkShaderBase& shader, bool allowPerspective)
        : fShader{shader}, fAllowPerspective{allowPerspective} {
    SkMatrix::I().get9(fMatrixStorage);
}

bool SkTransformShader::update(const SkMatrix& matrix) {
    return skia_rs_transform_shader_update(fAllowPerspective, &matrix, fMatrixStorage);
}

bool SkTransformShader::appendStages(const SkStageRec& rec,
                                     const SkShaders::MatrixRec& mRec) const {
    SkASSERT(!mRec.hasPendingMatrix());
    std::optional<SkShaders::MatrixRec> childMRec = mRec.apply(rec);
    if (!childMRec.has_value()) {
        return false;
    }
    childMRec->markTotalMatrixInvalid();

    auto type = fAllowPerspective ? SkRasterPipelineOp::matrix_perspective
                                  : SkRasterPipelineOp::matrix_2x3;
    rec.fPipeline->append(type, fMatrixStorage);
    return fShader->appendStages(rec, *childMRec);
}

// ============================================================================
// SkTriColorShader.cpp
// ============================================================================

bool SkTriColorShader::appendStages(const SkStageRec& rec, const SkShaders::MatrixRec&) const {
    rec.fPipeline->append(SkRasterPipelineOp::seed_shader);
    if (fUsePersp) {
        rec.fPipeline->append(SkRasterPipelineOp::matrix_perspective, &fM33);
    }
    rec.fPipeline->append(SkRasterPipelineOp::matrix_4x3, &fM43);
    return true;
}

bool SkTriColorShader::update(const SkMatrix& ctmInv,
                              const SkPoint pts[],
                              const SkPMColor4f colors[],
                              int index0,
                              int index1,
                              int index2) {
    SkiaRsTriColorUpdateResult res;
    skia_rs_tri_color_shader_update(
            fUsePersp, &ctmInv,
            pts[index0], pts[index1], pts[index2],
            colors[index0], colors[index1], colors[index2],
            &res);
    if (!res.valid) {
        return false;
    }
    fM33 = res.m33;
    std::memcpy(fM43.fMat.data(), res.m43, sizeof(res.m43));
    return true;
}

// ============================================================================
// SkWorkingColorSpaceShader.cpp
// ============================================================================

sk_sp<SkShader> SkWorkingColorSpaceShader::Make(sk_sp<SkShader> shader,
                                                sk_sp<SkColorSpace> inputCS,
                                                sk_sp<SkColorSpace> outputCS,
                                                bool workInUnpremul) {
    switch (skia_rs_working_cs_shader_classify(
            shader != nullptr, inputCS != nullptr, outputCS != nullptr, workInUnpremul)) {
        case 0: return nullptr;
        case 1: return shader;
        default:
            return sk_sp(new SkWorkingColorSpaceShader(std::move(shader),
                                                       std::move(inputCS),
                                                       std::move(outputCS),
                                                       workInUnpremul));
    }
}

bool SkWorkingColorSpaceShader::appendStages(const SkStageRec& rec,
                                             const SkShaders::MatrixRec& mRec) const {
    sk_sp<SkColorSpace> dstCS = sk_ref_sp(rec.fDstCS);
    if (!dstCS) {
        dstCS = SkColorSpace::MakeSRGB();
    }

    const SkAlphaType dstAT = kPremul_SkAlphaType;
    auto [inputCS, outputCS, workingAT] = this->workingSpace(dstCS, dstAT);

    SkColorInfo dst    = {rec.fDstColorType, dstAT,     dstCS},
                input  = {rec.fDstColorType, workingAT, inputCS},
                output = {rec.fDstColorType, workingAT, outputCS};

    const auto* dstToInput  = rec.fAlloc->make<SkColorSpaceXformSteps>(dst, input);
    const auto* outputToDst = rec.fAlloc->make<SkColorSpaceXformSteps>(output, dst);

    SkColor4f paintColorInWorkingSpace = skia_rs_working_cs_make_opaque(rec.fPaintColor);
    dstToInput->apply(paintColorInWorkingSpace.vec());

    SkStageRec workingRec = {rec.fPipeline,
                             rec.fAlloc,
                             rec.fDstColorType,
                             fInputSpace.get(),
                             paintColorInWorkingSpace,
                             rec.fSurfaceProps,
                             rec.fDstBounds};

    if (!as_SB(fShader)->appendStages(workingRec, mRec)) {
        return false;
    }

    outputToDst->apply(rec.fPipeline);
    return true;
}

void SkWorkingColorSpaceShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fShader.get());
    buffer.writeBool(fWorkInUnpremul);

    buffer.writeBool(SkToBool(fInputSpace));
    if (fInputSpace) {
        buffer.writeDataAsByteArray(fInputSpace->serialize().get());
    }

    buffer.writeBool(SkToBool(fOutputSpace));
    if (fOutputSpace) {
        buffer.writeDataAsByteArray(fOutputSpace->serialize().get());
    }
}

sk_sp<SkFlattenable> SkWorkingColorSpaceShader::CreateProc(SkReadBuffer& buffer) {
    sk_sp<SkShader> shader(buffer.readShader());

    const bool legacyWorkingCS = buffer.isVersionLT(SkPicturePriv::kWorkingColorSpaceOutput);
    bool workInUnpremul = !legacyWorkingCS && buffer.readBool();

    sk_sp<SkColorSpace> inputSpace;
    if (legacyWorkingCS || buffer.readBool()) {
        auto data = buffer.readByteArrayAsData();
        if (!buffer.validate(data != nullptr)) {
            return nullptr;
        }
        inputSpace = SkColorSpace::Deserialize(data->data(), data->size());
        if (!buffer.validate(inputSpace != nullptr)) {
            return nullptr;
        }
    }

    sk_sp<SkColorSpace> outputSpace;
    if (!legacyWorkingCS && buffer.readBool()) {
        auto data = buffer.readByteArrayAsData();
        if (!buffer.validate(data != nullptr)) {
            return nullptr;
        }
        outputSpace = SkColorSpace::Deserialize(data->data(), data->size());
        if (!buffer.validate(outputSpace != nullptr)) {
            return nullptr;
        }
    }

    return Make(std::move(shader), std::move(inputSpace), std::move(outputSpace), workInUnpremul);
}

void SkRegisterWorkingColorSpaceShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkWorkingColorSpaceShader);
}

// ============================================================================
// SkPictureShader.cpp
// ============================================================================

sk_sp<SkShader> SkPicture::makeShader(SkTileMode tmx, SkTileMode tmy, SkFilterMode filter,
                                      const SkMatrix* localMatrix, const SkRect* tile) const {
    if (localMatrix && !localMatrix->invert()) {
        return nullptr;
    }
    return SkPictureShader::Make(sk_ref_sp(this), tmx, tmy, filter, localMatrix, tile);
}

namespace {
static unsigned gImageFromPictureKeyNamespaceLabel;

struct ImageFromPictureKey : public SkResourceCache::Key {
public:
    ImageFromPictureKey(SkColorSpace* colorSpace, SkColorType colorType,
                        uint32_t pictureID, const SkRect& subset,
                        SkSize scale, const SkSurfaceProps& surfaceProps)
        : fColorSpaceXYZHash(colorSpace->toXYZD50Hash())
        , fColorSpaceTransferFnHash(colorSpace->transferFnHash())
        , fColorType(static_cast<uint32_t>(colorType))
        , fSubset(subset)
        , fScale(scale)
        , fSurfaceProps(surfaceProps)
    {
        static const size_t keySize = sizeof(fColorSpaceXYZHash) +
                                      sizeof(fColorSpaceTransferFnHash) +
                                      sizeof(fColorType) +
                                      sizeof(fSubset) +
                                      sizeof(fScale) +
                                      sizeof(fSurfaceProps);
        SkASSERT(sizeof(uint32_t) * (&fEndOfStruct - &fColorSpaceXYZHash) == keySize);
        this->init(&gImageFromPictureKeyNamespaceLabel,
                   SkPicturePriv::MakeSharedID(pictureID),
                   keySize);
    }

private:
    uint32_t       fColorSpaceXYZHash;
    uint32_t       fColorSpaceTransferFnHash;
    uint32_t       fColorType;
    SkRect         fSubset;
    SkSize         fScale;
    SkSurfaceProps fSurfaceProps;

    SkDEBUGCODE(uint32_t fEndOfStruct;)
};

struct ImageFromPictureRec : public SkResourceCache::Rec {
    ImageFromPictureRec(const ImageFromPictureKey& key, sk_sp<SkImage> image)
        : fKey(key)
        , fImage(std::move(image)) {}

    ImageFromPictureKey fKey;
    sk_sp<SkImage> fImage;

    const Key& getKey() const override { return fKey; }
    size_t bytesUsed() const override {
        return sizeof(fKey) + (size_t)fImage->width() * fImage->height() * 4;
    }
    const char* getCategory() const override { return "bitmap-shader"; }
    SkDiscardableMemory* diagnostic_only_getDiscardable() const override { return nullptr; }

    static bool Visitor(const SkResourceCache::Rec& baseRec, void* contextShader) {
        const ImageFromPictureRec& rec = static_cast<const ImageFromPictureRec&>(baseRec);
        sk_sp<SkImage>* result = reinterpret_cast<sk_sp<SkImage>*>(contextShader);
        *result = rec.fImage;
        return true;
    }
};

}  // namespace

SkPictureShader::SkPictureShader(sk_sp<SkPicture> picture,
                                 SkTileMode tmx,
                                 SkTileMode tmy,
                                 SkFilterMode filter,
                                 const SkRect* tile)
        : fPicture(std::move(picture))
        , fTile(tile ? *tile : fPicture->cullRect())
        , fTmx(tmx)
        , fTmy(tmy)
        , fFilter(filter) {}

sk_sp<SkShader> SkPictureShader::Make(sk_sp<SkPicture> picture, SkTileMode tmx, SkTileMode tmy,
                                      SkFilterMode filter, const SkMatrix* lm, const SkRect* tile) {
    SkRect cull = picture ? picture->cullRect() : SkRect::MakeEmpty();
    SkRect tileRect = tile ? *tile : SkRect::MakeEmpty();
    if (skia_rs_picture_shader_should_be_empty(picture != nullptr, cull, tile != nullptr, tileRect)) {
        return SkShaders::Empty();
    }
    return SkLocalMatrixShader::MakeWrapped<SkPictureShader>(lm,
                                                             std::move(picture),
                                                             tmx, tmy,
                                                             filter,
                                                             tile);
}

sk_sp<SkFlattenable> SkPictureShader::CreateProc(SkReadBuffer& buffer) {
    SkMatrix lm;
    if (buffer.isVersionLT(SkPicturePriv::Version::kNoShaderLocalMatrix)) {
        buffer.readMatrix(&lm);
    }
    auto tmx = buffer.read32LE(SkTileMode::kLastTileMode);
    auto tmy = buffer.read32LE(SkTileMode::kLastTileMode);
    SkRect tile = buffer.readRect();

    sk_sp<SkPicture> picture;

    SkFilterMode filter = SkFilterMode::kNearest;
    if (buffer.isVersionLT(SkPicturePriv::kNoFilterQualityShaders_Version)) {
        if (buffer.isVersionLT(SkPicturePriv::kPictureShaderFilterParam_Version)) {
            bool didSerialize = buffer.readBool();
            if (didSerialize) {
                picture = SkPicturePriv::MakeFromBuffer(buffer);
            }
        } else {
            unsigned legacyFilter = buffer.read32();
            if (legacyFilter <= (unsigned)SkFilterMode::kLast) {
                filter = (SkFilterMode)legacyFilter;
            }
            picture = SkPicturePriv::MakeFromBuffer(buffer);
        }
    } else {
        filter = buffer.read32LE(SkFilterMode::kLast);
        picture = SkPicturePriv::MakeFromBuffer(buffer);
    }
    return SkPictureShader::Make(picture, tmx, tmy, filter, &lm, &tile);
}

void SkPictureShader::flatten(SkWriteBuffer& buffer) const {
    buffer.write32((unsigned)fTmx);
    buffer.write32((unsigned)fTmy);
    buffer.writeRect(fTile);
    buffer.write32((unsigned)fFilter);
    SkPicturePriv::Flatten(fPicture, buffer);
}

static sk_sp<SkColorSpace> ref_or_srgb(SkColorSpace* cs) {
    return cs ? sk_ref_sp(cs) : SkColorSpace::MakeSRGB();
}

SkPictureShader::CachedImageInfo SkPictureShader::CachedImageInfo::Make(
        const SkRect& bounds,
        const SkMatrix& totalM,
        SkColorType dstColorType,
        SkColorSpace* dstColorSpace,
        const int maxTextureSize,
        const SkSurfaceProps& propsIn) {
    SkSurfaceProps props = propsIn.cloneWithPixelGeometry(kUnknown_SkPixelGeometry);

    SkiaRsPictureCachedImageParams params;
    skia_rs_picture_shader_cached_image_info(
            bounds, &totalM, static_cast<int32_t>(dstColorType), maxTextureSize, &params);
    if (!params.success) {
        return {false, {}, {}, {}, {}};
    }

    auto imgCS = ref_or_srgb(dstColorSpace);
    return {true,
            params.tile_scale,
            params.matrix_for_draw,
            SkImageInfo::Make(params.tile_size,
                              static_cast<SkColorType>(params.img_color_type),
                              kPremul_SkAlphaType,
                              imgCS),
            props};
}

sk_sp<SkImage> SkPictureShader::CachedImageInfo::makeImage(sk_sp<SkSurface> surf,
                                                           const SkPicture* pict) const {
    if (!surf) {
        return nullptr;
    }

    auto canvas = surf->getCanvas();
    canvas->concat(matrixForDraw);
    canvas->drawPicture(pict);

    return surf->makeTemporaryImage();
}

sk_sp<SkShader> SkPictureShader::rasterShader(const SkMatrix& totalM,
                                              SkColorType dstColorType,
                                              SkColorSpace* dstColorSpace,
                                              const SkSurfaceProps& propsIn) const {
    const int maxTextureSize_NotUsedForCPU = 0;
    CachedImageInfo info = CachedImageInfo::Make(fTile,
                                                 totalM,
                                                 dstColorType, dstColorSpace,
                                                 maxTextureSize_NotUsedForCPU,
                                                 propsIn);
    if (!info.success) {
        return nullptr;
    }

    ImageFromPictureKey key(info.imageInfo.colorSpace(), info.imageInfo.colorType(),
                            fPicture->uniqueID(), fTile, info.tileScale, info.props);

    sk_sp<SkImage> image;
    if (!SkResourceCache::Find(key, ImageFromPictureRec::Visitor, &image)) {
        image = info.makeImage(SkSurfaces::Raster(info.imageInfo, &info.props), fPicture.get());
        if (!image) {
            return nullptr;
        }

        SkResourceCache::Add(new ImageFromPictureRec(key, image));
        SkPicturePriv::AddedToCache(fPicture.get());
    }
    SkMatrix lm;
    skia_rs_picture_shader_tile_local_matrix(info.tileScale, &lm);
    return image->makeShader(fTmx, fTmy, SkSamplingOptions(fFilter), &lm);
}

bool SkPictureShader::appendStages(const SkStageRec& rec, const SkShaders::MatrixRec& mRec) const {
    auto& bitmapShader = *rec.fAlloc->make<sk_sp<SkShader>>();
    bitmapShader = this->rasterShader(mRec.totalMatrix(),
                                      rec.fDstColorType,
                                      rec.fDstCS,
                                      rec.fSurfaceProps);
    if (!bitmapShader) {
        return false;
    }
    return as_SB(bitmapShader)->appendStages(rec, mRec);
}

#ifdef SK_ENABLE_LEGACY_SHADERCONTEXT
SkShaderBase::Context* SkPictureShader::onMakeContext(const ContextRec& rec,
                                                      SkArenaAlloc* alloc) const {
    sk_sp<SkShader> bitmapShader = this->rasterShader(
            rec.fMatrixRec.totalMatrix(), rec.fDstColorType, rec.fDstColorSpace, rec.fProps);
    if (!bitmapShader) {
        return nullptr;
    }
    return as_SB(bitmapShader)->makeContext(rec, alloc);
}
#endif

// ============================================================================
// SkPerlinNoiseShaderImpl.cpp
// ============================================================================

SkPerlinNoiseShader::SkPerlinNoiseShader(SkPerlinNoiseShaderType type,
                                         SkScalar baseFrequencyX,
                                         SkScalar baseFrequencyY,
                                         int numOctaves,
                                         SkScalar seed,
                                         const SkISize* tileSize)
        : fType(type)
        , fBaseFrequencyX(baseFrequencyX)
        , fBaseFrequencyY(baseFrequencyY)
        , fNumOctaves(numOctaves > kMaxOctaves ? kMaxOctaves : numOctaves)
        , fSeed(seed)
        , fTileSize(nullptr == tileSize ? SkISize::Make(0, 0) : *tileSize)
        , fStitchTiles(!fTileSize.isEmpty()) {
    SkASSERT(numOctaves >= 0 && numOctaves <= kMaxOctaves);
    SkASSERT(fBaseFrequencyX >= 0);
    SkASSERT(fBaseFrequencyY >= 0);
    static_assert(SkPerlinNoiseShader::kBlockSize == 256);
}

sk_sp<SkFlattenable> SkPerlinNoiseShader::CreateProc(SkReadBuffer& buffer) {
    SkPerlinNoiseShaderType type = buffer.read32LE(SkPerlinNoiseShaderType::kLast);

    SkScalar freqX = buffer.readScalar();
    SkScalar freqY = buffer.readScalar();
    int octaves = buffer.read32LE<int>(kMaxOctaves);

    SkScalar seed = buffer.readScalar();
    SkISize tileSize;
    tileSize.fWidth = buffer.readInt();
    tileSize.fHeight = buffer.readInt();

    switch (type) {
        case SkPerlinNoiseShaderType::kFractalNoise:
            return SkShaders::MakeFractalNoise(freqX, freqY, octaves, seed, &tileSize);
        case SkPerlinNoiseShaderType::kTurbulence:
            return SkShaders::MakeTurbulence(freqX, freqY, octaves, seed, &tileSize);
        default:
            buffer.validate(false);
            return nullptr;
    }
}

void SkPerlinNoiseShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeInt((int)fType);
    buffer.writeScalar(fBaseFrequencyX);
    buffer.writeScalar(fBaseFrequencyY);
    buffer.writeInt(fNumOctaves);
    buffer.writeScalar(fSeed);
    buffer.writeInt(fTileSize.fWidth);
    buffer.writeInt(fTileSize.fHeight);
}

bool SkPerlinNoiseShader::appendStages(const SkStageRec& rec,
                                       const SkShaders::MatrixRec& mRec) const {
    std::optional<SkShaders::MatrixRec> newMRec = mRec.apply(rec);
    if (!newMRec.has_value()) {
        return false;
    }

    fInitPaintingDataOnce([&] {
        auto pd = std::make_unique<PaintingData>(SkISize::Make(0, 0), 0.0f, 0.0f, 0.0f);
        SkiaRsPerlinPaintingDataTables tables;
        skia_rs_perlin_compute_painting_data(
                fTileSize, fSeed, fBaseFrequencyX, fBaseFrequencyY, &tables);
        pd->fSeed = tables.seed;
        std::memcpy(pd->fLatticeSelector, tables.lattice_selector, sizeof(pd->fLatticeSelector));
        std::memcpy(pd->fNoise, tables.noise, sizeof(pd->fNoise));
        pd->fTileSize = tables.tile_size;
        pd->fBaseFrequency = {tables.base_frequency_x, tables.base_frequency_y};
        pd->fStitchDataInit.fWidth = tables.stitch_data_init.width;
        pd->fStitchDataInit.fWrapX = tables.stitch_data_init.wrap_x;
        pd->fStitchDataInit.fHeight = tables.stitch_data_init.height;
        pd->fStitchDataInit.fWrapY = tables.stitch_data_init.wrap_y;
        const_cast<SkPerlinNoiseShader*>(this)->fPaintingData = std::move(pd);
    });

    auto* ctx = rec.fAlloc->make<SkRasterPipelineContexts::PerlinNoiseCtx>();
    ctx->noiseType = fType;
    ctx->baseFrequencyX = fPaintingData->fBaseFrequency.fX;
    ctx->baseFrequencyY = fPaintingData->fBaseFrequency.fY;
    ctx->stitchDataInX = fPaintingData->fStitchDataInit.fWidth;
    ctx->stitchDataInY = fPaintingData->fStitchDataInit.fHeight;
    ctx->stitching = fStitchTiles;
    ctx->numOctaves = fNumOctaves;
    ctx->latticeSelector = fPaintingData->fLatticeSelector;
    ctx->noiseData = &fPaintingData->fNoise[0][0][0];

    rec.fPipeline->append(SkRasterPipelineOp::perlin_noise, ctx);
    return true;
}

void SkRegisterPerlinNoiseShaderFlattenable() {
    SK_REGISTER_FLATTENABLE(SkPerlinNoiseShader);
    SkFlattenable::Register("SkPerlinNoiseShaderImpl", SkPerlinNoiseShader::CreateProc);
}

namespace SkShaders {

sk_sp<SkShader> MakeFractalNoise(SkScalar baseFrequencyX,
                                 SkScalar baseFrequencyY,
                                 int numOctaves,
                                 SkScalar seed,
                                 const SkISize* tileSize) {
    switch (skia_rs_perlin_classify(
            static_cast<int32_t>(SkPerlinNoiseShaderType::kFractalNoise),
            baseFrequencyX, baseFrequencyY, numOctaves, seed, tileSize)) {
        case 0:
            return nullptr;
        case 1: {
            constexpr SkColor4f kTransparentGray = {0.5f, 0.5f, 0.5f, 0.5f};
            return SkShaders::Color(kTransparentGray, /* colorSpace= */ nullptr);
        }
        default:
            return sk_sp<SkShader>(new SkPerlinNoiseShader(SkPerlinNoiseShaderType::kFractalNoise,
                                                           baseFrequencyX,
                                                           baseFrequencyY,
                                                           numOctaves,
                                                           seed,
                                                           tileSize));
    }
}

sk_sp<SkShader> MakeTurbulence(SkScalar baseFrequencyX,
                               SkScalar baseFrequencyY,
                               int numOctaves,
                               SkScalar seed,
                               const SkISize* tileSize) {
    switch (skia_rs_perlin_classify(
            static_cast<int32_t>(SkPerlinNoiseShaderType::kTurbulence),
            baseFrequencyX, baseFrequencyY, numOctaves, seed, tileSize)) {
        case 0:
            return nullptr;
        case 2:
            return SkShaders::Color(SkColors::kTransparent, /* colorSpace= */ nullptr);
        default:
            return sk_sp<SkShader>(new SkPerlinNoiseShader(SkPerlinNoiseShaderType::kTurbulence,
                                                           baseFrequencyX,
                                                           baseFrequencyY,
                                                           numOctaves,
                                                           seed,
                                                           tileSize));
    }
}

}  // namespace SkShaders

// ============================================================================
// SkGainmapShader.cpp
// ============================================================================

static constexpr char gGainmapSKSL[] =
        "uniform shader base;"
        "uniform shader gainmap;"
        "uniform half4 logRatioMin;"
        "uniform half4 logRatioMax;"
        "uniform half4 gainmapGamma;"
        "uniform half4 epsilonBase;"
        "uniform half4 epsilonOther;"
        "uniform half W;"
        "uniform int gainmapIsAlpha;"
        "uniform int gainmapIsRed;"
        "uniform int singleChannel;"
        "uniform int noGamma;"
        "uniform int isApple;"
        "uniform half appleG;"
        "uniform half appleH;"
        ""
        "half4 main(float2 coord) {"
            "half4 S = base.eval(coord);"
            "half4 G = gainmap.eval(coord);"
            "if (gainmapIsAlpha == 1) {"
                "G = half4(G.a, G.a, G.a, 1.0);"
            "}"
            "if (gainmapIsRed == 1) {"
                "G = half4(G.r, G.r, G.r, 1.0);"
            "}"
            "if (singleChannel == 1) {"
                "half L;"
                "if (isApple == 1) {"
                    "L = pow(G.r, appleG);"
                    "L = log(1.0 + (appleH - 1.0) * pow(G.r, appleG));"
                "} else if (noGamma == 1) {"
                    "L = mix(logRatioMin.r, logRatioMax.r, G.r);"
                "} else {"
                    "L = mix(logRatioMin.r, logRatioMax.r, pow(G.r, gainmapGamma.r));"
                "}"
                "half3 H = (S.rgb + epsilonBase.rgb) * exp(L * W) - epsilonOther.rgb;"
                "return half4(H.r, H.g, H.b, S.a);"
            "} else {"
                "half3 L;"
                "if (isApple == 1) {"
                    "L = pow(G.rgb, half3(appleG));"
                    "L = log(half3(1.0) + (appleH - 1.0) * L);"
                "} else if (noGamma == 1) {"
                    "L = mix(logRatioMin.rgb, logRatioMax.rgb, G.rgb);"
                "} else {"
                    "L = mix(logRatioMin.rgb, logRatioMax.rgb, pow(G.rgb, gainmapGamma.rgb));"
                "}"
                "half3 H = (S.rgb + epsilonBase.rgb) * exp(L * W) - epsilonOther.rgb;"
                "return half4(H.r, H.g, H.b, S.a);"
            "}"
        "}";

static sk_sp<SkRuntimeEffect> gainmap_apply_effect() {
    static const SkRuntimeEffect* effect =
            SkRuntimeEffect::MakeForShader(SkString(gGainmapSKSL), {}).effect.release();
    SkASSERT(effect);
    return sk_ref_sp(effect);
}

sk_sp<SkShader> SkGainmapShader::Make(const sk_sp<const SkImage>& baseImage,
                                      const SkRect& baseRect,
                                      const SkSamplingOptions& baseSamplingOptions,
                                      const sk_sp<const SkImage>& gainmapImage,
                                      const SkRect& gainmapRect,
                                      const SkSamplingOptions& gainmapSamplingOptions,
                                      const SkGainmapInfo& gainmapInfo,
                                      const SkRect& dstRect,
                                      float dstHdrRatio,
                                      sk_sp<SkColorSpace> dstColorSpace) {
    return Make(baseImage, baseRect, baseSamplingOptions, gainmapImage, gainmapRect,
                gainmapSamplingOptions, gainmapInfo, dstRect, dstHdrRatio);
}

sk_sp<SkShader> SkGainmapShader::Make(const sk_sp<const SkImage>& baseImage,
                                      const SkRect& baseRect,
                                      const SkSamplingOptions& baseSamplingOptions,
                                      const sk_sp<const SkImage>& gainmapImage,
                                      const SkRect& gainmapRect,
                                      const SkSamplingOptions& gainmapSamplingOptions,
                                      const SkGainmapInfo& gainmapInfo,
                                      const SkRect& dstRect,
                                      float dstHdrRatio) {
    sk_sp<SkColorSpace> baseColorSpace =
            baseImage->colorSpace() ? baseImage->refColorSpace() : SkColorSpace::MakeSRGB();

    sk_sp<SkColorSpace> gainmapMathColorSpace =
            gainmapInfo.fGainmapMathColorSpace
                    ? gainmapInfo.fGainmapMathColorSpace->makeLinearGamma()
                    : baseColorSpace->makeLinearGamma();

    SkiaRsGainmapInfoInput infoIn{
            gainmapInfo.fGainmapRatioMin,
            gainmapInfo.fGainmapRatioMax,
            gainmapInfo.fGainmapGamma,
            gainmapInfo.fEpsilonSdr,
            gainmapInfo.fEpsilonHdr,
            gainmapInfo.fDisplayRatioSdr,
            gainmapInfo.fDisplayRatioHdr,
            static_cast<int32_t>(gainmapInfo.fBaseImageType),
            static_cast<int32_t>(gainmapInfo.fType),
    };
    SkiaRsGainmapUniforms u;
    skia_rs_gainmap_compute_uniforms(
            baseRect, gainmapRect, dstRect,
            static_cast<int32_t>(gainmapImage->colorType()),
            &infoIn, dstHdrRatio, &u);

    if (u.weight == 0.f) {
        return baseImage->makeShader(baseSamplingOptions, &u.base_rect_to_dst);
    }

    auto baseImageShader = baseImage->makeShader(baseSamplingOptions, &u.base_rect_to_dst);
    auto gainmapImageShader =
            gainmapImage->makeRawShader(gainmapSamplingOptions, &u.gainmap_rect_to_dst);

    sk_sp<SkShader> gainmapMathShader;
    {
        SkRuntimeShaderBuilder builder(gainmap_apply_effect());
        builder.child("base") = baseImageShader;
        builder.child("gainmap") = gainmapImageShader;
        builder.uniform("logRatioMin") = u.log_ratio_min;
        builder.uniform("logRatioMax") = u.log_ratio_max;
        builder.uniform("gainmapGamma") = u.gainmap_gamma;
        builder.uniform("epsilonBase") = u.epsilon_base;
        builder.uniform("epsilonOther") = u.epsilon_other;
        builder.uniform("noGamma") = u.no_gamma;
        builder.uniform("singleChannel") = u.single_channel;
        builder.uniform("gainmapIsAlpha") = u.gainmap_is_alpha;
        builder.uniform("gainmapIsRed") = u.gainmap_is_red;
        builder.uniform("W") = u.weight;
        builder.uniform("isApple") = u.is_apple;
        builder.uniform("appleG") = u.apple_g;
        builder.uniform("appleH") = u.apple_h;

        gainmapMathShader = builder.makeShader();
        SkASSERT(gainmapMathShader);
    }

    return gainmapMathShader->makeWithWorkingColorSpace(gainmapMathColorSpace);
}
