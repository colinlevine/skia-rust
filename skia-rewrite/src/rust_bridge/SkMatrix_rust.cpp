/*
 * Safe Rust bridge for src/core/SkMatrix.cpp (Layer 1.2 skia-rs-geometry).
 */

#include "include/core/SkMatrix.h"

#include "include/core/SkPathBuilder.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkSize.h"
#include "include/core/SkSpan.h"
#include "include/core/SkString.h"
#include "include/private/SkDebug.h"
#include "include/private/SkTo.h"
#include "src/core/SkMatrixPriv.h"
#include "src/core/SkMatrixUtils.h"
#include "src/core/SkSamplingPriv.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>

static_assert(sizeof(SkMatrix) == 40);

extern "C" {
void skia_rs_matrix_do_normalize_perspective(SkMatrix* m);
void skia_rs_matrix_reset(SkMatrix* m);
void skia_rs_matrix_set9(SkMatrix* m, const float buf[9]);
void skia_rs_matrix_set_affine(SkMatrix* m, const float buf[6]);
void skia_rs_matrix_set_affine_identity(float buf[6]);
uint8_t skia_rs_matrix_compute_perspective_type_mask(const SkMatrix* m);
uint8_t skia_rs_matrix_compute_type_mask(const SkMatrix* m);
bool skia_rs_matrix_eq(const SkMatrix* a, const SkMatrix* b);
bool skia_rs_matrix_is_similarity(const SkMatrix* m, float tol);
bool skia_rs_matrix_preserves_right_angles(const SkMatrix* m, float tol);
void skia_rs_matrix_set_translate(SkMatrix* m, float dx, float dy);
void skia_rs_matrix_pre_translate(SkMatrix* m, float dx, float dy);
void skia_rs_matrix_post_translate(SkMatrix* m, float dx, float dy);
void skia_rs_matrix_set_scale(SkMatrix* m, float sx, float sy);
void skia_rs_matrix_set_scale_pivot(SkMatrix* m, float sx, float sy, float px, float py);
void skia_rs_matrix_pre_scale(SkMatrix* m, float sx, float sy);
void skia_rs_matrix_pre_scale_pivot(SkMatrix* m, float sx, float sy, float px, float py);
void skia_rs_matrix_post_scale(SkMatrix* m, float sx, float sy);
void skia_rs_matrix_post_scale_pivot(SkMatrix* m, float sx, float sy, float px, float py);
bool skia_rs_matrix_post_idiv(SkMatrix* m, int32_t divx, int32_t divy);
void skia_rs_matrix_set_sin_cos(SkMatrix* m, float sin_v, float cos_v);
void skia_rs_matrix_set_sin_cos_pivot(SkMatrix* m, float sin_v, float cos_v, float px, float py);
void skia_rs_matrix_set_rsxform(SkMatrix* m, const SkRSXform* xform);
void skia_rs_matrix_set_rotate(SkMatrix* m, float degrees);
void skia_rs_matrix_set_rotate_pivot(SkMatrix* m, float degrees, float px, float py);
void skia_rs_matrix_pre_rotate(SkMatrix* m, float degrees);
void skia_rs_matrix_pre_rotate_pivot(SkMatrix* m, float degrees, float px, float py);
void skia_rs_matrix_post_rotate(SkMatrix* m, float degrees);
void skia_rs_matrix_post_rotate_pivot(SkMatrix* m, float degrees, float px, float py);
void skia_rs_matrix_set_skew(SkMatrix* m, float sx, float sy);
void skia_rs_matrix_set_skew_pivot(SkMatrix* m, float sx, float sy, float px, float py);
void skia_rs_matrix_pre_skew(SkMatrix* m, float sx, float sy);
void skia_rs_matrix_pre_skew_pivot(SkMatrix* m, float sx, float sy, float px, float py);
void skia_rs_matrix_post_skew(SkMatrix* m, float sx, float sy);
void skia_rs_matrix_post_skew_pivot(SkMatrix* m, float sx, float sy, float px, float py);
void skia_rs_matrix_scale_translate(float sx, float sy, float tx, float ty, SkMatrix* out);
bool skia_rs_matrix_rect_to_rect(const SkRect* src, const SkRect* dst, int32_t stf, SkMatrix* out);
void skia_rs_matrix_set_concat(SkMatrix* dst, const SkMatrix* a, const SkMatrix* b);
void skia_rs_matrix_pre_concat(SkMatrix* dst, const SkMatrix* m);
void skia_rs_matrix_post_concat(SkMatrix* dst, const SkMatrix* m);
bool skia_rs_matrix_as_affine(const SkMatrix* m, float affine[6]);
void skia_rs_matrix_compute_inv(float dst[9], const float src[9], double inv_det, bool is_persp);
bool skia_rs_matrix_invert(const SkMatrix* m, SkMatrix* out);
SkPoint skia_rs_matrix_map_point_perspective(const SkMatrix* m, SkPoint pt);
void skia_rs_matrix_map_pts_proc(
        int32_t proc_kind, const SkMatrix* m, SkPoint* dst, const SkPoint* src, int32_t count);
void skia_rs_matrix_map_homogeneous_with_stride(
        const SkMatrix* m, uint8_t* dst, size_t dst_stride,
        const uint8_t* src, size_t src_stride, int32_t count);
void skia_rs_matrix_map_points_to_homogeneous(
        const SkMatrix* m, SkPoint3* dst, const SkPoint* src, size_t count);
void skia_rs_matrix_map_vectors(
        const SkMatrix* m, SkPoint* dst, const SkPoint* src, size_t count);
void skia_rs_matrix_map_rect_scale_translate(const SkMatrix* m, SkRect* dst, const SkRect* src);
bool skia_rs_matrix_map_rect_non_perspective(
        const SkMatrix* m, SkRect* dst, const SkRect* src, bool* rect_stays_rect_out);
float skia_rs_matrix_map_radius(const SkMatrix* m, float radius);
bool skia_rs_matrix_poly_to_poly(
        const SkPoint* src, size_t src_len, const SkPoint* dst, size_t dst_len, SkMatrix* out);
float skia_rs_matrix_get_min_scale(const SkMatrix* m);
float skia_rs_matrix_get_max_scale(const SkMatrix* m);
bool skia_rs_matrix_get_min_max_scales(const SkMatrix* m, float out[2]);
bool skia_rs_matrix_decompose_scale(const SkMatrix* m, SkSize* scale_out, SkMatrix* remaining_out);
bool skia_rs_matrix_decompose_upper_2x2(
        const SkMatrix* m, SkPoint* rot1, SkPoint* scale, SkPoint* rot2);
bool skia_rs_matrix_treat_as_sprite(
        const SkMatrix* m, SkISize size, bool no_change_with_identity,
        bool is_linear_filter, bool is_anti_alias);
float skia_rs_matrix_differential_area_scale(const SkMatrix* m, SkPoint p);
bool skia_rs_matrix_nearly_affine(const SkMatrix* m, const SkRect* bounds, float tolerance);
float skia_rs_matrix_compute_res_scale_for_stroking(const SkMatrix* m);
}  // extern "C"

template <typename S, typename T> static int min_count(SkSpan<S> a, SkSpan<T> b) {
    return SkToInt(std::min(a.size(), b.size()));
}

void SkMatrix::doNormalizePerspective() { skia_rs_matrix_do_normalize_perspective(this); }
SkMatrix& SkMatrix::reset() { skia_rs_matrix_reset(this); return *this; }
SkMatrix& SkMatrix::set9(const SkScalar buffer[9]) { skia_rs_matrix_set9(this, buffer); return *this; }
SkMatrix& SkMatrix::setAffine(const SkScalar buffer[6]) { skia_rs_matrix_set_affine(this, buffer); return *this; }
uint8_t SkMatrix::computePerspectiveTypeMask() const { return skia_rs_matrix_compute_perspective_type_mask(this); }
uint8_t SkMatrix::computeTypeMask() const { return skia_rs_matrix_compute_type_mask(this); }

bool operator==(const SkMatrix& a, const SkMatrix& b) { return skia_rs_matrix_eq(&a, &b); }

bool SkMatrix::isSimilarity(SkScalar tol) const { return skia_rs_matrix_is_similarity(this, tol); }
bool SkMatrix::preservesRightAngles(SkScalar tol) const { return skia_rs_matrix_preserves_right_angles(this, tol); }

SkMatrix& SkMatrix::setTranslate(SkScalar dx, SkScalar dy) { skia_rs_matrix_set_translate(this, dx, dy); return *this; }
SkMatrix& SkMatrix::preTranslate(SkScalar dx, SkScalar dy) { skia_rs_matrix_pre_translate(this, dx, dy); return *this; }
SkMatrix& SkMatrix::postTranslate(SkScalar dx, SkScalar dy) { skia_rs_matrix_post_translate(this, dx, dy); return *this; }

SkMatrix& SkMatrix::setScale(SkScalar sx, SkScalar sy, SkScalar px, SkScalar py) { skia_rs_matrix_set_scale_pivot(this, sx, sy, px, py); return *this; }
SkMatrix& SkMatrix::setScale(SkScalar sx, SkScalar sy) { skia_rs_matrix_set_scale(this, sx, sy); return *this; }
SkMatrix& SkMatrix::preScale(SkScalar sx, SkScalar sy, SkScalar px, SkScalar py) { skia_rs_matrix_pre_scale_pivot(this, sx, sy, px, py); return *this; }
SkMatrix& SkMatrix::preScale(SkScalar sx, SkScalar sy) { skia_rs_matrix_pre_scale(this, sx, sy); return *this; }
SkMatrix& SkMatrix::postScale(SkScalar sx, SkScalar sy, SkScalar px, SkScalar py) { skia_rs_matrix_post_scale_pivot(this, sx, sy, px, py); return *this; }
SkMatrix& SkMatrix::postScale(SkScalar sx, SkScalar sy) { skia_rs_matrix_post_scale(this, sx, sy); return *this; }
bool SkMatrix::postIDiv(int divx, int divy) { return skia_rs_matrix_post_idiv(this, divx, divy); }

SkMatrix& SkMatrix::setSinCos(SkScalar sinV, SkScalar cosV, SkScalar px, SkScalar py) { skia_rs_matrix_set_sin_cos_pivot(this, sinV, cosV, px, py); return *this; }
SkMatrix& SkMatrix::setRSXform(const SkRSXform& xform) { skia_rs_matrix_set_rsxform(this, &xform); return *this; }
SkMatrix& SkMatrix::setSinCos(SkScalar sinV, SkScalar cosV) { skia_rs_matrix_set_sin_cos(this, sinV, cosV); return *this; }
SkMatrix& SkMatrix::setRotate(SkScalar degrees, SkScalar px, SkScalar py) { skia_rs_matrix_set_rotate_pivot(this, degrees, px, py); return *this; }
SkMatrix& SkMatrix::setRotate(SkScalar degrees) { skia_rs_matrix_set_rotate(this, degrees); return *this; }
SkMatrix& SkMatrix::preRotate(SkScalar degrees, SkScalar px, SkScalar py) { skia_rs_matrix_pre_rotate_pivot(this, degrees, px, py); return *this; }
SkMatrix& SkMatrix::preRotate(SkScalar degrees) { skia_rs_matrix_pre_rotate(this, degrees); return *this; }
SkMatrix& SkMatrix::postRotate(SkScalar degrees, SkScalar px, SkScalar py) { skia_rs_matrix_post_rotate_pivot(this, degrees, px, py); return *this; }
SkMatrix& SkMatrix::postRotate(SkScalar degrees) { skia_rs_matrix_post_rotate(this, degrees); return *this; }

SkMatrix& SkMatrix::setSkew(SkScalar sx, SkScalar sy, SkScalar px, SkScalar py) { skia_rs_matrix_set_skew_pivot(this, sx, sy, px, py); return *this; }
SkMatrix& SkMatrix::setSkew(SkScalar sx, SkScalar sy) { skia_rs_matrix_set_skew(this, sx, sy); return *this; }
SkMatrix& SkMatrix::preSkew(SkScalar sx, SkScalar sy, SkScalar px, SkScalar py) { skia_rs_matrix_pre_skew_pivot(this, sx, sy, px, py); return *this; }
SkMatrix& SkMatrix::preSkew(SkScalar sx, SkScalar sy) { skia_rs_matrix_pre_skew(this, sx, sy); return *this; }
SkMatrix& SkMatrix::postSkew(SkScalar sx, SkScalar sy, SkScalar px, SkScalar py) { skia_rs_matrix_post_skew_pivot(this, sx, sy, px, py); return *this; }
SkMatrix& SkMatrix::postSkew(SkScalar sx, SkScalar sy) { skia_rs_matrix_post_skew(this, sx, sy); return *this; }

SkMatrix SkMatrix::ScaleTranslate(float sx, float sy, float tx, float ty) {
    SkMatrix out;
    skia_rs_matrix_scale_translate(sx, sy, tx, ty, &out);
    return out;
}

std::optional<SkMatrix> SkMatrix::Rect2Rect(const SkRect& src, const SkRect& dst, ScaleToFit stf) {
    SkMatrix out;
    if (skia_rs_matrix_rect_to_rect(&src, &dst, static_cast<int32_t>(stf), &out)) {
        return out;
    }
    return {};
}

SkMatrix& SkMatrix::setConcat(const SkMatrix& a, const SkMatrix& b) {
    // Ensure type masks are cached on a & b just like SkMatrix::setConcat
    (void)a.getType();
    (void)b.getType();
    skia_rs_matrix_set_concat(this, &a, &b);
    return *this;
}

SkMatrix& SkMatrix::preConcat(const SkMatrix& mat) {
    skia_rs_matrix_pre_concat(this, &mat);
    return *this;
}

SkMatrix& SkMatrix::postConcat(const SkMatrix& mat) {
    skia_rs_matrix_post_concat(this, &mat);
    return *this;
}

void SkMatrix::SetAffineIdentity(SkScalar affine[6]) {
    skia_rs_matrix_set_affine_identity(affine);
}

bool SkMatrix::asAffine(SkScalar affine[6]) const {
    return skia_rs_matrix_as_affine(this, affine);
}

void SkMatrix::mapPoints(SkSpan<SkPoint> dst, SkSpan<const SkPoint> src) const {
    const auto count = min_count(dst, src);
    this->getMapPtsProc()(*this, dst.data(), src.data(), count);
}

void SkMatrix::ComputeInv(SkScalar dst[9], const SkScalar src[9], double invDet, bool isPersp) {
    skia_rs_matrix_compute_inv(dst, src, invDet, isPersp);
}

std::optional<SkMatrix> SkMatrix::invert() const {
    (void)this->getType();
    SkMatrix out;
    if (skia_rs_matrix_invert(this, &out)) {
        return out;
    }
    return {};
}

SkPoint SkMatrix::mapPointPerspective(SkPoint p) const {
    return skia_rs_matrix_map_point_perspective(this, p);
}

void SkMatrix::Identity_pts(const SkMatrix& m, SkPoint dst[], const SkPoint src[], int count) {
    skia_rs_matrix_map_pts_proc(0, &m, dst, src, count);
}

void SkMatrix::Trans_pts(const SkMatrix& m, SkPoint dst[], const SkPoint src[], int count) {
    skia_rs_matrix_map_pts_proc(1, &m, dst, src, count);
}

void SkMatrix::Scale_pts(const SkMatrix& m, SkPoint dst[], const SkPoint src[], int count) {
    skia_rs_matrix_map_pts_proc(2, &m, dst, src, count);
}

void SkMatrix::Affine_vpts(const SkMatrix& m, SkPoint dst[], const SkPoint src[], int count) {
    skia_rs_matrix_map_pts_proc(3, &m, dst, src, count);
}

void SkMatrix::Persp_pts(const SkMatrix& m, SkPoint dst[], const SkPoint src[], int count) {
    skia_rs_matrix_map_pts_proc(4, &m, dst, src, count);
}

const SkMatrix::MapPtsProc SkMatrix::gMapPtsProcs[] = {
    SkMatrix::Identity_pts, SkMatrix::Trans_pts,
    SkMatrix::Scale_pts,    SkMatrix::Scale_pts,
    SkMatrix::Affine_vpts,  SkMatrix::Affine_vpts,
    SkMatrix::Affine_vpts,  SkMatrix::Affine_vpts,
    SkMatrix::Persp_pts,    SkMatrix::Persp_pts,
    SkMatrix::Persp_pts,    SkMatrix::Persp_pts,
    SkMatrix::Persp_pts,    SkMatrix::Persp_pts,
    SkMatrix::Persp_pts,    SkMatrix::Persp_pts
};

void SkMatrixPriv::MapHomogeneousPointsWithStride(const SkMatrix& mx, SkPoint3 dst[],
                                                  size_t dstStride, const SkPoint3 src[],
                                                  size_t srcStride, int count) {
    skia_rs_matrix_map_homogeneous_with_stride(
            &mx,
            reinterpret_cast<uint8_t*>(dst), dstStride,
            reinterpret_cast<const uint8_t*>(src), srcStride,
            count);
}

void SkMatrix::mapHomogeneousPoints(SkSpan<SkPoint3> dst, SkSpan<const SkPoint3> src) const {
    const auto count = min_count(dst, src);
    SkMatrixPriv::MapHomogeneousPointsWithStride(
            *this, dst.data(), sizeof(SkPoint3), src.data(), sizeof(SkPoint3), count);
}

void SkMatrix::mapPointsToHomogeneous(SkSpan<SkPoint3> dst, SkSpan<const SkPoint> src) const {
    const size_t count = std::min(dst.size(), src.size());
    skia_rs_matrix_map_points_to_homogeneous(this, dst.data(), src.data(), count);
}

void SkMatrix::mapVectors(SkSpan<SkPoint> dst, SkSpan<const SkPoint> src) const {
    const size_t count = std::min(dst.size(), src.size());
    skia_rs_matrix_map_vectors(this, dst.data(), src.data(), count);
}

void SkMatrix::mapRectScaleTranslate(SkRect* dst, const SkRect& src) const {
    SkASSERT(dst);
    SkASSERT(this->isScaleTranslate());
    skia_rs_matrix_map_rect_scale_translate(this, dst, &src);
}

bool SkMatrix::mapRect(SkRect* dst, const SkRect& src) const {
    SkASSERT(dst);
    bool staysRect = false;
    if (skia_rs_matrix_map_rect_non_perspective(this, dst, &src, &staysRect)) {
        return staysRect;
    }
    SkPathBuilder builder;
    builder.addRect(src);
    builder.transform(*this);
    *dst = builder.computeBounds();
    return false;
}

SkScalar SkMatrix::mapRadius(SkScalar radius) const {
    return skia_rs_matrix_map_radius(this, radius);
}

std::optional<SkMatrix> SkMatrix::PolyToPoly(SkSpan<const SkPoint> src, SkSpan<const SkPoint> dst) {
    SkMatrix out;
    if (skia_rs_matrix_poly_to_poly(src.data(), src.size(), dst.data(), dst.size(), &out)) {
        return out;
    }
    return {};
}

SkScalar SkMatrix::getMinScale() const { return skia_rs_matrix_get_min_scale(this); }
SkScalar SkMatrix::getMaxScale() const { return skia_rs_matrix_get_max_scale(this); }

bool SkMatrix::getMinMaxScales(SkSpan<SkScalar, 2> scaleFactors) const {
    return skia_rs_matrix_get_min_max_scales(this, scaleFactors.data());
}

bool SkMatrix::getMinMaxScales(SkScalar scaleFactors[2]) const {
    return skia_rs_matrix_get_min_max_scales(this, scaleFactors);
}

const SkMatrix& SkMatrix::I() {
    static constexpr SkMatrix identity;
    return identity;
}

const SkMatrix& SkMatrix::InvalidMatrix() {
    static constexpr SkMatrix invalid(SK_ScalarMax, SK_ScalarMax, SK_ScalarMax,
                                      SK_ScalarMax, SK_ScalarMax, SK_ScalarMax,
                                      SK_ScalarMax, SK_ScalarMax, SK_ScalarMax,
                                      kTranslate_Mask | kScale_Mask |
                                      kAffine_Mask | kPerspective_Mask);
    return invalid;
}

bool SkMatrix::decomposeScale(SkSize* scale, SkMatrix* remaining) const {
    return skia_rs_matrix_decompose_scale(this, scale, remaining);
}

size_t SkMatrix::writeToMemory(void* buffer) const {
    static const size_t sizeInMemory = 9 * sizeof(SkScalar);
    if (buffer) {
        memcpy(buffer, fMat, sizeInMemory);
    }
    return sizeInMemory;
}

size_t SkMatrix::readFromMemory(const void* buffer, size_t length) {
    static const size_t sizeInMemory = 9 * sizeof(SkScalar);
    if (length < sizeInMemory) {
        return 0;
    }
    memcpy(fMat, buffer, sizeInMemory);
    this->setTypeMask(kUnknown_Mask);
    (void)this->getType();
    return sizeInMemory;
}

void SkMatrix::dump() const {
    SkString str;
    str.appendf("[%8.4f %8.4f %8.4f][%8.4f %8.4f %8.4f][%8.4f %8.4f %8.4f]",
                fMat[0], fMat[1], fMat[2], fMat[3], fMat[4], fMat[5],
                fMat[6], fMat[7], fMat[8]);
    SkDebugf("%s\n", str.c_str());
}

bool SkTreatAsSprite(const SkMatrix& mat, const SkISize& size, const SkSamplingOptions& sampling,
                     bool isAntiAlias) {
    return skia_rs_matrix_treat_as_sprite(
            &mat,
            size,
            SkSamplingPriv::NoChangeWithIdentityMatrix(sampling),
            sampling.filter == SkFilterMode::kLinear,
            isAntiAlias);
}

bool SkDecomposeUpper2x2(const SkMatrix& matrix,
                         SkPoint* rotation1,
                         SkPoint* scale,
                         SkPoint* rotation2) {
    return skia_rs_matrix_decompose_upper_2x2(&matrix, rotation1, scale, rotation2);
}

SkScalar SkMatrixPriv::DifferentialAreaScale(const SkMatrix& m, const SkPoint& p) {
    return skia_rs_matrix_differential_area_scale(&m, p);
}

bool SkMatrixPriv::NearlyAffine(const SkMatrix& m, const SkRect& bounds, SkScalar tolerance) {
    return skia_rs_matrix_nearly_affine(&m, &bounds, tolerance);
}

SkScalar SkMatrixPriv::ComputeResScaleForStroking(const SkMatrix& matrix) {
    return skia_rs_matrix_compute_res_scale_for_stroking(&matrix);
}
