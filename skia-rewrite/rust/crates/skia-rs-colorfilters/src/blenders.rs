//! Safe Rust port of `src/effects/SkBlenders.cpp`.

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_geometry::{scalar_nearly_equal, SK_SCALAR_NEARLY_ZERO};

/// Result of classifying `SkBlenders::Arithmetic(k1, k2, k3, k4, enforcePremul)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ArithmeticPlan {
    /// Non-finite coefficient(s): return `nullptr`.
    Invalid,
    /// Nearly equivalent to a standard `SkBlendMode`.
    BlendMode(BlendMode),
    /// Requires the `kArithmetic` runtime blender with `[k1, k2, k3, k4, pm_clamp]`.
    RuntimeUniforms([f32; 5]),
}

/// FFI-friendly flat representation of [`ArithmeticPlan`].
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ArithmeticResult {
    /// 0 = Invalid (null), 1 = BlendMode (`blend_mode` valid), 2 = RuntimeUniforms (`uniforms` valid)
    pub kind: i32,
    pub blend_mode: i32,
    pub uniforms: [f32; 5],
}

/// Exact port of `SkBlenders::Arithmetic` validation, blend-mode folding, and uniform packing.
pub fn classify_arithmetic(
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
    enforce_premul: bool,
) -> ArithmeticPlan {
    if !(k1.is_finite() && k2.is_finite() && k3.is_finite() && k4.is_finite()) {
        return ArithmeticPlan::Invalid;
    }

    const TABLE: [(f32, f32, f32, f32, BlendMode); 3] = [
        (0.0, 1.0, 0.0, 0.0, BlendMode::Src),
        (0.0, 0.0, 1.0, 0.0, BlendMode::Dst),
        (0.0, 0.0, 0.0, 0.0, BlendMode::Clear),
    ];

    for &(t1, t2, t3, t4, mode) in &TABLE {
        if scalar_nearly_equal(k1, t1, SK_SCALAR_NEARLY_ZERO)
            && scalar_nearly_equal(k2, t2, SK_SCALAR_NEARLY_ZERO)
            && scalar_nearly_equal(k3, t3, SK_SCALAR_NEARLY_ZERO)
            && scalar_nearly_equal(k4, t4, SK_SCALAR_NEARLY_ZERO)
        {
            return ArithmeticPlan::BlendMode(mode);
        }
    }

    ArithmeticPlan::RuntimeUniforms([
        k1,
        k2,
        k3,
        k4,
        if enforce_premul { 0.0_f32 } else { 1.0_f32 },
    ])
}

/// Convert [`ArithmeticPlan`] to FFI [`ArithmeticResult`].
pub fn classify_arithmetic_ffi(
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
    enforce_premul: bool,
) -> ArithmeticResult {
    match classify_arithmetic(k1, k2, k3, k4, enforce_premul) {
        ArithmeticPlan::Invalid => ArithmeticResult {
            kind: 0,
            blend_mode: 0,
            uniforms: [0.0; 5],
        },
        ArithmeticPlan::BlendMode(mode) => ArithmeticResult {
            kind: 1,
            blend_mode: mode as i32,
            uniforms: [0.0; 5],
        },
        ArithmeticPlan::RuntimeUniforms(uniforms) => ArithmeticResult {
            kind: 2,
            blend_mode: 0,
            uniforms,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_rejected() {
        assert_eq!(
            classify_arithmetic(f32::NAN, 0.0, 0.0, 0.0, true),
            ArithmeticPlan::Invalid
        );
        assert_eq!(
            classify_arithmetic(0.0, f32::INFINITY, 0.0, 0.0, false),
            ArithmeticPlan::Invalid
        );
        assert_eq!(
            classify_arithmetic(0.0, 0.0, f32::NEG_INFINITY, 0.0, true),
            ArithmeticPlan::Invalid
        );
        assert_eq!(
            classify_arithmetic(0.0, 0.0, 0.0, f32::NAN, false),
            ArithmeticPlan::Invalid
        );
    }

    #[test]
    fn folds_to_blend_modes() {
        assert_eq!(
            classify_arithmetic(0.0, 1.0, 0.0, 0.0, true),
            ArithmeticPlan::BlendMode(BlendMode::Src)
        );
        assert_eq!(
            classify_arithmetic(0.0, 0.0, 1.0, 0.0, false),
            ArithmeticPlan::BlendMode(BlendMode::Dst)
        );
        assert_eq!(
            classify_arithmetic(0.0, 0.0, 0.0, 0.0, true),
            ArithmeticPlan::BlendMode(BlendMode::Clear)
        );
        // Nearly equal within SK_SCALAR_NEARLY_ZERO
        assert_eq!(
            classify_arithmetic(1e-5, 1.0 - 1e-5, -1e-5, 1e-5, true),
            ArithmeticPlan::BlendMode(BlendMode::Src)
        );
    }

    #[test]
    fn runtime_uniforms_and_enforce_premul_flag() {
        assert_eq!(
            classify_arithmetic(0.5, 0.25, 0.75, -0.1, true),
            ArithmeticPlan::RuntimeUniforms([0.5, 0.25, 0.75, -0.1, 0.0])
        );
        assert_eq!(
            classify_arithmetic(0.5, 0.25, 0.75, -0.1, false),
            ArithmeticPlan::RuntimeUniforms([0.5, 0.25, 0.75, -0.1, 1.0])
        );
    }
}
