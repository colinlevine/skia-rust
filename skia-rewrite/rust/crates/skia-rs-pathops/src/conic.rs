#![forbid(unsafe_code)]

//! Port of `SkPathOpsConic.h/.cpp`.

use skia_rs_geometry::Point;

use crate::cubic::DCubic;
use crate::quad::DQuad;
use crate::types::{zero_or_one, DPoint, DRect, DVector};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DConic {
    pub pts: DQuad,
    pub weight: f32,
}

#[inline]
fn conic_deriv_coeff(s0: f64, s1: f64, s2: f64, w: f32) -> [f64; 3] {
    let w = w as f64;
    let p20 = s2 - s0;
    let p10 = s1 - s0;
    let wp10 = w * p10;
    [w * p20 - p20, p20 - 2.0 * wp10, wp10]
}

#[inline]
fn conic_eval_tan(s0: f64, s1: f64, s2: f64, w: f32, t: f64) -> f64 {
    let coeff = conic_deriv_coeff(s0, s1, s2, w);
    t * (t * coeff[0] + coeff[1]) + coeff[2]
}

#[inline]
fn conic_eval_numerator(s0: f64, s1: f64, s2: f64, w: f32, t: f64) -> f64 {
    let src2w = s1 * (w as f64);
    let c = s0;
    let a = s2 - 2.0 * src2w + c;
    let b = 2.0 * (src2w - c);
    (a * t + b) * t + c
}

#[inline]
fn conic_eval_denominator(w: f32, t: f64) -> f64 {
    let b = 2.0 * ((w as f64) - 1.0);
    let c = 1.0;
    let a = -b;
    (a * t + b) * t + c
}

impl DConic {
    pub const POINT_COUNT: usize = 3;
    pub const POINT_LAST: usize = 2;
    pub const MAX_INTERSECTIONS: usize = 4;

    #[inline]
    pub const fn new(p0: DPoint, p1: DPoint, p2: DPoint, weight: f32) -> Self {
        Self {
            pts: DQuad::new(p0, p1, p2),
            weight,
        }
    }

    #[inline]
    pub fn from_points(pts: [Point; 3], weight: f32) -> Self {
        Self {
            pts: DQuad::from_points(pts),
            weight,
        }
    }

    #[inline]
    pub fn collapsed(&self) -> bool {
        self.pts.collapsed()
    }

    #[inline]
    pub fn controls_inside(&self) -> bool {
        self.pts.controls_inside()
    }

    #[inline]
    pub fn flip(&self) -> Self {
        Self {
            pts: self.pts.flip(),
            weight: self.weight,
        }
    }

    #[inline]
    pub fn align(&self, end_index: usize, dst_pt: &mut DPoint) {
        self.pts.align(end_index, dst_pt);
    }

    #[inline]
    pub fn is_linear(&self, start_index: usize, end_index: usize) -> bool {
        self.pts.is_linear(start_index, end_index)
    }

    #[inline]
    pub fn monotonic_in_x(&self) -> bool {
        self.pts.monotonic_in_x()
    }

    #[inline]
    pub fn monotonic_in_y(&self) -> bool {
        self.pts.monotonic_in_y()
    }

    #[inline]
    pub fn other_pts(&self, odd_man: usize) -> [DPoint; 2] {
        self.pts.other_pts(odd_man)
    }

    pub fn find_extrema(s0: f64, s1: f64, s2: f64, w: f32, t: &mut [f64; 1]) -> usize {
        let coeff = conic_deriv_coeff(s0, s1, s2, w);
        let mut t_values = [0.0; 2];
        let roots = DQuad::roots_valid_t(coeff[0], coeff[1], coeff[2], &mut t_values);
        if roots == 1 {
            t[0] = t_values[0];
            return 1;
        }
        0
    }

    pub fn dxdy_at_t(&self, t: f64) -> DVector {
        let mut result = DVector {
            x: conic_eval_tan(
                self.pts.pts[0].x,
                self.pts.pts[1].x,
                self.pts.pts[2].x,
                self.weight,
                t,
            ),
            y: conic_eval_tan(
                self.pts.pts[0].y,
                self.pts.pts[1].y,
                self.pts.pts[2].y,
                self.weight,
                t,
            ),
        };
        if result.x == 0.0 && result.y == 0.0 && zero_or_one(t) {
            result = self.pts.pts[2].sub(self.pts.pts[0]);
        }
        result
    }

    #[inline]
    pub fn hull_intersects_quad(&self, quad: &DQuad, is_linear: &mut bool) -> bool {
        self.pts.hull_intersects_quad(quad, is_linear)
    }

    #[inline]
    pub fn hull_intersects_conic(&self, conic: &DConic, is_linear: &mut bool) -> bool {
        self.pts.hull_intersects_quad(&conic.pts, is_linear)
    }

    #[inline]
    pub fn hull_intersects_cubic(&self, cubic: &DCubic, is_linear: &mut bool) -> bool {
        cubic.hull_intersects_conic(self, is_linear)
    }

    pub fn pt_at_t(&self, t: f64) -> DPoint {
        if t == 0.0 {
            return self.pts.pts[0];
        }
        if t == 1.0 {
            return self.pts.pts[2];
        }
        let denom = conic_eval_denominator(self.weight, t);
        DPoint {
            x: conic_eval_numerator(
                self.pts.pts[0].x,
                self.pts.pts[1].x,
                self.pts.pts[2].x,
                self.weight,
                t,
            ) / denom,
            y: conic_eval_numerator(
                self.pts.pts[0].y,
                self.pts.pts[1].y,
                self.pts.pts[2].y,
                self.weight,
                t,
            ) / denom,
        }
    }

    pub fn sub_divide(&self, t1: f64, t2: f64) -> Self {
        let (ax, ay, az) = if t1 == 0.0 {
            (self.pts.pts[0].x, self.pts.pts[0].y, 1.0)
        } else if t1 != 1.0 {
            (
                conic_eval_numerator(
                    self.pts.pts[0].x,
                    self.pts.pts[1].x,
                    self.pts.pts[2].x,
                    self.weight,
                    t1,
                ),
                conic_eval_numerator(
                    self.pts.pts[0].y,
                    self.pts.pts[1].y,
                    self.pts.pts[2].y,
                    self.weight,
                    t1,
                ),
                conic_eval_denominator(self.weight, t1),
            )
        } else {
            (self.pts.pts[2].x, self.pts.pts[2].y, 1.0)
        };
        let mid_t = (t1 + t2) / 2.0;
        let dx = conic_eval_numerator(
            self.pts.pts[0].x,
            self.pts.pts[1].x,
            self.pts.pts[2].x,
            self.weight,
            mid_t,
        );
        let dy = conic_eval_numerator(
            self.pts.pts[0].y,
            self.pts.pts[1].y,
            self.pts.pts[2].y,
            self.weight,
            mid_t,
        );
        let dz = conic_eval_denominator(self.weight, mid_t);
        let (cx, cy, cz) = if t2 == 1.0 {
            (self.pts.pts[2].x, self.pts.pts[2].y, 1.0)
        } else if t2 != 0.0 {
            (
                conic_eval_numerator(
                    self.pts.pts[0].x,
                    self.pts.pts[1].x,
                    self.pts.pts[2].x,
                    self.weight,
                    t2,
                ),
                conic_eval_numerator(
                    self.pts.pts[0].y,
                    self.pts.pts[1].y,
                    self.pts.pts[2].y,
                    self.weight,
                    t2,
                ),
                conic_eval_denominator(self.weight, t2),
            )
        } else {
            (self.pts.pts[0].x, self.pts.pts[0].y, 1.0)
        };
        let bx = 2.0 * dx - (ax + cx) / 2.0;
        let by = 2.0 * dy - (ay + cy) / 2.0;
        let mut bz = 2.0 * dz - (az + cz) / 2.0;
        if bz == 0.0 {
            bz = 1.0;
        }
        DConic {
            pts: DQuad {
                pts: [
                    DPoint::new(ax / az, ay / az),
                    DPoint::new(bx / bz, by / bz),
                    DPoint::new(cx / cz, cy / cz),
                ],
            },
            weight: (bz / (az * cz).sqrt()) as f32,
        }
    }

    pub fn sub_divide_pt(&self, t1: f64, t2: f64) -> (DPoint, f32) {
        let chopped = self.sub_divide(t1, t2);
        (chopped.pts.pts[1], chopped.weight)
    }

    pub fn set_bounds(&self, sub: &DConic, start_t: f64, end_t: f64) -> DRect {
        let mut r = DRect::default();
        r.set_pt(sub.pts.pts[0]);
        r.add(sub.pts.pts[2]);
        let mut t_values = [0.0; 2];
        let mut roots = 0usize;
        if !sub.monotonic_in_x() {
            let mut tv = [0.0; 1];
            let n = Self::find_extrema(
                sub.pts.pts[0].x,
                sub.pts.pts[1].x,
                sub.pts.pts[2].x,
                sub.weight,
                &mut tv,
            );
            if n == 1 {
                t_values[roots] = tv[0];
                roots += 1;
            }
        }
        if !sub.monotonic_in_y() {
            let mut tv = [0.0; 1];
            let n = Self::find_extrema(
                sub.pts.pts[0].y,
                sub.pts.pts[1].y,
                sub.pts.pts[2].y,
                sub.weight,
                &mut tv,
            );
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
}
