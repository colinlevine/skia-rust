#![forbid(unsafe_code)]

//! Port of `SkPathOpsCommon.h/.cpp`.

use crate::arena::{AngleId, ContourId, OpCoincidenceList, OpState, SegmentId, SpanId, SK_MIN_S32};

impl OpState {
    pub fn angle_winding(
        &mut self,
        start: SpanId,
        end: SpanId,
        winding_ptr: &mut i32,
        sortable_ptr: &mut bool,
    ) -> Option<AngleId> {
        let mut angle = match self.seg_span_to_angle(start, end) {
            Some(a) => a,
            None => {
                *winding_ptr = SK_MIN_S32;
                return None;
            }
        };
        let mut compute_winding;
        let mut first_angle = angle;
        let mut looped = false;
        let mut unorderable = false;
        let mut winding = SK_MIN_S32;
        loop {
            angle = self.angles[angle].next?;
            unorderable |= self.angles[angle].unorderable;
            compute_winding = unorderable || (angle == first_angle && looped);
            if compute_winding {
                break;
            }
            looped |= angle == first_angle;
            winding = self.seg_wind_sum(angle);
            if winding != SK_MIN_S32 {
                break;
            }
        }
        if compute_winding {
            first_angle = angle;
            winding = SK_MIN_S32;
            loop {
                let start_span = self.angles[angle].start.unwrap();
                let end_span = self.angles[angle].end.unwrap();
                let lesser = self.span_starter(start_span, end_span);
                let mut test_winding = self.spans[lesser].wind_sum;
                if test_winding == SK_MIN_S32 {
                    test_winding = self.span_compute_wind_sum(lesser);
                }
                if test_winding != SK_MIN_S32 {
                    winding = test_winding;
                }
                angle = self.angles[angle].next.unwrap();
                if angle == first_angle {
                    break;
                }
            }
        }
        *sortable_ptr = !unorderable;
        *winding_ptr = winding;
        Some(angle)
    }

    pub fn find_undone(&mut self, contour_head: ContourId) -> Option<SpanId> {
        let mut contour_opt = Some(contour_head);
        while let Some(contour) = contour_opt {
            if !self.contours[contour].done {
                if let Some(res) = self.contour_undone_span(contour) {
                    return Some(res);
                }
            }
            contour_opt = self.contours[contour].next;
        }
        None
    }

    pub fn find_chase(
        &mut self,
        chase: &mut Vec<SpanId>,
        start_ptr: &mut SpanId,
        end_ptr: &mut SpanId,
    ) -> Option<SegmentId> {
        while let Some(span) = chase.pop() {
            let segment = self.spans[span].segment;
            let next_pt_t = self.pt_ts[self.spans[span].pt_t].next;
            *start_ptr = self.pt_ts[next_pt_t].span;
            let mut done = true;
            let mut end_opt: Option<SpanId> = None;
            if let Some(last) =
                self.seg_active_angle(segment, *start_ptr, start_ptr, &mut end_opt, &mut done)
            {
                *start_ptr = self.angles[last].start.unwrap();
                *end_ptr = self.angles[last].end.unwrap();
                chase.push(span);
                return Some(self.angle_segment(last));
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
            let mut angle = self.angle_winding(*start_ptr, *end_ptr, &mut winding, &mut sortable)?;
            if winding == SK_MIN_S32 {
                continue;
            }
            let mut sum_winding = 0;
            if sortable {
                sum_winding = self.seg_update_winding_reverse(angle);
            }
            let mut first: Option<SegmentId> = None;
            let first_angle = angle;
            loop {
                angle = self.angles[angle].next.unwrap();
                if angle == first_angle {
                    break;
                }
                let seg = self.angle_segment(angle);
                let a_start = self.angles[angle].start.unwrap();
                let a_end = self.angles[angle].end.unwrap();
                let mut max_winding = 0;
                if sortable {
                    self.seg_set_up_winding(a_start, a_end, &mut max_winding, &mut sum_winding);
                }
                if !self.seg_angle_done(angle) {
                    let starter = self.span_starter(a_start, a_end);
                    if first.is_none()
                        && (sortable || self.spans[starter].wind_sum != SK_MIN_S32)
                    {
                        first = Some(seg);
                        *start_ptr = a_start;
                        *end_ptr = a_end;
                    }
                    if sortable {
                        let _ = self.seg_mark_angle(seg, max_winding, sum_winding, angle, None);
                    }
                }
            }
            if let Some(f) = first {
                chase.push(span);
                return Some(f);
            }
        }
        None
    }

    pub fn sort_contour_list(
        &mut self,
        contour_list: &mut ContourId,
        even_odd: bool,
        opp_even_odd: bool,
    ) -> bool {
        let mut list: Vec<ContourId> = Vec::new();
        let mut contour_opt = Some(*contour_list);
        while let Some(c) = contour_opt {
            if self.contours[c].count > 0 {
                let is_op = self.contours[c].operand;
                self.contours[c].opp_xor = if is_op { even_odd } else { opp_even_odd };
                list.push(c);
            }
            contour_opt = self.contours[c].next;
        }
        if list.is_empty() {
            return false;
        }
        if list.len() > 1 {
            let contours = &self.contours;
            list.sort_by(|&a, &b| {
                let ba = &contours[a].bounds;
                let bb = &contours[b].bounds;
                let ord = if ba.top == bb.top {
                    ba.left.partial_cmp(&bb.left)
                } else {
                    ba.top.partial_cmp(&bb.top)
                };
                ord.unwrap_or(core::cmp::Ordering::Equal)
            });
        }
        let head = list[0];
        self.contour_head = head;
        *contour_list = head;
        for i in 0..(list.len() - 1) {
            self.contours[list[i]].next = Some(list[i + 1]);
        }
        self.contours[*list.last().unwrap()].next = None;
        true
    }

    fn calc_angles_all(&mut self, contour_list: ContourId) {
        let mut c_opt = Some(contour_list);
        while let Some(c) = c_opt {
            self.contour_calc_angles(c);
            c_opt = self.contours[c].next;
        }
    }

    fn missing_coincidence_all(&mut self, contour_list: ContourId) -> bool {
        let mut c_opt = Some(contour_list);
        let mut result = false;
        while let Some(c) = c_opt {
            result |= self.contour_missing_coincidence(c);
            c_opt = self.contours[c].next;
        }
        result
    }

    fn move_multiples_all(&mut self, contour_list: ContourId) -> bool {
        let mut c_opt = Some(contour_list);
        while let Some(c) = c_opt {
            if !self.contour_move_multiples(c) {
                return false;
            }
            c_opt = self.contours[c].next;
        }
        true
    }

    fn move_nearby_all(&mut self, contour_list: ContourId) -> bool {
        let mut c_opt = Some(contour_list);
        while let Some(c) = c_opt {
            if !self.contour_move_nearby(c) {
                return false;
            }
            c_opt = self.contours[c].next;
        }
        true
    }

    fn sort_angles_all(&mut self, contour_list: ContourId) -> bool {
        let mut c_opt = Some(contour_list);
        while let Some(c) = c_opt {
            if !self.contour_sort_angles(c) {
                return false;
            }
            c_opt = self.contours[c].next;
        }
        true
    }

    pub fn handle_coincidence(&mut self, contour_list: ContourId) -> bool {
        if !self.coincidence_add_expanded() {
            return false;
        }
        if !self.move_multiples_all(contour_list) {
            return false;
        }
        if !self.move_nearby_all(contour_list) {
            return false;
        }
        self.coincidence_correct_ends();
        if !self.coincidence_add_end_moved_spans() {
            return false;
        }
        const SAFETY_COUNT: i32 = 3;
        let mut safety_hatch = SAFETY_COUNT;
        loop {
            let mut added = false;
            if !self.coincidence_add_missing(&mut added) {
                return false;
            }
            if !added {
                break;
            }
            safety_hatch -= 1;
            if safety_hatch == 0 {
                return false;
            }
            let _ = self.move_nearby_all(contour_list);
        }
        if self.coincidence_expand() {
            let mut added = false;
            if !self.coincidence_add_missing(&mut added) {
                return false;
            }
            if !self.coincidence_add_expanded() {
                return false;
            }
            if !self.move_multiples_all(contour_list) {
                return false;
            }
            let _ = self.move_nearby_all(contour_list);
        }
        if !self.coincidence_add_expanded() {
            return false;
        }
        let _ = self.coincidence_mark();
        if self.missing_coincidence_all(contour_list) {
            let _ = self.coincidence_expand();
            if !self.coincidence_add_expanded() {
                return false;
            }
            if !self.coincidence_mark() {
                return false;
            }
        } else {
            let _ = self.coincidence_expand();
        }
        let _ = self.coincidence_expand();

        let mut overlaps = OpCoincidenceList::default();
        safety_hatch = SAFETY_COUNT;
        loop {
            let pairs_head = if overlaps.is_empty() {
                self.coincidence.head
            } else {
                overlaps.head
            };
            if !self.coincidence_apply_head(pairs_head) {
                return false;
            }
            match self.coincidence_find_overlaps_head(pairs_head) {
                Some(new_overlaps) => overlaps = new_overlaps,
                None => return false,
            }
            safety_hatch -= 1;
            if safety_hatch == 0 {
                return false;
            }
            if overlaps.is_empty() {
                break;
            }
        }
        self.calc_angles_all(contour_list);
        if !self.sort_angles_all(contour_list) {
            return false;
        }
        true
    }
}
