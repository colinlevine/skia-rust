#![forbid(unsafe_code)]

//! Port of `SkIntersections.h/.cpp`, `SkDLineIntersection.cpp`,
//! `SkDQuadLineIntersection.cpp`, `SkDConicLineIntersection.cpp`,
//! and `SkDCubicLineIntersection.cpp`.

use skia_rs_geometry::Point;
use skia_rs_path::types::Verb;

use crate::conic::DConic;
use crate::cubic::{DCubic, SearchAxis};
use crate::curve::DCurve;
use crate::line::DLine;
use crate::quad::DQuad;
use crate::types::{
    almost_equal_ulps, approximately_equal, approximately_one_or_less,
    approximately_one_or_less_double, approximately_zero, approximately_zero_or_more,
    approximately_zero_or_more_double, between, more_roughly_equal, not_almost_dequal_ulps,
    not_almost_equal_ulps_pin, pin_t, precisely_between, precisely_equal, precisely_zero,
    zero_or_one, DPoint, BUMP_EPSILON, SK_SCALAR_MAX,
};

#[derive(Clone, Debug)]
pub struct Intersections {
    pub pt: [DPoint; 13],
    pub pt2: [DPoint; 2],
    pub t: [[f64; 13]; 2],
    pub is_coincident: [u16; 2],
    pub nearly_same: [bool; 2],
    pub used: u8,
    pub max: u8,
    pub allow_near: bool,
    pub swap: bool,
}

impl Default for Intersections {
    fn default() -> Self {
        Self::new()
    }
}

impl Intersections {
    pub fn new() -> Self {
        Self {
            pt: [DPoint::default(); 13],
            pt2: [DPoint::default(); 2],
            t: [[0.0; 13]; 2],
            is_coincident: [0; 2],
            nearly_same: [false; 2],
            used: 0,
            max: 0,
            allow_near: true,
            swap: false,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        self.allow_near = true;
        self.used = 0;
        self.is_coincident = [0; 2];
    }

    #[inline]
    pub fn used(&self) -> usize {
        self.used as usize
    }

    #[inline]
    pub fn set_max(&mut self, max: usize) {
        self.max = max as u8;
    }

    #[inline]
    pub fn allow_near(&mut self, allow: bool) {
        self.allow_near = allow;
    }

    #[inline]
    pub fn swap(&mut self) {
        self.swap = !self.swap;
    }

    #[inline]
    pub fn swapped(&self) -> bool {
        self.swap
    }

    #[inline]
    pub fn is_coincident(&self, index: usize) -> bool {
        (self.is_coincident[0] & (1u16 << index)) != 0
    }

    #[inline]
    pub fn clear_coincidence(&mut self, index: usize) {
        let bit = 1u16 << index;
        self.is_coincident[0] &= !bit;
        self.is_coincident[1] &= !bit;
    }

    #[inline]
    pub fn set_coincident(&mut self, index: usize) {
        let bit = 1u16 << index;
        self.is_coincident[0] |= bit;
        self.is_coincident[1] |= bit;
    }

    #[inline]
    pub fn has_t(&self, t: f64) -> bool {
        self.used > 0
            && if t == 0.0 {
                self.t[0][0] == 0.0
            } else {
                self.t[0][self.used as usize - 1] == 1.0
            }
    }

    #[inline]
    pub fn has_opp_t(&self, t: f64) -> bool {
        self.used > 0 && (self.t[1][0] == t || self.t[1][self.used as usize - 1] == t)
    }

    #[inline]
    pub fn un_bump_t(&mut self, index: usize) -> bool {
        self.t[0][index] = self.t[0][index] * (1.0 + BUMP_EPSILON * 2.0) - BUMP_EPSILON;
        if !between(0.0, self.t[0][index], 1.0) {
            self.used = 0;
            return false;
        }
        true
    }

    #[inline]
    pub fn insert_swap(&mut self, one: f64, two: f64, pt: DPoint) -> i32 {
        if self.swap {
            self.insert(two, one, pt)
        } else {
            self.insert(one, two, pt)
        }
    }

    pub fn closest_to(
        &self,
        range_start: f64,
        range_end: f64,
        test_pt: DPoint,
        closest_dist: &mut f64,
    ) -> i32 {
        let mut closest: i32 = -1;
        *closest_dist = SK_SCALAR_MAX as f64;
        for index in 0..self.used() {
            if !between(range_start, self.t[0][index], range_end) {
                continue;
            }
            let dist = test_pt.distance_squared(self.pt[index]);
            if *closest_dist > dist {
                *closest_dist = dist;
                closest = index as i32;
            }
        }
        closest
    }

    pub fn flip(&mut self) {
        for index in 0..self.used() {
            self.t[1][index] = 1.0 - self.t[1][index];
        }
    }

    pub fn insert(&mut self, one: f64, two: f64, pt: DPoint) -> i32 {
        if self.is_coincident[0] == 3 && between(self.t[0][0], one, self.t[0][1]) {
            return -1;
        }
        let mut used = self.used();
        for index in 0..used {
            let old_one = self.t[0][index];
            let old_two = self.t[1][index];
            if one == old_one && two == old_two {
                return -1;
            }
            if more_roughly_equal(old_one, one) && more_roughly_equal(old_two, two) {
                if (!precisely_zero(one) || precisely_zero(old_one))
                    && (!precisely_equal(one, 1.0) || precisely_equal(old_one, 1.0))
                    && (!precisely_zero(two) || precisely_zero(old_two))
                    && (!precisely_equal(two, 1.0) || precisely_equal(old_two, 1.0))
                {
                    return -1;
                }
                let remaining = used - index - 1;
                for j in 0..remaining {
                    self.pt[index + j] = self.pt[index + j + 1];
                    self.t[0][index + j] = self.t[0][index + j + 1];
                    self.t[1][index + j] = self.t[1][index + j + 1];
                }
                let clear_mask = !((1u16 << index).wrapping_sub(1));
                self.is_coincident[0] = self.is_coincident[0]
                    .wrapping_sub((self.is_coincident[0] >> 1) & clear_mask);
                self.is_coincident[1] = self.is_coincident[1]
                    .wrapping_sub((self.is_coincident[1] >> 1) & clear_mask);
                used -= 1;
                self.used = used as u8;
                break;
            }
        }
        let mut insert_idx = 0usize;
        while insert_idx < used {
            if self.t[0][insert_idx] > one {
                break;
            }
            insert_idx += 1;
        }
        if used >= self.max as usize {
            self.used = 0;
            return 0;
        }
        let remaining = used - insert_idx;
        if remaining > 0 {
            for j in (0..remaining).rev() {
                self.pt[insert_idx + j + 1] = self.pt[insert_idx + j];
                self.t[0][insert_idx + j + 1] = self.t[0][insert_idx + j];
                self.t[1][insert_idx + j + 1] = self.t[1][insert_idx + j];
            }
            let clear_mask = !((1u16 << insert_idx).wrapping_sub(1));
            self.is_coincident[0] =
                self.is_coincident[0].wrapping_add(self.is_coincident[0] & clear_mask);
            self.is_coincident[1] =
                self.is_coincident[1].wrapping_add(self.is_coincident[1] & clear_mask);
        }
        self.pt[insert_idx] = pt;
        if !(0.0..=1.0).contains(&one) {
            return -1;
        }
        if !(0.0..=1.0).contains(&two) {
            return -1;
        }
        self.t[0][insert_idx] = one;
        self.t[1][insert_idx] = two;
        self.used = (used + 1) as u8;
        insert_idx as i32
    }

    pub fn insert_near(&mut self, one: f64, two: f64, pt1: DPoint, pt2: DPoint) {
        let idx = usize::from(one != 0.0);
        self.nearly_same[idx] = true;
        let _ = self.insert(one, two, pt1);
        self.pt2[idx] = pt2;
    }

    pub fn insert_coincident(&mut self, one: f64, two: f64, pt: DPoint) -> i32 {
        let index = self.insert_swap(one, two, pt);
        if index >= 0 {
            self.set_coincident(index as usize);
        }
        index
    }

    pub fn merge(&mut self, a: &Intersections, a_index: usize, b: &Intersections, b_index: usize) {
        self.reset();
        self.t[0][0] = a.t[0][a_index];
        self.t[1][0] = b.t[0][b_index];
        self.pt[0] = a.pt[a_index];
        self.pt2[0] = b.pt[b_index];
        self.used = 1;
    }

    pub fn most_outside(&self, range_start: f64, range_end: f64, origin: DPoint) -> i32 {
        let mut result: i32 = -1;
        for index in 0..self.used() {
            if !between(range_start, self.t[0][index], range_end) {
                continue;
            }
            if result < 0 {
                result = index as i32;
                continue;
            }
            let best = self.pt[result as usize].sub(origin);
            let test = self.pt[index].sub(origin);
            if test.cross_check(best) < 0.0 {
                result = index as i32;
            }
        }
        result
    }

    pub fn remove_one(&mut self, index: usize) {
        self.used -= 1;
        let used = self.used();
        if used <= index {
            return;
        }
        let remaining = used - index;
        for j in 0..remaining {
            self.pt[index + j] = self.pt[index + j + 1];
            self.t[0][index + j] = self.t[0][index + j + 1];
            self.t[1][index + j] = self.t[1][index + j + 1];
        }
        let co_bit = self.is_coincident[0] & (1u16 << index);
        let clear_mask = !((1u16 << index).wrapping_sub(1));
        self.is_coincident[0] = self.is_coincident[0]
            .wrapping_sub(((self.is_coincident[0] >> 1) & clear_mask).wrapping_add(co_bit));
        self.is_coincident[1] = self.is_coincident[1]
            .wrapping_sub(((self.is_coincident[1] >> 1) & clear_mask).wrapping_add(co_bit));
    }

    pub fn debug_coincident_used(&self) -> usize {
        if self.is_coincident[0] == 0 {
            return 0;
        }
        let mut count = 0usize;
        for index in 0..self.used() {
            if (self.is_coincident[0] & (1u16 << index)) != 0 {
                count += 1;
            }
        }
        count
    }

    // ========================================================================
    // Line-Line Intersections (SkDLineIntersection.cpp)
    // ========================================================================

    fn clean_up_parallel_lines(&mut self, parallel: bool) {
        while self.used() > 2 {
            self.remove_one(1);
        }
        if self.used() == 2 && !parallel {
            let start_match = self.t[0][0] == 0.0 || zero_or_one(self.t[1][0]);
            let end_match = self.t[0][1] == 1.0 || zero_or_one(self.t[1][1]);
            if (!start_match && !end_match) || approximately_equal(self.t[0][0], self.t[0][1]) {
                if start_match
                    && end_match
                    && (self.t[0][0] != 0.0 || !zero_or_one(self.t[1][0]))
                    && self.t[0][1] == 1.0
                    && zero_or_one(self.t[1][1])
                {
                    self.remove_one(0);
                } else {
                    self.remove_one(usize::from(end_match));
                }
            }
        }
        if self.used() == 2 {
            self.is_coincident[0] = 0x03;
            self.is_coincident[1] = 0x03;
        }
    }

    fn compute_points(&mut self, line: &DLine, used: usize) {
        self.pt[0] = line.pt_at_t(self.t[0][0]);
        self.used = used as u8;
        if used == 2 {
            self.pt[1] = line.pt_at_t(self.t[0][1]);
        }
    }

    pub fn intersect_ray_line_line(&mut self, a: &DLine, b: &DLine) -> usize {
        self.max = 2;
        let a_len = a.pts[1].sub(a.pts[0]);
        let b_len = b.pts[1].sub(b.pts[0]);
        let denom = b_len.y * a_len.x - a_len.y * b_len.x;
        let used = if !approximately_zero(denom) {
            let ab0 = a.pts[0].sub(b.pts[0]);
            let numer_a = (ab0.y * b_len.x - b_len.y * ab0.x) / denom;
            let numer_b = (ab0.y * a_len.x - a_len.y * ab0.x) / denom;
            self.t[0][0] = numer_a;
            self.t[1][0] = numer_b;
            1
        } else {
            if !almost_equal_ulps(
                a_len.x * a.pts[0].y - a_len.y * a.pts[0].x,
                a_len.x * b.pts[0].y - a_len.y * b.pts[0].x,
            ) {
                self.used = 0;
                return 0;
            }
            self.t[0][0] = 0.0;
            self.t[1][0] = 1.0;
            self.t[1][1] = 1.0;
            2
        };
        self.compute_points(a, used);
        self.used()
    }

    pub fn intersect_line_line(&mut self, a: &DLine, b: &DLine) -> usize {
        self.max = 3;
        for i_a in 0..2 {
            let t = b.exact_point(a.pts[i_a]);
            if t >= 0.0 {
                self.insert(i_a as f64, t, a.pts[i_a]);
            }
        }
        for i_b in 0..2 {
            let t = a.exact_point(b.pts[i_b]);
            if t >= 0.0 {
                self.insert(t, i_b as f64, b.pts[i_b]);
            }
        }
        let ax_len = a.pts[1].x - a.pts[0].x;
        let ay_len = a.pts[1].y - a.pts[0].y;
        let bx_len = b.pts[1].x - b.pts[0].x;
        let by_len = b.pts[1].y - b.pts[0].y;
        let ax_by_len = ax_len * by_len;
        let ay_bx_len = ay_len * bx_len;
        let unparallel = if self.allow_near {
            not_almost_equal_ulps_pin(ax_by_len, ay_bx_len)
        } else {
            not_almost_dequal_ulps(ax_by_len, ay_bx_len)
        };
        if unparallel && self.used == 0 {
            let ab0y = a.pts[0].y - b.pts[0].y;
            let ab0x = a.pts[0].x - b.pts[0].x;
            let numer_a = ab0y * bx_len - by_len * ab0x;
            let numer_b = ab0y * ax_len - ay_len * ab0x;
            let denom = ax_by_len - ay_bx_len;
            if between(0.0, numer_a, denom) && between(0.0, numer_b, denom) {
                self.t[0][0] = numer_a / denom;
                self.t[1][0] = numer_b / denom;
                self.compute_points(a, 1);
            }
        }
        if self.allow_near || !unparallel {
            let mut a_near_b = [0.0f64; 2];
            let mut b_near_a = [0.0f64; 2];
            let mut a_not_b = [false; 2];
            let mut b_not_a = [false; 2];
            let mut near_count = 0i32;
            for index in 0..2 {
                let t1 = b.near_point(a.pts[index], Some(&mut a_not_b[index]));
                a_near_b[index] = t1;
                near_count += i32::from(t1 >= 0.0);
                let t2 = a.near_point(b.pts[index], Some(&mut b_not_a[index]));
                b_near_a[index] = t2;
                near_count += i32::from(t2 >= 0.0);
            }
            if near_count > 0 {
                if near_count != 2 || a_not_b[0] == a_not_b[1] {
                    for i_a in 0..2 {
                        if !a_not_b[i_a] {
                            continue;
                        }
                        let nearer = usize::from(a_near_b[i_a] > 0.5);
                        if !b_not_a[nearer] {
                            continue;
                        }
                        self.insert_near(i_a as f64, nearer as f64, a.pts[i_a], b.pts[nearer]);
                        a_near_b[i_a] = -1.0;
                        b_near_a[nearer] = -1.0;
                        near_count -= 2;
                    }
                }
                if near_count > 0 {
                    for i_a in 0..2 {
                        if a_near_b[i_a] >= 0.0 {
                            self.insert(i_a as f64, a_near_b[i_a], a.pts[i_a]);
                        }
                    }
                    for i_b in 0..2 {
                        if b_near_a[i_b] >= 0.0 {
                            self.insert(b_near_a[i_b], i_b as f64, b.pts[i_b]);
                        }
                    }
                }
            }
        }
        self.clean_up_parallel_lines(!unparallel);
        self.used()
    }

    pub fn horizontal_intercept_line(line: &DLine, y: f64) -> f64 {
        pin_t((y - line.pts[0].y) / (line.pts[1].y - line.pts[0].y))
    }

    pub fn horizontal_line(
        &mut self,
        line: &DLine,
        left: f64,
        right: f64,
        y: f64,
        flipped: bool,
    ) -> usize {
        self.max = 3;
        let left_pt = DPoint::new(left, y);
        let t = line.exact_point(left_pt);
        if t >= 0.0 {
            self.insert(t, f64::from(flipped), left_pt);
        }
        if left != right {
            let right_pt = DPoint::new(right, y);
            let t = line.exact_point(right_pt);
            if t >= 0.0 {
                self.insert(t, f64::from(!flipped), right_pt);
            }
            for index in 0..2 {
                let t = DLine::exact_point_h(line.pts[index], left, right, y);
                if t >= 0.0 {
                    self.insert(index as f64, if flipped { 1.0 - t } else { t }, line.pts[index]);
                }
            }
        }
        let result = horizontal_coincident(line, y);
        if result == 1 && self.used == 0 {
            self.t[0][0] = Self::horizontal_intercept_line(line, y);
            let x_intercept = line.pts[0].x + self.t[0][0] * (line.pts[1].x - line.pts[0].x);
            if between(left, x_intercept, right) {
                self.t[1][0] = (x_intercept - left) / (right - left);
                if flipped {
                    self.t[1][0] = 1.0 - self.t[1][0];
                }
                self.pt[0] = DPoint::new(x_intercept, y);
                self.used = 1;
            }
        }
        if self.allow_near || result == 2 {
            let t = line.near_point(left_pt, None);
            if t >= 0.0 {
                self.insert(t, f64::from(flipped), left_pt);
            }
            if left != right {
                let right_pt = DPoint::new(right, y);
                let t = line.near_point(right_pt, None);
                if t >= 0.0 {
                    self.insert(t, f64::from(!flipped), right_pt);
                }
                for index in 0..2 {
                    let t = DLine::near_point_h(line.pts[index], left, right, y);
                    if t >= 0.0 {
                        self.insert(
                            index as f64,
                            if flipped { 1.0 - t } else { t },
                            line.pts[index],
                        );
                    }
                }
            }
        }
        self.clean_up_parallel_lines(result == 2);
        self.used()
    }

    pub fn vertical_intercept_line(line: &DLine, x: f64) -> f64 {
        pin_t((x - line.pts[0].x) / (line.pts[1].x - line.pts[0].x))
    }

    pub fn vertical_line(
        &mut self,
        line: &DLine,
        top: f64,
        bottom: f64,
        x: f64,
        flipped: bool,
    ) -> usize {
        self.max = 3;
        let top_pt = DPoint::new(x, top);
        let t = line.exact_point(top_pt);
        if t >= 0.0 {
            self.insert(t, f64::from(flipped), top_pt);
        }
        if top != bottom {
            let bottom_pt = DPoint::new(x, bottom);
            let t = line.exact_point(bottom_pt);
            if t >= 0.0 {
                self.insert(t, f64::from(!flipped), bottom_pt);
            }
            for index in 0..2 {
                let t = DLine::exact_point_v(line.pts[index], top, bottom, x);
                if t >= 0.0 {
                    self.insert(index as f64, if flipped { 1.0 - t } else { t }, line.pts[index]);
                }
            }
        }
        let result = vertical_coincident(line, x);
        if result == 1 && self.used == 0 {
            self.t[0][0] = Self::vertical_intercept_line(line, x);
            let y_intercept = line.pts[0].y + self.t[0][0] * (line.pts[1].y - line.pts[0].y);
            if between(top, y_intercept, bottom) {
                self.t[1][0] = (y_intercept - top) / (bottom - top);
                if flipped {
                    self.t[1][0] = 1.0 - self.t[1][0];
                }
                self.pt[0] = DPoint::new(x, y_intercept);
                self.used = 1;
            }
        }
        if self.allow_near || result == 2 {
            let t = line.near_point(top_pt, None);
            if t >= 0.0 {
                self.insert(t, f64::from(flipped), top_pt);
            }
            if top != bottom {
                let bottom_pt = DPoint::new(x, bottom);
                let t = line.near_point(bottom_pt, None);
                if t >= 0.0 {
                    self.insert(t, f64::from(!flipped), bottom_pt);
                }
                for index in 0..2 {
                    let t = DLine::near_point_v(line.pts[index], top, bottom, x);
                    if t >= 0.0 {
                        self.insert(
                            index as f64,
                            if flipped { 1.0 - t } else { t },
                            line.pts[index],
                        );
                    }
                }
            }
        }
        self.clean_up_parallel_lines(result == 2);
        self.used()
    }

    // ========================================================================
    // Quad-Line Intersections (SkDQuadLineIntersection.cpp)
    // ========================================================================

    pub fn horizontal_intercept_quad(quad: &DQuad, y: f64, roots: &mut [f64; 2]) -> usize {
        let mut d = quad.pts[2].y;
        let mut e = quad.pts[1].y;
        let mut f = quad.pts[0].y;
        d += f - 2.0 * e;
        e -= f;
        f -= y;
        DQuad::roots_valid_t(d, 2.0 * e, f, roots)
    }

    pub fn vertical_intercept_quad(quad: &DQuad, x: f64, roots: &mut [f64; 2]) -> usize {
        let mut d = quad.pts[2].x;
        let mut e = quad.pts[1].x;
        let mut f = quad.pts[0].x;
        d += f - 2.0 * e;
        e -= f;
        f -= x;
        DQuad::roots_valid_t(d, 2.0 * e, f, roots)
    }

    pub fn horizontal_quad(
        &mut self,
        quad: &DQuad,
        left: f64,
        right: f64,
        y: f64,
        flipped: bool,
    ) -> usize {
        let line = DLine::new(DPoint::new(left, y), DPoint::new(right, y));
        let mut q = LineQuadIntersections::new(quad, &line, self);
        q.horizontal_intersect(y, left, right, flipped)
    }

    pub fn vertical_quad(
        &mut self,
        quad: &DQuad,
        top: f64,
        bottom: f64,
        x: f64,
        flipped: bool,
    ) -> usize {
        let line = DLine::new(DPoint::new(x, top), DPoint::new(x, bottom));
        let mut q = LineQuadIntersections::new(quad, &line, self);
        q.vertical_intersect(x, top, bottom, flipped)
    }

    pub fn intersect_quad_line(&mut self, quad: &DQuad, line: &DLine) -> usize {
        let allow = self.allow_near;
        let mut q = LineQuadIntersections::new(quad, line, self);
        q.allow_near = allow;
        q.intersect()
    }

    pub fn intersect_ray_quad_line(&mut self, quad: &DQuad, line: &DLine) -> usize {
        self.set_max(5);
        let mut roots = [0.0; 2];
        let used = LineQuadIntersections::intersect_ray_static(quad, line, &mut roots);
        self.used = used as u8;
        for index in 0..used {
            self.t[0][index] = roots[index];
            self.pt[index] = quad.pt_at_t(roots[index]);
        }
        used
    }

    // ========================================================================
    // Conic-Line Intersections (SkDConicLineIntersection.cpp)
    // ========================================================================

    pub fn horizontal_intercept_conic(conic: &DConic, y: f64, roots: &mut [f64; 2]) -> usize {
        let vals = [conic.pts.pts[0].y, conic.pts.pts[1].y, conic.pts.pts[2].y];
        conic_valid_t(&vals, conic.weight, y, roots)
    }

    pub fn vertical_intercept_conic(conic: &DConic, x: f64, roots: &mut [f64; 2]) -> usize {
        let vals = [conic.pts.pts[0].x, conic.pts.pts[1].x, conic.pts.pts[2].x];
        conic_valid_t(&vals, conic.weight, x, roots)
    }

    pub fn horizontal_conic(
        &mut self,
        conic: &DConic,
        left: f64,
        right: f64,
        y: f64,
        flipped: bool,
    ) -> usize {
        let line = DLine::new(DPoint::new(left, y), DPoint::new(right, y));
        let mut c = LineConicIntersections::new(conic, &line, self);
        c.horizontal_intersect(y, left, right, flipped)
    }

    pub fn vertical_conic(
        &mut self,
        conic: &DConic,
        top: f64,
        bottom: f64,
        x: f64,
        flipped: bool,
    ) -> usize {
        let line = DLine::new(DPoint::new(x, top), DPoint::new(x, bottom));
        let mut c = LineConicIntersections::new(conic, &line, self);
        c.vertical_intersect(x, top, bottom, flipped)
    }

    pub fn intersect_conic_line(&mut self, conic: &DConic, line: &DLine) -> usize {
        let allow = self.allow_near;
        let mut c = LineConicIntersections::new(conic, line, self);
        c.allow_near = allow;
        c.intersect()
    }

    pub fn intersect_ray_conic_line(&mut self, conic: &DConic, line: &DLine) -> usize {
        self.set_max(4);
        let mut roots = [0.0; 2];
        let used = LineConicIntersections::intersect_ray_static(conic, line, &mut roots);
        self.used = used as u8;
        for index in 0..used {
            self.t[0][index] = roots[index];
            self.pt[index] = conic.pt_at_t(roots[index]);
        }
        used
    }

    // ========================================================================
    // Cubic-Line Intersections (SkDCubicLineIntersection.cpp)
    // ========================================================================

    pub fn horizontal_intercept_cubic(cubic: &DCubic, y: f64, roots: &mut [f64; 3]) -> usize {
        let (a, b, c, mut d) = DCubic::coefficients(
            cubic.pts[0].y,
            cubic.pts[1].y,
            cubic.pts[2].y,
            cubic.pts[3].y,
        );
        d -= y;
        let mut count = DCubic::roots_valid_t(a, b, c, d, roots);
        for index in 0..count {
            let calc_pt = cubic.pt_at_t(roots[index]);
            if !approximately_equal(calc_pt.y, y) {
                let mut extreme_ts = [0.0; 6];
                let mut ext = [0.0; 2];
                let extrema = DCubic::find_extrema(
                    cubic.pts[0].y,
                    cubic.pts[1].y,
                    cubic.pts[2].y,
                    cubic.pts[3].y,
                    &mut ext,
                );
                extreme_ts[..extrema].copy_from_slice(&ext[..extrema]);
                count = cubic.search_roots(&mut extreme_ts, extrema, y, SearchAxis::YAxis, roots);
                break;
            }
        }
        count
    }

    pub fn vertical_intercept_cubic(cubic: &DCubic, x: f64, roots: &mut [f64; 3]) -> usize {
        let (a, b, c, mut d) = DCubic::coefficients(
            cubic.pts[0].x,
            cubic.pts[1].x,
            cubic.pts[2].x,
            cubic.pts[3].x,
        );
        d -= x;
        let mut count = DCubic::roots_valid_t(a, b, c, d, roots);
        for index in 0..count {
            let calc_pt = cubic.pt_at_t(roots[index]);
            if !approximately_equal(calc_pt.x, x) {
                let mut extreme_ts = [0.0; 6];
                let mut ext = [0.0; 2];
                let extrema = DCubic::find_extrema(
                    cubic.pts[0].x,
                    cubic.pts[1].x,
                    cubic.pts[2].x,
                    cubic.pts[3].x,
                    &mut ext,
                );
                extreme_ts[..extrema].copy_from_slice(&ext[..extrema]);
                count = cubic.search_roots(&mut extreme_ts, extrema, x, SearchAxis::XAxis, roots);
                break;
            }
        }
        count
    }

    pub fn horizontal_cubic(
        &mut self,
        cubic: &DCubic,
        left: f64,
        right: f64,
        y: f64,
        flipped: bool,
    ) -> usize {
        let line = DLine::new(DPoint::new(left, y), DPoint::new(right, y));
        let mut c = LineCubicIntersections::new(cubic, &line, self);
        c.horizontal_intersect(y, left, right, flipped)
    }

    pub fn vertical_cubic(
        &mut self,
        cubic: &DCubic,
        top: f64,
        bottom: f64,
        x: f64,
        flipped: bool,
    ) -> usize {
        let line = DLine::new(DPoint::new(x, top), DPoint::new(x, bottom));
        let mut c = LineCubicIntersections::new(cubic, &line, self);
        c.vertical_intersect(x, top, bottom, flipped)
    }

    pub fn intersect_cubic_line(&mut self, cubic: &DCubic, line: &DLine) -> usize {
        let allow = self.allow_near;
        let mut c = LineCubicIntersections::new(cubic, line, self);
        c.allow_near = allow;
        c.intersect()
    }

    pub fn intersect_ray_cubic_line(&mut self, cubic: &DCubic, line: &DLine) -> usize {
        self.set_max(4);
        let mut roots = [0.0; 3];
        let used = LineCubicIntersections::intersect_ray_static(cubic, line, &mut roots);
        self.used = used as u8;
        for index in 0..used {
            self.t[0][index] = roots[index];
            self.pt[index] = cubic.pt_at_t(roots[index]);
        }
        used
    }

    // Convenience point-array helpers matching SkIntersections.h
    pub fn line_horizontal(
        &mut self,
        a: [Point; 2],
        left: f32,
        right: f32,
        y: f32,
        flipped: bool,
    ) -> usize {
        let line = DLine::from_points(a);
        self.max = 2;
        self.horizontal_line(&line, left as f64, right as f64, y as f64, flipped)
    }

    pub fn line_vertical(
        &mut self,
        a: [Point; 2],
        top: f32,
        bottom: f32,
        x: f32,
        flipped: bool,
    ) -> usize {
        let line = DLine::from_points(a);
        self.max = 2;
        self.vertical_line(&line, top as f64, bottom as f64, x as f64, flipped)
    }

    pub fn line_line(&mut self, a: [Point; 2], b: [Point; 2]) -> usize {
        let a_line = DLine::from_points(a);
        let b_line = DLine::from_points(b);
        self.max = 2;
        self.intersect_line_line(&a_line, &b_line)
    }

    pub fn quad_horizontal(
        &mut self,
        a: [Point; 3],
        left: f32,
        right: f32,
        y: f32,
        flipped: bool,
    ) -> usize {
        let quad = DQuad::from_points(a);
        self.max = 2;
        self.horizontal_quad(&quad, left as f64, right as f64, y as f64, flipped)
    }

    pub fn quad_vertical(
        &mut self,
        a: [Point; 3],
        top: f32,
        bottom: f32,
        x: f32,
        flipped: bool,
    ) -> usize {
        let quad = DQuad::from_points(a);
        self.max = 2;
        self.vertical_quad(&quad, top as f64, bottom as f64, x as f64, flipped)
    }

    pub fn quad_line(&mut self, a: [Point; 3], b: [Point; 2]) -> usize {
        let quad = DQuad::from_points(a);
        let line = DLine::from_points(b);
        self.intersect_quad_line(&quad, &line)
    }

    pub fn conic_horizontal(
        &mut self,
        a: [Point; 3],
        weight: f32,
        left: f32,
        right: f32,
        y: f32,
        flipped: bool,
    ) -> usize {
        let conic = DConic::from_points(a, weight);
        self.max = 2;
        self.horizontal_conic(&conic, left as f64, right as f64, y as f64, flipped)
    }

    pub fn conic_vertical(
        &mut self,
        a: [Point; 3],
        weight: f32,
        top: f32,
        bottom: f32,
        x: f32,
        flipped: bool,
    ) -> usize {
        let conic = DConic::from_points(a, weight);
        self.max = 2;
        self.vertical_conic(&conic, top as f64, bottom as f64, x as f64, flipped)
    }

    pub fn conic_line(&mut self, a: [Point; 3], weight: f32, b: [Point; 2]) -> usize {
        let conic = DConic::from_points(a, weight);
        let line = DLine::from_points(b);
        self.max = 3;
        self.intersect_conic_line(&conic, &line)
    }

    pub fn cubic_horizontal(
        &mut self,
        a: [Point; 4],
        left: f32,
        right: f32,
        y: f32,
        flipped: bool,
    ) -> usize {
        let cubic = DCubic::from_points(a);
        self.max = 3;
        self.horizontal_cubic(&cubic, left as f64, right as f64, y as f64, flipped)
    }

    pub fn cubic_vertical(
        &mut self,
        a: [Point; 4],
        top: f32,
        bottom: f32,
        x: f32,
        flipped: bool,
    ) -> usize {
        let cubic = DCubic::from_points(a);
        self.max = 3;
        self.vertical_cubic(&cubic, top as f64, bottom as f64, x as f64, flipped)
    }

    pub fn cubic_line(&mut self, a: [Point; 4], b: [Point; 2]) -> usize {
        let cubic = DCubic::from_points(a);
        let line = DLine::from_points(b);
        self.max = 3;
        self.intersect_cubic_line(&cubic, &line)
    }
}

fn horizontal_coincident(line: &DLine, y: f64) -> i32 {
    let (min, max) = if line.pts[0].y > line.pts[1].y {
        (line.pts[1].y, line.pts[0].y)
    } else {
        (line.pts[0].y, line.pts[1].y)
    };
    if min > y || max < y {
        return 0;
    }
    if almost_equal_ulps(min, max) && max - min < (line.pts[0].x - line.pts[1].x).abs() {
        return 2;
    }
    1
}

fn vertical_coincident(line: &DLine, x: f64) -> i32 {
    let (min, max) = if line.pts[0].x > line.pts[1].x {
        (line.pts[1].x, line.pts[0].x)
    } else {
        (line.pts[0].x, line.pts[1].x)
    };
    if !precisely_between(min, x, max) {
        return 0;
    }
    if almost_equal_ulps(min, max) {
        return 2;
    }
    1
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum PinTPoint {
    Uninitialized,
    Initialized,
}

// ============================================================================
// LineQuadIntersections
// ============================================================================

struct LineQuadIntersections<'a> {
    quad: &'a DQuad,
    line: &'a DLine,
    intersections: &'a mut Intersections,
    allow_near: bool,
}

impl<'a> LineQuadIntersections<'a> {
    fn new(quad: &'a DQuad, line: &'a DLine, intersections: &'a mut Intersections) -> Self {
        intersections.set_max(5);
        Self {
            quad,
            line,
            intersections,
            allow_near: true,
        }
    }

    fn intersect_ray_static(quad: &DQuad, line: &DLine, roots: &mut [f64; 2]) -> usize {
        let adj = line.pts[1].x - line.pts[0].x;
        let opp = line.pts[1].y - line.pts[0].y;
        let mut r = [0.0f64; 3];
        for n in 0..3 {
            r[n] = (quad.pts[n].y - line.pts[0].y) * adj - (quad.pts[n].x - line.pts[0].x) * opp;
        }
        let mut a = r[2];
        let mut b = r[1];
        let c = r[0];
        a += c - 2.0 * b;
        b -= c;
        DQuad::roots_valid_t(a, 2.0 * b, c, roots)
    }

    fn check_coincident(&mut self) {
        let mut last = self.intersections.used() as i32 - 1;
        let mut index = 0i32;
        while index < last {
            let idx = index as usize;
            let quad_mid_t = (self.intersections.t[0][idx] + self.intersections.t[0][idx + 1]) / 2.0;
            let quad_mid_pt = self.quad.pt_at_t(quad_mid_t);
            let t = self.line.near_point(quad_mid_pt, None);
            if t < 0.0 {
                index += 1;
                continue;
            }
            if self.intersections.is_coincident(idx) {
                self.intersections.remove_one(idx);
                last -= 1;
            } else if self.intersections.is_coincident(idx + 1) {
                self.intersections.remove_one(idx + 1);
                last -= 1;
            } else {
                self.intersections.set_coincident(idx);
                index += 1;
            }
            self.intersections.set_coincident(index as usize);
        }
    }

    fn intersect(&mut self) -> usize {
        self.add_exact_end_points();
        if self.allow_near {
            self.add_near_end_points();
        }
        let mut root_vals = [0.0; 2];
        let roots = Self::intersect_ray_static(self.quad, self.line, &mut root_vals);
        for index in 0..roots {
            let mut quad_t = root_vals[index];
            let mut line_t = self.find_line_t(quad_t);
            let mut pt = DPoint::default();
            if self.pin_ts(&mut quad_t, &mut line_t, &mut pt, PinTPoint::Uninitialized)
                && self.unique_answer(quad_t, pt)
            {
                self.intersections.insert(quad_t, line_t, pt);
            }
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn horizontal_intersect(
        &mut self,
        axis_intercept: f64,
        left: f64,
        right: f64,
        flipped: bool,
    ) -> usize {
        self.add_exact_horizontal_end_points(left, right, axis_intercept);
        if self.allow_near {
            self.add_near_horizontal_end_points(left, right, axis_intercept);
        }
        let mut root_vals = [0.0; 2];
        let roots = Intersections::horizontal_intercept_quad(self.quad, axis_intercept, &mut root_vals);
        for index in 0..roots {
            let mut quad_t = root_vals[index];
            let mut pt = self.quad.pt_at_t(quad_t);
            let mut line_t = (pt.x - left) / (right - left);
            if self.pin_ts(&mut quad_t, &mut line_t, &mut pt, PinTPoint::Initialized)
                && self.unique_answer(quad_t, pt)
            {
                self.intersections.insert(quad_t, line_t, pt);
            }
        }
        if flipped {
            self.intersections.flip();
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn vertical_intersect(
        &mut self,
        axis_intercept: f64,
        top: f64,
        bottom: f64,
        flipped: bool,
    ) -> usize {
        self.add_exact_vertical_end_points(top, bottom, axis_intercept);
        if self.allow_near {
            self.add_near_vertical_end_points(top, bottom, axis_intercept);
        }
        let mut root_vals = [0.0; 2];
        let roots = Intersections::vertical_intercept_quad(self.quad, axis_intercept, &mut root_vals);
        for index in 0..roots {
            let mut quad_t = root_vals[index];
            let mut pt = self.quad.pt_at_t(quad_t);
            let mut line_t = (pt.y - top) / (bottom - top);
            if self.pin_ts(&mut quad_t, &mut line_t, &mut pt, PinTPoint::Initialized)
                && self.unique_answer(quad_t, pt)
            {
                self.intersections.insert(quad_t, line_t, pt);
            }
        }
        if flipped {
            self.intersections.flip();
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn unique_answer(&self, quad_t: f64, pt: DPoint) -> bool {
        for inner in 0..self.intersections.used() {
            if self.intersections.pt[inner] != pt {
                continue;
            }
            let existing_quad_t = self.intersections.t[0][inner];
            if quad_t == existing_quad_t {
                return false;
            }
            let quad_mid_t = (existing_quad_t + quad_t) / 2.0;
            let quad_mid_pt = self.quad.pt_at_t(quad_mid_t);
            if quad_mid_pt.approximately_equal(pt) {
                return false;
            }
        }
        true
    }

    fn add_exact_end_points(&mut self) {
        for q_index in [0usize, 2] {
            let line_t = self.line.exact_point(self.quad.pts[q_index]);
            if line_t < 0.0 {
                continue;
            }
            let quad_t = (q_index >> 1) as f64;
            self.intersections.insert(quad_t, line_t, self.quad.pts[q_index]);
        }
    }

    fn add_near_end_points(&mut self) {
        for q_index in [0usize, 2] {
            let quad_t = (q_index >> 1) as f64;
            if self.intersections.has_t(quad_t) {
                continue;
            }
            let line_t = self.line.near_point(self.quad.pts[q_index], None);
            if line_t < 0.0 {
                continue;
            }
            self.intersections.insert(quad_t, line_t, self.quad.pts[q_index]);
        }
        self.add_line_near_end_points();
    }

    fn add_line_near_end_points(&mut self) {
        let curve = DCurve::from_quad(*self.quad);
        for l_index in 0..2 {
            let line_t = l_index as f64;
            if self.intersections.has_opp_t(line_t) {
                continue;
            }
            let quad_t = curve.near_point(Verb::Quad, self.line.pts[l_index], self.line.pts[1 - l_index]);
            if quad_t < 0.0 {
                continue;
            }
            self.intersections.insert(quad_t, line_t, self.line.pts[l_index]);
        }
    }

    fn add_exact_horizontal_end_points(&mut self, left: f64, right: f64, y: f64) {
        for q_index in [0usize, 2] {
            let line_t = DLine::exact_point_h(self.quad.pts[q_index], left, right, y);
            if line_t < 0.0 {
                continue;
            }
            let quad_t = (q_index >> 1) as f64;
            self.intersections.insert(quad_t, line_t, self.quad.pts[q_index]);
        }
    }

    fn add_near_horizontal_end_points(&mut self, left: f64, right: f64, y: f64) {
        for q_index in [0usize, 2] {
            let quad_t = (q_index >> 1) as f64;
            if self.intersections.has_t(quad_t) {
                continue;
            }
            let line_t = DLine::near_point_h(self.quad.pts[q_index], left, right, y);
            if line_t < 0.0 {
                continue;
            }
            self.intersections.insert(quad_t, line_t, self.quad.pts[q_index]);
        }
        self.add_line_near_end_points();
    }

    fn add_exact_vertical_end_points(&mut self, top: f64, bottom: f64, x: f64) {
        for q_index in [0usize, 2] {
            let line_t = DLine::exact_point_v(self.quad.pts[q_index], top, bottom, x);
            if line_t < 0.0 {
                continue;
            }
            let quad_t = (q_index >> 1) as f64;
            self.intersections.insert(quad_t, line_t, self.quad.pts[q_index]);
        }
    }

    fn add_near_vertical_end_points(&mut self, top: f64, bottom: f64, x: f64) {
        for q_index in [0usize, 2] {
            let quad_t = (q_index >> 1) as f64;
            if self.intersections.has_t(quad_t) {
                continue;
            }
            let line_t = DLine::near_point_v(self.quad.pts[q_index], top, bottom, x);
            if line_t < 0.0 {
                continue;
            }
            self.intersections.insert(quad_t, line_t, self.quad.pts[q_index]);
        }
        self.add_line_near_end_points();
    }

    fn find_line_t(&self, t: f64) -> f64 {
        let xy = self.quad.pt_at_t(t);
        let dx = self.line.pts[1].x - self.line.pts[0].x;
        let dy = self.line.pts[1].y - self.line.pts[0].y;
        if dx.abs() > dy.abs() {
            (xy.x - self.line.pts[0].x) / dx
        } else {
            (xy.y - self.line.pts[0].y) / dy
        }
    }

    fn pin_ts(&self, quad_t: &mut f64, line_t: &mut f64, pt: &mut DPoint, pt_set: PinTPoint) -> bool {
        if !approximately_one_or_less_double(*line_t) {
            return false;
        }
        if !approximately_zero_or_more_double(*line_t) {
            return false;
        }
        let q_t = pin_t(*quad_t);
        *quad_t = q_t;
        let l_t = pin_t(*line_t);
        *line_t = l_t;
        if l_t == 0.0 || l_t == 1.0 || (pt_set == PinTPoint::Uninitialized && q_t != 0.0 && q_t != 1.0) {
            *pt = self.line.pt_at_t(l_t);
        } else if pt_set == PinTPoint::Uninitialized {
            *pt = self.quad.pt_at_t(q_t);
        }
        let grid_pt = pt.as_point();
        if DPoint::approximately_equal_pts(grid_pt, self.line.pts[0].as_point()) {
            *pt = self.line.pts[0];
            *line_t = 0.0;
        } else if DPoint::approximately_equal_pts(grid_pt, self.line.pts[1].as_point()) {
            *pt = self.line.pts[1];
            *line_t = 1.0;
        }
        if self.intersections.used() > 0 && approximately_equal(self.intersections.t[1][0], *line_t) {
            return false;
        }
        if grid_pt == self.quad.pts[0].as_point() {
            *pt = self.quad.pts[0];
            *quad_t = 0.0;
        } else if grid_pt == self.quad.pts[2].as_point() {
            *pt = self.quad.pts[2];
            *quad_t = 1.0;
        }
        true
    }
}

// ============================================================================
// LineConicIntersections
// ============================================================================

fn conic_valid_t(r: &[f64; 3], weight: f32, axis_intercept: f64, roots: &mut [f64; 2]) -> usize {
    let w = weight as f64;
    let mut a = r[2];
    let mut b = r[1] * w - axis_intercept * w + axis_intercept;
    let mut c = r[0];
    a += c - 2.0 * b;
    b -= c;
    c -= axis_intercept;
    DQuad::roots_valid_t(a, 2.0 * b, c, roots)
}

struct LineConicIntersections<'a> {
    conic: &'a DConic,
    line: &'a DLine,
    intersections: &'a mut Intersections,
    allow_near: bool,
}

impl<'a> LineConicIntersections<'a> {
    fn new(conic: &'a DConic, line: &'a DLine, intersections: &'a mut Intersections) -> Self {
        intersections.set_max(4);
        Self {
            conic,
            line,
            intersections,
            allow_near: true,
        }
    }

    fn intersect_ray_static(conic: &DConic, line: &DLine, roots: &mut [f64; 2]) -> usize {
        let adj = line.pts[1].x - line.pts[0].x;
        let opp = line.pts[1].y - line.pts[0].y;
        let mut r = [0.0f64; 3];
        for n in 0..3 {
            r[n] = (conic.pts.pts[n].y - line.pts[0].y) * adj
                - (conic.pts.pts[n].x - line.pts[0].x) * opp;
        }
        conic_valid_t(&r, conic.weight, 0.0, roots)
    }

    fn check_coincident(&mut self) {
        let mut last = self.intersections.used() as i32 - 1;
        let mut index = 0i32;
        while index < last {
            let idx = index as usize;
            let conic_mid_t =
                (self.intersections.t[0][idx] + self.intersections.t[0][idx + 1]) / 2.0;
            let conic_mid_pt = self.conic.pt_at_t(conic_mid_t);
            let t = self.line.near_point(conic_mid_pt, None);
            if t < 0.0 {
                index += 1;
                continue;
            }
            if self.intersections.is_coincident(idx) {
                self.intersections.remove_one(idx);
                last -= 1;
            } else if self.intersections.is_coincident(idx + 1) {
                self.intersections.remove_one(idx + 1);
                last -= 1;
            } else {
                self.intersections.set_coincident(idx);
                index += 1;
            }
            self.intersections.set_coincident(index as usize);
        }
    }

    fn intersect(&mut self) -> usize {
        self.add_exact_end_points();
        if self.allow_near {
            self.add_near_end_points();
        }
        let mut root_vals = [0.0; 2];
        let roots = Self::intersect_ray_static(self.conic, self.line, &mut root_vals);
        for index in 0..roots {
            let mut conic_t = root_vals[index];
            let mut line_t = self.find_line_t(conic_t);
            let mut pt = DPoint::default();
            if self.pin_ts(&mut conic_t, &mut line_t, &mut pt, PinTPoint::Uninitialized)
                && self.unique_answer(conic_t, pt)
            {
                self.intersections.insert(conic_t, line_t, pt);
            }
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn horizontal_intersect(
        &mut self,
        axis_intercept: f64,
        left: f64,
        right: f64,
        flipped: bool,
    ) -> usize {
        self.add_exact_horizontal_end_points(left, right, axis_intercept);
        if self.allow_near {
            self.add_near_horizontal_end_points(left, right, axis_intercept);
        }
        let mut roots = [0.0; 2];
        let count =
            Intersections::horizontal_intercept_conic(self.conic, axis_intercept, &mut roots);
        for index in 0..count {
            let mut conic_t = roots[index];
            let mut pt = self.conic.pt_at_t(conic_t);
            let mut line_t = (pt.x - left) / (right - left);
            if self.pin_ts(&mut conic_t, &mut line_t, &mut pt, PinTPoint::Initialized)
                && self.unique_answer(conic_t, pt)
            {
                self.intersections.insert(conic_t, line_t, pt);
            }
        }
        if flipped {
            self.intersections.flip();
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn vertical_intersect(
        &mut self,
        axis_intercept: f64,
        top: f64,
        bottom: f64,
        flipped: bool,
    ) -> usize {
        self.add_exact_vertical_end_points(top, bottom, axis_intercept);
        if self.allow_near {
            self.add_near_vertical_end_points(top, bottom, axis_intercept);
        }
        let mut roots = [0.0; 2];
        let count = Intersections::vertical_intercept_conic(self.conic, axis_intercept, &mut roots);
        for index in 0..count {
            let mut conic_t = roots[index];
            let mut pt = self.conic.pt_at_t(conic_t);
            let mut line_t = (pt.y - top) / (bottom - top);
            if self.pin_ts(&mut conic_t, &mut line_t, &mut pt, PinTPoint::Initialized)
                && self.unique_answer(conic_t, pt)
            {
                self.intersections.insert(conic_t, line_t, pt);
            }
        }
        if flipped {
            self.intersections.flip();
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn add_exact_end_points(&mut self) {
        for c_index in [0usize, 2] {
            let line_t = self.line.exact_point(self.conic.pts.pts[c_index]);
            if line_t < 0.0 {
                continue;
            }
            let conic_t = (c_index >> 1) as f64;
            self.intersections
                .insert(conic_t, line_t, self.conic.pts.pts[c_index]);
        }
    }

    fn add_near_end_points(&mut self) {
        for c_index in [0usize, 2] {
            let conic_t = (c_index >> 1) as f64;
            if self.intersections.has_t(conic_t) {
                continue;
            }
            let line_t = self.line.near_point(self.conic.pts.pts[c_index], None);
            if line_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(conic_t, line_t, self.conic.pts.pts[c_index]);
        }
        self.add_line_near_end_points();
    }

    fn add_line_near_end_points(&mut self) {
        let curve = DCurve::from_conic(*self.conic);
        for l_index in 0..2 {
            let line_t = l_index as f64;
            if self.intersections.has_opp_t(line_t) {
                continue;
            }
            let conic_t =
                curve.near_point(Verb::Conic, self.line.pts[l_index], self.line.pts[1 - l_index]);
            if conic_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(conic_t, line_t, self.line.pts[l_index]);
        }
    }

    fn add_exact_horizontal_end_points(&mut self, left: f64, right: f64, y: f64) {
        for c_index in [0usize, 2] {
            let line_t = DLine::exact_point_h(self.conic.pts.pts[c_index], left, right, y);
            if line_t < 0.0 {
                continue;
            }
            let conic_t = (c_index >> 1) as f64;
            self.intersections
                .insert(conic_t, line_t, self.conic.pts.pts[c_index]);
        }
    }

    fn add_near_horizontal_end_points(&mut self, left: f64, right: f64, y: f64) {
        for c_index in [0usize, 2] {
            let conic_t = (c_index >> 1) as f64;
            if self.intersections.has_t(conic_t) {
                continue;
            }
            let line_t = DLine::near_point_h(self.conic.pts.pts[c_index], left, right, y);
            if line_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(conic_t, line_t, self.conic.pts.pts[c_index]);
        }
        self.add_line_near_end_points();
    }

    fn add_exact_vertical_end_points(&mut self, top: f64, bottom: f64, x: f64) {
        for c_index in [0usize, 2] {
            let line_t = DLine::exact_point_v(self.conic.pts.pts[c_index], top, bottom, x);
            if line_t < 0.0 {
                continue;
            }
            let conic_t = (c_index >> 1) as f64;
            self.intersections
                .insert(conic_t, line_t, self.conic.pts.pts[c_index]);
        }
    }

    fn add_near_vertical_end_points(&mut self, top: f64, bottom: f64, x: f64) {
        for c_index in [0usize, 2] {
            let conic_t = (c_index >> 1) as f64;
            if self.intersections.has_t(conic_t) {
                continue;
            }
            let line_t = DLine::near_point_v(self.conic.pts.pts[c_index], top, bottom, x);
            if line_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(conic_t, line_t, self.conic.pts.pts[c_index]);
        }
        self.add_line_near_end_points();
    }

    fn find_line_t(&self, t: f64) -> f64 {
        let xy = self.conic.pt_at_t(t);
        let dx = self.line.pts[1].x - self.line.pts[0].x;
        let dy = self.line.pts[1].y - self.line.pts[0].y;
        if dx.abs() > dy.abs() {
            (xy.x - self.line.pts[0].x) / dx
        } else {
            (xy.y - self.line.pts[0].y) / dy
        }
    }

    fn pin_ts(&self, conic_t: &mut f64, line_t: &mut f64, pt: &mut DPoint, pt_set: PinTPoint) -> bool {
        if !approximately_one_or_less_double(*line_t) {
            return false;
        }
        if !approximately_zero_or_more_double(*line_t) {
            return false;
        }
        let q_t = pin_t(*conic_t);
        *conic_t = q_t;
        let l_t = pin_t(*line_t);
        *line_t = l_t;
        if l_t == 0.0 || l_t == 1.0 || (pt_set == PinTPoint::Uninitialized && q_t != 0.0 && q_t != 1.0) {
            *pt = self.line.pt_at_t(l_t);
        } else if pt_set == PinTPoint::Uninitialized {
            *pt = self.conic.pt_at_t(q_t);
        }
        let grid_pt = pt.as_point();
        if DPoint::approximately_equal_pts(grid_pt, self.line.pts[0].as_point()) {
            *pt = self.line.pts[0];
            *line_t = 0.0;
        } else if DPoint::approximately_equal_pts(grid_pt, self.line.pts[1].as_point()) {
            *pt = self.line.pts[1];
            *line_t = 1.0;
        }
        if self.intersections.used() > 0 && approximately_equal(self.intersections.t[1][0], *line_t) {
            return false;
        }
        if grid_pt == self.conic.pts.pts[0].as_point() {
            *pt = self.conic.pts.pts[0];
            *conic_t = 0.0;
        } else if grid_pt == self.conic.pts.pts[2].as_point() {
            *pt = self.conic.pts.pts[2];
            *conic_t = 1.0;
        }
        true
    }

    fn unique_answer(&self, conic_t: f64, pt: DPoint) -> bool {
        for inner in 0..self.intersections.used() {
            if self.intersections.pt[inner] != pt {
                continue;
            }
            let existing_conic_t = self.intersections.t[0][inner];
            if conic_t == existing_conic_t {
                return false;
            }
            let conic_mid_t = (existing_conic_t + conic_t) / 2.0;
            let conic_mid_pt = self.conic.pt_at_t(conic_mid_t);
            if conic_mid_pt.approximately_equal(pt) {
                return false;
            }
        }
        true
    }
}

// ============================================================================
// LineCubicIntersections
// ============================================================================

struct LineCubicIntersections<'a> {
    cubic: &'a DCubic,
    line: &'a DLine,
    intersections: &'a mut Intersections,
    allow_near: bool,
}

impl<'a> LineCubicIntersections<'a> {
    fn new(cubic: &'a DCubic, line: &'a DLine, intersections: &'a mut Intersections) -> Self {
        intersections.set_max(4);
        Self {
            cubic,
            line,
            intersections,
            allow_near: true,
        }
    }

    fn intersect_ray_static(cubic: &DCubic, line: &DLine, roots: &mut [f64; 3]) -> usize {
        let adj = line.pts[1].x - line.pts[0].x;
        let opp = line.pts[1].y - line.pts[0].y;
        let mut c = DCubic::default();
        for n in 0..4 {
            c.pts[n].x =
                (cubic.pts[n].y - line.pts[0].y) * adj - (cubic.pts[n].x - line.pts[0].x) * opp;
        }
        let (a, b, cc, d) =
            DCubic::coefficients(c.pts[0].x, c.pts[1].x, c.pts[2].x, c.pts[3].x);
        let mut count = DCubic::roots_valid_t(a, b, cc, d, roots);
        for index in 0..count {
            let calc_pt = c.pt_at_t(roots[index]);
            if !approximately_zero(calc_pt.x) {
                for n in 0..4 {
                    c.pts[n].y = (cubic.pts[n].y - line.pts[0].y) * opp
                        + (cubic.pts[n].x - line.pts[0].x) * adj;
                }
                let mut extreme_ts = [0.0; 6];
                let mut ext = [0.0; 2];
                let extrema =
                    DCubic::find_extrema(c.pts[0].x, c.pts[1].x, c.pts[2].x, c.pts[3].x, &mut ext);
                extreme_ts[..extrema].copy_from_slice(&ext[..extrema]);
                count = c.search_roots(&mut extreme_ts, extrema, 0.0, SearchAxis::XAxis, roots);
                break;
            }
        }
        count
    }

    fn check_coincident(&mut self) {
        let mut last = self.intersections.used() as i32 - 1;
        let mut index = 0i32;
        while index < last {
            let idx = index as usize;
            let cubic_mid_t =
                (self.intersections.t[0][idx] + self.intersections.t[0][idx + 1]) / 2.0;
            let cubic_mid_pt = self.cubic.pt_at_t(cubic_mid_t);
            let t = self.line.near_point(cubic_mid_pt, None);
            if t < 0.0 {
                index += 1;
                continue;
            }
            if self.intersections.is_coincident(idx) {
                self.intersections.remove_one(idx);
                last -= 1;
            } else if self.intersections.is_coincident(idx + 1) {
                self.intersections.remove_one(idx + 1);
                last -= 1;
            } else {
                self.intersections.set_coincident(idx);
                index += 1;
            }
            self.intersections.set_coincident(index as usize);
        }
    }

    fn intersect(&mut self) -> usize {
        self.add_exact_end_points();
        if self.allow_near {
            self.add_near_end_points();
        }
        let mut root_vals = [0.0; 3];
        let roots = Self::intersect_ray_static(self.cubic, self.line, &mut root_vals);
        for index in 0..roots {
            let mut cubic_t = root_vals[index];
            let mut line_t = self.find_line_t(cubic_t);
            let mut pt = DPoint::default();
            if self.pin_ts(&mut cubic_t, &mut line_t, &mut pt, PinTPoint::Uninitialized)
                && self.unique_answer(cubic_t, pt)
            {
                self.intersections.insert(cubic_t, line_t, pt);
            }
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn horizontal_intersect(
        &mut self,
        axis_intercept: f64,
        left: f64,
        right: f64,
        flipped: bool,
    ) -> usize {
        self.add_exact_horizontal_end_points(left, right, axis_intercept);
        if self.allow_near {
            self.add_near_horizontal_end_points(left, right, axis_intercept);
        }
        let mut roots = [0.0; 3];
        let count =
            Intersections::horizontal_intercept_cubic(self.cubic, axis_intercept, &mut roots);
        for index in 0..count {
            let mut cubic_t = roots[index];
            let mut pt = DPoint::new(self.cubic.pt_at_t(cubic_t).x, axis_intercept);
            let mut line_t = (pt.x - left) / (right - left);
            if self.pin_ts(&mut cubic_t, &mut line_t, &mut pt, PinTPoint::Initialized)
                && self.unique_answer(cubic_t, pt)
            {
                self.intersections.insert(cubic_t, line_t, pt);
            }
        }
        if flipped {
            self.intersections.flip();
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn vertical_intersect(
        &mut self,
        axis_intercept: f64,
        top: f64,
        bottom: f64,
        flipped: bool,
    ) -> usize {
        self.add_exact_vertical_end_points(top, bottom, axis_intercept);
        if self.allow_near {
            self.add_near_vertical_end_points(top, bottom, axis_intercept);
        }
        let mut roots = [0.0; 3];
        let count = Intersections::vertical_intercept_cubic(self.cubic, axis_intercept, &mut roots);
        for index in 0..count {
            let mut cubic_t = roots[index];
            let mut pt = DPoint::new(axis_intercept, self.cubic.pt_at_t(cubic_t).y);
            let mut line_t = (pt.y - top) / (bottom - top);
            if self.pin_ts(&mut cubic_t, &mut line_t, &mut pt, PinTPoint::Initialized)
                && self.unique_answer(cubic_t, pt)
            {
                self.intersections.insert(cubic_t, line_t, pt);
            }
        }
        if flipped {
            self.intersections.flip();
        }
        self.check_coincident();
        self.intersections.used()
    }

    fn unique_answer(&self, cubic_t: f64, pt: DPoint) -> bool {
        for inner in 0..self.intersections.used() {
            if self.intersections.pt[inner] != pt {
                continue;
            }
            let existing_cubic_t = self.intersections.t[0][inner];
            if cubic_t == existing_cubic_t {
                return false;
            }
            let cubic_mid_t = (existing_cubic_t + cubic_t) / 2.0;
            let cubic_mid_pt = self.cubic.pt_at_t(cubic_mid_t);
            if cubic_mid_pt.approximately_equal(pt) {
                return false;
            }
        }
        true
    }

    fn add_exact_end_points(&mut self) {
        for c_index in [0usize, 3] {
            let line_t = self.line.exact_point(self.cubic.pts[c_index]);
            if line_t < 0.0 {
                continue;
            }
            let cubic_t = (c_index >> 1) as f64;
            self.intersections
                .insert(cubic_t, line_t, self.cubic.pts[c_index]);
        }
    }

    fn add_near_end_points(&mut self) {
        for c_index in [0usize, 3] {
            let cubic_t = (c_index >> 1) as f64;
            if self.intersections.has_t(cubic_t) {
                continue;
            }
            let line_t = self.line.near_point(self.cubic.pts[c_index], None);
            if line_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(cubic_t, line_t, self.cubic.pts[c_index]);
        }
        self.add_line_near_end_points();
    }

    fn add_line_near_end_points(&mut self) {
        let curve = DCurve::from_cubic(*self.cubic);
        for l_index in 0..2 {
            let line_t = l_index as f64;
            if self.intersections.has_opp_t(line_t) {
                continue;
            }
            let cubic_t =
                curve.near_point(Verb::Cubic, self.line.pts[l_index], self.line.pts[1 - l_index]);
            if cubic_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(cubic_t, line_t, self.line.pts[l_index]);
        }
    }

    fn add_exact_horizontal_end_points(&mut self, left: f64, right: f64, y: f64) {
        for c_index in [0usize, 3] {
            let line_t = DLine::exact_point_h(self.cubic.pts[c_index], left, right, y);
            if line_t < 0.0 {
                continue;
            }
            let cubic_t = (c_index >> 1) as f64;
            self.intersections
                .insert(cubic_t, line_t, self.cubic.pts[c_index]);
        }
    }

    fn add_near_horizontal_end_points(&mut self, left: f64, right: f64, y: f64) {
        for c_index in [0usize, 3] {
            let cubic_t = (c_index >> 1) as f64;
            if self.intersections.has_t(cubic_t) {
                continue;
            }
            let line_t = DLine::near_point_h(self.cubic.pts[c_index], left, right, y);
            if line_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(cubic_t, line_t, self.cubic.pts[c_index]);
        }
        self.add_line_near_end_points();
    }

    fn add_exact_vertical_end_points(&mut self, top: f64, bottom: f64, x: f64) {
        for c_index in [0usize, 3] {
            let line_t = DLine::exact_point_v(self.cubic.pts[c_index], top, bottom, x);
            if line_t < 0.0 {
                continue;
            }
            let cubic_t = (c_index >> 1) as f64;
            self.intersections
                .insert(cubic_t, line_t, self.cubic.pts[c_index]);
        }
    }

    fn add_near_vertical_end_points(&mut self, top: f64, bottom: f64, x: f64) {
        for c_index in [0usize, 3] {
            let cubic_t = (c_index >> 1) as f64;
            if self.intersections.has_t(cubic_t) {
                continue;
            }
            let line_t = DLine::near_point_v(self.cubic.pts[c_index], top, bottom, x);
            if line_t < 0.0 {
                continue;
            }
            self.intersections
                .insert(cubic_t, line_t, self.cubic.pts[c_index]);
        }
        self.add_line_near_end_points();
    }

    fn find_line_t(&self, t: f64) -> f64 {
        let xy = self.cubic.pt_at_t(t);
        let dx = self.line.pts[1].x - self.line.pts[0].x;
        let dy = self.line.pts[1].y - self.line.pts[0].y;
        if dx.abs() > dy.abs() {
            (xy.x - self.line.pts[0].x) / dx
        } else {
            (xy.y - self.line.pts[0].y) / dy
        }
    }

    fn pin_ts(
        &self,
        cubic_t: &mut f64,
        line_t: &mut f64,
        pt: &mut DPoint,
        pt_set: PinTPoint,
    ) -> bool {
        if !approximately_one_or_less(*line_t) {
            return false;
        }
        if !approximately_zero_or_more(*line_t) {
            return false;
        }
        let c_t = pin_t(*cubic_t);
        *cubic_t = c_t;
        let l_t = pin_t(*line_t);
        *line_t = l_t;
        let l_pt = self.line.pt_at_t(l_t);
        let c_pt = self.cubic.pt_at_t(c_t);
        if !l_pt.roughly_equal(c_pt) {
            return false;
        }
        if l_t == 0.0 || l_t == 1.0 || (pt_set == PinTPoint::Uninitialized && c_t != 0.0 && c_t != 1.0) {
            *pt = l_pt;
        } else if pt_set == PinTPoint::Uninitialized {
            *pt = c_pt;
        }
        let grid_pt = pt.as_point();
        if grid_pt == self.line.pts[0].as_point() {
            *line_t = 0.0;
        } else if grid_pt == self.line.pts[1].as_point() {
            *line_t = 1.0;
        }
        if grid_pt == self.cubic.pts[0].as_point() && approximately_equal(*cubic_t, 0.0) {
            *cubic_t = 0.0;
        } else if grid_pt == self.cubic.pts[3].as_point() && approximately_equal(*cubic_t, 1.0) {
            *cubic_t = 1.0;
        }
        true
    }
}
