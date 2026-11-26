#![forbid(unsafe_code)]

pub mod cubic_clipper;
pub mod edge_clipper;
pub mod geometry;
pub mod line_clipper;
pub mod m44;
pub mod matrix;
pub mod matrix22;
pub mod matrix_invert;
pub mod point;
pub mod point3;
pub mod quad_clipper;
pub mod rect;
pub mod rrect;
pub mod rsxform;
pub mod vert_state;
pub mod vertices;

pub use m44::{M44, V2, V3, V4};
pub use matrix::Matrix;
pub use point::{IPoint, ISize, Point, Size, Vector};
pub use point3::Point3;
pub use rect::{IRect, Rect};
pub use rrect::RRect;
pub use rsxform::RSXform;

pub const SK_SCALAR_NEARLY_ZERO: f32 = 1.0 / (1 << 12) as f32;
pub const SK_SCALAR_ROOT_2_OVER_2: f32 = 0.707106781;
pub const SK_SCALAR_PI: f32 = 3.14159265;
pub const W0_PLANE_DISTANCE: f32 = 1.0 / (1 << 14) as f32;

/// Matches C++ `std::min(a, b)`: returns `b` if `b < a`, else `a`.
#[inline]
pub fn cpp_min_f32(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// Matches C++ `std::max(a, b)`: returns `b` if `a < b`, else `a`.
#[inline]
pub fn cpp_max_f32(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// Matches C++ `sk_float_midpoint(a, b)`: `0.5f * a + 0.5f * b`.
#[inline]
pub fn float_midpoint(a: f32, b: f32) -> f32 {
    0.5_f32 * a + 0.5_f32 * b
}

/// Matches C++ `SkScalarNearlyZero(x, tolerance)`.
#[inline]
pub fn scalar_nearly_zero(x: f32, tolerance: f32) -> bool {
    x.abs() <= tolerance
}

/// Matches C++ `SkScalarNearlyEqual(x, y, tolerance)`.
#[inline]
pub fn scalar_nearly_equal(x: f32, y: f32, tolerance: f32) -> bool {
    (x - y).abs() <= tolerance
}

/// Matches C++ `sk_float_sgn(x)`.
#[inline]
pub fn float_sgn(x: f32) -> i32 {
    if x < 0.0 {
        -1
    } else if x > 0.0 {
        1
    } else {
        0
    }
}

/// Matches C++ `SkIsFinite(arr, count)`.
#[inline]
pub fn all_finite(vals: &[f32]) -> bool {
    let mut prod = 0.0_f32;
    for &v in vals {
        prod *= v;
    }
    prod == 0.0
}

/// Matches C++ `SkScalarCopySign(x, y)`.
#[inline]
pub fn copysign_f32(x: f32, y: f32) -> f32 {
    x.copysign(y)
}

/// Matches C++ `SkScalarInterp(A, B, t)`.
#[inline]
pub fn scalar_interp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Matches C++ `SkTPin(x, lo, hi)`.
#[inline]
pub fn pin_f32(x: f32, lo: f32, hi: f32) -> f32 {
    cpp_max_f32(lo, cpp_min_f32(x, hi))
}

#[inline]
pub fn pin_i32(x: i32, lo: i32, hi: i32) -> i32 {
    x.clamp(lo, hi)
}

#[inline]
pub fn float_saturate2int(mut x: f32) -> i32 {
    const MAX_S32_FITS_IN_FLOAT: f32 = 2_147_483_520.0;
    const MIN_S32_FITS_IN_FLOAT: f32 = -2_147_483_648.0;
    x = if x < MAX_S32_FITS_IN_FLOAT {
        x
    } else {
        MAX_S32_FITS_IN_FLOAT
    };
    x = if x > MIN_S32_FITS_IN_FLOAT {
        x
    } else {
        MIN_S32_FITS_IN_FLOAT
    };
    x as i32
}

#[inline]
pub fn scalar_round_to_int(x: f32) -> i32 {
    float_saturate2int((x + 0.5_f32).floor())
}
