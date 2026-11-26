/*
 * Safe Rust bridge for Phase 1 Layer 3.1 skia-rs-pathops:
 *   Public API (Op, Simplify, AsWinding, ComputeTightBounds, SkOpBuilder),
 *   SkPathOpsTypes, SkPathOpsLine/Quad/Conic/Cubic/Curve/Rect/CubicHull/CubicToQuads,
 *   SkReduceOrder, and SkIntersections / SkPathOpsTSect.
 */

#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathTypes.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkScalar.h"
#include "include/core/SkTypes.h"
#include "include/pathops/SkPathOps.h"
#include "src/core/SkGeometry.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkPathRaw.h"
#include "src/pathops/SkIntersections.h"
#include "src/pathops/SkPathOpsBounds.h"
#include "src/pathops/SkPathOpsCommon.h"
#include "src/pathops/SkPathOpsConic.h"
#include "src/pathops/SkPathOpsCubic.h"
#include "src/pathops/SkPathOpsCurve.h"
#include "src/pathops/SkPathOpsLine.h"
#include "src/pathops/SkPathOpsPoint.h"
#include "src/pathops/SkPathOpsQuad.h"
#include "src/pathops/SkPathOpsRect.h"
#include "src/pathops/SkPathOpsTypes.h"
#include "src/pathops/SkReduceOrder.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <vector>

struct SkiaRsPathOpsBuf;

struct FfiPathInput {
    const SkPoint* pts;
    size_t pt_count;
    const uint8_t* vbs;
    size_t vb_count;
    const float* cns;
    size_t cn_count;
    uint8_t fill_type;
    uint8_t convexity;
    uint8_t op;
};

struct FfiIntersections {
    SkDPoint pt[13];
    SkDPoint pt2[2];
    double t[2][13];
    uint16_t is_coincident[2];
    bool nearly_same[2];
    uint8_t used;
    uint8_t max;
    bool allow_near;
    bool swap;
};

static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkDPoint) == 16);
static_assert(sizeof(SkDVector) == 16);
static_assert(sizeof(SkDRect) == 32);
static_assert(sizeof(SkPathOpsBounds) == 16);

extern "C" {
const SkPoint* skia_rs_pathops_buf_points(const SkiaRsPathOpsBuf* buf, size_t* out_len);
const uint8_t* skia_rs_pathops_buf_verbs(const SkiaRsPathOpsBuf* buf, size_t* out_len);
const float* skia_rs_pathops_buf_conics(const SkiaRsPathOpsBuf* buf, size_t* out_len);
uint8_t skia_rs_pathops_buf_fill_type(const SkiaRsPathOpsBuf* buf);
void skia_rs_pathops_buf_free(SkiaRsPathOpsBuf* buf);

SkiaRsPathOpsBuf* skia_rs_pathops_op(
        const SkPoint* p1_pts, size_t p1_pt_count,
        const uint8_t* p1_vbs, size_t p1_vb_count,
        const float* p1_cns, size_t p1_cn_count,
        uint8_t p1_ft, uint8_t p1_conv,
        const SkPoint* p2_pts, size_t p2_pt_count,
        const uint8_t* p2_vbs, size_t p2_vb_count,
        const float* p2_cns, size_t p2_cn_count,
        uint8_t p2_ft, uint8_t p2_conv,
        uint8_t path_op);

SkiaRsPathOpsBuf* skia_rs_pathops_simplify(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* cns, size_t cn_count,
        uint8_t ft, uint8_t conv);

SkiaRsPathOpsBuf* skia_rs_pathops_as_winding(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* cns, size_t cn_count,
        uint8_t ft, uint8_t conv);

bool skia_rs_pathops_tight_bounds(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* cns, size_t cn_count,
        uint8_t ft, SkRect* out_rect);

bool skia_rs_pathops_fix_winding(
        const SkPoint* pts, size_t pt_count,
        const uint8_t* vbs, size_t vb_count,
        const float* cns, size_t cn_count,
        uint8_t ft, uint8_t conv,
        SkiaRsPathOpsBuf** out_buf);

SkiaRsPathOpsBuf* skia_rs_pathops_builder_resolve(const FfiPathInput* inputs, size_t count);

bool skia_rs_pathops_almost_bequal_ulps(float a, float b);
bool skia_rs_pathops_almost_pequal_ulps(float a, float b);
bool skia_rs_pathops_almost_dequal_ulps_f32(float a, float b);
bool skia_rs_pathops_almost_dequal_ulps_f64(double a, double b);
bool skia_rs_pathops_almost_equal_ulps(float a, float b);
bool skia_rs_pathops_almost_equal_ulps_no_normal_check(float a, float b);
bool skia_rs_pathops_almost_equal_ulps_pin(float a, float b);
bool skia_rs_pathops_not_almost_equal_ulps(float a, float b);
bool skia_rs_pathops_not_almost_equal_ulps_pin(float a, float b);
bool skia_rs_pathops_not_almost_dequal_ulps(float a, float b);
bool skia_rs_pathops_roughly_equal_ulps(float a, float b);
bool skia_rs_pathops_almost_between_ulps(float a, float b, float c);
bool skia_rs_pathops_almost_less_ulps(float a, float b);
bool skia_rs_pathops_almost_less_or_equal_ulps(float a, float b);
int skia_rs_pathops_ulps_distance(float a, float b);

SkDPoint skia_rs_pathops_dline_pt_at_t(const SkDPoint* pts, double t);
double skia_rs_pathops_dline_exact_point(const SkDPoint* pts, SkDPoint xy);
double skia_rs_pathops_dline_near_point(const SkDPoint* pts, SkDPoint xy, bool* unequal);
bool skia_rs_pathops_dline_near_ray(const SkDPoint* pts, SkDPoint xy);

int skia_rs_pathops_dquad_add_valid_ts(const double* s, int real_roots, double* t);
int skia_rs_pathops_dquad_roots_real(double a, double b, double c, double* s);
int skia_rs_pathops_dquad_roots_valid_t(double a, double b, double c, double* t);
SkDPoint skia_rs_pathops_dquad_pt_at_t(const SkDPoint* pts, double t);
SkDVector skia_rs_pathops_dquad_dxdy_at_t(const SkDPoint* pts, double t);
void skia_rs_pathops_dquad_sub_divide(const SkDPoint* pts, double t1, double t2, SkDPoint* out_pts);
int skia_rs_pathops_dquad_find_extrema(double a, double b, double c, double* t_out);

SkDPoint skia_rs_pathops_dconic_pt_at_t(const SkDPoint* pts, float weight, double t);
SkDVector skia_rs_pathops_dconic_dxdy_at_t(const SkDPoint* pts, float weight, double t);
void skia_rs_pathops_dconic_sub_divide(const SkDPoint* pts, float weight, double t1, double t2,
                                       SkDPoint* out_pts, float* out_weight);

void skia_rs_pathops_dcubic_coefficients(double s0, double s1, double s2, double s3,
                                         double* a, double* b, double* c, double* d);
int skia_rs_pathops_dcubic_find_extrema(double s0, double s1, double s2, double s3, double* t_values);
int skia_rs_pathops_dcubic_roots_valid_t(double a, double b, double c, double d, double* t);
int skia_rs_pathops_dcubic_roots_real(double a, double b, double c, double d, double* s);
int skia_rs_pathops_dcubic_complex_break(const SkPoint* points, float* t);
int skia_rs_pathops_dcubic_convex_hull(const SkDPoint* pts, int8_t* order);
int skia_rs_pathops_dcubic_search_roots(const SkDPoint* pts, double* extremas, int extrema_count,
                                        double axis_intercept, int axis, double* valid_roots);
double skia_rs_pathops_dcubic_calc_precision(const SkDPoint* pts);
int skia_rs_pathops_dcubic_find_inflections(const SkDPoint* pts, double* t_values);
int skia_rs_pathops_dcubic_find_max_curvature(const SkDPoint* pts, double* t_values);
bool skia_rs_pathops_dcubic_ends_are_extrema(const SkDPoint* pts);
SkDPoint skia_rs_pathops_dcubic_pt_at_t(const SkDPoint* pts, double t);
SkDVector skia_rs_pathops_dcubic_dxdy_at_t(const SkDPoint* pts, double t);
void skia_rs_pathops_dcubic_chop_at(const SkDPoint* pts, double t, SkDPoint* out_pts);
void skia_rs_pathops_dcubic_to_quad(const SkDPoint* pts, SkDPoint* out_pts);
void skia_rs_pathops_dcubic_sub_divide(const SkDPoint* pts, double t1, double t2, SkDPoint* out_pts);

void skia_rs_pathops_drect_set_quad_bounds(const SkDPoint* curve, const SkDPoint* sub,
                                           double t1, double t2, SkDRect* out_rect);
void skia_rs_pathops_drect_set_conic_bounds(const SkDPoint* curve, float cw,
                                            const SkDPoint* sub, float sw,
                                            double t1, double t2, SkDRect* out_rect);
void skia_rs_pathops_drect_set_cubic_bounds(const SkDPoint* curve, const SkDPoint* sub,
                                            double t1, double t2, SkDRect* out_rect);

void skia_rs_pathops_dcurve_set_quad_bounds(const SkDPoint* dcurve_pts, const SkPoint* curve,
                                            double t_start, double t_end,
                                            SkPathOpsBounds* out_bounds);
void skia_rs_pathops_dcurve_set_conic_bounds(const SkDPoint* dcurve_pts, float dcurve_w,
                                             const SkPoint* curve, float w,
                                             double t_start, double t_end,
                                             SkPathOpsBounds* out_bounds);
void skia_rs_pathops_dcurve_set_cubic_bounds(const SkDPoint* dcurve_pts, const SkPoint* curve,
                                             double t_start, double t_end,
                                             SkPathOpsBounds* out_bounds);

int skia_rs_pathops_reduce_line(const SkDPoint* pts, SkDPoint* out_pts);
int skia_rs_pathops_reduce_quad(const SkDPoint* pts, SkDPoint* out_pts);
int skia_rs_pathops_reduce_cubic(const SkDPoint* pts, int allow_quadratics, SkDPoint* out_pts);
uint8_t skia_rs_pathops_reduce_conic_pts(const SkPoint* pts, float w, SkPoint* out_pts);
uint8_t skia_rs_pathops_reduce_quad_pts(const SkPoint* pts, SkPoint* out_pts);
uint8_t skia_rs_pathops_reduce_cubic_pts(const SkPoint* pts, SkPoint* out_pts);

int skia_rs_pathops_i_horizontal_line(FfiIntersections* io, const SkDPoint* pts,
                                      double left, double right, double y, bool flipped);
int skia_rs_pathops_i_vertical_line(FfiIntersections* io, const SkDPoint* pts,
                                    double top, double bottom, double x, bool flipped);
int skia_rs_pathops_i_horizontal_quad(FfiIntersections* io, const SkDPoint* pts,
                                      double left, double right, double y, bool flipped);
int skia_rs_pathops_i_vertical_quad(FfiIntersections* io, const SkDPoint* pts,
                                    double top, double bottom, double x, bool flipped);
int skia_rs_pathops_i_horizontal_conic(FfiIntersections* io, const SkDPoint* pts, float w,
                                       double left, double right, double y, bool flipped);
int skia_rs_pathops_i_vertical_conic(FfiIntersections* io, const SkDPoint* pts, float w,
                                     double top, double bottom, double x, bool flipped);
int skia_rs_pathops_i_horizontal_cubic(FfiIntersections* io, const SkDPoint* pts,
                                       double left, double right, double y, bool flipped);
int skia_rs_pathops_i_vertical_cubic(FfiIntersections* io, const SkDPoint* pts,
                                     double top, double bottom, double x, bool flipped);
int skia_rs_pathops_i_line_line(FfiIntersections* io, const SkDPoint* a, const SkDPoint* b);
int skia_rs_pathops_i_quad_line(FfiIntersections* io, const SkDPoint* q, const SkDPoint* l);
int skia_rs_pathops_i_conic_line(FfiIntersections* io, const SkDPoint* c, float w, const SkDPoint* l);
int skia_rs_pathops_i_cubic_line(FfiIntersections* io, const SkDPoint* c, const SkDPoint* l);
int skia_rs_pathops_i_quad_quad(FfiIntersections* io, const SkDPoint* q1, const SkDPoint* q2);
int skia_rs_pathops_i_conic_quad(FfiIntersections* io, const SkDPoint* c, float w, const SkDPoint* q);
int skia_rs_pathops_i_conic_conic(FfiIntersections* io, const SkDPoint* c1, float w1,
                                  const SkDPoint* c2, float w2);
int skia_rs_pathops_i_cubic_quad(FfiIntersections* io, const SkDPoint* c, const SkDPoint* q);
int skia_rs_pathops_i_cubic_conic(FfiIntersections* io, const SkDPoint* cu,
                                  const SkDPoint* co, float w);
int skia_rs_pathops_i_cubic_cubic(FfiIntersections* io, const SkDPoint* c1, const SkDPoint* c2);
int skia_rs_pathops_i_ray_line(FfiIntersections* io, const SkDPoint* a, const SkDPoint* b);
int skia_rs_pathops_i_ray_quad(FfiIntersections* io, const SkDPoint* q, const SkDPoint* l);
int skia_rs_pathops_i_ray_conic(FfiIntersections* io, const SkDPoint* c, float w, const SkDPoint* l);
int skia_rs_pathops_i_ray_cubic(FfiIntersections* io, const SkDPoint* c, const SkDPoint* l);
int skia_rs_pathops_i_insert(FfiIntersections* io, double one, double two, SkDPoint pt);
int skia_rs_pathops_i_insert_coincident(FfiIntersections* io, double one, double two, SkDPoint pt);
int skia_rs_pathops_i_debug_coincident_used(const FfiIntersections* io);
}  // extern "C"

namespace {

SkPath path_from_rust_buf(SkiaRsPathOpsBuf* buf) {
    size_t npts = 0, nvbs = 0, ncns = 0;
    const SkPoint* pts = skia_rs_pathops_buf_points(buf, &npts);
    const uint8_t* vbs = skia_rs_pathops_buf_verbs(buf, &nvbs);
    const float* cns = skia_rs_pathops_buf_conics(buf, &ncns);
    auto ft = static_cast<SkPathFillType>(skia_rs_pathops_buf_fill_type(buf));
    SkPathBuilder pb(ft);
    if (nvbs > 0) {
        SkPathRaw raw{
            {pts, npts},
            {reinterpret_cast<const SkPathVerb*>(vbs), nvbs},
            {cns, ncns},
            SkRect::MakeEmpty(),
            ft,
            SkPathConvexity::kUnknown,
            0,
        };
        pb.addRaw(raw, SkPathBuilder::Reserve::kExact);
    }
    skia_rs_pathops_buf_free(buf);
    return pb.detach();
}

struct PathRawData {
    SkSpan<const SkPoint> pts;
    SkSpan<const SkPathVerb> vbs;
    SkSpan<const float> cns;
    uint8_t fill_type;
    uint8_t convexity;
};

PathRawData extract_path_data(const SkPath& path) {
    return PathRawData{
        path.points(),
        path.verbs(),
        path.conicWeights(),
        static_cast<uint8_t>(path.getFillType()),
        static_cast<uint8_t>(SkPathPriv::GetConvexityOrUnknown(path)),
    };
}

}  // namespace

// ============================================================================
// Public PathOps API: Op, OpDebug, Simplify, SimplifyDebug, AsWinding,
// ComputeTightBounds, SkOpBuilder
// ============================================================================

std::optional<SkPath> OpDebug(const SkPath& one, const SkPath& two, SkPathOp op
        SkDEBUGPARAMS(bool /*skipAssert*/) SkDEBUGPARAMS(const char* /*testName*/)) {
    if (!one.isFinite() || !two.isFinite()) {
        return {};
    }
    auto d1 = extract_path_data(one);
    auto d2 = extract_path_data(two);
    SkiaRsPathOpsBuf* buf = skia_rs_pathops_op(
            d1.pts.data(), d1.pts.size(),
            reinterpret_cast<const uint8_t*>(d1.vbs.data()), d1.vbs.size(),
            d1.cns.data(), d1.cns.size(),
            d1.fill_type, d1.convexity,
            d2.pts.data(), d2.pts.size(),
            reinterpret_cast<const uint8_t*>(d2.vbs.data()), d2.vbs.size(),
            d2.cns.data(), d2.cns.size(),
            d2.fill_type, d2.convexity,
            static_cast<uint8_t>(op));
    if (!buf) {
        return {};
    }
    return path_from_rust_buf(buf);
}

std::optional<SkPath> Op(const SkPath& one, const SkPath& two, SkPathOp op) {
    return OpDebug(one, two, op SkDEBUGPARAMS(true) SkDEBUGPARAMS(nullptr));
}

std::optional<SkPath> SimplifyDebug(const SkPath& path
        SkDEBUGPARAMS(bool /*skipAssert*/) SkDEBUGPARAMS(const char* /*testName*/)) {
    if (!path.isFinite()) {
        return {};
    }
    auto d = extract_path_data(path);
    SkiaRsPathOpsBuf* buf = skia_rs_pathops_simplify(
            d.pts.data(), d.pts.size(),
            reinterpret_cast<const uint8_t*>(d.vbs.data()), d.vbs.size(),
            d.cns.data(), d.cns.size(),
            d.fill_type, d.convexity);
    if (!buf) {
        return {};
    }
    return path_from_rust_buf(buf);
}

std::optional<SkPath> Simplify(const SkPath& path) {
    return SimplifyDebug(path SkDEBUGPARAMS(true) SkDEBUGPARAMS(nullptr));
}

std::optional<SkPath> AsWinding(const SkPath& path) {
    if (!path.isFinite()) {
        return {};
    }
    auto d = extract_path_data(path);
    SkiaRsPathOpsBuf* buf = skia_rs_pathops_as_winding(
            d.pts.data(), d.pts.size(),
            reinterpret_cast<const uint8_t*>(d.vbs.data()), d.vbs.size(),
            d.cns.data(), d.cns.size(),
            d.fill_type, d.convexity);
    if (!buf) {
        return {};
    }
    return path_from_rust_buf(buf);
}

bool ComputeTightBounds(const SkPath& path, SkRect* result) {
    auto d = extract_path_data(path);
    return skia_rs_pathops_tight_bounds(
            d.pts.data(), d.pts.size(),
            reinterpret_cast<const uint8_t*>(d.vbs.data()), d.vbs.size(),
            d.cns.data(), d.cns.size(),
            d.fill_type, result);
}

void SkOpBuilder::ReversePath(SkPath* path) {
    auto lastPt = path->getLastPt();
    if (!lastPt.has_value()) {
        return;
    }
    SkPathBuilder temp;
    temp.moveTo(*lastPt);
    SkPathPriv::ReversePathTo(&temp, *path);
    temp.close();
    *path = temp.detach();
}

bool SkOpBuilder::FixWinding(SkPath* path) {
    auto d = extract_path_data(*path);
    SkiaRsPathOpsBuf* buf = nullptr;
    if (!skia_rs_pathops_fix_winding(
            d.pts.data(), d.pts.size(),
            reinterpret_cast<const uint8_t*>(d.vbs.data()), d.vbs.size(),
            d.cns.data(), d.cns.size(),
            d.fill_type, d.convexity, &buf)) {
        return false;
    }
    if (buf) {
        *path = path_from_rust_buf(buf);
    }
    return true;
}

void SkOpBuilder::add(const SkPath& path, SkPathOp op) {
    if (fOps.empty() && op != kUnion_SkPathOp) {
        fPathRefs.push_back() = SkPath();
        *fOps.append() = kUnion_SkPathOp;
    }
    fPathRefs.push_back() = path;
    *fOps.append() = op;
}

void SkOpBuilder::reset() {
    fPathRefs.clear();
    fOps.reset();
}

std::optional<SkPath> SkOpBuilder::resolve() {
    const int count = fOps.size();
    std::vector<FfiPathInput> inputs;
    inputs.reserve(static_cast<size_t>(count));
    for (int i = 0; i < count; ++i) {
        auto d = extract_path_data(fPathRefs[i]);
        inputs.push_back(FfiPathInput{
            d.pts.data(),
            d.pts.size(),
            reinterpret_cast<const uint8_t*>(d.vbs.data()),
            d.vbs.size(),
            d.cns.data(),
            d.cns.size(),
            d.fill_type,
            d.convexity,
            static_cast<uint8_t>(fOps[i]),
        });
    }
    SkiaRsPathOpsBuf* buf = skia_rs_pathops_builder_resolve(inputs.data(), inputs.size());
    reset();
    if (!buf) {
        return {};
    }
    return path_from_rust_buf(buf);
}

// ============================================================================
// SkPathOpsTypes.cpp symbols
// ============================================================================

bool AlmostBequalUlps(float a, float b) { return skia_rs_pathops_almost_bequal_ulps(a, b); }
bool AlmostPequalUlps(float a, float b) { return skia_rs_pathops_almost_pequal_ulps(a, b); }
bool AlmostDequalUlps(float a, float b) { return skia_rs_pathops_almost_dequal_ulps_f32(a, b); }
bool AlmostDequalUlps(double a, double b) { return skia_rs_pathops_almost_dequal_ulps_f64(a, b); }
bool AlmostEqualUlps(float a, float b) { return skia_rs_pathops_almost_equal_ulps(a, b); }
bool AlmostEqualUlpsNoNormalCheck(float a, float b) {
    return skia_rs_pathops_almost_equal_ulps_no_normal_check(a, b);
}
bool AlmostEqualUlps_Pin(float a, float b) { return skia_rs_pathops_almost_equal_ulps_pin(a, b); }
bool NotAlmostEqualUlps(float a, float b) { return skia_rs_pathops_not_almost_equal_ulps(a, b); }
bool NotAlmostEqualUlps_Pin(float a, float b) {
    return skia_rs_pathops_not_almost_equal_ulps_pin(a, b);
}
bool NotAlmostDequalUlps(float a, float b) { return skia_rs_pathops_not_almost_dequal_ulps(a, b); }
bool RoughlyEqualUlps(float a, float b) { return skia_rs_pathops_roughly_equal_ulps(a, b); }
bool AlmostBetweenUlps(float a, float b, float c) {
    return skia_rs_pathops_almost_between_ulps(a, b, c);
}
bool AlmostLessUlps(float a, float b) { return skia_rs_pathops_almost_less_ulps(a, b); }
bool AlmostLessOrEqualUlps(float a, float b) {
    return skia_rs_pathops_almost_less_or_equal_ulps(a, b);
}
int UlpsDistance(float a, float b) { return skia_rs_pathops_ulps_distance(a, b); }

SkOpGlobalState::SkOpGlobalState(SkOpContourHead* head,
                                 SkArenaAlloc* allocator
                                 SkDEBUGPARAMS(bool debugSkipAssert)
                                 SkDEBUGPARAMS(const char* testName))
    : fAllocator(allocator)
    , fCoincidence(nullptr)
    , fContourHead(head)
    , fNested(0)
    , fWindingFailed(false)
    , fPhase(SkOpPhase::kIntersecting)
    SkDEBUGPARAMS(fDebugTestName(testName))
    SkDEBUGPARAMS(fAngleID(0))
    SkDEBUGPARAMS(fCoinID(0))
    SkDEBUGPARAMS(fContourID(0))
    SkDEBUGPARAMS(fPtTID(0))
    SkDEBUGPARAMS(fSegmentID(0))
    SkDEBUGPARAMS(fSpanID(0))
    SkDEBUGPARAMS(fDebugSkipAssert(debugSkipAssert)) {}

// ============================================================================
// SkDLine / SkDQuad / SkDConic / SkDCubic / SkDRect / SkDCurve / SkReduceOrder
// ============================================================================

SkDPoint SkDLine::ptAtT(double t) const { return skia_rs_pathops_dline_pt_at_t(fPts, t); }
double SkDLine::exactPoint(const SkDPoint& xy) const {
    return skia_rs_pathops_dline_exact_point(fPts, xy);
}
double SkDLine::nearPoint(const SkDPoint& xy, bool* unequal) const {
    return skia_rs_pathops_dline_near_point(fPts, xy, unequal);
}
bool SkDLine::nearRay(const SkDPoint& xy) const {
    return skia_rs_pathops_dline_near_ray(fPts, xy);
}

int SkDQuad::AddValidTs(double s[], int realRoots, double* t) {
    return skia_rs_pathops_dquad_add_valid_ts(s, realRoots, t);
}
int SkDQuad::RootsReal(double A, double B, double C, double t[2]) {
    return skia_rs_pathops_dquad_roots_real(A, B, C, t);
}
int SkDQuad::RootsValidT(const double A, const double B, const double C, double s[2]) {
    return skia_rs_pathops_dquad_roots_valid_t(A, B, C, s);
}
int SkDQuad::FindExtrema(const double src[], double tValue[1]) {
    return skia_rs_pathops_dquad_find_extrema(src[0], src[2], src[4], tValue);
}
SkDPoint SkDQuad::ptAtT(double t) const { return skia_rs_pathops_dquad_pt_at_t(fPts, t); }
SkDVector SkDQuad::dxdyAtT(double t) const { return skia_rs_pathops_dquad_dxdy_at_t(fPts, t); }
SkDQuad SkDQuad::subDivide(double t1, double t2) const {
    SkDQuad res;
    skia_rs_pathops_dquad_sub_divide(fPts, t1, t2, res.fPts);
    return res;
}

SkDPoint SkDConic::ptAtT(double t) const {
    return skia_rs_pathops_dconic_pt_at_t(fPts.fPts, fWeight, t);
}
SkDVector SkDConic::dxdyAtT(double t) const {
    return skia_rs_pathops_dconic_dxdy_at_t(fPts.fPts, fWeight, t);
}
SkDConic SkDConic::subDivide(double t1, double t2) const {
    SkDConic res;
    skia_rs_pathops_dconic_sub_divide(fPts.fPts, fWeight, t1, t2, res.fPts.fPts, &res.fWeight);
    return res;
}

const int SkDCubic::gPrecisionUnit = 1 << 20;

void SkDCubic::Coefficients(const double* src, double* A, double* B, double* C, double* D) {
    skia_rs_pathops_dcubic_coefficients(src[0], src[2], src[4], src[6], A, B, C, D);
}
int SkDCubic::FindExtrema(const double src[], double tValue[2]) {
    return skia_rs_pathops_dcubic_find_extrema(src[0], src[2], src[4], src[6], tValue);
}
int SkDCubic::RootsValidT(const double A, const double B, const double C, double D, double s[3]) {
    return skia_rs_pathops_dcubic_roots_valid_t(A, B, C, D, s);
}
int SkDCubic::RootsReal(double A, double B, double C, double D, double t[3]) {
    return skia_rs_pathops_dcubic_roots_real(A, B, C, D, t);
}
int SkDCubic::ComplexBreak(const SkPoint points[4], SkScalar* t) {
    return skia_rs_pathops_dcubic_complex_break(points, t);
}
int SkDCubic::convexHull(char order[4]) const {
    int8_t tmp[4] = {};
    int n = skia_rs_pathops_dcubic_convex_hull(fPts, tmp);
    for (int i = 0; i < 4; ++i) {
        order[i] = static_cast<char>(tmp[i]);
    }
    return n;
}
int SkDCubic::searchRoots(double extremes[6], int extrema, double axisIntercept,
                          SearchAxis xAxis, double* validRoots) const {
    return skia_rs_pathops_dcubic_search_roots(fPts, extremes, extrema, axisIntercept,
                                               static_cast<int>(xAxis), validRoots);
}
double SkDCubic::calcPrecision() const { return skia_rs_pathops_dcubic_calc_precision(fPts); }
int SkDCubic::findInflections(double tValues[2]) const {
    return skia_rs_pathops_dcubic_find_inflections(fPts, tValues);
}
int SkDCubic::findMaxCurvature(double tValues[]) const {
    return skia_rs_pathops_dcubic_find_max_curvature(fPts, tValues);
}
bool SkDCubic::endsAreExtremaInXOrY() const {
    return skia_rs_pathops_dcubic_ends_are_extrema(fPts);
}
SkDPoint SkDCubic::ptAtT(double t) const { return skia_rs_pathops_dcubic_pt_at_t(fPts, t); }
SkDVector SkDCubic::dxdyAtT(double t) const { return skia_rs_pathops_dcubic_dxdy_at_t(fPts, t); }
SkDCubicPair SkDCubic::chopAt(double t) const {
    SkDCubicPair pair;
    skia_rs_pathops_dcubic_chop_at(fPts, t, pair.pts.data());
    return pair;
}
SkDQuad SkDCubic::toQuad() const {
    SkDQuad q;
    skia_rs_pathops_dcubic_to_quad(fPts, q.fPts);
    return q;
}
SkDCubic SkDCubic::subDivide(double t1, double t2) const {
    SkDCubic res;
    skia_rs_pathops_dcubic_sub_divide(fPts, t1, t2, res.fPts);
    return res;
}

void SkDRect::setBounds(const SkDQuad& curve, const SkDQuad& sub, double tStart, double tEnd) {
    skia_rs_pathops_drect_set_quad_bounds(curve.fPts, sub.fPts, tStart, tEnd, this);
}
void SkDRect::setBounds(const SkDConic& curve, const SkDConic& sub, double tStart, double tEnd) {
    skia_rs_pathops_drect_set_conic_bounds(curve.fPts.fPts, curve.fWeight,
                                           sub.fPts.fPts, sub.fWeight, tStart, tEnd, this);
}
void SkDRect::setBounds(const SkDCubic& curve, const SkDCubic& sub, double tStart, double tEnd) {
    skia_rs_pathops_drect_set_cubic_bounds(curve.fPts, sub.fPts, tStart, tEnd, this);
}

void SkDCurve::setQuadBounds(const SkPoint curve[3], SkScalar /*curveWeight*/,
                             double tStart, double tEnd, SkPathOpsBounds* bounds) {
    skia_rs_pathops_dcurve_set_quad_bounds(fQuad.fPts, curve, tStart, tEnd, bounds);
}
void SkDCurve::setConicBounds(const SkPoint curve[3], SkScalar curveWeight,
                              double tStart, double tEnd, SkPathOpsBounds* bounds) {
    skia_rs_pathops_dcurve_set_conic_bounds(fConic.fPts.fPts, fConic.fWeight, curve, curveWeight,
                                            tStart, tEnd, bounds);
}
void SkDCurve::setCubicBounds(const SkPoint curve[4], SkScalar /*curveWeight*/,
                              double tStart, double tEnd, SkPathOpsBounds* bounds) {
    skia_rs_pathops_dcurve_set_cubic_bounds(fCubic.fPts, curve, tStart, tEnd, bounds);
}

int SkReduceOrder::reduce(const SkDLine& line) {
    return skia_rs_pathops_reduce_line(line.fPts, fCubic.fPts);
}
int SkReduceOrder::reduce(const SkDQuad& quad) {
    return skia_rs_pathops_reduce_quad(quad.fPts, fCubic.fPts);
}
int SkReduceOrder::reduce(const SkDCubic& cubic, Quadratics allowQuadratics) {
    return skia_rs_pathops_reduce_cubic(cubic.fPts, static_cast<int>(allowQuadratics), fCubic.fPts);
}
SkPath::Verb SkReduceOrder::Conic(const SkConic& conic, SkPoint* reducePts) {
    return static_cast<SkPath::Verb>(
            skia_rs_pathops_reduce_conic_pts(conic.fPts, conic.fW, reducePts));
}
SkPath::Verb SkReduceOrder::Quad(const SkPoint pts[3], SkPoint* reducePts) {
    return static_cast<SkPath::Verb>(skia_rs_pathops_reduce_quad_pts(pts, reducePts));
}
SkPath::Verb SkReduceOrder::Cubic(const SkPoint pts[4], SkPoint* reducePts) {
    return static_cast<SkPath::Verb>(skia_rs_pathops_reduce_cubic_pts(pts, reducePts));
}

// ============================================================================
// SkIntersections methods
// ============================================================================

namespace {

// Note: SkIntersections has private fields matching FfiIntersections up to fSwap.
// We read/write via member helper macros inside SkIntersections methods.
FfiIntersections to_ffi(const SkDPoint pt[13], const SkDPoint pt2[2], const double t[2][13],
                        const uint16_t isCoin[2], const bool nearlySame[2],
                        unsigned char used, unsigned char max, bool allowNear, bool swap) {
    FfiIntersections f;
    memcpy(f.pt, pt, sizeof(f.pt));
    memcpy(f.pt2, pt2, sizeof(f.pt2));
    memcpy(f.t, t, sizeof(f.t));
    f.is_coincident[0] = isCoin[0];
    f.is_coincident[1] = isCoin[1];
    f.nearly_same[0] = nearlySame[0];
    f.nearly_same[1] = nearlySame[1];
    f.used = used;
    f.max = max;
    f.allow_near = allowNear;
    f.swap = swap;
    return f;
}

void from_ffi(const FfiIntersections& f, SkDPoint pt[13], SkDPoint pt2[2], double t[2][13],
              uint16_t isCoin[2], bool nearlySame[2],
              unsigned char& used, unsigned char& max, bool& allowNear, bool& swap) {
    memcpy(pt, f.pt, sizeof(f.pt));
    memcpy(pt2, f.pt2, sizeof(f.pt2));
    memcpy(t, f.t, sizeof(f.t));
    isCoin[0] = f.is_coincident[0];
    isCoin[1] = f.is_coincident[1];
    nearlySame[0] = f.nearly_same[0];
    nearlySame[1] = f.nearly_same[1];
    used = f.used;
    max = f.max;
    allowNear = f.allow_near;
    swap = f.swap;
}

}  // namespace

#define I_TO_FFI() to_ffi(fPt, fPt2, fT, fIsCoincident, fNearlySame, fUsed, fMax, fAllowNear, fSwap)
#define I_FROM_FFI(f) from_ffi(f, fPt, fPt2, fT, fIsCoincident, fNearlySame, fUsed, fMax, fAllowNear, fSwap)

int SkIntersections::horizontal(const SkDLine& line, double left, double right, double y,
                                bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_horizontal_line(&f, line.fPts, left, right, y, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::vertical(const SkDLine& line, double top, double bottom, double x,
                              bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_vertical_line(&f, line.fPts, top, bottom, x, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::horizontal(const SkDQuad& quad, double left, double right, double y,
                                bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_horizontal_quad(&f, quad.fPts, left, right, y, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::vertical(const SkDQuad& quad, double top, double bottom, double x,
                              bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_vertical_quad(&f, quad.fPts, top, bottom, x, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::horizontal(const SkDConic& conic, double left, double right, double y,
                                bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_horizontal_conic(&f, conic.fPts.fPts, conic.fWeight,
                                               left, right, y, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::vertical(const SkDConic& conic, double top, double bottom, double x,
                              bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_vertical_conic(&f, conic.fPts.fPts, conic.fWeight,
                                             top, bottom, x, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::horizontal(const SkDCubic& cubic, double left, double right, double y,
                                bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_horizontal_cubic(&f, cubic.fPts, left, right, y, flipped);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::vertical(const SkDCubic& cubic, double top, double bottom, double x,
                              bool flipped) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_vertical_cubic(&f, cubic.fPts, top, bottom, x, flipped);
    I_FROM_FFI(f);
    return n;
}

int SkIntersections::intersect(const SkDLine& a, const SkDLine& b) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_line_line(&f, a.fPts, b.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDQuad& q, const SkDLine& l) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_quad_line(&f, q.fPts, l.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDConic& c, const SkDLine& l) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_conic_line(&f, c.fPts.fPts, c.fWeight, l.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDCubic& c, const SkDLine& l) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_cubic_line(&f, c.fPts, l.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDQuad& q1, const SkDQuad& q2) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_quad_quad(&f, q1.fPts, q2.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDConic& c, const SkDQuad& q) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_conic_quad(&f, c.fPts.fPts, c.fWeight, q.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDConic& c1, const SkDConic& c2) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_conic_conic(&f, c1.fPts.fPts, c1.fWeight,
                                          c2.fPts.fPts, c2.fWeight);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDCubic& c, const SkDQuad& q) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_cubic_quad(&f, c.fPts, q.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDCubic& cu, const SkDConic& co) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_cubic_conic(&f, cu.fPts, co.fPts.fPts, co.fWeight);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersect(const SkDCubic& c1, const SkDCubic& c2) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_cubic_cubic(&f, c1.fPts, c2.fPts);
    I_FROM_FFI(f);
    return n;
}

int SkIntersections::intersectRay(const SkDLine& a, const SkDLine& b) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_ray_line(&f, a.fPts, b.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersectRay(const SkDQuad& q, const SkDLine& l) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_ray_quad(&f, q.fPts, l.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersectRay(const SkDConic& c, const SkDLine& l) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_ray_conic(&f, c.fPts.fPts, c.fWeight, l.fPts);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::intersectRay(const SkDCubic& c, const SkDLine& l) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_ray_cubic(&f, c.fPts, l.fPts);
    I_FROM_FFI(f);
    return n;
}

int SkIntersections::insert(double one, double two, const SkDPoint& pt) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_insert(&f, one, two, pt);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::insertCoincident(double one, double two, const SkDPoint& pt) {
    FfiIntersections f = I_TO_FFI();
    int n = skia_rs_pathops_i_insert_coincident(&f, one, two, pt);
    I_FROM_FFI(f);
    return n;
}
int SkIntersections::debugCoincidentUsed() const {
    FfiIntersections f = I_TO_FFI();
    return skia_rs_pathops_i_debug_coincident_used(&f);
}

#undef I_TO_FFI
#undef I_FROM_FFI
