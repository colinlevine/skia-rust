/*
 * Safe Rust bridge for:
 *   src/core/SkRecord.cpp
 *   src/core/SkRecords.cpp
 *   src/core/SkRecordCanvas.cpp
 *   src/core/SkRecordDraw.cpp
 *   src/core/SkRecordOpts.cpp
 * (Phase 1 Layer 5.2 skia-rs-picture, GN target :core).
 */

#include "include/core/SkBBHFactory.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlender.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkData.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkMesh.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkString.h"
#include "include/core/SkSurface.h"
#include "include/core/SkTextBlob.h"
#include "include/core/SkVertices.h"
#include "include/private/SkAssert.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "include/private/chromium/Slug.h"
#include "src/core/SkCanvasPriv.h"
#include "src/core/SkDrawShadowInfo.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkRecord.h"
#include "src/core/SkRecordCanvas.h"
#include "src/core/SkRecordDraw.h"
#include "src/core/SkRecordOpts.h"
#include "src/core/SkRecordPattern.h"
#include "src/core/SkRecords.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"
#include "src/text/GlyphRun.h"
#include "src/utils/SkPatchUtils.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <new>
#include <optional>
#include <utility>
#include <vector>

using namespace skia_private;

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkM44) == 64);
static_assert(sizeof(SkBBoxHierarchy::Metadata) == sizeof(bool));

namespace {

struct RsRecordSlot {
    uint32_t op_type;
    size_t ptr;
};

struct RsPaintOptInfo {
    bool present;
    bool is_src_over;
    bool is_src_mode;
    bool has_shader;
    bool has_path_effect;
    bool has_mask_filter;
    bool has_color_filter;
    bool has_image_filter;
    uint8_t alpha;
    uint32_t color;
};

struct RsLayerFoldDecision {
    bool kill_save_layer_and_restore;
    bool update_paint_alpha;
    uint8_t new_alpha;
};

struct RsEdgeAAImageSetEntryInfo {
    SkRect dst_rect;
    int32_t matrix_index;
    bool has_clip;
};

struct RsFillBoundsState;

using RsAdjustPaintCb = bool (*)(void* ctx, size_t paint_token, SkRect* rect);

}  // namespace

extern "C" {
// SkRecord & SkRecordCanvas
int32_t skia_rs_record_next_capacity(int32_t reserved);
size_t skia_rs_record_bytes_used(size_t approx_bytes_allocated, size_t sizeof_record);
size_t skia_rs_record_defrag(RsRecordSlot* slots, size_t count);
SkIRect skia_rs_record_safe_picture_bounds(SkRect bounds);
int32_t skia_rs_record_lattice_flag_count(bool has_rect_types, int32_t x_count, int32_t y_count);

// SkRecordOpts
void skia_rs_record_opts_noop_save_restores(uint32_t* op_types, bool* noop_mask, size_t count);
size_t skia_rs_record_opts_find_save_layer_draw_restore(
        const uint32_t* op_types, size_t count, int32_t* out_begins, size_t out_cap);
RsLayerFoldDecision skia_rs_record_opts_eval_save_layer_draw_restore(
        bool has_backdrop,
        bool has_filters,
        const RsPaintOptInfo* layer_paint,
        const RsPaintOptInfo* draw_paint);
size_t skia_rs_record_opts_find_svg_opacity_filter(
        const uint32_t* op_types, size_t count, int32_t* out_begins, size_t out_cap);
RsLayerFoldDecision skia_rs_record_opts_eval_svg_opacity_filter(
        bool outer_has_backdrop,
        bool outer_has_filters,
        bool inner_has_filters,
        const RsPaintOptInfo* opacity_paint,
        const RsPaintOptInfo* filter_layer_paint);

// SkRecordDraw / FillBounds
bool skia_rs_fill_bounds_paint_may_affect_transparent_black(
        bool has_paint, bool if_affects, bool cf_affects, int32_t blend_mode);
RsFillBoundsState* skia_rs_fill_bounds_create(SkRect cull_rect, size_t count);
void skia_rs_fill_bounds_set_current_op(RsFillBoundsState* fb, size_t op);
void skia_rs_fill_bounds_get_ctm(const RsFillBoundsState* fb, SkMatrix* out_ctm);
void skia_rs_fill_bounds_update_ctm_set_matrix(RsFillBoundsState* fb, const SkMatrix* m);
void skia_rs_fill_bounds_update_ctm_set_m44(RsFillBoundsState* fb, const SkM44* m);
void skia_rs_fill_bounds_update_ctm_concat(RsFillBoundsState* fb, const SkMatrix* m);
void skia_rs_fill_bounds_update_ctm_concat44(RsFillBoundsState* fb, const SkM44* m);
void skia_rs_fill_bounds_update_ctm_scale(RsFillBoundsState* fb, float sx, float sy);
void skia_rs_fill_bounds_update_ctm_translate(RsFillBoundsState* fb, float dx, float dy);
void skia_rs_fill_bounds_push_save_block(
        RsFillBoundsState* fb,
        size_t paint_token,
        bool has_backdrop,
        bool paint_affects_transparent_black);
void skia_rs_fill_bounds_track_restore(RsFillBoundsState* fb, const SkMatrix* restored_ctm);
void skia_rs_fill_bounds_push_control(RsFillBoundsState* fb);
void skia_rs_fill_bounds_record_cull_rect_draw(RsFillBoundsState* fb);
void skia_rs_fill_bounds_record_noop(RsFillBoundsState* fb);
void skia_rs_fill_bounds_record_rect_draw(
        RsFillBoundsState* fb,
        SkRect local_rect,
        size_t paint_token,
        RsAdjustPaintCb adjust_fn,
        void* ctx);
void skia_rs_fill_bounds_record_points_draw(
        RsFillBoundsState* fb,
        const SkPoint* pts,
        size_t count,
        float stroke_width,
        size_t paint_token,
        RsAdjustPaintCb adjust_fn,
        void* ctx);
void skia_rs_fill_bounds_record_picture_draw(
        RsFillBoundsState* fb,
        SkRect pic_cull,
        const SkMatrix* pic_matrix,
        size_t paint_token,
        RsAdjustPaintCb adjust_fn,
        void* ctx);
void skia_rs_fill_bounds_record_text_blob_draw(
        RsFillBoundsState* fb,
        SkRect blob_bounds,
        float x,
        float y,
        size_t paint_token,
        RsAdjustPaintCb adjust_fn,
        void* ctx);
void skia_rs_fill_bounds_record_edge_aa_quad_draw(
        RsFillBoundsState* fb,
        SkRect rect,
        const SkPoint* clip_quad,
        RsAdjustPaintCb adjust_fn,
        void* ctx);
void skia_rs_fill_bounds_record_edge_aa_image_set_draw(
        RsFillBoundsState* fb,
        const RsEdgeAAImageSetEntryInfo* entries,
        size_t entry_count,
        const SkPoint* dst_clips,
        size_t clip_count,
        const SkMatrix* pre_view_matrices,
        size_t matrix_count,
        RsAdjustPaintCb adjust_fn,
        void* ctx);
void skia_rs_fill_bounds_finish_and_destroy(
        RsFillBoundsState* fb, SkRect* out_bounds, bool* out_is_draw, size_t count);
}  // extern "C"

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkRecord (src/core/SkRecord.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

SkRecord::~SkRecord() {
    Destroyer destroyer;
    for (int i = 0; i < this->count(); i++) {
        this->mutate(i, destroyer);
    }
}

void SkRecord::grow() {
    SkASSERT(fCount == fReserved);
    fReserved = skia_rs_record_next_capacity(fReserved);
    fRecords.realloc(fReserved);
}

size_t SkRecord::bytesUsed() const {
    return skia_rs_record_bytes_used(fApproxBytesAllocated, sizeof(SkRecord));
}

void SkRecord::defrag() {
    static_assert(sizeof(SkRecord::Record) == sizeof(RsRecordSlot));
    static_assert(sizeof(SkRecords::Type) == sizeof(uint32_t));
    fCount = SkToInt(skia_rs_record_defrag(
            reinterpret_cast<RsRecordSlot*>(fRecords.get()), SkToSizeT(fCount)));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkRecords (src/core/SkRecords.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace SkRecords {
TypedMatrix::TypedMatrix(const SkMatrix& matrix) : SkMatrix(matrix) {
    (void)this->getType();
}
}  // namespace SkRecords

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkDrawableList & SkRecordCanvas (src/core/SkRecordCanvas.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

SkDrawableList::~SkDrawableList() {
    for (SkDrawable* p : fArray) {
        p->unref();
    }
    fArray.reset();
}

SkSnapshotArray* SkDrawableList::newDrawableSnapshot() {
    const int count = fArray.size();
    if (0 == count) {
        return nullptr;
    }
    AutoTMalloc<const SkPicture*> pics(count);
    for (int i = 0; i < count; ++i) {
        pics[i] = fArray[i]->makePictureSnapshot().release();
        if (pics[i]) continue;

        for (int j = 0; j < i; ++j) pics[j]->unref();
        return nullptr;
    }
    return new SkSnapshotArray(pics.release(), count);
}

void SkDrawableList::append(SkDrawable* drawable) { *fArray.append() = SkRef(drawable); }

SkRecordCanvas::SkRecordCanvas(SkRecord* record, int width, int height)
        : SkCanvasVirtualEnforcer<SkNoDrawCanvas>(width, height)
        , fApproxBytesUsedBySubPictures(0)
        , fRecord(record) {
    SkASSERT(this->imageInfo().width() >= 0 && this->imageInfo().height() >= 0);
}

SkRecordCanvas::SkRecordCanvas(SkRecord* record, const SkRect& bounds)
        : SkCanvasVirtualEnforcer<SkNoDrawCanvas>(skia_rs_record_safe_picture_bounds(bounds))
        , fApproxBytesUsedBySubPictures(0)
        , fRecord(record) {
    SkASSERT(this->imageInfo().width() >= 0 && this->imageInfo().height() >= 0);
}

void SkRecordCanvas::reset(SkRecord* record, const SkRect& bounds) {
    this->forgetRecord();
    fRecord = record;
    this->resetCanvas(skia_rs_record_safe_picture_bounds(bounds));
    SkASSERT(this->imageInfo().width() >= 0 && this->imageInfo().height() >= 0);
}

void SkRecordCanvas::forgetRecord() {
    fDrawableList.reset(nullptr);
    fApproxBytesUsedBySubPictures = 0;
    fRecord = nullptr;
}

template <typename T, typename... Args>
void SkRecordCanvas::append(Args&&... args) {
    new (fRecord->append<T>()) T{std::forward<Args>(args)...};
}

#define INHERITED(method, ...) this->SkNoDrawCanvas::method(__VA_ARGS__)

template <typename T>
T* SkRecordCanvas::copy(const T* src) {
    if (nullptr == src) {
        return nullptr;
    }
    return new (fRecord->alloc<T>()) T(*src);
}

template <typename T>
T* SkRecordCanvas::copy(const T src[], size_t count) {
    if (nullptr == src) {
        return nullptr;
    }
    T* dst = fRecord->alloc<T>(count);
    for (size_t i = 0; i < count; i++) {
        new (dst + i) T(src[i]);
    }
    return dst;
}

template <>
char* SkRecordCanvas::copy(const char src[], size_t count) {
    if (nullptr == src) {
        return nullptr;
    }
    char* dst = fRecord->alloc<char>(count);
    memcpy(dst, src, count);
    return dst;
}

template <>
char* SkRecordCanvas::copy(const char* src) {
    return this->copy(src, strlen(src) + 1);
}

void SkRecordCanvas::onDrawPaint(const SkPaint& paint) {
    this->append<SkRecords::DrawPaint>(paint);
}

void SkRecordCanvas::onDrawBehind(const SkPaint& paint) {
    this->append<SkRecords::DrawBehind>(paint);
}

void SkRecordCanvas::onDrawPoints(PointMode mode,
                                  size_t count,
                                  const SkPoint pts[],
                                  const SkPaint& paint) {
    this->append<SkRecords::DrawPoints>(paint, mode, SkToUInt(count), this->copy(pts, count));
}

void SkRecordCanvas::onDrawRect(const SkRect& rect, const SkPaint& paint) {
    this->append<SkRecords::DrawRect>(paint, rect);
}

void SkRecordCanvas::onDrawRegion(const SkRegion& region, const SkPaint& paint) {
    this->append<SkRecords::DrawRegion>(paint, region);
}

void SkRecordCanvas::onDrawOval(const SkRect& oval, const SkPaint& paint) {
    this->append<SkRecords::DrawOval>(paint, oval);
}

void SkRecordCanvas::onDrawArc(const SkRect& oval,
                               SkScalar startAngle,
                               SkScalar sweepAngle,
                               bool useCenter,
                               const SkPaint& paint) {
    this->append<SkRecords::DrawArc>(paint, oval, startAngle, sweepAngle, useCenter);
}

void SkRecordCanvas::onDrawRRect(const SkRRect& rrect, const SkPaint& paint) {
    this->append<SkRecords::DrawRRect>(paint, rrect);
}

void SkRecordCanvas::onDrawDRRect(const SkRRect& outer,
                                  const SkRRect& inner,
                                  const SkPaint& paint) {
    this->append<SkRecords::DrawDRRect>(paint, outer, inner);
}

void SkRecordCanvas::onDrawDrawable(SkDrawable* drawable, const SkMatrix* matrix) {
    if (!fDrawableList) {
        fDrawableList = std::make_unique<SkDrawableList>();
    }
    fDrawableList->append(drawable);
    this->append<SkRecords::DrawDrawable>(
            this->copy(matrix), drawable->getBounds(), fDrawableList->count() - 1);
}

void SkRecordCanvas::onDrawPath(const SkPath& path, const SkPaint& paint) {
    this->append<SkRecords::DrawPath>(paint, path);
}

void SkRecordCanvas::onDrawImage2(const SkImage* image,
                                  SkScalar x,
                                  SkScalar y,
                                  const SkSamplingOptions& sampling,
                                  const SkPaint* paint) {
    this->append<SkRecords::DrawImage>(this->copy(paint), sk_ref_sp(image), x, y, sampling);
}

void SkRecordCanvas::onDrawImageRect2(const SkImage* image,
                                      const SkRect& src,
                                      const SkRect& dst,
                                      const SkSamplingOptions& sampling,
                                      const SkPaint* paint,
                                      SrcRectConstraint constraint) {
    this->append<SkRecords::DrawImageRect>(
            this->copy(paint), sk_ref_sp(image), src, dst, sampling, constraint);
}

void SkRecordCanvas::onDrawImageLattice2(const SkImage* image,
                                         const Lattice& lattice,
                                         const SkRect& dst,
                                         SkFilterMode filter,
                                         const SkPaint* paint) {
    int flagCount = skia_rs_record_lattice_flag_count(
            lattice.fRectTypes != nullptr, lattice.fXCount, lattice.fYCount);
    SkASSERT(lattice.fBounds);
    this->append<SkRecords::DrawImageLattice>(this->copy(paint),
                                              sk_ref_sp(image),
                                              lattice.fXCount,
                                              this->copy(lattice.fXDivs, lattice.fXCount),
                                              lattice.fYCount,
                                              this->copy(lattice.fYDivs, lattice.fYCount),
                                              flagCount,
                                              this->copy(lattice.fRectTypes, flagCount),
                                              this->copy(lattice.fColors, flagCount),
                                              *lattice.fBounds,
                                              dst,
                                              filter);
}

void SkRecordCanvas::onDrawTextBlob(const SkTextBlob* blob,
                                    SkScalar x,
                                    SkScalar y,
                                    const SkPaint& paint) {
    this->append<SkRecords::DrawTextBlob>(paint, sk_ref_sp(blob), x, y);
}

void SkRecordCanvas::onDrawSlug(const sktext::gpu::Slug* slug, const SkPaint& paint) {
    this->append<SkRecords::DrawSlug>(paint, sk_ref_sp(slug));
}

void SkRecordCanvas::onDrawGlyphRunList(const sktext::GlyphRunList& glyphRunList,
                                        const SkPaint& paint) {
    sk_sp<SkTextBlob> blob = sk_ref_sp(glyphRunList.blob());
    if (glyphRunList.blob() == nullptr) {
        blob = glyphRunList.makeBlob();
    }
    this->onDrawTextBlob(blob.get(), glyphRunList.origin().x(), glyphRunList.origin().y(), paint);
}

void SkRecordCanvas::onDrawPicture(const SkPicture* pic,
                                   const SkMatrix* matrix,
                                   const SkPaint* paint) {
    fApproxBytesUsedBySubPictures += pic->approximateBytesUsed();
    this->append<SkRecords::DrawPicture>(
            this->copy(paint), sk_ref_sp(pic), matrix ? *matrix : SkMatrix::I());
}

void SkRecordCanvas::onDrawVerticesObject(const SkVertices* vertices,
                                          SkBlendMode bmode,
                                          const SkPaint& paint) {
    this->append<SkRecords::DrawVertices>(
            paint, sk_ref_sp(const_cast<SkVertices*>(vertices)), bmode);
}

void SkRecordCanvas::onDrawMesh(const SkMesh& mesh,
                                sk_sp<SkBlender> blender,
                                const SkPaint& paint) {
    this->append<SkRecords::DrawMesh>(paint, mesh, std::move(blender));
}

void SkRecordCanvas::onDrawPatch(const SkPoint cubics[12],
                                 const SkColor colors[4],
                                 const SkPoint texCoords[4],
                                 SkBlendMode bmode,
                                 const SkPaint& paint) {
    this->append<SkRecords::DrawPatch>(
            paint,
            cubics ? this->copy(cubics, SkPatchUtils::kNumCtrlPts) : nullptr,
            colors ? this->copy(colors, SkPatchUtils::kNumCorners) : nullptr,
            texCoords ? this->copy(texCoords, SkPatchUtils::kNumCorners) : nullptr,
            bmode);
}

void SkRecordCanvas::onDrawAtlas2(const SkImage* atlas,
                                  const SkRSXform xform[],
                                  const SkRect tex[],
                                  const SkColor colors[],
                                  int count,
                                  SkBlendMode mode,
                                  const SkSamplingOptions& sampling,
                                  const SkRect* cull,
                                  const SkPaint* paint) {
    this->append<SkRecords::DrawAtlas>(this->copy(paint),
                                       sk_ref_sp(atlas),
                                       this->copy(xform, count),
                                       this->copy(tex, count),
                                       this->copy(colors, count),
                                       (unsigned)count,
                                       mode,
                                       sampling,
                                       this->copy(cull));
}

void SkRecordCanvas::onDrawShadowRec(const SkPath& path, const SkDrawShadowRec& rec) {
    this->append<SkRecords::DrawShadowRec>(path, rec);
}

void SkRecordCanvas::onDrawAnnotation(const SkRect& rect, const char key[], SkData* value) {
    this->append<SkRecords::DrawAnnotation>(rect, SkString(key), sk_ref_sp(value));
}

void SkRecordCanvas::onDrawEdgeAAQuad(const SkRect& rect,
                                      const SkPoint clip[4],
                                      QuadAAFlags aa,
                                      const SkColor4f& color,
                                      SkBlendMode mode) {
    this->append<SkRecords::DrawEdgeAAQuad>(rect, this->copy(clip, 4), aa, color, mode);
}

void SkRecordCanvas::onDrawEdgeAAImageSet2(const ImageSetEntry set[],
                                           int count,
                                           const SkPoint dstClips[],
                                           const SkMatrix preViewMatrices[],
                                           const SkSamplingOptions& sampling,
                                           const SkPaint* paint,
                                           SrcRectConstraint constraint) {
    int totalDstClipCount, totalMatrixCount;
    SkCanvasPriv::GetDstClipAndMatrixCounts(set, count, &totalDstClipCount, &totalMatrixCount);

    AutoTArray<ImageSetEntry> setCopy(count);
    for (int i = 0; i < count; ++i) {
        setCopy[i] = set[i];
    }

    this->append<SkRecords::DrawEdgeAAImageSet>(this->copy(paint),
                                                std::move(setCopy),
                                                count,
                                                this->copy(dstClips, totalDstClipCount),
                                                this->copy(preViewMatrices, totalMatrixCount),
                                                sampling,
                                                constraint);
}

void SkRecordCanvas::willSave() { this->append<SkRecords::Save>(); }

SkCanvas::SaveLayerStrategy SkRecordCanvas::getSaveLayerStrategy(const SaveLayerRec& rec) {
    AutoTArray<sk_sp<SkImageFilter>> filters(rec.fFilters.size());
    for (size_t i = 0; i < rec.fFilters.size(); ++i) {
        filters[i] = rec.fFilters[i];
    }

    this->append<SkRecords::SaveLayer>(this->copy(rec.fBounds),
                                       this->copy(rec.fPaint),
                                       sk_ref_sp(rec.fBackdrop),
                                       rec.fSaveLayerFlags,
                                       SkCanvasPriv::GetBackdropScaleFactor(rec),
                                       rec.fBackdropTileMode,
                                       std::move(filters));
    return SkCanvas::kNoLayer_SaveLayerStrategy;
}

bool SkRecordCanvas::onDoSaveBehind(const SkRect* subset) {
    this->append<SkRecords::SaveBehind>(this->copy(subset));
    return false;
}

void SkRecordCanvas::didRestore() { this->append<SkRecords::Restore>(this->getTotalMatrix()); }

void SkRecordCanvas::didConcat44(const SkM44& m) { this->append<SkRecords::Concat44>(m); }

void SkRecordCanvas::didSetM44(const SkM44& m) { this->append<SkRecords::SetM44>(m); }

void SkRecordCanvas::didScale(SkScalar sx, SkScalar sy) { this->append<SkRecords::Scale>(sx, sy); }

void SkRecordCanvas::didTranslate(SkScalar dx, SkScalar dy) {
    this->append<SkRecords::Translate>(dx, dy);
}

void SkRecordCanvas::onClipRect(const SkRect& rect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    INHERITED(onClipRect, rect, op, edgeStyle);
    SkRecords::ClipOpAndAA opAA(op, kSoft_ClipEdgeStyle == edgeStyle);
    this->append<SkRecords::ClipRect>(rect, opAA);
}

void SkRecordCanvas::onClipRRect(const SkRRect& rrect, SkClipOp op, ClipEdgeStyle edgeStyle) {
    INHERITED(onClipRRect, rrect, op, edgeStyle);
    SkRecords::ClipOpAndAA opAA(op, kSoft_ClipEdgeStyle == edgeStyle);
    this->append<SkRecords::ClipRRect>(rrect, opAA);
}

void SkRecordCanvas::onClipPath(const SkPath& path, SkClipOp op, ClipEdgeStyle edgeStyle) {
    INHERITED(onClipPath, path, op, edgeStyle);
    SkRecords::ClipOpAndAA opAA(op, kSoft_ClipEdgeStyle == edgeStyle);
    this->append<SkRecords::ClipPath>(path, opAA);
}

void SkRecordCanvas::onClipShader(sk_sp<SkShader> cs, SkClipOp op) {
    INHERITED(onClipShader, cs, op);
    this->append<SkRecords::ClipShader>(std::move(cs), op);
}

void SkRecordCanvas::onClipRegion(const SkRegion& deviceRgn, SkClipOp op) {
    INHERITED(onClipRegion, deviceRgn, op);
    this->append<SkRecords::ClipRegion>(deviceRgn, op);
}

void SkRecordCanvas::onResetClip() {
    INHERITED(onResetClip);
    this->append<SkRecords::ResetClip>();
}

sk_sp<SkSurface> SkRecordCanvas::onNewSurface(const SkImageInfo&, const SkSurfaceProps&) {
    return nullptr;
}

#undef INHERITED

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkRecordDraw (src/core/SkRecordDraw.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

void SkRecordDraw(const SkRecord& record,
                  SkCanvas* canvas,
                  SkPicture const* const drawablePicts[],
                  SkDrawable* const drawables[],
                  int drawableCount,
                  const SkBBoxHierarchy* bbh,
                  SkPicture::AbortCallback* callback) {
    SkAutoCanvasRestore saveRestore(canvas, true);

    if (bbh) {
        SkRect query = canvas->getLocalClipBounds();
        std::vector<int> ops;
        bbh->search(query, &ops);

        SkRecords::Draw draw(canvas, drawablePicts, drawables, drawableCount);
        for (int i = 0; i < (int)ops.size(); i++) {
            if (callback && callback->abort()) {
                return;
            }
            record.visit(ops[i], draw);
        }
    } else {
        SkRecords::Draw draw(canvas, drawablePicts, drawables, drawableCount);
        for (int i = 0; i < record.count(); i++) {
            if (callback && callback->abort()) {
                return;
            }
            record.visit(i, draw);
        }
    }
}

namespace SkRecords {

template <> void Draw::draw(const NoOp&) {}

#define DRAW(T, call) template <> void Draw::draw(const T& r) { fCanvas->call; }
DRAW(Restore, restore())
DRAW(Save, save())
DRAW(SaveLayer,
     saveLayer(SkCanvasPriv::ScaledBackdropLayer(
               r.bounds,
               r.paint,
               r.backdrop.get(),
               r.backdropScale,
               r.backdropTileMode,
               r.saveLayerFlags,
               SkCanvas::FilterSpan{const_cast<sk_sp<SkImageFilter>*>(r.filters.data()),
                                    r.filters.size()})))

template <> void Draw::draw(const SaveBehind& r) {
    SkCanvasPriv::SaveBehind(fCanvas, r.subset);
}

template <> void Draw::draw(const DrawBehind& r) {
    SkCanvasPriv::DrawBehind(fCanvas, r.paint);
}

DRAW(SetMatrix, setMatrix(fInitialCTM.asM33() * r.matrix))
DRAW(SetM44, setMatrix(fInitialCTM * r.matrix))
DRAW(Concat44, concat(r.matrix))
DRAW(Concat, concat(r.matrix))
DRAW(Translate, translate(r.dx, r.dy))
DRAW(Scale, scale(r.sx, r.sy))

DRAW(ClipPath, clipPath(r.path, r.opAA.op(), r.opAA.aa()))
DRAW(ClipRRect, clipRRect(r.rrect, r.opAA.op(), r.opAA.aa()))
DRAW(ClipRect, clipRect(r.rect, r.opAA.op(), r.opAA.aa()))
DRAW(ClipRegion, clipRegion(r.region, r.op))
DRAW(ClipShader, clipShader(r.shader, r.op))

template <> void Draw::draw(const ResetClip&) {
    SkCanvasPriv::ResetClip(fCanvas);
}

DRAW(DrawArc, drawArc(r.oval, r.startAngle, r.sweepAngle, r.useCenter, r.paint))
DRAW(DrawDRRect, drawDRRect(r.outer, r.inner, r.paint))
DRAW(DrawImage, drawImage(r.image.get(), r.left, r.top, r.sampling, r.paint))

template <> void Draw::draw(const DrawImageLattice& r) {
    SkCanvas::Lattice lattice;
    lattice.fXCount = r.xCount;
    lattice.fXDivs = r.xDivs;
    lattice.fYCount = r.yCount;
    lattice.fYDivs = r.yDivs;
    lattice.fRectTypes = (0 == r.flagCount) ? nullptr : r.flags;
    lattice.fColors = (0 == r.flagCount) ? nullptr : r.colors;
    lattice.fBounds = &r.src;
    fCanvas->drawImageLattice(r.image.get(), lattice, r.dst, r.filter, r.paint);
}

DRAW(DrawImageRect, drawImageRect(r.image.get(), r.src, r.dst, r.sampling, r.paint, r.constraint))
DRAW(DrawOval, drawOval(r.oval, r.paint))
DRAW(DrawPaint, drawPaint(r.paint))
DRAW(DrawPath, drawPath(r.path, r.paint))
DRAW(DrawPatch, drawPatch(r.cubics, r.colors, r.texCoords, r.bmode, r.paint))
DRAW(DrawPicture, drawPicture(r.picture.get(), &r.matrix, r.paint))
DRAW(DrawPoints, drawPoints(r.mode, {r.pts.data(), r.count}, r.paint))
DRAW(DrawRRect, drawRRect(r.rrect, r.paint))
DRAW(DrawRect, drawRect(r.rect, r.paint))
DRAW(DrawRegion, drawRegion(r.region, r.paint))
DRAW(DrawTextBlob, drawTextBlob(r.blob.get(), r.x, r.y, r.paint))
DRAW(DrawSlug, drawSlug(r.slug.get(), r.paint))
DRAW(DrawAtlas, drawAtlas(r.atlas.get(),
                          {r.xforms.data(), r.count},
                          {r.texs.data(), r.count},
                          {r.colors.data(), r.colors ? r.count : 0},
                          r.mode, r.sampling, r.cull, r.paint))
DRAW(DrawVertices, drawVertices(r.vertices, r.bmode, r.paint))
DRAW(DrawMesh, drawMesh(r.mesh, r.blender, r.paint))
DRAW(DrawShadowRec, private_draw_shadow_rec(r.path, r.rec))
DRAW(DrawAnnotation, drawAnnotation(r.rect, r.key.c_str(), r.value.get()))

DRAW(DrawEdgeAAQuad, experimental_DrawEdgeAAQuad(r.rect, r.clip, r.aa, r.color, r.mode))
DRAW(DrawEdgeAAImageSet, experimental_DrawEdgeAAImageSet(
        r.set.get(), r.count, r.dstClips, r.preViewMatrices, r.sampling, r.paint, r.constraint))

#undef DRAW

template <> void Draw::draw(const DrawDrawable& r) {
    SkASSERT(r.index >= 0);
    SkASSERT(r.index < fDrawableCount);
    if (fDrawables) {
        SkASSERT(nullptr == fDrawablePicts);
        fCanvas->drawDrawable(fDrawables[r.index], r.matrix);
    } else {
        fCanvas->drawPicture(fDrawablePicts[r.index], r.matrix, nullptr);
    }
}

namespace {

bool adjust_paint_cb(void*, size_t paint_token, SkRect* rect) {
    const SkPaint* paint = reinterpret_cast<const SkPaint*>(paint_token);
    if (paint) {
        if (paint->canComputeFastBounds()) {
            *rect = paint->computeFastBounds(*rect, rect);
            return true;
        }
        return false;
    }
    return true;
}

bool paint_may_affect_transparent_black(const SkPaint* paint) {
    if (!paint) {
        return false;
    }
    bool ifAffects = paint->getImageFilter() &&
                     as_IFB(paint->getImageFilter())->affectsTransparentBlack();
    bool cfAffects = paint->getColorFilter() &&
                     as_CFB(paint->getColorFilter())->affectsTransparentBlack();
    auto bm = paint->asBlendMode();
    int32_t blendMode = bm ? static_cast<int32_t>(bm.value()) : -1;
    return skia_rs_fill_bounds_paint_may_affect_transparent_black(
            true, ifAffects, cfAffects, blendMode);
}

class RustFillBoundsVisitor : SkNoncopyable {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    RustFillBoundsVisitor(RsFillBoundsState* state) : fState(state) {}

    void operator()(const Save&) {
        skia_rs_fill_bounds_push_save_block(fState, 0, false, false);
    }
    void operator()(const SaveLayer& op) {
        const SkPaint* paint = op.paint;
        skia_rs_fill_bounds_push_save_block(
                fState,
                reinterpret_cast<size_t>(paint),
                op.backdrop != nullptr,
                paint_may_affect_transparent_black(paint));
    }
    void operator()(const SaveBehind&) {
        skia_rs_fill_bounds_push_save_block(fState, 0, false, false);
    }
    void operator()(const Restore& op) {
        skia_rs_fill_bounds_track_restore(fState, &op.matrix);
    }

    void operator()(const SetMatrix& op) {
        skia_rs_fill_bounds_update_ctm_set_matrix(fState, &op.matrix);
        skia_rs_fill_bounds_push_control(fState);
    }
    void operator()(const SetM44& op) {
        skia_rs_fill_bounds_update_ctm_set_m44(fState, &op.matrix);
        skia_rs_fill_bounds_push_control(fState);
    }
    void operator()(const Concat& op) {
        skia_rs_fill_bounds_update_ctm_concat(fState, &op.matrix);
        skia_rs_fill_bounds_push_control(fState);
    }
    void operator()(const Concat44& op) {
        skia_rs_fill_bounds_update_ctm_concat44(fState, &op.matrix);
        skia_rs_fill_bounds_push_control(fState);
    }
    void operator()(const Scale& op) {
        skia_rs_fill_bounds_update_ctm_scale(fState, op.sx, op.sy);
        skia_rs_fill_bounds_push_control(fState);
    }
    void operator()(const Translate& op) {
        skia_rs_fill_bounds_update_ctm_translate(fState, op.dx, op.dy);
        skia_rs_fill_bounds_push_control(fState);
    }
    void operator()(const ClipRect&)   { skia_rs_fill_bounds_push_control(fState); }
    void operator()(const ClipRRect&)  { skia_rs_fill_bounds_push_control(fState); }
    void operator()(const ClipPath&)   { skia_rs_fill_bounds_push_control(fState); }
    void operator()(const ClipRegion&) { skia_rs_fill_bounds_push_control(fState); }
    void operator()(const ClipShader&) { skia_rs_fill_bounds_push_control(fState); }
    void operator()(const ResetClip&)  { skia_rs_fill_bounds_push_control(fState); }

    void operator()(const NoOp&)       { skia_rs_fill_bounds_record_noop(fState); }
    void operator()(const DrawPaint&)  { skia_rs_fill_bounds_record_cull_rect_draw(fState); }
    void operator()(const DrawBehind&) { skia_rs_fill_bounds_record_cull_rect_draw(fState); }

    void operator()(const DrawRect& op) { this->recordRect(op.rect, &op.paint); }
    void operator()(const DrawRegion& op) {
        this->recordRect(SkRect::Make(op.region.getBounds()), &op.paint);
    }
    void operator()(const DrawOval& op) { this->recordRect(op.oval, &op.paint); }
    void operator()(const DrawArc& op)  { this->recordRect(op.oval, &op.paint); }
    void operator()(const DrawRRect& op) { this->recordRect(op.rrect.rect(), &op.paint); }
    void operator()(const DrawDRRect& op) { this->recordRect(op.outer.rect(), &op.paint); }
    void operator()(const DrawImage& op) {
        const SkImage* image = op.image.get();
        SkRect rect = SkRect::MakeXYWH(op.left, op.top, image->width(), image->height());
        this->recordRect(rect, op.paint);
    }
    void operator()(const DrawImageLattice& op) { this->recordRect(op.dst, op.paint); }
    void operator()(const DrawImageRect& op)    { this->recordRect(op.dst, op.paint); }
    void operator()(const DrawPath& op) {
        if (op.path.isInverseFillType()) {
            skia_rs_fill_bounds_record_cull_rect_draw(fState);
        } else {
            this->recordRect(op.path.getBounds(), &op.paint);
        }
    }
    void operator()(const DrawPoints& op) {
        skia_rs_fill_bounds_record_points_draw(
                fState,
                op.pts.data(),
                op.count,
                op.paint.getStrokeWidth(),
                reinterpret_cast<size_t>(&op.paint),
                adjust_paint_cb,
                nullptr);
    }
    void operator()(const DrawPatch& op) {
        SkRect dst = SkRect::BoundsOrEmpty({op.cubics.data(), (size_t)SkPatchUtils::kNumCtrlPts});
        this->recordRect(dst, &op.paint);
    }
    void operator()(const DrawVertices& op) { this->recordRect(op.vertices->bounds(), &op.paint); }
    void operator()(const DrawMesh& op)     { this->recordRect(op.mesh.bounds(), &op.paint); }
    void operator()(const DrawAtlas& op) {
        if (op.cull) {
            this->recordRect(*op.cull, op.paint);
        } else {
            skia_rs_fill_bounds_record_cull_rect_draw(fState);
        }
    }
    void operator()(const DrawShadowRec& op) {
        SkMatrix ctm;
        skia_rs_fill_bounds_get_ctm(fState, &ctm);
        SkRect bounds;
        SkDrawShadowMetrics::GetLocalBounds(op.path, op.rec, ctm, &bounds);
        this->recordRect(bounds, nullptr);
    }
    void operator()(const DrawPicture& op) {
        skia_rs_fill_bounds_record_picture_draw(
                fState,
                op.picture->cullRect(),
                &op.matrix,
                reinterpret_cast<size_t>(static_cast<const SkPaint*>(op.paint)),
                adjust_paint_cb,
                nullptr);
    }
    void operator()(const DrawTextBlob& op) {
        skia_rs_fill_bounds_record_text_blob_draw(
                fState,
                op.blob->bounds(),
                op.x,
                op.y,
                reinterpret_cast<size_t>(&op.paint),
                adjust_paint_cb,
                nullptr);
    }
    void operator()(const DrawSlug& op) {
        this->recordRect(op.slug->sourceBoundsWithOrigin(), &op.paint);
    }
    void operator()(const DrawDrawable& op) { this->recordRect(op.worstCaseBounds, nullptr); }
    void operator()(const DrawAnnotation& op) { this->recordRect(op.rect, nullptr); }
    void operator()(const DrawEdgeAAQuad& op) {
        skia_rs_fill_bounds_record_edge_aa_quad_draw(
                fState, op.rect, op.clip.data(), adjust_paint_cb, nullptr);
    }
    void operator()(const DrawEdgeAAImageSet& op) {
        AutoSTArray<8, RsEdgeAAImageSetEntryInfo> entries(op.count);
        int totalClips = 0;
        int maxMatIdx = -1;
        for (int i = 0; i < op.count; ++i) {
            entries[i].dst_rect = op.set[i].fDstRect;
            entries[i].matrix_index = op.set[i].fMatrixIndex;
            entries[i].has_clip = op.set[i].fHasClip;
            if (op.set[i].fHasClip) {
                totalClips += 4;
            }
            if (op.set[i].fMatrixIndex > maxMatIdx) {
                maxMatIdx = op.set[i].fMatrixIndex;
            }
        }
        skia_rs_fill_bounds_record_edge_aa_image_set_draw(
                fState,
                entries.get(),
                SkToSizeT(op.count),
                op.dstClips.data(),
                SkToSizeT(totalClips),
                op.preViewMatrices.data(),
                SkToSizeT(maxMatIdx + 1),
                adjust_paint_cb,
                nullptr);
    }

private:
    void recordRect(const SkRect& rect, const SkPaint* paint) {
        skia_rs_fill_bounds_record_rect_draw(
                fState, rect, reinterpret_cast<size_t>(paint), adjust_paint_cb, nullptr);
    }

    RsFillBoundsState* fState;
};

}  // namespace
}  // namespace SkRecords

void SkRecordFillBounds(const SkRect& cullRect,
                        const SkRecord& record,
                        SkRect bounds[],
                        SkBBoxHierarchy::Metadata meta[]) {
    const int count = record.count();
    RsFillBoundsState* state = skia_rs_fill_bounds_create(cullRect, SkToSizeT(count));
    SkRecords::RustFillBoundsVisitor visitor(state);
    for (int i = 0; i < count; i++) {
        skia_rs_fill_bounds_set_current_op(state, SkToSizeT(i));
        record.visit(i, visitor);
    }
    skia_rs_fill_bounds_finish_and_destroy(
            state, bounds, reinterpret_cast<bool*>(meta), SkToSizeT(count));
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkRecordOpts (src/core/SkRecordOpts.cpp)
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

struct OpTypeExtractor {
    template <typename T>
    uint32_t operator()(const T&) const {
        return static_cast<uint32_t>(T::kType);
    }
};

RsPaintOptInfo summarize_paint(const SkPaint* p) {
    RsPaintOptInfo info{};
    if (!p) {
        return info;
    }
    info.present = true;
    info.is_src_over = p->isSrcOver();
    info.is_src_mode = (p->asBlendMode() == SkBlendMode::kSrc);
    info.has_shader = (p->getShader() != nullptr);
    info.has_path_effect = (p->getPathEffect() != nullptr);
    info.has_mask_filter = (p->getMaskFilter() != nullptr);
    info.has_color_filter = (p->getColorFilter() != nullptr);
    info.has_image_filter = (p->getImageFilter() != nullptr);
    info.alpha = p->getAlpha();
    info.color = p->getColor();
    return info;
}

void collect_op_types(const SkRecord& record, AutoSTMalloc<64, uint32_t>& opTypes) {
    const int count = record.count();
    OpTypeExtractor extractor;
    for (int i = 0; i < count; ++i) {
        opTypes[i] = record.visit(i, extractor);
    }
}

}  // namespace

void SkRecordNoopSaveRestores(SkRecord* record) {
    const int count = record->count();
    if (count == 0) {
        return;
    }
    AutoSTMalloc<64, uint32_t> opTypes(count);
    AutoSTMalloc<64, bool> noopMask(count);
    collect_op_types(*record, opTypes);

    skia_rs_record_opts_noop_save_restores(opTypes.get(), noopMask.get(), SkToSizeT(count));
    for (int i = 0; i < count; ++i) {
        if (noopMask[i]) {
            record->replace<SkRecords::NoOp>(i);
        }
    }
}

#ifndef SK_BUILD_FOR_ANDROID_FRAMEWORK
void SkRecordNoopSaveLayerDrawRestores(SkRecord* record) {
    const int count = record->count();
    if (count < 3) {
        return;
    }
    AutoSTMalloc<64, uint32_t> opTypes(count);
    collect_op_types(*record, opTypes);

    const size_t maxMatches = SkToSizeT(count / 3);
    AutoSTMalloc<32, int32_t> begins(maxMatches);
    const size_t numMatches = skia_rs_record_opts_find_save_layer_draw_restore(
            opTypes.get(), SkToSizeT(count), begins.get(), maxMatches);

    for (size_t m = 0; m < numMatches; ++m) {
        const int begin = begins[m];
        SkRecords::Is<SkRecords::SaveLayer> getSL;
        SkRecords::IsSingleDraw getDraw;
        if (!record->mutate(begin, getSL) || !record->mutate(begin + 1, getDraw)) {
            continue;
        }
        SkRecords::SaveLayer* sl = getSL.get();
        SkPaint* drawPaint = getDraw.get();
        RsPaintOptInfo lp = summarize_paint(sl->paint);
        RsPaintOptInfo dp = summarize_paint(drawPaint);
        RsLayerFoldDecision dec = skia_rs_record_opts_eval_save_layer_draw_restore(
                sl->backdrop != nullptr, !sl->filters.empty(), &lp, &dp);
        if (dec.kill_save_layer_and_restore) {
            if (dec.update_paint_alpha && drawPaint) {
                drawPaint->setAlpha(dec.new_alpha);
            }
            record->replace<SkRecords::NoOp>(begin);
            record->replace<SkRecords::NoOp>(begin + 2);
        }
    }
}
#endif

void SkRecordMergeSvgOpacityAndFilterLayers(SkRecord* record) {
    const int count = record->count();
    if (count < 7) {
        return;
    }
    AutoSTMalloc<64, uint32_t> opTypes(count);
    collect_op_types(*record, opTypes);

    const size_t maxMatches = SkToSizeT(count / 7);
    AutoSTMalloc<16, int32_t> begins(maxMatches);
    const size_t numMatches = skia_rs_record_opts_find_svg_opacity_filter(
            opTypes.get(), SkToSizeT(count), begins.get(), maxMatches);

    for (size_t m = 0; m < numMatches; ++m) {
        const int begin = begins[m];
        SkRecords::Is<SkRecords::SaveLayer> getOuter;
        SkRecords::Is<SkRecords::SaveLayer> getInner;
        if (!record->mutate(begin, getOuter) || !record->mutate(begin + 3, getInner)) {
            continue;
        }
        SkRecords::SaveLayer* outer = getOuter.get();
        SkRecords::SaveLayer* inner = getInner.get();
        SkPaint* filterLayerPaint = inner->paint;
        RsPaintOptInfo op = summarize_paint(outer->paint);
        RsPaintOptInfo fp = summarize_paint(filterLayerPaint);
        RsLayerFoldDecision dec = skia_rs_record_opts_eval_svg_opacity_filter(
                outer->backdrop != nullptr,
                !outer->filters.empty(),
                !inner->filters.empty(),
                &op,
                &fp);
        if (dec.kill_save_layer_and_restore) {
            if (dec.update_paint_alpha && filterLayerPaint) {
                filterLayerPaint->setAlpha(dec.new_alpha);
            }
            record->replace<SkRecords::NoOp>(begin);
            record->replace<SkRecords::NoOp>(begin + 6);
        }
    }
}

void SkRecordOptimize(SkRecord* record) {
#ifndef SK_BUILD_FOR_ANDROID_FRAMEWORK
    SkRecordNoopSaveLayerDrawRestores(record);
#endif
    SkRecordMergeSvgOpacityAndFilterLayers(record);
    record->defrag();
}
