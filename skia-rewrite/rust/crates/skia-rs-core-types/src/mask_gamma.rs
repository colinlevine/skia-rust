//! Safe Rust implementation of `SkMaskGamma` luminance conversions and correcting LUT builder.

use crate::scalar_round_to_int;

/// Luminance transfer mode matching the 3 subclasses of `SkColorSpaceLuminance` in `SkMaskGamma.cpp`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LuminanceMode {
    SRgb = 0,
    Linear = 1,
    Gamma = 2,
}

impl LuminanceMode {
    /// Matches `SkColorSpaceLuminance::Fetch(SkScalar gamma)`.
    #[inline]
    pub fn from_gamma(gamma: f32) -> Self {
        if gamma == 0.0_f32 {
            Self::SRgb
        } else if gamma == 1.0_f32 {
            Self::Linear
        } else {
            Self::Gamma
        }
    }
}

/// Converts perceptual luminance `[0, 1]` to linear luma `[0, 1]`.
#[inline]
pub fn to_luma(mode: LuminanceMode, gamma: f32, luminance: f32) -> f32 {
    match mode {
        LuminanceMode::Linear => luminance,
        LuminanceMode::Gamma => luminance.powf(gamma),
        LuminanceMode::SRgb => {
            if luminance <= 0.04045_f32 {
                luminance / 12.92_f32
            } else {
                ((luminance + 0.055_f32) / 1.055_f32).powf(2.4_f32)
            }
        }
    }
}

/// Converts linear luma `[0, 1]` to perceptual luminance `[0, 1]`.
#[inline]
pub fn from_luma(mode: LuminanceMode, gamma: f32, luma: f32) -> f32 {
    match mode {
        LuminanceMode::Linear => luma,
        LuminanceMode::Gamma => luma.powf(1.0_f32 / gamma),
        LuminanceMode::SRgb => {
            if luma <= 0.0031308_f32 {
                luma * 12.92_f32
            } else {
                1.055_f32 * luma.powf(1.0_f32 / 2.4_f32) - 0.055_f32
            }
        }
    }
}

#[inline]
pub fn apply_contrast(srca: f32, contrast: f32) -> f32 {
    srca + ((1.0_f32 - srca) * contrast * srca)
}

/// Matches `SkTMaskGamma_build_correcting_lut` in `SkMaskGamma.cpp`.
pub fn build_correcting_lut(
    table: &mut [u8; 256],
    src_i: u8,
    contrast: f32,
    mode: LuminanceMode,
    dst_gamma: f32,
) {
    let src = (src_i as f32) / 255.0_f32;
    let lin_src = to_luma(mode, dst_gamma, src);
    let dst = 1.0_f32 - src;
    let lin_dst = to_luma(mode, dst_gamma, dst);

    let adjusted_contrast = contrast * lin_dst;

    if (src - dst).abs() < (1.0_f32 / 256.0_f32) {
        let mut ii = 0.0_f32;
        for slot in table.iter_mut() {
            let raw_srca = ii / 255.0_f32;
            let srca = apply_contrast(raw_srca, adjusted_contrast);
            *slot = scalar_round_to_int(255.0_f32 * srca) as u8;
            ii += 1.0_f32;
        }
    } else {
        let mut ii = 0.0_f32;
        for slot in table.iter_mut() {
            let raw_srca = ii / 255.0_f32;
            let srca = apply_contrast(raw_srca, adjusted_contrast);
            let dsta = 1.0_f32 - srca;

            let lin_out = lin_src * srca + dsta * lin_dst;
            let out = from_luma(mode, dst_gamma, lin_out);

            let result = (out - dst) / (src - dst);
            *slot = scalar_round_to_int(255.0_f32 * result) as u8;
            ii += 1.0_f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_roundtrip() {
        for i in 0..=255u8 {
            let x = (i as f32) / 255.0;
            let luma = to_luma(LuminanceMode::SRgb, 0.0, x);
            let back = from_luma(LuminanceMode::SRgb, 0.0, luma);
            assert!((x - back).abs() < 1e-5, "i={i} x={x} back={back}");
        }
    }

    #[test]
    fn test_linear_lut_identity() {
        let mut lut = [0u8; 256];
        build_correcting_lut(&mut lut, 0, 0.0, LuminanceMode::Linear, 1.0);
        for (i, &v) in lut.iter().enumerate() {
            assert_eq!(v, i as u8);
        }
    }

    #[test]
    fn test_srgb_lut_endpoints_and_monotonicity() {
        for &src_i in &[0u8, 64, 127, 128, 192, 255] {
            let mut lut = [0u8; 256];
            build_correcting_lut(&mut lut, src_i, 0.5, LuminanceMode::SRgb, 0.0);
            assert_eq!(lut[0], 0);
            assert_eq!(lut[255], 255);
            for w in lut.windows(2) {
                assert!(w[0] <= w[1], "src_i={src_i} non-monotonic {} > {}", w[0], w[1]);
            }
        }
    }

    #[test]
    fn test_gamma_lut_endpoints() {
        let mut lut = [0u8; 256];
        build_correcting_lut(&mut lut, 0, 0.25, LuminanceMode::Gamma, 2.2);
        assert_eq!(lut[0], 0);
        assert_eq!(lut[255], 255);
        // For black text on white (src=0, dst=1) with gamma 2.2, linear blending produces higher
        // perceptual luminance, so less black coverage is needed (~80 at index 128).
        assert!(lut[128] < 128, "expected lut[128] < 128, got {}", lut[128]);

        // Conversely, white text on black (src=255, dst=0) boosts white coverage.
        let mut lut_white = [0u8; 256];
        build_correcting_lut(&mut lut_white, 255, 0.25, LuminanceMode::Gamma, 2.2);
        assert_eq!(lut_white[0], 0);
        assert_eq!(lut_white[255], 255);
        assert!(lut_white[128] > 128, "expected lut_white[128] > 128, got {}", lut_white[128]);
    }
}
