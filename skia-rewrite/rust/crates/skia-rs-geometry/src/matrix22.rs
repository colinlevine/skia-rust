#![forbid(unsafe_code)]

use crate::{copysign_f32, Point};

/// Computes `(sin, cos)` for the Givens rotation matrix corresponding to `h`.
/// Matches `SkComputeGivensRotation(const SkVector& h, SkMatrix* G)`.
pub fn compute_givens_rotation_sin_cos(h: Point) -> (f32, f32) {
    let a = h.x;
    let b = h.y;
    let (c, s);
    if b == 0.0 {
        c = copysign_f32(1.0, a);
        s = 0.0;
    } else if a == 0.0 {
        c = 0.0;
        s = -copysign_f32(1.0, b);
    } else if b.abs() > a.abs() {
        let t = a / b;
        let u = copysign_f32((1.0 + t * t).sqrt(), b);
        s = -1.0 / u;
        c = -s * t;
    } else {
        let t = b / a;
        let u = copysign_f32((1.0 + t * t).sqrt(), a);
        c = 1.0 / u;
        s = -c * t;
    }
    (s, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_givens_rotation() {
        let (s, c) = compute_givens_rotation_sin_cos(Point::new(3.0, 4.0));
        // Applying Givens rotation [c -s; s c] to (3, 4) zeroes y: s*3 + c*4 = 0
        assert!((s * 3.0 + c * 4.0).abs() < 1e-6);
        assert!((c * c + s * s - 1.0).abs() < 1e-6);
    }
}
