#![forbid(unsafe_code)]

//! Port of `SkOpAngle.h/.cpp`.

use skia_rs_path::types::Verb;

use crate::arena::{AngleId, OpState, SegmentId, SpanId};
use crate::cubic::DCubic;
use crate::curve::{curve_dpoint_at_t, curve_dslope_at_t, curve_intersect_ray};
use crate::intersections::Intersections;
use crate::line::{DLine, LineParameters};
use crate::types::{
    almost_bequal_ulps, almost_equal_ulps, approximately_between_orderable,
    approximately_equal, approximately_equal_orderable, approximately_zero, between, cpp_max_f64,
    cpp_min_f64, pathops_verb_to_points, DPoint, DVector,
};

impl OpState {
    #[inline]
    pub fn angle_segment(&self, angle: AngleId) -> SegmentId {
        self.spans[self.angles[angle].start.unwrap()].segment
    }

    #[inline]
    pub fn angle_starter(&self, angle: AngleId) -> SpanId {
        let start = self.angles[angle].start.unwrap();
        let end = self.angles[angle].end.unwrap();
        self.span_starter(start, end)
    }

    pub fn angle_set(&mut self, angle: AngleId, start: SpanId, end: SpanId) {
        self.angles[angle].start = Some(start);
        self.angles[angle].end = Some(end);
        self.angles[angle].computed_end = Some(end);
        self.angles[angle].next = None;
        self.angles[angle].compute_sector = false;
        self.angles[angle].computed_sector = false;
        self.angles[angle].check_coincidence = false;
        self.angles[angle].tangents_ambiguous = false;
        self.angle_set_spans(angle);
        self.angle_set_sector(angle);
    }

    fn angle_set_spans(&mut self, angle: AngleId) {
        self.angles[angle].unorderable = false;
        self.angles[angle].last_marked = None;
        let start = match self.angles[angle].start {
            Some(s) => s,
            None => {
                self.angles[angle].unorderable = true;
                return;
            }
        };
        let end = self.angles[angle].end.unwrap();
        let seg_id = self.spans[start].segment;
        let pts = self.segments[seg_id].pts;
        let verb = self.segments[seg_id].verb;
        let weight = self.segments[seg_id].weight;

        let mut curve = self.angles[angle].part.curve;
        self.seg_sub_divide(seg_id, start, end, &mut curve);
        self.angles[angle].part.curve = curve;
        self.angles[angle].original_curve_part = curve;
        self.angles[angle].part.set_curve_hull_sweep(verb);

        if verb != Verb::Line && !self.angles[angle].part.is_curve {
            let last_pt = self.angles[angle].part.curve.pts[pathops_verb_to_points(verb)];
            self.angles[angle].part.curve.pts[1] = last_pt;
            self.angles[angle].original_curve_part.pts[1] = last_pt;
            let line_half = DLine::new(
                DPoint::from_point(self.angles[angle].part.curve.pts[0].as_point()),
                DPoint::from_point(last_pt.as_point()),
            );
            self.angles[angle].tangent_half.line_end_points(&line_half);
            self.angles[angle].side = 0.0;
        }
        match verb {
            Verb::Line => {
                let c_p1 = pts[usize::from(self.span_t(start) < self.span_t(end))];
                let line_half = DLine::new(
                    DPoint::from_point(self.span_pt(start)),
                    DPoint::from_point(c_p1),
                );
                self.angles[angle].tangent_half.line_end_points(&line_half);
                self.angles[angle].side = 0.0;
            }
            Verb::Quad | Verb::Conic => {
                let mut tangent_part = LineParameters::default();
                let q = self.angles[angle].part.curve.as_quad();
                tangent_part.quad_end_points_all(&q);
                self.angles[angle].side =
                    -tangent_part.point_distance(self.angles[angle].part.curve.pts[2]);
            }
            Verb::Cubic => {
                let mut tangent_part = LineParameters::default();
                let c = self.angles[angle].part.curve.as_cubic();
                tangent_part.cubic_part(&c);
                let mut test_ts = [0.0f64; 4];
                let mut inf = [0.0f64; 2];
                let d_cubic = DCubic::from_points(pts);
                let mut test_count = d_cubic.find_inflections(&mut inf);
                test_ts[..test_count].copy_from_slice(&inf[..test_count]);
                let start_t = self.span_t(start);
                let end_t = self.span_t(end);
                for v in test_ts.iter_mut().take(test_count) {
                    if !between(start_t, *v, end_t) {
                        *v = -1.0;
                    }
                }
                test_ts[test_count] = start_t;
                test_count += 1;
                test_ts[test_count] = end_t;
                test_count += 1;
                test_ts[..test_count].sort_by(|a, b| {
                    a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal)
                });
                let mut best_side = 0.0f64;
                let test_cases = (test_count << 1) - 1;
                let mut index = 0usize;
                while test_ts[index] < 0.0 {
                    index += 1;
                }
                index <<= 1;
                let mut test_part = LineParameters::default();
                test_part.cubic_end_points_all(&c);
                while index < test_cases {
                    let test_index = index >> 1;
                    let mut test_t = test_ts[test_index];
                    if (index & 1) != 0 {
                        test_t = (test_t + test_ts[test_index + 1]) / 2.0;
                    }
                    let pt = curve_dpoint_at_t(&pts, weight, Verb::Cubic, test_t);
                    let test_side = test_part.point_distance(pt);
                    if best_side.abs() < test_side.abs() {
                        best_side = test_side;
                    }
                    index += 1;
                }
                self.angles[angle].side = -best_side;
            }
            _ => {}
        }
    }

    fn angle_find_sector(verb: Verb, x: f64, y: f64) -> i8 {
        let abs_x = x.abs();
        let abs_y = y.abs();
        let xy = if verb == Verb::Line || !almost_equal_ulps(abs_x, abs_y) {
            abs_x - abs_y
        } else {
            0.0
        };
        const SEDECIMANT: [[[i8; 3]; 3]; 3] = [
            [[4, 3, 2], [7, -1, 15], [10, 11, 12]],
            [[5, -1, 1], [-1, -1, -1], [9, -1, 13]],
            [[6, 3, 0], [7, -1, 15], [8, 11, 14]],
        ];
        let i0 = usize::from(xy >= 0.0) + usize::from(xy > 0.0);
        let i1 = usize::from(y >= 0.0) + usize::from(y > 0.0);
        let i2 = usize::from(x >= 0.0) + usize::from(x > 0.0);
        SEDECIMANT[i0][i1][i2] * 2 + 1
    }

    #[inline]
    fn angle_check_crosses_zero(&self, angle: AngleId) -> bool {
        let ss = self.angles[angle].sector_start;
        let se = self.angles[angle].sector_end;
        let start = ss.min(se);
        let end = ss.max(se);
        (end - start) > 16
    }

    fn angle_set_sector(&mut self, angle: AngleId) {
        let start = match self.angles[angle].start {
            Some(s) => s,
            None => {
                self.angles[angle].unorderable = true;
                return;
            }
        };
        let verb = self.segments[self.spans[start].segment].verb;
        let sw0 = self.angles[angle].part.sweep[0];
        let sec_start = Self::angle_find_sector(verb, sw0.x, sw0.y);
        self.angles[angle].sector_start = sec_start;
        if sec_start < 0 {
            self.angles[angle].sector_start = -1;
            self.angles[angle].sector_end = -1;
            self.angles[angle].sector_mask = 0;
            self.angles[angle].compute_sector = true;
            return;
        }
        if !self.angles[angle].part.is_curve {
            self.angles[angle].sector_end = sec_start;
            self.angles[angle].sector_mask = 1u32 << (sec_start as u32);
            return;
        }
        let sw1 = self.angles[angle].part.sweep[1];
        let sec_end = Self::angle_find_sector(verb, sw1.x, sw1.y);
        self.angles[angle].sector_end = sec_end;
        if sec_end < 0 {
            self.angles[angle].sector_start = -1;
            self.angles[angle].sector_end = -1;
            self.angles[angle].sector_mask = 0;
            self.angles[angle].compute_sector = true;
            return;
        }
        if sec_end == sec_start && (sec_start & 3) != 3 {
            self.angles[angle].sector_mask = 1u32 << (sec_start as u32);
            return;
        }
        let mut crosses_zero = self.angle_check_crosses_zero(angle);
        let mut s_min = sec_start.min(sec_end);
        let curve_bends_ccw = (sec_start == s_min) ^ crosses_zero;
        if (self.angles[angle].sector_start & 3) == 3 {
            self.angles[angle].sector_start =
                (self.angles[angle].sector_start + if curve_bends_ccw { 1 } else { 31 }) & 0x1f;
        }
        if (self.angles[angle].sector_end & 3) == 3 {
            self.angles[angle].sector_end =
                (self.angles[angle].sector_end + if curve_bends_ccw { 31 } else { 1 }) & 0x1f;
        }
        crosses_zero = self.angle_check_crosses_zero(angle);
        s_min = self.angles[angle].sector_start.min(self.angles[angle].sector_end);
        let s_max = self.angles[angle].sector_start.max(self.angles[angle].sector_end);
        if !crosses_zero {
            let shift = (31 - s_max + s_min) as u32;
            self.angles[angle].sector_mask = (u32::MAX >> shift) << (s_min as u32);
        } else {
            self.angles[angle].sector_mask =
                (u32::MAX >> (31 - s_min) as u32) | (u32::MAX << (s_max as u32));
        }
    }

    fn angle_compute_sector(&mut self, angle: AngleId) -> bool {
        if self.angles[angle].computed_sector {
            return !self.angles[angle].unorderable;
        }
        self.angles[angle].computed_sector = true;
        let f_start = self.angles[angle].start.unwrap();
        let f_end = self.angles[angle].end.unwrap();
        let step_up = self.span_t(f_start) < self.span_t(f_end);
        let mut check_end = Some(f_end);
        if self.span_final(f_end) && step_up {
            self.angles[angle].unorderable = true;
            return false;
        }
        let my_seg = self.spans[f_start].segment;
        'outer: while let Some(ce) = check_end {
            let other = self.spans[ce].segment;
            let mut o_span = Some(self.segments[other].head);
            while let Some(os) = o_span {
                if self.spans[os].segment == my_seg
                    && os != ce
                    && approximately_equal(self.span_t(os), self.span_t(ce))
                {
                    break 'outer;
                }
                if self.span_final(os) {
                    break;
                }
                o_span = self.spans[os].next;
            }
            check_end = if step_up {
                if !self.span_final(ce) {
                    self.spans[ce].next
                } else {
                    None
                }
            } else {
                self.spans[ce].prev
            };
        }
        let end_seg = self.spans[f_end].segment;
        let computed_end = if step_up {
            match check_end {
                Some(ce) => self.spans[ce].prev,
                None => Some(self.segments[end_seg].head),
            }
        } else {
            match check_end {
                Some(ce) => self.spans[ce].next,
                None => Some(self.segments[end_seg].tail),
            }
        };
        let computed_end = match computed_end {
            Some(ce) => ce,
            None => {
                self.angles[angle].unorderable = true;
                return false;
            }
        };
        if check_end == Some(f_end) || computed_end == f_end || computed_end == f_start {
            self.angles[angle].unorderable = true;
            return false;
        }
        if step_up != (self.span_t(f_start) < self.span_t(computed_end)) {
            self.angles[angle].unorderable = true;
            return false;
        }
        let save_end = self.angles[angle].end;
        self.angles[angle].computed_end = Some(computed_end);
        self.angles[angle].end = Some(computed_end);
        self.angle_set_spans(angle);
        self.angle_set_sector(angle);
        self.angles[angle].end = save_end;
        !self.angles[angle].unorderable
    }

    pub fn angle_after(&mut self, this: AngleId, test: AngleId) -> bool {
        let lh = test;
        let rh = self.angles[lh].next.unwrap();
        self.angles[this].part.curve = self.angles[this].original_curve_part;
        let origin = self.angles[this].part.curve.pts[0];
        self.angles[lh].part.curve = self.angles[lh].original_curve_part;
        self.angles[lh].part.curve.pts[0] = origin;
        self.angles[rh].part.curve = self.angles[rh].original_curve_part;
        self.angles[rh].part.curve.pts[0] = origin;

        if self.angles[lh].compute_sector && !self.angle_compute_sector(lh) {
            return true;
        }
        if self.angles[this].compute_sector && !self.angle_compute_sector(this) {
            return true;
        }
        if self.angles[rh].compute_sector && !self.angle_compute_sector(rh) {
            return true;
        }
        let lh_mask = self.angles[lh].sector_mask;
        let rh_mask = self.angles[rh].sector_mask;
        let this_mask = self.angles[this].sector_mask;
        let ltr_overlap = ((lh_mask | rh_mask) & this_mask) != 0;
        let lr_overlap = (lh_mask & rh_mask) != 0;
        let lr_order: i32;
        if !lr_overlap {
            if !ltr_overlap {
                let lh_end = self.angles[lh].sector_end;
                let rh_start = self.angles[rh].sector_start;
                let this_start = self.angles[this].sector_start;
                return (lh_end > rh_start) ^ (this_start > lh_end) ^ (this_start > rh_start);
            }
            let lr_gap =
                (self.angles[rh].sector_start as i32 - self.angles[lh].sector_start as i32 + 32)
                    & 0x1f;
            lr_order = if lr_gap > 20 {
                0
            } else if lr_gap > 11 {
                -1
            } else {
                1
            };
        } else {
            lr_order = self.angle_orderable(lh, rh);
            if !ltr_overlap && lr_order >= 0 {
                return lr_order == 0;
            }
        }
        let mut lt_order = if (lh_mask & this_mask) != 0 {
            self.angle_orderable(lh, this)
        } else {
            let lt_gap =
                (self.angles[this].sector_start as i32 - self.angles[lh].sector_start as i32 + 32)
                    & 0x1f;
            if lt_gap > 20 {
                0
            } else if lt_gap > 11 {
                -1
            } else {
                1
            }
        };
        let mut tr_order = if (rh_mask & this_mask) != 0 {
            self.angle_orderable(this, rh)
        } else {
            let tr_gap =
                (self.angles[rh].sector_start as i32 - self.angles[this].sector_start as i32 + 32)
                    & 0x1f;
            if tr_gap > 20 {
                0
            } else if tr_gap > 11 {
                -1
            } else {
                1
            }
        };
        self.angle_alignment_same_side(this, lh, &mut lt_order);
        self.angle_alignment_same_side(this, rh, &mut tr_order);
        if lr_order >= 0 && lt_order >= 0 && tr_order >= 0 {
            return if lr_order != 0 {
                (lt_order & tr_order) != 0
            } else {
                (lt_order | tr_order) != 0
            };
        }
        if lt_order == 0 && lr_order == 0 {
            return self.angle_opposite_planes(lh, this);
        } else if lt_order == 1 && tr_order == 0 {
            return self.angle_opposite_planes(this, rh);
        } else if lr_order == 1 && tr_order == 1 {
            return self.angle_opposite_planes(lh, rh);
        }
        if self.angles[this].unorderable
            || self.angles[lh].unorderable
            || self.angles[rh].unorderable
        {
            if !self.angles[this].part.is_curve
                && !self.angles[lh].part.is_curve
                && !self.angles[rh].part.is_curve
            {
                let lt_share = i32::from(
                    self.angles[lh].original_curve_part.pts[0]
                        == self.angles[this].original_curve_part.pts[0],
                );
                let lr_share = i32::from(
                    self.angles[lh].original_curve_part.pts[0]
                        == self.angles[rh].original_curve_part.pts[0],
                );
                let tr_share = i32::from(
                    self.angles[this].original_curve_part.pts[0]
                        == self.angles[rh].original_curve_part.pts[0],
                );
                if lt_share + lr_share + tr_share == 1 {
                    if lr_share != 0 {
                        let lt_o = self.angle_lines_on_original_side(lh, this);
                        let rt_o = self.angle_lines_on_original_side(rh, this);
                        if (rt_o ^ lt_o) == 1 {
                            return lt_o != 0;
                        }
                    } else if tr_share != 0 {
                        let tl_o = self.angle_lines_on_original_side(this, lh);
                        let rl_o = self.angle_lines_on_original_side(rh, lh);
                        if (tl_o ^ rl_o) == 1 {
                            return rl_o != 0;
                        }
                    } else {
                        let tr_o = self.angle_lines_on_original_side(rh, this);
                        let lr_o = self.angle_lines_on_original_side(lh, rh);
                        if (lr_o ^ tr_o) == 1 {
                            return tr_o != 0;
                        }
                    }
                }
            }
        }
        if lr_order < 0 {
            if lt_order < 0 {
                return tr_order != 0;
            }
            return lt_order != 0;
        }
        lr_order == 0
    }

    fn angle_line_on_one_side_raw(
        &self,
        origin: DPoint,
        line: DVector,
        test: AngleId,
        use_original: bool,
    ) -> i32 {
        let mut crosses = [0.0f64; 3];
        let test_verb = self.segments[self.angle_segment(test)].verb;
        let i_max = pathops_verb_to_points(test_verb);
        let test_curve = if use_original {
            &self.angles[test].original_curve_part
        } else {
            &self.angles[test].part.curve
        };
        for index in 1..=i_max {
            let xy1 = line.x * (test_curve.pts[index].y - origin.y);
            let xy2 = line.y * (test_curve.pts[index].x - origin.x);
            crosses[index - 1] = if almost_bequal_ulps(xy1, xy2) {
                0.0
            } else {
                xy1 - xy2
            };
        }
        if crosses[0] * crosses[1] < 0.0 {
            return -1;
        }
        if test_verb == Verb::Cubic
            && (crosses[0] * crosses[2] < 0.0 || crosses[1] * crosses[2] < 0.0)
        {
            return -1;
        }
        if crosses[0] != 0.0 {
            return i32::from(crosses[0] < 0.0);
        }
        if crosses[1] != 0.0 {
            return i32::from(crosses[1] < 0.0);
        }
        if test_verb == Verb::Cubic && crosses[2] != 0.0 {
            return i32::from(crosses[2] < 0.0);
        }
        -2
    }

    pub fn angle_line_on_one_side(
        &mut self,
        this: AngleId,
        test: AngleId,
        use_original: bool,
    ) -> i32 {
        let origin = self.angles[this].part.curve.pts[0];
        let line = self.angles[this].part.curve.pts[1].sub(origin);
        let mut result = self.angle_line_on_one_side_raw(origin, line, test, use_original);
        if result == -2 {
            self.angles[this].unorderable = true;
            result = -1;
        }
        result
    }

    fn angle_lines_on_original_side(&mut self, this: AngleId, test: AngleId) -> i32 {
        let origin = self.angles[this].original_curve_part.pts[0];
        let line = self.angles[this].original_curve_part.pts[1].sub(origin);
        let mut dots = [0.0f64; 2];
        let mut crosses = [0.0f64; 2];
        let test_curve = self.angles[test].original_curve_part;
        for index in 0..2 {
            let test_line = test_curve.pts[index].sub(origin);
            let xy1 = line.x * test_line.y;
            let xy2 = line.y * test_line.x;
            dots[index] = line.x * test_line.x + line.y * test_line.y;
            crosses[index] = if almost_bequal_ulps(xy1, xy2) {
                0.0
            } else {
                xy1 - xy2
            };
        }
        if crosses[0] * crosses[1] < 0.0 {
            return -1;
        }
        if crosses[0] != 0.0 {
            return i32::from(crosses[0] < 0.0);
        }
        if crosses[1] != 0.0 {
            return i32::from(crosses[1] < 0.0);
        }
        if (dots[0] == 0.0 && dots[1] < 0.0) || (dots[0] < 0.0 && dots[1] == 0.0) {
            return 2;
        }
        self.angles[this].unorderable = true;
        -1
    }

    fn angle_alignment_same_side(&self, this: AngleId, test: AngleId, order: &mut i32) {
        if *order < 0 || self.angles[this].part.is_curve || self.angles[test].part.is_curve {
            return;
        }
        let x_origin = self.angles[test].part.curve.pts[0];
        let o_origin = self.angles[test].original_curve_part.pts[0];
        if x_origin == o_origin {
            return;
        }
        let i_max = pathops_verb_to_points(self.segments[self.angle_segment(this)].verb);
        let x_line = self.angles[test].part.curve.pts[1].sub(x_origin);
        let o_line = self.angles[test].original_curve_part.pts[1].sub(o_origin);
        for index in 1..=i_max {
            let test_pt = self.angles[this].part.curve.pts[index];
            let x_cross = o_line.cross_check(test_pt.sub(x_origin));
            let o_cross = x_line.cross_check(test_pt.sub(o_origin));
            if o_cross * x_cross < 0.0 {
                *order ^= 1;
                break;
            }
        }
    }

    fn angle_mid_t(&self, angle: AngleId) -> f64 {
        (self.span_t(self.angles[angle].start.unwrap())
            + self.span_t(self.angles[angle].end.unwrap()))
            / 2.0
    }

    fn angle_dist_end_ratio(&self, angle: AngleId, dist: f64) -> f64 {
        let mut longest = 0.0f64;
        let seg = &self.segments[self.angle_segment(angle)];
        let pt_count = pathops_verb_to_points(seg.verb);
        for idx1 in 0..pt_count {
            for idx2 in (idx1 + 1)..=pt_count {
                let v = DVector::from_vector(seg.pts[idx2] - seg.pts[idx1]);
                let len_sq = v.length_squared();
                longest = cpp_max_f64(longest, len_sq);
            }
        }
        longest.sqrt() / dist
    }

    fn angle_tangents_diverge(&mut self, this: AngleId, rh: AngleId, s0xt0: f64) -> bool {
        if s0xt0 == 0.0 {
            return false;
        }
        let sweep0 = self.angles[this].part.sweep[0];
        let tweep0 = self.angles[rh].part.sweep[0];
        let s0dt0 = sweep0.dot(tweep0);
        if s0dt0 == 0.0 {
            return true;
        }
        let m = s0xt0 / s0dt0;
        let s_dist = sweep0.length() * m;
        let t_dist = tweep0.length() * m;
        let use_s = s_dist.abs() < t_dist.abs();
        let m_factor = if use_s {
            self.angle_dist_end_ratio(this, s_dist)
        } else {
            self.angle_dist_end_ratio(rh, t_dist)
        }
        .abs();
        self.angles[this].tangents_ambiguous = (50.0..200.0).contains(&m_factor);
        m_factor < 50.0
    }

    fn angle_end_to_side(&self, this: AngleId, rh: AngleId) -> Option<bool> {
        let seg_id = self.angle_segment(this);
        let seg = &self.segments[seg_id];
        let end_span = self.angles[this].end.unwrap();
        let ray0 = DPoint::from_point(self.span_pt(end_span));
        let slope_at_end = curve_dslope_at_t(&seg.pts, seg.weight, seg.verb, self.span_t(end_span));
        let ray_end = DLine::new(
            ray0,
            DPoint::new(ray0.x + slope_at_end.y, ray0.y - slope_at_end.x),
        );
        let mut i_end = Intersections::new();
        let opp_seg_id = self.angle_segment(rh);
        let opp_seg = &self.segments[opp_seg_id];
        curve_intersect_ray(&opp_seg.pts, opp_seg.weight, opp_seg.verb, &ray_end, &mut i_end);
        let mut end_dist = 0.0;
        let closest_end = i_end.closest_to(
            self.span_t(self.angles[rh].start.unwrap()),
            self.span_t(self.angles[rh].end.unwrap()),
            ray0,
            &mut end_dist,
        );
        if closest_end < 0 || end_dist == 0.0 {
            return None;
        }
        let start = DPoint::from_point(self.span_pt(self.angles[this].start.unwrap()));
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let curve = &self.angles[rh].part.curve;
        let opp_pts = pathops_verb_to_points(opp_seg.verb);
        for idx2 in 0..=opp_pts {
            min_x = cpp_min_f64(min_x, curve.pts[idx2].x);
            min_y = cpp_min_f64(min_y, curve.pts[idx2].y);
            max_x = cpp_max_f64(max_x, curve.pts[idx2].x);
            max_y = cpp_max_f64(max_y, curve.pts[idx2].y);
        }
        let max_width = cpp_max_f64(max_x - min_x, max_y - min_y);
        end_dist /= max_width;
        if !(end_dist >= 5e-12) {
            return None;
        }
        let opp_pt = i_end.pt[closest_end as usize];
        let v_left = ray0.sub(start);
        let v_right = opp_pt.sub(start);
        let dir = v_left.cross_no_normal_check(v_right);
        if dir == 0.0 {
            return None;
        }
        Some(dir < 0.0)
    }

    fn angle_mid_to_side(&self, this: AngleId, rh: AngleId) -> Option<bool> {
        let seg_id = self.angle_segment(this);
        let seg = &self.segments[seg_id];
        let start_pt = self.span_pt(self.angles[this].start.unwrap());
        let end_pt = self.span_pt(self.angles[this].end.unwrap());
        let d_start_pt = DPoint::from_point(start_pt);
        let mx = ((start_pt.x + end_pt.x) / 2.0) as f64;
        let my = ((start_pt.y + end_pt.y) / 2.0) as f64;
        let ray_mid = DLine::new(
            DPoint::new(mx, my),
            DPoint::new(
                mx + (end_pt.y - start_pt.y) as f64,
                my - (end_pt.x - start_pt.x) as f64,
            ),
        );
        let mut i_mid = Intersections::new();
        curve_intersect_ray(&seg.pts, seg.weight, seg.verb, &ray_mid, &mut i_mid);
        let i_outside = i_mid.most_outside(
            self.span_t(self.angles[this].start.unwrap()),
            self.span_t(self.angles[this].end.unwrap()),
            d_start_pt,
        );
        if i_outside < 0 {
            return None;
        }
        let opp_seg_id = self.angle_segment(rh);
        let opp_seg = &self.segments[opp_seg_id];
        let mut opp_mid = Intersections::new();
        curve_intersect_ray(&opp_seg.pts, opp_seg.weight, opp_seg.verb, &ray_mid, &mut opp_mid);
        let opp_outside = opp_mid.most_outside(
            self.span_t(self.angles[rh].start.unwrap()),
            self.span_t(self.angles[rh].end.unwrap()),
            d_start_pt,
        );
        if opp_outside < 0 {
            return None;
        }
        let i_side = i_mid.pt[i_outside as usize].sub(d_start_pt);
        let opp_side = opp_mid.pt[opp_outside as usize].sub(d_start_pt);
        let dir = i_side.cross_check(opp_side);
        if dir == 0.0 {
            return None;
        }
        Some(dir < 0.0)
    }

    fn angle_check_parallel(&mut self, this: AngleId, rh: AngleId) -> bool {
        let sweep = if self.angles[this].part.ordered {
            self.angles[this].part.sweep[0]
        } else {
            self.angles[this]
                .part
                .curve
                .pts[1]
                .sub(self.angles[this].part.curve.pts[0])
        };
        let tweep = if self.angles[rh].part.ordered {
            self.angles[rh].part.sweep[0]
        } else {
            self.angles[rh]
                .part
                .curve
                .pts[1]
                .sub(self.angles[rh].part.curve.pts[0])
        };
        let s0xt0 = sweep.cross_check(tweep);
        if self.angle_tangents_diverge(this, rh, s0xt0) {
            return s0xt0 < 0.0;
        }
        if !self.span_contains_span(
            self.angles[this].end.unwrap(),
            self.angles[rh].end.unwrap(),
        ) {
            if let Some(inside) = self.angle_end_to_side(this, rh) {
                return inside;
            }
            if let Some(inside) = self.angle_end_to_side(rh, this) {
                return !inside;
            }
        }
        if let Some(inside) = self.angle_mid_to_side(this, rh) {
            return inside;
        }
        if let Some(inside) = self.angle_mid_to_side(rh, this) {
            return !inside;
        }
        let m0 = self
            .seg_d_pt_at_t(self.angle_segment(this), self.angle_mid_t(this))
            .sub(self.angles[this].part.curve.pts[0]);
        let m1 = self
            .seg_d_pt_at_t(self.angle_segment(rh), self.angle_mid_t(rh))
            .sub(self.angles[rh].part.curve.pts[0]);
        let m0xm1 = m0.cross_check(m1);
        if m0xm1 == 0.0 {
            self.angles[this].unorderable = true;
            self.angles[rh].unorderable = true;
            return true;
        }
        m0xm1 < 0.0
    }

    pub fn angle_convex_hull_overlaps(&mut self, this: AngleId, rh: AngleId) -> i32 {
        let sweep = self.angles[this].part.sweep;
        let tweep = self.angles[rh].part.sweep;
        let s0xs1 = sweep[0].cross_check(sweep[1]);
        let s0xt0 = sweep[0].cross_check(tweep[0]);
        let s1xt0 = sweep[1].cross_check(tweep[0]);
        let mut t_between_s = if s0xs1 > 0.0 {
            s0xt0 > 0.0 && s1xt0 < 0.0
        } else {
            s0xt0 < 0.0 && s1xt0 > 0.0
        };
        let s0xt1 = sweep[0].cross_check(tweep[1]);
        let s1xt1 = sweep[1].cross_check(tweep[1]);
        t_between_s |= if s0xs1 > 0.0 {
            s0xt1 > 0.0 && s1xt1 < 0.0
        } else {
            s0xt1 < 0.0 && s1xt1 > 0.0
        };
        let t0xt1 = tweep[0].cross_check(tweep[1]);
        if t_between_s {
            return -1;
        }
        if (s0xt0 == 0.0 && s1xt1 == 0.0) || (s1xt0 == 0.0 && s0xt1 == 0.0) {
            return -1;
        }
        let mut s_between_t = if t0xt1 > 0.0 {
            s0xt0 < 0.0 && s0xt1 > 0.0
        } else {
            s0xt0 > 0.0 && s0xt1 < 0.0
        };
        s_between_t |= if t0xt1 > 0.0 {
            s1xt0 < 0.0 && s1xt1 > 0.0
        } else {
            s1xt0 > 0.0 && s1xt1 < 0.0
        };
        if s_between_t {
            return -1;
        }
        if s0xt0 >= 0.0 && s0xt1 >= 0.0 && s1xt0 >= 0.0 && s1xt1 >= 0.0 {
            return 0;
        }
        if s0xt0 <= 0.0 && s0xt1 <= 0.0 && s1xt0 <= 0.0 && s1xt1 <= 0.0 {
            return 1;
        }
        let m0 = self
            .seg_d_pt_at_t(self.angle_segment(this), self.angle_mid_t(this))
            .sub(self.angles[this].part.curve.pts[0]);
        let m1 = self
            .seg_d_pt_at_t(self.angle_segment(rh), self.angle_mid_t(rh))
            .sub(self.angles[rh].part.curve.pts[0]);
        let m0xm1 = m0.cross_check(m1);
        if s0xt0 > 0.0 && m0xm1 > 0.0 {
            return 0;
        }
        if s0xt0 < 0.0 && m0xm1 < 0.0 {
            return 1;
        }
        if self.angle_tangents_diverge(this, rh, s0xt0) {
            return i32::from(s0xt0 < 0.0);
        }
        i32::from(m0xm1 < 0.0)
    }

    pub fn angle_ends_intersect(&mut self, this: AngleId, rh: AngleId) -> bool {
        let l_seg_id = self.angle_segment(this);
        let r_seg_id = self.angle_segment(rh);
        let l_verb = self.segments[l_seg_id].verb;
        let r_verb = self.segments[r_seg_id].verb;
        let l_pts = pathops_verb_to_points(l_verb);
        let r_pts = pathops_verb_to_points(r_verb);
        let rays = [
            DLine::new(
                self.angles[this].part.curve.pts[0],
                self.angles[rh].part.curve.pts[r_pts],
            ),
            DLine::new(
                self.angles[this].part.curve.pts[0],
                self.angles[this].part.curve.pts[l_pts],
            ),
        ];
        if self.span_contains_span(
            self.angles[this].end.unwrap(),
            self.angles[rh].end.unwrap(),
        ) {
            return self.angle_check_parallel(this, rh);
        }
        let mut small_ts = [-1.0f64; 2];
        let mut limited = [false; 2];
        for index in 0..2 {
            let c_verb = if index != 0 { r_verb } else { l_verb };
            if c_verb == Verb::Line {
                continue;
            }
            let seg = &self.segments[if index != 0 { r_seg_id } else { l_seg_id }];
            let mut i = Intersections::new();
            curve_intersect_ray(&seg.pts, seg.weight, c_verb, &rays[index], &mut i);
            let a = if index != 0 { rh } else { this };
            let t_start = self.span_t(self.angles[a].start.unwrap());
            let t_end = self.span_t(self.angles[a].computed_end.unwrap());
            let test_ascends = t_start < t_end;
            let mut t = if test_ascends { 0.0 } else { 1.0 };
            for idx2 in 0..i.used() {
                let test_t = i.t[0][idx2];
                if !approximately_between_orderable(t_start, test_t, t_end) {
                    continue;
                }
                if approximately_equal_orderable(t_start, test_t) {
                    continue;
                }
                t = if test_ascends {
                    cpp_max_f64(t, test_t)
                } else {
                    cpp_min_f64(t, test_t)
                };
                small_ts[index] = t;
                limited[index] = approximately_equal_orderable(t, t_end);
            }
        }
        let mut s_ray_longer = false;
        let mut s_cept = DVector::default();
        let mut s_cept_t = -1.0;
        let mut s_index: i32 = -1;
        let mut use_intersect = false;
        for index in 0..2 {
            if small_ts[index] < 0.0 {
                continue;
            }
            let seg_id = if index != 0 { r_seg_id } else { l_seg_id };
            let d_pt = self.seg_d_pt_at_t(seg_id, small_ts[index]);
            let cept = d_pt.sub(rays[index].pts[0]);
            if (if index != 0 { l_pts } else { r_pts }) == 1 {
                let total = rays[index].pts[1].sub(rays[index].pts[0]);
                if cept.length_squared() * 2.0 < total.length_squared() {
                    continue;
                }
            }
            let end = rays[index].pts[1].sub(rays[index].pts[0]);
            if cept.x * end.x < 0.0 || cept.y * end.y < 0.0 {
                continue;
            }
            let ray_dist = cept.length();
            let end_dist = end.length();
            let ray_longer = ray_dist > end_dist;
            if limited[0] && limited[1] && ray_longer {
                use_intersect = true;
                s_ray_longer = ray_longer;
                s_cept = cept;
                s_cept_t = small_ts[index];
                s_index = index as i32;
                break;
            }
            let mut delta = (ray_dist - end_dist).abs();
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            let curve = if index != 0 {
                &self.angles[rh].part.curve
            } else {
                &self.angles[this].part.curve
            };
            let pt_count = if index != 0 { r_pts } else { l_pts };
            for idx2 in 0..=pt_count {
                min_x = cpp_min_f64(min_x, curve.pts[idx2].x);
                min_y = cpp_min_f64(min_y, curve.pts[idx2].y);
                max_x = cpp_max_f64(max_x, curve.pts[idx2].x);
                max_y = cpp_max_f64(max_y, curve.pts[idx2].y);
            }
            let max_width = cpp_max_f64(max_x - min_x, max_y - min_y);
            delta /= max_width;
            if delta < 4e-3
                && delta > 1e-3
                && !use_intersect
                && self.angles[this].part.is_curve
                && self.angles[rh].part.is_curve
                && self.angles[this].original_curve_part.pts[0] != self.angles[this].part.curve.pts[0]
            {
                let origin = self.angles[rh].original_curve_part.pts[0];
                let count = pathops_verb_to_points(r_verb);
                let line = self.angles[rh].original_curve_part.pts[count].sub(origin);
                let original_side = self.angle_line_on_one_side_raw(origin, line, this, true);
                if original_side >= 0 {
                    let translated_side =
                        self.angle_line_on_one_side_raw(origin, line, this, false);
                    if original_side != translated_side {
                        continue;
                    }
                }
            }
            if delta > 1e-3 {
                use_intersect = !use_intersect;
                if use_intersect {
                    s_ray_longer = ray_longer;
                    s_cept = cept;
                    s_cept_t = small_ts[index];
                    s_index = index as i32;
                }
            }
        }
        if use_intersect {
            let curve0 = if s_index != 0 {
                self.angles[rh].part.curve.pts[0]
            } else {
                self.angles[this].part.curve.pts[0]
            };
            let seg_id = if s_index != 0 { r_seg_id } else { l_seg_id };
            let t_start = if s_index != 0 {
                self.span_t(self.angles[rh].start.unwrap())
            } else {
                self.span_t(self.angles[this].start.unwrap())
            };
            let mid = self
                .seg_d_pt_at_t(seg_id, t_start + (s_cept_t - t_start) / 2.0)
                .sub(curve0);
            let sept_dir = mid.cross_check(s_cept);
            if sept_dir == 0.0 {
                return self.angle_check_parallel(this, rh);
            }
            s_ray_longer ^ (s_index == 0) ^ (sept_dir < 0.0)
        } else {
            self.angle_check_parallel(this, rh)
        }
    }

    pub fn angle_orderable(&mut self, this: AngleId, rh: AngleId) -> i32 {
        if !self.angles[this].part.is_curve {
            if !self.angles[rh].part.is_curve {
                let left_x = self.angles[this].tangent_half.dx();
                let left_y = self.angles[this].tangent_half.dy();
                let right_x = self.angles[rh].tangent_half.dx();
                let right_y = self.angles[rh].tangent_half.dy();
                let x_ry = left_x * right_y;
                let rx_y = right_x * left_y;
                if x_ry == rx_y {
                    if left_x * right_x < 0.0 || left_y * right_y < 0.0 {
                        return 1;
                    }
                    self.angles[this].unorderable = true;
                    self.angles[rh].unorderable = true;
                    return -1;
                }
                return i32::from(x_ry < rx_y);
            }
            let result = self.angle_line_on_one_side(this, rh, false);
            if result >= 0 {
                return result;
            }
            if self.angles[this].unorderable || approximately_zero(self.angles[rh].side) {
                self.angles[this].unorderable = true;
                self.angles[rh].unorderable = true;
                return -1;
            }
        } else if !self.angles[rh].part.is_curve {
            let result = self.angle_line_on_one_side(rh, this, false);
            if result >= 0 {
                return if result != 0 { 0 } else { 1 };
            }
            if self.angles[rh].unorderable || approximately_zero(self.angles[this].side) {
                self.angles[this].unorderable = true;
                self.angles[rh].unorderable = true;
                return -1;
            }
        } else {
            let result = self.angle_convex_hull_overlaps(this, rh);
            if result >= 0 {
                return result;
            }
        }
        i32::from(self.angle_ends_intersect(this, rh))
    }

    #[inline]
    fn angle_opposite_planes(&self, this: AngleId, rh: AngleId) -> bool {
        let start_span =
            (self.angles[rh].sector_start as i32 - self.angles[this].sector_start as i32).abs();
        start_span >= 8
    }

    pub fn angle_previous(&self, this: AngleId) -> AngleId {
        let mut last = self.angles[this].next.unwrap();
        loop {
            let next = self.angles[last].next.unwrap();
            if next == this {
                return last;
            }
            last = next;
        }
    }

    pub fn angle_last_marked(&mut self, this: AngleId) -> Option<SpanId> {
        let lm = self.angles[this].last_marked?;
        if self.spans[lm].chased {
            return None;
        }
        self.spans[lm].chased = true;
        Some(lm)
    }

    pub fn angle_loop_contains(&self, this: AngleId, angle: AngleId) -> bool {
        if self.angles[this].next.is_none() {
            return false;
        }
        let first = this;
        let mut lp = this;
        let t_seg = self.angle_segment(angle);
        let t_start = self.span_t(self.angles[angle].start.unwrap());
        let t_end = self.span_t(self.angles[angle].end.unwrap());
        loop {
            if self.angle_segment(lp) == t_seg {
                let l_start = self.span_t(self.angles[lp].start.unwrap());
                if l_start == t_end {
                    let l_end = self.span_t(self.angles[lp].end.unwrap());
                    if l_end == t_start {
                        return true;
                    }
                }
            }
            lp = self.angles[lp].next.unwrap();
            if lp == first {
                break;
            }
        }
        false
    }

    pub fn angle_loop_count(&self, this: AngleId) -> i32 {
        let mut count = 0;
        let first = this;
        let mut next = Some(this);
        loop {
            next = self.angles[next.unwrap()].next;
            count += 1;
            match next {
                Some(n) if n != first => {}
                _ => break,
            }
        }
        count
    }

    fn angle_merge(&mut self, this: AngleId, angle: AngleId) -> bool {
        let mut working = angle;
        loop {
            if this == working {
                return false;
            }
            working = self.angles[working].next.unwrap();
            if working == angle {
                break;
            }
        }
        loop {
            let next = self.angles[working].next.unwrap();
            self.angles[working].next = None;
            self.angle_insert(this, working);
            working = next;
            if working == angle {
                break;
            }
        }
        true
    }

    pub fn angle_insert(&mut self, this: AngleId, angle: AngleId) -> bool {
        if self.angles[angle].next.is_some() {
            if self.angle_loop_count(this) >= self.angle_loop_count(angle) {
                let _ = self.angle_merge(this, angle);
            } else if self.angles[this].next.is_some() {
                let _ = self.angle_merge(angle, this);
            } else {
                self.angle_insert(angle, this);
            }
            return true;
        }
        let singleton = self.angles[this].next.is_none();
        if singleton {
            self.angles[this].next = Some(this);
        }
        let mut next = self.angles[this].next.unwrap();
        if self.angles[next].next == Some(this) {
            if singleton || self.angle_after(angle, this) {
                self.angles[this].next = Some(angle);
                self.angles[angle].next = Some(next);
            } else {
                self.angles[next].next = Some(angle);
                self.angles[angle].next = Some(this);
            }
            return true;
        }
        let mut last = this;
        let mut flip_ambiguity = false;
        loop {
            let after_res = self.angle_after(angle, last);
            if after_res ^ (self.angles[angle].tangents_ambiguous && flip_ambiguity) {
                self.angles[last].next = Some(angle);
                self.angles[angle].next = Some(next);
                break;
            }
            last = next;
            if last == this {
                if flip_ambiguity {
                    return false;
                }
                flip_ambiguity = true;
            }
            next = self.angles[next].next.unwrap();
        }
        true
    }
}
