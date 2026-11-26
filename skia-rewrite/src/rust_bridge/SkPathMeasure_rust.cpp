/*
 * Safe Rust bridge for Layer 2.2 skia-rs-path (Path Measurement):
 *   SkContourMeasure.cpp, SkPathMeasure.cpp.
 */

#include "include/core/SkContourMeasure.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathMeasure.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSpan.h"
#include "include/private/SkAssert.h"
#include "include/private/SkTDArray.h"
#include "include/private/SkTo.h"
#include "src/core/SkPathMeasurePriv.h"
#include "src/core/SkPathPriv.h"

#include <array>
#include <bit>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <utility>

struct SkiaRsPathBuf;
struct SkiaRsContourMeasureIter;
struct SkiaRsContourMeasure;

struct SkiaRsContourSegment {
    float distance;
    uint32_t pt_index;
    uint32_t t_value_and_type;
};

static_assert(sizeof(SkiaRsContourSegment) == 12);

extern "C" {
const SkPoint* skia_rs_path_buf_points(const SkiaRsPathBuf* buf, size_t* out_len);
const uint8_t* skia_rs_path_buf_verbs(const SkiaRsPathBuf* buf, size_t* out_len);
const float* skia_rs_path_buf_conics(const SkiaRsPathBuf* buf, size_t* out_len);
void skia_rs_path_buf_free(SkiaRsPathBuf* buf);

float skia_rs_contour_t_value_to_scalar(uint32_t t);

SkiaRsContourMeasureIter* skia_rs_contour_iter_new(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        bool force_closed, float res_scale);
void skia_rs_contour_iter_free(SkiaRsContourMeasureIter* iter);
bool skia_rs_contour_iter_has_next(const SkiaRsContourMeasureIter* iter);
SkiaRsContourMeasure* skia_rs_contour_iter_next(SkiaRsContourMeasureIter* iter);

const SkiaRsContourSegment* skia_rs_contour_measure_segments(
        const SkiaRsContourMeasure* cm, size_t* out_len);
const SkPoint* skia_rs_contour_measure_pts(
        const SkiaRsContourMeasure* cm, size_t* out_len);
float skia_rs_contour_measure_length(const SkiaRsContourMeasure* cm);
bool skia_rs_contour_measure_is_closed(const SkiaRsContourMeasure* cm);
void skia_rs_contour_measure_free(SkiaRsContourMeasure* cm);

bool skia_rs_contour_get_pos_tan(
        const SkiaRsContourSegment* segs, size_t seg_count,
        const SkPoint* pts, size_t pt_count,
        float length, float distance,
        SkPoint* out_pos, SkVector* out_tan);
bool skia_rs_contour_get_matrix(
        const SkiaRsContourSegment* segs, size_t seg_count,
        const SkPoint* pts, size_t pt_count,
        float length, float distance, uint32_t flags,
        SkMatrix* out_matrix);
SkiaRsPathBuf* skia_rs_contour_get_segment(
        const SkiaRsContourSegment* segs, size_t seg_count,
        const SkPoint* pts, size_t pt_count,
        float length, float start_d, float stop_d, bool start_with_move_to,
        const SkPoint* dst_pts, size_t dst_pt_count,
        const uint8_t* dst_vbs, size_t dst_vb_count,
        const float* dst_conics, size_t dst_conic_count,
        uint8_t dst_fill_type, uint8_t dst_convexity, uint8_t dst_seg_mask,
        int32_t dst_last_move, uint8_t dst_is_a_type, uint8_t dst_is_a_dir,
        uint8_t dst_is_a_start, bool dst_is_volatile);
SkiaRsPathBuf* skia_rs_contour_seg_to(
        const SkPoint* pts, size_t pt_count,
        uint8_t seg_type, float start_t, float stop_t,
        const SkPoint* dst_pts, size_t dst_pt_count,
        const uint8_t* dst_vbs, size_t dst_vb_count,
        const float* dst_conics, size_t dst_conic_count,
        uint8_t dst_fill_type, uint8_t dst_convexity, uint8_t dst_seg_mask,
        int32_t dst_last_move, uint8_t dst_is_a_type, uint8_t dst_is_a_dir,
        uint8_t dst_is_a_start, bool dst_is_volatile);
}  // extern "C"

namespace {

void replay_rust_buf_verbs(SkPathBuilder* dst, SkiaRsPathBuf* buf, size_t skipVerbs) {
    if (!buf) {
        return;
    }
    size_t npts = 0, nvbs = 0, ncns = 0;
    const SkPoint* pts = skia_rs_path_buf_points(buf, &npts);
    const uint8_t* vbs = skia_rs_path_buf_verbs(buf, &nvbs);
    const float* cns = skia_rs_path_buf_conics(buf, &ncns);

    size_t pi = 0, ci = 0;
    for (size_t vi = 0; vi < nvbs; ++vi) {
        SkPathVerb v = static_cast<SkPathVerb>(vbs[vi]);
        bool emit = (vi >= skipVerbs);
        switch (v) {
            case SkPathVerb::kMove:
                if (emit) dst->moveTo(pts[pi]);
                pi += 1;
                break;
            case SkPathVerb::kLine:
                if (emit) dst->lineTo(pts[pi]);
                pi += 1;
                break;
            case SkPathVerb::kQuad:
                if (emit) dst->quadTo(pts[pi], pts[pi + 1]);
                pi += 2;
                break;
            case SkPathVerb::kConic:
                if (emit) dst->conicTo(pts[pi], pts[pi + 1], cns[ci]);
                pi += 2;
                ci += 1;
                break;
            case SkPathVerb::kCubic:
                if (emit) dst->cubicTo(pts[pi], pts[pi + 1], pts[pi + 2]);
                pi += 3;
                break;
            case SkPathVerb::kClose:
                if (emit) dst->close();
                break;
        }
    }
    skia_rs_path_buf_free(buf);
}

}  // namespace

// ============================================================================
// SkContourMeasure.cpp
// ============================================================================

SkScalar SkContourMeasure::Segment::getScalarT() const {
    static_assert(sizeof(SkContourMeasure::Segment) == sizeof(SkiaRsContourSegment));
    return skia_rs_contour_t_value_to_scalar(fTValue);
}

void SkContourMeasure_segTo(const SkPoint pts[], unsigned segType,
                            SkScalar startT, SkScalar stopT, SkPathBuilder* dst) {
    size_t ptCount = (segType == kLine_SegType) ? 2 : (segType == kQuad_SegType) ? 3 : 4;
    bool seeded = !dst->isEmpty();
    SkPoint seedPt = seeded ? dst->getLastPt().value_or(SkPoint{0, 0}) : SkPoint{0, 0};
    uint8_t seedVerb = static_cast<uint8_t>(SkPathVerb::kMove);

    auto* buf = skia_rs_contour_seg_to(
            pts, ptCount,
            static_cast<uint8_t>(segType), startT, stopT,
            seeded ? &seedPt : nullptr, seeded ? 1 : 0,
            seeded ? &seedVerb : nullptr, seeded ? 1 : 0,
            nullptr, 0,
            static_cast<uint8_t>(dst->fillType()),
            static_cast<uint8_t>(SkPathConvexity::kUnknown),
            0, seeded ? 0 : -1, 0, 0, 0, false);
    replay_rust_buf_verbs(dst, buf, seeded ? 1 : 0);
}

class SkContourMeasureIter::Impl {
public:
    Impl(const SkPath& path, bool forceClosed, SkScalar resScale) {
        auto pts = path.points();
        auto vbs = path.verbs();
        auto cns = path.conicWeights();
        fIter = skia_rs_contour_iter_new(
                pts.data(), pts.size(),
                reinterpret_cast<const uint8_t*>(vbs.data()), vbs.size(),
                cns.data(), cns.size(),
                forceClosed, resScale);
    }

    ~Impl() {
        skia_rs_contour_iter_free(fIter);
    }

    bool hasNextSegments() const {
        return skia_rs_contour_iter_has_next(fIter);
    }

    SkContourMeasure* buildSegments() {
        SkiaRsContourMeasure* cm = skia_rs_contour_iter_next(fIter);
        if (!cm) {
            return nullptr;
        }
        size_t nsegs = 0, npts = 0;
        const SkiaRsContourSegment* rawSegs = skia_rs_contour_measure_segments(cm, &nsegs);
        const SkPoint* rawPts = skia_rs_contour_measure_pts(cm, &npts);
        SkScalar length = skia_rs_contour_measure_length(cm);
        bool isClosed = skia_rs_contour_measure_is_closed(cm);

        SkTDArray<SkContourMeasure::Segment> segs;
        if (nsegs > 0) {
            segs.resize(nsegs);
            for (size_t i = 0; i < nsegs; ++i) {
                segs[i].fDistance = rawSegs[i].distance;
                segs[i].fPtIndex = rawSegs[i].pt_index;
                segs[i].fTValue = rawSegs[i].t_value_and_type & 0x3FFFFFFFu;
                segs[i].fType = (rawSegs[i].t_value_and_type >> 30) & 0x3u;
            }
        }
        SkTDArray<SkPoint> pts;
        if (npts > 0) {
            pts.append(npts, rawPts);
        }
        skia_rs_contour_measure_free(cm);
        return new SkContourMeasure(std::move(segs), std::move(pts), length, isClosed);
    }

private:
    SkiaRsContourMeasureIter* fIter = nullptr;
};

SkContourMeasureIter::SkContourMeasureIter() {}

SkContourMeasureIter::SkContourMeasureIter(const SkPath& path, bool forceClosed,
                                           SkScalar resScale) {
    this->reset(path, forceClosed, resScale);
}

SkContourMeasureIter::~SkContourMeasureIter() {}

SkContourMeasureIter::SkContourMeasureIter(SkContourMeasureIter&&) = default;
SkContourMeasureIter& SkContourMeasureIter::operator=(SkContourMeasureIter&&) = default;

void SkContourMeasureIter::reset(const SkPath& path, bool forceClosed, SkScalar resScale) {
    if (path.isFinite()) {
        fImpl = std::make_unique<Impl>(path, forceClosed, resScale);
    } else {
        fImpl.reset();
    }
}

sk_sp<SkContourMeasure> SkContourMeasureIter::next() {
    if (!fImpl) {
        return nullptr;
    }
    while (fImpl->hasNextSegments()) {
        if (auto* cm = fImpl->buildSegments()) {
            return sk_sp<SkContourMeasure>(cm);
        }
    }
    return nullptr;
}

SkContourMeasure::SkContourMeasure(SkTDArray<Segment>&& segs, SkTDArray<SkPoint>&& pts,
                                   SkScalar length, bool isClosed)
    : fSegments(std::move(segs))
    , fPts(std::move(pts))
    , fLength(length)
    , fIsClosed(isClosed) {}

const SkContourMeasure::Segment* SkContourMeasure::distanceToSegment(
        SkScalar distance, SkScalar* t) const {
    const Segment* seg = fSegments.begin();
    int count = fSegments.size();
    unsigned lo = 0;
    unsigned hi = count - 1;
    while (lo < hi) {
        unsigned mid = (hi + lo) >> 1;
        if (seg[mid].fDistance < distance) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    int index = static_cast<int>(hi);
    if (seg[hi].fDistance < distance && index + 1 < count) {
        index += 1;
    }
    seg = &seg[index];

    SkScalar startT = 0, startD = 0;
    if (index > 0) {
        startD = seg[-1].fDistance;
        if (seg[-1].fPtIndex == seg->fPtIndex) {
            startT = seg[-1].getScalarT();
        }
    }
    *t = startT + (seg->getScalarT() - startT) * (distance - startD) / (seg->fDistance - startD);
    return seg;
}

bool SkContourMeasure::getPosTan(SkScalar distance, SkPoint* pos, SkVector* tangent) const {
    static_assert(sizeof(Segment) == sizeof(SkiaRsContourSegment));
    static_assert(std::bit_cast<std::array<uint32_t, 3>>(Segment{0.0f, 0u, 1u, 2u})[2] ==
                  (1u | (2u << 30)));
    return skia_rs_contour_get_pos_tan(
            reinterpret_cast<const SkiaRsContourSegment*>(fSegments.begin()),
            static_cast<size_t>(fSegments.size()),
            fPts.begin(), static_cast<size_t>(fPts.size()),
            fLength, distance, pos, tangent);
}

bool SkContourMeasure::getMatrix(SkScalar distance, SkMatrix* matrix, MatrixFlags flags) const {
    return skia_rs_contour_get_matrix(
            reinterpret_cast<const SkiaRsContourSegment*>(fSegments.begin()),
            static_cast<size_t>(fSegments.size()),
            fPts.begin(), static_cast<size_t>(fPts.size()),
            fLength, distance, static_cast<uint32_t>(flags), matrix);
}

bool SkContourMeasure::getSegment(SkScalar startD, SkScalar stopD, SkPathBuilder* dst,
                                  bool startWithMoveTo) const {
    SkASSERT(dst);
    bool seeded = !startWithMoveTo && !dst->isEmpty();
    SkPoint seedPt = seeded ? dst->getLastPt().value_or(SkPoint{0, 0}) : SkPoint{0, 0};
    uint8_t seedVerb = static_cast<uint8_t>(SkPathVerb::kMove);

    auto* buf = skia_rs_contour_get_segment(
            reinterpret_cast<const SkiaRsContourSegment*>(fSegments.begin()),
            static_cast<size_t>(fSegments.size()),
            fPts.begin(), static_cast<size_t>(fPts.size()),
            fLength, startD, stopD, startWithMoveTo,
            seeded ? &seedPt : nullptr, seeded ? 1 : 0,
            seeded ? &seedVerb : nullptr, seeded ? 1 : 0,
            nullptr, 0,
            static_cast<uint8_t>(dst->fillType()),
            static_cast<uint8_t>(SkPathConvexity::kUnknown),
            0, seeded ? 0 : -1, 0, 0, 0, false);
    if (!buf) {
        return false;
    }
    replay_rust_buf_verbs(dst, buf, seeded ? 1 : 0);
    return true;
}

SkContourMeasure::VerbMeasure SkContourMeasure::ForwardVerbIterator::operator*() const {
    static constexpr auto seg_pt_count = std::to_array<size_t>({2, 3, 4, 4});
    static constexpr auto seg_verb = std::to_array<SkPathVerb>({
        SkPathVerb::kLine,
        SkPathVerb::kQuad,
        SkPathVerb::kCubic,
        SkPathVerb::kConic,
    });
    return {
        fSegments.front().fDistance,
        seg_verb[fSegments.front().fType],
        SkSpan(fPts.data() + fSegments.front().fPtIndex, seg_pt_count[fSegments.front().fType]),
    };
}

// ============================================================================
// SkPathMeasure.cpp
// ============================================================================

SkPathMeasure::SkPathMeasure() {}

SkPathMeasure::SkPathMeasure(const SkPath& path, bool forceClosed, SkScalar resScale)
    : fIter(path, forceClosed, resScale) {
    fContour = fIter.next();
}

SkPathMeasure::~SkPathMeasure() {}

void SkPathMeasure::setPath(const SkPath* path, bool forceClosed) {
    fIter.reset(path ? *path : SkPath(), forceClosed);
    fContour = fIter.next();
}

SkScalar SkPathMeasure::getLength() {
    return fContour ? fContour->length() : 0;
}

bool SkPathMeasure::getPosTan(SkScalar distance, SkPoint* position, SkVector* tangent) {
    return fContour && fContour->getPosTan(distance, position, tangent);
}

bool SkPathMeasure::getMatrix(SkScalar distance, SkMatrix* matrix, MatrixFlags flags) {
    return fContour && fContour->getMatrix(distance, matrix,
                                           static_cast<SkContourMeasure::MatrixFlags>(flags));
}

bool SkPathMeasure::getSegment(SkScalar startD, SkScalar stopD, SkPathBuilder* dst,
                               bool startWithMoveTo) {
    return fContour && fContour->getSegment(startD, stopD, dst, startWithMoveTo);
}

bool SkPathMeasure::isClosed() {
    return fContour && fContour->isClosed();
}

bool SkPathMeasure::nextContour() {
    fContour = fIter.next();
    return !!fContour;
}

#ifdef SK_DEBUG
void SkPathMeasure::dump() {}
#endif

size_t SkPathMeasurePriv::CountSegments(const SkPathMeasure& meas) {
    if (auto cntr = meas.currentMeasure()) {
        return cntr->fSegments.size();
    }
    return 0;
}
