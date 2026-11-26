#![forbid(unsafe_code)]

//! Port of `SkPathOpsSimplify.cpp`.

use skia_rs_geometry::{Point, Vector};
use skia_rs_path::analysis::compute_convexity;
use skia_rs_path::iter::LegacyPathIter;
use skia_rs_path::types::{Convexity, FillType, PathDataBuf, Verb};

use crate::arena::{ContourId, OpState, SpanId};
use crate::edge_builder::OpEdgeBuilder;
use crate::path_writer::PathWriter;
use crate::types::PathOpsMask;

fn bridge_winding(state: &mut OpState, contour_list: ContourId, writer: &mut PathWriter) -> bool {
    let mut unsortable = false;
    loop {
        let span = match state.find_sortable_top(contour_list) {
            Some(s) => s,
            None => break,
        };
        let mut current = state.spans[span].segment;
        let mut start = state.spans[span].next.unwrap();
        let mut end = span;
        let mut chase: Vec<SpanId> = Vec::new();
        loop {
            if state.seg_active_winding(start, end) {
                loop {
                    if !unsortable && state.seg_done(current) {
                        break;
                    }
                    let mut next_start = start;
                    let mut next_end = end;
                    let next = match state.seg_find_next_winding(
                        current,
                        &mut chase,
                        &mut next_start,
                        &mut next_end,
                        &mut unsortable,
                    ) {
                        Some(n) => n,
                        None => break,
                    };
                    if !state.seg_add_curve_to(current, start, end, writer) {
                        return false;
                    }
                    current = next;
                    start = next_start;
                    end = next_end;
                    if writer.is_closed(state) {
                        break;
                    }
                    if unsortable {
                        let starter = state.span_starter(start, end);
                        if state.spans[starter].done {
                            break;
                        }
                    }
                }
                if state.seg_active_winding(start, end) && !writer.is_closed(state) {
                    let span_start = state.span_starter(start, end);
                    if !state.spans[span_start].done {
                        if !state.seg_add_curve_to(current, start, end, writer) {
                            return false;
                        }
                        state.seg_mark_done(current, span_start);
                    }
                }
                writer.finish_contour(state);
            } else {
                let mut last: Option<SpanId> = None;
                if !state.seg_mark_and_chase_done(current, start, end, Some(&mut last)) {
                    return false;
                }
                if let Some(l) = last {
                    if !state.spans[l].chased {
                        state.spans[l].chased = true;
                        chase.push(l);
                    }
                }
            }
            match state.find_chase(&mut chase, &mut start, &mut end) {
                Some(c) => current = c,
                None => break,
            }
        }
    }
    true
}

fn bridge_xor(state: &mut OpState, contour_list: ContourId, writer: &mut PathWriter) -> bool {
    let mut unsortable = false;
    let mut safety_net = 1_000_000;
    loop {
        let span = match state.find_undone(contour_list) {
            Some(s) => s,
            None => break,
        };
        let mut current = state.spans[span].segment;
        let mut start = state.spans[span].next.unwrap();
        let mut end = span;
        loop {
            safety_net -= 1;
            if safety_net < 0 {
                return false;
            }
            if !unsortable && state.seg_done(current) {
                break;
            }
            let mut next_start = start;
            let mut next_end = end;
            let next = match state.seg_find_next_xor(
                current,
                &mut next_start,
                &mut next_end,
                &mut unsortable,
            ) {
                Some(n) => n,
                None => break,
            };
            if !state.seg_add_curve_to(current, start, end, writer) {
                return false;
            }
            current = next;
            start = next_start;
            end = next_end;
            if writer.is_closed(state) {
                break;
            }
            if unsortable {
                let starter = state.span_starter(start, end);
                if state.spans[starter].done {
                    break;
                }
            }
        }
        if !writer.is_closed(state) {
            let span_start = state.span_starter(start, end);
            if !state.spans[span_start].done {
                return false;
            }
        }
        writer.finish_contour(state);
    }
    true
}

fn path_is_trivial(path: &PathDataBuf) -> bool {
    let mut iter = LegacyPathIter::new(&path.points, &path.verbs, &path.conics, true);
    let mut prev_pt = Point::ZERO;
    let mut prev_vec = Vector::ZERO;

    let add_pt = |curr_pt: Point, prev_pt: &mut Point, prev_vec: &mut Vector| -> bool {
        if curr_pt == *prev_pt {
            return true;
        }
        let curr_vec = curr_pt - *prev_pt;
        if prev_vec.cross(curr_vec) != 0.0 {
            return false;
        }
        *prev_vec = curr_vec;
        *prev_pt = curr_pt;
        true
    };

    while let Some(rec) = iter.next_rec() {
        let pts = rec.pts();
        match rec.verb {
            Verb::Move => {
                prev_pt = pts[0];
                prev_vec = Vector::ZERO;
            }
            Verb::Cubic => {
                if !add_pt(pts[3], &mut prev_pt, &mut prev_vec)
                    || !add_pt(pts[2], &mut prev_pt, &mut prev_vec)
                    || !add_pt(pts[1], &mut prev_pt, &mut prev_vec)
                    || !add_pt(pts[0], &mut prev_pt, &mut prev_vec)
                {
                    return false;
                }
            }
            Verb::Conic | Verb::Quad => {
                if !add_pt(pts[2], &mut prev_pt, &mut prev_vec)
                    || !add_pt(pts[1], &mut prev_pt, &mut prev_vec)
                    || !add_pt(pts[0], &mut prev_pt, &mut prev_vec)
                {
                    return false;
                }
            }
            Verb::Line => {
                if !add_pt(pts[1], &mut prev_pt, &mut prev_vec)
                    || !add_pt(pts[0], &mut prev_pt, &mut prev_vec)
                {
                    return false;
                }
            }
            Verb::Close => {}
        }
    }
    true
}

pub fn path_is_convex(path: &PathDataBuf) -> bool {
    let conv = if path.convexity == Convexity::Unknown {
        compute_convexity(&path.points, &path.verbs, &path.conics)
    } else {
        path.convexity
    };
    conv.is_convex()
}

pub fn simplify(path: &PathDataBuf) -> Option<PathDataBuf> {
    let fill_type = if path.fill_type.is_inverse() {
        FillType::InverseEvenOdd
    } else {
        FillType::EvenOdd
    };

    if path_is_convex(path) {
        let mut result = if !path_is_trivial(path) {
            path.clone()
        } else {
            PathDataBuf::new(fill_type)
        };
        result.fill_type = fill_type;
        return Some(result);
    }

    let mut state = OpState::new();
    let mut contour_list = state.contour_head;
    let mut builder = OpEdgeBuilder::new(path, contour_list);
    if !builder.finish(&mut state) {
        return None;
    }
    if !state.sort_contour_list(&mut contour_list, false, false) {
        return Some(PathDataBuf::new(fill_type));
    }

    let mut current_opt = Some(contour_list);
    while let Some(current) = current_opt {
        let mut next_opt = Some(current);
        while let Some(next) = next_opt {
            if !state.add_intersect_ts(current, next) {
                break;
            }
            next_opt = state.contours[next].next;
        }
        current_opt = state.contours[current].next;
    }

    if !state.handle_coincidence(contour_list) {
        return None;
    }

    let mut wrapper = PathWriter::new(fill_type);
    let ok = if builder.xor_mask() == PathOpsMask::Winding {
        bridge_winding(&mut state, contour_list, &mut wrapper)
    } else {
        bridge_xor(&mut state, contour_list, &mut wrapper)
    };
    if !ok {
        return None;
    }
    wrapper.assemble(&mut state);
    Some(wrapper.native_path())
}
