#![forbid(unsafe_code)]

//! Port of `SkOpEdgeBuilder.h/.cpp`.

use skia_rs_geometry::geometry::{chop_quad_at_max_curvature, find_quad_max_curvature, Conic};
use skia_rs_geometry::Point;
use skia_rs_path::iter::RangeIter;
use skia_rs_path::types::{PathDataBuf, Verb};

use crate::arena::{ContourId, OpState};
use crate::contour::OpContourBuilder;
use crate::cubic::DCubic;
use crate::curve::ReduceOrder;
use crate::types::{
    pathops_verb_to_points, DPoint, PathOpsMask, FLT_EPSILON_ORDERABLE_ERR,
};

#[inline]
fn force_small_to_zero(mut pt: Point) -> Point {
    let eps = FLT_EPSILON_ORDERABLE_ERR as f32;
    if pt.x.abs() < eps {
        pt.x = 0.0;
    }
    if pt.y.abs() < eps {
        pt.y = 0.0;
    }
    pt
}

fn can_add_curve(verb: Verb, curve: &mut [Point]) -> bool {
    if verb == Verb::Move {
        return false;
    }
    let count = pathops_verb_to_points(verb);
    for pt in curve.iter_mut().take(count + 1) {
        *pt = force_small_to_zero(*pt);
    }
    verb != Verb::Line || !DPoint::approximately_equal_pts(curve[0], curve[1])
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum EdgeVerb {
    Move,
    Line,
    Quad,
    Conic,
    Cubic,
    Close,
    Done,
}

impl From<Verb> for EdgeVerb {
    fn from(v: Verb) -> Self {
        match v {
            Verb::Move => EdgeVerb::Move,
            Verb::Line => EdgeVerb::Line,
            Verb::Quad => EdgeVerb::Quad,
            Verb::Conic => EdgeVerb::Conic,
            Verb::Cubic => EdgeVerb::Cubic,
            Verb::Close => EdgeVerb::Close,
        }
    }
}

pub struct OpEdgeBuilder {
    path_pts: Vec<Point>,
    weights: Vec<f32>,
    path_verbs: Vec<EdgeVerb>,
    contour_builder: OpContourBuilder,
    xor_mask: [PathOpsMask; 2],
    second_half: usize,
    operand: bool,
    allow_open_contours: bool,
    unparseable: bool,
}

impl OpEdgeBuilder {
    pub fn new(path: &PathDataBuf, contours_head: ContourId) -> Self {
        let mask = if ((path.fill_type as u8) & 1) != 0 {
            PathOpsMask::EvenOdd
        } else {
            PathOpsMask::Winding
        };
        let mut builder = Self {
            path_pts: Vec::new(),
            weights: Vec::new(),
            path_verbs: Vec::new(),
            contour_builder: OpContourBuilder::new(contours_head),
            xor_mask: [mask, mask],
            second_half: 0,
            operand: false,
            allow_open_contours: false,
            unparseable: false,
        };
        builder.second_half = builder.pre_fetch(path);
        builder
    }

    pub fn add_operand(&mut self, path: &PathDataBuf) {
        debug_assert!(self.path_verbs.last() == Some(&EdgeVerb::Done));
        self.path_verbs.pop();
        self.xor_mask[1] = if ((path.fill_type as u8) & 1) != 0 {
            PathOpsMask::EvenOdd
        } else {
            PathOpsMask::Winding
        };
        self.pre_fetch(path);
    }

    #[inline]
    pub fn unparseable(&self) -> bool {
        self.unparseable
    }

    #[inline]
    pub fn xor_mask(&self) -> PathOpsMask {
        self.xor_mask[usize::from(self.operand)]
    }

    fn complete(&mut self, state: &mut OpState) {
        self.contour_builder.flush(state);
        if let Some(c) = self.contour_builder.contour() {
            if state.contours[c].count > 0 {
                state.contour_complete(c);
                self.contour_builder.set_contour(state, None);
            }
        }
    }

    pub fn finish(&mut self, state: &mut OpState) -> bool {
        self.operand = false;
        if self.unparseable || !self.walk(state) {
            return false;
        }
        self.complete(state);
        if let Some(c) = self.contour_builder.contour() {
            if state.contours[c].count == 0 {
                state.remove_contour(c);
            }
        }
        true
    }

    fn close_contour(&mut self, curve_end: Point, curve_start: Point) {
        if !DPoint::approximately_equal_pts(curve_end, curve_start) {
            self.path_verbs.push(EdgeVerb::Line);
            self.path_pts.push(curve_start);
        } else {
            let verb_count = self.path_verbs.len();
            let pts_count = self.path_pts.len();
            if verb_count > 0
                && self.path_verbs[verb_count - 1] == EdgeVerb::Line
                && pts_count >= 2
                && self.path_pts[pts_count - 2] == curve_start
            {
                self.path_verbs.pop();
                self.path_pts.pop();
            } else if pts_count > 0 {
                self.path_pts[pts_count - 1] = curve_start;
            }
        }
        self.path_verbs.push(EdgeVerb::Close);
    }

    fn pre_fetch(&mut self, path: &PathDataBuf) -> usize {
        if !path.is_finite() {
            self.unparseable = true;
            return 0;
        }
        let mut curve_start = Point::ZERO;
        let mut curve = [Point::ZERO; 4];
        let mut last_curve = false;
        for (path_verb, pts, w) in RangeIter::new(&path.points, &path.verbs, &path.conics) {
            let verb = match path_verb {
                Verb::Move => {
                    if !self.allow_open_contours && last_curve {
                        self.close_contour(curve[0], curve_start);
                    }
                    self.path_verbs.push(EdgeVerb::Move);
                    curve[0] = force_small_to_zero(pts[0]);
                    self.path_pts.push(curve[0]);
                    curve_start = curve[0];
                    last_curve = false;
                    continue;
                }
                Verb::Line => {
                    curve[1] = force_small_to_zero(pts[1]);
                    if DPoint::approximately_equal_pts(curve[0], curve[1]) {
                        if let Some(&last_verb) = self.path_verbs.last() {
                            if last_verb != EdgeVerb::Line && last_verb != EdgeVerb::Move {
                                if let Some(last_pt) = self.path_pts.last_mut() {
                                    *last_pt = curve[1];
                                }
                                curve[0] = curve[1];
                            }
                        }
                        continue;
                    }
                    Verb::Line
                }
                Verb::Quad => {
                    curve[1] = force_small_to_zero(pts[1]);
                    curve[2] = force_small_to_zero(pts[2]);
                    let mut reduced = [curve[0], curve[1], curve[2]];
                    let v = ReduceOrder::quad_pts(reduced, &mut reduced);
                    if v == Verb::Move {
                        continue;
                    }
                    curve[0..3].copy_from_slice(&reduced);
                    v
                }
                Verb::Conic => {
                    curve[1] = force_small_to_zero(pts[1]);
                    curve[2] = force_small_to_zero(pts[2]);
                    let mut reduced = [curve[0], curve[1], curve[2]];
                    let mut v = ReduceOrder::quad_pts(reduced, &mut reduced);
                    if v == Verb::Quad && w != 1.0 {
                        v = Verb::Conic;
                    } else if v == Verb::Move {
                        continue;
                    }
                    curve[0..3].copy_from_slice(&reduced);
                    v
                }
                Verb::Cubic => {
                    curve[1] = force_small_to_zero(pts[1]);
                    curve[2] = force_small_to_zero(pts[2]);
                    curve[3] = force_small_to_zero(pts[3]);
                    let mut reduced = curve;
                    let v = ReduceOrder::cubic_pts(curve, &mut reduced);
                    if v == Verb::Move {
                        continue;
                    }
                    curve = reduced;
                    v
                }
                Verb::Close => {
                    self.close_contour(curve[0], curve_start);
                    last_curve = false;
                    continue;
                }
            };
            self.path_verbs.push(EdgeVerb::from(verb));
            let pt_count = pathops_verb_to_points(verb);
            self.path_pts.extend_from_slice(&curve[1..=pt_count]);
            if verb == Verb::Conic {
                self.weights.push(w);
            }
            curve[0] = curve[pt_count];
            last_curve = true;
        }
        if !self.allow_open_contours && last_curve {
            self.close_contour(curve[0], curve_start);
        }
        self.path_verbs.push(EdgeVerb::Done);
        self.path_verbs.len() - 1
    }

    fn close(&mut self, state: &mut OpState) -> bool {
        self.complete(state);
        true
    }

    fn walk(&mut self, state: &mut OpState) -> bool {
        let mut verb_idx = 0usize;
        let end_of_first_half = self.second_half;
        let mut pts_idx = 0usize;
        let mut weight_idx = 0usize;
        let mut contour = self.contour_builder.contour();
        let mut move_to_ptr_bump = 0usize;

        while self.path_verbs[verb_idx] != EdgeVerb::Done {
            if verb_idx == end_of_first_half {
                self.operand = true;
            }
            let verb = self.path_verbs[verb_idx];
            verb_idx += 1;
            match verb {
                EdgeVerb::Move => {
                    if let Some(c) = contour {
                        if state.contours[c].count > 0 {
                            if self.allow_open_contours {
                                self.complete(state);
                            } else if !self.close(state) {
                                return false;
                            }
                            contour = self.contour_builder.contour();
                        }
                    }
                    if contour.is_none() {
                        let new_c = state.append_contour();
                        self.contour_builder.set_contour(state, Some(new_c));
                        contour = Some(new_c);
                    }
                    if let Some(c) = contour {
                        state.init_contour(
                            c,
                            self.operand,
                            self.xor_mask[usize::from(self.operand)] == PathOpsMask::EvenOdd,
                        );
                    }
                    pts_idx += move_to_ptr_bump;
                    move_to_ptr_bump = 1;
                    continue;
                }
                EdgeVerb::Line => {
                    let line_pts = [self.path_pts[pts_idx], self.path_pts[pts_idx + 1]];
                    self.contour_builder.add_line(state, line_pts);
                    pts_idx += 1;
                }
                EdgeVerb::Quad => {
                    let q_pts = [
                        self.path_pts[pts_idx],
                        self.path_pts[pts_idx + 1],
                        self.path_pts[pts_idx + 2],
                    ];
                    let vec1 = q_pts[1] - q_pts[0];
                    let vec2 = q_pts[2] - q_pts[1];
                    let mut added_split = false;
                    if vec1.dot(vec2) < 0.0 {
                        let mut pair = [Point::ZERO; 5];
                        if chop_quad_at_max_curvature(&q_pts, &mut pair) != 1 {
                            if !pair.iter().all(|p| p.is_finite()) {
                                return false;
                            }
                            for p in &mut pair {
                                *p = force_small_to_zero(*p);
                            }
                            let mut c_storage0 = [Point::ZERO; 3];
                            let mut c_storage1 = [Point::ZERO; 3];
                            let v1 =
                                ReduceOrder::quad_pts([pair[0], pair[1], pair[2]], &mut c_storage0);
                            let v2 =
                                ReduceOrder::quad_pts([pair[2], pair[3], pair[4]], &mut c_storage1);
                            let mut curve1 = if v1 != Verb::Line {
                                [pair[0], pair[1], pair[2]]
                            } else {
                                c_storage0
                            };
                            let mut curve2 = if v2 != Verb::Line {
                                [pair[2], pair[3], pair[4]]
                            } else {
                                c_storage1
                            };
                            if can_add_curve(v1, &mut curve1) && can_add_curve(v2, &mut curve2) {
                                self.contour_builder.add_curve(state, v1, &curve1, 1.0);
                                self.contour_builder.add_curve(state, v2, &curve2, 1.0);
                                added_split = true;
                            }
                        }
                    }
                    if !added_split {
                        self.contour_builder.add_quad(state, q_pts);
                    }
                    pts_idx += 2;
                }
                EdgeVerb::Conic => {
                    let c_pts = [
                        self.path_pts[pts_idx],
                        self.path_pts[pts_idx + 1],
                        self.path_pts[pts_idx + 2],
                    ];
                    let vec1 = c_pts[1] - c_pts[0];
                    let vec2 = c_pts[2] - c_pts[1];
                    let weight = self.weights[weight_idx];
                    weight_idx += 1;
                    let mut added_split = false;
                    if vec1.dot(vec2) < 0.0 {
                        let max_curvature = find_quad_max_curvature(&c_pts);
                        if 0.0 < max_curvature && max_curvature < 1.0 {
                            let conic = Conic {
                                pts: c_pts,
                                w: weight,
                            };
                            let mut pair = [Conic::default(); 2];
                            if conic.chop_at(max_curvature, &mut pair) {
                                let mut c_storage0 = [Point::ZERO; 3];
                                let mut c_storage1 = [Point::ZERO; 3];
                                let v1 = ReduceOrder::conic_pts(
                                    pair[0].pts,
                                    pair[0].w,
                                    &mut c_storage0,
                                );
                                let v2 = ReduceOrder::conic_pts(
                                    pair[1].pts,
                                    pair[1].w,
                                    &mut c_storage1,
                                );
                                let mut curve1 = if v1 != Verb::Line {
                                    pair[0].pts
                                } else {
                                    c_storage0
                                };
                                let mut curve2 = if v2 != Verb::Line {
                                    pair[1].pts
                                } else {
                                    c_storage1
                                };
                                if can_add_curve(v1, &mut curve1) && can_add_curve(v2, &mut curve2)
                                {
                                    self.contour_builder.add_curve(state, v1, &curve1, pair[0].w);
                                    self.contour_builder.add_curve(state, v2, &curve2, pair[1].w);
                                    added_split = true;
                                }
                            }
                        }
                    }
                    if !added_split {
                        self.contour_builder.add_conic(state, c_pts, weight);
                    }
                    pts_idx += 2;
                }
                EdgeVerb::Cubic => {
                    let c_pts = [
                        self.path_pts[pts_idx],
                        self.path_pts[pts_idx + 1],
                        self.path_pts[pts_idx + 2],
                        self.path_pts[pts_idx + 3],
                    ];
                    let mut split_t = [0.0f32; 3];
                    let breaks = DCubic::complex_break(&c_pts, &mut split_t);
                    if breaks == 0 {
                        self.contour_builder.add_cubic(state, c_pts);
                    } else {
                        #[derive(Copy, Clone)]
                        struct Split {
                            t: [f64; 2],
                            pts: [Point; 4],
                            reduced: [Point; 4],
                            verb: Verb,
                            can_add: bool,
                        }
                        let mut splits = [Split {
                            t: [0.0; 2],
                            pts: [Point::ZERO; 4],
                            reduced: [Point::ZERO; 4],
                            verb: Verb::Move,
                            can_add: false,
                        }; 4];
                        split_t[..breaks].sort_by(|a, b| {
                            a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal)
                        });
                        let dcubic = DCubic::from_points(c_pts);
                        for index in 0..=breaks {
                            splits[index].t[0] = if index > 0 {
                                split_t[index - 1] as f64
                            } else {
                                0.0
                            };
                            splits[index].t[1] = if index < breaks {
                                split_t[index] as f64
                            } else {
                                1.0
                            };
                            let part = dcubic.sub_divide(splits[index].t[0], splits[index].t[1]);
                            if !part.to_float_points(&mut splits[index].pts) {
                                return false;
                            }
                            splits[index].verb = ReduceOrder::cubic_pts(
                                splits[index].pts,
                                &mut splits[index].reduced,
                            );
                            let can = if splits[index].verb == Verb::Cubic {
                                can_add_curve(splits[index].verb, &mut splits[index].pts)
                            } else {
                                can_add_curve(splits[index].verb, &mut splits[index].reduced)
                            };
                            splits[index].can_add = can;
                        }
                        let break_limit = breaks.min(splits.len() - 1);
                        for index in 0..=breaks {
                            if !splits[index].can_add {
                                continue;
                            }
                            let mut prior = index;
                            while prior > 0 && !splits[prior - 1].can_add {
                                prior -= 1;
                            }
                            if prior < index {
                                splits[index].t[0] = splits[prior].t[0];
                                splits[index].pts[0] = splits[prior].pts[0];
                            }
                            let mut next = index;
                            while next < break_limit && !splits[next + 1].can_add {
                                next += 1;
                            }
                            if next > index {
                                splits[index].t[1] = splits[next].t[1];
                                splits[index].pts[3] = splits[next].pts[3];
                            }
                            if prior < index || next > index {
                                splits[index].verb = ReduceOrder::cubic_pts(
                                    splits[index].pts,
                                    &mut splits[index].reduced,
                                );
                            }
                            let ok = if splits[index].verb == Verb::Cubic {
                                can_add_curve(splits[index].verb, &mut splits[index].pts)
                            } else {
                                can_add_curve(splits[index].verb, &mut splits[index].reduced)
                            };
                            if !ok {
                                return false;
                            }
                            let curve_slice = if splits[index].verb == Verb::Cubic {
                                &splits[index].pts[..]
                            } else {
                                &splits[index].reduced[..]
                            };
                            self.contour_builder.add_curve(
                                state,
                                splits[index].verb,
                                curve_slice,
                                1.0,
                            );
                        }
                    }
                    pts_idx += 3;
                }
                EdgeVerb::Close => {
                    if !self.close(state) {
                        return false;
                    }
                    contour = None;
                    continue;
                }
                EdgeVerb::Done => unreachable!(),
            }
        }
        self.contour_builder.flush(state);
        if let Some(c) = contour {
            if state.contours[c].count > 0 && !self.allow_open_contours && !self.close(state) {
                return false;
            }
        }
        true
    }
}
