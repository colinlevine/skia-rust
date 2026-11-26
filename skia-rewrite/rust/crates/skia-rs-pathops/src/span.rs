#![forbid(unsafe_code)]

//! Port of `SkOpSpan.h/.cpp`.

use skia_rs_geometry::Point;

use crate::arena::{
    Collapsed, ContourId, OpState, PtTId, SegmentId, SpanId, MAX_WINDING_TRIES, SK_MIN_S32,
};
use crate::types::{between, cpp_max_f64, cpp_min_f64, zero_or_one};

impl OpState {
    // ========================================================================
    // OpPtT methods
    // ========================================================================

    #[inline]
    pub fn pt_t_segment(&self, pt_t: PtTId) -> SegmentId {
        self.spans[self.pt_ts[pt_t].span].segment
    }

    #[inline]
    pub fn pt_t_contour(&self, pt_t: PtTId) -> ContourId {
        self.segments[self.pt_t_segment(pt_t)].contour
    }

    #[inline]
    pub fn pt_t_alias(&self, pt_t: PtTId) -> bool {
        self.spans[self.pt_ts[pt_t].span].pt_t != pt_t
    }

    #[inline]
    pub fn pt_t_add_opp(&mut self, this: PtTId, opp: PtTId, opp_prev: PtTId) {
        let old_next = self.pt_ts[this].next;
        self.pt_ts[this].next = opp;
        self.pt_ts[opp_prev].next = old_next;
    }

    #[inline]
    pub fn pt_t_insert(&mut self, this: PtTId, span: PtTId) {
        let old_next = self.pt_ts[this].next;
        self.pt_ts[span].next = old_next;
        self.pt_ts[this].next = span;
    }

    pub fn pt_t_active(&self, this: PtTId) -> Option<PtTId> {
        if !self.pt_ts[this].deleted {
            return Some(this);
        }
        let target_span = self.pt_ts[this].span;
        let mut pt_t = self.pt_ts[this].next;
        while pt_t != this {
            if self.pt_ts[pt_t].span == target_span && !self.pt_ts[pt_t].deleted {
                return Some(pt_t);
            }
            pt_t = self.pt_ts[pt_t].next;
        }
        None
    }

    pub fn pt_t_contains(&self, this: PtTId, check: PtTId) -> bool {
        let mut pt_t = self.pt_ts[this].next;
        while pt_t != this {
            if pt_t == check {
                return true;
            }
            pt_t = self.pt_ts[pt_t].next;
        }
        false
    }

    pub fn pt_t_contains_seg_t(&self, this: PtTId, segment: SegmentId, t: f64) -> bool {
        let mut pt_t = self.pt_ts[this].next;
        while pt_t != this {
            if self.pt_ts[pt_t].t == t && self.pt_t_segment(pt_t) == segment {
                return true;
            }
            pt_t = self.pt_ts[pt_t].next;
        }
        false
    }

    pub fn pt_t_contains_seg(&self, this: PtTId, check: SegmentId) -> Option<PtTId> {
        let mut pt_t = self.pt_ts[this].next;
        while pt_t != this {
            if self.pt_t_segment(pt_t) == check && !self.pt_ts[pt_t].deleted {
                return Some(pt_t);
            }
            pt_t = self.pt_ts[pt_t].next;
        }
        None
    }

    pub fn pt_t_find(&self, this: PtTId, segment: SegmentId) -> Option<PtTId> {
        let mut pt_t = this;
        loop {
            if self.pt_t_segment(pt_t) == segment && !self.pt_ts[pt_t].deleted {
                return Some(pt_t);
            }
            pt_t = self.pt_ts[pt_t].next;
            if pt_t == this {
                return None;
            }
        }
    }

    pub fn pt_t_opp_prev(&self, this: PtTId, opp: PtTId) -> Option<PtTId> {
        let mut opp_prev = self.pt_ts[opp].next;
        if opp_prev == this {
            return None;
        }
        while self.pt_ts[opp_prev].next != opp {
            opp_prev = self.pt_ts[opp_prev].next;
            if opp_prev == this {
                return None;
            }
        }
        Some(opp_prev)
    }

    pub fn pt_t_overlaps(
        &self,
        s1: PtTId,
        e1: PtTId,
        s2: PtTId,
        e2: PtTId,
    ) -> Option<(PtTId, PtTId)> {
        let s1_t = self.pt_ts[s1].t;
        let e1_t = self.pt_ts[e1].t;
        let s2_t = self.pt_ts[s2].t;
        let e2_t = self.pt_ts[e2].t;
        let start1 = if s1_t < e1_t { s1 } else { e1 };
        let start2 = if s2_t < e2_t { s2 } else { e2 };
        let s_out = if between(s1_t, self.pt_ts[start2].t, e1_t) {
            Some(start2)
        } else if between(s2_t, self.pt_ts[start1].t, e2_t) {
            Some(start1)
        } else {
            None
        };
        let end1 = if s1_t < e1_t { e1 } else { s1 };
        let end2 = if s2_t < e2_t { e2 } else { s2 };
        let e_out = if between(s1_t, self.pt_ts[end2].t, e1_t) {
            Some(end2)
        } else if between(s2_t, self.pt_ts[end1].t, e2_t) {
            Some(end1)
        } else {
            None
        };
        if s_out == e_out {
            return None;
        }
        match (s_out, e_out) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        }
    }

    pub fn pt_t_already_seen(&self, this: PtTId, mut check: PtTId) -> bool {
        let pt = self.pt_ts[this].pt;
        while this != check {
            if pt == self.pt_ts[check].pt {
                return true;
            }
            check = self.pt_ts[check].next;
        }
        false
    }

    pub fn pt_t_prev(&self, this: PtTId) -> PtTId {
        let mut result = this;
        let mut next = self.pt_ts[this].next;
        while next != this {
            result = next;
            next = self.pt_ts[next].next;
        }
        result
    }

    #[inline]
    pub fn pt_t_starter(&self, this: PtTId, end: PtTId) -> PtTId {
        if self.pt_ts[this].t < self.pt_ts[end].t {
            this
        } else {
            end
        }
    }

    // ========================================================================
    // OpSpanBase / OpSpan methods
    // ========================================================================

    #[inline]
    pub fn span_t(&self, span: SpanId) -> f64 {
        self.pt_ts[self.spans[span].pt_t].t
    }

    #[inline]
    pub fn span_pt(&self, span: SpanId) -> Point {
        self.pt_ts[self.spans[span].pt_t].pt
    }

    #[inline]
    pub fn span_final(&self, span: SpanId) -> bool {
        self.span_t(span) == 1.0
    }

    #[inline]
    pub fn span_deleted(&self, span: SpanId) -> bool {
        self.pt_ts[self.spans[span].pt_t].deleted
    }

    #[inline]
    pub fn span_simple(&self, span: SpanId) -> bool {
        let pt_t = self.spans[span].pt_t;
        let next = self.pt_ts[pt_t].next;
        self.pt_ts[next].next == pt_t
    }

    #[inline]
    pub fn span_starter(&self, start: SpanId, end: SpanId) -> SpanId {
        if self.span_t(start) < self.span_t(end) {
            start
        } else {
            end
        }
    }

    #[inline]
    pub fn span_step(&self, start: SpanId, end: SpanId) -> i32 {
        if self.span_t(start) < self.span_t(end) {
            1
        } else {
            -1
        }
    }

    #[inline]
    pub fn span_is_canceled(&self, span: SpanId) -> bool {
        self.spans[span].wind_value == 0 && self.spans[span].opp_value == 0
    }

    pub fn span_add_opp(&mut self, this: SpanId, opp: SpanId) -> bool {
        let this_pt_t = self.spans[this].pt_t;
        let opp_pt_t = self.spans[opp].pt_t;
        let opp_prev = match self.pt_t_opp_prev(this_pt_t, opp_pt_t) {
            Some(p) => p,
            None => return true,
        };
        if !self.span_merge_matches(this, opp) {
            return false;
        }
        self.pt_t_add_opp(this_pt_t, opp_pt_t, opp_prev);
        self.span_check_for_collapsed_coincidence(this);
        true
    }

    pub fn span_collapsed(&self, this: SpanId, s: f64, e: f64) -> Collapsed {
        let start = self.spans[this].pt_t;
        let mut start_next: Option<PtTId> = None;
        let mut walk = start;
        let mut min = self.pt_ts[walk].t;
        let mut max = min;
        let segment = self.spans[this].segment;
        let mut safety_net = 100000;
        loop {
            walk = self.pt_ts[walk].next;
            if walk == start {
                break;
            }
            safety_net -= 1;
            if safety_net == 0 {
                return Collapsed::Error;
            }
            if Some(walk) == start_next {
                return Collapsed::Error;
            }
            if self.pt_t_segment(walk) != segment {
                continue;
            }
            let wt = self.pt_ts[walk].t;
            min = cpp_min_f64(min, wt);
            max = cpp_max_f64(max, wt);
            if between(min, s, max) && between(min, e, max) {
                return Collapsed::Yes;
            }
            start_next = Some(self.pt_ts[start].next);
        }
        Collapsed::No
    }

    pub fn span_contains_span(&self, this: SpanId, span: SpanId) -> bool {
        let start = self.spans[this].pt_t;
        let check = self.spans[span].pt_t;
        let mut walk = self.pt_ts[start].next;
        while walk != start {
            if walk == check {
                return true;
            }
            walk = self.pt_ts[walk].next;
        }
        false
    }

    pub fn span_contains_seg(&self, this: SpanId, segment: SegmentId) -> Option<PtTId> {
        let start = self.spans[this].pt_t;
        let mut walk = self.pt_ts[start].next;
        while walk != start {
            if !self.pt_ts[walk].deleted
                && self.pt_t_segment(walk) == segment
                && self.spans[self.pt_ts[walk].span].pt_t == walk
            {
                return Some(walk);
            }
            walk = self.pt_ts[walk].next;
        }
        None
    }

    pub fn span_contains_coin_end_span(&self, this: SpanId, coin: SpanId) -> bool {
        let mut next = self.spans[this].coin_end;
        while next != this {
            if next == coin {
                return true;
            }
            next = self.spans[next].coin_end;
        }
        false
    }

    pub fn span_contains_coin_end_seg(&self, this: SpanId, segment: SegmentId) -> bool {
        let mut next = self.spans[this].coin_end;
        while next != this {
            if self.spans[next].segment == segment {
                return true;
            }
            next = self.spans[next].coin_end;
        }
        false
    }

    pub fn span_insert_coin_end(&mut self, this: SpanId, coin: SpanId) {
        if self.span_contains_coin_end_span(this, coin) {
            return;
        }
        let coin_next = self.spans[coin].coin_end;
        self.spans[coin].coin_end = self.spans[this].coin_end;
        self.spans[this].coin_end = coin_next;
    }

    pub fn span_contains_coincidence_span(&self, this: SpanId, coin: SpanId) -> bool {
        let mut next = self.spans[this].coincident;
        while next != this {
            if next == coin {
                return true;
            }
            next = self.spans[next].coincident;
        }
        false
    }

    pub fn span_contains_coincidence_seg(&self, this: SpanId, segment: SegmentId) -> bool {
        let mut next = self.spans[this].coincident;
        loop {
            if self.spans[next].segment == segment {
                return true;
            }
            next = self.spans[next].coincident;
            if next == this {
                return false;
            }
        }
    }

    pub fn span_insert_coincidence_span(&mut self, this: SpanId, coin: SpanId) {
        if self.span_contains_coincidence_span(this, coin) {
            return;
        }
        let coin_next = self.spans[coin].coincident;
        self.spans[coin].coincident = self.spans[this].coincident;
        self.spans[this].coincident = coin_next;
    }

    pub fn span_insert_coincidence_seg(
        &mut self,
        this: SpanId,
        segment: SegmentId,
        flipped: bool,
        ordered: bool,
    ) -> bool {
        if self.span_contains_coincidence_seg(this, segment) {
            return true;
        }
        let start_pt_t = self.spans[this].pt_t;
        let mut next = self.pt_ts[start_pt_t].next;
        while next != start_pt_t {
            if self.pt_t_segment(next) == segment {
                let base = self.pt_ts[next].span;
                let span = if !ordered {
                    let this_next = match self.spans[this].next {
                        Some(n) => n,
                        None => return false,
                    };
                    let span_end_pt_t = match self.span_contains_seg(this_next, segment) {
                        Some(p) => p,
                        None => return false,
                    };
                    let span_end = self.pt_ts[span_end_pt_t].span;
                    let start = self.pt_t_starter(self.spans[base].pt_t, self.spans[span_end].pt_t);
                    let start_span = self.pt_ts[start].span;
                    if self.span_final(start_span) {
                        return false;
                    }
                    start_span
                } else if flipped {
                    match self.spans[base].prev {
                        Some(p) => p,
                        None => return false,
                    }
                } else {
                    if self.span_final(base) {
                        return false;
                    }
                    base
                };
                self.span_insert_coincidence_span(this, span);
                return true;
            }
            next = self.pt_ts[next].next;
        }
        true
    }

    pub fn span_merge(&mut self, this: SpanId, span: SpanId) {
        let this_pt_t = self.spans[this].pt_t;
        let span_pt_t = self.spans[span].pt_t;
        self.span_release(span, this_pt_t);
        if self.span_contains_span(this, span) {
            return;
        }
        let mut remainder = self.pt_ts[span_pt_t].next;
        self.pt_t_insert(this_pt_t, span_pt_t);
        while remainder != span_pt_t {
            let next = self.pt_ts[remainder].next;
            let mut compare = self.pt_ts[span_pt_t].next;
            let mut dup = false;
            while compare != span_pt_t {
                let next_c = self.pt_ts[compare].next;
                if self.pt_ts[next_c].span == self.pt_ts[remainder].span
                    && self.pt_ts[next_c].t == self.pt_ts[remainder].t
                {
                    dup = true;
                    break;
                }
                compare = next_c;
            }
            if !dup {
                self.pt_t_insert(span_pt_t, remainder);
            }
            remainder = next;
        }
        self.spans[this].span_adds += self.spans[span].span_adds;
    }

    pub fn span_check_for_collapsed_coincidence(&mut self, this: SpanId) {
        if self.coincidence.is_empty() {
            return;
        }
        let head = self.spans[this].pt_t;
        let mut test = head;
        loop {
            if self.pt_ts[test].coincident {
                self.coincidence_mark_collapsed(test);
            }
            test = self.pt_ts[test].next;
            if test == head {
                break;
            }
        }
        self.coincidence_release_deleted();
    }

    pub fn span_merge_matches(&mut self, this: SpanId, opp: SpanId) -> bool {
        let stop = self.spans[this].pt_t;
        let mut test = stop;
        let mut safety_hatch = 1_000_000;
        loop {
            safety_hatch -= 1;
            if safety_hatch == 0 {
                return false;
            }
            let test_next = self.pt_ts[test].next;
            if !self.pt_ts[test].deleted {
                let test_base = self.pt_ts[test].span;
                let segment = self.spans[test_base].segment;
                if !self.seg_done(segment) {
                    let inner_stop = self.spans[opp].pt_t;
                    let mut inner = inner_stop;
                    loop {
                        if self.pt_t_segment(inner) == segment && !self.pt_ts[inner].deleted {
                            let inner_base = self.pt_ts[inner].span;
                            if !zero_or_one(self.pt_ts[inner].t) {
                                self.span_release(inner_base, test);
                            } else if !zero_or_one(self.pt_ts[test].t) {
                                self.span_release(test_base, inner);
                            } else {
                                self.seg_mark_all_done(segment);
                                self.pt_ts[test].deleted = true;
                                self.pt_ts[inner].deleted = true;
                            }
                            break;
                        }
                        inner = self.pt_ts[inner].next;
                        if inner == inner_stop {
                            break;
                        }
                    }
                }
            }
            test = test_next;
            if test == stop {
                break;
            }
        }
        self.span_check_for_collapsed_coincidence(this);
        true
    }

    pub fn span_release(&mut self, this: SpanId, kept: PtTId) {
        let prev = self.spans[this].prev.unwrap();
        let next = self.spans[this].next.unwrap();
        self.spans[prev].next = Some(next);
        self.spans[next].prev = Some(prev);
        let seg = self.spans[this].segment;
        self.seg_release(seg, this);
        let this_pt_t = self.spans[this].pt_t;
        self.coincidence_fix_up(this_pt_t, kept);
        self.pt_ts[this_pt_t].deleted = true;
        let kept_span = self.pt_ts[kept].span;
        let mut test_pt_t = this_pt_t;
        loop {
            if self.pt_ts[test_pt_t].span == this {
                self.pt_ts[test_pt_t].span = kept_span;
            }
            test_pt_t = self.pt_ts[test_pt_t].next;
            if test_pt_t == this_pt_t {
                break;
            }
        }
    }

    pub fn span_set_opp_sum(&mut self, this: SpanId, opp_sum: i32) {
        if self.spans[this].opp_sum != SK_MIN_S32 && self.spans[this].opp_sum != opp_sum {
            self.winding_failed = true;
            return;
        }
        self.spans[this].opp_sum = opp_sum;
    }

    pub fn span_set_wind_sum(&mut self, this: SpanId, wind_sum: i32) {
        if self.spans[this].wind_sum != SK_MIN_S32 && self.spans[this].wind_sum != wind_sum {
            self.winding_failed = true;
            return;
        }
        self.spans[this].wind_sum = wind_sum;
    }

    pub fn span_compute_wind_sum(&mut self, this: SpanId) -> i32 {
        let contour_head = self.contour_head;
        let mut wind_try = 0;
        while !self.span_sortable_top(this, contour_head) {
            wind_try += 1;
            if wind_try >= MAX_WINDING_TRIES {
                break;
            }
        }
        self.spans[this].wind_sum
    }
}
