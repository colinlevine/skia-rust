#![forbid(unsafe_code)]

//! Port of `SkOpCoincidence.h/.cpp`.

use crate::arena::{
    CoinId, CoincidentSpans, Collapsed, OpCoincidenceList, OpState, PtTId, SegmentId, SpanId,
};
use crate::curve::curve_intersect_ray;
use crate::intersections::Intersections;
use crate::line::DLine;
use crate::types::{
    between, cpp_max_f64, cpp_min_f64, pathops_verb_to_points, zero_or_one, DPoint,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CoinEndField {
    CoinStart,
    CoinEnd,
    OppStart,
    OppEnd,
}

impl OpState {
    // ========================================================================
    // CoincidentSpans helpers
    // ========================================================================

    #[inline]
    fn coin_flipped(&self, coin: CoinId) -> bool {
        let c = &self.coin_spans[coin];
        self.pt_ts[c.opp_pt_t_start].t > self.pt_ts[c.opp_pt_t_end].t
    }

    #[inline]
    fn coin_set_coin_pt_t_start(&mut self, coin: CoinId, pt_t: PtTId) {
        self.coin_spans[coin].coin_pt_t_start = pt_t;
        self.pt_ts[pt_t].coincident = true;
    }

    #[inline]
    fn coin_set_coin_pt_t_end(&mut self, coin: CoinId, pt_t: PtTId) {
        self.coin_spans[coin].coin_pt_t_end = pt_t;
        self.pt_ts[pt_t].coincident = true;
    }

    #[inline]
    fn coin_set_opp_pt_t_start(&mut self, coin: CoinId, pt_t: PtTId) {
        self.coin_spans[coin].opp_pt_t_start = pt_t;
        self.pt_ts[pt_t].coincident = true;
    }

    #[inline]
    fn coin_set_opp_pt_t_end(&mut self, coin: CoinId, pt_t: PtTId) {
        self.coin_spans[coin].opp_pt_t_end = pt_t;
        self.pt_ts[pt_t].coincident = true;
    }

    #[inline]
    fn coin_set_starts(&mut self, coin: CoinId, coin_start: PtTId, opp_start: PtTId) {
        self.coin_set_coin_pt_t_start(coin, coin_start);
        self.coin_set_opp_pt_t_start(coin, opp_start);
    }

    #[inline]
    fn coin_set_ends(&mut self, coin: CoinId, coin_end: PtTId, opp_end: PtTId) {
        self.coin_set_coin_pt_t_end(coin, coin_end);
        self.coin_set_opp_pt_t_end(coin, opp_end);
    }

    fn coin_collapsed(&self, coin: CoinId, test: PtTId) -> bool {
        let c = self.coin_spans[coin];
        (c.coin_pt_t_start == test && self.pt_t_contains(c.coin_pt_t_end, test))
            || (c.coin_pt_t_end == test && self.pt_t_contains(c.coin_pt_t_start, test))
            || (c.opp_pt_t_start == test && self.pt_t_contains(c.opp_pt_t_end, test))
            || (c.opp_pt_t_end == test && self.pt_t_contains(c.opp_pt_t_start, test))
    }

    fn coin_correct_one_end(&mut self, coin: CoinId, field: CoinEndField) {
        let orig_pt_t = match field {
            CoinEndField::CoinStart => self.coin_spans[coin].coin_pt_t_start,
            CoinEndField::CoinEnd => self.coin_spans[coin].coin_pt_t_end,
            CoinEndField::OppStart => self.coin_spans[coin].opp_pt_t_start,
            CoinEndField::OppEnd => self.coin_spans[coin].opp_pt_t_end,
        };
        let orig_span = self.pt_ts[orig_pt_t].span;
        let test_pt_t = if let Some(prev) = self.spans[orig_span].prev {
            let next = self.spans[prev].next.unwrap();
            self.spans[next].pt_t
        } else {
            let next = self.spans[orig_span].next.unwrap();
            let prev = self.spans[next].prev.unwrap();
            self.spans[prev].pt_t
        };
        if orig_pt_t != test_pt_t {
            match field {
                CoinEndField::CoinStart => self.coin_set_coin_pt_t_start(coin, test_pt_t),
                CoinEndField::CoinEnd => self.coin_set_coin_pt_t_end(coin, test_pt_t),
                CoinEndField::OppStart => self.coin_set_opp_pt_t_start(coin, test_pt_t),
                CoinEndField::OppEnd => self.coin_set_opp_pt_t_end(coin, test_pt_t),
            }
        }
    }

    fn coin_correct_ends(&mut self, coin: CoinId) {
        self.coin_correct_one_end(coin, CoinEndField::CoinStart);
        self.coin_correct_one_end(coin, CoinEndField::CoinEnd);
        self.coin_correct_one_end(coin, CoinEndField::OppStart);
        self.coin_correct_one_end(coin, CoinEndField::OppEnd);
    }

    fn coin_expand(&mut self, coin: CoinId) -> bool {
        let mut expanded = false;
        let segment = self.pt_t_segment(self.coin_spans[coin].coin_pt_t_start);
        let opp_segment = self.pt_t_segment(self.coin_spans[coin].opp_pt_t_start);
        loop {
            let start = self.pt_ts[self.coin_spans[coin].coin_pt_t_start].span;
            let prev = match self.spans[start].prev {
                Some(p) => p,
                None => break,
            };
            let opp_pt_t = match self.span_contains_seg(prev, opp_segment) {
                Some(o) => o,
                None => break,
            };
            let mid_t = (self.span_t(prev) + self.span_t(start)) / 2.0;
            if !self.seg_is_close(segment, mid_t, opp_segment) {
                break;
            }
            let prev_pt_t = self.spans[prev].pt_t;
            self.coin_set_starts(coin, prev_pt_t, opp_pt_t);
            expanded = true;
        }
        loop {
            let end = self.pt_ts[self.coin_spans[coin].coin_pt_t_end].span;
            let next = if self.span_final(end) {
                None
            } else {
                self.spans[end].next
            };
            let next = match next {
                Some(n) if !self.span_deleted(n) => n,
                _ => break,
            };
            let opp_pt_t = match self.span_contains_seg(next, opp_segment) {
                Some(o) => o,
                None => break,
            };
            let mid_t = (self.span_t(end) + self.span_t(next)) / 2.0;
            if !self.seg_is_close(segment, mid_t, opp_segment) {
                break;
            }
            let next_pt_t = self.spans[next].pt_t;
            self.coin_set_ends(coin, next_pt_t, opp_pt_t);
            expanded = true;
        }
        expanded
    }

    fn coin_extend(
        &mut self,
        coin: CoinId,
        coin_pt_t_start: PtTId,
        coin_pt_t_end: PtTId,
        opp_pt_t_start: PtTId,
        opp_pt_t_end: PtTId,
    ) -> bool {
        let mut result = false;
        let c = self.coin_spans[coin];
        let flipped = self.coin_flipped(coin);
        if self.pt_ts[c.coin_pt_t_start].t > self.pt_ts[coin_pt_t_start].t
            || (if flipped {
                self.pt_ts[c.opp_pt_t_start].t < self.pt_ts[opp_pt_t_start].t
            } else {
                self.pt_ts[c.opp_pt_t_start].t > self.pt_ts[opp_pt_t_start].t
            })
        {
            self.coin_set_starts(coin, coin_pt_t_start, opp_pt_t_start);
            result = true;
        }
        let c = self.coin_spans[coin];
        let flipped = self.coin_flipped(coin);
        if self.pt_ts[c.coin_pt_t_end].t < self.pt_ts[coin_pt_t_end].t
            || (if flipped {
                self.pt_ts[c.opp_pt_t_end].t > self.pt_ts[opp_pt_t_end].t
            } else {
                self.pt_ts[c.opp_pt_t_end].t < self.pt_ts[opp_pt_t_end].t
            })
        {
            self.coin_set_ends(coin, coin_pt_t_end, opp_pt_t_end);
            result = true;
        }
        result
    }

    fn coin_contains(&self, coin: CoinId, mut s: PtTId, mut e: PtTId) -> bool {
        if self.pt_ts[s].t > self.pt_ts[e].t {
            core::mem::swap(&mut s, &mut e);
        }
        let c = self.coin_spans[coin];
        if self.pt_t_segment(s) == self.pt_t_segment(c.coin_pt_t_start) {
            self.pt_ts[c.coin_pt_t_start].t <= self.pt_ts[s].t
                && self.pt_ts[e].t <= self.pt_ts[c.coin_pt_t_end].t
        } else {
            let mut opp_ts = self.pt_ts[c.opp_pt_t_start].t;
            let mut opp_te = self.pt_ts[c.opp_pt_t_end].t;
            if opp_ts > opp_te {
                core::mem::swap(&mut opp_ts, &mut opp_te);
            }
            opp_ts <= self.pt_ts[s].t && self.pt_ts[e].t <= opp_te
        }
    }

    fn coin_ordered(&self, coin: CoinId, result: &mut bool) -> bool {
        let c = self.coin_spans[coin];
        let start = self.pt_ts[c.coin_pt_t_start].span;
        let end = self.pt_ts[c.coin_pt_t_end].span;
        let mut next = match self.spans[start].next {
            Some(n) => n,
            None => return false,
        };
        if next == end {
            *result = true;
            return true;
        }
        let flipped = self.coin_flipped(coin);
        let opp_seg = self.pt_t_segment(c.opp_pt_t_start);
        let mut opp_last_t = self.pt_ts[c.opp_pt_t_start].t;
        loop {
            let opp = match self.span_contains_seg(next, opp_seg) {
                Some(o) => o,
                None => return false,
            };
            let opp_t = self.pt_ts[opp].t;
            if (opp_last_t > opp_t) != flipped {
                *result = false;
                return true;
            }
            opp_last_t = opp_t;
            if next == end {
                break;
            }
            if self.span_final(next) {
                *result = false;
                return true;
            }
            next = match self.spans[next].next {
                Some(n) => n,
                None => {
                    *result = false;
                    return true;
                }
            };
        }
        *result = true;
        true
    }

    // ========================================================================
    // OpCoincidence methods
    // ========================================================================

    pub fn coincidence_ordered_segs(&self, coin_seg: SegmentId, opp_seg: SegmentId) -> bool {
        let cv = self.segments[coin_seg].verb as u8;
        let ov = self.segments[opp_seg].verb as u8;
        if cv < ov {
            return true;
        }
        if cv > ov {
            return false;
        }
        let pt_count = pathops_verb_to_points(self.segments[coin_seg].verb) + 1;
        let c_pts = &self.segments[coin_seg].pts;
        let o_pts = &self.segments[opp_seg].pts;
        for i in 0..pt_count {
            if c_pts[i].x < o_pts[i].x {
                return true;
            }
            if c_pts[i].x > o_pts[i].x {
                return false;
            }
            if c_pts[i].y < o_pts[i].y {
                return true;
            }
            if c_pts[i].y > o_pts[i].y {
                return false;
            }
        }
        true
    }

    #[inline]
    pub fn coincidence_ordered_pts(&self, coin_start: PtTId, opp_start: PtTId) -> bool {
        self.coincidence_ordered_segs(self.pt_t_segment(coin_start), self.pt_t_segment(opp_start))
    }

    pub fn coincidence_extend(
        &mut self,
        mut coin_pt_t_start: PtTId,
        mut coin_pt_t_end: PtTId,
        mut opp_pt_t_start: PtTId,
        mut opp_pt_t_end: PtTId,
    ) -> bool {
        let mut test_opt = self.coincidence.head;
        if test_opt.is_none() {
            return false;
        }
        let mut coin_seg = self.pt_t_segment(coin_pt_t_start);
        let mut opp_seg = self.pt_t_segment(opp_pt_t_start);
        if !self.coincidence_ordered_pts(coin_pt_t_start, opp_pt_t_start) {
            core::mem::swap(&mut coin_seg, &mut opp_seg);
            core::mem::swap(&mut coin_pt_t_start, &mut opp_pt_t_start);
            core::mem::swap(&mut coin_pt_t_end, &mut opp_pt_t_end);
            if self.pt_ts[coin_pt_t_start].t > self.pt_ts[coin_pt_t_end].t {
                core::mem::swap(&mut coin_pt_t_start, &mut coin_pt_t_end);
                core::mem::swap(&mut opp_pt_t_start, &mut opp_pt_t_end);
            }
        }
        let opp_min_t = cpp_min_f64(self.pt_ts[opp_pt_t_start].t, self.pt_ts[opp_pt_t_end].t);
        while let Some(test) = test_opt {
            let tc = self.coin_spans[test];
            if coin_seg == self.pt_t_segment(tc.coin_pt_t_start)
                && opp_seg == self.pt_t_segment(tc.opp_pt_t_start)
            {
                let o_test_min_t =
                    cpp_min_f64(self.pt_ts[tc.opp_pt_t_start].t, self.pt_ts[tc.opp_pt_t_end].t);
                let o_test_max_t =
                    cpp_max_f64(self.pt_ts[tc.opp_pt_t_start].t, self.pt_ts[tc.opp_pt_t_end].t);
                if (self.pt_ts[tc.coin_pt_t_start].t <= self.pt_ts[coin_pt_t_end].t
                    && self.pt_ts[coin_pt_t_start].t <= self.pt_ts[tc.coin_pt_t_end].t)
                    || (o_test_min_t <= o_test_max_t && opp_min_t <= o_test_max_t)
                {
                    self.coin_extend(
                        test,
                        coin_pt_t_start,
                        coin_pt_t_end,
                        opp_pt_t_start,
                        opp_pt_t_end,
                    );
                    return true;
                }
            }
            test_opt = tc.next;
        }
        false
    }

    pub fn coincidence_add_to_head(
        &mut self,
        head: &mut Option<CoinId>,
        mut coin_pt_t_start: PtTId,
        mut coin_pt_t_end: PtTId,
        mut opp_pt_t_start: PtTId,
        mut opp_pt_t_end: PtTId,
    ) {
        if !self.coincidence_ordered_pts(coin_pt_t_start, opp_pt_t_start) {
            if self.pt_ts[opp_pt_t_start].t < self.pt_ts[opp_pt_t_end].t {
                self.coincidence_add_to_head(
                    head,
                    opp_pt_t_start,
                    opp_pt_t_end,
                    coin_pt_t_start,
                    coin_pt_t_end,
                );
            } else {
                self.coincidence_add_to_head(
                    head,
                    opp_pt_t_end,
                    opp_pt_t_start,
                    coin_pt_t_end,
                    coin_pt_t_start,
                );
            }
            return;
        }
        coin_pt_t_start = self.spans[self.pt_ts[coin_pt_t_start].span].pt_t;
        coin_pt_t_end = self.spans[self.pt_ts[coin_pt_t_end].span].pt_t;
        opp_pt_t_start = self.spans[self.pt_ts[opp_pt_t_start].span].pt_t;
        opp_pt_t_end = self.spans[self.pt_ts[opp_pt_t_end].span].pt_t;
        let id = self.coin_spans.len();
        self.coin_spans.push(CoincidentSpans {
            next: *head,
            coin_pt_t_start,
            coin_pt_t_end,
            opp_pt_t_start,
            opp_pt_t_end,
        });
        self.pt_ts[coin_pt_t_start].coincident = true;
        self.pt_ts[coin_pt_t_end].coincident = true;
        self.pt_ts[opp_pt_t_start].coincident = true;
        self.pt_ts[opp_pt_t_end].coincident = true;
        *head = Some(id);
    }

    pub fn coincidence_add(
        &mut self,
        coin_pt_t_start: PtTId,
        coin_pt_t_end: PtTId,
        opp_pt_t_start: PtTId,
        opp_pt_t_end: PtTId,
    ) {
        let mut head = self.coincidence.head;
        self.coincidence_add_to_head(
            &mut head,
            coin_pt_t_start,
            coin_pt_t_end,
            opp_pt_t_start,
            opp_pt_t_end,
        );
        self.coincidence.head = head;
    }

    fn coincidence_add_end_moved_spans_pair(&mut self, base: SpanId, test_span: SpanId) -> bool {
        let stop_pt_t = self.spans[test_span].pt_t;
        let mut test_pt_t = self.pt_ts[stop_pt_t].next;
        let base_seg = self.spans[base].segment;
        let mut escape_hatch = 100000;
        while test_pt_t != stop_pt_t {
            escape_hatch -= 1;
            if escape_hatch <= 0 {
                return false;
            }
            let test_seg = self.pt_t_segment(test_pt_t);
            let test_span_of_pt = self.pt_ts[test_pt_t].span;
            if !self.pt_ts[test_pt_t].deleted
                && test_seg != base_seg
                && self.spans[test_span_of_pt].pt_t == test_pt_t
                && !self.coincidence_contains_seg_opp_t(base_seg, test_seg, self.pt_ts[test_pt_t].t)
            {
                let dxdy = self.seg_d_slope_at_t(base_seg, self.span_t(base));
                let pt = self.span_pt(base);
                let ray = DLine::new(
                    DPoint::from_point(pt),
                    DPoint::new(pt.x as f64 + dxdy.y, pt.y as f64 - dxdy.x),
                );
                let mut i = Intersections::new();
                let ts = self.segments[test_seg];
                curve_intersect_ray(&ts.pts, ts.weight, ts.verb, &ray, &mut i);
                for index in 0..i.used() {
                    let t = i.t[0][index];
                    if !between(0.0, t, 1.0) {
                        continue;
                    }
                    let opp_pt = i.pt[index];
                    if !opp_pt.approximately_equal_pt(pt) {
                        continue;
                    }
                    let opp_start = match self.seg_add_t(test_seg, t) {
                        Some(p) => p,
                        None => return false,
                    };
                    if opp_start == test_pt_t {
                        continue;
                    }
                    let opp_start_span = self.pt_ts[opp_start].span;
                    self.span_add_opp(opp_start_span, base);
                    if self.pt_ts[opp_start].deleted {
                        continue;
                    }
                    let mut coin_seg = self.spans[base].segment;
                    let mut opp_seg = self.pt_t_segment(opp_start);
                    let (mut coin_ts, mut coin_te, mut opp_ts, mut opp_te) =
                        if self.coincidence_ordered_segs(coin_seg, opp_seg) {
                            (
                                self.span_t(base),
                                self.span_t(test_span),
                                self.pt_ts[opp_start].t,
                                self.pt_ts[test_pt_t].t,
                            )
                        } else {
                            core::mem::swap(&mut coin_seg, &mut opp_seg);
                            (
                                self.pt_ts[opp_start].t,
                                self.pt_ts[test_pt_t].t,
                                self.span_t(base),
                                self.span_t(test_span),
                            )
                        };
                    if coin_ts > coin_te {
                        core::mem::swap(&mut coin_ts, &mut coin_te);
                        core::mem::swap(&mut opp_ts, &mut opp_te);
                    }
                    let mut added = false;
                    if !self.coincidence_add_or_overlap(
                        coin_seg, opp_seg, coin_ts, coin_te, opp_ts, opp_te, &mut added,
                    ) {
                        return false;
                    }
                }
            }
            test_pt_t = self.pt_ts[test_pt_t].next;
        }
        true
    }

    fn coincidence_add_end_moved_spans_pt(&mut self, pt_t: PtTId) -> bool {
        let base = self.pt_ts[pt_t].span;
        if self.span_final(base) {
            return false;
        }
        let prev = match self.spans[base].prev {
            Some(p) => p,
            None => return false,
        };
        if !self.span_is_canceled(prev)
            && !self.coincidence_add_end_moved_spans_pair(base, prev)
        {
            return false;
        }
        if !self.span_is_canceled(base) {
            let next = match self.spans[base].next {
                Some(n) => n,
                None => return false,
            };
            if !self.coincidence_add_end_moved_spans_pair(base, next) {
                return false;
            }
        }
        true
    }

    pub fn coincidence_add_end_moved_spans(&mut self) -> bool {
        let mut span_opt = self.coincidence.head;
        if span_opt.is_none() {
            return true;
        }
        self.coincidence.top = span_opt;
        self.coincidence.head = None;
        while let Some(span) = span_opt {
            let c = self.coin_spans[span];
            if self.pt_ts[c.coin_pt_t_start].pt != self.pt_ts[c.opp_pt_t_start].pt {
                if self.pt_ts[c.coin_pt_t_start].t == 1.0 {
                    return false;
                }
                let on_end = self.pt_ts[c.coin_pt_t_start].t == 0.0;
                let o_on_end = zero_or_one(self.pt_ts[c.opp_pt_t_start].t);
                if on_end {
                    if !o_on_end && !self.coincidence_add_end_moved_spans_pt(c.opp_pt_t_start) {
                        return false;
                    }
                } else if o_on_end && !self.coincidence_add_end_moved_spans_pt(c.coin_pt_t_start) {
                    return false;
                }
            }
            let c = self.coin_spans[span];
            if self.pt_ts[c.coin_pt_t_end].pt != self.pt_ts[c.opp_pt_t_end].pt {
                let on_end = self.pt_ts[c.coin_pt_t_end].t == 1.0;
                let o_on_end = zero_or_one(self.pt_ts[c.opp_pt_t_end].t);
                if on_end {
                    if !o_on_end && !self.coincidence_add_end_moved_spans_pt(c.opp_pt_t_end) {
                        return false;
                    }
                } else if o_on_end && !self.coincidence_add_end_moved_spans_pt(c.coin_pt_t_end) {
                    return false;
                }
            }
            span_opt = self.coin_spans[span].next;
        }
        self.coincidence_restore_head();
        true
    }

    pub fn coincidence_add_expanded(&mut self) -> bool {
        let mut coin_opt = self.coincidence.head;
        while let Some(coin) = coin_opt {
            let c = self.coin_spans[coin];
            let start_pt_t = c.coin_pt_t_start;
            let o_start_pt_t = c.opp_pt_t_start;
            let mut prior_t = self.pt_ts[start_pt_t].t;
            let mut o_prior_t = self.pt_ts[o_start_pt_t].t;
            if !self.pt_t_contains(start_pt_t, o_start_pt_t) {
                return false;
            }
            let start = self.pt_ts[start_pt_t].span;
            let o_start = self.pt_ts[o_start_pt_t].span;
            let mut end = self.pt_ts[c.coin_pt_t_end].span;
            let mut o_end = self.pt_ts[c.opp_pt_t_end].span;
            if self.span_deleted(o_end) || self.span_final(start) {
                return false;
            }
            let mut test = match self.spans[start].next {
                Some(n) => n,
                None => return false,
            };
            let flipped = self.coin_flipped(coin);
            if !flipped && self.span_final(o_start) {
                return false;
            }
            let mut o_test = if flipped {
                match self.spans[o_start].prev {
                    Some(p) => p,
                    None => return false,
                }
            } else {
                match self.spans[o_start].next {
                    Some(n) => n,
                    None => return false,
                }
            };
            let seg = self.spans[start].segment;
            let o_seg = self.spans[o_start].segment;
            while test != end || o_test != o_end {
                let contained_opp = self.pt_t_contains_seg(self.spans[test].pt_t, o_seg);
                let contained_this = self.pt_t_contains_seg(self.spans[o_test].pt_t, seg);
                if contained_opp.is_none() || contained_this.is_none() {
                    let (next_t, o_next_t) = if let Some(co) = contained_opp {
                        (self.span_t(test), self.pt_ts[co].t)
                    } else if let Some(ct) = contained_this {
                        (self.pt_ts[ct].t, self.span_t(o_test))
                    } else {
                        let mut walk = test;
                        let coin_end_span = self.pt_ts[self.coin_spans[coin].coin_pt_t_end].span;
                        let walk_opp = loop {
                            if self.span_final(walk) {
                                return false;
                            }
                            walk = match self.spans[walk].next {
                                Some(n) => n,
                                None => return false,
                            };
                            if let Some(wo) = self.pt_t_contains_seg(self.spans[walk].pt_t, o_seg) {
                                break Some(wo);
                            }
                            if walk == coin_end_span {
                                break None;
                            }
                        };
                        let wo = match walk_opp {
                            Some(w) => w,
                            None => return false,
                        };
                        (self.span_t(walk), self.pt_ts[wo].t)
                    };
                    let start_range = next_t - prior_t;
                    if start_range == 0.0 {
                        return false;
                    }
                    let start_part = (self.span_t(test) - prior_t) / start_range;
                    let o_start_range = o_next_t - o_prior_t;
                    if o_start_range == 0.0 {
                        return false;
                    }
                    let o_start_part = (self.span_t(o_test) - o_prior_t) / o_start_range;
                    if start_part == o_start_part {
                        return false;
                    }
                    let add_to_opp = if contained_opp.is_none() && contained_this.is_none() {
                        start_part < o_start_part
                    } else {
                        contained_this.is_some()
                    };
                    let mut start_over = false;
                    let success = if add_to_opp {
                        self.seg_add_expanded(
                            o_seg,
                            o_prior_t + o_start_range * start_part,
                            test,
                            &mut start_over,
                        )
                    } else {
                        self.seg_add_expanded(
                            seg,
                            prior_t + start_range * o_start_part,
                            o_test,
                            &mut start_over,
                        )
                    };
                    if !success {
                        return false;
                    }
                    if start_over {
                        test = start;
                        o_test = o_start;
                    }
                    end = self.pt_ts[self.coin_spans[coin].coin_pt_t_end].span;
                    o_end = self.pt_ts[self.coin_spans[coin].opp_pt_t_end].span;
                }
                if test != end {
                    if self.span_final(test) {
                        return false;
                    }
                    prior_t = self.span_t(test);
                    test = match self.spans[test].next {
                        Some(n) => n,
                        None => return false,
                    };
                }
                if o_test != o_end {
                    o_prior_t = self.span_t(o_test);
                    if self.coin_flipped(coin) {
                        o_test = match self.spans[o_test].prev {
                            Some(p) => p,
                            None => return false,
                        };
                    } else {
                        if self.span_final(o_test) {
                            return false;
                        }
                        o_test = match self.spans[o_test].next {
                            Some(n) => n,
                            None => return false,
                        };
                    }
                }
            }
            coin_opt = self.coin_spans[coin].next;
        }
        true
    }

    fn coincidence_t_range(&self, over_s: PtTId, t: f64, coin_seg: SegmentId) -> f64 {
        let mut work = self.pt_ts[over_s].span;
        let mut found_start: Option<PtTId> = None;
        let mut found_end: Option<PtTId> = None;
        let mut coin_start: Option<PtTId> = None;
        let mut coin_end: Option<PtTId> = None;
        loop {
            let contained = self.span_contains_seg(work, coin_seg);
            if let Some(c) = contained {
                if self.span_t(work) <= t {
                    coin_start = Some(c);
                    found_start = Some(self.spans[work].pt_t);
                }
                if self.span_t(work) >= t {
                    coin_end = Some(c);
                    found_end = Some(self.spans[work].pt_t);
                    break;
                }
            } else if self.span_final(work) {
                break;
            }
            match self.spans[work].next {
                Some(n) => work = n,
                None => break,
            }
        }
        let (cs, ce, fs, fe) = match (coin_start, coin_end, found_start, found_end) {
            (Some(cs), Some(ce), Some(fs), Some(fe)) => (cs, ce, fs, fe),
            _ => return 1.0,
        };
        let denom = self.pt_ts[fe].t - self.pt_ts[fs].t;
        let s_ratio = if denom != 0.0 {
            (t - self.pt_ts[fs].t) / denom
        } else {
            1.0
        };
        self.pt_ts[cs].t + (self.pt_ts[ce].t - self.pt_ts[cs].t) * s_ratio
    }

    #[allow(clippy::too_many_arguments)]
    fn coincidence_check_overlap(
        &self,
        check_head: CoinId,
        coin_seg: SegmentId,
        opp_seg: SegmentId,
        coin_ts: f64,
        coin_te: f64,
        mut opp_ts: f64,
        mut opp_te: f64,
        overlaps: &mut Vec<CoinId>,
    ) -> bool {
        if !self.coincidence_ordered_segs(coin_seg, opp_seg) {
            if opp_ts < opp_te {
                return self.coincidence_check_overlap(
                    check_head, opp_seg, coin_seg, opp_ts, opp_te, coin_ts, coin_te, overlaps,
                );
            }
            return self.coincidence_check_overlap(
                check_head, opp_seg, coin_seg, opp_te, opp_ts, coin_te, coin_ts, overlaps,
            );
        }
        let swap_opp = opp_ts > opp_te;
        if swap_opp {
            core::mem::swap(&mut opp_ts, &mut opp_te);
        }
        let mut check_opt = Some(check_head);
        while let Some(check) = check_opt {
            let cc = self.coin_spans[check];
            if self.pt_t_segment(cc.coin_pt_t_start) == coin_seg
                && self.pt_t_segment(cc.opp_pt_t_start) == opp_seg
            {
                let check_ts = self.pt_ts[cc.coin_pt_t_start].t;
                let check_te = self.pt_ts[cc.coin_pt_t_end].t;
                let coin_outside = coin_te < check_ts || coin_ts > check_te;
                let mut o_check_ts = self.pt_ts[cc.opp_pt_t_start].t;
                let mut o_check_te = self.pt_ts[cc.opp_pt_t_end].t;
                if swap_opp {
                    if o_check_ts <= o_check_te {
                        return false;
                    }
                    core::mem::swap(&mut o_check_ts, &mut o_check_te);
                }
                let opp_outside = opp_te < o_check_ts || opp_ts > o_check_te;
                if !(coin_outside && opp_outside) {
                    let coin_inside = coin_te <= check_te && coin_ts >= check_ts;
                    let opp_inside = opp_te <= o_check_te && opp_ts >= o_check_ts;
                    if coin_inside && opp_inside {
                        return false;
                    }
                    overlaps.push(check);
                }
            }
            check_opt = cc.next;
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn coincidence_add_if_missing(
        &mut self,
        over1s: PtTId,
        over2s: PtTId,
        t_start: f64,
        t_end: f64,
        coin_seg: SegmentId,
        opp_seg: SegmentId,
        added: &mut bool,
    ) -> bool {
        let mut coin_ts = self.coincidence_t_range(over1s, t_start, coin_seg);
        let mut coin_te = self.coincidence_t_range(over1s, t_end, coin_seg);
        let result = self.seg_collapsed(coin_seg, coin_ts, coin_te);
        if result != Collapsed::No {
            return result == Collapsed::Yes;
        }
        let mut opp_ts = self.coincidence_t_range(over2s, t_start, opp_seg);
        let mut opp_te = self.coincidence_t_range(over2s, t_end, opp_seg);
        let result = self.seg_collapsed(opp_seg, opp_ts, opp_te);
        if result != Collapsed::No {
            return result == Collapsed::Yes;
        }
        if coin_ts > coin_te {
            core::mem::swap(&mut coin_ts, &mut coin_te);
            core::mem::swap(&mut opp_ts, &mut opp_te);
        }
        let _ =
            self.coincidence_add_or_overlap(coin_seg, opp_seg, coin_ts, coin_te, opp_ts, opp_te, added);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn coincidence_add_or_overlap(
        &mut self,
        coin_seg: SegmentId,
        opp_seg: SegmentId,
        coin_ts: f64,
        coin_te: f64,
        opp_ts: f64,
        opp_te: f64,
        added: &mut bool,
    ) -> bool {
        let mut overlaps = Vec::new();
        let top = match self.coincidence.top {
            Some(t) => t,
            None => return false,
        };
        if !self.coincidence_check_overlap(
            top,
            coin_seg,
            opp_seg,
            coin_ts,
            coin_te,
            opp_ts,
            opp_te,
            &mut overlaps,
        ) {
            return true;
        }
        if let Some(head) = self.coincidence.head {
            if !self.coincidence_check_overlap(
                head,
                coin_seg,
                opp_seg,
                coin_ts,
                coin_te,
                opp_ts,
                opp_te,
                &mut overlaps,
            ) {
                return true;
            }
        }
        let overlap = overlaps.first().copied();
        if let Some(ov) = overlap {
            for &test in overlaps.iter().skip(1) {
                let tc = self.coin_spans[test];
                if self.pt_ts[self.coin_spans[ov].coin_pt_t_start].t
                    > self.pt_ts[tc.coin_pt_t_start].t
                {
                    self.coin_set_coin_pt_t_start(ov, tc.coin_pt_t_start);
                }
                if self.pt_ts[self.coin_spans[ov].coin_pt_t_end].t < self.pt_ts[tc.coin_pt_t_end].t
                {
                    self.coin_set_coin_pt_t_end(ov, tc.coin_pt_t_end);
                }
                if if self.coin_flipped(ov) {
                    self.pt_ts[self.coin_spans[ov].opp_pt_t_start].t
                        < self.pt_ts[tc.opp_pt_t_start].t
                } else {
                    self.pt_ts[self.coin_spans[ov].opp_pt_t_start].t
                        > self.pt_ts[tc.opp_pt_t_start].t
                } {
                    self.coin_set_opp_pt_t_start(ov, tc.opp_pt_t_start);
                }
                if if self.coin_flipped(ov) {
                    self.pt_ts[self.coin_spans[ov].opp_pt_t_end].t > self.pt_ts[tc.opp_pt_t_end].t
                } else {
                    self.pt_ts[self.coin_spans[ov].opp_pt_t_end].t < self.pt_ts[tc.opp_pt_t_end].t
                } {
                    self.coin_set_opp_pt_t_end(ov, tc.opp_pt_t_end);
                }
                let mut h = self.coincidence.head;
                let removed = self.coincidence_release_one(&mut h, test);
                self.coincidence.head = h;
                if !removed {
                    let mut t = self.coincidence.top;
                    let _ = self.coincidence_release_one(&mut t, test);
                    self.coincidence.top = t;
                }
            }
        }
        let mut cs = self.seg_existing(coin_seg, coin_ts, Some(opp_seg));
        let mut ce = self.seg_existing(coin_seg, coin_te, Some(opp_seg));
        if let (Some(ov), Some(cs_id), Some(ce_id)) = (overlap, cs, ce) {
            if self.coin_contains(ov, cs_id, ce_id) {
                return true;
            }
        }
        if cs.is_some() && cs == ce {
            return false;
        }
        let mut os = self.seg_existing(opp_seg, opp_ts, Some(coin_seg));
        let mut oe = self.seg_existing(opp_seg, opp_te, Some(coin_seg));
        if let (Some(ov), Some(os_id), Some(oe_id)) = (overlap, os, oe) {
            if self.coin_contains(ov, os_id, oe_id) {
                return true;
            }
        }
        if cs.is_some_and(|p| self.pt_ts[p].deleted)
            || os.is_some_and(|p| self.pt_ts[p].deleted)
            || ce.is_some_and(|p| self.pt_ts[p].deleted)
            || oe.is_some_and(|p| self.pt_ts[p].deleted)
        {
            return false;
        }
        let cs_existing = if cs.is_none() {
            self.seg_existing(coin_seg, coin_ts, None)
        } else {
            None
        };
        let ce_existing = if ce.is_none() {
            self.seg_existing(coin_seg, coin_te, None)
        } else {
            None
        };
        if cs_existing.is_some() && cs_existing == ce_existing {
            return false;
        }
        if let Some(cee) = ce_existing {
            if Some(cee) == cs
                || cs_existing
                    .or(cs)
                    .is_some_and(|x| self.pt_t_contains(cee, x))
            {
                return false;
            }
        }
        let os_existing = if os.is_none() {
            self.seg_existing(opp_seg, opp_ts, None)
        } else {
            None
        };
        let oe_existing = if oe.is_none() {
            self.seg_existing(opp_seg, opp_te, None)
        } else {
            None
        };
        if os_existing.is_some() && os_existing == oe_existing {
            return false;
        }
        if let Some(ose) = os_existing {
            if Some(ose) == oe
                || oe_existing
                    .or(oe)
                    .is_some_and(|x| self.pt_t_contains(ose, x))
            {
                return false;
            }
        }
        if let Some(oee) = oe_existing {
            if Some(oee) == os
                || os_existing
                    .or(os)
                    .is_some_and(|x| self.pt_t_contains(oee, x))
            {
                return false;
            }
        }
        if cs.is_none() || os.is_none() {
            let cs_w = match cs {
                Some(p) => p,
                None => match self.seg_add_t(coin_seg, coin_ts) {
                    Some(p) => p,
                    None => return false,
                },
            };
            if Some(cs_w) == ce {
                return true;
            }
            let os_w = match os {
                Some(p) => p,
                None => match self.seg_add_t(opp_seg, opp_ts) {
                    Some(p) => p,
                    None => return false,
                },
            };
            let cs_span = self.pt_ts[cs_w].span;
            let os_span = self.pt_ts[os_w].span;
            self.span_add_opp(cs_span, os_span);
            cs = Some(cs_w);
            os = self.pt_t_active(os_w);
            if os.is_none() {
                return false;
            }
            if ce.is_some_and(|p| self.pt_ts[p].deleted)
                || oe.is_some_and(|p| self.pt_ts[p].deleted)
            {
                return false;
            }
        }
        if ce.is_none() || oe.is_none() {
            let ce_w = match ce {
                Some(p) => p,
                None => match self.seg_add_t(coin_seg, coin_te) {
                    Some(p) => p,
                    None => return false,
                },
            };
            let oe_w = match oe {
                Some(p) => p,
                None => match self.seg_add_t(opp_seg, opp_te) {
                    Some(p) => p,
                    None => return false,
                },
            };
            let ce_span = self.pt_ts[ce_w].span;
            let oe_span = self.pt_ts[oe_w].span;
            if !self.span_add_opp(ce_span, oe_span) {
                return false;
            }
            ce = Some(ce_w);
            oe = Some(oe_w);
        }
        let (mut cs_id, mut ce_id, mut os_id, mut oe_id) =
            (cs.unwrap(), ce.unwrap(), os.unwrap(), oe.unwrap());
        if self.pt_ts[cs_id].deleted
            || self.pt_ts[os_id].deleted
            || self.pt_ts[ce_id].deleted
            || self.pt_ts[oe_id].deleted
        {
            return false;
        }
        if self.pt_t_contains(cs_id, ce_id) || self.pt_t_contains(os_id, oe_id) {
            return false;
        }
        let result = if let Some(ov) = overlap {
            if self.pt_t_segment(self.coin_spans[ov].coin_pt_t_start) == coin_seg {
                self.coin_extend(ov, cs_id, ce_id, os_id, oe_id)
            } else {
                if self.pt_ts[os_id].t > self.pt_ts[oe_id].t {
                    core::mem::swap(&mut cs_id, &mut ce_id);
                    core::mem::swap(&mut os_id, &mut oe_id);
                }
                self.coin_extend(ov, os_id, oe_id, cs_id, ce_id)
            }
        } else {
            self.coincidence_add(cs_id, ce_id, os_id, oe_id);
            true
        };
        if result {
            *added = true;
        }
        true
    }

    pub fn coincidence_add_missing(&mut self, added: &mut bool) -> bool {
        let mut outer_opt = self.coincidence.head;
        *added = false;
        if outer_opt.is_none() {
            return true;
        }
        self.coincidence.top = outer_opt;
        self.coincidence.head = None;
        while let Some(outer) = outer_opt {
            let oc = self.coin_spans[outer];
            let ocs = oc.coin_pt_t_start;
            if self.pt_ts[ocs].deleted {
                return false;
            }
            let outer_coin = self.pt_t_segment(ocs);
            if self.seg_done(outer_coin) {
                return false;
            }
            let oos = oc.opp_pt_t_start;
            if self.pt_ts[oos].deleted {
                return true;
            }
            let outer_opp = self.pt_t_segment(oos);
            let mut inner_opt = oc.next;
            while let Some(inner) = inner_opt {
                let ic = self.coin_spans[inner];
                let ics = ic.coin_pt_t_start;
                if self.pt_ts[ics].deleted {
                    return false;
                }
                let inner_coin = self.pt_t_segment(ics);
                if self.seg_done(inner_coin) {
                    return false;
                }
                let ios = ic.opp_pt_t_start;
                if self.pt_ts[ios].deleted {
                    return false;
                }
                let inner_opp = self.pt_t_segment(ios);
                let mut over_s = 0.0;
                let mut over_e = 0.0;
                if outer_coin == inner_coin {
                    let oce = self.coin_spans[outer].coin_pt_t_end;
                    if self.pt_ts[oce].deleted {
                        return true;
                    }
                    let ice = self.coin_spans[inner].coin_pt_t_end;
                    if self.pt_ts[ice].deleted {
                        return false;
                    }
                    if outer_opp != inner_opp
                        && self.coincidence_overlap(ocs, oce, ics, ice, &mut over_s, &mut over_e)
                    {
                        let s1 = self.pt_t_starter(ocs, oce);
                        let s2 = self.pt_t_starter(ics, ice);
                        if !self.coincidence_add_if_missing(
                            s1, s2, over_s, over_e, outer_opp, inner_opp, added,
                        ) {
                            return false;
                        }
                    }
                } else if outer_coin == inner_opp {
                    let oce = self.coin_spans[outer].coin_pt_t_end;
                    if self.pt_ts[oce].deleted {
                        return false;
                    }
                    let ioe = self.coin_spans[inner].opp_pt_t_end;
                    if self.pt_ts[ioe].deleted {
                        return false;
                    }
                    if outer_opp != inner_coin
                        && self.coincidence_overlap(ocs, oce, ios, ioe, &mut over_s, &mut over_e)
                    {
                        let s1 = self.pt_t_starter(ocs, oce);
                        let s2 = self.pt_t_starter(ios, ioe);
                        if !self.coincidence_add_if_missing(
                            s1, s2, over_s, over_e, outer_opp, inner_coin, added,
                        ) {
                            return false;
                        }
                    }
                } else if outer_opp == inner_coin {
                    let ooe = self.coin_spans[outer].opp_pt_t_end;
                    if self.pt_ts[ooe].deleted {
                        return false;
                    }
                    let ice = self.coin_spans[inner].coin_pt_t_end;
                    if self.pt_ts[ice].deleted {
                        return false;
                    }
                    if self.coincidence_overlap(oos, ooe, ics, ice, &mut over_s, &mut over_e) {
                        let s1 = self.pt_t_starter(oos, ooe);
                        let s2 = self.pt_t_starter(ics, ice);
                        if !self.coincidence_add_if_missing(
                            s1, s2, over_s, over_e, outer_coin, inner_opp, added,
                        ) {
                            return false;
                        }
                    }
                } else if outer_opp == inner_opp {
                    let ooe = self.coin_spans[outer].opp_pt_t_end;
                    if self.pt_ts[ooe].deleted {
                        return false;
                    }
                    let ioe = self.coin_spans[inner].opp_pt_t_end;
                    if self.pt_ts[ioe].deleted {
                        return true;
                    }
                    if self.coincidence_overlap(oos, ooe, ios, ioe, &mut over_s, &mut over_e) {
                        let s1 = self.pt_t_starter(oos, ooe);
                        let s2 = self.pt_t_starter(ios, ioe);
                        if !self.coincidence_add_if_missing(
                            s1, s2, over_s, over_e, outer_coin, inner_coin, added,
                        ) {
                            return false;
                        }
                    }
                }
                inner_opt = self.coin_spans[inner].next;
            }
            outer_opt = self.coin_spans[outer].next;
        }
        self.coincidence_restore_head();
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn coincidence_add_overlap(
        &mut self,
        overlaps_head: &mut Option<CoinId>,
        seg1: SegmentId,
        seg1o: SegmentId,
        seg2: SegmentId,
        seg2o: SegmentId,
        over_s: PtTId,
        over_e: PtTId,
    ) -> bool {
        let mut s1 = match self.pt_t_find(over_s, seg1) {
            Some(p) => p,
            None => return false,
        };
        let mut e1 = match self.pt_t_find(over_e, seg1) {
            Some(p) => p,
            None => return false,
        };
        let starter1 = self.pt_ts[self.pt_t_starter(s1, e1)].span;
        if self.spans[starter1].wind_value == 0 {
            s1 = match self.pt_t_find(over_s, seg1o) {
                Some(p) => p,
                None => return false,
            };
            e1 = match self.pt_t_find(over_e, seg1o) {
                Some(p) => p,
                None => return false,
            };
            let starter1o = self.pt_ts[self.pt_t_starter(s1, e1)].span;
            if self.spans[starter1o].wind_value == 0 {
                return true;
            }
        }
        let mut s2 = match self.pt_t_find(over_s, seg2) {
            Some(p) => p,
            None => return false,
        };
        let mut e2 = match self.pt_t_find(over_e, seg2) {
            Some(p) => p,
            None => return false,
        };
        let starter2 = self.pt_ts[self.pt_t_starter(s2, e2)].span;
        if self.spans[starter2].wind_value == 0 {
            s2 = match self.pt_t_find(over_s, seg2o) {
                Some(p) => p,
                None => return false,
            };
            e2 = match self.pt_t_find(over_e, seg2o) {
                Some(p) => p,
                None => return false,
            };
            let starter2o = self.pt_ts[self.pt_t_starter(s2, e2)].span;
            if self.spans[starter2o].wind_value == 0 {
                return true;
            }
        }
        if self.pt_t_segment(s1) == self.pt_t_segment(s2) {
            return true;
        }
        if self.pt_ts[s1].t > self.pt_ts[e1].t {
            core::mem::swap(&mut s1, &mut e1);
            core::mem::swap(&mut s2, &mut e2);
        }
        self.coincidence_add_to_head(overlaps_head, s1, e1, s2, e2);
        true
    }

    fn coincidence_contains_list_seg_opp_t(
        &self,
        mut coin_opt: Option<CoinId>,
        seg: SegmentId,
        opp: SegmentId,
        opp_t: f64,
    ) -> bool {
        while let Some(coin) = coin_opt {
            let c = self.coin_spans[coin];
            let c_seg = self.pt_t_segment(c.coin_pt_t_start);
            let o_seg = self.pt_t_segment(c.opp_pt_t_start);
            if c_seg == seg
                && o_seg == opp
                && between(
                    self.pt_ts[c.opp_pt_t_start].t,
                    opp_t,
                    self.pt_ts[c.opp_pt_t_end].t,
                )
            {
                return true;
            }
            if o_seg == seg
                && c_seg == opp
                && between(
                    self.pt_ts[c.coin_pt_t_start].t,
                    opp_t,
                    self.pt_ts[c.coin_pt_t_end].t,
                )
            {
                return true;
            }
            coin_opt = c.next;
        }
        false
    }

    pub fn coincidence_contains_seg_opp_t(
        &self,
        seg: SegmentId,
        opp: SegmentId,
        opp_t: f64,
    ) -> bool {
        self.coincidence_contains_list_seg_opp_t(self.coincidence.head, seg, opp, opp_t)
            || self.coincidence_contains_list_seg_opp_t(self.coincidence.top, seg, opp, opp_t)
    }

    pub fn coincidence_contains_pts(
        &self,
        mut coin_pt_t_start: PtTId,
        mut coin_pt_t_end: PtTId,
        mut opp_pt_t_start: PtTId,
        mut opp_pt_t_end: PtTId,
    ) -> bool {
        let mut test_opt = self.coincidence.head;
        if test_opt.is_none() {
            return false;
        }
        let mut coin_seg = self.pt_t_segment(coin_pt_t_start);
        let mut opp_seg = self.pt_t_segment(opp_pt_t_start);
        if !self.coincidence_ordered_pts(coin_pt_t_start, opp_pt_t_start) {
            core::mem::swap(&mut coin_seg, &mut opp_seg);
            core::mem::swap(&mut coin_pt_t_start, &mut opp_pt_t_start);
            core::mem::swap(&mut coin_pt_t_end, &mut opp_pt_t_end);
            if self.pt_ts[coin_pt_t_start].t > self.pt_ts[coin_pt_t_end].t {
                core::mem::swap(&mut coin_pt_t_start, &mut coin_pt_t_end);
                core::mem::swap(&mut opp_pt_t_start, &mut opp_pt_t_end);
            }
        }
        let opp_min_t = cpp_min_f64(self.pt_ts[opp_pt_t_start].t, self.pt_ts[opp_pt_t_end].t);
        let opp_max_t = cpp_max_f64(self.pt_ts[opp_pt_t_start].t, self.pt_ts[opp_pt_t_end].t);
        while let Some(test) = test_opt {
            let tc = self.coin_spans[test];
            if coin_seg == self.pt_t_segment(tc.coin_pt_t_start)
                && self.pt_ts[coin_pt_t_start].t >= self.pt_ts[tc.coin_pt_t_start].t
                && self.pt_ts[coin_pt_t_end].t <= self.pt_ts[tc.coin_pt_t_end].t
                && opp_seg == self.pt_t_segment(tc.opp_pt_t_start)
                && opp_min_t
                    >= cpp_min_f64(self.pt_ts[tc.opp_pt_t_start].t, self.pt_ts[tc.opp_pt_t_end].t)
                && opp_max_t
                    <= cpp_max_f64(self.pt_ts[tc.opp_pt_t_start].t, self.pt_ts[tc.opp_pt_t_end].t)
            {
                return true;
            }
            test_opt = tc.next;
        }
        false
    }

    pub fn coincidence_correct_ends(&mut self) {
        let mut coin_opt = self.coincidence.head;
        while let Some(coin) = coin_opt {
            self.coin_correct_ends(coin);
            coin_opt = self.coin_spans[coin].next;
        }
    }

    pub fn coincidence_apply_head(&mut self, mut coin_opt: Option<CoinId>) -> bool {
        while let Some(coin) = coin_opt {
            let c = self.coin_spans[coin];
            let start_span = self.pt_ts[c.coin_pt_t_start].span;
            if self.span_final(start_span) {
                return false;
            }
            let mut start = start_span;
            if !self.span_deleted(start) {
                let end = self.pt_ts[c.coin_pt_t_end].span;
                if start != self.span_starter(start, end) {
                    return false;
                }
                let flipped = self.coin_flipped(coin);
                let o_start_base = self.pt_ts[if flipped {
                    c.opp_pt_t_end
                } else {
                    c.opp_pt_t_start
                }]
                .span;
                if self.span_final(o_start_base) {
                    return false;
                }
                let mut o_start = o_start_base;
                if !self.span_deleted(o_start) {
                    let o_end = self.pt_ts[if flipped {
                        c.opp_pt_t_start
                    } else {
                        c.opp_pt_t_end
                    }]
                    .span;
                    let segment = self.spans[start].segment;
                    let o_segment = self.spans[o_start].segment;
                    let operand_swap = self.seg_operand(segment) != self.seg_operand(o_segment);
                    let mut skip = false;
                    if flipped {
                        if self.span_deleted(o_end) {
                            skip = true;
                        } else {
                            loop {
                                let o_next = match self.spans[o_start].next {
                                    Some(n) => n,
                                    None => return false,
                                };
                                if o_next == o_end {
                                    break;
                                }
                                if self.span_final(o_next) {
                                    return false;
                                }
                                o_start = o_next;
                            }
                        }
                    }
                    if !skip {
                        loop {
                            let mut wind_value = self.spans[start].wind_value;
                            let mut opp_value = self.spans[start].opp_value;
                            let mut o_wind_value = self.spans[o_start].wind_value;
                            let mut o_opp_value = self.spans[o_start].opp_value;
                            let mut wind_diff = if operand_swap {
                                o_opp_value
                            } else {
                                o_wind_value
                            };
                            let mut o_wind_diff = if operand_swap { opp_value } else { wind_value };
                            if !flipped {
                                wind_diff = -wind_diff;
                                o_wind_diff = -o_wind_diff;
                            }
                            let mut add_to_start = wind_value != 0
                                && (wind_value > wind_diff
                                    || (wind_value == wind_diff && o_wind_value <= o_wind_diff));
                            if if add_to_start {
                                self.spans[start].done
                            } else {
                                self.spans[o_start].done
                            } {
                                add_to_start = !add_to_start;
                            }
                            if add_to_start {
                                if operand_swap {
                                    core::mem::swap(&mut o_wind_value, &mut o_opp_value);
                                }
                                if flipped {
                                    wind_value -= o_wind_value;
                                    opp_value -= o_opp_value;
                                } else {
                                    wind_value += o_wind_value;
                                    opp_value += o_opp_value;
                                }
                                if self.seg_is_xor(segment) {
                                    wind_value &= 1;
                                }
                                if self.seg_opp_xor(segment) {
                                    opp_value &= 1;
                                }
                                o_wind_value = 0;
                                o_opp_value = 0;
                            } else {
                                if operand_swap {
                                    core::mem::swap(&mut wind_value, &mut opp_value);
                                }
                                if flipped {
                                    o_wind_value -= wind_value;
                                    o_opp_value -= opp_value;
                                } else {
                                    o_wind_value += wind_value;
                                    o_opp_value += opp_value;
                                }
                                if self.seg_is_xor(o_segment) {
                                    o_wind_value &= 1;
                                }
                                if self.seg_opp_xor(o_segment) {
                                    o_opp_value &= 1;
                                }
                                wind_value = 0;
                                opp_value = 0;
                            }
                            if wind_value <= -1 || o_wind_value <= -1 {
                                return false;
                            }
                            self.spans[start].wind_value = wind_value;
                            self.spans[start].opp_value = opp_value;
                            self.spans[o_start].wind_value = o_wind_value;
                            self.spans[o_start].opp_value = o_opp_value;
                            if wind_value == 0 && opp_value == 0 {
                                self.seg_mark_done(segment, start);
                            }
                            if o_wind_value == 0 && o_opp_value == 0 {
                                self.seg_mark_done(o_segment, o_start);
                            }
                            let next = match self.spans[start].next {
                                Some(n) => n,
                                None => return false,
                            };
                            let o_next = if flipped {
                                self.spans[o_start].prev
                            } else {
                                self.spans[o_start].next
                            };
                            if next == end {
                                break;
                            }
                            if self.span_final(next) {
                                return false;
                            }
                            start = next;
                            o_start = match o_next {
                                Some(on) if !self.span_final(on) => on,
                                _ => o_start,
                            };
                        }
                    }
                }
            }
            coin_opt = self.coin_spans[coin].next;
        }
        true
    }

    fn coincidence_release_one(&mut self, head: &mut Option<CoinId>, remove: CoinId) -> bool {
        if *head == Some(remove) {
            *head = self.coin_spans[remove].next;
            return true;
        }
        let mut prev_opt = *head;
        while let Some(prev) = prev_opt {
            let next_opt = self.coin_spans[prev].next;
            if next_opt == Some(remove) {
                self.coin_spans[prev].next = self.coin_spans[remove].next;
                return true;
            }
            prev_opt = next_opt;
        }
        false
    }

    fn coincidence_release_deleted_list(&mut self, head: &mut Option<CoinId>) {
        while let Some(h) = *head {
            if self.pt_ts[self.coin_spans[h].coin_pt_t_start].deleted {
                *head = self.coin_spans[h].next;
            } else {
                break;
            }
        }
        let mut prev_opt = *head;
        while let Some(prev) = prev_opt {
            let next_opt = self.coin_spans[prev].next;
            if let Some(next) = next_opt {
                if self.pt_ts[self.coin_spans[next].coin_pt_t_start].deleted {
                    self.coin_spans[prev].next = self.coin_spans[next].next;
                    continue;
                }
            }
            prev_opt = next_opt;
        }
    }

    pub fn coincidence_release_deleted(&mut self) {
        let mut h = self.coincidence.head;
        self.coincidence_release_deleted_list(&mut h);
        self.coincidence.head = h;
        let mut t = self.coincidence.top;
        self.coincidence_release_deleted_list(&mut t);
        self.coincidence.top = t;
    }

    fn coincidence_restore_head(&mut self) {
        let top = self.coincidence.top.take();
        if self.coincidence.head.is_none() {
            self.coincidence.head = top;
        } else {
            let mut tail = self.coincidence.head.unwrap();
            while let Some(next) = self.coin_spans[tail].next {
                tail = next;
            }
            self.coin_spans[tail].next = top;
        }
        while let Some(h) = self.coincidence.head {
            let c = self.coin_spans[h];
            if self.seg_done(self.pt_t_segment(c.coin_pt_t_start))
                || self.seg_done(self.pt_t_segment(c.opp_pt_t_start))
            {
                self.coincidence.head = c.next;
            } else {
                break;
            }
        }
        let mut prev_opt = self.coincidence.head;
        while let Some(prev) = prev_opt {
            let next_opt = self.coin_spans[prev].next;
            if let Some(next) = next_opt {
                let c = self.coin_spans[next];
                if self.seg_done(self.pt_t_segment(c.coin_pt_t_start))
                    || self.seg_done(self.pt_t_segment(c.opp_pt_t_start))
                {
                    self.coin_spans[prev].next = c.next;
                    continue;
                }
            }
            prev_opt = next_opt;
        }
    }

    pub fn coincidence_expand(&mut self) -> bool {
        let mut coin_opt = self.coincidence.head;
        if coin_opt.is_none() {
            return false;
        }
        let mut expanded = false;
        while let Some(coin) = coin_opt {
            if self.coin_expand(coin) {
                let c_start = self.coin_spans[coin].coin_pt_t_start;
                let o_start = self.coin_spans[coin].opp_pt_t_start;
                let mut test_opt = self.coincidence.head;
                while let Some(test) = test_opt {
                    let test_next = self.coin_spans[test].next;
                    if coin != test
                        && c_start == self.coin_spans[test].coin_pt_t_start
                        && o_start == self.coin_spans[test].opp_pt_t_start
                    {
                        let mut h = self.coincidence.head;
                        self.coincidence_release_one(&mut h, test);
                        self.coincidence.head = h;
                        break;
                    }
                    test_opt = test_next;
                }
                expanded = true;
            }
            coin_opt = self.coin_spans[coin].next;
        }
        expanded
    }

    pub fn coincidence_find_overlaps_head(
        &mut self,
        head: Option<CoinId>,
    ) -> Option<OpCoincidenceList> {
        let mut overlaps_head: Option<CoinId> = None;
        let mut outer_opt = head;
        while let Some(outer) = outer_opt {
            let oc = self.coin_spans[outer];
            let outer_coin = self.pt_t_segment(oc.coin_pt_t_start);
            let outer_opp = self.pt_t_segment(oc.opp_pt_t_start);
            let mut inner_opt = oc.next;
            while let Some(inner) = inner_opt {
                let ic = self.coin_spans[inner];
                let inner_coin = self.pt_t_segment(ic.coin_pt_t_start);
                if outer_coin != inner_coin {
                    let inner_opp = self.pt_t_segment(ic.opp_pt_t_start);
                    let overlap_pair = if outer_opp == inner_coin {
                        self.pt_t_overlaps(
                            oc.opp_pt_t_start,
                            oc.opp_pt_t_end,
                            ic.coin_pt_t_start,
                            ic.coin_pt_t_end,
                        )
                    } else if outer_coin == inner_opp {
                        self.pt_t_overlaps(
                            oc.coin_pt_t_start,
                            oc.coin_pt_t_end,
                            ic.opp_pt_t_start,
                            ic.opp_pt_t_end,
                        )
                    } else if outer_opp == inner_opp {
                        self.pt_t_overlaps(
                            oc.opp_pt_t_start,
                            oc.opp_pt_t_end,
                            ic.opp_pt_t_start,
                            ic.opp_pt_t_end,
                        )
                    } else {
                        None
                    };
                    if let Some((over_s, over_e)) = overlap_pair {
                        if !self.coincidence_add_overlap(
                            &mut overlaps_head,
                            outer_coin,
                            outer_opp,
                            inner_coin,
                            inner_opp,
                            over_s,
                            over_e,
                        ) {
                            return None;
                        }
                    }
                }
                inner_opt = ic.next;
            }
            outer_opt = oc.next;
        }
        Some(OpCoincidenceList {
            head: overlaps_head,
            top: None,
        })
    }

    fn coincidence_fix_up_list(&mut self, head: &mut Option<CoinId>, deleted: PtTId, kept: PtTId) {
        let kept_span = self.pt_ts[kept].span;
        // Walk with parent link
        let mut prev: Option<CoinId> = None;
        let mut cur_opt = *head;
        while let Some(coin) = cur_opt {
            let next = self.coin_spans[coin].next;
            let mut remove = false;
            if self.coin_spans[coin].coin_pt_t_start == deleted {
                if self.pt_ts[self.coin_spans[coin].coin_pt_t_end].span == kept_span {
                    remove = true;
                } else {
                    self.coin_set_coin_pt_t_start(coin, kept);
                }
            }
            if !remove && self.coin_spans[coin].coin_pt_t_end == deleted {
                if self.pt_ts[self.coin_spans[coin].coin_pt_t_start].span == kept_span {
                    remove = true;
                } else {
                    self.coin_set_coin_pt_t_end(coin, kept);
                }
            }
            if !remove && self.coin_spans[coin].opp_pt_t_start == deleted {
                if self.pt_ts[self.coin_spans[coin].opp_pt_t_end].span == kept_span {
                    remove = true;
                } else {
                    self.coin_set_opp_pt_t_start(coin, kept);
                }
            }
            if !remove && self.coin_spans[coin].opp_pt_t_end == deleted {
                if self.pt_ts[self.coin_spans[coin].opp_pt_t_start].span == kept_span {
                    remove = true;
                } else {
                    self.coin_set_opp_pt_t_end(coin, kept);
                }
            }
            if remove {
                match prev {
                    None => *head = next,
                    Some(p) => self.coin_spans[p].next = next,
                }
            } else {
                prev = Some(coin);
            }
            cur_opt = next;
        }
    }

    pub fn coincidence_fix_up(&mut self, deleted: PtTId, kept: PtTId) {
        if self.coincidence.head.is_some() {
            let mut h = self.coincidence.head;
            self.coincidence_fix_up_list(&mut h, deleted, kept);
            self.coincidence.head = h;
        }
        if self.coincidence.top.is_some() {
            let mut t = self.coincidence.top;
            self.coincidence_fix_up_list(&mut t, deleted, kept);
            self.coincidence.top = t;
        }
    }

    pub fn coincidence_mark(&mut self) -> bool {
        let mut coin_opt = self.coincidence.head;
        while let Some(coin) = coin_opt {
            let c = self.coin_spans[coin];
            let start = self.pt_ts[c.coin_pt_t_start].span;
            if self.span_final(start) || self.span_deleted(start) {
                return false;
            }
            let end = self.pt_ts[c.coin_pt_t_end].span;
            let mut o_start = self.pt_ts[c.opp_pt_t_start].span;
            let mut o_end = self.pt_ts[c.opp_pt_t_end].span;
            if self.span_deleted(o_end) {
                return false;
            }
            let flipped = self.coin_flipped(coin);
            if flipped {
                core::mem::swap(&mut o_start, &mut o_end);
            }
            if self.span_final(o_start) {
                return false;
            }
            self.span_insert_coincidence_span(start, o_start);
            self.span_insert_coin_end(end, o_end);
            let segment = self.spans[start].segment;
            let o_segment = self.spans[o_start].segment;
            let mut next = start;
            let mut o_next = o_start;
            let mut ordered = false;
            if !self.coin_ordered(coin, &mut ordered) {
                return false;
            }
            loop {
                next = match self.spans[next].next {
                    Some(n) => n,
                    None => return false,
                };
                if next == end {
                    break;
                }
                if self.span_final(next) {
                    return false;
                }
                if !self.span_insert_coincidence_seg(next, o_segment, flipped, ordered) {
                    return false;
                }
            }
            loop {
                o_next = match self.spans[o_next].next {
                    Some(n) => n,
                    None => return false,
                };
                if o_next == o_end {
                    break;
                }
                if self.span_final(o_next) {
                    return false;
                }
                if !self.span_insert_coincidence_seg(o_next, segment, flipped, ordered) {
                    return false;
                }
            }
            coin_opt = c.next;
        }
        true
    }

    fn coincidence_mark_collapsed_list(&mut self, head: &mut Option<CoinId>, test: PtTId) {
        let mut prev: Option<CoinId> = None;
        let mut cur_opt = *head;
        while let Some(coin) = cur_opt {
            let next = self.coin_spans[coin].next;
            if self.coin_collapsed(coin, test) {
                let c = self.coin_spans[coin];
                if zero_or_one(self.pt_ts[c.coin_pt_t_start].t)
                    && zero_or_one(self.pt_ts[c.coin_pt_t_end].t)
                {
                    let seg = self.pt_t_segment(c.coin_pt_t_start);
                    self.seg_mark_all_done(seg);
                }
                if zero_or_one(self.pt_ts[c.opp_pt_t_start].t)
                    && zero_or_one(self.pt_ts[c.opp_pt_t_end].t)
                {
                    let seg = self.pt_t_segment(c.opp_pt_t_start);
                    self.seg_mark_all_done(seg);
                }
                match prev {
                    None => *head = next,
                    Some(p) => self.coin_spans[p].next = next,
                }
            } else {
                prev = Some(coin);
            }
            cur_opt = next;
        }
    }

    pub fn coincidence_mark_collapsed(&mut self, test: PtTId) {
        let mut h = self.coincidence.head;
        self.coincidence_mark_collapsed_list(&mut h, test);
        self.coincidence.head = h;
        let mut t = self.coincidence.top;
        self.coincidence_mark_collapsed_list(&mut t, test);
        self.coincidence.top = t;
    }

    fn coincidence_overlap(
        &self,
        coin1s: PtTId,
        coin1e: PtTId,
        coin2s: PtTId,
        coin2e: PtTId,
        over_s: &mut f64,
        over_e: &mut f64,
    ) -> bool {
        let c1s = self.pt_ts[coin1s].t;
        let c1e = self.pt_ts[coin1e].t;
        let c2s = self.pt_ts[coin2s].t;
        let c2e = self.pt_ts[coin2e].t;
        *over_s = cpp_max_f64(cpp_min_f64(c1s, c1e), cpp_min_f64(c2s, c2e));
        *over_e = cpp_min_f64(cpp_max_f64(c1s, c1e), cpp_max_f64(c2s, c2e));
        *over_s < *over_e
    }

    fn coincidence_release_segment_list(
        &mut self,
        head: &mut Option<CoinId>,
        deleted: SegmentId,
    ) {
        let mut prev: Option<CoinId> = None;
        let mut cur_opt = *head;
        while let Some(coin) = cur_opt {
            let c = self.coin_spans[coin];
            let next = c.next;
            if self.pt_t_segment(c.coin_pt_t_start) == deleted
                || self.pt_t_segment(c.coin_pt_t_end) == deleted
                || self.pt_t_segment(c.opp_pt_t_start) == deleted
                || self.pt_t_segment(c.opp_pt_t_end) == deleted
            {
                match prev {
                    None => *head = next,
                    Some(p) => self.coin_spans[p].next = next,
                }
            } else {
                prev = Some(coin);
            }
            cur_opt = next;
        }
    }

    pub fn coincidence_release_segment(&mut self, deleted: SegmentId) {
        let mut h = self.coincidence.head;
        self.coincidence_release_segment_list(&mut h, deleted);
        self.coincidence.head = h;
        let mut t = self.coincidence.top;
        self.coincidence_release_segment_list(&mut t, deleted);
        self.coincidence.top = t;
    }
}
