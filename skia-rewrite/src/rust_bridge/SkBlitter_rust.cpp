/*
 * Safe Rust bridge for:
 *   src/core/SkBlitter.cpp
 *   src/core/SkBlitter_A8.cpp
 *   src/core/SkBlitter_ARGB32.cpp
 *   src/core/SkBlitter_Sprite.cpp
 *   src/core/SkSpriteBlitter_ARGB32.cpp
 * (Layer 3.3 skia-rs-raster).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkAlphaRuns.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkBlitMask.h"
#include "src/core/SkBlitRow.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkBlitter_A8.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkCoreBlitters.h"
#include "src/core/SkDrawTypes.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkMemset.h"
#include "src/core/SkPaintPriv.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkRegionPriv.h"
#include "src/core/SkSpriteBlitter.h"
#include "src/core/SkTLazy.h"
#include "src/shaders/SkShaderBase.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <utility>

using namespace skia_private;

#ifdef SK_PMCOLOR_IS_BGRA
static constexpr bool kPMColorIsBGRA = true;
#else
static constexpr bool kPMColorIsBGRA = false;
#endif

namespace {
struct SkiaRsFatAntiRectParams {
    SkIRect bounds;
    uint8_t top_l;
    uint8_t top_m;
    uint8_t top_r;
    uint8_t mid_l;
    uint8_t mid_r;
    uint8_t bot_l;
    uint8_t bot_m;
    uint8_t bot_r;
};
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkiaRsFatAntiRectParams) == 24);

struct SkiaRsBwMaskRowParams {
    int32_t bits_left;
    size_t  row_bytes;
    uint8_t left_mask;
    uint8_t right_mask;
};
static_assert(sizeof(SkiaRsBwMaskRowParams) == 24);

struct SkiaRsAntiRectDispatch {
    int32_t kind;
    int32_t x;
    int32_t y;
    int32_t width;
    int32_t height;
    uint8_t left_alpha;
    uint8_t right_alpha;
};
static_assert(sizeof(SkiaRsAntiRectDispatch) == 24);
}  // namespace

extern "C" {
using SkiaRsBwRunCallback = void (*)(void* ctx, int32_t x, int32_t width);

// SkBlitter_A8
int32_t skia_rs_a8_choose_blitter(int32_t color_type, bool has_shader, bool has_color_filter,
                                  bool has_clip_shader, bool draw_coverage,
                                  bool has_blend_mode, int32_t blend_mode);
void skia_rs_a8_coverage_blit_h(uint8_t* dst, int32_t width);
void skia_rs_a8_coverage_blit_anti_h(uint8_t* dst, const uint8_t* aa, const int16_t* runs);
void skia_rs_a8_coverage_blit_v(uint8_t* dst, size_t dst_rb, int32_t height, uint8_t alpha);
void skia_rs_a8_coverage_blit_rect(uint8_t* dst, size_t dst_rb, int32_t width, int32_t height);
void skia_rs_a8_coverage_blit_mask_a8(uint8_t* dst, size_t dst_rb,
                                      const uint8_t* mask, size_t mask_rb,
                                      int32_t width, int32_t height);
void skia_rs_a8_blit_h(uint8_t* dst, int32_t width, uint8_t src, int32_t op_kind);
void skia_rs_a8_blit_anti_h(uint8_t* dst, const uint8_t* aa, const int16_t* runs,
                            uint8_t src, int32_t op_kind);
void skia_rs_a8_blit_v(uint8_t* dst, size_t dst_rb, int32_t height,
                       uint8_t src, uint8_t aa, int32_t op_kind);
void skia_rs_a8_blit_rect(uint8_t* dst, size_t dst_rb, int32_t width, int32_t height,
                          uint8_t src, int32_t op_kind);
void skia_rs_a8_blit_mask_a8(uint8_t* dst, size_t dst_rb,
                             const uint8_t* mask, size_t mask_rb,
                             int32_t width, int32_t height, uint8_t src, int32_t op_kind);

// SkBlitter_ARGB32
void skia_rs_argb32_blit_anti_h(uint32_t* dst, const uint8_t* aa, const int16_t* runs,
                                uint32_t pm_color, int32_t mode);
void skia_rs_argb32_blit_anti_h2(uint32_t* dst, uint32_t pm_color,
                                 uint32_t a0, uint32_t a1, int32_t mode);
void skia_rs_argb32_blit_anti_v2(uint32_t* dst, size_t dst_rb, uint32_t pm_color,
                                 uint32_t a0, uint32_t a1, int32_t mode);
void skia_rs_argb32_blit_v(uint32_t* dst, size_t dst_rb, int32_t height,
                           uint32_t pm_color, uint8_t alpha);
void skia_rs_argb32_blit_rect(uint32_t* dst, size_t dst_rb, int32_t width, int32_t height,
                              uint32_t pm_color);
void skia_rs_argb32_blit_mask_lcd16(uint32_t* dst, size_t dst_rb,
                                    const uint16_t* mask, size_t mask_rb,
                                    uint32_t color, int32_t width, int32_t height, bool is_bgra);
void skia_rs_argb32_blit_mask_bw(uint32_t* dst, size_t dst_rb,
                                 const uint8_t* bits, size_t bits_rb, int32_t bit_offset,
                                 int32_t width, int32_t height, uint32_t pm_color, bool opaque);
void skia_rs_argb32_blit_mask_argb32(uint32_t* dst, size_t dst_rb,
                                     const uint32_t* src, size_t src_rb,
                                     int32_t width, int32_t height, uint32_t src_color);
void skia_rs_argb32_shader_blend_row_a8(uint32_t* dst, const uint8_t* mask, const uint32_t* src,
                                        int32_t n, bool opaque);
void skia_rs_argb32_shader_blend_row_lcd16(uint32_t* dst, const uint16_t* mask, const uint32_t* src,
                                           int32_t n, bool opaque, bool is_bgra);
uint32_t skia_rs_argb32_shader_blit_v_interp(uint32_t src, uint32_t dst, uint32_t alpha);

// SkBlitter & SkBlitter_Sprite & SkSpriteBlitter_ARGB32
bool skia_rs_blitter_compute_fat_anti_rect(SkRect rect, SkiaRsFatAntiRectParams* out);
SkiaRsBwMaskRowParams skia_rs_blitter_bw_mask_row_params(int32_t clip_left, int32_t clip_right,
                                                         int32_t mask_left, int32_t mask_right);
void skia_rs_blitter_bw_bits_to_runs(int32_t x, const uint8_t* bits, size_t row_bytes,
                                     uint8_t left_mask, uint8_t right_mask,
                                     SkiaRsBwRunCallback cb, void* ctx);
int32_t skia_rs_blitter_clip_anti_span(int32_t* x, int32_t y, uint8_t** aa, int16_t** runs,
                                       SkIRect clip, bool trim_right);
void skia_rs_blitter_alpha_runs_break_at(int16_t* runs, uint8_t* aa, int32_t x);
void skia_rs_blitter_alpha_runs_break(int16_t* runs, uint8_t* aa, int32_t x, int32_t count);
void skia_rs_blitter_write_rgn_gap(int16_t* runs, uint8_t* aa, int32_t offset, int32_t gap);
bool skia_rs_blitter_clip_h(int32_t left, int32_t y, int32_t width, SkIRect clip,
                            int32_t* out_left, int32_t* out_width);
bool skia_rs_blitter_clip_v(int32_t x, int32_t y, int32_t height, SkIRect clip,
                            int32_t* out_y, int32_t* out_height);
SkiaRsAntiRectDispatch skia_rs_blitter_clip_rect_anti_rect(int32_t left, int32_t y,
                                                           int32_t width, int32_t height,
                                                           uint8_t left_alpha, uint8_t right_alpha,
                                                           SkIRect clip);
SkiaRsAntiRectDispatch skia_rs_blitter_dispatch_anti_rect_piece(int32_t orig_x, int32_t orig_width,
                                                                SkIRect piece,
                                                                uint8_t left_alpha,
                                                                uint8_t right_alpha);
int32_t skia_rs_blitter_choose_clip_kind(bool clip_is_empty, bool clip_is_rect,
                                         SkIRect clip_bounds, const SkIRect* ir);
bool skia_rs_blitter_use_legacy(bool force_raster_pipeline, bool is_dither,
                                int32_t dst_alpha_type, bool is_src_over,
                                bool has_3d_mask_filter, bool has_color_space,
                                bool is_srgb_color_space, bool has_shader,
                                bool color4f_fits_in_bytes, bool is_n32);
bool skia_rs_sprite_memcpy_supports(int32_t dst_ct, int32_t src_ct,
                                    bool has_mask_filter, bool has_color_filter,
                                    bool has_image_filter, uint8_t paint_alpha,
                                    bool has_blend_mode, int32_t blend_mode,
                                    bool src_is_opaque);
void skia_rs_sprite_memcpy_blit_rect(uint8_t* dst, size_t dst_rb,
                                     const uint8_t* src, size_t src_rb,
                                     size_t bytes_to_copy, int32_t height);
bool skia_rs_sprite_choose_l32_supports(bool has_color_filter, bool has_mask_filter,
                                        bool src_is_n32, bool is_src_over);
uint32_t skia_rs_sprite_d32_s32_flags(uint8_t alpha, bool src_is_opaque);
}  // extern "C"

// ============================================================================
// SkBlitter.cpp
// ============================================================================

bool gSkForceRasterPipelineBlitter{false};

SkBlitter::~SkBlitter() {}

void SkBlitter::blitFatAntiRect(const SkRect& rect) {
    SkiaRsFatAntiRectParams p;
    if (!skia_rs_blitter_compute_fat_anti_rect(rect, &p)) {
        return;
    }
    const SkIRect& bounds = p.bounds;
    SkASSERT(bounds.width() >= 3);

    int runSize = bounds.width() + 1;
    void* storage = this->allocBlitMemory(runSize * (sizeof(int16_t) + sizeof(SkAlpha)));
    int16_t* runs = reinterpret_cast<int16_t*>(storage);
    SkAlpha* alphas = reinterpret_cast<SkAlpha*>(runs + runSize);

    runs[0] = 1;
    runs[1] = bounds.width() - 2;
    runs[bounds.width() - 1] = 1;
    runs[bounds.width()] = 0;

    alphas[0] = p.top_l;
    alphas[1] = p.top_m;
    alphas[bounds.width() - 1] = p.top_r;
    this->blitAntiH(bounds.fLeft, bounds.fTop, alphas, runs);

    if (bounds.height() > 2) {
        this->blitAntiRect(bounds.fLeft, bounds.fTop + 1, bounds.width() - 2, bounds.height() - 2,
                           p.mid_l, p.mid_r);
    }

    if (bounds.height() > 1) {
        alphas[0] = p.bot_l;
        alphas[1] = p.bot_m;
        alphas[bounds.width() - 1] = p.bot_r;
        this->blitAntiH(bounds.fLeft, bounds.fBottom - 1, alphas, runs);
    }
}

void SkBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    if (alpha == 255) {
        this->blitRect(x, y, 1, height);
    } else {
        int16_t runs[2] = {1, 0};
        while (--height >= 0) {
            this->blitAntiH(x, y++, &alpha, runs);
        }
    }
}

void SkBlitter::blitRect(int x, int y, int width, int height) {
    SkASSERT(width > 0);
    while (--height >= 0) {
        this->blitH(x, y++, width);
    }
}

void SkBlitter::blitAntiRect(int x, int y, int width, int height,
                             SkAlpha leftAlpha, SkAlpha rightAlpha) {
    if (leftAlpha > 0) {
        this->blitV(x, y, height, leftAlpha);
    }
    x++;
    if (width > 0) {
        this->blitRect(x, y, width, height);
        x += width;
    }
    if (rightAlpha > 0) {
        this->blitV(x, y, height, rightAlpha);
    }
}

namespace {
struct BwBlitHContext {
    SkBlitter* blitter;
    int y;
};
void bw_emit_run(void* ctx, int32_t x, int32_t width) {
    auto* c = static_cast<BwBlitHContext*>(ctx);
    c->blitter->blitH(x, c->y, width);
}
}  // namespace

void SkBlitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(mask.fBounds.contains(clip));

    if (mask.fFormat == SkMask::kLCD16_Format) {
        return;
    }

    if (mask.fFormat == SkMask::kBW_Format) {
        int cx = clip.fLeft;
        int cy = clip.fTop;
        int height = clip.height();
        const uint8_t* bits = mask.getAddr1(cx, cy);
        SkiaRsBwMaskRowParams rowParams = skia_rs_blitter_bw_mask_row_params(
                cx, clip.fRight, mask.fBounds.fLeft, mask.fBounds.fRight);

        while (--height >= 0) {
            BwBlitHContext ctx{this, cy};
            skia_rs_blitter_bw_bits_to_runs(rowParams.bits_left, bits, rowParams.row_bytes,
                                            rowParams.left_mask, rowParams.right_mask,
                                            bw_emit_run, &ctx);
            bits += mask.fRowBytes;
            cy += 1;
        }
    } else {
        int width = clip.width();
        AutoSTMalloc<64, int16_t> runStorage(width + 1);
        int16_t* runs = runStorage.get();
        const uint8_t* aa = mask.getAddr8(clip.fLeft, clip.fTop);

        SkOpts::memset16(reinterpret_cast<uint16_t*>(runs), 1, width);
        runs[width] = 0;

        int height = clip.height();
        int y = clip.fTop;
        while (--height >= 0) {
            this->blitAntiH(clip.fLeft, y, aa, runs);
            aa += mask.fRowBytes;
            y += 1;
        }
    }
}

void SkBlitter::blitMaskRegion(const SkMask& mask, const SkRegion& clip) {
    if (clip.quickReject(mask.fBounds)) {
        return;
    }

    SkRegion::Cliperator clipper(clip, mask.fBounds);
    while (!clipper.done()) {
        const SkIRect& cr = clipper.rect();
        this->blitMask(mask, cr);
        clipper.next();
    }
}

void SkBlitter::blitRectRegion(const SkIRect& rect, const SkRegion& clip) {
    SkRegion::Cliperator clipper(clip, rect);
    while (!clipper.done()) {
        const SkIRect& cr = clipper.rect();
        this->blitRect(cr.fLeft, cr.fTop, cr.width(), cr.height());
        clipper.next();
    }
}

void SkBlitter::blitRegion(const SkRegion& clip) {
    SkRegionPriv::VisitSpans(clip, [this](const SkIRect& r) {
        this->blitRect(r.left(), r.top(), r.width(), r.height());
    });
}

// ============================================================================
// SkRectClipBlitter & SkRgnClipBlitter & SkBlitterClipper
// ============================================================================

static inline void dispatch_anti_rect(SkBlitter* blitter, const SkiaRsAntiRectDispatch& d) {
    switch (d.kind) {
        case 1:
            blitter->blitRect(d.x, d.y, d.width, d.height);
            break;
        case 2:
            blitter->blitV(d.x, d.y, d.height, d.left_alpha);
            break;
        case 3:
            blitter->blitAntiRect(d.x, d.y, d.width, d.height, d.left_alpha, d.right_alpha);
            break;
        default:
            break;
    }
}

void SkRectClipBlitter::blitH(int left, int y, int width) {
    SkASSERT(width > 0);
    int32_t outLeft = 0, outWidth = 0;
    if (skia_rs_blitter_clip_h(left, y, width, fClipRect, &outLeft, &outWidth)) {
        fBlitter->blitH(outLeft, y, outWidth);
    }
}

void SkRectClipBlitter::blitAntiH(int left, int y,
                                  const SkAlpha const_aa[],
                                  const int16_t const_runs[]) {
    SkAlpha* aa = const_cast<SkAlpha*>(const_aa);
    int16_t* runs = const_cast<int16_t*>(const_runs);
    int width = skia_rs_blitter_clip_anti_span(&left, y, &aa, &runs, fClipRect, true);
    if (width > 0) {
        SkASSERT(runs[width] == 0);
        fBlitter->blitAntiH(left, y, aa, runs);
    }
}

void SkRectClipBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    SkASSERT(height > 0);
    int32_t outY = 0, outHeight = 0;
    if (skia_rs_blitter_clip_v(x, y, height, fClipRect, &outY, &outHeight)) {
        fBlitter->blitV(x, outY, outHeight, alpha);
    }
}

void SkRectClipBlitter::blitRect(int left, int y, int width, int height) {
    SkIRect r;
    r.setLTRB(left, y, left + width, y + height);
    if (r.intersect(fClipRect)) {
        fBlitter->blitRect(r.fLeft, r.fTop, r.width(), r.height());
    }
}

void SkRectClipBlitter::blitAntiRect(int left, int y, int width, int height,
                                     SkAlpha leftAlpha, SkAlpha rightAlpha) {
    SkiaRsAntiRectDispatch d = skia_rs_blitter_clip_rect_anti_rect(
            left, y, width, height, leftAlpha, rightAlpha, fClipRect);
    dispatch_anti_rect(fBlitter, d);
}

void SkRectClipBlitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(mask.fBounds.contains(clip));
    SkIRect r = clip;
    if (r.intersect(fClipRect)) {
        fBlitter->blitMask(mask, r);
    }
}

void SkRgnClipBlitter::blitH(int x, int y, int width) {
    SkRegion::Spanerator span(*fRgn, y, x, x + width);
    int left, right;
    while (span.next(&left, &right)) {
        SkASSERT(left < right);
        fBlitter->blitH(left, y, right - left);
    }
}

void SkRgnClipBlitter::blitAntiH(int x, int y, const SkAlpha const_aa[],
                                 const int16_t const_runs[]) {
    SkAlpha* aa = const_cast<SkAlpha*>(const_aa);
    int16_t* runs = const_cast<int16_t*>(const_runs);

    int width = skia_rs_blitter_clip_anti_span(&x, y, &aa, &runs, fRgn->getBounds(), false);
    if (width <= 0) {
        return;
    }

    SkRegion::Spanerator span(*fRgn, y, x, x + width);
    int left, right;
    SkDEBUGCODE(const SkIRect& bounds = fRgn->getBounds();)

    int prevRite = x;
    while (span.next(&left, &right)) {
        SkASSERT(x <= left);
        SkASSERT(left < right);
        SkASSERT(left >= bounds.fLeft && right <= bounds.fRight);

        if (prevRite == x && left > x) {
            int dx = left - x;
            skia_rs_blitter_alpha_runs_break_at(runs, aa, dx);
            runs += dx;
            aa += dx;
            x = left;
            prevRite = left;
        }

        int offset = prevRite - x;
        skia_rs_blitter_alpha_runs_break(runs + offset, aa + offset, left - prevRite, right - left);

        if (left > prevRite) {
            skia_rs_blitter_write_rgn_gap(runs, aa, offset, left - prevRite);
        }

        prevRite = right;
    }

    if (prevRite > x) {
        runs[prevRite - x] = 0;
        fBlitter->blitAntiH(x, y, aa, runs);
    }
}

void SkRgnClipBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    SkIRect bounds;
    bounds.setXYWH(x, y, 1, height);
    SkRegion::Cliperator iter(*fRgn, bounds);
    while (!iter.done()) {
        const SkIRect& r = iter.rect();
        SkASSERT(bounds.contains(r));
        fBlitter->blitV(x, r.fTop, r.height(), alpha);
        iter.next();
    }
}

void SkRgnClipBlitter::blitRect(int x, int y, int width, int height) {
    SkIRect bounds;
    bounds.setXYWH(x, y, width, height);
    SkRegion::Cliperator iter(*fRgn, bounds);
    while (!iter.done()) {
        const SkIRect& r = iter.rect();
        SkASSERT(bounds.contains(r));
        fBlitter->blitRect(r.fLeft, r.fTop, r.width(), r.height());
        iter.next();
    }
}

void SkRgnClipBlitter::blitAntiRect(int x, int y, int width, int height,
                                    SkAlpha leftAlpha, SkAlpha rightAlpha) {
    SkIRect bounds;
    bounds.setXYWH(x, y, width + 2, height);
    SkRegion::Cliperator iter(*fRgn, bounds);
    while (!iter.done()) {
        const SkIRect& r = iter.rect();
        SkASSERT(bounds.contains(r));
        SkiaRsAntiRectDispatch d = skia_rs_blitter_dispatch_anti_rect_piece(
                x, width, r, leftAlpha, rightAlpha);
        dispatch_anti_rect(fBlitter, d);
        iter.next();
    }
}

void SkRgnClipBlitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(mask.fBounds.contains(clip));
    SkRegion::Cliperator iter(*fRgn, clip);
    const SkIRect& r = iter.rect();
    SkBlitter* blitter = fBlitter;
    while (!iter.done()) {
        blitter->blitMask(mask, r);
        iter.next();
    }
}

SkBlitter* SkBlitterClipper::apply(SkBlitter* blitter, const SkRegion* clip,
                                   const SkIRect* ir) {
    if (clip) {
        const SkIRect& clipR = clip->getBounds();
        int32_t kind = skia_rs_blitter_choose_clip_kind(clip->isEmpty(), clip->isRect(), clipR, ir);
        switch (kind) {
            case 1:
                blitter = &fNullBlitter;
                break;
            case 2:
                fRectBlitter.init(blitter, clipR);
                blitter = &fRectBlitter;
                break;
            case 3:
                fRgnBlitter.init(blitter, clip);
                blitter = &fRgnBlitter;
                break;
            default:
                break;
        }
    }
    return blitter;
}

bool SkBlitter::UseLegacyBlitter(const SkPixmap& device,
                                 const SkPaint& paint,
                                 const SkMatrix& matrix) {
#if defined(SK_FORCE_RASTER_PIPELINE_BLITTER)
    constexpr bool kCompileTimeForceRP = true;
#else
    constexpr bool kCompileTimeForceRP = false;
#endif
    const SkMaskFilterBase* mf = as_MFB(paint.getMaskFilter());
    auto cs = device.colorSpace();
    return skia_rs_blitter_use_legacy(
            gSkForceRasterPipelineBlitter || kCompileTimeForceRP,
            paint.isDither(),
            static_cast<int32_t>(device.alphaType()),
            paint.isSrcOver(),
            mf && mf->getFormat() == SkMask::k3D_Format,
            cs != nullptr,
            cs != nullptr && cs->isSRGB(),
            paint.getShader() != nullptr,
            paint.getColor4f().fitsInBytes(),
            device.colorType() == kN32_SkColorType);
}

SkBlitter* SkBlitter::Choose(const SkPixmap& device,
                             const SkMatrix& ctm,
                             const SkPaint& origPaint,
                             SkArenaAlloc* alloc,
                             SkDrawCoverage drawCoverage,
                             sk_sp<SkShader> clipShader,
                             const SkSurfaceProps& props,
                             const SkRect& devBounds) {
    SkASSERT(alloc);

    if (kUnknown_SkColorType == device.colorType()) {
        return alloc->make<SkNullBlitter>();
    }

    SkTCopyOnFirstWrite<SkPaint> paint(origPaint);

    if (auto mode = paint->asBlendMode()) {
        if (mode.value() != SkBlendMode::kSrcOver) {
            switch (CheckFastPath(*paint, SkColorTypeIsAlwaysOpaque(device.colorType()))) {
                case SkBlendFastPath::kSrcOver:
                    paint.writable()->setBlendMode(SkBlendMode::kSrcOver);
                    break;
                case SkBlendFastPath::kSkipDrawing:
                    return alloc->make<SkNullBlitter>();
                default:
                    break;
            }
        }

        if (mode.value() == SkBlendMode::kClear) {
            SkPaint* p = paint.writable();
            p->setShader(nullptr);
            p->setColorFilter(nullptr);
            p->setBlendMode(SkBlendMode::kSrc);
            p->setColor(0x00000000);
        }
    }

    if (paint->getColorFilter()) {
        SkPaintPriv::RemoveColorFilter(paint.writable(), device.colorSpace());
    }
    SkASSERT(!paint->getColorFilter());

    if (drawCoverage == SkDrawCoverage::kYes) {
        if (device.colorType() == kAlpha_8_SkColorType) {
            SkASSERT(!paint->getShader());
            SkASSERT(paint->isSrcOver());
            return alloc->make<SkA8_Coverage_Blitter>(device, *paint);
        }
        return alloc->make<SkNullBlitter>();
    }

    if (paint->isDither() && !SkPaintPriv::ShouldDither(*paint, device.colorType())) {
        paint.writable()->setDither(false);
    }

    auto CreateSkRPBlitter = [&]() -> SkBlitter* {
        auto blitter = SkCreateRasterPipelineBlitter(
                device, *paint, ctm, alloc, clipShader, props, devBounds);
        return blitter ? blitter : alloc->make<SkNullBlitter>();
    };

    if (clipShader || !UseLegacyBlitter(device, *paint, ctm)) {
        return CreateSkRPBlitter();
    }

    SkASSERT(device.colorType() == kN32_SkColorType);
    SkASSERT(paint->asBlendMode() == SkBlendMode::kSrcOver);

    SkShaderBase::Context* shaderContext = nullptr;
    if (paint->getShader()) {
        shaderContext = as_SB(paint->getShader())
                                ->makeContext({paint->getAlpha(),
                                               SkShaders::MatrixRec(ctm),
                                               device.colorType(),
                                               device.colorSpace(),
                                               props},
                                              alloc);
        if (!shaderContext) {
            return CreateSkRPBlitter();
        }
    }

    if (shaderContext) {
        return alloc->make<SkARGB32_Shader_Blitter>(device, *paint, shaderContext);
    } else if (paint->getColor() == SK_ColorBLACK) {
        return alloc->make<SkARGB32_Black_Blitter>(device, *paint);
    } else if (paint->getAlpha() == 0xFF) {
        return alloc->make<SkARGB32_Opaque_Blitter>(device, *paint);
    } else {
        return alloc->make<SkARGB32_Blitter>(device, *paint);
    }
}

SkShaderBlitter::SkShaderBlitter(const SkPixmap& device,
                                 const SkPaint& paint,
                                 SkShaderBase::Context* shaderContext)
        : SkRasterBlitter(device), fShader(paint.refShader()), fShaderContext(shaderContext) {
    SkASSERT(fShader);
    SkASSERT(fShaderContext);
}

SkShaderBlitter::~SkShaderBlitter() = default;

#ifdef SK_DEBUG
void SkRectClipCheckBlitter::blitH(int x, int y, int width) {
    SkASSERT(fClipRect.contains(SkIRect::MakeXYWH(x, y, width, 1)));
    fBlitter->blitH(x, y, width);
}

void SkRectClipCheckBlitter::blitAntiH(int x, int y, const SkAlpha aa[], const int16_t runs[]) {
    const int16_t* iter = runs;
    for (; *iter; iter += *iter)
        ;
    int width = iter - runs;
    SkASSERT(fClipRect.contains(SkIRect::MakeXYWH(x, y, width, 1)));
    fBlitter->blitAntiH(x, y, aa, runs);
}

void SkRectClipCheckBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    SkASSERT(fClipRect.contains(SkIRect::MakeXYWH(x, y, 1, height)));
    fBlitter->blitV(x, y, height, alpha);
}

void SkRectClipCheckBlitter::blitRect(int x, int y, int width, int height) {
    SkASSERT(fClipRect.contains(SkIRect::MakeXYWH(x, y, width, height)));
    fBlitter->blitRect(x, y, width, height);
}

void SkRectClipCheckBlitter::blitAntiRect(int x, int y, int width, int height,
                                          SkAlpha leftAlpha, SkAlpha rightAlpha) {
    bool skipLeft = !leftAlpha;
    bool skipRight = !rightAlpha;
    SkIRect r = SkIRect::MakeXYWH(x + skipLeft, y, width + 2 - skipRight - skipLeft, height);
    SkASSERT(r.isEmpty() || fClipRect.contains(r));
    fBlitter->blitAntiRect(x, y, width, height, leftAlpha, rightAlpha);
}

void SkRectClipCheckBlitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(mask.fBounds.contains(clip));
    SkASSERT(fClipRect.contains(clip));
    fBlitter->blitMask(mask, clip);
}

void SkRectClipCheckBlitter::blitAntiH2(int x, int y, U8CPU a0, U8CPU a1) {
    SkASSERT(fClipRect.contains(SkIRect::MakeXYWH(x, y, 2, 1)));
    fBlitter->blitAntiH2(x, y, a0, a1);
}

void SkRectClipCheckBlitter::blitAntiV2(int x, int y, U8CPU a0, U8CPU a1) {
    SkASSERT(fClipRect.contains(SkIRect::MakeXYWH(x, y, 1, 2)));
    fBlitter->blitAntiV2(x, y, a0, a1);
}
#endif

// ============================================================================
// SkBlitter_A8.cpp
// ============================================================================

SkA8_Coverage_Blitter::SkA8_Coverage_Blitter(const SkPixmap& device, const SkPaint& paint)
        : fDevice(device) {
    SkASSERT(nullptr == paint.getShader());
    SkASSERT(nullptr == paint.getColorFilter());
}

void SkA8_Coverage_Blitter::blitAntiH(int x, int y, const SkAlpha antialias[],
                                      const int16_t runs[]) {
    skia_rs_a8_coverage_blit_anti_h(fDevice.writable_addr8(x, y), antialias, runs);
}

void SkA8_Coverage_Blitter::blitH(int x, int y, int width) {
    skia_rs_a8_coverage_blit_h(fDevice.writable_addr8(x, y), width);
}

void SkA8_Coverage_Blitter::blitV(int x, int y, int height, SkAlpha alpha) {
    skia_rs_a8_coverage_blit_v(fDevice.writable_addr8(x, y), fDevice.rowBytes(), height, alpha);
}

void SkA8_Coverage_Blitter::blitRect(int x, int y, int width, int height) {
    skia_rs_a8_coverage_blit_rect(fDevice.writable_addr8(x, y), fDevice.rowBytes(), width, height);
}

void SkA8_Coverage_Blitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    if (SkMask::kA8_Format != mask.fFormat) {
        this->SkBlitter::blitMask(mask, clip);
        return;
    }
    skia_rs_a8_coverage_blit_mask_a8(
            fDevice.writable_addr8(clip.fLeft, clip.fTop), fDevice.rowBytes(),
            mask.getAddr8(clip.fLeft, clip.fTop), mask.fRowBytes,
            clip.width(), clip.height());
}

class SkA8_Blitter final : public SkBlitter {
public:
    SkA8_Blitter(const SkPixmap& device, const SkPaint& paint, int32_t opKind)
            : fDevice(device), fOpKind(opKind), fSrc(paint.getAlpha()) {
        SkASSERT(nullptr == paint.getShader());
        SkASSERT(nullptr == paint.getColorFilter());
    }

    void blitH(int x, int y, int width) override {
        skia_rs_a8_blit_h(fDevice.writable_addr8(x, y), width, fSrc, fOpKind);
    }

    void blitAntiH(int x, int y, const SkAlpha antialias[], const int16_t runs[]) override {
        skia_rs_a8_blit_anti_h(fDevice.writable_addr8(x, y), antialias, runs, fSrc, fOpKind);
    }

    void blitV(int x, int y, int height, SkAlpha aa) override {
        skia_rs_a8_blit_v(fDevice.writable_addr8(x, y), fDevice.rowBytes(), height, fSrc, aa, fOpKind);
    }

    void blitRect(int x, int y, int width, int height) override {
        skia_rs_a8_blit_rect(fDevice.writable_addr8(x, y), fDevice.rowBytes(),
                             width, height, fSrc, fOpKind);
    }

    void blitMask(const SkMask& mask, const SkIRect& clip) override {
        if (SkMask::kA8_Format != mask.fFormat) {
            this->SkBlitter::blitMask(mask, clip);
            return;
        }
        skia_rs_a8_blit_mask_a8(
                fDevice.writable_addr8(clip.fLeft, clip.fTop), fDevice.rowBytes(),
                mask.getAddr8(clip.fLeft, clip.fTop), mask.fRowBytes,
                clip.width(), clip.height(), fSrc, fOpKind);
    }

private:
    const SkPixmap fDevice;
    int32_t        fOpKind;
    SkAlpha        fSrc;
};

SkBlitter* SkChooseA8Blitter(const SkPixmap& dst,
                             const SkMatrix& ctm,
                             const SkPaint& paint,
                             SkArenaAlloc* alloc,
                             SkDrawCoverage drawCoverage,
                             sk_sp<SkShader> clipShader) {
    auto mode = paint.asBlendMode();
    int32_t choice = skia_rs_a8_choose_blitter(
            static_cast<int32_t>(dst.colorType()),
            paint.getShader() != nullptr,
            paint.getColorFilter() != nullptr,
            clipShader != nullptr,
            drawCoverage == SkDrawCoverage::kYes,
            mode.has_value(),
            mode.has_value() ? static_cast<int32_t>(*mode) : 0);
    switch (choice) {
        case 1: return alloc->make<SkA8_Coverage_Blitter>(dst, paint);
        case 2: return alloc->make<SkA8_Blitter>(dst, paint, 0);
        case 3: return alloc->make<SkA8_Blitter>(dst, paint, 1);
        default: return nullptr;
    }
}

SkBlitter* SkA8Blitter_Choose(const SkPixmap& dst,
                              const SkMatrix& ctm,
                              const SkPaint& paint,
                              SkArenaAlloc* alloc,
                              SkDrawCoverage coverage,
                              sk_sp<SkShader> clipShader,
                              const SkSurfaceProps&,
                              const SkRect&) {
    return SkChooseA8Blitter(dst, ctm, paint, alloc, coverage, clipShader);
}

// ============================================================================
// SkBlitter_ARGB32.cpp
// ============================================================================

static bool blit_color_n32(const SkPixmap& device,
                           const SkMask& mask,
                           const SkIRect& clip,
                           SkColor color) {
    int x = clip.fLeft, y = clip.fTop;
    if (device.colorType() == kN32_SkColorType && mask.fFormat == SkMask::kA8_Format) {
        SkOpts::blit_mask_d32_a8(device.writable_addr32(x, y), device.rowBytes(),
                                 reinterpret_cast<const SkAlpha*>(mask.getAddr(x, y)),
                                 mask.fRowBytes, color, clip.width(), clip.height());
        return true;
    }
    if (device.colorType() == kN32_SkColorType && mask.fFormat == SkMask::kLCD16_Format) {
        skia_rs_argb32_blit_mask_lcd16(
                device.writable_addr32(x, y), device.rowBytes(),
                reinterpret_cast<const uint16_t*>(mask.getAddr(x, y)), mask.fRowBytes,
                color, clip.width(), clip.height(), kPMColorIsBGRA);
        return true;
    }
    return false;
}

SkARGB32_Blitter::SkARGB32_Blitter(const SkPixmap& device, const SkPaint& paint)
        : SkRasterBlitter(device) {
    SkColor color = paint.getColor();
    fColor = color;
    fSrcA = SkColorGetA(color);
    fPMColor = SkPreMultiplyColor(fColor);
}

void SkARGB32_Blitter::blitH(int x, int y, int width) {
    SkASSERT(x >= 0 && y >= 0 && x + width <= fDevice.width());
    SkBlitRow::Color32(fDevice.writable_addr32(x, y), width, fPMColor);
}

void SkARGB32_Blitter::blitAntiH(int x, int y, const SkAlpha antialias[],
                                 const int16_t runs[]) {
    SkASSERT(fSrcA != 0xFF);
    if (fSrcA == 0) {
        return;
    }
    skia_rs_argb32_blit_anti_h(fDevice.writable_addr32(x, y), antialias, runs, fPMColor, 0);
}

void SkARGB32_Blitter::blitAntiH2(int x, int y, U8CPU a0, U8CPU a1) {
    uint32_t* device = fDevice.writable_addr32(x, y);
    SkDEBUGCODE((void)fDevice.writable_addr32(x + 1, y);)
    skia_rs_argb32_blit_anti_h2(device, fPMColor, a0, a1, 0);
}

void SkARGB32_Blitter::blitAntiV2(int x, int y, U8CPU a0, U8CPU a1) {
    uint32_t* device = fDevice.writable_addr32(x, y);
    SkDEBUGCODE((void)fDevice.writable_addr32(x, y + 1);)
    skia_rs_argb32_blit_anti_v2(device, fDevice.rowBytes(), fPMColor, a0, a1, 0);
}

void SkARGB32_Blitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(mask.fBounds.contains(clip));
    SkASSERT(fSrcA != 0xFF);
    if (fSrcA == 0) {
        return;
    }
    if (blit_color_n32(fDevice, mask, clip, fColor)) {
        return;
    }
    switch (mask.fFormat) {
        case SkMask::kBW_Format:
            skia_rs_argb32_blit_mask_bw(
                    fDevice.writable_addr32(clip.fLeft, clip.fTop), fDevice.rowBytes(),
                    mask.getAddr1(clip.fLeft, clip.fTop), mask.fRowBytes,
                    (clip.fLeft - mask.fBounds.fLeft) & 7,
                    clip.width(), clip.height(), fPMColor, false);
            break;
        case SkMask::kARGB32_Format:
            skia_rs_argb32_blit_mask_argb32(
                    fDevice.writable_addr32(clip.fLeft, clip.fTop), fDevice.rowBytes(),
                    mask.getAddr32(clip.fLeft, clip.fTop), mask.fRowBytes,
                    clip.width(), clip.height(), fPMColor);
            break;
        default:
            SK_ABORT("Mask format not handled.");
    }
}

void SkARGB32_Opaque_Blitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(fSrcA == 0xFF);
    SkASSERT(mask.fBounds.contains(clip));
    if (blit_color_n32(fDevice, mask, clip, fColor)) {
        return;
    }
    switch (mask.fFormat) {
        case SkMask::kBW_Format:
            skia_rs_argb32_blit_mask_bw(
                    fDevice.writable_addr32(clip.fLeft, clip.fTop), fDevice.rowBytes(),
                    mask.getAddr1(clip.fLeft, clip.fTop), mask.fRowBytes,
                    (clip.fLeft - mask.fBounds.fLeft) & 7,
                    clip.width(), clip.height(), fPMColor, true);
            break;
        case SkMask::kARGB32_Format:
            skia_rs_argb32_blit_mask_argb32(
                    fDevice.writable_addr32(clip.fLeft, clip.fTop), fDevice.rowBytes(),
                    mask.getAddr32(clip.fLeft, clip.fTop), mask.fRowBytes,
                    clip.width(), clip.height(), fPMColor);
            break;
        default:
            SK_ABORT("Mask format not handled.");
    }
}

void SkARGB32_Opaque_Blitter::blitAntiH(int x, int y, const SkAlpha antialias[],
                                        const int16_t runs[]) {
    SkASSERT(fSrcA == 0xFF);
    skia_rs_argb32_blit_anti_h(fDevice.writable_addr32(x, y), antialias, runs, fPMColor, 1);
}

void SkARGB32_Opaque_Blitter::blitAntiH2(int x, int y, U8CPU a0, U8CPU a1) {
    uint32_t* device = fDevice.writable_addr32(x, y);
    SkDEBUGCODE((void)fDevice.writable_addr32(x + 1, y);)
    skia_rs_argb32_blit_anti_h2(device, fPMColor, a0, a1, 1);
}

void SkARGB32_Opaque_Blitter::blitAntiV2(int x, int y, U8CPU a0, U8CPU a1) {
    uint32_t* device = fDevice.writable_addr32(x, y);
    SkDEBUGCODE((void)fDevice.writable_addr32(x, y + 1);)
    skia_rs_argb32_blit_anti_v2(device, fDevice.rowBytes(), fPMColor, a0, a1, 1);
}

std::optional<SkBlitter::DirectBlit> SkARGB32_Opaque_Blitter::canDirectBlit() {
    return {{fDevice, fPMColor}};
}

void SkARGB32_Blitter::blitV(int x, int y, int height, SkAlpha alpha) {
    if (alpha == 0 || fSrcA == 0) {
        return;
    }
    skia_rs_argb32_blit_v(fDevice.writable_addr32(x, y), fDevice.rowBytes(),
                          height, fPMColor, alpha);
}

void SkARGB32_Blitter::blitRect(int x, int y, int width, int height) {
    SkASSERT(x >= 0 && y >= 0 && x + width <= fDevice.width() && y + height <= fDevice.height());
    if (fSrcA == 0) {
        return;
    }
    if (SkGetPackedA32(fPMColor) == 0xFF) {
        SkOpts::rect_memset32(fDevice.writable_addr32(x, y), fPMColor, width,
                              fDevice.rowBytes(), height);
    } else {
        skia_rs_argb32_blit_rect(fDevice.writable_addr32(x, y), fDevice.rowBytes(),
                                 width, height, fPMColor);
    }
}

void SkARGB32_Black_Blitter::blitAntiH(int x, int y, const SkAlpha antialias[],
                                       const int16_t runs[]) {
    skia_rs_argb32_blit_anti_h(fDevice.writable_addr32(x, y), antialias, runs, 0xFF000000u, 2);
}

void SkARGB32_Black_Blitter::blitAntiH2(int x, int y, U8CPU a0, U8CPU a1) {
    uint32_t* device = fDevice.writable_addr32(x, y);
    SkDEBUGCODE((void)fDevice.writable_addr32(x + 1, y);)
    skia_rs_argb32_blit_anti_h2(device, 0xFF000000u, a0, a1, 2);
}

void SkARGB32_Black_Blitter::blitAntiV2(int x, int y, U8CPU a0, U8CPU a1) {
    uint32_t* device = fDevice.writable_addr32(x, y);
    SkDEBUGCODE((void)fDevice.writable_addr32(x, y + 1);)
    skia_rs_argb32_blit_anti_v2(device, fDevice.rowBytes(), 0xFF000000u, a0, a1, 2);
}

SkARGB32_Shader_Blitter::SkARGB32_Shader_Blitter(const SkPixmap& device,
                                                 const SkPaint& paint,
                                                 SkShaderBase::Context* shaderContext)
        : SkShaderBlitter(device, paint, shaderContext) {
    fBuffer.reset(device.width());
    SkASSERT(paint.isSrcOver());

    int flags = 0;
    if (!(shaderContext->getFlags() & SkShaderBase::kOpaqueAlpha_Flag)) {
        flags |= SkBlitRow::kSrcPixelAlpha_Flag32;
    }
    fProc32 = SkBlitRow::Factory32(flags);
    fProc32Blend = SkBlitRow::Factory32(flags | SkBlitRow::kGlobalAlpha_Flag32);
    fShadeDirectlyIntoDevice =
            SkToBool(shaderContext->getFlags() & SkShaderBase::kOpaqueAlpha_Flag);
}

SkARGB32_Shader_Blitter::~SkARGB32_Shader_Blitter() = default;

void SkARGB32_Shader_Blitter::blitH(int x, int y, int width) {
    SkASSERT(x >= 0 && y >= 0 && x + width <= fDevice.width());
    uint32_t* device = fDevice.writable_addr32(x, y);
    if (fShadeDirectlyIntoDevice) {
        fShaderContext->shadeSpan(x, y, device, width);
    } else {
        SkPMColor* span = fBuffer;
        fShaderContext->shadeSpan(x, y, span, width);
        fProc32(device, span, width, 255);
    }
}

void SkARGB32_Shader_Blitter::blitRect(int x, int y, int width, int height) {
    SkASSERT(x >= 0 && y >= 0 && x + width <= fDevice.width() && y + height <= fDevice.height());
    uint32_t* device = fDevice.writable_addr32(x, y);
    const size_t deviceRB = fDevice.rowBytes();
    auto* shaderContext = fShaderContext;
    SkPMColor* span = fBuffer;

    if (fShadeDirectlyIntoDevice) {
        do {
            shaderContext->shadeSpan(x, y, device, width);
            y += 1;
            device = reinterpret_cast<uint32_t*>(reinterpret_cast<char*>(device) + deviceRB);
        } while (--height > 0);
    } else {
        SkBlitRow::Proc32 proc = fProc32;
        do {
            shaderContext->shadeSpan(x, y, span, width);
            proc(device, span, width, 255);
            y += 1;
            device = reinterpret_cast<uint32_t*>(reinterpret_cast<char*>(device) + deviceRB);
        } while (--height > 0);
    }
}

void SkARGB32_Shader_Blitter::blitAntiH(int x, int y, const SkAlpha antialias[],
                                        const int16_t runs[]) {
    SkPMColor* span = fBuffer;
    uint32_t* device = fDevice.writable_addr32(x, y);
    auto* shaderContext = fShaderContext;

    if (fShadeDirectlyIntoDevice || (shaderContext->getFlags() & SkShaderBase::kOpaqueAlpha_Flag)) {
        for (;;) {
            int count = *runs;
            if (count <= 0) {
                break;
            }
            int aa = *antialias;
            if (aa) {
                if (aa == 255) {
                    shaderContext->shadeSpan(x, y, device, count);
                } else {
                    shaderContext->shadeSpan(x, y, span, count);
                    fProc32Blend(device, span, count, aa);
                }
            }
            device += count;
            runs += count;
            antialias += count;
            x += count;
        }
    } else {
        for (;;) {
            int count = *runs;
            if (count <= 0) {
                break;
            }
            int aa = *antialias;
            if (aa) {
                shaderContext->shadeSpan(x, y, span, count);
                if (aa == 255) {
                    fProc32(device, span, count, 255);
                } else {
                    fProc32Blend(device, span, count, aa);
                }
            }
            device += count;
            runs += count;
            antialias += count;
            x += count;
        }
    }
}

void SkARGB32_Shader_Blitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkASSERT(mask.fBounds.contains(clip));
    bool opaque = SkToBool(fShaderContext->getFlags() & SkShaderBase::kOpaqueAlpha_Flag);

    if (mask.fFormat != SkMask::kA8_Format && mask.fFormat != SkMask::kLCD16_Format) {
        this->SkShaderBlitter::blitMask(mask, clip);
        return;
    }

    const int x = clip.fLeft;
    const int width = clip.width();
    int y = clip.fTop;
    int height = clip.height();

    char* dstRow = reinterpret_cast<char*>(fDevice.writable_addr32(x, y));
    const size_t dstRB = fDevice.rowBytes();
    const uint8_t* maskRow = reinterpret_cast<const uint8_t*>(mask.getAddr(x, y));
    const size_t maskRB = mask.fRowBytes;
    SkPMColor* span = fBuffer;

    do {
        fShaderContext->shadeSpan(x, y, span, width);
        if (mask.fFormat == SkMask::kA8_Format) {
            skia_rs_argb32_shader_blend_row_a8(
                    reinterpret_cast<SkPMColor*>(dstRow), maskRow, span, width, opaque);
        } else {
            skia_rs_argb32_shader_blend_row_lcd16(
                    reinterpret_cast<SkPMColor*>(dstRow),
                    reinterpret_cast<const uint16_t*>(maskRow),
                    span, width, opaque, kPMColorIsBGRA);
        }
        dstRow += dstRB;
        maskRow += maskRB;
        y += 1;
    } while (--height > 0);
}

void SkARGB32_Shader_Blitter::blitV(int x, int y, int height, SkAlpha alpha) {
    SkASSERT(x >= 0 && y >= 0 && y + height <= fDevice.height());
    uint32_t* device = fDevice.writable_addr32(x, y);
    const size_t deviceRB = fDevice.rowBytes();

    if (fShadeDirectlyIntoDevice) {
        if (255 == alpha) {
            do {
                fShaderContext->shadeSpan(x, y, device, 1);
                y += 1;
                device = reinterpret_cast<uint32_t*>(reinterpret_cast<char*>(device) + deviceRB);
            } while (--height > 0);
        } else {
            do {
                SkPMColor c;
                fShaderContext->shadeSpan(x, y, &c, 1);
                *device = skia_rs_argb32_shader_blit_v_interp(c, *device, alpha);
                y += 1;
                device = reinterpret_cast<uint32_t*>(reinterpret_cast<char*>(device) + deviceRB);
            } while (--height > 0);
        }
    } else {
        SkPMColor* span = fBuffer;
        SkBlitRow::Proc32 proc = (255 == alpha) ? fProc32 : fProc32Blend;
        do {
            fShaderContext->shadeSpan(x, y, span, 1);
            proc(device, span, 1, alpha);
            y += 1;
            device = reinterpret_cast<uint32_t*>(reinterpret_cast<char*>(device) + deviceRB);
        } while (--height > 0);
    }
}

// ============================================================================
// SkBlitter_Sprite.cpp & SkSpriteBlitter_ARGB32.cpp
// ============================================================================

SkSpriteBlitter::SkSpriteBlitter(const SkPixmap& source) : fSource(source) {}

bool SkSpriteBlitter::setup(const SkPixmap& dst, int left, int top, const SkPaint& paint) {
    fDst = dst;
    fLeft = left;
    fTop = top;
    fPaint = &paint;
    return true;
}

void SkSpriteBlitter::blitH(int x, int y, int width) {
    SkDEBUGFAIL("how did we get here?");
    this->blitRect(x, y, width, 1);
}

void SkSpriteBlitter::blitAntiH(int x, int y, const SkAlpha antialias[], const int16_t runs[]) {
    SkDEBUGFAIL("how did we get here?");
}

void SkSpriteBlitter::blitV(int x, int y, int height, SkAlpha alpha) {
    SkDEBUGFAIL("how did we get here?");
    INHERITED::blitV(x, y, height, alpha);
}

void SkSpriteBlitter::blitMask(const SkMask& mask, const SkIRect& clip) {
    SkDEBUGFAIL("how did we get here?");
    INHERITED::blitMask(mask, clip);
}

class SkSpriteBlitter_Memcpy final : public SkSpriteBlitter {
public:
    static bool Supports(const SkPixmap& dst, const SkPixmap& src, const SkPaint& paint) {
        SkASSERT(0 == SkColorSpaceXformSteps(src, dst).fFlags.mask());
        auto mode = paint.asBlendMode();
        return skia_rs_sprite_memcpy_supports(
                static_cast<int32_t>(dst.colorType()),
                static_cast<int32_t>(src.colorType()),
                paint.getMaskFilter() != nullptr,
                paint.getColorFilter() != nullptr,
                paint.getImageFilter() != nullptr,
                paint.getAlpha(),
                mode.has_value(),
                mode.has_value() ? static_cast<int32_t>(*mode) : 0,
                src.isOpaque());
    }

    explicit SkSpriteBlitter_Memcpy(const SkPixmap& src) : INHERITED(src) {}

    void blitRect(int x, int y, int width, int height) override {
        SkASSERT(fDst.colorType() == fSource.colorType());
        SkASSERT(width > 0 && height > 0);
        const size_t bytesToCopy = static_cast<size_t>(width) << fSource.shiftPerPixel();
        skia_rs_sprite_memcpy_blit_rect(
                static_cast<uint8_t*>(fDst.writable_addr(x, y)), fDst.rowBytes(),
                static_cast<const uint8_t*>(fSource.addr(x - fLeft, y - fTop)), fSource.rowBytes(),
                bytesToCopy, height);
    }

private:
    using INHERITED = SkSpriteBlitter;
};

class SkRasterPipelineSpriteBlitter final : public SkSpriteBlitter {
public:
    SkRasterPipelineSpriteBlitter(const SkPixmap& src, SkArenaAlloc* alloc,
                                  sk_sp<SkShader> clipShader)
            : INHERITED(src)
            , fAlloc(alloc)
            , fBlitter(nullptr)
            , fSrcPtr{nullptr, 0}
            , fClipShader(std::move(clipShader)) {}

    bool setup(const SkPixmap& dst, int left, int top, const SkPaint& paint) override {
        fDst = dst;
        fLeft = left;
        fTop = top;
        fPaintColor = paint.getColor4f();

        SkRasterPipeline p(fAlloc);
        p.appendLoad(fSource.colorType(), &fSrcPtr);

        if (SkColorTypeIsAlphaOnly(fSource.colorType())) {
            p.appendSetRGB(fAlloc, fPaintColor);
            p.append(SkRasterPipelineOp::premul);
        }
        if (auto dstCS = fDst.colorSpace()) {
            auto srcCS = fSource.colorSpace();
            if (!srcCS || SkColorTypeIsAlphaOnly(fSource.colorType())) {
                srcCS = sk_srgb_singleton();
            }
            auto srcAT = fSource.isOpaque() ? kOpaque_SkAlphaType : kPremul_SkAlphaType;
            fAlloc->make<SkColorSpaceXformSteps>(srcCS, srcAT, dstCS, kPremul_SkAlphaType)
                    ->apply(&p);
        }
        if (fPaintColor.fA != 1.0f) {
            p.append(SkRasterPipelineOp::scale_1_float, &fPaintColor.fA);
        }

        bool is_opaque = fSource.isOpaque() && fPaintColor.fA == 1.0f;
        fBlitter = SkCreateRasterPipelineBlitter(fDst, paint, p, is_opaque, fAlloc, fClipShader);
        return fBlitter != nullptr;
    }

    void blitRect(int x, int y, int width, int height) override {
        fSrcPtr.stride = fSource.rowBytesAsPixels();
        size_t bpp = fSource.info().bytesPerPixel();
        fSrcPtr.pixels = static_cast<char*>(fSource.writable_addr(-fLeft + x, -fTop + y))
                         - bpp * x - bpp * y * fSrcPtr.stride;
        fBlitter->blitRect(x, y, width, height);
    }

private:
    SkArenaAlloc*                       fAlloc;
    SkBlitter*                          fBlitter;
    SkRasterPipelineContexts::MemoryCtx fSrcPtr;
    SkColor4f                           fPaintColor;
    sk_sp<SkShader>                     fClipShader;
    using INHERITED = SkSpriteBlitter;
};

class Sprite_D32_S32 final : public SkSpriteBlitter {
public:
    Sprite_D32_S32(const SkPixmap& src, U8CPU alpha) : INHERITED(src) {
        SkASSERT(src.colorType() == kN32_SkColorType);
        uint32_t flags32 = skia_rs_sprite_d32_s32_flags(SkToU8(alpha), src.isOpaque());
        fProc32 = SkBlitRow::Factory32(flags32);
        fAlpha = alpha;
    }

    void blitRect(int x, int y, int width, int height) override {
        SkASSERT(width > 0 && height > 0);
        uint32_t* SK_RESTRICT dst = fDst.writable_addr32(x, y);
        const uint32_t* SK_RESTRICT src = fSource.addr32(x - fLeft, y - fTop);
        size_t dstRB = fDst.rowBytes();
        size_t srcRB = fSource.rowBytes();
        SkBlitRow::Proc32 proc = fProc32;
        U8CPU alpha = fAlpha;
        do {
            proc(dst, src, width, alpha);
            dst = reinterpret_cast<uint32_t* SK_RESTRICT>(
                    reinterpret_cast<char*>(dst) + dstRB);
            src = reinterpret_cast<const uint32_t* SK_RESTRICT>(
                    reinterpret_cast<const char*>(src) + srcRB);
        } while (--height != 0);
    }

private:
    SkBlitRow::Proc32 fProc32;
    U8CPU             fAlpha;
    using INHERITED = SkSpriteBlitter;
};

SkSpriteBlitter* SkSpriteBlitter::ChooseL32(const SkPixmap& source, const SkPaint& paint,
                                            SkArenaAlloc* allocator) {
    SkASSERT(allocator != nullptr);
    if (skia_rs_sprite_choose_l32_supports(
                paint.getColorFilter() != nullptr,
                paint.getMaskFilter() != nullptr,
                source.colorType() == kN32_SkColorType,
                paint.isSrcOver())) {
        return allocator->make<Sprite_D32_S32>(source, paint.getAlpha());
    }
    return nullptr;
}

SkBlitter* SkBlitter::ChooseSprite(const SkPixmap& dst, const SkPaint& paint,
                                   const SkPixmap& source, int left, int top,
                                   SkArenaAlloc* alloc, sk_sp<SkShader> clipShader) {
    SkASSERT(alloc != nullptr);

    if (source.alphaType() == kUnpremul_SkAlphaType) {
        return nullptr;
    }

    SkSpriteBlitter* blitter = nullptr;

    if (gSkForceRasterPipelineBlitter) {
        // Do not use any of these optimized memory blitters
    } else if (0 == SkColorSpaceXformSteps(source, dst).fFlags.mask() && !clipShader) {
        if (!blitter && SkSpriteBlitter_Memcpy::Supports(dst, source, paint)) {
            blitter = alloc->make<SkSpriteBlitter_Memcpy>(source);
        }
        if (!blitter) {
            switch (dst.colorType()) {
                case kN32_SkColorType:
                    blitter = SkSpriteBlitter::ChooseL32(source, paint, alloc);
                    break;
                default:
                    break;
            }
        }
    }
    if (!blitter && !paint.getMaskFilter()) {
        blitter = alloc->make<SkRasterPipelineSpriteBlitter>(source, alloc, clipShader);
    }

    if (blitter && blitter->setup(dst, left, top, paint)) {
        return blitter;
    }

    return nullptr;
}
