/*
 * Safe Rust bridge for:
 *   - src/core/SkScan.cpp
 *   - src/core/SkScan_Path.cpp
 * (Phase 1 Layer 3.2: skia-rs-scan)
 */

#include "include/core/SkPath.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/private/SkAssert.h"
#include "include/private/SkFixed.h"
#include "include/private/SkMath.h"
#include "src/core/SkAAClip.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkMask.h"
#include "src/core/SkPathRaw.h"
#include "src/core/SkPathRawShapes.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkScan.h"
#include "src/core/SkScanPriv.h"

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

void skia_rs_scan_fill_irect(
        const SkIRect* r, const SkiaRsRegionClipVTable* clip, const SkiaRsBlitterVTable* blitter);
void skia_rs_scan_fill_xrect(
        const SkXRect* xr, const SkiaRsRegionClipVTable* clip, const SkiaRsBlitterVTable* blitter);
void skia_rs_scan_fill_rect(
        const SkRect* r, const SkiaRsRegionClipVTable* clip, const SkiaRsBlitterVTable* blitter);
bool skia_rs_scan_path_requires_tiling(const SkIRect* bounds);
void skia_rs_scan_clip_limit_rect(SkIRect* out);
void skia_rs_scan_compute_fill_path_bounds(
        const SkRect* raw_bounds, SkIRect* out_ir, bool* out_pre_clipped);
void skia_rs_scan_conservative_round_to_int(const SkRect* src, SkIRect* out_ir);
void skia_rs_scan_fill_path_clipped(
        const SkiaRsPathRawView* raw,
        const SkIRect* clip_rect,
        const SkiaRsBlitterVTable* blitter,
        int32_t start_y,
        int32_t stop_y,
        bool path_contained_in_clip);
void skia_rs_scan_fill_triangle_clipped(
        const SkPoint* pts,
        const SkIRect* clip_rect,
        const SkiaRsBlitterVTable* blitter,
        const SkIRect* ir);

}  // extern "C"

namespace {

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
        +[](void* ctx, int32_t x, int32_t stopx, int32_t fy, int32_t dy) {
            auto* b = static_cast<SkBlitter*>(ctx);
            do {
                b->blitH(x, fy >> 16, 1);
                fy += dy;
            } while (++x < stopx);
        },
        +[](void* ctx, int32_t y, int32_t stopy, int32_t fx, int32_t dx) {
            auto* b = static_cast<SkBlitter*>(ctx);
            do {
                b->blitH(fx >> 16, y, 1);
                fx += dx;
            } while (++y < stopy);
        },
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

bool clip_to_limit(const SkRegion& orig, SkRegion* reduced) {
    SkIRect limitR;
    skia_rs_scan_clip_limit_rect(&limitR);
    if (limitR.contains(orig.getBounds())) {
        return false;
    }
    reduced->op(orig, limitR, SkRegion::kIntersect_Op);
    return true;
}

}  // namespace

// ============================================================================
// SkScan (replaces src/core/SkScan.cpp)
// ============================================================================

void SkScan::FillIRect(const SkIRect& r, const SkRegion* clip, SkBlitter* blitter) {
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_fill_irect(&r, &rvt, &bvt);
    } else {
        skia_rs_scan_fill_irect(&r, nullptr, &bvt);
    }
}

void SkScan::FillXRect(const SkXRect& xr, const SkRegion* clip, SkBlitter* blitter) {
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_fill_xrect(&xr, &rvt, &bvt);
    } else {
        skia_rs_scan_fill_xrect(&xr, nullptr, &bvt);
    }
}

void SkScan::FillRect(const SkRect& r, const SkRegion* clip, SkBlitter* blitter) {
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    if (clip) {
        SkiaRsRegionClipVTable rvt = make_region_vtable(clip);
        skia_rs_scan_fill_rect(&r, &rvt, &bvt);
    } else {
        skia_rs_scan_fill_rect(&r, nullptr, &bvt);
    }
}

void SkScan::FillIRect(const SkIRect& r, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isEmpty() || r.isEmpty()) {
        return;
    }
    if (clip.isBW()) {
        FillIRect(r, &clip.bwRgn(), blitter);
        return;
    }
    SkAAClipBlitterWrapper wrapper(clip, blitter);
    FillIRect(r, &wrapper.getRgn(), wrapper.getBlitter());
}

void SkScan::FillXRect(const SkXRect& xr, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isEmpty() || xr.isEmpty()) {
        return;
    }
    if (clip.isBW()) {
        FillXRect(xr, &clip.bwRgn(), blitter);
        return;
    }
    SkAAClipBlitterWrapper wrapper(clip, blitter);
    FillXRect(xr, &wrapper.getRgn(), wrapper.getBlitter());
}

void SkScan::FillRect(const SkRect& r, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isEmpty() || r.isEmpty()) {
        return;
    }
    if (clip.isBW()) {
        FillRect(r, &clip.bwRgn(), blitter);
        return;
    }
    SkAAClipBlitterWrapper wrapper(clip, blitter);
    FillRect(r, &wrapper.getRgn(), wrapper.getBlitter());
}

// ============================================================================
// SkScan_Path (replaces src/core/SkScan_Path.cpp)
// ============================================================================

void sk_blit_above(SkBlitter* blitter, const SkIRect& ir, const SkRegion& clip) {
    const SkIRect& cr = clip.getBounds();
    SkIRect tmp;
    tmp.fLeft = cr.fLeft;
    tmp.fRight = cr.fRight;
    tmp.fTop = cr.fTop;
    tmp.fBottom = ir.fTop;
    if (!tmp.isEmpty()) {
        blitter->blitRectRegion(tmp, clip);
    }
}

void sk_blit_below(SkBlitter* blitter, const SkIRect& ir, const SkRegion& clip) {
    const SkIRect& cr = clip.getBounds();
    SkIRect tmp;
    tmp.fLeft = cr.fLeft;
    tmp.fRight = cr.fRight;
    tmp.fTop = ir.fBottom;
    tmp.fBottom = cr.fBottom;
    if (!tmp.isEmpty()) {
        blitter->blitRectRegion(tmp, clip);
    }
}

SkScanClipper::SkScanClipper(SkBlitter* blitter,
                             const SkRegion* clip,
                             const SkIRect& ir,
                             bool skipRejectTest,
                             bool irPreClipped) {
    fBlitter = nullptr;
    fClipRect = nullptr;

    if (clip) {
        fClipRect = &clip->getBounds();
        if (!skipRejectTest && !SkIRect::Intersects(*fClipRect, ir)) {
            return;
        }

        if (clip->isRect()) {
            if (!irPreClipped && fClipRect->contains(ir)) {
#ifdef SK_DEBUG
                fRectClipCheckBlitter.init(blitter, *fClipRect);
                blitter = &fRectClipCheckBlitter;
#endif
                fClipRect = nullptr;
            } else {
                if (irPreClipped ||
                    fClipRect->fLeft > ir.fLeft || fClipRect->fRight < ir.fRight) {
                    fRectBlitter.init(blitter, *fClipRect);
                    blitter = &fRectBlitter;
                } else {
#ifdef SK_DEBUG
                    fRectClipCheckBlitter.init(blitter, *fClipRect);
                    blitter = &fRectClipCheckBlitter;
#endif
                }
            }
        } else {
            fRgnBlitter.init(blitter, clip);
            blitter = &fRgnBlitter;
        }
    }
    fBlitter = blitter;
}

void SkScan::FillPath(const SkPathRaw& raw, const SkRegion& origClip, SkBlitter* blitter) {
    if (origClip.isEmpty()) {
        return;
    }

    const SkRegion* clipPtr = &origClip;
    SkRegion finiteClip;
    if (clip_to_limit(origClip, &finiteClip)) {
        if (finiteClip.isEmpty()) {
            return;
        }
        clipPtr = &finiteClip;
    }

    SkRect rawBounds = raw.bounds();
    SkIRect ir;
    bool irPreClipped = false;
    skia_rs_scan_compute_fill_path_bounds(&rawBounds, &ir, &irPreClipped);
    if (ir.isEmpty()) {
        if (raw.isInverseFillType()) {
            blitter->blitRegion(*clipPtr);
        }
        return;
    }

    SkScanClipper clipper(blitter, clipPtr, ir, raw.isInverseFillType(), irPreClipped);
    blitter = clipper.getBlitter();
    if (blitter) {
        if (raw.isInverseFillType()) {
            sk_blit_above(blitter, ir, *clipPtr);
        }
        SkASSERT(clipper.getClipRect() == nullptr ||
                 *clipper.getClipRect() == clipPtr->getBounds());
        SkiaRsPathRawView view = to_path_raw_view(raw);
        SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
        const SkIRect& clipBounds = clipPtr->getBounds();
        skia_rs_scan_fill_path_clipped(
                &view, &clipBounds, &bvt, ir.fTop, ir.fBottom, clipper.getClipRect() == nullptr);
        if (raw.isInverseFillType()) {
            sk_blit_below(blitter, ir, *clipPtr);
        }
    }
}

bool SkScan::PathRequiresTiling(const SkIRect& bounds) {
    return skia_rs_scan_path_requires_tiling(&bounds);
}

void SkScan::FillTriangle(const SkPoint pts[], const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isEmpty()) {
        return;
    }

    const auto r = SkRect::Bounds({pts, 3});
    if (!r) {
        return;
    }

    const SkScalar limit = SK_MaxS16 >> 1;
    if (!SkRect::MakeLTRB(-limit, -limit, limit, limit).contains(*r)) {
        SkPathRawShapes::Triangle tri({pts, 3}, *r);
        FillPath(tri, clip, blitter);
        return;
    }

    SkIRect ir;
    skia_rs_scan_conservative_round_to_int(&*r, &ir);
    if (ir.isEmpty() || !SkIRect::Intersects(ir, clip.getBounds())) {
        return;
    }

    SkAAClipBlitterWrapper wrap;
    const SkRegion* clipRgn;
    if (clip.isBW()) {
        clipRgn = &clip.bwRgn();
    } else {
        wrap.init(clip, blitter);
        clipRgn = &wrap.getRgn();
        blitter = wrap.getBlitter();
    }

    SkScanClipper clipper(blitter, clipRgn, ir);
    blitter = clipper.getBlitter();
    if (blitter) {
        SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
        skia_rs_scan_fill_triangle_clipped(pts, clipper.getClipRect(), &bvt, &ir);
    }
}

void SkScan::FillPath(const SkPathRaw& raw, const SkRasterClip& clip, SkBlitter* blitter) {
    if (clip.isEmpty()) {
        return;
    }
    if (clip.isBW()) {
        SkScan::FillPath(raw, clip.bwRgn(), blitter);
    } else {
        SkRegion tmp;
        SkAAClipBlitter aaBlitter;
        tmp.setRect(clip.getBounds());
        aaBlitter.init(blitter, &clip.aaRgn());
        SkScan::FillPath(raw, tmp, &aaBlitter);
    }
}
