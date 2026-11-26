#![forbid(unsafe_code)]

use crate::float_sgn;
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

/// Matches `SkPoint` / `SkVector` (`{ float fX, fY; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

pub type Vector = Point;

/// Matches `SkIPoint` (`{ int32_t fX, fY; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IPoint {
    pub x: i32,
    pub y: i32,
}

/// Matches `SkSize` (`{ float fWidth, fHeight; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// Matches `SkISize` (`{ int32_t fWidth, fHeight; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ISize {
    pub width: i32,
    pub height: i32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn set(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        let mut accum = 0.0_f32;
        accum *= self.x;
        accum *= self.y;
        accum == 0.0
    }

    #[inline]
    pub fn scale(self, scale: f32) -> Self {
        Self {
            x: self.x * scale,
            y: self.y * scale,
        }
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    #[inline]
    pub fn cross(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    #[inline]
    pub fn length_sqd(self) -> f32 {
        self.dot(self)
    }

    #[inline]
    pub fn distance_to_sqd(a: Self, b: Self) -> f32 {
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        dx * dx + dy * dy
    }

    #[inline]
    pub fn length(dx: f32, dy: f32) -> f32 {
        let mag2 = dx * dx + dy * dy;
        if mag2.is_finite() {
            mag2.sqrt()
        } else {
            let xx = dx as f64;
            let yy = dy as f64;
            (xx * xx + yy * yy).sqrt() as f32
        }
    }

    #[inline]
    pub fn mag(self) -> f32 {
        Self::length(self.x, self.y)
    }

    pub fn set_point_length(
        &mut self,
        x: f32,
        y: f32,
        length: f32,
        orig_length: Option<&mut f32>,
    ) -> bool {
        let xx = x as f64;
        let yy = y as f64;
        let dmag = (xx * xx + yy * yy).sqrt();
        let dscale = (length as f64) / dmag;
        let nx = (xx * dscale) as f32;
        let ny = (yy * dscale) as f32;
        if !nx.is_finite() || !ny.is_finite() || (nx == 0.0 && ny == 0.0) {
            self.set(0.0, 0.0);
            return false;
        }
        let mag = if orig_length.is_some() {
            dmag as f32
        } else {
            0.0
        };
        self.set(nx, ny);
        if let Some(out) = orig_length {
            *out = mag;
        }
        true
    }

    pub fn set_length(&mut self, x: f32, y: f32, length: f32) -> bool {
        self.set_point_length(x, y, length, None)
    }

    pub fn set_length_in_place(&mut self, length: f32) -> bool {
        self.set_point_length(self.x, self.y, length, None)
    }

    pub fn normalize(&mut self) -> bool {
        self.set_point_length(self.x, self.y, 1.0, None)
    }

    pub fn set_normalize(&mut self, x: f32, y: f32) -> bool {
        self.set_point_length(x, y, 1.0, None)
    }

    pub fn normalize_with_length(&mut self) -> f32 {
        let mut mag = 0.0_f32;
        if self.set_point_length(self.x, self.y, 1.0, Some(&mut mag)) {
            mag
        } else {
            0.0
        }
    }

    pub fn distance_to_line_between_sqd(pt: Self, a: Self, b: Self) -> (f32, i32) {
        let u = b - a;
        let v = pt - a;

        let u_length_sqd = u.length_sqd();
        let det = u.cross(v);
        let side = float_sgn(det);

        let mut temp = det / u_length_sqd;
        temp *= det;
        if !temp.is_finite() {
            return (v.length_sqd(), side);
        }
        (temp, side)
    }

    pub fn distance_to_line_segment_between_sqd(pt: Self, a: Self, b: Self) -> f32 {
        let u = b - a;
        let v = pt - a;

        let u_length_sqd = u.length_sqd();
        let u_dot_v = u.dot(v);

        if u_dot_v <= 0.0 {
            v.length_sqd()
        } else if u_dot_v > u_length_sqd {
            Self::distance_to_sqd(b, pt)
        } else {
            let det = u.cross(v);
            let mut temp = det / u_length_sqd;
            temp *= det;
            if !temp.is_finite() {
                return v.length_sqd();
            }
            temp
        }
    }
}

impl Add for Point {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl AddAssign for Point {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Point {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl SubAssign for Point {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Neg for Point {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl Mul<f32> for Point {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_length_and_normalize() {
        assert_eq!(Point::length(3.0, 4.0), 5.0);
        // Overflow fallback to double
        let big = 1e30_f32;
        let len = Point::length(big, big);
        assert!(len.is_finite());
        assert!((len - big * std::f32::consts::SQRT_2).abs() / len < 1e-5);

        let mut p = Point::new(3.0, 4.0);
        let orig = p.normalize_with_length();
        assert_eq!(orig, 5.0);
        assert!((p.x - 0.6).abs() < 1e-6);
        assert!((p.y - 0.8).abs() < 1e-6);

        let mut z = Point::new(0.0, 0.0);
        assert!(!z.normalize());
        assert_eq!(z, Point::ZERO);
    }

    #[test]
    fn test_distance_to_line() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(10.0, 0.0);
        let (d2, side) = Point::distance_to_line_between_sqd(Point::new(5.0, 3.0), a, b);
        assert!((d2 - 9.0).abs() < 1e-5);
        assert_eq!(side, 1);

        let seg_d2 = Point::distance_to_line_segment_between_sqd(Point::new(13.0, 4.0), a, b);
        assert!((seg_d2 - 25.0).abs() < 1e-5);
    }
}
