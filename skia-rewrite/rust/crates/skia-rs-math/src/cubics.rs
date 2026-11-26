//! Cubic root solver matching Skia's `SkCubics`.

use crate::floating_point::{double_nearly_zero, doubles_nearly_equal_ulps};
use crate::quads;

const DOUBLE_PI: f64 = 3.14159265358979323846264338327950288;

#[inline]
fn nearly_equal(x: f64, y: f64) -> bool {
    if double_nearly_zero(x) {
        double_nearly_zero(y)
    } else {
        doubles_nearly_equal_ulps(x, y, 16)
    }
}

#[inline]
fn close_to_a_quadratic(a: f64, b: f64) -> bool {
    if double_nearly_zero(b) {
        double_nearly_zero(a)
    } else {
        (a / b).abs() < 1.0e-7
    }
}

#[inline]
pub fn eval_at(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    t.mul_add(t.mul_add(t.mul_add(a, b), c), d)
}

pub fn roots_real(mut a_coef: f64, b_coef: f64, c_coef: f64, d_coef: f64, solution: &mut [f64; 3]) -> usize {
    if close_to_a_quadratic(a_coef, b_coef) {
        let mut q = [0.0; 2];
        let n = quads::roots_real(b_coef, c_coef, d_coef, &mut q);
        solution[..n].copy_from_slice(&q[..n]);
        return n;
    }
    if double_nearly_zero(d_coef) {
        let mut q = [0.0; 2];
        let mut num = quads::roots_real(a_coef, b_coef, c_coef, &mut q);
        solution[..num].copy_from_slice(&q[..num]);
        for i in 0..num {
            if double_nearly_zero(solution[i]) {
                return num;
            }
        }
        solution[num] = 0.0;
        num += 1;
        return num;
    }
    if double_nearly_zero(a_coef + b_coef + c_coef + d_coef) {
        let mut q = [0.0; 2];
        let mut num = quads::roots_real(a_coef, a_coef + b_coef, -d_coef, &mut q);
        solution[..num].copy_from_slice(&q[..num]);
        for i in 0..num {
            if doubles_nearly_equal_ulps(solution[i], 1.0, 16) {
                return num;
            }
        }
        solution[num] = 1.0;
        num += 1;
        return num;
    }

    let inv_a = 1.0 / a_coef;
    let a = b_coef * inv_a;
    let b = c_coef * inv_a;
    let c = d_coef * inv_a;

    let a2 = a * a;
    let q = (a2 - b * 3.0) / 9.0;
    let r = (2.0 * a2 * a - 9.0 * a * b + 27.0 * c) / 54.0;
    let r2 = r * r;
    let q3 = q * q * q;
    let r2_minus_q3 = r2 - q3;
    if !r2_minus_q3.is_finite() {
        return 0;
    }

    let adiv3 = a / 3.0;
    let mut count = 0usize;
    if r2_minus_q3 < 0.0 {
        let theta = (r / q3.sqrt()).clamp(-1.0, 1.0).acos();
        let neg2_root_q = -2.0 * q.sqrt();

        let root0 = neg2_root_q * (theta / 3.0).cos() - adiv3;
        solution[count] = root0;
        count += 1;

        let root1 = neg2_root_q * ((theta + 2.0 * DOUBLE_PI) / 3.0).cos() - adiv3;
        if !nearly_equal(solution[0], root1) {
            solution[count] = root1;
            count += 1;
        }
        let root2 = neg2_root_q * ((theta - 2.0 * DOUBLE_PI) / 3.0).cos() - adiv3;
        if !nearly_equal(solution[0], root2) && (count == 1 || !nearly_equal(solution[1], root2)) {
            solution[count] = root2;
            count += 1;
        }
    } else {
        let sqrt_r2_minus_q3 = r2_minus_q3.sqrt();
        a_coef = r.abs() + sqrt_r2_minus_q3;
        a_coef = a_coef.cbrt();
        if r > 0.0 {
            a_coef = -a_coef;
        }
        if !double_nearly_zero(a_coef) {
            a_coef += q / a_coef;
        }
        let root0 = a_coef - adiv3;
        solution[count] = root0;
        count += 1;
        if !double_nearly_zero(r2) && doubles_nearly_equal_ulps(r2, q3, 16) {
            let root1 = -a_coef / 2.0 - adiv3;
            if !nearly_equal(solution[0], root1) {
                solution[count] = root1;
                count += 1;
            }
        }
    }
    count
}

pub fn roots_valid_t(a: f64, b: f64, c: f64, d: f64, solution: &mut [f64; 3]) -> usize {
    let mut all_roots = [0.0; 3];
    let real_roots = roots_real(a, b, c, d, &mut all_roots);
    let mut found = 0usize;
    for &t_value in &all_roots[..real_roots] {
        if t_value <= 1.00005 && (t_value >= 1.0 || doubles_nearly_equal_ulps(t_value, 1.0, 16)) {
            if (found < 1 || !doubles_nearly_equal_ulps(solution[0], 1.0, 16))
                && (found < 2 || !doubles_nearly_equal_ulps(solution[1], 1.0, 16))
            {
                solution[found] = 1.0;
                found += 1;
            }
        } else if t_value >= -0.00005 && (t_value <= 0.0 || double_nearly_zero(t_value)) {
            if (found < 1 || !double_nearly_zero(solution[0]))
                && (found < 2 || !double_nearly_zero(solution[1]))
            {
                solution[found] = 0.0;
                found += 1;
            }
        } else if t_value > 0.0 && t_value < 1.0 {
            solution[found] = t_value;
            found += 1;
        }
    }
    found
}

#[inline]
fn approximately_zero(x: f64) -> bool {
    x.abs() < 0.00000001
}

fn find_extrema_valid_t(a: f64, b: f64, c: f64, t: &mut [f64; 2]) -> usize {
    let mut roots = [0.0; 2];
    let num_roots = quads::roots_real(3.0 * a, 2.0 * b, c, &mut roots);
    let mut valid = 0usize;
    for &tv in &roots[..num_roots] {
        if (0.0..=1.0).contains(&tv) {
            t[valid] = tv;
            valid += 1;
        }
    }
    valid
}

fn binary_search(a: f64, b: f64, c: f64, d: f64, mut start: f64, mut stop: f64) -> f64 {
    let left = eval_at(a, b, c, d, start);
    if approximately_zero(left) {
        return start;
    }
    let right = eval_at(a, b, c, d, stop);
    if !left.is_finite() || !right.is_finite() {
        return -1.0;
    }
    if (left > 0.0 && right > 0.0) || (left < 0.0 && right < 0.0) {
        return -1.0;
    }

    for _ in 0..1000 {
        let step = (start + stop) / 2.0;
        let curr = eval_at(a, b, c, d, step);
        if approximately_zero(curr) {
            return step;
        }
        if (curr < 0.0 && left < 0.0) || (curr > 0.0 && left > 0.0) {
            start = step;
        } else {
            stop = step;
        }
    }
    -1.0
}

pub fn binary_search_roots_valid_t(a: f64, b: f64, c: f64, d: f64, solution: &mut [f64; 3]) -> usize {
    if !a.is_finite() || !b.is_finite() || !c.is_finite() || !d.is_finite() {
        return 0;
    }
    let mut regions = [0.0, 0.0, 0.0, 1.0];
    let mut min_max = [0.0; 2];
    let extrema_count = find_extrema_valid_t(a, b, c, &mut min_max);
    let mut start_index = 2 - extrema_count;
    if extrema_count == 1 {
        regions[start_index + 1] = min_max[0];
    } else if extrema_count == 2 {
        regions[start_index + 1] = min_max[0].min(min_max[1]);
        regions[start_index + 2] = min_max[0].max(min_max[1]);
    }
    let mut found = 0usize;
    while start_index < 3 {
        let root = binary_search(a, b, c, d, regions[start_index], regions[start_index + 1]);
        if root >= 0.0 {
            if (found < 1 || !approximately_zero(solution[0] - root))
                && (found < 2 || !approximately_zero(solution[1] - root))
            {
                solution[found] = root;
                found += 1;
            }
        }
        start_index += 1;
    }
    found
}
