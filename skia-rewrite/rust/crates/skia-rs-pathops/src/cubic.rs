#![forbid(unsafe_code)]

//! Port of `SkPathOpsCubic.h/.cpp`, `SkOpCubicHull.cpp`, and `SkDCubicToQuads.cpp`.

use skia_rs_geometry::{
    geometry::{classify_cubic, CubicType},
    Point,
};

use crate::conic::DConic;
use crate::intersections::Intersections;
use crate::line::LineParameters;
use crate::quad::DQuad;
use crate::types::{
    almost_bequal_ulps, almost_dequal_ulps_f64, approximately_equal, approximately_equal_half,
    approximately_one_or_less, approximately_zero, approximately_zero_or_more,
    approximately_zero_when_compared_to, between, cpp_max_f64, cpp_min_f64, d_interp,
    precisely_between, precisely_zero, roughly_between, zero_or_one, DPoint, DRect, DVector,
    FLT_EPSILON_ORDERABLE_ERR, SK_DOUBLE_PI,
};

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SearchAxis {
    XAxis = 0,
    YAxis = 1,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DCubic {
    pub pts: [DPoint; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DCubicPair {
    pub pts: [DPoint; 7],
}

impl DCubicPair {
    #[inline]
    pub fn first(&self) -> DCubic {
        DCubic {
            pts: [self.pts[0], self.pts[1], self.pts[2], self.pts[3]],
        }
    }

    #[inline]
    pub fn second(&self) -> DCubic {
        DCubic {
            pts: [self.pts[3], self.pts[4], self.pts[5], self.pts[6]],
        }
    }
}

#[inline]
pub fn other_two(one: usize, two: usize) -> usize {
    (1usize >> (3 - (one ^ two))) ^ 3
}

#[inline]
fn derivative_at_t(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    let one_t = 1.0 - t;
    3.0 * ((b - a) * one_t * one_t + 2.0 * (c - b) * t * one_t + (d - c) * t * t)
}

#[inline]
fn interp_cubic_coord(s0: f64, s1: f64, s2: f64, s3: f64, t: f64) -> f64 {
    let ab = d_interp(s0, s1, t);
    let bc = d_interp(s1, s2, t);
    let cd = d_interp(s2, s3, t);
    let abc = d_interp(ab, bc, t);
    let bcd = d_interp(bc, cd, t);
    d_interp(abc, bcd, t)
}

#[inline]
fn formulate_f1_dot_f2(s0: f64, s1: f64, s2: f64, s3: f64) -> [f64; 4] {
    let a = s1 - s0;
    let b = s2 - 2.0 * s1 + s0;
    let c = s3 + 3.0 * (s1 - s2) - s0;
    [c * c, 3.0 * b * c, 2.0 * b * b + c * a, a * b]
}

fn rotate_cubic(cubic: &DCubic, zero: usize, index: usize) -> Option<DCubic> {
    let dy = cubic.pts[index].y - cubic.pts[zero].y;
    let dx = cubic.pts[index].x - cubic.pts[zero].x;
    if approximately_zero(dy) {
        if approximately_zero(dx) {
            return None;
        }
        let mut rot_path = *cubic;
        if dy != 0.0 {
            rot_path.pts[index].y = cubic.pts[zero].y;
            let mask = other_two(index, zero);
            let side1 = index ^ mask;
            let side2 = zero ^ mask;
            if approximately_equal(cubic.pts[side1].y, cubic.pts[zero].y) {
                rot_path.pts[side1].y = cubic.pts[zero].y;
            }
            if approximately_equal(cubic.pts[side2].y, cubic.pts[zero].y) {
                rot_path.pts[side2].y = cubic.pts[zero].y;
            }
        }
        return Some(rot_path);
    }
    let mut rot_path = DCubic::default();
    for i in 0..4 {
        rot_path.pts[i].x = cubic.pts[i].x * dx + cubic.pts[i].y * dy;
        rot_path.pts[i].y = cubic.pts[i].y * dx - cubic.pts[i].x * dy;
    }
    Some(rot_path)
}

#[inline]
fn hull_side(x: f64) -> i32 {
    (x > 0.0) as i32 + (x >= 0.0) as i32
}

impl DCubic {
    pub const POINT_COUNT: usize = 4;
    pub const POINT_LAST: usize = 3;
    pub const MAX_INTERSECTIONS: usize = 9;
    pub const PRECISION_UNIT: i32 = 256;

    #[inline]
    pub const fn new(p0: DPoint, p1: DPoint, p2: DPoint, p3: DPoint) -> Self {
        Self {
            pts: [p0, p1, p2, p3],
        }
    }

    #[inline]
    pub fn from_points(pts: [Point; 4]) -> Self {
        Self {
            pts: [
                DPoint::from_point(pts[0]),
                DPoint::from_point(pts[1]),
                DPoint::from_point(pts[2]),
                DPoint::from_point(pts[3]),
            ],
        }
    }

    #[inline]
    pub fn collapsed(&self) -> bool {
        self.pts[0].approximately_equal(self.pts[1])
            && self.pts[0].approximately_equal(self.pts[2])
            && self.pts[0].approximately_equal(self.pts[3])
    }

    #[inline]
    pub fn controls_inside(&self) -> bool {
        let v01 = self.pts[0].sub(self.pts[1]);
        let v02 = self.pts[0].sub(self.pts[2]);
        let v03 = self.pts[0].sub(self.pts[3]);
        let v13 = self.pts[1].sub(self.pts[3]);
        let v23 = self.pts[2].sub(self.pts[3]);
        v03.dot(v01) > 0.0 && v03.dot(v02) > 0.0 && v03.dot(v13) > 0.0 && v03.dot(v23) > 0.0
    }

    #[inline]
    pub fn align(&self, end_index: usize, ctrl_index: usize, dst_pt: &mut DPoint) {
        if self.pts[end_index].x == self.pts[ctrl_index].x {
            dst_pt.x = self.pts[end_index].x;
        }
        if self.pts[end_index].y == self.pts[ctrl_index].y {
            dst_pt.y = self.pts[end_index].y;
        }
    }

    pub fn binary_search(&self, min: f64, max: f64, axis_intercept: f64, axis: SearchAxis) -> f64 {
        let mut t = (min + max) / 2.0;
        let mut step = (t - min) / 2.0;
        let mut cubic_at_t = self.pt_at_t(t);
        let coord = |pt: DPoint| match axis {
            SearchAxis::XAxis => pt.x,
            SearchAxis::YAxis => pt.y,
        };
        let mut calc_pos = coord(cubic_at_t);
        let mut calc_dist = calc_pos - axis_intercept;
        loop {
            let prior_t = cpp_max_f64(min, t - step);
            let less_pt = self.pt_at_t(prior_t);
            if approximately_equal_half(less_pt.x, cubic_at_t.x)
                && approximately_equal_half(less_pt.y, cubic_at_t.y)
            {
                return -1.0;
            }
            let less_dist = coord(less_pt) - axis_intercept;
            let last_step = step;
            step /= 2.0;
            if if calc_dist > 0.0 {
                calc_dist > less_dist
            } else {
                calc_dist < less_dist
            } {
                t = prior_t;
            } else {
                let next_t = t + last_step;
                if next_t > max {
                    return -1.0;
                }
                let more_pt = self.pt_at_t(next_t);
                if approximately_equal_half(more_pt.x, cubic_at_t.x)
                    && approximately_equal_half(more_pt.y, cubic_at_t.y)
                {
                    return -1.0;
                }
                let more_dist = coord(more_pt) - axis_intercept;
                if if calc_dist > 0.0 {
                    calc_dist <= more_dist
                } else {
                    calc_dist >= more_dist
                } {
                    if approximately_equal(calc_pos, axis_intercept) {
                        break;
                    }
                    continue;
                }
                t = next_t;
            }
            cubic_at_t = self.pt_at_t(t);
            calc_pos = coord(cubic_at_t);
            calc_dist = calc_pos - axis_intercept;
            if approximately_equal(calc_pos, axis_intercept) {
                break;
            }
        }
        t
    }

    pub fn calc_precision(&self) -> f64 {
        (self.pts[1].sub(self.pts[0]).length()
            + self.pts[2].sub(self.pts[1]).length()
            + self.pts[3].sub(self.pts[2]).length())
            / (Self::PRECISION_UNIT as f64)
    }

    pub fn chop_at(&self, t: f64) -> DCubicPair {
        let mut dst = DCubicPair::default();
        if t == 0.5 {
            dst.pts[0] = self.pts[0];
            dst.pts[1].x = (self.pts[0].x + self.pts[1].x) / 2.0;
            dst.pts[1].y = (self.pts[0].y + self.pts[1].y) / 2.0;
            dst.pts[2].x = (self.pts[0].x + 2.0 * self.pts[1].x + self.pts[2].x) / 4.0;
            dst.pts[2].y = (self.pts[0].y + 2.0 * self.pts[1].y + self.pts[2].y) / 4.0;
            dst.pts[3].x =
                (self.pts[0].x + 3.0 * (self.pts[1].x + self.pts[2].x) + self.pts[3].x) / 8.0;
            dst.pts[3].y =
                (self.pts[0].y + 3.0 * (self.pts[1].y + self.pts[2].y) + self.pts[3].y) / 8.0;
            dst.pts[4].x = (self.pts[1].x + 2.0 * self.pts[2].x + self.pts[3].x) / 4.0;
            dst.pts[4].y = (self.pts[1].y + 2.0 * self.pts[2].y + self.pts[3].y) / 4.0;
            dst.pts[5].x = (self.pts[2].x + self.pts[3].x) / 2.0;
            dst.pts[5].y = (self.pts[2].y + self.pts[3].y) / 2.0;
            dst.pts[6] = self.pts[3];
            return dst;
        }
        let abx = d_interp(self.pts[0].x, self.pts[1].x, t);
        let bcx = d_interp(self.pts[1].x, self.pts[2].x, t);
        let cdx = d_interp(self.pts[2].x, self.pts[3].x, t);
        let abcx = d_interp(abx, bcx, t);
        let bcdx = d_interp(bcx, cdx, t);
        let abcdx = d_interp(abcx, bcdx, t);

        let aby = d_interp(self.pts[0].y, self.pts[1].y, t);
        let bcy = d_interp(self.pts[1].y, self.pts[2].y, t);
        let cdy = d_interp(self.pts[2].y, self.pts[3].y, t);
        let abcy = d_interp(aby, bcy, t);
        let bcdy = d_interp(bcy, cdy, t);
        let abcdy = d_interp(abcy, bcdy, t);

        dst.pts[0] = self.pts[0];
        dst.pts[1] = DPoint::new(abx, aby);
        dst.pts[2] = DPoint::new(abcx, abcy);
        dst.pts[3] = DPoint::new(abcdx, abcdy);
        dst.pts[4] = DPoint::new(bcdx, bcdy);
        dst.pts[5] = DPoint::new(cdx, cdy);
        dst.pts[6] = self.pts[3];
        dst
    }

    #[inline]
    pub fn coefficients(s0: f64, s1: f64, s2: f64, s3: f64) -> (f64, f64, f64, f64) {
        let mut a = s3;
        let mut b = s2 * 3.0;
        let mut c = s1 * 3.0;
        let d = s0;
        a -= d - c + b;
        b += 3.0 * d - 2.0 * c;
        c -= 3.0 * d;
        (a, b, c, d)
    }

    pub fn ends_are_extrema_in_x_or_y(&self) -> bool {
        (between(self.pts[0].x, self.pts[1].x, self.pts[3].x)
            && between(self.pts[0].x, self.pts[2].x, self.pts[3].x))
            || (between(self.pts[0].y, self.pts[1].y, self.pts[3].y)
                && between(self.pts[0].y, self.pts[2].y, self.pts[3].y))
    }

    pub fn convex_hull(&self, order: &mut [u8; 4]) -> usize {
        let mut y_min = 0usize;
        for index in 1..4 {
            if self.pts[y_min].y > self.pts[index].y
                || (self.pts[y_min].y == self.pts[index].y
                    && self.pts[y_min].x > self.pts[index].x)
            {
                y_min = index;
            }
        }
        order[0] = y_min as u8;
        let mut mid_x: i32 = -1;
        let mut backup_y_min: i32 = -1;
        for _pass in 0..2 {
            for index in 0..4 {
                if index == y_min {
                    continue;
                }
                let mask = other_two(y_min, index);
                let side1 = y_min ^ mask;
                let side2 = index ^ mask;
                let rot_path = match rotate_cubic(self, y_min, index) {
                    Some(rp) => rp,
                    None => {
                        order[1] = side1 as u8;
                        order[2] = side2 as u8;
                        return 3;
                    }
                };
                let mut sides = hull_side(rot_path.pts[side1].y - rot_path.pts[y_min].y);
                sides ^= hull_side(rot_path.pts[side2].y - rot_path.pts[y_min].y);
                if sides == 2 {
                    if mid_x >= 0 {
                        order[0] = 0;
                        order[1] = 3;
                        if self.pts[1] == self.pts[0] || self.pts[1] == self.pts[3] {
                            order[2] = 2;
                            return 3;
                        }
                        if self.pts[2] == self.pts[0] || self.pts[2] == self.pts[3] {
                            order[2] = 1;
                            return 3;
                        }
                        let dist1_0 = self.pts[1].distance_squared(self.pts[0]);
                        let dist1_3 = self.pts[1].distance_squared(self.pts[3]);
                        let dist2_0 = self.pts[2].distance_squared(self.pts[0]);
                        let dist2_3 = self.pts[2].distance_squared(self.pts[3]);
                        let smallest1 = cpp_min_f64(dist1_0, dist1_3);
                        let smallest2 = cpp_min_f64(dist2_0, dist2_3);
                        if approximately_zero(cpp_min_f64(smallest1, smallest2)) {
                            order[2] = if smallest1 < smallest2 { 2 } else { 1 };
                            return 3;
                        }
                    }
                    mid_x = index as i32;
                } else if sides == 0 {
                    backup_y_min = index as i32;
                }
            }
            if mid_x >= 0 {
                break;
            }
            if backup_y_min < 0 {
                break;
            }
            y_min = backup_y_min as usize;
            backup_y_min = -1;
        }
        let mid_x = if mid_x < 0 { y_min ^ 3 } else { mid_x as usize };
        let mask = other_two(y_min, mid_x);
        let least = y_min ^ mask;
        let most = mid_x ^ mask;
        order[0] = y_min as u8;
        order[1] = least as u8;

        let mid_path = match rotate_cubic(self, least, most) {
            Some(mp) => mp,
            None => {
                order[2] = mid_x as u8;
                return 3;
            }
        };
        let mut mid_sides = hull_side(mid_path.pts[y_min].y - mid_path.pts[least].y);
        mid_sides ^= hull_side(mid_path.pts[mid_x].y - mid_path.pts[least].y);
        if mid_sides != 2 {
            order[2] = most as u8;
            return 3;
        }
        order[2] = mid_x as u8;
        order[3] = most as u8;
        4
    }

    pub fn hull_intersects_pts(&self, pts: &[DPoint], is_linear: &mut bool) -> bool {
        let mut linear = true;
        let mut hull_order = [0u8; 4];
        let hull_count = self.convex_hull(&mut hull_order);
        let mut end1 = hull_order[0] as usize;
        let mut hull_index = 0usize;
        let mut end_pt0 = self.pts[end1];
        loop {
            hull_index = (hull_index + 1) % hull_count;
            let end2 = hull_order[hull_index] as usize;
            let end_pt1 = self.pts[end2];
            let orig_x = end_pt0.x;
            let orig_y = end_pt0.y;
            let adj = end_pt1.x - orig_x;
            let opp = end_pt1.y - orig_y;
            let odd_man_mask = other_two(end1, end2);
            let odd_man = end1 ^ odd_man_mask;
            let mut sign = (self.pts[odd_man].y - orig_y) * adj - (self.pts[odd_man].x - orig_x) * opp;
            let odd_man2 = end2 ^ odd_man_mask;
            let sign2 = (self.pts[odd_man2].y - orig_y) * adj - (self.pts[odd_man2].x - orig_x) * opp;
            if sign * sign2 < 0.0 {
                if hull_index == 0 {
                    break;
                }
                continue;
            }
            if approximately_zero(sign) {
                sign = sign2;
                if approximately_zero(sign) {
                    if hull_index == 0 {
                        break;
                    }
                    continue;
                }
            }
            linear = false;
            let mut found_outlier = false;
            for pt in pts {
                let test = (pt.y - orig_y) * adj - (pt.x - orig_x) * opp;
                if test * sign > 0.0 && !precisely_zero(test) {
                    found_outlier = true;
                    break;
                }
            }
            if !found_outlier {
                return false;
            }
            end_pt0 = end_pt1;
            end1 = end2;
            if hull_index == 0 {
                break;
            }
        }
        *is_linear = linear;
        true
    }

    #[inline]
    pub fn hull_intersects_cubic(&self, c2: &DCubic, is_linear: &mut bool) -> bool {
        self.hull_intersects_pts(&c2.pts, is_linear)
    }

    #[inline]
    pub fn hull_intersects_quad(&self, quad: &DQuad, is_linear: &mut bool) -> bool {
        self.hull_intersects_pts(&quad.pts, is_linear)
    }

    #[inline]
    pub fn hull_intersects_conic(&self, conic: &DConic, is_linear: &mut bool) -> bool {
        self.hull_intersects_quad(&conic.pts, is_linear)
    }

    pub fn is_linear(&self, start_index: usize, end_index: usize) -> bool {
        if self.pts[0].approximately_dequal(self.pts[3]) {
            let q = DQuad {
                pts: [self.pts[0], self.pts[1], self.pts[2]],
            };
            return q.is_linear(0, 2);
        }
        let mut line_params = LineParameters::default();
        line_params.cubic_end_points(self, start_index, end_index);
        line_params.normalize();
        let tiniest = cpp_min_f64(
            cpp_min_f64(
                cpp_min_f64(
                    cpp_min_f64(
                        cpp_min_f64(
                            cpp_min_f64(
                                cpp_min_f64(self.pts[0].x, self.pts[0].y),
                                self.pts[1].x,
                            ),
                            self.pts[1].y,
                        ),
                        self.pts[2].x,
                    ),
                    self.pts[2].y,
                ),
                self.pts[3].x,
            ),
            self.pts[3].y,
        );
        let mut largest = cpp_max_f64(
            cpp_max_f64(
                cpp_max_f64(
                    cpp_max_f64(
                        cpp_max_f64(
                            cpp_max_f64(
                                cpp_max_f64(self.pts[0].x, self.pts[0].y),
                                self.pts[1].x,
                            ),
                            self.pts[1].y,
                        ),
                        self.pts[2].x,
                    ),
                    self.pts[2].y,
                ),
                self.pts[3].x,
            ),
            self.pts[3].y,
        );
        largest = cpp_max_f64(largest, -tiniest);
        let distance = line_params.control_pt_distance_cubic(self, 1);
        if !approximately_zero_when_compared_to(distance, largest) {
            return false;
        }
        let distance = line_params.control_pt_distance_cubic(self, 2);
        approximately_zero_when_compared_to(distance, largest)
    }

    pub fn complex_break(points: &[Point; 4], t: &mut [f32]) -> usize {
        let cubic = Self::from_points(*points);
        if cubic.monotonic_in_x() && cubic.monotonic_in_y() {
            return 0;
        }
        let mut tt = [0.0f64; 2];
        let mut ss = [0.0f64; 2];
        let cubic_type = classify_cubic(points, Some(&mut tt), Some(&mut ss), None);
        match cubic_type {
            CubicType::Loop => {
                let (td, te, sd, se) = (tt[0], tt[1], ss[0], ss[1]);
                if roughly_between(0.0, td, sd) && roughly_between(0.0, te, se) {
                    t[0] = ((td * se + te * sd) / (2.0 * sd * se)) as f32;
                    return usize::from(t[0] > 0.0 && t[0] < 1.0);
                }
                // fallthrough
                Self::complex_break_serpentine(&cubic, t)
            }
            CubicType::Serpentine | CubicType::LocalCusp | CubicType::CuspAtInfinity => {
                Self::complex_break_serpentine(&cubic, t)
            }
            _ => 0,
        }
    }

    fn complex_break_serpentine(cubic: &DCubic, t: &mut [f32]) -> usize {
        let mut inflection_ts = [0.0f64; 2];
        let inf_t_count = cubic.find_inflections(&mut inflection_ts);
        let mut max_curvature = [0.0f64; 3];
        let roots = cubic.find_max_curvature(&mut max_curvature);
        if inf_t_count == 2 {
            for &mc in max_curvature.iter().take(roots) {
                if between(inflection_ts[0], mc, inflection_ts[1]) {
                    t[0] = mc as f32;
                    return usize::from(t[0] > 0.0 && t[0] < 1.0);
                }
            }
            0
        } else {
            let mut result_count = 0usize;
            let precision = cubic.calc_precision() * 2.0;
            for &test_t in max_curvature.iter().take(roots) {
                if test_t <= 0.0 || test_t >= 1.0 {
                    continue;
                }
                let d_pt = DVector {
                    x: derivative_at_t(
                        cubic.pts[0].x,
                        cubic.pts[1].x,
                        cubic.pts[2].x,
                        cubic.pts[3].x,
                        test_t,
                    ),
                    y: derivative_at_t(
                        cubic.pts[0].y,
                        cubic.pts[1].y,
                        cubic.pts[2].y,
                        cubic.pts[3].y,
                        test_t,
                    ),
                };
                if d_pt.length() < precision {
                    t[result_count] = test_t as f32;
                    result_count += 1;
                }
            }
            if result_count == 0 && inf_t_count == 1 {
                t[0] = inflection_ts[0] as f32;
                result_count = usize::from(t[0] > 0.0 && t[0] < 1.0);
            }
            result_count
        }
    }

    #[inline]
    pub fn monotonic_in_x(&self) -> bool {
        precisely_between(self.pts[0].x, self.pts[1].x, self.pts[3].x)
            && precisely_between(self.pts[0].x, self.pts[2].x, self.pts[3].x)
    }

    #[inline]
    pub fn monotonic_in_y(&self) -> bool {
        precisely_between(self.pts[0].y, self.pts[1].y, self.pts[3].y)
            && precisely_between(self.pts[0].y, self.pts[2].y, self.pts[3].y)
    }

    #[inline]
    pub fn other_pts(&self, index: usize) -> [DPoint; 3] {
        let offset = usize::from(index == 0);
        [
            self.pts[offset],
            self.pts[offset + 1],
            self.pts[offset + 2],
        ]
    }

    pub fn search_roots(
        &self,
        extreme_ts: &mut [f64; 6],
        mut extrema: usize,
        axis_intercept: f64,
        axis: SearchAxis,
        valid_roots: &mut [f64; 3],
    ) -> usize {
        let mut inf = [0.0f64; 2];
        let inf_count = self.find_inflections(&mut inf);
        for &v in inf.iter().take(inf_count) {
            extreme_ts[extrema] = v;
            extrema += 1;
        }
        extreme_ts[extrema] = 0.0;
        extrema += 1;
        extreme_ts[extrema] = 1.0;
        // Sort extreme_ts[0..=extrema]
        extreme_ts[..=extrema].sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let mut valid_count = 0usize;
        for index in 0..extrema {
            let min = extreme_ts[index];
            let max = extreme_ts[index + 1];
            if min == max {
                continue;
            }
            let new_t = self.binary_search(min, max, axis_intercept, axis);
            if new_t >= 0.0 {
                if valid_count >= 3 {
                    return 0;
                }
                valid_roots[valid_count] = new_t;
                valid_count += 1;
            }
        }
        valid_count
    }

    pub fn roots_valid_t(a: f64, b: f64, c: f64, d: f64, t: &mut [f64; 3]) -> usize {
        let mut s = [0.0f64; 3];
        let real_roots = Self::roots_real(a, b, c, d, &mut s);
        let mut found_roots = DQuad::add_valid_ts(&s, real_roots, t);
        for &t_value in s.iter().take(real_roots) {
            if !approximately_one_or_less(t_value) && between(1.0, t_value, 1.00005) {
                let mut dup = false;
                for &existing in t.iter().take(found_roots) {
                    if approximately_equal(existing, 1.0) {
                        dup = true;
                        break;
                    }
                }
                if !dup && found_roots < 3 {
                    t[found_roots] = 1.0;
                    found_roots += 1;
                }
            } else if !approximately_zero_or_more(t_value) && between(-0.00005, t_value, 0.0) {
                let mut dup = false;
                for &existing in t.iter().take(found_roots) {
                    if approximately_equal(existing, 0.0) {
                        dup = true;
                        break;
                    }
                }
                if !dup && found_roots < 3 {
                    t[found_roots] = 0.0;
                    found_roots += 1;
                }
            }
        }
        found_roots
    }

    pub fn roots_real(a_in: f64, b_in: f64, c_in: f64, d_in: f64, s: &mut [f64; 3]) -> usize {
        if approximately_zero(a_in)
            && approximately_zero_when_compared_to(a_in, b_in)
            && approximately_zero_when_compared_to(a_in, c_in)
            && approximately_zero_when_compared_to(a_in, d_in)
        {
            let mut qs = [0.0; 2];
            let n = DQuad::roots_real(b_in, c_in, d_in, &mut qs);
            for i in 0..n {
                s[i] = qs[i];
            }
            return n;
        }
        if approximately_zero_when_compared_to(d_in, a_in)
            && approximately_zero_when_compared_to(d_in, b_in)
            && approximately_zero_when_compared_to(d_in, c_in)
        {
            let mut qs = [0.0; 2];
            let mut num = DQuad::roots_real(a_in, b_in, c_in, &mut qs);
            for i in 0..num {
                s[i] = qs[i];
            }
            for i in 0..num {
                if approximately_zero(s[i]) {
                    return num;
                }
            }
            s[num] = 0.0;
            num += 1;
            return num;
        }
        if approximately_zero(a_in + b_in + c_in + d_in) {
            let mut qs = [0.0; 2];
            let mut num = DQuad::roots_real(a_in, a_in + b_in, -d_in, &mut qs);
            for i in 0..num {
                s[i] = qs[i];
            }
            for i in 0..num {
                if almost_dequal_ulps_f64(s[i], 1.0) {
                    return num;
                }
            }
            s[num] = 1.0;
            num += 1;
            return num;
        }
        let inv_a = 1.0 / a_in;
        let a = b_in * inv_a;
        let b = c_in * inv_a;
        let c = d_in * inv_a;
        let a2 = a * a;
        let q = (a2 - b * 3.0) / 9.0;
        let r_val = (2.0 * a2 * a - 9.0 * a * b + 27.0 * c) / 54.0;
        let r2 = r_val * r_val;
        let q3 = q * q * q;
        let r2_minus_q3 = r2 - q3;
        let adiv3 = a / 3.0;
        let mut roots_len = 0usize;
        if r2_minus_q3 < 0.0 {
            let theta = cpp_max_f64(-1.0, cpp_min_f64(r_val / q3.sqrt(), 1.0)).acos();
            let neg2_root_q = -2.0 * q.sqrt();
            let r = neg2_root_q * (theta / 3.0).cos() - adiv3;
            s[roots_len] = r;
            roots_len += 1;

            let r = neg2_root_q * ((theta + 2.0 * SK_DOUBLE_PI) / 3.0).cos() - adiv3;
            if !almost_dequal_ulps_f64(s[0], r) {
                s[roots_len] = r;
                roots_len += 1;
            }
            let r = neg2_root_q * ((theta - 2.0 * SK_DOUBLE_PI) / 3.0).cos() - adiv3;
            if !almost_dequal_ulps_f64(s[0], r)
                && (roots_len == 1 || !almost_dequal_ulps_f64(s[1], r))
            {
                s[roots_len] = r;
                roots_len += 1;
            }
        } else {
            let sqrt_r2_minus_q3 = r2_minus_q3.sqrt();
            let mut aa = (r_val.abs() + sqrt_r2_minus_q3).cbrt();
            if r_val > 0.0 {
                aa = -aa;
            }
            if aa != 0.0 {
                aa += q / aa;
            }
            let r = aa - adiv3;
            s[roots_len] = r;
            roots_len += 1;
            if almost_dequal_ulps_f64(r2, q3) {
                let r = -aa / 2.0 - adiv3;
                if !almost_dequal_ulps_f64(s[0], r) {
                    s[roots_len] = r;
                    roots_len += 1;
                }
            }
        }
        roots_len
    }

    pub fn dxdy_at_t(&self, t: f64) -> DVector {
        let mut result = DVector {
            x: derivative_at_t(
                self.pts[0].x,
                self.pts[1].x,
                self.pts[2].x,
                self.pts[3].x,
                t,
            ),
            y: derivative_at_t(
                self.pts[0].y,
                self.pts[1].y,
                self.pts[2].y,
                self.pts[3].y,
                t,
            ),
        };
        if result.x == 0.0 && result.y == 0.0 {
            if t == 0.0 {
                result = self.pts[2].sub(self.pts[0]);
            } else if t == 1.0 {
                result = self.pts[3].sub(self.pts[1]);
            }
            if result.x == 0.0 && result.y == 0.0 && zero_or_one(t) {
                result = self.pts[3].sub(self.pts[0]);
            }
        }
        result
    }

    pub fn find_inflections(&self, t_values: &mut [f64; 2]) -> usize {
        let ax = self.pts[1].x - self.pts[0].x;
        let ay = self.pts[1].y - self.pts[0].y;
        let bx = self.pts[2].x - 2.0 * self.pts[1].x + self.pts[0].x;
        let by = self.pts[2].y - 2.0 * self.pts[1].y + self.pts[0].y;
        let cx = self.pts[3].x + 3.0 * (self.pts[1].x - self.pts[2].x) - self.pts[0].x;
        let cy = self.pts[3].y + 3.0 * (self.pts[1].y - self.pts[2].y) - self.pts[0].y;
        DQuad::roots_valid_t(
            bx * cy - by * cx,
            ax * cy - ay * cx,
            ax * by - ay * bx,
            t_values,
        )
    }

    pub fn find_extrema(a: f64, b: f64, c: f64, d: f64, t_values: &mut [f64; 2]) -> usize {
        let ca = d - a + 3.0 * (b - c);
        let cb = 2.0 * (a - b - b + c);
        let cc = b - a;
        DQuad::roots_valid_t(ca, cb, cc, t_values)
    }

    pub fn find_max_curvature(&self, t_values: &mut [f64; 3]) -> usize {
        let mut coeff_x = formulate_f1_dot_f2(
            self.pts[0].x,
            self.pts[1].x,
            self.pts[2].x,
            self.pts[3].x,
        );
        let coeff_y = formulate_f1_dot_f2(
            self.pts[0].y,
            self.pts[1].y,
            self.pts[2].y,
            self.pts[3].y,
        );
        for i in 0..4 {
            coeff_x[i] += coeff_y[i];
        }
        Self::roots_valid_t(coeff_x[0], coeff_x[1], coeff_x[2], coeff_x[3], t_values)
    }

    #[inline]
    pub fn pt_at_t(&self, t: f64) -> DPoint {
        if t == 0.0 {
            return self.pts[0];
        }
        if t == 1.0 {
            return self.pts[3];
        }
        let one_t = 1.0 - t;
        let one_t2 = one_t * one_t;
        let a = one_t2 * one_t;
        let b = 3.0 * one_t2 * t;
        let t2 = t * t;
        let c = 3.0 * one_t * t2;
        let d = t2 * t;
        DPoint {
            x: a * self.pts[0].x + b * self.pts[1].x + c * self.pts[2].x + d * self.pts[3].x,
            y: a * self.pts[0].y + b * self.pts[1].y + c * self.pts[2].y + d * self.pts[3].y,
        }
    }

    pub fn sub_divide(&self, t1: f64, t2: f64) -> Self {
        if t1 == 0.0 || t2 == 1.0 {
            if t1 == 0.0 && t2 == 1.0 {
                return *self;
            }
            let pair = self.chop_at(if t1 == 0.0 { t2 } else { t1 });
            return if t1 == 0.0 {
                pair.first()
            } else {
                pair.second()
            };
        }
        let ax = interp_cubic_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, self.pts[3].x, t1);
        let ay = interp_cubic_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, self.pts[3].y, t1);
        let t_e = (t1 * 2.0 + t2) / 3.0;
        let ex = interp_cubic_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, self.pts[3].x, t_e);
        let ey = interp_cubic_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, self.pts[3].y, t_e);
        let t_f = (t1 + t2 * 2.0) / 3.0;
        let fx = interp_cubic_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, self.pts[3].x, t_f);
        let fy = interp_cubic_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, self.pts[3].y, t_f);
        let dx = interp_cubic_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, self.pts[3].x, t2);
        let dy = interp_cubic_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, self.pts[3].y, t2);
        let mx = ex * 27.0 - ax * 8.0 - dx;
        let my = ey * 27.0 - ay * 8.0 - dy;
        let nx = fx * 27.0 - ax - dx * 8.0;
        let ny = fy * 27.0 - ay - dy * 8.0;
        DCubic {
            pts: [
                DPoint::new(ax, ay),
                DPoint::new((mx * 2.0 - nx) / 18.0, (my * 2.0 - ny) / 18.0),
                DPoint::new((nx * 2.0 - mx) / 18.0, (ny * 2.0 - my) / 18.0),
                DPoint::new(dx, dy),
            ],
        }
    }

    pub fn sub_divide_pts(&self, a: DPoint, d: DPoint, t1: f64, t2: f64) -> [DPoint; 2] {
        let sub = self.sub_divide(t1, t2);
        let mut dst = [
            sub.pts[1].add_vec(a.sub(sub.pts[0])),
            sub.pts[2].add_vec(d.sub(sub.pts[3])),
        ];
        if t1 == 0.0 || t2 == 0.0 {
            let idx = usize::from(t1 != 0.0);
            self.align(0, 1, &mut dst[idx]);
        }
        if t1 == 1.0 || t2 == 1.0 {
            let idx = usize::from(t1 != 1.0);
            self.align(3, 2, &mut dst[idx]);
        }
        if almost_bequal_ulps(dst[0].x, a.x) {
            dst[0].x = a.x;
        }
        if almost_bequal_ulps(dst[0].y, a.y) {
            dst[0].y = a.y;
        }
        if almost_bequal_ulps(dst[1].x, d.x) {
            dst[1].x = d.x;
        }
        if almost_bequal_ulps(dst[1].y, d.y) {
            dst[1].y = d.y;
        }
        dst
    }

    pub fn to_float_points(&self, pts: &mut [Point; 4]) -> bool {
        let mut all_finite = true;
        for i in 0..4 {
            let mut x = self.pts[i].x as f32;
            let mut y = self.pts[i].y as f32;
            if (x.abs() as f64) < FLT_EPSILON_ORDERABLE_ERR {
                x = 0.0;
            }
            if (y.abs() as f64) < FLT_EPSILON_ORDERABLE_ERR {
                y = 0.0;
            }
            pts[i] = Point { x, y };
            if !x.is_finite() || !y.is_finite() {
                all_finite = false;
            }
        }
        all_finite
    }

    pub fn top(&self, d_curve: &DCubic, start_t: f64, end_t: f64, top_pt: &mut DPoint) -> f64 {
        let mut extreme_ts = [0.0f64; 2];
        let mut top_t = -1.0;
        let roots = Self::find_extrema(
            self.pts[0].y,
            self.pts[1].y,
            self.pts[2].y,
            self.pts[3].y,
            &mut extreme_ts,
        );
        for &et in extreme_ts.iter().take(roots) {
            let t = start_t + (end_t - start_t) * et;
            let mid = d_curve.pt_at_t(t);
            if top_pt.y > mid.y || (top_pt.y == mid.y && top_pt.x > mid.x) {
                top_t = t;
                *top_pt = mid;
            }
        }
        top_t
    }

    pub fn to_quad(&self) -> DQuad {
        let from_c1 = DPoint::new(
            (3.0 * self.pts[1].x - self.pts[0].x) / 2.0,
            (3.0 * self.pts[1].y - self.pts[0].y) / 2.0,
        );
        let from_c2 = DPoint::new(
            (3.0 * self.pts[2].x - self.pts[3].x) / 2.0,
            (3.0 * self.pts[2].y - self.pts[3].y) / 2.0,
        );
        DQuad {
            pts: [
                self.pts[0],
                DPoint::new((from_c1.x + from_c2.x) / 2.0, (from_c1.y + from_c2.y) / 2.0),
                self.pts[3],
            ],
        }
    }

    pub fn horizontal_intersect(&self, y_intercept: f64, roots: &mut [f64; 3]) -> usize {
        Intersections::horizontal_intercept_cubic(self, y_intercept, roots)
    }

    pub fn vertical_intersect(&self, x_intercept: f64, roots: &mut [f64; 3]) -> usize {
        Intersections::vertical_intercept_cubic(self, x_intercept, roots)
    }

    pub fn set_bounds(&self, sub: &DCubic, start_t: f64, end_t: f64) -> DRect {
        let mut r = DRect::default();
        r.set_pt(sub.pts[0]);
        r.add(sub.pts[3]);
        let mut t_values = [0.0f64; 4];
        let mut roots = 0usize;
        if !sub.monotonic_in_x() {
            let mut tv = [0.0; 2];
            let n = Self::find_extrema(
                sub.pts[0].x,
                sub.pts[1].x,
                sub.pts[2].x,
                sub.pts[3].x,
                &mut tv,
            );
            for &v in tv.iter().take(n) {
                t_values[roots] = v;
                roots += 1;
            }
        }
        if !sub.monotonic_in_y() {
            let mut tv = [0.0; 2];
            let n = Self::find_extrema(
                sub.pts[0].y,
                sub.pts[1].y,
                sub.pts[2].y,
                sub.pts[3].y,
                &mut tv,
            );
            for &v in tv.iter().take(n) {
                t_values[roots] = v;
                roots += 1;
            }
        }
        for &tv in t_values.iter().take(roots) {
            let t = start_t + (end_t - start_t) * tv;
            r.add(self.pt_at_t(t));
        }
        r
    }
}
