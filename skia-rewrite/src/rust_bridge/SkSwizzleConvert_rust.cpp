/*
 * Safe Rust bridge for:
 *   src/core/SkSwizzle.cpp
 *   src/core/SkSwizzler_opts.cpp
 *   src/core/SkConvertPixels.cpp
 * (Layer 2.1 skia-rs-color).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkSwizzle.h"
#include "include/effects/SkColorMatrix.h"
#include "include/private/SkAssert.h"
#include "include/private/SkTemplates.h"
#include "include/private/chromium/SkConvertPixelsWithColorMatrix.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkRectMemcpy.h"
#include "src/core/SkSwizzlePriv.h"

#include <cstddef>
#include <cstdint>

extern "C" {
void skia_rs_swizzle_rgba_to_bgra(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgba_to_rgba_premul(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgba_to_bgra_premul(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgba_premul_to_rgba(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgba_premul_to_rgba_fast(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgba_premul_to_bgra(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgba_premul_to_bgra_fast(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_inverted_cmyk_to_rgb1(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_inverted_cmyk_to_bgr1(uint32_t* dst, const uint32_t* src, int count);
void skia_rs_swizzle_rgb_to_rgb1(uint32_t* dst, const uint8_t* src, int count);
void skia_rs_swizzle_rgb_to_bgr1(uint32_t* dst, const uint8_t* src, int count);
void skia_rs_swizzle_gray_to_rgb1(uint32_t* dst, const uint8_t* src, int count);
void skia_rs_swizzle_graya_to_rgba(uint32_t* dst, const uint8_t* src, int count);
void skia_rs_swizzle_graya_to_rgba_premul(uint32_t* dst, const uint8_t* src, int count);

bool skia_rs_convert_validate_strides(int src_ct, size_t src_rb, int dst_ct, size_t dst_rb,
                                      bool same_buffer, int* out_src_stride, int* out_dst_stride);
int skia_rs_convert_select_fast_path(int src_ct, int dst_ct, SkColorSpaceXformSteps::Flags flags,
                                     bool allow_unpremul_swizzle);
bool skia_rs_convert_row_to_alpha8(int src_ct, const uint8_t* src_row, size_t src_row_len,
                                   uint8_t* dst_row, size_t width);
}  // extern "C"

// ============================================================================
// SkSwizzler_opts.cpp & SkSwizzle.cpp
// ============================================================================

namespace SkOpts {
    Swizzle_8888_u32 RGBA_to_BGRA          = skia_rs_swizzle_rgba_to_bgra;
    Swizzle_8888_u32 RGBA_to_rgbA          = skia_rs_swizzle_rgba_to_rgba_premul;
    Swizzle_8888_u32 RGBA_to_bgrA          = skia_rs_swizzle_rgba_to_bgra_premul;
#if defined(SK_USE_FAST_UNPREMUL_324099025)
    Swizzle_8888_u32 rgbA_to_RGBA          = skia_rs_swizzle_rgba_premul_to_rgba_fast;
    Swizzle_8888_u32 rgbA_to_BGRA          = skia_rs_swizzle_rgba_premul_to_bgra_fast;
#else
    Swizzle_8888_u32 rgbA_to_RGBA          = skia_rs_swizzle_rgba_premul_to_rgba;
    Swizzle_8888_u32 rgbA_to_BGRA          = skia_rs_swizzle_rgba_premul_to_bgra;
#endif
    Swizzle_8888_u32 inverted_CMYK_to_RGB1 = skia_rs_swizzle_inverted_cmyk_to_rgb1;
    Swizzle_8888_u32 inverted_CMYK_to_BGR1 = skia_rs_swizzle_inverted_cmyk_to_bgr1;

    Swizzle_8888_u8 RGB_to_RGB1            = skia_rs_swizzle_rgb_to_rgb1;
    Swizzle_8888_u8 RGB_to_BGR1            = skia_rs_swizzle_rgb_to_bgr1;
    Swizzle_8888_u8 gray_to_RGB1           = skia_rs_swizzle_gray_to_rgb1;
    Swizzle_8888_u8 grayA_to_RGBA          = skia_rs_swizzle_graya_to_rgba;
    Swizzle_8888_u8 grayA_to_rgbA          = skia_rs_swizzle_graya_to_rgba_premul;

    // Keep the safe Rust swizzler implementations active (do not overwrite with C++ SIMD opts).
    void Init_Swizzler() {}
}  // namespace SkOpts

void SkSwapRB(uint32_t* dest, const uint32_t* src, int count) {
    SkOpts::RGBA_to_BGRA(dest, src, count);
}

// ============================================================================
// SkConvertPixels.cpp
// ============================================================================

static void convert_with_pipeline(const SkImageInfo& dstInfo,
                                  void* dstRow,
                                  int dstStride,
                                  const SkImageInfo& srcInfo,
                                  const void* srcRow,
                                  int srcStride,
                                  const SkColorSpaceXformSteps& steps,
                                  const SkColorMatrix* srcMatrix,
                                  const SkColorMatrix* dstMatrix) {
    SkRasterPipelineContexts::MemoryCtx src = {const_cast<void*>(srcRow), srcStride};
    SkRasterPipelineContexts::MemoryCtx dst = {dstRow, dstStride};

    float srcRowMajor[20];
    float dstRowMajor[20];

    SkRasterPipeline_<256> pipeline;
    pipeline.appendLoad(srcInfo.colorType(), &src);
    if (srcMatrix) {
        if (srcInfo.alphaType() == kPremul_SkAlphaType) {
            pipeline.append(SkRasterPipelineOp::unpremul);
        }
        srcMatrix->getRowMajor(srcRowMajor);
        pipeline.append(SkRasterPipelineOp::matrix_4x5, srcRowMajor);
        pipeline.append(SkRasterPipelineOp::clamp_a_01);
    }
    steps.apply(&pipeline);
    if (dstMatrix) {
        dstMatrix->getRowMajor(dstRowMajor);
        pipeline.append(SkRasterPipelineOp::matrix_4x5, dstRowMajor);
        pipeline.append(SkRasterPipelineOp::clamp_a_01);
        if (dstInfo.alphaType() == kPremul_SkAlphaType) {
            pipeline.append(SkRasterPipelineOp::premul);
        }
    }
    pipeline.appendStore(dstInfo.colorType(), &dst);
    pipeline.run(0, 0, srcInfo.width(), srcInfo.height());
}

static bool convert_pixels_common(const SkImageInfo& srcInfo,
                                  const void* srcPixels,
                                  size_t srcRB,
                                  const SkColorMatrix* srcMatrix,
                                  const SkImageInfo& dstInfo,
                                  void* dstPixels,
                                  size_t dstRB,
                                  const SkColorMatrix* dstMatrix) {
    SkASSERT(dstInfo.dimensions() == srcInfo.dimensions());
    SkASSERT(SkImageInfoValidConversion(dstInfo, srcInfo));

    int srcStride = 0;
    int dstStride = 0;
    if (!skia_rs_convert_validate_strides(static_cast<int>(srcInfo.colorType()),
                                          srcRB,
                                          static_cast<int>(dstInfo.colorType()),
                                          dstRB,
                                          srcPixels == dstPixels,
                                          &srcStride,
                                          &dstStride)) {
        return false;
    }

    SkColorSpaceXformSteps steps{srcInfo.colorSpace(),
                                 srcMatrix ? kUnpremul_SkAlphaType : srcInfo.alphaType(),
                                 dstInfo.colorSpace(),
                                 dstMatrix ? kUnpremul_SkAlphaType : dstInfo.alphaType()};

    if (!srcMatrix && !dstMatrix) {
#if defined(SK_ARM_HAS_NEON)
        constexpr bool kAllowUnpremulSwizzle = true;
#else
        constexpr bool kAllowUnpremulSwizzle = false;
#endif
        int kind = skia_rs_convert_select_fast_path(static_cast<int>(srcInfo.colorType()),
                                                    static_cast<int>(dstInfo.colorType()),
                                                    steps.fFlags,
                                                    kAllowUnpremulSwizzle);
        switch (kind) {
            case 1: {  // RectMemcpy
                if (srcPixels != dstPixels) {
                    SkRectMemcpy(dstPixels, dstRB,
                                 srcPixels, srcRB, dstInfo.minRowBytes(), dstInfo.height());
                }
                return true;
            }
            case 2:    // SwizzleRgbaToBgra
            case 3:    // SwizzleRgbaToRgbaPremul
            case 4:    // SwizzleRgbaToBgraPremul
            case 5:    // SwizzleRgbaPremulToRgba
            case 6: {  // SwizzleRgbaPremulToBgra
                SkOpts::Swizzle_8888_u32 fn = nullptr;
                switch (kind) {
                    case 2: fn = SkOpts::RGBA_to_BGRA; break;
                    case 3: fn = SkOpts::RGBA_to_rgbA; break;
                    case 4: fn = SkOpts::RGBA_to_bgrA; break;
                    case 5: fn = SkOpts::rgbA_to_RGBA; break;
                    case 6: fn = SkOpts::rgbA_to_BGRA; break;
                    default: SkUNREACHABLE;
                }
                for (int y = 0; y < dstInfo.height(); ++y) {
                    fn(static_cast<uint32_t*>(dstPixels),
                       static_cast<const uint32_t*>(srcPixels),
                       dstInfo.width());
                    dstPixels = SkTAddOffset<void>(dstPixels, dstRB);
                    srcPixels = SkTAddOffset<const void>(srcPixels, srcRB);
                }
                return true;
            }
            case 7: {  // ConvertToAlpha8
                const size_t srcMinRowBytes = srcInfo.minRowBytes();
                const size_t width = static_cast<size_t>(srcInfo.width());
                auto* dst = static_cast<uint8_t*>(dstPixels);
                const auto* src = static_cast<const uint8_t*>(srcPixels);
                for (int y = 0; y < srcInfo.height(); ++y) {
                    if (!skia_rs_convert_row_to_alpha8(static_cast<int>(srcInfo.colorType()),
                                                       src, srcMinRowBytes, dst, width)) {
                        goto pipeline_fallback;
                    }
                    dst = SkTAddOffset<uint8_t>(dst, dstRB);
                    src = SkTAddOffset<const uint8_t>(src, srcRB);
                }
                return true;
            }
            default:
                break;
        }
    }

pipeline_fallback:
    convert_with_pipeline(dstInfo, dstPixels, dstStride,
                          srcInfo, srcPixels, srcStride,
                          steps, srcMatrix, dstMatrix);
    return true;
}

bool SkConvertPixels(const SkImageInfo& dstInfo,       void* dstPixels, size_t dstRB,
                     const SkImageInfo& srcInfo, const void* srcPixels, size_t srcRB) {
    return convert_pixels_common(srcInfo, srcPixels, srcRB, /*srcMatrix=*/nullptr,
                                 dstInfo, dstPixels, dstRB, /*dstMatrix=*/nullptr);
}

bool SkConvertPixelsWithColorMatrix(const SkPixmap& src,
                                    const SkColorMatrix* srcMatrix,
                                    const SkPixmap& dst,
                                    const SkColorMatrix* dstMatrix) {
    if (!src.addr() || !dst.writable_addr() ||
        src.dimensions() != dst.dimensions() ||
        !SkImageInfoValidConversion(dst.info(), src.info())) {
        return false;
    }
    return convert_pixels_common(src.info(), src.addr(), src.rowBytes(), srcMatrix,
                                 dst.info(), dst.writable_addr(), dst.rowBytes(), dstMatrix);
}
