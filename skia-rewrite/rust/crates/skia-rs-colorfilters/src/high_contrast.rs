//! Safe Rust port of `src/effects/SkHighContrastFilter.cpp`.

use skia_rs_geometry::pin_f32;

/// Matches `SkHighContrastConfig::InvertStyle`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum InvertStyle {
    #[default]
    NoInvert = 0,
    InvertBrightness = 1,
    InvertLightness = 2,
}

impl InvertStyle {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::NoInvert),
            1 => Some(Self::InvertBrightness),
            2 => Some(Self::InvertLightness),
            _ => None,
        }
    }
}

/// Exact `#[repr(C)]` match for `SkHighContrastConfig`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HighContrastConfig {
    pub grayscale: bool,
    pub invert_style: i32,
    pub contrast: f32,
}

impl Default for HighContrastConfig {
    fn default() -> Self {
        Self {
            grayscale: false,
            invert_style: InvertStyle::NoInvert as i32,
            contrast: 0.0_f32,
        }
    }
}

impl HighContrastConfig {
    /// Exact port of `SkHighContrastConfig::isValid()`.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.invert_style >= InvertStyle::NoInvert as i32
            && self.invert_style <= InvertStyle::InvertLightness as i32
            && self.contrast >= -1.0_f32
            && self.contrast <= 1.0_f32
    }
}

/// Uniform block passed to the `kHighContrast` known runtime effect.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct HighContrastUniforms {
    pub grayscale: f32,
    pub invert_style: f32,
    pub contrast: f32,
}

/// Exact port of `SkHighContrastFilter::Make` validation and uniform calculation.
pub fn compute_high_contrast_uniforms(
    config: &HighContrastConfig,
) -> Option<HighContrastUniforms> {
    if !config.is_valid() {
        return None;
    }

    // Pull contrast in from exact +/-1 by FLT_EPSILON to avoid dividing by zero in (1+c)/(1-c).
    let c = pin_f32(
        config.contrast,
        -1.0_f32 + f32::EPSILON,
        1.0_f32 - f32::EPSILON,
    );

    Some(HighContrastUniforms {
        grayscale: if config.grayscale { 1.0_f32 } else { 0.0_f32 },
        invert_style: config.invert_style as f32,
        contrast: (1.0_f32 + c) / (1.0_f32 - c),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_cpp() {
        assert_eq!(std::mem::size_of::<HighContrastConfig>(), 12);
        assert_eq!(std::mem::size_of::<HighContrastUniforms>(), 12);
    }

    #[test]
    fn rejects_invalid_configs() {
        assert!(compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: false,
            invert_style: -1,
            contrast: 0.0,
        })
        .is_none());
        assert!(compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: false,
            invert_style: 3,
            contrast: 0.0,
        })
        .is_none());
        assert!(compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: false,
            invert_style: 0,
            contrast: -1.01,
        })
        .is_none());
        assert!(compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: false,
            invert_style: 0,
            contrast: 1.01,
        })
        .is_none());
        assert!(compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: false,
            invert_style: 0,
            contrast: f32::NAN,
        })
        .is_none());
    }

    #[test]
    fn valid_uniforms_and_boundary_pinning() {
        let u0 = compute_high_contrast_uniforms(&HighContrastConfig::default()).unwrap();
        assert_eq!(u0.grayscale, 0.0_f32);
        assert_eq!(u0.invert_style, 0.0_f32);
        assert_eq!(u0.contrast, 1.0_f32);

        let u_pos = compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: true,
            invert_style: InvertStyle::InvertLightness as i32,
            contrast: 1.0_f32,
        })
        .unwrap();
        assert_eq!(u_pos.grayscale, 1.0_f32);
        assert_eq!(u_pos.invert_style, 2.0_f32);
        assert!(u_pos.contrast.is_finite());
        assert!(u_pos.contrast > 1e6);

        let u_neg = compute_high_contrast_uniforms(&HighContrastConfig {
            grayscale: false,
            invert_style: InvertStyle::InvertBrightness as i32,
            contrast: -1.0_f32,
        })
        .unwrap();
        assert_eq!(u_neg.invert_style, 1.0_f32);
        assert!(u_neg.contrast > 0.0_f32 && u_neg.contrast < 1e-6);
    }
}
