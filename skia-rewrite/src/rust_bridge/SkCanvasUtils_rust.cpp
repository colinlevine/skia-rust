/*
 * Safe Rust bridge for:
 *   src/utils/SkNWayCanvas.cpp
 *   src/utils/SkNullCanvas.cpp
 *   src/utils/SkPaintFilterCanvas.cpp
 *   src/utils/SkCanvasStack.cpp
 *   src/utils/SkCanvasStateUtils.cpp
 * (Phase 1 Layer 5.3 skia-rs-canvas, GN target :skia / skia_utils_private).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkSize.h"
#include "include/core/SkSurface.h"  // IWYU pragma: keep
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTypes.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTDArray.h"
#include "include/utils/SkCanvasStateUtils.h"
#include "include/utils/SkNWayCanvas.h"
#include "include/utils/SkNoDrawCanvas.h"
#include "include/utils/SkNullCanvas.h"
#include "include/utils/SkPaintFilterCanvas.h"
#include "src/core/SkCanvasPriv.h"
#include "src/core/SkDevice.h"
#include "src/core/SkWriter32.h"
#include "src/partition_alloc/raw_ptr_exclusion.h"
#include "src/utils/SkCanvasStack.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <iterator>
#include <optional>
#include <utility>

namespace sktext {
class GlyphRunList;
}
class SkData;
class SkDrawable;
class SkImage;
class SkPath;
class SkPicture;
class SkRRect;
class SkRegion;
class SkTextBlob;
class SkVertices;
enum class SkClipOp;
struct SkDrawShadowRec;

namespace {

struct SkiaRsStateClipRect {
    int32_t left;
    int32_t top;
    int32_t right;
    int32_t bottom;
};

static_assert(sizeof(SkIPoint) == 8);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkM44) == 64);
static_assert(sizeof(SkiaRsStateClipRect) == 16);

extern "C" {
SkIRect skia_rs_canvas_stack_local_subtract_bounds(
        SkIRect canvas_bounds, SkIPoint new_origin, SkIPoint existing_origin);
void skia_rs_canvas_stack_layer_matrix(SkIPoint origin, const SkM44* mx, SkM44* out);
int32_t skia_rs_canvas_state_color_to_raster_config(int32_t color_type, int32_t n32_color_type);
int32_t skia_rs_canvas_state_raster_config_to_color(int32_t raster_config, int32_t n32_color_type);
SkIRect skia_rs_canvas_state_union_clip_rects(const SkiaRsStateClipRect* rects, int32_t count);
bool skia_rs_canvas_paint_filter_can_restore_null(
        float alphaf, bool has_color_filter, bool has_image_filter,
        bool has_blend_mode, int32_t blend_mode);
}

}  // namespace

// ============================================================================
// SkNWayCanvas.cpp
// ============================================================================

SkNWayCanvas::SkNWayCanvas(int width, int height) : INHERITED(width, height) {}

SkNWayCanvas::~SkNWayCanvas() {
    this->removeAll();
}

void SkNWayCanvas::addCanvas(SkCanvas* canvas) {
    if (!fList.empty()) {
        SkASSERT(fList[0]->rootDevice() != this->rootDevice());
    }
    if (canvas) {
        *fList.append() = canvas;
    }
}

void SkNWayCanvas::removeCanvas(SkCanvas* canvas) {
    auto found = std::find(fList.begin(), fList.end(), canvas);
    if (found != fList.end()) {
        fList.removeShuffle(std::distance(fList.begin(), found));
    }
}

void SkNWayCanvas::removeAll() {
    fList.reset();
}

class SkNWayCanvas::Iter {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    Iter(const SkTDArray<SkCanvas*>& list) : fList(list), fIndex(0) {}
    bool next() {
        if (fIndex < fList.size()) {
            fCanvas = fList[fIndex++];
            return true;
        }
        return false;
    }
    SkCanvas* operator->() { return fCanvas; }
    SkCanvas* get() const { return fCanvas; }

private:
    const SkTDArray<SkCanvas*>& fList;
    int fIndex;
    SkCanvas* fCanvas;
};

void SkNWayCanvas::willSave() {
    Iter iter(fList);
    while (iter.next()) {
        iter->save();
    }
    this->INHERITED::willSave();
}

SkCanvas::SaveLayerStrategy SkNWayCanvas::getSaveLayerStrategy(const SaveLayerRec& rec) {
    Iter iter(fList);
    while (iter.next()) {
        iter->saveLayer(rec);
    }
    this->INHERITED::getSaveLayerStrategy(rec);
    return kNoLayer_SaveLayerStrategy;
}

bool SkNWayCanvas::onDoSaveBehind(const SkRect* bounds) {
    Iter iter(fList);
    while (iter.next()) {
        SkCanvasPriv::SaveBehind(iter.get(), bounds);
    }
    this->INHERITED::onDoSaveBehind(bounds);
    return false;
}

void SkNWayCanvas::willRestore() {
    Iter iter(fList);
    while (iter.next()) {
        iter->restore();
    }
    this->INHERITED::willRestore();
}

void SkNWayCanvas::didConcat44(const SkM44& m) {
    Iter iter(fList);
    while (iter.next()) {
        iter->concat(m);
    }
}

void SkNWayCanvas::didSetM44(const SkM44& matrix) {
    Iter iter(fList);
    while (iter.next()) {
        iter->setMatrix(matrix);
    }
}

void SkNWayCanvas::didTranslate(SkScalar x, SkScalar y) {
    Iter iter(fList);
    while (iter.next()) {
        iter->translate(x, y);
    }
}

void SkNWayCanvas::didScale(SkScalar x, SkScalar y) {
    Iter iter(fList);
    while (iter.next()) {
        iter->scale(x, y);
    }
}

void SkNWayCanvas::onClipRect(const SkRect& rect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    Iter iter(fList);
    while (iter.next()) {
        iter->clipRect(rect, op, kSoft_ClipEdgeStyle == edgeStyle);
    }
    this->INHERITED::onClipRect(rect, op, edgeStyle);
}

void SkNWayCanvas::onClipRRect(const SkRRect& rrect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    Iter iter(fList);
    while (iter.next()) {
        iter->clipRRect(rrect, op, kSoft_ClipEdgeStyle == edgeStyle);
    }
    this->INHERITED::onClipRRect(rrect, op, edgeStyle);
}

void SkNWayCanvas::onClipPath(const SkPath& path, SkClipOp op, ClipEdgeStyle edgeStyle) {
    Iter iter(fList);
    while (iter.next()) {
        iter->clipPath(path, op, kSoft_ClipEdgeStyle == edgeStyle);
    }
    this->INHERITED::onClipPath(path, op, edgeStyle);
}

void SkNWayCanvas::onClipShader(sk_sp<SkShader> sh, SkClipOp op) {
    Iter iter(fList);
    while (iter.next()) {
        iter->clipShader(sh, op);
    }
    this->INHERITED::onClipShader(std::move(sh), op);
}

void SkNWayCanvas::onClipRegion(const SkRegion& deviceRgn, SkClipOp op) {
    Iter iter(fList);
    while (iter.next()) {
        iter->clipRegion(deviceRgn, op);
    }
    this->INHERITED::onClipRegion(deviceRgn, op);
}

void SkNWayCanvas::onResetClip() {
    Iter iter(fList);
    while (iter.next()) {
        SkCanvasPriv::ResetClip(iter.get());
    }
    this->INHERITED::onResetClip();
}

void SkNWayCanvas::onDrawPaint(const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawPaint(paint);
    }
}

void SkNWayCanvas::onDrawBehind(const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        SkCanvasPriv::DrawBehind(iter.get(), paint);
    }
}

void SkNWayCanvas::onDrawPoints(PointMode mode, size_t count, const SkPoint pts[],
                                const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawPoints(mode, {pts, count}, paint);
    }
}

void SkNWayCanvas::onDrawRect(const SkRect& rect, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawRect(rect, paint);
    }
}

void SkNWayCanvas::onDrawRegion(const SkRegion& region, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawRegion(region, paint);
    }
}

void SkNWayCanvas::onDrawOval(const SkRect& rect, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawOval(rect, paint);
    }
}

void SkNWayCanvas::onDrawArc(const SkRect& rect, SkScalar startAngle, SkScalar sweepAngle,
                             bool useCenter, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawArc(rect, startAngle, sweepAngle, useCenter, paint);
    }
}

void SkNWayCanvas::onDrawRRect(const SkRRect& rrect, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawRRect(rrect, paint);
    }
}

void SkNWayCanvas::onDrawDRRect(const SkRRect& outer, const SkRRect& inner, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawDRRect(outer, inner, paint);
    }
}

void SkNWayCanvas::onDrawPath(const SkPath& path, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawPath(path, paint);
    }
}

void SkNWayCanvas::onDrawImage2(const SkImage* image, SkScalar left, SkScalar top,
                                const SkSamplingOptions& sampling, const SkPaint* paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawImage(image, left, top, sampling, paint);
    }
}

void SkNWayCanvas::onDrawImageRect2(const SkImage* image, const SkRect& src, const SkRect& dst,
                                    const SkSamplingOptions& sampling, const SkPaint* paint,
                                    SrcRectConstraint constraint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawImageRect(image, src, dst, sampling, paint, constraint);
    }
}

void SkNWayCanvas::onDrawImageLattice2(const SkImage* image, const Lattice& lattice,
                                       const SkRect& dst, SkFilterMode filter,
                                       const SkPaint* paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawImageLattice(image, lattice, dst, filter, paint);
    }
}

void SkNWayCanvas::onDrawAtlas2(const SkImage* image, const SkRSXform xform[], const SkRect tex[],
                                const SkColor colors[], int count, SkBlendMode bmode,
                                const SkSamplingOptions& sampling, const SkRect* cull,
                                const SkPaint* paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawAtlas(image,
                        {xform, (size_t)count},
                        {tex, (size_t)count},
                        {colors, colors ? (size_t)count : 0},
                        bmode, sampling, cull, paint);
    }
}

void SkNWayCanvas::onDrawMesh(const SkMesh& mesh, sk_sp<SkBlender> blender, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawMesh(mesh, blender, paint);
    }
}

void SkNWayCanvas::onDrawGlyphRunList(const sktext::GlyphRunList& list, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->onDrawGlyphRunList(list, paint);
    }
}

void SkNWayCanvas::onDrawTextBlob(const SkTextBlob* blob, SkScalar x, SkScalar y,
                                  const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawTextBlob(blob, x, y, paint);
    }
}

void SkNWayCanvas::onDrawSlug(const sktext::gpu::Slug* slug, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawSlug(slug, paint);
    }
}

void SkNWayCanvas::onDrawPicture(const SkPicture* picture, const SkMatrix* matrix,
                                 const SkPaint* paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawPicture(picture, matrix, paint);
    }
}

void SkNWayCanvas::onDrawDrawable(SkDrawable* drawable, const SkMatrix* matrix) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawDrawable(drawable, matrix);
    }
}

void SkNWayCanvas::onDrawVerticesObject(const SkVertices* vertices,
                                        SkBlendMode bmode, const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawVertices(vertices, bmode, paint);
    }
}

void SkNWayCanvas::onDrawPatch(const SkPoint cubics[12], const SkColor colors[4],
                               const SkPoint texCoords[4], SkBlendMode bmode,
                               const SkPaint& paint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawPatch(cubics, colors, texCoords, bmode, paint);
    }
}

void SkNWayCanvas::onDrawShadowRec(const SkPath& path, const SkDrawShadowRec& rec) {
    Iter iter(fList);
    while (iter.next()) {
        iter->private_draw_shadow_rec(path, rec);
    }
}

void SkNWayCanvas::onDrawAnnotation(const SkRect& rect, const char key[], SkData* data) {
    Iter iter(fList);
    while (iter.next()) {
        iter->drawAnnotation(rect, key, data);
    }
}

void SkNWayCanvas::onDrawEdgeAAQuad(const SkRect& rect, const SkPoint clip[4],
                                    QuadAAFlags aa, const SkColor4f& color, SkBlendMode mode) {
    Iter iter(fList);
    while (iter.next()) {
        iter->experimental_DrawEdgeAAQuad(rect, clip, aa, color, mode);
    }
}

void SkNWayCanvas::onDrawEdgeAAImageSet2(const ImageSetEntry set[], int count,
                                         const SkPoint dstClips[], const SkMatrix preViewMatrices[],
                                         const SkSamplingOptions& sampling, const SkPaint* paint,
                                         SrcRectConstraint constraint) {
    Iter iter(fList);
    while (iter.next()) {
        iter->experimental_DrawEdgeAAImageSet(
                set, count, dstClips, preViewMatrices, sampling, paint, constraint);
    }
}

// ============================================================================
// SkNullCanvas.cpp
// ============================================================================

std::unique_ptr<SkCanvas> SkMakeNullCanvas() {
    return std::unique_ptr<SkCanvas>(new SkNWayCanvas(0, 0));
}

// ============================================================================
// SkPaintFilterCanvas.cpp
// ============================================================================

class SkPaintFilterCanvas::AutoPaintFilter {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    AutoPaintFilter(const SkPaintFilterCanvas* canvas, const SkPaint* paint)
            : fPaint(paint ? *paint : SkPaint()) {
        fShouldDraw = canvas->onFilter(fPaint);
    }

    AutoPaintFilter(const SkPaintFilterCanvas* canvas, const SkPaint& paint)
            : AutoPaintFilter(canvas, &paint) {}

    const SkPaint& paint() const { return fPaint; }
    bool shouldDraw() const { return fShouldDraw; }

private:
    SkPaint fPaint;
    bool fShouldDraw;
};

SkPaintFilterCanvas::SkPaintFilterCanvas(SkCanvas* canvas)
        : SkCanvasVirtualEnforcer<SkNWayCanvas>(canvas->imageInfo().width(),
                                                canvas->imageInfo().height()) {
    this->clipRect(SkRect::Make(canvas->getDeviceClipBounds()));
    this->setMatrix(canvas->getLocalToDevice());
    this->addCanvas(canvas);
}

void SkPaintFilterCanvas::onDrawPaint(const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawPaint(apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawBehind(const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawBehind(apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawPoints(PointMode mode, size_t count, const SkPoint pts[],
                                       const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawPoints(mode, count, pts, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawRect(const SkRect& rect, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawRect(rect, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawRRect(const SkRRect& rrect, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawRRect(rrect, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawDRRect(const SkRRect& outer, const SkRRect& inner,
                                       const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawDRRect(outer, inner, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawRegion(const SkRegion& region, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawRegion(region, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawOval(const SkRect& rect, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawOval(rect, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawArc(const SkRect& rect, SkScalar startAngle, SkScalar sweepAngle,
                                    bool useCenter, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawArc(rect, startAngle, sweepAngle, useCenter, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawPath(const SkPath& path, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawPath(path, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawImage2(const SkImage* image, SkScalar left, SkScalar top,
                                       const SkSamplingOptions& sampling, const SkPaint* paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawImage2(image, left, top, sampling, &apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawImageRect2(const SkImage* image, const SkRect& src,
                                           const SkRect& dst, const SkSamplingOptions& sampling,
                                           const SkPaint* paint, SrcRectConstraint constraint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawImageRect2(image, src, dst, sampling, &apf.paint(), constraint);
    }
}

void SkPaintFilterCanvas::onDrawImageLattice2(const SkImage* image, const Lattice& lattice,
                                              const SkRect& dst, SkFilterMode filter,
                                              const SkPaint* paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawImageLattice2(image, lattice, dst, filter, &apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawVerticesObject(const SkVertices* vertices,
                                               SkBlendMode bmode, const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawVerticesObject(vertices, bmode, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawPatch(const SkPoint cubics[12], const SkColor colors[4],
                                      const SkPoint texCoords[4], SkBlendMode bmode,
                                      const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawPatch(cubics, colors, texCoords, bmode, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawPicture(const SkPicture* picture, const SkMatrix* m,
                                        const SkPaint* originalPaint) {
    AutoPaintFilter apf(this, originalPaint);
    if (apf.shouldDraw()) {
        const SkPaint* newPaint = &apf.paint();
        if (originalPaint == nullptr) {
            auto bm = newPaint->asBlendMode();
            if (skia_rs_canvas_paint_filter_can_restore_null(
                        newPaint->getAlphaf(),
                        newPaint->getColorFilter() != nullptr,
                        newPaint->getImageFilter() != nullptr,
                        bm.has_value(),
                        bm.has_value() ? static_cast<int32_t>(*bm) : -1)) {
                newPaint = nullptr;
            }
        }
        this->SkNWayCanvas::onDrawPicture(picture, m, newPaint);
    }
}

void SkPaintFilterCanvas::onDrawDrawable(SkDrawable* drawable, const SkMatrix* matrix) {
    AutoPaintFilter apf(this, nullptr);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawDrawable(drawable, matrix);
    }
}

void SkPaintFilterCanvas::onDrawGlyphRunList(const sktext::GlyphRunList& list,
                                             const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawGlyphRunList(list, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawTextBlob(const SkTextBlob* blob, SkScalar x, SkScalar y,
                                         const SkPaint& paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawTextBlob(blob, x, y, apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawAtlas2(const SkImage* image, const SkRSXform xform[],
                                       const SkRect tex[], const SkColor colors[], int count,
                                       SkBlendMode bmode, const SkSamplingOptions& sampling,
                                       const SkRect* cull, const SkPaint* paint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawAtlas2(
                image, xform, tex, colors, count, bmode, sampling, cull, &apf.paint());
    }
}

void SkPaintFilterCanvas::onDrawAnnotation(const SkRect& rect, const char key[], SkData* value) {
    this->SkNWayCanvas::onDrawAnnotation(rect, key, value);
}

void SkPaintFilterCanvas::onDrawShadowRec(const SkPath& path, const SkDrawShadowRec& rec) {
    this->SkNWayCanvas::onDrawShadowRec(path, rec);
}

void SkPaintFilterCanvas::onDrawEdgeAAQuad(const SkRect& rect, const SkPoint clip[4],
                                           QuadAAFlags aa, const SkColor4f& color,
                                           SkBlendMode mode) {
    SkPaint paint;
    paint.setColor(color);
    paint.setBlendMode(mode);
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawEdgeAAQuad(rect, clip, aa, apf.paint().getColor4f(),
                                             apf.paint().getBlendMode_or(SkBlendMode::kSrcOver));
    }
}

void SkPaintFilterCanvas::onDrawEdgeAAImageSet2(const ImageSetEntry set[], int count,
                                                const SkPoint dstClips[],
                                                const SkMatrix preViewMatrices[],
                                                const SkSamplingOptions& sampling,
                                                const SkPaint* paint,
                                                SrcRectConstraint constraint) {
    AutoPaintFilter apf(this, paint);
    if (apf.shouldDraw()) {
        this->SkNWayCanvas::onDrawEdgeAAImageSet2(
                set, count, dstClips, preViewMatrices, sampling, &apf.paint(), constraint);
    }
}

sk_sp<SkSurface> SkPaintFilterCanvas::onNewSurface(const SkImageInfo& info,
                                                   const SkSurfaceProps& props) {
    return this->proxy()->makeSurface(info, &props);
}

bool SkPaintFilterCanvas::onPeekPixels(SkPixmap* pixmap) {
    return this->proxy()->peekPixels(pixmap);
}

bool SkPaintFilterCanvas::onAccessTopLayerPixels(SkPixmap* pixmap) {
    SkImageInfo info;
    size_t rowBytes;
    void* addr = this->proxy()->accessTopLayerPixels(&info, &rowBytes);
    if (!addr) {
        return false;
    }
    pixmap->reset(info, addr, rowBytes);
    return true;
}

SkImageInfo SkPaintFilterCanvas::onImageInfo() const {
    return this->proxy()->imageInfo();
}

bool SkPaintFilterCanvas::onGetProps(SkSurfaceProps* props, bool top) const {
    if (props) {
        *props = top ? this->proxy()->getTopProps() : this->proxy()->getBaseProps();
    }
    return true;
}

// ============================================================================
// SkCanvasStack.cpp
// ============================================================================

SkCanvasStack::SkCanvasStack(int width, int height) : INHERITED(width, height) {}

SkCanvasStack::~SkCanvasStack() {
    this->removeAll();
}

void SkCanvasStack::pushCanvas(std::unique_ptr<SkCanvas> canvas, const SkIPoint& origin) {
    if (canvas) {
        const SkIRect canvasBounds = SkIRect::MakeSize(canvas->getBaseLayerSize());
        this->INHERITED::addCanvas(canvas.get());

        CanvasData* data = &fCanvasData.push_back();
        data->origin = origin;
        data->requiredClip.setRect(canvasBounds);
        data->ownedCanvas = std::move(canvas);

        for (int i = fList.size() - 1; i > 0; --i) {
            SkIRect localBounds = skia_rs_canvas_stack_local_subtract_bounds(
                    canvasBounds, origin, fCanvasData[i - 1].origin);
            fCanvasData[i - 1].requiredClip.op(localBounds, SkRegion::kDifference_Op);
            fList[i - 1]->clipRegion(fCanvasData[i - 1].requiredClip);
        }
    }
    SkASSERT(fList.size() == fCanvasData.size());
}

void SkCanvasStack::removeAll() {
    this->INHERITED::removeAll();
    fCanvasData.clear();
}

void SkCanvasStack::clipToZOrderedBounds() {
    SkASSERT(fList.size() == fCanvasData.size());
    for (int i = 0; i < fList.size(); ++i) {
        fList[i]->clipRegion(fCanvasData[i].requiredClip);
    }
}

void SkCanvasStack::didSetM44(const SkM44& mx) {
    SkASSERT(fList.size() == fCanvasData.size());
    for (int i = 0; i < fList.size(); ++i) {
        SkM44 layerMx;
        skia_rs_canvas_stack_layer_matrix(fCanvasData[i].origin, &mx, &layerMx);
        fList[i]->setMatrix(layerMx);
    }
    this->SkCanvas::didSetM44(mx);
}

void SkCanvasStack::onClipRect(const SkRect& r, SkClipOp op, ClipEdgeStyle edgeStyle) {
    this->INHERITED::onClipRect(r, op, edgeStyle);
    this->clipToZOrderedBounds();
}

void SkCanvasStack::onClipRRect(const SkRRect& rr, SkClipOp op, ClipEdgeStyle edgeStyle) {
    this->INHERITED::onClipRRect(rr, op, edgeStyle);
    this->clipToZOrderedBounds();
}

void SkCanvasStack::onClipPath(const SkPath& p, SkClipOp op, ClipEdgeStyle edgeStyle) {
    this->INHERITED::onClipPath(p, op, edgeStyle);
    this->clipToZOrderedBounds();
}

void SkCanvasStack::onClipShader(sk_sp<SkShader> cs, SkClipOp op) {
    this->INHERITED::onClipShader(std::move(cs), op);
}

void SkCanvasStack::onClipRegion(const SkRegion& deviceRgn, SkClipOp op) {
    SkASSERT(fList.size() == fCanvasData.size());
    for (int i = 0; i < fList.size(); ++i) {
        SkRegion tempRegion;
        deviceRgn.translate(-fCanvasData[i].origin.x(),
                            -fCanvasData[i].origin.y(), &tempRegion);
        tempRegion.op(fCanvasData[i].requiredClip, SkRegion::kIntersect_Op);
        fList[i]->clipRegion(tempRegion, op);
    }
    this->SkCanvas::onClipRegion(deviceRgn, op);
}

// ============================================================================
// SkCanvasStateUtils.cpp (Stable C ABI)
// ============================================================================

enum RasterConfigs {
    kUnknown_RasterConfig   = 0,
    kRGB_565_RasterConfig   = 1,
    kARGB_8888_RasterConfig = 2
};
typedef int32_t RasterConfig;

enum CanvasBackends {
    kUnknown_CanvasBackend = 0,
    kRaster_CanvasBackend  = 1,
    kGPU_CanvasBackend     = 2,
    kPDF_CanvasBackend     = 3
};
typedef int32_t CanvasBackend;

struct ClipRect {
    int32_t left, top, right, bottom;
};
static_assert(sizeof(ClipRect) == sizeof(SkiaRsStateClipRect));

struct SkMCState {
    std::array<float, 9> matrix;
    int32_t clipRectCount;
    RAW_PTR_EXCLUSION ClipRect* clipRects;
};

struct SkCanvasLayerState {
    CanvasBackend type;
    int32_t x, y;
    int32_t width;
    int32_t height;

    SkMCState mcState;

    union {
        struct {
            RasterConfig config;
            uint64_t rowBytes;
            RAW_PTR_EXCLUSION void* pixels;
        } raster;
        struct {
            int32_t textureID;
        } gpu;
    };
};

class SkCanvasState {
public:
    SkCanvasState(int32_t version, SkCanvas* canvas) {
        SkASSERT(canvas);
        this->version = version;
        width = canvas->getBaseLayerSize().width();
        height = canvas->getBaseLayerSize().height();
    }

    int32_t version;
    int32_t width;
    int32_t height;
    int32_t alignmentPadding;
};

class SkCanvasState_v1 : public SkCanvasState {
public:
    static const int32_t kVersion = 1;

    SkCanvasState_v1(SkCanvas* canvas) : INHERITED(kVersion, canvas) {
        layerCount = 0;
        layers = nullptr;
        mcState.clipRectCount = 0;
        mcState.clipRects = nullptr;
        originalCanvas = canvas;
    }

    ~SkCanvasState_v1() {
        for (int i = 0; i < layerCount; ++i) {
            if (layers[i].mcState.clipRectCount > 0) {
                sk_free(layers[i].mcState.clipRects);
            }
        }
        if (mcState.clipRectCount > 0) {
            sk_free(mcState.clipRects);
        }
        sk_free(layers);
    }

    SkMCState mcState;
    int32_t layerCount;
    RAW_PTR_EXCLUSION SkCanvasLayerState* layers;

private:
    RAW_PTR_EXCLUSION SkCanvas* originalCanvas;
    using INHERITED = SkCanvasState;
};

static void setup_MC_state(SkMCState* state, const SkMatrix& matrix, const SkIRect& clip) {
    state->clipRectCount = 0;
    for (int i = 0; i < 9; i++) {
        state->matrix[i] = matrix.get(i);
    }

    if (!clip.isEmpty()) {
        state->clipRectCount = 1;
        state->clipRects = (ClipRect*)sk_malloc_throw(sizeof(ClipRect));
        state->clipRects->left = clip.fLeft;
        state->clipRects->top = clip.fTop;
        state->clipRects->right = clip.fRight;
        state->clipRects->bottom = clip.fBottom;
    }
}

SkCanvasState* SkCanvasStateUtils::CaptureCanvasState(SkCanvas* canvas) {
    SkASSERT(canvas);

    if (canvas->androidFramework_isClipAA()) {
        return nullptr;
    }

    auto canvasState = std::make_unique<SkCanvasState_v1>(canvas);
    setup_MC_state(&canvasState->mcState, canvas->getTotalMatrix(), canvas->getDeviceClipBounds());

    SkDevice* device = canvas->topDevice();
    SkASSERT(device);

    SkSWriter32<sizeof(SkCanvasLayerState)> layerWriter;
    SkPixmap pmap;
    if (!device->accessPixels(&pmap) || 0 == pmap.width() || 0 == pmap.height()) {
        return nullptr;
    }
    if (!device->isPixelAlignedToGlobal()) {
        return nullptr;
    }

    SkIPoint origin = device->getOrigin();

    RasterConfig rc = skia_rs_canvas_state_color_to_raster_config(
            static_cast<int32_t>(pmap.colorType()),
            static_cast<int32_t>(kN32_SkColorType));
    if (rc == kUnknown_RasterConfig) {
        return nullptr;
    }

    SkCanvasLayerState* layerState =
            (SkCanvasLayerState*)layerWriter.reserve(sizeof(SkCanvasLayerState));
    layerState->type = kRaster_CanvasBackend;
    layerState->x = origin.x();
    layerState->y = origin.y();
    layerState->width = pmap.width();
    layerState->height = pmap.height();
    layerState->raster.config = rc;
    layerState->raster.rowBytes = pmap.rowBytes();
    layerState->raster.pixels = pmap.writable_addr();

    setup_MC_state(&layerState->mcState, device->localToDevice(), device->devClipBounds());

    SkASSERT(layerWriter.bytesWritten() == sizeof(SkCanvasLayerState));
    canvasState->layerCount = 1;
    canvasState->layers = (SkCanvasLayerState*)sk_malloc_throw(layerWriter.bytesWritten());
    layerWriter.flatten(canvasState->layers);

    return canvasState.release();
}

static void setup_canvas_from_MC_state(const SkMCState& state, SkCanvas* canvas) {
    SkMatrix matrix;
    for (int i = 0; i < 9; i++) {
        matrix.set(i, state.matrix[i]);
    }

    SkIRect bounds = skia_rs_canvas_state_union_clip_rects(
            reinterpret_cast<const SkiaRsStateClipRect*>(state.clipRects),
            state.clipRectCount);

    canvas->clipRect(SkRect::Make(bounds));
    canvas->concat(matrix);
}

static std::unique_ptr<SkCanvas>
make_canvas_from_canvas_layer(const SkCanvasLayerState& layerState) {
    SkASSERT(kRaster_CanvasBackend == layerState.type);

    SkColorType colorType = static_cast<SkColorType>(
            skia_rs_canvas_state_raster_config_to_color(
                    layerState.raster.config,
                    static_cast<int32_t>(kN32_SkColorType)));
    if (colorType == kUnknown_SkColorType) {
        return nullptr;
    }

    SkBitmap bitmap;
    bitmap.installPixels(SkImageInfo::Make(layerState.width, layerState.height,
                                           colorType, kPremul_SkAlphaType),
                         layerState.raster.pixels, (size_t)layerState.raster.rowBytes);

    SkASSERT(!bitmap.empty());
    SkASSERT(!bitmap.isNull());

    auto canvas = std::make_unique<SkCanvas>(bitmap);
    setup_canvas_from_MC_state(layerState.mcState, canvas.get());
    return canvas;
}

std::unique_ptr<SkCanvas> SkCanvasStateUtils::MakeFromCanvasState(const SkCanvasState* state) {
    SkASSERT(state);
    SkASSERT(SkCanvasState_v1::kVersion == state->version);

    const SkCanvasState_v1* state_v1 = static_cast<const SkCanvasState_v1*>(state);
    if (state_v1->layerCount < 1) {
        return nullptr;
    }

    auto canvas = std::make_unique<SkCanvasStack>(state->width, state->height);
    setup_canvas_from_MC_state(state_v1->mcState, canvas.get());

    for (int i = state_v1->layerCount - 1; i >= 0; --i) {
        std::unique_ptr<SkCanvas> canvasLayer = make_canvas_from_canvas_layer(state_v1->layers[i]);
        if (!canvasLayer) {
            return nullptr;
        }
        canvas->pushCanvas(std::move(canvasLayer),
                           SkIPoint::Make(state_v1->layers[i].x, state_v1->layers[i].y));
    }

    return canvas;
}

void SkCanvasStateUtils::ReleaseCanvasState(SkCanvasState* state) {
    SkASSERT(!state || SkCanvasState_v1::kVersion == state->version);
    delete static_cast<SkCanvasState_v1*>(state);
}
