#![forbid(unsafe_code)]

//! Port of `SkOpBuilder.cpp`.

use skia_rs_geometry::{Matrix, Rect};
use skia_rs_path::analysis::compute_first_direction;
use skia_rs_path::builder::PathBuilder;
use skia_rs_path::types::{AddPathMode, FillType, FirstDirection, PathDataBuf, Verb};

use crate::arena::{OpPhase, OpState};
use crate::edge_builder::OpEdgeBuilder;
use crate::op::op;
use crate::path_writer::PathWriter;
use crate::simplify::{path_is_convex, simplify};
use crate::types::PathOp;

fn one_contour(path: &PathDataBuf) -> bool {
    if !path.is_finite() {
        return false;
    }
    for &v in path.verbs.iter().skip(1) {
        if v == Verb::Move {
            return false;
        }
    }
    true
}

fn reverse_path(path: &mut PathDataBuf) {
    let last_pt = match path.points.last() {
        Some(&p) => p,
        None => return,
    };
    let mut temp = PathBuilder::new(FillType::Winding);
    temp.move_to(last_pt);
    temp.reverse_path_to(&path.points, &path.verbs, &path.conics);
    temp.close();
    *path = temp.into_buf();
}

pub fn fix_winding(path: &mut PathDataBuf) -> bool {
    let fill_type = match path.fill_type {
        FillType::InverseEvenOdd => FillType::InverseWinding,
        FillType::EvenOdd => FillType::Winding,
        ft => ft,
    };
    if one_contour(path) {
        if let Some(raw) = path.as_raw(false) {
            let dir = compute_first_direction(&raw);
            if dir != FirstDirection::Unknown {
                if dir == FirstDirection::Cw {
                    reverse_path(path);
                }
                path.fill_type = fill_type;
                return true;
            }
        }
    }
    let mut state = OpState::new();
    let contour_head = state.contour_head;
    let mut builder = OpEdgeBuilder::new(path, contour_head);
    if builder.unparseable() || !builder.finish(&mut state) {
        return false;
    }
    if state.contours[contour_head].count == 0 {
        return true;
    }
    if state.contours[contour_head].next.is_none() {
        return false;
    }
    state.contour_join_all_segments();
    state.contour_reset_reverse();
    let mut write_path = false;
    state.phase = OpPhase::FixWinding;
    while let Some(top_span) = state.find_sortable_top(contour_head) {
        let top_seg = state.spans[top_span].segment;
        let top_contour = state.segments[top_seg].contour;
        if ((state.nested & 1) != 0) != (state.contours[top_contour].ccw != 0) {
            state.contours[top_contour].reverse = true;
            write_path = true;
        }
        state.contour_mark_all_done(top_contour);
        state.nested = 0;
    }
    if !write_path {
        path.fill_type = fill_type;
        return true;
    }

    let mut wound_path = PathWriter::new(fill_type);
    let mut test_opt = Some(contour_head);
    while let Some(test) = test_opt {
        if state.contours[test].count > 0 {
            if state.contours[test].reverse {
                state.contour_to_reverse_path(test, &mut wound_path);
            } else {
                state.contour_to_path(test, &mut wound_path);
            }
        }
        test_opt = state.contours[test].next;
    }
    *path = wound_path.native_path();
    true
}

#[derive(Clone, Debug, Default)]
pub struct OpBuilder {
    path_refs: Vec<PathDataBuf>,
    ops: Vec<PathOp>,
}

impl OpBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, path: PathDataBuf, path_op: PathOp) {
        if self.ops.is_empty() && path_op != PathOp::Union {
            self.path_refs.push(PathDataBuf::new(FillType::Winding));
            self.ops.push(PathOp::Union);
        }
        self.path_refs.push(path);
        self.ops.push(path_op);
    }

    pub fn reset(&mut self) {
        self.path_refs.clear();
        self.ops.clear();
    }

    pub fn resolve(&mut self) -> Option<PathDataBuf> {
        let count = self.ops.len();
        if count == 0 {
            self.reset();
            return Some(PathDataBuf::new(FillType::Winding));
        }
        let mut all_union = true;
        let mut first_dir = FirstDirection::Unknown;
        for index in 0..count {
            if self.ops[index] != PathOp::Union || self.path_refs[index].fill_type.is_inverse() {
                all_union = false;
                break;
            }
            if path_is_convex(&self.path_refs[index]) {
                let dir = match self.path_refs[index].as_raw(false) {
                    Some(raw) => compute_first_direction(&raw),
                    None => FirstDirection::Unknown,
                };
                if dir == FirstDirection::Unknown {
                    all_union = false;
                    break;
                }
                if first_dir == FirstDirection::Unknown {
                    first_dir = dir;
                } else if first_dir != dir {
                    reverse_path(&mut self.path_refs[index]);
                }
                continue;
            }
            let test_bounds = Rect::bounds_or_empty(&self.path_refs[index].points);
            for inner in 0..index {
                let inner_bounds = Rect::bounds_or_empty(&self.path_refs[inner].points);
                if Rect::intersects(inner_bounds, test_bounds) {
                    all_union = false;
                    break;
                }
            }
            if !all_union {
                break;
            }
        }
        if !all_union {
            let mut result = self.path_refs[0].clone();
            for index in 1..count {
                match op(&result, &self.path_refs[index], self.ops[index]) {
                    Some(res) => result = res,
                    None => {
                        self.reset();
                        return None;
                    }
                }
            }
            self.reset();
            return Some(result);
        }

        let mut sum = PathBuilder::new(FillType::Winding);
        let identity = Matrix::identity();
        for index in 0..count {
            let mut res = match simplify(&self.path_refs[index]) {
                Some(r) => r,
                None => {
                    self.reset();
                    return None;
                }
            };
            if !res.is_empty() {
                if !fix_winding(&mut res) {
                    self.reset();
                    return None;
                }
                sum.add_path(
                    &res.points,
                    &res.verbs,
                    &res.conics,
                    res.segment_mask,
                    res.is_finite(),
                    (res.is_a_type, res.is_a_dir, res.is_a_start_index),
                    res.convexity,
                    res.is_volatile,
                    &identity,
                    AddPathMode::Append,
                );
            }
        }
        self.reset();
        simplify(&sum.into_buf())
    }
}
