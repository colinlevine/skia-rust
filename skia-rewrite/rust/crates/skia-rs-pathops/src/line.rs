#![forbid(unsafe_code)]

//! Port of `SkPathOpsLine.h/.cpp` and `SkLineParameters.h`.

use skia_rs_geometry::Point;

use crate::cubic::DCubic;
use crate::quad::DQuad;
use crate::types::{
    almost_bequal_ulps, almost_between_ulps, almost_equal_ulps_pin, approximately_zero, between,
    cpp_max_f64, cpp_min_f64, not_almost_equal_ulps, pin_t, DPoint, DBL_EPSILON,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DLine {
    pub pts: [DPoint; 2],
}

#[inline]
fn interpolated_largest(p0: DPoint, p1: DPoint, t: f64) -> f64 {
    const SEGMENT_EXTENT_FLOOR_SCALE: f64 = 0.001;
    let largest0 = cpp_max_f64(p0.x.abs(), p0.y.abs());
    let largest1 = cpp_max_f64(p1.x.abs(), p1.y.abs());
    let seg_extent = cpp_max_f64((p1.x - p0.x).abs(), (p1.y - p0.y).abs());
    let min_floor = seg_extent * SEGMENT_EXTENT_FLOOR_SCALE;
    cpp_max_f64((1.0 - t) * largest0 + t * largest1, min_floor)
}

impl DLine {
    #[inline]
    pub const fn new(p0: DPoint, p1: DPoint) -> Self {
        Self { pts: [p0, p1] }
    }

    #[inline]
    pub fn from_points(pts: [Point; 2]) -> Self {
        Self {
            pts: [DPoint::from_point(pts[0]), DPoint::from_point(pts[1])],
        }
    }

    #[inline]
    pub fn pt_at_t(&self, t: f64) -> DPoint {
        if t == 0.0 {
            return self.pts[0];
        }
        if t == 1.0 {
            return self.pts[1];
        }
        let one_t = 1.0 - t;
        DPoint {
            x: one_t * self.pts[0].x + t * self.pts[1].x,
            y: one_t * self.pts[0].y + t * self.pts[1].y,
        }
    }

    #[inline]
    pub fn exact_point(&self, xy: DPoint) -> f64 {
        if xy == self.pts[0] {
            return 0.0;
        }
        if xy == self.pts[1] {
            return 1.0;
        }
        -1.0
    }

    pub fn near_point(&self, xy: DPoint, unequal: Option<&mut bool>) -> f64 {
        if self.pts[0].x == self.pts[1].x && !almost_bequal_ulps(xy.x, self.pts[0].x) {
            return -1.0;
        }
        if self.pts[0].y == self.pts[1].y && !almost_bequal_ulps(xy.y, self.pts[0].y) {
            return -1.0;
        }
        let len = self.pts[1].sub(self.pts[0]);
        let denom = len.x * len.x + len.y * len.y;
        let ab0 = xy.sub(self.pts[0]);
        let numer = len.x * ab0.x + ab0.y * len.y;
        if !between(0.0, numer, denom) {
            return -1.0;
        }
        let t = if denom != 0.0 { numer / denom } else { 0.0 };
        let real_pt = self.pt_at_t(t);
        let dist = real_pt.distance(xy);
        let largest = interpolated_largest(self.pts[0], self.pts[1], t);
        if !almost_equal_ulps_pin(largest, largest + dist) {
            return -1.0;
        }
        if let Some(u) = unequal {
            *u = (largest as f32) != ((largest + dist) as f32);
        }
        pin_t(t)
    }

    pub fn near_ray(&self, xy: DPoint) -> bool {
        let len = self.pts[1].sub(self.pts[0]);
        let denom = len.x * len.x + len.y * len.y;
        let ab0 = xy.sub(self.pts[0]);
        let numer = len.x * ab0.x + ab0.y * len.y;
        let t = numer / denom;
        let real_pt = self.pt_at_t(t);
        let dist = real_pt.distance(xy);
        let tiniest = cpp_min_f64(
            cpp_min_f64(cpp_min_f64(self.pts[0].x, self.pts[0].y), self.pts[1].x),
            self.pts[1].y,
        );
        let mut largest = cpp_max_f64(
            cpp_max_f64(cpp_max_f64(self.pts[0].x, self.pts[0].y), self.pts[1].x),
            self.pts[1].y,
        );
        largest = cpp_max_f64(largest, -tiniest);
        crate::types::roughly_equal_ulps(largest, largest + dist)
    }

    #[inline]
    pub fn exact_point_h(xy: DPoint, left: f64, right: f64, y: f64) -> f64 {
        if xy.y == y {
            if xy.x == left {
                return 0.0;
            }
            if xy.x == right {
                return 1.0;
            }
        }
        -1.0
    }

    pub fn near_point_h(xy: DPoint, left: f64, right: f64, y: f64) -> f64 {
        if !almost_bequal_ulps(xy.y, y) {
            return -1.0;
        }
        if !almost_between_ulps(left, xy.x, right) {
            return -1.0;
        }
        let t = pin_t((xy.x - left) / (right - left));
        let real_pt_x = (1.0 - t) * left + t * right;
        let dx = xy.y - y;
        let dy = xy.x - real_pt_x;
        let dist = (dx * dx + dy * dy).sqrt();
        let largest = interpolated_largest(DPoint::new(left, y), DPoint::new(right, y), t);
        if !almost_equal_ulps_pin(largest, largest + dist) {
            return -1.0;
        }
        t
    }

    #[inline]
    pub fn exact_point_v(xy: DPoint, top: f64, bottom: f64, x: f64) -> f64 {
        if xy.x == x {
            if xy.y == top {
                return 0.0;
            }
            if xy.y == bottom {
                return 1.0;
            }
        }
        -1.0
    }

    pub fn near_point_v(xy: DPoint, top: f64, bottom: f64, x: f64) -> f64 {
        if !almost_bequal_ulps(xy.x, x) {
            return -1.0;
        }
        if !almost_between_ulps(top, xy.y, bottom) {
            return -1.0;
        }
        let t = pin_t((xy.y - top) / (bottom - top));
        let real_pt_y = (1.0 - t) * top + t * bottom;
        let dx = xy.x - x;
        let dy = xy.y - real_pt_y;
        let dist = (dx * dx + dy * dy).sqrt();
        let largest = interpolated_largest(DPoint::new(x, top), DPoint::new(x, bottom), t);
        if !almost_equal_ulps_pin(largest, largest + dist) {
            return -1.0;
        }
        t
    }
}

/// Mirrors `SkLineParameters` from `SkLineParameters.h`.
#[derive(Copy, Clone, Debug, Default)]
pub struct LineParameters {
    a: f64,
    b: f64,
    c: f64,
}

impl LineParameters {
    pub fn cubic_end_points_all(&mut self, pts: &DCubic) -> bool {
        let mut end_index = 1usize;
        self.cubic_end_points(pts, 0, end_index);
        if self.dy() != 0.0 {
            return true;
        }
        if self.dx() == 0.0 {
            end_index += 1;
            self.cubic_end_points(pts, 0, end_index);
            if self.dy() != 0.0 {
                return true;
            }
            if self.dx() == 0.0 {
                end_index += 1;
                self.cubic_end_points(pts, 0, end_index);
                return false;
            }
        }
        if self.dx() < 0.0 {
            return true;
        }
        end_index += 1;
        if not_almost_equal_ulps(pts.pts[0].y, pts.pts[end_index].y) {
            if pts.pts[0].y > pts.pts[end_index].y {
                self.a = DBL_EPSILON;
            }
            return true;
        }
        if end_index == 3 {
            return true;
        }
        if pts.pts[0].y > pts.pts[3].y {
            self.a = DBL_EPSILON;
        }
        true
    }

    #[inline]
    pub fn cubic_end_points(&mut self, pts: &DCubic, s: usize, e: usize) {
        self.a = pts.pts[s].y - pts.pts[e].y;
        self.b = pts.pts[e].x - pts.pts[s].x;
        self.c = pts.pts[s].x * pts.pts[e].y - pts.pts[e].x * pts.pts[s].y;
    }

    pub fn cubic_part(&mut self, part: &DCubic) -> f64 {
        self.cubic_end_points_all(part);
        let line01 = DLine::new(part.pts[0], part.pts[1]);
        if part.pts[0] == part.pts[1] || line01.near_ray(part.pts[2]) {
            return self.point_distance(part.pts[3]);
        }
        self.point_distance(part.pts[2])
    }

    #[inline]
    pub fn line_end_points(&mut self, pts: &DLine) {
        self.a = pts.pts[0].y - pts.pts[1].y;
        self.b = pts.pts[1].x - pts.pts[0].x;
        self.c = pts.pts[0].x * pts.pts[1].y - pts.pts[1].x * pts.pts[0].y;
    }

    pub fn quad_end_points_all(&mut self, pts: &DQuad) -> bool {
        self.quad_end_points(pts, 0, 1);
        if self.dy() != 0.0 {
            return true;
        }
        if self.dx() == 0.0 {
            self.quad_end_points(pts, 0, 2);
            return false;
        }
        if self.dx() < 0.0 {
            return true;
        }
        if pts.pts[0].y > pts.pts[2].y {
            self.a = DBL_EPSILON;
        }
        true
    }

    #[inline]
    pub fn quad_end_points(&mut self, pts: &DQuad, s: usize, e: usize) {
        self.a = pts.pts[s].y - pts.pts[e].y;
        self.b = pts.pts[e].x - pts.pts[s].x;
        self.c = pts.pts[s].x * pts.pts[e].y - pts.pts[e].x * pts.pts[s].y;
    }

    pub fn quad_part(&mut self, part: &DQuad) -> f64 {
        self.quad_end_points_all(part);
        self.point_distance(part.pts[2])
    }

    #[inline]
    pub fn normal_squared(&self) -> f64 {
        self.a * self.a + self.b * self.b
    }

    pub fn normalize(&mut self) -> bool {
        let normal = self.normal_squared().sqrt();
        if approximately_zero(normal) {
            self.a = 0.0;
            self.b = 0.0;
            self.c = 0.0;
            return false;
        }
        let reciprocal = 1.0 / normal;
        self.a *= reciprocal;
        self.b *= reciprocal;
        self.c *= reciprocal;
        true
    }

    #[inline]
    pub fn control_pt_distance_cubic(&self, pts: &DCubic, index: usize) -> f64 {
        self.a * pts.pts[index].x + self.b * pts.pts[index].y + self.c
    }

    #[inline]
    pub fn control_pt_distance_quad(&self, pts: &DQuad) -> f64 {
        self.a * pts.pts[1].x + self.b * pts.pts[1].y + self.c
    }

    #[inline]
    pub fn point_distance(&self, pt: DPoint) -> f64 {
        self.a * pt.x + self.b * pt.y + self.c
    }

    #[inline]
    pub fn dx(&self) -> f64 {
        self.b
    }

    #[inline]
    pub fn dy(&self) -> f64 {
        -self.a
    }
}
