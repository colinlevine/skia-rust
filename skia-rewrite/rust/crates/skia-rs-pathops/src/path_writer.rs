#![forbid(unsafe_code)]

//! Port of `SkPathWriter.h/.cpp`.

use skia_rs_geometry::{Matrix, Point};
use skia_rs_path::builder::PathBuilder;
use skia_rs_path::types::{AddPathMode, FillType, PathDataBuf};

use crate::arena::{OpState, PtTId};
use crate::types::zero_or_one;

pub struct PathWriter {
    builder: PathBuilder,
    current: PathBuilder,
    partials: Vec<PathBuilder>,
    end_pt_ts: Vec<PtTId>,
    defer: [Option<PtTId>; 2],
    first_pt_t: Option<PtTId>,
}

impl PathWriter {
    pub fn new(fill_type: FillType) -> Self {
        let mut pw = Self {
            builder: PathBuilder::new(fill_type),
            current: PathBuilder::new(FillType::Winding),
            partials: Vec::new(),
            end_pt_ts: Vec::new(),
            defer: [None, None],
            first_pt_t: None,
        };
        pw.init();
        pw
    }

    pub fn init(&mut self) {
        self.current.reset();
        self.first_pt_t = None;
        self.defer = [None, None];
    }

    #[inline]
    pub fn has_move(&self) -> bool {
        self.first_pt_t.is_none()
    }

    pub fn is_closed(&self, state: &OpState) -> bool {
        self.matched_last(state, self.first_pt_t)
    }

    pub fn native_path(&mut self) -> PathDataBuf {
        core::mem::take(&mut self.builder).into_buf()
    }

    fn close(&mut self, state: &OpState) {
        if self.current.is_empty() {
            return;
        }
        debug_assert!(self.is_closed(state));
        self.current.close();
        if let Some(raw) = self.current.buf.as_raw(false) {
            self.builder.add_raw(&raw);
        }
        self.init();
    }

    pub fn conic_to(&mut self, state: &OpState, pt1: Point, pt2: PtTId, weight: f32) {
        let pt2_pt = self.update(state, pt2);
        self.current.conic_to(pt1, pt2_pt, weight);
    }

    pub fn cubic_to(&mut self, state: &OpState, pt1: Point, pt2: Point, pt3: PtTId) {
        let pt3_pt = self.update(state, pt3);
        self.current.cubic_to(pt1, pt2, pt3_pt);
    }

    pub fn quad_to(&mut self, state: &OpState, pt1: Point, pt2: PtTId) {
        let pt2_pt = self.update(state, pt2);
        self.current.quad_to(pt1, pt2_pt);
    }

    pub fn deferred_line(&mut self, state: &OpState, pt: PtTId) -> bool {
        let d0 = match self.defer[0] {
            Some(d) => d,
            None => return true,
        };
        if d0 == pt {
            return true;
        }
        if state.pt_t_contains(pt, d0) {
            return true;
        }
        if self.matched_last(state, Some(pt)) {
            return false;
        }
        if self.defer[1].is_some() && self.changed_slopes(state, pt) {
            self.line_to(state);
            self.defer[0] = self.defer[1];
        }
        self.defer[1] = Some(pt);
        true
    }

    pub fn deferred_move(&mut self, state: &OpState, pt: PtTId) {
        if self.defer[1].is_none() {
            self.first_pt_t = Some(pt);
            self.defer[0] = Some(pt);
            return;
        }
        if !self.matched_last(state, Some(pt)) {
            self.finish_contour(state);
            self.first_pt_t = Some(pt);
            self.defer[0] = Some(pt);
        }
    }

    pub fn finish_contour(&mut self, state: &OpState) {
        if !self.matched_last(state, self.defer[0]) {
            if self.defer[1].is_none() {
                return;
            }
            self.line_to(state);
        }
        if self.current.is_empty() {
            return;
        }
        if self.is_closed(state) {
            self.close(state);
        } else if let (Some(first), Some(last)) = (self.first_pt_t, self.defer[1]) {
            self.end_pt_ts.push(first);
            self.end_pt_ts.push(last);
            let cur = core::mem::take(&mut self.current);
            self.partials.push(cur);
            self.init();
        }
    }

    fn line_to(&mut self, state: &OpState) {
        if self.current.is_empty() {
            self.move_to(state);
        }
        if let Some(d1) = self.defer[1] {
            self.current.line_to(state.pt_ts[d1].pt);
        }
    }

    fn matched_last(&self, state: &OpState, test: Option<PtTId>) -> bool {
        if test == self.defer[1] {
            return true;
        }
        match (test, self.defer[1]) {
            (Some(t), Some(d1)) => state.pt_t_contains(t, d1),
            _ => false,
        }
    }

    fn move_to(&mut self, state: &OpState) {
        if let Some(first) = self.first_pt_t {
            self.current.move_to(state.pt_ts[first].pt);
        }
    }

    fn update(&mut self, state: &OpState, pt: PtTId) -> Point {
        if self.defer[1].is_none() {
            self.move_to(state);
        } else if !self.matched_last(state, self.defer[0]) {
            self.line_to(state);
        }
        let mut result = state.pt_ts[pt].pt;
        if let Some(first) = self.first_pt_t {
            let first_pt = state.pt_ts[first].pt;
            if result != first_pt && state.pt_t_contains(first, pt) {
                result = first_pt;
            }
        }
        self.defer[0] = Some(pt);
        self.defer[1] = Some(pt);
        result
    }

    fn some_assembly_required(&mut self, state: &OpState) -> bool {
        self.finish_contour(state);
        !self.end_pt_ts.is_empty()
    }

    fn changed_slopes(&self, state: &OpState, pt_t: PtTId) -> bool {
        if self.matched_last(state, self.defer[0]) {
            return false;
        }
        let d0 = self.defer[0].unwrap();
        let d1 = self.defer[1].unwrap();
        let defer_dxdy = state.pt_ts[d1].pt - state.pt_ts[d0].pt;
        let line_dxdy = state.pt_ts[pt_t].pt - state.pt_ts[d1].pt;
        defer_dxdy.x * line_dxdy.y != defer_dxdy.y * line_dxdy.x
    }

    pub fn assemble(&mut self, state: &mut OpState) {
        if !self.some_assembly_required(state) {
            return;
        }
        let end_count = self.end_pt_ts.len();
        const MAX_PARTIAL_CONTOURS: usize = 10000;
        if end_count > MAX_PARTIAL_CONTOURS * 2 {
            return;
        }

        // Lengthen any partial contour adjacent to a simple segment
        for p_index in 0..end_count {
            let mut op_pt_t = self.end_pt_ts[p_index];
            let mut part_writer = PathWriter::new(FillType::Winding);
            loop {
                let t = state.pt_ts[op_pt_t].t;
                if !zero_or_one(t) {
                    break;
                }
                let op_span_base = state.pt_ts[op_pt_t].span;
                let mut start = if t != 0.0 {
                    match state.spans[op_span_base].prev {
                        Some(p) => p,
                        None => break,
                    }
                } else {
                    match state.spans[op_span_base].next {
                        Some(n) => n,
                        None => break,
                    }
                };
                let mut step = if t != 0.0 { 1 } else { -1 };
                let op_segment = state.spans[op_span_base].segment;
                let next_segment = match state.seg_is_simple(op_segment, &mut start, &mut step) {
                    Some(ns) => ns,
                    None => break,
                };
                let op_span_end = if state.span_t(start) != 0.0 {
                    match state.spans[start].prev {
                        Some(p) => p,
                        None => break,
                    }
                } else {
                    match state.spans[start].next {
                        Some(n) => n,
                        None => break,
                    }
                };
                let starter = state.span_starter(start, op_span_end);
                if state.spans[starter].already_added {
                    break;
                }
                let _ = state.seg_add_curve_to(next_segment, start, op_span_end, &mut part_writer);
                op_pt_t = state.spans[op_span_end].pt_t;
                self.end_pt_ts[p_index] = op_pt_t;
            }
            part_writer.finish_contour(state);
            if part_writer.partials.is_empty() {
                continue;
            }
            let part = part_writer.partials[0].buf.clone();
            let partial = &mut self.partials[p_index >> 1];
            let identity = Matrix::identity();
            if (p_index & 1) != 0 {
                partial.add_path(
                    &part.points,
                    &part.verbs,
                    &part.conics,
                    part.segment_mask,
                    part.is_finite(),
                    (part.is_a_type, part.is_a_dir, part.is_a_start_index),
                    part.convexity,
                    part.is_volatile,
                    &identity,
                    AddPathMode::Extend,
                );
            } else {
                let mut reverse = PathBuilder::new(FillType::Winding);
                reverse.reverse_add_path(&part.points, &part.verbs, &part.conics);
                let detached = core::mem::take(partial).into_buf();
                reverse.add_path(
                    &detached.points,
                    &detached.verbs,
                    &detached.conics,
                    detached.segment_mask,
                    detached.is_finite(),
                    (
                        detached.is_a_type,
                        detached.is_a_dir,
                        detached.is_a_start_index,
                    ),
                    detached.convexity,
                    detached.is_volatile,
                    &identity,
                    AddPathMode::Extend,
                );
                *partial = reverse;
            }
        }

        let link_count = end_count / 2;
        let mut s_link = vec![i32::MAX; link_count];
        let mut e_link = vec![i32::MAX; link_count];
        let entries = end_count * (end_count - 1) / 2;
        let mut distances = Vec::with_capacity(entries);
        let mut sorted_dist = Vec::with_capacity(entries);
        let mut dist_lookup = Vec::with_capacity(entries);
        let mut r_row = 0usize;
        let mut d_index = 0usize;
        for r_index in 0..(end_count - 1) {
            let o_pt = state.pt_ts[self.end_pt_ts[r_index]].pt;
            for i_index in (r_index + 1)..end_count {
                let i_pt = state.pt_ts[self.end_pt_ts[i_index]].pt;
                let dx = i_pt.x as f64 - o_pt.x as f64;
                let dy = i_pt.y as f64 - o_pt.y as f64;
                let dist = dx * dx + dy * dy;
                dist_lookup.push(r_row + i_index);
                distances.push(dist);
                sorted_dist.push(d_index);
                d_index += 1;
            }
            r_row += end_count;
        }
        sorted_dist.sort_by(|&a, &b| {
            distances[a]
                .partial_cmp(&distances[b])
                .unwrap_or(core::cmp::Ordering::Equal)
        });

        let mut remaining = link_count;
        for &sd in &sorted_dist {
            let pair = dist_lookup[sd];
            let row = pair / end_count;
            let col = pair - row * end_count;
            let ndx_one = row >> 1;
            let end_one = (row & 1) != 0;
            let val_one = if end_one {
                e_link[ndx_one]
            } else {
                s_link[ndx_one]
            };
            if val_one != i32::MAX {
                continue;
            }
            let ndx_two = col >> 1;
            let end_two = (col & 1) != 0;
            let val_two = if end_two {
                e_link[ndx_two]
            } else {
                s_link[ndx_two]
            };
            if val_two != i32::MAX {
                continue;
            }
            let flip = end_one == end_two;
            let link_val_one = if flip {
                !(ndx_two as i32)
            } else {
                ndx_two as i32
            };
            let link_val_two = if flip {
                !(ndx_one as i32)
            } else {
                ndx_one as i32
            };
            if end_one {
                e_link[ndx_one] = link_val_one;
            } else {
                s_link[ndx_one] = link_val_one;
            }
            if end_two {
                e_link[ndx_two] = link_val_two;
            } else {
                s_link[ndx_two] = link_val_two;
            }
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }

        let mut r_index = 0usize;
        let identity = Matrix::identity();
        loop {
            let mut forward = true;
            let mut first = true;
            let s_index = s_link[r_index];
            s_link[r_index] = i32::MAX;
            let mut e_index = if s_index < 0 {
                let idx = (!s_index) as usize;
                let v = s_link[idx];
                s_link[idx] = i32::MAX;
                v
            } else {
                let idx = s_index as usize;
                let v = e_link[idx];
                e_link[idx] = i32::MAX;
                v
            };

            loop {
                let contour = self.partials[r_index].buf.clone();
                if !first {
                    if self.builder.get_last_pt().is_none() {
                        return;
                    }
                    if !forward && contour.points.is_empty() {
                        return;
                    }
                }
                if forward {
                    let mode = if first {
                        AddPathMode::Append
                    } else {
                        AddPathMode::Extend
                    };
                    self.builder.add_path(
                        &contour.points,
                        &contour.verbs,
                        &contour.conics,
                        contour.segment_mask,
                        contour.is_finite(),
                        (
                            contour.is_a_type,
                            contour.is_a_dir,
                            contour.is_a_start_index,
                        ),
                        contour.convexity,
                        contour.is_volatile,
                        &identity,
                        mode,
                    );
                } else {
                    self.builder
                        .reverse_path_to(&contour.points, &contour.verbs, &contour.conics);
                }
                if first {
                    first = false;
                }
                let check_idx = if ((r_index as i32) != e_index) ^ forward {
                    e_index
                } else {
                    !e_index
                };
                if s_index == check_idx {
                    self.builder.close();
                    break;
                }
                if forward {
                    e_index = e_link[r_index];
                    e_link[r_index] = i32::MAX;
                    if e_index >= 0 {
                        s_link[e_index as usize] = i32::MAX;
                    } else {
                        e_link[(!e_index) as usize] = i32::MAX;
                    }
                } else {
                    e_index = s_link[r_index];
                    s_link[r_index] = i32::MAX;
                    if e_index >= 0 {
                        e_link[e_index as usize] = i32::MAX;
                    } else {
                        s_link[(!e_index) as usize] = i32::MAX;
                    }
                }
                let next_r = e_index;
                if next_r < 0 {
                    forward = !forward;
                    r_index = (!next_r) as usize;
                } else {
                    r_index = next_r as usize;
                }
            }

            let mut found_next = link_count;
            for idx in 0..link_count {
                if s_link[idx] != i32::MAX {
                    found_next = idx;
                    break;
                }
            }
            r_index = found_next;
            if r_index >= link_count {
                break;
            }
        }
    }
}
