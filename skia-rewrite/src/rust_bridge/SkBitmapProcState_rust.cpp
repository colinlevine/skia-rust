/*
 * Safe Rust bridge for:
 *   src/core/SkBitmapProcState.cpp
 *   src/core/SkBitmapProcState_matrixProcs.cpp
 *   src/core/SkBitmapProcState_opts.cpp
 * (Layer 3.4a skia-rs-bitmap).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkFeatures.h"
#include "include/private/SkFixed.h"
#include "src/core/SkBitmapProcState.h"
#include "src/core/SkMipmapAccessor.h"

#include <cstddef>
#include <cstdint>
#include <tuple>

class SkImage;
class SkImage_Base;

namespace {

struct SkiaRsProcStateParams {
    SkMatrix inv_matrix;
    int64_t inv_sx;
    int64_t inv_ky;
    int32_t filter_one_x;
    int32_t filter_one_y;
    int32_t width;
    int32_t height;
    int32_t tile_mode_x;
    int32_t tile_mode_y;
    uint16_t alpha_scale;
    bool bilerp;
};

struct SkiaRsChooseProcsResult {
    int64_t inv_sx;
    int64_t inv_ky;
    int32_t filter_one_x;
    int32_t filter_one_y;
    uint16_t alpha_scale;
    int32_t matrix_proc;
    int32_t shader_proc;
    bool is_scale_translate;
};

static_assert(sizeof(SkMatrix) == 40);
static_assert(offsetof(SkiaRsProcStateParams, inv_sx) == 40);
static_assert(sizeof(SkiaRsProcStateParams) == 88);
static_assert(sizeof(SkiaRsChooseProcsResult) == 40);

static SkiaRsProcStateParams make_params(const SkBitmapProcState& s) {
    SkiaRsProcStateParams p;
    p.inv_matrix = s.fInvMatrix;
    p.inv_sx = s.fInvSx;
    p.inv_ky = s.fInvKy;
    p.filter_one_x = s.fFilterOneX;
    p.filter_one_y = s.fFilterOneY;
    p.width = s.fPixmap.width();
    p.height = s.fPixmap.height();
    p.tile_mode_x = static_cast<int32_t>(s.fTileModeX);
    p.tile_mode_y = static_cast<int32_t>(s.fTileModeY);
    p.alpha_scale = s.fAlphaScale;
    p.bilerp = s.fBilerp;
    return p;
}

}  // namespace

extern "C" {
void skia_rs_bps_init_adjust_matrix_and_bilerp(SkMatrix* inv_matrix,
                                               int32_t width,
                                               int32_t height,
                                               int32_t tile_x,
                                               int32_t tile_y,
                                               bool* bilerp);
void skia_rs_bps_choose_procs(const SkMatrix* inv_matrix,
                              int32_t ct,
                              int32_t n32_ct,
                              int32_t width,
                              int32_t height,
                              int32_t tile_x,
                              int32_t tile_y,
                              bool bilerp,
                              uint8_t paint_alpha,
                              int32_t filter_one_x,
                              int32_t filter_one_y,
                              SkiaRsChooseProcsResult* out);
int32_t skia_rs_bps_choose_matrix_proc(const SkMatrix* inv_matrix,
                                       int32_t width,
                                       int32_t height,
                                       int32_t tile_x,
                                       int32_t tile_y,
                                       bool bilerp,
                                       bool translate_only_matrix,
                                       int32_t* filter_one_x,
                                       int32_t* filter_one_y);
int32_t skia_rs_bps_choose_shader_proc32(const SkMatrix* inv_matrix,
                                         int32_t ct,
                                         int32_t n32_ct,
                                         int32_t width,
                                         int32_t tile_x,
                                         int32_t tile_y,
                                         bool bilerp,
                                         uint16_t alpha_scale,
                                         int32_t* filter_one_x,
                                         int32_t* filter_one_y);
bool skia_rs_bps_setup_for_translate(const SkMatrix* inv_matrix,
                                     bool bilerp,
                                     int32_t filter_one_x,
                                     int32_t filter_one_y,
                                     int32_t* out_fx,
                                     int32_t* out_fy);
int32_t skia_rs_bps_max_count_for_buffer_size(size_t buffer_size,
                                              bool is_scale_translate,
                                              bool bilerp);
uint32_t skia_rs_bps_pack_clamp(int32_t f, uint32_t max);
uint32_t skia_rs_bps_pack_repeat(int32_t f, uint32_t max, size_t width);
uint32_t skia_rs_bps_pack_mirror(int32_t f, uint32_t max, size_t width);
void skia_rs_bps_run_matrix_proc(int32_t kind,
                                 const SkiaRsProcStateParams* s,
                                 uint32_t* xy,
                                 int32_t count,
                                 int32_t x,
                                 int32_t y);
bool skia_rs_bps_validate_matrix_proc_output(const uint32_t* xy,
                                             size_t count,
                                             uint32_t width,
                                             uint32_t height,
                                             bool is_scale_translate,
                                             bool bilerp);
void skia_rs_bps_s32_alpha_d32_filter_dx(const uint8_t* pixels,
                                         int32_t width,
                                         int32_t height,
                                         size_t row_bytes,
                                         uint32_t alpha_scale,
                                         const uint32_t* xy,
                                         int32_t count,
                                         uint32_t* colors);
void skia_rs_bps_s32_alpha_d32_filter_dxdy(const uint8_t* pixels,
                                           int32_t width,
                                           int32_t height,
                                           size_t row_bytes,
                                           uint32_t alpha_scale,
                                           const uint32_t* xy,
                                           int32_t count,
                                           uint32_t* colors);
void skia_rs_bps_s32_alpha_d32_nofilter_dx(const uint8_t* pixels,
                                           int32_t width,
                                           int32_t height,
                                           size_t row_bytes,
                                           uint32_t alpha_scale,
                                           const uint32_t* xy,
                                           int32_t count,
                                           uint32_t* colors);
void skia_rs_bps_s32_alpha_d32_nofilter_dxdy(const uint8_t* pixels,
                                             int32_t width,
                                             int32_t height,
                                             size_t row_bytes,
                                             uint32_t alpha_scale,
                                             const uint32_t* xy,
                                             int32_t count,
                                             uint32_t* colors);
void skia_rs_bps_clamp_s32_opaque_d32_nofilter_dx_shaderproc(const SkiaRsProcStateParams* s,
                                                             const uint8_t* pixels,
                                                             size_t row_bytes,
                                                             int32_t x,
                                                             int32_t y,
                                                             uint32_t* dst,
                                                             int32_t count);
void skia_rs_bps_clamp_s32_d32_nofilter_trans_shaderproc(const SkiaRsProcStateParams* s,
                                                         const uint8_t* pixels,
                                                         size_t row_bytes,
                                                         int32_t x,
                                                         int32_t y,
                                                         uint32_t* colors,
                                                         int32_t count);
void skia_rs_bps_repeat_s32_d32_nofilter_trans_shaderproc(const SkiaRsProcStateParams* s,
                                                          const uint8_t* pixels,
                                                          size_t row_bytes,
                                                          int32_t x,
                                                          int32_t y,
                                                          uint32_t* colors,
                                                          int32_t count);
void skia_rs_bps_s32_d32_const_x_shaderproc(const SkiaRsProcStateParams* s,
                                            int32_t matrix_proc_kind,
                                            const uint8_t* pixels,
                                            size_t row_bytes,
                                            int32_t x,
                                            int32_t y,
                                            uint32_t* colors,
                                            int32_t count);
void skia_rs_bps_do_nothing_shaderproc(uint32_t* colors, int32_t count);
}  // extern "C"

// ============================================================================
// Sample procs & SkOpts (SkBitmapProcState_opts.cpp)
// ============================================================================

namespace {

void rust_S32_alpha_D32_filter_DX(const SkBitmapProcState& s,
                                  const uint32_t* xy, int count, SkPMColor* colors) {
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(s.fBilerp);
    SkASSERT(4 == s.fPixmap.info().bytesPerPixel());
    SkASSERT(s.fAlphaScale <= 256);
    skia_rs_bps_s32_alpha_d32_filter_dx(static_cast<const uint8_t*>(s.fPixmap.addr()),
                                        s.fPixmap.width(),
                                        s.fPixmap.height(),
                                        s.fPixmap.rowBytes(),
                                        s.fAlphaScale,
                                        xy,
                                        count,
                                        colors);
}

#if defined(SK_ARM_HAS_NEON)
void rust_S32_alpha_D32_filter_DXDY(const SkBitmapProcState& s,
                                    const uint32_t* xy, int count, SkPMColor* colors) {
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(s.fBilerp);
    SkASSERT(4 == s.fPixmap.info().bytesPerPixel());
    SkASSERT(s.fAlphaScale <= 256);
    skia_rs_bps_s32_alpha_d32_filter_dxdy(static_cast<const uint8_t*>(s.fPixmap.addr()),
                                          s.fPixmap.width(),
                                          s.fPixmap.height(),
                                          s.fPixmap.rowBytes(),
                                          s.fAlphaScale,
                                          xy,
                                          count,
                                          colors);
}
#endif

void rust_S32_alpha_D32_nofilter_DX(const SkBitmapProcState& s,
                                    const uint32_t* xy, int count, SkPMColor* colors) {
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(s.fInvMatrix.isScaleTranslate());
    SkASSERT(!s.fBilerp);
    SkASSERT(4 == s.fPixmap.info().bytesPerPixel());
    SkASSERT(s.fAlphaScale <= 256);
    skia_rs_bps_s32_alpha_d32_nofilter_dx(static_cast<const uint8_t*>(s.fPixmap.addr()),
                                          s.fPixmap.width(),
                                          s.fPixmap.height(),
                                          s.fPixmap.rowBytes(),
                                          s.fAlphaScale,
                                          xy,
                                          count,
                                          colors);
}

void rust_S32_alpha_D32_nofilter_DXDY(const SkBitmapProcState& s,
                                      const uint32_t* xy, int count, SkPMColor* colors) {
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(!s.fBilerp);
    SkASSERT(4 == s.fPixmap.info().bytesPerPixel());
    SkASSERT(s.fAlphaScale <= 256);
    skia_rs_bps_s32_alpha_d32_nofilter_dxdy(static_cast<const uint8_t*>(s.fPixmap.addr()),
                                            s.fPixmap.width(),
                                            s.fPixmap.height(),
                                            s.fPixmap.rowBytes(),
                                            s.fAlphaScale,
                                            xy,
                                            count,
                                            colors);
}

// ============================================================================
// Matrix proc trampolines (SkBitmapProcState_matrixProcs.cpp)
// ============================================================================

template <int32_t Kind>
void matrix_proc_trampoline(const SkBitmapProcState& s,
                            uint32_t xy[], int count, int x, int y) {
    SkiaRsProcStateParams p = make_params(s);
    skia_rs_bps_run_matrix_proc(Kind, &p, xy, count, x, y);
}

SkBitmapProcState::MatrixProc kind_to_matrix_proc(int32_t kind) {
    switch (kind) {
        case 1:  return matrix_proc_trampoline<1>;
        case 2:  return matrix_proc_trampoline<2>;
        case 3:  return matrix_proc_trampoline<3>;
        case 4:  return matrix_proc_trampoline<4>;
        case 5:  return matrix_proc_trampoline<5>;
        case 6:  return matrix_proc_trampoline<6>;
        case 7:  return matrix_proc_trampoline<7>;
        case 8:  return matrix_proc_trampoline<8>;
        case 9:  return matrix_proc_trampoline<9>;
        case 10: return matrix_proc_trampoline<10>;
        case 11: return matrix_proc_trampoline<11>;
        case 12: return matrix_proc_trampoline<12>;
        case 13: return matrix_proc_trampoline<13>;
        case 14: return matrix_proc_trampoline<14>;
        case 15: return matrix_proc_trampoline<15>;
        default: return nullptr;
    }
}

// ============================================================================
// Shader proc trampolines (SkBitmapProcState.cpp)
// ============================================================================

void rust_Clamp_S32_opaque_D32_nofilter_DX_shaderproc(const void* sIn, int x, int y,
                                                      SkPMColor* dst, int count) {
    const SkBitmapProcState& s = *static_cast<const SkBitmapProcState*>(sIn);
    SkASSERT(s.fInvMatrix.isScaleTranslate());
    SkASSERT(s.fAlphaScale == 256);
    SkiaRsProcStateParams p = make_params(s);
    skia_rs_bps_clamp_s32_opaque_d32_nofilter_dx_shaderproc(
            &p, static_cast<const uint8_t*>(s.fPixmap.addr()), s.fPixmap.rowBytes(), x, y, dst, count);
}

void rust_Clamp_S32_D32_nofilter_trans_shaderproc(const void* sIn, int x, int y,
                                                  SkPMColor* colors, int count) {
    const SkBitmapProcState& s = *static_cast<const SkBitmapProcState*>(sIn);
    SkASSERT(s.fInvMatrix.isTranslate());
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(!s.fBilerp);
    SkiaRsProcStateParams p = make_params(s);
    skia_rs_bps_clamp_s32_d32_nofilter_trans_shaderproc(
            &p, static_cast<const uint8_t*>(s.fPixmap.addr()), s.fPixmap.rowBytes(), x, y, colors, count);
}

void rust_Repeat_S32_D32_nofilter_trans_shaderproc(const void* sIn, int x, int y,
                                                   SkPMColor* colors, int count) {
    const SkBitmapProcState& s = *static_cast<const SkBitmapProcState*>(sIn);
    SkASSERT(s.fInvMatrix.isTranslate());
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(!s.fBilerp);
    SkiaRsProcStateParams p = make_params(s);
    skia_rs_bps_repeat_s32_d32_nofilter_trans_shaderproc(
            &p, static_cast<const uint8_t*>(s.fPixmap.addr()), s.fPixmap.rowBytes(), x, y, colors, count);
}

void rust_S32_D32_constX_shaderproc(const void* sIn, int x, int y,
                                    SkPMColor* colors, int count) {
    const SkBitmapProcState& s = *static_cast<const SkBitmapProcState*>(sIn);
    SkASSERT(s.fInvMatrix.isScaleTranslate());
    SkASSERT(count > 0 && colors != nullptr);
    SkASSERT(1 == s.fPixmap.width());
    int32_t matrixKind = 0;
    if (s.fBilerp) {
        if (s.fTileModeX == SkTileMode::kClamp) {
            matrixKind = 5;   // ClampFilterScale
        } else if (s.fTileModeX == SkTileMode::kRepeat) {
            matrixKind = 9;   // RepeatFilterScale
        } else {
            matrixKind = 13;  // MirrorFilterScale
        }
    }
    SkiaRsProcStateParams p = make_params(s);
    skia_rs_bps_s32_d32_const_x_shaderproc(
            &p, matrixKind, static_cast<const uint8_t*>(s.fPixmap.addr()), s.fPixmap.rowBytes(), x, y, colors, count);
}

void rust_DoNothing_shaderproc(const void*, int, int, SkPMColor* colors, int count) {
    skia_rs_bps_do_nothing_shaderproc(colors, count);
}

SkBitmapProcState::ShaderProc32 kind_to_shader_proc(int32_t kind) {
    switch (kind) {
        case 1: return rust_Clamp_S32_opaque_D32_nofilter_DX_shaderproc;
        case 2: return rust_S32_D32_constX_shaderproc;
        case 3: return rust_Clamp_S32_D32_nofilter_trans_shaderproc;
        case 4: return rust_Repeat_S32_D32_nofilter_trans_shaderproc;
        case 5: return rust_DoNothing_shaderproc;
        default: return nullptr;
    }
}

}  // namespace

namespace SkOpts {
    void (*S32_alpha_D32_filter_DX)(const SkBitmapProcState&,
                                    const uint32_t* xy, int count, SkPMColor*)
        = rust_S32_alpha_D32_filter_DX;
#if defined(SK_ARM_HAS_NEON)
    void (*S32_alpha_D32_filter_DXDY)(const SkBitmapProcState&,
                                      const uint32_t* xy, int count, SkPMColor*)
        = rust_S32_alpha_D32_filter_DXDY;
#else
    // Match baseline Skia on non-NEON targets so SkImageShader falls back to SkRasterPipeline
    // for non-scale-translate matrices identically to baseline.
    void (*S32_alpha_D32_filter_DXDY)(const SkBitmapProcState&,
                                      const uint32_t* xy, int count, SkPMColor*)
        = nullptr;
#endif

    // Keep the safe Rust implementations active (do not overwrite with C++ SIMD opts).
    void Init_BitmapProcState() {}
}  // namespace SkOpts

// ============================================================================
// SkBitmapProcState member implementations
// ============================================================================

SkBitmapProcState::SkBitmapProcState(const SkImage_Base* image, SkTileMode tmx, SkTileMode tmy)
    : fImage(image)
    , fTileModeX(tmx)
    , fTileModeY(tmy) {}

bool SkBitmapProcState::init(const SkMatrix& inv, SkAlpha paintAlpha,
                             const SkSamplingOptions& sampling) {
    SkASSERT(!inv.hasPerspective());
    SkASSERT(SkOpts::S32_alpha_D32_filter_DXDY || inv.isScaleTranslate());
    SkASSERT(!sampling.isAniso());
    SkASSERT(!sampling.useCubic);
    SkASSERT(sampling.mipmap != SkMipmapMode::kLinear);

    fPixmap.reset();
    fBilerp = false;

    auto* access = SkMipmapAccessor::Make(&fAlloc, (const SkImage*)fImage, inv, sampling.mipmap);
    if (!access) {
        return false;
    }
    std::tie(fPixmap, fInvMatrix) = access->level();
    fInvMatrix.preConcat(inv);

    fPaintAlpha = paintAlpha;
    fBilerp = sampling.filter == SkFilterMode::kLinear;
    SkASSERT(fPixmap.addr());

    skia_rs_bps_init_adjust_matrix_and_bilerp(&fInvMatrix,
                                              fPixmap.width(),
                                              fPixmap.height(),
                                              static_cast<int32_t>(fTileModeX),
                                              static_cast<int32_t>(fTileModeY),
                                              &fBilerp);
    return true;
}

bool SkBitmapProcState::chooseProcs() {
    SkASSERT(!fInvMatrix.hasPerspective());
    SkASSERT(SkOpts::S32_alpha_D32_filter_DXDY || fInvMatrix.isScaleTranslate());
    SkASSERT(fPixmap.colorType() == kN32_SkColorType);
    SkASSERT(fPixmap.alphaType() == kPremul_SkAlphaType ||
             fPixmap.alphaType() == kOpaque_SkAlphaType);
    SkASSERT(fTileModeX != SkTileMode::kDecal);

    SkiaRsChooseProcsResult res{};
    skia_rs_bps_choose_procs(&fInvMatrix,
                             static_cast<int32_t>(fPixmap.colorType()),
                             static_cast<int32_t>(kN32_SkColorType),
                             fPixmap.width(),
                             fPixmap.height(),
                             static_cast<int32_t>(fTileModeX),
                             static_cast<int32_t>(fTileModeY),
                             fBilerp,
                             fPaintAlpha,
                             fFilterOneX,
                             fFilterOneY,
                             &res);

    fInvSx = res.inv_sx;
    fInvKy = res.inv_ky;
    fFilterOneX = res.filter_one_x;
    fFilterOneY = res.filter_one_y;
    fAlphaScale = res.alpha_scale;
    fMatrixProc = kind_to_matrix_proc(res.matrix_proc);
    SkASSERT(fMatrixProc);

    if (res.is_scale_translate) {
        fSampleProc32 = fBilerp ? SkOpts::S32_alpha_D32_filter_DX   : rust_S32_alpha_D32_nofilter_DX;
    } else {
        fSampleProc32 = fBilerp ? SkOpts::S32_alpha_D32_filter_DXDY : rust_S32_alpha_D32_nofilter_DXDY;
    }
    SkASSERT(fSampleProc32);

    fShaderProc32 = kind_to_shader_proc(res.shader_proc);
    return true;
}

bool SkBitmapProcState::setupForTranslate() {
    int32_t fx = 0;
    int32_t fy = 0;
    if (!skia_rs_bps_setup_for_translate(&fInvMatrix, fBilerp, fFilterOneX, fFilterOneY, &fx, &fy)) {
        return false;
    }
    fFilterOneX = fx;
    fFilterOneY = fy;
    return true;
}

SkBitmapProcState::ShaderProc32 SkBitmapProcState::chooseShaderProc32() {
    int32_t kind = skia_rs_bps_choose_shader_proc32(&fInvMatrix,
                                                    static_cast<int32_t>(fPixmap.colorType()),
                                                    static_cast<int32_t>(kN32_SkColorType),
                                                    fPixmap.width(),
                                                    static_cast<int32_t>(fTileModeX),
                                                    static_cast<int32_t>(fTileModeY),
                                                    fBilerp,
                                                    fAlphaScale,
                                                    &fFilterOneX,
                                                    &fFilterOneY);
    return kind_to_shader_proc(kind);
}

SkBitmapProcState::MatrixProc SkBitmapProcState::chooseMatrixProc(bool translate_only_matrix) {
    SkASSERT(!fInvMatrix.hasPerspective());
    SkASSERT(fTileModeX != SkTileMode::kDecal);
    int32_t kind = skia_rs_bps_choose_matrix_proc(&fInvMatrix,
                                                  fPixmap.width(),
                                                  fPixmap.height(),
                                                  static_cast<int32_t>(fTileModeX),
                                                  static_cast<int32_t>(fTileModeY),
                                                  fBilerp,
                                                  translate_only_matrix,
                                                  &fFilterOneX,
                                                  &fFilterOneY);
    SkASSERT(fTileModeX == fTileModeY);
    return kind_to_matrix_proc(kind);
}

#ifdef SK_DEBUG
void SkBitmapProcState::DebugMatrixProc(const SkBitmapProcState& state,
                                        uint32_t bitmapXY[], int count,
                                        int x, int y) {
    SkASSERT(bitmapXY);
    SkASSERT(count > 0);

    state.fMatrixProc(state, bitmapXY, count, x, y);

    SkASSERT(skia_rs_bps_validate_matrix_proc_output(
            bitmapXY,
            static_cast<size_t>(count),
            static_cast<uint32_t>(state.fPixmap.width()),
            static_cast<uint32_t>(state.fPixmap.height()),
            state.fInvMatrix.isScaleTranslate(),
            state.fBilerp));
}

SkBitmapProcState::MatrixProc SkBitmapProcState::getMatrixProc() const {
    return DebugMatrixProc;
}
#endif

int SkBitmapProcState::maxCountForBufferSize(size_t bufferSize) const {
    return skia_rs_bps_max_count_for_buffer_size(
            bufferSize, fInvMatrix.isScaleTranslate(), fBilerp);
}

// ============================================================================
// sktests helpers (SkBitmapProcState_matrixProcs.cpp)
// ============================================================================

uint32_t sktests::pack_clamp(SkFixed f, unsigned max) {
    return skia_rs_bps_pack_clamp(f, max);
}

uint32_t sktests::pack_repeat(SkFixed f, unsigned max, size_t width) {
    return skia_rs_bps_pack_repeat(f, max, width);
}

uint32_t sktests::pack_mirror(SkFixed f, unsigned max, size_t width) {
    return skia_rs_bps_pack_mirror(f, max, width);
}
