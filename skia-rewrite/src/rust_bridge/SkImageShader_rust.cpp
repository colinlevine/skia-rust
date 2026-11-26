/*
 * Safe Rust bridge for:
 *   src/shaders/SkImageShader.cpp
 *   src/shaders/SkBitmapProcShader.cpp
 * (Phase 1 Layer 4.1 skia-rs-shaders, GN target :core).
 */

#include "src/shaders/SkImageShader.h"

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRect.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkTileMode.h"
#include "include/private/SkAssert.h"
#include "include/private/SkMath.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "modules/skcms/skcms.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkBitmapProcState.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMipmapAccessor.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkSamplingPriv.h"
#include "src/core/SkWriteBuffer.h"
#include "src/image/SkImage_Base.h"
#include "src/partition_alloc/raw_ptr.h"
#include "src/shaders/SkBitmapProcShader.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <tuple>
#include <utility>

class SkColorSpace;

namespace {

constexpr bool kIsAndroidFramework =
#if defined(SK_BUILD_FOR_ANDROID_FRAMEWORK)
        true;
#else
        false;
#endif

struct SkiaRsDrawRectGeometry {
    bool valid;
    SkRect src;
    SkRect dst;
    SkMatrix local_matrix;
    bool image_is_alpha_only;
};

struct SkiaRsMipLevelInitParams {
    float width_f;
    float height_f;
    float inv_width;
    float inv_height;
    float cubic_weights[16];
    bool has_cubic_weights;
    bool round_down_at_integer;
    int32_t mirror_bias_dir;
    bool has_decal;
    float decal_limit_x;
    float decal_limit_y;
    float decal_inclusive_edge_x;
    float decal_inclusive_edge_y;
};

}  // namespace

static_assert(sizeof(SkM44) == 64);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkSamplingOptions) == 24);

extern "C" {
void skia_rs_image_shader_cubic_matrix(float b, float c, SkM44* out);
void skia_rs_image_shader_cubic_weights(float b, float c, float out_weights[16]);
int32_t skia_rs_image_shader_optimize_tile_mode(
        int32_t tm, int32_t dimension, bool android_framework);
int32_t skia_rs_image_shader_classify_make_subset(
        bool has_image, int32_t img_w, int32_t img_h, SkRect subset,
        const SkSamplingOptions* sampling);
int32_t skia_rs_image_shader_classify_make_raw(bool has_image, bool use_cubic);
bool skia_rs_image_shader_is_opaque(bool img_is_opaque, int32_t tmx, int32_t tmy);
void skia_rs_image_shader_aniso_fallback(bool image_is_mipped, SkSamplingOptions* out);
void skia_rs_image_shader_tweak_sampling(
        const SkSamplingOptions* sampling, const SkMatrix* matrix, SkSamplingOptions* out);
void skia_rs_image_shader_draw_rect_geometry(
        int32_t img_w, int32_t img_h, int32_t img_ct, SkRect src, SkRect dst,
        SkiaRsDrawRectGeometry* out);
void skia_rs_image_shader_mip_level_init(
        int32_t pm_width, int32_t pm_height, const SkSamplingOptions* sampling,
        int32_t tmx, int32_t tmy, SkiaRsMipLevelInitParams* out);
int32_t skia_rs_image_shader_classify_fast_path(
        int32_t ct, const SkSamplingOptions* sampling, int32_t tmx, int32_t tmy,
        SkRect dst_bounds, const SkMatrix* base_inv, bool coords_already_seeded,
        bool total_matrix_is_valid, const SkMatrix* total_matrix, const SkMatrix* total_inv);
bool skia_rs_image_shader_cubic_uses_clamp_01(int32_t at, bool clamp_as_if_unpremul);
bool skia_rs_image_shader_legacy_can_handle(const SkMatrix* inv, bool has_dxdy_opt);
bool skia_rs_image_shader_legacy_sampling_supported(const SkSamplingOptions* sampling);
uint32_t skia_rs_bitmap_proc_shader_flags(bool pixmap_is_opaque, uint8_t paint_alpha);
}  // extern "C"

// ============================================================================
// SkImageShader.cpp
// ============================================================================

SkM44 SkImageShader::CubicResamplerMatrix(float B, float C) {
    SkM44 out;
    skia_rs_image_shader_cubic_matrix(B, C, &out);
    return out;
}

static SkTileMode optimize(SkTileMode tm, int dimension) {
    SkASSERT(dimension > 0);
    return static_cast<SkTileMode>(skia_rs_image_shader_optimize_tile_mode(
            static_cast<int32_t>(tm), dimension, kIsAndroidFramework));
}

#if defined(SK_DEBUG)
static bool needs_subset(SkImage* img, const SkRect& subset) {
    return subset != SkRect::Make(img->dimensions());
}
#endif

SkImageShader::SkImageShader(sk_sp<SkImage> img,
                             const SkRect& subset,
                             SkTileMode tmx, SkTileMode tmy,
                             const SkSamplingOptions& sampling,
                             bool raw,
                             bool clampAsIfUnpremul)
        : fImage(std::move(img))
        , fSampling(sampling)
        , fTileModeX(optimize(tmx, fImage->width()))
        , fTileModeY(optimize(tmy, fImage->height()))
        , fSubset(subset)
        , fRaw(raw)
        , fClampAsIfUnpremul(clampAsIfUnpremul) {
    SkASSERT(!fRaw || !fClampAsIfUnpremul);
    SkASSERT(!fRaw || !fSampling.useCubic);
}

sk_sp<SkFlattenable> SkImageShader::CreateProc(SkReadBuffer& buffer) {
    auto tmx = buffer.read32LE<SkTileMode>(SkTileMode::kLastTileMode);
    auto tmy = buffer.read32LE<SkTileMode>(SkTileMode::kLastTileMode);

    SkSamplingOptions sampling;
    bool readSampling = true;
    if (buffer.isVersionLT(SkPicturePriv::kNoFilterQualityShaders_Version) &&
        !buffer.readBool())
    {
        readSampling = false;
    }
    if (readSampling) {
        sampling = buffer.readSampling();
    }

    SkMatrix localMatrix;
    if (buffer.isVersionLT(SkPicturePriv::Version::kNoShaderLocalMatrix)) {
        buffer.readMatrix(&localMatrix);
    }
    sk_sp<SkImage> img = buffer.readImage();
    if (!img) {
        return nullptr;
    }

    bool raw = buffer.isVersionLT(SkPicturePriv::Version::kRawImageShaders) ? false
                                                                            : buffer.readBool();

    return raw ? SkImageShader::MakeRaw(std::move(img), tmx, tmy, sampling, &localMatrix)
               : SkImageShader::Make(std::move(img), tmx, tmy, sampling, &localMatrix);
}

void SkImageShader::flatten(SkWriteBuffer& buffer) const {
    buffer.writeUInt((unsigned)fTileModeX);
    buffer.writeUInt((unsigned)fTileModeY);
    buffer.writeSampling(fSampling);
    buffer.writeImage(fImage.get());
    SkASSERT(fClampAsIfUnpremul == false);
    SkASSERT(!needs_subset(fImage.get(), fSubset));
    buffer.writeBool(fRaw);
}

bool SkImageShader::isOpaque() const {
    return skia_rs_image_shader_is_opaque(
            fImage->isOpaque(),
            static_cast<int32_t>(fTileModeX),
            static_cast<int32_t>(fTileModeY));
}

#ifdef SK_ENABLE_LEGACY_SHADERCONTEXT
SkShaderBase::Context* SkImageShader::onMakeContext(const ContextRec& rec,
                                                    SkArenaAlloc* alloc) const {
    SkASSERT(!needs_subset(fImage.get(), fSubset));
    if (fImage->alphaType() == kUnpremul_SkAlphaType) {
        return nullptr;
    }
    if (fImage->colorType() != kN32_SkColorType) {
        return nullptr;
    }
    if (fTileModeX != fTileModeY) {
        return nullptr;
    }
    if (fTileModeX == SkTileMode::kDecal || fTileModeY == SkTileMode::kDecal) {
        return nullptr;
    }

    SkSamplingOptions sampling = fSampling;
    if (sampling.isAniso()) {
        skia_rs_image_shader_aniso_fallback(fImage->hasMipmaps(), &sampling);
    }
    if (!skia_rs_image_shader_legacy_sampling_supported(&sampling)) {
        return nullptr;
    }
    if (fImage->width() > 32767 || fImage->height() > 32767) {
        return nullptr;
    }
    auto inv = rec.fMatrixRec.totalInverse();
    if (!inv || !skia_rs_image_shader_legacy_can_handle(
                        &*inv, SkOpts::S32_alpha_D32_filter_DXDY != nullptr)) {
        return nullptr;
    }
    if (!rec.isLegacyCompatible(fImage->colorSpace())) {
        return nullptr;
    }
    return SkBitmapProcLegacyShader::MakeContext(*this, fTileModeX, fTileModeY, sampling,
                                                 as_IB(fImage.get()), rec, alloc);
}
#endif

SkImage* SkImageShader::onIsAImage(SkMatrix* texM, SkTileMode xy[]) const {
    if (texM) {
        *texM = SkMatrix::I();
    }
    if (xy) {
        xy[0] = fTileModeX;
        xy[1] = fTileModeY;
    }
    return const_cast<SkImage*>(fImage.get());
}

sk_sp<SkShader> SkImageShader::Make(sk_sp<SkImage> image,
                                    SkTileMode tmx, SkTileMode tmy,
                                    const SkSamplingOptions& options,
                                    const SkMatrix* localMatrix,
                                    bool clampAsIfUnpremul) {
    SkRect subset = image ? SkRect::Make(image->dimensions()) : SkRect::MakeEmpty();
    return MakeSubset(std::move(image), subset, tmx, tmy, options, localMatrix, clampAsIfUnpremul);
}

sk_sp<SkShader> SkImageShader::MakeRaw(sk_sp<SkImage> image,
                                       SkTileMode tmx, SkTileMode tmy,
                                       const SkSamplingOptions& options,
                                       const SkMatrix* localMatrix) {
    switch (skia_rs_image_shader_classify_make_raw(image != nullptr, options.useCubic)) {
        case 0: return nullptr;
        case 1: return SkShaders::Empty();
        default: break;
    }
    auto subset = SkRect::Make(image->dimensions());
    sk_sp<SkShader> s = sk_make_sp<SkImageShader>(image,
                                                  subset,
                                                  tmx, tmy,
                                                  options,
                                                  /*raw=*/true,
                                                  /*clampAsIfUnpremul=*/false);
    return s->makeWithLocalMatrix(localMatrix ? *localMatrix : SkMatrix::I());
}

sk_sp<SkShader> SkImageShader::MakeSubset(sk_sp<SkImage> image,
                                          const SkRect& subset,
                                          SkTileMode tmx, SkTileMode tmy,
                                          const SkSamplingOptions& options,
                                          const SkMatrix* localMatrix,
                                          bool clampAsIfUnpremul) {
    int imgW = image ? image->width() : 0;
    int imgH = image ? image->height() : 0;
    switch (skia_rs_image_shader_classify_make_subset(
            image != nullptr, imgW, imgH, subset, &options)) {
        case 0: return nullptr;
        case 1: return SkShaders::Empty();
        default: break;
    }

    sk_sp<SkShader> s = sk_make_sp<SkImageShader>(std::move(image),
                                                  subset,
                                                  tmx, tmy,
                                                  options,
                                                  /*raw=*/false,
                                                  clampAsIfUnpremul);
    return s->makeWithLocalMatrix(localMatrix ? *localMatrix : SkMatrix::I());
}

std::pair<SkRect, sk_sp<SkShader>> SkImageShader::MakeForDrawRect(const SkImage* image,
                                                                  const SkPaint& paint,
                                                                  const SkSamplingOptions& sampling,
                                                                  SkRect src,
                                                                  SkRect dst,
                                                                  bool strictSrcSubset) {
    SkASSERT(image);
    SkASSERT(paint.getStyle() == SkPaint::kFill_Style && !paint.getPathEffect());
    SkASSERT(src.isFinite() && dst.isFinite() && dst.isSorted());

    SkiaRsDrawRectGeometry geom;
    skia_rs_image_shader_draw_rect_geometry(
            image->width(), image->height(),
            static_cast<int32_t>(image->colorType()),
            src, dst, &geom);
    if (!geom.valid) {
        return {SkRect::MakeEmpty(), nullptr};
    }

    sk_sp<SkShader> imgShader;
    if (strictSrcSubset) {
        imgShader = SkImageShader::MakeSubset(sk_ref_sp(image), geom.src,
                                              SkTileMode::kClamp, SkTileMode::kClamp,
                                              sampling, &geom.local_matrix);
    } else {
        imgShader = image->makeShader(SkTileMode::kClamp, SkTileMode::kClamp,
                                      sampling, &geom.local_matrix);
    }
    if (!imgShader) {
        return {SkRect::MakeEmpty(), nullptr};
    }
    if (geom.image_is_alpha_only && paint.getShader()) {
        imgShader = SkShaders::Blend(SkBlendMode::kDstIn, paint.refShader(), std::move(imgShader));
    }
    return {geom.dst, std::move(imgShader)};
}

void SkShaderBase::RegisterFlattenables() { SK_REGISTER_FLATTENABLE(SkImageShader); }

namespace {

struct MipLevelHelper {
    SK_ONLY_STACK_ALLOCATABLE;

    SkPixmap pm;
    SkMatrix inv;
    SkRasterPipelineContexts::GatherCtx* gather;
    SkRasterPipelineContexts::TileCtx* limitX;
    SkRasterPipelineContexts::TileCtx* limitY;
    SkRasterPipelineContexts::DecalTileCtx* decalCtx = nullptr;

    void allocAndInit(SkArenaAlloc* alloc,
                      const SkSamplingOptions& sampling,
                      SkTileMode tileModeX,
                      SkTileMode tileModeY) {
        SkiaRsMipLevelInitParams p;
        skia_rs_image_shader_mip_level_init(
                pm.width(), pm.height(), &sampling,
                static_cast<int32_t>(tileModeX), static_cast<int32_t>(tileModeY), &p);

        gather = alloc->make<SkRasterPipelineContexts::GatherCtx>();
        gather->pixels = pm.addr();
        gather->stride = pm.rowBytesAsPixels();
        gather->width = p.width_f;
        gather->height = p.height_f;
        if (p.has_cubic_weights) {
            std::memcpy(gather->weights, p.cubic_weights, sizeof(gather->weights));
        }
        gather->roundDownAtInteger = p.round_down_at_integer;

        limitX = alloc->make<SkRasterPipelineContexts::TileCtx>();
        limitY = alloc->make<SkRasterPipelineContexts::TileCtx>();
        limitX->scale = p.width_f;
        limitX->invScale = p.inv_width;
        limitX->mirrorBiasDir = p.mirror_bias_dir;
        limitY->scale = p.height_f;
        limitY->invScale = p.inv_height;
        limitY->mirrorBiasDir = p.mirror_bias_dir;

        if (p.has_decal) {
            decalCtx = alloc->make<SkRasterPipelineContexts::DecalTileCtx>();
            decalCtx->limit_x = p.decal_limit_x;
            decalCtx->limit_y = p.decal_limit_y;
            decalCtx->inclusiveEdge_x = p.decal_inclusive_edge_x;
            decalCtx->inclusiveEdge_y = p.decal_inclusive_edge_y;
        }
    }
};

}  // namespace

bool SkImageShader::appendStages(const SkStageRec& rec, const SkShaders::MatrixRec& mRec) const {
    SkASSERT(!needs_subset(fImage.get(), fSubset));

    auto sampling = fSampling;
    if (sampling.isAniso()) {
        skia_rs_image_shader_aniso_fallback(fImage->hasMipmaps(), &sampling);
    }

    SkRasterPipeline* p = rec.fPipeline;
    SkArenaAlloc* alloc = rec.fAlloc;

    SkMatrix baseInv;
    if (mRec.totalMatrixIsValid()) {
        auto inv = mRec.totalInverse();
        if (!inv) {
            return false;
        }
        baseInv = *inv;
        baseInv.normalizePerspective();
    }

    SkASSERT(!sampling.useCubic || sampling.mipmap == SkMipmapMode::kNone);
    auto* access = SkMipmapAccessor::Make(alloc, fImage.get(), baseInv, sampling.mipmap);
    if (!access) {
        return false;
    }

    MipLevelHelper upper;
    std::tie(upper.pm, upper.inv) = access->level();

    const bool coordsAlreadySeeded = mRec.rasterPipelineCoordsAreSeeded();
    const SkMatrix totalInv = SkMatrix::Concat(upper.inv, baseInv);
    if (!sampling.useCubic && mRec.totalMatrixIsValid()) {
        skia_rs_image_shader_tweak_sampling(&sampling, &totalInv, &sampling);
    }

    if (!mRec.apply(rec, upper.inv)) {
        return false;
    }

    upper.allocAndInit(alloc, sampling, fTileModeX, fTileModeY);

    MipLevelHelper lower;
    SkRasterPipelineContexts::MipmapCtx* mipmapCtx = nullptr;
    float lowerWeight = access->lowerWeight();
    if (lowerWeight > 0) {
        std::tie(lower.pm, lower.inv) = access->lowerLevel();
        mipmapCtx = alloc->make<SkRasterPipelineContexts::MipmapCtx>();
        mipmapCtx->lowerWeight = lowerWeight;
        mipmapCtx->scaleX = static_cast<float>(lower.pm.width()) / upper.pm.width();
        mipmapCtx->scaleY = static_cast<float>(lower.pm.height()) / upper.pm.height();

        lower.allocAndInit(alloc, sampling, fTileModeX, fTileModeY);
        p->append(SkRasterPipelineOp::mipmap_linear_init, mipmapCtx);
    }

    const bool decalBothAxes = fTileModeX == SkTileMode::kDecal && fTileModeY == SkTileMode::kDecal;

    auto append_tiling_and_gather = [&](const MipLevelHelper* level) {
        if (decalBothAxes) {
            p->append(SkRasterPipelineOp::decal_x_and_y, level->decalCtx);
        } else {
            switch (fTileModeX) {
                case SkTileMode::kClamp:  break;
                case SkTileMode::kMirror: p->append(SkRasterPipelineOp::mirror_x, level->limitX); break;
                case SkTileMode::kRepeat: p->append(SkRasterPipelineOp::repeat_x, level->limitX); break;
                case SkTileMode::kDecal:  p->append(SkRasterPipelineOp::decal_x,  level->decalCtx); break;
            }
            switch (fTileModeY) {
                case SkTileMode::kClamp:  break;
                case SkTileMode::kMirror: p->append(SkRasterPipelineOp::mirror_y, level->limitY); break;
                case SkTileMode::kRepeat: p->append(SkRasterPipelineOp::repeat_y, level->limitY); break;
                case SkTileMode::kDecal:  p->append(SkRasterPipelineOp::decal_y,  level->decalCtx); break;
            }
        }

        void* ctx = level->gather;
        switch (level->pm.colorType()) {
            case kAlpha_8_SkColorType:      p->append(SkRasterPipelineOp::gather_a8,    ctx); break;
            case kA16_unorm_SkColorType:    p->append(SkRasterPipelineOp::gather_a16,   ctx); break;
            case kA16_float_SkColorType:    p->append(SkRasterPipelineOp::gather_af16,  ctx); break;
            case kR16_float_SkColorType:    p->append(SkRasterPipelineOp::gather_rf16,  ctx); break;
            case kRGB_565_SkColorType:      p->append(SkRasterPipelineOp::gather_565,   ctx); break;
            case kARGB_4444_SkColorType:    p->append(SkRasterPipelineOp::gather_4444,  ctx); break;
            case kR8G8_unorm_SkColorType:   p->append(SkRasterPipelineOp::gather_rg88,  ctx); break;
            case kR16_unorm_SkColorType:    p->append(SkRasterPipelineOp::gather_r16,   ctx); break;
            case kR16G16_unorm_SkColorType: p->append(SkRasterPipelineOp::gather_rg1616,ctx); break;
            case kR16G16_float_SkColorType: p->append(SkRasterPipelineOp::gather_rgf16, ctx); break;
            case kRGBA_8888_SkColorType:    p->append(SkRasterPipelineOp::gather_8888,  ctx); break;
            case kRGBA_1010102_SkColorType: p->append(SkRasterPipelineOp::gather_1010102, ctx); break;
            case kR16G16B16A16_unorm_SkColorType:
                p->append(SkRasterPipelineOp::gather_16161616, ctx);
                break;
            case kRGBA_F16Norm_SkColorType:
            case kRGBA_F16_SkColorType:     p->append(SkRasterPipelineOp::gather_f16,   ctx); break;
            case kRGBA_F32_SkColorType:     p->append(SkRasterPipelineOp::gather_f32,   ctx); break;
            case kBGRA_10101010_XR_SkColorType:
                p->append(SkRasterPipelineOp::gather_10101010_xr, ctx);
                p->append(SkRasterPipelineOp::swap_rb);
                break;
            case kRGBA_10x6_SkColorType:    p->append(SkRasterPipelineOp::gather_10x6,  ctx); break;
            case kGray_8_SkColorType:
                p->append(SkRasterPipelineOp::gather_a8, ctx);
                p->append(SkRasterPipelineOp::alpha_to_gray);
                break;
            case kR8_unorm_SkColorType:
                p->append(SkRasterPipelineOp::gather_a8, ctx);
                p->append(SkRasterPipelineOp::alpha_to_red);
                break;
            case kRGB_888x_SkColorType:
                p->append(SkRasterPipelineOp::gather_8888, ctx);
                p->append(SkRasterPipelineOp::force_opaque);
                break;
            case kRGB_F16F16F16x_SkColorType:
                p->append(SkRasterPipelineOp::gather_f16, ctx);
                p->append(SkRasterPipelineOp::force_opaque);
                break;
            case kBGRA_1010102_SkColorType:
                p->append(SkRasterPipelineOp::gather_1010102, ctx);
                p->append(SkRasterPipelineOp::swap_rb);
                break;
            case kRGB_101010x_SkColorType:
                p->append(SkRasterPipelineOp::gather_1010102, ctx);
                p->append(SkRasterPipelineOp::force_opaque);
                break;
            case kBGR_101010x_XR_SkColorType:
                p->append(SkRasterPipelineOp::gather_1010102_xr, ctx);
                p->append(SkRasterPipelineOp::force_opaque);
                p->append(SkRasterPipelineOp::swap_rb);
                break;
            case kBGR_101010x_SkColorType:
                p->append(SkRasterPipelineOp::gather_1010102, ctx);
                p->append(SkRasterPipelineOp::force_opaque);
                p->append(SkRasterPipelineOp::swap_rb);
                break;
            case kBGRA_8888_SkColorType:
                p->append(SkRasterPipelineOp::gather_8888, ctx);
                p->append(SkRasterPipelineOp::swap_rb);
                break;
            case kBGR_888x_SkColorType:
                p->append(SkRasterPipelineOp::gather_8888, ctx);
                p->append(SkRasterPipelineOp::force_opaque);
                p->append(SkRasterPipelineOp::swap_rb);
                break;
            case kSRGBA_8888_SkColorType:
                p->append(SkRasterPipelineOp::gather_8888, ctx);
                p->appendTransferFunction(*skcms_sRGB_TransferFunction());
                break;
            case kUnknown_SkColorType: SkASSERT(false);
        }
        if (level->decalCtx) {
            p->append(SkRasterPipelineOp::check_decal_mask, level->decalCtx);
        }
    };

    auto append_misc = [&] {
        SkColorSpace* cs = upper.pm.colorSpace();
        SkAlphaType   at = upper.pm.alphaType();

        if (SkColorTypeIsAlphaOnly(upper.pm.colorType()) && !fRaw) {
            p->appendSetRGB(alloc, rec.fPaintColor);
            cs = rec.fDstCS;
            at = kUnpremul_SkAlphaType;
        }

        if (sampling.useCubic) {
            p->append(skia_rs_image_shader_cubic_uses_clamp_01(
                              static_cast<int32_t>(at), fClampAsIfUnpremul)
                          ? SkRasterPipelineOp::clamp_01
                          : SkRasterPipelineOp::clamp_gamut);
        }

        if (!fRaw) {
            alloc->make<SkColorSpaceXformSteps>(cs, at, rec.fDstCS, kPremul_SkAlphaType)->apply(p);
        }
        return true;
    };

    SkColorType ct = upper.pm.colorType();
    SkMatrix totalM = mRec.totalMatrix();
    int32_t fastPath = skia_rs_image_shader_classify_fast_path(
            static_cast<int32_t>(ct), &sampling,
            static_cast<int32_t>(fTileModeX), static_cast<int32_t>(fTileModeY),
            rec.fDstBounds, &baseInv, coordsAlreadySeeded,
            mRec.totalMatrixIsValid(), &totalM, &totalInv);
    if (fastPath != 0) {
        switch (fastPath) {
            case 1:
                p->append(SkRasterPipelineOp::bilerp_clamp_8888_force_highp, upper.gather);
                break;
            case 2:
                p->append(SkRasterPipelineOp::bilerp_clamp_8888_uniform_y, upper.gather);
                break;
            case 3:
                p->append(SkRasterPipelineOp::bilerp_clamp_8888, upper.gather);
                break;
            case 4:
                p->append(SkRasterPipelineOp::bicubic_clamp_8888, upper.gather);
                break;
        }
        if (ct == kBGRA_8888_SkColorType) {
            p->append(SkRasterPipelineOp::swap_rb);
        }
        return append_misc();
    }

    SkRasterPipelineContexts::SamplerCtx* sampler =
            alloc->make<SkRasterPipelineContexts::SamplerCtx>();

    auto sample = [&](SkRasterPipelineOp setup_x,
                      SkRasterPipelineOp setup_y,
                      const MipLevelHelper* level) {
        p->append(setup_x, sampler);
        p->append(setup_y, sampler);
        append_tiling_and_gather(level);
        p->append(SkRasterPipelineOp::accumulate, sampler);
    };

    auto sample_level = [&](const MipLevelHelper* level) {
        if (sampling.useCubic) {
            skia_rs_image_shader_cubic_weights(
                    sampling.cubic.B, sampling.cubic.C, sampler->weights);

            p->append(SkRasterPipelineOp::bicubic_setup, sampler);

            sample(SkRasterPipelineOp::bicubic_n3x, SkRasterPipelineOp::bicubic_n3y, level);
            sample(SkRasterPipelineOp::bicubic_n1x, SkRasterPipelineOp::bicubic_n3y, level);
            sample(SkRasterPipelineOp::bicubic_p1x, SkRasterPipelineOp::bicubic_n3y, level);
            sample(SkRasterPipelineOp::bicubic_p3x, SkRasterPipelineOp::bicubic_n3y, level);

            sample(SkRasterPipelineOp::bicubic_n3x, SkRasterPipelineOp::bicubic_n1y, level);
            sample(SkRasterPipelineOp::bicubic_n1x, SkRasterPipelineOp::bicubic_n1y, level);
            sample(SkRasterPipelineOp::bicubic_p1x, SkRasterPipelineOp::bicubic_n1y, level);
            sample(SkRasterPipelineOp::bicubic_p3x, SkRasterPipelineOp::bicubic_n1y, level);

            sample(SkRasterPipelineOp::bicubic_n3x, SkRasterPipelineOp::bicubic_p1y, level);
            sample(SkRasterPipelineOp::bicubic_n1x, SkRasterPipelineOp::bicubic_p1y, level);
            sample(SkRasterPipelineOp::bicubic_p1x, SkRasterPipelineOp::bicubic_p1y, level);
            sample(SkRasterPipelineOp::bicubic_p3x, SkRasterPipelineOp::bicubic_p1y, level);

            sample(SkRasterPipelineOp::bicubic_n3x, SkRasterPipelineOp::bicubic_p3y, level);
            sample(SkRasterPipelineOp::bicubic_n1x, SkRasterPipelineOp::bicubic_p3y, level);
            sample(SkRasterPipelineOp::bicubic_p1x, SkRasterPipelineOp::bicubic_p3y, level);
            sample(SkRasterPipelineOp::bicubic_p3x, SkRasterPipelineOp::bicubic_p3y, level);

            p->append(SkRasterPipelineOp::move_dst_src);
        } else if (sampling.filter == SkFilterMode::kLinear) {
            p->append(SkRasterPipelineOp::bilinear_setup, sampler);

            sample(SkRasterPipelineOp::bilinear_nx, SkRasterPipelineOp::bilinear_ny, level);
            sample(SkRasterPipelineOp::bilinear_px, SkRasterPipelineOp::bilinear_ny, level);
            sample(SkRasterPipelineOp::bilinear_nx, SkRasterPipelineOp::bilinear_py, level);
            sample(SkRasterPipelineOp::bilinear_px, SkRasterPipelineOp::bilinear_py, level);

            p->append(SkRasterPipelineOp::move_dst_src);
        } else {
            append_tiling_and_gather(level);
        }
    };

    sample_level(&upper);

    if (mipmapCtx) {
        p->append(SkRasterPipelineOp::mipmap_linear_update, mipmapCtx);
        sample_level(&lower);
        p->append(SkRasterPipelineOp::mipmap_linear_finish, mipmapCtx);
    }

    return append_misc();
}

namespace SkShaders {

sk_sp<SkShader> Image(sk_sp<SkImage> image,
                      SkTileMode tmx, SkTileMode tmy,
                      const SkSamplingOptions& options,
                      const SkMatrix* localMatrix) {
    return SkImageShader::Make(std::move(image), tmx, tmy, options, localMatrix);
}

sk_sp<SkShader> RawImage(sk_sp<SkImage> image,
                         SkTileMode tmx, SkTileMode tmy,
                         const SkSamplingOptions& options,
                         const SkMatrix* localMatrix) {
    return SkImageShader::MakeRaw(std::move(image), tmx, tmy, options, localMatrix);
}

}  // namespace SkShaders

// ============================================================================
// SkBitmapProcShader.cpp
// ============================================================================

class BitmapProcShaderContext : public SkShaderBase::Context {
public:
    BitmapProcShaderContext(const SkShaderBase& shader, const SkShaderBase::ContextRec& rec,
                            SkBitmapProcState* state)
        : INHERITED(shader, rec)
        , fState(state)
        , fFlags(skia_rs_bitmap_proc_shader_flags(
                  fState->fPixmap.isOpaque(), this->getPaintAlpha())) {}

    uint32_t getFlags() const override { return fFlags; }

    void shadeSpan(int x, int y, SkPMColor dstC[], int count) override {
        const SkBitmapProcState& state = *fState;
        if (state.getShaderProc32()) {
            state.getShaderProc32()(&state, x, y, dstC, count);
            return;
        }

        const int BUF_MAX = 128;
        uint32_t buffer[BUF_MAX];
        SkBitmapProcState::MatrixProc   mproc = state.getMatrixProc();
        SkBitmapProcState::SampleProc32 sproc = state.getSampleProc32();
        const int max = state.maxCountForBufferSize(sizeof(buffer[0]) * BUF_MAX);

        SkASSERT(state.fPixmap.addr());

        for (;;) {
            int n = std::min(count, max);
            SkASSERT(n > 0 && n < BUF_MAX * 2);
            mproc(state, buffer, n, x, y);
            sproc(state, buffer, n, dstC);

            if ((count -= n) == 0) {
                break;
            }
            SkASSERT(count > 0);
            x += n;
            dstC += n;
        }
    }

private:
    raw_ptr<SkBitmapProcState> fState;
    uint32_t fFlags;

    using INHERITED = SkShaderBase::Context;
};

SkShaderBase::Context* SkBitmapProcLegacyShader::MakeContext(
        const SkShaderBase& shader, SkTileMode tmx, SkTileMode tmy,
        const SkSamplingOptions& sampling, const SkImage_Base* image,
        const ContextRec& rec, SkArenaAlloc* alloc) {
    auto totalInverse = rec.fMatrixRec.totalInverse();
    if (!totalInverse) {
        return nullptr;
    }

    SkBitmapProcState* state = alloc->make<SkBitmapProcState>(image, tmx, tmy);
    if (!state->setup(*totalInverse, rec.fPaintAlpha, sampling)) {
        return nullptr;
    }
    return alloc->make<BitmapProcShaderContext>(shader, rec, state);
}
