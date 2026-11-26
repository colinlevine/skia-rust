//! Safe Rust port of `src/effects/colorfilters/SkWorkingFormatColorFilter.cpp`.

use skia_rs_color::color_space::{Matrix3x3, TransferFunction};
use skia_rs_color::image_info::AlphaType;

/// Exact `#[repr(C)]` match for `SkWorkingFormatCalculator`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WorkingFormatCalculator {
    pub tf: TransferFunction,
    pub use_dst_tf: bool,
    pub gamut: Matrix3x3,
    pub use_dst_gamut: bool,
    pub at: AlphaType,
    pub use_dst_at: bool,
}

impl Default for WorkingFormatCalculator {
    fn default() -> Self {
        Self {
            tf: TransferFunction::default(),
            use_dst_tf: true,
            gamut: Matrix3x3::default(),
            use_dst_gamut: true,
            at: AlphaType::Unknown,
            use_dst_at: true,
        }
    }
}

/// Resolved working format components computed from a calculator and a destination color space.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ResolvedWorkingFormat {
    pub tf: TransferFunction,
    pub gamut: Matrix3x3,
    pub alpha_type: AlphaType,
}

impl WorkingFormatCalculator {
    /// Exact port of `SkWorkingFormatCalculator::SkWorkingFormatCalculator(tf, gamut, at)`.
    pub fn new(
        tf: Option<TransferFunction>,
        gamut: Option<Matrix3x3>,
        at: Option<AlphaType>,
    ) -> Self {
        let mut calc = Self::default();
        if let Some(tf_val) = tf {
            calc.tf = tf_val;
            calc.use_dst_tf = false;
        }
        if let Some(gamut_val) = gamut {
            calc.gamut = gamut_val;
            calc.use_dst_gamut = false;
        }
        if let Some(at_val) = at {
            calc.at = at_val;
            calc.use_dst_at = false;
        }
        calc
    }

    /// Resolve the working format given destination transfer function and gamut (when needed).
    /// Exact match for `SkWorkingFormatCalculator::workingFormat`.
    pub fn resolve(
        &self,
        dst_tf: TransferFunction,
        dst_gamut: Matrix3x3,
    ) -> ResolvedWorkingFormat {
        let tf = if self.use_dst_tf { dst_tf } else { self.tf };
        let gamut = if self.use_dst_gamut {
            dst_gamut
        } else {
            self.gamut
        };
        let alpha_type = if self.use_dst_at {
            AlphaType::Premul
        } else {
            self.at
        };
        ResolvedWorkingFormat {
            tf,
            gamut,
            alpha_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_color::color_space::{GAMUT_DISPLAY_P3, GAMUT_SRGB, TF_LINEAR, TF_SRGB};

    #[test]
    fn layout_matches_cpp() {
        assert_eq!(std::mem::size_of::<WorkingFormatCalculator>(), 80);
    }

    #[test]
    fn overrides_and_dst_fallbacks() {
        let all_dst = WorkingFormatCalculator::new(None, None, None);
        assert!(all_dst.use_dst_tf && all_dst.use_dst_gamut && all_dst.use_dst_at);
        let r1 = all_dst.resolve(TF_SRGB, GAMUT_SRGB);
        assert_eq!(r1.tf, TF_SRGB);
        assert_eq!(r1.gamut, GAMUT_SRGB);
        assert_eq!(r1.alpha_type, AlphaType::Premul);

        let override_tf_at =
            WorkingFormatCalculator::new(Some(TF_LINEAR), None, Some(AlphaType::Unpremul));
        assert!(!override_tf_at.use_dst_tf);
        assert!(override_tf_at.use_dst_gamut);
        assert!(!override_tf_at.use_dst_at);
        let r2 = override_tf_at.resolve(TF_SRGB, GAMUT_DISPLAY_P3);
        assert_eq!(r2.tf, TF_LINEAR);
        assert_eq!(r2.gamut, GAMUT_DISPLAY_P3);
        assert_eq!(r2.alpha_type, AlphaType::Unpremul);

        let override_all = WorkingFormatCalculator::new(
            Some(TF_LINEAR),
            Some(GAMUT_SRGB),
            Some(AlphaType::Opaque),
        );
        let r3 = override_all.resolve(TF_SRGB, GAMUT_DISPLAY_P3);
        assert_eq!(r3.tf, TF_LINEAR);
        assert_eq!(r3.gamut, GAMUT_SRGB);
        assert_eq!(r3.alpha_type, AlphaType::Opaque);
    }
}
