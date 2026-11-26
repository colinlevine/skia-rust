//! Safe Rust port of `src/effects/SkColorMatrix.cpp`.

use skia_rs_color::yuv_math::{color_matrix_rgb2yuv, color_matrix_yuv2rgb};

const K_R_SCALE: usize = 0;
const K_G_SCALE: usize = 6;
const K_B_SCALE: usize = 12;
const K_A_SCALE: usize = 18;

const K_R_TRANS: usize = 4;
const K_G_TRANS: usize = 9;
const K_B_TRANS: usize = 14;
const K_A_TRANS: usize = 19;

const K_HUE_R: f32 = 0.213_f32;
const K_HUE_G: f32 = 0.715_f32;
const K_HUE_B: f32 = 0.072_f32;

/// 4x5 row-major color matrix matching `SkColorMatrix` (`std::array<float, 20> fMat`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ColorMatrix {
    pub mat: [f32; 20],
}

impl Default for ColorMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

impl ColorMatrix {
    /// Identity 4x5 color matrix.
    #[inline]
    pub const fn identity() -> Self {
        Self {
            mat: [
                1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 1.0, 0.0,
            ],
        }
    }

    /// Construct from a 20-element row-major array.
    #[inline]
    pub const fn from_row_major(mat: [f32; 20]) -> Self {
        Self { mat }
    }

    /// Exact port of `SkColorMatrix::RGBtoYUV(SkYUVColorSpace)`.
    #[inline]
    pub fn rgb_to_yuv(cs: i32, high_precision: bool) -> Self {
        let mut m = Self::identity();
        color_matrix_rgb2yuv(cs, &mut m.mat, high_precision);
        m
    }

    /// Exact port of `SkColorMatrix::YUVtoRGB(SkYUVColorSpace)`.
    #[inline]
    pub fn yuv_to_rgb(cs: i32, high_precision: bool) -> Self {
        let mut m = Self::identity();
        color_matrix_yuv2rgb(cs, &mut m.mat, high_precision);
        m
    }

    /// Exact port of `SkColorMatrix::setIdentity()`.
    #[inline]
    pub fn set_identity(&mut self) {
        self.mat.fill(0.0_f32);
        self.mat[K_R_SCALE] = 1.0;
        self.mat[K_G_SCALE] = 1.0;
        self.mat[K_B_SCALE] = 1.0;
        self.mat[K_A_SCALE] = 1.0;
    }

    /// Exact port of `SkColorMatrix::setScale(rScale, gScale, bScale, aScale)`.
    #[inline]
    pub fn set_scale(&mut self, r_scale: f32, g_scale: f32, b_scale: f32, a_scale: f32) {
        self.mat.fill(0.0_f32);
        self.mat[K_R_SCALE] = r_scale;
        self.mat[K_G_SCALE] = g_scale;
        self.mat[K_B_SCALE] = b_scale;
        self.mat[K_A_SCALE] = a_scale;
    }

    /// Exact port of `SkColorMatrix::postTranslate(dr, dg, db, da)`.
    #[inline]
    pub fn post_translate(&mut self, dr: f32, dg: f32, db: f32, da: f32) {
        self.mat[K_R_TRANS] += dr;
        self.mat[K_G_TRANS] += dg;
        self.mat[K_B_TRANS] += db;
        self.mat[K_A_TRANS] += da;
    }

    /// Exact port of `set_concat(result, outer, inner)`.
    pub fn concat_arrays(outer: &[f32; 20], inner: &[f32; 20]) -> [f32; 20] {
        let mut target = [0.0_f32; 20];
        let mut index = 0usize;
        let mut j = 0usize;
        while j < 20 {
            for i in 0..4 {
                target[index] = outer[j] * inner[i]
                    + outer[j + 1] * inner[i + 5]
                    + outer[j + 2] * inner[i + 10]
                    + outer[j + 3] * inner[i + 15];
                index += 1;
            }
            target[index] = outer[j] * inner[4]
                + outer[j + 1] * inner[9]
                + outer[j + 2] * inner[14]
                + outer[j + 3] * inner[19]
                + outer[j + 4];
            index += 1;
            j += 5;
        }
        target
    }

    /// Exact port of `SkColorMatrix::setConcat(matA, matB)`.
    #[inline]
    pub fn set_concat(&mut self, mat_a: &ColorMatrix, mat_b: &ColorMatrix) {
        self.mat = Self::concat_arrays(&mat_a.mat, &mat_b.mat);
    }

    /// Exact port of `SkColorMatrix::preConcat(mat)` (`setConcat(*this, mat)`).
    #[inline]
    pub fn pre_concat(&mut self, mat: &ColorMatrix) {
        let cur = self.mat;
        self.mat = Self::concat_arrays(&cur, &mat.mat);
    }

    /// Exact port of `SkColorMatrix::postConcat(mat)` (`setConcat(mat, *this)`).
    #[inline]
    pub fn post_concat(&mut self, mat: &ColorMatrix) {
        let cur = self.mat;
        self.mat = Self::concat_arrays(&mat.mat, &cur);
    }

    /// Exact port of `SkColorMatrix::setSaturation(sat)`.
    #[inline]
    pub fn set_saturation(&mut self, sat: f32) {
        self.mat.fill(0.0_f32);

        let r = K_HUE_R * (1.0 - sat);
        let g = K_HUE_G * (1.0 - sat);
        let b = K_HUE_B * (1.0 - sat);

        self.mat[0] = r + sat;
        self.mat[1] = g;
        self.mat[2] = b;

        self.mat[5] = r;
        self.mat[6] = g + sat;
        self.mat[7] = b;

        self.mat[10] = r;
        self.mat[11] = g;
        self.mat[12] = b + sat;

        self.mat[K_A_SCALE] = 1.0;
    }

    /// Apply this 4x5 matrix to an unpremultiplied RGBA color `[r, g, b, a]`.
    #[inline]
    pub fn apply(&self, rgba: [f32; 4]) -> [f32; 4] {
        let m = &self.mat;
        [
            m[0] * rgba[0] + m[1] * rgba[1] + m[2] * rgba[2] + m[3] * rgba[3] + m[4],
            m[5] * rgba[0] + m[6] * rgba[1] + m[7] * rgba[2] + m[8] * rgba[3] + m[9],
            m[10] * rgba[0] + m[11] * rgba[1] + m[12] * rgba[2] + m[13] * rgba[3] + m[14],
            m[15] * rgba[0] + m[16] * rgba[1] + m[17] * rgba[2] + m[18] * rgba[3] + m[19],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_color::yuv_math::{YUV_CS_IDENTITY, YUV_CS_REC601_8BIT_FULL, YUV_CS_REC709_8BIT_LIMITED};

    #[test]
    fn identity_scale_translate() {
        let mut m = ColorMatrix::default();
        assert_eq!(m.apply([0.2, 0.4, 0.6, 0.8]), [0.2, 0.4, 0.6, 0.8]);

        m.set_scale(2.0, 0.5, 3.0, 1.0);
        m.post_translate(0.1, -0.2, 0.3, 0.0);
        let out = m.apply([0.25, 0.5, 0.1, 0.75]);
        assert!((out[0] - 0.6).abs() < 1e-6);
        assert!((out[1] - 0.05).abs() < 1e-6);
        assert!((out[2] - 0.6).abs() < 1e-6);
        assert!((out[3] - 0.75).abs() < 1e-6);

        m.set_identity();
        assert_eq!(m, ColorMatrix::identity());
    }

    #[test]
    fn concat_pre_post_and_aliasing() {
        let mut a = ColorMatrix::identity();
        a.set_scale(2.0, 3.0, 4.0, 1.0);
        a.post_translate(0.1, 0.2, 0.3, 0.0);

        let mut b = ColorMatrix::identity();
        b.set_scale(0.5, 1.0, 0.25, 0.5);
        b.post_translate(0.25, -0.1, 0.5, 0.1);

        let mut ab = ColorMatrix::identity();
        ab.set_concat(&a, &b);

        let input = [0.3, 0.4, 0.5, 0.6];
        let step_by_step = a.apply(b.apply(input));
        let combined = ab.apply(input);
        for i in 0..4 {
            assert!((step_by_step[i] - combined[i]).abs() < 1e-6);
        }

        let mut pre = a;
        pre.pre_concat(&b);
        assert_eq!(pre, ab);

        let mut post = b;
        post.post_concat(&a);
        assert_eq!(post, ab);
    }

    #[test]
    fn saturation_zero_and_one() {
        let mut m = ColorMatrix::identity();
        m.set_saturation(1.0);
        assert_eq!(m, ColorMatrix::identity());

        m.set_saturation(0.0);
        let gray = m.apply([1.0, 0.5, 0.25, 0.9]);
        let expected_luma = 0.213 * 1.0 + 0.715 * 0.5 + 0.072 * 0.25;
        assert!((gray[0] - expected_luma).abs() < 1e-6);
        assert!((gray[1] - expected_luma).abs() < 1e-6);
        assert!((gray[2] - expected_luma).abs() < 1e-6);
        assert_eq!(gray[3], 0.9);
    }

    #[test]
    fn yuv_roundtrip() {
        let id = ColorMatrix::rgb_to_yuv(YUV_CS_IDENTITY, false);
        assert_eq!(id, ColorMatrix::identity());

        for cs in [YUV_CS_REC601_8BIT_FULL, YUV_CS_REC709_8BIT_LIMITED] {
            let r2y = ColorMatrix::rgb_to_yuv(cs, false);
            let y2r = ColorMatrix::yuv_to_rgb(cs, false);
            let mut rt = ColorMatrix::identity();
            rt.set_concat(&y2r, &r2y);
            let rgb = [0.7, 0.3, 0.5, 1.0];
            let out = rt.apply(rgb);
            for i in 0..4 {
                assert!((out[i] - rgb[i]).abs() < 1e-4);
            }
        }
    }
}
