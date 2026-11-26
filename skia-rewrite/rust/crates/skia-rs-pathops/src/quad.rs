#![forbid(unsafe_code)]

//! Port of `SkPathOpsQuad.h/.cpp`.

use skia_rs_geometry::Point;

use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::intersections::Intersections;
use crate::line::{DLine, LineParameters};
use crate::types::{
    almost_bequal_ulps, almost_dequal_ulps_f64, approximately_equal, approximately_greater_than_one,
    approximately_less_than_zero, approximately_one_or_less, approximately_zero,
    approximately_zero_inverse, approximately_zero_or_more, approximately_zero_when_compared_to,
    between, cpp_max_f64, cpp_min_f64, d_interp, precisely_zero, zero_or_one, DPoint, DRect,
    DVector,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DQuad {
    pub pts: [DPoint; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DQuadPair {
    pub pts: [DPoint; 5],
}

impl DQuadPair {
    #[inline]
    pub fn first(&self) -> DQuad {
        DQuad {
            pts: [self.pts[0], self.pts[1], self.pts[2]],
        }
    }

    #[inline]
    pub fn second(&self) -> DQuad {
        DQuad {
            pts: [self.pts[2], self.pts[3], self.pts[4]],
        }
    }
}

fn point_in_triangle(pts: &[DPoint; 3], test: DPoint) -> bool {
    let v0 = pts[2].sub(pts[0]);
    let v1 = pts[1].sub(pts[0]);
    let v2 = test.sub(pts[0]);
    let dot00 = v0.dot(v0);
    let dot01 = v0.dot(v1);
    let dot02 = v0.dot(v2);
    let dot11 = v1.dot(v1);
    let dot12 = v1.dot(v2);
    let denom = dot00 * dot11 - dot01 * dot01;
    let u = dot11 * dot02 - dot01 * dot12;
    let v = dot00 * dot12 - dot01 * dot02;
    if denom >= 0.0 {
        u >= 0.0 && v >= 0.0 && u + v < denom
    } else {
        u <= 0.0 && v <= 0.0 && u + v > denom
    }
}

#[inline]
fn matches_end(pts: &[DPoint; 3], test: DPoint) -> bool {
    pts[0] == test || pts[2] == test
}

#[inline]
fn handle_zero(b: f64, c: f64, s: &mut [f64; 2]) -> usize {
    if approximately_zero(b) {
        s[0] = 0.0;
        return usize::from(c == 0.0);
    }
    s[0] = -c / b;
    1
}

#[inline]
fn interp_quad_coord(a: f64, b: f64, c: f64, t: f64) -> f64 {
    if t == 0.0 {
        return a;
    }
    if t == 1.0 {
        return c;
    }
    let ab = d_interp(a, b, t);
    let bc = d_interp(b, c, t);
    d_interp(ab, bc, t)
}

#[inline]
fn valid_unit_divide(mut numer: f64, mut denom: f64) -> Option<f64> {
    if numer < 0.0 {
        numer = -numer;
        denom = -denom;
    }
    if denom == 0.0 || numer == 0.0 || numer >= denom {
        return None;
    }
    let r = numer / denom;
    if r == 0.0 {
        return None;
    }
    Some(r)
}

impl DQuad {
    pub const POINT_COUNT: usize = 3;
    pub const POINT_LAST: usize = 2;
    pub const MAX_INTERSECTIONS: usize = 4;

    #[inline]
    pub const fn new(p0: DPoint, p1: DPoint, p2: DPoint) -> Self {
        Self { pts: [p0, p1, p2] }
    }

    #[inline]
    pub fn from_points(pts: [Point; 3]) -> Self {
        Self {
            pts: [
                DPoint::from_point(pts[0]),
                DPoint::from_point(pts[1]),
                DPoint::from_point(pts[2]),
            ],
        }
    }

    #[inline]
    pub fn collapsed(&self) -> bool {
        self.pts[0].approximately_equal(self.pts[1])
            && self.pts[0].approximately_equal(self.pts[2])
    }

    #[inline]
    pub fn controls_inside(&self) -> bool {
        let v01 = self.pts[0].sub(self.pts[1]);
        let v02 = self.pts[0].sub(self.pts[2]);
        let v12 = self.pts[1].sub(self.pts[2]);
        v02.dot(v01) > 0.0 && v02.dot(v12) > 0.0
    }

    #[inline]
    pub fn flip(&self) -> Self {
        Self {
            pts: [self.pts[2], self.pts[1], self.pts[0]],
        }
    }

    #[inline]
    pub fn other_pts(&self, odd_man: usize) -> [DPoint; 2] {
        let mut out = [DPoint::default(); 2];
        for opp in 1..Self::POINT_COUNT {
            let mut end = (odd_man ^ opp) as i32 - odd_man as i32;
            end &= !(end >> 2);
            out[opp - 1] = self.pts[end as usize];
        }
        out
    }

    pub fn hull_intersects_quad(&self, q2: &DQuad, is_linear: &mut bool) -> bool {
        let mut linear = true;
        for odd_man in 0..Self::POINT_COUNT {
            let end_pt = self.other_pts(odd_man);
            let orig_x = end_pt[0].x;
            let orig_y = end_pt[0].y;
            let adj = end_pt[1].x - orig_x;
            let opp = end_pt[1].y - orig_y;
            let sign = (self.pts[odd_man].y - orig_y) * adj - (self.pts[odd_man].x - orig_x) * opp;
            if approximately_zero(sign) {
                continue;
            }
            linear = false;
            let mut found_outlier = false;
            for n in 0..Self::POINT_COUNT {
                let test = (q2.pts[n].y - orig_y) * adj - (q2.pts[n].x - orig_x) * opp;
                if test * sign > 0.0 && !precisely_zero(test) {
                    found_outlier = true;
                    break;
                }
            }
            if !found_outlier {
                return false;
            }
        }
        if linear && !matches_end(&self.pts, q2.pts[0]) && !matches_end(&self.pts, q2.pts[2]) {
            if point_in_triangle(&self.pts, q2.pts[0]) || point_in_triangle(&self.pts, q2.pts[2]) {
                linear = false;
            }
        }
        *is_linear = linear;
        true
    }

    #[inline]
    pub fn hull_intersects_conic(&self, conic: &DConic, is_linear: &mut bool) -> bool {
        conic.hull_intersects_quad(self, is_linear)
    }

    #[inline]
    pub fn hull_intersects_cubic(&self, cubic: &DCubic, is_linear: &mut bool) -> bool {
        cubic.hull_intersects_quad(self, is_linear)
    }

    pub fn add_valid_ts(s: &[f64], real_roots: usize, t: &mut [f64]) -> usize {
        let mut found_roots = 0usize;
        for &raw in s.iter().take(real_roots) {
            let mut t_value = raw;
            if approximately_zero_or_more(t_value) && approximately_one_or_less(t_value) {
                if approximately_less_than_zero(t_value) {
                    t_value = 0.0;
                } else if approximately_greater_than_one(t_value) {
                    t_value = 1.0;
                }
                let mut dup = false;
                for &existing in t.iter().take(found_roots) {
                    if approximately_equal(existing, t_value) {
                        dup = true;
                        break;
                    }
                }
                if !dup {
                    t[found_roots] = t_value;
                    found_roots += 1;
                }
            }
        }
        found_roots
    }

    pub fn roots_valid_t(a: f64, b: f64, c: f64, t: &mut [f64; 2]) -> usize {
        let mut s = [0.0; 2];
        let real_roots = Self::roots_real(a, b, c, &mut s);
        Self::add_valid_ts(&s, real_roots, t)
    }

    pub fn roots_real(a: f64, b: f64, c: f64, s: &mut [f64; 2]) -> usize {
        if a == 0.0 {
            return handle_zero(b, c, s);
        }
        let p = b / (2.0 * a);
        let q = c / a;
        if approximately_zero(a) && (approximately_zero_inverse(p) || approximately_zero_inverse(q))
        {
            return handle_zero(b, c, s);
        }
        let p2 = p * p;
        if !almost_dequal_ulps_f64(p2, q) && p2 < q {
            return 0;
        }
        let sqrt_d = if p2 > q { (p2 - q).sqrt() } else { 0.0 };
        s[0] = sqrt_d - p;
        s[1] = -sqrt_d - p;
        1 + usize::from(!almost_dequal_ulps_f64(s[0], s[1]))
    }

    pub fn is_linear(&self, start_index: usize, end_index: usize) -> bool {
        let mut line_params = LineParameters::default();
        line_params.quad_end_points(self, start_index, end_index);
        line_params.normalize();
        let distance = line_params.control_pt_distance_quad(self);
        let tiniest = cpp_min_f64(
            cpp_min_f64(
                cpp_min_f64(
                    cpp_min_f64(cpp_min_f64(self.pts[0].x, self.pts[0].y), self.pts[1].x),
                    self.pts[1].y,
                ),
                self.pts[2].x,
            ),
            self.pts[2].y,
        );
        let mut largest = cpp_max_f64(
            cpp_max_f64(
                cpp_max_f64(
                    cpp_max_f64(cpp_max_f64(self.pts[0].x, self.pts[0].y), self.pts[1].x),
                    self.pts[1].y,
                ),
                self.pts[2].x,
            ),
            self.pts[2].y,
        );
        largest = cpp_max_f64(largest, -tiniest);
        approximately_zero_when_compared_to(distance, largest)
    }

    pub fn dxdy_at_t(&self, t: f64) -> DVector {
        let a = t - 1.0;
        let b = 1.0 - 2.0 * t;
        let c = t;
        let mut result = DVector {
            x: a * self.pts[0].x + b * self.pts[1].x + c * self.pts[2].x,
            y: a * self.pts[0].y + b * self.pts[1].y + c * self.pts[2].y,
        };
        if result.x == 0.0 && result.y == 0.0 && zero_or_one(t) {
            result = self.pts[2].sub(self.pts[0]);
        }
        result
    }

    #[inline]
    pub fn pt_at_t(&self, t: f64) -> DPoint {
        if t == 0.0 {
            return self.pts[0];
        }
        if t == 1.0 {
            return self.pts[2];
        }
        let one_t = 1.0 - t;
        let a = one_t * one_t;
        let b = 2.0 * one_t * t;
        let c = t * t;
        DPoint {
            x: a * self.pts[0].x + b * self.pts[1].x + c * self.pts[2].x,
            y: a * self.pts[0].y + b * self.pts[1].y + c * self.pts[2].y,
        }
    }

    #[inline]
    pub fn monotonic_in_x(&self) -> bool {
        between(self.pts[0].x, self.pts[1].x, self.pts[2].x)
    }

    #[inline]
    pub fn monotonic_in_y(&self) -> bool {
        between(self.pts[0].y, self.pts[1].y, self.pts[2].y)
    }

    pub fn sub_divide(&self, t1: f64, t2: f64) -> Self {
        if t1 == 0.0 && t2 == 1.0 {
            return *self;
        }
        let ax = interp_quad_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, t1);
        let ay = interp_quad_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, t1);
        let mid_t = (t1 + t2) / 2.0;
        let dx = interp_quad_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, mid_t);
        let dy = interp_quad_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, mid_t);
        let cx = interp_quad_coord(self.pts[0].x, self.pts[1].x, self.pts[2].x, t2);
        let cy = interp_quad_coord(self.pts[0].y, self.pts[1].y, self.pts[2].y, t2);
        let bx = 2.0 * dx - (ax + cx) / 2.0;
        let by = 2.0 * dy - (ay + cy) / 2.0;
        DQuad {
            pts: [
                DPoint::new(ax, ay),
                DPoint::new(bx, by),
                DPoint::new(cx, cy),
            ],
        }
    }

    #[inline]
    pub fn align(&self, end_index: usize, dst_pt: &mut DPoint) {
        if self.pts[end_index].x == self.pts[1].x {
            dst_pt.x = self.pts[end_index].x;
        }
        if self.pts[end_index].y == self.pts[1].y {
            dst_pt.y = self.pts[end_index].y;
        }
    }

    pub fn sub_divide_pt(&self, a: DPoint, c: DPoint, t1: f64, t2: f64) -> DPoint {
        let sub = self.sub_divide(t1, t2);
        let b0 = DLine::new(a, sub.pts[1].add_vec(a.sub(sub.pts[0])));
        let b1 = DLine::new(c, sub.pts[1].add_vec(c.sub(sub.pts[2])));
        let mut i = Intersections::new();
        i.intersect_ray_line_line(&b0, &b1);
        let mut b = if i.used() == 1 && i.t[0][0] >= 0.0 && i.t[1][0] >= 0.0 {
            i.pt[0]
        } else {
            return DPoint::mid(b0.pts[1], b1.pts[1]);
        };
        if t1 == 0.0 || t2 == 0.0 {
            self.align(0, &mut b);
        }
        if t1 == 1.0 || t2 == 1.0 {
            self.align(2, &mut b);
        }
        if almost_bequal_ulps(b.x, a.x) {
            b.x = a.x;
        } else if almost_bequal_ulps(b.x, c.x) {
            b.x = c.x;
        }
        if almost_bequal_ulps(b.y, a.y) {
            b.y = a.y;
        } else if almost_bequal_ulps(b.y, c.y) {
            b.y = c.y;
        }
        b
    }

    pub fn chop_at(&self, t: f64) -> DQuadPair {
        let abx = d_interp(self.pts[0].x, self.pts[1].x, t);
        let bcx = d_interp(self.pts[1].x, self.pts[2].x, t);
        let aby = d_interp(self.pts[0].y, self.pts[1].y, t);
        let bcy = d_interp(self.pts[1].y, self.pts[2].y, t);
        DQuadPair {
            pts: [
                self.pts[0],
                DPoint::new(abx, aby),
                DPoint::new(d_interp(abx, bcx, t), d_interp(aby, bcy, t)),
                DPoint::new(bcx, bcy),
                self.pts[2],
            ],
        }
    }

    pub fn find_extrema(a: f64, b: f64, c: f64, t_value: &mut [f64; 1]) -> usize {
        if let Some(r) = valid_unit_divide(a - b, a - b - b + c) {
            t_value[0] = r;
            1
        } else {
            0
        }
    }

    #[inline]
    pub fn set_abc(p0: f64, p1: f64, p2: f64) -> (f64, f64, f64) {
        let mut a = p0;
        let mut b = 2.0 * p1;
        let c = p2;
        b -= c;
        a -= b;
        b -= c;
        (a, b, c)
    }

    pub fn horizontal_intersect(&self, y_intercept: f64, roots: &mut [f64; 2]) -> usize {
        Intersections::horizontal_intercept_quad(self, y_intercept, roots)
    }

    pub fn vertical_intersect(&self, x_intercept: f64, roots: &mut [f64; 2]) -> usize {
        Intersections::vertical_intercept_quad(self, x_intercept, roots)
    }

    pub fn set_bounds(&self, sub: &DQuad, start_t: f64, end_t: f64) -> DRect {
        let mut r = DRect::default();
        r.set_pt(sub.pts[0]);
        r.add(sub.pts[2]);
        let mut t_values = [0.0; 2];
        let mut roots = 0usize;
        if !sub.monotonic_in_x() {
            let mut tv = [0.0; 1];
            let n = Self::find_extrema(sub.pts[0].x, sub.pts[1].x, sub.pts[2].x, &mut tv);
            if n == 1 {
                t_values[roots] = tv[0];
                roots += 1;
            }
        }
        if !sub.monotonic_in_y() {
            let mut tv = [0.0; 1];
            let n = Self::find_extrema(sub.pts[0].y, sub.pts[1].y, sub.pts[2].y, &mut tv);
            if n == 1 {
                t_values[roots] = tv[0];
                roots += 1;
            }
        }
        for &tv in t_values.iter().take(roots) {
            let t = start_t + (end_t - start_t) * tv;
            r.add(self.pt_at_t(t));
        }
        r
    }

    pub fn debug_to_cubic(&self) -> DCubic {
        let mut cubic = DCubic::default();
        cubic.pts[0] = self.pts[0];
        cubic.pts[2] = self.pts[1];
        cubic.pts[3] = self.pts[2];
        cubic.pts[1].x = (cubic.pts[0].x + cubic.pts[2].x * 2.0) / 3.0;
        cubic.pts[1].y = (cubic.pts[0].y + cubic.pts[2].y * 2.0) / 3.0;
        cubic.pts[2].x = (cubic.pts[3].x + cubic.pts[2].x * 2.0) / 3.0;
        cubic.pts[2].y = (cubic.pts[3].y + cubic.pts[2].y * 2.0) / 3.0;
        cubic
    }
}
