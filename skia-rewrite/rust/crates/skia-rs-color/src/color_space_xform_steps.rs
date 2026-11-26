//! Safe Rust port of `src/core/SkColorSpaceXformSteps.cpp`.

use crate::color_space::{Matrix3x3, TransferFunction, GAMUT_REC2020, TF_LINEAR};
use crate::image_info::AlphaType;

/// Matches `enum skcms_TFType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TfType {
    Invalid = 0,
    SRGBish = 1,
    PQish = 2,
    HLGish = 3,
    HLGinvish = 4,
    PQ = 5,
    HLG = 6,
}

impl TfType {
    #[inline]
    pub const fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::SRGBish,
            2 => Self::PQish,
            3 => Self::HLGish,
            4 => Self::HLGinvish,
            5 => Self::PQ,
            6 => Self::HLG,
            _ => Self::Invalid,
        }
    }
}

/// Matches `SkColorSpaceXformSteps::Flags`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct XformStepsFlags {
    pub unpremul: bool,
    pub linearize: bool,
    pub src_ootf: bool,
    pub gamut_transform: bool,
    pub dst_ootf: bool,
    pub encode: bool,
    pub premul: bool,
}

impl XformStepsFlags {
    #[inline]
    pub const fn mask(&self) -> u32 {
        (if self.unpremul { 1 } else { 0 })
            | (if self.linearize { 2 } else { 0 })
            | (if self.src_ootf { 32 } else { 0 })
            | (if self.gamut_transform { 4 } else { 0 })
            | (if self.dst_ootf { 64 } else { 0 })
            | (if self.encode { 8 } else { 0 })
            | (if self.premul { 16 } else { 0 })
    }
}

/// Exact `#[repr(C)]` match for `SkColorSpaceXformSteps`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ColorSpaceXformSteps {
    pub flags: XformStepsFlags,
    pub src_tf: TransferFunction,
    pub dst_tf_inv: TransferFunction,
    pub src_to_dst_matrix: [f32; 9],
    pub src_ootf: [f32; 4],
    pub dst_ootf: [f32; 4],
}

pub const PQISH_TF: TransferFunction = TransferFunction {
    g: -2.0_f32,
    a: -107.0_f32 / 128.0_f32,
    b: 1.0_f32,
    c: 32.0_f32 / 2523.0_f32,
    d: 2413.0_f32 / 128.0_f32,
    e: -2392.0_f32 / 128.0_f32,
    f: 8192.0_f32 / 1305.0_f32,
};

pub const HLGISH_TF: TransferFunction = TransferFunction {
    g: -3.0_f32,
    a: 2.0_f32,
    b: 2.0_f32,
    c: 1.0_f32 / 0.17883277_f32,
    d: 0.28466892_f32,
    e: 0.55991073_f32,
    f: 0.0_f32,
};

/// Compute the Y vector for the HLG OOTF in the primaries of a color space with `to_xyzd50`.
pub fn compute_ootf_y(to_xyzd50: &Matrix3x3) -> [f32; 3] {
    let rec2020_inv = GAMUT_REC2020
        .invert()
        .expect("Rec2020 gamut matrix is invertible");
    let m = rec2020_inv.concat(to_xyzd50);
    const Y_REC2020: [f32; 3] = [0.262700_f32, 0.678000_f32, 0.059300_f32];
    let mut y = [0.0_f32; 3];
    for i in 0..3 {
        let mut sum = 0.0_f32;
        for j in 0..3 {
            sum += m.vals[j][i] * Y_REC2020[j];
        }
        y[i] = sum;
    }
    y
}

/// Inputs extracted from `src` and `dst` `SkColorSpace` needed to build `ColorSpaceXformSteps`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct XformStepsInput {
    pub src_tf: TransferFunction,
    pub src_to_xyzd50: Matrix3x3,
    pub src_tf_hash: u32,
    pub src_xyz_hash: u32,
    pub src_tf_type: TfType,
    pub src_at: AlphaType,

    pub dst_tf: TransferFunction,
    pub dst_tf_inv: TransferFunction,
    pub dst_to_xyzd50: Matrix3x3,
    pub dst_from_xyzd50: Matrix3x3,
    pub dst_tf_hash: u32,
    pub dst_xyz_hash: u32,
    pub dst_tf_type: TfType,
    pub dst_at: AlphaType,
}

impl ColorSpaceXformSteps {
    /// Exact port of `SkColorSpaceXformSteps::SkColorSpaceXformSteps`.
    /// Note: For `dst_tf_type == PQ` or `HLG`, the caller supplies the already-inverted
    /// `PQish` / `HLGish(f = 1/12 - 1)` in `dst_tf_inv` (computed via `skcms_TransferFunction_invert`)
    /// so bit-exact `skcms` inversion is preserved.
    pub fn build(input: &XformStepsInput) -> Self {
        let mut steps = Self::default();

        let src_at = input.src_at;
        let dst_at = if input.dst_at == AlphaType::Opaque {
            src_at
        } else {
            input.dst_at
        };

        let src_hash = ((input.src_tf_hash as u64) << 32) | (input.src_xyz_hash as u64);
        let dst_hash = ((input.dst_tf_hash as u64) << 32) | (input.dst_xyz_hash as u64);
        if src_hash == dst_hash && src_at == dst_at {
            return steps;
        }

        let mut scale_factor = 1.0_f32;

        match input.src_tf_type {
            TfType::PQ => {
                scale_factor *= 10000.0_f32 / input.src_tf.a;
                steps.src_tf = PQISH_TF;
                steps.flags.linearize = true;
            }
            TfType::HLG => {
                scale_factor *= input.src_tf.b / input.src_tf.a;
                steps.flags.linearize = true;
                steps.src_tf = HLGISH_TF;
                steps.src_tf.f = 1.0_f32 / 12.0_f32 - 1.0_f32;
                if input.src_tf.c != 1.0_f32 {
                    steps.flags.src_ootf = true;
                    steps.src_ootf[3] = input.src_tf.c - 1.0_f32;
                    let y = compute_ootf_y(&input.src_to_xyzd50);
                    steps.src_ootf[0] = y[0];
                    steps.src_ootf[1] = y[1];
                    steps.src_ootf[2] = y[2];
                }
            }
            _ => {
                steps.flags.linearize = !input.src_tf.bit_eq(&TF_LINEAR);
                if steps.flags.linearize {
                    steps.src_tf = input.src_tf;
                }
            }
        }

        match input.dst_tf_type {
            TfType::PQ => {
                scale_factor /= 10000.0_f32 / input.dst_tf.a;
                steps.flags.encode = true;
                steps.dst_tf_inv = input.dst_tf_inv;
            }
            TfType::HLG => {
                scale_factor /= input.dst_tf.b / input.dst_tf.a;
                steps.flags.encode = true;
                steps.dst_tf_inv = input.dst_tf_inv;
                if input.dst_tf.c != 1.0_f32 {
                    steps.flags.dst_ootf = true;
                    steps.dst_ootf[3] = 1.0_f32 / input.dst_tf.c - 1.0_f32;
                    let y = compute_ootf_y(&input.dst_to_xyzd50);
                    steps.dst_ootf[0] = y[0];
                    steps.dst_ootf[1] = y[1];
                    steps.dst_ootf[2] = y[2];
                }
            }
            _ => {
                steps.flags.encode = !input.dst_tf.bit_eq(&TF_LINEAR);
                if steps.flags.encode {
                    steps.dst_tf_inv = input.dst_tf_inv;
                }
            }
        }

        steps.flags.unpremul = src_at == AlphaType::Premul;
        steps.flags.gamut_transform =
            input.src_xyz_hash != input.dst_xyz_hash || scale_factor != 1.0_f32;
        steps.flags.premul = src_at != AlphaType::Opaque && dst_at == AlphaType::Premul;

        if steps.flags.gamut_transform {
            let src_to_dst = input.dst_from_xyzd50.concat(&input.src_to_xyzd50);
            // Column-major storage
            steps.src_to_dst_matrix[0] = src_to_dst.vals[0][0] * scale_factor;
            steps.src_to_dst_matrix[1] = src_to_dst.vals[1][0] * scale_factor;
            steps.src_to_dst_matrix[2] = src_to_dst.vals[2][0] * scale_factor;

            steps.src_to_dst_matrix[3] = src_to_dst.vals[0][1] * scale_factor;
            steps.src_to_dst_matrix[4] = src_to_dst.vals[1][1] * scale_factor;
            steps.src_to_dst_matrix[5] = src_to_dst.vals[2][1] * scale_factor;

            steps.src_to_dst_matrix[6] = src_to_dst.vals[0][2] * scale_factor;
            steps.src_to_dst_matrix[7] = src_to_dst.vals[1][2] * scale_factor;
            steps.src_to_dst_matrix[8] = src_to_dst.vals[2][2] * scale_factor;
        }

        // If the source and destination OOTFs cancel each other out, skip both.
        if steps.flags.src_ootf && !steps.flags.gamut_transform && steps.flags.dst_ootf {
            if (steps.src_ootf[3] + 1.0_f32) * (steps.dst_ootf[3] + 1.0_f32) == 1.0_f32 {
                steps.flags.src_ootf = false;
                steps.flags.dst_ootf = false;
            }
        }

        // If we linearize then immediately reencode with the same transfer function, skip both.
        if steps.flags.linearize
            && !steps.flags.src_ootf
            && !steps.flags.gamut_transform
            && !steps.flags.dst_ootf
            && steps.flags.encode
            && input.src_tf_hash == input.dst_tf_hash
        {
            steps.flags.linearize = false;
            steps.flags.encode = false;
        }

        // Skip unpremul...premul if there are no non-linear operations between.
        if steps.flags.unpremul
            && !steps.flags.linearize
            && !steps.flags.encode
            && steps.flags.premul
        {
            steps.flags.unpremul = false;
            steps.flags.premul = false;
        }

        steps
    }

    /// Step 1 of `apply(float rgba[4])`: unpremul.
    #[inline]
    pub fn apply_unpremul(&self, rgba: &mut [f32; 4]) {
        if self.flags.unpremul {
            let inv_a = 1.0_f32 / rgba[3];
            let inv_a = if inv_a * 0.0_f32 == 0.0_f32 {
                inv_a
            } else {
                0.0_f32
            };
            rgba[0] *= inv_a;
            rgba[1] *= inv_a;
            rgba[2] *= inv_a;
        }
    }

    /// Steps 3-5 of `apply(float rgba[4])`: src_ootf, gamut_transform, dst_ootf.
    #[inline]
    pub fn apply_mid(&self, rgba: &mut [f32; 4]) {
        if self.flags.src_ootf {
            let y = self.src_ootf[0] * rgba[0]
                + self.src_ootf[1] * rgba[1]
                + self.src_ootf[2] * rgba[2];
            let y_pow = y.powf(self.src_ootf[3]);
            rgba[0] *= y_pow;
            rgba[1] *= y_pow;
            rgba[2] *= y_pow;
        }
        if self.flags.gamut_transform {
            let temp = [rgba[0], rgba[1], rgba[2]];
            for i in 0..3 {
                rgba[i] = self.src_to_dst_matrix[i] * temp[0]
                    + self.src_to_dst_matrix[3 + i] * temp[1]
                    + self.src_to_dst_matrix[6 + i] * temp[2];
            }
        }
        if self.flags.dst_ootf {
            let y = self.dst_ootf[0] * rgba[0]
                + self.dst_ootf[1] * rgba[1]
                + self.dst_ootf[2] * rgba[2];
            let y_pow = y.powf(self.dst_ootf[3]);
            rgba[0] *= y_pow;
            rgba[1] *= y_pow;
            rgba[2] *= y_pow;
        }
    }

    /// Step 7 of `apply(float rgba[4])`: premul.
    #[inline]
    pub fn apply_premul(&self, rgba: &mut [f32; 4]) {
        if self.flags.premul {
            rgba[0] *= rgba[3];
            rgba[1] *= rgba[3];
            rgba[2] *= rgba[3];
        }
    }

    /// Full `apply(float rgba[4])` with caller-provided transfer function evaluator.
    pub fn apply_with_tf_eval(
        &self,
        rgba: &mut [f32; 4],
        mut eval_tf: impl FnMut(&TransferFunction, f32) -> f32,
    ) {
        self.apply_unpremul(rgba);
        if self.flags.linearize {
            rgba[0] = eval_tf(&self.src_tf, rgba[0]);
            rgba[1] = eval_tf(&self.src_tf, rgba[1]);
            rgba[2] = eval_tf(&self.src_tf, rgba[2]);
        }
        self.apply_mid(rgba);
        if self.flags.encode {
            rgba[0] = eval_tf(&self.dst_tf_inv, rgba[0]);
            rgba[1] = eval_tf(&self.dst_tf_inv, rgba[1]);
            rgba[2] = eval_tf(&self.dst_tf_inv, rgba[2]);
        }
        self.apply_premul(rgba);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color_space::{GAMUT_DISPLAY_P3, GAMUT_SRGB, TF_SRGB};

    #[test]
    fn noop_same_space_and_alpha() {
        let inv_srgb = GAMUT_SRGB.invert().unwrap();
        let inp = XformStepsInput {
            src_tf: TF_SRGB,
            src_to_xyzd50: GAMUT_SRGB,
            src_tf_hash: TF_SRGB.hash32(),
            src_xyz_hash: GAMUT_SRGB.hash32(),
            src_tf_type: TfType::SRGBish,
            src_at: AlphaType::Premul,
            dst_tf: TF_SRGB,
            dst_tf_inv: TF_SRGB,
            dst_to_xyzd50: GAMUT_SRGB,
            dst_from_xyzd50: inv_srgb,
            dst_tf_hash: TF_SRGB.hash32(),
            dst_xyz_hash: GAMUT_SRGB.hash32(),
            dst_tf_type: TfType::SRGBish,
            dst_at: AlphaType::Premul,
        };
        let steps = ColorSpaceXformSteps::build(&inp);
        assert_eq!(steps.flags.mask(), 0);
    }

    #[test]
    fn linear_gamut_change_skips_unpremul_premul() {
        let inv_p3 = GAMUT_DISPLAY_P3.invert().unwrap();
        let inp = XformStepsInput {
            src_tf: TF_LINEAR,
            src_to_xyzd50: GAMUT_SRGB,
            src_tf_hash: TF_LINEAR.hash32(),
            src_xyz_hash: GAMUT_SRGB.hash32(),
            src_tf_type: TfType::SRGBish,
            src_at: AlphaType::Premul,
            dst_tf: TF_LINEAR,
            dst_tf_inv: TF_LINEAR,
            dst_to_xyzd50: GAMUT_DISPLAY_P3,
            dst_from_xyzd50: inv_p3,
            dst_tf_hash: TF_LINEAR.hash32(),
            dst_xyz_hash: GAMUT_DISPLAY_P3.hash32(),
            dst_tf_type: TfType::SRGBish,
            dst_at: AlphaType::Premul,
        };
        let steps = ColorSpaceXformSteps::build(&inp);
        assert!(steps.flags.gamut_transform);
        assert!(!steps.flags.linearize);
        assert!(!steps.flags.encode);
        assert!(!steps.flags.unpremul);
        assert!(!steps.flags.premul);
    }

    #[test]
    fn apply_unpremul_zero_alpha_safe() {
        let mut steps = ColorSpaceXformSteps::default();
        steps.flags.unpremul = true;
        let mut rgba = [0.5, 0.25, 0.1, 0.0];
        steps.apply_unpremul(&mut rgba);
        assert_eq!(rgba, [0.0, 0.0, 0.0, 0.0]);
    }
}
