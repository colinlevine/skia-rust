//! Cubic and quadratic Bezier curve utilities matching Skia's `SkBezierCurves`.

use crate::cubics;
use crate::floating_point::double_to_float;
use crate::quads;

#[inline]
fn interpolate(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
fn pin_t_range(t: f64) -> f64 {
    if double_to_float(t + 1.0) == 1.0f32 {
        0.0
    } else if double_to_float(t) == 1.0f32 {
        1.0
    } else {
        t
    }
}

pub fn cubic_eval_at(curve: &[f64; 8], t: f64) -> [f64; 2] {
    if t == 0.0 {
        return [curve[0], curve[1]];
    }
    if t == 1.0 {
        return [curve[6], curve[7]];
    }
    let one_minus_t = 1.0 - t;
    let one_minus_t_sq = one_minus_t * one_minus_t;
    let a = one_minus_t_sq * one_minus_t;
    let b = 3.0 * one_minus_t_sq * t;
    let t_sq = t * t;
    let c = 3.0 * one_minus_t * t_sq;
    let d = t_sq * t;

    [
        a * curve[0] + b * curve[2] + c * curve[4] + d * curve[6],
        a * curve[1] + b * curve[3] + c * curve[5] + d * curve[7],
    ]
}

pub fn cubic_subdivide(curve: &[f64; 8], t: f64, out: &mut [f64; 14]) {
    out[0] = curve[0];
    out[1] = curve[1];

    out[12] = curve[6];
    out[13] = curve[7];

    let x01 = interpolate(curve[0], curve[2], t);
    let y01 = interpolate(curve[1], curve[3], t);
    let x12 = interpolate(curve[2], curve[4], t);
    let y12 = interpolate(curve[3], curve[5], t);
    let x23 = interpolate(curve[4], curve[6], t);
    let y23 = interpolate(curve[5], curve[7], t);

    out[2] = x01;
    out[3] = y01;

    out[10] = x23;
    out[11] = y23;

    out[4] = interpolate(x01, x12, t);
    out[5] = interpolate(y01, y12, t);

    out[8] = interpolate(x12, x23, t);
    out[9] = interpolate(y12, y23, t);

    out[6] = interpolate(out[4], out[8], t);
    out[7] = interpolate(out[5], out[9], t);
}

pub fn cubic_convert_to_polynomial(curve: &[f64; 8], y_values: bool) -> [f64; 4] {
    let o = usize::from(y_values);
    let p0 = curve[o];
    let p1 = curve[o + 2];
    let p2 = curve[o + 4];
    let p3 = curve[o + 6];
    [
        -p0 + 3.0 * p1 - 3.0 * p2 + p3,
        3.0 * p0 - 6.0 * p1 + 3.0 * p2,
        -3.0 * p0 + 3.0 * p1,
        p0,
    ]
}

#[allow(clippy::too_many_arguments)]
pub fn cubic_intersect(
    ax: f64,
    bx: f64,
    cx: f64,
    dx: f64,
    ay: f64,
    by: f64,
    cy: f64,
    dy: f64,
    to_intersect: f32,
    storage: &mut [f32; 3],
) -> usize {
    let mut roots = [0.0; 3];
    let n = cubics::roots_real(ay, by, cy, dy - to_intersect as f64, &mut roots);
    let mut count = 0usize;
    for &t in &roots[..n] {
        let pinned_t = pin_t_range(t);
        if (0.0..=1.0).contains(&pinned_t) {
            storage[count] = cubics::eval_at(ax, bx, cx, dx, pinned_t) as f32;
            count += 1;
        }
    }
    count
}

pub fn cubic_intersect_with_horizontal_line(
    pts: &[[f32; 2]; 4],
    y_intercept: f32,
    storage: &mut [f32; 3],
) -> usize {
    let p0 = [pts[0][0] as f64, pts[0][1] as f64];
    let p1 = [pts[1][0] as f64, pts[1][1] as f64];
    let p2 = [pts[2][0] as f64, pts[2][1] as f64];
    let p3 = [pts[3][0] as f64, pts[3][1] as f64];

    let ax = -p0[0] + 3.0 * p1[0] - 3.0 * p2[0] + p3[0];
    let bx = 3.0 * p0[0] - 6.0 * p1[0] + 3.0 * p2[0];
    let cx = -3.0 * p0[0] + 3.0 * p1[0];
    let dx = p0[0];

    let ay = -p0[1] + 3.0 * p1[1] - 3.0 * p2[1] + p3[1];
    let by = 3.0 * p0[1] - 6.0 * p1[1] + 3.0 * p2[1];
    let cy = -3.0 * p0[1] + 3.0 * p1[1];
    let dy = p0[1];

    cubic_intersect(ax, bx, cx, dx, ay, by, cy, dy, y_intercept, storage)
}

#[allow(clippy::too_many_arguments)]
pub fn quad_intersect(
    ax: f64,
    bx: f64,
    cx: f64,
    ay: f64,
    by: f64,
    cy: f64,
    y_intercept: f64,
    storage: &mut [f32; 2],
) -> usize {
    let res = quads::roots(ay, by, cy - y_intercept);
    let mut count = 0usize;
    let t0 = pin_t_range(res.root0);
    if (0.0..=1.0).contains(&t0) {
        storage[count] = quads::eval_at(ax, -2.0 * bx, cx, t0) as f32;
        count += 1;
    }
    let t1 = pin_t_range(res.root1);
    if (0.0..=1.0).contains(&t1) && t1 != t0 {
        storage[count] = quads::eval_at(ax, -2.0 * bx, cx, t1) as f32;
        count += 1;
    }
    count
}

pub fn quad_intersect_with_horizontal_line(
    pts: &[[f32; 2]; 3],
    y_intercept: f32,
    storage: &mut [f32; 2],
) -> usize {
    let p0 = [pts[0][0] as f64, pts[0][1] as f64];
    let p1 = [pts[1][0] as f64, pts[1][1] as f64];
    let p2 = [pts[2][0] as f64, pts[2][1] as f64];

    let ax = p0[0] - 2.0 * p1[0] + p2[0];
    let bx = p0[0] - p1[0];
    let cx = p0[0];

    let ay = p0[1] - 2.0 * p1[1] + p2[1];
    let by = p0[1] - p1[1];
    let cy = p0[1];

    quad_intersect(ax, bx, cx, ay, by, cy, y_intercept as f64, storage)
}
