/*
 * Safe Rust bridge for Layer 1.1 skia-rs-math core modules:
 *   SkBase64, SkChecksum, SkHalf, SkScalar, SkFloatingPoint,
 *   SkSafeMath, SkMathPriv, SkQuads, SkCubics, SkBezierCurves,
 *   SkCubicMap, SkUTF.
 */

#include "include/core/SkCubicMap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSpan.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkTFitsIn.h"
#include "src/core/SkBase64.h"
#include "src/core/SkBezierCurves.h"
#include "src/core/SkChecksum.h"
#include "src/core/SkCubics.h"
#include "src/core/SkHalf.h"
#include "src/core/SkMathPriv.h"
#include "src/core/SkQuads.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkUTF.h"

#include <array>
#include <cstddef>
#include <cstdint>

extern "C" {

// SkBase64
int32_t skia_rs_base64_decode(const uint8_t* src, size_t src_len, uint8_t* dst, size_t* dst_len_out);
size_t skia_rs_base64_encode(const uint8_t* src, size_t len, uint8_t* dst, const uint8_t* encode_map);

// SkChecksum
uint32_t skia_rs_checksum_hash32(const uint8_t* data, size_t bytes, uint32_t seed);
uint64_t skia_rs_checksum_hash64(const uint8_t* data, size_t bytes, uint64_t seed);

// SkHalf
uint16_t skia_rs_float_to_half(float f);
float skia_rs_half_to_float(uint16_t h);

// SkScalar
float skia_rs_float_interp_func(float search_key, const float* keys, const float* values, int32_t length);

// SkFloatingPoint
bool skia_rs_doubles_nearly_equal_ulps(double a, double b, uint8_t max_ulps_diff);
bool skia_rs_double_nearly_zero(double a);

// SkSafeMath & SkMathPriv
size_t skia_rs_safe_math_add(size_t x, size_t y);
size_t skia_rs_safe_math_mul(size_t x, size_t y);
int32_t skia_rs_sqrt_bits(int32_t x, int32_t count);

// SkQuads
struct SkiaRsQuadRoots {
    double discriminant;
    double root0;
    double root1;
};
double skia_rs_quads_discriminant(double a, double b, double c);
SkiaRsQuadRoots skia_rs_quads_roots(double a, double b, double c);
int32_t skia_rs_quads_roots_real(double a, double b, double c, double solution[2]);
double skia_rs_quads_eval_at(double a, double b, double c, double t);

// SkCubics
int32_t skia_rs_cubics_roots_real(double a, double b, double c, double d, double solution[3]);
int32_t skia_rs_cubics_roots_valid_t(double a, double b, double c, double d, double solution[3]);
int32_t skia_rs_cubics_binary_search_roots_valid_t(
        double a, double b, double c, double d, double solution[3]);

// SkBezierCurves
void skia_rs_bezier_cubic_eval_at(const double curve[8], double t, double out[2]);
void skia_rs_bezier_cubic_subdivide(const double curve[8], double t, double out[14]);
void skia_rs_bezier_cubic_convert_to_poly(const double curve[8], bool y_values, double out[4]);
size_t skia_rs_bezier_cubic_intersect_horiz(
        const float pts[4][2], float y_intercept, float storage[3]);
size_t skia_rs_bezier_cubic_intersect(
        double ax, double bx, double cx, double dx,
        double ay, double by, double cy, double dy,
        float to_intersect, float storage[3]);
size_t skia_rs_bezier_quad_intersect_horiz(
        const float pts[3][2], float y_intercept, float storage[2]);
size_t skia_rs_bezier_quad_intersect(
        double ax, double bx, double cx,
        double ay, double by, double cy,
        double y_intercept, float storage[2]);

// SkCubicMap
struct SkiaRsCubicMap {
    float coeff[3][2];
    int32_t map_type;
};
void skia_rs_cubic_map_init(float p1x, float p1y, float p2x, float p2y, SkiaRsCubicMap* out);
float skia_rs_cubic_map_y_from_x(const SkiaRsCubicMap* raw, float x);
void skia_rs_cubic_map_from_t(const SkiaRsCubicMap* raw, float t, float out[2]);

// SkUTF
int32_t skia_rs_utf_count_utf8(const uint8_t* ptr, size_t len);
int32_t skia_rs_utf_count_utf16(const uint16_t* ptr, size_t count);
int32_t skia_rs_utf_count_utf32(const int32_t* ptr, size_t count);
int32_t skia_rs_utf_next_utf8(const uint8_t* ptr, size_t len, size_t* consumed_out);
int32_t skia_rs_utf_next_utf16(const uint16_t* ptr, size_t len, size_t* consumed_out);
int32_t skia_rs_utf_next_utf32(const int32_t* ptr, size_t len, size_t* consumed_out);
size_t skia_rs_utf_to_utf8(int32_t uni, uint8_t dst[4]);
size_t skia_rs_utf_to_utf16(int32_t uni, uint16_t dst[2]);
int32_t skia_rs_utf_utf8_to_utf16(uint16_t* dst, int32_t dst_cap, const uint8_t* src, size_t src_len);
int32_t skia_rs_utf_utf16_to_utf8(uint8_t* dst, int32_t dst_cap, const uint16_t* src, size_t src_len);

}  // extern "C"

// --- SkBase64 ---

SkBase64::Error SkBase64::Decode(const void* srcv, size_t srcLength, void* dstv, size_t* dstLength) {
    return static_cast<SkBase64::Error>(skia_rs_base64_decode(
            static_cast<const uint8_t*>(srcv),
            srcLength,
            static_cast<uint8_t*>(dstv),
            dstLength));
}

size_t SkBase64::Encode(const void* srcv, size_t length, void* dstv, const char* encodeMap) {
    return skia_rs_base64_encode(
            static_cast<const uint8_t*>(srcv),
            length,
            static_cast<uint8_t*>(dstv),
            reinterpret_cast<const uint8_t*>(encodeMap));
}

// --- SkChecksum ---

namespace SkChecksum {
uint32_t Hash32(const void* data, size_t bytes, uint32_t seed) {
    return skia_rs_checksum_hash32(static_cast<const uint8_t*>(data), bytes, seed);
}
uint64_t Hash64(const void* data, size_t bytes, uint64_t seed) {
    return skia_rs_checksum_hash64(static_cast<const uint8_t*>(data), bytes, seed);
}
}  // namespace SkChecksum

// --- SkHalf ---

SkHalf SkFloatToHalf(float f) {
    return skia_rs_float_to_half(f);
}

float SkHalfToFloat(SkHalf h) {
    return skia_rs_half_to_float(h);
}

// --- SkScalar ---

float SkFloatInterpFunc(float searchKey, const float keys[], const float values[], int length) {
    return skia_rs_float_interp_func(searchKey, keys, values, length);
}

// --- SkFloatingPoint ---

bool sk_doubles_nearly_equal_ulps(double a, double b, uint8_t maxUlpsDiff) {
    return skia_rs_doubles_nearly_equal_ulps(a, b, maxUlpsDiff);
}

bool sk_double_nearly_zero(double a) {
    return skia_rs_double_nearly_zero(a);
}

// --- SkSafeMath & SkMathPriv ---

size_t SkSafeMath::Add(size_t x, size_t y) {
    return skia_rs_safe_math_add(x, y);
}

size_t SkSafeMath::Mul(size_t x, size_t y) {
    return skia_rs_safe_math_mul(x, y);
}

int32_t SkSqrtBits(int32_t x, int count) {
    return skia_rs_sqrt_bits(x, count);
}

// --- SkQuads ---

double SkQuads::Discriminant(const double a, const double b, const double c) {
    return skia_rs_quads_discriminant(a, b, c);
}

SkQuads::RootResult SkQuads::Roots(double A, double B, double C) {
    SkiaRsQuadRoots r = skia_rs_quads_roots(A, B, C);
    return {r.discriminant, r.root0, r.root1};
}

int SkQuads::RootsReal(const double A, const double B, const double C, double solution[2]) {
    return skia_rs_quads_roots_real(A, B, C, solution);
}

double SkQuads::EvalAt(double A, double B, double C, double t) {
    return skia_rs_quads_eval_at(A, B, C, t);
}

// --- SkCubics ---

int SkCubics::RootsReal(double A, double B, double C, double D, double solution[3]) {
    return skia_rs_cubics_roots_real(A, B, C, D, solution);
}

int SkCubics::RootsValidT(double A, double B, double C, double D, double solution[3]) {
    return skia_rs_cubics_roots_valid_t(A, B, C, D, solution);
}

int SkCubics::BinarySearchRootsValidT(
        double A, double B, double C, double D, SkSpan<double, 3> solution) {
    return skia_rs_cubics_binary_search_roots_valid_t(A, B, C, D, solution.data());
}

// --- SkBezierCurves ---

std::array<double, 2> SkBezierCubic::EvalAt(const double curve[8], double t) {
    std::array<double, 2> out;
    skia_rs_bezier_cubic_eval_at(curve, t, out.data());
    return out;
}

void SkBezierCubic::Subdivide(SkSpan<const double, 8> curve,
                              double t,
                              SkSpan<double, 14> twoCurves) {
    skia_rs_bezier_cubic_subdivide(curve.data(), t, twoCurves.data());
}

std::array<double, 4> SkBezierCubic::ConvertToPolynomial(const double curve[8], bool yValues) {
    std::array<double, 4> out;
    skia_rs_bezier_cubic_convert_to_poly(curve, yValues, out.data());
    return out;
}

SkSpan<const float> SkBezierCubic::IntersectWithHorizontalLine(
        SkSpan<const SkPoint> controlPoints, float yIntercept, float* intersectionStorage) {
    float pts[4][2] = {
        {controlPoints[0].fX, controlPoints[0].fY},
        {controlPoints[1].fX, controlPoints[1].fY},
        {controlPoints[2].fX, controlPoints[2].fY},
        {controlPoints[3].fX, controlPoints[3].fY},
    };
    size_t n = skia_rs_bezier_cubic_intersect_horiz(pts, yIntercept, intersectionStorage);
    return {intersectionStorage, n};
}

SkSpan<const float> SkBezierCubic::Intersect(
        double AX, double BX, double CX, double DX,
        double AY, double BY, double CY, double DY,
        float toIntersect, float intersectionsStorage[3]) {
    size_t n = skia_rs_bezier_cubic_intersect(
            AX, BX, CX, DX, AY, BY, CY, DY, toIntersect, intersectionsStorage);
    return {intersectionsStorage, n};
}

SkSpan<const float> SkBezierQuad::IntersectWithHorizontalLine(
        SkSpan<const SkPoint> controlPoints, float yIntercept, float intersectionStorage[2]) {
    float pts[3][2] = {
        {controlPoints[0].fX, controlPoints[0].fY},
        {controlPoints[1].fX, controlPoints[1].fY},
        {controlPoints[2].fX, controlPoints[2].fY},
    };
    size_t n = skia_rs_bezier_quad_intersect_horiz(pts, yIntercept, intersectionStorage);
    return {intersectionStorage, n};
}

SkSpan<const float> SkBezierQuad::Intersect(
        double AX, double BX, double CX,
        double AY, double BY, double CY,
        double yIntercept, float intersectionStorage[2]) {
    size_t n = skia_rs_bezier_quad_intersect(
            AX, BX, CX, AY, BY, CY, yIntercept, intersectionStorage);
    return {intersectionStorage, n};
}

// --- SkCubicMap ---

SkCubicMap::SkCubicMap(SkPoint p1, SkPoint p2) {
    SkiaRsCubicMap raw;
    skia_rs_cubic_map_init(p1.fX, p1.fY, p2.fX, p2.fY, &raw);
    for (int i = 0; i < 3; ++i) {
        fCoeff[i] = {raw.coeff[i][0], raw.coeff[i][1]};
    }
    fType = static_cast<Type>(raw.map_type);
}

float SkCubicMap::computeYFromX(float x) const {
    SkiaRsCubicMap raw;
    for (int i = 0; i < 3; ++i) {
        raw.coeff[i][0] = fCoeff[i].fX;
        raw.coeff[i][1] = fCoeff[i].fY;
    }
    raw.map_type = static_cast<int32_t>(fType);
    return skia_rs_cubic_map_y_from_x(&raw, x);
}

SkPoint SkCubicMap::computeFromT(float t) const {
    SkiaRsCubicMap raw;
    for (int i = 0; i < 3; ++i) {
        raw.coeff[i][0] = fCoeff[i].fX;
        raw.coeff[i][1] = fCoeff[i].fY;
    }
    raw.map_type = static_cast<int32_t>(fType);
    float out[2];
    skia_rs_cubic_map_from_t(&raw, t, out);
    return {out[0], out[1]};
}

// --- SkUTF ---

static constexpr bool is_align2(uintptr_t x) { return (x & 1) == 0; }
static constexpr bool is_align4(uintptr_t x) { return (x & 3) == 0; }

int SkUTF::CountUTF8(const char* utf8, size_t byteLength) {
    return skia_rs_utf_count_utf8(reinterpret_cast<const uint8_t*>(utf8), byteLength);
}

int SkUTF::CountUTF16(const uint16_t* utf16, size_t byteLength) {
    if (!utf16 || !is_align2(reinterpret_cast<uintptr_t>(utf16)) || !is_align2(byteLength)) {
        return -1;
    }
    return skia_rs_utf_count_utf16(utf16, byteLength >> 1);
}

int SkUTF::CountUTF32(const int32_t* utf32, size_t byteLength) {
    if (!is_align4(reinterpret_cast<uintptr_t>(utf32)) ||
        !is_align4(byteLength) ||
        !SkTFitsIn<int>(byteLength >> 2)) {
        return -1;
    }
    return skia_rs_utf_count_utf32(utf32, byteLength >> 2);
}

SkUnichar SkUTF::NextUTF8(const char** ptr, const char* end) {
    if (!ptr || !end) {
        return -1;
    }
    const char* p = *ptr;
    if (!p || p >= end) {
        *ptr = end;
        return -1;
    }
    size_t consumed = 0;
    int32_t cp = skia_rs_utf_next_utf8(
            reinterpret_cast<const uint8_t*>(p), static_cast<size_t>(end - p), &consumed);
    *ptr = p + consumed;
    return cp;
}

SkUnichar SkUTF::NextUTF8WithReplacement(const char** ptr, const char* end) {
    SkUnichar val = SkUTF::NextUTF8(ptr, end);
    return val < 0 ? 0xFFFD : val;
}

SkUnichar SkUTF::NextUTF16(const uint16_t** ptr, const uint16_t* end) {
    if (!ptr || !end) {
        return -1;
    }
    const uint16_t* p = *ptr;
    if (!p || p >= end || !is_align2(reinterpret_cast<uintptr_t>(p))) {
        *ptr = end;
        return -1;
    }
    size_t consumed = 0;
    int32_t cp = skia_rs_utf_next_utf16(p, static_cast<size_t>(end - p), &consumed);
    *ptr = p + consumed;
    return cp;
}

SkUnichar SkUTF::NextUTF32(const int32_t** ptr, const int32_t* end) {
    if (!ptr || !end) {
        return -1;
    }
    const int32_t* p = *ptr;
    if (!p || p >= end || !is_align4(reinterpret_cast<uintptr_t>(p))) {
        *ptr = end;
        return -1;
    }
    size_t consumed = 0;
    int32_t cp = skia_rs_utf_next_utf32(p, static_cast<size_t>(end - p), &consumed);
    *ptr = p + consumed;
    return cp;
}

size_t SkUTF::ToUTF8(SkUnichar uni, char utf8[SkUTF::kMaxBytesInUTF8Sequence]) {
    return skia_rs_utf_to_utf8(uni, reinterpret_cast<uint8_t*>(utf8));
}

size_t SkUTF::ToUTF16(SkUnichar uni, uint16_t utf16[2]) {
    return skia_rs_utf_to_utf16(uni, utf16);
}

int SkUTF::UTF8ToUTF16(uint16_t dst[], int dstCapacity, const char src[], size_t srcByteLength) {
    if (!dst) {
        dstCapacity = 0;
    }
    return skia_rs_utf_utf8_to_utf16(
            dst, dstCapacity, reinterpret_cast<const uint8_t*>(src), srcByteLength);
}

int SkUTF::UTF16ToUTF8(char dst[], int dstCapacity, const uint16_t src[], size_t srcLength) {
    if (!dst) {
        dstCapacity = 0;
    }
    return skia_rs_utf_utf16_to_utf8(
            reinterpret_cast<uint8_t*>(dst), dstCapacity, src, srcLength);
}
