#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkEdgeBuilder.h` and `src/core/SkEdgeBuilder.cpp`.

use crate::analytic_edge::{
    AnalyticCubicState, AnalyticEdge, AnalyticEdgeBase, AnalyticQuadState,
};
use crate::edge::{BasicEdge, CubicEdgeState, EdgeBase, EdgeType, QuadEdgeState};
use crate::fixed::abs32;
use skia_rs_geometry::edge_clipper::{EdgeClipOutput, VERB_CUBIC, VERB_LINE, VERB_QUAD};
use skia_rs_geometry::geometry::{
    chop_cubic_at_y_extrema, chop_quad_at_y_extrema, Conic, MAX_CONIC_TO_QUAD_POW2,
};
use skia_rs_geometry::line_clipper::{clip_line, LINE_CLIPPER_MAX_POINTS};
use skia_rs_geometry::{all_finite, IRect, Point, Rect};
use skia_rs_path::iter::{Edge as IterEdge, EdgeIter};
use skia_rs_path::{PathRaw, SEG_LINE};

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Combine {
    No = 0,
    Partial = 1,
    Total = 2,
}

#[inline]
pub fn is_vertical_basic(edge: &EdgeBase) -> bool {
    edge.dx_dy == 0 && edge.edge_type() == EdgeType::Line
}

#[inline]
pub fn is_vertical_analytic(edge: &AnalyticEdgeBase) -> bool {
    edge.dx == 0 && edge.edge_type() == EdgeType::Line
}

/// Matches `SkBasicEdgeBuilder::combineVertical`.
pub fn combine_vertical_basic(
    edge: &EdgeBase,
    last: &mut EdgeBase,
    can_cull_to_the_right: bool,
) -> Combine {
    if last.edge_type() != EdgeType::Line || last.dx_dy != 0 || edge.x != last.x {
        return Combine::No;
    }
    if edge.winding == last.winding {
        if edge.last_y.wrapping_add(1) == last.first_y {
            last.first_y = edge.first_y;
            return Combine::Partial;
        }
        if edge.first_y == last.last_y.wrapping_add(1) {
            last.last_y = edge.last_y;
            return Combine::Partial;
        }
        return Combine::No;
    }
    if !can_cull_to_the_right {
        return Combine::No;
    }
    if edge.first_y == last.first_y {
        if edge.last_y == last.last_y {
            return Combine::Total;
        }
        if edge.last_y < last.last_y {
            last.first_y = edge.last_y + 1;
            return Combine::Partial;
        }
        last.first_y = last.last_y + 1;
        last.last_y = edge.last_y;
        last.winding = edge.winding;
        return Combine::Partial;
    }
    if edge.last_y == last.last_y {
        if edge.first_y > last.first_y {
            last.last_y = edge.first_y - 1;
            return Combine::Partial;
        }
        last.last_y = last.first_y - 1;
        last.first_y = edge.first_y;
        last.winding = edge.winding;
        return Combine::Partial;
    }
    Combine::No
}

/// Matches `SkAnalyticEdgeBuilder::combineVertical`.
pub fn combine_vertical_analytic(
    edge: &AnalyticEdgeBase,
    last: &mut AnalyticEdgeBase,
    can_cull_to_the_right: bool,
) -> Combine {
    #[inline]
    fn approx_eq(a: i32, b: i32) -> bool {
        abs32(a.wrapping_sub(b)) < 0x100
    }

    if last.edge_type() != EdgeType::Line || last.dx != 0 || edge.x != last.x {
        return Combine::No;
    }
    if edge.winding == last.winding {
        if edge.lower_y == last.upper_y {
            last.upper_y = edge.upper_y;
            last.y = last.upper_y;
            return Combine::Partial;
        }
        if approx_eq(edge.upper_y, last.lower_y) {
            last.lower_y = edge.lower_y;
            return Combine::Partial;
        }
        return Combine::No;
    }
    if !can_cull_to_the_right {
        return Combine::No;
    }
    if approx_eq(edge.upper_y, last.upper_y) {
        if approx_eq(edge.lower_y, last.lower_y) {
            return Combine::Total;
        }
        if edge.lower_y < last.lower_y {
            last.upper_y = edge.lower_y;
            last.y = last.upper_y;
            return Combine::Partial;
        }
        last.upper_y = last.lower_y;
        last.y = last.upper_y;
        last.lower_y = edge.lower_y;
        last.winding = edge.winding;
        return Combine::Partial;
    }
    if approx_eq(edge.lower_y, last.lower_y) {
        if edge.upper_y > last.upper_y {
            last.lower_y = edge.upper_y;
            return Combine::Partial;
        }
        last.lower_y = last.upper_y;
        last.upper_y = edge.upper_y;
        last.y = last.upper_y;
        last.winding = edge.winding;
        return Combine::Partial;
    }
    Combine::No
}

/// Helper matching `SkAutoConicToQuads::computeQuads`.
pub fn conic_to_quads(pts: &[Point; 3], weight: f32, tol: f32, out: &mut Vec<Point>) -> usize {
    if weight <= 0.0 {
        out.clear();
        out.push(pts[0]);
        out.push(Point::new(
            (pts[0].x + pts[2].x) * 0.5,
            (pts[0].y + pts[2].y) * 0.5,
        ));
        out.push(pts[2]);
        return 1;
    }
    let conic = Conic {
        pts: *pts,
        w: weight,
    };
    let pow2 = conic.compute_quad_pow2(tol).clamp(0, MAX_CONIC_TO_QUAD_POW2);
    let max_quads = 1usize << (pow2 as u32);
    out.resize(1 + 2 * max_quads, Point::ZERO);
    conic.chop_into_quads_pow2(out, pow2) as usize
}

/// Sink trait for receiving clipped/monotonic edge primitives from `drive_edge_builder`.
pub trait EdgeSink {
    fn add_line(&mut self, pts: &[Point; 2]);
    fn add_quad(&mut self, pts: &[Point; 3]);
    fn add_cubic(&mut self, pts: &[Point; 4]);
}

/// Drives path iteration, conic-to-quad conversion, Y-extrema chopping, and optional edge clipping.
/// Returns `false` if non-finite points were encountered (in which case caller returns 0 edges).
pub fn drive_edge_builder<S: EdgeSink>(
    raw: &PathRaw<'_>,
    iclip: Option<IRect>,
    can_cull_to_the_right: bool,
    sink: &mut S,
) -> bool {
    if raw.segment_mask == SEG_LINE {
        // Matches SkEdgeBuilder::buildPoly
        if iclip.is_some() && raw.points.len().checked_mul(3).is_none() {
            return false;
        }
        let mut iter = EdgeIter::new(raw);
        if let Some(ic) = iclip {
            let clip = Rect::from_irect(ic);
            let mut lines = [Point::ZERO; LINE_CLIPPER_MAX_POINTS];
            while let Some(e) = iter.next_edge() {
                if e.edge == IterEdge::Line {
                    let line_count = clip_line(
                        &[e.points[0], e.points[1]],
                        clip,
                        &mut lines,
                        can_cull_to_the_right,
                    );
                    for i in 0..(line_count as usize) {
                        sink.add_line(&[lines[i], lines[i + 1]]);
                    }
                }
            }
        } else {
            while let Some(e) = iter.next_edge() {
                if e.edge == IterEdge::Line {
                    sink.add_line(&[e.points[0], e.points[1]]);
                }
            }
        }
        return true;
    }

    // General path (matches SkEdgeBuilder::build)
    let mut quad_buf = Vec::new();
    const K_CONIC_TOL: f32 = 0.25;

    if let Some(ic) = iclip {
        let clip = Rect::from_irect(ic);
        let mut clipper = EdgeClipOutput::new(can_cull_to_the_right);
        let mut iter = EdgeIter::new(raw);
        let mut is_finite = true;

        let flush_clipper = |clipper: &EdgeClipOutput, sink: &mut S| -> bool {
            let mut p_idx = 0usize;
            for v_idx in 0..clipper.verb_count {
                match clipper.verbs[v_idx] {
                    VERB_LINE => {
                        let pts = [clipper.points[p_idx], clipper.points[p_idx + 1]];
                        p_idx += 2;
                        if !all_finite(&[pts[0].x, pts[0].y, pts[1].x, pts[1].y]) {
                            return false;
                        }
                        sink.add_line(&pts);
                    }
                    VERB_QUAD => {
                        let pts = [
                            clipper.points[p_idx],
                            clipper.points[p_idx + 1],
                            clipper.points[p_idx + 2],
                        ];
                        p_idx += 3;
                        if !all_finite(&[
                            pts[0].x, pts[0].y, pts[1].x, pts[1].y, pts[2].x, pts[2].y,
                        ]) {
                            return false;
                        }
                        sink.add_quad(&pts);
                    }
                    VERB_CUBIC => {
                        let pts = [
                            clipper.points[p_idx],
                            clipper.points[p_idx + 1],
                            clipper.points[p_idx + 2],
                            clipper.points[p_idx + 3],
                        ];
                        p_idx += 4;
                        if !all_finite(&[
                            pts[0].x, pts[0].y, pts[1].x, pts[1].y, pts[2].x, pts[2].y, pts[3].x,
                            pts[3].y,
                        ]) {
                            return false;
                        }
                        sink.add_cubic(&pts);
                    }
                    _ => {}
                }
            }
            true
        };

        while let Some(e) = iter.next_edge() {
            match e.edge {
                IterEdge::Line => {
                    if clipper.clip_line(e.points[0], e.points[1], clip) {
                        if !flush_clipper(&clipper, sink) {
                            is_finite = false;
                        }
                    }
                }
                IterEdge::Quad => {
                    let q = [e.points[0], e.points[1], e.points[2]];
                    if clipper.clip_quad(&q, clip) {
                        if !flush_clipper(&clipper, sink) {
                            is_finite = false;
                        }
                    }
                }
                IterEdge::Conic => {
                    let q_in = [e.points[0], e.points[1], e.points[2]];
                    let count = conic_to_quads(&q_in, e.conic_weight, K_CONIC_TOL, &mut quad_buf);
                    for i in 0..count {
                        let q = [quad_buf[i * 2], quad_buf[i * 2 + 1], quad_buf[i * 2 + 2]];
                        if clipper.clip_quad(&q, clip) {
                            if !flush_clipper(&clipper, sink) {
                                is_finite = false;
                            }
                        }
                    }
                }
                IterEdge::Cubic => {
                    let c = [e.points[0], e.points[1], e.points[2], e.points[3]];
                    if clipper.clip_cubic(&c, clip) {
                        if !flush_clipper(&clipper, sink) {
                            is_finite = false;
                        }
                    }
                }
            }
        }
        return is_finite;
    }

    let mut iter = EdgeIter::new(raw);
    let mut mono_y = [Point::ZERO; 10];
    let mut mono_x = [Point::ZERO; 5];

    while let Some(e) = iter.next_edge() {
        match e.edge {
            IterEdge::Line => {
                sink.add_line(&[e.points[0], e.points[1]]);
            }
            IterEdge::Quad => {
                let q = [e.points[0], e.points[1], e.points[2]];
                let n = chop_quad_at_y_extrema(&q, &mut mono_x);
                for i in 0..=(n as usize) {
                    sink.add_quad(&[mono_x[i * 2], mono_x[i * 2 + 1], mono_x[i * 2 + 2]]);
                }
            }
            IterEdge::Conic => {
                let q_in = [e.points[0], e.points[1], e.points[2]];
                let count = conic_to_quads(&q_in, e.conic_weight, K_CONIC_TOL, &mut quad_buf);
                for qi in 0..count {
                    let q = [
                        quad_buf[qi * 2],
                        quad_buf[qi * 2 + 1],
                        quad_buf[qi * 2 + 2],
                    ];
                    let n = chop_quad_at_y_extrema(&q, &mut mono_x);
                    for i in 0..=(n as usize) {
                        sink.add_quad(&[mono_x[i * 2], mono_x[i * 2 + 1], mono_x[i * 2 + 2]]);
                    }
                }
            }
            IterEdge::Cubic => {
                let c = [e.points[0], e.points[1], e.points[2], e.points[3]];
                let n = chop_cubic_at_y_extrema(&c, Some(&mut mono_y));
                for i in 0..=(n as usize) {
                    sink.add_cubic(&[
                        mono_y[i * 3],
                        mono_y[i * 3 + 1],
                        mono_y[i * 3 + 2],
                        mono_y[i * 3 + 3],
                    ]);
                }
            }
        }
    }
    true
}

/// Safe Rust `SkBasicEdgeBuilder`.
pub struct BasicEdgeBuilder {
    pub edges: Vec<BasicEdge>,
    can_cull_to_the_right: bool,
}

impl BasicEdgeBuilder {
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            can_cull_to_the_right: true,
        }
    }

    pub fn build_edges(&mut self, raw: &PathRaw<'_>, shifted_clip: Option<IRect>) -> usize {
        self.edges.clear();
        self.can_cull_to_the_right = !raw.convexity.is_convex();
        let ok = drive_edge_builder(raw, shifted_clip, self.can_cull_to_the_right, self);
        if !ok {
            self.edges.clear();
            return 0;
        }
        self.edges.len()
    }
}

impl Default for BasicEdgeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EdgeSink for BasicEdgeBuilder {
    fn add_line(&mut self, pts: &[Point; 2]) {
        let mut edge = EdgeBase::default();
        if edge.set_line(pts[0], pts[1], None) {
            let combine = if is_vertical_basic(&edge) {
                if let Some(last) = self.edges.last_mut() {
                    combine_vertical_basic(&edge, last.base_mut(), self.can_cull_to_the_right)
                } else {
                    Combine::No
                }
            } else {
                Combine::No
            };
            match combine {
                Combine::Total => {
                    self.edges.pop();
                }
                Combine::Partial => {}
                Combine::No => {
                    self.edges.push(BasicEdge::Line(edge));
                }
            }
        }
    }

    fn add_quad(&mut self, pts: &[Point; 3]) {
        let mut edge = QuadEdgeState::default();
        if edge.set_quadratic(pts) {
            self.edges.push(BasicEdge::Quad(edge));
        }
    }

    fn add_cubic(&mut self, pts: &[Point; 4]) {
        let mut edge = CubicEdgeState::default();
        if edge.set_cubic(pts) {
            self.edges.push(BasicEdge::Cubic(edge));
        }
    }
}

/// Safe Rust `SkAnalyticEdgeBuilder`.
pub struct AnalyticEdgeBuilder {
    pub edges: Vec<AnalyticEdge>,
    can_cull_to_the_right: bool,
}

impl AnalyticEdgeBuilder {
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            can_cull_to_the_right: true,
        }
    }

    pub fn build_edges(&mut self, raw: &PathRaw<'_>, shifted_clip: Option<IRect>) -> usize {
        self.edges.clear();
        self.can_cull_to_the_right = !raw.convexity.is_convex();
        let ok = drive_edge_builder(raw, shifted_clip, self.can_cull_to_the_right, self);
        if !ok {
            self.edges.clear();
            return 0;
        }
        self.edges.len()
    }
}

impl Default for AnalyticEdgeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EdgeSink for AnalyticEdgeBuilder {
    fn add_line(&mut self, pts: &[Point; 2]) {
        let mut edge = AnalyticEdgeBase::default();
        if edge.set_line(pts[0], pts[1]) {
            let combine = if is_vertical_analytic(&edge) {
                if let Some(last) = self.edges.last_mut() {
                    combine_vertical_analytic(&edge, last.base_mut(), self.can_cull_to_the_right)
                } else {
                    Combine::No
                }
            } else {
                Combine::No
            };
            match combine {
                Combine::Total => {
                    self.edges.pop();
                }
                Combine::Partial => {}
                Combine::No => {
                    self.edges.push(AnalyticEdge::Line(edge));
                }
            }
        }
    }

    fn add_quad(&mut self, pts: &[Point; 3]) {
        let mut edge = AnalyticQuadState::default();
        if edge.set_quadratic(pts) {
            self.edges.push(AnalyticEdge::Quad(edge));
        }
    }

    fn add_cubic(&mut self, pts: &[Point; 4]) {
        let mut edge = AnalyticCubicState::default();
        if edge.set_cubic(pts) {
            self.edges.push(AnalyticEdge::Cubic(edge));
        }
    }
}
