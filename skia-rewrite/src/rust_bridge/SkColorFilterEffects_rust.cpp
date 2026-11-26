/*
 * Safe Rust bridge for:
 *   src/effects/colorfilters/SkBlendModeColorFilter.cpp
 *   src/effects/colorfilters/SkColorFilterBase.cpp
 *   src/effects/colorfilters/SkColorSpaceXformColorFilter.cpp
 *   src/effects/colorfilters/SkComposeColorFilter.cpp
 *   src/effects/colorfilters/SkGaussianColorFilter.cpp
 *   src/effects/colorfilters/SkMatrixColorFilter.cpp
 *   src/effects/colorfilters/SkTableColorFilter.cpp
 *   src/effects/colorfilters/SkWorkingFormatColorFilter.cpp
 * (Phase 1 Layer 4.2 skia-rs-colorfilters, GN target :skia).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorTable.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkColorMatrix.h"
#include "include/private/SkAssert.h"
#include "modules/skcms/skcms.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorFilterPriv.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkNoDestructor.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/colorfilters/SkBlendModeColorFilter.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"
#include "src/effects/colorfilters/SkColorSpaceXformColorFilter.h"
#include "src/effects/colorfilters/SkComposeColorFilter.h"
#include "src/effects/colorfilters/SkGaussianColorFilter.h"
#include "src/effects/colorfilters/SkMatrixColorFilter.h"
#include "src/effects/colorfilters/SkTableColorFilter.h"
#include "src/effects/colorfilters/SkWorkingFormatColorFilter.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>

namespace {
struct SkiaRsBlendSimplifyResult {
    bool should_create;
    SkColor4f color;
    int32_t mode;
};

struct SkiaRsXformStagePlan {
    bool unpremul;
    bool premul;
};

struct SkiaRsMatrixStagePlan {
    bool unpremul;
    bool hsla;
    bool clamp_all;
    bool premul;
};

struct SkiaRsTableStagePlan {
    bool unpremul;
    bool premul;
};

struct SkiaRsWorkingFormatCalculator {
    skcms_TransferFunction tf;
    bool use_dst_tf;
    skcms_Matrix3x3 gamut;
    bool use_dst_gamut;
    SkAlphaType at;
    bool use_dst_at;
};

struct SkiaRsResolvedWorkingFormat {
    skcms_TransferFunction tf;
    skcms_Matrix3x3 gamut;
    SkAlphaType alpha_type;
};
}  // namespace

static_assert(sizeof(SkColor4f) == 16);
static_assert(sizeof(SkPMColor4f) == 16);
static_assert(sizeof(skcms_TransferFunction) == 28);
static_assert(sizeof(skcms_Matrix3x3) == 36);
static_assert(sizeof(SkWorkingFormatCalculator) == sizeof(SkiaRsWorkingFormatCalculator));
static_assert(sizeof(SkiaRsWorkingFormatCalculator) == 80);
static_assert(sizeof(SkiaRsResolvedWorkingFormat) == 68);

extern "C" {
// SkColorFilterBase
SkColor4f skia_rs_color_filter_unpremul_color4f(SkPMColor4f pm);
bool skia_rs_color_filter_pm_is_opaque(SkPMColor4f pm);
bool skia_rs_color_filter_base_on_as_a_color_mode();
bool skia_rs_color_filter_base_on_as_a_color_matrix();

// SkBlendModeColorFilter
bool skia_rs_blend_mode_cf_is_valid_mode(int32_t mode);
SkColor4f skia_rs_blend_mode_cf_pin_alpha(SkColor4f color);
SkiaRsBlendSimplifyResult skia_rs_blend_mode_cf_simplify(SkColor4f srgb, int32_t mode);
bool skia_rs_blend_mode_cf_is_alpha_unchanged(int32_t mode);
uint32_t skia_rs_blend_mode_cf_to_sk_color(SkColor4f color);
SkColor4f skia_rs_blend_mode_cf_from_sk_color(uint32_t color);

// SkColorSpaceXformColorFilter
SkiaRsXformStagePlan skia_rs_xform_cf_plan_stages(bool shader_is_opaque);
int32_t skia_rs_xform_cf_decode_legacy_gamma_dir(uint32_t dir);

// SkComposeColorFilter
bool skia_rs_compose_cf_is_alpha_unchanged(bool outer_unchanged, bool inner_unchanged);
bool skia_rs_compose_cf_inner_is_opaque(bool shader_is_opaque, bool inner_unchanged);
int32_t skia_rs_compose_cf_classify(bool has_outer, bool has_inner);

// SkMatrixColorFilter
bool skia_rs_matrix_cf_is_valid(const float* matrix);
bool skia_rs_matrix_cf_is_alpha_unchanged(const float* matrix);
SkiaRsMatrixStagePlan skia_rs_matrix_cf_plan_stages(
        bool shader_is_opaque, bool alpha_is_unchanged, uint8_t domain, bool clamp);

// SkTableColorFilter
SkiaRsTableStagePlan skia_rs_table_cf_plan_stages(bool shader_is_opaque, uint8_t alpha_table_255);

// SkWorkingFormatColorFilter
void skia_rs_working_format_init(const skcms_TransferFunction* tf,
                                 const skcms_Matrix3x3* gamut,
                                 const SkAlphaType* at,
                                 SkiaRsWorkingFormatCalculator* out);
void skia_rs_working_format_resolve(const SkiaRsWorkingFormatCalculator* calc,
                                    const skcms_TransferFunction* dst_tf,
                                    const skcms_Matrix3x3* dst_gamut,
                                    SkiaRsResolvedWorkingFormat* out);
}  // extern "C"

// ============================================================================
// SkColorFilterBase.cpp
// ============================================================================

bool SkColorFilterBase::onAsAColorMode(SkColor*, SkBlendMode*) const {
    return skia_rs_color_filter_base_on_as_a_color_mode();
}

bool SkColorFilterBase::onAsAColorMatrix(float matrix[20]) const {
    return skia_rs_color_filter_base_on_as_a_color_matrix();
}

SkPMColor4f SkColorFilterBase::onFilterColor4f(const SkPMColor4f& color,
                                               SkColorSpace* dstCS) const {
    constexpr size_t kEnoughForCommonFilters = 2048;  // big enough for a tiny SkSL program
    SkSTArenaAlloc<kEnoughForCommonFilters> alloc;
    SkRasterPipeline pipeline(&alloc);
    pipeline.appendConstantColor(&alloc, color.vec());
    SkSurfaceProps props{};  // default OK; colorFilters don't render text
    SkStageRec rec = {&pipeline,
                      &alloc,
                      kRGBA_F32_SkColorType,
                      dstCS,
                      skia_rs_color_filter_unpremul_color4f(color),
                      props,
                      SkRect::MakeEmpty()};

    if (as_CFB(this)->appendStages(rec, skia_rs_color_filter_pm_is_opaque(color))) {
        SkPMColor4f dst;
        SkRasterPipelineContexts::MemoryCtx dstPtr = {&dst, 0};
        pipeline.append(SkRasterPipelineOp::store_f32, &dstPtr);
        pipeline.run(0, 0, 1, 1);
        return dst;
    }

    SkDEBUGFAIL("onFilterColor4f unimplemented for this filter");
    return SkPMColor4f{0, 0, 0, 0};
}

// ============================================================================
// SkBlendModeColorFilter.cpp
// ============================================================================

SkBlendModeColorFilter::SkBlendModeColorFilter(const SkColor4f& color, SkBlendMode mode)
        : fColor(color), fMode(mode) {}

bool SkBlendModeColorFilter::onAsAColorMode(SkColor* color, SkBlendMode* mode) const {
    if (color) {
        *color = skia_rs_blend_mode_cf_to_sk_color(fColor);
    }
    if (mode) {
        *mode = fMode;
    }
    return true;
}

bool SkBlendModeColorFilter::onIsAlphaUnchanged() const {
    return skia_rs_blend_mode_cf_is_alpha_unchanged(static_cast<int32_t>(fMode));
}

void SkBlendModeColorFilter::flatten(SkWriteBuffer& buffer) const {
    buffer.writeColor4f(fColor);
    buffer.writeUInt(static_cast<int>(fMode));
}

sk_sp<SkFlattenable> SkBlendModeColorFilter::CreateProc(SkReadBuffer& buffer) {
    if (buffer.isVersionLT(SkPicturePriv::kBlend4fColorFilter)) {
        // Color is 8-bit, sRGB
        SkColor color = buffer.readColor();
        SkBlendMode mode = static_cast<SkBlendMode>(buffer.readUInt());
        return SkColorFilters::Blend(
                skia_rs_blend_mode_cf_from_sk_color(color), /*sRGB*/ nullptr, mode);
    } else {
        // Color is 32-bit, sRGB
        SkColor4f color;
        buffer.readColor4f(&color);
        SkBlendMode mode = static_cast<SkBlendMode>(buffer.readUInt());
        return SkColorFilters::Blend(color, /*sRGB*/ nullptr, mode);
    }
}

bool SkBlendModeColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    rec.fPipeline->append(SkRasterPipelineOp::move_src_dst);
    SkColor4f color = fColor;
    SkColorSpaceXformSteps(sk_srgb_singleton(), kUnpremul_SkAlphaType,
                           rec.fDstCS,          kPremul_SkAlphaType).apply(color.vec());
    rec.fPipeline->appendConstantColor(rec.fAlloc, color.vec());
    SkBlendMode_AppendStages(fMode, rec.fPipeline);
    return true;
}

sk_sp<SkColorFilter> SkColorFilters::Blend(const SkColor4f& color,
                                           sk_sp<SkColorSpace> colorSpace,
                                           SkBlendMode mode) {
    if (!skia_rs_blend_mode_cf_is_valid_mode(static_cast<int32_t>(mode))) {
        return nullptr;
    }

    SkColor4f srgb = skia_rs_blend_mode_cf_pin_alpha(color);
    SkColorSpaceXformSteps(colorSpace.get(),    kUnpremul_SkAlphaType,
                           sk_srgb_singleton(), kUnpremul_SkAlphaType).apply(srgb.vec());

    SkiaRsBlendSimplifyResult res =
            skia_rs_blend_mode_cf_simplify(srgb, static_cast<int32_t>(mode));
    if (!res.should_create) {
        return nullptr;
    }

    return sk_sp<SkColorFilter>(
            new SkBlendModeColorFilter(res.color, static_cast<SkBlendMode>(res.mode)));
}

sk_sp<SkColorFilter> SkColorFilters::Blend(SkColor color, SkBlendMode mode) {
    return Blend(skia_rs_blend_mode_cf_from_sk_color(color), /*sRGB*/ nullptr, mode);
}

void SkRegisterModeColorFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkBlendModeColorFilter);
    // Previous name
    SkFlattenable::Register("SkModeColorFilter", SkBlendModeColorFilter::CreateProc);
}

// ============================================================================
// SkColorSpaceXformColorFilter.cpp
// ============================================================================

SkColorSpaceXformColorFilter::SkColorSpaceXformColorFilter(sk_sp<SkColorSpace> src,
                                                           sk_sp<SkColorSpace> dst)
        : fSrc(std::move(src))
        , fDst(std::move(dst))
        , fSteps(  // We handle premul/unpremul separately, so here just always upm->upm.
                  fSrc.get(),
                  kUnpremul_SkAlphaType,
                  fDst.get(),
                  kUnpremul_SkAlphaType) {}

bool SkColorSpaceXformColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    SkiaRsXformStagePlan plan = skia_rs_xform_cf_plan_stages(shaderIsOpaque);
    if (plan.unpremul) {
        rec.fPipeline->append(SkRasterPipelineOp::unpremul);
    }

    fSteps.apply(rec.fPipeline);

    if (plan.premul) {
        rec.fPipeline->append(SkRasterPipelineOp::premul);
    }
    return true;
}

void SkColorSpaceXformColorFilter::flatten(SkWriteBuffer& buffer) const {
    buffer.writeDataAsByteArray(fSrc->serialize().get());
    buffer.writeDataAsByteArray(fDst->serialize().get());
}

sk_sp<SkFlattenable> SkColorSpaceXformColorFilter::LegacyGammaOnlyCreateProc(SkReadBuffer& buffer) {
    uint32_t dir = buffer.read32();
    int32_t decoded = skia_rs_xform_cf_decode_legacy_gamma_dir(dir);
    if (!buffer.validate(decoded >= 0)) {
        return nullptr;
    }
    if (decoded == 0) {
        return SkColorFilters::LinearToSRGBGamma();
    }
    return SkColorFilters::SRGBToLinearGamma();
}

sk_sp<SkFlattenable> SkColorSpaceXformColorFilter::CreateProc(SkReadBuffer& buffer) {
    std::array<sk_sp<SkColorSpace>, 2> colorSpaces;
    for (int i = 0; i < 2; ++i) {
        auto data = buffer.readByteArrayAsData();
        if (!buffer.validate(data != nullptr)) {
            return nullptr;
        }
        colorSpaces[i] = SkColorSpace::Deserialize(data->data(), data->size());
        if (!buffer.validate(colorSpaces[i] != nullptr)) {
            return nullptr;
        }
    }
    return sk_sp<SkFlattenable>(
            new SkColorSpaceXformColorFilter(std::move(colorSpaces[0]), std::move(colorSpaces[1])));
}

sk_sp<SkColorFilter> SkColorFilters::LinearToSRGBGamma() {
    static SkNoDestructor<SkColorSpaceXformColorFilter> gSingleton(SkColorSpace::MakeSRGBLinear(),
                                                                   SkColorSpace::MakeSRGB());
    return sk_ref_sp(gSingleton.get());
}

sk_sp<SkColorFilter> SkColorFilters::SRGBToLinearGamma() {
    static SkNoDestructor<SkColorSpaceXformColorFilter> gSingleton(SkColorSpace::MakeSRGB(),
                                                                   SkColorSpace::MakeSRGBLinear());
    return sk_ref_sp(gSingleton.get());
}

sk_sp<SkColorFilter> SkColorFilterPriv::MakeColorSpaceXform(sk_sp<SkColorSpace> src,
                                                            sk_sp<SkColorSpace> dst) {
    return sk_make_sp<SkColorSpaceXformColorFilter>(std::move(src), std::move(dst));
}

void SkRegisterSkColorSpaceXformColorFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkColorSpaceXformColorFilter);
    // Previous name
    SkFlattenable::Register("ColorSpaceXformColorFilter", SkColorSpaceXformColorFilter::CreateProc);
    // TODO(ccameron): Remove after grace period for SKPs to stop using old serialization.
    SkFlattenable::Register("SkSRGBGammaColorFilter",
                            SkColorSpaceXformColorFilter::LegacyGammaOnlyCreateProc);
}

// ============================================================================
// SkComposeColorFilter.cpp
// ============================================================================

SkComposeColorFilter::SkComposeColorFilter(sk_sp<SkColorFilter> outer, sk_sp<SkColorFilter> inner)
        : fOuter(as_CFB_sp(std::move(outer))), fInner(as_CFB_sp(std::move(inner))) {
    SkASSERT(fOuter && fInner);
}

bool SkComposeColorFilter::onIsAlphaUnchanged() const {
    return skia_rs_compose_cf_is_alpha_unchanged(
            fOuter->isAlphaUnchanged(), fInner->isAlphaUnchanged());
}

bool SkComposeColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    bool innerIsOpaque =
            skia_rs_compose_cf_inner_is_opaque(shaderIsOpaque, fInner->isAlphaUnchanged());
    return fInner->appendStages(rec, shaderIsOpaque) && fOuter->appendStages(rec, innerIsOpaque);
}

void SkComposeColorFilter::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fOuter.get());
    buffer.writeFlattenable(fInner.get());
}

sk_sp<SkFlattenable> SkComposeColorFilter::CreateProc(SkReadBuffer& buffer) {
    sk_sp<SkColorFilter> outer(buffer.readColorFilter());
    sk_sp<SkColorFilter> inner(buffer.readColorFilter());
    switch (skia_rs_compose_cf_classify(outer != nullptr, inner != nullptr)) {
        case 3:
            return outer->makeComposed(std::move(inner));
        case 1:
            return outer;
        case 2:
            return inner;
        default:
            return nullptr;
    }
}

sk_sp<SkColorFilter> SkColorFilter::makeComposed(sk_sp<SkColorFilter> inner) const {
    if (skia_rs_compose_cf_classify(true, inner != nullptr) != 3) {
        return sk_ref_sp(this);
    }
    return sk_sp<SkColorFilter>(new SkComposeColorFilter(sk_ref_sp(this), std::move(inner)));
}

void SkRegisterComposeColorFilterFlattenable() { SK_REGISTER_FLATTENABLE(SkComposeColorFilter); }

// ============================================================================
// SkGaussianColorFilter.cpp
// ============================================================================

SkGaussianColorFilter::SkGaussianColorFilter() : SkColorFilterBase() {}

bool SkGaussianColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    rec.fPipeline->append(SkRasterPipelineOp::gauss_a_to_rgba);
    return true;
}

sk_sp<SkFlattenable> SkGaussianColorFilter::CreateProc(SkReadBuffer&) {
    return SkColorFilterPriv::MakeGaussian();
}

sk_sp<SkColorFilter> SkColorFilterPriv::MakeGaussian() {
    return sk_sp<SkColorFilter>(new SkGaussianColorFilter);
}

// ============================================================================
// SkMatrixColorFilter.cpp
// ============================================================================

SkMatrixColorFilter::SkMatrixColorFilter(const float array[20], Domain domain, Clamp clamp)
        : fAlphaIsUnchanged(skia_rs_matrix_cf_is_alpha_unchanged(array))
        , fDomain(domain)
        , fClamp(clamp) {
    memcpy(fMatrix, array, 20 * sizeof(float));
}

void SkMatrixColorFilter::flatten(SkWriteBuffer& buffer) const {
    SkASSERT(sizeof(fMatrix) / sizeof(float) == 20);
    buffer.writeScalarArray(fMatrix);

    // RGBA flag
    buffer.writeBool(fDomain == Domain::kRGBA);
    buffer.writeBool(fClamp == Clamp::kYes);
}

sk_sp<SkFlattenable> SkMatrixColorFilter::CreateProc(SkReadBuffer& buffer) {
    float matrix[20];
    if (!buffer.readScalarArray(matrix)) {
        return nullptr;
    }

    auto is_rgba = buffer.readBool();
    Clamp clamp = buffer.isVersionLT(SkPicturePriv::kUnclampedMatrixColorFilter)
                          ? Clamp::kYes
                          : (buffer.readBool() ? Clamp::kYes : Clamp::kNo);
    // clamp option is ignored for HSL-domain filters
    return is_rgba ? SkColorFilters::Matrix(matrix, clamp) : SkColorFilters::HSLAMatrix(matrix);
}

bool SkMatrixColorFilter::onAsAColorMatrix(float matrix[20]) const {
    if (matrix) {
        memcpy(matrix, fMatrix, 20 * sizeof(float));
    }
    return true;
}

bool SkMatrixColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    SkiaRsMatrixStagePlan plan = skia_rs_matrix_cf_plan_stages(
            shaderIsOpaque,
            fAlphaIsUnchanged,
            static_cast<uint8_t>(fDomain),
            fClamp == Clamp::kYes);

    SkRasterPipeline* p = rec.fPipeline;
    if (plan.unpremul) {
        p->append(SkRasterPipelineOp::unpremul);
    }
    if (plan.hsla) {
        p->append(SkRasterPipelineOp::rgb_to_hsl);
    }
    p->append(SkRasterPipelineOp::matrix_4x5, fMatrix);
    if (plan.hsla) {
        p->append(SkRasterPipelineOp::hsl_to_rgb);
    }
    if (plan.clamp_all) {
        p->append(SkRasterPipelineOp::clamp_01);
    } else {
        // We still need to clamp alpha, regardless
        p->append(SkRasterPipelineOp::clamp_a_01);
    }
    if (plan.premul) {
        p->append(SkRasterPipelineOp::premul);
    }
    return true;
}

static sk_sp<SkColorFilter> MakeMatrix(const float array[20],
                                       SkMatrixColorFilter::Domain domain,
                                       SkColorFilters::Clamp clamp) {
    if (!skia_rs_matrix_cf_is_valid(array)) {
        return nullptr;
    }
    return sk_make_sp<SkMatrixColorFilter>(array, domain, clamp);
}

sk_sp<SkColorFilter> SkColorFilters::Matrix(const float array[20], Clamp clamp) {
    return MakeMatrix(array, SkMatrixColorFilter::Domain::kRGBA, clamp);
}

sk_sp<SkColorFilter> SkColorFilters::Matrix(const SkColorMatrix& cm, Clamp clamp) {
    return MakeMatrix(cm.fMat.data(), SkMatrixColorFilter::Domain::kRGBA, clamp);
}

sk_sp<SkColorFilter> SkColorFilters::HSLAMatrix(const float array[20]) {
    return MakeMatrix(array, SkMatrixColorFilter::Domain::kHSLA, Clamp::kYes);
}

sk_sp<SkColorFilter> SkColorFilters::HSLAMatrix(const SkColorMatrix& cm) {
    return MakeMatrix(cm.fMat.data(), SkMatrixColorFilter::Domain::kHSLA, Clamp::kYes);
}

void SkRegisterMatrixColorFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkMatrixColorFilter);
    // Previous name
    SkFlattenable::Register("SkColorFilter_Matrix", SkMatrixColorFilter::CreateProc);
}

// ============================================================================
// SkTableColorFilter.cpp
// ============================================================================

bool SkTableColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    SkRasterPipeline* p = rec.fPipeline;

    SkRasterPipelineContexts::TablesCtx* tables =
            rec.fAlloc->make<SkRasterPipelineContexts::TablesCtx>();
    tables->a = fTable->alphaTable();
    tables->r = fTable->redTable();
    tables->g = fTable->greenTable();
    tables->b = fTable->blueTable();

    SkiaRsTableStagePlan plan = skia_rs_table_cf_plan_stages(shaderIsOpaque, tables->a[0xff]);
    if (plan.unpremul) {
        p->append(SkRasterPipelineOp::unpremul);
    }
    p->append(SkRasterPipelineOp::byte_tables, tables);
    if (plan.premul) {
        p->append(SkRasterPipelineOp::premul);
    }
    return true;
}

void SkTableColorFilter::flatten(SkWriteBuffer& buffer) const {
    fTable->flatten(buffer);
}

sk_sp<SkFlattenable> SkTableColorFilter::CreateProc(SkReadBuffer& buffer) {
    return SkColorFilters::Table(SkColorTable::Deserialize(buffer));
}

sk_sp<SkColorFilter> SkColorFilters::Table(const uint8_t table[256]) {
    return SkColorFilters::Table(SkColorTable::Make(table));
}

sk_sp<SkColorFilter> SkColorFilters::TableARGB(const uint8_t tableA[256],
                                               const uint8_t tableR[256],
                                               const uint8_t tableG[256],
                                               const uint8_t tableB[256]) {
    return SkColorFilters::Table(SkColorTable::Make(tableA, tableR, tableG, tableB));
}

sk_sp<SkColorFilter> SkColorFilters::Table(sk_sp<SkColorTable> table) {
    if (!table) {
        return nullptr;
    }
    return sk_make_sp<SkTableColorFilter>(std::move(table));
}

void SkRegisterTableColorFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkTableColorFilter);
    // Previous name
    SkFlattenable::Register("SkTable_ColorFilter", SkTableColorFilter::CreateProc);
}

// ============================================================================
// SkWorkingFormatColorFilter.cpp
// ============================================================================

SkWorkingFormatColorFilter::SkWorkingFormatColorFilter(sk_sp<SkColorFilter> child,
                                                       const skcms_TransferFunction* tf,
                                                       const skcms_Matrix3x3* gamut,
                                                       const SkAlphaType* at)
        : fWorkingFormatCalculator(tf, gamut, at) {
    SkASSERT(child);
    fChild = std::move(child);
}

sk_sp<SkColorSpace> SkWorkingFormatColorFilter::workingFormat(const sk_sp<SkColorSpace>& dstCS,
                                                              SkAlphaType* outAT) const {
    return fWorkingFormatCalculator.workingFormat(dstCS, outAT);
}

bool SkWorkingFormatColorFilter::appendStages(const SkStageRec& rec, bool shaderIsOpaque) const {
    sk_sp<SkColorSpace> dstCS = sk_ref_sp(rec.fDstCS);

    if (!dstCS) {
        dstCS = SkColorSpace::MakeSRGB();
    }

    SkAlphaType workingAT;
    sk_sp<SkColorSpace> workingCS = this->workingFormat(dstCS, &workingAT);

    SkColorInfo dst = {rec.fDstColorType, kPremul_SkAlphaType, dstCS},
                working = {rec.fDstColorType, workingAT, workingCS};

    const auto* dstToWorking = rec.fAlloc->make<SkColorSpaceXformSteps>(dst, working);
    const auto* workingToDst = rec.fAlloc->make<SkColorSpaceXformSteps>(working, dst);

    SkStageRec workingRec = {rec.fPipeline,
                             rec.fAlloc,
                             rec.fDstColorType,
                             workingCS.get(),
                             rec.fPaintColor,
                             rec.fSurfaceProps,
                             rec.fDstBounds};

    dstToWorking->apply(rec.fPipeline);
    if (!as_CFB(fChild)->appendStages(workingRec, shaderIsOpaque)) {
        return false;
    }
    workingToDst->apply(rec.fPipeline);
    return true;
}

SkPMColor4f SkWorkingFormatColorFilter::onFilterColor4f(const SkPMColor4f& origColor,
                                                        SkColorSpace* rawDstCS) const {
    sk_sp<SkColorSpace> dstCS = sk_ref_sp(rawDstCS);
    if (!dstCS) {
        dstCS = SkColorSpace::MakeSRGB();
    }

    SkAlphaType workingAT;
    sk_sp<SkColorSpace> workingCS = this->workingFormat(dstCS, &workingAT);

    SkColorInfo dst = {kUnknown_SkColorType, kPremul_SkAlphaType, dstCS},
                working = {kUnknown_SkColorType, workingAT, workingCS};

    SkPMColor4f color = origColor;
    SkColorSpaceXformSteps{dst, working}.apply(color.vec());
    color = as_CFB(fChild)->onFilterColor4f(color, working.colorSpace());
    SkColorSpaceXformSteps{working, dst}.apply(color.vec());
    return color;
}

bool SkWorkingFormatColorFilter::onIsAlphaUnchanged() const { return fChild->isAlphaUnchanged(); }

bool SkWorkingFormatColorFilter::onAsAColorMode(SkColor* color, SkBlendMode* mode) const {
    return fChild->asAColorMode(color, mode);
}

bool SkWorkingFormatColorFilter::onAsAColorMatrix(float matrix[20]) const {
    return fChild->asAColorMatrix(matrix);
}

void SkWorkingFormatColorFilter::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fChild.get());
    fWorkingFormatCalculator.flatten(buffer);
}

sk_sp<SkFlattenable> SkWorkingFormatColorFilter::CreateProc(SkReadBuffer& buffer) {
    sk_sp<SkColorFilter> child = buffer.readColorFilter();
    bool useDstTF = buffer.readBool(), useDstGamut = buffer.readBool(),
         useDstAT = buffer.readBool();

    skcms_TransferFunction tf;
    skcms_Matrix3x3 gamut;
    SkAlphaType at;

    if (!useDstTF) {
        buffer.readScalarArray({&tf.g, sizeof(skcms_TransferFunction) / sizeof(SkScalar)});
    }
    if (!useDstGamut) {
        buffer.readScalarArray({&gamut.vals[0][0], sizeof(skcms_Matrix3x3) / sizeof(SkScalar)});
    }
    if (!useDstAT) {
        at = buffer.read32LE(kLastEnum_SkAlphaType);
    }

    return SkColorFilterPriv::WithWorkingFormat(std::move(child),
                                                useDstTF ? nullptr : &tf,
                                                useDstGamut ? nullptr : &gamut,
                                                useDstAT ? nullptr : &at);
}

sk_sp<SkColorFilter> SkColorFilterPriv::WithWorkingFormat(sk_sp<SkColorFilter> child,
                                                          const skcms_TransferFunction* tf,
                                                          const skcms_Matrix3x3* gamut,
                                                          const SkAlphaType* at) {
    if (!child) {
        return nullptr;
    }
    return sk_make_sp<SkWorkingFormatColorFilter>(std::move(child), tf, gamut, at);
}

void SkRegisterWorkingFormatColorFilterFlattenable() {
    SK_REGISTER_FLATTENABLE(SkWorkingFormatColorFilter);
}

SkWorkingFormatCalculator::SkWorkingFormatCalculator(const skcms_TransferFunction* tf,
                                                     const skcms_Matrix3x3* gamut,
                                                     const SkAlphaType* at) {
    SkiaRsWorkingFormatCalculator init;
    skia_rs_working_format_init(tf, gamut, at, &init);
    fTF = init.tf;
    fUseDstTF = init.use_dst_tf;
    fGamut = init.gamut;
    fUseDstGamut = init.use_dst_gamut;
    fAT = init.at;
    fUseDstAT = init.use_dst_at;
}

sk_sp<SkColorSpace> SkWorkingFormatCalculator::workingFormat(const sk_sp<SkColorSpace>& dstCS,
                                                             SkAlphaType* outAT) const {
    skcms_TransferFunction dstTF{};
    skcms_Matrix3x3 dstGamut{};

    if (fUseDstTF) {
        SkAssertResult(dstCS->isNumericalTransferFn(&dstTF));
    }
    if (fUseDstGamut) {
        SkAssertResult(dstCS->toXYZD50(&dstGamut));
    }

    SkiaRsWorkingFormatCalculator calc = {
            fTF, fUseDstTF, fGamut, fUseDstGamut, fAT, fUseDstAT};
    SkiaRsResolvedWorkingFormat resolved;
    skia_rs_working_format_resolve(
            &calc,
            fUseDstTF ? &dstTF : nullptr,
            fUseDstGamut ? &dstGamut : nullptr,
            &resolved);

    *outAT = resolved.alpha_type;
    return SkColorSpace::MakeRGB(resolved.tf, resolved.gamut);
}

void SkWorkingFormatCalculator::flatten(SkWriteBuffer& buffer) const {
    buffer.writeBool(fUseDstTF);
    buffer.writeBool(fUseDstGamut);
    buffer.writeBool(fUseDstAT);
    if (!fUseDstTF) {
        buffer.writeScalarArray({&fTF.g, sizeof(skcms_TransferFunction) / sizeof(SkScalar)});
    }
    if (!fUseDstGamut) {
        buffer.writeScalarArray({&fGamut.vals[0][0], sizeof(skcms_Matrix3x3) / sizeof(SkScalar)});
    }
    if (!fUseDstAT) {
        buffer.writeInt(fAT);
    }
}
