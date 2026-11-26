#![forbid(unsafe_code)]

//! Port of `SkPathOpsWinding.cpp`.

use skia_rs_geometry::Point;
use skia_rs_path::types::Verb;

use crate::arena::{
    ContourId, OpPhase, OpState, SegmentId, SpanId, MAX_WINDING_TRIES, SK_MIN_S32,
};
use crate::curve::{curve_intercept_h, curve_intercept_v};
use crate::types::{
    approximately_between, approximately_equal, approximately_zero, pathops_verb_to_points,
    roughly_equal, DPoint, DVector, PathOpsBounds,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
enum RayDir {
    Left = 0,
    Top = 1,
    Right = 2,
    Bottom = 3,
}

impl RayDir {
    #[inline]
    fn from_u8(v: u8) -> Self {
        match v {
            0 => RayDir::Left,
            1 => RayDir::Top,
            2 => RayDir::Right,
            _ => RayDir::Bottom,
        }
    }

    #[inline]
    fn xy_index(self) -> usize {
        (self as usize) & 1
    }

    #[inline]
    fn less_than(self) -> bool {
        ((self as u8) & 2) == 0
    }
}

#[inline]
fn pt_xy(pt: Point, dir: RayDir) -> f32 {
    if dir.xy_index() == 0 {
        pt.x
    } else {
        pt.y
    }
}

#[inline]
fn pt_yx(pt: Point, dir: RayDir) -> f32 {
    if dir.xy_index() == 0 {
        pt.y
    } else {
        pt.x
    }
}

#[inline]
fn pt_dxdy(v: DVector, dir: RayDir) -> f64 {
    if dir.xy_index() == 0 {
        v.x
    } else {
        v.y
    }
}

#[inline]
fn pt_dydx(v: DVector, dir: RayDir) -> f64 {
    if dir.xy_index() == 0 {
        v.y
    } else {
        v.x
    }
}

#[inline]
fn rect_side(b: &PathOpsBounds, dir: RayDir) -> f32 {
    match dir {
        RayDir::Left => b.left,
        RayDir::Top => b.top,
        RayDir::Right => b.right,
        RayDir::Bottom => b.bottom,
    }
}

#[inline]
fn sideways_overlap(b: &PathOpsBounds, pt: Point, dir: RayDir) -> bool {
    if dir.xy_index() == 0 {
        approximately_between(b.top as f64, pt.y as f64, b.bottom as f64)
    } else {
        approximately_between(b.left as f64, pt.x as f64, b.right as f64)
    }
}

#[inline]
fn ccw_dxdy(v: DVector, dir: RayDir) -> bool {
    let v_part_pos = pt_dydx(v, dir) > 0.0;
    let left_bottom = (((dir as u8) + 1) & 2) != 0;
    v_part_pos == left_bottom
}

#[derive(Copy, Clone, Debug)]
struct RayHit {
    span: Option<SpanId>,
    pt: Point,
    t: f64,
    slope: DVector,
    valid: bool,
}

fn get_t_guess(mut t_try: i32, dir_offset: &mut u8) -> f64 {
    let mut t = 0.5;
    *dir_offset = (t_try & 1) as u8;
    let t_base = t_try >> 1;
    let mut t_bits = 0;
    loop {
        t_try >>= 1;
        if t_try == 0 {
            break;
        }
        t /= 2.0;
        t_bits += 1;
    }
    if t_bits != 0 {
        let t_index = (t_base - 1) & ((1 << t_bits) - 1);
        t += t * 2.0 * (t_index as f64);
    }
    t
}

impl OpState {
    fn contour_ray_check(
        &self,
        contour: ContourId,
        base: &RayHit,
        dir: RayDir,
        hits: &mut Vec<RayHit>,
    ) {
        let base_xy = pt_xy(base.pt, dir) as f64;
        let bounds_xy = rect_side(&self.contours[contour].bounds, dir) as f64;
        let check_less_than = dir.less_than();
        if !approximately_equal(base_xy, bounds_xy) && (base_xy < bounds_xy) == check_less_than {
            return;
        }
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            self.seg_ray_check(seg, base, dir, hits);
            seg_opt = self.segments[seg].next;
        }
    }

    fn seg_ray_check(&self, seg: SegmentId, base: &RayHit, dir: RayDir, hits: &mut Vec<RayHit>) {
        let s = &self.segments[seg];
        if !sideways_overlap(&s.bounds, base.pt, dir) {
            return;
        }
        let base_xy = pt_xy(base.pt, dir) as f64;
        let bounds_xy = rect_side(&s.bounds, dir) as f64;
        let check_less_than = dir.less_than();
        if !approximately_equal(base_xy, bounds_xy) && (base_xy < bounds_xy) == check_less_than {
            return;
        }
        let mut t_vals = [0.0f64; 3];
        let base_yx = pt_yx(base.pt, dir);
        let roots = if dir.xy_index() == 0 {
            curve_intercept_h(&s.pts, s.weight, s.verb, base_yx, &mut t_vals)
        } else {
            curve_intercept_v(&s.pts, s.weight, s.verb, base_yx, &mut t_vals)
        };
        let base_seg = base.span.map(|sp| self.spans[sp].segment);
        for &t in t_vals.iter().take(roots) {
            if base_seg == Some(seg) && approximately_equal(base.t, t) {
                continue;
            }
            let mut slope = DVector::new(0.0, 0.0);
            let mut valid = false;
            let pt = if approximately_zero(t) {
                s.pts[0]
            } else if approximately_equal(t, 1.0) {
                s.pts[pathops_verb_to_points(s.verb)]
            } else {
                let p = self.seg_pt_at_t(seg, t);
                if DPoint::approximately_equal_pts(p, base.pt) {
                    if base_seg == Some(seg) {
                        continue;
                    }
                } else {
                    let pt_xy_val = pt_xy(p, dir) as f64;
                    if !approximately_equal(base_xy, pt_xy_val)
                        && (base_xy < pt_xy_val) == check_less_than
                    {
                        continue;
                    }
                    slope = self.seg_d_slope_at_t(seg, t);
                    if s.verb == Verb::Cubic
                        && base_seg == Some(seg)
                        && roughly_equal(base.t, t)
                        && DPoint::roughly_equal_pts(p, base.pt)
                    {
                        continue;
                    }
                    if (pt_dydx(slope, dir) * 10000.0).abs() > pt_dxdy(slope, dir).abs() {
                        valid = true;
                    }
                }
                p
            };
            let span = self.seg_winding_span_at_t(seg, t);
            match span {
                None => valid = false,
                Some(sp) => {
                    if self.spans[sp].wind_value == 0 && self.spans[sp].opp_value == 0 {
                        continue;
                    }
                }
            }
            hits.push(RayHit {
                span,
                pt,
                t,
                slope,
                valid,
            });
        }
    }

    pub fn seg_winding_span_at_t(&self, seg: SegmentId, t_hit: f64) -> Option<SpanId> {
        let mut span = self.segments[seg].head;
        loop {
            let next = self.spans[span].next?;
            let next_t = self.span_t(next);
            if approximately_equal(t_hit, next_t) {
                return None;
            }
            if t_hit < next_t {
                return Some(span);
            }
            if self.span_final(next) {
                break;
            }
            span = next;
        }
        None
    }

    pub fn span_sortable_top(&mut self, span: SpanId, contour_head: ContourId) -> bool {
        let t_try = self.spans[span].top_t_try;
        self.spans[span].top_t_try += 1;
        let mut dir_offset = 0u8;
        let t = get_t_guess(t_try, &mut dir_offset);

        let next_span = match self.spans[span].next {
            Some(n) => n,
            None => return false,
        };
        let hit_t = self.span_t(span) * (1.0 - t) + self.span_t(next_span) * t;
        let segment = self.spans[span].segment;
        let hit_slope = self.seg_d_slope_at_t(segment, hit_t);
        if hit_slope.x == 0.0 && hit_slope.y == 0.0 {
            return false;
        }
        let hit_pt = self.seg_pt_at_t(segment, hit_t);
        let hit_base = RayHit {
            span: Some(span),
            pt: hit_pt,
            t: hit_t,
            slope: hit_slope,
            valid: true,
        };
        let base_dir = if hit_slope.x.abs() < hit_slope.y.abs() {
            RayDir::Left
        } else {
            RayDir::Top
        };
        let dir = RayDir::from_u8((base_dir as u8) + dir_offset);
        if (self.segments[segment].verb as u8) > (Verb::Line as u8)
            && pt_dydx(hit_slope, dir) == 0.0
        {
            return false;
        }

        let mut hits = Vec::with_capacity(8);
        hits.push(hit_base);
        let mut contour_opt = Some(contour_head);
        while let Some(contour) = contour_opt {
            if self.contours[contour].count > 0 {
                self.contour_ray_check(contour, &hit_base, dir, &mut hits);
            }
            contour_opt = self.contours[contour].next;
        }

        // Sort hits
        hits.sort_by(|a, b| {
            let ord = if dir.xy_index() != 0 {
                if dir.less_than() {
                    a.pt.y.partial_cmp(&b.pt.y)
                } else {
                    b.pt.y.partial_cmp(&a.pt.y)
                }
            } else if dir.less_than() {
                a.pt.x.partial_cmp(&b.pt.x)
            } else {
                b.pt.x.partial_cmp(&a.pt.x)
            };
            ord.unwrap_or(core::cmp::Ordering::Equal)
        });

        let count = hits.len();
        let mut last: Option<Point> = None;
        let mut wind = 0i32;
        let mut opp_wind = 0i32;
        for index in 0..count {
            let hit = hits[index];
            if !hit.valid {
                return false;
            }
            let ccw = ccw_dxdy(hit.slope, dir);
            let h_span = match hit.span {
                Some(sp) => sp,
                None => return false,
            };
            let hit_segment = self.spans[h_span].segment;
            if self.spans[h_span].wind_value == 0 && self.spans[h_span].opp_value == 0 {
                continue;
            }
            if let Some(lp) = last {
                if DPoint::approximately_equal_pts(lp, hit.pt) {
                    return false;
                }
            }
            if index < count - 1 && DPoint::approximately_equal_pts(hits[index + 1].pt, hit.pt) {
                return false;
            }
            let operand = self.seg_operand(hit_segment);
            if operand {
                core::mem::swap(&mut wind, &mut opp_wind);
            }
            let last_wind = wind;
            let last_opp = opp_wind;
            let wind_value = if ccw {
                -self.spans[h_span].wind_value
            } else {
                self.spans[h_span].wind_value
            };
            let opp_value = if ccw {
                -self.spans[h_span].opp_value
            } else {
                self.spans[h_span].opp_value
            };
            wind += wind_value;
            opp_wind += opp_value;
            let mut sum_set = false;
            let span_sum = self.spans[h_span].wind_sum;
            let wind_sum = if Self::seg_use_inner_winding(last_wind, wind) {
                wind
            } else {
                last_wind
            };
            if span_sum == SK_MIN_S32 {
                self.span_set_wind_sum(h_span, wind_sum);
                sum_set = true;
            }
            let o_span_sum = self.spans[h_span].opp_sum;
            let opp_sum = if Self::seg_use_inner_winding(last_opp, opp_wind) {
                opp_wind
            } else {
                last_opp
            };
            if o_span_sum == SK_MIN_S32 {
                self.span_set_opp_sum(h_span, opp_sum);
            }
            if sum_set {
                if self.phase == OpPhase::FixWinding {
                    let c = self.segments[hit_segment].contour;
                    self.contours[c].ccw = i32::from(ccw);
                } else {
                    let h_next = self.spans[h_span].next.unwrap();
                    let _ = self.seg_mark_and_chase_winding_opp(
                        hit_segment,
                        h_span,
                        h_next,
                        wind_sum,
                        opp_sum,
                        None,
                    );
                    let _ = self.seg_mark_and_chase_winding_opp(
                        hit_segment,
                        h_next,
                        h_span,
                        wind_sum,
                        opp_sum,
                        None,
                    );
                }
            }
            if operand {
                core::mem::swap(&mut wind, &mut opp_wind);
            }
            last = Some(hit.pt);
            self.nested += 1;
        }
        true
    }

    pub fn seg_find_sortable_top(
        &mut self,
        seg: SegmentId,
        contour_head: ContourId,
    ) -> Option<SpanId> {
        let mut span = self.segments[seg].head;
        loop {
            let next = self.spans[span].next?;
            if !self.spans[span].done {
                if self.spans[span].wind_sum != SK_MIN_S32 {
                    return Some(span);
                }
                if self.span_sortable_top(span, contour_head) {
                    return Some(span);
                }
            }
            if self.span_final(next) {
                break;
            }
            span = next;
        }
        None
    }

    pub fn contour_find_sortable_top(
        &mut self,
        contour: ContourId,
        contour_head: ContourId,
    ) -> Option<SpanId> {
        let mut all_done = true;
        if self.contours[contour].count > 0 {
            let mut seg_opt = self.contours[contour].head;
            while let Some(seg) = seg_opt {
                if !self.seg_done(seg) {
                    all_done = false;
                    if let Some(res) = self.seg_find_sortable_top(seg, contour_head) {
                        return Some(res);
                    }
                }
                seg_opt = self.segments[seg].next;
            }
        }
        if all_done {
            self.contours[contour].done = true;
        }
        None
    }

    pub fn find_sortable_top(&mut self, contour_head: ContourId) -> Option<SpanId> {
        for _ in 0..MAX_WINDING_TRIES {
            let mut contour_opt = Some(contour_head);
            while let Some(contour) = contour_opt {
                if !self.contours[contour].done {
                    if let Some(res) = self.contour_find_sortable_top(contour, contour_head) {
                        return Some(res);
                    }
                }
                contour_opt = self.contours[contour].next;
            }
        }
        None
    }
}
