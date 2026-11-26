/*
 * Safe Rust bridge for Layer 1.2 skia-rs-geometry (Batch 1):
 *   SkPoint, SkPoint3, SkRect / SkIRect / SkRectPriv,
 *   SkRSXform, SkMatrixInvert, SkM44, SkVertState.
 */

#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRect.h"
#include "include/core/SkString.h"
#include "include/core/SkTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFloatingPoint.h"
#include "src/core/SkMatrixInvert.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkPointPriv.h"
#include "src/core/SkRectPriv.h"
#include "src/core/SkStringUtils.h"
#include "src/core/SkVertState.h"
#include "src/core/SkVx.h"

#include <cstddef>
#include <cstdint>
#include <cstring>

static_assert(sizeof(SkPoint) == 8);
static_assert(sizeof(SkPoint3) == 12);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkRSXform) == 16);
static_assert(sizeof(SkV3) == 12);
static_assert(sizeof(SkV4) == 16);
static_assert(sizeof(SkM44) == 64);

extern "C" {

// SkPoint
float skia_rs_point_length(float dx, float dy);
bool skia_rs_point_set_length(SkPoint* pt, float x, float y, float length);
float skia_rs_point_normalize_with_length(SkPoint* pt);
float skia_rs_point_distance_to_line_between_sqd(SkPoint pt, SkPoint a, SkPoint b, int32_t* side_out);
float skia_rs_point_distance_to_line_segment_between_sqd(SkPoint pt, SkPoint a, SkPoint b);

// SkPoint3
float skia_rs_point3_length(float x, float y, float z);
bool skia_rs_point3_normalize(SkPoint3* pt);

// SkIRect & SkRect
bool skia_rs_irect_intersect(SkIRect* dst, const SkIRect* a, const SkIRect* b);
void skia_rs_irect_join(SkIRect* dst, const SkIRect* r);
bool skia_rs_irect_subtract(const SkIRect* a, const SkIRect* b, SkIRect* out);
SkIRect skia_rs_irect_closest_disjoint_edge(const SkIRect* src, const SkIRect* dst);
bool skia_rs_rect_bounds(const SkPoint* pts, size_t count, SkRect* out);
bool skia_rs_rect_intersect(SkRect* dst, const SkRect* r);
bool skia_rs_rect_intersect_pair(SkRect* dst, const SkRect* a, const SkRect* b);
void skia_rs_rect_join(SkRect* dst, const SkRect* r);
bool skia_rs_rect_subtract(const SkRect* a, const SkRect* b, SkRect* out);
void skia_rs_rect_quad_contains_rect_mask(
        const SkM44* m, const SkRect* a, const SkRect* b, float tol, int32_t out_mask[4]);

// SkRSXform
void skia_rs_rsxform_to_quad(const SkRSXform* xform, float width, float height, SkPoint quad[4]);
void skia_rs_rsxform_to_tri_strip(const SkRSXform* xform, float width, float height, SkPoint strip[4]);

// SkMatrixInvert
float skia_rs_invert_2x2_matrix(const float in_mat[4], float out_mat[4]);
float skia_rs_invert_3x3_matrix(const float in_mat[9], float out_mat[9]);
float skia_rs_invert_4x4_matrix(const float in_mat[16], float out_mat[16]);

// SkM44
bool skia_rs_m44_eq(const SkM44* a, const SkM44* b);
void skia_rs_m44_get_row_major(const SkM44* m, float dst[16]);
void skia_rs_m44_set_concat(SkM44* dst, const SkM44* a, const SkM44* b);
void skia_rs_m44_pre_concat_m33(SkM44* dst, const float b[9]);
void skia_rs_m44_pre_translate(SkM44* dst, float x, float y, float z);
void skia_rs_m44_post_translate(SkM44* dst, float x, float y, float z);
void skia_rs_m44_pre_scale_xy(SkM44* dst, float x, float y);
void skia_rs_m44_pre_scale_xyz(SkM44* dst, float x, float y, float z);
SkV4 skia_rs_m44_map(const SkM44* m, float x, float y, float z, float w);
SkRect skia_rs_m44_map_rect(const SkM44* m, const SkRect* src);
void skia_rs_m44_normalize_perspective(SkM44* dst);
bool skia_rs_m44_invert(const SkM44* m, SkM44* out_inv);
void skia_rs_m44_transpose(const SkM44* m, SkM44* out);
void skia_rs_m44_set_rotate_unit_sin_cos(SkM44* dst, SkV3 axis, float sin_angle, float cos_angle);
void skia_rs_m44_set_rotate(SkM44* dst, SkV3 axis, float radians);
void skia_rs_m44_rect_to_rect(const SkRect* src, const SkRect* dst, SkM44* out);
void skia_rs_m44_look_at(const SkV3* eye, const SkV3* center, const SkV3* up, SkM44* out);
void skia_rs_m44_perspective(float near, float far, float angle, SkM44* out);

// SkVertState
bool skia_rs_vert_state_step(
        int32_t mode, const uint16_t* indices, int32_t count,
        int32_t* curr_index, int32_t* f0, int32_t* f1, int32_t* f2);

}  // extern "C"

// --- SkPoint ---

void SkPoint::scale(float scale, SkPoint* dst) const {
    SkASSERT(dst);
    dst->set(fX * scale, fY * scale);
}

bool SkPoint::normalize() {
    return skia_rs_point_set_length(this, fX, fY, 1.0f);
}

bool SkPoint::setNormalize(float x, float y) {
    return skia_rs_point_set_length(this, x, y, 1.0f);
}

bool SkPoint::setLength(float length) {
    return skia_rs_point_set_length(this, fX, fY, length);
}

bool SkPoint::setLength(float x, float y, float length) {
    return skia_rs_point_set_length(this, x, y, length);
}

float SkPoint::Normalize(SkPoint* pt) {
    return skia_rs_point_normalize_with_length(pt);
}

float SkPoint::Length(float dx, float dy) {
    return skia_rs_point_length(dx, dy);
}

bool SkPointPriv::SetLengthFast(SkPoint* pt, float length) {
    return skia_rs_point_set_length(pt, pt->fX, pt->fY, length);
}

float SkPointPriv::DistanceToLineBetweenSqd(const SkPoint& pt, const SkPoint& a,
                                            const SkPoint& b, Side* side) {
    int32_t s = 0;
    float d2 = skia_rs_point_distance_to_line_between_sqd(pt, a, b, side ? &s : nullptr);
    if (side) {
        *side = static_cast<Side>(s);
    }
    return d2;
}

float SkPointPriv::DistanceToLineSegmentBetweenSqd(const SkPoint& pt, const SkPoint& a,
                                                   const SkPoint& b) {
    return skia_rs_point_distance_to_line_segment_between_sqd(pt, a, b);
}

// --- SkPoint3 ---

SkScalar SkPoint3::Length(SkScalar x, SkScalar y, SkScalar z) {
    return skia_rs_point3_length(x, y, z);
}

bool SkPoint3::normalize() {
    return skia_rs_point3_normalize(this);
}

// --- SkIRect & SkRect ---

bool SkIRect::intersect(const SkIRect& a, const SkIRect& b) {
    return skia_rs_irect_intersect(this, &a, &b);
}

void SkIRect::join(const SkIRect& r) {
    skia_rs_irect_join(this, &r);
}

std::optional<SkRect> SkRect::Bounds(SkSpan<const SkPoint> points) {
    SkRect out;
    if (skia_rs_rect_bounds(points.data(), points.size(), &out)) {
        return out;
    }
    return {};
}

bool SkRect::setBoundsCheck(SkSpan<const SkPoint> pts) {
    SkRect out;
    if (skia_rs_rect_bounds(pts.data(), pts.size(), &out)) {
        *this = out;
        return true;
    }
    *this = MakeEmpty();
    return false;
}

void SkRect::setBoundsNoCheck(SkSpan<const SkPoint> pts) {
    SkRect out;
    if (skia_rs_rect_bounds(pts.data(), pts.size(), &out)) {
        *this = out;
    } else {
        this->setLTRB(SK_FloatNaN, SK_FloatNaN, SK_FloatNaN, SK_FloatNaN);
    }
}

bool SkRect::intersect(const SkRect& r) {
    return skia_rs_rect_intersect(this, &r);
}

bool SkRect::intersect(const SkRect& a, const SkRect& b) {
    return skia_rs_rect_intersect_pair(this, &a, &b);
}

void SkRect::join(const SkRect& r) {
    skia_rs_rect_join(this, &r);
}

static const char* set_scalar_rect_dump(SkString* storage, float value, SkScalarAsStringType asType) {
    storage->reset();
    SkAppendScalar(storage, value, asType);
    return storage->c_str();
}

SkString SkRect::dumpToString(bool asHex) const {
    SkScalarAsStringType asType = asHex ? kHex_SkScalarAsStringType : kDec_SkScalarAsStringType;
    SkString line;
    if (asHex) {
        SkString tmp;
        line.printf( "SkRect::MakeLTRB(%s, /* %f */\n", set_scalar_rect_dump(&tmp, fLeft, asType), fLeft);
        line.appendf("                 %s, /* %f */\n", set_scalar_rect_dump(&tmp, fTop, asType), fTop);
        line.appendf("                 %s, /* %f */\n", set_scalar_rect_dump(&tmp, fRight, asType), fRight);
        line.appendf("                 %s  /* %f */);", set_scalar_rect_dump(&tmp, fBottom, asType), fBottom);
    } else {
        SkString strL, strT, strR, strB;
        SkAppendScalarDec(&strL, fLeft);
        SkAppendScalarDec(&strT, fTop);
        SkAppendScalarDec(&strR, fRight);
        SkAppendScalarDec(&strB, fBottom);
        line.printf("SkRect::MakeLTRB(%s, %s, %s, %s);",
                    strL.c_str(), strT.c_str(), strR.c_str(), strB.c_str());
    }
    return line;
}

void SkRect::dump(bool asHex) const {
    SkDebugf("%s\n", this->dumpToString(asHex).c_str());
}

bool SkRectPriv::Subtract(const SkRect& a, const SkRect& b, SkRect* out) {
    return skia_rs_rect_subtract(&a, &b, out);
}

bool SkRectPriv::Subtract(const SkIRect& a, const SkIRect& b, SkIRect* out) {
    return skia_rs_irect_subtract(&a, &b, out);
}

bool SkRectPriv::QuadContainsRect(const SkMatrix& m,
                                  const SkIRect& a,
                                  const SkIRect& b,
                                  float tol) {
    return QuadContainsRect(SkM44(m), SkRect::Make(a), SkRect::Make(b), tol);
}

bool SkRectPriv::QuadContainsRect(const SkM44& m, const SkRect& a, const SkRect& b, float tol) {
    return all(QuadContainsRectMask(m, a, b, tol));
}

skvx::int4 SkRectPriv::QuadContainsRectMask(const SkM44& m,
                                            const SkRect& a,
                                            const SkRect& b,
                                            float tol) {
    SkDEBUGCODE(SkM44 inverse;)
    SkASSERT(m.invert(&inverse));
    int32_t mask[4];
    skia_rs_rect_quad_contains_rect_mask(&m, &a, &b, tol, mask);
    return skvx::int4::Load(mask);
}

SkIRect SkRectPriv::ClosestDisjointEdge(const SkIRect& src, const SkIRect& dst) {
    return skia_rs_irect_closest_disjoint_edge(&src, &dst);
}

// --- SkRSXform ---

void SkRSXform::toQuad(SkScalar width, SkScalar height, SkPoint quad[4]) const {
    skia_rs_rsxform_to_quad(this, width, height, quad);
}

void SkRSXform::toTriStrip(SkScalar width, SkScalar height, SkPoint strip[4]) const {
    skia_rs_rsxform_to_tri_strip(this, width, height, strip);
}

// --- SkMatrixInvert ---

SkScalar SkInvert2x2Matrix(const SkScalar inMatrix[4], SkScalar outMatrix[4]) {
    return skia_rs_invert_2x2_matrix(inMatrix, outMatrix);
}

SkScalar SkInvert3x3Matrix(const SkScalar inMatrix[9], SkScalar outMatrix[9]) {
    return skia_rs_invert_3x3_matrix(inMatrix, outMatrix);
}

SkScalar SkInvert4x4Matrix(const SkScalar inMatrix[16], SkScalar outMatrix[16]) {
    return skia_rs_invert_4x4_matrix(inMatrix, outMatrix);
}

// --- SkM44 ---

bool SkM44::operator==(const SkM44& other) const {
    return skia_rs_m44_eq(this, &other);
}

void SkM44::getRowMajor(SkScalar v[]) const {
    skia_rs_m44_get_row_major(this, v);
}

SkM44& SkM44::setConcat(const SkM44& a, const SkM44& b) {
    skia_rs_m44_set_concat(this, &a, &b);
    return *this;
}

SkM44& SkM44::preConcat(const SkMatrix& b) {
    float mat[9];
    b.get9(mat);
    skia_rs_m44_pre_concat_m33(this, mat);
    return *this;
}

SkM44& SkM44::preTranslate(SkScalar x, SkScalar y, SkScalar z) {
    skia_rs_m44_pre_translate(this, x, y, z);
    return *this;
}

SkM44& SkM44::postTranslate(SkScalar x, SkScalar y, SkScalar z) {
    skia_rs_m44_post_translate(this, x, y, z);
    return *this;
}

SkM44& SkM44::preScale(SkScalar x, SkScalar y) {
    skia_rs_m44_pre_scale_xy(this, x, y);
    return *this;
}

SkM44& SkM44::preScale(SkScalar x, SkScalar y, SkScalar z) {
    skia_rs_m44_pre_scale_xyz(this, x, y, z);
    return *this;
}

SkV4 SkM44::map(float x, float y, float z, float w) const {
    return skia_rs_m44_map(this, x, y, z, w);
}

SkRect SkMatrixPriv::MapRect(const SkM44& m, const SkRect& src) {
    return skia_rs_m44_map_rect(&m, &src);
}

void SkM44::normalizePerspective() {
    skia_rs_m44_normalize_perspective(this);
}

bool SkM44::invert(SkM44* inverse) const {
    return skia_rs_m44_invert(this, inverse);
}

SkM44 SkM44::transpose() const {
    SkM44 trans(SkM44::kUninitialized_Constructor);
    skia_rs_m44_transpose(this, &trans);
    return trans;
}

SkM44& SkM44::setRotateUnitSinCos(SkV3 axis, SkScalar sinAngle, SkScalar cosAngle) {
    skia_rs_m44_set_rotate_unit_sin_cos(this, axis, sinAngle, cosAngle);
    return *this;
}

SkM44& SkM44::setRotate(SkV3 axis, SkScalar radians) {
    skia_rs_m44_set_rotate(this, axis, radians);
    return *this;
}

void SkM44::dump() const {
    SkDebugf("|%g %g %g %g|\n"
             "|%g %g %g %g|\n"
             "|%g %g %g %g|\n"
             "|%g %g %g %g|\n",
             fMat[0], fMat[4], fMat[8],  fMat[12],
             fMat[1], fMat[5], fMat[9],  fMat[13],
             fMat[2], fMat[6], fMat[10], fMat[14],
             fMat[3], fMat[7], fMat[11], fMat[15]);
}

SkM44 SkM44::RectToRect(const SkRect& src, const SkRect& dst) {
    SkM44 m(SkM44::kUninitialized_Constructor);
    skia_rs_m44_rect_to_rect(&src, &dst, &m);
    return m;
}

SkM44 SkM44::LookAt(const SkV3& eye, const SkV3& center, const SkV3& up) {
    SkM44 m(SkM44::kUninitialized_Constructor);
    skia_rs_m44_look_at(&eye, &center, &up, &m);
    return m;
}

SkM44 SkM44::Perspective(float near, float far, float angle) {
    SkASSERT(far > near);
    SkASSERT(angle * 0.5f != 0);
    SkM44 m(SkM44::kUninitialized_Constructor);
    skia_rs_m44_perspective(near, far, angle, &m);
    return m;
}

// --- SkVertState ---

bool VertState::Triangles(VertState* state) {
    return skia_rs_vert_state_step(
            0, nullptr, state->fCount, &state->fCurrIndex, &state->f0, &state->f1, &state->f2);
}

bool VertState::TrianglesX(VertState* state) {
    return skia_rs_vert_state_step(
            0, state->fIndices, state->fCount, &state->fCurrIndex, &state->f0, &state->f1, &state->f2);
}

bool VertState::TriangleStrip(VertState* state) {
    return skia_rs_vert_state_step(
            1, nullptr, state->fCount, &state->fCurrIndex, &state->f0, &state->f1, &state->f2);
}

bool VertState::TriangleStripX(VertState* state) {
    return skia_rs_vert_state_step(
            1, state->fIndices, state->fCount, &state->fCurrIndex, &state->f0, &state->f1, &state->f2);
}

bool VertState::TriangleFan(VertState* state) {
    return skia_rs_vert_state_step(
            2, nullptr, state->fCount, &state->fCurrIndex, &state->f0, &state->f1, &state->f2);
}

bool VertState::TriangleFanX(VertState* state) {
    return skia_rs_vert_state_step(
            2, state->fIndices, state->fCount, &state->fCurrIndex, &state->f0, &state->f1, &state->f2);
}

VertState::Proc VertState::chooseProc(SkVertices::VertexMode mode) {
    switch (mode) {
        case SkVertices::kTriangles_VertexMode:
            return fIndices ? TrianglesX : Triangles;
        case SkVertices::kTriangleStrip_VertexMode:
            return fIndices ? TriangleStripX : TriangleStrip;
        case SkVertices::kTriangleFan_VertexMode:
            return fIndices ? TriangleFanX : TriangleFan;
        default:
            return nullptr;
    }
}
