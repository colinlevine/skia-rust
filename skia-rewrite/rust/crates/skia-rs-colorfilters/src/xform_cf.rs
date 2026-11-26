//! Safe Rust port of `src/effects/colorfilters/SkColorSpaceXformColorFilter.cpp`.

use skia_rs_color::color::Color4f;
use skia_rs_color::color_space_xform_steps::ColorSpaceXformSteps;

use crate::color_filter::{premul_color4f, unpremul_color4f};

/// Legacy `SkSRGBGammaColorFilter` direction tag (`LegacyGammaOnlyCreateProc`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LegacyGammaDirection {
    LinearToSRGB = 0,
    SRGBToLinear = 1,
}

/// Validate and decode the legacy `SkSRGBGammaColorFilter` direction word.
#[inline]
pub const fn decode_legacy_gamma_dir(dir: u32) -> Option<LegacyGammaDirection> {
    match dir {
        0 => Some(LegacyGammaDirection::LinearToSRGB),
        1 => Some(LegacyGammaDirection::SRGBToLinear),
        _ => None,
    }
}

/// Whether `SkColorSpaceXformColorFilter::appendStages` must wrap `fSteps` with
/// `unpremul` / `premul` stages.
#[inline]
pub const fn needs_unpremul_premul(shader_is_opaque: bool) -> bool {
    !shader_is_opaque
}

/// `SkColorSpaceXformColorFilter::onIsAlphaUnchanged()` is always `true`.
#[inline]
pub const fn is_alpha_unchanged() -> bool {
    true
}

/// Stage plan for `SkColorSpaceXformColorFilter::appendStages`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct XformStagePlan {
    pub unpremul: bool,
    pub premul: bool,
}

/// Plan unpremul/premul wrapping for `SkColorSpaceXformColorFilter::appendStages`.
#[inline]
pub const fn plan_xform_stages(shader_is_opaque: bool) -> XformStagePlan {
    let wrap = needs_unpremul_premul(shader_is_opaque);
    XformStagePlan {
        unpremul: wrap,
        premul: wrap,
    }
}

/// Apply the linear portion of `ColorSpaceXformSteps` (configured unpremul -> unpremul)
/// to a premultiplied `Color4f`, wrapping with unpremul/premul when `!shader_is_opaque`.
pub fn apply_xform_mid_to_pm_color(
    steps: &ColorSpaceXformSteps,
    pm: Color4f,
    shader_is_opaque: bool,
) -> Color4f {
    let plan = plan_xform_stages(shader_is_opaque);
    let c = if plan.unpremul {
        unpremul_color4f(pm)
    } else {
        pm
    };
    let mut rgba = [c.r, c.g, c.b, c.a];
    steps.apply_unpremul(&mut rgba);
    steps.apply_mid(&mut rgba);
    steps.apply_premul(&mut rgba);
    let out = Color4f::new(rgba[0], rgba[1], rgba[2], rgba[3]);
    if plan.premul {
        premul_color4f(out)
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_gamma_dir_decoding() {
        assert_eq!(
            decode_legacy_gamma_dir(0),
            Some(LegacyGammaDirection::LinearToSRGB)
        );
        assert_eq!(
            decode_legacy_gamma_dir(1),
            Some(LegacyGammaDirection::SRGBToLinear)
        );
        assert_eq!(decode_legacy_gamma_dir(2), None);
        assert_eq!(decode_legacy_gamma_dir(u32::MAX), None);
    }

    #[test]
    fn stage_wrapping_and_identity_xform() {
        assert!(needs_unpremul_premul(false));
        assert!(!needs_unpremul_premul(true));
        assert!(is_alpha_unchanged());

        let identity = ColorSpaceXformSteps::default();
        let pm = Color4f::new(0.25, 0.125, 0.5, 0.5);
        let out = apply_xform_mid_to_pm_color(&identity, pm, false);
        assert!((out.r - pm.r).abs() < 1e-6);
        assert!((out.g - pm.g).abs() < 1e-6);
        assert!((out.b - pm.b).abs() < 1e-6);
        assert_eq!(out.a, pm.a);
    }
}
