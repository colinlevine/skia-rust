/*
 * Safe Rust bridge for:
 *   src/core/SkRasterPipeline.cpp
 *   src/core/SkRasterPipelineBlitter.cpp
 * (Layer 3.3 skia-rs-raster).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSurfaceProps.h"
#include "include/private/SkAssert.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkOnce.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkCoreBlitters.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMask.h"
#include "src/core/SkMemset.h"
#include "src/core/SkOpts.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkRasterPipelineVizualizer.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"
#include "src/shaders/SkShaderBase.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <functional>
#include <optional>
#include <utility>
#include <vector>

using namespace skia_private;
using Op = SkRasterPipelineOp;

namespace {
struct SkiaRsConstantColorClassification {
    int32_t  kind;
    uint16_t rgba16[4];
};
static_assert(sizeof(SkiaRsConstantColorClassification) == 12);

struct SkiaRsMaskPlaneAddress {
    int32_t   stride;
    ptrdiff_t byte_offset;
};
static_assert(sizeof(SkiaRsMaskPlaneAddress) == 16);
}  // namespace

extern "C" {
bool skia_rs_rp_is_bounded_rgb(const float* rgb);
SkiaRsConstantColorClassification skia_rs_rp_classify_constant_color(const float* rgba);
int32_t skia_rs_rp_classify_matrix(uint32_t type_mask, bool has_perspective);
int32_t skia_rs_rp_classify_transfer_fn(int32_t tf_type, float a, float b, float c,
                                        float d, float e, float f);
bool skia_rs_rp_color_type_is_normalized(int32_t ct);
int32_t skia_rs_rp_stages_needed(int32_t num_stages, bool has_rewind_ctx);

bool skia_rs_rp_can_direct_blit(bool has_shader, bool has_color_filter, bool is_dither,
                                bool has_blend_mode, int32_t blend_mode, float alphaf);
float skia_rs_rp_dither_rate(int32_t ct);
bool skia_rs_rp_can_use_srcover_8888(bool has_blend_mode, int32_t blend_mode,
                                     int32_t dst_ct, bool has_dst_cs,
                                     int32_t dst_at, float dither_rate);
bool skia_rs_rp_should_prescale_coverage(bool has_blend_mode, int32_t blend_mode,
                                         bool rgb_coverage);
SkiaRsMaskPlaneAddress skia_rs_rp_mask_plane_address(int32_t bounds_left, int32_t bounds_top,
                                                     size_t row_bytes, int32_t plane,
                                                     size_t image_size, bool is_lcd16);
bool skia_rs_rp_can_use_memset2d(bool is_constant, bool has_blend_mode, int32_t blend_mode,
                                 int32_t bytes_per_pixel);
}  // extern "C"

// ============================================================================
// SkRasterPipeline.cpp
// ============================================================================

bool gForceHighPrecisionRasterPipeline;

SkRasterPipeline::SkRasterPipeline(SkArenaAlloc* alloc) : fAlloc(alloc) {
    this->reset();
}

void SkRasterPipeline::reset() {
    fRewindCtx   = nullptr;
    fStages      = nullptr;
    fTailPointer = nullptr;
    fNumStages   = 0;
    fMemoryCtxInfos.clear();
}

void SkRasterPipeline::append(SkRasterPipelineOp op, void* ctx) {
    SkASSERT(op != Op::uniform_color);
    SkASSERT(op != Op::unbounded_uniform_color);
    SkASSERT(op != Op::set_rgb);
    SkASSERT(op != Op::unbounded_set_rgb);
    SkASSERT(op != Op::parametric);
    SkASSERT(op != Op::gamma_);
    SkASSERT(op != Op::PQish);
    SkASSERT(op != Op::HLGish);
    SkASSERT(op != Op::HLGinvish);
    SkASSERT(op != Op::stack_checkpoint);
    SkASSERT(op != Op::stack_rewind);
    this->uncheckedAppend(op, ctx);
}

uint8_t* SkRasterPipeline::tailPointer() {
    if (!fTailPointer) {
        fTailPointer = fAlloc->make<uint8_t>(0xFF);
    }
    return fTailPointer;
}

void SkRasterPipeline::uncheckedAppend(SkRasterPipelineOp op, void* ctx) {
    bool isLoad = false, isStore = false;
    SkColorType ct = kUnknown_SkColorType;

#define COLOR_TYPE_CASE(stage_ct, sk_ct) \
    case Op::load_##stage_ct:            \
    case Op::load_##stage_ct##_dst:      \
        ct = sk_ct;                      \
        isLoad = true;                   \
        break;                           \
    case Op::store_##stage_ct:           \
        ct = sk_ct;                      \
        isStore = true;                  \
        break;

    switch (op) {
        COLOR_TYPE_CASE(a8, kAlpha_8_SkColorType)
        COLOR_TYPE_CASE(565, kRGB_565_SkColorType)
        COLOR_TYPE_CASE(4444, kARGB_4444_SkColorType)
        COLOR_TYPE_CASE(8888, kRGBA_8888_SkColorType)
        COLOR_TYPE_CASE(rg88, kR8G8_unorm_SkColorType)
        COLOR_TYPE_CASE(16161616, kR16G16B16A16_unorm_SkColorType)
        COLOR_TYPE_CASE(a16, kA16_unorm_SkColorType)
        COLOR_TYPE_CASE(r16, kR16_unorm_SkColorType)
        COLOR_TYPE_CASE(rg1616, kR16G16_unorm_SkColorType)
        COLOR_TYPE_CASE(f16, kRGBA_F16_SkColorType)
        COLOR_TYPE_CASE(af16, kA16_float_SkColorType)
        COLOR_TYPE_CASE(rf16, kR16_float_SkColorType)
        COLOR_TYPE_CASE(rgf16, kR16G16_float_SkColorType)
        COLOR_TYPE_CASE(f32, kRGBA_F32_SkColorType)
        COLOR_TYPE_CASE(1010102, kRGBA_1010102_SkColorType)
        COLOR_TYPE_CASE(1010102_xr, kBGR_101010x_XR_SkColorType)
        COLOR_TYPE_CASE(10101010_xr, kBGRA_10101010_XR_SkColorType)
        COLOR_TYPE_CASE(10x6, kRGBA_10x6_SkColorType)
#undef COLOR_TYPE_CASE

        case Op::debug_r:
        case Op::debug_g:
        case Op::debug_b:
        case Op::debug_a:
        case Op::debug_r_255:
        case Op::debug_g_255:
        case Op::debug_b_255:
        case Op::debug_a_255:
        case Op::debug_x:
        case Op::debug_y:
            ct = kRGBA_8888_SkColorType;
            isStore = true;
            break;
        case Op::store_r8:
            ct = kR8_unorm_SkColorType;
            isStore = true;
            break;
        case Op::srcover_rgba_8888:
            ct = kRGBA_8888_SkColorType;
            isLoad = true;
            isStore = true;
            break;
        case Op::scale_u8:
        case Op::lerp_u8:
            ct = kAlpha_8_SkColorType;
            isLoad = true;
            break;
        case Op::scale_565:
        case Op::lerp_565:
            ct = kRGB_565_SkColorType;
            isLoad = true;
            break;
        case Op::emboss: {
            auto* embossCtx = static_cast<SkRasterPipelineContexts::EmbossCtx*>(ctx);
            this->addMemoryContext(&embossCtx->add,
                                   SkColorTypeBytesPerPixel(kAlpha_8_SkColorType),
                                   true, false);
            this->addMemoryContext(&embossCtx->mul,
                                   SkColorTypeBytesPerPixel(kAlpha_8_SkColorType),
                                   true, false);
            break;
        }
        case Op::init_lane_masks: {
            auto* initCtx = static_cast<SkRasterPipelineContexts::InitLaneMasksCtx*>(ctx);
            initCtx->tail = this->tailPointer();
            break;
        }
        case Op::branch_if_all_lanes_active: {
            auto* branchCtx = static_cast<SkRasterPipelineContexts::BranchIfAllLanesActiveCtx*>(ctx);
            branchCtx->tail = this->tailPointer();
            break;
        }
        default:
            break;
    }

    fStages = fAlloc->make<StageList>(StageList{fStages, op, ctx});
    fNumStages += 1;

    if (isLoad || isStore) {
        SkASSERT(ct != kUnknown_SkColorType);
        this->addMemoryContext(static_cast<SkRasterPipelineContexts::MemoryCtx*>(ctx),
                               SkColorTypeBytesPerPixel(ct),
                               isLoad, isStore);
    }
}

void SkRasterPipeline::append(SkRasterPipelineOp op, uintptr_t ctx) {
    void* ptrCtx;
    memcpy(&ptrCtx, &ctx, sizeof(ctx));
    this->append(op, ptrCtx);
}

void SkRasterPipeline::extend(const SkRasterPipeline& src) {
    if (src.empty()) {
        return;
    }
    if (src.fRewindCtx && !fRewindCtx) {
        fRewindCtx = fAlloc->make<SkRasterPipelineContexts::RewindCtx>();
    }
    auto stages = fAlloc->makeArrayDefault<StageList>(src.fNumStages);

    int n = src.fNumStages;
    const StageList* st = src.fStages;
    while (n --> 1) {
        stages[n]      = *st;
        stages[n].prev = &stages[n - 1];

        switch (stages[n].stage) {
            case Op::stack_rewind:
                stages[n].ctx = fRewindCtx;
                break;
            case Op::init_lane_masks: {
                auto* ctx = static_cast<SkRasterPipelineContexts::InitLaneMasksCtx*>(stages[n].ctx);
                ctx->tail = this->tailPointer();
                break;
            }
            case Op::branch_if_all_lanes_active: {
                auto* ctx = static_cast<SkRasterPipelineContexts::BranchIfAllLanesActiveCtx*>(
                        stages[n].ctx);
                ctx->tail = this->tailPointer();
                break;
            }
            default:
                break;
        }
        st = st->prev;
    }
    stages[0]      = *st;
    stages[0].prev = fStages;

    fStages = &stages[src.fNumStages - 1];
    fNumStages += src.fNumStages;
    for (const SkRasterPipelineContexts::MemoryCtxInfo& info : src.fMemoryCtxInfos) {
        this->addMemoryContext(info.context, info.bytesPerPixel, info.load, info.store);
    }
}

const char* SkRasterPipeline::GetOpName(SkRasterPipelineOp op) {
    const char* name = "";
    switch (op) {
    #define M(x) case Op::x: name = #x; break;
        SK_RASTER_PIPELINE_OPS_ALL(M)
    #undef M
    }
    return name;
}

void SkRasterPipeline::dump() const {
    SkDebugf("SkRasterPipeline, %d stages\n", fNumStages);
    std::vector<const char*> stages;
    for (auto st = fStages; st; st = st->prev) {
        stages.push_back(GetOpName(st->stage));
    }
    std::reverse(stages.begin(), stages.end());
    for (const char* name : stages) {
        SkDebugf("\t%s\n", name);
    }
    SkDebugf("\n");
}

void SkRasterPipeline::appendSetRGB(SkArenaAlloc* alloc, const float rgb[3]) {
    auto arg = alloc->makeArrayDefault<float>(3);
    arg[0] = rgb[0];
    arg[1] = rgb[1];
    arg[2] = rgb[2];
    auto op = skia_rs_rp_is_bounded_rgb(rgb) ? Op::set_rgb : Op::unbounded_set_rgb;
    this->uncheckedAppend(op, arg);
}

void SkRasterPipeline::appendConstantColor(SkArenaAlloc* alloc, const float rgba[4]) {
    SkASSERT(0 <= rgba[3] && rgba[3] <= 1);
    SkiaRsConstantColorClassification cls = skia_rs_rp_classify_constant_color(rgba);
    if (cls.kind == 0) {
        this->append(Op::black_color);
    } else if (cls.kind == 1) {
        this->append(Op::white_color);
    } else {
        auto ctx = alloc->make<SkRasterPipelineContexts::UniformColorCtx>();
        ctx->r = rgba[0];
        ctx->g = rgba[1];
        ctx->b = rgba[2];
        ctx->a = rgba[3];
        if (cls.kind == 2) {
            ctx->rgba[0] = cls.rgba16[0];
            ctx->rgba[1] = cls.rgba16[1];
            ctx->rgba[2] = cls.rgba16[2];
            ctx->rgba[3] = cls.rgba16[3];
            this->uncheckedAppend(Op::uniform_color, ctx);
        } else {
            this->uncheckedAppend(Op::unbounded_uniform_color, ctx);
        }
    }
}

void SkRasterPipeline::appendMatrix(SkArenaAlloc* alloc, const SkMatrix& matrix) {
    int32_t kind = skia_rs_rp_classify_matrix(
            static_cast<uint32_t>(matrix.getType()), matrix.hasPerspective());
    switch (kind) {
        case 0:
            return;
        case 1: {
            float* trans = alloc->makeArrayDefault<float>(2);
            trans[0] = matrix.getTranslateX();
            trans[1] = matrix.getTranslateY();
            this->append(Op::matrix_translate, trans);
            break;
        }
        case 2: {
            float* scaleTrans = alloc->makeArrayDefault<float>(4);
            scaleTrans[0] = matrix.getScaleX();
            scaleTrans[1] = matrix.getScaleY();
            scaleTrans[2] = matrix.getTranslateX();
            scaleTrans[3] = matrix.getTranslateY();
            this->append(Op::matrix_scale_translate, scaleTrans);
            break;
        }
        case 3: {
            float* storage = alloc->makeArrayDefault<float>(9);
            matrix.get9(storage);
            this->append(Op::matrix_2x3, storage);
            break;
        }
        default: {
            float* storage = alloc->makeArrayDefault<float>(9);
            matrix.get9(storage);
            this->append(Op::matrix_perspective, storage);
            break;
        }
    }
}

void SkRasterPipeline::appendLoad(SkColorType ct, const SkRasterPipelineContexts::MemoryCtx* ctx) {
    switch (ct) {
        case kUnknown_SkColorType: SkASSERT(false); break;

        case kAlpha_8_SkColorType:           this->append(Op::load_a8,      ctx); break;
        case kA16_unorm_SkColorType:         this->append(Op::load_a16,     ctx); break;
        case kA16_float_SkColorType:         this->append(Op::load_af16,    ctx); break;
        case kRGB_565_SkColorType:           this->append(Op::load_565,     ctx); break;
        case kARGB_4444_SkColorType:         this->append(Op::load_4444,    ctx); break;
        case kR8G8_unorm_SkColorType:        this->append(Op::load_rg88,    ctx); break;
        case kR16_unorm_SkColorType:         this->append(Op::load_r16,     ctx); break;
        case kR16_float_SkColorType:         this->append(Op::load_rf16,    ctx); break;
        case kR16G16_unorm_SkColorType:      this->append(Op::load_rg1616,  ctx); break;
        case kR16G16_float_SkColorType:      this->append(Op::load_rgf16,   ctx); break;
        case kRGBA_8888_SkColorType:         this->append(Op::load_8888,    ctx); break;
        case kRGBA_1010102_SkColorType:      this->append(Op::load_1010102, ctx); break;
        case kR16G16B16A16_unorm_SkColorType:this->append(Op::load_16161616,ctx); break;
        case kRGBA_F16Norm_SkColorType:
        case kRGBA_F16_SkColorType:          this->append(Op::load_f16,     ctx); break;
        case kRGBA_F32_SkColorType:          this->append(Op::load_f32,     ctx); break;
        case kRGBA_10x6_SkColorType:         this->append(Op::load_10x6,    ctx); break;

        case kGray_8_SkColorType:            this->append(Op::load_a8, ctx);
                                             this->append(Op::alpha_to_gray);
                                             break;

        case kR8_unorm_SkColorType:          this->append(Op::load_a8, ctx);
                                             this->append(Op::alpha_to_red);
                                             break;

        case kRGB_888x_SkColorType:          this->append(Op::load_8888, ctx);
                                             this->append(Op::force_opaque);
                                             break;

        case kBGRA_1010102_SkColorType:      this->append(Op::load_1010102, ctx);
                                             this->append(Op::swap_rb);
                                             break;

        case kRGB_101010x_SkColorType:       this->append(Op::load_1010102, ctx);
                                             this->append(Op::force_opaque);
                                             break;

        case kBGR_101010x_SkColorType:       this->append(Op::load_1010102, ctx);
                                             this->append(Op::force_opaque);
                                             this->append(Op::swap_rb);
                                             break;

        case kBGRA_10101010_XR_SkColorType:  this->append(Op::load_10101010_xr, ctx);
                                             this->append(Op::swap_rb);
                                             break;

        case kBGR_101010x_XR_SkColorType:    this->append(Op::load_1010102_xr, ctx);
                                             this->append(Op::force_opaque);
                                             this->append(Op::swap_rb);
                                             break;
        case kRGB_F16F16F16x_SkColorType:    this->append(Op::load_f16, ctx);
                                             this->append(Op::force_opaque);
                                             break;

        case kBGRA_8888_SkColorType:         this->append(Op::load_8888, ctx);
                                             this->append(Op::swap_rb);
                                             break;

        case kBGR_888x_SkColorType:          this->append(Op::load_8888, ctx);
                                             this->append(Op::force_opaque);
                                             this->append(Op::swap_rb);
                                             break;

        case kSRGBA_8888_SkColorType:
            this->append(Op::load_8888, ctx);
            this->appendTransferFunction(*skcms_sRGB_TransferFunction());
            break;
    }
}

void SkRasterPipeline::appendLoadDst(SkColorType ct,
                                     const SkRasterPipelineContexts::MemoryCtx* ctx) {
    switch (ct) {
        case kUnknown_SkColorType: SkASSERT(false); break;

        case kAlpha_8_SkColorType:            this->append(Op::load_a8_dst,      ctx); break;
        case kA16_unorm_SkColorType:          this->append(Op::load_a16_dst,     ctx); break;
        case kA16_float_SkColorType:          this->append(Op::load_af16_dst,    ctx); break;
        case kRGB_565_SkColorType:            this->append(Op::load_565_dst,     ctx); break;
        case kARGB_4444_SkColorType:          this->append(Op::load_4444_dst,    ctx); break;
        case kR8G8_unorm_SkColorType:         this->append(Op::load_rg88_dst,    ctx); break;
        case kR16_unorm_SkColorType:          this->append(Op::load_r16_dst,     ctx); break;
        case kR16_float_SkColorType:          this->append(Op::load_rf16_dst,    ctx); break;
        case kR16G16_unorm_SkColorType:       this->append(Op::load_rg1616_dst,  ctx); break;
        case kR16G16_float_SkColorType:       this->append(Op::load_rgf16_dst,   ctx); break;
        case kRGBA_8888_SkColorType:          this->append(Op::load_8888_dst,    ctx); break;
        case kRGBA_1010102_SkColorType:       this->append(Op::load_1010102_dst, ctx); break;
        case kR16G16B16A16_unorm_SkColorType: this->append(Op::load_16161616_dst,ctx); break;
        case kRGBA_F16Norm_SkColorType:
        case kRGBA_F16_SkColorType:           this->append(Op::load_f16_dst,     ctx); break;
        case kRGBA_F32_SkColorType:           this->append(Op::load_f32_dst,     ctx); break;
        case kRGBA_10x6_SkColorType:          this->append(Op::load_10x6_dst,    ctx); break;

        case kGray_8_SkColorType:             this->append(Op::load_a8_dst, ctx);
                                              this->append(Op::alpha_to_gray_dst);
                                              break;

        case kR8_unorm_SkColorType:           this->append(Op::load_a8_dst, ctx);
                                              this->append(Op::alpha_to_red_dst);
                                              break;

        case kRGB_888x_SkColorType:           this->append(Op::load_8888_dst, ctx);
                                              this->append(Op::force_opaque_dst);
                                              break;

        case kBGRA_1010102_SkColorType:       this->append(Op::load_1010102_dst, ctx);
                                              this->append(Op::swap_rb_dst);
                                              break;

        case kRGB_101010x_SkColorType:        this->append(Op::load_1010102_dst, ctx);
                                              this->append(Op::force_opaque_dst);
                                              break;

        case kBGR_101010x_SkColorType:        this->append(Op::load_1010102_dst, ctx);
                                              this->append(Op::force_opaque_dst);
                                              this->append(Op::swap_rb_dst);
                                              break;

        case kBGR_101010x_XR_SkColorType:     this->append(Op::load_1010102_xr_dst, ctx);
                                              this->append(Op::force_opaque_dst);
                                              this->append(Op::swap_rb_dst);
                                              break;

        case kBGRA_10101010_XR_SkColorType:   this->append(Op::load_10101010_xr_dst, ctx);
                                              this->append(Op::swap_rb_dst);
                                              break;
        case kRGB_F16F16F16x_SkColorType:     this->append(Op::load_f16_dst, ctx);
                                              this->append(Op::force_opaque_dst);
                                              break;

        case kBGRA_8888_SkColorType:          this->append(Op::load_8888_dst, ctx);
                                              this->append(Op::swap_rb_dst);
                                              break;

        case kBGR_888x_SkColorType:           this->append(Op::load_8888_dst, ctx);
                                              this->append(Op::force_opaque_dst);
                                              this->append(Op::swap_rb_dst);
                                              break;

        case kSRGBA_8888_SkColorType:
            this->append(Op::load_8888_dst, ctx);
            this->append(Op::swap_src_dst);
            this->appendTransferFunction(*skcms_sRGB_TransferFunction());
            this->append(Op::swap_src_dst);
            break;
    }
}

void SkRasterPipeline::appendStore(SkColorType ct, const SkRasterPipelineContexts::MemoryCtx* ctx) {
    switch (ct) {
        case kUnknown_SkColorType: SkASSERT(false); break;

        case kAlpha_8_SkColorType:            this->append(Op::store_a8,      ctx); break;
        case kR8_unorm_SkColorType:           this->append(Op::store_r8,      ctx); break;
        case kA16_unorm_SkColorType:          this->append(Op::store_a16,     ctx); break;
        case kA16_float_SkColorType:          this->append(Op::store_af16,    ctx); break;
        case kRGB_565_SkColorType:            this->append(Op::store_565,     ctx); break;
        case kARGB_4444_SkColorType:          this->append(Op::store_4444,    ctx); break;
        case kR8G8_unorm_SkColorType:         this->append(Op::store_rg88,    ctx); break;
        case kR16_unorm_SkColorType:          this->append(Op::store_r16,     ctx); break;
        case kR16_float_SkColorType:          this->append(Op::store_rf16,    ctx); break;
        case kR16G16_unorm_SkColorType:       this->append(Op::store_rg1616,  ctx); break;
        case kR16G16_float_SkColorType:       this->append(Op::store_rgf16,   ctx); break;
        case kRGBA_8888_SkColorType:          this->append(Op::store_8888,    ctx); break;
        case kRGBA_1010102_SkColorType:       this->append(Op::store_1010102, ctx); break;
        case kR16G16B16A16_unorm_SkColorType: this->append(Op::store_16161616,ctx); break;
        case kRGBA_F16Norm_SkColorType:
        case kRGBA_F16_SkColorType:           this->append(Op::store_f16,     ctx); break;
        case kRGBA_F32_SkColorType:           this->append(Op::store_f32,     ctx); break;
        case kRGBA_10x6_SkColorType:          this->append(Op::store_10x6,    ctx); break;

        case kRGB_888x_SkColorType:           this->append(Op::force_opaque);
                                              this->append(Op::store_8888, ctx);
                                              break;

        case kBGRA_1010102_SkColorType:       this->append(Op::swap_rb);
                                              this->append(Op::store_1010102, ctx);
                                              break;

        case kRGB_101010x_SkColorType:        this->append(Op::force_opaque);
                                              this->append(Op::store_1010102, ctx);
                                              break;

        case kBGR_101010x_SkColorType:        this->append(Op::force_opaque);
                                              this->append(Op::swap_rb);
                                              this->append(Op::store_1010102, ctx);
                                              break;

        case kBGR_101010x_XR_SkColorType:     this->append(Op::force_opaque);
                                              this->append(Op::swap_rb);
                                              this->append(Op::store_1010102_xr, ctx);
                                              break;
        case kRGB_F16F16F16x_SkColorType:     this->append(Op::force_opaque);
                                              this->append(Op::store_f16, ctx);
                                              break;

        case kBGRA_10101010_XR_SkColorType:   this->append(Op::swap_rb);
                                              this->append(Op::store_10101010_xr, ctx);
                                              break;

        case kGray_8_SkColorType:             this->append(Op::bt709_luminance_or_luma_to_alpha);
                                              this->append(Op::store_a8, ctx);
                                              break;

        case kBGRA_8888_SkColorType:          this->append(Op::swap_rb);
                                              this->append(Op::store_8888, ctx);
                                              break;

        case kBGR_888x_SkColorType:           this->append(Op::force_opaque);
                                              this->append(Op::swap_rb);
                                              this->append(Op::store_8888, ctx);
                                              break;

        case kSRGBA_8888_SkColorType:
            this->appendTransferFunction(*skcms_sRGB_Inverse_TransferFunction());
            this->append(Op::store_8888, ctx);
            break;
    }
}

void SkRasterPipeline::appendTransferFunction(const skcms_TransferFunction& tf) {
    void* ctx = const_cast<void*>(static_cast<const void*>(&tf));
    int32_t kind = skia_rs_rp_classify_transfer_fn(
            static_cast<int32_t>(skcms_TransferFunction_getType(&tf)),
            tf.a, tf.b, tf.c, tf.d, tf.e, tf.f);
    switch (kind) {
        case 1: this->uncheckedAppend(Op::gamma_,     ctx); break;
        case 2: this->uncheckedAppend(Op::parametric, ctx); break;
        case 3: this->uncheckedAppend(Op::PQish,      ctx); break;
        case 4: this->uncheckedAppend(Op::HLGish,     ctx); break;
        case 5: this->uncheckedAppend(Op::HLGinvish,  ctx); break;
        default: SkASSERT(false); break;
    }
}

void SkRasterPipeline::appendClampIfNormalized(const SkImageInfo& info) {
    if (skia_rs_rp_color_type_is_normalized(static_cast<int32_t>(info.colorType()))) {
        this->uncheckedAppend(Op::clamp_01, nullptr);
    }
}

void SkRasterPipeline::appendStackRewind() {
    if (!fRewindCtx) {
        fRewindCtx = fAlloc->make<SkRasterPipelineContexts::RewindCtx>();
    }
    this->uncheckedAppend(Op::stack_rewind, fRewindCtx);
}

static void prepend_to_pipeline(SkRasterPipelineStage*& ip, SkOpts::StageFn stageFn, void* ctx) {
    --ip;
    ip->fn = stageFn;
    ip->ctx = ctx;
}

bool SkRasterPipeline::buildLowpPipeline(SkRasterPipelineStage* ip) const {
    if (gForceHighPrecisionRasterPipeline || fRewindCtx) {
        return false;
    }
    prepend_to_pipeline(ip, SkOpts::just_return_lowp, nullptr);
    for (const StageList* st = fStages; st; st = st->prev) {
        int opIndex = static_cast<int>(st->stage);
        if (opIndex >= kNumRasterPipelineLowpOps || !SkOpts::ops_lowp[opIndex]) {
            return false;
        }
        prepend_to_pipeline(ip, SkOpts::ops_lowp[opIndex], st->ctx);
    }
    return true;
}

void SkRasterPipeline::buildHighpPipeline(SkRasterPipelineStage* ip) const {
    prepend_to_pipeline(ip, SkOpts::just_return_highp, nullptr);
    for (const StageList* st = fStages; st; st = st->prev) {
        int opIndex = static_cast<int>(st->stage);
        prepend_to_pipeline(ip, SkOpts::ops_highp[opIndex], st->ctx);
    }
    if (fRewindCtx) {
        const int rewindIndex = static_cast<int>(Op::stack_checkpoint);
        prepend_to_pipeline(ip, SkOpts::ops_highp[rewindIndex], fRewindCtx);
    }
}

SkRasterPipeline::StartPipelineFn SkRasterPipeline::buildPipeline(SkRasterPipelineStage* ip) const {
    if (this->buildLowpPipeline(ip)) {
        return SkOpts::start_pipeline_lowp;
    }
    this->buildHighpPipeline(ip);
    return SkOpts::start_pipeline_highp;
}

int SkRasterPipeline::stagesNeeded() const {
    return skia_rs_rp_stages_needed(fNumStages, fRewindCtx != nullptr);
}

void SkRasterPipeline::run(size_t x, size_t y, size_t w, size_t h) const {
    if (this->empty()) {
        return;
    }
    int stagesNeeded = this->stagesNeeded();
    AutoSTMalloc<32, SkRasterPipelineStage> program(stagesNeeded);

    size_t numMemoryCtxs = fMemoryCtxInfos.size();
    AutoSTMalloc<2, SkRasterPipelineContexts::MemoryCtxPatch> patches(numMemoryCtxs);
    for (size_t i = 0; i < numMemoryCtxs; ++i) {
        patches[i].info = fMemoryCtxInfos[i];
        patches[i].backup = nullptr;
        memset(patches[i].scratch, 0, sizeof(patches[i].scratch));
    }

    auto start_pipeline = this->buildPipeline(program.get() + stagesNeeded);
    start_pipeline(x, y, x + w, y + h, program.get(),
                   SkSpan{patches.data(), numMemoryCtxs},
                   fTailPointer);
}

std::function<void(size_t, size_t, size_t, size_t)> SkRasterPipeline::compile() const {
    if (this->empty()) {
        return [](size_t, size_t, size_t, size_t) {};
    }
    int stagesNeeded = this->stagesNeeded();
    SkRasterPipelineStage* program = fAlloc->makeArray<SkRasterPipelineStage>(stagesNeeded);

    size_t numMemoryCtxs = fMemoryCtxInfos.size();
    SkRasterPipelineContexts::MemoryCtxPatch* patches =
            fAlloc->makeArray<SkRasterPipelineContexts::MemoryCtxPatch>(numMemoryCtxs);
    for (size_t i = 0; i < numMemoryCtxs; ++i) {
        patches[i].info = fMemoryCtxInfos[i];
        patches[i].backup = nullptr;
        memset(patches[i].scratch, 0, sizeof(patches[i].scratch));
    }
    uint8_t* tailPointer = fTailPointer;

    auto start_pipeline = this->buildPipeline(program + stagesNeeded);
    return [=](size_t x, size_t y, size_t w, size_t h) {
        start_pipeline(x, y, x + w, y + h, program, {patches, numMemoryCtxs}, tailPointer);
    };
}

void SkRasterPipeline::addMemoryContext(SkRasterPipelineContexts::MemoryCtx* ctx,
                                        int bytesPerPixel,
                                        bool load,
                                        bool store) {
    SkRasterPipelineContexts::MemoryCtxInfo* info = std::find_if(
            fMemoryCtxInfos.begin(), fMemoryCtxInfos.end(),
            [=](const SkRasterPipelineContexts::MemoryCtxInfo& i) { return i.context == ctx; });
    if (info != fMemoryCtxInfos.end()) {
        SkASSERT(bytesPerPixel == info->bytesPerPixel);
        info->load = info->load || load;
        info->store = info->store || store;
    } else {
        fMemoryCtxInfos.push_back(
                SkRasterPipelineContexts::MemoryCtxInfo{ctx, bytesPerPixel, load, store});
    }
}

// ============================================================================
// SkRasterPipelineBlitter.cpp
// ============================================================================

static bool can_direct_blit_rp(const SkPaint& paint) {
    auto mode = paint.asBlendMode();
    return skia_rs_rp_can_direct_blit(
            paint.getShader() != nullptr,
            paint.getColorFilter() != nullptr,
            paint.isDither(),
            mode.has_value(),
            mode.has_value() ? static_cast<int32_t>(*mode) : 0,
            paint.getAlphaf());
}

class SkRasterPipelineBlitter final : public SkBlitter {
public:
    static SkBlitter* Create(const SkPixmap& dst,
                             const SkPaint& paint,
                             const SkColor4f& dstPaintColor,
                             SkArenaAlloc* alloc,
                             const SkRasterPipeline& shaderPipeline,
                             bool is_opaque,
                             bool is_constant,
                             const SkShader* clipShader);

    SkRasterPipelineBlitter(SkPixmap dst, const SkPaint& paint, SkArenaAlloc* alloc)
            : fDst(std::move(dst))
            , fAlloc(alloc)
            , fColorPipeline(alloc)
            , fBlendPipeline(alloc)
            , fCanDirectBlit(can_direct_blit_rp(paint))
            , fDirectBlitPaintColor(paint.getColor4f()) {}

    void blitH(int x, int y, int w) override;
    void blitAntiH(int x, int y, const SkAlpha[], const int16_t[]) override;
    void blitAntiH2(int x, int y, U8CPU a0, U8CPU a1) override;
    void blitAntiV2(int x, int y, U8CPU a0, U8CPU a1) override;
    void blitMask(const SkMask&, const SkIRect& clip) override;
    void blitRect(int x, int y, int width, int height) override;
    void blitV(int x, int y, int height, SkAlpha alpha) override;
    std::optional<DirectBlit> canDirectBlit() override;

private:
    void appendLoadDst(SkRasterPipeline*) const;
    void appendStore(SkRasterPipeline*) const;
    void appendClipScale(SkRasterPipeline*) const;
    void appendClipLerp(SkRasterPipeline*) const;

    SkPixmap                   fDst;
    SkArenaAlloc*              fAlloc;
    SkRasterPipeline           fColorPipeline;
    SkRasterPipeline           fBlendPipeline;
    std::optional<SkBlendMode> fBlendMode;
    void*                      fClipShaderBuffer = nullptr;

    bool                    fCanDirectBlit;
    const SkColor4f         fDirectBlitPaintColor;
    std::optional<uint64_t> fDirectBlitValue;

    SkRasterPipelineContexts::MemoryCtx fDstPtr  = {nullptr, 0};
    SkRasterPipelineContexts::MemoryCtx fMaskPtr = {nullptr, 0};
    SkRasterPipelineContexts::EmbossCtx fEmbossCtx;

    void (*fMemset2D)(SkPixmap*, int x, int y, int w, int h, uint64_t color) = nullptr;
    uint64_t fMemsetColor = 0;

    std::function<void(size_t, size_t, size_t, size_t)> fBlitRect,
                                                        fBlitAntiH,
                                                        fBlitMaskA8,
                                                        fBlitMaskLCD16,
                                                        fBlitMask3D;

    float fCurrentCoverage = 0.0f;
    float fDitherRate      = 0.0f;

    using INHERITED = SkBlitter;
};

static SkColor4f paint_color_to_dst(const SkPaint& paint, const SkPixmap& dst) {
    SkColor4f paintColor = paint.getColor4f();
    SkColorSpaceXformSteps(sk_srgb_singleton(), kUnpremul_SkAlphaType,
                           dst.colorSpace(),    kUnpremul_SkAlphaType).apply(paintColor.vec());
    return paintColor;
}

static bool create_pipeline_for_blitter(const SkPixmap& dst,
                                        const SkPaint& paint,
                                        const SkMatrix& ctm,
                                        SkArenaAlloc* alloc,
                                        const SkSurfaceProps& props,
                                        SkRasterPipeline* shaderPipeline,
                                        SkColor4f* dstPaintColor,
                                        bool* isOpaqueOut,
                                        bool* isConstantOut,
                                        const SkRect& devBounds) {
    *dstPaintColor = paint_color_to_dst(paint, dst);

    auto shader = as_SB(paint.getShader());
    if (!shader) {
        shaderPipeline->appendConstantColor(alloc, dstPaintColor->premul().vec());
        *isOpaqueOut = dstPaintColor->fA == 1.0f;
        *isConstantOut = true;
        return true;
    }

    SkColorSpace* dstCS = dst.colorSpace();
    SkColorType dstCT = dst.colorType();
    *isOpaqueOut = shader->isOpaque() && dstPaintColor->fA == 1.0f;
    *isConstantOut = shader->isConstant();
    if (shader->appendRootStages(
                SkStageRec{shaderPipeline, alloc, dstCT, dstCS, *dstPaintColor, props, devBounds},
                ctm)) {
        if (dstPaintColor->fA != 1.0f) {
            shaderPipeline->append(SkRasterPipelineOp::scale_1_float,
                                   alloc->make<float>(dstPaintColor->fA));
        }
        return true;
    }
    return false;
}

SkBlitter* SkCreateRasterPipelineBlitter(const SkPixmap& dst,
                                         const SkPaint& paint,
                                         const SkMatrix& ctm,
                                         SkArenaAlloc* alloc,
                                         sk_sp<SkShader> clipShader,
                                         const SkSurfaceProps& props,
                                         const SkRect& devBounds) {
    SkRasterPipeline_<256> shaderPipeline;
    SkColor4f dstPaintColor;
    bool is_opaque, is_constant;
    if (!create_pipeline_for_blitter(dst, paint, ctm, alloc, props, &shaderPipeline,
                                     &dstPaintColor, &is_opaque, &is_constant, devBounds)) {
        return nullptr;
    }
    return SkRasterPipelineBlitter::Create(dst, paint, dstPaintColor, alloc, shaderPipeline,
                                           is_opaque, is_constant, clipShader.get());
}

namespace SkRasterPipelineVisualizer {

SkBlitter* CreateBlitter(const SkPixmap& dst,
                         const std::vector<DebugStage>& debugStages,
                         const SkPaint& paint,
                         const SkMatrix& ctm,
                         SkArenaAlloc* alloc,
                         sk_sp<SkShader> clipShader,
                         const SkSurfaceProps& props) {
    SkRasterPipeline_<256> shaderPipeline;
    SkColor4f dstPaintColor;
    bool is_opaque, is_constant;
    if (!create_pipeline_for_blitter(dst, paint, ctm, alloc, props, &shaderPipeline,
                                     &dstPaintColor, &is_opaque, &is_constant,
                                     SkRect::MakeEmpty())) {
        return nullptr;
    }

    static SkOnce once;
    once([&] { shaderPipeline.dump(); });

    std::vector<SkRasterPipeline::StageList> stages;
    for (auto st = shaderPipeline.getStageList(); st; st = st->prev) {
        stages.push_back(*st);
    }
    std::reverse(stages.begin(), stages.end());

    SkASSERT_RELEASE(stages.size() == debugStages.size());
    SkRasterPipeline_<256> shaderPipeline2;

    for (size_t i = 0; i < stages.size(); i++) {
        shaderPipeline2.append(stages[i].stage, stages[i].ctx);
        const auto& stage = debugStages[i];
        SkASSERT_RELEASE(stage.panels.size() == stage.ops.size());
        for (size_t j = 0; j < stage.panels.size() && j < stage.ops.size(); j++) {
            auto panelCtx = alloc->make<SkRasterPipelineContexts::MemoryCtx>();
            panelCtx->pixels = stage.panels[j].pixmap().writable_addr();
            panelCtx->stride = stage.panels[j].pixmap().rowBytesAsPixels();
            shaderPipeline2.append(stage.ops[j], panelCtx);
        }
    }

    return SkRasterPipelineBlitter::Create(dst, paint, dstPaintColor, alloc, shaderPipeline2,
                                           is_opaque, is_constant, clipShader.get());
}

}  // namespace SkRasterPipelineVisualizer

SkBlitter* SkCreateRasterPipelineBlitter(const SkPixmap& dst,
                                         const SkPaint& paint,
                                         const SkRasterPipeline& shaderPipeline,
                                         bool is_opaque,
                                         SkArenaAlloc* alloc,
                                         sk_sp<SkShader> clipShader) {
    bool is_constant = false;
    return SkRasterPipelineBlitter::Create(dst, paint, paint_color_to_dst(paint, dst), alloc,
                                           shaderPipeline, is_opaque, is_constant,
                                           clipShader.get());
}

SkBlitter* SkRasterPipelineBlitter::Create(const SkPixmap& dst,
                                           const SkPaint& paint,
                                           const SkColor4f& dstPaintColor,
                                           SkArenaAlloc* alloc,
                                           const SkRasterPipeline& shaderPipeline,
                                           bool is_opaque,
                                           bool is_constant,
                                           const SkShader* clipShader) {
    auto blitter = alloc->make<SkRasterPipelineBlitter>(dst, paint, alloc);
    auto colorPipeline = &blitter->fColorPipeline;

    if (clipShader) {
        auto clipP = colorPipeline;
        SkColorType clipCT = kRGBA_8888_SkColorType;
        SkColorSpace* clipCS = nullptr;
        SkSurfaceProps props{};
        SkStageRec rec = {
                clipP, alloc, clipCT, clipCS, SkColors::kBlack, props, SkRect::MakeEmpty()};
        if (as_SB(clipShader)->appendRootStages(rec, SkMatrix::I())) {
            struct Storage {
                float fA[SkRasterPipelineContexts::kMaxStride];
            };
            auto storage = alloc->make<Storage>();
            clipP->append(SkRasterPipelineOp::store_src_a, storage->fA);
            blitter->fClipShaderBuffer = storage->fA;
            is_constant = false;
        } else {
            return nullptr;
        }
    }

    colorPipeline->extend(shaderPipeline);

    if (auto colorFilter = paint.getColorFilter()) {
        SkSurfaceProps props{};
        SkStageRec rec = {colorPipeline, alloc, dst.colorType(), dst.colorSpace(),
                          dstPaintColor, props, SkRect::MakeEmpty()};
        if (!as_CFB(colorFilter)->appendStages(rec, is_opaque)) {
            return nullptr;
        }
        is_opaque = is_opaque && as_CFB(colorFilter)->isAlphaUnchanged();
    }

    if (paint.isDither() && !is_constant) {
        blitter->fDitherRate = skia_rs_rp_dither_rate(static_cast<int32_t>(dst.info().colorType()));
        if (blitter->fDitherRate > 0.0f) {
            colorPipeline->append(SkRasterPipelineOp::dither, &blitter->fDitherRate);
        }
    }

    if (is_constant) {
        SkColor4f constantColor;
        SkRasterPipelineContexts::MemoryCtx constantColorPtr = {&constantColor, 0};
        colorPipeline->appendClampIfNormalized(dst.info());
        colorPipeline->append(SkRasterPipelineOp::store_f32, &constantColorPtr);
        colorPipeline->run(0, 0, 1, 1);
        colorPipeline->reset();
        colorPipeline->appendConstantColor(alloc, constantColor);
        is_opaque = constantColor.fA == 1.0f;
    }

    auto blendPipeline = &blitter->fBlendPipeline;
    sk_sp<SkBlender> blender = paint.refBlender();
    if (!blender) {
        blender = SkBlender::Mode(SkBlendMode::kSrcOver);
    }

    if (is_opaque && as_BB(blender)->asBlendMode() == SkBlendMode::kSrcOver) {
        blender = SkBlender::Mode(SkBlendMode::kSrc);
    }

    auto bm = as_BB(blender)->asBlendMode();
    if (skia_rs_rp_can_use_memset2d(is_constant, bm.has_value(),
                                    bm.has_value() ? static_cast<int32_t>(*bm) : 0,
                                    dst.info().bytesPerPixel())) {
        SkRasterPipeline_<256> p;
        p.extend(*colorPipeline);
        blitter->fDstPtr = SkRasterPipelineContexts::MemoryCtx{&blitter->fMemsetColor, 0};
        blitter->appendStore(&p);
        p.run(0, 0, 1, 1);

        switch (blitter->fDst.shiftPerPixel()) {
            case 0: blitter->fMemset2D = [](SkPixmap* d, int x, int y, int w, int h, uint64_t c) {
                void* p = d->writable_addr(x, y);
                while (h --> 0) {
                    memset(p, c, w);
                    p = SkTAddOffset<void>(p, d->rowBytes());
                }
            }; break;

            case 1: blitter->fMemset2D = [](SkPixmap* d, int x, int y, int w, int h, uint64_t c) {
                SkOpts::rect_memset16(d->writable_addr16(x, y), c, w, d->rowBytes(), h);
            }; break;

            case 2: blitter->fMemset2D = [](SkPixmap* d, int x, int y, int w, int h, uint64_t c) {
                SkOpts::rect_memset32(d->writable_addr32(x, y), c, w, d->rowBytes(), h);
            }; break;

            case 3: blitter->fMemset2D = [](SkPixmap* d, int x, int y, int w, int h, uint64_t c) {
                SkOpts::rect_memset64(d->writable_addr64(x, y), c, w, d->rowBytes(), h);
            }; break;
        }
    }

    {
        SkSurfaceProps props{};
        SkStageRec rec = {blendPipeline, alloc, dst.colorType(), dst.colorSpace(),
                          dstPaintColor, props, SkRect::MakeEmpty()};
        if (!as_BB(blender)->appendStages(rec)) {
            return nullptr;
        }
        blitter->fBlendMode = as_BB(blender)->asBlendMode();
    }

    blitter->fDstPtr = SkRasterPipelineContexts::MemoryCtx{
        blitter->fDst.writable_addr(),
        blitter->fDst.rowBytesAsPixels(),
    };

    return blitter;
}

void SkRasterPipelineBlitter::appendLoadDst(SkRasterPipeline* p) const {
    p->appendLoadDst(fDst.info().colorType(), &fDstPtr);
    if (fDst.info().alphaType() == kUnpremul_SkAlphaType) {
        p->append(SkRasterPipelineOp::premul_dst);
    }
}

void SkRasterPipelineBlitter::appendStore(SkRasterPipeline* p) const {
    if (fDst.info().alphaType() == kUnpremul_SkAlphaType) {
        p->append(SkRasterPipelineOp::unpremul);
    }
    p->appendStore(fDst.info().colorType(), &fDstPtr);
}

void SkRasterPipelineBlitter::appendClipScale(SkRasterPipeline* p) const {
    if (fClipShaderBuffer) {
        p->append(SkRasterPipelineOp::scale_native, fClipShaderBuffer);
    }
}

void SkRasterPipelineBlitter::appendClipLerp(SkRasterPipeline* p) const {
    if (fClipShaderBuffer) {
        p->append(SkRasterPipelineOp::lerp_native, fClipShaderBuffer);
    }
}

void SkRasterPipelineBlitter::blitH(int x, int y, int w) {
    this->blitRect(x, y, w, 1);
}

void SkRasterPipelineBlitter::blitRect(int x, int y, int w, int h) {
    if (fMemset2D) {
        fMemset2D(&fDst, x, y, w, h, fMemsetColor);
        return;
    }

    if (!fBlitRect) {
        SkRasterPipeline p(fAlloc);
        p.extend(fColorPipeline);
        p.appendClampIfNormalized(fDst.info());
        if (skia_rs_rp_can_use_srcover_8888(
                    fBlendMode.has_value(),
                    fBlendMode.has_value() ? static_cast<int32_t>(*fBlendMode) : 0,
                    static_cast<int32_t>(fDst.info().colorType()),
                    fDst.colorSpace() != nullptr,
                    static_cast<int32_t>(fDst.info().alphaType()),
                    fDitherRate)) {
            if (fDst.info().colorType() == kBGRA_8888_SkColorType) {
                p.append(SkRasterPipelineOp::swap_rb);
            }
            this->appendClipScale(&p);
            p.append(SkRasterPipelineOp::srcover_rgba_8888, &fDstPtr);
        } else {
            if (fBlendMode != SkBlendMode::kSrc) {
                this->appendLoadDst(&p);
                p.extend(fBlendPipeline);
                this->appendClipLerp(&p);
            } else if (fClipShaderBuffer) {
                this->appendLoadDst(&p);
                this->appendClipLerp(&p);
            }
            this->appendStore(&p);
        }
        fBlitRect = p.compile();
    }

    fBlitRect(x, y, w, h);
}

void SkRasterPipelineBlitter::blitAntiH(int x, int y, const SkAlpha aa[], const int16_t runs[]) {
    if (!fBlitAntiH) {
        SkRasterPipeline p(fAlloc);
        p.extend(fColorPipeline);
        p.appendClampIfNormalized(fDst.info());
        if (skia_rs_rp_should_prescale_coverage(
                    fBlendMode.has_value(),
                    fBlendMode.has_value() ? static_cast<int32_t>(*fBlendMode) : 0,
                    false)) {
            p.append(SkRasterPipelineOp::scale_1_float, &fCurrentCoverage);
            this->appendClipScale(&p);
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
        } else {
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
            p.append(SkRasterPipelineOp::lerp_1_float, &fCurrentCoverage);
            this->appendClipLerp(&p);
        }
        this->appendStore(&p);
        fBlitAntiH = p.compile();
    }

    for (int16_t run = *runs; run > 0; run = *runs) {
        switch (*aa) {
            case 0x00: break;
            case 0xff: this->blitRect(x, y, run, 1); break;
            default:
                fCurrentCoverage = *aa * (1 / 255.0f);
                fBlitAntiH(x, y, run, 1);
        }
        x    += run;
        runs += run;
        aa   += run;
    }
}

void SkRasterPipelineBlitter::blitAntiH2(int x, int y, U8CPU a0, U8CPU a1) {
    SkIRect clip = {x, y, x + 2, y + 1};
    uint8_t coverage[] = {static_cast<uint8_t>(a0), static_cast<uint8_t>(a1)};
    SkMask mask(coverage, clip, 2, SkMask::kA8_Format);
    this->blitMask(mask, clip);
}

void SkRasterPipelineBlitter::blitAntiV2(int x, int y, U8CPU a0, U8CPU a1) {
    SkIRect clip = {x, y, x + 1, y + 2};
    uint8_t coverage[] = {static_cast<uint8_t>(a0), static_cast<uint8_t>(a1)};
    SkMask mask(coverage, clip, 1, SkMask::kA8_Format);
    this->blitMask(mask, clip);
}

void SkRasterPipelineBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    SkIRect clip = {x, y, x + 1, y + height};
    SkMask mask(&alpha, clip, 0, SkMask::kA8_Format);
    this->blitMask(mask, clip);
}

void SkRasterPipelineBlitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    if (mask.fFormat == SkMask::kBW_Format) {
        return INHERITED::blitMask(mask, clip);
    }

    SkASSERT(mask.fFormat == SkMask::kA8_Format
          || mask.fFormat == SkMask::kLCD16_Format
          || mask.fFormat == SkMask::k3D_Format);

    auto extract_mask_plane = [&mask](int plane, SkRasterPipelineContexts::MemoryCtx* ctx) {
        SkiaRsMaskPlaneAddress addr = skia_rs_rp_mask_plane_address(
                mask.fBounds.left(), mask.fBounds.top(), mask.fRowBytes,
                plane, mask.computeImageSize(), mask.fFormat == SkMask::kLCD16_Format);
        ctx->stride = addr.stride;
        ctx->pixels = reinterpret_cast<void*>(
                reinterpret_cast<uintptr_t>(mask.fImage) + static_cast<uintptr_t>(addr.byte_offset));
    };

    extract_mask_plane(0, &fMaskPtr);
    if (mask.fFormat == SkMask::k3D_Format) {
        extract_mask_plane(1, &fEmbossCtx.mul);
        extract_mask_plane(2, &fEmbossCtx.add);
    }

    bool hasBM = fBlendMode.has_value();
    int32_t bmVal = hasBM ? static_cast<int32_t>(*fBlendMode) : 0;

    if (mask.fFormat == SkMask::kA8_Format && !fBlitMaskA8) {
        SkRasterPipeline p(fAlloc);
        p.extend(fColorPipeline);
        p.appendClampIfNormalized(fDst.info());
        if (skia_rs_rp_should_prescale_coverage(hasBM, bmVal, false)) {
            p.append(SkRasterPipelineOp::scale_u8, &fMaskPtr);
            this->appendClipScale(&p);
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
        } else {
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
            p.append(SkRasterPipelineOp::lerp_u8, &fMaskPtr);
            this->appendClipLerp(&p);
        }
        this->appendStore(&p);
        fBlitMaskA8 = p.compile();
    }
    if (mask.fFormat == SkMask::kLCD16_Format && !fBlitMaskLCD16) {
        SkRasterPipeline p(fAlloc);
        p.extend(fColorPipeline);
        p.appendClampIfNormalized(fDst.info());
        if (skia_rs_rp_should_prescale_coverage(hasBM, bmVal, true)) {
            this->appendLoadDst(&p);
            p.append(SkRasterPipelineOp::scale_565, &fMaskPtr);
            this->appendClipScale(&p);
            p.extend(fBlendPipeline);
        } else {
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
            p.append(SkRasterPipelineOp::lerp_565, &fMaskPtr);
            this->appendClipLerp(&p);
        }
        this->appendStore(&p);
        fBlitMaskLCD16 = p.compile();
    }
    if (mask.fFormat == SkMask::k3D_Format && !fBlitMask3D) {
        SkRasterPipeline p(fAlloc);
        p.extend(fColorPipeline);
        p.append(SkRasterPipelineOp::emboss, &fEmbossCtx);
        p.appendClampIfNormalized(fDst.info());
        if (skia_rs_rp_should_prescale_coverage(hasBM, bmVal, false)) {
            p.append(SkRasterPipelineOp::scale_u8, &fMaskPtr);
            this->appendClipScale(&p);
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
        } else {
            this->appendLoadDst(&p);
            p.extend(fBlendPipeline);
            p.append(SkRasterPipelineOp::lerp_u8, &fMaskPtr);
            this->appendClipLerp(&p);
        }
        this->appendStore(&p);
        fBlitMask3D = p.compile();
    }

    std::function<void(size_t, size_t, size_t, size_t)>* blitter = nullptr;
    switch (mask.fFormat) {
        case SkMask::kA8_Format:    blitter = &fBlitMaskA8;    break;
        case SkMask::kLCD16_Format: blitter = &fBlitMaskLCD16; break;
        case SkMask::k3D_Format:    blitter = &fBlitMask3D;    break;
        default:
            SkASSERT(false);
            return;
    }

    SkASSERT(blitter);
    (*blitter)(clip.left(), clip.top(), clip.width(), clip.height());
}

std::optional<SkBlitter::DirectBlit> SkRasterPipelineBlitter::canDirectBlit() {
    if (fCanDirectBlit) {
        if (!fDirectBlitValue.has_value()) {
            union {
                uint64_t u8[1];
                uint32_t u4[2];
                uint16_t u2[4];
                uint8_t  u1[8];
            } dstBuffer;
            auto dst = SkImageInfo::Make(1, 1, fDst.info().colorType(), fDst.info().alphaType());
            auto src = SkImageInfo::Make(1, 1, kRGBA_F32_SkColorType, kUnpremul_SkAlphaType);
            if (!SkConvertPixels(dst, &dstBuffer, sizeof(dstBuffer),
                                 src, &fDirectBlitPaintColor, sizeof(fDirectBlitPaintColor))) {
                goto FAIL;
            }
            switch (dst.bytesPerPixel()) {
                case 1: fDirectBlitValue = dstBuffer.u1[0]; break;
                case 2: fDirectBlitValue = dstBuffer.u2[0]; break;
                case 4: fDirectBlitValue = dstBuffer.u4[0]; break;
                case 8: fDirectBlitValue = dstBuffer.u8[0]; break;
                default: goto FAIL;
            }
        }
        return {{fDst, fDirectBlitValue.value()}};
    }
FAIL:
    fCanDirectBlit = false;
    return {};
}
