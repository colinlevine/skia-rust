#![forbid(unsafe_code)]

use crate::SK_SCALAR_NEARLY_ZERO;

/// Matches `SkPoint3` (`{ float fX, fY, fZ; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Point3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[inline]
fn get_length_squared(x: f32, y: f32, z: f32) -> f32 {
    x * x + y * y + z * z
}

#[inline]
fn is_length_nearly_zero(x: f32, y: f32, z: f32) -> (bool, f32) {
    let mag_sq = get_length_squared(x, y, z);
    (mag_sq <= (SK_SCALAR_NEARLY_ZERO * SK_SCALAR_NEARLY_ZERO), mag_sq)
}

impl Point3 {
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub fn set(&mut self, x: f32, y: f32, z: f32) {
        self.x = x;
        self.y = y;
        self.z = z;
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

    pub fn length(x: f32, y: f32, z: f32) -> f32 {
        let mag_sq = get_length_squared(x, y, z);
        if mag_sq.is_finite() {
            mag_sq.sqrt()
        } else {
            let xx = x as f64;
            let yy = y as f64;
            let zz = z as f64;
            (xx * xx + yy * yy + zz * zz).sqrt() as f32
        }
    }

    pub fn normalize(&mut self) -> bool {
        let (nearly_zero, mag_sq) = is_length_nearly_zero(self.x, self.y, self.z);
        if nearly_zero {
            self.set(0.0, 0.0, 0.0);
            return false;
        }
        let inv_scale = if mag_sq.is_finite() {
            mag_sq as f64
        } else {
            let xx = self.x as f64;
            let yy = self.y as f64;
            let zz = self.z as f64;
            xx * xx + yy * yy + zz * zz
        };
        let scale = 1.0_f64 / inv_scale.sqrt();
        self.x = (self.x as f64 * scale) as f32;
        self.y = (self.y as f64 * scale) as f32;
        self.z = (self.z as f64 * scale) as f32;
        if !self.x.is_finite() || !self.y.is_finite() || !self.z.is_finite() {
            self.set(0.0, 0.0, 0.0);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point3_length_and_normalize() {
        assert_eq!(Point3::length(1.0, 2.0, 2.0), 3.0);
        let mut p = Point3::new(1.0, 2.0, 2.0);
        assert!(p.normalize());
        assert!((p.x - 1.0 / 3.0).abs() < 1e-6);
        assert!((p.y - 2.0 / 3.0).abs() < 1e-6);
        assert!((p.z - 2.0 / 3.0).abs() < 1e-6);

        let mut tiny = Point3::new(1e-6, 0.0, 0.0);
        assert!(!tiny.normalize());
        assert_eq!(tiny, Point3::new(0.0, 0.0, 0.0));
    }
}
