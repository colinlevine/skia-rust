#![forbid(unsafe_code)]

use crate::{
    all_finite, copysign_f32, cpp_max_f32, cpp_min_f32, pin_f32, point3::Point3,
    scalar_interp, scalar_nearly_zero, Matrix, Point, Rect, SK_SCALAR_NEARLY_ZERO, SK_SCALAR_PI,
    SK_SCALAR_ROOT_2_OVER_2,
};
use skia_rs_math::{bezier, cubics};

pub const MAX_CONIC_TO_QUAD_POW2: i32 = 5;
pub const MAX_CONICS_FOR_ARC: usize = 5;

/// Matches `SkCubicType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CubicType {
    Serpentine = 0,
    Loop = 1,
    LocalCusp = 2,
    CuspAtInfinity = 3,
    Quadratic = 4,
    LineOrPoint = 5,
}

/// Matches `SkConic` (`{ SkPoint fPts[3]; SkScalar fW; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Conic {
    pub pts: [Point; 3],
    pub w: f32,
}

#[inline]
fn is_not_monotonic(a: f32, b: f32, c: f32) -> bool {
    let ab = a - b;
    let mut bc = b - c;
    if ab < 0.0 {
        bc = -bc;
    }
    ab == 0.0 || bc < 0.0
}

#[inline]
fn valid_unit_divide(mut numer: f32, mut denom: f32, ratio: &mut f32) -> usize {
    if numer < 0.0 {
        numer = -numer;
        denom = -denom;
    }
    if denom == 0.0 || numer == 0.0 || numer >= denom {
        return 0;
    }
    let r = numer / denom;
    if r.is_nan() || r == 0.0 {
        return 0;
    }
    *ratio = r;
    1
}

#[inline]
fn equals_within_tolerance(a: Point, b: Point) -> bool {
    scalar_nearly_zero(a.x - b.x, SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_zero(a.y - b.y, SK_SCALAR_NEARLY_ZERO)
}

#[inline]
fn points_are_finite(pts: &[Point]) -> bool {
    let mut accum = 0.0_f32;
    for p in pts {
        accum *= p.x;
        accum *= p.y;
    }
    accum == 0.0
}

pub fn find_unit_quad_roots(a: f32, b: f32, c: f32, roots: &mut [f32; 2]) -> i32 {
    if a == 0.0 {
        return valid_unit_divide(-c, b, &mut roots[0]) as i32;
    }
    let dr = (b as f64) * (b as f64) - 4.0 * (a as f64) * (c as f64);
    if dr < 0.0 {
        return 0;
    }
    let r = dr.sqrt() as f32;
    if !r.is_finite() {
        return 0;
    }
    let q = if b < 0.0 { -(b - r) / 2.0 } else { -(b + r) / 2.0 };
    let mut count = 0usize;
    count += valid_unit_divide(q, a, &mut roots[count]);
    if count < 2 {
        count += valid_unit_divide(c, q, &mut roots[count]);
    }
    if count == 2 {
        if roots[0] > roots[1] {
            roots.swap(0, 1);
        } else if roots[0] == roots[1] {
            count -= 1;
        }
    }
    count as i32
}

#[inline]
fn quad_coeff_eval(src: &[Point; 3], t: f32) -> Point {
    let c = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let b = (p1 - c) + (p1 - c);
    let a = p2 - (p1 + p1) + c;
    Point::new((a.x * t + b.x) * t + c.x, (a.y * t + b.y) * t + c.y)
}

pub fn eval_quad_at(src: &[Point; 3], t: f32) -> Point {
    quad_coeff_eval(src, t)
}

pub fn eval_quad_tangent_at(src: &[Point; 3], t: f32) -> Point {
    if (t == 0.0 && src[0] == src[1]) || (t == 1.0 && src[1] == src[2]) {
        return src[2] - src[0];
    }
    let b = src[1] - src[0];
    let a = src[2] - src[1] - b;
    let tan = Point::new(a.x * t + b.x, a.y * t + b.y);
    tan + tan
}

#[inline]
fn interp_pt(v0: Point, v1: Point, t: f32) -> Point {
    Point::new(v0.x + (v1.x - v0.x) * t, v0.y + (v1.y - v0.y) * t)
}

pub fn chop_quad_at(src: &[Point; 3], dst: &mut [Point; 5], t: f32) {
    let p0 = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let p01 = interp_pt(p0, p1, t);
    let p12 = interp_pt(p1, p2, t);
    dst[0] = p0;
    dst[1] = p01;
    dst[2] = interp_pt(p01, p12, t);
    dst[3] = p12;
    dst[4] = p2;
}

pub fn chop_quad_at_half(src: &[Point; 3], dst: &mut [Point; 5]) {
    chop_quad_at(src, dst, 0.5);
}

pub fn measure_angle_between_vectors(a: Point, b: Point) -> f32 {
    let mut cos_theta = a.dot(b) / (a.dot(a) * b.dot(b)).sqrt();
    cos_theta = cpp_max_f32(cpp_min_f32(1.0, cos_theta), -1.0);
    cos_theta.acos()
}

pub fn find_bisector(a: Point, b: Point) -> Point {
    let (v0, v1) = if a.dot(b) >= 0.0 {
        (a, b)
    } else if a.cross(b) >= 0.0 {
        (Point::new(-a.y, a.x), Point::new(b.y, -b.x))
    } else {
        (Point::new(a.y, -a.x), Point::new(-b.y, b.x))
    };
    let inv0 = 1.0_f32 / (v0.x * v0.x + v0.y * v0.y).sqrt();
    let inv1 = 1.0_f32 / (v1.x * v1.x + v1.y * v1.y).sqrt();
    Point::new(v0.x * inv0 + v1.x * inv1, v0.y * inv0 + v1.y * inv1)
}

pub fn find_quad_mid_tangent(src: &[Point; 3]) -> f32 {
    let tan0 = src[1] - src[0];
    let tan1 = src[2] - src[1];
    let bisector = find_bisector(tan0, -tan1);
    let mut t = tan0.dot(bisector) / (tan0 - tan1).dot(bisector);
    if !(t > 0.0 && t < 1.0) {
        t = 0.5;
    }
    t
}

pub fn find_quad_extrema(a: f32, b: f32, c: f32, t_value: &mut f32) -> i32 {
    valid_unit_divide(a - b, a - b - b + c, t_value) as i32
}

pub fn chop_quad_at_y_extrema(src: &[Point; 3], dst: &mut [Point; 5]) -> i32 {
    let a = src[0].y;
    let mut b = src[1].y;
    let c = src[2].y;
    if is_not_monotonic(a, b, c) {
        let mut t_value = 0.0_f32;
        if valid_unit_divide(a - b, a - b - b + c, &mut t_value) != 0 {
            chop_quad_at(src, dst, t_value);
            dst[1].y = dst[2].y;
            dst[3].y = dst[2].y;
            return 1;
        }
        b = if (a - b).abs() < (b - c).abs() { a } else { c };
    }
    dst[0].set(src[0].x, a);
    dst[1].set(src[1].x, b);
    dst[2].set(src[2].x, c);
    0
}

pub fn chop_quad_at_x_extrema(src: &[Point; 3], dst: &mut [Point; 5]) -> i32 {
    let a = src[0].x;
    let mut b = src[1].x;
    let c = src[2].x;
    if is_not_monotonic(a, b, c) {
        let mut t_value = 0.0_f32;
        if valid_unit_divide(a - b, a - b - b + c, &mut t_value) != 0 {
            chop_quad_at(src, dst, t_value);
            dst[1].x = dst[2].x;
            dst[3].x = dst[2].x;
            return 1;
        }
        b = if (a - b).abs() < (b - c).abs() { a } else { c };
    }
    dst[0].set(a, src[0].y);
    dst[1].set(b, src[1].y);
    dst[2].set(c, src[2].y);
    0
}

pub fn find_quad_max_curvature(src: &[Point; 3]) -> f32 {
    let ax = src[1].x - src[0].x;
    let ay = src[1].y - src[0].y;
    let bx = src[0].x - src[1].x - src[1].x + src[2].x;
    let by = src[0].y - src[1].y - src[1].y + src[2].y;

    let mut numer = -(ax * bx + ay * by);
    let mut denom = bx * bx + by * by;
    if denom < 0.0 {
        numer = -numer;
        denom = -denom;
    }
    if numer <= 0.0 {
        return 0.0;
    }
    if numer >= denom {
        return 1.0;
    }
    numer / denom
}

pub fn chop_quad_at_max_curvature(src: &[Point; 3], dst: &mut [Point; 5]) -> i32 {
    let t = find_quad_max_curvature(src);
    if t > 0.0 && t < 1.0 {
        chop_quad_at(src, dst, t);
        2
    } else {
        dst[0..3].copy_from_slice(src);
        1
    }
}

pub fn convert_quad_to_cubic(src: &[Point; 3], dst: &mut [Point; 4]) {
    let scale = (2.0_f64 / 3.0_f64) as f32;
    let s0 = src[0];
    let s1 = src[1];
    let s2 = src[2];
    dst[0] = s0;
    dst[1] = Point::new(s0.x + (s1.x - s0.x) * scale, s0.y + (s1.y - s0.y) * scale);
    dst[2] = Point::new(s2.x + (s1.x - s2.x) * scale, s2.y + (s1.y - s2.y) * scale);
    dst[3] = s2;
}

// --- Cubics ---

fn eval_cubic_loc(src: &[Point; 4], t: f32) -> Point {
    let p0 = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let p3 = src[3];
    let ax = p3.x + 3.0 * (p1.x - p2.x) - p0.x;
    let ay = p3.y + 3.0 * (p1.y - p2.y) - p0.y;
    let bx = 3.0 * (p2.x - (p1.x + p1.x) + p0.x);
    let by = 3.0 * (p2.y - (p1.y + p1.y) + p0.y);
    let cx = 3.0 * (p1.x - p0.x);
    let cy = 3.0 * (p1.y - p0.y);
    Point::new(
        ((ax * t + bx) * t + cx) * t + p0.x,
        ((ay * t + by) * t + cy) * t + p0.y,
    )
}

fn eval_cubic_derivative(src: &[Point; 4], t: f32) -> Point {
    let p0 = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let p3 = src[3];
    let ax = p3.x + 3.0 * (p1.x - p2.x) - p0.x;
    let ay = p3.y + 3.0 * (p1.y - p2.y) - p0.y;
    let bx = 2.0 * (p2.x - (p1.x + p1.x) + p0.x);
    let by = 2.0 * (p2.y - (p1.y + p1.y) + p0.y);
    let cx = p1.x - p0.x;
    let cy = p1.y - p0.y;
    Point::new((ax * t + bx) * t + cx, (ay * t + by) * t + cy)
}

fn eval_cubic_2nd_derivative(src: &[Point; 4], t: f32) -> Point {
    let p0 = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let p3 = src[3];
    let ax = p3.x + 3.0 * (p1.x - p2.x) - p0.x;
    let ay = p3.y + 3.0 * (p1.y - p2.y) - p0.y;
    let bx = p2.x - (p1.x + p1.x) + p0.x;
    let by = p2.y - (p1.y + p1.y) + p0.y;
    Point::new(ax * t + bx, ay * t + by)
}

pub fn eval_cubic_at(
    src: &[Point; 4],
    t: f32,
    loc: Option<&mut Point>,
    tangent: Option<&mut Point>,
    curvature: Option<&mut Point>,
) {
    if let Some(l) = loc {
        *l = eval_cubic_loc(src, t);
    }
    if let Some(tan) = tangent {
        if (t == 0.0 && src[0] == src[1]) || (t == 1.0 && src[2] == src[3]) {
            *tan = if t == 0.0 {
                src[2] - src[0]
            } else {
                src[3] - src[1]
            };
            if tan.x == 0.0 && tan.y == 0.0 {
                *tan = src[3] - src[0];
            }
        } else {
            *tan = eval_cubic_derivative(src, t);
        }
    }
    if let Some(curv) = curvature {
        *curv = eval_cubic_2nd_derivative(src, t);
    }
}

pub fn find_cubic_extrema(a: f32, b: f32, c: f32, d: f32, t_values: &mut [f32; 2]) -> i32 {
    let ca = d - a + 3.0 * (b - c);
    let cb = 2.0 * (a - b - b + c);
    let cc = b - a;
    find_unit_quad_roots(ca, cb, cc, t_values)
}

#[inline]
fn unchecked_mix_pt(a: Point, b: Point, t: f32) -> Point {
    Point::new((b.x - a.x) * t + a.x, (b.y - a.y) * t + a.y)
}

pub fn chop_cubic_at_1(src: &[Point; 4], dst: &mut [Point; 7], t: f32) {
    if t == 1.0 {
        dst[0..4].copy_from_slice(src);
        dst[4] = src[3];
        dst[5] = src[3];
        dst[6] = src[3];
        return;
    }
    let p0 = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let p3 = src[3];

    let ab = unchecked_mix_pt(p0, p1, t);
    let bc = unchecked_mix_pt(p1, p2, t);
    let cd = unchecked_mix_pt(p2, p3, t);
    let abc = unchecked_mix_pt(ab, bc, t);
    let bcd = unchecked_mix_pt(bc, cd, t);
    let abcd = unchecked_mix_pt(abc, bcd, t);

    dst[0] = p0;
    dst[1] = ab;
    dst[2] = abc;
    dst[3] = abcd;
    dst[4] = bcd;
    dst[5] = cd;
    dst[6] = p3;
}

pub fn chop_cubic_at_2(src: &[Point; 4], dst: &mut [Point; 10], t0: f32, t1: f32) {
    if t1 == 1.0 {
        let mut tmp = [Point::ZERO; 7];
        chop_cubic_at_1(src, &mut tmp, t0);
        dst[0..7].copy_from_slice(&tmp);
        dst[7] = src[3];
        dst[8] = src[3];
        dst[9] = src[3];
        return;
    }
    let p0 = src[0];
    let p1 = src[1];
    let p2 = src[2];
    let p3 = src[3];

    let ab0 = unchecked_mix_pt(p0, p1, t0);
    let bc0 = unchecked_mix_pt(p1, p2, t0);
    let cd0 = unchecked_mix_pt(p2, p3, t0);
    let abc0 = unchecked_mix_pt(ab0, bc0, t0);
    let bcd0 = unchecked_mix_pt(bc0, cd0, t0);
    let abcd0 = unchecked_mix_pt(abc0, bcd0, t0);

    let ab1 = unchecked_mix_pt(p0, p1, t1);
    let bc1 = unchecked_mix_pt(p1, p2, t1);
    let cd1 = unchecked_mix_pt(p2, p3, t1);
    let abc1 = unchecked_mix_pt(ab1, bc1, t1);
    let bcd1 = unchecked_mix_pt(bc1, cd1, t1);
    let abcd1 = unchecked_mix_pt(abc1, bcd1, t1);

    let mid0 = unchecked_mix_pt(abc0, bcd0, t1);
    let mid1 = unchecked_mix_pt(abc1, bcd1, t0);

    dst[0] = p0;
    dst[1] = ab0;
    dst[2] = abc0;
    dst[3] = abcd0;
    dst[4] = mid0;
    dst[5] = mid1;
    dst[6] = abcd1;
    dst[7] = bcd1;
    dst[8] = cd1;
    dst[9] = p3;
}

pub fn chop_cubic_at_many(src: &[Point; 4], dst: &mut [Point], t_values: &[f32]) {
    let t_count = t_values.len();
    if t_count == 0 {
        dst[0..4].copy_from_slice(src);
        return;
    }
    let mut cur_src = *src;
    let mut dst_off = 0usize;
    let mut i = 0usize;
    while i + 1 < t_count {
        let mut t0 = t_values[i];
        let mut t1 = t_values[i + 1];
        if i != 0 {
            let last_t = t_values[i - 1];
            t0 = pin_f32((t0 - last_t) / (1.0 - last_t), 0.0, 1.0);
            t1 = pin_f32((t1 - last_t) / (1.0 - last_t), 0.0, 1.0);
        }
        let mut tmp = [Point::ZERO; 10];
        chop_cubic_at_2(&cur_src, &mut tmp, t0, t1);
        dst[dst_off..dst_off + 10].copy_from_slice(&tmp);
        cur_src = [tmp[6], tmp[7], tmp[8], tmp[9]];
        dst_off += 6;
        i += 2;
    }
    if i < t_count {
        let mut t = t_values[i];
        if i != 0 {
            let last_t = t_values[i - 1];
            t = pin_f32((t - last_t) / (1.0 - last_t), 0.0, 1.0);
        }
        let mut tmp = [Point::ZERO; 7];
        chop_cubic_at_1(&cur_src, &mut tmp, t);
        dst[dst_off..dst_off + 7].copy_from_slice(&tmp);
    }
}

pub fn chop_cubic_at_half(src: &[Point; 4], dst: &mut [Point; 7]) {
    chop_cubic_at_1(src, dst, 0.5);
}

pub fn measure_non_inflect_cubic_rotation(pts: &[Point; 4]) -> f32 {
    let a = pts[1] - pts[0];
    let b = pts[2] - pts[1];
    let c = pts[3] - pts[2];
    if a.is_zero() {
        return measure_angle_between_vectors(b, c);
    }
    if b.is_zero() {
        return measure_angle_between_vectors(a, c);
    }
    if c.is_zero() {
        return measure_angle_between_vectors(a, b);
    }
    2.0 * SK_SCALAR_PI
        - measure_angle_between_vectors(a, -b)
        - measure_angle_between_vectors(b, -c)
}

fn solve_quadratic_equation_for_midtangent(a: f32, b: f32, c: f32, discr: f32) -> f32 {
    let q = -0.5_f32 * (b + copysign_f32(discr.sqrt(), b));
    let half_qa = -0.5_f32 * q * a;
    let mut t = if (q * q + half_qa).abs() < (a * c + half_qa).abs() {
        q / a
    } else {
        c / q
    };
    if !(t > 0.0 && t < 1.0) {
        t = 0.5;
    }
    t
}

pub fn find_cubic_mid_tangent(src: &[Point; 4]) -> f32 {
    let tan0 = if src[0] == src[1] {
        src[2] - src[0]
    } else {
        src[1] - src[0]
    };
    let tan1 = if src[2] == src[3] {
        src[3] - src[1]
    } else {
        src[3] - src[2]
    };
    let bisector = find_bisector(tan0, -tan1);

    let m0 = [-1.0_f32, 2.0, -1.0];
    let m1 = [3.0_f32, -4.0, 1.0];
    let m2 = [-3.0_f32, 2.0, 0.0];
    let mut cx = [src[3].x, 0.0, 0.0];
    let mut cy = [src[3].y, 0.0, 0.0];
    for i in 0..3 {
        cx[i] = m2[i].mul_add(src[2].x, cx[i]);
        cx[i] = m1[i].mul_add(src[1].x, cx[i]);
        cx[i] = m0[i].mul_add(src[0].x, cx[i]);
        cy[i] = m2[i].mul_add(src[2].y, cy[i]);
        cy[i] = m1[i].mul_add(src[1].y, cy[i]);
        cy[i] = m0[i].mul_add(src[0].y, cy[i]);
    }
    let a = cx[0] * bisector.x + cy[0] * bisector.y;
    let b = cx[1] * bisector.x + cy[1] * bisector.y;
    let c = cx[2] * bisector.x + cy[2] * bisector.y;
    let discr = b * b - 4.0 * a * c;
    if discr > 0.0 {
        solve_quadratic_equation_for_midtangent(a, b, c, discr)
    } else {
        let a2 = cx[0] * tan0.x + cy[0] * tan0.y;
        let b2 = cx[1] * tan0.x + cy[1] * tan0.y;
        let mut t = 0.0_f32;
        if a2 != 0.0 {
            t = -b2 / (2.0 * a2);
        }
        if !(t > 0.0 && t < 1.0) {
            t = 0.5;
        }
        t
    }
}

pub fn chop_cubic_at_y_extrema(src: &[Point; 4], dst: Option<&mut [Point; 10]>) -> i32 {
    let mut t_values = [0.0_f32; 2];
    let roots = find_cubic_extrema(src[0].y, src[1].y, src[2].y, src[3].y, &mut t_values);
    if let Some(d) = dst {
        chop_cubic_at_many(src, d, &t_values[..roots as usize]);
        if roots > 0 {
            d[2].y = d[3].y;
            d[4].y = d[3].y;
            if roots == 2 {
                d[5].y = d[6].y;
                d[7].y = d[6].y;
            }
        }
    }
    roots
}

pub fn chop_cubic_at_x_extrema(src: &[Point; 4], dst: Option<&mut [Point; 10]>) -> i32 {
    let mut t_values = [0.0_f32; 2];
    let roots = find_cubic_extrema(src[0].x, src[1].x, src[2].x, src[3].x, &mut t_values);
    if let Some(d) = dst {
        chop_cubic_at_many(src, d, &t_values[..roots as usize]);
        if roots > 0 {
            d[2].x = d[3].x;
            d[4].x = d[3].x;
            if roots == 2 {
                d[5].x = d[6].x;
                d[7].x = d[6].x;
            }
        }
    }
    roots
}

pub fn find_cubic_inflections(src: &[Point; 4], t_values: &mut [f32; 2]) -> i32 {
    let ax = src[1].x - src[0].x;
    let ay = src[1].y - src[0].y;
    let bx = src[2].x - 2.0 * src[1].x + src[0].x;
    let by = src[2].y - 2.0 * src[1].y + src[0].y;
    let cx = src[3].x + 3.0 * (src[1].x - src[2].x) - src[0].x;
    let cy = src[3].y + 3.0 * (src[1].y - src[2].y) - src[0].y;
    find_unit_quad_roots(bx * cy - by * cx, ax * cy - ay * cx, ax * by - ay * bx, t_values)
}

pub fn chop_cubic_at_inflections(src: &[Point; 4], dst: Option<&mut [Point; 10]>) -> i32 {
    let mut t_values = [0.0_f32; 2];
    let count = find_cubic_inflections(src, &mut t_values);
    if let Some(d) = dst {
        if count == 0 {
            d[0..4].copy_from_slice(src);
        } else {
            chop_cubic_at_many(src, d, &t_values[..count as usize]);
        }
    }
    count + 1
}

fn calc_dot_cross_cubic(p0: Point, p1: Point, p2: Point) -> f64 {
    let x_comp = (p0.x as f64) * ((p1.y as f64) - (p2.y as f64));
    let y_comp = (p0.y as f64) * ((p2.x as f64) - (p1.x as f64));
    let w_comp = (p1.x as f64) * (p2.y as f64) - (p1.y as f64) * (p2.x as f64);
    x_comp + y_comp + w_comp
}

#[inline]
fn previous_inverse_pow2(n: f64) -> f64 {
    let mut bits = n.to_bits();
    bits = ((1023_u64 * 2) << 52)
        .wrapping_add((1_u64 << 52) - 1)
        .wrapping_sub(bits);
    bits &= 0x7ff_u64 << 52;
    f64::from_bits(bits)
}

fn write_cubic_inflection_roots(
    t0: f64,
    s0: f64,
    t1: f64,
    s1: f64,
    t: &mut [f64; 2],
    s: &mut [f64; 2],
) {
    t[0] = t0;
    s[0] = s0;
    t[1] = -t1.copysign(t1 * s1);
    s[1] = -s1.abs();
    if s[1].copysign(s[0]) * t[0] > -s[0].abs() * t[1] {
        t.swap(0, 1);
        s.swap(0, 1);
    }
}

pub fn classify_cubic(
    p: &[Point; 4],
    mut t_out: Option<&mut [f64; 2]>,
    mut s_out: Option<&mut [f64; 2]>,
    d_out: Option<&mut [f64; 4]>,
) -> CubicType {
    let a1 = calc_dot_cross_cubic(p[0], p[3], p[2]);
    let a2 = calc_dot_cross_cubic(p[1], p[0], p[3]);
    let a3 = calc_dot_cross_cubic(p[2], p[1], p[0]);

    let mut d3 = 3.0 * a3;
    let mut d2 = d3 - a2;
    let mut d1 = d2 - a2 + a1;

    let max12 = if d1.abs() < d2.abs() { d2.abs() } else { d1.abs() };
    let dmax = if max12 < d3.abs() { d3.abs() } else { max12 };
    let norm = previous_inverse_pow2(dmax);
    d1 *= norm;
    d2 *= norm;
    d3 *= norm;

    if let Some(d) = d_out {
        d[3] = d3;
        d[2] = d2;
        d[1] = d1;
        d[0] = 0.0;
    }

    if d1 != 0.0 {
        let discr = 3.0 * d2 * d2 - 4.0 * d1 * d3;
        if discr > 0.0 {
            if let (Some(t), Some(s)) = (t_out.as_deref_mut(), s_out.as_deref_mut()) {
                let q = 3.0 * d2 + (3.0 * discr).sqrt().copysign(d2);
                write_cubic_inflection_roots(q, 6.0 * d1, 2.0 * d3, q, t, s);
            }
            CubicType::Serpentine
        } else if discr < 0.0 {
            if let (Some(t), Some(s)) = (t_out.as_deref_mut(), s_out.as_deref_mut()) {
                let q = d2 + (-discr).sqrt().copysign(d2);
                write_cubic_inflection_roots(q, 2.0 * d1, 2.0 * (d2 * d2 - d3 * d1), d1 * q, t, s);
            }
            CubicType::Loop
        } else {
            if let (Some(t), Some(s)) = (t_out.as_deref_mut(), s_out.as_deref_mut()) {
                write_cubic_inflection_roots(d2, 2.0 * d1, d2, 2.0 * d1, t, s);
            }
            CubicType::LocalCusp
        }
    } else if d2 != 0.0 {
        if let (Some(t), Some(s)) = (t_out.as_deref_mut(), s_out.as_deref_mut()) {
            write_cubic_inflection_roots(d3, 3.0 * d2, 1.0, 0.0, t, s);
        }
        CubicType::CuspAtInfinity
    } else {
        if let (Some(t), Some(s)) = (t_out.as_deref_mut(), s_out.as_deref_mut()) {
            write_cubic_inflection_roots(1.0, 0.0, 1.0, 0.0, t, s);
        }
        if d3 != 0.0 {
            CubicType::Quadratic
        } else {
            CubicType::LineOrPoint
        }
    }
}

fn bubble_sort_f32(arr: &mut [f32]) {
    let count = arr.len();
    for i in (1..count).rev() {
        for j in (1..=i).rev() {
            if arr[j] < arr[j - 1] {
                arr.swap(j, j - 1);
            }
        }
    }
}

fn collapse_duplicates(arr: &mut [f32]) -> usize {
    let mut count = arr.len();
    let mut start = 0usize;
    let mut n = count;
    while n > 1 {
        if arr[start] == arr[start + 1] {
            for i in 1..n {
                arr[start + i - 1] = arr[start + i];
            }
            count -= 1;
        } else {
            start += 1;
        }
        n -= 1;
    }
    count
}

fn solve_cubic_poly(coeff: &[f32; 4], t_values: &mut [f32; 3]) -> i32 {
    if scalar_nearly_zero(coeff[0], SK_SCALAR_NEARLY_ZERO) {
        let mut q = [0.0_f32; 2];
        let n = find_unit_quad_roots(coeff[1], coeff[2], coeff[3], &mut q);
        t_values[0] = q[0];
        t_values[1] = q[1];
        return n;
    }
    let inv_a = 1.0_f32 / coeff[0];
    let a = coeff[1] * inv_a;
    let b = coeff[2] * inv_a;
    let c = coeff[3] * inv_a;

    let q = (a * a - b * 3.0) / 9.0;
    let r = (2.0 * a * a * a - 9.0 * a * b + 27.0 * c) / 54.0;

    let q3 = q * q * q;
    let r2_minus_q3 = r * r - q3;
    let adiv3 = a / 3.0;

    if r2_minus_q3 < 0.0 {
        let theta = pin_f32(r / q3.sqrt(), -1.0, 1.0).acos();
        let neg2_root_q = -2.0 * q.sqrt();
        t_values[0] = pin_f32(neg2_root_q * (theta / 3.0).cos() - adiv3, 0.0, 1.0);
        t_values[1] =
            pin_f32(neg2_root_q * ((theta + 2.0 * SK_SCALAR_PI) / 3.0).cos() - adiv3, 0.0, 1.0);
        t_values[2] =
            pin_f32(neg2_root_q * ((theta - 2.0 * SK_SCALAR_PI) / 3.0).cos() - adiv3, 0.0, 1.0);
        bubble_sort_f32(t_values);
        collapse_duplicates(t_values) as i32
    } else {
        let mut big_a = r.abs() + r2_minus_q3.sqrt();
        big_a = big_a.powf(0.3333333_f32);
        if r > 0.0 {
            big_a = -big_a;
        }
        if big_a != 0.0 {
            big_a += q / big_a;
        }
        t_values[0] = pin_f32(big_a - adiv3, 0.0, 1.0);
        1
    }
}

fn formulate_f1_dot_f2(v0: f32, v1: f32, v2: f32, v3: f32) -> [f32; 4] {
    let a = v1 - v0;
    let b = v2 - 2.0 * v1 + v0;
    let c = v3 + 3.0 * (v1 - v2) - v0;
    [c * c, 3.0 * b * c, 2.0 * b * b + c * a, a * b]
}

pub fn find_cubic_max_curvature(src: &[Point; 4], t_values: &mut [f32; 3]) -> i32 {
    let mut coeff_x = formulate_f1_dot_f2(src[0].x, src[1].x, src[2].x, src[3].x);
    let coeff_y = formulate_f1_dot_f2(src[0].y, src[1].y, src[2].y, src[3].y);
    for i in 0..4 {
        coeff_x[i] += coeff_y[i];
    }
    solve_cubic_poly(&coeff_x, t_values)
}

pub fn chop_cubic_at_max_curvature(
    src: &[Point; 4],
    dst: Option<&mut [Point; 13]>,
    t_values_out: Option<&mut [f32; 3]>,
) -> i32 {
    let mut roots = [0.0_f32; 3];
    let root_count = find_cubic_max_curvature(src, &mut roots) as usize;
    let mut filtered = [0.0_f32; 3];
    let mut count = 0usize;
    for &r in &roots[..root_count] {
        if r > 0.0 && r < 1.0 {
            filtered[count] = r;
            count += 1;
        }
    }
    if let Some(t_out) = t_values_out {
        t_out[..count].copy_from_slice(&filtered[..count]);
    }
    if let Some(d) = dst {
        if count == 0 {
            d[0..4].copy_from_slice(src);
        } else {
            chop_cubic_at_many(src, d, &filtered[..count]);
        }
    }
    (count + 1) as i32
}

fn calc_cubic_precision(src: &[Point; 4]) -> f32 {
    (Point::distance_to_sqd(src[1], src[0])
        + Point::distance_to_sqd(src[2], src[1])
        + Point::distance_to_sqd(src[3], src[2]))
        * 1e-8
}

fn on_same_side(src: &[Point; 4], test_idx: usize, line_idx: usize) -> bool {
    let origin = src[line_idx];
    let line = src[line_idx + 1] - origin;
    let c0 = line.cross(src[test_idx] - origin);
    let c1 = line.cross(src[test_idx + 1] - origin);
    c0 * c1 >= 0.0
}

pub fn find_cubic_cusp(src: &[Point; 4]) -> f32 {
    if src[0] == src[1] || src[2] == src[3] {
        return -1.0;
    }
    if on_same_side(src, 0, 2) || on_same_side(src, 2, 0) {
        return -1.0;
    }
    let mut max_curv = [0.0_f32; 3];
    let roots = find_cubic_max_curvature(src, &mut max_curv) as usize;
    for &test_t in &max_curv[..roots] {
        if test_t <= 0.0 || test_t >= 1.0 {
            continue;
        }
        let d_pt = eval_cubic_derivative(src, test_t);
        if d_pt.length_sqd() < calc_cubic_precision(src) {
            return test_t;
        }
    }
    -1.0
}

#[inline]
fn close_enough_to_zero(x: f64) -> bool {
    x.abs() < 0.00001
}

fn first_axis_intersection(
    coefficients: &[f64; 8],
    y_direction: bool,
    axis_intercept: f64,
) -> Option<f64> {
    let [a, b, c, mut d] = bezier::cubic_convert_to_polynomial(coefficients, y_direction);
    d -= axis_intercept;
    let mut roots = [0.0_f64; 3];
    let count = cubics::roots_valid_t(a, b, c, d, &mut roots);
    for &r in &roots[..count] {
        if close_enough_to_zero(cubics::eval_at(a, b, c, d, r)) {
            return Some(r);
        }
    }
    let count = cubics::binary_search_roots_valid_t(a, b, c, d, &mut roots);
    for &r in &roots[..count] {
        if close_enough_to_zero(cubics::eval_at(a, b, c, d, r)) {
            return Some(r);
        }
    }
    None
}

pub fn chop_mono_cubic_at_y(src: &[Point; 4], y: f32, dst: &mut [Point; 7]) -> bool {
    let coeffs = [
        src[0].x as f64, src[0].y as f64,
        src[1].x as f64, src[1].y as f64,
        src[2].x as f64, src[2].y as f64,
        src[3].x as f64, src[3].y as f64,
    ];
    if let Some(sol) = first_axis_intersection(&coeffs, true, y as f64) {
        let mut pair = [0.0_f64; 14];
        bezier::cubic_subdivide(&coeffs, sol, &mut pair);
        for i in 0..7 {
            dst[i].x = pair[i * 2] as f32;
            dst[i].y = pair[i * 2 + 1] as f32;
        }
        true
    } else {
        false
    }
}

pub fn chop_mono_cubic_at_x(src: &[Point; 4], x: f32, dst: &mut [Point; 7]) -> bool {
    let coeffs = [
        src[0].x as f64, src[0].y as f64,
        src[1].x as f64, src[1].y as f64,
        src[2].x as f64, src[2].y as f64,
        src[3].x as f64, src[3].y as f64,
    ];
    if let Some(sol) = first_axis_intersection(&coeffs, false, x as f64) {
        let mut pair = [0.0_f64; 14];
        bezier::cubic_subdivide(&coeffs, sol, &mut pair);
        for i in 0..7 {
            dst[i].x = pair[i * 2] as f32;
            dst[i].y = pair[i * 2 + 1] as f32;
        }
        true
    } else {
        false
    }
}

// --- Conics ---

fn conic_deriv_coeff(c0: f32, c1: f32, c2: f32, w: f32) -> [f32; 3] {
    let p20 = c2 - c0;
    let p10 = c1 - c0;
    let w_p10 = w * p10;
    [w * p20 - p20, p20 - 2.0 * w_p10, w_p10]
}

fn conic_find_extrema(c0: f32, c1: f32, c2: f32, w: f32) -> Option<f32> {
    let coeff = conic_deriv_coeff(c0, c1, c2, w);
    let mut t_values = [0.0_f32; 2];
    let roots = find_unit_quad_roots(coeff[0], coeff[1], coeff[2], &mut t_values);
    if roots == 1 {
        Some(t_values[0])
    } else {
        None
    }
}

#[inline]
fn p3d_interp_coord(c0: f32, c1: f32, c2: f32, t: f32) -> (f32, f32, f32) {
    let ab = scalar_interp(c0, c1, t);
    let bc = scalar_interp(c1, c2, t);
    (ab, scalar_interp(ab, bc, t), bc)
}

#[inline]
fn ratquad_map_to_3d(pts: &[Point; 3], w: f32) -> [Point3; 3] {
    [
        Point3::new(pts[0].x, pts[0].y, 1.0),
        Point3::new(pts[1].x * w, pts[1].y * w, w),
        Point3::new(pts[2].x, pts[2].y, 1.0),
    ]
}

#[inline]
fn project_down(src: Point3) -> Point {
    Point::new(src.x / src.z, src.y / src.z)
}

#[inline]
fn subdivide_w_value(w: f32) -> f32 {
    (0.5_f32 + w * 0.5_f32).sqrt()
}

#[inline]
fn bad_conic_w(w: f32) -> bool {
    w < 0.0 || !w.is_finite()
}

#[inline]
fn between(a: f32, b: f32, c: f32) -> bool {
    (a - b) * (c - b) <= 0.0
}

fn subdivide_conic(src: &Conic, pts: &mut [Point], offset: usize, level: i32) -> usize {
    if level == 0 {
        pts[offset] = src.pts[1];
        pts[offset + 1] = src.pts[2];
        return offset + 2;
    }
    let mut dst = [Conic::default(); 2];
    src.chop(&mut dst);
    let start_y = src.pts[0].y;
    let end_y = src.pts[2].y;
    if between(start_y, src.pts[1].y, end_y) {
        let mid_y = dst[0].pts[2].y;
        if !between(start_y, mid_y, end_y) {
            let closer_y = if (mid_y - start_y).abs() < (mid_y - end_y).abs() {
                start_y
            } else {
                end_y
            };
            dst[0].pts[2].y = closer_y;
            dst[1].pts[0].y = closer_y;
        }
        if !between(start_y, dst[0].pts[1].y, dst[0].pts[2].y) {
            dst[0].pts[1].y = start_y;
        }
        if !between(dst[1].pts[0].y, dst[1].pts[1].y, end_y) {
            dst[1].pts[1].y = end_y;
        }
    }
    let next_off = subdivide_conic(&dst[0], pts, offset, level - 1);
    subdivide_conic(&dst[1], pts, next_off, level - 1)
}

struct ConicCoeff {
    numer_a: Point,
    numer_b: Point,
    numer_c: Point,
    denom_a: f32,
    denom_b: f32,
    denom_c: f32,
}

impl ConicCoeff {
    fn new(conic: &Conic) -> Self {
        let p0 = conic.pts[0];
        let p1 = conic.pts[1];
        let p2 = conic.pts[2];
        let w = conic.w;
        let p1w = Point::new(p1.x * w, p1.y * w);
        let numer_c = p0;
        let numer_a = Point::new(p2.x - (p1w.x + p1w.x) + p0.x, p2.y - (p1w.y + p1w.y) + p0.y);
        let numer_b = Point::new((p1w.x - p0.x) * 2.0, (p1w.y - p0.y) * 2.0);
        let denom_c = 1.0_f32;
        let denom_b = (w - denom_c) * 2.0;
        let denom_a = -denom_b;
        Self {
            numer_a,
            numer_b,
            numer_c,
            denom_a,
            denom_b,
            denom_c,
        }
    }

    #[inline]
    fn eval_numer(&self, t: f32) -> Point {
        Point::new(
            (self.numer_a.x * t + self.numer_b.x) * t + self.numer_c.x,
            (self.numer_a.y * t + self.numer_b.y) * t + self.numer_c.y,
        )
    }

    #[inline]
    fn eval_denom(&self, t: f32) -> f32 {
        (self.denom_a * t + self.denom_b) * t + self.denom_c
    }

    #[inline]
    fn eval(&self, t: f32) -> Point {
        let n = self.eval_numer(t);
        let d = self.eval_denom(t);
        Point::new(n.x / d, n.y / d)
    }
}

impl Conic {
    pub fn set(&mut self, pts: [Point; 3], w: f32) {
        self.pts = pts;
        self.w = if w > 0.0 && w.is_finite() { w } else { 1.0 };
    }

    pub fn chop_at(&self, t: f32, dst: &mut [Conic; 2]) -> bool {
        let tmp = ratquad_map_to_3d(&self.pts, self.w);
        let (x0, x1, x2) = p3d_interp_coord(tmp[0].x, tmp[1].x, tmp[2].x, t);
        let (y0, y1, y2) = p3d_interp_coord(tmp[0].y, tmp[1].y, tmp[2].y, t);
        let (z0, z1, z2) = p3d_interp_coord(tmp[0].z, tmp[1].z, tmp[2].z, t);

        let t0 = Point3::new(x0, y0, z0);
        let t1 = Point3::new(x1, y1, z1);
        let t2 = Point3::new(x2, y2, z2);

        dst[0].pts[0] = self.pts[0];
        dst[0].pts[1] = project_down(t0);
        dst[0].pts[2] = project_down(t1);
        dst[1].pts[0] = dst[0].pts[2];
        dst[1].pts[1] = project_down(t2);
        dst[1].pts[2] = self.pts[2];

        let root = t1.z.sqrt();
        dst[0].w = t0.z / root;
        dst[1].w = t2.z / root;

        let all = [
            dst[0].pts[0].x, dst[0].pts[0].y,
            dst[0].pts[1].x, dst[0].pts[1].y,
            dst[0].pts[2].x, dst[0].pts[2].y,
            dst[0].w,
            dst[1].pts[0].x, dst[1].pts[0].y,
            dst[1].pts[1].x, dst[1].pts[1].y,
            dst[1].pts[2].x, dst[1].pts[2].y,
            dst[1].w,
        ];
        all_finite(&all)
    }

    pub fn chop_at_range(&self, t1: f32, t2: f32, dst: &mut Conic) {
        if t1 == 0.0 || t2 == 1.0 {
            if t1 == 0.0 && t2 == 1.0 {
                *dst = *self;
                return;
            }
            let mut pair = [Conic::default(); 2];
            if self.chop_at(if t1 != 0.0 { t1 } else { t2 }, &mut pair) {
                *dst = pair[usize::from(t1 != 0.0)];
                return;
            }
        }
        let coeff = ConicCoeff::new(self);
        let a_xy = coeff.eval_numer(t1);
        let a_zz = coeff.eval_denom(t1);
        let mid_t = (t1 + t2) / 2.0;
        let d_xy = coeff.eval_numer(mid_t);
        let d_zz = coeff.eval_denom(mid_t);
        let c_xy = coeff.eval_numer(t2);
        let c_zz = coeff.eval_denom(t2);
        let b_xy = Point::new(
            (d_xy.x + d_xy.x) - (a_xy.x + c_xy.x) * 0.5,
            (d_xy.y + d_xy.y) - (a_xy.y + c_xy.y) * 0.5,
        );
        let b_zz = (d_zz + d_zz) - (a_zz + c_zz) * 0.5;
        dst.pts[0] = Point::new(a_xy.x / a_zz, a_xy.y / a_zz);
        dst.pts[1] = Point::new(b_xy.x / b_zz, b_xy.y / b_zz);
        dst.pts[2] = Point::new(c_xy.x / c_zz, c_xy.y / c_zz);
        dst.w = b_zz / (a_zz * c_zz).sqrt();
    }

    pub fn eval_at(&self, t: f32) -> Point {
        ConicCoeff::new(self).eval(t)
    }

    pub fn eval_tangent_at(&self, t: f32) -> Point {
        if (t == 0.0 && self.pts[0] == self.pts[1]) || (t == 1.0 && self.pts[1] == self.pts[2]) {
            return self.pts[2] - self.pts[0];
        }
        let p20 = self.pts[2] - self.pts[0];
        let p10 = self.pts[1] - self.pts[0];
        let c = p10 * self.w;
        let a = p20 * self.w - p20;
        let b = p20 - c - c;
        Point::new((a.x * t + b.x) * t + c.x, (a.y * t + b.y) * t + c.y)
    }

    pub fn chop(&self, dst: &mut [Conic; 2]) {
        let scale = 1.0_f32 / (1.0_f32 + self.w);
        let t0 = self.pts[0] * scale;
        let t1 = self.pts[1] * (self.w * scale);
        let t2 = self.pts[2] * scale;

        let p1 = t0 + t1;
        let p3 = t1 + t2;
        let p2 = Point::new(
            0.5 * t0.x + t1.x + 0.5 * t2.x,
            0.5 * t0.y + t1.y + 0.5 * t2.y,
        );

        dst[0].pts[0] = self.pts[0];
        dst[0].pts[1] = p1;
        dst[0].pts[2] = p2;
        dst[1].pts[0] = p2;
        dst[1].pts[1] = p3;
        dst[1].pts[2] = self.pts[2];

        let new_w = subdivide_w_value(self.w);
        dst[0].w = new_w;
        dst[1].w = new_w;
    }

    pub fn compute_as_quad_error(&self) -> Point {
        let a = self.w - 1.0;
        let k = a / (4.0 * (2.0 + a));
        let x = k * (self.pts[0].x - 2.0 * self.pts[1].x + self.pts[2].x);
        let y = k * (self.pts[0].y - 2.0 * self.pts[1].y + self.pts[2].y);
        Point::new(x, y)
    }

    pub fn as_quad_tol(&self, tol: f32) -> bool {
        let e = self.compute_as_quad_error();
        (e.x * e.x + e.y * e.y) <= tol * tol
    }

    pub fn compute_quad_pow2(&self, tol: f32) -> i32 {
        if tol < 0.0 || !tol.is_finite() || !points_are_finite(&self.pts) || bad_conic_w(self.w) {
            return 0;
        }
        let e = self.compute_as_quad_error();
        let mut error = (e.x * e.x + e.y * e.y).sqrt();
        let mut pow2 = 0;
        while pow2 < MAX_CONIC_TO_QUAD_POW2 {
            if error <= tol {
                break;
            }
            error *= 0.25;
            pow2 += 1;
        }
        pow2
    }

    pub fn chop_into_quads_pow2(&self, pts: &mut [Point], mut pow2: i32) -> i32 {
        if bad_conic_w(self.w) {
            pow2 = 0;
        }
        pts[0] = self.pts[0];
        let mut handled_extreme = false;
        if pow2 == MAX_CONIC_TO_QUAD_POW2 {
            let mut dst = [Conic::default(); 2];
            self.chop(&mut dst);
            if equals_within_tolerance(dst[0].pts[1], dst[0].pts[2])
                && equals_within_tolerance(dst[1].pts[0], dst[1].pts[1])
            {
                pts[1] = dst[0].pts[1];
                pts[2] = dst[0].pts[1];
                pts[3] = dst[0].pts[1];
                pts[4] = dst[1].pts[2];
                pow2 = 1;
                handled_extreme = true;
            }
        }
        if !handled_extreme {
            subdivide_conic(self, pts, 1, pow2);
        }
        let quad_count = 1_usize << (pow2 as u32);
        let pt_count = 2 * quad_count + 1;
        if !points_are_finite(&pts[..pt_count]) {
            for p in &mut pts[1..pt_count - 1] {
                *p = self.pts[1];
            }
        }
        quad_count as i32
    }

    pub fn find_mid_tangent(&self) -> f32 {
        let tan0 = self.pts[1] - self.pts[0];
        let tan1 = self.pts[2] - self.pts[1];
        let bisector = find_bisector(tan0, -tan1);
        let a_v = (self.pts[2] - self.pts[0]) * (self.w - 1.0);
        let b_v = (self.pts[2] - self.pts[0]) - (self.pts[1] - self.pts[0]) * (self.w * 2.0);
        let c_v = (self.pts[1] - self.pts[0]) * self.w;
        let a = bisector.dot(a_v);
        let b = bisector.dot(b_v);
        let c = bisector.dot(c_v);
        solve_quadratic_equation_for_midtangent(a, b, c, b * b - 4.0 * a * c)
    }

    pub fn find_x_extrema(&self) -> Option<f32> {
        conic_find_extrema(self.pts[0].x, self.pts[1].x, self.pts[2].x, self.w)
    }

    pub fn find_y_extrema(&self) -> Option<f32> {
        conic_find_extrema(self.pts[0].y, self.pts[1].y, self.pts[2].y, self.w)
    }

    pub fn chop_at_x_extrema(&self, dst: &mut [Conic; 2]) -> bool {
        if let Some(t) = self.find_x_extrema() {
            if !self.chop_at(t, dst) {
                return false;
            }
            let val = dst[0].pts[2].x;
            dst[0].pts[1].x = val;
            dst[1].pts[0].x = val;
            dst[1].pts[1].x = val;
            true
        } else {
            false
        }
    }

    pub fn chop_at_y_extrema(&self, dst: &mut [Conic; 2]) -> bool {
        if let Some(t) = self.find_y_extrema() {
            if !self.chop_at(t, dst) {
                return false;
            }
            let val = dst[0].pts[2].y;
            dst[0].pts[1].y = val;
            dst[1].pts[0].y = val;
            dst[1].pts[1].y = val;
            true
        } else {
            false
        }
    }

    pub fn compute_tight_bounds(&self) -> Rect {
        let mut pts = [Point::ZERO; 4];
        pts[0] = self.pts[0];
        pts[1] = self.pts[2];
        let mut count = 2usize;
        if let Some(t) = self.find_x_extrema() {
            pts[count] = self.eval_at(t);
            count += 1;
        }
        if let Some(t) = self.find_y_extrema() {
            pts[count] = self.eval_at(t);
            count += 1;
        }
        Rect::bounds_or_empty(&pts[..count])
    }

    pub fn compute_fast_bounds(&self) -> Rect {
        Rect::bounds_or_empty(&self.pts)
    }

    pub fn transform_w(pts: &[Point; 3], w: f32, matrix: &Matrix) -> f32 {
        if !matrix.has_perspective() {
            return w;
        }
        let src = ratquad_map_to_3d(pts, w);
        let dst0 = matrix.map_homogeneous_point(src[0]);
        let dst1 = matrix.map_homogeneous_point(src[1]);
        let dst2 = matrix.map_homogeneous_point(src[2]);
        let w0 = dst0.z as f64;
        let w1 = dst1.z as f64;
        let w2 = dst2.z as f64;
        ((w1 * w1) / (w0 * w2)).sqrt() as f32
    }

    /// `dir_is_ccw`: `false` for `SkPathDirection::kCW`, `true` for `kCCW`.
    pub fn build_unit_arc(
        u_start: Point,
        u_stop: Point,
        dir_is_ccw: bool,
        user_matrix: Option<&Matrix>,
        dst: &mut [Conic; MAX_CONICS_FOR_ARC],
    ) -> i32 {
        let x = u_start.dot(u_stop);
        let mut y = u_start.cross(u_stop);
        let abs_y = y.abs();

        if abs_y <= SK_SCALAR_NEARLY_ZERO
            && x > 0.0
            && ((y >= 0.0 && !dir_is_ccw) || (y <= 0.0 && dir_is_ccw))
        {
            return 0;
        }
        if dir_is_ccw {
            y = -y;
        }

        let mut quadrant = 0usize;
        if y == 0.0 {
            quadrant = 2;
        } else if x == 0.0 {
            quadrant = if y > 0.0 { 1 } else { 3 };
        } else {
            if y < 0.0 {
                quadrant += 2;
            }
            if (x < 0.0) != (y < 0.0) {
                quadrant += 1;
            }
        }

        const QUADRANT_PTS: [Point; 8] = [
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
            Point::new(-1.0, 1.0),
            Point::new(-1.0, 0.0),
            Point::new(-1.0, -1.0),
            Point::new(0.0, -1.0),
            Point::new(1.0, -1.0),
        ];

        let mut conic_count = quadrant;
        for i in 0..conic_count {
            dst[i].set(
                [QUADRANT_PTS[i * 2], QUADRANT_PTS[i * 2 + 1], QUADRANT_PTS[i * 2 + 2]],
                SK_SCALAR_ROOT_2_OVER_2,
            );
        }

        let final_p = Point::new(x, y);
        let last_q = QUADRANT_PTS[quadrant * 2];
        let dot = last_q.dot(final_p);
        if dot.is_nan() {
            return 0;
        }
        if dot < 1.0 {
            let mut off_curve = Point::new(last_q.x + x, last_q.y + y);
            let cos_theta_over_2 = ((1.0 + dot) / 2.0).sqrt();
            off_curve.set_length_in_place(1.0 / cos_theta_over_2);
            if !equals_within_tolerance(last_q, off_curve) {
                dst[conic_count].set([last_q, off_curve, final_p], cos_theta_over_2);
                conic_count += 1;
            }
        }

        let mut matrix = Matrix::identity();
        matrix.set_sin_cos(u_start.y, u_start.x);
        if dir_is_ccw {
            matrix.pre_scale(1.0, -1.0);
        }
        if let Some(um) = user_matrix {
            matrix.post_concat(um);
        }
        for conic in &mut dst[..conic_count] {
            matrix.map_points_in_place(&mut conic.pts);
        }
        conic_count as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quad_and_cubic_chops() {
        let q = [Point::new(0.0, 0.0), Point::new(1.0, 2.0), Point::new(2.0, 0.0)];
        let mid = eval_quad_at(&q, 0.5);
        assert_eq!(mid, Point::new(1.0, 1.0));
        let mut chopped = [Point::ZERO; 5];
        assert_eq!(chop_quad_at_y_extrema(&q, &mut chopped), 1);
        assert_eq!(chopped[2], Point::new(1.0, 1.0));

        let c = [
            Point::new(0.0, 0.0),
            Point::new(1.0, 3.0),
            Point::new(2.0, -3.0),
            Point::new(3.0, 0.0),
        ];
        let mut t_vals = [0.0; 2];
        assert_eq!(find_cubic_inflections(&c, &mut t_vals), 1);
        assert!((t_vals[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_conic_arc() {
        let mut conics = [Conic::default(); MAX_CONICS_FOR_ARC];
        let n = Conic::build_unit_arc(
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
            false,
            None,
            &mut conics,
        );
        assert_eq!(n, 1);
        assert!((conics[0].w - SK_SCALAR_ROOT_2_OVER_2).abs() < 1e-6);
    }
}
