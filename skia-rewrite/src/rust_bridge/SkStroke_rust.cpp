/*
 * Safe Rust bridge for Layer 2.2 skia-rs-path (Stroking & PathUtils):
 *   SkStroke.cpp, SkStrokeRec.cpp, SkStrokerPriv.cpp, SkPathUtils.cpp.
 */

#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPathUtils.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkScalar.h"
#include "include/core/SkStrokeRec.h"
#include "include/private/SkAssert.h"
#include "include/private/SkTo.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkPaintDefaults.h"
#include "src/core/SkPathEnums.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkStroke.h"
#include "src/core/SkStrokerPriv.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>

struct SkiaRsPathBuf;

struct SkiaRsPathBufMeta {
    uint8_t fill_type;
    uint8_t convexity;
    uint8_t segment_mask;
    uint8_t is_a_type;
    uint8_t is_a_dir;
    uint8_t is_a_start_index;
    bool is_volatile;
    uint8_t _pad;
    int32_t last_move_index;
};

extern "C" {
const SkPoint* skia_rs_path_buf_points(const SkiaRsPathBuf* buf, size_t* out_len);
const uint8_t* skia_rs_path_buf_verbs(const SkiaRsPathBuf* buf, size_t* out_len);
const float* skia_rs_path_buf_conics(const SkiaRsPathBuf* buf, size_t* out_len);
SkiaRsPathBufMeta skia_rs_path_buf_meta(const SkiaRsPathBuf* buf);
void skia_rs_path_buf_free(SkiaRsPathBuf* buf);

float skia_rs_stroke_rec_inflation_radius(
        uint8_t join, float miter_limit, uint8_t cap, float stroke_width);

SkiaRsPathBuf* skia_rs_stroke_path(
        float width, float miter_limit, float res_scale,
        uint8_t cap, uint8_t join, bool do_fill,
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        SkRect bounds, uint8_t fill_type, uint8_t convexity, uint8_t segment_mask);

SkiaRsPathBuf* skia_rs_stroke_rect(
        float width, float miter_limit, float res_scale,
        uint8_t cap, uint8_t join, bool do_fill,
        SkRect rect, uint8_t dir);

SkiaRsPathBuf* skia_rs_stroker_apply_cap(
        uint8_t cap,
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t fill_type, uint8_t convexity, uint8_t segment_mask, int32_t last_move_index,
        uint8_t is_a_type, uint8_t is_a_dir, uint8_t is_a_start_index, bool is_volatile,
        SkPoint pivot, SkVector normal, SkPoint stop, bool extend_last_pt);

void skia_rs_stroker_apply_join(
        uint8_t join,
        const SkPoint* outer_pts, size_t outer_pt_count,
        const uint8_t* outer_vbs, size_t outer_vb_count,
        const float* outer_conics, size_t outer_conic_count,
        uint8_t outer_fill_type, uint8_t outer_convexity, uint8_t outer_seg_mask,
        int32_t outer_last_move, uint8_t outer_is_a_type, uint8_t outer_is_a_dir,
        uint8_t outer_is_a_start, bool outer_is_volatile,
        const SkPoint* inner_pts, size_t inner_pt_count,
        const uint8_t* inner_vbs, size_t inner_vb_count,
        const float* inner_conics, size_t inner_conic_count,
        uint8_t inner_fill_type, uint8_t inner_convexity, uint8_t inner_seg_mask,
        int32_t inner_last_move, uint8_t inner_is_a_type, uint8_t inner_is_a_dir,
        uint8_t inner_is_a_start, bool inner_is_volatile,
        SkVector before_unit_normal, SkPoint pivot, SkVector after_unit_normal,
        float radius, float inv_miter_limit, bool prev_is_line, bool curr_is_line,
        SkiaRsPathBuf** out_outer, SkiaRsPathBuf** out_inner);
}  // extern "C"

// Friend of SkPathBuilder (declared in include/core/SkPathBuilder.h)
class SkPathStroker {
public:
    static void ApplyRustBuf(SkPathBuilder* dst, SkiaRsPathBuf* buf) {
        if (!buf) {
            return;
        }
        size_t npts = 0, nvbs = 0, ncns = 0;
        const SkPoint* pts = skia_rs_path_buf_points(buf, &npts);
        const uint8_t* vbs = skia_rs_path_buf_verbs(buf, &nvbs);
        const float* cns = skia_rs_path_buf_conics(buf, &ncns);
        auto meta = skia_rs_path_buf_meta(buf);

        dst->fPts.clear();
        if (npts > 0) {
            memcpy(dst->fPts.push_back_n(npts), pts, npts * sizeof(SkPoint));
        }
        dst->fVerbs.clear();
        if (nvbs > 0) {
            memcpy(dst->fVerbs.push_back_n(nvbs), vbs, nvbs * sizeof(SkPathVerb));
        }
        dst->fConicWeights.clear();
        if (ncns > 0) {
            memcpy(dst->fConicWeights.push_back_n(ncns), cns, ncns * sizeof(float));
        }
        dst->fFillType = static_cast<SkPathFillType>(meta.fill_type);
        dst->fIsVolatile = meta.is_volatile;
        dst->fConvexity = static_cast<SkPathConvexity>(meta.convexity);
        dst->fSegmentMask = meta.segment_mask;
        dst->fLastMoveIndex = meta.last_move_index;
        dst->fType = static_cast<SkPathIsAType>(meta.is_a_type);
        dst->fIsA.fDirection = static_cast<SkPathDirection>(meta.is_a_dir);
        dst->fIsA.fStartIndex = meta.is_a_start_index;

        skia_rs_path_buf_free(buf);
    }

    static void RunCap(SkPaint::Cap cap, SkPathBuilder* sink,
                       const SkPoint& pivot, const SkVector& normal,
                       const SkPoint& stop, bool extendLastPt) {
        auto* buf = skia_rs_stroker_apply_cap(
                static_cast<uint8_t>(cap),
                sink->fPts.data(), sink->fPts.size(),
                reinterpret_cast<const uint8_t*>(sink->fVerbs.data()), sink->fVerbs.size(),
                sink->fConicWeights.data(), sink->fConicWeights.size(),
                static_cast<uint8_t>(sink->fFillType),
                static_cast<uint8_t>(sink->fConvexity),
                static_cast<uint8_t>(sink->fSegmentMask),
                sink->fLastMoveIndex,
                static_cast<uint8_t>(sink->fType),
                static_cast<uint8_t>(sink->fIsA.fDirection),
                sink->fIsA.fStartIndex,
                sink->fIsVolatile,
                pivot, normal, stop, extendLastPt);
        ApplyRustBuf(sink, buf);
    }

    static void RunJoin(SkPaint::Join join, SkPathBuilder* outer, SkPathBuilder* inner,
                        const SkVector& beforeUnitNormal, const SkPoint& pivot,
                        const SkVector& afterUnitNormal, SkScalar radius,
                        SkScalar invMiterLimit, bool prevIsLine, bool currIsLine) {
        SkiaRsPathBuf* outOuter = nullptr;
        SkiaRsPathBuf* outInner = nullptr;
        skia_rs_stroker_apply_join(
                static_cast<uint8_t>(join),
                outer->fPts.data(), outer->fPts.size(),
                reinterpret_cast<const uint8_t*>(outer->fVerbs.data()), outer->fVerbs.size(),
                outer->fConicWeights.data(), outer->fConicWeights.size(),
                static_cast<uint8_t>(outer->fFillType),
                static_cast<uint8_t>(outer->fConvexity),
                static_cast<uint8_t>(outer->fSegmentMask),
                outer->fLastMoveIndex,
                static_cast<uint8_t>(outer->fType),
                static_cast<uint8_t>(outer->fIsA.fDirection),
                outer->fIsA.fStartIndex,
                outer->fIsVolatile,
                inner->fPts.data(), inner->fPts.size(),
                reinterpret_cast<const uint8_t*>(inner->fVerbs.data()), inner->fVerbs.size(),
                inner->fConicWeights.data(), inner->fConicWeights.size(),
                static_cast<uint8_t>(inner->fFillType),
                static_cast<uint8_t>(inner->fConvexity),
                static_cast<uint8_t>(inner->fSegmentMask),
                inner->fLastMoveIndex,
                static_cast<uint8_t>(inner->fType),
                static_cast<uint8_t>(inner->fIsA.fDirection),
                inner->fIsA.fStartIndex,
                inner->fIsVolatile,
                beforeUnitNormal, pivot, afterUnitNormal,
                radius, invMiterLimit, prevIsLine, currIsLine,
                &outOuter, &outInner);
        ApplyRustBuf(outer, outOuter);
        ApplyRustBuf(inner, outInner);
    }
};

// ============================================================================
// SkStrokerPriv.cpp
// ============================================================================

static void ButtCapper(SkPathBuilder* sink, const SkPoint& pivot, const SkVector& normal,
                       const SkPoint& stop, bool extendLastPt) {
    SkPathStroker::RunCap(SkPaint::kButt_Cap, sink, pivot, normal, stop, extendLastPt);
}

static void RoundCapper(SkPathBuilder* sink, const SkPoint& pivot, const SkVector& normal,
                        const SkPoint& stop, bool extendLastPt) {
    SkPathStroker::RunCap(SkPaint::kRound_Cap, sink, pivot, normal, stop, extendLastPt);
}

static void SquareCapper(SkPathBuilder* sink, const SkPoint& pivot, const SkVector& normal,
                         const SkPoint& stop, bool extendLastPt) {
    SkPathStroker::RunCap(SkPaint::kSquare_Cap, sink, pivot, normal, stop, extendLastPt);
}

static void MiterJoiner(SkPathBuilder* outer, SkPathBuilder* inner,
                        const SkVector& beforeUnitNormal, const SkPoint& pivot,
                        const SkVector& afterUnitNormal, SkScalar radius,
                        SkScalar invMiterLimit, bool prevIsLine, bool currIsLine) {
    SkPathStroker::RunJoin(SkPaint::kMiter_Join, outer, inner, beforeUnitNormal, pivot,
                           afterUnitNormal, radius, invMiterLimit, prevIsLine, currIsLine);
}

static void RoundJoiner(SkPathBuilder* outer, SkPathBuilder* inner,
                        const SkVector& beforeUnitNormal, const SkPoint& pivot,
                        const SkVector& afterUnitNormal, SkScalar radius,
                        SkScalar invMiterLimit, bool prevIsLine, bool currIsLine) {
    SkPathStroker::RunJoin(SkPaint::kRound_Join, outer, inner, beforeUnitNormal, pivot,
                           afterUnitNormal, radius, invMiterLimit, prevIsLine, currIsLine);
}

static void BluntJoiner(SkPathBuilder* outer, SkPathBuilder* inner,
                        const SkVector& beforeUnitNormal, const SkPoint& pivot,
                        const SkVector& afterUnitNormal, SkScalar radius,
                        SkScalar invMiterLimit, bool prevIsLine, bool currIsLine) {
    SkPathStroker::RunJoin(SkPaint::kBevel_Join, outer, inner, beforeUnitNormal, pivot,
                           afterUnitNormal, radius, invMiterLimit, prevIsLine, currIsLine);
}

SkStrokerPriv::CapProc SkStrokerPriv::CapFactory(SkPaint::Cap cap) {
    static constexpr auto gCappers = std::to_array<SkStrokerPriv::CapProc>({
        ButtCapper, RoundCapper, SquareCapper
    });
    SkASSERT((unsigned)cap < SkPaint::kCapCount);
    return gCappers[cap];
}

SkStrokerPriv::JoinProc SkStrokerPriv::JoinFactory(SkPaint::Join join) {
    static constexpr auto gJoiners = std::to_array<SkStrokerPriv::JoinProc>({
        MiterJoiner, RoundJoiner, BluntJoiner
    });
    SkASSERT((unsigned)join < SkPaint::kJoinCount);
    return gJoiners[join];
}

// ============================================================================
// SkStroke.cpp
// ============================================================================

SkStroke::SkStroke() {
    fWidth = SK_Scalar1;
    fMiterLimit = SkPaintDefaults_MiterLimit;
    fResScale = 1;
    fCap = SkPaint::kDefault_Cap;
    fJoin = SkPaint::kDefault_Join;
    fDoFill = false;
}

SkStroke::SkStroke(const SkPaint& p) {
    fWidth = p.getStrokeWidth();
    fMiterLimit = p.getStrokeMiter();
    fResScale = 1;
    fCap = (uint8_t)p.getStrokeCap();
    fJoin = (uint8_t)p.getStrokeJoin();
    fDoFill = SkToU8(p.getStyle() == SkPaint::kStrokeAndFill_Style);
}

SkStroke::SkStroke(const SkPaint& p, SkScalar width) {
    fWidth = width;
    fMiterLimit = p.getStrokeMiter();
    fResScale = 1;
    fCap = (uint8_t)p.getStrokeCap();
    fJoin = (uint8_t)p.getStrokeJoin();
    fDoFill = SkToU8(p.getStyle() == SkPaint::kStrokeAndFill_Style);
}

void SkStroke::setWidth(SkScalar width) {
    SkASSERT(width >= 0);
    fWidth = width;
}

void SkStroke::setMiterLimit(SkScalar miterLimit) {
    SkASSERT(miterLimit >= 0);
    fMiterLimit = miterLimit;
}

void SkStroke::setCap(SkPaint::Cap cap) {
    SkASSERT((unsigned)cap < SkPaint::kCapCount);
    fCap = SkToU8(cap);
}

void SkStroke::setJoin(SkPaint::Join join) {
    SkASSERT((unsigned)join < SkPaint::kJoinCount);
    fJoin = SkToU8(join);
}

void SkStroke::strokePath(const SkPath& src, SkPathBuilder* dst) const {
    SkASSERT(dst);
    float radius = fWidth / 2.f;
    if (radius <= 0) {
        return;
    }
    const auto raw = SkPathPriv::Raw(src, SkResolveConvexity::kNo);
    if (!raw) {
        return;
    }
    auto* buf = skia_rs_stroke_path(
            fWidth, fMiterLimit, fResScale, fCap, fJoin, fDoFill,
            raw->fPoints.data(), raw->fPoints.size(),
            reinterpret_cast<const uint8_t*>(raw->fVerbs.data()), raw->fVerbs.size(),
            raw->fConics.data(), raw->fConics.size(),
            raw->fBounds,
            static_cast<uint8_t>(raw->fFillType),
            static_cast<uint8_t>(raw->fConvexity),
            raw->fSegmentMask);
    SkPathStroker::ApplyRustBuf(dst, buf);
}

void SkStroke::strokeRect(const SkRect& origRect, SkPathBuilder* dst, SkPathDirection dir) const {
    SkASSERT(dst != nullptr);
    auto* buf = skia_rs_stroke_rect(
            fWidth, fMiterLimit, fResScale, fCap, fJoin, fDoFill,
            origRect, static_cast<uint8_t>(dir));
    SkPathStroker::ApplyRustBuf(dst, buf);
}

// ============================================================================
// SkStrokeRec.cpp
// ============================================================================

#define kStrokeRec_FillStyleWidth (-SK_Scalar1)

SkStrokeRec::SkStrokeRec(InitStyle s) {
    fResScale = 1;
    fWidth = (kFill_InitStyle == s) ? kStrokeRec_FillStyleWidth : 0;
    fMiterLimit = SkPaintDefaults_MiterLimit;
    fCap = SkPaint::kDefault_Cap;
    fJoin = SkPaint::kDefault_Join;
    fStrokeAndFill = false;
}

SkStrokeRec::SkStrokeRec(const SkPaint& paint, SkScalar resScale) {
    this->init(paint, paint.getStyle(), resScale);
}

SkStrokeRec::SkStrokeRec(const SkPaint& paint, SkPaint::Style styleOverride, SkScalar resScale) {
    this->init(paint, styleOverride, resScale);
}

void SkStrokeRec::init(const SkPaint& paint, SkPaint::Style style, SkScalar resScale) {
    fResScale = resScale;
    switch (style) {
        case SkPaint::kFill_Style:
            fWidth = kStrokeRec_FillStyleWidth;
            fStrokeAndFill = false;
            break;
        case SkPaint::kStroke_Style:
            fWidth = paint.getStrokeWidth();
            fStrokeAndFill = false;
            break;
        case SkPaint::kStrokeAndFill_Style:
            if (0 == paint.getStrokeWidth()) {
                fWidth = kStrokeRec_FillStyleWidth;
                fStrokeAndFill = false;
            } else {
                fWidth = paint.getStrokeWidth();
                fStrokeAndFill = true;
            }
            break;
        default:
            fWidth = kStrokeRec_FillStyleWidth;
            fStrokeAndFill = false;
            break;
    }
    fMiterLimit = paint.getStrokeMiter();
    fCap = paint.getStrokeCap();
    fJoin = paint.getStrokeJoin();
}

SkStrokeRec::Style SkStrokeRec::getStyle() const {
    if (fWidth < 0) {
        return kFill_Style;
    } else if (0 == fWidth) {
        return kHairline_Style;
    } else {
        return fStrokeAndFill ? kStrokeAndFill_Style : kStroke_Style;
    }
}

void SkStrokeRec::setFillStyle() {
    fWidth = kStrokeRec_FillStyleWidth;
    fStrokeAndFill = false;
}

void SkStrokeRec::setHairlineStyle() {
    fWidth = 0;
    fStrokeAndFill = false;
}

void SkStrokeRec::setStrokeStyle(SkScalar width, bool strokeAndFill) {
    if (strokeAndFill && (0 == width)) {
        this->setFillStyle();
    } else {
        fWidth = width;
        fStrokeAndFill = strokeAndFill;
    }
}

#ifdef SK_DEBUG
bool gDebugStrokerErrorSet = false;
SkScalar gDebugStrokerError;
#endif

bool SkStrokeRec::applyToPath(SkPathBuilder* dst, const SkPath& src) const {
    if (fWidth <= 0) {
        return false;
    }
    SkStroke stroker;
    stroker.setCap((SkPaint::Cap)fCap);
    stroker.setJoin((SkPaint::Join)fJoin);
    stroker.setMiterLimit(fMiterLimit);
    stroker.setWidth(fWidth);
    stroker.setDoFill(fStrokeAndFill);
#ifdef SK_DEBUG
    stroker.setResScale(gDebugStrokerErrorSet ? gDebugStrokerError : fResScale);
#else
    stroker.setResScale(fResScale);
#endif
    stroker.strokePath(src, dst);
    return true;
}

void SkStrokeRec::applyToPaint(SkPaint* paint) const {
    if (fWidth < 0) {
        paint->setStyle(SkPaint::kFill_Style);
        return;
    }
    paint->setStyle(fStrokeAndFill ? SkPaint::kStrokeAndFill_Style : SkPaint::kStroke_Style);
    paint->setStrokeWidth(fWidth);
    paint->setStrokeMiter(fMiterLimit);
    paint->setStrokeCap((SkPaint::Cap)fCap);
    paint->setStrokeJoin((SkPaint::Join)fJoin);
}

SkScalar SkStrokeRec::getInflationRadius() const {
    return GetInflationRadius((SkPaint::Join)fJoin, fMiterLimit, (SkPaint::Cap)fCap, fWidth);
}

SkScalar SkStrokeRec::GetInflationRadius(const SkPaint& paint, SkPaint::Style style) {
    SkScalar width = SkPaint::kFill_Style == style ? -SK_Scalar1 : paint.getStrokeWidth();
    return GetInflationRadius(paint.getStrokeJoin(), paint.getStrokeMiter(), paint.getStrokeCap(),
                              width);
}

SkScalar SkStrokeRec::GetInflationRadius(SkPaint::Join join, SkScalar miterLimit,
                                         SkPaint::Cap cap, SkScalar strokeWidth) {
    return skia_rs_stroke_rec_inflation_radius(
            static_cast<uint8_t>(join), miterLimit, static_cast<uint8_t>(cap), strokeWidth);
}

// ============================================================================
// SkPathUtils.cpp
// ============================================================================

namespace skpathutils {

bool FillPathWithPaint(const SkPath& origSrc, const SkPaint& paint, SkPathBuilder* builder,
                       const SkRect* cullRect, const SkMatrix& ctm) {
    if (!origSrc.isFinite()) {
        builder->reset();
        return false;
    }

    const SkScalar resScale = SkMatrixPriv::ComputeResScaleForStroking(ctm);
    SkStrokeRec rec(paint, resScale);

#if defined(SK_BUILD_FOR_FUZZER)
    if (rec.getStyle() == SkStrokeRec::Style::kStroke_Style && rec.getWidth() < 0.001) {
        return false;
    }
    if (rec.needToApply()) {
        const SkRect& bounds = origSrc.getBounds();
        if (bounds.width() > 1e9f || bounds.height() > 1e9f) {
            return false;
        }
    }
#endif

    const SkPath* srcPtr = &origSrc;
    SkPath pathStorage;
    SkPathEffect* pe = paint.getPathEffect();
    if (pe && pe->filterPath(builder, origSrc, &rec, cullRect, ctm)) {
        pathStorage = builder->detach();
        srcPtr = &pathStorage;
    }
    if (!rec.applyToPath(builder, *srcPtr)) {
        *builder = *srcPtr;
    }

    if (!builder->isFinite()) {
        builder->reset();
    }
    return !rec.isHairlineStyle();
}

bool FillPathWithPaint(const SkPath& src, const SkPaint& paint, SkPathBuilder* dst) {
    return FillPathWithPaint(src, paint, dst, nullptr, SkMatrix::I());
}

SkPath FillPathWithPaint(const SkPath& src, const SkPaint& paint, bool* isFillPtr) {
    SkPathBuilder builder;
    bool isFill = FillPathWithPaint(src, paint, &builder);
    if (isFillPtr) {
        *isFillPtr = isFill;
    }
    return builder.detach();
}

}  // namespace skpathutils
