#![forbid(unsafe_code)]

//! Port of `SkPathOpsTSect.h/.cpp`.
//! Safe index-based arena implementation of Skia's T-section curve-curve intersection engine.

use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::intersections::Intersections;
use crate::line::DLine;
use crate::quad::DQuad;
use crate::types::{
    approximately_greater_than_one, approximately_less_than_zero,
    approximately_zero_when_compared_to, between, cpp_max_f64, cpp_min_f64,
    precisely_zero, precisely_zero_when_compared_to, roughly_between, DPoint, DRect, DVector,
};

const COINCIDENT_SPAN_COUNT: i32 = 9;
const ZERO_S1_SET: u32 = 1;
const ONE_S1_SET: u32 = 2;
const ZERO_S2_SET: u32 = 4;
const ONE_S2_SET: u32 = 8;

#[derive(Copy, Clone, Debug)]
pub enum TCurve {
    Quad(DQuad),
    Conic(DConic),
    Cubic(DCubic),
}

impl TCurve {
    #[inline]
    pub fn point_count(&self) -> usize {
        match self {
            TCurve::Quad(_) => DQuad::POINT_COUNT,
            TCurve::Conic(_) => DConic::POINT_COUNT,
            TCurve::Cubic(_) => DCubic::POINT_COUNT,
        }
    }

    #[inline]
    pub fn point_last(&self) -> usize {
        self.point_count() - 1
    }

    #[inline]
    pub fn max_intersections(&self) -> usize {
        match self {
            TCurve::Quad(_) => DQuad::MAX_INTERSECTIONS,
            TCurve::Conic(_) => DConic::MAX_INTERSECTIONS,
            TCurve::Cubic(_) => DCubic::MAX_INTERSECTIONS,
        }
    }

    #[inline]
    pub fn is_conic(&self) -> bool {
        matches!(self, TCurve::Conic(_))
    }

    #[inline]
    pub fn pt(&self, n: usize) -> DPoint {
        match self {
            TCurve::Quad(q) => q.pts[n],
            TCurve::Conic(c) => c.pts.pts[n],
            TCurve::Cubic(c) => c.pts[n],
        }
    }

    #[inline]
    pub fn collapsed(&self) -> bool {
        match self {
            TCurve::Quad(q) => q.collapsed(),
            TCurve::Conic(c) => c.collapsed(),
            TCurve::Cubic(c) => c.collapsed(),
        }
    }

    #[inline]
    pub fn controls_inside(&self) -> bool {
        match self {
            TCurve::Quad(q) => q.controls_inside(),
            TCurve::Conic(c) => c.controls_inside(),
            TCurve::Cubic(c) => c.controls_inside(),
        }
    }

    #[inline]
    pub fn pt_at_t(&self, t: f64) -> DPoint {
        match self {
            TCurve::Quad(q) => q.pt_at_t(t),
            TCurve::Conic(c) => c.pt_at_t(t),
            TCurve::Cubic(c) => c.pt_at_t(t),
        }
    }

    #[inline]
    pub fn dxdy_at_t(&self, t: f64) -> DVector {
        match self {
            TCurve::Quad(q) => q.dxdy_at_t(t),
            TCurve::Conic(c) => c.dxdy_at_t(t),
            TCurve::Cubic(c) => c.dxdy_at_t(t),
        }
    }

    #[inline]
    pub fn sub_divide(&self, t1: f64, t2: f64) -> TCurve {
        match self {
            TCurve::Quad(q) => TCurve::Quad(q.sub_divide(t1, t2)),
            TCurve::Conic(c) => TCurve::Conic(c.sub_divide(t1, t2)),
            TCurve::Cubic(c) => TCurve::Cubic(c.sub_divide(t1, t2)),
        }
    }

    #[inline]
    pub fn bounds(&self) -> DRect {
        match self {
            TCurve::Quad(q) => q.set_bounds(q, 0.0, 1.0),
            TCurve::Conic(c) => c.set_bounds(c, 0.0, 1.0),
            TCurve::Cubic(c) => c.set_bounds(c, 0.0, 1.0),
        }
    }

    #[inline]
    pub fn intersect_ray(&self, i: &mut Intersections, line: &DLine) -> usize {
        match self {
            TCurve::Quad(q) => i.intersect_ray_quad_line(q, line),
            TCurve::Conic(c) => i.intersect_ray_conic_line(c, line),
            TCurve::Cubic(c) => i.intersect_ray_cubic_line(c, line),
        }
    }

    pub fn hull_intersects(&self, other: &TCurve, is_linear: &mut bool) -> bool {
        match (self, other) {
            (TCurve::Quad(a), TCurve::Quad(b)) => a.hull_intersects_quad(b, is_linear),
            (TCurve::Quad(a), TCurve::Conic(b)) => b.hull_intersects_quad(a, is_linear),
            (TCurve::Quad(a), TCurve::Cubic(b)) => b.hull_intersects_quad(a, is_linear),
            (TCurve::Conic(a), TCurve::Quad(b)) => a.hull_intersects_quad(b, is_linear),
            (TCurve::Conic(a), TCurve::Conic(b)) => a.hull_intersects_conic(b, is_linear),
            (TCurve::Conic(a), TCurve::Cubic(b)) => b.hull_intersects_conic(a, is_linear),
            (TCurve::Cubic(a), TCurve::Quad(b)) => a.hull_intersects_quad(b, is_linear),
            (TCurve::Cubic(a), TCurve::Conic(b)) => a.hull_intersects_conic(b, is_linear),
            (TCurve::Cubic(a), TCurve::Cubic(b)) => a.hull_intersects_cubic(b, is_linear),
        }
    }

    pub fn other_pts(&self, index: usize, out: &mut [DPoint; 3]) -> usize {
        match self {
            TCurve::Quad(q) => {
                let p = q.other_pts(index);
                out[0] = p[0];
                out[1] = p[1];
                2
            }
            TCurve::Conic(c) => {
                let p = c.other_pts(index);
                out[0] = p[0];
                out[1] = p[1];
                2
            }
            TCurve::Cubic(c) => {
                let p = c.other_pts(index);
                out[0] = p[0];
                out[1] = p[1];
                out[2] = p[2];
                3
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct TCoincident {
    perp_pt: DPoint,
    perp_t: f64,
    is_match: bool,
}

impl Default for TCoincident {
    fn default() -> Self {
        Self::new()
    }
}

impl TCoincident {
    #[inline]
    fn new() -> Self {
        Self {
            perp_pt: DPoint::new(f64::NAN, f64::NAN),
            perp_t: -1.0,
            is_match: false,
        }
    }

    #[inline]
    fn init(&mut self) {
        self.perp_t = -1.0;
        self.is_match = false;
        self.perp_pt = DPoint::new(f64::NAN, f64::NAN);
    }

    #[inline]
    fn mark_coincident(&mut self) {
        if !self.is_match {
            self.perp_t = -1.0;
        }
        self.is_match = true;
    }

    fn set_perp(&mut self, c1: &TCurve, t: f64, c_pt: DPoint, c2: &TCurve) {
        let dxdy = c1.dxdy_at_t(t);
        let perp = DLine::new(c_pt, DPoint::new(c_pt.x + dxdy.y, c_pt.y - dxdy.x));
        let mut i = Intersections::new();
        let used = c2.intersect_ray(&mut i, &perp);
        if used == 0 || used == 3 {
            self.init();
            return;
        }
        self.perp_t = i.t[0][0];
        self.perp_pt = i.pt[0];
        if used == 2 {
            let dist_sq = self.perp_pt.sub(c_pt).length_squared();
            let dist2_sq = i.pt[1].sub(c_pt).length_squared();
            if dist2_sq < dist_sq {
                self.perp_t = i.t[0][1];
                self.perp_pt = i.pt[1];
            }
        }
        self.is_match = c_pt.approximately_equal(self.perp_pt);
    }
}

#[derive(Copy, Clone, Debug)]
struct TSpanBounded {
    bounded: usize,
    next: Option<usize>,
}

#[derive(Copy, Clone, Debug)]
struct TSpan {
    part: TCurve,
    coin_start: TCoincident,
    coin_end: TCoincident,
    bounded: Option<usize>,
    prev: Option<usize>,
    next: Option<usize>,
    bounds: DRect,
    start_t: f64,
    end_t: f64,
    bounds_max: f64,
    collapsed: bool,
    has_perp: bool,
    is_linear: bool,
    is_line: bool,
    deleted: bool,
}

impl TSpan {
    fn new(curve: TCurve) -> Self {
        Self {
            part: curve,
            coin_start: TCoincident::new(),
            coin_end: TCoincident::new(),
            bounded: None,
            prev: None,
            next: None,
            bounds: DRect::default(),
            start_t: 0.0,
            end_t: 1.0,
            bounds_max: 0.0,
            collapsed: false,
            has_perp: false,
            is_linear: false,
            is_line: false,
            deleted: false,
        }
    }

    #[inline]
    fn point_first(&self) -> DPoint {
        self.part.pt(0)
    }

    #[inline]
    fn point_last(&self) -> DPoint {
        self.part.pt(self.part.point_last())
    }

    fn init(&mut self, c: &TCurve) {
        self.prev = None;
        self.next = None;
        self.start_t = 0.0;
        self.end_t = 1.0;
        self.bounded = None;
        self.reset_bounds(c);
    }

    fn reset_bounds(&mut self, c: &TCurve) {
        self.is_linear = false;
        self.is_line = false;
        self.init_bounds(c);
    }

    fn init_bounds(&mut self, c: &TCurve) -> bool {
        if self.start_t.is_nan() || self.end_t.is_nan() {
            return false;
        }
        self.part = c.sub_divide(self.start_t, self.end_t);
        self.bounds = self.part.bounds();
        self.coin_start.init();
        self.coin_end.init();
        self.bounds_max = cpp_max_f64(self.bounds.width(), self.bounds.height());
        self.collapsed = self.part.collapsed();
        self.has_perp = false;
        self.deleted = false;
        self.bounds.valid()
    }

    fn mark_coincident(&mut self) {
        self.coin_start.mark_coincident();
        self.coin_end.mark_coincident();
    }

    fn only_end_points_in_common(
        &self,
        opp: &TSpan,
        start: &mut bool,
        opp_start: &mut bool,
        pts_in_common: &mut bool,
    ) -> bool {
        if opp.point_first() == self.point_first() {
            *start = true;
            *opp_start = true;
        } else if opp.point_first() == self.point_last() {
            *start = false;
            *opp_start = true;
        } else if opp.point_last() == self.point_first() {
            *start = true;
            *opp_start = false;
        } else if opp.point_last() == self.point_last() {
            *start = false;
            *opp_start = false;
        } else {
            *pts_in_common = false;
            return false;
        }
        *pts_in_common = true;
        let mut other_pts = [DPoint::default(); 3];
        let mut opp_other_pts = [DPoint::default(); 3];
        let base_index = if *start { 0 } else { self.part.point_last() };
        let c1 = self.part.other_pts(base_index, &mut other_pts);
        let c2 = opp
            .part
            .other_pts(if *opp_start { 0 } else { opp.part.point_last() }, &mut opp_other_pts);
        let base = self.part.pt(base_index);
        for o1 in 0..c1 {
            let v1 = other_pts[o1].sub(base);
            for o2 in 0..c2 {
                let v2 = opp_other_pts[o2].sub(base);
                if v2.dot(v1) >= 0.0 {
                    return false;
                }
            }
        }
        true
    }

    fn hull_check(&mut self, opp: &TSpan, start: &mut bool, opp_start: &mut bool) -> i32 {
        if self.is_linear {
            return -1;
        }
        let mut pts_in_common = false;
        if self.only_end_points_in_common(opp, start, opp_start, &mut pts_in_common) {
            return 2;
        }
        let mut linear = false;
        if self.part.hull_intersects(&opp.part, &mut linear) {
            if !linear {
                return 1;
            }
            self.is_linear = true;
            self.is_line = self.part.controls_inside();
            return if pts_in_common { 1 } else { -1 };
        }
        i32::from(pts_in_common) << 1
    }

    fn linear_intersects(&self, q2: &TCurve) -> i32 {
        let mut start = 0usize;
        let mut end = self.part.point_last();
        if !self.part.controls_inside() {
            let mut dist = 0.0f64;
            let pc = self.part.point_count();
            for outer in 0..(pc - 1) {
                for inner in (outer + 1)..pc {
                    let test = self.part.pt(outer).sub(self.part.pt(inner)).length_squared();
                    if dist > test {
                        continue;
                    }
                    dist = test;
                    start = outer;
                    end = inner;
                }
            }
        }
        let orig_x = self.part.pt(start).x;
        let orig_y = self.part.pt(start).y;
        let adj = self.part.pt(end).x - orig_x;
        let opp = self.part.pt(end).y - orig_y;
        let max_part = cpp_max_f64(adj.abs(), opp.abs());
        let mut sign = 0.0f64;
        for n in 0..q2.point_count() {
            let dx = q2.pt(n).y - orig_y;
            let dy = q2.pt(n).x - orig_x;
            let max_val = cpp_max_f64(max_part, cpp_max_f64(dx.abs(), dy.abs()));
            let test = (q2.pt(n).y - orig_y) * adj - (q2.pt(n).x - orig_x) * opp;
            if precisely_zero_when_compared_to(test, max_val) {
                return 1;
            }
            if approximately_zero_when_compared_to(test, max_val) {
                return 3;
            }
            if n == 0 {
                sign = test;
                continue;
            }
            if test * sign < 0.0 {
                return 1;
            }
        }
        0
    }
}

struct TSectState {
    curve: TCurve,
    head: Option<usize>,
    coincident: Option<usize>,
    deleted: Option<usize>,
    active_count: i32,
    removed_start_t: bool,
    removed_end_t: bool,
    hung: bool,
}

impl TSectState {
    fn new(curve: TCurve) -> Self {
        Self {
            curve,
            head: None,
            coincident: None,
            deleted: None,
            active_count: 0,
            removed_start_t: false,
            removed_end_t: false,
            hung: false,
        }
    }

    #[inline]
    fn point_last(&self) -> DPoint {
        self.curve.pt(self.curve.point_last())
    }

    #[inline]
    fn reset_removed_ends(&mut self) {
        self.removed_start_t = false;
        self.removed_end_t = false;
    }
}

fn is_parallel(this_line: &DLine, opp: &TCurve) -> bool {
    if !opp.is_conic() {
        return false;
    }
    let mut finds = 0i32;
    let mut this_perp = DLine::new(
        DPoint::new(
            this_line.pts[1].x + (this_line.pts[1].y - this_line.pts[0].y),
            this_line.pts[1].y + (this_line.pts[0].x - this_line.pts[1].x),
        ),
        this_line.pts[1],
    );
    let mut perp_ray_i = Intersections::new();
    opp.intersect_ray(&mut perp_ray_i, &this_perp);
    for p_index in 0..perp_ray_i.used() {
        finds += i32::from(perp_ray_i.pt[p_index].approximately_equal(this_perp.pts[1]));
    }
    this_perp.pts[1] = DPoint::new(
        this_line.pts[0].x + (this_line.pts[1].y - this_line.pts[0].y),
        this_line.pts[0].y + (this_line.pts[0].x - this_line.pts[1].x),
    );
    this_perp.pts[0] = this_line.pts[0];
    opp.intersect_ray(&mut perp_ray_i, &this_perp);
    for p_index in 0..perp_ray_i.used() {
        finds += i32::from(perp_ray_i.pt[p_index].approximately_equal(this_perp.pts[0]));
    }
    finds >= 2
}

struct TSectCtx {
    sects: [TSectState; 2],
    spans: Vec<TSpan>,
    bounded_nodes: Vec<TSpanBounded>,
}

impl TSectCtx {
    fn new(c1: TCurve, c2: TCurve) -> Self {
        let mut ctx = Self {
            sects: [TSectState::new(c1), TSectState::new(c2)],
            spans: Vec::with_capacity(32),
            bounded_nodes: Vec::with_capacity(64),
        };
        let h0 = ctx.add_one(0);
        ctx.sects[0].head = Some(h0);
        let c0 = ctx.sects[0].curve;
        ctx.spans[h0].init(&c0);

        let h1 = ctx.add_one(1);
        ctx.sects[1].head = Some(h1);
        let c1_curve = ctx.sects[1].curve;
        ctx.spans[h1].init(&c1_curve);

        ctx
    }

    fn add_one(&mut self, s: usize) -> usize {
        let idx = if let Some(del) = self.sects[s].deleted {
            self.sects[s].deleted = self.spans[del].next;
            del
        } else {
            let curve = self.sects[s].curve;
            let idx = self.spans.len();
            self.spans.push(TSpan::new(curve));
            idx
        };
        self.spans[idx].bounded = None;
        self.spans[idx].has_perp = false;
        self.spans[idx].deleted = false;
        self.sects[s].active_count += 1;
        idx
    }

    fn add_bounded(&mut self, span: usize, opp_span: usize) {
        let node_idx = self.bounded_nodes.len();
        let old_head = self.spans[span].bounded;
        self.bounded_nodes.push(TSpanBounded {
            bounded: opp_span,
            next: old_head,
        });
        self.spans[span].bounded = Some(node_idx);
    }

    fn span_contains(&self, mut work: Option<usize>, t: f64) -> bool {
        while let Some(w) = work {
            if between(self.spans[w].start_t, t, self.spans[w].end_t) {
                return true;
            }
            work = self.spans[w].next;
        }
        false
    }

    fn opp_t(&self, span: usize, t: f64) -> Option<usize> {
        let mut b = self.spans[span].bounded;
        while let Some(bi) = b {
            let test = self.bounded_nodes[bi].bounded;
            if between(self.spans[test].start_t, t, self.spans[test].end_t) {
                return Some(test);
            }
            b = self.bounded_nodes[bi].next;
        }
        None
    }

    fn find_opp_span(&self, span: usize, opp: usize) -> Option<usize> {
        let mut b = self.spans[span].bounded;
        while let Some(bi) = b {
            let test = self.bounded_nodes[bi].bounded;
            if opp == test {
                return Some(test);
            }
            b = self.bounded_nodes[bi].next;
        }
        None
    }

    fn has_bounded(&self, s: usize, span: usize) -> bool {
        let mut test = self.sects[s].head;
        while let Some(t) = test {
            if self.find_opp_span(t, span).is_some() {
                return true;
            }
            test = self.spans[t].next;
        }
        false
    }

    fn closest_bounded_t(&self, span: usize, pt: DPoint) -> f64 {
        let mut result = -1.0;
        let mut closest = f64::MAX;
        let mut b = self.spans[span].bounded;
        while let Some(bi) = b {
            let test = self.bounded_nodes[bi].bounded;
            let start_dist = self.spans[test].point_first().distance_squared(pt);
            if closest > start_dist {
                closest = start_dist;
                result = self.spans[test].start_t;
            }
            let end_dist = self.spans[test].point_last().distance_squared(pt);
            if closest > end_dist {
                closest = end_dist;
                result = self.spans[test].end_t;
            }
            b = self.bounded_nodes[bi].next;
        }
        result
    }

    fn hulls_intersect(
        &mut self,
        span: usize,
        opp: usize,
        start: &mut bool,
        opp_start: &mut bool,
    ) -> i32 {
        if !self.spans[span].bounds.intersects(&self.spans[opp].bounds) {
            return 0;
        }
        let opp_copy = self.spans[opp];
        let hull_sect = self.spans[span].hull_check(&opp_copy, start, opp_start);
        if hull_sect >= 0 {
            return hull_sect;
        }
        let span_copy = self.spans[span];
        let hull_sect = self.spans[opp].hull_check(&span_copy, opp_start, start);
        if hull_sect >= 0 {
            return hull_sect;
        }
        -1
    }

    fn linears_intersect(&self, span: usize, opp: usize) -> bool {
        let result = self.spans[span].linear_intersects(&self.spans[opp].part);
        if result <= 1 {
            return result != 0;
        }
        let result = self.spans[opp].linear_intersects(&self.spans[span].part);
        result != 0
    }

    fn remove_bounded(&mut self, span: usize, opp: usize) -> bool {
        if self.spans[span].has_perp {
            let mut found_start = false;
            let mut found_end = false;
            let coin_start_t = self.spans[span].coin_start.perp_t;
            let coin_end_t = self.spans[span].coin_end.perp_t;
            let mut b = self.spans[span].bounded;
            while let Some(bi) = b {
                let test = self.bounded_nodes[bi].bounded;
                if opp != test {
                    let st = self.spans[test].start_t;
                    let et = self.spans[test].end_t;
                    found_start |= between(st, coin_start_t, et);
                    found_end |= between(st, coin_end_t, et);
                }
                b = self.bounded_nodes[bi].next;
            }
            if !found_start || !found_end {
                self.spans[span].has_perp = false;
                self.spans[span].coin_start.init();
                self.spans[span].coin_end.init();
            }
        }
        let mut b = self.spans[span].bounded;
        let mut prev: Option<usize> = None;
        while let Some(bi) = b {
            let bounded_next = self.bounded_nodes[bi].next;
            if opp == self.bounded_nodes[bi].bounded {
                if let Some(p) = prev {
                    self.bounded_nodes[p].next = bounded_next;
                    return false;
                } else {
                    self.spans[span].bounded = bounded_next;
                    return bounded_next.is_none();
                }
            }
            prev = Some(bi);
            b = bounded_next;
        }
        false
    }

    fn remove_all_bounded(&mut self, span: usize) -> bool {
        let mut delete_span = false;
        let mut b = self.spans[span].bounded;
        while let Some(bi) = b {
            let opp = self.bounded_nodes[bi].bounded;
            delete_span |= self.remove_bounded(opp, span);
            b = self.bounded_nodes[bi].next;
        }
        delete_span
    }

    fn split_at(&mut self, new_span: usize, work: usize, t: f64) -> bool {
        self.spans[new_span].start_t = t;
        self.spans[new_span].end_t = self.spans[work].end_t;
        if self.spans[new_span].start_t == self.spans[new_span].end_t {
            self.spans[new_span].collapsed = true;
            return false;
        }
        self.spans[work].end_t = t;
        if self.spans[work].start_t == self.spans[work].end_t {
            self.spans[work].collapsed = true;
            return false;
        }
        let work_next = self.spans[work].next;
        self.spans[new_span].prev = Some(work);
        self.spans[new_span].next = work_next;
        self.spans[new_span].is_linear = self.spans[work].is_linear;
        self.spans[new_span].is_line = self.spans[work].is_line;
        self.spans[work].next = Some(new_span);
        if let Some(wn) = work_next {
            self.spans[wn].prev = Some(new_span);
        }
        let mut b = self.spans[work].bounded;
        self.spans[new_span].bounded = None;
        while let Some(bi) = b {
            let opp = self.bounded_nodes[bi].bounded;
            self.add_bounded(new_span, opp);
            b = self.bounded_nodes[bi].next;
        }
        let mut b = self.spans[new_span].bounded;
        while let Some(bi) = b {
            let opp = self.bounded_nodes[bi].bounded;
            self.add_bounded(opp, new_span);
            b = self.bounded_nodes[bi].next;
        }
        true
    }

    fn add_split_at(&mut self, s: usize, span: usize, t: f64) -> usize {
        let result = self.add_one(s);
        self.split_at(result, span, t);
        let curve = self.sects[s].curve;
        self.spans[result].init_bounds(&curve);
        self.spans[span].init_bounds(&curve);
        result
    }

    fn add_following(&mut self, s: usize, prior: Option<usize>) -> usize {
        let result = self.add_one(s);
        let start_t = match prior {
            Some(p) => self.spans[p].end_t,
            None => 0.0,
        };
        let next = match prior {
            Some(p) => self.spans[p].next,
            None => self.sects[s].head,
        };
        let end_t = match next {
            Some(n) => self.spans[n].start_t,
            None => 1.0,
        };
        self.spans[result].start_t = start_t;
        self.spans[result].end_t = end_t;
        self.spans[result].prev = prior;
        self.spans[result].next = next;
        if let Some(p) = prior {
            self.spans[p].next = Some(result);
        } else {
            self.sects[s].head = Some(result);
        }
        if let Some(n) = next {
            self.spans[n].prev = Some(result);
        }
        let curve = self.sects[s].curve;
        self.spans[result].reset_bounds(&curve);
        result
    }

    fn span_at_t(&self, s: usize, t: f64) -> (Option<usize>, Option<usize>) {
        let mut test = self.sects[s].head;
        let mut prev = None;
        while let Some(ti) = test {
            if self.spans[ti].end_t >= t {
                break;
            }
            prev = Some(ti);
            test = self.spans[ti].next;
        }
        let found = test.filter(|&ti| self.spans[ti].start_t <= t);
        (found, prev)
    }

    fn add_for_perp(&mut self, s: usize, span: usize, t: f64) {
        if self.opp_t(span, t).is_none() {
            let (opp_opt, prior_span) = self.span_at_t(s, t);
            let opp = match opp_opt {
                Some(o) => o,
                None => self.add_following(s, prior_span),
            };
            self.add_bounded(opp, span);
            self.add_bounded(span, opp);
        }
    }

    fn bounds_max(&mut self, s: usize) -> Option<usize> {
        let mut test = self.sects[s].head?;
        let mut largest = test;
        let mut l_collapsed = self.spans[largest].collapsed;
        let mut safety_net = 10000;
        while let Some(next) = self.spans[test].next {
            test = next;
            safety_net -= 1;
            if safety_net == 0 {
                self.sects[s].hung = true;
                return None;
            }
            let t_collapsed = self.spans[test].collapsed;
            if (l_collapsed && !t_collapsed)
                || (l_collapsed == t_collapsed
                    && self.spans[largest].bounds_max < self.spans[test].bounds_max)
            {
                largest = test;
                l_collapsed = t_collapsed;
            }
        }
        Some(largest)
    }

    fn tail(&self, s: usize) -> Option<usize> {
        let mut result = self.sects[s].head?;
        let mut next = result;
        let mut safety_net = 100000;
        while let Some(n) = self.spans[next].next {
            next = n;
            safety_net -= 1;
            if safety_net == 0 {
                return None;
            }
            if self.spans[next].end_t > self.spans[result].end_t {
                result = next;
            }
        }
        Some(result)
    }

    fn removed_end_check(&mut self, s: usize, span: usize) {
        if self.spans[span].start_t == 0.0 {
            self.sects[s].removed_start_t = true;
        }
        if self.spans[span].end_t == 1.0 {
            self.sects[s].removed_end_t = true;
        }
    }

    fn unlink_span(&mut self, s: usize, span: usize) -> bool {
        let prev = self.spans[span].prev;
        let next = self.spans[span].next;
        if let Some(p) = prev {
            self.spans[p].next = next;
            if let Some(n) = next {
                self.spans[n].prev = Some(p);
                if self.spans[n].start_t > self.spans[n].end_t {
                    return false;
                }
            }
        } else {
            self.sects[s].head = next;
            if let Some(n) = next {
                self.spans[n].prev = None;
            }
        }
        true
    }

    fn mark_span_gone(&mut self, s: usize, span: usize) -> bool {
        self.sects[s].active_count -= 1;
        if self.sects[s].active_count < 0 {
            return false;
        }
        self.spans[span].next = self.sects[s].deleted;
        self.sects[s].deleted = Some(span);
        self.spans[span].deleted = true;
        true
    }

    fn remove_span(&mut self, s: usize, span: usize) -> bool {
        self.removed_end_check(s, span);
        if !self.unlink_span(s, span) {
            return false;
        }
        self.mark_span_gone(s, span)
    }

    fn remove_all_but(&mut self, _s: usize, keep: usize, span: usize, opp_s: usize) {
        let mut test_bounded = self.spans[span].bounded;
        while let Some(bi) = test_bounded {
            let bounded = self.bounded_nodes[bi].bounded;
            let next = self.bounded_nodes[bi].next;
            if bounded != keep && !self.spans[bounded].deleted {
                let _ = self.remove_bounded(span, bounded);
                if self.remove_bounded(bounded, span) {
                    self.remove_span(opp_s, bounded);
                }
            }
            test_bounded = next;
        }
    }

    fn remove_spans(&mut self, s: usize, span: usize, opp_s: usize) -> bool {
        let mut bounded = self.spans[span].bounded;
        while let Some(bi) = bounded {
            let span_bounded = self.bounded_nodes[bi].bounded;
            let next = self.bounded_nodes[bi].next;
            if self.remove_bounded(span, span_bounded) {
                self.remove_span(s, span);
            }
            if self.remove_bounded(span_bounded, span) {
                self.remove_span(opp_s, span_bounded);
            }
            if self.spans[span].deleted && self.has_bounded(opp_s, span) {
                return false;
            }
            bounded = next;
        }
        true
    }

    fn delete_empty_spans(&mut self, s: usize) -> bool {
        let mut next = self.sects[s].head;
        let mut safety_hatch = 1000;
        while let Some(test) = next {
            next = self.spans[test].next;
            if self.spans[test].bounded.is_none() && !self.remove_span(s, test) {
                return false;
            }
            safety_hatch -= 1;
            if safety_hatch < 0 {
                return false;
            }
        }
        true
    }

    fn remove_span_range(&mut self, s: usize, first: usize, last: usize) {
        if first == last {
            return;
        }
        let final_span = self.spans[last].next;
        let mut next = self.spans[first].next;
        while let Some(span) = next {
            if Some(span) == final_span {
                break;
            }
            next = self.spans[span].next;
            self.mark_span_gone(s, span);
        }
        if let Some(f) = final_span {
            self.spans[f].prev = Some(first);
        }
        self.spans[first].next = final_span;
    }

    fn update_bounded(&mut self, first: usize, last: usize, opp_first: usize) -> bool {
        let mut test = Some(first);
        let final_span = self.spans[last].next;
        let mut delete_span = false;
        while let Some(t) = test {
            delete_span |= self.remove_all_bounded(t);
            test = self.spans[t].next;
            if test == final_span {
                break;
            }
        }
        self.spans[first].bounded = None;
        self.add_bounded(first, opp_first);
        delete_span
    }

    fn remove_coincident(&mut self, s: usize, span: usize, is_between: bool) -> bool {
        if !self.unlink_span(s, span) {
            return false;
        }
        if is_between || between(0.0, self.spans[span].coin_start.perp_t, 1.0) {
            self.sects[s].active_count -= 1;
            self.spans[span].next = self.sects[s].coincident;
            self.sects[s].coincident = Some(span);
        } else {
            self.mark_span_gone(s, span);
        }
        true
    }

    fn coincident_has_t(&self, s: usize, t: f64) -> bool {
        let mut test = self.sects[s].coincident;
        while let Some(ti) = test {
            if between(self.spans[ti].start_t, t, self.spans[ti].end_t) {
                return true;
            }
            test = self.spans[ti].next;
        }
        false
    }

    fn collapsed_count(&self, s: usize) -> usize {
        let mut result = 0usize;
        let mut test = self.sects[s].head;
        while let Some(ti) = test {
            if self.spans[ti].collapsed {
                result += 1;
            }
            test = self.spans[ti].next;
        }
        result
    }

    fn count_consecutive_spans(&self, first: usize) -> (i32, usize) {
        let mut consecutive = 1;
        let mut last = first;
        while let Some(next) = self.spans[last].next {
            if self.spans[next].start_t > self.spans[last].end_t {
                break;
            }
            consecutive += 1;
            last = next;
        }
        (consecutive, last)
    }

    fn compute_perpendiculars(&mut self, s: usize, opp_s: usize, first: usize, last: Option<usize>) {
        let last = match last {
            Some(l) => l,
            None => return,
        };
        let curve = self.sects[s].curve;
        let opp_curve = self.sects[opp_s].curve;
        let mut work = first;
        let mut prior: Option<usize> = None;
        loop {
            if !self.spans[work].has_perp && !self.spans[work].collapsed {
                if let Some(p) = prior {
                    self.spans[work].coin_start = self.spans[p].coin_end;
                } else {
                    let st = self.spans[work].start_t;
                    let pf = self.spans[work].point_first();
                    self.spans[work].coin_start.set_perp(&curve, st, pf, &opp_curve);
                }
                if self.spans[work].coin_start.is_match {
                    let perp_t = self.spans[work].coin_start.perp_t;
                    if self.coincident_has_t(opp_s, perp_t) {
                        self.spans[work].coin_start.init();
                    } else {
                        self.add_for_perp(opp_s, work, perp_t);
                    }
                }
                let et = self.spans[work].end_t;
                let pl = self.spans[work].point_last();
                self.spans[work].coin_end.set_perp(&curve, et, pl, &opp_curve);
                if self.spans[work].coin_end.is_match {
                    let perp_t = self.spans[work].coin_end.perp_t;
                    if self.coincident_has_t(opp_s, perp_t) {
                        self.spans[work].coin_end.init();
                    } else {
                        self.add_for_perp(opp_s, work, perp_t);
                    }
                }
                self.spans[work].has_perp = true;
            }
            if work == last {
                break;
            }
            prior = Some(work);
            match self.spans[work].next {
                Some(n) => work = n,
                None => break,
            }
        }
    }

    fn find_coincident_run(&self, first_in: usize, last_ptr: &mut Option<usize>) -> Option<usize> {
        let mut work = first_in;
        let mut last_candidate: Option<usize> = None;
        let mut first: Option<usize> = None;
        loop {
            if self.spans[work].coin_start.is_match {
                if !self.spans[work].coin_end.is_match {
                    break;
                }
                last_candidate = Some(work);
                if first.is_none() {
                    first = Some(work);
                }
            } else if first.is_some() && self.spans[work].collapsed {
                *last_ptr = last_candidate;
                return first;
            } else {
                last_candidate = None;
            }
            if Some(work) == *last_ptr {
                return first;
            }
            match self.spans[work].next {
                Some(n) => work = n,
                None => return None,
            }
        }
        if last_candidate.is_some() {
            *last_ptr = last_candidate;
        }
        first
    }

    fn binary_search_coin(
        &self,
        s: usize,
        opp_s: usize,
        t_start: f64,
        mut t_step: f64,
        result_t: &mut f64,
        opp_t: &mut f64,
        opp_first: &mut Option<usize>,
    ) -> bool {
        let curve = self.sects[s].curve;
        let opp = self.sects[opp_s].curve;
        let mut work = TSpan::new(curve);
        let mut result = t_start;
        work.start_t = t_start;
        work.end_t = t_start;
        let mut last = curve.pt_at_t(t_start);
        let mut opp_pt = DPoint::default();
        let mut flip = false;
        let mut contained = false;
        let down = t_step < 0.0;
        loop {
            t_step *= 0.5;
            work.start_t += t_step;
            if flip {
                t_step = -t_step;
                flip = false;
            }
            work.init_bounds(&curve);
            if work.collapsed {
                return false;
            }
            if last.approximately_equal(work.point_first()) {
                break;
            }
            last = work.point_first();
            work.coin_start.set_perp(&curve, work.start_t, last, &opp);
            if work.coin_start.is_match {
                let opp_t_test = work.coin_start.perp_t;
                if self.span_contains(self.sects[opp_s].head, opp_t_test) {
                    *opp_t = opp_t_test;
                    opp_pt = work.coin_start.perp_pt;
                    contained = true;
                    if if down {
                        result <= work.start_t
                    } else {
                        result >= work.start_t
                    } {
                        *opp_first = None;
                        return false;
                    }
                    result = work.start_t;
                    continue;
                }
            }
            t_step = -t_step;
            flip = true;
        }
        if !contained {
            return false;
        }
        if last.approximately_equal(curve.pt(0)) {
            result = 0.0;
        } else if last.approximately_equal(self.sects[s].point_last()) {
            result = 1.0;
        }
        if opp_pt.approximately_equal(opp.pt(0)) {
            *opp_t = 0.0;
        } else if opp_pt.approximately_equal(self.sects[opp_s].point_last()) {
            *opp_t = 1.0;
        }
        *result_t = result;
        true
    }

    fn extract_coincident(
        &mut self,
        s: usize,
        opp_s: usize,
        first_in: usize,
        last_in: usize,
        result: &mut Option<usize>,
    ) -> bool {
        let mut last_opt = Some(last_in);
        let first_opt = self.find_coincident_run(first_in, &mut last_opt);
        let (mut first, last) = match (first_opt, last_opt) {
            (Some(f), Some(l)) => (f, l),
            _ => {
                *result = None;
                return true;
            }
        };
        let start_t = self.spans[first].start_t;
        let mut opp_start_t = 0.0;
        let mut opp_end_t;
        let prev = self.spans[first].prev;
        let mut opp_first = self.opp_t(first, self.spans[first].coin_start.perp_t);
        let opp_matched = self.spans[first].coin_start.perp_t < self.spans[first].coin_end.perp_t;
        let mut coin_start = 0.0;
        let mut did_split = false;
        if let Some(p) = prev {
            if self.spans[p].end_t == start_t
                && self.binary_search_coin(
                    s,
                    opp_s,
                    start_t,
                    self.spans[p].start_t - start_t,
                    &mut coin_start,
                    &mut opp_start_t,
                    &mut opp_first,
                )
                && self.spans[p].start_t < coin_start
                && coin_start < start_t
            {
                if let Some(cut_first) = self.opp_t(p, opp_start_t) {
                    did_split = true;
                    let mut of = cut_first;
                    first = self.add_split_at(s, p, coin_start);
                    self.spans[first].mark_coincident();
                    self.spans[p].coin_end.mark_coincident();
                    if self.spans[of].start_t < opp_start_t && opp_start_t < self.spans[of].end_t {
                        let opp_half = self.add_split_at(opp_s, of, opp_start_t);
                        if opp_matched {
                            self.spans[of].coin_end.mark_coincident();
                            self.spans[opp_half].mark_coincident();
                            of = opp_half;
                        } else {
                            self.spans[of].mark_coincident();
                            self.spans[opp_half].coin_start.mark_coincident();
                        }
                    }
                    opp_first = Some(of);
                }
            }
        }
        if !did_split && opp_first.is_none() {
            return false;
        }
        let mut opp_last = self.opp_t(last, self.spans[last].coin_end.perp_t);
        if !opp_matched {
            core::mem::swap(&mut opp_first, &mut opp_last);
        }
        let opp_first = match opp_first {
            Some(of) => of,
            None => {
                *result = None;
                return true;
            }
        };
        let opp_last = match opp_last {
            Some(ol) => ol,
            None => {
                *result = None;
                return true;
            }
        };
        let mut delete_empty = self.update_bounded(first, last, opp_first);
        delete_empty |= self.update_bounded(opp_first, opp_last, first);
        self.remove_span_range(s, first, last);
        self.remove_span_range(opp_s, opp_first, opp_last);
        self.spans[first].end_t = self.spans[last].end_t;
        let curve = self.sects[s].curve;
        let opp_curve = self.sects[opp_s].curve;
        self.spans[first].reset_bounds(&curve);
        let fst = self.spans[first].start_t;
        let fpf = self.spans[first].point_first();
        self.spans[first].coin_start.set_perp(&curve, fst, fpf, &opp_curve);
        let fet = self.spans[first].end_t;
        let fpl = self.spans[first].point_last();
        self.spans[first].coin_end.set_perp(&curve, fet, fpl, &opp_curve);
        opp_start_t = self.spans[first].coin_start.perp_t;
        opp_end_t = self.spans[first].coin_end.perp_t;
        if between(0.0, opp_start_t, 1.0) && between(0.0, opp_end_t, 1.0) {
            if !opp_matched {
                core::mem::swap(&mut opp_start_t, &mut opp_end_t);
            }
            self.spans[opp_first].start_t = opp_start_t;
            self.spans[opp_first].end_t = opp_end_t;
            self.spans[opp_first].reset_bounds(&opp_curve);
        }
        let next_after = self.spans[first].next;
        if !self.remove_coincident(s, first, false) {
            return false;
        }
        if !self.remove_coincident(opp_s, opp_first, true) {
            return false;
        }
        if delete_empty {
            if !self.delete_empty_spans(s) || !self.delete_empty_spans(opp_s) {
                *result = None;
                return false;
            }
        }
        *result = match next_after {
            Some(na)
                if !self.spans[na].deleted
                    && self.sects[s].head.is_some()
                    && self.sects[opp_s].head.is_some() =>
            {
                Some(na)
            }
            _ => None,
        };
        true
    }

    fn coincident_check(&mut self, s: usize, opp_s: usize) -> bool {
        let mut first = match self.sects[s].head {
            Some(h) => h,
            None => return false,
        };
        loop {
            let (consecutive, last) = self.count_consecutive_spans(first);
            let next = self.spans[last].next;
            if consecutive >= COINCIDENT_SPAN_COUNT {
                self.compute_perpendiculars(s, opp_s, first, Some(last));
                let mut coin_start = Some(first);
                while let Some(cs) = coin_start {
                    if self.spans[last].deleted {
                        break;
                    }
                    let mut next_cs = None;
                    if !self.extract_coincident(s, opp_s, cs, last, &mut next_cs) {
                        return false;
                    }
                    coin_start = next_cs;
                }
                if self.sects[s].head.is_none() || self.sects[opp_s].head.is_none() {
                    break;
                }
                match next {
                    Some(n) if !self.spans[n].deleted => {}
                    _ => break,
                }
            }
            match next {
                Some(n) => first = n,
                None => break,
            }
        }
        true
    }

    fn coincident_force(&mut self, s: usize, opp_s: usize, start1s: f64, start1e: f64) {
        let first = match self.sects[s].head {
            Some(h) => h,
            None => return,
        };
        let last = match self.tail(s) {
            Some(l) => l,
            None => return,
        };
        let opp_first = match self.sects[opp_s].head {
            Some(h) => h,
            None => return,
        };
        let opp_last = match self.tail(opp_s) {
            Some(l) => l,
            None => return,
        };
        let mut delete_empty = self.update_bounded(first, last, opp_first);
        delete_empty |= self.update_bounded(opp_first, opp_last, first);
        self.remove_span_range(s, first, last);
        self.remove_span_range(opp_s, opp_first, opp_last);
        self.spans[first].start_t = start1s;
        self.spans[first].end_t = start1e;
        let curve = self.sects[s].curve;
        let opp_curve = self.sects[opp_s].curve;
        self.spans[first].reset_bounds(&curve);
        self.spans[first]
            .coin_start
            .set_perp(&curve, start1s, curve.pt(0), &opp_curve);
        self.spans[first].coin_end.set_perp(
            &curve,
            start1e,
            self.sects[s].point_last(),
            &opp_curve,
        );
        let cs_t = self.spans[first].coin_start.perp_t;
        let ce_t = self.spans[first].coin_end.perp_t;
        let opp_matched = cs_t < ce_t;
        let mut opp_start_t = if cs_t == -1.0 { 0.0 } else { cpp_max_f64(0.0, cs_t) };
        let mut opp_end_t = if ce_t == -1.0 { 1.0 } else { cpp_min_f64(1.0, ce_t) };
        if !opp_matched {
            core::mem::swap(&mut opp_start_t, &mut opp_end_t);
        }
        self.spans[opp_first].start_t = opp_start_t;
        self.spans[opp_first].end_t = opp_end_t;
        self.spans[opp_first].reset_bounds(&opp_curve);
        self.remove_coincident(s, first, false);
        self.remove_coincident(opp_s, opp_first, true);
        if delete_empty {
            self.delete_empty_spans(s);
            self.delete_empty_spans(opp_s);
        }
    }

    fn merge_coincidence(&mut self, s: usize, opp_s: usize) {
        let mut small_limit = 0.0;
        let curve = self.sects[s].curve;
        let opp_curve = self.sects[opp_s].curve;
        loop {
            let mut smaller: Option<usize> = None;
            let mut test = self.sects[s].coincident;
            if test.is_none() {
                return;
            }
            while let Some(ti) = test {
                if self.spans[ti].start_t >= small_limit {
                    let take = match smaller {
                        None => true,
                        Some(sm) => self.spans[sm].end_t >= self.spans[ti].start_t,
                    };
                    if take {
                        smaller = Some(ti);
                    }
                }
                test = self.spans[ti].next;
            }
            let smaller = match smaller {
                Some(sm) => sm,
                None => return,
            };
            small_limit = self.spans[smaller].end_t;
            let mut prior: Option<usize> = None;
            let mut larger: Option<usize> = None;
            let mut larger_prior: Option<usize> = None;
            let mut test = self.sects[s].coincident;
            while let Some(ti) = test {
                if self.spans[ti].start_t >= self.spans[smaller].end_t {
                    let take = match larger {
                        None => true,
                        Some(lg) => self.spans[lg].start_t >= self.spans[ti].start_t,
                    };
                    if take {
                        larger_prior = prior;
                        larger = Some(ti);
                    }
                }
                prior = Some(ti);
                test = self.spans[ti].next;
            }
            let larger = match larger {
                Some(lg) => lg,
                None => continue,
            };
            let mid_t = (self.spans[smaller].end_t + self.spans[larger].start_t) / 2.0;
            let mid_pt = curve.pt_at_t(mid_t);
            let mut coin = TCoincident::new();
            coin.set_perp(&curve, mid_t, mid_pt, &opp_curve);
            if coin.is_match {
                self.spans[smaller].end_t = self.spans[larger].end_t;
                self.spans[smaller].coin_end = self.spans[larger].coin_end;
                let larger_next = self.spans[larger].next;
                if let Some(lp) = larger_prior {
                    self.spans[lp].next = larger_next;
                } else {
                    self.sects[s].coincident = larger_next;
                }
            }
        }
    }

    fn recover_collapsed(&mut self, s: usize) {
        let mut deleted = self.sects[s].deleted;
        while let Some(del) = deleted {
            let del_next = self.spans[del].next;
            if self.spans[del].collapsed {
                let del_start = self.spans[del].start_t;
                let mut prev: Option<usize> = None;
                let mut cur = self.sects[s].head;
                while let Some(c) = cur {
                    if self.spans[c].end_t > del_start {
                        break;
                    }
                    prev = Some(c);
                    cur = self.spans[c].next;
                }
                self.spans[del].next = cur;
                if let Some(p) = prev {
                    self.spans[p].next = Some(del);
                } else {
                    self.sects[s].head = Some(del);
                }
            }
            deleted = del_next;
        }
    }

    fn remove_by_perpendicular(&mut self, s: usize, opp_s: usize) -> bool {
        let mut test = self.sects[s].head;
        while let Some(ti) = test {
            let next = self.spans[ti].next;
            if self.spans[ti].coin_start.perp_t >= 0.0 && self.spans[ti].coin_end.perp_t >= 0.0 {
                let start_v = self.spans[ti].coin_start.perp_pt.sub(self.spans[ti].point_first());
                let end_v = self.spans[ti].coin_end.perp_pt.sub(self.spans[ti].point_last());
                if start_v.dot(end_v) > 0.0 && !self.remove_spans(s, ti, opp_s) {
                    return false;
                }
            }
            test = next;
        }
        true
    }

    fn lines_intersect(
        &self,
        s: usize,
        span: usize,
        opp_s: usize,
        opp_span: usize,
        i: &mut Intersections,
    ) -> usize {
        let mut this_ray_i = Intersections::new();
        let mut opp_ray_i = Intersections::new();
        let mut this_line = DLine::new(self.spans[span].point_first(), self.spans[span].point_last());
        let mut opp_line = DLine::new(
            self.spans[opp_span].point_first(),
            self.spans[opp_span].point_last(),
        );
        let curve = self.sects[s].curve;
        let opp_curve = self.sects[opp_s].curve;
        let mut loop_count = 0i32;
        let mut best_dist_sq = f64::MAX;
        if opp_curve.intersect_ray(&mut this_ray_i, &this_line) == 0 {
            return 0;
        }
        if curve.intersect_ray(&mut opp_ray_i, &opp_line) == 0 {
            return 0;
        }
        if this_ray_i.used() > 1 {
            let mut pt_matches = 0i32;
            for t_index in 0..this_ray_i.used() {
                for l_index in 0..2 {
                    pt_matches +=
                        i32::from(this_ray_i.pt[t_index].approximately_equal(this_line.pts[l_index]));
                }
            }
            if pt_matches == 2 || is_parallel(&this_line, &opp_curve) {
                return 2;
            }
        }
        if opp_ray_i.used() > 1 {
            let mut pt_matches = 0i32;
            for o_index in 0..opp_ray_i.used() {
                for l_index in 0..2 {
                    pt_matches +=
                        i32::from(opp_ray_i.pt[o_index].approximately_equal(opp_line.pts[l_index]));
                }
            }
            if pt_matches == 2 || is_parallel(&opp_line, &curve) {
                return 2;
            }
        }
        let span_start_t = self.spans[span].start_t;
        let span_end_t = self.spans[span].end_t;
        let opp_span_start_t = self.spans[opp_span].start_t;
        let opp_span_end_t = self.spans[opp_span].end_t;
        loop {
            let mut closest = f64::MAX;
            let mut close_index = 0usize;
            let mut opp_close_index = 0usize;
            for index in 0..opp_ray_i.used() {
                if !roughly_between(span_start_t, opp_ray_i.t[0][index], span_end_t) {
                    continue;
                }
                for o_index in 0..this_ray_i.used() {
                    if !roughly_between(opp_span_start_t, this_ray_i.t[0][o_index], opp_span_end_t) {
                        continue;
                    }
                    let dist_sq = this_ray_i.pt[index].distance_squared(opp_ray_i.pt[o_index]);
                    if closest > dist_sq {
                        closest = dist_sq;
                        close_index = index;
                        opp_close_index = o_index;
                    }
                }
            }
            if closest == f64::MAX {
                break;
            }
            let opp_i_pt = this_ray_i.pt[opp_close_index];
            let i_pt = opp_ray_i.pt[close_index];
            if between(span_start_t, opp_ray_i.t[0][close_index], span_end_t)
                && between(
                    opp_span_start_t,
                    this_ray_i.t[0][opp_close_index],
                    opp_span_end_t,
                )
                && opp_i_pt.approximately_equal(i_pt)
            {
                i.merge(&opp_ray_i, close_index, &this_ray_i, opp_close_index);
                return i.used();
            }
            let dist_sq = opp_i_pt.distance_squared(i_pt);
            loop_count += 1;
            if best_dist_sq < dist_sq || loop_count > 5 {
                return 0;
            }
            best_dist_sq = dist_sq;
            let opp_start = opp_ray_i.t[0][close_index];
            this_line.pts[0] = curve.pt_at_t(opp_start);
            this_line.pts[1] = this_line.pts[0].add_vec(curve.dxdy_at_t(opp_start));
            if opp_curve.intersect_ray(&mut this_ray_i, &this_line) == 0 {
                break;
            }
            let start = this_ray_i.t[0][opp_close_index];
            opp_line.pts[0] = opp_curve.pt_at_t(start);
            opp_line.pts[1] = opp_line.pts[0].add_vec(opp_curve.dxdy_at_t(start));
            if curve.intersect_ray(&mut opp_ray_i, &opp_line) == 0 {
                break;
            }
        }
        // convergence may fail if the curves are nearly coincident
        let mut o_coin_s = TCoincident::new();
        let mut o_coin_e = TCoincident::new();
        o_coin_s.set_perp(&opp_curve, opp_span_start_t, self.spans[opp_span].point_first(), &curve);
        o_coin_e.set_perp(&opp_curve, opp_span_end_t, self.spans[opp_span].point_last(), &curve);
        let mut t_start = o_coin_s.perp_t;
        let mut t_end = o_coin_e.perp_t;
        let swap = t_start > t_end;
        if swap {
            core::mem::swap(&mut t_start, &mut t_end);
        }
        t_start = cpp_max_f64(t_start, span_start_t);
        t_end = cpp_min_f64(t_end, span_end_t);
        if t_start > t_end {
            return 0;
        }
        let perp_s = if t_start == span_start_t {
            let mut coin_s = TCoincident::new();
            coin_s.set_perp(&curve, span_start_t, self.spans[span].point_first(), &opp_curve);
            self.spans[span].point_first().sub(coin_s.perp_pt)
        } else if swap {
            o_coin_e.perp_pt.sub(self.spans[opp_span].point_last())
        } else {
            o_coin_s.perp_pt.sub(self.spans[opp_span].point_first())
        };
        let perp_e = if t_end == span_end_t {
            let mut coin_e = TCoincident::new();
            coin_e.set_perp(&curve, span_end_t, self.spans[span].point_last(), &opp_curve);
            self.spans[span].point_last().sub(coin_e.perp_pt)
        } else if swap {
            o_coin_s.perp_pt.sub(self.spans[opp_span].point_first())
        } else {
            o_coin_e.perp_pt.sub(self.spans[opp_span].point_last())
        };
        if perp_s.dot(perp_e) >= 0.0 {
            return 0;
        }
        let mut coin_w = TCoincident::new();
        let mut work_t = t_start;
        let mut t_step = t_end - t_start;
        let mut work_pt;
        loop {
            t_step *= 0.5;
            if precisely_zero(t_step) {
                return 0;
            }
            work_t += t_step;
            work_pt = curve.pt_at_t(work_t);
            coin_w.set_perp(&curve, work_t, work_pt, &opp_curve);
            let perp_t = coin_w.perp_t;
            if if coin_w.is_match {
                !between(opp_span_start_t, perp_t, opp_span_end_t)
            } else {
                perp_t < 0.0
            } {
                continue;
            }
            let perp_w = work_pt.sub(coin_w.perp_pt);
            if (perp_s.dot(perp_w) >= 0.0) == (t_step < 0.0) {
                t_step = -t_step;
            }
            if work_pt.approximately_equal(coin_w.perp_pt) {
                break;
            }
        }
        let opp_t_test = coin_w.perp_t;
        if !self.span_contains(self.sects[opp_s].head, opp_t_test) {
            return 0;
        }
        i.set_max(1);
        i.insert(work_t, opp_t_test, work_pt);
        1
    }

    fn intersects(
        &mut self,
        s: usize,
        span: usize,
        opp_s: usize,
        opp_span: usize,
        opp_result: &mut i32,
    ) -> i32 {
        let mut span_start = false;
        let mut opp_start = false;
        let mut hull_result = self.hulls_intersect(span, opp_span, &mut span_start, &mut opp_start);
        if hull_result >= 0 {
            if hull_result == 2 {
                let single_bounded = match self.spans[span].bounded {
                    None => true,
                    Some(bi) => self.bounded_nodes[bi].next.is_none(),
                };
                if single_bounded {
                    if span_start {
                        self.spans[span].end_t = self.spans[span].start_t;
                    } else {
                        self.spans[span].start_t = self.spans[span].end_t;
                    }
                } else {
                    hull_result = 1;
                }
                let opp_single = match self.spans[opp_span].bounded {
                    None => true,
                    Some(bi) => self.bounded_nodes[bi].next.is_none(),
                };
                if opp_single {
                    if let Some(bi) = self.spans[opp_span].bounded {
                        if self.bounded_nodes[bi].bounded != span {
                            return 0;
                        }
                    }
                    if opp_start {
                        self.spans[opp_span].end_t = self.spans[opp_span].start_t;
                    } else {
                        self.spans[opp_span].start_t = self.spans[opp_span].end_t;
                    }
                    *opp_result = 2;
                } else {
                    *opp_result = 1;
                }
            } else {
                *opp_result = 1;
            }
            return hull_result;
        }
        if self.spans[span].is_line && self.spans[opp_span].is_line {
            let mut i = Intersections::new();
            let sects = self.lines_intersect(s, span, opp_s, opp_span, &mut i);
            if sects == 2 {
                *opp_result = 1;
                return 1;
            }
            if sects == 0 {
                return -1;
            }
            self.removed_end_check(s, span);
            self.spans[span].start_t = i.t[0][0];
            self.spans[span].end_t = i.t[0][0];
            self.removed_end_check(opp_s, opp_span);
            self.spans[opp_span].start_t = i.t[1][0];
            self.spans[opp_span].end_t = i.t[1][0];
            *opp_result = 2;
            return 2;
        }
        if self.spans[span].is_linear || self.spans[opp_span].is_linear {
            let r = i32::from(self.linears_intersect(span, opp_span));
            *opp_result = r;
            return r;
        }
        *opp_result = 1;
        1
    }

    fn trim(&mut self, s: usize, span: usize, opp_s: usize) -> bool {
        let curve = self.sects[s].curve;
        if !self.spans[span].init_bounds(&curve) {
            return false;
        }
        let opp_curve = self.sects[opp_s].curve;
        let mut test_bounded = self.spans[span].bounded;
        while let Some(bi) = test_bounded {
            let test = self.bounded_nodes[bi].bounded;
            let next = self.bounded_nodes[bi].next;
            let mut opp_sects = 0;
            let sects = self.intersects(s, span, opp_s, test, &mut opp_sects);
            if sects >= 1 {
                if opp_sects == 2 {
                    self.spans[test].init_bounds(&opp_curve);
                    self.remove_all_but(opp_s, span, test, s);
                }
                if sects == 2 {
                    self.spans[span].init_bounds(&curve);
                    self.remove_all_but(s, test, span, opp_s);
                    return true;
                }
            } else {
                if self.remove_bounded(span, test) {
                    self.remove_span(s, span);
                }
                if self.remove_bounded(test, span) {
                    self.remove_span(opp_s, test);
                }
            }
            test_bounded = next;
        }
        true
    }

    fn ends_equal(&self, intersections: &mut Intersections) -> u32 {
        let mut zero_one_set = 0u32;
        let c1 = &self.sects[0].curve;
        let c2 = &self.sects[1].curve;
        let c1_0 = c1.pt(0);
        let c1_last = self.sects[0].point_last();
        let c2_0 = c2.pt(0);
        let c2_last = self.sects[1].point_last();

        if c1_0 == c2_0 {
            zero_one_set |= ZERO_S1_SET | ZERO_S2_SET;
            intersections.insert(0.0, 0.0, c1_0);
        }
        if c1_0 == c2_last {
            zero_one_set |= ZERO_S1_SET | ONE_S2_SET;
            intersections.insert(0.0, 1.0, c1_0);
        }
        if c1_last == c2_0 {
            zero_one_set |= ONE_S1_SET | ZERO_S2_SET;
            intersections.insert(1.0, 0.0, c1_last);
        }
        if c1_last == c2_last {
            zero_one_set |= ONE_S1_SET | ONE_S2_SET;
            intersections.insert(1.0, 1.0, c1_last);
        }
        if (zero_one_set & (ZERO_S1_SET | ZERO_S2_SET)) == 0 && c1_0.approximately_equal(c2_0) {
            zero_one_set |= ZERO_S1_SET | ZERO_S2_SET;
            intersections.insert_near(0.0, 0.0, c1_0, c2_0);
        }
        if (zero_one_set & (ZERO_S1_SET | ONE_S2_SET)) == 0 && c1_0.approximately_equal(c2_last) {
            zero_one_set |= ZERO_S1_SET | ONE_S2_SET;
            intersections.insert_near(0.0, 1.0, c1_0, c2_last);
        }
        if (zero_one_set & (ONE_S1_SET | ZERO_S2_SET)) == 0 && c1_last.approximately_equal(c2_0) {
            zero_one_set |= ONE_S1_SET | ZERO_S2_SET;
            intersections.insert_near(1.0, 0.0, c1_last, c2_0);
        }
        if (zero_one_set & (ONE_S1_SET | ONE_S2_SET)) == 0 && c1_last.approximately_equal(c2_last)
        {
            zero_one_set |= ONE_S1_SET | ONE_S2_SET;
            intersections.insert_near(1.0, 1.0, c1_last, c2_last);
        }
        zero_one_set
    }

    fn binary_search(&mut self, intersections: &mut Intersections) {
        intersections.reset();
        intersections.set_max(self.sects[0].curve.max_intersections() + 4);
        let span1 = self.sects[0].head.unwrap();
        let span2 = self.sects[1].head.unwrap();
        let mut opp_sect = 0;
        let sect = self.intersects(0, span1, 1, span2, &mut opp_sect);
        if sect == 0 {
            return;
        }
        if sect == 2 && opp_sect == 2 {
            let _ = self.ends_equal(intersections);
            return;
        }
        self.add_bounded(span1, span2);
        self.add_bounded(span2, span1);
        const MAX_COIN_LOOP_COUNT: i32 = 8;
        let mut coin_loop_count = MAX_COIN_LOOP_COUNT;
        let mut start1s = 0.0;
        let mut start1e = 1.0;
        loop {
            let largest1 = match self.bounds_max(0) {
                Some(l) => l,
                None => {
                    if self.sects[0].hung {
                        return;
                    }
                    break;
                }
            };
            let largest2 = self.bounds_max(1);
            let pick_1 = match largest2 {
                None => true,
                Some(l2) => {
                    self.spans[largest1].bounds_max > self.spans[l2].bounds_max
                        || (!self.spans[largest1].collapsed && self.spans[l2].collapsed)
                }
            };
            if pick_1 {
                if self.sects[1].hung {
                    return;
                }
                if self.spans[largest1].collapsed {
                    break;
                }
                self.sects[0].reset_removed_ends();
                self.sects[1].reset_removed_ends();
                let half1 = self.add_one(0);
                let mid_t = (self.spans[largest1].start_t + self.spans[largest1].end_t) * 0.5;
                if !self.split_at(half1, largest1, mid_t) {
                    break;
                }
                if !self.trim(0, largest1, 1) {
                    return;
                }
                if !self.trim(0, half1, 1) {
                    return;
                }
            } else {
                let largest2 = largest2.unwrap();
                if self.spans[largest2].collapsed {
                    break;
                }
                self.sects[0].reset_removed_ends();
                self.sects[1].reset_removed_ends();
                let half2 = self.add_one(1);
                let mid_t = (self.spans[largest2].start_t + self.spans[largest2].end_t) * 0.5;
                if !self.split_at(half2, largest2, mid_t) {
                    break;
                }
                if !self.trim(1, largest2, 0) {
                    return;
                }
                if !self.trim(1, half2, 0) {
                    return;
                }
            }
            if self.sects[0].active_count >= COINCIDENT_SPAN_COUNT
                && self.sects[1].active_count >= COINCIDENT_SPAN_COUNT
            {
                if coin_loop_count == MAX_COIN_LOOP_COUNT {
                    if let (Some(h), Some(t)) = (self.sects[0].head, self.tail(0)) {
                        start1s = self.spans[h].start_t;
                        start1e = self.spans[t].end_t;
                    }
                }
                if !self.coincident_check(0, 1) {
                    return;
                }
                coin_loop_count -= 1;
                if coin_loop_count == 0
                    && self.sects[0].head.is_some()
                    && self.sects[1].head.is_some()
                {
                    self.coincident_force(0, 1, start1s, start1e);
                }
            }
            if self.sects[0].active_count >= COINCIDENT_SPAN_COUNT
                && self.sects[1].active_count >= COINCIDENT_SPAN_COUNT
            {
                let h0 = match self.sects[0].head {
                    Some(h) => h,
                    None => return,
                };
                let t0 = self.tail(0);
                self.compute_perpendiculars(0, 1, h0, t0);
                let h1 = match self.sects[1].head {
                    Some(h) => h,
                    None => return,
                };
                let t1 = self.tail(1);
                self.compute_perpendiculars(1, 0, h1, t1);
                if !self.remove_by_perpendicular(0, 1) {
                    return;
                }
                if self.collapsed_count(0) > self.sects[0].curve.max_intersections() {
                    break;
                }
            }
            if self.sects[0].head.is_none() || self.sects[1].head.is_none() {
                break;
            }
        }
        if let Some(coin_head) = self.sects[0].coincident {
            if self.spans[coin_head].next.is_some() {
                self.merge_coincidence(0, 1);
            }
            let mut coin = self.sects[0].coincident;
            loop {
                let c = match coin {
                    Some(ci) => ci,
                    None => return,
                };
                if self.spans[c].coin_start.is_match && self.spans[c].coin_end.is_match {
                    let perp_t = self.spans[c].coin_start.perp_t;
                    if perp_t < 0.0 {
                        return;
                    }
                    let index = intersections.insert_coincident(
                        self.spans[c].start_t,
                        perp_t,
                        self.spans[c].point_first(),
                    );
                    if intersections.insert_coincident(
                        self.spans[c].end_t,
                        self.spans[c].coin_end.perp_t,
                        self.spans[c].point_last(),
                    ) < 0
                        && index >= 0
                    {
                        intersections.clear_coincidence(index as usize);
                    }
                }
                coin = self.spans[c].next;
                if coin.is_none() {
                    break;
                }
            }
        }
        let zero_one_set = self.ends_equal(intersections);
        let c1 = self.sects[0].curve;
        let c2 = self.sects[1].curve;
        if self.sects[0].removed_start_t && (zero_one_set & ZERO_S1_SET) == 0 {
            let mut perp = TCoincident::new();
            perp.set_perp(&c1, 0.0, c1.pt(0), &c2);
            if perp.is_match {
                intersections.insert(0.0, perp.perp_t, perp.perp_pt);
            }
        }
        if self.sects[0].removed_end_t && (zero_one_set & ONE_S1_SET) == 0 {
            let mut perp = TCoincident::new();
            perp.set_perp(&c1, 1.0, self.sects[0].point_last(), &c2);
            if perp.is_match {
                intersections.insert(1.0, perp.perp_t, perp.perp_pt);
            }
        }
        if self.sects[1].removed_start_t && (zero_one_set & ZERO_S2_SET) == 0 {
            let mut perp = TCoincident::new();
            perp.set_perp(&c2, 0.0, c2.pt(0), &c1);
            if perp.is_match {
                intersections.insert(perp.perp_t, 0.0, perp.perp_pt);
            }
        }
        if self.sects[1].removed_end_t && (zero_one_set & ONE_S2_SET) == 0 {
            let mut perp = TCoincident::new();
            perp.set_perp(&c2, 1.0, self.sects[1].point_last(), &c1);
            if perp.is_match {
                intersections.insert(perp.perp_t, 1.0, perp.perp_pt);
            }
        }
        if self.sects[0].head.is_none() || self.sects[1].head.is_none() {
            return;
        }
        self.recover_collapsed(0);
        self.recover_collapsed(1);
        let head1 = self.sects[0].head.unwrap();
        if (zero_one_set & ZERO_S1_SET) == 0 && approximately_less_than_zero(self.spans[head1].start_t)
        {
            let start1 = c1.pt(0);
            if self.spans[head1].bounded.is_some() {
                let t = self.closest_bounded_t(head1, start1);
                if c2.pt_at_t(t).approximately_equal(start1) {
                    intersections.insert(0.0, t, start1);
                }
            }
        }
        let head2 = self.sects[1].head.unwrap();
        if (zero_one_set & ZERO_S2_SET) == 0 && approximately_less_than_zero(self.spans[head2].start_t)
        {
            let start2 = c2.pt(0);
            if self.spans[head2].bounded.is_some() {
                let t = self.closest_bounded_t(head2, start2);
                if c1.pt_at_t(t).approximately_equal(start2) {
                    intersections.insert(t, 0.0, start2);
                }
            }
        }
        if (zero_one_set & ONE_S1_SET) == 0 {
            let tail1 = match self.tail(0) {
                Some(t) => t,
                None => return,
            };
            if approximately_greater_than_one(self.spans[tail1].end_t) {
                let end1 = self.sects[0].point_last();
                if self.spans[tail1].bounded.is_some() {
                    let t = self.closest_bounded_t(tail1, end1);
                    if c2.pt_at_t(t).approximately_equal(end1) {
                        intersections.insert(1.0, t, end1);
                    }
                }
            }
        }
        if (zero_one_set & ONE_S2_SET) == 0 {
            let tail2 = match self.tail(1) {
                Some(t) => t,
                None => return,
            };
            if approximately_greater_than_one(self.spans[tail2].end_t) {
                let end2 = self.sects[1].point_last();
                if self.spans[tail2].bounded.is_some() {
                    let t = self.closest_bounded_t(tail2, end2);
                    if c1.pt_at_t(t).approximately_equal(end2) {
                        intersections.insert(t, 1.0, end2);
                    }
                }
            }
        }
        let mut closest = ClosestSect::new();
        let mut result1 = Some(head1);
        while let Some(mut r1) = result1 {
            while self.spans[r1].coin_start.is_match && self.spans[r1].coin_end.is_match {
                match self.spans[r1].next {
                    Some(n) => r1 = n,
                    None => {
                        result1 = None;
                        break;
                    }
                }
            }
            if result1.is_none() {
                break;
            }
            let mut result2 = self.sects[1].head;
            while let Some(r2) = result2 {
                closest.find(&self.spans, r1, r2);
                result2 = self.spans[r2].next;
            }
            result1 = self.spans[r1].next;
        }
        closest.finish(&self.spans, intersections);

        let mut last = intersections.used() as i32 - 1;
        let mut index = 0i32;
        while index < last {
            let idx = index as usize;
            if intersections.is_coincident(idx) && intersections.is_coincident(idx + 1) {
                index += 1;
                continue;
            }
            let mid_t = (intersections.t[0][idx] + intersections.t[0][idx + 1]) / 2.0;
            let mid_pt = c1.pt_at_t(mid_t);
            let mut perp = TCoincident::new();
            perp.set_perp(&c1, mid_t, mid_pt, &c2);
            if !perp.is_match {
                index += 1;
                continue;
            }
            if intersections.is_coincident(idx) {
                intersections.remove_one(idx);
                last -= 1;
            } else if intersections.is_coincident(idx + 1) {
                intersections.remove_one(idx + 1);
                last -= 1;
            } else {
                intersections.set_coincident(idx);
                index += 1;
            }
            intersections.set_coincident(index as usize);
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct ClosestRecord {
    c1_span: usize,
    c2_span: usize,
    c1_start_t: f64,
    c1_end_t: f64,
    c2_start_t: f64,
    c2_end_t: f64,
    closest: f64,
    c1_index: usize,
    c2_index: usize,
}

impl ClosestRecord {
    fn new() -> Self {
        Self {
            c1_span: 0,
            c2_span: 0,
            c1_start_t: 0.0,
            c1_end_t: 0.0,
            c2_start_t: 0.0,
            c2_end_t: 0.0,
            closest: f32::MAX as f64,
            c1_index: 0,
            c2_index: 0,
        }
    }

    fn reset(&mut self) {
        self.closest = f32::MAX as f64;
    }

    fn find_end(&mut self, spans: &[TSpan], span1: usize, span2: usize, c1_idx: usize, c2_idx: usize) {
        let p1 = spans[span1].part.pt(c1_idx);
        let p2 = spans[span2].part.pt(c2_idx);
        if !p1.approximately_equal(p2) {
            return;
        }
        let dist = p1.distance_squared(p2);
        if self.closest < dist {
            return;
        }
        self.c1_span = span1;
        self.c2_span = span2;
        self.c1_start_t = spans[span1].start_t;
        self.c1_end_t = spans[span1].end_t;
        self.c2_start_t = spans[span2].start_t;
        self.c2_end_t = spans[span2].end_t;
        self.c1_index = c1_idx;
        self.c2_index = c2_idx;
        self.closest = dist;
    }

    fn mates_with(&self, spans: &[TSpan], mate: &ClosestRecord) -> bool {
        let s1 = &spans[self.c1_span];
        let m1 = &spans[mate.c1_span];
        let s2 = &spans[self.c2_span];
        let m2 = &spans[mate.c2_span];
        self.c1_span == mate.c1_span
            || s1.end_t == m1.start_t
            || s1.start_t == m1.end_t
            || self.c2_span == mate.c2_span
            || s2.end_t == m2.start_t
            || s2.start_t == m2.end_t
    }

    fn merge(&mut self, mate: &ClosestRecord) {
        self.c1_span = mate.c1_span;
        self.c2_span = mate.c2_span;
        self.closest = mate.closest;
        self.c1_index = mate.c1_index;
        self.c2_index = mate.c2_index;
    }

    fn update(&mut self, mate: &ClosestRecord) {
        self.c1_start_t = cpp_min_f64(self.c1_start_t, mate.c1_start_t);
        self.c1_end_t = cpp_max_f64(self.c1_end_t, mate.c1_end_t);
        self.c2_start_t = cpp_min_f64(self.c2_start_t, mate.c2_start_t);
        self.c2_end_t = cpp_max_f64(self.c2_end_t, mate.c2_end_t);
    }

    fn add_intersection(&self, spans: &[TSpan], intersections: &mut Intersections) {
        let s1 = &spans[self.c1_span];
        let s2 = &spans[self.c2_span];
        let r1t = if self.c1_index != 0 { s1.end_t } else { s1.start_t };
        let r2t = if self.c2_index != 0 { s2.end_t } else { s2.start_t };
        intersections.insert(r1t, r2t, s1.part.pt(self.c1_index));
    }
}

struct ClosestSect {
    closest: Vec<ClosestRecord>,
    used: usize,
}

impl ClosestSect {
    fn new() -> Self {
        let mut closest = Vec::with_capacity(DCubic::MAX_INTERSECTIONS * 2);
        closest.push(ClosestRecord::new());
        Self { closest, used: 0 }
    }

    fn find(&mut self, spans: &[TSpan], span1: usize, span2: usize) -> bool {
        let last1 = spans[span1].part.point_last();
        let last2 = spans[span2].part.point_last();
        let record = &mut self.closest[self.used];
        record.find_end(spans, span1, span2, 0, 0);
        record.find_end(spans, span1, span2, 0, last2);
        record.find_end(spans, span1, span2, last1, 0);
        record.find_end(spans, span1, span2, last1, last2);
        if record.closest == f32::MAX as f64 {
            return false;
        }
        let rec_copy = *record;
        for index in 0..self.used {
            if self.closest[index].mates_with(spans, &rec_copy) {
                if self.closest[index].closest > rec_copy.closest {
                    self.closest[index].merge(&rec_copy);
                }
                self.closest[index].update(&rec_copy);
                self.closest[self.used].reset();
                return false;
            }
        }
        self.used += 1;
        self.closest.push(ClosestRecord::new());
        true
    }

    fn finish(&self, spans: &[TSpan], intersections: &mut Intersections) {
        let mut indices: Vec<usize> = (0..self.used).collect();
        indices.sort_by(|&a, &b| {
            self.closest[a]
                .closest
                .partial_cmp(&self.closest[b].closest)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        for idx in indices {
            self.closest[idx].add_intersection(spans, intersections);
        }
    }
}

pub fn tsect_intersect(c1: TCurve, c2: TCurve, intersections: &mut Intersections) -> usize {
    let mut ctx = TSectCtx::new(c1, c2);
    ctx.binary_search(intersections);
    intersections.used()
}

impl Intersections {
    pub fn intersect_quad_quad(&mut self, q1: &DQuad, q2: &DQuad) -> usize {
        tsect_intersect(TCurve::Quad(*q1), TCurve::Quad(*q2), self)
    }

    pub fn intersect_conic_quad(&mut self, c: &DConic, q: &DQuad) -> usize {
        tsect_intersect(TCurve::Conic(*c), TCurve::Quad(*q), self)
    }

    pub fn intersect_conic_conic(&mut self, c1: &DConic, c2: &DConic) -> usize {
        tsect_intersect(TCurve::Conic(*c1), TCurve::Conic(*c2), self)
    }

    pub fn intersect_cubic_quad(&mut self, c: &DCubic, q: &DQuad) -> usize {
        tsect_intersect(TCurve::Cubic(*c), TCurve::Quad(*q), self)
    }

    pub fn intersect_cubic_conic(&mut self, cu: &DCubic, co: &DConic) -> usize {
        tsect_intersect(TCurve::Cubic(*cu), TCurve::Conic(*co), self)
    }

    pub fn intersect_cubic_cubic(&mut self, c1: &DCubic, c2: &DCubic) -> usize {
        tsect_intersect(TCurve::Cubic(*c1), TCurve::Cubic(*c2), self)
    }
}
