#![forbid(unsafe_code)]

//! Safe arena-based representation of the PathOps graph (`OpState`),
//! replacing raw self-referential C++ pointers in `SkOpContour`, `SkOpSegment`,
//! `SkOpSpanBase`, `SkOpSpan`, `SkOpPtT`, `SkOpAngle`, and `SkOpCoincidence`.

use skia_rs_geometry::Point;
use skia_rs_path::types::Verb;

use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::curve::{DCurve, DCurveSweep};
use crate::line::LineParameters;
use crate::quad::DQuad;
use crate::types::{cpp_max_f32, cpp_min_f32, pathops_verb_to_points, PathOpsBounds};

pub type ContourId = usize;
pub type SegmentId = usize;
pub type SpanId = usize;
pub type PtTId = usize;
pub type AngleId = usize;
pub type CoinId = usize;

pub const MAX_WINDING_TRIES: i32 = 10;
pub const SK_MAX_S32: i32 = i32::MAX;
pub const SK_MIN_S32: i32 = -i32::MAX;
pub const SK_NAN_32: i32 = i32::MIN;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OpPhase {
    NoChange,
    Intersecting,
    Walking,
    FixWinding,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Collapsed {
    No,
    Yes,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AngleIncludeType {
    UnaryWinding = 0,
    UnaryXor = 1,
    BinarySingle = 2,
    BinaryOpp = 3,
}

#[derive(Copy, Clone, Debug)]
pub struct OpPtT {
    pub t: f64,
    pub pt: Point,
    pub span: SpanId,
    pub next: PtTId,
    pub deleted: bool,
    pub duplicate_pt: bool,
    pub coincident: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct OpSpan {
    pub pt_t: PtTId,
    pub segment: SegmentId,
    pub coin_end: SpanId,
    pub from_angle: Option<AngleId>,
    pub prev: Option<SpanId>,
    pub span_adds: i32,
    pub aligned: bool,
    pub chased: bool,
    // SkOpSpan fields (valid when !final)
    pub coincident: SpanId,
    pub to_angle: Option<AngleId>,
    pub next: Option<SpanId>,
    pub wind_sum: i32,
    pub opp_sum: i32,
    pub wind_value: i32,
    pub opp_value: i32,
    pub top_t_try: i32,
    pub done: bool,
    pub already_added: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct OpAngle {
    pub original_curve_part: DCurve,
    pub part: DCurveSweep,
    pub side: f64,
    pub tangent_half: LineParameters,
    pub next: Option<AngleId>,
    pub last_marked: Option<SpanId>,
    pub start: Option<SpanId>,
    pub end: Option<SpanId>,
    pub computed_end: Option<SpanId>,
    pub sector_mask: u32,
    pub sector_start: i8,
    pub sector_end: i8,
    pub unorderable: bool,
    pub compute_sector: bool,
    pub computed_sector: bool,
    pub check_coincidence: bool,
    pub tangents_ambiguous: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct OpSegment {
    pub head: SpanId,
    pub tail: SpanId,
    pub contour: ContourId,
    pub next: Option<SegmentId>,
    pub prev: Option<SegmentId>,
    pub pts: [Point; 4],
    pub bounds: PathOpsBounds,
    pub weight: f32,
    pub count: i32,
    pub done_count: i32,
    pub verb: Verb,
    pub visited: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct OpContour {
    pub head: Option<SegmentId>,
    pub tail: Option<SegmentId>,
    pub next: Option<ContourId>,
    pub bounds: PathOpsBounds,
    pub ccw: i32,
    pub count: i32,
    pub done: bool,
    pub operand: bool,
    pub reverse: bool,
    pub xor: bool,
    pub opp_xor: bool,
}

impl Default for OpContour {
    fn default() -> Self {
        Self {
            head: None,
            tail: None,
            next: None,
            bounds: PathOpsBounds::default(),
            ccw: -1,
            count: 0,
            done: false,
            operand: false,
            reverse: false,
            xor: false,
            opp_xor: false,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct CoincidentSpans {
    pub next: Option<CoinId>,
    pub coin_pt_t_start: PtTId,
    pub coin_pt_t_end: PtTId,
    pub opp_pt_t_start: PtTId,
    pub opp_pt_t_end: PtTId,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct OpCoincidenceList {
    pub head: Option<CoinId>,
    pub top: Option<CoinId>,
}

impl OpCoincidenceList {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.is_none() && self.top.is_none()
    }
}

pub struct OpState {
    pub contours: Vec<OpContour>,
    pub segments: Vec<OpSegment>,
    pub spans: Vec<OpSpan>,
    pub pt_ts: Vec<OpPtT>,
    pub angles: Vec<OpAngle>,
    pub coin_spans: Vec<CoincidentSpans>,
    pub contour_head: ContourId,
    pub coincidence: OpCoincidenceList,
    pub nested: i32,
    pub allocated_op_span: bool,
    pub winding_failed: bool,
    pub phase: OpPhase,
}

impl Default for OpState {
    fn default() -> Self {
        Self::new()
    }
}

impl OpState {
    pub fn new() -> Self {
        let mut contours = Vec::with_capacity(8);
        contours.push(OpContour::default());
        Self {
            contours,
            segments: Vec::with_capacity(32),
            spans: Vec::with_capacity(64),
            pt_ts: Vec::with_capacity(64),
            angles: Vec::with_capacity(64),
            coin_spans: Vec::with_capacity(16),
            contour_head: 0,
            coincidence: OpCoincidenceList::default(),
            nested: 0,
            allocated_op_span: false,
            winding_failed: false,
            phase: OpPhase::Intersecting,
        }
    }

    pub fn append_contour(&mut self) -> ContourId {
        let id = self.contours.len();
        self.contours.push(OpContour::default());
        let mut prev = self.contour_head;
        while let Some(next) = self.contours[prev].next {
            prev = next;
        }
        self.contours[prev].next = Some(id);
        id
    }

    pub fn remove_contour(&mut self, contour: ContourId) {
        if contour == self.contour_head {
            return;
        }
        let mut prev = self.contour_head;
        while let Some(next) = self.contours[prev].next {
            if next == contour {
                self.contours[prev].next = None;
                return;
            }
            prev = next;
        }
    }

    pub fn init_contour(&mut self, contour: ContourId, operand: bool, is_xor: bool) {
        self.contours[contour].operand = operand;
        self.contours[contour].xor = is_xor;
    }

    pub fn alloc_span(
        &mut self,
        segment: SegmentId,
        prev: Option<SpanId>,
        t: f64,
        pt: Point,
        is_base_only: bool,
    ) -> SpanId {
        let span_id = self.spans.len();
        let pt_t_id = self.pt_ts.len();
        self.pt_ts.push(OpPtT {
            t,
            pt,
            span: span_id,
            next: pt_t_id,
            deleted: false,
            duplicate_pt: false,
            coincident: false,
        });
        self.spans.push(OpSpan {
            pt_t: pt_t_id,
            segment,
            coin_end: span_id,
            from_angle: None,
            prev,
            span_adds: 0,
            aligned: true,
            chased: false,
            coincident: span_id,
            to_angle: None,
            next: None,
            wind_sum: SK_MIN_S32,
            opp_sum: SK_MIN_S32,
            wind_value: 1,
            opp_value: 0,
            top_t_try: 0,
            done: false,
            already_added: false,
        });
        if !is_base_only {
            self.segments[segment].count += 1;
        }
        span_id
    }

    pub fn alloc_angle(&mut self) -> AngleId {
        let id = self.angles.len();
        self.angles.push(OpAngle::default());
        id
    }

    pub fn append_segment(
        &mut self,
        contour: ContourId,
        pts: [Point; 4],
        weight: f32,
        verb: Verb,
        bounds: PathOpsBounds,
    ) -> SegmentId {
        let seg_id = self.segments.len();
        let prev = self.contours[contour].tail;
        let last_pt = pts[pathops_verb_to_points(verb)];
        // Placeholder head/tail, updated immediately after alloc_span.
        self.segments.push(OpSegment {
            head: 0,
            tail: 0,
            contour,
            next: None,
            prev,
            pts,
            bounds,
            weight,
            count: 0,
            done_count: 0,
            verb,
            visited: false,
        });
        let head = self.alloc_span(seg_id, None, 0.0, pts[0], false);
        let tail = self.alloc_span(seg_id, Some(head), 1.0, last_pt, true);
        self.spans[head].next = Some(tail);
        self.segments[seg_id].head = head;
        self.segments[seg_id].tail = tail;

        if let Some(p) = prev {
            self.segments[p].next = Some(seg_id);
        } else {
            self.contours[contour].head = Some(seg_id);
        }
        self.contours[contour].tail = Some(seg_id);
        self.contours[contour].count += 1;
        seg_id
    }

    pub fn add_line(&mut self, contour: ContourId, pts: [Point; 2]) -> SegmentId {
        let bounds = PathOpsBounds::new(
            cpp_min_f32(pts[0].x, pts[1].x),
            cpp_min_f32(pts[0].y, pts[1].y),
            cpp_max_f32(pts[0].x, pts[1].x),
            cpp_max_f32(pts[0].y, pts[1].y),
        );
        self.append_segment(
            contour,
            [pts[0], pts[1], Point::ZERO, Point::ZERO],
            1.0,
            Verb::Line,
            bounds,
        )
    }

    pub fn add_quad(&mut self, contour: ContourId, pts: [Point; 3]) -> SegmentId {
        let curve = DCurve::from_quad(DQuad::from_points(pts));
        let bounds = curve.set_quad_bounds(pts, 0.0, 1.0);
        self.append_segment(
            contour,
            [pts[0], pts[1], pts[2], Point::ZERO],
            1.0,
            Verb::Quad,
            bounds,
        )
    }

    pub fn add_conic(&mut self, contour: ContourId, pts: [Point; 3], weight: f32) -> SegmentId {
        let curve = DCurve::from_conic(DConic::from_points(pts, weight));
        let bounds = curve.set_conic_bounds(pts, weight, 0.0, 1.0);
        self.append_segment(
            contour,
            [pts[0], pts[1], pts[2], Point::ZERO],
            weight,
            Verb::Conic,
            bounds,
        )
    }

    pub fn add_cubic(&mut self, contour: ContourId, pts: [Point; 4]) -> SegmentId {
        let curve = DCurve::from_cubic(DCubic::from_points(pts));
        let bounds = curve.set_cubic_bounds(pts, 0.0, 1.0);
        self.append_segment(contour, pts, 1.0, Verb::Cubic, bounds)
    }

    pub fn contour_set_bounds(&mut self, contour: ContourId) {
        let mut seg = match self.contours[contour].head {
            Some(s) => s,
            None => return,
        };
        let mut bounds = self.segments[seg].bounds;
        while let Some(next) = self.segments[seg].next {
            bounds.add_bounds(&self.segments[next].bounds);
            seg = next;
        }
        self.contours[contour].bounds = bounds;
    }

    pub fn contour_join_segments(&mut self, contour: ContourId) {
        let head = match self.contours[contour].head {
            Some(h) => h,
            None => return,
        };
        let mut seg = head;
        loop {
            let next = self.segments[seg].next;
            let target = next.unwrap_or(head);
            let tail_pt_t = self.spans[self.segments[seg].tail].pt_t;
            let start_pt_t = self.spans[self.segments[target].head].pt_t;
            self.pt_t_add_opp(tail_pt_t, start_pt_t, start_pt_t);
            match next {
                Some(n) => seg = n,
                None => break,
            }
        }
    }

    pub fn contour_join_all_segments(&mut self) {
        let mut cur = Some(self.contour_head);
        while let Some(c) = cur {
            if self.contours[c].count > 0 {
                self.contour_join_segments(c);
            }
            cur = self.contours[c].next;
        }
    }

    pub fn contour_reset_reverse(&mut self) {
        let mut cur = Some(self.contour_head);
        while let Some(c) = cur {
            if self.contours[c].count > 0 {
                self.contours[c].ccw = -1;
                self.contours[c].reverse = false;
            }
            cur = self.contours[c].next;
        }
    }

    pub fn contour_mark_all_done(&mut self, contour: ContourId) {
        let mut cur = self.contours[contour].head;
        while let Some(seg) = cur {
            self.seg_mark_all_done(seg);
            cur = self.segments[seg].next;
        }
    }
}
