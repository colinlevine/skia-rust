/*
 * Safe Rust bridge for:
 *   src/core/SkDraw.cpp
 *   src/core/SkDraw_atlas.cpp
 *   src/core/SkDraw_text.cpp
 *   src/core/SkDraw_vertices.cpp
 * (Phase 1 Layer 5.3 skia-rs-canvas, GN target :core).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPathUtils.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStrokeRec.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/core/SkVertices.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFixed.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkAAClip.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkAutoBlitterChoose.h"
#include "src/core/SkBlendModePriv.h"
#include "src/core/SkBlenderBase.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkBlitter_A8.h"
#include "src/core/SkColorData.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkColorSpaceXformSteps.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkCoreBlitters.h"
#include "src/core/SkDevice.h"
#include "src/core/SkDraw.h"
#include "src/core/SkDrawProcs.h"
#include "src/core/SkDrawTypes.h"
#include "src/core/SkEffectPriv.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkGlyphRunPainter.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkMatrixUtils.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkPathData.h"
#include "src/core/SkPathEffectBase.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkPathRawShapes.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkRectPriv.h"
#include "src/core/SkScan.h"
#include "src/core/SkSurfacePriv.h"
#include "src/core/SkTLazy.h"
#include "src/core/SkVertState.h"
#include "src/core/SkVerticesPriv.h"
#include "src/core/SkZip.h"
#include "src/image/SkImage_Raster.h"
#include "src/shaders/SkImageShader.h"
#include "src/shaders/SkShaderBase.h"
#include "src/shaders/SkTransformShader.h"
#include "src/shaders/SkTriColorShader.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <utility>

class SkCanvas;
class SkResourceCache;
namespace sktext {
class GlyphRunList;
}

using namespace skia_private;

namespace {

struct SkiaRsDrawRectPlan {
    int32_t action;
    int32_t rect_type;
    SkRect dev_rect;
    SkPoint stroke_size;
    SkIRect bbox_ir;
};

struct SkiaRsPtProcInitResult {
    bool valid;
    float radius;
    SkRect clip_bounds;
};

struct SkiaRsHairlinePaintPlan {
    bool modify;
    bool set_alpha;
    uint8_t new_alpha;
};

struct SkiaRsTransformedMaskGeometry {
    bool valid;
    SkIRect bounds;
    uint32_t row_bytes;
};

struct SkiaRsAtlasQuadMatrix {
    bool invertible;
    SkMatrix mx;
    SkMatrix inv;
};

struct SkiaRsUniformColorRgba {
    uint16_t rgba_u16[4];
    float r;
    float g;
    float b;
    float a;
};

struct SkiaRsClippedTriangle2D {
    int32_t count;
    SkPoint pts[4];
};

struct SkiaRsVerticesBlendPlan {
    bool drop_colors;
    bool blender_is_dst;
    bool drop_shader_and_tex;
};

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkPoint3) == 12);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkRSXform) == 16);
static_assert(sizeof(SkXRect) == 16);
static_assert(sizeof(SkiaRsDrawRectPlan) == 48);
static_assert(sizeof(SkiaRsPtProcInitResult) == 24);
static_assert(sizeof(SkiaRsHairlinePaintPlan) == 3);
static_assert(sizeof(SkiaRsTransformedMaskGeometry) == 24);
static_assert(sizeof(SkiaRsUniformColorRgba) == 24);
static_assert(sizeof(SkiaRsClippedTriangle2D) == 36);
static_assert(sizeof(SkiaRsVerticesBlendPlan) == 3);

extern "C" {
bool skia_rs_canvas_draw_treat_aa_stroke_as_hairline(
        float stroke_width, const SkMatrix* matrix, float* out_coverage);
SkiaRsHairlinePaintPlan skia_rs_canvas_draw_hairline_paint_plan(
        int32_t style, float stroke_width, bool is_aa, uint8_t alpha,
        bool supports_coverage_as_alpha, const SkMatrix* matrix);
int32_t skia_rs_canvas_draw_compute_rect_type(
        SkRect rect, int32_t style, float stroke_width, int32_t stroke_join,
        float stroke_miter, bool has_path_effect, bool has_mask_filter,
        const SkMatrix* matrix, SkPoint* out_stroke_size);
SkiaRsDrawRectPlan skia_rs_canvas_draw_compute_rect_plan(
        SkRect pre_paint_rect, SkRect paint_rect, int32_t style, float stroke_width,
        int32_t stroke_join, float stroke_miter, bool has_path_effect,
        bool has_mask_filter, const SkMatrix* ctm);
SkiaRsPtProcInitResult skia_rs_canvas_draw_pt_proc_init(
        int32_t mode, bool has_path_effect, bool has_mask_filter, float stroke_width,
        int32_t stroke_cap, const SkMatrix* matrix, SkIRect clip_bounds);
int32_t skia_rs_canvas_draw_pt_proc_choose(
        int32_t mode, bool is_aa, float stroke_width, int32_t stroke_cap, float radius);
SkRect skia_rs_canvas_draw_make_square_rad(SkPoint center, float radius);
SkXRect skia_rs_canvas_draw_make_xrect(SkRect r);
bool skia_rs_canvas_draw_conservative_local_clip(
        const SkMatrix* ctm, SkIRect rc_bounds, bool rc_empty, SkRect* out_local);
SkIRect skia_rs_canvas_draw_mask_bounds_init(SkRect dev_path_bounds);
bool skia_rs_canvas_draw_trim_mask_bounds(
        SkIRect* bounds, SkIRect clip_bounds, int32_t margin_x, int32_t margin_y);
SkiaRsTransformedMaskGeometry skia_rs_canvas_draw_transformed_bitmap_mask(
        int32_t bm_width, int32_t bm_height, const SkMatrix* ctm,
        SkIRect dst_bounds, SkIRect rc_bounds);
void skia_rs_canvas_draw_mapped_bitmap_round_out(
        const SkMatrix* ctm, const SkMatrix* prematrix, int32_t width, int32_t height,
        SkMatrix* out_matrix, SkIRect* out_ir);
SkiaRsAtlasQuadMatrix skia_rs_canvas_draw_atlas_quad_matrix(
        SkRSXform xform, SkRect tex, const SkMatrix* ctm);
SkiaRsUniformColorRgba skia_rs_canvas_draw_load_uniform_color(const float* rgba);
bool skia_rs_canvas_draw_check_glyph_position(SkPoint pos);
bool skia_rs_canvas_draw_compute_is_opaque_colors(const uint32_t* colors, int32_t count);
SkiaRsClippedTriangle2D skia_rs_canvas_draw_clip_triangle_3d(
        SkPoint3 p0, SkPoint3 p1, SkPoint3 p2);
bool skia_rs_canvas_draw_texture_to_matrix(
        const SkPoint* src, const SkPoint* dst, SkMatrix* out_matrix);
SkiaRsVerticesBlendPlan skia_rs_canvas_draw_vertices_blend_simplify(
        bool has_blend_mode, int32_t blend_mode, bool has_colors);
bool skia_rs_canvas_draw_vertices_early_out(
        size_t vertex_count, size_t index_count, bool rc_empty);
}

}  // namespace

// ============================================================================
// SkDraw.cpp
// ============================================================================

namespace skcpu {

static SkPaint make_paint_with_image_and_mips(const SkPaint& origPaint,
                                              const SkBitmap& bitmap,
                                              const SkSamplingOptions& sampling,
                                              SkMatrix* matrix,
                                              sk_sp<SkMipmap> mips) {
    SkPaint paint(origPaint);
    auto img = SkImage_Raster::MakeFromBitmap(bitmap, SkCopyPixelsMode::kNever, std::move(mips));
    paint.setShader(img->makeShaderForPaint(
            origPaint, SkTileMode::kClamp, SkTileMode::kClamp, sampling, matrix));
    return paint;
}

static SkPaint make_paint_with_image(const SkPaint& origPaint,
                                     const SkBitmap& bitmap,
                                     const SkSamplingOptions& sampling,
                                     SkMatrix* matrix) {
    return make_paint_with_image_and_mips(origPaint, bitmap, sampling, matrix, nullptr);
}

Draw::Draw() { fBlitterChooser = SkBlitter::Choose; }

struct PtProcRec {
    SK_ONLY_STACK_ALLOCATABLE;

    SkCanvas::PointMode fMode;
    const SkPaint* fPaint;
    const SkRegion* fClip;
    const SkRasterClip* fRC;

    SkRect fClipBounds;
    SkScalar fRadius;

    typedef void (*Proc)(const PtProcRec&, SkSpan<const SkPoint> devPts, SkBlitter*);

    bool init(SkCanvas::PointMode, const SkPaint&, const SkMatrix* matrix, const SkRasterClip*);
    Proc chooseProc(SkBlitter** blitter);

private:
    SkAAClipBlitterWrapper fWrapper;
};

#define DIRECT_BLIT_LOOP(writable_method, value)   \
    do {                                           \
        for (auto p : devPts) {                    \
            int x = SkScalarFloorToInt(p.fX);      \
            int y = SkScalarFloorToInt(p.fY);      \
            if (cr.contains(x, y)) {               \
                *pm.writable_method(x, y) = value; \
            }                                      \
        }                                          \
    } while (0)

static void bw_pt_hair_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                            SkBlitter* blitter) {
    const auto direct = blitter->canDirectBlit();
    if (direct && rec.fClip->isRect()) {
        const SkIRect cr = rec.fClip->getBounds();
        auto pm = direct->pm;
        const auto v = direct->value;
        switch (pm.info().bytesPerPixel()) {
            case 1: DIRECT_BLIT_LOOP(writable_addr8, v); break;
            case 2: DIRECT_BLIT_LOOP(writable_addr16, v); break;
            case 4: DIRECT_BLIT_LOOP(writable_addr32, v); break;
            case 8: DIRECT_BLIT_LOOP(writable_addr64, v); break;
            default: SkASSERT(false);
        }
    } else {
        for (auto p : devPts) {
            int x = SkScalarFloorToInt(p.fX);
            int y = SkScalarFloorToInt(p.fY);
            if (rec.fClip->contains(x, y)) {
                blitter->blitH(x, y, 1);
            }
        }
    }
}

#undef DIRECT_BLIT_LOOP

static void bw_line_hair_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                              SkBlitter* blitter) {
    for (size_t i = 0; i + 1 < devPts.size(); i += 2) {
        SkScan::HairLine({&devPts[i], 2}, *rec.fRC, blitter);
    }
}

static void bw_poly_hair_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                              SkBlitter* blitter) {
    SkScan::HairLine(devPts, *rec.fRC, blitter);
}

static void aa_line_hair_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                              SkBlitter* blitter) {
    for (size_t i = 0; i + 1 < devPts.size(); i += 2) {
        SkScan::AntiHairLine({&devPts[i], 2}, *rec.fRC, blitter);
    }
}

static void aa_poly_hair_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                              SkBlitter* blitter) {
    SkScan::AntiHairLine(devPts, *rec.fRC, blitter);
}

static void bw_square_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                           SkBlitter* blitter) {
    for (auto p : devPts) {
        SkRect r = skia_rs_canvas_draw_make_square_rad(p, rec.fRadius);
        if (r.intersect(rec.fClipBounds)) {
            SkScan::FillXRect(skia_rs_canvas_draw_make_xrect(r), *rec.fRC, blitter);
        }
    }
}

static void aa_square_proc(const PtProcRec& rec, SkSpan<const SkPoint> devPts,
                           SkBlitter* blitter) {
    for (auto p : devPts) {
        SkRect r = skia_rs_canvas_draw_make_square_rad(p, rec.fRadius);
        if (r.intersect(rec.fClipBounds)) {
            SkScan::AntiFillXRect(skia_rs_canvas_draw_make_xrect(r), *rec.fRC, blitter);
        }
    }
}

bool PtProcRec::init(SkCanvas::PointMode mode, const SkPaint& paint,
                     const SkMatrix* matrix, const SkRasterClip* rc) {
    SkiaRsPtProcInitResult res = skia_rs_canvas_draw_pt_proc_init(
            static_cast<int32_t>(mode),
            paint.getPathEffect() != nullptr,
            paint.getMaskFilter() != nullptr,
            paint.getStrokeWidth(),
            static_cast<int32_t>(paint.getStrokeCap()),
            matrix,
            rc->getBounds());
    if (!res.valid) {
        return false;
    }
    fMode = mode;
    fPaint = &paint;
    fClip = nullptr;
    fRC = rc;
    fClipBounds = res.clip_bounds;
    fRadius = res.radius;
    return true;
}

PtProcRec::Proc PtProcRec::chooseProc(SkBlitter** blitterPtr) {
    SkBlitter* blitter = *blitterPtr;
    if (fRC->isBW()) {
        fClip = &fRC->bwRgn();
    } else {
        fWrapper.init(*fRC, blitter);
        fClip = &fWrapper.getRgn();
        blitter = fWrapper.getBlitter();
        *blitterPtr = blitter;
    }

    int32_t kind = skia_rs_canvas_draw_pt_proc_choose(
            static_cast<int32_t>(fMode),
            fPaint->isAntiAlias(),
            fPaint->getStrokeWidth(),
            static_cast<int32_t>(fPaint->getStrokeCap()),
            fRadius);
    switch (kind) {
        case 1: return aa_square_proc;
        case 2: return aa_line_hair_proc;
        case 3: return aa_poly_hair_proc;
        case 4: return bw_pt_hair_proc;
        case 5: return bw_line_hair_proc;
        case 6: return bw_poly_hair_proc;
        case 7: return bw_square_proc;
        default: return nullptr;
    }
}

#define MAX_DEV_PTS 32

void Draw::drawPoints(SkCanvas::PointMode mode,
                      SkSpan<const SkPoint> points,
                      const SkPaint& paint,
                      SkDevice* device) const {
    if (SkCanvas::kLines_PointMode == mode) {
        points = points.first(points.size() & ~1);
    }

    SkDEBUGCODE(this->validate();)

    if (points.empty() || fRC->isEmpty()) {
        return;
    }

    PtProcRec rec;
    if (!device && rec.init(mode, paint, fCTM, fRC)) {
        SkAutoBlitterChoose blitter(*this, nullptr, paint, SkRect::MakeEmpty());

        SkPoint devPts[MAX_DEV_PTS];
        SkBlitter* bltr = blitter.get();
        PtProcRec::Proc proc = rec.chooseProc(&bltr);
        const size_t backup = (SkCanvas::kPolygon_PointMode == mode);

        auto count = points.size();
        auto pts = points.data();
        do {
            size_t n = count;
            if (n > MAX_DEV_PTS) {
                n = MAX_DEV_PTS;
            }
            fCTM->mapPoints({devPts, n}, {pts, n});
            if (!SkIsFinite(&devPts[0].fX, n * 2)) {
                return;
            }
            proc(rec, {devPts, n}, bltr);
            pts += n - backup;
            SkASSERT(count >= n);
            count -= n;
            if (count > 0) {
                count += backup;
            }
        } while (count != 0);
    } else {
        this->drawDevicePoints(mode, points, paint, device);
    }
}

static bool clipHandlesSprite(const SkRasterClip& clip, int x, int y, const SkPixmap& pmap) {
    return clip.isBW() || clip.quickContains(SkIRect::MakeXYWH(x, y, pmap.width(), pmap.height()));
}

void Draw::drawBitmap(const SkBitmap& bitmap,
                      const SkMatrix& prematrix,
                      const SkRect* dstBounds,
                      const SkSamplingOptions& sampling,
                      const SkPaint& origPaint,
                      sk_sp<SkMipmap> mips) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty() ||
        bitmap.width() == 0 || bitmap.height() == 0 ||
        bitmap.colorType() == kUnknown_SkColorType) {
        return;
    }

    SkTCopyOnFirstWrite<SkPaint> paint(origPaint);
    if (origPaint.getStyle() != SkPaint::kFill_Style) {
        paint.writable()->setStyle(SkPaint::kFill_Style);
    }

    SkMatrix matrix;
    SkIRect mappedIR;
    skia_rs_canvas_draw_mapped_bitmap_round_out(
            fCTM, &prematrix, bitmap.width(), bitmap.height(), &matrix, &mappedIR);

    if (fRC->quickReject(mappedIR)) {
        return;
    }

    if (!SkColorTypeIsAlphaOnly(bitmap.colorType()) &&
        SkTreatAsSprite(matrix, bitmap.dimensions(), sampling, paint->isAntiAlias())) {
        SkPixmap pmap;
        if (!bitmap.peekPixels(&pmap)) {
            return;
        }
        int ix = SkScalarRoundToInt(matrix.getTranslateX());
        int iy = SkScalarRoundToInt(matrix.getTranslateY());
        if (clipHandlesSprite(*fRC, ix, iy, pmap)) {
            SkSTArenaAlloc<kSkBlitterContextSize> allocator;
            SkBlitter* blitter = SkBlitter::ChooseSprite(fDst, *paint, pmap, ix, iy, &allocator,
                                                         fRC->clipShader());
            if (blitter) {
                SkScan::FillIRect(SkIRect::MakeXYWH(ix, iy, pmap.width(), pmap.height()),
                                  *fRC, blitter);
                return;
            }
        }
    }

    Draw draw(*this);
    draw.fCTM = &matrix;

#if defined(SK_SUPPORT_LEGACY_ALPHA_BITMAP_AS_COVERAGE)
    if (bitmap.colorType() == kAlpha_8_SkColorType && !paint->getColorFilter()) {
        draw.drawBitmapAsMask(bitmap, sampling, *paint, nullptr);
        return;
    }
#endif

    SkPaint paintWithShader =
            make_paint_with_image_and_mips(*paint, bitmap, sampling, nullptr, mips);
    const SkRect srcBounds = SkRect::MakeIWH(bitmap.width(), bitmap.height());
    if (dstBounds) {
        this->drawRect(srcBounds, paintWithShader, &prematrix, dstBounds);
    } else {
        draw.drawRect(srcBounds, paintWithShader);
    }
}

void Draw::drawSprite(const SkBitmap& bitmap, int x, int y, const SkPaint& origPaint) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty() ||
        bitmap.width() == 0 || bitmap.height() == 0 ||
        bitmap.colorType() == kUnknown_SkColorType) {
        return;
    }

    const SkIRect bounds = SkIRect::MakeXYWH(x, y, bitmap.width(), bitmap.height());

    if (fRC->quickReject(bounds)) {
        return;
    }

    SkPaint paint(origPaint);
    paint.setStyle(SkPaint::kFill_Style);

    SkPixmap pmap;
    if (!bitmap.peekPixels(&pmap)) {
        return;
    }

    if (nullptr == paint.getColorFilter() && clipHandlesSprite(*fRC, x, y, pmap)) {
        SkSTArenaAlloc<kSkBlitterContextSize> allocator;
        SkBlitter* blitter = SkBlitter::ChooseSprite(fDst, paint, pmap, x, y, &allocator,
                                                     fRC->clipShader());
        if (blitter) {
            SkScan::FillIRect(bounds, *fRC, blitter);
            return;
        }
    }

    SkMatrix matrix;
    SkRect r;
    r.set(bounds);
    matrix.setTranslate(r.fLeft, r.fTop);
    SkPaint paintWithShader = make_paint_with_image(paint, bitmap, SkSamplingOptions(), &matrix);
    Draw draw(*this);
    draw.fCTM = &SkMatrix::I();
    draw.drawRect(r, paintWithShader);
}

void Draw::drawDevMask(const SkMask& srcM,
                       const SkPaint& paint,
                       const SkMatrix* paintMatrix) const {
    if (srcM.fBounds.isEmpty()) {
        return;
    }

    const SkMask* mask = &srcM;

    SkMaskBuilder dstM;
    if (paint.getMaskFilter() &&
        as_MFB(paint.getMaskFilter())->filterMask(&dstM, srcM, *fCTM, nullptr)) {
        mask = &dstM;
    }
    SkAutoMaskFreeImage ami(dstM.image());

    SkAutoBlitterChoose blitterChooser(*this, paintMatrix, paint, SkRect::Make(dstM.bounds()));
    SkBlitter* blitter = blitterChooser.get();

    SkAAClipBlitterWrapper wrapper;
    const SkRegion* clipRgn;

    if (fRC->isBW()) {
        clipRgn = &fRC->bwRgn();
    } else {
        wrapper.init(*fRC, blitter);
        clipRgn = &wrapper.getRgn();
        blitter = wrapper.getBlitter();
    }
    blitter->blitMaskRegion(*mask, *clipRgn);
}

void Draw::drawBitmapAsMask(const SkBitmap& bitmap,
                            const SkSamplingOptions& sampling,
                            const SkPaint& paint,
                            const SkMatrix* paintMatrix) const {
    SkASSERT(bitmap.colorType() == kAlpha_8_SkColorType);

    if (fRC->isEmpty()) {
        return;
    }

    if (SkTreatAsSprite(*fCTM, bitmap.dimensions(), sampling, paint.isAntiAlias())) {
        int ix = SkScalarRoundToInt(fCTM->getTranslateX());
        int iy = SkScalarRoundToInt(fCTM->getTranslateY());

        SkPixmap pmap;
        if (!bitmap.peekPixels(&pmap)) {
            return;
        }
        SkMask mask(pmap.addr8(0, 0),
                    SkIRect::MakeXYWH(ix, iy, pmap.width(), pmap.height()),
                    SkToU32(pmap.rowBytes()),
                    SkMask::kA8_Format);

        this->drawDevMask(mask, paint, paintMatrix);
    } else {
        SkASSERT(fDst.bounds().contains(fRC->getBounds()));
        SkiaRsTransformedMaskGeometry geom = skia_rs_canvas_draw_transformed_bitmap_mask(
                bitmap.width(), bitmap.height(), fCTM, fDst.bounds(), fRC->getBounds());
        if (!geom.valid) {
            return;
        }

        SkMaskBuilder mask;
        mask.bounds() = geom.bounds;
        mask.format() = SkMask::kA8_Format;
        mask.rowBytes() = geom.row_bytes;
        size_t size = mask.computeImageSize();
        if (0 == size) {
            return;
        }

        AutoTMalloc<uint8_t> storage(size);
        mask.image() = storage.get();
        memset(mask.image(), 0, size);

        {
            SkBitmap device;
            device.installPixels(SkImageInfo::MakeA8(mask.fBounds.width(), mask.fBounds.height()),
                                 mask.image(), mask.fRowBytes);

            SkCanvas c(device);
            c.translate(-SkIntToScalar(mask.fBounds.fLeft),
                        -SkIntToScalar(mask.fBounds.fTop));
            c.concat(*fCTM);

            SkPaint tmpPaint;
            tmpPaint.setAntiAlias(paint.isAntiAlias());
            tmpPaint.setDither(paint.isDither());
            SkPaint paintWithShader = make_paint_with_image(tmpPaint, bitmap, sampling, nullptr);
            SkRect rr;
            rr.setIWH(bitmap.width(), bitmap.height());
            c.drawRect(rr, paintWithShader);
        }
        this->drawDevMask(mask, paint, paintMatrix);
    }
}

bool Draw::computeConservativeLocalClipBounds(SkRect* localBounds) const {
    return skia_rs_canvas_draw_conservative_local_clip(
            fCTM, fRC->getBounds(), fRC->isEmpty(), localBounds);
}

void Draw::drawPaint(const SkPaint& paint) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty()) {
        return;
    }

    SkIRect devRect;
    devRect.setWH(fDst.width(), fDst.height());

    SkAutoBlitterChoose blitter(*this, nullptr, paint, SkRect::Make(devRect));
    SkScan::FillIRect(devRect, *fRC, blitter.get());
}

Draw::RectType Draw::ComputeRectType(const SkRect& rect,
                                     const SkPaint& paint,
                                     const SkMatrix& matrix,
                                     SkPoint* strokeSize) {
    int32_t rt = skia_rs_canvas_draw_compute_rect_type(
            rect,
            static_cast<int32_t>(paint.getStyle()),
            paint.getStrokeWidth(),
            static_cast<int32_t>(paint.getStrokeJoin()),
            paint.getStrokeMiter(),
            paint.getPathEffect() != nullptr,
            paint.getMaskFilter() != nullptr,
            &matrix,
            strokeSize);
    return static_cast<RectType>(rt);
}

static void draw_rect_as_path(const Draw& orig,
                              const SkRect& prePaintRect,
                              const SkPaint& paint,
                              const SkMatrix& ctm) {
    Draw draw(orig);
    draw.fCTM = &ctm;
    draw.drawPath(SkPath::Rect(prePaintRect), paint, nullptr);
}

void Draw::drawRect(const SkRect& prePaintRect,
                    const SkPaint& paint,
                    const SkMatrix* paintMatrix,
                    const SkRect* postPaintRect) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty()) {
        return;
    }

    SkTCopyOnFirstWrite<SkMatrix> matrix(fCTM);
    if (paintMatrix) {
        SkASSERT(postPaintRect);
        matrix.writable()->preConcat(*paintMatrix);
    } else {
        SkASSERT(!postPaintRect);
    }

    const SkRect& paintRect = paintMatrix ? *postPaintRect : prePaintRect;
    SkiaRsDrawRectPlan plan = skia_rs_canvas_draw_compute_rect_plan(
            prePaintRect,
            paintRect,
            static_cast<int32_t>(paint.getStyle()),
            paint.getStrokeWidth(),
            static_cast<int32_t>(paint.getStrokeJoin()),
            paint.getStrokeMiter(),
            paint.getPathEffect() != nullptr,
            paint.getMaskFilter() != nullptr,
            fCTM);

    if (plan.action == 0) {
        return;
    }
    if (plan.action == 1) {
        draw_rect_as_path(*this, prePaintRect, paint, *matrix);
        return;
    }

    if (fRC->quickReject(plan.bbox_ir)) {
        return;
    }

    SkAutoBlitterChoose blitterStorage(*this, matrix, paint, plan.dev_rect);
    const SkRasterClip& clip = *fRC;
    SkBlitter* blitter = blitterStorage.get();

    switch (static_cast<RectType>(plan.rect_type)) {
        case RectType::kFill:
            if (paint.isAntiAlias()) {
                SkScan::AntiFillRect(plan.dev_rect, clip, blitter);
            } else {
                SkScan::FillRect(plan.dev_rect, clip, blitter);
            }
            break;
        case RectType::kStroke:
            if (paint.isAntiAlias()) {
                SkScan::AntiFrameRect(plan.dev_rect, plan.stroke_size, clip, blitter);
            } else {
                SkScan::FrameRect(plan.dev_rect, plan.stroke_size, clip, blitter);
            }
            break;
        case RectType::kHair:
            if (paint.isAntiAlias()) {
                SkScan::AntiHairRect(plan.dev_rect, clip, blitter);
            } else {
                SkScan::HairRect(plan.dev_rect, clip, blitter);
            }
            break;
        default:
            SkDEBUGFAIL("bad rtype");
    }
}

bool DrawTreatAAStrokeAsHairline(SkScalar strokeWidth, const SkMatrix& matrix, SkScalar* coverage) {
    SkASSERT(strokeWidth > 0);
    return skia_rs_canvas_draw_treat_aa_stroke_as_hairline(strokeWidth, &matrix, coverage);
}

void Draw::drawOval(const SkRect& oval, const SkPaint& paint) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty()) {
        return;
    }

    this->drawPath(SkPath::Oval(oval), paint, nullptr);
}

void Draw::drawRRect(const SkRRect& rrect, const SkPaint& paint) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty()) {
        return;
    }

    {
        SkScalar coverage;
        if (skcpu::DrawTreatAsHairline(paint, *fCTM, &coverage)) {
            goto DRAW_PATH;
        }

        if (paint.getPathEffect() || paint.getStyle() != SkPaint::kFill_Style) {
            goto DRAW_PATH;
        }
    }

    if (paint.getMaskFilter()) {
        if (this->drawRRectNinePatch(rrect, paint)) {
            return;
        }
    }

DRAW_PATH:
    this->drawPath(SkPath::RRect(rrect), paint, nullptr);
}

bool Draw::drawRRectNinePatch(const SkRRect& rrect, const SkPaint& paint) const {
    SkASSERT(paint.getMaskFilter());

    if (auto rr = rrect.transform(*fCTM)) {
        SkAutoBlitterChoose blitter(*this, nullptr, paint, rrect.getBounds());
        SkResourceCache* cache = nullptr;
        const SkMaskFilterBase* maskFilter = as_MFB(paint.getMaskFilter());
        if (rrect.getType() == SkRRect::kRect_Type) {
            SkRect devRect = rr->rect();
            if (maskFilter->filterRects(SkSpan(&devRect, 1), *fCTM, *fRC, blitter.get(), cache) ==
                SkMaskFilterBase::FilterReturn::kTrue) {
                return true;
            }
        } else {
            if (maskFilter->filterRRect(*rr, *fCTM, *fRC, blitter.get(), cache)) {
                return true;
            }
        }
    }
    return false;
}

void Draw::drawDevPath(const SkPathRaw& raw,
                       const SkPaint& paint,
                       SkDrawCoverage drawCoverage,
                       SkBlitter* customBlitter,
                       bool doFill) const {
    if (SkPathPriv::TooBigForMath(raw.bounds())) {
        return;
    }

    SkBlitter* blitter = nullptr;
    SkAutoBlitterChoose blitterStorage;
    if (nullptr == customBlitter) {
        blitter = blitterStorage.choose(*this, nullptr, paint, raw.bounds(), drawCoverage);
    } else {
        blitter = customBlitter;
    }

    if (paint.getMaskFilter()) {
        SkStrokeRec::InitStyle style = doFill ? SkStrokeRec::kFill_InitStyle
                                              : SkStrokeRec::kHairline_InitStyle;
        SkResourceCache* cache = nullptr;
        if (as_MFB(paint.getMaskFilter())->filterPath(raw, *fCTM, *fRC, blitter, style, cache)) {
            return;
        }
    }

    void (*proc)(const SkPathRaw&, const SkRasterClip&, SkBlitter*);
    if (doFill) {
        if (paint.isAntiAlias()) {
            proc = SkScan::AntiFillPath;
        } else {
            proc = SkScan::FillPath;
        }
    } else {
        if (paint.isAntiAlias()) {
            switch (paint.getStrokeCap()) {
                case SkPaint::kButt_Cap:   proc = SkScan::AntiHairPath;       break;
                case SkPaint::kSquare_Cap: proc = SkScan::AntiHairSquarePath; break;
                case SkPaint::kRound_Cap:  proc = SkScan::AntiHairRoundPath;  break;
            }
        } else {
            switch (paint.getStrokeCap()) {
                case SkPaint::kButt_Cap:   proc = SkScan::HairPath;       break;
                case SkPaint::kSquare_Cap: proc = SkScan::HairSquarePath; break;
                case SkPaint::kRound_Cap:  proc = SkScan::HairRoundPath;  break;
            }
        }
    }
    proc(raw, *fRC, blitter);
}

static std::optional<SkPaint> modifyPaintForHairlines(const SkPaint& origPaint,
                                                      const SkMatrix& matrix) {
    const auto bm = origPaint.asBlendMode();
    const bool supportsCoverage = bm && SkBlendMode_SupportsCoverageAsAlpha(bm.value());
    SkiaRsHairlinePaintPlan plan = skia_rs_canvas_draw_hairline_paint_plan(
            static_cast<int32_t>(origPaint.getStyle()),
            origPaint.getStrokeWidth(),
            origPaint.isAntiAlias(),
            origPaint.getAlpha(),
            supportsCoverage,
            &matrix);
    if (!plan.modify) {
        return {};
    }
    SkPaint paint(origPaint);
    paint.setStrokeWidth(0);
    if (plan.set_alpha) {
        paint.setAlpha(plan.new_alpha);
    }
    return paint;
}

void Draw::drawPath(const SkPath& origSrcPath,
                    const SkPaint& origPaint,
                    const SkMatrix* prePathMatrix,
                    SkDrawCoverage drawCoverage,
                    SkBlitter* customBlitter) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty()) {
        return;
    }

    std::optional<SkPaint> newPaint = modifyPaintForHairlines(origPaint, *fCTM);
    const SkPaint* paint = newPaint.has_value() ? &newPaint.value() : &origPaint;

    const bool needsFillPath = paint->getPathEffect() || paint->getStyle() != SkPaint::kFill_Style;

    SkPathBuilder builder;
    std::optional<SkPathRaw> raw;
    bool doFill = true;

    sk_sp<SkPathData> pdata;

    if (needsFillPath) {
        SkRect cullRect;
        const SkRect* cullRectPtr = nullptr;
        if (this->computeConservativeLocalClipBounds(&cullRect)) {
            cullRectPtr = &cullRect;
        }

        std::optional<SkPath> prePathStorage;
        const SkPath* pathPtr = &origSrcPath;
        if (prePathMatrix) {
            prePathStorage = pathPtr->tryMakeTransform(*prePathMatrix);
            if (!prePathStorage.has_value()) {
                return;
            }
            pathPtr = &prePathStorage.value();
        }
        doFill = skpathutils::FillPathWithPaint(*pathPtr, *paint, &builder, cullRectPtr, *fCTM);
        builder.transform(*fCTM);
        raw = SkPathPriv::Raw(builder, SkResolveConvexity::kYes);
    } else {
        SkMatrix matrix = *fCTM;
        if (prePathMatrix) {
            matrix.preConcat(*prePathMatrix);
        }

        if (matrix.isIdentity()) {
            raw = SkPathPriv::Raw(origSrcPath, SkResolveConvexity::kYes);
        } else {
            raw = SkPathPriv::Raw(origSrcPath, SkResolveConvexity::kNo);
            if (raw && (pdata = SkPathData::MakeTransform(*raw, matrix))) {
                raw = pdata->raw(origSrcPath.getFillType(), SkResolveConvexity::kYes);
            } else {
                return;
            }
        }
    }

    if (!raw) {
        return;
    }

#if defined(SK_BUILD_FOR_FUZZER)
    if (raw->points().size() > 1000) {
        return;
    }
#endif

    this->drawDevPath(*raw, *paint, drawCoverage, customBlitter, doFill);
}

#ifdef SK_DEBUG
void Draw::validate() const {
    SkASSERT(fCTM != nullptr);
    SkASSERT(fRC != nullptr);

    const SkIRect& cr = fRC->getBounds();
    SkIRect br;
    br.setWH(fDst.width(), fDst.height());
    SkASSERT(cr.isEmpty() || br.contains(cr));
}
#endif

static bool compute_mask_bounds(const SkRect& devPathBounds,
                                const SkIRect& clipBounds,
                                const SkMaskFilter* filter,
                                const SkMatrix* filterMatrix,
                                SkIRect* bounds) {
    SkASSERT(filter);
    SkASSERT(filterMatrix);
    *bounds = skia_rs_canvas_draw_mask_bounds_init(devPathBounds);

    SkIVector margin = SkIPoint::Make(0, 0);
    SkMask srcM(nullptr, *bounds, 0, SkMask::kA8_Format);
    SkMaskBuilder dstM;
    if (!as_MFB(filter)->filterMask(&dstM, srcM, *filterMatrix, &margin)) {
        return false;
    }

    return skia_rs_canvas_draw_trim_mask_bounds(bounds, clipBounds, margin.fX, margin.fY);
}

static void draw_into_mask(const SkMask& mask,
                           SkPathRaw raw,
                           SkStrokeRec::InitStyle style) {
    SkPixmap dst;
    if (!dst.reset(mask)) {
        return;
    }

    const float dx = -mask.fBounds.fLeft;
    const float dy = -mask.fBounds.fTop;
    const SkMatrix translate = SkMatrix::Translate(dx, dy);

    SkPaint paint;
    paint.setAntiAlias(true);
    SkBlitterSizedArena alloc;
    SkBlitter* blitter = SkChooseA8Blitter(dst, translate, paint, &alloc,
                                           SkDrawCoverage::kNo, nullptr);

    skia_private::AutoSTArray<32, SkPoint> devPoints(raw.fPoints.size());
    translate.mapPoints(devPoints, raw.fPoints);
    raw.fPoints = devPoints;
    raw.fBounds = raw.fBounds.makeOffset(dx, dy);
    if (!raw.fBounds.isFinite()) {
        return;
    }

    const SkRasterClip clip(SkIRect::MakeWH(mask.fBounds.width(), mask.fBounds.height()));

    switch (style) {
        case SkStrokeRec::kHairline_InitStyle:
            SkScan::AntiHairPath(raw, clip, blitter);
            break;
        case SkStrokeRec::kFill_InitStyle:
            SkScan::AntiFillPath(raw, clip, blitter);
            break;
    }
}

bool DrawToMask(const SkPathRaw& devRaw,
                const SkIRect& clipBounds,
                const SkMaskFilter* filter,
                const SkMatrix* filterMatrix,
                SkMaskBuilder* dst,
                SkMaskBuilder::CreateMode mode,
                SkStrokeRec::InitStyle style) {
    SkASSERT(filter);
    if (devRaw.empty()) {
        return false;
    }

    if (SkMaskBuilder::kJustRenderImage_CreateMode != mode) {
        static const SkRect kInverseBounds = {SK_ScalarNegativeInfinity,
                                              SK_ScalarNegativeInfinity,
                                              SK_ScalarInfinity,
                                              SK_ScalarInfinity};
        SkRect pathBounds = devRaw.isInverseFillType() ? kInverseBounds : devRaw.bounds();
        if (!compute_mask_bounds(pathBounds, clipBounds, filter, filterMatrix, &dst->bounds())) {
            return false;
        }
    }

    if (SkMaskBuilder::kComputeBoundsAndRenderImage_CreateMode == mode) {
        dst->format() = SkMask::kA8_Format;
        dst->rowBytes() = dst->fBounds.width();
        size_t size = dst->computeImageSize();
        if (0 == size) {
            return false;
        }
        dst->image() = SkMaskBuilder::AllocImage(size, SkMaskBuilder::kZeroInit_Alloc);
    }

    if (SkMaskBuilder::kJustComputeBounds_CreateMode != mode) {
        draw_into_mask(*dst, devRaw, style);
    }
    return true;
}

void Draw::drawDevicePoints(SkCanvas::PointMode mode,
                            SkSpan<const SkPoint> points,
                            const SkPaint& paint,
                            SkDevice* device) const {
    if (SkCanvas::kLines_PointMode == mode) {
        points = points.first(points.size() & ~1);
    }

    SkDEBUGCODE(this->validate();)

    if (points.empty() || fRC->isEmpty()) {
        return;
    }

    if (!SkIsFinite(&points[0].fX, points.size() * 2)) {
        return;
    }

    switch (mode) {
        case SkCanvas::kPoints_PointMode: {
            SkPaint newPaint(paint);
            newPaint.setStyle(SkPaint::kFill_Style);

            float width = newPaint.getStrokeWidth();
            float radius = width / 2.f;

            if (newPaint.getStrokeCap() == SkPaint::kRound_Cap) {
                if (device) {
                    for (const auto& pt : points) {
                        SkRect r = SkRect::MakeLTRB(pt.fX - radius, pt.fY - radius,
                                                    pt.fX + radius, pt.fY + radius);
                        device->drawOval(r, newPaint);
                    }
                } else {
                    SkPath path = SkPath::Circle(0, 0, radius);
                    SkMatrix preMatrix;

                    for (const auto& pt : points) {
                        preMatrix.setTranslate(pt.fX, pt.fY);
                        this->drawPath(path, newPaint, &preMatrix);
                    }
                }
            } else {
                SkRect r;
                for (const auto& pt : points) {
                    r.fLeft = pt.fX - radius;
                    r.fTop = pt.fY - radius;
                    r.fRight = r.fLeft + width;
                    r.fBottom = r.fTop + width;
                    if (device) {
                        device->drawRect(r, newPaint);
                    } else {
                        this->drawRect(r, newPaint);
                    }
                }
            }
            break;
        }
        case SkCanvas::kLines_PointMode:
            if (2 == points.size() && paint.getPathEffect()) {
                SkStrokeRec stroke(paint);
                SkPathEffectBase::PointData pointData;
                SkPath path = SkPath::Line(points[0], points[1]);
                SkRect cullRect = SkRect::Make(fRC->getBounds());

                if (as_PEB(paint.getPathEffect())
                            ->asPoints(&pointData, path, stroke, *fCTM, &cullRect)) {
                    SkPaint newP(paint);
                    newP.setPathEffect(nullptr);
                    newP.setStyle(SkPaint::kFill_Style);

                    if (!pointData.fFirst.isEmpty()) {
                        if (device) {
                            device->drawPath(pointData.fFirst, newP);
                        } else {
                            this->drawPath(pointData.fFirst, newP, nullptr);
                        }
                    }

                    if (!pointData.fLast.isEmpty()) {
                        if (device) {
                            device->drawPath(pointData.fLast, newP);
                        } else {
                            this->drawPath(pointData.fLast, newP, nullptr);
                        }
                    }

                    if (pointData.fSize.fX == pointData.fSize.fY) {
                        SkASSERT(pointData.fSize.fX == newP.getStrokeWidth() / 2.f);
                        if (SkPathEffectBase::PointData::kCircles_PointFlag & pointData.fFlags) {
                            newP.setStrokeCap(SkPaint::kRound_Cap);
                        } else {
                            newP.setStrokeCap(SkPaint::kButt_Cap);
                        }

                        if (device) {
                            device->drawPoints(
                                    SkCanvas::kPoints_PointMode, pointData.points(), newP);
                        } else {
                            this->drawDevicePoints(
                                    SkCanvas::kPoints_PointMode, pointData.points(), newP, device);
                        }
                        break;
                    } else {
                        SkASSERT(!(SkPathEffectBase::PointData::kCircles_PointFlag &
                                   pointData.fFlags));

                        SkRect r;
                        for (const auto& pt : pointData.points()) {
                            r.setLTRB(pt.fX - pointData.fSize.fX,
                                      pt.fY - pointData.fSize.fY,
                                      pt.fX + pointData.fSize.fX,
                                      pt.fY + pointData.fSize.fY);
                            if (device) {
                                device->drawRect(r, newP);
                            } else {
                                this->drawRect(r, newP);
                            }
                        }
                    }

                    break;
                }
            }
            [[fallthrough]];
        case SkCanvas::kPolygon_PointMode: {
            auto count = points.size() - 1;
            SkPaint p(paint);
            p.setStyle(SkPaint::kStroke_Style);
            size_t inc = (SkCanvas::kLines_PointMode == mode) ? 2 : 1;

            for (size_t i = 0; i < count; i += inc) {
                auto path = SkPath::Line(points[i], points[i + 1]);
                if (device) {
                    device->drawPath(path, p);
                } else {
                    this->drawPath(path, p, nullptr);
                }
            }
            break;
        }
    }
}

// ============================================================================
// SkDraw_atlas.cpp
// ============================================================================

static void fill_rect(const SkMatrix& ctm, const SkRasterClip& rc,
                      const SkRect& r, SkBlitter* blitter) {
    if (ctm.rectStaysRect()) {
        SkScan::FillRect(ctm.mapRect(r), rc, blitter);
    } else {
        SkPathRawShapes::Rect raw(r);
        ctm.mapPoints(raw.fStorage);
        if (auto bounds = SkRect::Bounds(raw.fStorage)) {
            raw.fBounds = *bounds;
            SkScan::FillPath(raw, rc, blitter);
        }
    }
}

void Draw::drawAtlas(SkSpan<const SkRSXform> xform,
                     SkSpan<const SkRect> textures,
                     SkSpan<const SkColor> colors,
                     sk_sp<SkBlender> blender,
                     const SkPaint& paint) {
    SkASSERT(xform.size() == textures.size());
    SkASSERT(colors.empty() || (xform.size() == colors.size()));

    sk_sp<SkShader> atlasShader = paint.refShader();
    if (!atlasShader || xform.empty()) {
        return;
    }

    SkSTArenaAlloc<256> alloc;

    SkPaint p(paint);
    p.setAntiAlias(false);
    p.setStyle(SkPaint::kFill_Style);
    p.setShader(nullptr);
    p.setMaskFilter(nullptr);

    const bool perspective = fCTM->hasPerspective();
    auto transformShader = alloc.make<SkTransformShader>(*as_SB(atlasShader), perspective);

    SkRasterPipeline pipeline(&alloc);
    SkSurfaceProps props = SkSurfacePropsCopyOrDefault(fProps);
    SkStageRec rec = {&pipeline,
                      &alloc,
                      fDst.colorType(),
                      fDst.colorSpace(),
                      p.getColor4f(),
                      props,
                      SkRect::MakeEmpty()};
    if (!as_SB(transformShader)->appendRootStages(rec, SkMatrix::I())) {
        return;
    }

    SkRasterPipelineContexts::UniformColorCtx* uniformCtx = nullptr;
    SkColorSpaceXformSteps steps(sk_srgb_singleton(), kUnpremul_SkAlphaType,
                                 rec.fDstCS, kUnpremul_SkAlphaType);
    if (!colors.empty()) {
        uniformCtx = alloc.make<SkRasterPipelineContexts::UniformColorCtx>();
        rec.fPipeline->append(SkRasterPipelineOp::uniform_color_dst, uniformCtx);
        std::optional<SkBlendMode> bm = as_BB(blender)->asBlendMode();
        if (!bm.has_value()) {
            return;
        }
        SkBlendMode_AppendStages(*bm, rec.fPipeline);
    }

    bool isOpaque = colors.empty() && transformShader->isOpaque();
    if (p.getAlphaf() != 1) {
        rec.fPipeline->append(SkRasterPipelineOp::scale_1_float, alloc.make<float>(p.getAlphaf()));
        isOpaque = false;
    }

    auto blitter = SkCreateRasterPipelineBlitter(fDst, p, pipeline, isOpaque, &alloc,
                                                 fRC->clipShader());
    if (!blitter) {
        return;
    }

    for (size_t i = 0; i < xform.size(); ++i) {
        if (!colors.empty()) {
            SkColor4f c4 = SkColor4f::FromColor(colors[i]);
            steps.apply(c4.vec());
            SkiaRsUniformColorRgba uc =
                    skia_rs_canvas_draw_load_uniform_color(c4.premul().vec());
            uniformCtx->rgba[0] = uc.rgba_u16[0]; uniformCtx->r = uc.r;
            uniformCtx->rgba[1] = uc.rgba_u16[1]; uniformCtx->g = uc.g;
            uniformCtx->rgba[2] = uc.rgba_u16[2]; uniformCtx->b = uc.b;
            uniformCtx->rgba[3] = uc.rgba_u16[3]; uniformCtx->a = uc.a;
        }

        SkiaRsAtlasQuadMatrix qm =
                skia_rs_canvas_draw_atlas_quad_matrix(xform[i], textures[i], fCTM);
        if (qm.invertible) {
            if (transformShader->update(qm.inv)) {
                fill_rect(qm.mx, *fRC, textures[i], blitter);
            }
        } else {
            return;
        }
    }
}

// ============================================================================
// SkDraw_text.cpp
// ============================================================================

void Draw::paintMasks(SkZip<const SkGlyph*, SkPoint> accepted, const SkPaint& paint) const {
    SkSTArenaAlloc<kSkBlitterContextSize> alloc;
    SkBlitter* blitter = SkBlitter::Choose(fDst,
                                           *fCTM,
                                           paint,
                                           &alloc,
                                           SkDrawCoverage::kNo,
                                           fRC->clipShader(),
                                           SkSurfacePropsCopyOrDefault(fProps),
                                           SkRect::MakeEmpty());

    SkAAClipBlitterWrapper wrapper{*fRC, blitter};
    blitter = wrapper.getBlitter();

    bool useRegion = fRC->isBW() && !fRC->isRect();

    if (useRegion) {
        for (auto [glyph, pos] : accepted) {
            if (skia_rs_canvas_draw_check_glyph_position(pos)) {
                SkMask mask = glyph->mask(pos);
                SkRegion::Cliperator clipper(fRC->bwRgn(), mask.fBounds);
                if (!clipper.done()) {
                    if (SkMask::kARGB32_Format == mask.fFormat) {
                        SkBitmap bm;
                        bm.installPixels(SkImageInfo::MakeN32Premul(mask.fBounds.size()),
                                         const_cast<uint8_t*>(mask.fImage),
                                         mask.fRowBytes);
                        bm.setImmutable();
                        this->drawSprite(bm, mask.fBounds.x(), mask.fBounds.y(), paint);
                    } else {
                        const SkIRect& cr = clipper.rect();
                        do {
                            blitter->blitMask(mask, cr);
                            clipper.next();
                        } while (!clipper.done());
                    }
                }
            }
        }
    } else {
        SkIRect clipBounds = fRC->isBW() ? fRC->bwRgn().getBounds()
                                         : fRC->aaRgn().getBounds();
        for (auto [glyph, pos] : accepted) {
            if (skia_rs_canvas_draw_check_glyph_position(pos)) {
                SkMask mask = glyph->mask(pos);
                SkIRect storage;
                const SkIRect* bounds = &mask.fBounds;

                if (!clipBounds.containsNoEmptyCheck(mask.fBounds)) {
                    if (!storage.intersect(mask.fBounds, clipBounds)) {
                        continue;
                    }
                    bounds = &storage;
                }

                if (SkMask::kARGB32_Format == mask.fFormat) {
                    SkBitmap bm;
                    bm.installPixels(SkImageInfo::MakeN32Premul(mask.fBounds.size()),
                                     const_cast<uint8_t*>(mask.fImage),
                                     mask.fRowBytes);
                    bm.setImmutable();
                    this->drawSprite(bm, mask.fBounds.x(), mask.fBounds.y(), paint);
                } else {
                    blitter->blitMask(mask, *bounds);
                }
            }
        }
    }
}

void Draw::drawGlyphRunList(SkCanvas* canvas,
                            GlyphRunListPainter* glyphPainter,
                            const sktext::GlyphRunList& glyphRunList,
                            const SkPaint& paint) const {
    SkDEBUGCODE(this->validate();)

    if (fRC->isEmpty()) {
        return;
    }

    glyphPainter->drawForBitmapDevice(canvas, this, glyphRunList, paint, *fCTM);
}

}  // namespace skcpu

// ============================================================================
// SkDraw_vertices.cpp
// ============================================================================

// Non-static to maintain 100% strong-symbol parity with SkDraw_vertices.cpp.
std::optional<SkMatrix> texture_to_matrix(const VertState& state, const SkPoint verts[],
                                          const SkPoint texs[]) {
    SkPoint src[3] = {verts[state.f0], verts[state.f1], verts[state.f2]};
    SkPoint dst[3] = {texs[state.f0], texs[state.f1], texs[state.f2]};
    SkMatrix out;
    if (skia_rs_canvas_draw_texture_to_matrix(src, dst, &out)) {
        return out;
    }
    return std::nullopt;
}

static SkPMColor4f* convert_colors(const SkColor src[],
                                   int count,
                                   SkColorSpace* deviceCS,
                                   SkArenaAlloc* alloc,
                                   bool skipColorXform) {
    SkPMColor4f* dst = alloc->makeArray<SkPMColor4f>(count);
    auto dstCS = skipColorXform ? nullptr : sk_ref_sp(deviceCS);
    SkImageInfo srcInfo = SkImageInfo::Make(
            count, 1, kBGRA_8888_SkColorType, kUnpremul_SkAlphaType, SkColorSpace::MakeSRGB());
    SkImageInfo dstInfo =
            SkImageInfo::Make(count, 1, kRGBA_F32_SkColorType, kPremul_SkAlphaType, dstCS);
    SkAssertResult(SkConvertPixels(dstInfo, dst, 0, srcInfo, src, 0));
    return dst;
}

static void fill_triangle(const VertState& state, SkBlitter* blitter, const SkRasterClip& rc,
                          const SkPoint dev2[], const SkPoint3 dev3[]) {
    if (dev3) {
        SkiaRsClippedTriangle2D clipped = skia_rs_canvas_draw_clip_triangle_3d(
                dev3[state.f0], dev3[state.f1], dev3[state.f2]);
        if (clipped.count > 0) {
            SkASSERT(clipped.count == 3 || clipped.count == 4);
            SkScan::FillTriangle(clipped.pts, rc, blitter);
            if (clipped.count == 4) {
                clipped.pts[1] = clipped.pts[2];
                clipped.pts[2] = clipped.pts[3];
                SkScan::FillTriangle(clipped.pts, rc, blitter);
            }
        }
    } else {
        SkPoint tmp[] = {dev2[state.f0], dev2[state.f1], dev2[state.f2]};
        SkScan::FillTriangle(tmp, rc, blitter);
    }
}

namespace skcpu {

void Draw::drawFixedVertices(const SkVertices* vertices,
                             sk_sp<SkBlender> blender,
                             const SkPaint& paint,
                             const SkMatrix& ctmInverse,
                             const SkPoint* dev2,
                             const SkPoint3* dev3,
                             SkArenaAlloc* outerAlloc,
                             bool skipColorXform) const {
    SkVerticesPriv info(vertices->priv());

    const int vertexCount = info.vertexCount();
    const int indexCount = info.indexCount();
    const SkPoint* positions = info.positions();
    const SkPoint* texCoords = info.texCoords();
    const uint16_t* indices = info.indices();
    const SkColor* colors = info.colors();

    SkShader* paintShader = paint.getShader();

    if (paintShader) {
        if (!texCoords) {
            texCoords = positions;
        }
    } else {
        texCoords = nullptr;
    }

    std::optional<SkBlendMode> bm = as_BB(blender)->asBlendMode();
    SkiaRsVerticesBlendPlan bplan = skia_rs_canvas_draw_vertices_blend_simplify(
            bm.has_value(),
            bm.has_value() ? static_cast<int32_t>(*bm) : -1,
            colors != nullptr);
    if (bplan.drop_colors) {
        colors = nullptr;
    }
    const bool blenderIsDst = bplan.blender_is_dst;
    if (bplan.drop_shader_and_tex) {
        texCoords = nullptr;
        paintShader = nullptr;
    }

    SkASSERT((texCoords != nullptr) == (paintShader != nullptr));

    const bool usePerspective = fCTM->hasPerspective();

    SkTriColorShader* triColorShader = nullptr;
    SkPMColor4f* dstColors = nullptr;
    if (colors) {
        dstColors =
                convert_colors(colors, vertexCount, fDst.colorSpace(), outerAlloc, skipColorXform);
        bool isOpaque = skia_rs_canvas_draw_compute_is_opaque_colors(colors, vertexCount);
        triColorShader = outerAlloc->make<SkTriColorShader>(isOpaque, usePerspective);
    }

    auto applyShaderColorBlend = [&](SkShader* shader) -> sk_sp<SkShader> {
        if (!colors) {
            return sk_ref_sp(shader);
        }
        if (blenderIsDst) {
            return sk_ref_sp(triColorShader);
        }
        sk_sp<SkShader> shaderWithWhichToBlend;
        if (!shader) {
            shaderWithWhichToBlend = SkShaders::Color(paint.getColor4f().makeOpaque(), nullptr);
        } else {
            shaderWithWhichToBlend = sk_ref_sp(shader);
        }
        return SkShaders::Blend(blender,
                                sk_ref_sp(triColorShader),
                                std::move(shaderWithWhichToBlend));
    };

    SkTransformShader* transformShader = nullptr;
    const SkMatrix* ctm = fCTM;
    if (texCoords && texCoords != positions) {
        paintShader = transformShader = outerAlloc->make<SkTransformShader>(*as_SB(paintShader),
                                                                            usePerspective);
        ctm = &SkMatrix::I();
    }
    sk_sp<SkShader> blenderShader = applyShaderColorBlend(paintShader);

    SkPaint finalPaint{paint};
    finalPaint.setShader(std::move(blenderShader));

    VertState state(vertexCount, indices, indexCount);
    VertState::Proc vertProc = state.chooseProc(info.mode());
    SkSurfaceProps props = SkSurfacePropsCopyOrDefault(fProps);

    auto blitter = SkCreateRasterPipelineBlitter(fDst,
                                                 finalPaint,
                                                 *ctm,
                                                 outerAlloc,
                                                 fRC->clipShader(),
                                                 props,
                                                 SkRect::MakeEmpty());
    if (!blitter) {
        return;
    }
    while (vertProc(&state)) {
        if (triColorShader && !triColorShader->update(ctmInverse, positions, dstColors,
                                                      state.f0, state.f1, state.f2)) {
            continue;
        }

        std::optional<SkMatrix> localM;
        if (!transformShader || ((localM = texture_to_matrix(state, positions, texCoords)) &&
                                 transformShader->update(SkMatrix::Concat(*localM, ctmInverse)))) {
            fill_triangle(state, blitter, *fRC, dev2, dev3);
        }
    }
}

void Draw::drawVertices(const SkVertices* vertices,
                        sk_sp<SkBlender> blender,
                        const SkPaint& paint,
                        bool skipColorXform) const {
    SkVerticesPriv info(vertices->priv());
    const size_t vertexCount = info.vertexCount();
    const size_t indexCount = info.indexCount();

    if (skia_rs_canvas_draw_vertices_early_out(vertexCount, indexCount, fRC->isEmpty())) {
        return;
    }
    auto ctmInv = fCTM->invert();
    if (!ctmInv) {
        return;
    }

    constexpr size_t kDefVertexCount = 16;
    constexpr size_t kOuterSize = sizeof(SkTriColorShader) +
                                  (2 * sizeof(SkPoint) + sizeof(SkColor4f)) * kDefVertexCount;
    SkSTArenaAlloc<kOuterSize> outerAlloc;

    SkPoint* dev2 = nullptr;
    SkPoint3* dev3 = nullptr;

    if (fCTM->hasPerspective()) {
        dev3 = outerAlloc.makeArray<SkPoint3>(vertexCount);
        fCTM->mapPointsToHomogeneous({dev3, vertexCount}, {info.positions(), vertexCount});
        if (!SkIsFinite((const SkScalar*)dev3, vertexCount * 3)) {
            return;
        }
    } else {
        dev2 = outerAlloc.makeArray<SkPoint>(vertexCount);
        fCTM->mapPoints({dev2, vertexCount}, {info.positions(), vertexCount});
        if (SkRect::BoundsOrEmpty({dev2, vertexCount}).isEmpty()) {
            return;
        }
    }

    this->drawFixedVertices(
            vertices, std::move(blender), paint, *ctmInv, dev2, dev3, &outerAlloc, skipColorXform);
}

}  // namespace skcpu
