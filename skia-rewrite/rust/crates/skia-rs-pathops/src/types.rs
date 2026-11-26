#![forbid(unsafe_code)]

//! Port of `SkPathOpsTypes.h/.cpp`, `SkPathOpsPoint.h`, `SkPathOpsRect.h/.cpp`,
//! and `SkPathOpsBounds.h`.

use skia_rs_geometry::{Point, Rect, Vector};
use skia_rs_path::types::Verb;

pub const SK_SCALAR_MAX: f32 = 3.402_823_5e38_f32;
pub const SK_DOUBLE_PI: f64 = 3.14159265358979323846264338327950288;

/// FLT_EPSILON == 1.19209290E-07 == 1 / (2 ^ 23)
pub const FLT_EPSILON_F32: f32 = f32::EPSILON;
pub const FLT_EPSILON: f64 = f32::EPSILON as f64;
/// DBL_EPSILON == 2.2204460492503131e-16
pub const DBL_EPSILON: f64 = f64::EPSILON;

pub const FLT_EPSILON_CUBED: f64 = FLT_EPSILON * FLT_EPSILON * FLT_EPSILON;
pub const FLT_EPSILON_HALF: f64 = FLT_EPSILON / 2.0;
pub const FLT_EPSILON_DOUBLE: f64 = FLT_EPSILON * 2.0;
pub const FLT_EPSILON_ORDERABLE_ERR: f64 = FLT_EPSILON * 16.0;
pub const FLT_EPSILON_SQUARED: f64 = FLT_EPSILON * FLT_EPSILON;
pub const FLT_EPSILON_SQRT: f64 = 0.00034526697709225118;
pub const FLT_EPSILON_INVERSE: f64 = 1.0 / FLT_EPSILON;
pub const DBL_EPSILON_ERR: f64 = DBL_EPSILON * 4.0;
pub const DBL_EPSILON_SUBDIVIDE_ERR: f64 = DBL_EPSILON * 16.0;
pub const ROUGH_EPSILON: f64 = FLT_EPSILON * 64.0;
pub const MORE_ROUGH_EPSILON: f64 = FLT_EPSILON * 256.0;
pub const WAY_ROUGH_EPSILON: f64 = FLT_EPSILON * 2048.0;
pub const BUMP_EPSILON: f64 = FLT_EPSILON * 4096.0;

pub const INVERSE_NUMBER_RANGE: f32 = FLT_EPSILON_ORDERABLE_ERR as f32;

/// Mirrors `enum SkPathOp` in `include/pathops/SkPathOps.h`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathOp {
    Difference = 0,
    Intersect = 1,
    Union = 2,
    Xor = 3,
    ReverseDifference = 4,
}

impl PathOp {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Difference),
            1 => Some(Self::Intersect),
            2 => Some(Self::Union),
            3 => Some(Self::Xor),
            4 => Some(Self::ReverseDifference),
            _ => None,
        }
    }

    #[inline]
    pub fn name(self) -> &'static str {
        match self {
            Self::Difference => "diff",
            Self::Intersect => "sect",
            Self::Union => "union",
            Self::Xor => "xor",
            Self::ReverseDifference => "rdiff",
        }
    }
}

/// Mirrors `enum SkPathOpsMask`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathOpsMask {
    Winding = -1,
    No = 0,
    EvenOdd = 1,
}

/// Matches C++ `std::min(a, b)` for `f64` (returns `b` if `b < a`, else `a`).
#[inline]
pub fn cpp_min_f64(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Matches C++ `std::max(a, b)` for `f64` (returns `b` if `a < b`, else `a`).
#[inline]
pub fn cpp_max_f64(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// Matches C++ `std::min(a, b)` for `f32`.
#[inline]
pub fn cpp_min_f32(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// Matches C++ `std::max(a, b)` for `f32`.
#[inline]
pub fn cpp_max_f32(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// Matches C++ `SkSignBitTo2sCompliment` / `SkFloatAs2sCompliment`.
#[inline]
pub fn float_as_2s_compliment(x: f32) -> i32 {
    let mut bits = x.to_bits() as i32;
    if bits < 0 {
        bits &= 0x7FFF_FFFF;
        bits = -bits;
    }
    bits
}

#[inline]
fn arguments_denormalized(a: f32, b: f32, epsilon: i32) -> bool {
    let denormalized_check = FLT_EPSILON_F32 * (epsilon as f32) / 2.0_f32;
    a.abs() <= denormalized_check && b.abs() <= denormalized_check
}

#[inline]
fn equal_ulps(a: f32, b: f32, epsilon: i32, depsilon: i32) -> bool {
    if arguments_denormalized(a, b, depsilon) {
        return true;
    }
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits < b_bits + eps && b_bits < a_bits + eps
}

#[inline]
fn equal_ulps_no_normal_check(a: f32, b: f32, epsilon: i32) -> bool {
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits < b_bits + eps && b_bits < a_bits + eps
}

#[inline]
fn equal_ulps_pin(a: f32, b: f32, epsilon: i32, depsilon: i32) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    if arguments_denormalized(a, b, depsilon) {
        return true;
    }
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits < b_bits + eps && b_bits < a_bits + eps
}

#[inline]
fn d_equal_ulps(a: f32, b: f32, epsilon: i32) -> bool {
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits < b_bits + eps && b_bits < a_bits + eps
}

#[inline]
fn not_equal_ulps(a: f32, b: f32, epsilon: i32) -> bool {
    if arguments_denormalized(a, b, epsilon) {
        return false;
    }
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits >= b_bits + eps || b_bits >= a_bits + eps
}

#[inline]
fn not_equal_ulps_pin(a: f32, b: f32, epsilon: i32) -> bool {
    if !a.is_finite() || !b.is_finite() {
        return false;
    }
    if arguments_denormalized(a, b, epsilon) {
        return false;
    }
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits >= b_bits + eps || b_bits >= a_bits + eps
}

#[inline]
fn d_not_equal_ulps(a: f32, b: f32, epsilon: i32) -> bool {
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits >= b_bits + eps || b_bits >= a_bits + eps
}

#[inline]
fn less_ulps(a: f32, b: f32, epsilon: i32) -> bool {
    if arguments_denormalized(a, b, epsilon) {
        return a <= b - FLT_EPSILON_F32 * (epsilon as f32);
    }
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits <= b_bits - eps
}

#[inline]
fn less_or_equal_ulps(a: f32, b: f32, epsilon: i32) -> bool {
    if arguments_denormalized(a, b, epsilon) {
        return a < b + FLT_EPSILON_F32 * (epsilon as f32);
    }
    let a_bits = float_as_2s_compliment(a) as i64;
    let b_bits = float_as_2s_compliment(b) as i64;
    let eps = epsilon as i64;
    a_bits < b_bits + eps
}

#[inline]
pub fn almost_bequal_ulps_f32(a: f32, b: f32) -> bool {
    equal_ulps(a, b, 2, 2)
}

#[inline]
pub fn almost_bequal_ulps(a: f64, b: f64) -> bool {
    almost_bequal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn almost_pequal_ulps_f32(a: f32, b: f32) -> bool {
    equal_ulps(a, b, 8, 8)
}

#[inline]
pub fn almost_pequal_ulps(a: f64, b: f64) -> bool {
    almost_pequal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn almost_dequal_ulps_f32(a: f32, b: f32) -> bool {
    d_equal_ulps(a, b, 16)
}

#[inline]
pub fn almost_dequal_ulps_f64(a: f64, b: f64) -> bool {
    if a.abs() < SK_SCALAR_MAX as f64 && b.abs() < SK_SCALAR_MAX as f64 {
        return almost_dequal_ulps_f32(a as f32, b as f32);
    }
    ((a - b).abs() / cpp_max_f64(a.abs(), b.abs())) < (FLT_EPSILON_F32 * 16.0_f32) as f64
}

#[inline]
pub fn almost_equal_ulps_f32(a: f32, b: f32) -> bool {
    equal_ulps(a, b, 16, 16)
}

#[inline]
pub fn almost_equal_ulps(a: f64, b: f64) -> bool {
    almost_equal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn almost_equal_ulps_pt(pt1: Point, pt2: Point) -> bool {
    almost_equal_ulps_f32(pt1.x, pt2.x) && almost_equal_ulps_f32(pt1.y, pt2.y)
}

#[inline]
pub fn almost_equal_ulps_no_normal_check_f32(a: f32, b: f32) -> bool {
    equal_ulps_no_normal_check(a, b, 16)
}

#[inline]
pub fn almost_equal_ulps_no_normal_check(a: f64, b: f64) -> bool {
    almost_equal_ulps_no_normal_check_f32(a as f32, b as f32)
}

#[inline]
pub fn almost_equal_ulps_pin_f32(a: f32, b: f32) -> bool {
    equal_ulps_pin(a, b, 16, 16)
}

#[inline]
pub fn almost_equal_ulps_pin(a: f64, b: f64) -> bool {
    almost_equal_ulps_pin_f32(a as f32, b as f32)
}

#[inline]
pub fn not_almost_equal_ulps_f32(a: f32, b: f32) -> bool {
    not_equal_ulps(a, b, 16)
}

#[inline]
pub fn not_almost_equal_ulps(a: f64, b: f64) -> bool {
    not_almost_equal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn not_almost_equal_ulps_pin_f32(a: f32, b: f32) -> bool {
    not_equal_ulps_pin(a, b, 16)
}

#[inline]
pub fn not_almost_equal_ulps_pin(a: f64, b: f64) -> bool {
    not_almost_equal_ulps_pin_f32(a as f32, b as f32)
}

#[inline]
pub fn not_almost_dequal_ulps_f32(a: f32, b: f32) -> bool {
    d_not_equal_ulps(a, b, 16)
}

#[inline]
pub fn not_almost_dequal_ulps(a: f64, b: f64) -> bool {
    not_almost_dequal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn roughly_equal_ulps_f32(a: f32, b: f32) -> bool {
    equal_ulps(a, b, 256, 1024)
}

#[inline]
pub fn roughly_equal_ulps(a: f64, b: f64) -> bool {
    roughly_equal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn almost_between_ulps_f32(a: f32, b: f32, c: f32) -> bool {
    const ULPS_EPSILON: i32 = 2;
    if a <= c {
        less_or_equal_ulps(a, b, ULPS_EPSILON) && less_or_equal_ulps(b, c, ULPS_EPSILON)
    } else {
        less_or_equal_ulps(b, a, ULPS_EPSILON) && less_or_equal_ulps(c, b, ULPS_EPSILON)
    }
}

#[inline]
pub fn almost_between_ulps(a: f64, b: f64, c: f64) -> bool {
    almost_between_ulps_f32(a as f32, b as f32, c as f32)
}

#[inline]
pub fn almost_less_ulps_f32(a: f32, b: f32) -> bool {
    less_ulps(a, b, 16)
}

#[inline]
pub fn almost_less_ulps(a: f64, b: f64) -> bool {
    almost_less_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn almost_less_or_equal_ulps_f32(a: f32, b: f32) -> bool {
    less_or_equal_ulps(a, b, 16)
}

#[inline]
pub fn almost_less_or_equal_ulps(a: f64, b: f64) -> bool {
    almost_less_or_equal_ulps_f32(a as f32, b as f32)
}

#[inline]
pub fn ulps_distance_f32(a: f32, b: f32) -> i32 {
    let float_int_a = a.to_bits() as i32;
    let float_int_b = b.to_bits() as i32;
    if (float_int_a < 0) != (float_int_b < 0) {
        return if a == b { 0 } else { i32::MAX };
    }
    float_int_a.wrapping_sub(float_int_b).wrapping_abs()
}

#[inline]
pub fn ulps_distance(a: f64, b: f64) -> i32 {
    ulps_distance_f32(a as f32, b as f32)
}

// ============================================================================
// Epsilon comparison helpers (from SkPathOpsTypes.h)
// ============================================================================

#[inline]
pub fn zero_or_one(x: f64) -> bool {
    x == 0.0 || x == 1.0
}

#[inline]
pub fn approximately_zero(x: f64) -> bool {
    x.abs() < FLT_EPSILON
}

#[inline]
pub fn approximately_zero_f32(x: f32) -> bool {
    (x.abs() as f64) < FLT_EPSILON
}

#[inline]
pub fn precisely_zero(x: f64) -> bool {
    x.abs() < DBL_EPSILON_ERR
}

#[inline]
pub fn precisely_subdivide_zero(x: f64) -> bool {
    x.abs() < DBL_EPSILON_SUBDIVIDE_ERR
}

#[inline]
pub fn approximately_zero_half(x: f64) -> bool {
    x.abs() < FLT_EPSILON_HALF
}

#[inline]
pub fn approximately_zero_double(x: f64) -> bool {
    x.abs() < FLT_EPSILON_DOUBLE
}

#[inline]
pub fn approximately_zero_orderable(x: f64) -> bool {
    x.abs() < FLT_EPSILON_ORDERABLE_ERR
}

#[inline]
pub fn approximately_zero_squared(x: f64) -> bool {
    x.abs() < FLT_EPSILON_SQUARED
}

#[inline]
pub fn approximately_zero_sqrt(x: f64) -> bool {
    x.abs() < FLT_EPSILON_SQRT
}

#[inline]
pub fn roughly_zero(x: f64) -> bool {
    x.abs() < ROUGH_EPSILON
}

#[inline]
pub fn approximately_zero_inverse(x: f64) -> bool {
    x.abs() > FLT_EPSILON_INVERSE
}

#[inline]
pub fn approximately_zero_when_compared_to(x: f64, y: f64) -> bool {
    x == 0.0 || x.abs() < (y * FLT_EPSILON).abs()
}

#[inline]
pub fn precisely_zero_when_compared_to(x: f64, y: f64) -> bool {
    x == 0.0 || x.abs() < (y * DBL_EPSILON).abs()
}

#[inline]
pub fn roughly_zero_when_compared_to(x: f64, y: f64) -> bool {
    x == 0.0 || x.abs() < (y * ROUGH_EPSILON).abs()
}

#[inline]
pub fn approximately_equal(x: f64, y: f64) -> bool {
    approximately_zero(x - y)
}

#[inline]
pub fn precisely_equal(x: f64, y: f64) -> bool {
    precisely_zero(x - y)
}

#[inline]
pub fn precisely_subdivide_equal(x: f64, y: f64) -> bool {
    precisely_subdivide_zero(x - y)
}

#[inline]
pub fn approximately_equal_half(x: f64, y: f64) -> bool {
    approximately_zero_half(x - y)
}

#[inline]
pub fn approximately_equal_double(x: f64, y: f64) -> bool {
    approximately_zero_double(x - y)
}

#[inline]
pub fn approximately_equal_orderable(x: f64, y: f64) -> bool {
    approximately_zero_orderable(x - y)
}

#[inline]
pub fn approximately_equal_squared(x: f64, y: f64) -> bool {
    approximately_equal(x, y)
}

#[inline]
pub fn approximately_greater(x: f64, y: f64) -> bool {
    x - FLT_EPSILON >= y
}

#[inline]
pub fn approximately_greater_double(x: f64, y: f64) -> bool {
    x - FLT_EPSILON_DOUBLE >= y
}

#[inline]
pub fn approximately_greater_orderable(x: f64, y: f64) -> bool {
    x - FLT_EPSILON_ORDERABLE_ERR >= y
}

#[inline]
pub fn approximately_greater_or_equal(x: f64, y: f64) -> bool {
    x + FLT_EPSILON > y
}

#[inline]
pub fn approximately_greater_or_equal_double(x: f64, y: f64) -> bool {
    x + FLT_EPSILON_DOUBLE > y
}

#[inline]
pub fn approximately_greater_or_equal_orderable(x: f64, y: f64) -> bool {
    x + FLT_EPSILON_ORDERABLE_ERR > y
}

#[inline]
pub fn approximately_lesser(x: f64, y: f64) -> bool {
    x + FLT_EPSILON <= y
}

#[inline]
pub fn approximately_lesser_double(x: f64, y: f64) -> bool {
    x + FLT_EPSILON_DOUBLE <= y
}

#[inline]
pub fn approximately_lesser_orderable(x: f64, y: f64) -> bool {
    x + FLT_EPSILON_ORDERABLE_ERR <= y
}

#[inline]
pub fn approximately_lesser_or_equal(x: f64, y: f64) -> bool {
    x - FLT_EPSILON < y
}

#[inline]
pub fn approximately_lesser_or_equal_double(x: f64, y: f64) -> bool {
    x - FLT_EPSILON_DOUBLE < y
}

#[inline]
pub fn approximately_lesser_or_equal_orderable(x: f64, y: f64) -> bool {
    x - FLT_EPSILON_ORDERABLE_ERR < y
}

#[inline]
pub fn approximately_greater_than_one(x: f64) -> bool {
    x > 1.0 - FLT_EPSILON
}

#[inline]
pub fn precisely_greater_than_one(x: f64) -> bool {
    x > 1.0 - DBL_EPSILON_ERR
}

#[inline]
pub fn approximately_less_than_zero(x: f64) -> bool {
    x < FLT_EPSILON
}

#[inline]
pub fn precisely_less_than_zero(x: f64) -> bool {
    x < DBL_EPSILON_ERR
}

#[inline]
pub fn approximately_negative(x: f64) -> bool {
    x < FLT_EPSILON
}

#[inline]
pub fn approximately_negative_orderable(x: f64) -> bool {
    x < FLT_EPSILON_ORDERABLE_ERR
}

#[inline]
pub fn precisely_negative(x: f64) -> bool {
    x < DBL_EPSILON_ERR
}

#[inline]
pub fn approximately_one_or_less(x: f64) -> bool {
    x < 1.0 + FLT_EPSILON
}

#[inline]
pub fn approximately_one_or_less_double(x: f64) -> bool {
    x < 1.0 + FLT_EPSILON_DOUBLE
}

#[inline]
pub fn approximately_positive(x: f64) -> bool {
    x > -FLT_EPSILON
}

#[inline]
pub fn approximately_positive_squared(x: f64) -> bool {
    x > -FLT_EPSILON_SQUARED
}

#[inline]
pub fn approximately_zero_or_more(x: f64) -> bool {
    x > -FLT_EPSILON
}

#[inline]
pub fn approximately_zero_or_more_double(x: f64) -> bool {
    x > -FLT_EPSILON_DOUBLE
}

#[inline]
pub fn approximately_between_orderable(a: f64, b: f64, c: f64) -> bool {
    if a <= c {
        approximately_negative_orderable(a - b) && approximately_negative_orderable(b - c)
    } else {
        approximately_negative_orderable(b - a) && approximately_negative_orderable(c - b)
    }
}

#[inline]
pub fn approximately_between(a: f64, b: f64, c: f64) -> bool {
    if a <= c {
        approximately_negative(a - b) && approximately_negative(b - c)
    } else {
        approximately_negative(b - a) && approximately_negative(c - b)
    }
}

#[inline]
pub fn precisely_between(a: f64, b: f64, c: f64) -> bool {
    if a <= c {
        precisely_negative(a - b) && precisely_negative(b - c)
    } else {
        precisely_negative(b - a) && precisely_negative(c - b)
    }
}

#[inline]
pub fn between(a: f64, b: f64, c: f64) -> bool {
    (a - b) * (c - b) <= 0.0
}

#[inline]
pub fn roughly_equal(x: f64, y: f64) -> bool {
    (x - y).abs() < ROUGH_EPSILON
}

#[inline]
pub fn roughly_negative(x: f64) -> bool {
    x < ROUGH_EPSILON
}

#[inline]
pub fn roughly_between(a: f64, b: f64, c: f64) -> bool {
    if a <= c {
        roughly_negative(a - b) && roughly_negative(b - c)
    } else {
        roughly_negative(b - a) && roughly_negative(c - b)
    }
}

#[inline]
pub fn more_roughly_equal(x: f64, y: f64) -> bool {
    (x - y).abs() < MORE_ROUGH_EPSILON
}

#[inline]
pub fn pathops_points_to_verb(points: usize) -> Verb {
    match points {
        0 => Verb::Move,
        1 => Verb::Line,
        2 => Verb::Quad,
        3 => Verb::Cubic,
        _ => Verb::Move,
    }
}

#[inline]
pub fn pathops_verb_to_points(verb: Verb) -> usize {
    match verb {
        Verb::Line => 1,
        Verb::Quad | Verb::Conic => 2,
        Verb::Cubic => 3,
        Verb::Move | Verb::Close => 0,
    }
}

#[inline]
pub fn d_interp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
pub fn d_sign(x: f64) -> i32 {
    (x > 0.0) as i32 - (x < 0.0) as i32
}

#[inline]
pub fn d_side(x: f64) -> i32 {
    (x > 0.0) as i32 + (x >= 0.0) as i32
}

#[inline]
pub fn d_side_bit(x: f64) -> i32 {
    1 << d_side(x)
}

#[inline]
pub fn pin_t(t: f64) -> f64 {
    if precisely_less_than_zero(t) {
        0.0
    } else if precisely_greater_than_one(t) {
        1.0
    } else {
        t
    }
}

// ============================================================================
// DVector & DPoint (exact #[repr(C)] layout matching SkDVector / SkDPoint)
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DVector {
    pub x: f64,
    pub y: f64,
}

impl DVector {
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn from_vector(v: Vector) -> Self {
        Self {
            x: v.x as f64,
            y: v.y as f64,
        }
    }

    #[inline]
    pub fn as_vector(self) -> Vector {
        Vector {
            x: self.x as f32,
            y: self.y as f32,
        }
    }

    #[inline]
    pub fn cross(self, a: Self) -> f64 {
        self.x * a.y - self.y * a.x
    }

    #[inline]
    pub fn cross_check(self, a: Self) -> f64 {
        let xy = self.x * a.y;
        let yx = self.y * a.x;
        if almost_equal_ulps(xy, yx) {
            0.0
        } else {
            xy - yx
        }
    }

    #[inline]
    pub fn cross_no_normal_check(self, a: Self) -> f64 {
        let xy = self.x * a.y;
        let yx = self.y * a.x;
        if almost_equal_ulps_no_normal_check(xy, yx) {
            0.0
        } else {
            xy - yx
        }
    }

    #[inline]
    pub fn dot(self, a: Self) -> f64 {
        self.x * a.x + self.y * a.y
    }

    #[inline]
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    #[inline]
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    #[inline]
    pub fn normalize(&mut self) -> &mut Self {
        let inv_len = 1.0 / self.length();
        self.x *= inv_len;
        self.y *= inv_len;
        self
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DPoint {
    pub x: f64,
    pub y: f64,
}

impl DPoint {
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn from_point(pt: Point) -> Self {
        Self {
            x: pt.x as f64,
            y: pt.y as f64,
        }
    }

    #[inline]
    pub fn as_point(self) -> Point {
        Point {
            x: self.x as f32,
            y: self.y as f32,
        }
    }

    #[inline]
    pub fn sub(self, b: Self) -> DVector {
        DVector {
            x: self.x - b.x,
            y: self.y - b.y,
        }
    }

    #[inline]
    pub fn add_vec(self, v: DVector) -> Self {
        Self {
            x: self.x + v.x,
            y: self.y + v.y,
        }
    }

    #[inline]
    pub fn sub_vec(self, v: DVector) -> Self {
        Self {
            x: self.x - v.x,
            y: self.y - v.y,
        }
    }

    #[inline]
    pub fn distance_squared(self, a: Self) -> f64 {
        self.sub(a).length_squared()
    }

    #[inline]
    pub fn distance(self, a: Self) -> f64 {
        self.sub(a).length()
    }

    #[inline]
    pub fn mid(a: Self, b: Self) -> Self {
        Self {
            x: (a.x + b.x) / 2.0,
            y: (a.y + b.y) / 2.0,
        }
    }

    #[inline]
    pub fn approximately_zero(self) -> bool {
        approximately_zero(self.x) && approximately_zero(self.y)
    }

    #[inline]
    pub fn approximately_dequal(self, a: Self) -> bool {
        if approximately_equal(self.x, a.x) && approximately_equal(self.y, a.y) {
            return true;
        }
        if !roughly_equal_ulps(self.x, a.x) || !roughly_equal_ulps(self.y, a.y) {
            return false;
        }
        let dist = self.distance(a);
        let tiniest = cpp_min_f64(cpp_min_f64(cpp_min_f64(self.x, a.x), self.y), a.y);
        let mut largest = cpp_max_f64(cpp_max_f64(cpp_max_f64(self.x, a.x), self.y), a.y);
        largest = cpp_max_f64(largest, -tiniest);
        almost_dequal_ulps_f64(largest, largest + dist)
    }

    #[inline]
    pub fn approximately_dequal_pt(self, a: Point) -> bool {
        self.approximately_dequal(Self::from_point(a))
    }

    #[inline]
    pub fn approximately_equal(self, a: Self) -> bool {
        if approximately_equal(self.x, a.x) && approximately_equal(self.y, a.y) {
            return true;
        }
        if !roughly_equal_ulps(self.x, a.x) || !roughly_equal_ulps(self.y, a.y) {
            return false;
        }
        let dist = self.distance(a);
        let tiniest = cpp_min_f64(cpp_min_f64(cpp_min_f64(self.x, a.x), self.y), a.y);
        let mut largest = cpp_max_f64(cpp_max_f64(cpp_max_f64(self.x, a.x), self.y), a.y);
        largest = cpp_max_f64(largest, -tiniest);
        almost_pequal_ulps(largest, largest + dist)
    }

    #[inline]
    pub fn approximately_equal_pt(self, a: Point) -> bool {
        self.approximately_equal(Self::from_point(a))
    }

    #[inline]
    pub fn approximately_equal_pts(a: Point, b: Point) -> bool {
        if approximately_equal(a.x as f64, b.x as f64)
            && approximately_equal(a.y as f64, b.y as f64)
        {
            return true;
        }
        if !roughly_equal_ulps_f32(a.x, b.x) || !roughly_equal_ulps_f32(a.y, b.y) {
            return false;
        }
        let d_a = Self::from_point(a);
        let d_b = Self::from_point(b);
        let dist = d_a.distance(d_b);
        let tiniest = cpp_min_f32(cpp_min_f32(cpp_min_f32(a.x, b.x), a.y), b.y);
        let mut largest = cpp_max_f32(cpp_max_f32(cpp_max_f32(a.x, b.x), a.y), b.y);
        largest = cpp_max_f32(largest, -tiniest);
        almost_dequal_ulps_f64(largest as f64, largest as f64 + dist)
    }

    #[inline]
    pub fn roughly_equal(self, a: Self) -> bool {
        if roughly_equal(self.x, a.x) && roughly_equal(self.y, a.y) {
            return true;
        }
        let dist = self.distance(a);
        let tiniest = cpp_min_f64(cpp_min_f64(cpp_min_f64(self.x, a.x), self.y), a.y);
        let mut largest = cpp_max_f64(cpp_max_f64(cpp_max_f64(self.x, a.x), self.y), a.y);
        largest = cpp_max_f64(largest, -tiniest);
        roughly_equal_ulps(largest, largest + dist)
    }

    #[inline]
    pub fn roughly_equal_pts(a: Point, b: Point) -> bool {
        if !roughly_equal_ulps_f32(a.x, b.x) && !roughly_equal_ulps_f32(a.y, b.y) {
            return false;
        }
        let d_a = Self::from_point(a);
        let d_b = Self::from_point(b);
        let dist = d_a.distance(d_b);
        let tiniest = cpp_min_f32(cpp_min_f32(cpp_min_f32(a.x, b.x), a.y), b.y);
        let mut largest = cpp_max_f32(cpp_max_f32(cpp_max_f32(a.x, b.x), a.y), b.y);
        largest = cpp_max_f32(largest, -tiniest);
        roughly_equal_ulps(largest as f64, largest as f64 + dist)
    }

    #[inline]
    pub fn way_roughly_equal_pts(a: Point, b: Point) -> bool {
        let largest_number = cpp_max_f32(
            a.x.abs(),
            cpp_max_f32(a.y.abs(), cpp_max_f32(b.x.abs(), b.y.abs())),
        );
        let diff_x = a.x - b.x;
        let diff_y = a.y - b.y;
        let largest_diff = cpp_max_f32(diff_x.abs(), diff_y.abs());
        roughly_zero_when_compared_to(largest_diff as f64, largest_number as f64)
    }
}

// ============================================================================
// DRect & PathOpsBounds
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DRect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl DRect {
    #[inline]
    pub fn set_pt(&mut self, pt: DPoint) {
        self.left = pt.x;
        self.right = pt.x;
        self.top = pt.y;
        self.bottom = pt.y;
    }

    #[inline]
    pub fn add(&mut self, pt: DPoint) {
        self.left = cpp_min_f64(self.left, pt.x);
        self.top = cpp_min_f64(self.top, pt.y);
        self.right = cpp_max_f64(self.right, pt.x);
        self.bottom = cpp_max_f64(self.bottom, pt.y);
    }

    #[inline]
    pub fn contains(&self, pt: DPoint) -> bool {
        approximately_between(self.left, pt.x, self.right)
            && approximately_between(self.top, pt.y, self.bottom)
    }

    #[inline]
    pub fn intersects(&self, r: &DRect) -> bool {
        r.left <= self.right && self.left <= r.right && r.top <= self.bottom && self.top <= r.bottom
    }

    #[inline]
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    #[inline]
    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }

    #[inline]
    pub fn valid(&self) -> bool {
        self.left <= self.right && self.top <= self.bottom
    }
}

/// Mirrors `struct SkPathOpsBounds : public SkRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PathOpsBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl PathOpsBounds {
    #[inline]
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[inline]
    pub fn set_empty(&mut self) {
        self.left = 0.0;
        self.top = 0.0;
        self.right = 0.0;
        self.bottom = 0.0;
    }

    #[inline]
    pub fn set_ltrb(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.left = left;
        self.top = top;
        self.right = right;
        self.bottom = bottom;
    }

    #[inline]
    pub fn set_point_bounds(&mut self, pt1: Point, pt2: Point) {
        self.left = cpp_min_f32(pt1.x, pt2.x);
        self.top = cpp_min_f32(pt1.y, pt2.y);
        self.right = cpp_max_f32(pt1.x, pt2.x);
        self.bottom = cpp_max_f32(pt1.y, pt2.y);
    }

    #[inline]
    pub fn as_rect(self) -> Rect {
        Rect {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }

    #[inline]
    pub fn from_rect(r: Rect) -> Self {
        Self {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }

    #[inline]
    pub fn intersects(a: &Self, b: &Self) -> bool {
        almost_less_or_equal_ulps_f32(a.left, b.right)
            && almost_less_or_equal_ulps_f32(b.left, a.right)
            && almost_less_or_equal_ulps_f32(a.top, b.bottom)
            && almost_less_or_equal_ulps_f32(b.top, a.bottom)
    }

    #[inline]
    pub fn add_ltrb(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        if left < self.left {
            self.left = left;
        }
        if top < self.top {
            self.top = top;
        }
        if right > self.right {
            self.right = right;
        }
        if bottom > self.bottom {
            self.bottom = bottom;
        }
    }

    #[inline]
    pub fn add_bounds(&mut self, to_add: &Self) {
        self.add_ltrb(to_add.left, to_add.top, to_add.right, to_add.bottom);
    }

    #[inline]
    pub fn add_pt(&mut self, pt: Point) {
        if pt.x < self.left {
            self.left = pt.x;
        }
        if pt.y < self.top {
            self.top = pt.y;
        }
        if pt.x > self.right {
            self.right = pt.x;
        }
        if pt.y > self.bottom {
            self.bottom = pt.y;
        }
    }

    #[inline]
    pub fn add_dpt(&mut self, pt: DPoint) {
        let x = pt.x as f32;
        let y = pt.y as f32;
        if x < self.left {
            self.left = x;
        }
        if y < self.top {
            self.top = y;
        }
        if x > self.right {
            self.right = x;
        }
        if y > self.bottom {
            self.bottom = y;
        }
    }

    #[inline]
    pub fn almost_contains(&self, pt: Point) -> bool {
        almost_less_or_equal_ulps_f32(self.left, pt.x)
            && almost_less_or_equal_ulps_f32(pt.x, self.right)
            && almost_less_or_equal_ulps_f32(self.top, pt.y)
            && almost_less_or_equal_ulps_f32(pt.y, self.bottom)
    }

    #[inline]
    pub fn contains(&self, pt: Point) -> bool {
        self.left <= pt.x && self.top <= pt.y && self.right >= pt.x && self.bottom >= pt.y
    }

    #[inline]
    pub fn is_finite(&self) -> bool {
        self.left.is_finite()
            && self.top.is_finite()
            && self.right.is_finite()
            && self.bottom.is_finite()
    }
}
