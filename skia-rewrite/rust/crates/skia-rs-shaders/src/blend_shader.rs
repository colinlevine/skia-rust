//! Safe Rust port of `SkBlendShader.cpp`.

use skia_rs_color::blend_mode::{self, BlendMode, BlendModeCoeff};

/// Action returned by `classify_blend_shader`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendShaderAction {
    /// Return `nullptr` (either `src` or `dst` is null).
    ReturnNull = 0,
    /// Return `SkShaders::Color(0)` (`kClear`).
    ReturnClear = 1,
    /// Return `dst` (`kDst`).
    ReturnDst = 2,
    /// Return `src` (`kSrc`).
    ReturnSrc = 3,
    /// Create an `SkBlendShader` with the mode.
    CreateBlend = 4,
}

/// Matches `SkShaders::Blend(SkBlendMode, dst, src)` simplification rules.
#[inline]
pub fn classify_blend_shader(mode: i32, has_dst: bool, has_src: bool) -> BlendShaderAction {
    if !has_src || !has_dst {
        return BlendShaderAction::ReturnNull;
    }
    match BlendMode::from_i32(mode) {
        Some(BlendMode::Clear) => BlendShaderAction::ReturnClear,
        Some(BlendMode::Dst) => BlendShaderAction::ReturnDst,
        Some(BlendMode::Src) => BlendShaderAction::ReturnSrc,
        _ => BlendShaderAction::CreateBlend,
    }
}

/// Matches `SkBlendShader::isOpaque()`.
pub fn blend_shader_is_opaque(mode: BlendMode, src_is_opaque: bool, dst_is_opaque: bool) -> bool {
    let (src_coeff, dst_coeff) = match blend_mode::as_coeff(mode) {
        Some(pair) => pair,
        None => return false, // pessimistic
    };
    let coeff_is_opaque = |coeff: BlendModeCoeff| -> bool {
        let src_alpha = matches!(coeff, BlendModeCoeff::SA | BlendModeCoeff::SC);
        let dst_alpha = matches!(coeff, BlendModeCoeff::DA | BlendModeCoeff::DC);
        coeff == BlendModeCoeff::One
            || (src_alpha && src_is_opaque)
            || (dst_alpha && dst_is_opaque)
    };
    (src_is_opaque && coeff_is_opaque(src_coeff)) || (dst_is_opaque && coeff_is_opaque(dst_coeff))
}

/// Matches `SkBlendShader::CreateProc` mode validation (`mode <= kLastMode`).
#[inline]
pub const fn is_valid_blend_mode(mode: u32) -> bool {
    mode <= (BlendMode::Luminosity as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_classify() {
        assert_eq!(
            classify_blend_shader(BlendMode::SrcOver as i32, false, true),
            BlendShaderAction::ReturnNull
        );
        assert_eq!(
            classify_blend_shader(BlendMode::Clear as i32, true, true),
            BlendShaderAction::ReturnClear
        );
        assert_eq!(
            classify_blend_shader(BlendMode::Dst as i32, true, true),
            BlendShaderAction::ReturnDst
        );
        assert_eq!(
            classify_blend_shader(BlendMode::Src as i32, true, true),
            BlendShaderAction::ReturnSrc
        );
        assert_eq!(
            classify_blend_shader(BlendMode::SrcOver as i32, true, true),
            BlendShaderAction::CreateBlend
        );
    }

    #[test]
    fn blend_opacity() {
        // SrcOver: (One, ISA) -> opaque if src is opaque (since coeff One)
        assert!(blend_shader_is_opaque(BlendMode::SrcOver, true, false));
        assert!(!blend_shader_is_opaque(BlendMode::SrcOver, false, true));
        // Plus: (One, One) -> opaque if either src or dst is opaque
        assert!(blend_shader_is_opaque(BlendMode::Plus, true, false));
        assert!(blend_shader_is_opaque(BlendMode::Plus, false, true));
        assert!(!blend_shader_is_opaque(BlendMode::Plus, false, false));
        // SrcIn: (DA, Zero) -> opaque only if both src and dst are opaque
        assert!(blend_shader_is_opaque(BlendMode::SrcIn, true, true));
        assert!(!blend_shader_is_opaque(BlendMode::SrcIn, true, false));
        assert!(!blend_shader_is_opaque(BlendMode::SrcIn, false, true));
        // Multiply (non-coeff): pessimistic false
        assert!(!blend_shader_is_opaque(BlendMode::Multiply, true, true));
    }
}
