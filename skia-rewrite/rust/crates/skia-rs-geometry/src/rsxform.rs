#![forbid(unsafe_code)]

use crate::Point;

/// Matches `SkRSXform` (`{ float fSCos, fSSin, fTx, fTy; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct RSXform {
    pub scos: f32,
    pub ssin: f32,
    pub tx: f32,
    pub ty: f32,
}

impl RSXform {
    #[inline]
    pub const fn new(scos: f32, ssin: f32, tx: f32, ty: f32) -> Self {
        Self { scos, ssin, tx, ty }
    }

    pub fn to_quad(self, width: f32, height: f32) -> [Point; 4] {
        let m00 = self.scos;
        let m01 = -self.ssin;
        let m02 = self.tx;
        let m10 = -m01;
        let m11 = m00;
        let m12 = self.ty;

        [
            Point::new(m02, m12),
            Point::new(m00 * width + m02, m10 * width + m12),
            Point::new(
                m00 * width + m01 * height + m02,
                m10 * width + m11 * height + m12,
            ),
            Point::new(m01 * height + m02, m11 * height + m12),
        ]
    }

    pub fn to_tri_strip(self, width: f32, height: f32) -> [Point; 4] {
        let m00 = self.scos;
        let m01 = -self.ssin;
        let m02 = self.tx;
        let m10 = -m01;
        let m11 = m00;
        let m12 = self.ty;

        [
            Point::new(m02, m12),
            Point::new(m01 * height + m02, m11 * height + m12),
            Point::new(m00 * width + m02, m10 * width + m12),
            Point::new(
                m00 * width + m01 * height + m02,
                m10 * width + m11 * height + m12,
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rsxform_quad_and_strip() {
        let x = RSXform::new(1.0, 0.0, 10.0, 20.0);
        let q = x.to_quad(5.0, 3.0);
        assert_eq!(q[0], Point::new(10.0, 20.0));
        assert_eq!(q[1], Point::new(15.0, 20.0));
        assert_eq!(q[2], Point::new(15.0, 23.0));
        assert_eq!(q[3], Point::new(10.0, 23.0));

        let s = x.to_tri_strip(5.0, 3.0);
        assert_eq!(s[0], q[0]);
        assert_eq!(s[1], q[3]);
        assert_eq!(s[2], q[1]);
        assert_eq!(s[3], q[2]);
    }
}
