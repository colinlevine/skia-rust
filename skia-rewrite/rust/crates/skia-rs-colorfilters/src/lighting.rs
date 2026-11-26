//! Safe Rust port of `src/effects/SkColorMatrixFilter.cpp` (`SkColorFilters::Lighting`).

use skia_rs_color::color::{color_get_b, color_get_g, color_get_r, Color};

use crate::color_matrix::ColorMatrix;

/// Exact constant `SK_ColorBLACK = 0xFF000000`.
const OPAQUE_ALPHA_MASK: Color = 0xFF00_0000;

/// Exact port of `byte_to_unit_float(U8CPU byte)` from `SkColorMatrixFilter.cpp`.
#[inline]
pub fn byte_to_unit_float(byte: u32) -> f32 {
    if byte == 0xFF {
        // want to get this exact
        1.0_f32
    } else {
        (byte as f32) * 0.00392156862745_f32
    }
}

/// Result of planning `SkColorFilters::Lighting(mul, add)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LightingPlan {
    /// `add` RGB is zero: use `SkColorFilters::Blend(mul | SK_ColorBLACK, SkBlendMode::kModulate)`.
    BlendModulate(Color),
    /// Non-zero `add` RGB: use `SkColorFilters::Matrix(matrix)`.
    Matrix(ColorMatrix),
}

/// FFI-friendly flat representation of [`LightingPlan`].
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LightingResult {
    /// `true` => use `Blend(blend_color, Modulate)`; `false` => use `Matrix(matrix)`.
    pub is_blend: bool,
    pub blend_color: u32,
    pub matrix: ColorMatrix,
}

/// Exact port of `SkColorFilters::Lighting(SkColor mul, SkColor add)`.
pub fn classify_lighting(mul: Color, add: Color) -> LightingPlan {
    // Omit the alpha and compare only the RGB values.
    if (add & !OPAQUE_ALPHA_MASK) == 0 {
        return LightingPlan::BlendModulate(mul | OPAQUE_ALPHA_MASK);
    }

    let mut matrix = ColorMatrix::identity();
    matrix.set_scale(
        byte_to_unit_float(color_get_r(mul)),
        byte_to_unit_float(color_get_g(mul)),
        byte_to_unit_float(color_get_b(mul)),
        1.0_f32,
    );
    matrix.post_translate(
        byte_to_unit_float(color_get_r(add)),
        byte_to_unit_float(color_get_g(add)),
        byte_to_unit_float(color_get_b(add)),
        0.0_f32,
    );
    LightingPlan::Matrix(matrix)
}

/// Convert [`LightingPlan`] to FFI [`LightingResult`].
pub fn classify_lighting_ffi(mul: Color, add: Color) -> LightingResult {
    match classify_lighting(mul, add) {
        LightingPlan::BlendModulate(c) => LightingResult {
            is_blend: true,
            blend_color: c,
            matrix: ColorMatrix::identity(),
        },
        LightingPlan::Matrix(matrix) => LightingResult {
            is_blend: false,
            blend_color: 0,
            matrix,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_color::color::color_set_argb;

    #[test]
    fn byte_to_unit_float_endpoints() {
        assert_eq!(byte_to_unit_float(0), 0.0_f32);
        assert_eq!(byte_to_unit_float(0xFF), 1.0_f32);
        assert!((byte_to_unit_float(128) - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn zero_add_rgb_folds_to_modulate_blend() {
        // Alpha of `add` is ignored, alpha of `mul` is forced to 0xFF.
        let mul = color_set_argb(0x12, 0x80, 0x40, 0x20);
        let add = color_set_argb(0xAB, 0x00, 0x00, 0x00);
        assert_eq!(
            classify_lighting(mul, add),
            LightingPlan::BlendModulate(color_set_argb(0xFF, 0x80, 0x40, 0x20))
        );
    }

    #[test]
    fn nonzero_add_rgb_builds_scale_translate_matrix() {
        let mul = color_set_argb(0x00, 0xFF, 0x80, 0x00);
        let add = color_set_argb(0xFF, 0x00, 0x40, 0xFF);
        match classify_lighting(mul, add) {
            LightingPlan::Matrix(m) => {
                assert_eq!(m.mat[0], 1.0_f32);
                assert!((m.mat[6] - byte_to_unit_float(0x80)).abs() < 1e-7);
                assert_eq!(m.mat[12], 0.0_f32);
                assert_eq!(m.mat[18], 1.0_f32);
                assert_eq!(m.mat[4], 0.0_f32);
                assert!((m.mat[9] - byte_to_unit_float(0x40)).abs() < 1e-7);
                assert_eq!(m.mat[14], 1.0_f32);
                assert_eq!(m.mat[19], 0.0_f32);
            }
            _ => panic!("expected Matrix plan"),
        }
    }
}
