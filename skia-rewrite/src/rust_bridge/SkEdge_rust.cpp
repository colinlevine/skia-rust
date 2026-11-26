/*
 * Safe Rust bridge for:
 *   - src/core/SkEdge.cpp
 *   - src/core/SkAnalyticEdge.cpp
 *   - src/core/SkEdgeBuilder.cpp
 *   - src/core/SkAlphaRuns.cpp
 * (Phase 1 Layer 3.2: skia-rs-scan)
 */

#include "include/core/SkPath.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFixed.h"
#include "include/private/SkTo.h"
#include "src/core/SkAlphaRuns.h"
#include "src/core/SkAnalyticEdge.h"
#include "src/core/SkEdge.h"
#include "src/core/SkEdgeBuilder.h"
#include "src/core/SkMemset.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkPathRaw.h"

#include <cstddef>
#include <cstdint>

extern "C" {

struct SkiaRsEdgeBase {
    int32_t x;
    int32_t dx_dy;
    int32_t first_y;
    int32_t last_y;
    int8_t edge_type;
    int8_t winding;
    uint8_t segment_count;
    uint8_t curve_shift;
};
static_assert(sizeof(SkiaRsEdgeBase) == 20);

struct SkiaRsQuadEdgeState {
    SkiaRsEdgeBase base;
    int32_t qx;
    int32_t qy;
    int32_t q_dx_dt;
    int32_t q_dy_dt;
    int32_t q_d2x_dt2;
    int32_t q_d2y_dt2;
    int32_t q_last_x;
    int32_t q_last_y;
};
static_assert(sizeof(SkiaRsQuadEdgeState) == 52);

struct SkiaRsCubicEdgeState {
    SkiaRsEdgeBase base;
    int32_t cx;
    int32_t cy;
    int32_t c_dx_dt;
    int32_t c_dy_dt;
    int32_t c_d2x_dt2;
    int32_t c_d2y_dt2;
    int32_t c_d3x_dt3;
    int32_t c_d3y_dt3;
    int32_t c_last_x;
    int32_t c_last_y;
    uint8_t to_fixed_shift;
    uint8_t _pad[3];
};
static_assert(sizeof(SkiaRsCubicEdgeState) == 64);

struct SkiaRsAnalyticEdgeBase {
    int32_t x;
    int32_t dx;
    int32_t upper_x;
    int32_t y;
    int32_t upper_y;
    int32_t lower_y;
    int32_t dy;
    int8_t edge_type;
    int8_t curve_count;
    uint8_t curve_shift;
    int8_t winding;
};
static_assert(sizeof(SkiaRsAnalyticEdgeBase) == 32);

struct SkiaRsAnalyticQuadState {
    SkiaRsAnalyticEdgeBase base;
    int32_t qx;
    int32_t qy;
    int32_t q_dx;
    int32_t q_dy;
    int32_t q_ddx;
    int32_t q_ddy;
    int32_t q_last_x;
    int32_t q_last_y;
    int32_t snapped_x;
    int32_t snapped_y;
};
static_assert(sizeof(SkiaRsAnalyticQuadState) == 72);

struct SkiaRsAnalyticCubicState {
    SkiaRsAnalyticEdgeBase base;
    int32_t cx;
    int32_t cy;
    int32_t c_dx;
    int32_t c_dy;
    int32_t c_ddx;
    int32_t c_ddy;
    int32_t c_dddx;
    int32_t c_dddy;
    int32_t c_last_x;
    int32_t c_last_y;
    int32_t snapped_y;
    uint8_t to_fixed_shift;
    uint8_t _pad[3];
};
static_assert(sizeof(SkiaRsAnalyticCubicState) == 80);

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

struct SkiaRsEdgeSinkVTable {
    void* ctx;
    void (*add_line)(void*, const SkPoint*);
    void (*add_quad)(void*, const SkPoint*);
    void (*add_cubic)(void*, const SkPoint*);
};
static_assert(sizeof(SkiaRsEdgeSinkVTable) == 4 * sizeof(void*));

bool skia_rs_edge_set_line(
        SkiaRsEdgeBase* state, const SkPoint* p0, const SkPoint* p1, const SkIRect* clip);
bool skia_rs_edge_update_line(
        SkiaRsEdgeBase* state, int32_t x_start, int32_t y_start, int32_t x_end, int32_t y_end);
void skia_rs_edge_chop_line_with_clip(SkiaRsEdgeBase* state, const SkIRect* clip);
bool skia_rs_quad_edge_set_quadratic(SkiaRsQuadEdgeState* state, const SkPoint* pts);
bool skia_rs_quad_edge_next_segment(SkiaRsQuadEdgeState* state);
bool skia_rs_cubic_edge_set_cubic(SkiaRsCubicEdgeState* state, const SkPoint* pts);
bool skia_rs_cubic_edge_next_segment(SkiaRsCubicEdgeState* state);

bool skia_rs_analytic_edge_set_line(
        SkiaRsAnalyticEdgeBase* state, const SkPoint* p0, const SkPoint* p1);
bool skia_rs_analytic_edge_update_line(
        SkiaRsAnalyticEdgeBase* state,
        int32_t x0,
        int32_t y0,
        int32_t x1,
        int32_t y1,
        int32_t slope);
bool skia_rs_analytic_quad_set_without_update(
        SkiaRsAnalyticQuadState* state, const SkPoint* pts, int32_t shift);
bool skia_rs_analytic_quad_set(SkiaRsAnalyticQuadState* state, const SkPoint* pts);
bool skia_rs_analytic_quad_update(SkiaRsAnalyticQuadState* state);
bool skia_rs_analytic_cubic_set_without_update(
        SkiaRsAnalyticCubicState* state, const SkPoint* pts, int32_t shift);
bool skia_rs_analytic_cubic_set(SkiaRsAnalyticCubicState* state, const SkPoint* pts);
bool skia_rs_analytic_cubic_update(SkiaRsAnalyticCubicState* state);

int32_t skia_rs_edge_builder_combine_vertical_basic(
        const SkiaRsEdgeBase* edge, SkiaRsEdgeBase* last, bool can_cull_to_the_right);
int32_t skia_rs_edge_builder_combine_vertical_analytic(
        const SkiaRsAnalyticEdgeBase* edge,
        SkiaRsAnalyticEdgeBase* last,
        bool can_cull_to_the_right);
bool skia_rs_edge_builder_drive(
        const SkiaRsPathRawView* raw,
        const SkIRect* iclip,
        bool can_cull_to_the_right,
        const SkiaRsEdgeSinkVTable* sink);

void skia_rs_alpha_runs_reset(int16_t* runs, uint8_t* alpha, int32_t width);
bool skia_rs_alpha_runs_validate(const int16_t* runs, int32_t width);
bool skia_rs_alpha_runs_assert_valid(
        const int16_t* runs, const uint8_t* alpha, int32_t width, int32_t y, int32_t max_step);

}  // extern "C"

namespace {

SkiaRsAnalyticEdgeBase load_analytic_base(const SkAnalyticEdge& e) {
    return {
        e.fX,
        e.fDX,
        e.fUpperX,
        e.fY,
        e.fUpperY,
        e.fLowerY,
        e.fDY,
        static_cast<int8_t>(e.fEdgeType),
        e.fCurveCount,
        e.fCurveShift,
        static_cast<int8_t>(e.fWinding),
    };
}

void store_analytic_base(SkAnalyticEdge& e, const SkiaRsAnalyticEdgeBase& s) {
    e.fX = s.x;
    e.fDX = s.dx;
    e.fUpperX = s.upper_x;
    e.fY = s.y;
    e.fUpperY = s.upper_y;
    e.fLowerY = s.lower_y;
    e.fDY = s.dy;
    e.fEdgeType = static_cast<SkAnalyticEdge::Type>(s.edge_type);
    e.fCurveCount = s.curve_count;
    e.fCurveShift = s.curve_shift;
    e.fWinding = static_cast<SkAnalyticEdge::Winding>(s.winding);
}

SkiaRsAnalyticQuadState load_analytic_quad(const SkAnalyticQuadraticEdge& e) {
    return {
        load_analytic_base(e),
        e.fQx,
        e.fQy,
        e.fQDx,
        e.fQDy,
        e.fQDDx,
        e.fQDDy,
        e.fQLastX,
        e.fQLastY,
        e.fSnappedX,
        e.fSnappedY,
    };
}

void store_analytic_quad(SkAnalyticQuadraticEdge& e, const SkiaRsAnalyticQuadState& s) {
    store_analytic_base(e, s.base);
    e.fQx = s.qx;
    e.fQy = s.qy;
    e.fQDx = s.q_dx;
    e.fQDy = s.q_dy;
    e.fQDDx = s.q_ddx;
    e.fQDDy = s.q_ddy;
    e.fQLastX = s.q_last_x;
    e.fQLastY = s.q_last_y;
    e.fSnappedX = s.snapped_x;
    e.fSnappedY = s.snapped_y;
}

SkiaRsAnalyticCubicState load_analytic_cubic(const SkAnalyticCubicEdge& e) {
    return {
        load_analytic_base(e),
        e.fCx,
        e.fCy,
        e.fCDx,
        e.fCDy,
        e.fCDDx,
        e.fCDDy,
        e.fCDDDx,
        e.fCDDDy,
        e.fCLastX,
        e.fCLastY,
        e.fSnappedY,
        e.fToFixedShift,
        {0, 0, 0},
    };
}

void store_analytic_cubic(SkAnalyticCubicEdge& e, const SkiaRsAnalyticCubicState& s) {
    store_analytic_base(e, s.base);
    e.fCx = s.cx;
    e.fCy = s.cy;
    e.fCDx = s.c_dx;
    e.fCDy = s.c_dy;
    e.fCDDx = s.c_ddx;
    e.fCDDy = s.c_ddy;
    e.fCDDDx = s.c_dddx;
    e.fCDDDy = s.c_dddy;
    e.fCLastX = s.c_last_x;
    e.fCLastY = s.c_last_y;
    e.fSnappedY = s.snapped_y;
    e.fToFixedShift = s.to_fixed_shift;
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

bool is_vertical(const SkEdge* edge) {
    return edge->fDxDy == 0 && edge->fEdgeType == SkEdge::Type::kLine;
}

bool is_vertical(const SkAnalyticEdge* edge) {
    return edge->fDX == 0 && edge->fEdgeType == SkAnalyticEdge::Type::kLine;
}

}  // namespace

// ============================================================================
// SkEdge / SkQuadraticEdge / SkCubicEdge (replaces src/core/SkEdge.cpp)
// ============================================================================

#if defined(SK_DEBUG)
void SkEdge::dump() const {
    SkASSERT(fSegmentCount == 0);
    SkDebugf("line edge: firstY:%d lastY:%d x:%g dx/dy:%g\n"
             "\twinding:%d curveShift:%u\n",
             fFirstY,
             fLastY,
             SkFixedToFloat(fX),
             SkFixedToFloat(fDxDy),
             static_cast<int8_t>(fWinding),
             fCurveShift);
}

void SkQuadraticEdge::dump() const {
    SkDebugf("quad edge; %u segment(s) left: firstY:%d lastY:%d x:%g dx/dy:%g\n"
             "\tqx:%g qy:%g dqx:%g dqy:%g ddqx:%g ddqy:%g qLastX:%g qLastY:%g\n"
             "\twinding:%d curveShift:%u\n",
             fSegmentCount,
             fFirstY,
             fLastY,
             SkFixedToFloat(fX),
             SkFixedToFloat(fDxDy),
             SkFixedToFloat(fQx),
             SkFixedToFloat(fQy),
             SkFixedToFloat(fQDxDt),
             SkFixedToFloat(fQDyDt),
             SkFixedToFloat(fQD2xDt2),
             SkFixedToFloat(fQD2yDt2),
             SkFixedToFloat(fQLastX),
             SkFixedToFloat(fQLastY),
             static_cast<int8_t>(fWinding),
             fCurveShift);
}

void SkCubicEdge::dump() const {
    SkDebugf("cube edge; %u segment(s) left: firstY:%d lastY:%d x:%g dx/dy:%g\n"
             "qx:%g qy:%g dcx:%g dcy:%g ddcx:%g ddcy:%g dddcx:%g dddcy:%g cLastX:%g cLastY:%g\n"
             "\twinding:%d curveShift:%u toFixedShift:%u\n",
             fSegmentCount,
             fFirstY,
             fLastY,
             SkFixedToFloat(fX),
             SkFixedToFloat(fDxDy),
             SkFixedToFloat(fCx),
             SkFixedToFloat(fCy),
             SkFixedToFloat(fCDxDt),
             SkFixedToFloat(fCDyDt),
             SkFixedToFloat(fCD2xDt2),
             SkFixedToFloat(fCD2yDt2),
             SkFixedToFloat(fCD3xDt3),
             SkFixedToFloat(fCD3yDt3),
             SkFixedToFloat(fCLastX),
             SkFixedToFloat(fCLastY),
             static_cast<int8_t>(fWinding),
             fCurveShift,
             fToFixedShift);
}
#endif

bool SkEdge::setLine(const SkPoint& p0, const SkPoint& p1, const SkIRect* clip) {
    SkiaRsEdgeBase s{};
    if (!skia_rs_edge_set_line(&s, &p0, &p1, clip)) {
        return false;
    }
    fX = s.x;
    fDxDy = s.dx_dy;
    fFirstY = s.first_y;
    fLastY = s.last_y;
    fEdgeType = static_cast<Type>(s.edge_type);
    fWinding = static_cast<Winding>(s.winding);
    fSegmentCount = s.segment_count;
    fCurveShift = s.curve_shift;
    return true;
}

bool SkEdge::nextSegment() {
    SkDEBUGFAILF("Shouldn't be asking a linear edge to go to the next curve.");
    return false;
}

bool SkEdge::updateLine(SkFixed xStart, SkFixed yStart, SkFixed xEnd, SkFixed yEnd) {
    SkiaRsEdgeBase s = {
        fX,
        fDxDy,
        fFirstY,
        fLastY,
        static_cast<int8_t>(fEdgeType),
        static_cast<int8_t>(fWinding),
        fSegmentCount,
        fCurveShift,
    };
    if (!skia_rs_edge_update_line(&s, xStart, yStart, xEnd, yEnd)) {
        return false;
    }
    fX = s.x;
    fDxDy = s.dx_dy;
    fFirstY = s.first_y;
    fLastY = s.last_y;
    return true;
}

void SkEdge::chopLineWithClip(const SkIRect& clip) {
    SkiaRsEdgeBase s = {
        fX,
        fDxDy,
        fFirstY,
        fLastY,
        static_cast<int8_t>(fEdgeType),
        static_cast<int8_t>(fWinding),
        fSegmentCount,
        fCurveShift,
    };
    skia_rs_edge_chop_line_with_clip(&s, &clip);
    fX = s.x;
    fFirstY = s.first_y;
}

bool SkQuadraticEdge::setQuadratic(const SkPoint pts[3]) {
    SkiaRsQuadEdgeState s{};
    bool ok = skia_rs_quad_edge_set_quadratic(&s, pts);
    fX = s.base.x;
    fDxDy = s.base.dx_dy;
    fFirstY = s.base.first_y;
    fLastY = s.base.last_y;
    fEdgeType = static_cast<Type>(s.base.edge_type);
    fWinding = static_cast<Winding>(s.base.winding);
    fSegmentCount = s.base.segment_count;
    fCurveShift = s.base.curve_shift;
    fQx = s.qx;
    fQy = s.qy;
    fQDxDt = s.q_dx_dt;
    fQDyDt = s.q_dy_dt;
    fQD2xDt2 = s.q_d2x_dt2;
    fQD2yDt2 = s.q_d2y_dt2;
    fQLastX = s.q_last_x;
    fQLastY = s.q_last_y;
    return ok;
}

bool SkQuadraticEdge::nextSegment() {
    SkiaRsQuadEdgeState s = {
        {
            fX,
            fDxDy,
            fFirstY,
            fLastY,
            static_cast<int8_t>(fEdgeType),
            static_cast<int8_t>(fWinding),
            fSegmentCount,
            fCurveShift,
        },
        fQx,
        fQy,
        fQDxDt,
        fQDyDt,
        fQD2xDt2,
        fQD2yDt2,
        fQLastX,
        fQLastY,
    };
    bool ok = skia_rs_quad_edge_next_segment(&s);
    fX = s.base.x;
    fDxDy = s.base.dx_dy;
    fFirstY = s.base.first_y;
    fLastY = s.base.last_y;
    fSegmentCount = s.base.segment_count;
    fQx = s.qx;
    fQy = s.qy;
    fQDxDt = s.q_dx_dt;
    fQDyDt = s.q_dy_dt;
    return ok;
}

bool SkCubicEdge::setCubic(const SkPoint pts[4]) {
    SkiaRsCubicEdgeState s{};
    bool ok = skia_rs_cubic_edge_set_cubic(&s, pts);
    fX = s.base.x;
    fDxDy = s.base.dx_dy;
    fFirstY = s.base.first_y;
    fLastY = s.base.last_y;
    fEdgeType = static_cast<Type>(s.base.edge_type);
    fWinding = static_cast<Winding>(s.base.winding);
    fSegmentCount = s.base.segment_count;
    fCurveShift = s.base.curve_shift;
    fCx = s.cx;
    fCy = s.cy;
    fCDxDt = s.c_dx_dt;
    fCDyDt = s.c_dy_dt;
    fCD2xDt2 = s.c_d2x_dt2;
    fCD2yDt2 = s.c_d2y_dt2;
    fCD3xDt3 = s.c_d3x_dt3;
    fCD3yDt3 = s.c_d3y_dt3;
    fCLastX = s.c_last_x;
    fCLastY = s.c_last_y;
    fToFixedShift = s.to_fixed_shift;
    return ok;
}

bool SkCubicEdge::nextSegment() {
    SkiaRsCubicEdgeState s = {
        {
            fX,
            fDxDy,
            fFirstY,
            fLastY,
            static_cast<int8_t>(fEdgeType),
            static_cast<int8_t>(fWinding),
            fSegmentCount,
            fCurveShift,
        },
        fCx,
        fCy,
        fCDxDt,
        fCDyDt,
        fCD2xDt2,
        fCD2yDt2,
        fCD3xDt3,
        fCD3yDt3,
        fCLastX,
        fCLastY,
        fToFixedShift,
        {0, 0, 0},
    };
    bool ok = skia_rs_cubic_edge_next_segment(&s);
    fX = s.base.x;
    fDxDy = s.base.dx_dy;
    fFirstY = s.base.first_y;
    fLastY = s.base.last_y;
    fSegmentCount = s.base.segment_count;
    fCx = s.cx;
    fCy = s.cy;
    fCDxDt = s.c_dx_dt;
    fCDyDt = s.c_dy_dt;
    fCD2xDt2 = s.c_d2x_dt2;
    fCD2yDt2 = s.c_d2y_dt2;
    return ok;
}

// ============================================================================
// SkAnalyticEdge (replaces src/core/SkAnalyticEdge.cpp)
// ============================================================================

bool SkAnalyticEdge::setLine(const SkPoint& p0, const SkPoint& p1) {
    SkiaRsAnalyticEdgeBase s{};
    if (!skia_rs_analytic_edge_set_line(&s, &p0, &p1)) {
        return false;
    }
    store_analytic_base(*this, s);
    return true;
}

bool SkAnalyticEdge::updateLine(SkFixed x0, SkFixed y0, SkFixed x1, SkFixed y1, SkFixed slope) {
    SkiaRsAnalyticEdgeBase s = load_analytic_base(*this);
    bool ok = skia_rs_analytic_edge_update_line(&s, x0, y0, x1, y1, slope);
    store_analytic_base(*this, s);
    return ok;
}

bool SkAnalyticEdge::update(SkFixed last_y) {
    SkASSERT(last_y >= fLowerY);
    if (fCurveCount < 0) {
        return static_cast<SkAnalyticCubicEdge*>(this)->updateCubic();
    } else if (fCurveCount > 0) {
        return static_cast<SkAnalyticQuadraticEdge*>(this)->updateQuadratic();
    }
    return false;
}

bool SkAnalyticQuadraticEdge::setQuadraticWithoutUpdate(const SkPoint pts[3], int shift) {
    SkiaRsAnalyticQuadState s{};
    if (!skia_rs_analytic_quad_set_without_update(&s, pts, shift)) {
        return false;
    }
    store_analytic_quad(*this, s);
    return true;
}

bool SkAnalyticQuadraticEdge::setQuadratic(const SkPoint pts[3]) {
    SkiaRsAnalyticQuadState s{};
    bool ok = skia_rs_analytic_quad_set(&s, pts);
    store_analytic_quad(*this, s);
    return ok;
}

bool SkAnalyticQuadraticEdge::updateQuadratic() {
    SkiaRsAnalyticQuadState s = load_analytic_quad(*this);
    bool ok = skia_rs_analytic_quad_update(&s);
    store_analytic_quad(*this, s);
    return ok;
}

bool SkAnalyticCubicEdge::setCubicWithoutUpdate(const SkPoint pts[4], int shift) {
    SkiaRsAnalyticCubicState s{};
    if (!skia_rs_analytic_cubic_set_without_update(&s, pts, shift)) {
        return false;
    }
    store_analytic_cubic(*this, s);
    return true;
}

bool SkAnalyticCubicEdge::setCubic(const SkPoint pts[4]) {
    SkiaRsAnalyticCubicState s{};
    bool ok = skia_rs_analytic_cubic_set(&s, pts);
    store_analytic_cubic(*this, s);
    return ok;
}

bool SkAnalyticCubicEdge::updateCubic() {
    SkiaRsAnalyticCubicState s = load_analytic_cubic(*this);
    bool ok = skia_rs_analytic_cubic_update(&s);
    store_analytic_cubic(*this, s);
    return ok;
}

// ============================================================================
// SkEdgeBuilder (replaces src/core/SkEdgeBuilder.cpp)
// ============================================================================

SkEdgeBuilder::Combine SkBasicEdgeBuilder::combineVertical(const SkEdge* edge, SkEdge* last) {
    SkiaRsEdgeBase e = {
        edge->fX,
        edge->fDxDy,
        edge->fFirstY,
        edge->fLastY,
        static_cast<int8_t>(edge->fEdgeType),
        static_cast<int8_t>(edge->fWinding),
        edge->segmentsLeft(),
        0,
    };
    SkiaRsEdgeBase l = {
        last->fX,
        last->fDxDy,
        last->fFirstY,
        last->fLastY,
        static_cast<int8_t>(last->fEdgeType),
        static_cast<int8_t>(last->fWinding),
        last->segmentsLeft(),
        0,
    };
    int32_t res = skia_rs_edge_builder_combine_vertical_basic(&e, &l, fCanCullToTheRight);
    last->fFirstY = l.first_y;
    last->fLastY = l.last_y;
    last->fWinding = static_cast<SkEdge::Winding>(l.winding);
    return static_cast<Combine>(res);
}

SkEdgeBuilder::Combine SkAnalyticEdgeBuilder::combineVertical(const SkAnalyticEdge* edge,
                                                              SkAnalyticEdge* last) {
    SkiaRsAnalyticEdgeBase e = load_analytic_base(*edge);
    SkiaRsAnalyticEdgeBase l = load_analytic_base(*last);
    int32_t res = skia_rs_edge_builder_combine_vertical_analytic(&e, &l, fCanCullToTheRight);
    store_analytic_base(*last, l);
    return static_cast<Combine>(res);
}

void SkBasicEdgeBuilder::addLine(const SkPoint pts[]) {
    SkEdge* edge = fAlloc.make<SkEdge>();
    if (edge->setLine(pts[0], pts[1])) {
        Combine combine = is_vertical(edge) && !fList.empty()
                                  ? this->combineVertical(edge, static_cast<SkEdge*>(fList.back()))
                                  : kNo_Combine;
        switch (combine) {
            case kTotal_Combine:
                fList.pop_back();
                break;
            case kPartial_Combine:
                break;
            case kNo_Combine:
                fList.push_back(edge);
                break;
        }
    }
}

void SkAnalyticEdgeBuilder::addLine(const SkPoint pts[]) {
    SkAnalyticEdge* edge = fAlloc.make<SkAnalyticEdge>();
    if (edge->setLine(pts[0], pts[1])) {
        Combine combine =
                is_vertical(edge) && !fList.empty()
                        ? this->combineVertical(edge, static_cast<SkAnalyticEdge*>(fList.back()))
                        : kNo_Combine;
        switch (combine) {
            case kTotal_Combine:
                fList.pop_back();
                break;
            case kPartial_Combine:
                break;
            case kNo_Combine:
                fList.push_back(edge);
                break;
        }
    }
}

void SkBasicEdgeBuilder::addQuad(const SkPoint pts[]) {
    SkQuadraticEdge* edge = fAlloc.make<SkQuadraticEdge>();
    if (edge->setQuadratic(pts)) {
        fList.push_back(edge);
    }
}

void SkAnalyticEdgeBuilder::addQuad(const SkPoint pts[]) {
    SkAnalyticQuadraticEdge* edge = fAlloc.make<SkAnalyticQuadraticEdge>();
    if (edge->setQuadratic(pts)) {
        fList.push_back(edge);
    }
}

void SkBasicEdgeBuilder::addCubic(const SkPoint pts[]) {
    SkCubicEdge* edge = fAlloc.make<SkCubicEdge>();
    if (edge->setCubic(pts)) {
        fList.push_back(edge);
    }
}

void SkAnalyticEdgeBuilder::addCubic(const SkPoint pts[]) {
    SkAnalyticCubicEdge* edge = fAlloc.make<SkAnalyticCubicEdge>();
    if (edge->setCubic(pts)) {
        fList.push_back(edge);
    }
}

SkEdgeBuilder::Combine SkAnalyticEdgeBuilder::addPolyLine(const SkPoint pts[],
                                                          char* arg_edge,
                                                          char** arg_edgePtr) {
    auto edge = reinterpret_cast<SkAnalyticEdge*>(arg_edge);
    auto edgePtr = reinterpret_cast<SkAnalyticEdge**>(arg_edgePtr);

    if (edge->setLine(pts[0], pts[1])) {
        return is_vertical(edge) && edgePtr > reinterpret_cast<SkAnalyticEdge**>(fEdgeList)
                       ? this->combineVertical(edge, edgePtr[-1])
                       : kNo_Combine;
    }
    return SkEdgeBuilder::kPartial_Combine;
}

SkRect SkBasicEdgeBuilder::recoverClip(const SkIRect& src) const { return SkRect::Make(src); }
SkRect SkAnalyticEdgeBuilder::recoverClip(const SkIRect& src) const { return SkRect::Make(src); }

char* SkAnalyticEdgeBuilder::allocEdges(size_t n, size_t* size) {
    *size = sizeof(SkAnalyticEdge);
    return reinterpret_cast<char*>(fAlloc.makeArrayDefault<SkAnalyticEdge>(n));
}

int SkEdgeBuilder::buildPoly(const SkPathRaw& raw, const SkIRect* iclip, bool canCullToTheRight) {
    return this->build(raw, iclip, canCullToTheRight);
}

int SkEdgeBuilder::build(const SkPathRaw& raw, const SkIRect* iclip, bool canCullToTheRight) {
    SkiaRsPathRawView view = to_path_raw_view(raw);
    SkiaRsEdgeSinkVTable sink = {
        this,
        +[](void* ctx, const SkPoint* pts) {
            static_cast<SkEdgeBuilder*>(ctx)->addLine(pts);
        },
        +[](void* ctx, const SkPoint* pts) {
            static_cast<SkEdgeBuilder*>(ctx)->addQuad(pts);
        },
        +[](void* ctx, const SkPoint* pts) {
            static_cast<SkEdgeBuilder*>(ctx)->addCubic(pts);
        },
    };
    bool ok = skia_rs_edge_builder_drive(&view, iclip, canCullToTheRight, &sink);
    fEdgeList = fList.begin();
    return ok ? fList.size() : 0;
}

int SkEdgeBuilder::buildEdges(const SkPathRaw& raw, const SkIRect* shiftedClip) {
    const bool canCullToTheRight = !raw.isKnownToBeConvex();
    fCanCullToTheRight = canCullToTheRight;

    const int count = SkPath::kLine_SegmentMask == raw.segmentMasks()
                              ? this->buildPoly(raw, shiftedClip, canCullToTheRight)
                              : this->build(raw, shiftedClip, canCullToTheRight);

    SkASSERT(count >= 0);
    if (!canCullToTheRight) {
        SkASSERT(count != 1);
    }
    return count;
}

int SkEdgeBuilder::buildEdges(const SkPath& path, const SkIRect* shiftedClip) {
    if (auto raw = SkPathPriv::Raw(path, SkResolveConvexity::kYes)) {
        return this->buildEdges(*raw, shiftedClip);
    }
    return 0;
}

// ============================================================================
// SkAlphaRuns (replaces src/core/SkAlphaRuns.cpp)
// ============================================================================

void SkAlphaRuns::reset(int width) {
    SkASSERT(width > 0);
#ifdef SK_DEBUG
    SkOpts::memset16(reinterpret_cast<uint16_t*>(fRuns), static_cast<uint16_t>(-42), width);
#endif
    skia_rs_alpha_runs_reset(fRuns, fAlpha, width);
    SkDEBUGCODE(fWidth = width;)
    SkDEBUGCODE(this->validate();)
}

#ifdef SK_DEBUG
void SkAlphaRuns::assertValid(int y, int maxStep) const {
    SkASSERT(skia_rs_alpha_runs_assert_valid(fRuns, fAlpha, fWidth, y, maxStep));
}

void SkAlphaRuns::dump() const {
    const int16_t* runs = fRuns;
    const uint8_t* alpha = fAlpha;

    SkDebugf("Runs");
    while (*runs) {
        int n = *runs;
        SkDebugf(" %02x", *alpha);
        if (n > 1) {
            SkDebugf(",%d", n);
        }
        alpha += n;
        runs += n;
    }
    SkDebugf("\n");
}

void SkAlphaRuns::validate() const {
    SkASSERT(skia_rs_alpha_runs_validate(fRuns, fWidth));
}
#endif
