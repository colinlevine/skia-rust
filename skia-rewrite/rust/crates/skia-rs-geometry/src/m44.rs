#![forbid(unsafe_code)]

use crate::matrix_invert::invert_4x4_matrix;
use crate::{
    cpp_min_f32, scalar_nearly_zero, Rect, SK_SCALAR_NEARLY_ZERO, W0_PLANE_DISTANCE,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct V4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl V3 {
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    #[inline]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    #[inline]
    pub fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    #[inline]
    pub fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    #[inline]
    pub fn scale(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
}

/// Column-major 4x4 matrix matching `SkM44`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct M44 {
    pub mat: [f32; 16],
}

impl Default for M44 {
    fn default() -> Self {
        Self::identity()
    }
}

impl PartialEq for M44 {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        for i in 0..16 {
            if self.mat[i] != other.mat[i] {
                return false;
            }
        }
        true
    }
}

fn transpose_arrays(src: &[f32; 16]) -> [f32; 16] {
    [
        src[0], src[4], src[8], src[12],
        src[1], src[5], src[9], src[13],
        src[2], src[6], src[10], src[14],
        src[3], src[7], src[11], src[15],
    ]
}

impl M44 {
    #[inline]
    pub const fn identity() -> Self {
        Self {
            mat: [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub const fn from_rows_scalar(
        m0: f32, m4: f32, m8: f32, m12: f32,
        m1: f32, m5: f32, m9: f32, m13: f32,
        m2: f32, m6: f32, m10: f32, m14: f32,
        m3: f32, m7: f32, m11: f32, m15: f32,
    ) -> Self {
        Self {
            mat: [
                m0, m1, m2, m3,
                m4, m5, m6, m7,
                m8, m9, m10, m11,
                m12, m13, m14, m15,
            ],
        }
    }

    #[inline]
    pub const fn cols(c0: V4, c1: V4, c2: V4, c3: V4) -> Self {
        Self {
            mat: [
                c0.x, c0.y, c0.z, c0.w,
                c1.x, c1.y, c1.z, c1.w,
                c2.x, c2.y, c2.z, c2.w,
                c3.x, c3.y, c3.z, c3.w,
            ],
        }
    }

    #[inline]
    pub fn rc(&self, r: usize, c: usize) -> f32 {
        self.mat[c * 4 + r]
    }

    #[inline]
    pub fn set_rc(&mut self, r: usize, c: usize, val: f32) {
        self.mat[c * 4 + r] = val;
    }

    #[inline]
    pub fn get_row_major(&self) -> [f32; 16] {
        transpose_arrays(&self.mat)
    }

    pub fn set_concat(&mut self, a: &Self, b: &Self) {
        let mut out = [0.0_f32; 16];
        for col in 0..4 {
            let r0 = b.mat[col * 4];
            let r1 = b.mat[col * 4 + 1];
            let r2 = b.mat[col * 4 + 2];
            let r3 = b.mat[col * 4 + 3];
            for row in 0..4 {
                out[col * 4 + row] = a.mat[row] * r0
                    + (a.mat[4 + row] * r1 + (a.mat[8 + row] * r2 + a.mat[12 + row] * r3));
            }
        }
        self.mat = out;
    }

    pub fn pre_concat_m33(&mut self, b: &[f32; 9]) {
        let c0 = [self.mat[0], self.mat[1], self.mat[2], self.mat[3]];
        let c1 = [self.mat[4], self.mat[5], self.mat[6], self.mat[7]];
        let c3 = [self.mat[12], self.mat[13], self.mat[14], self.mat[15]];

        let compute = |r0: f32, r1: f32, r3: f32| -> [f32; 4] {
            [
                c0[0] * r0 + (c1[0] * r1 + c3[0] * r3),
                c0[1] * r0 + (c1[1] * r1 + c3[1] * r3),
                c0[2] * r0 + (c1[2] * r1 + c3[2] * r3),
                c0[3] * r0 + (c1[3] * r1 + c3[3] * r3),
            ]
        };

        let m0 = compute(b[0], b[3], b[6]);
        let m1 = compute(b[1], b[4], b[7]);
        let m3 = compute(b[2], b[5], b[8]);

        self.mat[0..4].copy_from_slice(&m0);
        self.mat[4..8].copy_from_slice(&m1);
        self.mat[12..16].copy_from_slice(&m3);
    }

    pub fn pre_translate(&mut self, x: f32, y: f32, z: f32) {
        for row in 0..4 {
            self.mat[12 + row] = self.mat[row] * x
                + (self.mat[4 + row] * y + (self.mat[8 + row] * z + self.mat[12 + row]));
        }
    }

    pub fn post_translate(&mut self, x: f32, y: f32, z: f32) {
        let t = [x, y, z, 0.0_f32];
        for col in 0..4 {
            let w = self.mat[col * 4 + 3];
            for row in 0..4 {
                self.mat[col * 4 + row] = t[row] * w + self.mat[col * 4 + row];
            }
        }
    }

    pub fn pre_scale_xy(&mut self, x: f32, y: f32) {
        for row in 0..4 {
            self.mat[row] *= x;
            self.mat[4 + row] *= y;
        }
    }

    pub fn pre_scale_xyz(&mut self, x: f32, y: f32, z: f32) {
        for row in 0..4 {
            self.mat[row] *= x;
            self.mat[4 + row] *= y;
            self.mat[8 + row] *= z;
        }
    }

    pub fn map(&self, x: f32, y: f32, z: f32, w: f32) -> V4 {
        V4 {
            x: self.mat[0] * x + (self.mat[4] * y + (self.mat[8] * z + self.mat[12] * w)),
            y: self.mat[1] * x + (self.mat[5] * y + (self.mat[9] * z + self.mat[13] * w)),
            z: self.mat[2] * x + (self.mat[6] * y + (self.mat[10] * z + self.mat[14] * w)),
            w: self.mat[3] * x + (self.mat[7] * y + (self.mat[11] * z + self.mat[15] * w)),
        }
    }

    pub fn map_rect(&self, src: Rect) -> Rect {
        let has_perspective = self.mat[3] != 0.0
            || self.mat[7] != 0.0
            || self.mat[11] != 0.0
            || self.mat[15] != 1.0;
        if has_perspective {
            map_rect_perspective(src, &self.mat)
        } else {
            map_rect_affine(src, &self.mat)
        }
    }

    pub fn normalize_perspective(&mut self) {
        if self.mat[15] != 1.0
            && self.mat[15] != 0.0
            && self.mat[3] == 0.0
            && self.mat[7] == 0.0
            && self.mat[11] == 0.0
        {
            let inv = (1.0_f64 / self.mat[15] as f64) as f32;
            for v in &mut self.mat {
                *v *= inv;
            }
            self.mat[15] = 1.0;
        }
    }

    pub fn invert(&self) -> Option<Self> {
        let mut tmp = [0.0_f32; 16];
        if invert_4x4_matrix(&self.mat, Some(&mut tmp)) == 0.0 {
            return None;
        }
        Some(Self { mat: tmp })
    }

    pub fn transpose(&self) -> Self {
        Self {
            mat: transpose_arrays(&self.mat),
        }
    }

    pub fn set_rotate_unit_sin_cos(&mut self, axis: V3, sin_angle: f32, cos_angle: f32) {
        let x = axis.x;
        let y = axis.y;
        let z = axis.z;
        let c = cos_angle;
        let s = sin_angle;
        let t = 1.0 - c;

        *self = Self::from_rows_scalar(
            t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0,
            t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0,
            t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0,
            0.0, 0.0, 0.0, 1.0,
        );
    }

    pub fn set_rotate(&mut self, axis: V3, radians: f32) {
        let len = axis.length();
        if len > 0.0 && len.is_finite() {
            let unit = axis.scale(1.0 / len);
            self.set_rotate_unit_sin_cos(unit, radians.sin(), radians.cos());
        } else {
            *self = Self::identity();
        }
    }

    pub fn rect_to_rect(src: Rect, dst: Rect) -> Self {
        if src.is_empty() {
            return Self::identity();
        } else if dst.is_empty() {
            return Self::from_rows_scalar(
                0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            );
        }
        let sx = dst.width() / src.width();
        let sy = dst.height() / src.height();
        let tx = dst.left - sx * src.left;
        let ty = dst.top - sy * src.top;
        Self::from_rows_scalar(
            sx, 0.0, 0.0, tx,
            0.0, sy, 0.0, ty,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn look_at(eye: V3, center: V3, up: V3) -> Self {
        let normalize = |v: V3| -> V3 {
            let vlen = v.length();
            if scalar_nearly_zero(vlen, SK_SCALAR_NEARLY_ZERO) {
                v
            } else {
                v.scale(1.0 / vlen)
            }
        };
        let v4 = |v: V3, w: f32| V4 {
            x: v.x,
            y: v.y,
            z: v.z,
            w,
        };
        let f = normalize(center.sub(eye));
        let u = normalize(up);
        let s = normalize(f.cross(u));

        let basis = Self::cols(v4(s, 0.0), v4(s.cross(f), 0.0), v4(f.neg(), 0.0), v4(eye, 1.0));
        basis.invert().unwrap_or_else(Self::identity)
    }

    pub fn perspective(near: f32, far: f32, angle: f32) -> Self {
        let denom_inv = 1.0_f32 / (far - near);
        let half_angle = angle * 0.5;
        let cot = 1.0_f32 / half_angle.tan();

        let mut m = Self::identity();
        m.set_rc(0, 0, cot);
        m.set_rc(1, 1, cot);
        m.set_rc(2, 2, (far + near) * denom_inv);
        m.set_rc(2, 3, 2.0 * far * near * denom_inv);
        m.set_rc(3, 2, -1.0);
        m
    }
}

#[inline]
fn min4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        cpp_min_f32(a[0], b[0]),
        cpp_min_f32(a[1], b[1]),
        cpp_min_f32(a[2], b[2]),
        cpp_min_f32(a[3], b[3]),
    ]
}

fn map_rect_affine(src: Rect, mat: &[f32; 16]) -> Rect {
    let flip = [1.0_f32, 1.0, -1.0, -1.0];
    let c0 = [mat[0], mat[1], -mat[0], -mat[1]];
    let c1 = [mat[4], mat[5], -mat[4], -mat[5]];
    let c3 = [mat[12], mat[13], mat[12], mat[13]];

    let corner = |x: f32, y: f32| -> [f32; 4] {
        [
            c0[0] * x + c1[0] * y,
            c0[1] * x + c1[1] * y,
            c0[2] * x + c1[2] * y,
            c0[3] * x + c1[3] * y,
        ]
    };

    let m = min4(
        min4(corner(src.left, src.top), corner(src.right, src.top)),
        min4(corner(src.left, src.bottom), corner(src.right, src.bottom)),
    );

    Rect::make_ltrb(
        c3[0] + flip[0] * m[0],
        c3[1] + flip[1] * m[1],
        c3[2] + flip[2] * m[2],
        c3[3] + flip[3] * m[3],
    )
}

fn map_rect_perspective(src: Rect, mat: &[f32; 16]) -> Rect {
    let corner = |x: f32, y: f32| -> [f32; 4] {
        [
            mat[0] * x + mat[4] * y + mat[12],
            mat[1] * x + mat[5] * y + mat[13],
            mat[2] * x + mat[6] * y + mat[14],
            mat[3] * x + mat[7] * y + mat[15],
        ]
    };
    let tl = corner(src.left, src.top);
    let tr = corner(src.right, src.top);
    let bl = corner(src.left, src.bottom);
    let br = corner(src.right, src.bottom);

    let flip = [1.0_f32, 1.0, -1.0, -1.0];
    let project = |p0: [f32; 4], p1: [f32; 4], p2: [f32; 4]| -> [f32; 4] {
        let w0 = p0[3];
        if w0 >= W0_PLANE_DISTANCE {
            [
                flip[0] * p0[0] / w0,
                flip[1] * p0[1] / w0,
                flip[2] * p0[0] / w0,
                flip[3] * p0[1] / w0,
            ]
        } else {
            let clip = |p: [f32; 4]| -> [f32; 4] {
                let w = p[3];
                if w >= W0_PLANE_DISTANCE {
                    let t = (W0_PLANE_DISTANCE - w0) / (w - w0);
                    let cx = (t * p[0] + (1.0 - t) * p0[0]) / W0_PLANE_DISTANCE;
                    let cy = (t * p[1] + (1.0 - t) * p0[1]) / W0_PLANE_DISTANCE;
                    [flip[0] * cx, flip[1] * cy, flip[2] * cx, flip[3] * cy]
                } else {
                    [f32::INFINITY; 4]
                }
            };
            min4(clip(p1), clip(p2))
        }
    };

    let m = min4(
        min4(project(tl, tr, bl), project(tr, br, tl)),
        min4(project(br, bl, tr), project(bl, tl, br)),
    );
    Rect::make_ltrb(
        flip[0] * m[0],
        flip[1] * m[1],
        flip[2] * m[2],
        flip[3] * m[3],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_m44_basic() {
        let mut m = M44::identity();
        m.pre_translate(3.0, 4.0, 5.0);
        let v = m.map(1.0, 2.0, 3.0, 1.0);
        assert_eq!(v, V4 { x: 4.0, y: 6.0, z: 8.0, w: 1.0 });

        let inv = m.invert().unwrap();
        let mut prod = M44::identity();
        prod.set_concat(&m, &inv);
        assert_eq!(prod, M44::identity());
    }
}
