/*
 * Safe Rust bridge for:
 *   src/core/SkMemset_opts.cpp
 *   src/core/SkBlitMask_opts.cpp
 *   src/core/SkBlitRow_opts.cpp
 *   src/core/SkBlitRow_D32.cpp
 *   src/core/SkOpts.cpp
 * (Layer 3.3 skia-rs-raster).
 */

#include "include/core/SkColor.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTypes.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkFeatures.h"
#include "include/private/SkOnce.h"
#include "src/core/SkBitmapProcState.h"
#include "src/core/SkBlitMask.h"
#include "src/core/SkBlitRow.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkCpu.h"
#include "src/core/SkMemset.h"
#include "src/core/SkOpts.h"
#include "src/core/SkOptsTargets.h"
#include "src/core/SkSwizzlePriv.h"

#define SK_OPTS_TARGET SK_OPTS_TARGET_DEFAULT
#include "src/opts/SkOpts_SetTarget.h"

#include "src/opts/SkRasterPipeline_opts.h"  // IWYU pragma: keep

#include "src/opts/SkOpts_RestoreTarget.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <iterator>

extern "C" {
void skia_rs_memset16(uint16_t* dst, uint16_t value, int count);
void skia_rs_memset32(uint32_t* dst, uint32_t value, int count);
void skia_rs_memset64(uint64_t* dst, uint64_t value, int count);
void skia_rs_rect_memset16(uint16_t* dst, uint16_t value, int count, size_t row_bytes, int height);
void skia_rs_rect_memset32(uint32_t* dst, uint32_t value, int count, size_t row_bytes, int height);
void skia_rs_rect_memset64(uint64_t* dst, uint64_t value, int count, size_t row_bytes, int height);

void skia_rs_blit_mask_d32_a8_rgba(uint32_t* dst, size_t dst_rb,
                                   const uint8_t* mask, size_t mask_rb,
                                   uint32_t color, int w, int h);
void skia_rs_blit_mask_d32_a8_bgra(uint32_t* dst, size_t dst_rb,
                                   const uint8_t* mask, size_t mask_rb,
                                   uint32_t color, int w, int h);

void skia_rs_blit_row_color32(uint32_t* dst, int count, uint32_t color);
void skia_rs_blit_row_s32_opaque(uint32_t* dst, const uint32_t* src, int count, U8CPU alpha);
void skia_rs_blit_row_s32_blend(uint32_t* dst, const uint32_t* src, int count, U8CPU alpha);
void skia_rs_blit_row_s32a_opaque(uint32_t* dst, const uint32_t* src, int count, U8CPU alpha);
void skia_rs_blit_row_s32a_blend(uint32_t* dst, const uint32_t* src, int count, U8CPU alpha);
}  // extern "C"

// ============================================================================
// SkMemset_opts.cpp, SkBlitMask_opts.cpp, SkBlitRow_opts.cpp, SkOpts.cpp
// ============================================================================

namespace SkOpts {
    // Memset opts -> safe Rust implementations.
    void (*memset16)(uint16_t[], uint16_t, int)                 = skia_rs_memset16;
    void (*memset32)(uint32_t[], uint32_t, int)                 = skia_rs_memset32;
    void (*memset64)(uint64_t[], uint64_t, int)                 = skia_rs_memset64;
    void (*rect_memset16)(uint16_t[], uint16_t, int, size_t, int) = skia_rs_rect_memset16;
    void (*rect_memset32)(uint32_t[], uint32_t, int, size_t, int) = skia_rs_rect_memset32;
    void (*rect_memset64)(uint64_t[], uint64_t, int, size_t, int) = skia_rs_rect_memset64;

    // Keep safe Rust memset implementations active (do not overwrite with C++ SIMD opts).
    void Init_Memset() {}

    // BlitMask opts -> safe Rust implementation.
#ifdef SK_PMCOLOR_IS_BGRA
    void (*blit_mask_d32_a8)(SkPMColor*, size_t, const SkAlpha*, size_t, SkColor, int, int) =
            skia_rs_blit_mask_d32_a8_bgra;
#else
    void (*blit_mask_d32_a8)(SkPMColor*, size_t, const SkAlpha*, size_t, SkColor, int, int) =
            skia_rs_blit_mask_d32_a8_rgba;
#endif

    // Keep safe Rust blit_mask implementation active (do not overwrite with C++ SIMD opts).
    void Init_BlitMask() {}

    // BlitRow opts -> safe Rust implementations.
    void (*blit_row_color32)(SkPMColor*, int, SkPMColor) = skia_rs_blit_row_color32;
    void (*blit_row_s32a_opaque)(SkPMColor*, const SkPMColor*, int, U8CPU) =
            skia_rs_blit_row_s32a_opaque;

    // Keep safe Rust blit_row implementations active (do not overwrite with C++ SIMD opts).
    void Init_BlitRow() {}

    // RasterPipeline stage dispatch tables (kept compatible with src/opts/SkOpts_*.cpp).
    size_t raster_pipeline_lowp_stride  = SK_OPTS_NS::raster_pipeline_lowp_stride();
    size_t raster_pipeline_highp_stride = SK_OPTS_NS::raster_pipeline_highp_stride();

#define M(st) (StageFn)SK_OPTS_NS::st,
    StageFn ops_highp[] = { SK_RASTER_PIPELINE_OPS_ALL(M) };
    StageFn just_return_highp = (StageFn)SK_OPTS_NS::just_return;
    void (*start_pipeline_highp)(size_t, size_t, size_t, size_t, SkRasterPipelineStage*,
                                 SkSpan<SkRasterPipelineContexts::MemoryCtxPatch>,
                                 uint8_t*) =
            SK_OPTS_NS::start_pipeline;
#undef M

#define M(st) (StageFn)SK_OPTS_NS::lowp::st,
    StageFn ops_lowp[] = { SK_RASTER_PIPELINE_OPS_LOWP(M) };
    StageFn just_return_lowp = (StageFn)SK_OPTS_NS::lowp::just_return;
    void (*start_pipeline_lowp)(size_t, size_t, size_t, size_t, SkRasterPipelineStage*,
                                SkSpan<SkRasterPipelineContexts::MemoryCtxPatch>,
                                uint8_t*) =
            SK_OPTS_NS::lowp::start_pipeline;
#undef M

    // Each Init_foo() is defined in src/opts/SkOpts_foo.cpp.
    void Init_ml3();
    void Init_ml4();
    void Init_lasx();

    void Init() {
#if defined(SK_ENABLE_OPTIMIZE_SIZE)
        // All Init_foo functions are omitted when optimizing for size
#elif defined(SK_CPU_X86)
#if SK_CPU_X64_LEVEL < SK_CPU_X64_LEVEL_AVX2
        if (SkCpu::Supports(SkX64::ML3)) {
            Init_ml3();
        }
#endif

#if (SK_CPU_X64_LEVEL < SK_CPU_X64_LEVEL_ML4)
#if defined(SK_ENABLE_AVX512_OPTS) && !defined(SK_DISABLE_AVX512_OPTS)
        if (SkCpu::Supports(SkX64::ML4)) {
            Init_ml4();
        }
#endif
#endif

#elif defined(SK_CPU_LOONGARCH)
#if SK_CPU_LSX_LEVEL < SK_CPU_LSX_LEVEL_LASX
        if (SkCpu::Supports(SkLoongArch::ASX)) {
            Init_lasx();
        }
#endif
#endif

        SkOpts::Init_BitmapProcState();
        SkOpts::Init_BlitMask();
        SkOpts::Init_BlitRow();
        SkOpts::Init_Memset();
        SkOpts::Init_Swizzler();
    }
}  // namespace SkOpts

// ============================================================================
// SkBlitRow_D32.cpp
// ============================================================================

SkBlitRow::Proc32 SkBlitRow::Factory32(unsigned flags) {
    static constexpr auto kProcs = std::to_array<SkBlitRow::Proc32>({
        skia_rs_blit_row_s32_opaque,
        skia_rs_blit_row_s32_blend,
        nullptr,  // blit_row_s32a_opaque is in SkOpts
        skia_rs_blit_row_s32a_blend,
    });

    SkASSERT(flags < std::size(kProcs));
    flags &= std::size(kProcs) - 1;

    return flags == Flags32::kSrcPixelAlpha_Flag32 ? SkOpts::blit_row_s32a_opaque : kProcs[flags];
}

void SkBlitRow::Color32(SkPMColor dst[], int count, SkPMColor color) {
    switch (SkGetPackedA32(color)) {
        case   0: return;
        case 255: SkOpts::memset32(dst, color, count); return;
    }
    return SkOpts::blit_row_color32(dst, count, color);
}
