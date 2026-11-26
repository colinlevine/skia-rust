/*
 * Safe Rust bridge for:
 *   - src/core/SkScan_AntiPath.cpp
 *   - src/core/SkScan_AAAPath.cpp
 * (Phase 1 Layer 3.2: skia-rs-scan)
 */

#include "include/core/SkPath.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRegion.h"
#include "include/private/SkAssert.h"
#include "src/core/SkAAClip.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkMask.h"
#include "src/core/SkPathRaw.h"
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

int32_t skia_rs_scan_classify_anti_fill_path(
        const SkRect* path_bounds, const SkIRect* clip_bounds, bool is_inverse, SkIRect* out_ir);
void skia_rs_scan_aaa_fill_path(
        const SkiaRsPathRawView* raw,
        const SkiaRsBlitterVTable* blitter,
        const SkIRect* ir,
        const SkIRect* clip_bounds,
        bool force_rle);

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

}  // namespace

// ============================================================================
// SkScan_AntiPath (replaces src/core/SkScan_AntiPath.cpp)
// ============================================================================

void SkScan::AntiFillPath(const SkPathRaw& path,
                          const SkRegion& origClip,
                          SkBlitter* blitter,
                          bool forceRLE) {
    if (origClip.isEmpty()) {
        return;
    }

    const bool isInverse = path.isInverseFillType();
    SkRect pathBounds = path.bounds();
    const SkIRect& origClipBounds = origClip.getBounds();
    SkIRect ir;
    int32_t action =
            skia_rs_scan_classify_anti_fill_path(&pathBounds, &origClipBounds, isInverse, &ir);
    if (action == 0) {
        if (isInverse) {
            blitter->blitRegion(origClip);
        }
        return;
    }
    if (action == 1) {
        return;
    }
    if (action == 2) {
        SkScan::FillPath(path, origClip, blitter);
        return;
    }

    SkRegion tmpClipStorage;
    const SkRegion* clipRgn = &origClip;
    {
        static const int32_t kMaxClipCoord = 32767;
        const SkIRect& bounds = origClip.getBounds();
        if (bounds.fRight > kMaxClipCoord || bounds.fBottom > kMaxClipCoord) {
            SkIRect limit = {0, 0, kMaxClipCoord, kMaxClipCoord};
            tmpClipStorage.op(origClip, limit, SkRegion::kIntersect_Op);
            clipRgn = &tmpClipStorage;
        }
    }

    SkScanClipper clipper(blitter, clipRgn, ir);
    if (clipper.getBlitter() == nullptr) {
        if (isInverse) {
            blitter->blitRegion(*clipRgn);
        }
        return;
    }

    SkASSERT(clipper.getClipRect() == nullptr || *clipper.getClipRect() == clipRgn->getBounds());
    blitter = clipper.getBlitter();

    if (isInverse) {
        sk_blit_above(blitter, ir, *clipRgn);
    }

    SkScan::AAAFillPath(path, blitter, ir, clipRgn->getBounds(), forceRLE);

    if (isInverse) {
        sk_blit_below(blitter, ir, *clipRgn);
    }
}

void SkScan::AntiFillPath(const SkPathRaw& raw, const SkRasterClip& clip, SkBlitter* blitter) {
    SkASSERT(raw.bounds().isFinite());
    if (clip.isEmpty()) {
        return;
    }
    if (clip.isBW()) {
        AntiFillPath(raw, clip.bwRgn(), blitter, false);
    } else {
        SkRegion tmp;
        SkAAClipBlitter aaBlitter;
        tmp.setRect(clip.getBounds());
        aaBlitter.init(blitter, &clip.aaRgn());
        AntiFillPath(raw, tmp, &aaBlitter, true);
    }
}

// ============================================================================
// SkScan_AAAPath (replaces src/core/SkScan_AAAPath.cpp)
// ============================================================================

void SkScan::AAAFillPath(const SkPathRaw& path,
                         SkBlitter* blitter,
                         const SkIRect& ir,
                         const SkIRect& clipBounds,
                         bool forceRLE) {
    SkiaRsPathRawView view = to_path_raw_view(path);
    SkiaRsBlitterVTable bvt = make_blitter_vtable(blitter);
    skia_rs_scan_aaa_fill_path(&view, &bvt, &ir, &clipBounds, forceRLE);
}
