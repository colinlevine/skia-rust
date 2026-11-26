/*
 * Safe Rust bridge for Layer 1.2 skia-rs-geometry (Batch 3):
 *   SkGeometry, SkLineClipper, SkQuadClipper, SkCubicClipper, SkEdgeClipper.
 */

#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTypes.h"
#include "src/core/SkCubicClipper.h"
#include "src/core/SkEdgeClipper.h"
#include "src/core/SkGeometry.h"
#include "src/core/SkLineClipper.h"
#include "src/core/SkPathPriv.h"
#include "src/core/SkQuadClipper.h"

#include <cstddef>
#include <cstdint>
#include <cstring>

static_assert(sizeof(SkConic) == 28);

extern "C" {
// Quads
int32_t skia_rs_find_unit_quad_roots(float a, float b, float c, float roots[2]);
SkPoint skia_rs_eval_quad_at(const SkPoint src[3], float t);
SkPoint skia_rs_eval_quad_tangent_at(const SkPoint src[3], float t);
void skia_rs_chop_quad_at(const SkPoint src[3], SkPoint dst[5], float t);
float skia_rs_measure_angle_between_vectors(SkPoint a, SkPoint b);
SkPoint skia_rs_find_bisector(SkPoint a, SkPoint b);
float skia_rs_find_quad_mid_tangent(const SkPoint src[3]);
int32_t skia_rs_find_quad_extrema(float a, float b, float c, float* t_value);
int32_t skia_rs_chop_quad_at_y_extrema(const SkPoint src[3], SkPoint dst[5]);
int32_t skia_rs_chop_quad_at_x_extrema(const SkPoint src[3], SkPoint dst[5]);
float skia_rs_find_quad_max_curvature(const SkPoint src[3]);
int32_t skia_rs_chop_quad_at_max_curvature(const SkPoint src[3], SkPoint dst[5]);
void skia_rs_convert_quad_to_cubic(const SkPoint src[3], SkPoint dst[4]);

// Cubics
void skia_rs_eval_cubic_at(
        const SkPoint src[4], float t, SkPoint* loc, SkPoint* tangent, SkPoint* curvature);
int32_t skia_rs_find_cubic_extrema(float a, float b, float c, float d, float t_values[2]);
void skia_rs_chop_cubic_at_1(const SkPoint src[4], SkPoint dst[7], float t);
void skia_rs_chop_cubic_at_2(const SkPoint src[4], SkPoint dst[10], float t0, float t1);
void skia_rs_chop_cubic_at_many(
        const SkPoint src[4], SkPoint* dst, const float* t_values, int32_t t_count);
float skia_rs_measure_non_inflect_cubic_rotation(const SkPoint pts[4]);
float skia_rs_find_cubic_mid_tangent(const SkPoint src[4]);
int32_t skia_rs_chop_cubic_at_y_extrema(const SkPoint src[4], SkPoint dst[10]);
int32_t skia_rs_chop_cubic_at_x_extrema(const SkPoint src[4], SkPoint dst[10]);
int32_t skia_rs_find_cubic_inflections(const SkPoint src[4], float t_values[2]);
int32_t skia_rs_chop_cubic_at_inflections(const SkPoint src[4], SkPoint dst[10]);
int32_t skia_rs_classify_cubic(const SkPoint p[4], double t[2], double s[2], double d[4]);
int32_t skia_rs_find_cubic_max_curvature(const SkPoint src[4], float t_values[3]);
int32_t skia_rs_chop_cubic_at_max_curvature(const SkPoint src[4], SkPoint dst[13], float t_values[3]);
float skia_rs_find_cubic_cusp(const SkPoint src[4]);
bool skia_rs_chop_mono_cubic_at_y(const SkPoint src[4], float y, SkPoint dst[7]);
bool skia_rs_chop_mono_cubic_at_x(const SkPoint src[4], float x, SkPoint dst[7]);

// Conics
bool skia_rs_conic_chop_at(const SkConic* conic, float t, SkConic dst[2]);
void skia_rs_conic_chop_at_range(const SkConic* conic, float t1, float t2, SkConic* dst);
SkPoint skia_rs_conic_eval_at(const SkConic* conic, float t);
SkPoint skia_rs_conic_eval_tangent_at(const SkConic* conic, float t);
void skia_rs_conic_chop(const SkConic* conic, SkConic dst[2]);
SkPoint skia_rs_conic_compute_as_quad_error(const SkConic* conic);
bool skia_rs_conic_as_quad_tol(const SkConic* conic, float tol);
int32_t skia_rs_conic_compute_quad_pow2(const SkConic* conic, float tol);
int32_t skia_rs_conic_chop_into_quads_pow2(const SkConic* conic, SkPoint* pts, int32_t pow2);
float skia_rs_conic_find_mid_tangent(const SkConic* conic);
bool skia_rs_conic_find_x_extrema(const SkConic* conic, float* t_out);
bool skia_rs_conic_find_y_extrema(const SkConic* conic, float* t_out);
bool skia_rs_conic_chop_at_x_extrema(const SkConic* conic, SkConic dst[2]);
bool skia_rs_conic_chop_at_y_extrema(const SkConic* conic, SkConic dst[2]);
SkRect skia_rs_conic_compute_tight_bounds(const SkConic* conic);
SkRect skia_rs_conic_compute_fast_bounds(const SkConic* conic);
float skia_rs_conic_transform_w(const SkPoint pts[3], float w, const SkMatrix* matrix);
int32_t skia_rs_conic_build_unit_arc(
        SkPoint u_start, SkPoint u_stop, bool dir_is_ccw,
        const SkMatrix* user_matrix, SkConic dst[SkConic::kMaxConicsForArc]);

// Clippers
bool skia_rs_line_clipper_intersect_line(const SkPoint src[2], const SkRect* clip, SkPoint dst[2]);
int32_t skia_rs_line_clipper_clip_line(
        const SkPoint pts[2], const SkRect* clip, SkPoint lines[4], bool can_cull_to_the_right);
bool skia_rs_quad_clipper_clip_quad(const SkRect* clip, const SkPoint src_pts[3], SkPoint dst[3]);
bool skia_rs_cubic_clipper_chop_mono_at_y(const SkPoint pts[4], float y, float* t_out);
bool skia_rs_cubic_clipper_clip_cubic(const SkRect* clip, const SkPoint src_pts[4], SkPoint dst[4]);
bool skia_rs_edge_clipper_clip_line(
        SkPoint p0, SkPoint p1, const SkRect* clip, bool can_cull_to_the_right,
        uint8_t* verbs_out, size_t* verb_count_out, SkPoint* pts_out);
bool skia_rs_edge_clipper_clip_quad(
        const SkPoint src_pts[3], const SkRect* clip, bool can_cull_to_the_right,
        uint8_t* verbs_out, size_t* verb_count_out, SkPoint* pts_out);
bool skia_rs_edge_clipper_clip_cubic(
        const SkPoint src_pts[4], const SkRect* clip, bool can_cull_to_the_right,
        uint8_t* verbs_out, size_t* verb_count_out, SkPoint* pts_out);
}  // extern "C"

// --- SkGeometry ---

int SkFindUnitQuadRoots(SkScalar A, SkScalar B, SkScalar C, SkScalar roots[2]) {
    return skia_rs_find_unit_quad_roots(A, B, C, roots);
}

void SkEvalQuadAt(const SkPoint src[3], SkScalar t, SkPoint* pt, SkVector* tangent) {
    if (pt) {
        *pt = skia_rs_eval_quad_at(src, t);
    }
    if (tangent) {
        *tangent = skia_rs_eval_quad_tangent_at(src, t);
    }
}

SkPoint SkEvalQuadAt(const SkPoint src[3], SkScalar t) {
    return skia_rs_eval_quad_at(src, t);
}

SkVector SkEvalQuadTangentAt(const SkPoint src[3], SkScalar t) {
    return skia_rs_eval_quad_tangent_at(src, t);
}

void SkChopQuadAt(const SkPoint src[3], SkPoint dst[5], SkScalar t) {
    skia_rs_chop_quad_at(src, dst, t);
}

void SkChopQuadAtHalf(const SkPoint src[3], SkPoint dst[5]) {
    skia_rs_chop_quad_at(src, dst, 0.5f);
}

float SkMeasureAngleBetweenVectors(SkVector a, SkVector b) {
    return skia_rs_measure_angle_between_vectors(a, b);
}

SkVector SkFindBisector(SkVector a, SkVector b) {
    return skia_rs_find_bisector(a, b);
}

float SkFindQuadMidTangent(const SkPoint src[3]) {
    return skia_rs_find_quad_mid_tangent(src);
}

int SkFindQuadExtrema(SkScalar a, SkScalar b, SkScalar c, SkScalar tValue[1]) {
    return skia_rs_find_quad_extrema(a, b, c, tValue);
}

int SkChopQuadAtYExtrema(const SkPoint src[3], SkPoint dst[5]) {
    return skia_rs_chop_quad_at_y_extrema(src, dst);
}

int SkChopQuadAtXExtrema(const SkPoint src[3], SkPoint dst[5]) {
    return skia_rs_chop_quad_at_x_extrema(src, dst);
}

SkScalar SkFindQuadMaxCurvature(const SkPoint src[3]) {
    return skia_rs_find_quad_max_curvature(src);
}

int SkChopQuadAtMaxCurvature(const SkPoint src[3], SkPoint dst[5]) {
    return skia_rs_chop_quad_at_max_curvature(src, dst);
}

void SkConvertQuadToCubic(const SkPoint src[3], SkPoint dst[4]) {
    skia_rs_convert_quad_to_cubic(src, dst);
}

void SkEvalCubicAt(const SkPoint src[4], SkScalar t, SkPoint* loc,
                   SkVector* tangent, SkVector* curvature) {
    skia_rs_eval_cubic_at(src, t, loc, tangent, curvature);
}

int SkFindCubicExtrema(SkScalar a, SkScalar b, SkScalar c, SkScalar d, SkScalar tValues[2]) {
    return skia_rs_find_cubic_extrema(a, b, c, d, tValues);
}

void SkChopCubicAt(const SkPoint src[4], SkPoint dst[7], SkScalar t) {
    skia_rs_chop_cubic_at_1(src, dst, t);
}

void SkChopCubicAt(const SkPoint src[4], SkPoint dst[10], float t0, float t1) {
    skia_rs_chop_cubic_at_2(src, dst, t0, t1);
}

void SkChopCubicAt(const SkPoint src[4], SkPoint dst[], const SkScalar tValues[], int tCount) {
    skia_rs_chop_cubic_at_many(src, dst, tValues, tCount);
}

void SkChopCubicAtHalf(const SkPoint src[4], SkPoint dst[7]) {
    skia_rs_chop_cubic_at_1(src, dst, 0.5f);
}

float SkMeasureNonInflectCubicRotation(const SkPoint pts[4]) {
    return skia_rs_measure_non_inflect_cubic_rotation(pts);
}

float SkFindCubicMidTangent(const SkPoint src[4]) {
    return skia_rs_find_cubic_mid_tangent(src);
}

int SkChopCubicAtYExtrema(const SkPoint src[4], SkPoint dst[10]) {
    return skia_rs_chop_cubic_at_y_extrema(src, dst);
}

int SkChopCubicAtXExtrema(const SkPoint src[4], SkPoint dst[10]) {
    return skia_rs_chop_cubic_at_x_extrema(src, dst);
}

int SkFindCubicInflections(const SkPoint src[4], SkScalar tValues[2]) {
    return skia_rs_find_cubic_inflections(src, tValues);
}

int SkChopCubicAtInflections(const SkPoint src[4], SkPoint dst[10]) {
    return skia_rs_chop_cubic_at_inflections(src, dst);
}

SkCubicType SkClassifyCubic(const SkPoint P[4], double t[2], double s[2], double d[4]) {
    return static_cast<SkCubicType>(skia_rs_classify_cubic(P, t, s, d));
}

int SkFindCubicMaxCurvature(const SkPoint src[4], SkScalar tValues[3]) {
    return skia_rs_find_cubic_max_curvature(src, tValues);
}

int SkChopCubicAtMaxCurvature(const SkPoint src[4], SkPoint dst[13], SkScalar tValues[3]) {
    return skia_rs_chop_cubic_at_max_curvature(src, dst, tValues);
}

SkScalar SkFindCubicCusp(const SkPoint src[4]) {
    return skia_rs_find_cubic_cusp(src);
}

bool SkChopMonoCubicAtY(const SkPoint src[4], SkScalar y, SkPoint dst[7]) {
    return skia_rs_chop_mono_cubic_at_y(src, y, dst);
}

bool SkChopMonoCubicAtX(const SkPoint src[4], SkScalar x, SkPoint dst[7]) {
    return skia_rs_chop_mono_cubic_at_x(src, x, dst);
}

bool SkConic::chopAt(SkScalar t, SkConic dst[2]) const {
    return skia_rs_conic_chop_at(this, t, dst);
}

void SkConic::chopAt(SkScalar t1, SkScalar t2, SkConic* dst) const {
    skia_rs_conic_chop_at_range(this, t1, t2, dst);
}

SkPoint SkConic::evalAt(SkScalar t) const {
    return skia_rs_conic_eval_at(this, t);
}

SkVector SkConic::evalTangentAt(SkScalar t) const {
    return skia_rs_conic_eval_tangent_at(this, t);
}

void SkConic::evalAt(SkScalar t, SkPoint* pt, SkVector* tangent) const {
    if (pt) {
        *pt = skia_rs_conic_eval_at(this, t);
    }
    if (tangent) {
        *tangent = skia_rs_conic_eval_tangent_at(this, t);
    }
}

void SkConic::chop(SkConic* SK_RESTRICT dst) const {
    skia_rs_conic_chop(this, dst);
}

void SkConic::computeAsQuadError(SkVector* err) const {
    *err = skia_rs_conic_compute_as_quad_error(this);
}

bool SkConic::asQuadTol(SkScalar tol) const {
    return skia_rs_conic_as_quad_tol(this, tol);
}

int SkConic::computeQuadPOW2(SkScalar tol) const {
    return skia_rs_conic_compute_quad_pow2(this, tol);
}

int SkConic::chopIntoQuadsPOW2(SkPoint pts[], int pow2) const {
    return skia_rs_conic_chop_into_quads_pow2(this, pts, pow2);
}

float SkConic::findMidTangent() const {
    return skia_rs_conic_find_mid_tangent(this);
}

bool SkConic::findXExtrema(SkScalar* t) const {
    return skia_rs_conic_find_x_extrema(this, t);
}

bool SkConic::findYExtrema(SkScalar* t) const {
    return skia_rs_conic_find_y_extrema(this, t);
}

bool SkConic::chopAtXExtrema(SkConic dst[2]) const {
    return skia_rs_conic_chop_at_x_extrema(this, dst);
}

bool SkConic::chopAtYExtrema(SkConic dst[2]) const {
    return skia_rs_conic_chop_at_y_extrema(this, dst);
}

void SkConic::computeTightBounds(SkRect* bounds) const {
    *bounds = skia_rs_conic_compute_tight_bounds(this);
}

void SkConic::computeFastBounds(SkRect* bounds) const {
    *bounds = skia_rs_conic_compute_fast_bounds(this);
}

SkScalar SkConic::TransformW(const SkPoint pts[3], SkScalar w, const SkMatrix& matrix) {
    return skia_rs_conic_transform_w(pts, w, &matrix);
}

int SkConic::BuildUnitArc(const SkVector& uStart, const SkVector& uStop, SkPathDirection dir,
                          const SkMatrix* userMatrix, SkConic dst[kMaxConicsForArc]) {
    return skia_rs_conic_build_unit_arc(
            uStart, uStop, dir == SkPathDirection::kCCW, userMatrix, dst);
}

// --- SkLineClipper ---

bool SkLineClipper::IntersectLine(const SkPoint src[2], const SkRect& clip, SkPoint dst[2]) {
    return skia_rs_line_clipper_intersect_line(src, &clip, dst);
}

int SkLineClipper::ClipLine(const SkPoint pts[2], const SkRect& clip, SkPoint lines[kMaxPoints],
                            bool canCullToTheRight) {
    return skia_rs_line_clipper_clip_line(pts, &clip, lines, canCullToTheRight);
}

// --- SkQuadClipper ---

SkQuadClipper::SkQuadClipper() {
    fClip.setEmpty();
}

void SkQuadClipper::setClip(const SkIRect& clip) {
    fClip.set(clip);
}

bool SkQuadClipper::clipQuad(SkSpan<const SkPoint, 3> srcPts, SkSpan<SkPoint, 3> dst) {
    return skia_rs_quad_clipper_clip_quad(&fClip, srcPts.data(), dst.data());
}

// --- SkCubicClipper ---

SkCubicClipper::SkCubicClipper() {
    fClip.setEmpty();
}

void SkCubicClipper::setClip(const SkIRect& clip) {
    fClip.set(clip);
}

bool SkCubicClipper::ChopMonoAtY(SkSpan<const SkPoint, 4> pts, SkScalar y, SkScalar* t) {
    return skia_rs_cubic_clipper_chop_mono_at_y(pts.data(), y, t);
}

bool SkCubicClipper::clipCubic(SkSpan<const SkPoint, 4> srcPts, SkSpan<SkPoint, 4> dst) {
    return skia_rs_cubic_clipper_clip_cubic(&fClip, srcPts.data(), dst.data());
}

// --- SkEdgeClipper ---

static_assert(static_cast<uint8_t>(SkPathVerb::kLine) == 1);
static_assert(static_cast<uint8_t>(SkPathVerb::kQuad) == 2);
static_assert(static_cast<uint8_t>(SkPathVerb::kCubic) == 4);

bool SkEdgeClipper::clipLine(SkPoint p0, SkPoint p1, const SkRect& clip) {
    uint8_t verbs[kMaxVerbs];
    size_t verbCount = 0;
    bool ok = skia_rs_edge_clipper_clip_line(
            p0, p1, &clip, fCanCullToTheRight, verbs, &verbCount, fPoints);
    for (size_t i = 0; i < verbCount; ++i) {
        fVerbs[i] = static_cast<SkPathVerb>(verbs[i]);
    }
    fCurrVerbStop = fVerbs + verbCount;
    fCurrPoint = fPoints;
    fCurrVerb = fVerbs;
    return ok;
}

bool SkEdgeClipper::clipQuad(const SkPoint srcPts[3], const SkRect& clip) {
    uint8_t verbs[kMaxVerbs];
    size_t verbCount = 0;
    bool ok = skia_rs_edge_clipper_clip_quad(
            srcPts, &clip, fCanCullToTheRight, verbs, &verbCount, fPoints);
    for (size_t i = 0; i < verbCount; ++i) {
        fVerbs[i] = static_cast<SkPathVerb>(verbs[i]);
    }
    fCurrVerbStop = fVerbs + verbCount;
    fCurrPoint = fPoints;
    fCurrVerb = fVerbs;
    return ok;
}

bool SkEdgeClipper::clipCubic(const SkPoint srcPts[4], const SkRect& clip) {
    uint8_t verbs[kMaxVerbs];
    size_t verbCount = 0;
    bool ok = skia_rs_edge_clipper_clip_cubic(
            srcPts, &clip, fCanCullToTheRight, verbs, &verbCount, fPoints);
    for (size_t i = 0; i < verbCount; ++i) {
        fVerbs[i] = static_cast<SkPathVerb>(verbs[i]);
    }
    fCurrVerbStop = fVerbs + verbCount;
    fCurrPoint = fPoints;
    fCurrVerb = fVerbs;
    return ok;
}

std::optional<SkPathVerb> SkEdgeClipper::next(SkPoint pts[]) {
    if (fCurrVerb >= fCurrVerbStop) {
        return {};
    }
    auto verb = *fCurrVerb++;
    switch (verb) {
        case SkPathVerb::kLine:
            memcpy(pts, fCurrPoint, 2 * sizeof(SkPoint));
            fCurrPoint += 2;
            break;
        case SkPathVerb::kQuad:
            memcpy(pts, fCurrPoint, 3 * sizeof(SkPoint));
            fCurrPoint += 3;
            break;
        case SkPathVerb::kCubic:
            memcpy(pts, fCurrPoint, 4 * sizeof(SkPoint));
            fCurrPoint += 4;
            break;
        default:
            break;
    }
    return verb;
}

void SkEdgeClipper::ClipPath(const SkPathRaw& raw, const SkRect& clip, bool canCullToTheRight,
                             void (*consume)(SkEdgeClipper*, bool newCtr, void* ctx), void* ctx) {
    SkAutoConicToQuads quadder;
    constexpr float kConicTol = 0.25f;

    SkPathEdgeIter iter(raw);
    SkEdgeClipper clipper(canCullToTheRight);

    while (auto e = iter.next()) {
        switch (e.fEdge) {
            case SkPathEdgeIter::Edge::kLine:
                if (clipper.clipLine(e.fPts[0], e.fPts[1], clip)) {
                    consume(&clipper, e.fIsNewContour, ctx);
                }
                break;
            case SkPathEdgeIter::Edge::kQuad:
                if (clipper.clipQuad(e.fPts, clip)) {
                    consume(&clipper, e.fIsNewContour, ctx);
                }
                break;
            case SkPathEdgeIter::Edge::kConic: {
                const SkPoint* quadPts =
                        quadder.computeQuads(e.fPts, iter.conicWeight(), kConicTol);
                for (int i = 0; i < quadder.countQuads(); ++i) {
                    if (clipper.clipQuad(quadPts, clip)) {
                        consume(&clipper, e.fIsNewContour, ctx);
                    }
                    quadPts += 2;
                }
            } break;
            case SkPathEdgeIter::Edge::kCubic:
                if (clipper.clipCubic(e.fPts, clip)) {
                    consume(&clipper, e.fIsNewContour, ctx);
                }
                break;
            default:
                break;
        }
    }
}
