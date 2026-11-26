/*
 * Safe Rust bridge for Layer 2.2 skia-rs-path (Utils — :skia / skia_utils_private):
 *   src/utils/SkDashPath.cpp, src/utils/SkParsePath.cpp.
 */

#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSpan.h"
#include "include/core/SkString.h"
#include "include/core/SkStrokeRec.h"
#include "include/core/SkTypes.h"
#include "include/utils/SkParsePath.h"
#include "src/core/SkPathEffectBase.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkPathRaw.h"
#include "src/utils/SkDashPathPriv.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>

struct SkiaRsPathBuf;
struct SkiaRsString;

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

struct SkiaRsStrokeRec {
    float res_scale;
    float width;
    float miter_limit;
    uint8_t cap;
    uint8_t join;
    bool stroke_and_fill;
    uint8_t _pad;
};

static_assert(sizeof(SkiaRsStrokeRec) == 16);

extern "C" {
const SkPoint* skia_rs_path_buf_points(const SkiaRsPathBuf* buf, size_t* out_len);
const uint8_t* skia_rs_path_buf_verbs(const SkiaRsPathBuf* buf, size_t* out_len);
const float* skia_rs_path_buf_conics(const SkiaRsPathBuf* buf, size_t* out_len);
SkiaRsPathBufMeta skia_rs_path_buf_meta(const SkiaRsPathBuf* buf);
void skia_rs_path_buf_free(SkiaRsPathBuf* buf);

const uint8_t* skia_rs_string_data(const SkiaRsString* s, size_t* out_len);
void skia_rs_string_free(SkiaRsString* s);

bool skia_rs_dash_valid_path(float phase, const float* intervals, size_t count);
void skia_rs_dash_calc_parameters(
        float phase, const float* intervals, size_t count,
        float* out_initial_dash_length, size_t* out_initial_dash_index,
        float* out_interval_length, float* out_adjusted_phase);
SkiaRsPathBuf* skia_rs_dash_internal_filter(
        const SkPoint* dst_pts, size_t dst_pt_count,
        const uint8_t* dst_vbs, size_t dst_vb_count,
        const float* dst_conics, size_t dst_conic_count,
        uint8_t dst_fill_type, uint8_t dst_convexity, uint8_t dst_seg_mask,
        int32_t dst_last_move, uint8_t dst_is_a_type, uint8_t dst_is_a_dir,
        uint8_t dst_is_a_start, bool dst_is_volatile,
        const SkPoint* src_pts, size_t src_pt_count,
        const uint8_t* src_vbs, size_t src_vb_count,
        const float* src_conics, size_t src_conic_count,
        SkRect src_bounds, uint8_t src_fill_type, uint8_t src_convexity, uint8_t src_seg_mask,
        SkiaRsStrokeRec* io_rec,
        const SkRect* cull_rect,
        const float* intervals, size_t interval_count,
        float initial_dash_length, int32_t initial_dash_index,
        float interval_length, float start_phase,
        bool allow_stroke_rec_application,
        bool* out_ok);

SkiaRsPathBuf* skia_rs_parse_path_from_svg(const uint8_t* data, size_t len);
SkiaRsString* skia_rs_parse_path_to_svg(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* conics, size_t conic_count,
        uint8_t encoding);
}  // extern "C"

namespace {

void replace_builder_from_rust_buf(SkPathBuilder* dst, SkiaRsPathBuf* buf) {
    if (!buf) {
        return;
    }
    size_t npts = 0, nvbs = 0, ncns = 0;
    const SkPoint* pts = skia_rs_path_buf_points(buf, &npts);
    const uint8_t* vbs = skia_rs_path_buf_verbs(buf, &nvbs);
    const float* cns = skia_rs_path_buf_conics(buf, &ncns);
    auto meta = skia_rs_path_buf_meta(buf);

    dst->reset();
    dst->setFillType(static_cast<SkPathFillType>(meta.fill_type));
    dst->setIsVolatile(meta.is_volatile);
    if (nvbs > 0) {
        SkPathRaw raw{
            {pts, npts},
            {reinterpret_cast<const SkPathVerb*>(vbs), nvbs},
            {cns, ncns},
            SkRect::MakeEmpty(),
            static_cast<SkPathFillType>(meta.fill_type),
            static_cast<SkPathConvexity>(meta.convexity),
            meta.segment_mask,
        };
        dst->addRaw(raw, SkPathBuilder::Reserve::kGrow);
    }
    skia_rs_path_buf_free(buf);
}

}  // namespace

// ============================================================================
// SkDashPath.cpp
// ============================================================================

void SkDashPath::CalcDashParameters(SkScalar phase, SkSpan<const SkScalar> intervals,
                                    SkScalar* initialDashLength, size_t* initialDashIndex,
                                    SkScalar* intervalLength, SkScalar* adjustedPhase) {
    skia_rs_dash_calc_parameters(
            phase, intervals.data(), intervals.size(),
            initialDashLength, initialDashIndex, intervalLength, adjustedPhase);
}

bool SkDashPath::InternalFilter(SkPathBuilder* dst, const SkPath& src, SkStrokeRec* rec,
                                const SkRect* cullRect, SkSpan<const SkScalar> aIntervals,
                                SkScalar initialDashLength, int32_t initialDashIndex,
                                SkScalar intervalLength, SkScalar startPhase,
                                StrokeRecApplication strokeRecApplication) {
    if (src.points().empty()) {
        return true;
    }
    auto srcRawOpt = SkPathPriv::Raw(src, SkResolveConvexity::kNo);
    if (!srcRawOpt) {
        return false;
    }
    const SkPathRaw& raw = *srcRawOpt;

    SkiaRsStrokeRec ioRec{};
    ioRec.res_scale = rec->getResScale();
    ioRec.width = rec->getWidth();
    ioRec.miter_limit = rec->getMiter();
    ioRec.cap = static_cast<uint8_t>(rec->getCap());
    ioRec.join = static_cast<uint8_t>(rec->getJoin());
    ioRec.stroke_and_fill = (rec->getStyle() == SkStrokeRec::kStrokeAndFill_Style);

    auto dstPts = dst->points();
    auto dstVbs = dst->verbs();
    auto dstCns = dst->conicWeights();
    int32_t dstLastMove = dstVbs.empty()
            ? -1
            : SkPathPriv::FindLastMoveToIndex(dstVbs, dstPts.size());

    bool ok = false;
    auto* buf = skia_rs_dash_internal_filter(
            dstPts.data(), dstPts.size(),
            reinterpret_cast<const uint8_t*>(dstVbs.data()), dstVbs.size(),
            dstCns.data(), dstCns.size(),
            static_cast<uint8_t>(dst->fillType()),
            static_cast<uint8_t>(SkPathConvexity::kUnknown),
            SkPathPriv::ComputeSegmentMask(dstVbs),
            dstLastMove, 0, 0, 0, false,
            raw.fPoints.data(), raw.fPoints.size(),
            reinterpret_cast<const uint8_t*>(raw.fVerbs.data()), raw.fVerbs.size(),
            raw.fConics.data(), raw.fConics.size(),
            raw.fBounds,
            static_cast<uint8_t>(raw.fFillType),
            static_cast<uint8_t>(raw.fConvexity),
            raw.fSegmentMask,
            &ioRec,
            cullRect,
            aIntervals.data(), aIntervals.size(),
            initialDashLength, initialDashIndex, intervalLength, startPhase,
            strokeRecApplication == StrokeRecApplication::kAllow,
            &ok);

    if (ioRec.width < 0) {
        rec->setFillStyle();
    } else if (ioRec.width == 0) {
        rec->setHairlineStyle();
    } else {
        rec->setStrokeStyle(ioRec.width, ioRec.stroke_and_fill);
    }
    rec->setStrokeParams(static_cast<SkPaint::Cap>(ioRec.cap),
                         static_cast<SkPaint::Join>(ioRec.join),
                         ioRec.miter_limit);

    replace_builder_from_rust_buf(dst, buf);
    return ok;
}

bool SkDashPath::FilterDashPath(SkPathBuilder* dst, const SkPath& src, SkStrokeRec* rec,
                                const SkRect* cullRect, const SkPathEffectBase::DashInfo& info) {
    if (!ValidDashPath(info.fPhase, info.fIntervals)) {
        return false;
    }
    SkScalar initialDashLength = 0;
    size_t initialDashIndex = 0;
    SkScalar intervalLength = 0;
    CalcDashParameters(info.fPhase, info.fIntervals, &initialDashLength,
                       &initialDashIndex, &intervalLength);
    return InternalFilter(dst, src, rec, cullRect, info.fIntervals, initialDashLength,
                          initialDashIndex, intervalLength, info.fPhase);
}

bool SkDashPath::ValidDashPath(SkScalar phase, SkSpan<const SkScalar> intervals) {
    return skia_rs_dash_valid_path(phase, intervals.data(), intervals.size());
}

// ============================================================================
// SkParsePath.cpp
// ============================================================================

std::optional<SkPath> SkParsePath::FromSVGString(const char data[]) {
    if (!data) {
        return {};
    }
    auto* buf = skia_rs_parse_path_from_svg(
            reinterpret_cast<const uint8_t*>(data), strlen(data));
    if (!buf) {
        return {};
    }
    size_t npts = 0, nvbs = 0, ncns = 0;
    const SkPoint* pts = skia_rs_path_buf_points(buf, &npts);
    const uint8_t* vbs = skia_rs_path_buf_verbs(buf, &nvbs);
    const float* cns = skia_rs_path_buf_conics(buf, &ncns);
    auto meta = skia_rs_path_buf_meta(buf);
    SkPath path = SkPath::Raw(
            {pts, npts},
            {reinterpret_cast<const SkPathVerb*>(vbs), nvbs},
            {cns, ncns},
            static_cast<SkPathFillType>(meta.fill_type),
            meta.is_volatile);
    skia_rs_path_buf_free(buf);
    return path;
}

SkString SkParsePath::ToSVGString(const SkPath& path, PathEncoding encoding) {
    auto pts = path.points();
    auto vbs = path.verbs();
    auto cns = path.conicWeights();
    auto* rs = skia_rs_parse_path_to_svg(
            pts.data(), pts.size(),
            reinterpret_cast<const uint8_t*>(vbs.data()), vbs.size(),
            cns.data(), cns.size(),
            encoding == PathEncoding::Relative ? 1 : 0);
    if (!rs) {
        return SkString();
    }
    size_t len = 0;
    const uint8_t* data = skia_rs_string_data(rs, &len);
    SkString out(reinterpret_cast<const char*>(data), len);
    skia_rs_string_free(rs);
    return out;
}
