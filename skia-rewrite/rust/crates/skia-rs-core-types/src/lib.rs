#![forbid(unsafe_code)]

//! Safe Rust implementations of Layer 2.4 core types, streams, buffers, strings,
//! flattenables, paint helpers, descriptors, masks, blur masks, Gaussian filters,
//! and mask gamma LUTs.

pub mod blur_mask;
pub mod buffer;
pub mod data;
pub mod descriptor;
pub mod flattenable;
pub mod gauss_filter;
pub mod mask;
pub mod mask_gamma;
pub mod paint;
pub mod stream;
pub mod string;

/// Align `x` up to a multiple of 4 (wrapping arithmetic matching Skia's `SkAlign4`).
#[inline]
pub const fn align4(x: usize) -> usize {
    x.wrapping_add(3) & !3usize
}

/// Returns true iff `x` is a multiple of 4 (matching `SkIsAlign4`).
#[inline]
pub const fn is_align4(x: usize) -> bool {
    (x & 3) == 0
}

/// Matching Skia's `SkAlpha255To256(alpha)`: `alpha + 1`.
#[inline]
pub const fn alpha_255_to_256(alpha: u8) -> u32 {
    (alpha as u32) + 1
}

/// Matching Skia's `SkAlphaMul(value, alpha256)`: `(value * alpha256) >> 8`.
#[inline]
pub const fn alpha_mul(value: u8, alpha256: u32) -> u8 {
    (((value as u32) * alpha256) >> 8) as u8
}

/// Matching Skia's `SkMulDiv255Round(a, b)`:
/// `unsigned prod = a * b + 128; return (prod + (prod >> 8)) >> 8;`
#[inline]
pub const fn mul_div_255_round(a: u8, b: u8) -> u8 {
    let prod = (a as u32) * (b as u32) + 128;
    ((prod + (prod >> 8)) >> 8) as u8
}

/// Matching Skia's `SkScalarCeilToInt(x)` -> `sk_float_ceil2int`.
#[inline]
pub fn scalar_ceil_to_int(x: f32) -> i32 {
    skia_rs_geometry::float_saturate2int(x.ceil())
}

/// Matching Skia's `SkScalarFloorToInt(x)` -> `sk_float_floor2int`.
#[inline]
pub fn scalar_floor_to_int(x: f32) -> i32 {
    skia_rs_geometry::float_saturate2int(x.floor())
}

/// Matching Skia's `SkScalarRoundToInt(x)` / `sk_float_round2int`.
#[inline]
pub fn scalar_round_to_int(x: f32) -> i32 {
    skia_rs_geometry::scalar_round_to_int(x)
}
