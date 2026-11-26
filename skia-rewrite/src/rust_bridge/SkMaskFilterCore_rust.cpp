/*
 * Safe Rust bridge for:
 *   src/core/SkMaskFilter.cpp
 *   src/core/SkMaskFilterBase.cpp
 *   src/core/SkBlurMaskFilterImpl.cpp
 *   src/core/SkMaskBlurFilter.cpp
 * (Phase 1 Layer 4.3 skia-rs-effects, GN target :core).
 */

#include "include/core/SkBlendMode.h"
#include "include/core/SkBlurTypes.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRRect.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkRegion.h"
#include "include/core/SkScalar.h"
#include "include/core/SkStrokeRec.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkImageFilters.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkTemplates.h"
#include "src/core/SkAutoMalloc.h"
#include "src/core/SkBlitter.h"
#include "src/core/SkBlitter_A8.h"
#include "src/core/SkBlurMask.h"
#include "src/core/SkBlurMaskFilterImpl.h"
#include "src/core/SkCachedData.h"
#include "src/core/SkDraw.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskBlurFilter.h"
#include "src/core/SkMaskCache.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkTLazy.h"
#include "src/core/SkWriteBuffer.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <utility>

struct SkDeserialProcs;

namespace {

struct RsRRectNinePatchPlan {
    bool valid;
    uint8_t pad[3];
    int32_t total_small_width;
    int32_t total_small_height;
    SkIPoint center;
};

struct RsRectsNinePatchPlan {
    bool valid;
    uint8_t pad[3];
    SkIPoint center;
    SkRect small_r0;
    SkRect small_r1;
};

struct RsNinePatchLayout {
    SkIRect corners_src[4];
    SkIPoint corners_dst_xy[4];
    SkIRect inner_r;
    SkIRect top_r;
    SkIRect bottom_r;
    SkIRect left_r;
    SkIRect right_r;
};

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkIPoint) == 8);
static_assert(sizeof(SkVector) == 8);
static_assert(sizeof(SkV2) == 8);
static_assert(sizeof(RsRRectNinePatchPlan) == 20);
static_assert(sizeof(RsRectsNinePatchPlan) == 44);
static_assert(sizeof(RsNinePatchLayout) == 176);

}  // namespace

extern "C" {
bool skia_rs_blur_mf_is_valid_sigma(float sigma);
float skia_rs_blur_mf_compute_device_sigma(float sigma, bool ignore_xform, const SkMatrix* ctm);
SkV2 skia_rs_blur_mf_compute_local_sigma(float sigma, bool ignore_xform, const SkMatrix* ctm);
SkV2 skia_rs_blur_mf_as_image_filter_sigma(float sigma, bool ignore_xform, const SkMatrix* ctm);
SkRect skia_rs_blur_mf_compute_fast_bounds(SkRect src, float sigma);
bool skia_rs_blur_mf_rect_exceeds(SkRect r, float v);
RsRRectNinePatchPlan skia_rs_blur_mf_plan_rrect_nine_patch(SkRect rect,
                                                           const SkVector* radii,
                                                           SkIPoint margin);
RsRectsNinePatchPlan skia_rs_blur_mf_plan_rects_nine_patch(SkRect r0,
                                                           bool has_r1,
                                                           SkRect r1,
                                                           SkIRect src_bounds,
                                                           SkIRect dst_bounds);
RsNinePatchLayout skia_rs_mask_filter_compute_nine_patch_layout(SkIRect mask_bounds,
                                                                SkIRect outer_r,
                                                                SkIPoint center);
double skia_rs_mask_blur_pin_sigma(double sigma);
bool skia_rs_mask_blur_has_no_blur(double sigma_w, double sigma_h);
SkIPoint skia_rs_mask_blur_compute_border(double sigma_w, double sigma_h);
SkIPoint skia_rs_mask_blur_pixels(double sigma_w,
                                  double sigma_h,
                                  uint8_t src_format,
                                  const uint8_t* src,
                                  size_t src_len,
                                  size_t src_rb,
                                  size_t src_w,
                                  size_t src_h,
                                  uint8_t* dst,
                                  size_t dst_len,
                                  size_t dst_rb,
                                  size_t dst_w,
                                  size_t dst_h);
}  // extern "C"

// ============================================================================
// SkMaskFilter (src/core/SkMaskFilter.cpp)
// ============================================================================

void SkMaskFilter::RegisterFlattenables() {
    sk_register_blur_maskfilter_createproc();
}

sk_sp<SkMaskFilter> SkMaskFilter::Deserialize(const void* data, size_t size,
                                              const SkDeserialProcs* procs) {
    return sk_sp<SkMaskFilter>(static_cast<SkMaskFilter*>(
            SkFlattenable::Deserialize(kSkMaskFilter_Type, data, size, procs).release()));
}

// ============================================================================
// SkMaskFilterBase (src/core/SkMaskFilterBase.cpp)
// ============================================================================

SkMaskFilterBase::NinePatch::NinePatch(const SkMask& mask,
                                       SkIRect outerRect,
                                       SkIPoint center,
                                       sk_sp<SkCachedData> cache)
        : fMask(mask), fOuterRect(outerRect), fCenter(center), fCache(std::move(cache)) {}

SkMaskFilterBase::NinePatch::NinePatch(NinePatch&&) = default;

SkMaskFilterBase::NinePatch::~NinePatch() {
    if (fCache) {
        SkASSERT((const void*)fMask.fImage == fCache->data());
    } else {
        SkMaskBuilder::FreeImage(const_cast<uint8_t*>(fMask.fImage));
    }
}

bool SkMaskFilterBase::asABlur(BlurRec*) const {
    return false;
}

std::pair<sk_sp<SkImageFilter>, bool> SkMaskFilterBase::asImageFilter(const SkMatrix&,
                                                                      const SkPaint&) const {
    return std::make_pair(nullptr, false);
}

static SkMask extract_mask_subset(const SkMask& src, SkIRect bounds, int32_t newX, int32_t newY) {
    SkASSERT(src.fBounds.contains(bounds));
    const int dx = bounds.left() - src.fBounds.left();
    const int dy = bounds.top() - src.fBounds.top();
    bounds.offsetTo(newX, newY);
    return SkMask(src.fImage + dy * src.fRowBytes + dx,
                  bounds,
                  src.fRowBytes,
                  src.fFormat);
}

static void blit_clipped_mask(SkBlitter* blitter, const SkMask& mask,
                              const SkIRect& bounds, const SkIRect& clipR) {
    SkIRect r;
    if (r.intersect(bounds, clipR)) {
        blitter->blitMask(mask, r);
    }
}

static void blit_clipped_rect(SkBlitter* blitter, const SkIRect& rect, const SkIRect& clipR) {
    SkIRect r;
    if (r.intersect(rect, clipR)) {
        blitter->blitRect(r.left(), r.top(), r.width(), r.height());
    }
}

static void draw_nine_clipped(const SkMask& mask, const SkIRect& outerR,
                              const SkIPoint& center, bool fillCenter,
                              const SkIRect& clipR, SkBlitter* blitter) {
    const RsNinePatchLayout layout =
            skia_rs_mask_filter_compute_nine_patch_layout(mask.fBounds, outerR, center);
    const int cx = center.x();
    const int cy = center.y();

    for (int i = 0; i < 4; ++i) {
        const SkIRect& bounds = layout.corners_src[i];
        if (bounds.width() > 0 && bounds.height() > 0) {
            SkMask m = extract_mask_subset(
                    mask, bounds, layout.corners_dst_xy[i].x(), layout.corners_dst_xy[i].y());
            blit_clipped_mask(blitter, m, m.fBounds, clipR);
        }
    }

    if (fillCenter) {
        blit_clipped_rect(blitter, layout.inner_r, clipR);
    }

    const int innerW = layout.inner_r.width();
    size_t storageSize = (innerW + 1) * (sizeof(int16_t) + sizeof(uint8_t));
    SkAutoSMalloc<4 * 1024> storage(storageSize);
    int16_t* runs = (int16_t*)storage.get();
    uint8_t* alpha = (uint8_t*)(runs + innerW + 1);

    SkIRect r = layout.top_r;
    if (r.intersect(clipR)) {
        const int startY = std::max(0, r.top() - outerR.top());
        const int stopY = startY + r.height();
        const int width = r.width();
        runs[0] = width;
        runs[width] = 0;
        for (int y = startY; y < stopY; ++y) {
            alpha[0] = *mask.getAddr8(cx, mask.fBounds.top() + y);
            blitter->blitAntiH(r.left(), outerR.top() + y, alpha, runs);
        }
    }

    r = layout.bottom_r;
    if (r.intersect(clipR)) {
        const int startY = outerR.bottom() - r.bottom();
        const int stopY = startY + r.height();
        const int width = r.width();
        runs[0] = width;
        runs[width] = 0;
        for (int y = startY; y < stopY; ++y) {
            alpha[0] = *mask.getAddr8(cx, mask.fBounds.bottom() - y - 1);
            blitter->blitAntiH(r.left(), outerR.bottom() - y - 1, alpha, runs);
        }
    }

    r = layout.left_r;
    if (r.intersect(clipR)) {
        SkMask leftMask(mask.getAddr8(mask.fBounds.left() + r.left() - outerR.left(),
                                      mask.fBounds.top() + cy),
                        r,
                        0,
                        SkMask::kA8_Format);
        blitter->blitMask(leftMask, r);
    }

    r = layout.right_r;
    if (r.intersect(clipR)) {
        SkMask rightMask(mask.getAddr8(mask.fBounds.right() - outerR.right() + r.left(),
                                       mask.fBounds.top() + cy),
                         r,
                         0,
                         SkMask::kA8_Format);
        blitter->blitMask(rightMask, r);
    }
}

static void draw_nine(const SkMask& mask, const SkIRect& outerR, const SkIPoint& center,
                      bool fillCenter, const SkRasterClip& clip, SkBlitter* blitter) {
    SkAAClipBlitterWrapper wrapper(clip, blitter);
    blitter = wrapper.getBlitter();

    SkRegion::Cliperator clipper(wrapper.getRgn(), outerR);
    if (!clipper.done()) {
        const SkIRect& cr = clipper.rect();
        do {
            draw_nine_clipped(mask, outerR, center, fillCenter, cr, blitter);
            clipper.next();
        } while (!clipper.done());
    }
}

static int countNestedRects(const SkPathRaw& raw, SkRect rects[2]) {
    if (SkPathPriv::IsNestedFillRects(raw, rects)) {
        return 2;
    }
    if (auto r = raw.isRect()) {
        rects[0] = *r;
        return 1;
    }
    return 0;
}

bool SkMaskFilterBase::filterRRect(const SkRRect& devRRect,
                                   const SkMatrix& matrix,
                                   const SkRasterClip& clip,
                                   SkBlitter* blitter,
                                   SkResourceCache* cache) const {
    std::optional<NinePatch> patch =
            this->filterRRectToNine(devRRect, matrix, clip.getBounds(), cache);
    if (!patch.has_value()) {
        return false;
    }
    draw_nine(patch->fMask, patch->fOuterRect, patch->fCenter, true, clip, blitter);
    return true;
}

SkMaskFilterBase::FilterReturn SkMaskFilterBase::filterRects(SkSpan<const SkRect> devRects,
                                                             const SkMatrix& matrix,
                                                             const SkRasterClip& clip,
                                                             SkBlitter* blitter,
                                                             SkResourceCache* cache) const {
    std::optional<NinePatch> patch;
    FilterReturn filterReturn =
            this->filterRectsToNine(devRects, matrix, clip.getBounds(), &patch, cache);
    switch (filterReturn) {
        case FilterReturn::kFalse:
            SkASSERT(!patch.has_value());
            break;
        case FilterReturn::kTrue:
            draw_nine(patch->fMask, patch->fOuterRect, patch->fCenter, 1 == devRects.size(), clip,
                      blitter);
            break;
        case FilterReturn::kUnimplemented:
            SkASSERT(!patch.has_value());
            break;
    }
    return filterReturn;
}

bool SkMaskFilterBase::filterPath(const SkPathRaw& devRaw,
                                  const SkMatrix& matrix,
                                  const SkRasterClip& clip,
                                  SkBlitter* blitter,
                                  SkStrokeRec::InitStyle style,
                                  SkResourceCache* cache) const {
    SkRect rects[2];
    int rectCount = 0;
    if (SkStrokeRec::kFill_InitStyle == style) {
        rectCount = countNestedRects(devRaw, rects);
    }
    if (rectCount > 0) {
        switch (this->filterRects(SkSpan(rects, rectCount), matrix, clip, blitter, cache)) {
            case FilterReturn::kFalse:
                return false;
            case FilterReturn::kTrue:
                return true;
            case FilterReturn::kUnimplemented:
                break;
        }
    }

    SkMaskBuilder srcM, dstM;
#if defined(SK_BUILD_FOR_FUZZER)
    if (devRaw.verbs().size() > 1000 || devRaw.points().size() > 1000) {
        return false;
    }
#endif
    if (!skcpu::DrawToMask(devRaw,
                           clip.getBounds(),
                           this,
                           &matrix,
                           &srcM,
                           SkMaskBuilder::kComputeBoundsAndRenderImage_CreateMode,
                           style)) {
        return false;
    }
    SkAutoMaskFreeImage autoSrc(srcM.image());

    if (!this->filterMask(&dstM, srcM, matrix, nullptr)) {
        return false;
    }
    SkAutoMaskFreeImage autoDst(dstM.image());

    SkAAClipBlitterWrapper wrapper(clip, blitter);
    blitter = wrapper.getBlitter();

    SkRegion::Cliperator clipper(wrapper.getRgn(), dstM.fBounds);
    if (!clipper.done()) {
        const SkIRect& cr = clipper.rect();
        do {
            blitter->blitMask(dstM, cr);
            clipper.next();
        } while (!clipper.done());
    }
    return true;
}

std::optional<SkMaskFilterBase::NinePatch> SkMaskFilterBase::filterRRectToNine(
        const SkRRect&, const SkMatrix&, const SkIRect&, SkResourceCache*) const {
    return std::nullopt;
}

SkMaskFilterBase::FilterReturn SkMaskFilterBase::filterRectsToNine(SkSpan<const SkRect>,
                                                                   const SkMatrix&,
                                                                   const SkIRect&,
                                                                   std::optional<NinePatch>*,
                                                                   SkResourceCache*) const {
    return FilterReturn::kUnimplemented;
}

void SkMaskFilterBase::computeFastBounds(const SkRect& src, SkRect* dst) const {
    SkMask srcM(nullptr, src.roundOut(), 0, SkMask::kA8_Format);
    SkMaskBuilder dstM;
    SkIPoint margin;
    if (this->filterMask(&dstM, srcM, SkMatrix::I(), &margin)) {
        dst->set(dstM.fBounds);
    } else {
        dst->set(srcM.fBounds);
    }
}

// ============================================================================
// SkBlurMaskFilterImpl (src/core/SkBlurMaskFilterImpl.cpp)
// ============================================================================

SkBlurMaskFilterImpl::SkBlurMaskFilterImpl(SkScalar sigma, SkBlurStyle style, bool respectCTM)
    : fSigma(sigma)
    , fBlurStyle(style)
    , fRespectCTM(respectCTM) {
    SkASSERT(fSigma > 0);
    SkASSERT((unsigned)style <= kLastEnum_SkBlurStyle);
}

SkMask::Format SkBlurMaskFilterImpl::getFormat() const {
    return SkMask::kA8_Format;
}

bool SkBlurMaskFilterImpl::asABlur(BlurRec* rec) const {
    if (this->ignoreXform()) {
        return false;
    }
    if (rec) {
        rec->fSigma = fSigma;
        rec->fStyle = fBlurStyle;
    }
    return true;
}

std::pair<sk_sp<SkImageFilter>, bool> SkBlurMaskFilterImpl::asImageFilter(const SkMatrix& ctm,
                                                                          const SkPaint&) const {
    SkV2 sigma = skia_rs_blur_mf_as_image_filter_sigma(fSigma, this->ignoreXform(), &ctm);
    sk_sp<SkImageFilter> filter = SkImageFilters::Blur(sigma.x, sigma.y, nullptr);
    switch (fBlurStyle) {
        case kInner_SkBlurStyle:
            return {SkImageFilters::Blend(SkBlendMode::kDstIn, std::move(filter), nullptr), false};
        case kSolid_SkBlurStyle:
            return {SkImageFilters::Blend(SkBlendMode::kSrcOver, std::move(filter), nullptr), false};
        case kOuter_SkBlurStyle:
            return {SkImageFilters::Blend(SkBlendMode::kDstOut, std::move(filter), nullptr), false};
        case kNormal_SkBlurStyle:
            return {filter, false};
    }
    SkUNREACHABLE;
}

SkScalar SkBlurMaskFilterImpl::computeXformedDeviceSigma(const SkMatrix& ctm) const {
    return skia_rs_blur_mf_compute_device_sigma(fSigma, this->ignoreXform(), &ctm);
}

SkV2 SkBlurMaskFilterImpl::computeXformedLocalSigma(const SkMatrix& ctm) const {
    return skia_rs_blur_mf_compute_local_sigma(fSigma, this->ignoreXform(), &ctm);
}

bool SkBlurMaskFilterImpl::filterMask(SkMaskBuilder* dst, const SkMask& src,
                                      const SkMatrix& matrix,
                                      SkIPoint* margin) const {
    SkScalar sigma = this->computeXformedDeviceSigma(matrix);
    return SkBlurMask::BoxBlur(dst, src, sigma, fBlurStyle, margin);
}

bool SkBlurMaskFilterImpl::filterRectMask(SkMaskBuilder* dst, const SkRect& r,
                                          const SkMatrix& matrix,
                                          SkIPoint* margin,
                                          SkMaskBuilder::CreateMode createMode) const {
    SkScalar sigma = this->computeXformedDeviceSigma(matrix);
    return SkBlurMask::BlurRect(sigma, dst, r, fBlurStyle, margin, createMode);
}

static bool prepare_to_draw_into_mask(const SkRect& bounds, SkMaskBuilder* mask) {
    SkASSERT(mask != nullptr);
    mask->bounds() = bounds.roundOut();
    mask->rowBytes() = SkAlign4(mask->fBounds.width());
    mask->format() = SkMask::kA8_Format;
    const size_t size = mask->computeImageSize();
    if (size == 0) {
        return false;
    }
    mask->image() = SkMaskBuilder::AllocImage(size, SkMaskBuilder::kZeroInit_Alloc);
    return mask->fImage != nullptr;
}

template <typename Proc>
static bool draw_into_mask(SkMaskBuilder* mask, const SkRect& bounds, Proc proc) {
    if (!prepare_to_draw_into_mask(bounds, mask)) {
        return false;
    }

    const int dx = mask->fBounds.fLeft;
    const int dy = mask->fBounds.fTop;
    SkRasterClip rclip(mask->fBounds);
    rclip.setRect(mask->fBounds.makeOffset(-dx, -dy));

    SkASSERT(mask->fFormat == SkMask::kA8_Format);
    auto info = SkImageInfo::MakeA8(mask->fBounds.width(), mask->fBounds.height());
    auto pm = SkPixmap(info, mask->fImage, mask->fRowBytes);

    SkMatrix ctm = SkMatrix::Translate(-SkIntToScalar(dx), -SkIntToScalar(dy));

    skcpu::Draw draw;
    draw.fBlitterChooser = SkA8Blitter_Choose;
    draw.fCTM = &ctm;
    draw.fDst = pm;
    draw.fRC = &rclip;

    SkPaint paint;
    paint.setAntiAlias(true);
    proc(draw, paint);
    return true;
}

static bool draw_rects_into_mask(SkSpan<const SkRect> rects, SkMaskBuilder* mask) {
    SkASSERT(rects.size() == 1 || rects.size() == 2);
    return draw_into_mask(mask, rects[0], [&](skcpu::Draw& draw, const SkPaint& paint) {
        if (rects.size() == 1) {
            draw.drawRect(rects[0], paint);
        } else {
            SkPath path = SkPathBuilder().addRect(rects[0])
                                         .addRect(rects[1])
                                         .setFillType(SkPathFillType::kEvenOdd)
                                         .detach();
            draw.drawPath(path, paint, nullptr);
        }
    });
}

static bool draw_rrect_into_mask(const SkRRect& rrect, SkMaskBuilder* mask) {
    return draw_into_mask(mask, rrect.rect(), [&](skcpu::Draw& draw, const SkPaint& paint) {
        draw.drawRRect(rrect, paint);
    });
}

static sk_sp<SkCachedData> copy_mask_to_cacheddata(SkMaskBuilder* mask, SkResourceCache* cache) {
    const size_t size = mask->computeTotalImageSize();
    sk_sp<SkCachedData> data =
            cache ? cache->newCachedData(size) : SkResourceCache::NewCachedData(size);
    if (data) {
        memcpy(data->writable_data(), mask->fImage, size);
        SkMaskBuilder::FreeImage(mask->image());
        mask->image() = (uint8_t*)data->writable_data();
    }
    return data;
}

std::optional<SkMaskFilterBase::NinePatch> SkBlurMaskFilterImpl::filterRRectToNine(
        const SkRRect& rrect,
        const SkMatrix& matrix,
        const SkIRect& clipBounds,
        SkResourceCache* cache) const {
    switch (rrect.getType()) {
        case SkRRect::kEmpty_Type:
            return std::nullopt;
        case SkRRect::kRect_Type:
            SkDEBUGFAIL("Should use a different special case");
            return std::nullopt;
        case SkRRect::kOval_Type:
            return std::nullopt;
        case SkRRect::kSimple_Type:
        case SkRRect::kNinePatch_Type:
        case SkRRect::kComplex_Type:
            break;
    }

    if (kInner_SkBlurStyle == fBlurStyle) {
        return std::nullopt;
    }
    if (skia_rs_blur_mf_rect_exceeds(rrect.rect(), SkIntToScalar(32767))) {
        return std::nullopt;
    }

    SkIVector margin;
    SkMaskBuilder srcM(nullptr, rrect.rect().roundOut(), 0, SkMask::kA8_Format), dstM;
    if (!this->filterMask(&dstM, srcM, matrix, &margin)) {
        return std::nullopt;
    }

    RsRRectNinePatchPlan plan =
            skia_rs_blur_mf_plan_rrect_nine_patch(rrect.rect(), rrect.radii().data(), margin);
    if (!plan.valid) {
        return std::nullopt;
    }

    SkRect smallR = SkRect::MakeWH(plan.total_small_width, plan.total_small_height);
    SkRRect smallRR;
    smallRR.setRectRadii(smallR, rrect.radii().data());

    const float sigma = this->computeXformedDeviceSigma(matrix);
    SkTLazy<SkMask> cachedMask;
    sk_sp<SkCachedData> cached =
            SkMaskCache::Find(sigma, fBlurStyle, smallRR, &cachedMask, cache);
    if (!cached) {
        if (!draw_rrect_into_mask(smallRR, &srcM)) {
            return std::nullopt;
        }
        SkAutoMaskFreeImage amf(srcM.image());

        SkMaskBuilder filterM;
        if (!this->filterMask(&filterM, srcM, matrix, nullptr)) {
            return std::nullopt;
        }
        SkASSERT(filterM.fBounds.width() == (srcM.fBounds.width() + 2 * margin.fX));
        SkASSERT(filterM.fBounds.height() == (srcM.fBounds.height() + 2 * margin.fY));

        cached = copy_mask_to_cacheddata(&filterM, cache);
        if (cached) {
            SkMaskCache::Add(sigma, fBlurStyle, smallRR, filterM, cached, cache);
        }
        cachedMask.init(filterM);
    }

    SkIRect bounds = cachedMask->fBounds;
    bounds.offsetTo(0, 0);
    return std::optional<SkMaskFilterBase::NinePatch>(
            std::in_place,
            SkMask{cachedMask->fImage, bounds, cachedMask->fRowBytes, cachedMask->fFormat},
            dstM.fBounds,
            plan.center,
            std::move(cached));
}

SkMaskFilterBase::FilterReturn SkBlurMaskFilterImpl::filterRectsToNine(
        SkSpan<const SkRect> rects,
        const SkMatrix& matrix,
        const SkIRect& clipBounds,
        std::optional<NinePatch>* patch,
        SkResourceCache* cache) const {
    SkASSERT(patch != nullptr);
    SkASSERT(rects.size() == 1 || rects.size() == 2);

    if (kInner_SkBlurStyle == fBlurStyle || kOuter_SkBlurStyle == fBlurStyle) {
        return FilterReturn::kUnimplemented;
    }
    if (skia_rs_blur_mf_rect_exceeds(rects[0], SkIntToScalar(32767))) {
        return FilterReturn::kUnimplemented;
    }

    SkIPoint margin;
    SkMaskBuilder srcM(nullptr, rects[0].roundOut(), 0, SkMask::kA8_Format), dstM;

    bool filterResult = false;
    if (rects.size() == 1) {
        filterResult = this->filterRectMask(&dstM, rects[0], matrix, &margin,
                                            SkMaskBuilder::kJustComputeBounds_CreateMode);
    } else {
        filterResult = this->filterMask(&dstM, srcM, matrix, &margin);
    }
    if (!filterResult) {
        return FilterReturn::kFalse;
    }

    const bool hasSecond = (rects.size() == 2);
    const SkRect r1 = hasSecond ? rects[1] : SkRect::MakeEmpty();
    RsRectsNinePatchPlan plan = skia_rs_blur_mf_plan_rects_nine_patch(
            rects[0], hasSecond, r1, srcM.fBounds, dstM.fBounds);
    if (!plan.valid) {
        return FilterReturn::kUnimplemented;
    }

    SkRect smallR[2] = {plan.small_r0, plan.small_r1};
    int rectCount = hasSecond ? 2 : 1;
    if (rectCount == 2) {
        SkASSERT(!smallR[1].isEmpty());
    }

    const SkScalar sigma = this->computeXformedDeviceSigma(matrix);
    SkTLazy<SkMask> cachedMask;
    SkSpan<const SkRect> smallRects = SkSpan(smallR, rectCount);
    sk_sp<SkCachedData> cached =
            SkMaskCache::Find(sigma, fBlurStyle, smallRects, &cachedMask, cache);
    if (!cached) {
        SkMaskBuilder filterM;
        if (rectCount == 2) {
            if (!draw_rects_into_mask(smallRects, &srcM)) {
                return FilterReturn::kFalse;
            }
            SkAutoMaskFreeImage amf(srcM.image());
            if (!this->filterMask(&filterM, srcM, matrix, nullptr)) {
                return FilterReturn::kFalse;
            }
        } else {
            if (!this->filterRectMask(&filterM, smallR[0], matrix, nullptr,
                                      SkMaskBuilder::kComputeBoundsAndRenderImage_CreateMode)) {
                return FilterReturn::kFalse;
            }
        }
        cached = copy_mask_to_cacheddata(&filterM, cache);
        if (cached) {
            SkMaskCache::Add(sigma, fBlurStyle, smallRects, filterM, cached, cache);
        }
        cachedMask.init(filterM);
    }

    SkIRect bounds = cachedMask->fBounds;
    bounds.offsetTo(0, 0);
    patch->emplace(SkMask{cachedMask->fImage, bounds, cachedMask->fRowBytes, cachedMask->fFormat},
                   dstM.fBounds,
                   plan.center,
                   std::move(cached));
    return FilterReturn::kTrue;
}

void SkBlurMaskFilterImpl::computeFastBounds(const SkRect& src, SkRect* dst) const {
    *dst = skia_rs_blur_mf_compute_fast_bounds(src, fSigma);
}

sk_sp<SkFlattenable> SkBlurMaskFilterImpl::CreateProc(SkReadBuffer& buffer) {
    const SkScalar sigma = buffer.readScalar();
    SkBlurStyle style = buffer.read32LE(kLastEnum_SkBlurStyle);
    uint32_t flags = buffer.read32LE(0x3);
    bool respectCTM = !(flags & 1);
    return SkMaskFilter::MakeBlur((SkBlurStyle)style, sigma, respectCTM);
}

void SkBlurMaskFilterImpl::flatten(SkWriteBuffer& buffer) const {
    buffer.writeScalar(fSigma);
    buffer.writeUInt(fBlurStyle);
    buffer.writeUInt(!fRespectCTM);
}

void sk_register_blur_maskfilter_createproc() { SK_REGISTER_FLATTENABLE(SkBlurMaskFilterImpl); }

sk_sp<SkMaskFilter> SkMaskFilter::MakeBlur(SkBlurStyle style, SkScalar sigma, bool respectCTM) {
    if (skia_rs_blur_mf_is_valid_sigma(sigma)) {
        return sk_sp<SkMaskFilter>(new SkBlurMaskFilterImpl(sigma, style, respectCTM));
    }
    return nullptr;
}

// ============================================================================
// SkMaskBlurFilter (src/core/SkMaskBlurFilter.cpp)
// ============================================================================

SkMaskBlurFilter::SkMaskBlurFilter(double sigmaW, double sigmaH)
    : fSigmaW{skia_rs_mask_blur_pin_sigma(sigmaW)}
    , fSigmaH{skia_rs_mask_blur_pin_sigma(sigmaH)} {
    SkASSERT(sigmaW >= 0);
    SkASSERT(sigmaH >= 0);
}

bool SkMaskBlurFilter::hasNoBlur() const {
    return skia_rs_mask_blur_has_no_blur(fSigmaW, fSigmaH);
}

SkIPoint SkMaskBlurFilter::blur(const SkMask& src, SkMaskBuilder* dst) const {
    const SkIPoint border = skia_rs_mask_blur_compute_border(fSigmaW, fSigmaH);
    SkASSERT(border.x() >= 0 && border.y() >= 0);

    *dst = SkMaskBuilder::PrepareDestination(border.x(), border.y(), src);
    if (src.fImage == nullptr) {
        return border;
    }
    if (dst->fImage == nullptr) {
        dst->bounds().setEmpty();
        return {0, 0};
    }

    const int srcW = src.fBounds.width();
    const int srcH = src.fBounds.height();
    const int dstW = dst->fBounds.width();
    const int dstH = dst->fBounds.height();
    if (srcW <= 0 || srcH <= 0 || dstW <= 0 || dstH <= 0) {
        return {0, 0};
    }

    const size_t srcLen = src.computeImageSize();
    const size_t dstLen = dst->computeImageSize();
    return skia_rs_mask_blur_pixels(fSigmaW,
                                    fSigmaH,
                                    static_cast<uint8_t>(src.fFormat),
                                    src.fImage,
                                    srcLen,
                                    src.fRowBytes,
                                    static_cast<size_t>(srcW),
                                    static_cast<size_t>(srcH),
                                    dst->image(),
                                    dstLen,
                                    dst->fRowBytes,
                                    static_cast<size_t>(dstW),
                                    static_cast<size_t>(dstH));
}
