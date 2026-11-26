/*
 * Safe Rust bridge for:
 *   - src/core/SkScan_Hairline.cpp
 *   - src/core/SkScan_Antihair.cpp
 * (Phase 1 Layer 3.2: skia-rs-scan)
 */

#include "include/core/SkImageInfo.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/private/SkAssert.h"
#include "include/private/SkFixed.h"
#include "src/core/SkAAClip.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkMask.h"
#include "src/core/SkPathRaw.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkScan.h"

#include <cstddef>
#include <cstdint>

extern "C" {

struct SkiaRsPathRawView {
    const SkPoint* points;
    size_t point_count;
    const uint8_t* verbs;
    size_t verb_count;
    const float* conics;
    size_t conic_count;
    SkRect bounds;
    uint8_t fill_type;
    uint8_t convexity;
    uint8_t segment_mask;
    uint8_t _pad[5];
};
static_assert(sizeof(SkiaRsPathRawView) == 6 * sizeof(void*) + 24);

struct SkiaRsBlitterVTable {
    void* ctx;
    void (*blit_h)(void*, int32_t, int32_t, int32_t);
    void (*blit_anti_h)(void*, int32_t, int32_t, uint8_t*, int16_t*);
    void (*blit_v)(void*, int32_t, int32_t, int32_t, uint8_t);
    void (*blit_rect)(void*, int32_t, int32_t, int32_t, int32_t);
    void (*blit_anti_rect)(void*, int32_t, int32_t, int32_t, int32_t, uint8_t, uint8_t);
    void (*blit_anti_h2)(void*, int32_t, int32_t, uint8_t, uint8_t);
    void (*blit_anti_v2)(void*, int32_t, int32_t, uint8_t, uint8_t);
    void (*blit_mask_a8)(void*, const uint8_t*, const SkIRect*, uint32_t, const SkIRect*);
    void (*blit_fat_anti_rect)(void*, const SkRect*);
    int32_t (*request_rows_preserved)(void*);
    void (*blit_hori_line)(void*, int32_t, int32_t, int32_t, int32_t);
    void (*blit_vert_line)(void*, int32_t, int32_t, int32_t, int32_t);
};
static_assert(sizeof(SkiaRsBlitterVTable) == 13 * sizeof(void*));

struct SkiaRsRegionClipVTable {
    const void* ctx;
    SkIRect bounds;
    bool is_empty;
    bool is_rect;
    uint8_t _pad[6];
    bool (*quick_contains)(const void*, const SkIRect*);
    void (*for_each_clip_rect)(
            const void*, const SkIRect*, void (*)(void*, const SkIRect*), void*);
};
static_assert(sizeof(SkiaRsRegionClipVTable) == 3 * sizeof(void*) + 24);

struct SkiaRsAntiFrameRectParams {
    SkIRect outer_rect;
    int32_t outer_l;
    int32_t outer_t;
    int32_t outer_r;
    int32_t outer_b;
    float rx;
    float ry;
};
static_assert(sizeof(SkiaRsAntiFrameRectParams) == 40);

void skia_rs_scan_hair_line_rgn(
        const SkPoint* pts,
        size_t count,
        const SkiaRsRegionClipVTable* clip,
        const SkiaRsBlitterVTable* orig_blitter,
        const SkiaRsBlitterVTable* clipped_blitter);
bool skia_rs_scan_hair_rect_bounds(
        const SkRect* rect, const SkIRect* clip_bounds, SkIRect* out_r);
void skia_rs_scan_hair_rect_emit(const SkIRect* r, const SkiaRsBlitterVTable* blitter);
void skia_rs_scan_hair_path_bounds(
        const SkRect* raw_bounds, uint8_t cap_style, SkIRect* out_ir);
void skia_rs_scan_hair_path_clipped(
        const SkiaRsPathRawView* raw,
        const SkiaRsRegionClipVTable* clip,
        bool clip_is_rect,
        const SkiaRsBlitterVTable* orig_blitter,
        const SkiaRsBlitterVTable* clipped_blitter,
        uint8_t cap_style,
        bool is_anti);
void skia_rs_scan_frame_rect(
        const SkRect* r,
        const SkPoint* stroke_size,
        const SkiaRsRegionClipVTable* clip,
        const SkiaRsBlitterVTable* blitter);

void skia_rs_scan_anti_hair_line_rgn(
        const SkPoint* pts,
        size_t count,
        const SkiaRsRegionClipVTable* clip,
        const SkiaRsBlitterVTable* blitter);
void skia_rs_scan_anti_fill_xrect(
        const SkXRect* xr, const SkiaRsRegionClipVTable* clip, const SkiaRsBlitterVTable* blitter);
void skia_rs_scan_anti_fill_rect(
        const SkRect* r, const SkiaRsRegionClipVTable* clip, const SkiaRsBlitterVTable* blitter);
void skia_rs_scan_anti_frame_rect_setup(
        const SkRect* r, const SkPoint* stroke_size, SkiaRsAntiFrameRectParams* out_params);
void skia_rs_scan_anti_frame_rect_draw(
        const SkRect* r,
        const SkPoint* stroke_size,
        const SkiaRsAntiFrameRectParams* params,
        const SkiaRsBlitterVTable* blitter);

}  // extern "C"

namespace {

void direct_or_blit_hori_line(void* ctx, int32_t x, int32_t stopx, int32_t fy, int32_t dy) {
    auto* blitter = static_cast<SkBlitter*>(ctx);
    if (auto direct = blitter->canDirectBlit()) {
        const auto pm = direct->pm;
        const auto value = direct->value;
        switch (pm.info().bytesPerPixel()) {
            case 1:
                do {
                    *pm.writable_addr8(x, fy >> 16) = value;
                    fy += dy;
                } while (++x < stopx);
                break;
            case 2:
                do {
                    *pm.writable_addr16(x, fy >> 16) = value;
                    fy += dy;
                } while (++x < stopx);
                break;
            case 4:
                do {
                    *pm.writable_addr32(x, fy >> 16) = value;
                    fy += dy;
                } while (++x < stopx);
                break;
            case 8:
                do {
                    *pm.writable_addr64(x, fy >> 16) = value;
                    fy += dy;
                } while (++x < stopx);
                break;
        }
    } else {
        do {
            blitter->blitH(x, fy >> 16, 1);
            fy += dy;
        } while (++x < stopx);
    }
}

void direct_or_blit_vert_line(void* ctx, int32_t y, int32_t stopy, int32_t fx, int32_t dx) {
    auto* blitter = static_cast<SkBlitter*>(ctx);
    if (auto direct = blitter->canDirectBlit()) {
        const auto pm = direct->pm;
        const auto value = direct->value;
        switch (pm.info().bytesPerPixel()) {
            case 1:
                do {
                    *pm.writable_addr8(fx >> 16, y) = value;
                    fx += dx;
                } while (++y < stopy);
                break;
            case 2:
                do {
                    *pm.writable_addr16(fx >> 16, y) = value;
                    fx += dx;
                } while (++y < stopy);
                break;
            case 4:
                do {
                    *pm.writable_addr32(fx >> 16, y) = value;
                    fx += dx;
                } while (++y < stopy);
                break;
            case 8:
                do {
                    *pm.writable_addr64(fx >> 16, y) = value;
                    fx += dx;
                } while (++y < stopy);
                break;
        }
    } else {
        do {
            blitter->blitH(fx >> 16, y, 1);
            fx += dx;
        } while (++y < stopy);
    }
}

SkiaRsBlitterVTable make_blitter_vtable(SkBlitter* blitter) {
    return {
        blitter,
        +[](void* ctx, int32_t x, int32_t y, int32_t w) {
            static_cast<SkBlitter*>(ctx)->blitH(x, y, w);
        },
        +[](void* ctx, int32_t x, int32_t y, uint8_t* aa, int16_t* runs) {
            static_cast<SkBlitter*>(ctx)->blitAntiH(x, y, aa, runs);
        },
        +[](void* ctx, int32_t x, int32_t y, int32_t h, uint8_t a) {
            static_cast<SkBlitter*>(ctx)->blitV(x, y, h, a);
        },
        +[](void* ctx, int32_t x, int32_t y, int32_t w, int32_t h) {
            static_cast<SkBlitter*>(ctx)->blitRect(x, y, w, h);
        },
        +[](void* ctx, int32_t x, int32_t y, int32_t w, int32_t h, uint8_t la, uint8_t ra) {
            static_cast<SkBlitter*>(ctx)->blitAntiRect(x, y, w, h, la, ra);
        },
        +[](void* ctx, int32_t x, int32_t y, uint8_t a0, uint8_t a1) {
            static_cast<SkBlitter*>(ctx)->blitAntiH2(x, y, a0, a1);
        },
        +[](void* ctx, int32_t x, int32_t y, uint8_t a0, uint8_t a1) {
            static_cast<SkBlitter*>(ctx)->blitAntiV2(x, y, a0, a1);
        },
        +[](void* ctx,
            const uint8_t* img,
            const SkIRect* bounds,
            uint32_t rb,
            const SkIRect* clip) {
            SkMask mask(img, *bounds, rb, SkMask::kA8_Format);
            static_cast<SkBlitter*>(ctx)->blitMask(mask, *clip);
        },
        +[](void* ctx, const SkRect* r) {
            static_cast<SkBlitter*>(ctx)->blitFatAntiRect(*r);
        },
        +[](void* ctx) -> int32_t {
            return static_cast<SkBlitter*>(ctx)->requestRowsPreserved();
        },
        direct_or_blit_hori_line,
        direct_or_blit_vert_line,
    };
}

SkiaRsRegionClipVTable make_region_vtable(const SkRegion* clip) {
    return {
        clip,
        clip->getBounds(),
        clip->isEmpty(),
        clip->isRect(),
        {0, 0, 0, 0, 0, 0},
        +[](const void* ctx, const SkIRect* r) -> bool {
            return static_cast<const SkRegion*>(ctx)->quickContains(*r);
        },
        +[](const void* ctx,
            const SkIRect* query,
            void (*cb)(void*, const SkIRect*),
            void* cb_ctx) {
            SkRegion::Cliperator cliper(*static_cast<const SkRegion*>(ctx), *query);
            while (!cliper.done()) {
                const SkIRect& rr = cliper.rect();
                cb(cb_ctx, &rr);
                cliper.next();
            }
        },
    };
}

SkiaRsPathRawView to_path_raw_view(const SkPathRaw& raw) {
    SkiaRsPathRawView v{};
    v.points = raw.fPoints.data();
    v.point_count = raw.fPoints.size();
    v.verbs = reinterpret_cast<const uint8_t*>(raw.fVerbs.data());
    v.verb_count = raw.fVerbs.size();
    v.conics = raw.fConics.data();
    v.conic_count = raw.fConics.size();
    v.bounds = raw.fBounds;
    v.fill_type = static_cast<uint8_t>(raw.fFillType);
    v.convexity = static_cast<uint8_t>(raw.fConvexity);
    v.segment_mask = raw.fSegmentMask;
    return v;
}

void hair_path_dispatch(const SkPathRaw& raw,
                        const SkRasterClip& rclip,
                        SkBlitter* blitter,
                        SkPaint::Cap capStyle,
                        bool isAnti) {
    if (raw.empty()) {
        return;
    }

    SkRect rawBounds = raw.bounds();
    SkIRect ibounds;
    uint8_t cap = static_cast<uint8_t>(capStyle);
    skia_rs_scan_hair_path_bounds(&rawBounds, cap, &ibounds);
    if (rclip.quickReject(ibounds)) {
        return;
    }

    SkAAClipBlitterWrapper wrap;
    SkBlitterClipper clipper;
    const SkRegion* clip = nullptr;
    SkBlitter* clippedBlitter = nullptr;

    if (!rclip.quickContains(ibounds)) {
        if (rclip.isBW()) {
            clip = &rclip.bwRgn();
        } else {
            wrap.init(rclip, blitter);
            blitter = wrap.getBlitter();
            clip = &wrap.getRgn();
        }
        if (!isAnti && clip && !clip->isEmpty()) {
            clippedBlitter = clipper.apply(blitter, clip);
        }
    }

    SkiaRsPathRawView view = to_path_raw_view(raw);
    SkiaRsBlitterVTable origVt = make_blitter_vtable(blitter);
    SkiaRsBlitterVTable clippedVt =
            clippedBlitter ? make_blitter_vtable(clippedBlitter) : origVt;
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_hair_path_clipped(
                &view,
                &rvt,
                rclip.isRect(),
                &origVt,
                clippedBlitter ? &clippedVt : nullptr,
                cap,
                isAnti);
    } else {
        skia_rs_scan_hair_path_clipped(
                &view, nullptr, rclip.isRect(), &origVt, nullptr, cap, isAnti);
    }
}

}  // namespace

// ============================================================================
// SkScan_Hairline (replaces src/core/SkScan_Hairline.cpp)
// ============================================================================

void SkScan::HairLineRgn(SkSpan<const SkPoint> src, const SkRegion* clip, SkBlitter* origBlitter) {
    if (src.size() < 2) {
        return;
    }
    SkBlitterClipper clipper;
    SkiaRsBlitterVTable origVt = make_blitter_vtable(origBlitter);
    if (clip) {
        SkBlitter* clipped = clipper.apply(origBlitter, clip);
        SkiaRsBlitterVTable clippedVt = make_blitter_vtable(clipped);
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_hair_line_rgn(src.data(), src.size(), &rvt, &origVt, &clippedVt);
    } else {
        skia_rs_scan_hair_line_rgn(src.data(), src.size(), nullptr, &origVt, nullptr);
    }
}

void SkScan::HairRect(const SkRect& rect, const SkRasterClip& clip, SkBlitter* blitter) {
    SkAAClipBlitterWrapper wrapper;
    SkBlitterClipper clipper;
    SkIRect r;
    const SkIRect& cb = clip.getBounds();
    if (!skia_rs_scan_hair_rect_bounds(&rect, &cb, &r)) {
        return;
    }
    if (clip.quickReject(r)) {
        return;
    }
    if (!clip.quickContains(r)) {
        const SkRegion* clipRgn;
        if (clip.isBW()) {
            clipRgn = &clip.bwRgn();
        } else {
            wrapper.init(clip, blitter);
            clipRgn = &wrapper.getRgn();
            blitter = wrapper.getBlitter();
        }
        blitter = clipper.apply(blitter, clipRgn);
    }
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    skia_rs_scan_hair_rect_emit(&r, &bvt);
}

void SkScan::HairPath(const SkPathRaw& raw, const SkRasterClip& clip, SkBlitter* blitter) {
    hair_path_dispatch(raw, clip, blitter, SkPaint::kButt_Cap, false);
}

void SkScan::AntiHairPath(const SkPathRaw& raw, const SkRasterClip& clip, SkBlitter* blitter) {
    hair_path_dispatch(raw, clip, blitter, SkPaint::kButt_Cap, true);
}

void SkScan::HairSquarePath(const SkPathRaw& raw, const SkRasterClip& clip, SkBlitter* blitter) {
    hair_path_dispatch(raw, clip, blitter, SkPaint::kSquare_Cap, false);
}

void SkScan::AntiHairSquarePath(const SkPathRaw& raw,
                                const SkRasterClip& clip,
                                SkBlitter* blitter) {
    hair_path_dispatch(raw, clip, blitter, SkPaint::kSquare_Cap, true);
}

void SkScan::HairRoundPath(const SkPathRaw& raw, const SkRasterClip& clip, SkBlitter* blitter) {
    hair_path_dispatch(raw, clip, blitter, SkPaint::kRound_Cap, false);
}

void SkScan::AntiHairRoundPath(const SkPathRaw& raw,
                               const SkRasterClip& clip,
                               SkBlitter* blitter) {
    hair_path_dispatch(raw, clip, blitter, SkPaint::kRound_Cap, true);
}

void SkScan::FrameRect(const SkRect& r,
                       const SkPoint& strokeSize,
                       const SkRasterClip& clip,
                       SkBlitter* blitter) {
    SkASSERT(strokeSize.fX >= 0 && strokeSize.fY >= 0);
    if (strokeSize.fX < 0 || strokeSize.fY < 0 || clip.isEmpty()) {
        return;
    }
    if (clip.isBW()) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(&clip.bwRgn());
        SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
        skia_rs_scan_frame_rect(&r, &strokeSize, &rvt, &bvt);
    } else {
        SkAAClipBlitterWrapper wrapper(clip, blitter);
        SkiaRsRegionClipVTable rvt = make_region_vtable(&wrapper.getRgn());
        SkiaRsBlitterVTable bvt = make_blitter_vtable(wrapper.getBlitter());
        skia_rs_scan_frame_rect(&r, &strokeSize, &rvt, &bvt);
    }
}

void SkScan::HairLine(SkSpan<const SkPoint> pts, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isBW()) {
        HairLineRgn(pts, &clip.bwRgn(), blitter);
    } else {
        const SkRegion* clipRgn = nullptr;
        const auto r = SkRect::BoundsOrEmpty(pts).makeOutset(SK_ScalarHalf, SK_ScalarHalf);
        SkAAClipBlitterWrapper wrap;
        if (!clip.quickContains(r.roundOut())) {
            wrap.init(clip, blitter);
            blitter = wrap.getBlitter();
            clipRgn = &wrap.getRgn();
        }
        HairLineRgn(pts, clipRgn, blitter);
    }
}

void SkScan::AntiHairLine(SkSpan<const SkPoint> pts,
                          const SkRasterClip& clip,
                          SkBlitter* blitter) {
    if (clip.isBW()) {
        AntiHairLineRgn(pts, &clip.bwRgn(), blitter);
    } else {
        const SkRegion* clipRgn = nullptr;
        const auto r = SkRect::BoundsOrEmpty(pts);
        SkAAClipBlitterWrapper wrap;
        if (!clip.quickContains(r.roundOut().makeOutset(1, 1))) {
            wrap.init(clip, blitter);
            blitter = wrap.getBlitter();
            clipRgn = &wrap.getRgn();
        }
        AntiHairLineRgn(pts, clipRgn, blitter);
    }
}

// ============================================================================
// SkScan_Antihair (replaces src/core/SkScan_Antihair.cpp)
// ============================================================================

void SkScan::AntiHairLineRgn(SkSpan<const SkPoint> src, const SkRegion* clip, SkBlitter* blitter) {
    if (src.size() < 2 || (clip && clip->isEmpty())) {
        return;
    }
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_anti_hair_line_rgn(src.data(), src.size(), &rvt, &bvt);
    } else {
        skia_rs_scan_anti_hair_line_rgn(src.data(), src.size(), nullptr, &bvt);
    }
}

void SkScan::AntiHairRect(const SkRect& rect, const SkRasterClip& clip, SkBlitter* blitter) {
    SkPoint pts[5];
    pts[0].set(rect.fLeft, rect.fTop);
    pts[1].set(rect.fRight, rect.fTop);
    pts[2].set(rect.fRight, rect.fBottom);
    pts[3].set(rect.fLeft, rect.fBottom);
    pts[4] = pts[0];
    SkScan::AntiHairLine(pts, clip, blitter);
}

void SkScan::AntiFillXRect(const SkXRect& xr, const SkRegion* clip, SkBlitter* blitter) {
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_anti_fill_xrect(&xr, &rvt, &bvt);
    } else {
        skia_rs_scan_anti_fill_xrect(&xr, nullptr, &bvt);
    }
}

void SkScan::AntiFillXRect(const SkXRect& xr, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isBW()) {
        AntiFillXRect(xr, &clip.bwRgn(), blitter);
    } else {
        SkIRect outerBounds;
        XRect_roundOut(xr, &outerBounds);
        if (clip.quickContains(outerBounds)) {
            AntiFillXRect(xr, nullptr, blitter);
        } else {
            SkAAClipBlitterWrapper wrapper(clip, blitter);
            AntiFillXRect(xr, &wrapper.getRgn(), wrapper.getBlitter());
        }
    }
}

void SkScan::AntiFillRect(const SkRect& origR, const SkRegion* clip, SkBlitter* blitter) {
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_anti_fill_rect(&origR, &rvt, &bvt);
    } else {
        skia_rs_scan_anti_fill_rect(&origR, nullptr, &bvt);
    }
}

void SkScan::AntiFillRect(const SkRect& r, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isBW()) {
        AntiFillRect(r, &clip.bwRgn(), blitter);
    } else {
        SkAAClipBlitterWrapper wrap(clip, blitter);
        AntiFillRect(r, &wrap.getRgn(), wrap.getBlitter());
    }
}

void SkScan::AntiFrameRect(const SkRect& r,
                           const SkPoint& strokeSize,
                           const SkRegion* clip,
                           SkBlitter* blitter) {
    SkASSERT(strokeSize.fX >= 0 && strokeSize.fY >= 0);

    SkiaRsAntiFrameRectParams params{};
    skia_rs_scan_anti_frame_rect_setup(&r, &strokeSize, &params);

    SkBlitterClipper clipper;
    if (clip) {
        if (clip->quickReject(params.outer_rect)) {
            return;
        }
        if (!clip->contains(params.outer_rect)) {
            blitter = clipper.apply(blitter, clip, &params.outer_rect);
        }
    }

    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    skia_rs_scan_anti_frame_rect_draw(&r, &strokeSize, &params, &bvt);
}

void SkScan::AntiFrameRect(const SkRect& r,
                           const SkPoint& strokeSize,
                           const SkRasterClip& clip,
                           SkBlitter* blitter) {
    if (clip.isBW()) {
        AntiFrameRect(r, strokeSize, &clip.bwRgn(), blitter);
    } else {
        SkAAClipBlitterWrapper wrap(clip, blitter);
        AntiFrameRect(r, strokeSize, &wrap.getRgn(), wrap.getBlitter());
    }
}
