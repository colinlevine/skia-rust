#![forbid(unsafe_code)]

//! Port of `SkPathOpsCurve.h/.cpp` and `SkReduceOrder.h/.cpp`.

use skia_rs_geometry::Point;
use skia_rs_path::types::Verb;

use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::intersections::Intersections;
use crate::line::DLine;
use crate::quad::DQuad;
use crate::types::{
    almost_between_ulps, almost_equal_ulps, almost_equal_ulps_pin, approximately_equal,
    approximately_equal_half, approximately_zero, between, cpp_max_f64, cpp_min_f64,
    pathops_points_to_verb, pathops_verb_to_points, pin_t, roughly_zero_when_compared_to, DPoint,
    DVector, PathOpsBounds,
};

/// Safe Rust equivalent of `SkDCurve` (an algebraic curve of degree 1..3, or rational quad).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DCurve {
    pub pts: [DPoint; 4],
    pub weight: f32,
}

impl DCurve {
    #[inline]
    pub fn from_line(l: DLine) -> Self {
        Self {
            pts: [l.pts[0], l.pts[1], DPoint::default(), DPoint::default()],
            weight: 1.0,
        }
    }

    #[inline]
    pub fn from_quad(q: DQuad) -> Self {
        Self {
            pts: [q.pts[0], q.pts[1], q.pts[2], DPoint::default()],
            weight: 1.0,
        }
    }

    #[inline]
    pub fn from_conic(c: DConic) -> Self {
        Self {
            pts: [c.pts.pts[0], c.pts.pts[1], c.pts.pts[2], DPoint::default()],
            weight: c.weight,
        }
    }

    #[inline]
    pub fn from_cubic(c: DCubic) -> Self {
        Self {
            pts: c.pts,
            weight: 1.0,
        }
    }

    #[inline]
    pub fn as_line(&self) -> DLine {
        DLine {
            pts: [self.pts[0], self.pts[1]],
        }
    }

    #[inline]
    pub fn as_quad(&self) -> DQuad {
        DQuad {
            pts: [self.pts[0], self.pts[1], self.pts[2]],
        }
    }

    #[inline]
    pub fn as_conic(&self) -> DConic {
        DConic {
            pts: self.as_quad(),
            weight: self.weight,
        }
    }

    #[inline]
    pub fn as_cubic(&self) -> DCubic {
        DCubic { pts: self.pts }
    }

    #[inline]
    pub fn pt_at_t(&self, verb: Verb, t: f64) -> DPoint {
        match verb {
            Verb::Line => self.as_line().pt_at_t(t),
            Verb::Quad => self.as_quad().pt_at_t(t),
            Verb::Conic => self.as_conic().pt_at_t(t),
            Verb::Cubic => self.as_cubic().pt_at_t(t),
            _ => self.pts[0],
        }
    }

    #[inline]
    pub fn dxdy_at_t(&self, verb: Verb, t: f64) -> DVector {
        match verb {
            Verb::Line => self.pts[1].sub(self.pts[0]),
            Verb::Quad => self.as_quad().dxdy_at_t(t),
            Verb::Conic => self.as_conic().dxdy_at_t(t),
            Verb::Cubic => self.as_cubic().dxdy_at_t(t),
            _ => DVector::default(),
        }
    }

    pub fn intersect_ray(&self, verb: Verb, ray: &DLine, i: &mut Intersections) {
        match verb {
            Verb::Line => {
                i.intersect_ray_line_line(&self.as_line(), ray);
            }
            Verb::Quad => {
                i.intersect_ray_quad_line(&self.as_quad(), ray);
            }
            Verb::Conic => {
                i.intersect_ray_conic_line(&self.as_conic(), ray);
            }
            Verb::Cubic => {
                i.intersect_ray_cubic_line(&self.as_cubic(), ray);
            }
            _ => {}
        }
    }

    pub fn near_point(&self, verb: Verb, xy: DPoint, opp: DPoint) -> f64 {
        let count = pathops_verb_to_points(verb);
        let mut min_x = self.pts[0].x;
        let mut max_x = min_x;
        for index in 1..=count {
            min_x = cpp_min_f64(min_x, self.pts[index].x);
            max_x = cpp_max_f64(max_x, self.pts[index].x);
        }
        if !almost_between_ulps(min_x, xy.x, max_x) {
            return -1.0;
        }
        let mut min_y = self.pts[0].y;
        let mut max_y = min_y;
        for index in 1..=count {
            min_y = cpp_min_f64(min_y, self.pts[index].y);
            max_y = cpp_max_f64(max_y, self.pts[index].y);
        }
        if !almost_between_ulps(min_y, xy.y, max_y) {
            return -1.0;
        }
        let mut i = Intersections::new();
        let perp = DLine::new(
            xy,
            DPoint::new(xy.x + opp.y - xy.y, xy.y + xy.x - opp.x),
        );
        self.intersect_ray(verb, &perp, &mut i);
        let mut min_index: i32 = -1;
        let mut min_dist = f32::MAX as f64;
        for index in 0..i.used() {
            let dist = xy.distance(i.pt[index]);
            if min_dist > dist {
                min_dist = dist;
                min_index = index as i32;
            }
        }
        if min_index < 0 {
            return -1.0;
        }
        let largest = cpp_max_f64(cpp_max_f64(max_x, max_y), -cpp_min_f64(min_x, min_y));
        if !almost_equal_ulps_pin(largest, largest + min_dist) {
            return -1.0;
        }
        pin_t(i.t[0][min_index as usize])
    }

    pub fn set_conic_bounds(
        &self,
        curve: [Point; 3],
        curve_weight: f32,
        t_start: f64,
        t_end: f64,
    ) -> PathOpsBounds {
        let d_curve = DConic::from_points(curve, curve_weight);
        let d_rect = d_curve.set_bounds(&self.as_conic(), t_start, t_end);
        PathOpsBounds::new(
            d_rect.left as f32,
            d_rect.top as f32,
            d_rect.right as f32,
            d_rect.bottom as f32,
        )
    }

    pub fn set_cubic_bounds(&self, curve: [Point; 4], t_start: f64, t_end: f64) -> PathOpsBounds {
        let d_curve = DCubic::from_points(curve);
        let d_rect = d_curve.set_bounds(&self.as_cubic(), t_start, t_end);
        PathOpsBounds::new(
            d_rect.left as f32,
            d_rect.top as f32,
            d_rect.right as f32,
            d_rect.bottom as f32,
        )
    }

    pub fn set_quad_bounds(&self, curve: [Point; 3], t_start: f64, t_end: f64) -> PathOpsBounds {
        let d_curve = DQuad::from_points(curve);
        let d_rect = d_curve.set_bounds(&self.as_quad(), t_start, t_end);
        PathOpsBounds::new(
            d_rect.left as f32,
            d_rect.top as f32,
            d_rect.right as f32,
            d_rect.bottom as f32,
        )
    }
}

/// Mirrors `SkDCurveSweep`.
#[derive(Copy, Clone, Debug, Default)]
pub struct DCurveSweep {
    pub curve: DCurve,
    pub sweep: [DVector; 2],
    pub is_curve: bool,
    pub ordered: bool,
}

impl DCurveSweep {
    pub fn set_curve_hull_sweep(&mut self, verb: Verb) {
        self.ordered = true;
        self.sweep[0] = self.curve.pts[1].sub(self.curve.pts[0]);
        if verb == Verb::Line {
            self.sweep[1] = self.sweep[0];
            self.is_curve = false;
            return;
        }
        self.sweep[1] = self.curve.pts[2].sub(self.curve.pts[0]);
        let mut max_val = 0.0f64;
        for index in 0..=pathops_verb_to_points(verb) {
            max_val = cpp_max_f64(
                max_val,
                cpp_max_f64(self.curve.pts[index].x.abs(), self.curve.pts[index].y.abs()),
            );
        }
        if verb != Verb::Cubic {
            if roughly_zero_when_compared_to(self.sweep[0].x, max_val)
                && roughly_zero_when_compared_to(self.sweep[0].y, max_val)
            {
                self.sweep[0] = self.sweep[1];
            }
            self.is_curve = self.sweep[0].cross_check(self.sweep[1]) != 0.0;
            return;
        }
        let third_sweep = self.curve.pts[3].sub(self.curve.pts[0]);
        if self.sweep[0].x == 0.0 && self.sweep[0].y == 0.0 {
            self.sweep[0] = self.sweep[1];
            self.sweep[1] = third_sweep;
            if roughly_zero_when_compared_to(self.sweep[0].x, max_val)
                && roughly_zero_when_compared_to(self.sweep[0].y, max_val)
            {
                self.sweep[0] = self.sweep[1];
                self.curve.pts[1] = self.curve.pts[3];
            }
            self.is_curve = self.sweep[0].cross_check(self.sweep[1]) != 0.0;
            return;
        }
        let s1x3 = self.sweep[0].cross_check(third_sweep);
        let s3x2 = third_sweep.cross_check(self.sweep[1]);
        if s1x3 * s3x2 >= 0.0 {
            self.is_curve = self.sweep[0].cross_check(self.sweep[1]) != 0.0;
            return;
        }
        let s2x1 = self.sweep[1].cross_check(self.sweep[0]);
        if s3x2 * s2x1 < 0.0 {
            self.sweep[0] = self.sweep[1];
            self.ordered = false;
        }
        self.sweep[1] = third_sweep;
        self.is_curve = self.sweep[0].cross_check(self.sweep[1]) != 0.0;
    }
}

// ============================================================================
// Curve helper dispatch functions from SkPathOpsCurve.h
// ============================================================================

#[inline]
pub fn curve_dpoint_at_t(pts: &[Point], weight: f32, verb: Verb, t: f64) -> DPoint {
    match verb {
        Verb::Line => DLine::from_points([pts[0], pts[1]]).pt_at_t(t),
        Verb::Quad => DQuad::from_points([pts[0], pts[1], pts[2]]).pt_at_t(t),
        Verb::Conic => DConic::from_points([pts[0], pts[1], pts[2]], weight).pt_at_t(t),
        Verb::Cubic => DCubic::from_points([pts[0], pts[1], pts[2], pts[3]]).pt_at_t(t),
        _ => DPoint::from_point(pts[0]),
    }
}

#[inline]
pub fn curve_point_at_t(pts: &[Point], weight: f32, verb: Verb, t: f64) -> Point {
    curve_dpoint_at_t(pts, weight, verb, t).as_point()
}

#[inline]
pub fn curve_dslope_at_t(pts: &[Point], weight: f32, verb: Verb, t: f64) -> DVector {
    match verb {
        Verb::Line => {
            let l = DLine::from_points([pts[0], pts[1]]);
            l.pts[1].sub(l.pts[0])
        }
        Verb::Quad => DQuad::from_points([pts[0], pts[1], pts[2]]).dxdy_at_t(t),
        Verb::Conic => DConic::from_points([pts[0], pts[1], pts[2]], weight).dxdy_at_t(t),
        Verb::Cubic => DCubic::from_points([pts[0], pts[1], pts[2], pts[3]]).dxdy_at_t(t),
        _ => DVector::default(),
    }
}

#[inline]
pub fn curve_is_vertical(pts: &[Point], weight: f32, verb: Verb, start_t: f64, end_t: f64) -> bool {
    match verb {
        Verb::Line => {
            let line = DLine::from_points([pts[0], pts[1]]);
            almost_equal_ulps(line.pt_at_t(start_t).x, line.pt_at_t(end_t).x)
        }
        Verb::Quad => {
            let dst = DQuad::from_points([pts[0], pts[1], pts[2]]).sub_divide(start_t, end_t);
            almost_equal_ulps(dst.pts[0].x, dst.pts[1].x)
                && almost_equal_ulps(dst.pts[1].x, dst.pts[2].x)
        }
        Verb::Conic => {
            let dst =
                DConic::from_points([pts[0], pts[1], pts[2]], weight).sub_divide(start_t, end_t);
            almost_equal_ulps(dst.pts.pts[0].x, dst.pts.pts[1].x)
                && almost_equal_ulps(dst.pts.pts[1].x, dst.pts.pts[2].x)
        }
        Verb::Cubic => {
            let dst =
                DCubic::from_points([pts[0], pts[1], pts[2], pts[3]]).sub_divide(start_t, end_t);
            almost_equal_ulps(dst.pts[0].x, dst.pts[1].x)
                && almost_equal_ulps(dst.pts[1].x, dst.pts[2].x)
                && almost_equal_ulps(dst.pts[2].x, dst.pts[3].x)
        }
        _ => false,
    }
}

pub fn curve_intersect_ray(
    pts: &[Point],
    weight: f32,
    verb: Verb,
    ray: &DLine,
    i: &mut Intersections,
) {
    match verb {
        Verb::Line => {
            i.intersect_ray_line_line(&DLine::from_points([pts[0], pts[1]]), ray);
        }
        Verb::Quad => {
            i.intersect_ray_quad_line(&DQuad::from_points([pts[0], pts[1], pts[2]]), ray);
        }
        Verb::Conic => {
            i.intersect_ray_conic_line(&DConic::from_points([pts[0], pts[1], pts[2]], weight), ray);
        }
        Verb::Cubic => {
            i.intersect_ray_cubic_line(
                &DCubic::from_points([pts[0], pts[1], pts[2], pts[3]]),
                ray,
            );
        }
        _ => {}
    }
}

pub fn curve_intercept_h(pts: &[Point], weight: f32, verb: Verb, y: f32, roots: &mut [f64; 3]) -> usize {
    match verb {
        Verb::Line => {
            if pts[0].y == pts[1].y {
                return 0;
            }
            let line = DLine::from_points([pts[0], pts[1]]);
            roots[0] = Intersections::horizontal_intercept_line(&line, y as f64);
            usize::from(between(0.0, roots[0], 1.0))
        }
        Verb::Quad => {
            let mut r = [0.0; 2];
            let n = Intersections::horizontal_intercept_quad(
                &DQuad::from_points([pts[0], pts[1], pts[2]]),
                y as f64,
                &mut r,
            );
            roots[..n].copy_from_slice(&r[..n]);
            n
        }
        Verb::Conic => {
            let mut r = [0.0; 2];
            let n = Intersections::horizontal_intercept_conic(
                &DConic::from_points([pts[0], pts[1], pts[2]], weight),
                y as f64,
                &mut r,
            );
            roots[..n].copy_from_slice(&r[..n]);
            n
        }
        Verb::Cubic => {
            DCubic::from_points([pts[0], pts[1], pts[2], pts[3]])
                .horizontal_intersect(y as f64, roots)
        }
        _ => 0,
    }
}

pub fn curve_intercept_v(pts: &[Point], weight: f32, verb: Verb, x: f32, roots: &mut [f64; 3]) -> usize {
    match verb {
        Verb::Line => {
            if pts[0].x == pts[1].x {
                return 0;
            }
            let line = DLine::from_points([pts[0], pts[1]]);
            roots[0] = Intersections::vertical_intercept_line(&line, x as f64);
            usize::from(between(0.0, roots[0], 1.0))
        }
        Verb::Quad => {
            let mut r = [0.0; 2];
            let n = Intersections::vertical_intercept_quad(
                &DQuad::from_points([pts[0], pts[1], pts[2]]),
                x as f64,
                &mut r,
            );
            roots[..n].copy_from_slice(&r[..n]);
            n
        }
        Verb::Conic => {
            let mut r = [0.0; 2];
            let n = Intersections::vertical_intercept_conic(
                &DConic::from_points([pts[0], pts[1], pts[2]], weight),
                x as f64,
                &mut r,
            );
            roots[..n].copy_from_slice(&r[..n]);
            n
        }
        Verb::Cubic => {
            DCubic::from_points([pts[0], pts[1], pts[2], pts[3]])
                .vertical_intersect(x as f64, roots)
        }
        _ => 0,
    }
}

// ============================================================================
// SkReduceOrder port
// ============================================================================

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReduceQuadratics {
    NoQuadratics = 0,
    AllowQuadratics = 1,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ReduceOrder {
    pub pts: [DPoint; 4],
}

impl ReduceOrder {
    #[inline]
    pub fn as_line(&self) -> DLine {
        DLine {
            pts: [self.pts[0], self.pts[1]],
        }
    }

    #[inline]
    pub fn as_quad(&self) -> DQuad {
        DQuad {
            pts: [self.pts[0], self.pts[1], self.pts[2]],
        }
    }

    #[inline]
    pub fn as_cubic(&self) -> DCubic {
        DCubic { pts: self.pts }
    }

    pub fn reduce_line(&mut self, line: &DLine) -> usize {
        self.pts[0] = line.pts[0];
        let different = usize::from(line.pts[0] != line.pts[1]);
        self.pts[1] = line.pts[different];
        1 + different
    }

    pub fn reduce_quad(&mut self, quad: &DQuad) -> usize {
        let mut min_x = 0usize;
        let mut max_x = 0usize;
        let mut min_y = 0usize;
        let mut max_y = 0usize;
        let mut min_x_set = 0u32;
        let mut min_y_set = 0u32;
        for index in 1..3 {
            if quad.pts[min_x].x > quad.pts[index].x {
                min_x = index;
            }
            if quad.pts[min_y].y > quad.pts[index].y {
                min_y = index;
            }
            if quad.pts[max_x].x < quad.pts[index].x {
                max_x = index;
            }
            if quad.pts[max_y].y < quad.pts[index].y {
                max_y = index;
            }
        }
        for index in 0..3 {
            if almost_equal_ulps(quad.pts[index].x, quad.pts[min_x].x) {
                min_x_set |= 1 << index;
            }
            if almost_equal_ulps(quad.pts[index].y, quad.pts[min_y].y) {
                min_y_set |= 1 << index;
            }
        }
        if (min_x_set & 0x05) == 0x5 && (min_y_set & 0x05) == 0x5 {
            self.pts[0] = quad.pts[0];
            self.pts[1] = quad.pts[0];
            return 1;
        }
        if min_x_set == 0x7 || min_y_set == 0x7 || quad.is_linear(0, 2) {
            self.pts[0] = quad.pts[0];
            self.pts[1] = quad.pts[2];
            return 1 + usize::from(!self.pts[0].approximately_equal(self.pts[1]));
        }
        self.pts[0] = quad.pts[0];
        self.pts[1] = quad.pts[1];
        self.pts[2] = quad.pts[2];
        3
    }

    fn check_cubic_quadratic(&mut self, cubic: &DCubic) -> usize {
        let dx10 = cubic.pts[1].x - cubic.pts[0].x;
        let dx23 = cubic.pts[2].x - cubic.pts[3].x;
        let mid_x = cubic.pts[0].x + dx10 * 3.0 / 2.0;
        let side_ax = mid_x - cubic.pts[3].x;
        let side_bx = dx23 * 3.0 / 2.0;
        if if approximately_zero(side_ax) {
            !approximately_equal(side_ax, side_bx)
        } else {
            !almost_equal_ulps_pin(side_ax, side_bx)
        } {
            return 0;
        }
        let dy10 = cubic.pts[1].y - cubic.pts[0].y;
        let dy23 = cubic.pts[2].y - cubic.pts[3].y;
        let mid_y = cubic.pts[0].y + dy10 * 3.0 / 2.0;
        let side_ay = mid_y - cubic.pts[3].y;
        let side_by = dy23 * 3.0 / 2.0;
        if if approximately_zero(side_ay) {
            !approximately_equal(side_ay, side_by)
        } else {
            !almost_equal_ulps_pin(side_ay, side_by)
        } {
            return 0;
        }
        self.pts[0] = cubic.pts[0];
        self.pts[1] = DPoint::new(mid_x, mid_y);
        self.pts[2] = cubic.pts[3];
        3
    }

    pub fn reduce_cubic(&mut self, cubic: &DCubic, allow_quadratics: ReduceQuadratics) -> usize {
        let mut min_x = 0usize;
        let mut max_x = 0usize;
        let mut min_y = 0usize;
        let mut max_y = 0usize;
        let mut min_x_set = 0u32;
        let mut min_y_set = 0u32;
        for index in 1..4 {
            if cubic.pts[min_x].x > cubic.pts[index].x {
                min_x = index;
            }
            if cubic.pts[min_y].y > cubic.pts[index].y {
                min_y = index;
            }
            if cubic.pts[max_x].x < cubic.pts[index].x {
                max_x = index;
            }
            if cubic.pts[max_y].y < cubic.pts[index].y {
                max_y = index;
            }
        }
        for index in 0..4 {
            let cx = cubic.pts[index].x;
            let cy = cubic.pts[index].y;
            let denom = cpp_max_f64(
                cx.abs(),
                cpp_max_f64(
                    cy.abs(),
                    cpp_max_f64(cubic.pts[min_x].x.abs(), cubic.pts[min_y].y.abs()),
                ),
            );
            if denom == 0.0 {
                min_x_set |= 1 << index;
                min_y_set |= 1 << index;
                continue;
            }
            let inv = 1.0 / denom;
            if approximately_equal_half(cx * inv, cubic.pts[min_x].x * inv) {
                min_x_set |= 1 << index;
            }
            if approximately_equal_half(cy * inv, cubic.pts[min_y].y * inv) {
                min_y_set |= 1 << index;
            }
        }
        if min_x_set == 0xF {
            if min_y_set == 0xF {
                self.pts[0] = cubic.pts[0];
                self.pts[1] = cubic.pts[0];
                return 1;
            }
            self.pts[0] = cubic.pts[0];
            self.pts[1] = cubic.pts[3];
            return 1 + usize::from(!self.pts[0].approximately_equal(self.pts[1]));
        }
        if min_y_set == 0xF || cubic.is_linear(0, 3) {
            self.pts[0] = cubic.pts[0];
            self.pts[1] = cubic.pts[3];
            return 1 + usize::from(!self.pts[0].approximately_equal(self.pts[1]));
        }
        if allow_quadratics == ReduceQuadratics::AllowQuadratics {
            let res = self.check_cubic_quadratic(cubic);
            if res != 0 {
                return res;
            }
        }
        self.pts = cubic.pts;
        4
    }

    pub fn quad_pts(a: [Point; 3], reduce_pts: &mut [Point]) -> Verb {
        let quad = DQuad::from_points(a);
        let mut reducer = Self::default();
        let order = reducer.reduce_quad(&quad);
        if order == 2 {
            reduce_pts[0] = reducer.pts[0].as_point();
            reduce_pts[1] = reducer.pts[1].as_point();
        }
        pathops_points_to_verb(order - 1)
    }

    pub fn conic_pts(pts: [Point; 3], w: f32, reduce_pts: &mut [Point]) -> Verb {
        let verb = Self::quad_pts(pts, reduce_pts);
        if (verb as u8) > (Verb::Line as u8) && w == 1.0 {
            return Verb::Quad;
        }
        if verb == Verb::Quad {
            Verb::Conic
        } else {
            verb
        }
    }

    pub fn cubic_pts(a: [Point; 4], reduce_pts: &mut [Point]) -> Verb {
        if DPoint::approximately_equal_pts(a[0], a[1])
            && DPoint::approximately_equal_pts(a[0], a[2])
            && DPoint::approximately_equal_pts(a[0], a[3])
        {
            reduce_pts[0] = a[0];
            return Verb::Move;
        }
        let cubic = DCubic::from_points(a);
        let mut reducer = Self::default();
        let order = reducer.reduce_cubic(&cubic, ReduceQuadratics::AllowQuadratics);
        if order == 2 || order == 3 {
            for index in 0..order {
                reduce_pts[index] = reducer.pts[index].as_point();
            }
        }
        pathops_points_to_verb(order - 1)
    }
}
