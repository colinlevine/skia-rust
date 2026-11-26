#![forbid(unsafe_code)]

use crate::{
    all_finite, cpp_max_f32, cpp_min_f32, point3::Point3, scalar_nearly_equal, scalar_nearly_zero,
    scalar_round_to_int, IRect, ISize, Point, RSXform, Rect, Size, SK_SCALAR_NEARLY_ZERO,
    SK_SCALAR_PI,
};

pub const MSCALE_X: usize = 0;
pub const MSKEW_X: usize = 1;
pub const MTRANS_X: usize = 2;
pub const MSKEW_Y: usize = 3;
pub const MSCALE_Y: usize = 4;
pub const MTRANS_Y: usize = 5;
pub const MPERSP_0: usize = 6;
pub const MPERSP_1: usize = 7;
pub const MPERSP_2: usize = 8;

pub const ASCALE_X: usize = 0;
pub const ASKEW_Y: usize = 1;
pub const ASKEW_X: usize = 2;
pub const ASCALE_Y: usize = 3;
pub const ATRANS_X: usize = 4;
pub const ATRANS_Y: usize = 5;

pub const IDENTITY_MASK: i32 = 0;
pub const TRANSLATE_MASK: i32 = 0x01;
pub const SCALE_MASK: i32 = 0x02;
pub const AFFINE_MASK: i32 = 0x04;
pub const PERSPECTIVE_MASK: i32 = 0x08;
pub const RECT_STAYS_RECT_MASK: i32 = 0x10;
pub const ONLY_PERSPECTIVE_VALID_MASK: i32 = 0x40;
pub const UNKNOWN_MASK: i32 = 0x80;
pub const ORABLE_MASKS: i32 =
    TRANSLATE_MASK | SCALE_MASK | AFFINE_MASK | PERSPECTIVE_MASK;
pub const ALL_MASKS: i32 = ORABLE_MASKS | RECT_STAYS_RECT_MASK;

const SCALAR_SIN_COS_NEARLY_ZERO: f32 = 1.0 / (1 << 16) as f32;
const SCALAR_1_INT: i32 = 0x3f80_0000;

/// Matches `SkMatrix::ScaleToFit`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScaleToFit {
    Fill = 0,
    Start = 1,
    Center = 2,
    End = 3,
}

/// Dense 3x3 matrix matching `SkMatrix` (`{ float fMat[9]; mutable int32_t fTypeMask; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Matrix {
    pub mat: [f32; 9],
    pub type_mask: i32,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::identity()
    }
}

impl PartialEq for Matrix {
    fn eq(&self, other: &Self) -> bool {
        self.mat[0] == other.mat[0]
            && self.mat[1] == other.mat[1]
            && self.mat[2] == other.mat[2]
            && self.mat[3] == other.mat[3]
            && self.mat[4] == other.mat[4]
            && self.mat[5] == other.mat[5]
            && self.mat[6] == other.mat[6]
            && self.mat[7] == other.mat[7]
            && self.mat[8] == other.mat[8]
    }
}

#[inline]
fn scalar_as_2s_compliment(x: f32) -> i32 {
    let mut bits = x.to_bits() as i32;
    if bits < 0 {
        bits &= 0x7fff_ffff;
        bits = bits.wrapping_neg();
    }
    bits
}

#[inline]
fn sin_snap_to_zero(radians: f32) -> f32 {
    let v = radians.sin();
    if scalar_nearly_zero(v, SCALAR_SIN_COS_NEARLY_ZERO) {
        0.0
    } else {
        v
    }
}

#[inline]
fn cos_snap_to_zero(radians: f32) -> f32 {
    let v = radians.cos();
    if scalar_nearly_zero(v, SCALAR_SIN_COS_NEARLY_ZERO) {
        0.0
    } else {
        v
    }
}

#[inline]
fn is_degenerate_2x2(scale_x: f32, skew_x: f32, skew_y: f32, scale_y: f32) -> bool {
    let perp_dot = scale_x * scale_y - skew_x * skew_y;
    scalar_nearly_zero(perp_dot, SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO)
}

#[inline]
fn sdot2(a: f32, b: f32, c: f32, d: f32) -> f32 {
    a * b + c * d
}

#[inline]
fn sdot3(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> f32 {
    a * b + c * d + e * f
}

#[inline]
fn scross(a: f32, b: f32, c: f32, d: f32) -> f32 {
    a * b - c * d
}

#[inline]
fn scross_dscale(a: f32, b: f32, c: f32, d: f32, scale: f64) -> f32 {
    (scross(a, b, c, d) as f64 * scale) as f32
}

#[inline]
fn dcross(a: f64, b: f64, c: f64, d: f64) -> f64 {
    a * b - c * d
}

#[inline]
fn dcross_dscale(a: f64, b: f64, c: f64, d: f64, scale: f64) -> f32 {
    (dcross(a, b, c, d) * scale) as f32
}

#[inline]
fn muladdmul(a: f32, b: f32, c: f32, d: f32) -> f32 {
    ((a as f64) * (b as f64) + (c as f64) * (d as f64)) as f32
}

#[inline]
fn rowcol3(row: &[f32], col: &[f32]) -> f32 {
    row[0] * col[0] + row[1] * col[3] + row[2] * col[6]
}

#[inline]
fn only_scale_and_translate(mask: i32) -> bool {
    (mask & (AFFINE_MASK | PERSPECTIVE_MASK)) == 0
}

fn sk_determinant(mat: &[f32; 9], is_perspective: bool) -> f64 {
    if is_perspective {
        (mat[MSCALE_X] as f64)
            * dcross(
                mat[MSCALE_Y] as f64,
                mat[MPERSP_2] as f64,
                mat[MTRANS_Y] as f64,
                mat[MPERSP_1] as f64,
            )
            + (mat[MSKEW_X] as f64)
                * dcross(
                    mat[MTRANS_Y] as f64,
                    mat[MPERSP_0] as f64,
                    mat[MSKEW_Y] as f64,
                    mat[MPERSP_2] as f64,
                )
            + (mat[MTRANS_X] as f64)
                * dcross(
                    mat[MSKEW_Y] as f64,
                    mat[MPERSP_1] as f64,
                    mat[MSCALE_Y] as f64,
                    mat[MPERSP_0] as f64,
                )
    } else {
        dcross(
            mat[MSCALE_X] as f64,
            mat[MSCALE_Y] as f64,
            mat[MSKEW_X] as f64,
            mat[MSKEW_Y] as f64,
        )
    }
}

fn sk_inv_determinant(mat: &[f32; 9], is_perspective: bool) -> f64 {
    let det = sk_determinant(mat, is_perspective);
    let tol = SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO;
    if scalar_nearly_zero(det as f32, tol) {
        return 0.0;
    }
    1.0 / det
}

#[inline]
fn check_for_zero(x: f32) -> bool {
    x * x == 0.0
}

#[inline]
fn sort_as_rect(ltrb: [f32; 4]) -> Rect {
    let rblt = [ltrb[2], ltrb[3], ltrb[0], ltrb[1]];
    let min0 = cpp_min_f32(ltrb[2], rblt[2]);
    let min1 = cpp_min_f32(ltrb[3], rblt[3]);
    let max0 = cpp_max_f32(ltrb[0], rblt[0]);
    let max1 = cpp_max_f32(ltrb[1], rblt[1]);
    Rect::make_ltrb(min0, min1, max0, max1)
}

impl Matrix {
    #[inline]
    pub const fn identity() -> Self {
        Self {
            mat: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            type_mask: IDENTITY_MASK | RECT_STAYS_RECT_MASK,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub const fn new_with_mask(
        sx: f32, kx: f32, tx: f32,
        ky: f32, sy: f32, ty: f32,
        p0: f32, p1: f32, p2: f32,
        type_mask: i32,
    ) -> Self {
        Self {
            mat: [sx, kx, tx, ky, sy, ty, p0, p1, p2],
            type_mask,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub const fn make_all(
        sx: f32, kx: f32, tx: f32,
        ky: f32, sy: f32, ty: f32,
        p0: f32, p1: f32, p2: f32,
    ) -> Self {
        Self::new_with_mask(sx, kx, tx, ky, sy, ty, p0, p1, p2, UNKNOWN_MASK)
    }

    #[inline]
    pub fn reset(&mut self) {
        *self = Self::identity();
    }

    #[inline]
    pub fn set9(&mut self, buffer: &[f32; 9]) {
        self.mat = *buffer;
        self.type_mask = UNKNOWN_MASK;
    }

    #[inline]
    pub fn set_affine(&mut self, buffer: &[f32; 6]) {
        self.mat[MSCALE_X] = buffer[ASCALE_X];
        self.mat[MSKEW_X] = buffer[ASKEW_X];
        self.mat[MTRANS_X] = buffer[ATRANS_X];
        self.mat[MSKEW_Y] = buffer[ASKEW_Y];
        self.mat[MSCALE_Y] = buffer[ASCALE_Y];
        self.mat[MTRANS_Y] = buffer[ATRANS_Y];
        self.mat[MPERSP_0] = 0.0;
        self.mat[MPERSP_1] = 0.0;
        self.mat[MPERSP_2] = 1.0;
        self.type_mask = UNKNOWN_MASK;
    }

    #[inline]
    pub fn set_affine_identity(affine: &mut [f32; 6]) {
        affine[ASCALE_X] = 1.0;
        affine[ASKEW_Y] = 0.0;
        affine[ASKEW_X] = 0.0;
        affine[ASCALE_Y] = 1.0;
        affine[ATRANS_X] = 0.0;
        affine[ATRANS_Y] = 0.0;
    }

    pub fn do_normalize_perspective(&mut self) {
        if self.mat[MPERSP_0] == 0.0 && self.mat[MPERSP_1] == 0.0 {
            let p2 = self.mat[MPERSP_2];
            if p2 != 0.0 && p2 != 1.0 {
                let inv = 1.0_f64 / (p2 as f64);
                for i in 0..6 {
                    self.mat[i] = ((self.mat[i] as f64) * inv) as f32;
                }
                self.mat[MPERSP_2] = 1.0;
            }
            self.type_mask = UNKNOWN_MASK;
        }
    }

    pub fn compute_perspective_type_mask(&self) -> u8 {
        if self.mat[MPERSP_0] != 0.0 || self.mat[MPERSP_1] != 0.0 || self.mat[MPERSP_2] != 1.0 {
            return ORABLE_MASKS as u8;
        }
        (ONLY_PERSPECTIVE_VALID_MASK | UNKNOWN_MASK) as u8
    }

    pub fn compute_type_mask(&self) -> u8 {
        if self.mat[MPERSP_0] != 0.0 || self.mat[MPERSP_1] != 0.0 || self.mat[MPERSP_2] != 1.0 {
            return ORABLE_MASKS as u8;
        }
        let mut mask: i32 = 0;
        if self.mat[MTRANS_X] != 0.0 || self.mat[MTRANS_Y] != 0.0 {
            mask |= TRANSLATE_MASK;
        }
        let mut m00 = scalar_as_2s_compliment(self.mat[MSCALE_X]);
        let mut m01 = scalar_as_2s_compliment(self.mat[MSKEW_X]);
        let mut m10 = scalar_as_2s_compliment(self.mat[MSKEW_Y]);
        let mut m11 = scalar_as_2s_compliment(self.mat[MSCALE_Y]);

        if (m01 | m10) != 0 {
            mask |= AFFINE_MASK | SCALE_MASK;
            m01 = i32::from(m01 != 0);
            m10 = i32::from(m10 != 0);
            let dp0 = i32::from((m00 | m11) == 0);
            let ds1 = m01 & m10;
            mask |= (dp0 & ds1) << 4;
        } else {
            if ((m00 ^ SCALAR_1_INT) | (m11 ^ SCALAR_1_INT)) != 0 {
                mask |= SCALE_MASK;
            }
            m00 = i32::from(m00 != 0);
            m11 = i32::from(m11 != 0);
            mask |= (m00 & m11) << 4;
        }
        mask as u8
    }

    #[inline]
    pub fn get_type_cached(&self) -> i32 {
        if (self.type_mask & UNKNOWN_MASK) != 0 {
            (self.compute_type_mask() as i32) & 0xF
        } else {
            self.type_mask & 0xF
        }
    }

    #[inline]
    pub fn get_type_mut(&mut self) -> i32 {
        if (self.type_mask & UNKNOWN_MASK) != 0 {
            self.type_mask = self.compute_type_mask() as i32;
        }
        self.type_mask & 0xF
    }

    #[inline]
    pub fn is_identity(&self) -> bool {
        self.get_type_cached() == 0
    }

    #[inline]
    pub fn is_trivially_identity(&self) -> bool {
        if (self.type_mask & UNKNOWN_MASK) != 0 {
            return false;
        }
        (self.type_mask & 0xF) == 0
    }

    #[inline]
    pub fn is_scale_translate(&self) -> bool {
        (self.get_type_cached() & !(SCALE_MASK | TRANSLATE_MASK)) == 0
    }

    #[inline]
    pub fn has_perspective(&self) -> bool {
        let mask = if (self.type_mask & UNKNOWN_MASK) != 0
            && (self.type_mask & ONLY_PERSPECTIVE_VALID_MASK) == 0
        {
            self.compute_perspective_type_mask() as i32
        } else {
            self.type_mask
        };
        (mask & PERSPECTIVE_MASK) != 0
    }

    #[inline]
    pub fn rect_stays_rect(&self) -> bool {
        let mask = if (self.type_mask & UNKNOWN_MASK) != 0 {
            self.compute_type_mask() as i32
        } else {
            self.type_mask
        };
        (mask & RECT_STAYS_RECT_MASK) != 0
    }

    #[inline]
    pub fn is_finite(&self) -> bool {
        all_finite(&self.mat)
    }

    #[inline]
    fn update_translate_mask(&mut self) {
        if self.mat[MTRANS_X] != 0.0 || self.mat[MTRANS_Y] != 0.0 {
            self.type_mask |= TRANSLATE_MASK;
        } else {
            self.type_mask &= !TRANSLATE_MASK;
        }
    }

    pub fn is_similarity(&self, tol: f32) -> bool {
        let mask = self.get_type_cached();
        if mask <= TRANSLATE_MASK {
            return true;
        }
        if (mask & PERSPECTIVE_MASK) != 0 {
            return false;
        }
        let mx = self.mat[MSCALE_X];
        let my = self.mat[MSCALE_Y];
        if (mask & AFFINE_MASK) == 0 {
            return !scalar_nearly_zero(mx, SK_SCALAR_NEARLY_ZERO)
                && scalar_nearly_equal(mx.abs(), my.abs(), SK_SCALAR_NEARLY_ZERO);
        }
        let sx = self.mat[MSKEW_X];
        let sy = self.mat[MSKEW_Y];
        if is_degenerate_2x2(mx, sx, sy, my) {
            return false;
        }
        (scalar_nearly_equal(mx, my, tol) && scalar_nearly_equal(sx, -sy, tol))
            || (scalar_nearly_equal(mx, -my, tol) && scalar_nearly_equal(sx, sy, tol))
    }

    pub fn preserves_right_angles(&self, tol: f32) -> bool {
        let mask = self.get_type_cached();
        if mask <= TRANSLATE_MASK {
            return true;
        }
        if (mask & PERSPECTIVE_MASK) != 0 {
            return false;
        }
        let mx = self.mat[MSCALE_X];
        let my = self.mat[MSCALE_Y];
        let sx = self.mat[MSKEW_X];
        let sy = self.mat[MSKEW_Y];
        if is_degenerate_2x2(mx, sx, sy, my) {
            return false;
        }
        let v0 = Point::new(mx, sy);
        let v1 = Point::new(sx, my);
        scalar_nearly_zero(v0.dot(v1), tol * tol)
    }

    pub fn scale_translate(sx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        let mut mask = 0;
        if sx != 1.0 || sy != 1.0 {
            mask |= SCALE_MASK;
        }
        if tx != 0.0 || ty != 0.0 {
            mask |= TRANSLATE_MASK;
        }
        if sx != 0.0 && sy != 0.0 {
            mask |= RECT_STAYS_RECT_MASK;
        }
        Self::new_with_mask(sx, 0.0, tx, 0.0, sy, ty, 0.0, 0.0, 1.0, mask)
    }

    pub fn translate(dx: f32, dy: f32) -> Self {
        let mask = if dx != 0.0 || dy != 0.0 {
            TRANSLATE_MASK | RECT_STAYS_RECT_MASK
        } else {
            IDENTITY_MASK | RECT_STAYS_RECT_MASK
        };
        Self::new_with_mask(1.0, 0.0, dx, 0.0, 1.0, dy, 0.0, 0.0, 1.0, mask)
    }

    pub fn set_translate(&mut self, dx: f32, dy: f32) {
        *self = Self::translate(dx, dy);
    }

    pub fn pre_translate(&mut self, dx: f32, dy: f32) {
        let mask = self.get_type_mut();
        if mask <= TRANSLATE_MASK {
            self.mat[MTRANS_X] += dx;
            self.mat[MTRANS_Y] += dy;
        } else if (mask & PERSPECTIVE_MASK) != 0 {
            let m = Self::translate(dx, dy);
            self.pre_concat(&m);
            return;
        } else {
            self.mat[MTRANS_X] += sdot2(self.mat[MSCALE_X], dx, self.mat[MSKEW_X], dy);
            self.mat[MTRANS_Y] += sdot2(self.mat[MSKEW_Y], dx, self.mat[MSCALE_Y], dy);
        }
        self.update_translate_mask();
    }

    pub fn post_translate(&mut self, dx: f32, dy: f32) {
        if self.has_perspective() {
            let m = Self::translate(dx, dy);
            self.post_concat(&m);
        } else {
            self.mat[MTRANS_X] += dx;
            self.mat[MTRANS_Y] += dy;
            self.update_translate_mask();
        }
    }

    pub fn set_scale(&mut self, sx: f32, sy: f32) {
        let rect_mask = if sx == 0.0 || sy == 0.0 {
            0
        } else {
            RECT_STAYS_RECT_MASK
        };
        let mask = if sx == 1.0 && sy == 1.0 {
            IDENTITY_MASK | rect_mask
        } else {
            SCALE_MASK | rect_mask
        };
        *self = Self::new_with_mask(sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 1.0, mask);
    }

    pub fn set_scale_pivot(&mut self, sx: f32, sy: f32, px: f32, py: f32) {
        if sx == 1.0 && sy == 1.0 {
            self.reset();
        } else {
            *self = Self::scale_translate(sx, sy, px - sx * px, py - sy * py);
        }
    }

    pub fn pre_scale_pivot(&mut self, sx: f32, sy: f32, px: f32, py: f32) {
        if sx == 1.0 && sy == 1.0 {
            return;
        }
        let mut m = Self::identity();
        m.set_scale_pivot(sx, sy, px, py);
        self.pre_concat(&m);
    }

    pub fn pre_scale(&mut self, sx: f32, sy: f32) {
        if sx == 1.0 && sy == 1.0 {
            return;
        }
        self.mat[MSCALE_X] *= sx;
        self.mat[MSKEW_Y] *= sx;
        self.mat[MPERSP_0] *= sx;

        self.mat[MSKEW_X] *= sy;
        self.mat[MSCALE_Y] *= sy;
        self.mat[MPERSP_1] *= sy;

        if self.mat[MSCALE_X] == 1.0
            && self.mat[MSCALE_Y] == 1.0
            && (self.type_mask & (PERSPECTIVE_MASK | AFFINE_MASK)) == 0
        {
            self.type_mask &= !SCALE_MASK;
        } else {
            self.type_mask |= SCALE_MASK;
            if sx == 0.0 || sy == 0.0 {
                self.type_mask &= !RECT_STAYS_RECT_MASK;
            }
        }
    }

    pub fn post_scale_pivot(&mut self, sx: f32, sy: f32, px: f32, py: f32) {
        if sx == 1.0 && sy == 1.0 {
            return;
        }
        let mut m = Self::identity();
        m.set_scale_pivot(sx, sy, px, py);
        self.post_concat(&m);
    }

    pub fn post_scale(&mut self, sx: f32, sy: f32) {
        if sx == 1.0 && sy == 1.0 {
            return;
        }
        let mut m = Self::identity();
        m.set_scale(sx, sy);
        self.post_concat(&m);
    }

    pub fn post_idiv(&mut self, divx: i32, divy: i32) -> bool {
        if divx == 0 || divy == 0 {
            return false;
        }
        let inv_x = 1.0_f32 / (divx as f32);
        let inv_y = 1.0_f32 / (divy as f32);
        self.mat[MSCALE_X] *= inv_x;
        self.mat[MSKEW_X] *= inv_x;
        self.mat[MTRANS_X] *= inv_x;
        self.mat[MSCALE_Y] *= inv_y;
        self.mat[MSKEW_Y] *= inv_y;
        self.mat[MTRANS_Y] *= inv_y;
        self.type_mask = UNKNOWN_MASK;
        true
    }

    pub fn set_sin_cos_pivot(&mut self, sin_v: f32, cos_v: f32, px: f32, py: f32) {
        let one_minus_cos = 1.0 - cos_v;
        self.mat[MSCALE_X] = cos_v;
        self.mat[MSKEW_X] = -sin_v;
        self.mat[MTRANS_X] = sdot2(sin_v, py, one_minus_cos, px);
        self.mat[MSKEW_Y] = sin_v;
        self.mat[MSCALE_Y] = cos_v;
        self.mat[MTRANS_Y] = sdot2(-sin_v, px, one_minus_cos, py);
        self.mat[MPERSP_0] = 0.0;
        self.mat[MPERSP_1] = 0.0;
        self.mat[MPERSP_2] = 1.0;
        self.type_mask = UNKNOWN_MASK | ONLY_PERSPECTIVE_VALID_MASK;
    }

    pub fn set_sin_cos(&mut self, sin_v: f32, cos_v: f32) {
        self.mat[MSCALE_X] = cos_v;
        self.mat[MSKEW_X] = -sin_v;
        self.mat[MTRANS_X] = 0.0;
        self.mat[MSKEW_Y] = sin_v;
        self.mat[MSCALE_Y] = cos_v;
        self.mat[MTRANS_Y] = 0.0;
        self.mat[MPERSP_0] = 0.0;
        self.mat[MPERSP_1] = 0.0;
        self.mat[MPERSP_2] = 1.0;
        self.type_mask = UNKNOWN_MASK | ONLY_PERSPECTIVE_VALID_MASK;
    }

    pub fn set_rsxform(&mut self, xform: RSXform) {
        self.mat[MSCALE_X] = xform.scos;
        self.mat[MSKEW_X] = -xform.ssin;
        self.mat[MTRANS_X] = xform.tx;
        self.mat[MSKEW_Y] = xform.ssin;
        self.mat[MSCALE_Y] = xform.scos;
        self.mat[MTRANS_Y] = xform.ty;
        self.mat[MPERSP_0] = 0.0;
        self.mat[MPERSP_1] = 0.0;
        self.mat[MPERSP_2] = 1.0;
        self.type_mask = UNKNOWN_MASK | ONLY_PERSPECTIVE_VALID_MASK;
    }

    pub fn set_rotate_pivot(&mut self, degrees: f32, px: f32, py: f32) {
        let rad = degrees * (SK_SCALAR_PI / 180.0);
        self.set_sin_cos_pivot(sin_snap_to_zero(rad), cos_snap_to_zero(rad), px, py);
    }

    pub fn set_rotate(&mut self, degrees: f32) {
        let rad = degrees * (SK_SCALAR_PI / 180.0);
        self.set_sin_cos(sin_snap_to_zero(rad), cos_snap_to_zero(rad));
    }

    pub fn pre_rotate_pivot(&mut self, degrees: f32, px: f32, py: f32) {
        let mut m = Self::identity();
        m.set_rotate_pivot(degrees, px, py);
        self.pre_concat(&m);
    }

    pub fn pre_rotate(&mut self, degrees: f32) {
        let mut m = Self::identity();
        m.set_rotate(degrees);
        self.pre_concat(&m);
    }

    pub fn post_rotate_pivot(&mut self, degrees: f32, px: f32, py: f32) {
        let mut m = Self::identity();
        m.set_rotate_pivot(degrees, px, py);
        self.post_concat(&m);
    }

    pub fn post_rotate(&mut self, degrees: f32) {
        let mut m = Self::identity();
        m.set_rotate(degrees);
        self.post_concat(&m);
    }

    pub fn set_skew_pivot(&mut self, sx: f32, sy: f32, px: f32, py: f32) {
        *self = Self::new_with_mask(
            1.0, sx, -sx * py,
            sy, 1.0, -sy * px,
            0.0, 0.0, 1.0,
            UNKNOWN_MASK | ONLY_PERSPECTIVE_VALID_MASK,
        );
    }

    pub fn set_skew(&mut self, sx: f32, sy: f32) {
        self.mat[MSCALE_X] = 1.0;
        self.mat[MSKEW_X] = sx;
        self.mat[MTRANS_X] = 0.0;
        self.mat[MSKEW_Y] = sy;
        self.mat[MSCALE_Y] = 1.0;
        self.mat[MTRANS_Y] = 0.0;
        self.mat[MPERSP_0] = 0.0;
        self.mat[MPERSP_1] = 0.0;
        self.mat[MPERSP_2] = 1.0;
        self.type_mask = UNKNOWN_MASK | ONLY_PERSPECTIVE_VALID_MASK;
    }

    pub fn pre_skew_pivot(&mut self, sx: f32, sy: f32, px: f32, py: f32) {
        let mut m = Self::identity();
        m.set_skew_pivot(sx, sy, px, py);
        self.pre_concat(&m);
    }

    pub fn pre_skew(&mut self, sx: f32, sy: f32) {
        let mut m = Self::identity();
        m.set_skew(sx, sy);
        self.pre_concat(&m);
    }

    pub fn post_skew_pivot(&mut self, sx: f32, sy: f32, px: f32, py: f32) {
        let mut m = Self::identity();
        m.set_skew_pivot(sx, sy, px, py);
        self.post_concat(&m);
    }

    pub fn post_skew(&mut self, sx: f32, sy: f32) {
        let mut m = Self::identity();
        m.set_skew(sx, sy);
        self.post_concat(&m);
    }

    pub fn rect_to_rect(src: Rect, dst: Rect, stf: ScaleToFit) -> Option<Self> {
        if src.is_empty() {
            return None;
        }
        let mut sx = dst.width() / src.width();
        let mut sy = dst.height() / src.height();
        let mut x_larger = false;

        if stf != ScaleToFit::Fill {
            if sx > sy {
                x_larger = true;
                sx = sy;
            } else {
                sy = sx;
            }
        }

        let mut tx = dst.left - src.left * sx;
        let mut ty = dst.top - src.top * sy;
        if stf == ScaleToFit::Center || stf == ScaleToFit::End {
            let mut diff = if x_larger {
                dst.width() - src.width() * sy
            } else {
                dst.height() - src.height() * sy
            };
            if stf == ScaleToFit::Center {
                diff /= 2.0;
            }
            if x_larger {
                tx += diff;
            } else {
                ty += diff;
            }
        }
        Some(Self::scale_translate(sx, sy, tx, ty))
    }

    pub fn set_concat(&mut self, a: &Self, b: &Self) {
        let a_type = a.get_type_cached();
        let b_type = b.get_type_cached();

        if a.is_trivially_identity() {
            *self = *b;
            self.type_mask = b.compute_type_mask() as i32;
            // Preserve exact SkMatrix behavior: *this = b (after b.getType() already cached b's type)
            return;
        }
        if b.is_trivially_identity() {
            *self = *a;
            self.type_mask = a.compute_type_mask() as i32;
            return;
        }
        if only_scale_and_translate(a_type | b_type) {
            *self = Self::scale_translate(
                a.mat[MSCALE_X] * b.mat[MSCALE_X],
                a.mat[MSCALE_Y] * b.mat[MSCALE_Y],
                a.mat[MSCALE_X] * b.mat[MTRANS_X] + a.mat[MTRANS_X],
                a.mat[MSCALE_Y] * b.mat[MTRANS_Y] + a.mat[MTRANS_Y],
            );
            return;
        }
        let mut tmp = Self::identity();
        if ((a_type | b_type) & PERSPECTIVE_MASK) != 0 {
            tmp.mat[MSCALE_X] = rowcol3(&a.mat[0..], &b.mat[0..]);
            tmp.mat[MSKEW_X] = rowcol3(&a.mat[0..], &b.mat[1..]);
            tmp.mat[MTRANS_X] = rowcol3(&a.mat[0..], &b.mat[2..]);
            tmp.mat[MSKEW_Y] = rowcol3(&a.mat[3..], &b.mat[0..]);
            tmp.mat[MSCALE_Y] = rowcol3(&a.mat[3..], &b.mat[1..]);
            tmp.mat[MTRANS_Y] = rowcol3(&a.mat[3..], &b.mat[2..]);
            tmp.mat[MPERSP_0] = rowcol3(&a.mat[6..], &b.mat[0..]);
            tmp.mat[MPERSP_1] = rowcol3(&a.mat[6..], &b.mat[1..]);
            tmp.mat[MPERSP_2] = rowcol3(&a.mat[6..], &b.mat[2..]);
            tmp.type_mask = UNKNOWN_MASK;
        } else {
            tmp.mat[MSCALE_X] =
                muladdmul(a.mat[MSCALE_X], b.mat[MSCALE_X], a.mat[MSKEW_X], b.mat[MSKEW_Y]);
            tmp.mat[MSKEW_X] =
                muladdmul(a.mat[MSCALE_X], b.mat[MSKEW_X], a.mat[MSKEW_X], b.mat[MSCALE_Y]);
            tmp.mat[MTRANS_X] =
                muladdmul(a.mat[MSCALE_X], b.mat[MTRANS_X], a.mat[MSKEW_X], b.mat[MTRANS_Y])
                    + a.mat[MTRANS_X];
            tmp.mat[MSKEW_Y] =
                muladdmul(a.mat[MSKEW_Y], b.mat[MSCALE_X], a.mat[MSCALE_Y], b.mat[MSKEW_Y]);
            tmp.mat[MSCALE_Y] =
                muladdmul(a.mat[MSKEW_Y], b.mat[MSKEW_X], a.mat[MSCALE_Y], b.mat[MSCALE_Y]);
            tmp.mat[MTRANS_Y] =
                muladdmul(a.mat[MSKEW_Y], b.mat[MTRANS_X], a.mat[MSCALE_Y], b.mat[MTRANS_Y])
                    + a.mat[MTRANS_Y];
            tmp.mat[MPERSP_0] = 0.0;
            tmp.mat[MPERSP_1] = 0.0;
            tmp.mat[MPERSP_2] = 1.0;
            tmp.type_mask = UNKNOWN_MASK | ONLY_PERSPECTIVE_VALID_MASK;
        }
        *self = tmp;
    }

    pub fn pre_concat(&mut self, mat: &Self) {
        if !mat.is_identity() {
            let a = *self;
            let b = *mat;
            self.set_concat(&a, &b);
        }
    }

    pub fn post_concat(&mut self, mat: &Self) {
        if !mat.is_identity() {
            let a = *mat;
            let b = *self;
            self.set_concat(&a, &b);
        }
    }

    pub fn as_affine(&self, affine: Option<&mut [f32; 6]>) -> bool {
        if self.has_perspective() {
            return false;
        }
        if let Some(a) = affine {
            a[ASCALE_X] = self.mat[MSCALE_X];
            a[ASKEW_Y] = self.mat[MSKEW_Y];
            a[ASKEW_X] = self.mat[MSKEW_X];
            a[ASCALE_Y] = self.mat[MSCALE_Y];
            a[ATRANS_X] = self.mat[MTRANS_X];
            a[ATRANS_Y] = self.mat[MTRANS_Y];
        }
        true
    }

    pub fn compute_inv(dst: &mut [f32; 9], src: &[f32; 9], inv_det: f64, is_persp: bool) {
        if is_persp {
            dst[MSCALE_X] =
                scross_dscale(src[MSCALE_Y], src[MPERSP_2], src[MTRANS_Y], src[MPERSP_1], inv_det);
            dst[MSKEW_X] =
                scross_dscale(src[MTRANS_X], src[MPERSP_1], src[MSKEW_X], src[MPERSP_2], inv_det);
            dst[MTRANS_X] =
                scross_dscale(src[MSKEW_X], src[MTRANS_Y], src[MTRANS_X], src[MSCALE_Y], inv_det);

            dst[MSKEW_Y] =
                scross_dscale(src[MTRANS_Y], src[MPERSP_0], src[MSKEW_Y], src[MPERSP_2], inv_det);
            dst[MSCALE_Y] =
                scross_dscale(src[MSCALE_X], src[MPERSP_2], src[MTRANS_X], src[MPERSP_0], inv_det);
            dst[MTRANS_Y] =
                scross_dscale(src[MTRANS_X], src[MSKEW_Y], src[MSCALE_X], src[MTRANS_Y], inv_det);

            dst[MPERSP_0] =
                scross_dscale(src[MSKEW_Y], src[MPERSP_1], src[MSCALE_Y], src[MPERSP_0], inv_det);
            dst[MPERSP_1] =
                scross_dscale(src[MSKEW_X], src[MPERSP_0], src[MSCALE_X], src[MPERSP_1], inv_det);
            dst[MPERSP_2] =
                scross_dscale(src[MSCALE_X], src[MSCALE_Y], src[MSKEW_X], src[MSKEW_Y], inv_det);
        } else {
            dst[MSCALE_X] = ((src[MSCALE_Y] as f64) * inv_det) as f32;
            dst[MSKEW_X] = (-(src[MSKEW_X] as f64) * inv_det) as f32;
            dst[MTRANS_X] = dcross_dscale(
                src[MSKEW_X] as f64,
                src[MTRANS_Y] as f64,
                src[MSCALE_Y] as f64,
                src[MTRANS_X] as f64,
                inv_det,
            );
            dst[MSKEW_Y] = (-(src[MSKEW_Y] as f64) * inv_det) as f32;
            dst[MSCALE_Y] = ((src[MSCALE_X] as f64) * inv_det) as f32;
            dst[MTRANS_Y] = dcross_dscale(
                src[MSKEW_Y] as f64,
                src[MTRANS_X] as f64,
                src[MSCALE_X] as f64,
                src[MTRANS_Y] as f64,
                inv_det,
            );
            dst[MPERSP_0] = 0.0;
            dst[MPERSP_1] = 0.0;
            dst[MPERSP_2] = 1.0;
        }
    }

    pub fn invert(&self) -> Option<Self> {
        let full_mask = if (self.type_mask & UNKNOWN_MASK) != 0 {
            self.compute_type_mask() as i32
        } else {
            self.type_mask
        };
        let mask = full_mask & 0xF;

        if mask == IDENTITY_MASK {
            let mut copy = *self;
            copy.type_mask = full_mask;
            return Some(copy);
        }

        if (mask & !(SCALE_MASK | TRANSLATE_MASK)) == 0 {
            if (mask & SCALE_MASK) != 0 {
                let inv_sx = 1.0_f32 / self.mat[MSCALE_X];
                let inv_sy = 1.0_f32 / self.mat[MSCALE_Y];
                if !inv_sx.is_finite() || !inv_sy.is_finite() {
                    return None;
                }
                let inv_tx = -self.mat[MTRANS_X] * inv_sx;
                let inv_ty = -self.mat[MTRANS_Y] * inv_sy;
                if !inv_tx.is_finite() || !inv_ty.is_finite() {
                    return None;
                }
                return Some(Self::new_with_mask(
                    inv_sx, 0.0, inv_tx,
                    0.0, inv_sy, inv_ty,
                    0.0, 0.0, 1.0,
                    mask | RECT_STAYS_RECT_MASK,
                ));
            }
            if !self.mat[MTRANS_X].is_finite() || !self.mat[MTRANS_Y].is_finite() {
                return None;
            }
            return Some(Self::translate(-self.mat[MTRANS_X], -self.mat[MTRANS_Y]));
        }

        let is_persp = (mask & PERSPECTIVE_MASK) != 0;
        let inv_det = sk_inv_determinant(&self.mat, is_persp);
        if inv_det == 0.0 {
            return None;
        }
        let mut inv = Self::identity();
        Self::compute_inv(&mut inv.mat, &self.mat, inv_det, is_persp);
        if !inv.is_finite() {
            return None;
        }
        inv.type_mask = full_mask;
        Some(inv)
    }

    #[inline]
    pub fn map_point_perspective(&self, p: Point) -> Point {
        let x = sdot2(p.x, self.mat[MSCALE_X], p.y, self.mat[MSKEW_X]) + self.mat[MTRANS_X];
        let y = sdot2(p.x, self.mat[MSKEW_Y], p.y, self.mat[MSCALE_Y]) + self.mat[MTRANS_Y];
        let mut z = sdot2(p.x, self.mat[MPERSP_0], p.y, self.mat[MPERSP_1]) + self.mat[MPERSP_2];
        if z != 0.0 {
            z = 1.0 / z;
        }
        Point::new(x * z, y * z)
    }

    pub fn identity_pts(dst: &mut [Point], src: &[Point]) {
        let n = dst.len().min(src.len());
        dst[..n].copy_from_slice(&src[..n]);
    }

    pub fn trans_pts(&self, dst: &mut [Point], src: &[Point]) {
        let n = dst.len().min(src.len());
        let tx = self.mat[MTRANS_X];
        let ty = self.mat[MTRANS_Y];
        for i in 0..n {
            dst[i] = Point::new(src[i].x + tx, src[i].y + ty);
        }
    }

    pub fn scale_pts(&self, dst: &mut [Point], src: &[Point]) {
        let n = dst.len().min(src.len());
        let tx = self.mat[MTRANS_X];
        let ty = self.mat[MTRANS_Y];
        let sx = self.mat[MSCALE_X];
        let sy = self.mat[MSCALE_Y];
        for i in 0..n {
            dst[i] = Point::new(src[i].x * sx + tx, src[i].y * sy + ty);
        }
    }

    pub fn affine_vpts(&self, dst: &mut [Point], src: &[Point]) {
        let n = dst.len().min(src.len());
        let tx = self.mat[MTRANS_X];
        let ty = self.mat[MTRANS_Y];
        let sx = self.mat[MSCALE_X];
        let sy = self.mat[MSCALE_Y];
        let kx = self.mat[MSKEW_X];
        let ky = self.mat[MSKEW_Y];
        for i in 0..n {
            let x = src[i].x;
            let y = src[i].y;
            dst[i] = Point::new(x * sx + y * kx + tx, y * sy + x * ky + ty);
        }
    }

    pub fn persp_pts(&self, dst: &mut [Point], src: &[Point]) {
        let n = dst.len().min(src.len());
        for i in 0..n {
            let sx = src[i].x;
            let sy = src[i].y;
            let x = sdot2(sx, self.mat[MSCALE_X], sy, self.mat[MSKEW_X]) + self.mat[MTRANS_X];
            let y = sdot2(sx, self.mat[MSKEW_Y], sy, self.mat[MSCALE_Y]) + self.mat[MTRANS_Y];
            let mut z =
                sdot2(sx, self.mat[MPERSP_0], sy, self.mat[MPERSP_1]) + self.mat[MPERSP_2];
            if z != 0.0 {
                z = 1.0 / z;
            }
            dst[i] = Point::new(x * z, y * z);
        }
    }

    pub fn map_points(&self, dst: &mut [Point], src: &[Point]) {
        let mask = self.get_type_cached();
        if (mask & PERSPECTIVE_MASK) != 0 {
            self.persp_pts(dst, src);
        } else if (mask & AFFINE_MASK) != 0 {
            self.affine_vpts(dst, src);
        } else if (mask & SCALE_MASK) != 0 {
            self.scale_pts(dst, src);
        } else if (mask & TRANSLATE_MASK) != 0 {
            self.trans_pts(dst, src);
        } else {
            Self::identity_pts(dst, src);
        }
    }

    pub fn map_points_in_place(&self, pts: &mut [Point]) {
        let mask = self.get_type_cached();
        if (mask & PERSPECTIVE_MASK) != 0 {
            for p in pts.iter_mut() {
                *p = self.map_point_perspective(*p);
            }
        } else if (mask & AFFINE_MASK) != 0 {
            let tx = self.mat[MTRANS_X];
            let ty = self.mat[MTRANS_Y];
            let sx = self.mat[MSCALE_X];
            let sy = self.mat[MSCALE_Y];
            let kx = self.mat[MSKEW_X];
            let ky = self.mat[MSKEW_Y];
            for p in pts.iter_mut() {
                let x = p.x;
                let y = p.y;
                p.x = x * sx + y * kx + tx;
                p.y = y * sy + x * ky + ty;
            }
        } else if (mask & SCALE_MASK) != 0 {
            let tx = self.mat[MTRANS_X];
            let ty = self.mat[MTRANS_Y];
            let sx = self.mat[MSCALE_X];
            let sy = self.mat[MSCALE_Y];
            for p in pts.iter_mut() {
                p.x = p.x * sx + tx;
                p.y = p.y * sy + ty;
            }
        } else if (mask & TRANSLATE_MASK) != 0 {
            let tx = self.mat[MTRANS_X];
            let ty = self.mat[MTRANS_Y];
            for p in pts.iter_mut() {
                p.x += tx;
                p.y += ty;
            }
        }
    }

    pub fn map_homogeneous_point(&self, src: Point3) -> Point3 {
        let x = sdot3(
            src.x, self.mat[MSCALE_X],
            src.y, self.mat[MSKEW_X],
            src.z, self.mat[MTRANS_X],
        );
        let y = sdot3(
            src.x, self.mat[MSKEW_Y],
            src.y, self.mat[MSCALE_Y],
            src.z, self.mat[MTRANS_Y],
        );
        let w = sdot3(
            src.x, self.mat[MPERSP_0],
            src.y, self.mat[MPERSP_1],
            src.z, self.mat[MPERSP_2],
        );
        Point3::new(x, y, w)
    }

    pub fn map_points_to_homogeneous(&self, dst: &mut [Point3], src: &[Point]) {
        let count = dst.len().min(src.len());
        if self.is_identity() {
            for i in 0..count {
                dst[i] = Point3::new(src[i].x, src[i].y, 1.0);
            }
        } else if self.has_perspective() {
            for i in 0..count {
                dst[i] = Point3::new(
                    self.mat[0] * src[i].x + self.mat[1] * src[i].y + self.mat[2],
                    self.mat[3] * src[i].x + self.mat[4] * src[i].y + self.mat[5],
                    self.mat[6] * src[i].x + self.mat[7] * src[i].y + self.mat[8],
                );
            }
        } else {
            for i in 0..count {
                dst[i] = Point3::new(
                    self.mat[0] * src[i].x + self.mat[1] * src[i].y + self.mat[2],
                    self.mat[3] * src[i].x + self.mat[4] * src[i].y + self.mat[5],
                    1.0,
                );
            }
        }
    }

    pub fn map_vectors(&self, dst: &mut [Point], src: &[Point]) {
        let count = dst.len().min(src.len());
        if self.has_perspective() {
            let origin = self.map_point_perspective(Point::ZERO);
            for i in (0..count).rev() {
                dst[i] = self.map_point_perspective(src[i]) - origin;
            }
        } else {
            let mut tmp = *self;
            tmp.mat[MTRANS_X] = 0.0;
            tmp.mat[MTRANS_Y] = 0.0;
            tmp.type_mask &= !TRANSLATE_MASK;
            tmp.map_points(&mut dst[..count], &src[..count]);
        }
    }

    pub fn map_vectors_in_place(&self, vecs: &mut [Point]) {
        if self.has_perspective() {
            let origin = self.map_point_perspective(Point::ZERO);
            for v in vecs.iter_mut().rev() {
                *v = self.map_point_perspective(*v) - origin;
            }
        } else {
            let mut tmp = *self;
            tmp.mat[MTRANS_X] = 0.0;
            tmp.mat[MTRANS_Y] = 0.0;
            tmp.type_mask &= !TRANSLATE_MASK;
            tmp.map_points_in_place(vecs);
        }
    }

    pub fn map_rect_scale_translate(&self, src: Rect) -> Rect {
        let sx = self.mat[MSCALE_X];
        let sy = self.mat[MSCALE_Y];
        let tx = self.mat[MTRANS_X];
        let ty = self.mat[MTRANS_Y];
        sort_as_rect([
            src.left * sx + tx,
            src.top * sy + ty,
            src.right * sx + tx,
            src.bottom * sy + ty,
        ])
    }

    /// Maps `src` rect for non-perspective matrices; returns `None` if perspective
    /// (caller handles perspective via path builder).
    pub fn map_rect_non_perspective(&self, src: Rect) -> Option<(Rect, bool)> {
        if self.get_type_cached() <= TRANSLATE_MASK {
            let tx = self.mat[MTRANS_X];
            let ty = self.mat[MTRANS_Y];
            let dst = sort_as_rect([
                src.left + tx,
                src.top + ty,
                src.right + tx,
                src.bottom + ty,
            ]);
            return Some((dst, true));
        }
        if self.is_scale_translate() {
            return Some((self.map_rect_scale_translate(src), true));
        }
        if self.has_perspective() {
            return None;
        }
        let mut quad = src.to_quad();
        self.map_points_in_place(&mut quad);
        let mut dst = Rect::make_empty();
        dst.set_bounds_no_check(&quad);
        Some((dst, self.rect_stays_rect()))
    }

    pub fn map_radius(&self, radius: f32) -> f32 {
        let mut vec = [Point::new(radius, 0.0), Point::new(0.0, radius)];
        self.map_vectors_in_place(&mut vec);
        let d0 = vec[0].mag();
        let d1 = vec[1].mag();
        (d0 * d1).sqrt()
    }

    fn poly2_proc(src: &[Point]) -> Option<Self> {
        Some(Self::make_all(
            src[1].y - src[0].y,
            src[1].x - src[0].x,
            src[0].x,
            src[0].x - src[1].x,
            src[1].y - src[0].y,
            src[0].y,
            0.0,
            0.0,
            1.0,
        ))
    }

    fn poly3_proc(src: &[Point]) -> Option<Self> {
        Some(Self::make_all(
            src[2].x - src[0].x,
            src[1].x - src[0].x,
            src[0].x,
            src[2].y - src[0].y,
            src[1].y - src[0].y,
            src[0].y,
            0.0,
            0.0,
            1.0,
        ))
    }

    fn poly4_proc(src: &[Point]) -> Option<Self> {
        let x0 = src[2].x - src[0].x;
        let y0 = src[2].y - src[0].y;
        let x1 = src[2].x - src[1].x;
        let y1 = src[2].y - src[1].y;
        let x2 = src[2].x - src[3].x;
        let y2 = src[2].y - src[3].y;

        let cond2 = if x2 > 0.0 {
            if y2 > 0.0 { x2 > y2 } else { x2 > -y2 }
        } else if y2 > 0.0 {
            -x2 > y2
        } else {
            x2 < y2
        };
        let a1 = if cond2 {
            let denom = (x1 * y2) / x2 - y1;
            if check_for_zero(denom) {
                return None;
            }
            (((x0 - x1) * y2 / x2) - y0 + y1) / denom
        } else {
            let denom = x1 - (y1 * x2) / y2;
            if check_for_zero(denom) {
                return None;
            }
            (x0 - x1 - ((y0 - y1) * x2) / y2) / denom
        };

        let cond1 = if x1 > 0.0 {
            if y1 > 0.0 { x1 > y1 } else { x1 > -y1 }
        } else if y1 > 0.0 {
            -x1 > y1
        } else {
            x1 < y1
        };
        let a2 = if cond1 {
            let denom = y2 - (x2 * y1) / x1;
            if check_for_zero(denom) {
                return None;
            }
            (y0 - y2 - ((x0 - x2) * y1) / x1) / denom
        } else {
            let denom = (y2 * x1) / y1 - x2;
            if check_for_zero(denom) {
                return None;
            }
            (((y0 - y2) * x1) / y1 - x0 + x2) / denom
        };

        Some(Self::make_all(
            a2 * src[3].x + src[3].x - src[0].x,
            a1 * src[1].x + src[1].x - src[0].x,
            src[0].x,
            a2 * src[3].y + src[3].y - src[0].y,
            a1 * src[1].y + src[1].y - src[0].y,
            src[0].y,
            a2,
            a1,
            1.0,
        ))
    }

    pub fn poly_to_poly(src: &[Point], dst: &[Point]) -> Option<Self> {
        if src.len() != dst.len() || src.len() > 4 {
            return None;
        }
        match src.len() {
            0 => Some(Self::identity()),
            1 => Some(Self::translate(dst[0].x - src[0].x, dst[0].y - src[0].y)),
            2..=4 => {
                let proc: fn(&[Point]) -> Option<Self> = match src.len() {
                    2 => Self::poly2_proc,
                    3 => Self::poly3_proc,
                    _ => Self::poly4_proc,
                };
                let temp_src = proc(src)?;
                let inv = temp_src.invert()?;
                let temp_dst = proc(dst)?;
                let mut res = Self::identity();
                res.set_concat(&temp_dst, &inv);
                Some(res)
            }
            _ => None,
        }
    }

    pub fn get_min_max_scales(&self) -> Option<[f32; 2]> {
        let type_mask = self.get_type_cached();
        if (type_mask & PERSPECTIVE_MASK) != 0 {
            return None;
        }
        if type_mask == IDENTITY_MASK {
            return Some([1.0, 1.0]);
        }
        if (type_mask & AFFINE_MASK) == 0 {
            let mut s0 = self.mat[MSCALE_X].abs();
            let mut s1 = self.mat[MSCALE_Y].abs();
            if s0 > s1 {
                std::mem::swap(&mut s0, &mut s1);
            }
            return Some([s0, s1]);
        }
        let m = &self.mat;
        let a = sdot2(m[MSCALE_X], m[MSCALE_X], m[MSKEW_Y], m[MSKEW_Y]);
        let b = sdot2(m[MSCALE_X], m[MSKEW_X], m[MSCALE_Y], m[MSKEW_Y]);
        let c = sdot2(m[MSKEW_X], m[MSKEW_X], m[MSCALE_Y], m[MSCALE_Y]);
        let b_sqd = b * b;
        let (mut r0, mut r1) = if b_sqd <= SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO {
            if a > c { (c, a) } else { (a, c) }
        } else {
            let a_minus_c = a - c;
            let a_plus_c_div2 = (a + c) / 2.0;
            let x = (a_minus_c * a_minus_c + 4.0 * b_sqd).sqrt() / 2.0;
            (a_plus_c_div2 - x, a_plus_c_div2 + x)
        };
        if !r0.is_finite() || !r1.is_finite() {
            return None;
        }
        if r0 < 0.0 {
            r0 = 0.0;
        }
        if r1 < 0.0 {
            r1 = 0.0;
        }
        Some([r0.sqrt(), r1.sqrt()])
    }

    pub fn get_min_scale(&self) -> f32 {
        let type_mask = self.get_type_cached();
        if (type_mask & PERSPECTIVE_MASK) != 0 {
            return -1.0;
        }
        if type_mask == IDENTITY_MASK {
            return 1.0;
        }
        if (type_mask & AFFINE_MASK) == 0 {
            return cpp_min_f32(self.mat[MSCALE_X].abs(), self.mat[MSCALE_Y].abs());
        }
        let m = &self.mat;
        let a = sdot2(m[MSCALE_X], m[MSCALE_X], m[MSKEW_Y], m[MSKEW_Y]);
        let b = sdot2(m[MSCALE_X], m[MSKEW_X], m[MSCALE_Y], m[MSKEW_Y]);
        let c = sdot2(m[MSKEW_X], m[MSKEW_X], m[MSCALE_Y], m[MSCALE_Y]);
        let b_sqd = b * b;
        let mut r0 = if b_sqd <= SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO {
            cpp_min_f32(a, c)
        } else {
            let a_minus_c = a - c;
            let a_plus_c_div2 = (a + c) / 2.0;
            let x = (a_minus_c * a_minus_c + 4.0 * b_sqd).sqrt() / 2.0;
            a_plus_c_div2 - x
        };
        if !r0.is_finite() {
            return -1.0;
        }
        if r0 < 0.0 {
            r0 = 0.0;
        }
        r0.sqrt()
    }

    pub fn get_max_scale(&self) -> f32 {
        let type_mask = self.get_type_cached();
        if (type_mask & PERSPECTIVE_MASK) != 0 {
            return -1.0;
        }
        if type_mask == IDENTITY_MASK {
            return 1.0;
        }
        if (type_mask & AFFINE_MASK) == 0 {
            return cpp_max_f32(self.mat[MSCALE_X].abs(), self.mat[MSCALE_Y].abs());
        }
        let m = &self.mat;
        let a = sdot2(m[MSCALE_X], m[MSCALE_X], m[MSKEW_Y], m[MSKEW_Y]);
        let b = sdot2(m[MSCALE_X], m[MSKEW_X], m[MSCALE_Y], m[MSKEW_Y]);
        let c = sdot2(m[MSKEW_X], m[MSKEW_X], m[MSCALE_Y], m[MSCALE_Y]);
        let b_sqd = b * b;
        let mut r0 = if b_sqd <= SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO {
            cpp_max_f32(a, c)
        } else {
            let a_minus_c = a - c;
            let a_plus_c_div2 = (a + c) / 2.0;
            let x = (a_minus_c * a_minus_c + 4.0 * b_sqd).sqrt() / 2.0;
            a_plus_c_div2 + x
        };
        if !r0.is_finite() {
            return -1.0;
        }
        if r0 < 0.0 {
            r0 = 0.0;
        }
        r0.sqrt()
    }

    pub fn decompose_scale(&self) -> Option<(Size, Self)> {
        if self.has_perspective() {
            return None;
        }
        let sx = Point::length(self.mat[MSCALE_X], self.mat[MSKEW_Y]);
        let sy = Point::length(self.mat[MSKEW_X], self.mat[MSCALE_Y]);
        if !sx.is_finite()
            || !sy.is_finite()
            || scalar_nearly_zero(sx, SK_SCALAR_NEARLY_ZERO)
            || scalar_nearly_zero(sy, SK_SCALAR_NEARLY_ZERO)
        {
            return None;
        }
        let mut remaining = *self;
        remaining.pre_scale(1.0 / sx, 1.0 / sy);
        Some((Size { width: sx, height: sy }, remaining))
    }

    pub fn decompose_upper_2x2(&self) -> Option<(Point, Point, Point)> {
        let a = self.mat[MSCALE_X];
        let b = self.mat[MSKEW_X];
        let c = self.mat[MSKEW_Y];
        let d = self.mat[MSCALE_Y];

        if is_degenerate_2x2(a, b, c, d) {
            return None;
        }

        let (mut cos_q, mut sin_q, sa, sb, sd);
        if scalar_nearly_equal(b, c, SK_SCALAR_NEARLY_ZERO) {
            cos_q = 1.0_f32;
            sin_q = 0.0_f32;
            sa = a as f64;
            sb = b as f64;
            sd = d as f64;
        } else {
            cos_q = a + d;
            sin_q = c - b;
            let reciplen = 1.0_f32 / (cos_q * cos_q + sin_q * sin_q).sqrt();
            cos_q *= reciplen;
            sin_q *= reciplen;

            sa = (a * cos_q + c * sin_q) as f64;
            sb = (b * cos_q + d * sin_q) as f64;
            sd = (-b * sin_q + d * cos_q) as f64;
        }

        let (w1, w2, mut cos1, mut sin1, cos2, sin2);
        if scalar_nearly_zero(sb as f32, SK_SCALAR_NEARLY_ZERO) {
            cos1 = 1.0_f32;
            sin1 = 0.0_f32;
            w1 = sa;
            w2 = sd;
            cos2 = cos_q;
            sin2 = sin_q;
        } else {
            let diff = sa - sd;
            let discriminant = (diff * diff + 4.0 * sb * sb).sqrt();
            let trace = sa + sd;
            if diff > 0.0 {
                w1 = 0.5 * (trace + discriminant);
                w2 = 0.5 * (trace - discriminant);
            } else {
                w1 = 0.5 * (trace - discriminant);
                w2 = 0.5 * (trace + discriminant);
            }

            cos1 = sb as f32;
            sin1 = (w1 - sa) as f32;
            let reciplen = 1.0_f32 / (cos1 * cos1 + sin1 * sin1).sqrt();
            cos1 *= reciplen;
            sin1 *= reciplen;

            cos2 = cos1 * cos_q - sin1 * sin_q;
            sin2 = sin1 * cos_q + cos1 * sin_q;
            sin1 = -sin1;
        }

        Some((
            Point::new(cos1, sin1),
            Point::new(w1 as f32, w2 as f32),
            Point::new(cos2, sin2),
        ))
    }

    pub fn treat_as_sprite(
        &self,
        size: ISize,
        no_change_with_identity: bool,
        is_linear_filter: bool,
        is_anti_alias: bool,
    ) -> bool {
        if !no_change_with_identity {
            return false;
        }
        const ANTI_ALIAS_SUBPIXEL_BITS: u32 = 4;
        let subpixel_bits = if is_anti_alias {
            ANTI_ALIAS_SUBPIXEL_BITS
        } else {
            0
        };
        let ty = self.get_type_cached();
        if (ty & !(SCALE_MASK | TRANSLATE_MASK)) != 0 {
            return false;
        }
        if is_linear_filter
            && (self.mat[MTRANS_X] != (self.mat[MTRANS_X] as i32 as f32)
                || self.mat[MTRANS_Y] != (self.mat[MTRANS_Y] as i32 as f32))
        {
            return false;
        }
        if subpixel_bits == 0 && (ty & !TRANSLATE_MASK) == 0 {
            return true;
        }
        if self.mat[MSCALE_X] < 0.0 || self.mat[MSCALE_Y] < 0.0 {
            return false;
        }

        let mut isrc = IRect::make_ltrb(0, 0, size.width, size.height);
        let src = Rect::from_irect(isrc);
        let mut dst = self.map_rect_scale_translate(src);

        let tx_i = scalar_round_to_int(self.mat[MTRANS_X]);
        let ty_i = scalar_round_to_int(self.mat[MTRANS_Y]);
        isrc.left = isrc.left.wrapping_add(tx_i);
        isrc.top = isrc.top.wrapping_add(ty_i);
        isrc.right = isrc.right.wrapping_add(tx_i);
        isrc.bottom = isrc.bottom.wrapping_add(ty_i);

        if subpixel_bits != 0 {
            isrc.left = ((isrc.left as u32).wrapping_shl(subpixel_bits)) as i32;
            isrc.top = ((isrc.top as u32).wrapping_shl(subpixel_bits)) as i32;
            isrc.right = ((isrc.right as u32).wrapping_shl(subpixel_bits)) as i32;
            isrc.bottom = ((isrc.bottom as u32).wrapping_shl(subpixel_bits)) as i32;

            let scale = (1_u32 << subpixel_bits) as f32;
            dst.left *= scale;
            dst.top *= scale;
            dst.right *= scale;
            dst.bottom *= scale;
        }

        let idst = IRect::make_ltrb(
            scalar_round_to_int(dst.left),
            scalar_round_to_int(dst.top),
            scalar_round_to_int(dst.right),
            scalar_round_to_int(dst.bottom),
        );
        isrc == idst
    }

    pub fn differential_area_scale(&self, p: Point) -> f32 {
        let mut xyw = [Point3::default()];
        self.map_points_to_homogeneous(&mut xyw, &[p]);
        let xyw = xyw[0];
        if xyw.z < SK_SCALAR_NEARLY_ZERO {
            return f32::INFINITY;
        }
        let jac = [
            xyw.x, xyw.y, xyw.z,
            self.mat[MSCALE_X], self.mat[MSKEW_Y], self.mat[MPERSP_0],
            self.mat[MSKEW_X], self.mat[MSCALE_Y], self.mat[MPERSP_1],
        ];
        let mut denom = 1.0_f64 / (xyw.z as f64);
        denom = denom * denom * denom;
        ((sk_determinant(&jac, true) * denom) as f32).abs()
    }

    pub fn nearly_affine(&self, bounds: Rect, mut tolerance: f32) -> bool {
        if !self.has_perspective() {
            return true;
        }
        let quad = bounds.to_quad();
        let mut xyw = [Point3::default(); 4];
        self.map_points_to_homogeneous(&mut xyw, &quad);

        let v1 = Point3::new(self.mat[MSCALE_X], self.mat[MSKEW_Y], self.mat[MPERSP_0]);
        let v2 = Point3::new(self.mat[MSKEW_X], self.mat[MSCALE_Y], self.mat[MPERSP_1]);
        let det_cross = v1.cross(v2);

        if xyw[0].z < SK_SCALAR_NEARLY_ZERO {
            return false;
        }
        let mut det = det_cross.dot(xyw[0]) as f64;
        let mut denom = 1.0_f64 / (xyw[0].z as f64);
        denom = denom * denom * denom;
        let a0 = ((det * denom) as f32).abs();

        tolerance *= tolerance;
        for i in 1..4 {
            if xyw[i].z < SK_SCALAR_NEARLY_ZERO {
                return false;
            }
            det = det_cross.dot(xyw[i]) as f64;
            denom = 1.0_f64 / (xyw[i].z as f64);
            denom = denom * denom * denom;
            let a = ((det * denom) as f32).abs();
            if !scalar_nearly_equal(a0, a, tolerance) {
                return false;
            }
        }
        true
    }

    pub fn compute_res_scale_for_stroking(&self) -> f32 {
        let sx = Point::length(self.mat[MSCALE_X], self.mat[MSKEW_Y]);
        let sy = Point::length(self.mat[MSKEW_X], self.mat[MSCALE_Y]);
        if sx.is_finite() && sy.is_finite() {
            let scale = cpp_max_f32(sx, sy);
            if scale > 0.0 {
                return scale;
            }
        }
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_concat_and_invert() {
        let mut m = Matrix::identity();
        m.pre_translate(10.0, 20.0);
        m.pre_scale(2.0, 3.0);
        m.pre_rotate(90.0);
        let inv = m.invert().unwrap();
        let mut id = Matrix::identity();
        id.set_concat(&m, &inv);
        for i in 0..9 {
            let expected = if i == 0 || i == 4 || i == 8 { 1.0 } else { 0.0 };
            assert!((id.mat[i] - expected).abs() < 1e-5, "idx {i}: {}", id.mat[i]);
        }
    }

    #[test]
    fn test_matrix_poly_to_poly() {
        let src = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let dst = [
            Point::new(5.0, 5.0),
            Point::new(25.0, 5.0),
            Point::new(25.0, 35.0),
            Point::new(5.0, 35.0),
        ];
        let m = Matrix::poly_to_poly(&src, &dst).unwrap();
        let mut mapped = [Point::ZERO; 4];
        m.map_points(&mut mapped, &src);
        for i in 0..4 {
            assert!((mapped[i].x - dst[i].x).abs() < 1e-4);
            assert!((mapped[i].y - dst[i].y).abs() < 1e-4);
        }
    }
}
