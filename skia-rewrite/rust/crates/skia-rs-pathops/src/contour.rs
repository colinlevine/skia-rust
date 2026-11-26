#![forbid(unsafe_code)]

//! Port of `SkOpContour.h/.cpp` (remaining methods + `OpContourBuilder`).

use skia_rs_geometry::Point;
use skia_rs_path::types::Verb;

use crate::arena::{ContourId, OpState, SpanId};
use crate::path_writer::PathWriter;

impl OpState {
    pub fn contour_complete(&mut self, contour: ContourId) {
        self.contour_set_bounds(contour);
    }

    pub fn contour_calc_angles(&mut self, contour: ContourId) {
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            self.seg_calc_angles(seg);
            seg_opt = self.segments[seg].next;
        }
    }

    pub fn contour_missing_coincidence(&mut self, contour: ContourId) -> bool {
        let mut seg_opt = self.contours[contour].head;
        let mut result = false;
        while let Some(seg) = seg_opt {
            if self.seg_missing_coincidence(seg) {
                result = true;
            }
            seg_opt = self.segments[seg].next;
        }
        result
    }

    pub fn contour_move_multiples(&mut self, contour: ContourId) -> bool {
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            if !self.seg_move_multiples(seg) {
                return false;
            }
            seg_opt = self.segments[seg].next;
        }
        true
    }

    pub fn contour_move_nearby(&mut self, contour: ContourId) -> bool {
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            if !self.seg_move_nearby(seg) {
                return false;
            }
            seg_opt = self.segments[seg].next;
        }
        true
    }

    pub fn contour_sort_angles(&mut self, contour: ContourId) -> bool {
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            if !self.seg_sort_angles(seg) {
                return false;
            }
            seg_opt = self.segments[seg].next;
        }
        true
    }

    pub fn contour_to_path(&mut self, contour: ContourId, path: &mut PathWriter) {
        if self.contours[contour].count == 0 {
            return;
        }
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            let head = self.segments[seg].head;
            let tail = self.segments[seg].tail;
            let _ = self.seg_add_curve_to(seg, head, tail, path);
            seg_opt = self.segments[seg].next;
        }
        path.finish_contour(self);
        path.assemble(self);
    }

    pub fn contour_to_reverse_path(&mut self, contour: ContourId, path: &mut PathWriter) {
        let mut seg_opt = self.contours[contour].tail;
        while let Some(seg) = seg_opt {
            let head = self.segments[seg].head;
            let tail = self.segments[seg].tail;
            let _ = self.seg_add_curve_to(seg, tail, head, path);
            seg_opt = self.segments[seg].prev;
        }
        path.finish_contour(self);
        path.assemble(self);
    }

    pub fn contour_undone_span(&mut self, contour: ContourId) -> Option<SpanId> {
        let mut seg_opt = self.contours[contour].head;
        while let Some(seg) = seg_opt {
            if !self.seg_done(seg) {
                return self.seg_undone_span(seg);
            }
            seg_opt = self.segments[seg].next;
        }
        self.contours[contour].done = true;
        None
    }
}

pub struct OpContourBuilder {
    contour: Option<ContourId>,
    last_line: [Point; 2],
    last_is_line: bool,
}

impl OpContourBuilder {
    pub fn new(contour: ContourId) -> Self {
        Self {
            contour: Some(contour),
            last_line: [Point::ZERO; 2],
            last_is_line: false,
        }
    }

    #[inline]
    pub fn contour(&self) -> Option<ContourId> {
        self.contour
    }

    pub fn set_contour(&mut self, state: &mut OpState, contour: Option<ContourId>) {
        self.flush(state);
        self.contour = contour;
    }

    pub fn add_conic(&mut self, state: &mut OpState, pts: [Point; 3], weight: f32) {
        self.flush(state);
        if let Some(c) = self.contour {
            state.add_conic(c, pts, weight);
        }
    }

    pub fn add_cubic(&mut self, state: &mut OpState, pts: [Point; 4]) {
        self.flush(state);
        if let Some(c) = self.contour {
            state.add_cubic(c, pts);
        }
    }

    pub fn add_quad(&mut self, state: &mut OpState, pts: [Point; 3]) {
        self.flush(state);
        if let Some(c) = self.contour {
            state.add_quad(c, pts);
        }
    }

    pub fn add_line(&mut self, state: &mut OpState, pts: [Point; 2]) {
        if self.last_is_line {
            if self.last_line[0] == pts[1] && self.last_line[1] == pts[0] {
                self.last_is_line = false;
                return;
            } else {
                self.flush(state);
            }
        }
        self.last_line = pts;
        self.last_is_line = true;
    }

    pub fn add_curve(&mut self, state: &mut OpState, verb: Verb, pts: &[Point], weight: f32) {
        match verb {
            Verb::Line => self.add_line(state, [pts[0], pts[1]]),
            Verb::Quad => self.add_quad(state, [pts[0], pts[1], pts[2]]),
            Verb::Conic => self.add_conic(state, [pts[0], pts[1], pts[2]], weight),
            Verb::Cubic => self.add_cubic(state, [pts[0], pts[1], pts[2], pts[3]]),
            _ => {}
        }
    }

    pub fn flush(&mut self, state: &mut OpState) {
        if !self.last_is_line {
            return;
        }
        if let Some(c) = self.contour {
            state.add_line(c, self.last_line);
        }
        self.last_is_line = false;
    }
}
