//! Quadratic root solver matching Skia's `SkQuads`.

use crate::floating_point::{double_nearly_zero, doubles_nearly_equal_ulps};

#[derive(Debug, Clone, Copy)]
pub struct QuadRootResult {
    pub discriminant: f64,
    pub root0: f64,
    pub root1: f64,
}

fn solve_linear(m: f64, b: f64, solution: &mut [f64; 2]) -> usize {
    if double_nearly_zero(m) {
        solution[0] = 0.0;
        if double_nearly_zero(b) {
            return 1;
        }
        return 0;
    }
    solution[0] = -b / m;
    if !solution[0].is_finite() {
        return 0;
    }
    1
}

#[inline]
fn close_to_linear(a: f64, b: f64) -> bool {
    if a != 0.0 {
        (b / a).abs() >= 1.0e+16
    } else {
        true
    }
}

pub fn discriminant(a: f64, b: f64, c: f64) -> f64 {
    let b2 = b * b;
    let ac = a * c;
    let rough = b2 - ac;
    if 3.0 * rough.abs() >= b2 + ac {
        return rough;
    }
    let b2_err = b.mul_add(b, -b2);
    let ac_err = a.mul_add(c, -ac);
    (b2 - ac) + (b2_err - ac_err)
}

pub fn roots(a: f64, b: f64, c: f64) -> QuadRootResult {
    let disc = discriminant(a, b, c);
    if a == 0.0 {
        let root = if b == 0.0 {
            if c == 0.0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        } else {
            c / (2.0 * b)
        };
        return QuadRootResult {
            discriminant: disc,
            root0: root,
            root1: root,
        };
    }

    if disc == 0.0 {
        let r = b / a;
        return QuadRootResult {
            discriminant: disc,
            root0: r,
            root1: r,
        };
    }

    if disc > 0.0 {
        let d = disc.sqrt();
        let r = if b > 0.0 { b + d } else { b - d };
        return QuadRootResult {
            discriminant: disc,
            root0: r / a,
            root1: c / r,
        };
    }

    QuadRootResult {
        discriminant: disc,
        root0: f64::NAN,
        root1: f64::NAN,
    }
}

#[inline]
fn zero_if_tiny(x: f64) -> f64 {
    if double_nearly_zero(x) {
        0.0
    } else {
        x
    }
}

pub fn roots_real(a: f64, b: f64, c: f64, solution: &mut [f64; 2]) -> usize {
    if close_to_linear(a, b) {
        return solve_linear(b, c, solution);
    }

    let res = roots(a, -0.5 * b, c);
    if !res.discriminant.is_finite() || res.discriminant < 0.0 {
        return 0;
    }

    let mut count = 0usize;
    let r0 = zero_if_tiny(res.root0);
    if r0.is_finite() {
        solution[count] = r0;
        count += 1;
    }
    let r1 = zero_if_tiny(res.root1);
    if r1.is_finite() {
        solution[count] = r1;
        count += 1;
    }

    if count == 2 && doubles_nearly_equal_ulps(solution[0], solution[1], 16) {
        count = 1;
    }
    count
}

#[inline]
pub fn eval_at(a: f64, b: f64, c: f64, t: f64) -> f64 {
    a.mul_add(t, b).mul_add(t, c)
}
