/*
 * Safe Rust bridge for:
 *   src/core/SkDrawable.cpp
 *   src/core/SkRecordedDrawable.cpp
 *   src/core/SkAnnotation.cpp
 *   src/core/SkLatticeIter.cpp
 *   src/image/SkPictureImageGenerator.cpp
 * (Phase 1 Layer 5.2 skia-rs-picture, GN target :core).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkAnnotation.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageGenerator.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPictureRecorder.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSize.h"
#include "include/private/SkAssert.h"
#include "include/private/SkTo.h"
#include "src/core/SkAnnotationKeys.h"
#include "src/core/SkLatticeIter.h"
#include "src/core/SkPictureData.h"
#include "src/core/SkPicturePlayback.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkPictureRecord.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkRecordDraw.h"
#include "src/core/SkRecordedDrawable.h"
#include "src/core/SkTLazy.h"
#include "src/core/SkWriteBuffer.h"
#include "src/image/SkImageGeneratorPriv.h"
#include "src/image/SkPictureImageGenerator.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>

static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkColor) == 4);
static_assert(sizeof(SkCanvas::Lattice::RectType) == 1);

namespace {

struct RsLatticePlan;

struct RsLatticeNextOut {
    SkIRect src;
    SkRect dst;
    bool is_fixed_color;
    uint32_t fixed_color;
};

}  // namespace

extern "C" {
// Lattice
bool skia_rs_lattice_valid(int32_t width,
                           int32_t height,
                           const SkIRect* bounds,
                           const int32_t* x_divs,
                           int32_t x_count,
                           const int32_t* y_divs,
                           int32_t y_count);
bool skia_rs_lattice_valid_nine(int32_t width, int32_t height, const SkIRect* center);
RsLatticePlan* skia_rs_lattice_plan_create(const SkIRect* bounds,
                                           const int32_t* x_divs,
                                           int32_t x_count,
                                           const int32_t* y_divs,
                                           int32_t y_count,
                                           const uint8_t* rect_types,
                                           const uint32_t* colors,
                                           const SkRect* dst);
RsLatticePlan* skia_rs_lattice_plan_create_nine(
        int32_t w, int32_t h, const SkIRect* center, const SkRect* dst);
void skia_rs_lattice_plan_info(const RsLatticePlan* plan,
                               size_t* out_x_len,
                               size_t* out_y_len,
                               size_t* out_rt_len,
                               int32_t* out_num_rects,
                               int32_t* out_num_to_draw);
void skia_rs_lattice_plan_copy(const RsLatticePlan* plan,
                               int32_t* out_src_x,
                               float* out_dst_x,
                               int32_t* out_src_y,
                               float* out_dst_y,
                               uint8_t* out_rect_types,
                               uint32_t* out_colors);
void skia_rs_lattice_plan_destroy(RsLatticePlan* plan);
bool skia_rs_lattice_next(const int32_t* src_x,
                          size_t src_x_len,
                          const int32_t* src_y,
                          size_t src_y_len,
                          const float* dst_x,
                          const float* dst_y,
                          const uint8_t* rect_types,
                          size_t rect_types_len,
                          const uint32_t* colors,
                          int32_t num_rects_in_lattice,
                          int32_t* curr_x,
                          int32_t* curr_y,
                          RsLatticeNextOut* out);
void skia_rs_lattice_map_dst_scale_translate(
        float* dst_x, size_t dst_x_len, float* dst_y, size_t dst_y_len, const SkMatrix* matrix);

// Drawable / RecordedDrawable / Annotation
uint32_t skia_rs_drawable_next_generation_id();
const char* skia_rs_annotation_key_url();
const char* skia_rs_annotation_key_define_named_dest();
const char* skia_rs_annotation_key_link_named_dest();
SkRect skia_rs_annotate_named_dest_rect(SkPoint point);
size_t skia_rs_recorded_drawable_approx_bytes(size_t self_size,
                                              size_t record_bytes,
                                              size_t bbh_bytes,
                                              size_t drawables_bytes);

// PictureImageGenerator
int32_t skia_rs_picture_image_gen_validate(
        bool has_picture, bool has_color_space, int32_t width, int32_t height, bool is_f16);
}  // extern "C"

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkAnnotation
///////////////////////////////////////////////////////////////////////////////////////////////////

const char* SkAnnotationKeys::URL_Key() { return skia_rs_annotation_key_url(); }

const char* SkAnnotationKeys::Define_Named_Dest_Key() {
    return skia_rs_annotation_key_define_named_dest();
}

const char* SkAnnotationKeys::Link_Named_Dest_Key() {
    return skia_rs_annotation_key_link_named_dest();
}

void SkAnnotateRectWithURL(SkCanvas* canvas, const SkRect& rect, SkData* value) {
    if (nullptr == value) {
        return;
    }
    canvas->drawAnnotation(rect, SkAnnotationKeys::URL_Key(), value);
}

void SkAnnotateNamedDestination(SkCanvas* canvas, const SkPoint& point, SkData* name) {
    if (nullptr == name) {
        return;
    }
    const SkRect rect = skia_rs_annotate_named_dest_rect(point);
    canvas->drawAnnotation(rect, SkAnnotationKeys::Define_Named_Dest_Key(), name);
}

void SkAnnotateLinkToDestination(SkCanvas* canvas, const SkRect& rect, SkData* name) {
    if (nullptr == name) {
        return;
    }
    canvas->drawAnnotation(rect, SkAnnotationKeys::Link_Named_Dest_Key(), name);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkDrawable
///////////////////////////////////////////////////////////////////////////////////////////////////

SkDrawable::SkDrawable() : fGenerationID(0) {}

void SkDrawable::draw(SkCanvas* canvas, const SkMatrix* matrix) {
    SkAutoCanvasRestore acr(canvas, true);
    if (matrix) {
        canvas->concat(*matrix);
    }
    this->onDraw(canvas);
}

void SkDrawable::draw(SkCanvas* canvas, SkScalar x, SkScalar y) {
    SkMatrix matrix = SkMatrix::Translate(x, y);
    this->draw(canvas, &matrix);
}

sk_sp<SkPicture> SkDrawable::makePictureSnapshot() { return this->onMakePictureSnapshot(); }

uint32_t SkDrawable::getGenerationID() {
    if (0 == fGenerationID) {
        fGenerationID = skia_rs_drawable_next_generation_id();
    }
    return fGenerationID;
}

SkRect SkDrawable::getBounds() { return this->onGetBounds(); }

size_t SkDrawable::approximateBytesUsed() { return this->onApproximateBytesUsed(); }

size_t SkDrawable::onApproximateBytesUsed() { return 0; }

void SkDrawable::notifyDrawingChanged() { fGenerationID = 0; }

sk_sp<SkPicture> SkDrawable::onMakePictureSnapshot() {
    SkPictureRecorder recorder;
    const SkRect bounds = this->getBounds();
    SkCanvas* canvas = recorder.beginRecording(bounds);
    this->draw(canvas);
    return recorder.finishRecordingAsPicture();
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkRecordedDrawable
///////////////////////////////////////////////////////////////////////////////////////////////////

size_t SkRecordedDrawable::onApproximateBytesUsed() {
    size_t drawablesSize = 0;
    if (fDrawableList) {
        for (auto&& drawable : *fDrawableList) {
            drawablesSize += drawable->approximateBytesUsed();
        }
    }
    return skia_rs_recorded_drawable_approx_bytes(sizeof(*this),
                                                  fRecord ? fRecord->bytesUsed() : 0,
                                                  fBBH ? fBBH->bytesUsed() : 0,
                                                  drawablesSize);
}

void SkRecordedDrawable::onDraw(SkCanvas* canvas) {
    SkDrawable* const* drawables = nullptr;
    int drawableCount = 0;
    if (fDrawableList) {
        drawables = fDrawableList->begin();
        drawableCount = fDrawableList->count();
    }
    SkRecordDraw(*fRecord, canvas, nullptr, drawables, drawableCount, fBBH.get(), nullptr);
}

sk_sp<SkPicture> SkRecordedDrawable::onMakePictureSnapshot() {
    std::unique_ptr<SkSnapshotArray> pictList{
            fDrawableList ? fDrawableList->newDrawableSnapshot() : nullptr};

    size_t subPictureBytes = 0;
    for (int i = 0; pictList && i < pictList->count(); i++) {
        subPictureBytes += pictList->begin()[i]->approximateBytesUsed();
    }

    return SkPicturePriv::MakePicture(
            fBounds, fRecord, std::move(pictList), fBBH, subPictureBytes);
}

void SkRecordedDrawable::flatten(SkWriteBuffer& buffer) const {
    buffer.writeRect(fBounds);

    SkPictInfo info;
    SkPictureRecord pictureRecord(SkISize::Make(fBounds.width(), fBounds.height()), 0);

    SkBBoxHierarchy* bbh;
    if (pictureRecord.getLocalClipBounds().contains(fBounds)) {
        bbh = nullptr;
    } else {
        bbh = fBBH.get();
    }

    SkDrawable* const* drawables = fDrawableList ? fDrawableList->begin() : nullptr;
    int drawableCount = fDrawableList ? fDrawableList->count() : 0;
    pictureRecord.beginRecording();
    SkRecordDraw(*fRecord, &pictureRecord, nullptr, drawables, drawableCount, bbh, nullptr);
    pictureRecord.endRecording();

    SkPictureData pictureData(pictureRecord, info);
    pictureData.flatten(buffer);
}

sk_sp<SkFlattenable> SkRecordedDrawable::CreateProc(SkReadBuffer& buffer) {
    if (!buffer.isValid()) {
        return nullptr;
    }
    SkRect bounds;
    buffer.readRect(&bounds);

    SkPictInfo info;
    info.setVersion(buffer.getVersion());
    info.fCullRect = bounds;
    std::unique_ptr<SkPictureData> pictureData(SkPictureData::CreateFromBuffer(buffer, info));
    if (!pictureData) {
        return nullptr;
    }

    SkPicturePlayback playback(pictureData.get());
    SkPictureRecorder recorder;
    playback.draw(recorder.beginRecording(bounds), nullptr, &buffer);
    return recorder.finishRecordingAsDrawable();
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkLatticeIter
///////////////////////////////////////////////////////////////////////////////////////////////////

bool SkLatticeIter::Valid(int width, int height, const SkCanvas::Lattice& lattice) {
    SkASSERT(lattice.fBounds);
    return skia_rs_lattice_valid(width,
                                 height,
                                 lattice.fBounds,
                                 lattice.fXDivs,
                                 lattice.fXCount,
                                 lattice.fYDivs,
                                 lattice.fYCount);
}

bool SkLatticeIter::Valid(int width, int height, const SkIRect& center) {
    return skia_rs_lattice_valid_nine(width, height, &center);
}

SkLatticeIter::SkLatticeIter(const SkCanvas::Lattice& lattice, const SkRect& dst) {
    SkASSERT(lattice.fBounds);
    RsLatticePlan* plan = skia_rs_lattice_plan_create(
            lattice.fBounds,
            lattice.fXDivs,
            lattice.fXCount,
            lattice.fYDivs,
            lattice.fYCount,
            reinterpret_cast<const uint8_t*>(lattice.fRectTypes),
            lattice.fColors,
            &dst);
    size_t xLen = 0, yLen = 0, rtLen = 0;
    skia_rs_lattice_plan_info(plan, &xLen, &yLen, &rtLen, &fNumRectsInLattice, &fNumRectsToDraw);
    fSrcX.reset(SkToInt(xLen));
    fDstX.reset(SkToInt(xLen));
    fSrcY.reset(SkToInt(yLen));
    fDstY.reset(SkToInt(yLen));
    if (rtLen > 0) {
        fRectTypes.push_back_n(SkToInt(rtLen));
        fColors.push_back_n(SkToInt(rtLen));
    }
    skia_rs_lattice_plan_copy(
            plan,
            fSrcX.begin(),
            fDstX.begin(),
            fSrcY.begin(),
            fDstY.begin(),
            rtLen > 0 ? reinterpret_cast<uint8_t*>(fRectTypes.begin()) : nullptr,
            rtLen > 0 ? fColors.begin() : nullptr);
    skia_rs_lattice_plan_destroy(plan);
    fCurrX = 0;
    fCurrY = 0;
}

SkLatticeIter::SkLatticeIter(int w, int h, const SkIRect& c, const SkRect& dst) {
    SkASSERT(SkIRect::MakeWH(w, h).contains(c));
    RsLatticePlan* plan = skia_rs_lattice_plan_create_nine(w, h, &c, &dst);
    size_t xLen = 0, yLen = 0, rtLen = 0;
    skia_rs_lattice_plan_info(plan, &xLen, &yLen, &rtLen, &fNumRectsInLattice, &fNumRectsToDraw);
    fSrcX.reset(SkToInt(xLen));
    fDstX.reset(SkToInt(xLen));
    fSrcY.reset(SkToInt(yLen));
    fDstY.reset(SkToInt(yLen));
    skia_rs_lattice_plan_copy(
            plan, fSrcX.begin(), fDstX.begin(), fSrcY.begin(), fDstY.begin(), nullptr, nullptr);
    skia_rs_lattice_plan_destroy(plan);
    fCurrX = 0;
    fCurrY = 0;
}

bool SkLatticeIter::next(SkIRect* src, SkRect* dst, bool* isFixedColor, SkColor* fixedColor) {
    RsLatticeNextOut out{};
    if (!skia_rs_lattice_next(
                fSrcX.begin(),
                SkToSizeT(fSrcX.size()),
                fSrcY.begin(),
                SkToSizeT(fSrcY.size()),
                fDstX.begin(),
                fDstY.begin(),
                fRectTypes.empty() ? nullptr : reinterpret_cast<const uint8_t*>(fRectTypes.begin()),
                SkToSizeT(fRectTypes.size()),
                fColors.empty() ? nullptr : fColors.begin(),
                fNumRectsInLattice,
                &fCurrX,
                &fCurrY,
                &out)) {
        return false;
    }
    *src = out.src;
    *dst = out.dst;
    if (isFixedColor && fixedColor) {
        *isFixedColor = out.is_fixed_color;
        if (out.is_fixed_color) {
            *fixedColor = out.fixed_color;
        }
    }
    return true;
}

void SkLatticeIter::mapDstScaleTranslate(const SkMatrix& matrix) {
    SkASSERT(matrix.isScaleTranslate());
    skia_rs_lattice_map_dst_scale_translate(
            fDstX.begin(), SkToSizeT(fDstX.size()), fDstY.begin(), SkToSizeT(fDstY.size()), &matrix);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkPictureImageGenerator
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace SkImageGenerators {
std::unique_ptr<SkImageGenerator> MakeFromPicture(const SkISize& size,
                                                  sk_sp<SkPicture> picture,
                                                  const SkMatrix* matrix,
                                                  const SkPaint* paint,
                                                  SkImages::BitDepth bitDepth,
                                                  sk_sp<SkColorSpace> colorSpace) {
    return MakeFromPicture(size, std::move(picture), matrix, paint, bitDepth, std::move(colorSpace), {});
}

std::unique_ptr<SkImageGenerator> MakeFromPicture(const SkISize& size,
                                                  sk_sp<SkPicture> picture,
                                                  const SkMatrix* matrix,
                                                  const SkPaint* paint,
                                                  SkImages::BitDepth bitDepth,
                                                  sk_sp<SkColorSpace> colorSpace,
                                                  SkSurfaceProps props) {
    int32_t ct = skia_rs_picture_image_gen_validate(picture != nullptr,
                                                    colorSpace != nullptr,
                                                    size.width(),
                                                    size.height(),
                                                    SkImages::BitDepth::kF16 == bitDepth);
    if (ct < 0) {
        return nullptr;
    }

    SkColorType colorType = (ct == 1) ? kRGBA_F16_SkColorType : kN32_SkColorType;
    SkImageInfo info =
            SkImageInfo::Make(size, colorType, kPremul_SkAlphaType, std::move(colorSpace));
    return std::unique_ptr<SkImageGenerator>(
            new SkPictureImageGenerator(info, std::move(picture), matrix, paint, props));
}
}  // namespace SkImageGenerators

SkPictureImageGenerator::SkPictureImageGenerator(const SkImageInfo& info,
                                                 sk_sp<SkPicture> picture,
                                                 const SkMatrix* matrix,
                                                 const SkPaint* paint,
                                                 const SkSurfaceProps& props)
        : SkImageGenerator(info), fPicture(std::move(picture)), fProps(props) {
    if (matrix) {
        fMatrix = *matrix;
    } else {
        fMatrix.reset();
    }
    if (paint) {
        fPaint = *paint;
    }
}

bool SkPictureImageGenerator::onGetPixels(const SkImageInfo& info,
                                          void* pixels,
                                          size_t rowBytes,
                                          const Options& opts) {
    std::unique_ptr<SkCanvas> canvas = SkCanvas::MakeRasterDirect(info, pixels, rowBytes, &fProps);
    if (!canvas) {
        return false;
    }
    canvas->clear(0);
    canvas->drawPicture(fPicture, &fMatrix, SkOptAddressOrNull(fPaint));
    return true;
}
