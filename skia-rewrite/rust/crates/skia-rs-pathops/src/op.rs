#![forbid(unsafe_code)]

//! Port of `SkPathOpsOp.cpp`.

use skia_rs_path::builder::PathBuilder;
use skia_rs_path::shapes::is_rect_contour;
use skia_rs_path::types::{Direction, FillType, PathDataBuf, Verb};

use crate::arena::{ContourId, OpState, SegmentId, SpanId, SK_MIN_S32};
use crate::edge_builder::OpEdgeBuilder;
use crate::path_writer::PathWriter;
use crate::simplify::simplify;
use crate::types::{PathOp, PathOpsMask};

const OP_INVERSE: [[[PathOp; 2]; 2]; 5] = [
    // Difference
    [
        [PathOp::Difference, PathOp::Intersect],
        [PathOp::Union, PathOp::ReverseDifference],
    ],
    // Intersect
    [
        [PathOp::Intersect, PathOp::Difference],
        [PathOp::ReverseDifference, PathOp::Union],
    ],
    // Union
    [
        [PathOp::Union, PathOp::ReverseDifference],
        [PathOp::Difference, PathOp::Intersect],
    ],
    // Xor
    [[PathOp::Xor, PathOp::Xor], [PathOp::Xor, PathOp::Xor]],
    // ReverseDifference
    [
        [PathOp::ReverseDifference, PathOp::Union],
        [PathOp::Intersect, PathOp::Difference],
    ],
];

const OUT_INVERSE: [[[bool; 2]; 2]; 5] = [
    [[false, false], [true, false]], // Difference
    [[false, false], [false, true]], // Intersect
    [[false, true], [true, true]],   // Union
    [[false, true], [true, false]],  // Xor
    [[false, true], [false, false]], // ReverseDifference
];

fn find_chase_op(
    state: &mut OpState,
    chase: &mut Vec<SpanId>,
    start_ptr: &mut SpanId,
    end_ptr: &mut SpanId,
    result: &mut Option<SegmentId>,
) -> bool {
    while let Some(span) = chase.pop() {
        let prev_pt_t = state.pt_t_prev(state.spans[span].pt_t);
        *start_ptr = state.pt_ts[prev_pt_t].span;
        let segment = state.spans[*start_ptr].segment;
        let mut done = true;
        let mut end_opt: Option<SpanId> = None;
        if let Some(last) =
            state.seg_active_angle(segment, *start_ptr, start_ptr, &mut end_opt, &mut done)
        {
            *start_ptr = state.angles[last].start.unwrap();
            *end_ptr = state.angles[last].end.unwrap();
            chase.push(span);
            *result = Some(state.angle_segment(last));
            return true;
        }
        if done {
            continue;
        }
        let end_val = match end_opt {
            Some(e) => e,
            None => continue,
        };
        *end_ptr = end_val;
        let mut winding = SK_MIN_S32;
        let mut sortable = false;
        let mut angle = match state.angle_winding(*start_ptr, *end_ptr, &mut winding, &mut sortable)
        {
            Some(a) => a,
            None => {
                *result = None;
                return true;
            }
        };
        if winding == SK_MIN_S32 {
            continue;
        }
        let mut sum_mi_winding = 0;
        let mut sum_su_winding = 0;
        if sortable {
            let a_seg = state.angle_segment(angle);
            sum_mi_winding = state.seg_update_winding_reverse(angle);
            if sum_mi_winding == SK_MIN_S32 {
                *result = None;
                return true;
            }
            sum_su_winding = state.seg_update_opp_winding_reverse(angle);
            if sum_su_winding == SK_MIN_S32 {
                *result = None;
                return true;
            }
            if state.seg_operand(a_seg) {
                core::mem::swap(&mut sum_mi_winding, &mut sum_su_winding);
            }
        }
        let mut first: Option<SegmentId> = None;
        let first_angle = angle;
        loop {
            angle = state.angles[angle].next.unwrap();
            if angle == first_angle {
                break;
            }
            let seg = state.angle_segment(angle);
            let a_start = state.angles[angle].start.unwrap();
            let a_end = state.angles[angle].end.unwrap();
            let mut max_winding = 0;
            let mut sum_winding = 0;
            let mut opp_max_winding = 0;
            let mut opp_sum_winding = 0;
            if sortable {
                state.seg_set_up_windings_binary(
                    seg,
                    a_start,
                    a_end,
                    &mut sum_mi_winding,
                    &mut sum_su_winding,
                    &mut max_winding,
                    &mut sum_winding,
                    &mut opp_max_winding,
                    &mut opp_sum_winding,
                );
            }
            if !state.seg_angle_done(angle) {
                let starter = state.span_starter(a_start, a_end);
                if first.is_none() && (sortable || state.spans[starter].wind_sum != SK_MIN_S32) {
                    first = Some(seg);
                    *start_ptr = a_start;
                    *end_ptr = a_end;
                }
                if sortable
                    && !state.seg_mark_angle_opp(
                        seg,
                        max_winding,
                        sum_winding,
                        opp_max_winding,
                        opp_sum_winding,
                        angle,
                        None,
                    )
                {
                    return false;
                }
            }
        }
        if let Some(f) = first {
            chase.push(span);
            *result = Some(f);
            return true;
        }
    }
    *result = None;
    true
}

fn bridge_op(
    state: &mut OpState,
    contour_list: ContourId,
    op: PathOp,
    xor_mask: i32,
    xor_op_mask: i32,
    writer: &mut PathWriter,
) -> bool {
    let mut unsortable = false;
    let mut last_simple;
    let mut simple = false;
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
            if state.seg_active_op(current, start, end, xor_mask, xor_op_mask, op) {
                loop {
                    if !unsortable && state.seg_done(current) {
                        break;
                    }
                    let mut next_start = start;
                    let mut next_end = end;
                    last_simple = simple;
                    let next = state.seg_find_next_op(
                        current,
                        &mut chase,
                        &mut next_start,
                        &mut next_end,
                        &mut unsortable,
                        &mut simple,
                        op,
                        xor_mask,
                        xor_op_mask,
                    );
                    let next_seg = match next {
                        Some(ns) => ns,
                        None => {
                            if !unsortable
                                && writer.has_move()
                                && state.segments[current].verb != Verb::Line
                                && !writer.is_closed(state)
                            {
                                if !state.seg_add_curve_to(current, start, end, writer) {
                                    return false;
                                }
                            } else if last_simple
                                && !state.seg_add_curve_to(current, start, end, writer)
                            {
                                return false;
                            }
                            break;
                        }
                    };
                    if !state.seg_add_curve_to(current, start, end, writer) {
                        return false;
                    }
                    current = next_seg;
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
            let mut next_cur: Option<SegmentId> = None;
            if !find_chase_op(state, &mut chase, &mut start, &mut end, &mut next_cur) {
                return false;
            }
            match next_cur {
                Some(c) => current = c,
                None => break,
            }
        }
    }
    true
}

pub fn op(one: &PathDataBuf, two: &PathDataBuf, mut path_op: PathOp) -> Option<PathDataBuf> {
    let one_inv = usize::from(one.fill_type.is_inverse());
    let two_inv = usize::from(two.fill_type.is_inverse());
    path_op = OP_INVERSE[path_op as usize][one_inv][two_inv];
    let inverse_fill = OUT_INVERSE[path_op as usize][one_inv][two_inv];
    let fill_type = if inverse_fill {
        FillType::InverseEvenOdd
    } else {
        FillType::EvenOdd
    };

    if path_op == PathOp::Intersect {
        if let (Some(rc1), Some(rc2)) = (
            is_rect_contour(&one.points, &one.verbs, one.segment_mask as u32, false),
            is_rect_contour(&two.points, &two.verbs, two.segment_mask as u32, false),
        ) {
            let mut r1 = rc1.rect;
            let mut b = PathBuilder::new(fill_type);
            if r1.intersect(rc2.rect) {
                b.add_rect(r1, Direction::Cw, 0);
            }
            return Some(b.into_buf());
        }
    }

    if one.is_empty() || two.is_empty() {
        let mut work = match path_op {
            PathOp::Intersect => PathDataBuf::new(FillType::Winding),
            PathOp::Union | PathOp::Xor => {
                if one.is_empty() {
                    two.clone()
                } else {
                    one.clone()
                }
            }
            PathOp::Difference => {
                if !one.is_empty() {
                    one.clone()
                } else {
                    PathDataBuf::new(FillType::Winding)
                }
            }
            PathOp::ReverseDifference => {
                if !two.is_empty() {
                    two.clone()
                } else {
                    PathDataBuf::new(FillType::Winding)
                }
            }
        };
        if inverse_fill != work.fill_type.is_inverse() {
            work.fill_type = work.fill_type.toggle_inverse();
        }
        return simplify(&work);
    }

    let mut state = OpState::new();
    let mut contour_list = state.contour_head;
    let (minuend, subtrahend, eff_op) = if path_op == PathOp::ReverseDifference {
        (two, one, PathOp::Difference)
    } else {
        (one, two, path_op)
    };

    let mut builder = OpEdgeBuilder::new(minuend, contour_list);
    if builder.unparseable() {
        return None;
    }
    let xor_mask = builder.xor_mask();
    builder.add_operand(subtrahend);
    if !builder.finish(&mut state) {
        return None;
    }
    let xor_op_mask = builder.xor_mask();
    if !state.sort_contour_list(
        &mut contour_list,
        xor_mask == PathOpsMask::EvenOdd,
        xor_op_mask == PathOpsMask::EvenOdd,
    ) {
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
    if !bridge_op(
        &mut state,
        contour_list,
        eff_op,
        xor_mask as i32,
        xor_op_mask as i32,
        &mut wrapper,
    ) {
        return None;
    }
    wrapper.assemble(&mut state);
    Some(wrapper.native_path())
}
