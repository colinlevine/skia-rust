#![forbid(unsafe_code)]

//! Port of `SkOpSegment.h/.cpp`.

use skia_rs_geometry::Point;
use skia_rs_path::types::Verb;

use crate::arena::{
    AngleId, AngleIncludeType, Collapsed, OpState, PtTId, SegmentId, SpanId, SK_MAX_S32,
    SK_MIN_S32, SK_NAN_32,
};
use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::curve::{
    curve_dpoint_at_t, curve_dslope_at_t, curve_intersect_ray, curve_is_vertical,
    curve_point_at_t, DCurve, DCurveSweep,
};
use crate::intersections::Intersections;
use crate::line::DLine;
use crate::path_writer::PathWriter;
use crate::quad::DQuad;
use crate::types::{
    between, cpp_max_f64, pathops_verb_to_points, precisely_equal, roughly_equal, zero_or_one,
    DPoint, DVector, PathOp, FLT_EPSILON, SK_SCALAR_MAX,
};

const K_UNARY_ACTIVE_EDGE: [[bool; 2]; 2] = [[false, true], [true, false]];

// [op][miFrom][miTo][suFrom][suTo]
const K_ACTIVE_EDGE: [[[[[bool; 2]; 2]; 2]; 2]; 4] = [
    // kDifference: mi - su
    [
        [
            [[false, false], [false, false]],
            [[true, false], [true, false]],
        ],
        [
            [[true, true], [false, false]],
            [[false, true], [true, false]],
        ],
    ],
    // kIntersect: mi & su
    [
        [
            [[false, false], [false, false]],
            [[false, true], [false, true]],
        ],
        [
            [[false, false], [true, true]],
            [[false, true], [true, false]],
        ],
    ],
    // kUnion: mi | su
    [
        [
            [[false, true], [true, false]],
            [[true, true], [false, false]],
        ],
        [
            [[true, false], [true, false]],
            [[false, false], [false, false]],
        ],
    ],
    // kXOR: mi ^ su
    [
        [
            [[false, true], [true, false]],
            [[true, false], [false, true]],
        ],
        [
            [[true, false], [false, true]],
            [[false, true], [true, false]],
        ],
    ],
];

#[inline]
fn distance_to_sqd(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

impl OpState {
    // ========================================================================
    // Simple accessors / inline helpers from SkOpSegment.h
    // ========================================================================

    #[inline]
    pub fn seg_done(&self, seg: SegmentId) -> bool {
        self.segments[seg].done_count == self.segments[seg].count
    }

    #[inline]
    pub fn seg_angle_done(&self, angle: AngleId) -> bool {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        let starter = self.span_starter(start, end);
        self.spans[starter].done
    }

    #[inline]
    pub fn seg_d_pt_at_t(&self, seg: SegmentId, mid: f64) -> DPoint {
        let s = &self.segments[seg];
        curve_dpoint_at_t(&s.pts, s.weight, s.verb, mid)
    }

    #[inline]
    pub fn seg_pt_at_t(&self, seg: SegmentId, mid: f64) -> Point {
        let s = &self.segments[seg];
        curve_point_at_t(&s.pts, s.weight, s.verb, mid)
    }

    #[inline]
    pub fn seg_d_slope_at_t(&self, seg: SegmentId, mid: f64) -> DVector {
        let s = &self.segments[seg];
        curve_dslope_at_t(&s.pts, s.weight, s.verb, mid)
    }

    #[inline]
    pub fn seg_is_horizontal(&self, seg: SegmentId) -> bool {
        let b = &self.segments[seg].bounds;
        b.top == b.bottom
    }

    #[inline]
    pub fn seg_is_vertical(&self, seg: SegmentId) -> bool {
        let b = &self.segments[seg].bounds;
        b.left == b.right
    }

    #[inline]
    pub fn seg_is_vertical_span(&self, seg: SegmentId, start: SpanId, end: SpanId) -> bool {
        let s = &self.segments[seg];
        curve_is_vertical(&s.pts, s.weight, s.verb, self.span_t(start), self.span_t(end))
    }

    #[inline]
    pub fn seg_is_xor(&self, seg: SegmentId) -> bool {
        self.contours[self.segments[seg].contour].xor
    }

    #[inline]
    pub fn seg_operand(&self, seg: SegmentId) -> bool {
        self.contours[self.segments[seg].contour].operand
    }

    #[inline]
    pub fn seg_opp_xor(&self, seg: SegmentId) -> bool {
        self.contours[self.segments[seg].contour].opp_xor
    }

    #[inline]
    pub fn seg_visited(&mut self, seg: SegmentId) -> bool {
        if !self.segments[seg].visited {
            self.segments[seg].visited = true;
            false
        } else {
            true
        }
    }

    #[inline]
    pub fn seg_span_sign(&self, start: SpanId, end: SpanId) -> i32 {
        if self.span_t(start) < self.span_t(end) {
            -self.spans[start].wind_value
        } else {
            self.spans[end].wind_value
        }
    }

    #[inline]
    pub fn seg_opp_sign(&self, start: SpanId, end: SpanId) -> i32 {
        if self.span_t(start) < self.span_t(end) {
            -self.spans[start].opp_value
        } else {
            self.spans[end].opp_value
        }
    }

    #[inline]
    pub fn seg_span_to_angle(&self, start: SpanId, end: SpanId) -> Option<AngleId> {
        if self.span_t(start) < self.span_t(end) {
            self.spans[start].to_angle
        } else {
            self.spans[start].from_angle
        }
    }

    #[inline]
    pub fn seg_use_inner_winding(outer_winding: i32, inner_winding: i32) -> bool {
        let abs_out = outer_winding.abs();
        let abs_in = inner_winding.abs();
        if abs_out == abs_in {
            outer_winding < 0
        } else {
            abs_out < abs_in
        }
    }

    #[inline]
    pub fn seg_wind_sum(&self, angle: AngleId) -> i32 {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        let min_span = self.span_starter(start, end);
        self.spans[min_span].wind_sum
    }

    #[inline]
    pub fn seg_set_up_winding(
        &self,
        start: SpanId,
        end: SpanId,
        max_winding: &mut i32,
        sum_winding: &mut i32,
    ) {
        let delta_sum = self.seg_span_sign(start, end);
        *max_winding = *sum_winding;
        if *sum_winding == SK_MIN_S32 {
            return;
        }
        *sum_winding -= delta_sum;
    }

    pub fn seg_set_up_windings_unary(
        &self,
        start: SpanId,
        end: SpanId,
        sum_mi_winding: &mut i32,
        max_winding: &mut i32,
        sum_winding: &mut i32,
    ) {
        let delta_sum = self.seg_span_sign(start, end);
        *max_winding = *sum_mi_winding;
        *sum_mi_winding -= delta_sum;
        *sum_winding = *sum_mi_winding;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn seg_set_up_windings_binary(
        &self,
        seg: SegmentId,
        start: SpanId,
        end: SpanId,
        sum_mi_winding: &mut i32,
        sum_su_winding: &mut i32,
        max_winding: &mut i32,
        sum_winding: &mut i32,
        opp_max_winding: &mut i32,
        opp_sum_winding: &mut i32,
    ) {
        let delta_sum = self.seg_span_sign(start, end);
        let opp_delta_sum = self.seg_opp_sign(start, end);
        if self.seg_operand(seg) {
            *max_winding = *sum_su_winding;
            *sum_su_winding -= delta_sum;
            *sum_winding = *sum_su_winding;
            *opp_max_winding = *sum_mi_winding;
            *opp_sum_winding = {
                *sum_mi_winding -= opp_delta_sum;
                *sum_mi_winding
            };
        } else {
            *max_winding = *sum_mi_winding;
            *sum_mi_winding -= delta_sum;
            *sum_winding = *sum_mi_winding;
            *opp_max_winding = *sum_su_winding;
            *opp_sum_winding = {
                *sum_su_winding -= opp_delta_sum;
                *sum_su_winding
            };
        }
    }

    // ========================================================================
    // Active angle / op / winding
    // ========================================================================

    pub fn seg_active_angle(
        &self,
        seg: SegmentId,
        start: SpanId,
        start_ptr: &mut SpanId,
        end_ptr: &mut Option<SpanId>,
        done: &mut bool,
    ) -> Option<AngleId> {
        if let Some(res) = self.seg_active_angle_inner(seg, start, start_ptr, end_ptr, done) {
            return Some(res);
        }
        self.seg_active_angle_other(start, start_ptr, end_ptr, done)
    }

    pub fn seg_active_angle_inner(
        &self,
        _seg: SegmentId,
        start: SpanId,
        start_ptr: &mut SpanId,
        end_ptr: &mut Option<SpanId>,
        done: &mut bool,
    ) -> Option<AngleId> {
        if !self.span_final(start) {
            if self.spans[start].wind_value != 0 || self.spans[start].opp_value != 0 {
                let next = self.spans[start].next.unwrap();
                if end_ptr.is_none() {
                    *start_ptr = start;
                    *end_ptr = Some(next);
                }
                if !self.spans[start].done {
                    if self.spans[start].wind_sum != SK_MIN_S32 {
                        return self.seg_span_to_angle(start, next);
                    }
                    *done = false;
                }
            }
        }
        if let Some(down_span) = self.spans[start].prev {
            if self.spans[down_span].wind_value != 0 || self.spans[down_span].opp_value != 0 {
                if end_ptr.is_none() {
                    *start_ptr = start;
                    *end_ptr = Some(down_span);
                }
                if !self.spans[down_span].done {
                    if self.spans[down_span].wind_sum != SK_MIN_S32 {
                        return self.seg_span_to_angle(start, down_span);
                    }
                    *done = false;
                }
            }
        }
        None
    }

    pub fn seg_active_angle_other(
        &self,
        start: SpanId,
        start_ptr: &mut SpanId,
        end_ptr: &mut Option<SpanId>,
        done: &mut bool,
    ) -> Option<AngleId> {
        let o_pt_t = self.pt_ts[self.spans[start].pt_t].next;
        let other = self.pt_t_segment(o_pt_t);
        let o_span = self.pt_ts[o_pt_t].span;
        self.seg_active_angle_inner(other, o_span, start_ptr, end_ptr, done)
    }

    pub fn seg_active_op(
        &mut self,
        seg: SegmentId,
        start: SpanId,
        end: SpanId,
        xor_mi_mask: i32,
        xor_su_mask: i32,
        op: PathOp,
    ) -> bool {
        let mut sum_mi_winding = self.seg_update_winding(end, start);
        let mut sum_su_winding = self.seg_update_opp_winding(end, start);
        if self.seg_operand(seg) {
            core::mem::swap(&mut sum_mi_winding, &mut sum_su_winding);
        }
        self.seg_active_op_with_sums(
            seg,
            xor_mi_mask,
            xor_su_mask,
            start,
            end,
            op,
            &mut sum_mi_winding,
            &mut sum_su_winding,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn seg_active_op_with_sums(
        &self,
        seg: SegmentId,
        xor_mi_mask: i32,
        xor_su_mask: i32,
        start: SpanId,
        end: SpanId,
        op: PathOp,
        sum_mi_winding: &mut i32,
        sum_su_winding: &mut i32,
    ) -> bool {
        let mut max_winding = 0;
        let mut sum_winding = 0;
        let mut opp_max_winding = 0;
        let mut opp_sum_winding = 0;
        self.seg_set_up_windings_binary(
            seg,
            start,
            end,
            sum_mi_winding,
            sum_su_winding,
            &mut max_winding,
            &mut sum_winding,
            &mut opp_max_winding,
            &mut opp_sum_winding,
        );
        let (mi_from, mi_to, su_from, su_to) = if self.seg_operand(seg) {
            (
                (opp_max_winding & xor_mi_mask) != 0,
                (opp_sum_winding & xor_mi_mask) != 0,
                (max_winding & xor_su_mask) != 0,
                (sum_winding & xor_su_mask) != 0,
            )
        } else {
            (
                (max_winding & xor_mi_mask) != 0,
                (sum_winding & xor_mi_mask) != 0,
                (opp_max_winding & xor_su_mask) != 0,
                (opp_sum_winding & xor_su_mask) != 0,
            )
        };
        K_ACTIVE_EDGE[op as usize][usize::from(mi_from)][usize::from(mi_to)]
            [usize::from(su_from)][usize::from(su_to)]
    }

    pub fn seg_active_winding(&mut self, start: SpanId, end: SpanId) -> bool {
        let mut sum_winding = self.seg_update_winding(end, start);
        self.seg_active_winding_with_sum(start, end, &mut sum_winding)
    }

    pub fn seg_active_winding_with_sum(
        &self,
        start: SpanId,
        end: SpanId,
        sum_winding: &mut i32,
    ) -> bool {
        let mut max_winding = 0;
        self.seg_set_up_winding(start, end, &mut max_winding, sum_winding);
        let from = max_winding != 0;
        let to = *sum_winding != 0;
        K_UNARY_ACTIVE_EDGE[usize::from(from)][usize::from(to)]
    }

    // ========================================================================
    // Curve writing / subdivision
    // ========================================================================

    pub fn seg_add_curve_to(
        &mut self,
        seg: SegmentId,
        start: SpanId,
        end: SpanId,
        path: &mut PathWriter,
    ) -> bool {
        let span_start = self.span_starter(start, end);
        if self.spans[span_start].already_added {
            return false;
        }
        self.spans[span_start].already_added = true;
        let start_seg = self.spans[start].segment;
        let verb_orig = self.segments[seg].verb;
        let mut curve_part = DCurveSweep::default();
        self.seg_sub_divide(start_seg, start, end, &mut curve_part.curve);
        curve_part.set_curve_hull_sweep(verb_orig);
        let verb = if curve_part.is_curve {
            verb_orig
        } else {
            Verb::Line
        };
        let start_pt_t = self.spans[start].pt_t;
        let end_pt_t = self.spans[end].pt_t;
        path.deferred_move(self, start_pt_t);
        match verb {
            Verb::Line => {
                if !path.deferred_line(self, end_pt_t) {
                    return false;
                }
            }
            Verb::Quad => {
                path.quad_to(self, curve_part.curve.pts[1].as_point(), end_pt_t);
            }
            Verb::Conic => {
                path.conic_to(
                    self,
                    curve_part.curve.pts[1].as_point(),
                    end_pt_t,
                    curve_part.curve.weight,
                );
            }
            Verb::Cubic => {
                path.cubic_to(
                    self,
                    curve_part.curve.pts[1].as_point(),
                    curve_part.curve.pts[2].as_point(),
                    end_pt_t,
                );
            }
            _ => {}
        }
        true
    }

    pub fn seg_sub_divide(
        &self,
        seg: SegmentId,
        start: SpanId,
        end: SpanId,
        edge: &mut DCurve,
    ) -> bool {
        let start_pt_t = self.spans[start].pt_t;
        let end_pt_t = self.spans[end].pt_t;
        let s = &self.segments[seg];
        let verb = s.verb;
        edge.pts[0] = DPoint::from_point(self.pt_ts[start_pt_t].pt);
        let points = pathops_verb_to_points(verb);
        edge.pts[points] = DPoint::from_point(self.pt_ts[end_pt_t].pt);
        if verb == Verb::Line {
            return false;
        }
        let start_t = self.pt_ts[start_pt_t].t;
        let end_t = self.pt_ts[end_pt_t].t;
        if (start_t == 0.0 || end_t == 0.0) && (start_t == 1.0 || end_t == 1.0) {
            if verb == Verb::Quad {
                edge.pts[1] = DPoint::from_point(s.pts[1]);
                return false;
            }
            if verb == Verb::Conic {
                edge.pts[1] = DPoint::from_point(s.pts[1]);
                edge.weight = s.weight;
                return false;
            }
            if start_t == 0.0 {
                edge.pts[1] = DPoint::from_point(s.pts[1]);
                edge.pts[2] = DPoint::from_point(s.pts[2]);
                return false;
            }
            edge.pts[1] = DPoint::from_point(s.pts[2]);
            edge.pts[2] = DPoint::from_point(s.pts[1]);
            return false;
        }
        match verb {
            Verb::Quad => {
                let q = DQuad::from_points([s.pts[0], s.pts[1], s.pts[2]]);
                edge.pts[1] = q.sub_divide_pt(edge.pts[0], edge.pts[2], start_t, end_t);
            }
            Verb::Conic => {
                let c = DConic::from_points([s.pts[0], s.pts[1], s.pts[2]], s.weight);
                let (mid, w) = c.sub_divide_pt(start_t, end_t);
                edge.pts[1] = mid;
                edge.weight = w;
            }
            Verb::Cubic => {
                let c = DCubic::from_points(s.pts);
                let mid = c.sub_divide_pts(edge.pts[0], edge.pts[3], start_t, end_t);
                edge.pts[1] = mid[0];
                edge.pts[2] = mid[1];
            }
            _ => {}
        }
        true
    }

    // ========================================================================
    // Span / t insertion and lookup
    // ========================================================================

    pub fn seg_existing(&self, seg: SegmentId, t: f64, opp: Option<SegmentId>) -> Option<PtTId> {
        let mut test = self.segments[seg].head;
        let pt = self.seg_pt_at_t(seg, t);
        let test_pt_t = loop {
            let cur_pt_t = self.spans[test].pt_t;
            if self.pt_ts[cur_pt_t].t == t {
                break cur_pt_t;
            }
            if !self.seg_match(seg, cur_pt_t, seg, t, pt) {
                if t < self.pt_ts[cur_pt_t].t {
                    return None;
                }
            } else {
                if opp.is_none() {
                    return Some(cur_pt_t);
                }
                let mut lp = self.pt_ts[cur_pt_t].next;
                let mut found = false;
                while lp != cur_pt_t {
                    if self.pt_t_segment(lp) == seg
                        && self.pt_ts[lp].t == t
                        && self.pt_ts[lp].pt == pt
                    {
                        found = true;
                        break;
                    }
                    lp = self.pt_ts[lp].next;
                }
                if found {
                    break cur_pt_t;
                }
                return None;
            }
            match self.spans[test].next {
                Some(n) => test = n,
                None => return None,
            }
        };
        if let Some(o) = opp {
            if self.span_contains_seg(test, o).is_none() {
                return None;
            }
        }
        Some(test_pt_t)
    }

    pub fn seg_add_expanded(
        &mut self,
        seg: SegmentId,
        new_t: f64,
        test: SpanId,
        start_over: &mut bool,
    ) -> bool {
        if self.seg_contains(seg, new_t) {
            return true;
        }
        self.allocated_op_span = false;
        if !between(0.0, new_t, 1.0) {
            return false;
        }
        let new_pt_t = self.seg_add_t(seg, new_t);
        *start_over |= self.allocated_op_span;
        let new_pt_t = match new_pt_t {
            Some(p) => p,
            None => return false,
        };
        self.pt_ts[new_pt_t].pt = self.seg_pt_at_t(seg, new_t);
        let test_pt_t = self.spans[test].pt_t;
        if let Some(opp_prev) = self.pt_t_opp_prev(test_pt_t, new_pt_t) {
            let new_span = self.pt_ts[new_pt_t].span;
            self.span_merge_matches(test, new_span);
            self.pt_t_add_opp(test_pt_t, new_pt_t, opp_prev);
            self.span_check_for_collapsed_coincidence(test);
        }
        true
    }

    fn seg_insert_span(&mut self, seg: SegmentId, prev: SpanId, t: f64, pt: Point) -> SpanId {
        self.allocated_op_span = true;
        let next = self.spans[prev].next;
        let span = self.alloc_span(seg, Some(prev), t, pt, false);
        self.spans[prev].next = Some(span);
        self.spans[span].next = next;
        if let Some(n) = next {
            self.spans[n].prev = Some(span);
        }
        span
    }

    pub fn seg_add_t_pt(&mut self, seg: SegmentId, t: f64, pt: Point) -> Option<PtTId> {
        let mut span_base = self.segments[seg].head;
        let tail = self.segments[seg].tail;
        loop {
            let result = self.spans[span_base].pt_t;
            let res_t = self.pt_ts[result].t;
            if t == res_t || (!zero_or_one(t) && self.seg_match(seg, result, seg, t, pt)) {
                self.spans[span_base].span_adds += 1;
                return Some(result);
            }
            if t < res_t {
                let prev = self.spans[self.pt_ts[result].span].prev?;
                let span = self.seg_insert_span(seg, prev, t, pt);
                self.spans[span].span_adds += 1;
                return Some(self.spans[span].pt_t);
            }
            if span_base == tail {
                return None;
            }
            span_base = self.spans[span_base].next?;
        }
    }

    pub fn seg_add_t(&mut self, seg: SegmentId, t: f64) -> Option<PtTId> {
        let pt = self.seg_pt_at_t(seg, t);
        self.seg_add_t_pt(seg, t, pt)
    }

    // ========================================================================
    // Angle creation & sorting
    // ========================================================================

    pub fn seg_add_start_span(&mut self, seg: SegmentId) -> AngleId {
        let head = self.segments[seg].head;
        let next = self.spans[head].next.unwrap();
        let angle = self.alloc_angle();
        self.angle_set(angle, head, next);
        self.spans[head].to_angle = Some(angle);
        angle
    }

    pub fn seg_add_end_span(&mut self, seg: SegmentId) -> AngleId {
        let tail = self.segments[seg].tail;
        let prev = self.spans[tail].prev.unwrap();
        let angle = self.alloc_angle();
        self.angle_set(angle, tail, prev);
        self.spans[tail].from_angle = Some(angle);
        angle
    }

    pub fn seg_calc_angles(&mut self, seg: SegmentId) {
        let head = self.segments[seg].head;
        let tail = self.segments[seg].tail;
        let mut active_prior = !self.span_is_canceled(head);
        if active_prior && !self.span_simple(head) {
            self.seg_add_start_span(seg);
        }
        let mut prior = head;
        let mut span_base = self.spans[head].next.unwrap();
        while span_base != tail {
            if active_prior {
                let prior_angle = self.alloc_angle();
                self.angle_set(prior_angle, span_base, prior);
                self.spans[span_base].from_angle = Some(prior_angle);
            }
            let active = !self.span_is_canceled(span_base);
            let next = self.spans[span_base].next.unwrap();
            if active {
                let angle = self.alloc_angle();
                self.angle_set(angle, span_base, next);
                self.spans[span_base].to_angle = Some(angle);
            }
            active_prior = active;
            prior = span_base;
            span_base = next;
        }
        if active_prior && !self.span_simple(tail) {
            self.seg_add_end_span(seg);
        }
    }

    pub fn seg_clear_all(&mut self, seg: SegmentId) {
        let mut span = Some(self.segments[seg].head);
        while let Some(s) = span {
            if self.span_final(s) {
                break;
            }
            self.seg_clear_one(seg, s);
            span = self.spans[s].next;
        }
        self.coincidence_release_segment(seg);
    }

    pub fn seg_clear_one(&mut self, seg: SegmentId, span: SpanId) {
        self.spans[span].wind_value = 0;
        self.spans[span].opp_value = 0;
        self.seg_mark_done(seg, span);
    }

    pub fn seg_collapsed(&self, seg: SegmentId, s: f64, e: f64) -> Collapsed {
        let mut span = self.segments[seg].head;
        loop {
            let res = self.span_collapsed(span, s, e);
            if res != Collapsed::No {
                return res;
            }
            if self.span_final(span) {
                break;
            }
            match self.spans[span].next {
                Some(n) => span = n,
                None => break,
            }
        }
        Collapsed::No
    }

    // ========================================================================
    // Compute sums
    // ========================================================================

    pub fn seg_compute_one_sum(
        &mut self,
        base_angle: AngleId,
        next_angle: AngleId,
        include_type: AngleIncludeType,
    ) -> bool {
        let base_segment = self.angle_segment(base_angle);
        let mut sum_mi_winding = self.seg_update_winding_reverse(base_angle);
        let mut sum_su_winding = 0;
        let binary = include_type >= AngleIncludeType::BinarySingle;
        if binary {
            sum_su_winding = self.seg_update_opp_winding_reverse(base_angle);
            if self.seg_operand(base_segment) {
                core::mem::swap(&mut sum_mi_winding, &mut sum_su_winding);
            }
        }
        let next_segment = self.angle_segment(next_angle);
        let next_start = self.angles[next_angle].start.unwrap();
        let next_end = self.angles[next_angle].end.unwrap();
        let mut max_winding = 0;
        let mut sum_winding = 0;
        let mut last: Option<SpanId> = None;
        if binary {
            let mut opp_max_winding = 0;
            let mut opp_sum_winding = 0;
            self.seg_set_up_windings_binary(
                next_segment,
                next_start,
                next_end,
                &mut sum_mi_winding,
                &mut sum_su_winding,
                &mut max_winding,
                &mut sum_winding,
                &mut opp_max_winding,
                &mut opp_sum_winding,
            );
            if !self.seg_mark_angle_opp(
                next_segment,
                max_winding,
                sum_winding,
                opp_max_winding,
                opp_sum_winding,
                next_angle,
                Some(&mut last),
            ) {
                return false;
            }
        } else {
            self.seg_set_up_windings_unary(
                next_start,
                next_end,
                &mut sum_mi_winding,
                &mut max_winding,
                &mut sum_winding,
            );
            if !self.seg_mark_angle(
                next_segment,
                max_winding,
                sum_winding,
                next_angle,
                Some(&mut last),
            ) {
                return false;
            }
        }
        self.angles[next_angle].last_marked = last;
        true
    }

    pub fn seg_compute_one_sum_reverse(
        &mut self,
        base_angle: AngleId,
        next_angle: AngleId,
        include_type: AngleIncludeType,
    ) -> bool {
        let base_segment = self.angle_segment(base_angle);
        let mut sum_mi_winding = self.seg_update_winding_angle(base_angle);
        let mut sum_su_winding = 0;
        let binary = include_type >= AngleIncludeType::BinarySingle;
        if binary {
            sum_su_winding = self.seg_update_opp_winding_angle(base_angle);
            if self.seg_operand(base_segment) {
                core::mem::swap(&mut sum_mi_winding, &mut sum_su_winding);
            }
        }
        let next_segment = self.angle_segment(next_angle);
        let next_start = self.angles[next_angle].start.unwrap();
        let next_end = self.angles[next_angle].end.unwrap();
        let mut max_winding = 0;
        let mut sum_winding = 0;
        let mut last: Option<SpanId> = None;
        if binary {
            let mut opp_max_winding = 0;
            let mut opp_sum_winding = 0;
            self.seg_set_up_windings_binary(
                next_segment,
                next_end,
                next_start,
                &mut sum_mi_winding,
                &mut sum_su_winding,
                &mut max_winding,
                &mut sum_winding,
                &mut opp_max_winding,
                &mut opp_sum_winding,
            );
            if !self.seg_mark_angle_opp(
                next_segment,
                max_winding,
                sum_winding,
                opp_max_winding,
                opp_sum_winding,
                next_angle,
                Some(&mut last),
            ) {
                return false;
            }
        } else {
            self.seg_set_up_windings_unary(
                next_end,
                next_start,
                &mut sum_mi_winding,
                &mut max_winding,
                &mut sum_winding,
            );
            if !self.seg_mark_angle(
                next_segment,
                max_winding,
                sum_winding,
                next_angle,
                Some(&mut last),
            ) {
                return false;
            }
        }
        self.angles[next_angle].last_marked = last;
        true
    }

    pub fn seg_compute_sum(
        &mut self,
        start: SpanId,
        end: SpanId,
        include_type: AngleIncludeType,
    ) -> i32 {
        let first_angle_opt = self.seg_span_to_angle(end, start);
        let mut first_angle = match first_angle_opt {
            Some(a) if self.angles[a].next.is_some() => a,
            _ => return SK_NAN_32,
        };
        let mut base_angle: Option<AngleId> = None;
        let mut try_reverse = false;
        let mut angle = self.angle_previous(first_angle);
        let mut next = self.angles[angle].next.unwrap();
        first_angle = next;
        loop {
            let prior = angle;
            angle = next;
            next = self.angles[angle].next.unwrap();
            if self.angles[prior].unorderable
                || self.angles[angle].unorderable
                || self.angles[next].unorderable
            {
                base_angle = None;
            } else {
                let starter = self.angle_starter(angle);
                let test_winding = self.spans[starter].wind_sum;
                if SK_MIN_S32 != test_winding {
                    base_angle = Some(angle);
                    try_reverse = true;
                } else if let Some(ba) = base_angle {
                    self.seg_compute_one_sum(ba, angle, include_type);
                    let starter2 = self.angle_starter(angle);
                    base_angle = if SK_MIN_S32 != self.spans[starter2].wind_sum {
                        Some(angle)
                    } else {
                        None
                    };
                }
            }
            if next == first_angle {
                break;
            }
        }
        if let Some(ba) = base_angle {
            let fa_starter = self.angle_starter(first_angle);
            if SK_MIN_S32 == self.spans[fa_starter].wind_sum {
                first_angle = ba;
                try_reverse = true;
            }
        }
        if try_reverse {
            base_angle = None;
            let mut prior = first_angle;
            loop {
                angle = prior;
                prior = self.angle_previous(angle);
                next = self.angles[angle].next.unwrap();
                if self.angles[prior].unorderable
                    || self.angles[angle].unorderable
                    || self.angles[next].unorderable
                {
                    base_angle = None;
                } else {
                    let starter = self.angle_starter(angle);
                    let test_winding = self.spans[starter].wind_sum;
                    if SK_MIN_S32 != test_winding {
                        base_angle = Some(angle);
                    } else if let Some(ba) = base_angle {
                        self.seg_compute_one_sum_reverse(ba, angle, include_type);
                        let starter2 = self.angle_starter(angle);
                        base_angle = if SK_MIN_S32 != self.spans[starter2].wind_sum {
                            Some(angle)
                        } else {
                            None
                        };
                    }
                }
                if prior == first_angle {
                    break;
                }
            }
        }
        let starter = self.span_starter(start, end);
        self.spans[starter].wind_sum
    }

    pub fn seg_contains(&self, seg: SegmentId, new_t: f64) -> bool {
        let mut span_base = self.segments[seg].head;
        let tail = self.segments[seg].tail;
        loop {
            if self.pt_t_contains_seg_t(self.spans[span_base].pt_t, seg, new_t) {
                return true;
            }
            if span_base == tail {
                break;
            }
            match self.spans[span_base].next {
                Some(n) => span_base = n,
                None => break,
            }
        }
        false
    }

    pub fn seg_release(&mut self, seg: SegmentId, span: SpanId) {
        if self.spans[span].done {
            self.segments[seg].done_count -= 1;
        }
        self.segments[seg].count -= 1;
    }

    // ========================================================================
    // Find next (Op, Winding, Xor)
    // ========================================================================

    #[allow(clippy::too_many_arguments)]
    pub fn seg_find_next_op(
        &mut self,
        seg: SegmentId,
        chase: &mut Vec<SpanId>,
        next_start: &mut SpanId,
        next_end: &mut SpanId,
        unsortable: &mut bool,
        simple: &mut bool,
        op: PathOp,
        xor_mi_mask: i32,
        xor_su_mask: i32,
    ) -> Option<SegmentId> {
        let start = *next_start;
        let end = *next_end;
        let mut step = self.span_step(start, end);
        let other = self.seg_is_simple(seg, next_start, &mut step);
        *simple = other.is_some();
        if let Some(other_seg) = other {
            let start_span = self.span_starter(start, end);
            if self.spans[start_span].done {
                return None;
            }
            self.seg_mark_done(seg, start_span);
            *next_end = if step > 0 {
                self.spans[*next_start].next.unwrap()
            } else {
                self.spans[*next_start].prev.unwrap()
            };
            return Some(other_seg);
        }
        let end_near = if step > 0 {
            self.spans[*next_start].next.unwrap()
        } else {
            self.spans[*next_start].prev.unwrap()
        };
        let calc_winding = self.seg_compute_sum(start, end_near, AngleIncludeType::BinaryOpp);
        if calc_winding == SK_NAN_32 {
            *unsortable = true;
            let starter = self.span_starter(start, end);
            self.seg_mark_done(seg, starter);
            return None;
        }
        let angle = self.seg_span_to_angle(end, start)?;
        if self.angles[angle].unorderable {
            *unsortable = true;
            let starter = self.span_starter(start, end);
            self.seg_mark_done(seg, starter);
            return None;
        }
        let mut sum_mi_winding = self.seg_update_winding(end, start);
        if sum_mi_winding == SK_MIN_S32 {
            *unsortable = true;
            let starter = self.span_starter(start, end);
            self.seg_mark_done(seg, starter);
            return None;
        }
        let mut sum_su_winding = self.seg_update_opp_winding(end, start);
        if self.seg_operand(seg) {
            core::mem::swap(&mut sum_mi_winding, &mut sum_su_winding);
        }
        let mut next_angle = self.angles[angle].next.unwrap();
        let mut found_angle: Option<AngleId> = None;
        let mut found_done = false;
        let mut active_count = 0;
        loop {
            let next_segment = self.angle_segment(next_angle);
            let na_start = self.angles[next_angle].start.unwrap();
            let na_end = self.angles[next_angle].end.unwrap();
            let active_angle = self.seg_active_op_with_sums(
                next_segment,
                xor_mi_mask,
                xor_su_mask,
                na_start,
                na_end,
                op,
                &mut sum_mi_winding,
                &mut sum_su_winding,
            );
            if active_angle {
                active_count += 1;
                if found_angle.is_none() || (found_done && (active_count & 1) != 0) {
                    found_angle = Some(next_angle);
                    found_done = self.seg_angle_done(next_angle);
                }
            }
            if !self.seg_done(next_segment) {
                if !active_angle {
                    let _ = self.seg_mark_and_chase_done(next_segment, na_start, na_end, None);
                }
                if let Some(last) = self.angle_last_marked(next_angle) {
                    chase.push(last);
                }
            }
            next_angle = self.angles[next_angle].next.unwrap();
            if next_angle == angle {
                break;
            }
        }
        let start_seg = self.spans[start].segment;
        let starter = self.span_starter(start, end);
        self.seg_mark_done(start_seg, starter);
        let fa = found_angle?;
        *next_start = self.angles[fa].start.unwrap();
        *next_end = self.angles[fa].end.unwrap();
        Some(self.angle_segment(fa))
    }

    pub fn seg_find_next_winding(
        &mut self,
        seg: SegmentId,
        chase: &mut Vec<SpanId>,
        next_start: &mut SpanId,
        next_end: &mut SpanId,
        unsortable: &mut bool,
    ) -> Option<SegmentId> {
        let start = *next_start;
        let end = *next_end;
        let mut step = self.span_step(start, end);
        if let Some(other_seg) = self.seg_is_simple(seg, next_start, &mut step) {
            let start_span = self.span_starter(start, end);
            if self.spans[start_span].done {
                return None;
            }
            self.seg_mark_done(seg, start_span);
            *next_end = if step > 0 {
                self.spans[*next_start].next.unwrap()
            } else {
                self.spans[*next_start].prev.unwrap()
            };
            return Some(other_seg);
        }
        let end_near = if step > 0 {
            self.spans[*next_start].next.unwrap()
        } else {
            self.spans[*next_start].prev.unwrap()
        };
        let calc_winding = self.seg_compute_sum(start, end_near, AngleIncludeType::UnaryWinding);
        if calc_winding == SK_NAN_32 {
            *unsortable = true;
            let starter = self.span_starter(start, end);
            self.seg_mark_done(seg, starter);
            return None;
        }
        let angle = self.seg_span_to_angle(end, start)?;
        if self.angles[angle].unorderable {
            *unsortable = true;
            let starter = self.span_starter(start, end);
            self.seg_mark_done(seg, starter);
            return None;
        }
        let mut sum_winding = self.seg_update_winding(end, start);
        let mut next_angle = self.angles[angle].next.unwrap();
        let mut found_angle: Option<AngleId> = None;
        let mut found_done = false;
        let mut active_count = 0;
        loop {
            let next_segment = self.angle_segment(next_angle);
            let na_start = self.angles[next_angle].start.unwrap();
            let na_end = self.angles[next_angle].end.unwrap();
            let active_angle = self.seg_active_winding_with_sum(na_start, na_end, &mut sum_winding);
            if active_angle {
                active_count += 1;
                if found_angle.is_none() || (found_done && (active_count & 1) != 0) {
                    found_angle = Some(next_angle);
                    found_done = self.seg_angle_done(next_angle);
                }
            }
            if !self.seg_done(next_segment) {
                if !active_angle {
                    let _ = self.seg_mark_and_chase_done(next_segment, na_start, na_end, None);
                }
                if let Some(last) = self.angle_last_marked(next_angle) {
                    chase.push(last);
                }
            }
            next_angle = self.angles[next_angle].next.unwrap();
            if next_angle == angle {
                break;
            }
        }
        let start_seg = self.spans[start].segment;
        let starter = self.span_starter(start, end);
        self.seg_mark_done(start_seg, starter);
        let fa = found_angle?;
        *next_start = self.angles[fa].start.unwrap();
        *next_end = self.angles[fa].end.unwrap();
        Some(self.angle_segment(fa))
    }

    pub fn seg_find_next_xor(
        &mut self,
        seg: SegmentId,
        next_start: &mut SpanId,
        next_end: &mut SpanId,
        unsortable: &mut bool,
    ) -> Option<SegmentId> {
        let start = *next_start;
        let end = *next_end;
        let mut step = self.span_step(start, end);
        if let Some(other_seg) = self.seg_is_simple(seg, next_start, &mut step) {
            let start_span = self.span_starter(start, end);
            if self.spans[start_span].done {
                return None;
            }
            self.seg_mark_done(seg, start_span);
            *next_end = if step > 0 {
                self.spans[*next_start].next.unwrap()
            } else {
                self.spans[*next_start].prev.unwrap()
            };
            return Some(other_seg);
        }
        let angle = match self.seg_span_to_angle(end, start) {
            Some(a) if !self.angles[a].unorderable => a,
            _ => {
                *unsortable = true;
                let starter = self.span_starter(start, end);
                self.seg_mark_done(seg, starter);
                return None;
            }
        };
        let mut next_angle_opt = self.angles[angle].next;
        let mut found_angle: Option<AngleId> = None;
        let mut found_done = false;
        let mut active_count = 0;
        loop {
            let next_angle = next_angle_opt?;
            active_count += 1;
            if found_angle.is_none() || (found_done && (active_count & 1) != 0) {
                found_angle = Some(next_angle);
                found_done = self.seg_angle_done(next_angle);
                if !found_done {
                    break;
                }
            }
            next_angle_opt = self.angles[next_angle].next;
            if next_angle_opt == Some(angle) {
                break;
            }
        }
        let start_seg = self.spans[start].segment;
        let starter = self.span_starter(start, end);
        self.seg_mark_done(start_seg, starter);
        let fa = found_angle?;
        *next_start = self.angles[fa].start.unwrap();
        *next_end = self.angles[fa].end.unwrap();
        Some(self.angle_segment(fa))
    }

    // ========================================================================
    // Proximity / mark / chase
    // ========================================================================

    pub fn seg_is_close(&self, seg: SegmentId, t: f64, opp: SegmentId) -> bool {
        let c_pt = self.seg_d_pt_at_t(seg, t);
        let dxdy = self.seg_d_slope_at_t(seg, t);
        let perp = DLine::new(c_pt, DPoint::new(c_pt.x + dxdy.y, c_pt.y - dxdy.x));
        let mut i = Intersections::new();
        let o = &self.segments[opp];
        curve_intersect_ray(&o.pts, o.weight, o.verb, &perp, &mut i);
        for index in 0..i.used() {
            if c_pt.roughly_equal(i.pt[index]) {
                return true;
            }
        }
        false
    }

    pub fn seg_mark_all_done(&mut self, seg: SegmentId) {
        let mut span = Some(self.segments[seg].head);
        while let Some(s) = span {
            if self.span_final(s) {
                break;
            }
            self.seg_mark_done(seg, s);
            span = self.spans[s].next;
        }
    }

    pub fn seg_mark_and_chase_done(
        &mut self,
        seg: SegmentId,
        mut start: SpanId,
        end: SpanId,
        found: Option<&mut Option<SpanId>>,
    ) -> bool {
        let mut step = self.span_step(start, end);
        let mut min_span = self.span_starter(start, end);
        self.seg_mark_done(seg, min_span);
        let mut last: Option<SpanId> = None;
        let mut other = seg;
        let mut prior_done: Option<SpanId> = None;
        let mut last_done: Option<SpanId> = None;
        let mut safety_net = 1000;
        while let Some(next_other) =
            self.seg_next_chase(other, &mut start, &mut step, Some(&mut min_span), Some(&mut last))
        {
            other = next_other;
            safety_net -= 1;
            if safety_net == 0 {
                return false;
            }
            if self.seg_done(other) {
                break;
            }
            if last_done == Some(min_span) || prior_done == Some(min_span) {
                if let Some(f) = found {
                    *f = None;
                }
                return true;
            }
            self.seg_mark_done(other, min_span);
            prior_done = last_done;
            last_done = Some(min_span);
        }
        if let Some(f) = found {
            *f = last;
        }
        true
    }

    pub fn seg_mark_and_chase_winding(
        &mut self,
        seg: SegmentId,
        mut start: SpanId,
        end: SpanId,
        winding: i32,
        last_ptr: Option<&mut Option<SpanId>>,
    ) -> bool {
        let mut span_start = self.span_starter(start, end);
        let mut step = self.span_step(start, end);
        let success = self.seg_mark_winding(seg, span_start, winding);
        let mut last: Option<SpanId> = None;
        let mut other = seg;
        let mut safety_net = 1000;
        while let Some(next_other) = self.seg_next_chase(
            other,
            &mut start,
            &mut step,
            Some(&mut span_start),
            Some(&mut last),
        ) {
            other = next_other;
            safety_net -= 1;
            if safety_net == 0 {
                return false;
            }
            if self.spans[span_start].wind_sum != SK_MIN_S32 {
                break;
            }
            let _ = self.seg_mark_winding(other, span_start, winding);
        }
        if let Some(lp) = last_ptr {
            *lp = last;
        }
        success
    }

    pub fn seg_mark_and_chase_winding_opp(
        &mut self,
        seg: SegmentId,
        mut start: SpanId,
        end: SpanId,
        winding: i32,
        opp_winding: i32,
        last_ptr: Option<&mut Option<SpanId>>,
    ) -> bool {
        let mut span_start = self.span_starter(start, end);
        let mut step = self.span_step(start, end);
        let success = self.seg_mark_winding_opp(seg, span_start, winding, opp_winding);
        let mut last: Option<SpanId> = None;
        let mut other = seg;
        let mut safety_net = 1000;
        while let Some(next_other) = self.seg_next_chase(
            other,
            &mut start,
            &mut step,
            Some(&mut span_start),
            Some(&mut last),
        ) {
            other = next_other;
            safety_net -= 1;
            if safety_net == 0 {
                return false;
            }
            if self.spans[span_start].wind_sum != SK_MIN_S32 {
                if self.seg_operand(seg) == self.seg_operand(other) {
                    if self.spans[span_start].wind_sum != winding
                        || self.spans[span_start].opp_sum != opp_winding
                    {
                        self.winding_failed = true;
                        return true;
                    }
                } else {
                    if self.spans[span_start].wind_sum != opp_winding {
                        return false;
                    }
                    if self.spans[span_start].opp_sum != winding {
                        return false;
                    }
                }
                break;
            }
            if self.seg_operand(seg) == self.seg_operand(other) {
                let _ = self.seg_mark_winding_opp(other, span_start, winding, opp_winding);
            } else {
                let _ = self.seg_mark_winding_opp(other, span_start, opp_winding, winding);
            }
        }
        if let Some(lp) = last_ptr {
            *lp = last;
        }
        success
    }

    pub fn seg_mark_angle(
        &mut self,
        seg: SegmentId,
        mut max_winding: i32,
        sum_winding: i32,
        angle: AngleId,
        result: Option<&mut Option<SpanId>>,
    ) -> bool {
        if Self::seg_use_inner_winding(max_winding, sum_winding) {
            max_winding = sum_winding;
        }
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.seg_mark_and_chase_winding(seg, start, end, max_winding, result)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn seg_mark_angle_opp(
        &mut self,
        seg: SegmentId,
        mut max_winding: i32,
        sum_winding: i32,
        mut opp_max_winding: i32,
        opp_sum_winding: i32,
        angle: AngleId,
        result: Option<&mut Option<SpanId>>,
    ) -> bool {
        if Self::seg_use_inner_winding(max_winding, sum_winding) {
            max_winding = sum_winding;
        }
        if opp_max_winding != opp_sum_winding
            && Self::seg_use_inner_winding(opp_max_winding, opp_sum_winding)
        {
            opp_max_winding = opp_sum_winding;
        }
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.seg_mark_and_chase_winding_opp(seg, start, end, max_winding, opp_max_winding, result)
    }

    pub fn seg_mark_done(&mut self, seg: SegmentId, span: SpanId) {
        if self.spans[span].done {
            return;
        }
        self.spans[span].done = true;
        self.segments[seg].done_count += 1;
    }

    pub fn seg_mark_winding(&mut self, _seg: SegmentId, span: SpanId, winding: i32) -> bool {
        if self.spans[span].done {
            return false;
        }
        self.span_set_wind_sum(span, winding);
        true
    }

    pub fn seg_mark_winding_opp(
        &mut self,
        _seg: SegmentId,
        span: SpanId,
        winding: i32,
        opp_winding: i32,
    ) -> bool {
        if self.spans[span].done {
            return false;
        }
        self.span_set_wind_sum(span, winding);
        self.span_set_opp_sum(span, opp_winding);
        true
    }

    pub fn seg_match(
        &self,
        seg: SegmentId,
        base: PtTId,
        test_parent: SegmentId,
        test_t: f64,
        test_pt: Point,
    ) -> bool {
        if seg == test_parent && precisely_equal(self.pt_ts[base].t, test_t) {
            return true;
        }
        if !DPoint::approximately_equal_pts(test_pt, self.pt_ts[base].pt) {
            return false;
        }
        seg != test_parent
            || !self.seg_pts_disjoint(seg, self.pt_ts[base].t, self.pt_ts[base].pt, test_t, test_pt)
    }

    #[inline]
    pub fn seg_is_simple(
        &self,
        seg: SegmentId,
        end: &mut SpanId,
        step: &mut i32,
    ) -> Option<SegmentId> {
        self.seg_next_chase(seg, end, step, None, None)
    }

    pub fn seg_next_chase(
        &self,
        _seg: SegmentId,
        start_ptr: &mut SpanId,
        step_ptr: &mut i32,
        min_ptr: Option<&mut SpanId>,
        last: Option<&mut Option<SpanId>>,
    ) -> Option<SegmentId> {
        let orig_start = *start_ptr;
        let step = *step_ptr;
        let mut end_span = if step > 0 {
            self.spans[orig_start].next?
        } else {
            self.spans[orig_start].prev?
        };
        let angle = if step > 0 {
            self.spans[end_span].from_angle
        } else {
            self.spans[end_span].to_angle
        };
        let (other, found_span, other_end) = match angle {
            None => {
                let end_t = self.span_t(end_span);
                if end_t != 0.0 && end_t != 1.0 {
                    return None;
                }
                let other_pt_t = self.pt_ts[self.spans[end_span].pt_t].next;
                let other = self.pt_t_segment(other_pt_t);
                let found_span = self.pt_ts[other_pt_t].span;
                let other_end = if step > 0 {
                    if !self.span_final(found_span) {
                        self.spans[found_span].next
                    } else {
                        None
                    }
                } else {
                    self.spans[found_span].prev
                };
                (other, found_span, other_end)
            }
            Some(a) => {
                let loop_count = self.angle_loop_count(a);
                if loop_count > 2 {
                    if let Some(l) = last {
                        *l = Some(end_span);
                    }
                    return None;
                }
                let next = self.angles[a].next?;
                let other = self.angle_segment(next);
                let found_span = self.angles[next].start.unwrap();
                end_span = found_span;
                let other_end = self.angles[next].end;
                (other, found_span, other_end)
            }
        };
        let other_end = other_end?;
        let found_step = self.span_step(found_span, other_end);
        if *step_ptr != found_step {
            if let Some(l) = last {
                *l = Some(end_span);
            }
            return None;
        }
        let orig_min = if step < 0 {
            self.spans[orig_start].prev?
        } else {
            orig_start
        };
        let found_min = self.span_starter(found_span, other_end);
        if self.spans[found_min].wind_value != self.spans[orig_min].wind_value
            || self.spans[found_min].opp_value != self.spans[orig_min].opp_value
        {
            if let Some(l) = last {
                *l = Some(end_span);
            }
            return None;
        }
        *start_ptr = found_span;
        *step_ptr = found_step;
        if let Some(mp) = min_ptr {
            *mp = found_min;
        }
        Some(other)
    }

    // ========================================================================
    // Missing coincidence / moveMultiples / moveNearby
    // ========================================================================

    pub fn seg_clear_visited(&mut self, mut span: SpanId) {
        loop {
            let stop_pt_t = self.spans[span].pt_t;
            let mut pt_t = self.pt_ts[stop_pt_t].next;
            while pt_t != stop_pt_t {
                let opp = self.pt_t_segment(pt_t);
                self.segments[opp].visited = false;
                pt_t = self.pt_ts[pt_t].next;
            }
            if self.span_final(span) {
                break;
            }
            match self.spans[span].next {
                Some(n) => span = n,
                None => break,
            }
        }
    }

    pub fn seg_missing_coincidence(&mut self, seg: SegmentId) -> bool {
        if self.seg_done(seg) {
            return false;
        }
        let head = self.segments[seg].head;
        let mut prior: Option<SpanId> = None;
        let mut span_base = head;
        let mut result = false;
        let mut safety_net = 1000;
        loop {
            let span_stop_pt_t = self.spans[span_base].pt_t;
            let mut pt_t = self.pt_ts[span_stop_pt_t].next;
            while pt_t != span_stop_pt_t {
                safety_net -= 1;
                if safety_net == 0 {
                    return false;
                }
                if !self.pt_ts[pt_t].deleted {
                    let opp = self.spans[self.pt_ts[pt_t].span].segment;
                    let should_check = !self.seg_done(opp)
                        && self.seg_visited(opp)
                        && span_base != head
                        && self.pt_t_segment(pt_t) != seg
                        && (self.span_final(span_base)
                            || !self.span_contains_coincidence_seg(span_base, opp))
                        && !self.span_contains_coin_end_seg(span_base, opp);
                    if should_check {
                        let mut prior_pt_t: Option<PtTId> = None;
                        let mut prior_opp: Option<SegmentId> = None;
                        let mut prior_test = self.spans[span_base].prev;
                        while prior_opp.is_none() {
                            let pt_span = match prior_test {
                                Some(p) => p,
                                None => break,
                            };
                            let prior_stop_pt_t = self.spans[pt_span].pt_t;
                            let mut walk = self.pt_ts[prior_stop_pt_t].next;
                            while walk != prior_stop_pt_t {
                                if !self.pt_ts[walk].deleted
                                    && self.spans[self.pt_ts[walk].span].segment == opp
                                {
                                    prior = Some(pt_span);
                                    prior_opp = Some(opp);
                                    prior_pt_t = Some(walk);
                                    break;
                                }
                                walk = self.pt_ts[walk].next;
                            }
                            prior_test = self.spans[pt_span].prev;
                        }
                        if let (Some(_), Some(mut p_pt_t), Some(prior_span)) =
                            (prior_opp, prior_pt_t, prior)
                        {
                            if p_pt_t != pt_t {
                                let mut cur_pt_t = pt_t;
                                let mut opp_start = self.spans[prior_span].pt_t;
                                let mut opp_end = self.spans[span_base].pt_t;
                                let swapped = self.pt_ts[p_pt_t].t > self.pt_ts[cur_pt_t].t;
                                if swapped {
                                    core::mem::swap(&mut p_pt_t, &mut cur_pt_t);
                                    core::mem::swap(&mut opp_start, &mut opp_end);
                                }
                                let root_prior_pt_t = self.spans[self.pt_ts[p_pt_t].span].pt_t;
                                let root_pt_t = self.spans[self.pt_ts[cur_pt_t].span].pt_t;
                                let root_opp_start = self.spans[self.pt_ts[opp_start].span].pt_t;
                                let root_opp_end = self.spans[self.pt_ts[opp_end].span].pt_t;
                                if !self.coincidence_contains_pts(
                                    root_prior_pt_t,
                                    root_pt_t,
                                    root_opp_start,
                                    root_opp_end,
                                ) && self.seg_test_for_coincidence(
                                    seg,
                                    root_prior_pt_t,
                                    root_pt_t,
                                    prior_span,
                                    span_base,
                                    opp,
                                ) {
                                    if !self.coincidence_extend(
                                        root_prior_pt_t,
                                        root_pt_t,
                                        root_opp_start,
                                        root_opp_end,
                                    ) {
                                        self.coincidence_add(
                                            root_prior_pt_t,
                                            root_pt_t,
                                            root_opp_start,
                                            root_opp_end,
                                        );
                                    }
                                    result = true;
                                }
                            }
                        }
                    }
                }
                pt_t = self.pt_ts[pt_t].next;
            }
            if self.span_final(span_base) {
                break;
            }
            match self.spans[span_base].next {
                Some(n) => span_base = n,
                None => break,
            }
        }
        self.seg_clear_visited(head);
        result
    }

    pub fn seg_move_multiples(&mut self, seg: SegmentId) -> bool {
        let mut test_opt = Some(self.segments[seg].head);
        while let Some(test) = test_opt {
            let add_count = self.spans[test].span_adds;
            if add_count > 1 {
                let start_pt_t = self.spans[test].pt_t;
                let mut test_pt_t = start_pt_t;
                let mut safety_hatch = 1000;
                'pt_loop: loop {
                    safety_hatch -= 1;
                    if safety_hatch == 0 {
                        return false;
                    }
                    let opp_span = self.pt_ts[test_pt_t].span;
                    let opp_segment = self.spans[opp_span].segment;
                    if self.spans[opp_span].span_adds != add_count
                        && !self.span_deleted(opp_span)
                        && opp_segment != seg
                    {
                        let opp_span_t = self.span_t(opp_span);
                        let mut opp_first = opp_span;
                        let mut opp_prev_opt = self.spans[opp_span].prev;
                        while let Some(opp_prev) = opp_prev_opt {
                            if !roughly_equal(self.span_t(opp_prev), opp_span_t) {
                                break;
                            }
                            if self.spans[opp_prev].span_adds != add_count
                                && !self.span_deleted(opp_prev)
                            {
                                opp_first = opp_prev;
                            }
                            opp_prev_opt = self.spans[opp_prev].prev;
                        }
                        let mut opp_last = opp_span;
                        let mut opp_next_opt = if self.span_final(opp_span) {
                            None
                        } else {
                            self.spans[opp_span].next
                        };
                        while let Some(opp_next) = opp_next_opt {
                            if !roughly_equal(self.span_t(opp_next), opp_span_t) {
                                break;
                            }
                            if self.spans[opp_next].span_adds != add_count
                                && !self.span_deleted(opp_next)
                            {
                                opp_last = opp_next;
                            }
                            opp_next_opt = if self.span_final(opp_next) {
                                None
                            } else {
                                self.spans[opp_next].next
                            };
                        }
                        if opp_first != opp_last {
                            let mut opp_test = opp_first;
                            loop {
                                if opp_test != opp_span {
                                    let opp_start_pt_t = self.spans[opp_test].pt_t;
                                    let opp_pt_t = self.pt_ts[opp_start_pt_t].next;
                                    while opp_pt_t != opp_start_pt_t {
                                        let opp_pt_t_seg = self.pt_t_segment(opp_pt_t);
                                        if opp_pt_t_seg == seg {
                                            break;
                                        }
                                        let mut match_pt_t = start_pt_t;
                                        let mut found_match = false;
                                        loop {
                                            if self.pt_t_segment(match_pt_t) == opp_pt_t_seg {
                                                found_match = true;
                                                break;
                                            }
                                            match_pt_t = self.pt_ts[match_pt_t].next;
                                            if match_pt_t == start_pt_t {
                                                break;
                                            }
                                        }
                                        if found_match {
                                            self.span_merge_matches(opp_test, opp_span);
                                            self.span_add_opp(opp_test, opp_span);
                                            break 'pt_loop;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                                if opp_test == opp_last {
                                    break;
                                }
                                match self.spans[opp_test].next {
                                    Some(n) => opp_test = n,
                                    None => break,
                                }
                            }
                        }
                    }
                    test_pt_t = self.pt_ts[test_pt_t].next;
                    if test_pt_t == start_pt_t {
                        break;
                    }
                }
            }
            test_opt = if self.span_final(test) {
                None
            } else {
                self.spans[test].next
            };
        }
        true
    }

    pub fn seg_spans_nearby(
        &self,
        ref_span: SpanId,
        check_span: SpanId,
        found: &mut bool,
    ) -> bool {
        let ref_head = self.spans[ref_span].pt_t;
        let check_head = self.spans[check_span].pt_t;
        if !DPoint::way_roughly_equal_pts(self.pt_ts[ref_head].pt, self.pt_ts[check_head].pt) {
            *found = false;
            return true;
        }
        let mut dist_sq_best = SK_SCALAR_MAX;
        let mut ref_best: Option<PtTId> = None;
        let mut check_best: Option<PtTId> = None;
        let mut ref_cur = ref_head;
        'outer: loop {
            if !self.pt_ts[ref_cur].deleted {
                while self.pt_t_already_seen(ref_cur, ref_head) {
                    ref_cur = self.pt_ts[ref_cur].next;
                    if ref_cur == ref_head {
                        break 'outer;
                    }
                }
                let mut check = check_head;
                let ref_seg = self.pt_t_segment(ref_cur);
                let mut escape_hatch = 100;
                'inner: loop {
                    if !self.pt_ts[check].deleted {
                        while self.pt_t_already_seen(check, check_head) {
                            check = self.pt_ts[check].next;
                            if check == check_head {
                                break 'inner;
                            }
                        }
                        let dist_sq = distance_to_sqd(self.pt_ts[ref_cur].pt, self.pt_ts[check].pt);
                        let check_seg = self.pt_t_segment(check);
                        if dist_sq_best > dist_sq
                            && (ref_seg != check_seg
                                || !self.seg_pts_disjoint(
                                    ref_seg,
                                    self.pt_ts[ref_cur].t,
                                    self.pt_ts[ref_cur].pt,
                                    self.pt_ts[check].t,
                                    self.pt_ts[check].pt,
                                ))
                        {
                            dist_sq_best = dist_sq;
                            ref_best = Some(ref_cur);
                            check_best = Some(check);
                        }
                        escape_hatch -= 1;
                        if escape_hatch <= 0 {
                            return false;
                        }
                    }
                    check = self.pt_ts[check].next;
                    if check == check_head {
                        break;
                    }
                }
            }
            ref_cur = self.pt_ts[ref_cur].next;
            if ref_cur == ref_head {
                break;
            }
        }
        *found = match (ref_best, check_best) {
            (Some(rb), Some(cb)) => {
                let rb_seg = self.pt_t_segment(rb);
                let cb_seg = self.pt_t_segment(cb);
                self.seg_match(rb_seg, rb, cb_seg, self.pt_ts[cb].t, self.pt_ts[cb].pt)
            }
            _ => false,
        };
        true
    }

    pub fn seg_move_nearby(&mut self, seg: SegmentId) -> bool {
        let head = self.segments[seg].head;
        let mut span_base = head;
        let mut escape_hatch = 9999;
        loop {
            let head_pt_t = self.spans[span_base].pt_t;
            let mut pt_t = self.pt_ts[head_pt_t].next;
            while pt_t != head_pt_t {
                escape_hatch -= 1;
                if escape_hatch == 0 {
                    return false;
                }
                let test = self.pt_ts[pt_t].span;
                if self.pt_t_segment(pt_t) == seg
                    && !self.pt_ts[pt_t].deleted
                    && test != span_base
                    && self.spans[test].pt_t == pt_t
                {
                    if self.span_final(test) {
                        if span_base == head {
                            self.seg_clear_all(seg);
                            return true;
                        }
                        self.span_release(span_base, pt_t);
                    } else if self.spans[test].prev.is_some() {
                        self.span_release(test, head_pt_t);
                    }
                    break;
                }
                pt_t = self.pt_ts[pt_t].next;
            }
            span_base = match self.spans[span_base].next {
                Some(n) => n,
                None => break,
            };
            if self.span_final(span_base) {
                break;
            }
        }

        span_base = head;
        loop {
            let test = match self.spans[span_base].next {
                Some(n) => n,
                None => break,
            };
            let mut found = false;
            if !self.seg_spans_nearby(span_base, test, &mut found) {
                return false;
            }
            if found {
                if self.span_final(test) {
                    if self.spans[span_base].prev.is_some() {
                        self.span_merge(test, span_base);
                    } else {
                        self.seg_clear_all(seg);
                        return true;
                    }
                } else {
                    self.span_merge(span_base, test);
                }
            }
            span_base = test;
            if self.span_final(span_base) {
                break;
            }
        }
        true
    }

    pub fn seg_pts_disjoint(
        &self,
        seg: SegmentId,
        t1: f64,
        pt1: Point,
        t2: f64,
        pt2: Point,
    ) -> bool {
        if self.segments[seg].verb == Verb::Line {
            return false;
        }
        let mid_t = (t1 + t2) / 2.0;
        let mid_pt = self.seg_pt_at_t(seg, mid_t);
        let se_dist_sq = cpp_max_f64((distance_to_sqd(pt1, pt2) * 2.0) as f64, FLT_EPSILON * 2.0);
        (distance_to_sqd(mid_pt, pt1) as f64) > se_dist_sq
            || (distance_to_sqd(mid_pt, pt2) as f64) > se_dist_sq
    }

    // ========================================================================
    // Sort angles
    // ========================================================================

    pub fn seg_sort_angles(&mut self, seg: SegmentId) -> bool {
        let mut span = self.segments[seg].head;
        loop {
            let from_angle = self.spans[span].from_angle;
            let to_angle = if self.span_final(span) {
                None
            } else {
                self.spans[span].to_angle
            };
            if from_angle.is_some() || to_angle.is_some() {
                let base_angle = match (from_angle, to_angle) {
                    (Some(fa), Some(ta)) => {
                        if !self.angle_insert(fa, ta) {
                            return false;
                        }
                        fa
                    }
                    (Some(fa), None) => fa,
                    (None, Some(ta)) => ta,
                    (None, None) => unreachable!(),
                };
                let stop_pt_t = self.spans[span].pt_t;
                let mut pt_t = stop_pt_t;
                let mut safety_net = 1000;
                loop {
                    safety_net -= 1;
                    if safety_net == 0 {
                        return false;
                    }
                    let o_span = self.pt_ts[pt_t].span;
                    if o_span != span {
                        if let Some(o_angle) = self.spans[o_span].from_angle {
                            if !self.angle_loop_contains(o_angle, base_angle) {
                                self.angle_insert(base_angle, o_angle);
                            }
                        }
                        if !self.span_final(o_span) {
                            if let Some(o_angle) = self.spans[o_span].to_angle {
                                if !self.angle_loop_contains(o_angle, base_angle) {
                                    self.angle_insert(base_angle, o_angle);
                                }
                            }
                        }
                    }
                    pt_t = self.pt_ts[pt_t].next;
                    if pt_t == stop_pt_t {
                        break;
                    }
                }
                if self.angle_loop_count(base_angle) == 1 {
                    self.spans[span].from_angle = None;
                    if to_angle.is_some() {
                        self.spans[span].to_angle = None;
                    }
                }
            }
            if self.span_final(span) {
                break;
            }
            match self.spans[span].next {
                Some(n) => span = n,
                None => break,
            }
        }
        true
    }

    // ========================================================================
    // Test for coincidence / undone / winding updates
    // ========================================================================

    pub fn seg_test_for_coincidence(
        &self,
        seg: SegmentId,
        prior_pt_t: PtTId,
        pt_t: PtTId,
        prior: SpanId,
        span_base: SpanId,
        opp: SegmentId,
    ) -> bool {
        let mid_t = (self.span_t(prior) + self.span_t(span_base)) / 2.0;
        let mid_pt = self.seg_pt_at_t(seg, mid_t);
        let mut coincident = true;
        if !DPoint::approximately_equal_pts(self.pt_ts[prior_pt_t].pt, mid_pt)
            && !DPoint::approximately_equal_pts(self.pt_ts[pt_t].pt, mid_pt)
        {
            if self.pt_ts[prior_pt_t].span == self.pt_ts[pt_t].span {
                return false;
            }
            coincident = false;
            let mut i = Intersections::new();
            let mut curve_part = DCurve::default();
            self.seg_sub_divide(seg, prior, span_base, &mut curve_part);
            let verb = self.segments[seg].verb;
            let dxdy = curve_part.dxdy_at_t(verb, 0.5);
            let part_mid_pt = curve_part.pt_at_t(verb, 0.5);
            let ray = DLine::new(
                DPoint::from_point(mid_pt),
                DPoint::new(part_mid_pt.x + dxdy.y, part_mid_pt.y - dxdy.x),
            );
            let mut opp_part = DCurve::default();
            self.seg_sub_divide(
                opp,
                self.pt_ts[prior_pt_t].span,
                self.pt_ts[pt_t].span,
                &mut opp_part,
            );
            opp_part.intersect_ray(self.segments[opp].verb, &ray, &mut i);
            for index in 0..i.used() {
                if !between(0.0, i.t[0][index], 1.0) {
                    continue;
                }
                let opp_pt = i.pt[index];
                if opp_pt.approximately_dequal_pt(mid_pt) {
                    coincident = true;
                }
            }
        }
        coincident
    }

    pub fn seg_undone_span(&self, seg: SegmentId) -> Option<SpanId> {
        let mut span = self.segments[seg].head;
        loop {
            let next = self.spans[span].next?;
            if !self.spans[span].done {
                return Some(span);
            }
            if self.span_final(next) {
                break;
            }
            span = next;
        }
        None
    }

    pub fn seg_update_opp_winding(&self, start: SpanId, end: SpanId) -> i32 {
        let lesser = self.span_starter(start, end);
        let mut opp_winding = self.spans[lesser].opp_sum;
        let opp_span_winding = self.seg_opp_sign(start, end);
        if opp_span_winding != 0
            && Self::seg_use_inner_winding(opp_winding.wrapping_sub(opp_span_winding), opp_winding)
            && opp_winding != SK_MAX_S32
        {
            opp_winding = opp_winding.wrapping_sub(opp_span_winding);
        }
        opp_winding
    }

    pub fn seg_update_opp_winding_angle(&self, angle: AngleId) -> i32 {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.seg_update_opp_winding(end, start)
    }

    pub fn seg_update_opp_winding_reverse(&self, angle: AngleId) -> i32 {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.seg_update_opp_winding(start, end)
    }

    pub fn seg_update_winding(&mut self, start: SpanId, end: SpanId) -> i32 {
        let lesser = self.span_starter(start, end);
        let mut winding = self.spans[lesser].wind_sum;
        if winding == SK_MIN_S32 {
            winding = self.span_compute_wind_sum(lesser);
        }
        if winding == SK_MIN_S32 {
            return winding;
        }
        let span_winding = self.seg_span_sign(start, end);
        if winding != 0
            && Self::seg_use_inner_winding(winding.wrapping_sub(span_winding), winding)
            && winding != SK_MAX_S32
        {
            winding = winding.wrapping_sub(span_winding);
        }
        winding
    }

    pub fn seg_update_winding_angle(&mut self, angle: AngleId) -> i32 {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.seg_update_winding(end, start)
    }

    pub fn seg_update_winding_reverse(&mut self, angle: AngleId) -> i32 {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.seg_update_winding(start, end)
    }
}
