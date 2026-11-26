#![forbid(unsafe_code)]

use crate::builder::PathBuilder;
use crate::iter::RangeIter;
use crate::types::{ieee_float_divide, point_distance, PathRaw, Verb};
use skia_rs_geometry::geometry::{
    chop_cubic_at_1, chop_cubic_at_half, chop_quad_at, chop_quad_at_half, eval_cubic_at,
    eval_quad_at, eval_quad_tangent_at, Conic,
};
use skia_rs_geometry::{scalar_interp, Matrix, Point, Vector};

pub const K_MAX_T_VALUE: u32 = 0x3FFF_FFFF;
pub const K_LINE_SEG_TYPE: u8 = 0;
pub const K_QUAD_SEG_TYPE: u8 = 1;
pub const K_CUBIC_SEG_TYPE: u8 = 2;
pub const K_CONIC_SEG_TYPE: u8 = 3;

pub const GET_POSITION_MATRIX_FLAG: u32 = 0x01;
pub const GET_TANGENT_MATRIX_FLAG: u32 = 0x02;
pub const GET_POS_AND_TAN_MATRIX_FLAG: u32 = GET_POSITION_MATRIX_FLAG | GET_TANGENT_MATRIX_FLAG;

const CHEAP_DIST_LIMIT: f32 = 0.5;
const K_MAX_RECURSION_DEPTH: i32 = 8;

/// Matches `tValue2Scalar` in `SkContourMeasure.cpp`.
#[inline]
pub const fn t_value_to_scalar(t: u32) -> f32 {
    const K_MAX_T_RECIPROCAL: f32 = 1.0_f32 / (K_MAX_T_VALUE as f32);
    (t as f32) * K_MAX_T_RECIPROCAL
}

/// Binary-compatible with `SkContourMeasure::Segment` on little-endian platforms:
/// ```cpp
/// struct Segment {
///     SkScalar fDistance;
///     unsigned fPtIndex;
///     unsigned fTValue : 30;
///     unsigned fType : 2;
/// };
/// ```
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ContourSegment {
    pub distance: f32,
    pub pt_index: u32,
    pub t_value_and_type: u32,
}

impl ContourSegment {
    #[inline]
    pub const fn new(distance: f32, pt_index: u32, t_value: u32, seg_type: u8) -> Self {
        Self {
            distance,
            pt_index,
            t_value_and_type: (t_value & K_MAX_T_VALUE) | (((seg_type & 0x3) as u32) << 30),
        }
    }

    #[inline]
    pub const fn t_value(self) -> u32 {
        self.t_value_and_type & K_MAX_T_VALUE
    }

    #[inline]
    pub const fn seg_type(self) -> u8 {
        ((self.t_value_and_type >> 30) & 0x3) as u8
    }

    #[inline]
    pub const fn get_scalar_t(self) -> f32 {
        t_value_to_scalar(self.t_value())
    }
}

/// Matches `SkContourMeasure_segTo`.
pub fn contour_measure_seg_to(
    pts: &[Point],
    seg_type: u8,
    start_t: f32,
    stop_t: f32,
    dst: &mut PathBuilder,
) {
    debug_assert!((0.0..=1.0).contains(&start_t));
    debug_assert!((0.0..=1.0).contains(&stop_t));
    debug_assert!(start_t <= stop_t);

    if start_t == stop_t {
        if !dst.is_empty() {
            if let Some(last_pt) = dst.get_last_pt() {
                dst.line_to(last_pt);
            }
        }
        return;
    }

    let mut tmp0 = [Point::ZERO; 7];
    let mut tmp1 = [Point::ZERO; 7];

    match seg_type {
        K_LINE_SEG_TYPE => {
            if stop_t == 1.0 {
                dst.line_to(pts[1]);
            } else {
                dst.line_to(Point::new(
                    scalar_interp(pts[0].x, pts[1].x, stop_t),
                    scalar_interp(pts[0].y, pts[1].y, stop_t),
                ));
            }
        }
        K_QUAD_SEG_TYPE => {
            let src = [pts[0], pts[1], pts[2]];
            let mut qtmp0 = [Point::ZERO; 5];
            let mut qtmp1 = [Point::ZERO; 5];
            if start_t == 0.0 {
                if stop_t == 1.0 {
                    dst.quad_to(pts[1], pts[2]);
                } else {
                    chop_quad_at(&src, &mut qtmp0, stop_t);
                    dst.quad_to(qtmp0[1], qtmp0[2]);
                }
            } else {
                chop_quad_at(&src, &mut qtmp0, start_t);
                if stop_t == 1.0 {
                    dst.quad_to(qtmp0[3], qtmp0[4]);
                } else {
                    let sub = [qtmp0[2], qtmp0[3], qtmp0[4]];
                    chop_quad_at(&sub, &mut qtmp1, (stop_t - start_t) / (1.0 - start_t));
                    dst.quad_to(qtmp1[1], qtmp1[2]);
                }
            }
        }
        K_CONIC_SEG_TYPE => {
            let conic = Conic {
                pts: [pts[0], pts[2], pts[3]],
                w: pts[1].x,
            };
            if start_t == 0.0 {
                if stop_t == 1.0 {
                    dst.conic_to(conic.pts[1], conic.pts[2], conic.w);
                } else {
                    let mut tmp = [Conic::default(); 2];
                    if conic.chop_at(stop_t, &mut tmp) {
                        dst.conic_to(tmp[0].pts[1], tmp[0].pts[2], tmp[0].w);
                    }
                }
            } else if stop_t == 1.0 {
                let mut tmp = [Conic::default(); 2];
                if conic.chop_at(start_t, &mut tmp) {
                    dst.conic_to(tmp[1].pts[1], tmp[1].pts[2], tmp[1].w);
                }
            } else {
                let mut tmp = Conic::default();
                conic.chop_at_range(start_t, stop_t, &mut tmp);
                dst.conic_to(tmp.pts[1], tmp.pts[2], tmp.w);
            }
        }
        K_CUBIC_SEG_TYPE => {
            let src = [pts[0], pts[1], pts[2], pts[3]];
            if start_t == 0.0 {
                if stop_t == 1.0 {
                    dst.cubic_to(pts[1], pts[2], pts[3]);
                } else {
                    chop_cubic_at_1(&src, &mut tmp0, stop_t);
                    dst.cubic_to(tmp0[1], tmp0[2], tmp0[3]);
                }
            } else {
                chop_cubic_at_1(&src, &mut tmp0, start_t);
                if stop_t == 1.0 {
                    dst.cubic_to(tmp0[4], tmp0[5], tmp0[6]);
                } else {
                    let sub = [tmp0[3], tmp0[4], tmp0[5], tmp0[6]];
                    chop_cubic_at_1(&sub, &mut tmp1, (stop_t - start_t) / (1.0 - start_t));
                    dst.cubic_to(tmp1[1], tmp1[2], tmp1[3]);
                }
            }
        }
        _ => {}
    }
}

#[inline]
fn tspan_big_enough(tspan: i32) -> bool {
    (tspan >> 10) != 0
}

#[inline]
fn quad_too_curvy(pts: &[Point; 3], tolerance: f32) -> bool {
    let dx = (pts[1].x / 2.0_f32) - (((pts[0].x + pts[2].x) / 2.0_f32) / 2.0_f32);
    let dy = (pts[1].y / 2.0_f32) - (((pts[0].y + pts[2].y) / 2.0_f32) / 2.0_f32);
    let dist = dx.abs().max(dy.abs());
    dist > tolerance
}

#[inline]
fn conic_too_curvy(first_pt: Point, mid_t_pt: Point, last_pt: Point, tolerance: f32) -> bool {
    let mid_ends = Point::new((first_pt.x + last_pt.x) * 0.5_f32, (first_pt.y + last_pt.y) * 0.5_f32);
    let dx = mid_t_pt.x - mid_ends.x;
    let dy = mid_t_pt.y - mid_ends.y;
    let dist = dx.abs().max(dy.abs());
    dist > tolerance
}

#[inline]
fn cheap_dist_exceeds_limit(pt: Point, x: f32, y: f32, tolerance: f32) -> bool {
    let dist = (x - pt.x).abs().max((y - pt.y).abs());
    dist > tolerance
}

#[inline]
fn cubic_too_curvy(pts: &[Point; 4], tolerance: f32) -> bool {
    cheap_dist_exceeds_limit(
        pts[1],
        scalar_interp(pts[0].x, pts[3].x, 1.0_f32 / 3.0_f32),
        scalar_interp(pts[0].y, pts[3].y, 1.0_f32 / 3.0_f32),
        tolerance,
    ) || cheap_dist_exceeds_limit(
        pts[2],
        scalar_interp(pts[0].x, pts[3].x, 2.0_f32 / 3.0_f32),
        scalar_interp(pts[0].y, pts[3].y, 2.0_f32 / 3.0_f32),
        tolerance,
    )
}

/// Computes position and/or unit tangent for a segment at parameter `t`.
pub fn compute_pos_tan(
    pts: &[Point],
    seg_type: u8,
    t: f32,
    pos: Option<&mut Point>,
    tangent: Option<&mut Vector>,
) {
    match seg_type {
        K_LINE_SEG_TYPE => {
            if let Some(p) = pos {
                *p = Point::new(
                    scalar_interp(pts[0].x, pts[1].x, t),
                    scalar_interp(pts[0].y, pts[1].y, t),
                );
            }
            if let Some(tan) = tangent {
                tan.set_normalize(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
            }
        }
        K_QUAD_SEG_TYPE => {
            let src = [pts[0], pts[1], pts[2]];
            if let Some(p) = pos {
                *p = eval_quad_at(&src, t);
            }
            if let Some(tan) = tangent {
                *tan = eval_quad_tangent_at(&src, t);
                tan.normalize();
            }
        }
        K_CONIC_SEG_TYPE => {
            let conic = Conic {
                pts: [pts[0], pts[2], pts[3]],
                w: pts[1].x,
            };
            if let Some(p) = pos {
                *p = conic.eval_at(t);
            }
            if let Some(tan) = tangent {
                *tan = conic.eval_tangent_at(t);
                tan.normalize();
            }
        }
        K_CUBIC_SEG_TYPE => {
            let src = [pts[0], pts[1], pts[2], pts[3]];
            let mut p_out = Point::ZERO;
            let mut t_out = Vector::ZERO;
            eval_cubic_at(
                &src,
                t,
                if pos.is_some() { Some(&mut p_out) } else { None },
                if tangent.is_some() { Some(&mut t_out) } else { None },
                None,
            );
            if let Some(p) = pos {
                *p = p_out;
            }
            if let Some(tan) = tangent {
                t_out.normalize();
                *tan = t_out;
            }
        }
        _ => {}
    }
}

/// Binary search matching `SkTKSearch<Segment, SkScalar>` in `SkContourMeasure.cpp`.
pub fn search_segment_index(segments: &[ContourSegment], distance: f32) -> Option<(usize, f32)> {
    let count = segments.len();
    if count == 0 {
        return None;
    }

    let mut lo = 0usize;
    let mut hi = count - 1;
    while lo < hi {
        let mid = (hi + lo) >> 1;
        if segments[mid].distance < distance {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }

    // SkTKSearch returns ~hi if distance < base[hi].fDistance, or ~(hi+1) if base[hi].fDistance < distance,
    // and distanceToSegment does `index ^= (index >> 31)`, which maps:
    //   exact match `hi` -> `hi`
    //   `~hi` -> `hi`
    //   `~(hi+1)` -> `hi+1` (note: distance <= length == segments[count-1].distance, so base[hi].fDistance < distance never happens for valid distance)
    let index = if segments[hi].distance < distance {
        (hi + 1).min(count - 1)
    } else {
        hi
    };

    let seg = segments[index];
    let mut start_t = 0.0_f32;
    let mut start_d = 0.0_f32;
    if index > 0 {
        let prev = segments[index - 1];
        start_d = prev.distance;
        if prev.pt_index == seg.pt_index {
            start_t = prev.get_scalar_t();
        }
    }

    let t = start_t + (seg.get_scalar_t() - start_t) * (distance - start_d) / (seg.distance - start_d);
    Some((index, t))
}

#[derive(Clone, Debug)]
pub struct ContourMeasure {
    pub segments: Vec<ContourSegment>,
    pub pts: Vec<Point>,
    pub length: f32,
    pub is_closed: bool,
}

impl ContourMeasure {
    #[inline]
    pub fn length(&self) -> f32 {
        self.length
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    pub fn get_pos_tan(
        &self,
        distance: f32,
        pos: Option<&mut Point>,
        tangent: Option<&mut Vector>,
    ) -> bool {
        contour_get_pos_tan(&self.segments, &self.pts, self.length, distance, pos, tangent)
    }

    pub fn get_matrix(&self, distance: f32, flags: u32) -> Option<Matrix> {
        contour_get_matrix(&self.segments, &self.pts, self.length, distance, flags)
    }

    pub fn get_segment(
        &self,
        start_d: f32,
        stop_d: f32,
        dst: &mut PathBuilder,
        start_with_move_to: bool,
    ) -> bool {
        contour_get_segment(
            &self.segments,
            &self.pts,
            self.length,
            start_d,
            stop_d,
            dst,
            start_with_move_to,
        )
    }
}

/// Stateless slice-based `getPosTan` so FFI can operate directly on C++ `SkContourMeasure` arrays.
pub fn contour_get_pos_tan(
    segments: &[ContourSegment],
    pts: &[Point],
    length: f32,
    mut distance: f32,
    pos: Option<&mut Point>,
    tangent: Option<&mut Vector>,
) -> bool {
    if distance.is_nan() || segments.is_empty() {
        return false;
    }

    if distance < 0.0 {
        distance = 0.0;
    } else if distance > length {
        distance = length;
    }

    let Some((index, t)) = search_segment_index(segments, distance) else {
        return false;
    };
    if t.is_nan() {
        return false;
    }

    let seg = segments[index];
    let pt_idx = seg.pt_index as usize;
    if pt_idx >= pts.len() {
        return false;
    }
    compute_pos_tan(&pts[pt_idx..], seg.seg_type(), t, pos, tangent);
    true
}

/// Stateless slice-based `getMatrix`.
pub fn contour_get_matrix(
    segments: &[ContourSegment],
    pts: &[Point],
    length: f32,
    distance: f32,
    flags: u32,
) -> Option<Matrix> {
    let mut position = Point::ZERO;
    let mut tangent = Vector::ZERO;
    if contour_get_pos_tan(
        segments,
        pts,
        length,
        distance,
        Some(&mut position),
        Some(&mut tangent),
    ) {
        let mut matrix = Matrix::identity();
        if (flags & GET_TANGENT_MATRIX_FLAG) != 0 {
            matrix.set_sin_cos_pivot(tangent.y, tangent.x, 0.0, 0.0);
        }
        if (flags & GET_POSITION_MATRIX_FLAG) != 0 {
            matrix.post_translate(position.x, position.y);
        }
        Some(matrix)
    } else {
        None
    }
}

/// Stateless slice-based `getSegment`.
pub fn contour_get_segment(
    segments: &[ContourSegment],
    pts: &[Point],
    length: f32,
    mut start_d: f32,
    mut stop_d: f32,
    dst: &mut PathBuilder,
    start_with_move_to: bool,
) -> bool {
    if start_d < 0.0 {
        start_d = 0.0;
    }
    if stop_d > length {
        stop_d = length;
    }
    if !(start_d <= stop_d) {
        return false;
    }
    if segments.is_empty() {
        return false;
    }

    let Some((mut seg_idx, mut start_t)) = search_segment_index(segments, start_d) else {
        return false;
    };
    if !start_t.is_finite() {
        return false;
    }
    let Some((stop_idx, stop_t)) = search_segment_index(segments, stop_d) else {
        return false;
    };
    if !stop_t.is_finite() {
        return false;
    }
    debug_assert!(seg_idx <= stop_idx);

    if start_with_move_to {
        let seg = segments[seg_idx];
        let mut p = Point::ZERO;
        compute_pos_tan(&pts[seg.pt_index as usize..], seg.seg_type(), start_t, Some(&mut p), None);
        dst.move_to(p);
    }

    let stop_pt_index = segments[stop_idx].pt_index;
    if segments[seg_idx].pt_index == stop_pt_index {
        let seg = segments[seg_idx];
        contour_measure_seg_to(&pts[seg.pt_index as usize..], seg.seg_type(), start_t, stop_t, dst);
    } else {
        loop {
            let seg = segments[seg_idx];
            contour_measure_seg_to(
                &pts[seg.pt_index as usize..],
                seg.seg_type(),
                start_t,
                1.0,
                dst,
            );
            let cur_pt_idx = seg.pt_index;
            seg_idx += 1;
            while seg_idx < segments.len() && segments[seg_idx].pt_index == cur_pt_idx {
                seg_idx += 1;
            }
            start_t = 0.0;
            if segments[seg_idx].pt_index >= stop_pt_index {
                break;
            }
        }
        let seg = segments[seg_idx];
        contour_measure_seg_to(&pts[seg.pt_index as usize..], seg.seg_type(), 0.0, stop_t, dst);
    }

    true
}

/// Iterator over contours of a path, matching `SkContourMeasureIter`.
#[derive(Clone, Debug)]
pub struct ContourMeasureIter {
    points: Vec<Point>,
    verbs: Vec<Verb>,
    conics: Vec<f32>,
    verb_idx: usize,
    pt_idx: usize,
    conic_idx: usize,
    tolerance: f32,
    force_closed: bool,
    valid: bool,
}

impl ContourMeasureIter {
    pub fn empty() -> Self {
        Self {
            points: Vec::new(),
            verbs: Vec::new(),
            conics: Vec::new(),
            verb_idx: 0,
            pt_idx: 0,
            conic_idx: 0,
            tolerance: CHEAP_DIST_LIMIT,
            force_closed: false,
            valid: false,
        }
    }

    pub fn new(raw: &PathRaw<'_>, force_closed: bool, res_scale: f32) -> Self {
        let is_finite = raw.points.iter().all(|p| p.is_finite());
        if !is_finite {
            return Self::empty();
        }
        Self {
            points: raw.points.to_vec(),
            verbs: raw.verbs.to_vec(),
            conics: raw.conics.to_vec(),
            verb_idx: 0,
            pt_idx: 0,
            conic_idx: 0,
            tolerance: CHEAP_DIST_LIMIT * ieee_float_divide(1.0, res_scale),
            force_closed,
            valid: true,
        }
    }

    pub fn from_slices(
        points: &[Point],
        verbs: &[Verb],
        conics: &[f32],
        force_closed: bool,
        res_scale: f32,
    ) -> Self {
        let is_finite = points.iter().all(|p| p.is_finite());
        if !is_finite {
            return Self::empty();
        }
        Self {
            points: points.to_vec(),
            verbs: verbs.to_vec(),
            conics: conics.to_vec(),
            verb_idx: 0,
            pt_idx: 0,
            conic_idx: 0,
            tolerance: CHEAP_DIST_LIMIT * ieee_float_divide(1.0, res_scale),
            force_closed,
            valid: true,
        }
    }

    pub fn has_next_segments(&self) -> bool {
        self.valid && self.verb_idx < self.verbs.len()
    }

    pub fn next_contour(&mut self) -> Option<ContourMeasure> {
        if !self.valid {
            return None;
        }
        while self.has_next_segments() {
            if let Some(cm) = self.build_segments() {
                return Some(cm);
            }
        }
        None
    }

    fn build_segments(&mut self) -> Option<ContourMeasure> {
        let mut pt_index: i32 = -1;
        let mut distance: f32 = 0.0;
        let mut have_seen_close = self.force_closed;
        let mut have_seen_move_to = false;

        let mut segments = Vec::new();
        let mut f_pts = Vec::new();

        // Advance using RangeIter logic starting from (verb_idx, pt_idx, conic_idx).
        let mut iter = RangeIter::new_at(
            &self.points,
            &self.verbs,
            &self.conics,
            self.verb_idx,
            self.pt_idx,
            self.conic_idx,
        );

        while let Some((verb, pts, w)) = iter.peek() {
            if have_seen_move_to && verb == Verb::Move {
                break;
            }
            iter.advance();
            match verb {
                Verb::Move => {
                    pt_index += 1;
                    f_pts.push(pts[0]);
                    have_seen_move_to = true;
                }
                Verb::Line => {
                    let prev_d = distance;
                    distance = compute_line_seg(
                        pts[0],
                        pts[1],
                        distance,
                        pt_index as u32,
                        &mut segments,
                    );
                    if distance > prev_d {
                        f_pts.push(pts[1]);
                        pt_index += 1;
                    }
                }
                Verb::Quad => {
                    let prev_d = distance;
                    let qpts = [pts[0], pts[1], pts[2]];
                    distance = compute_quad_segs(
                        &qpts,
                        distance,
                        0,
                        K_MAX_T_VALUE as i32,
                        pt_index as u32,
                        0,
                        self.tolerance,
                        &mut segments,
                    );
                    if distance > prev_d {
                        f_pts.push(pts[1]);
                        f_pts.push(pts[2]);
                        pt_index += 2;
                    }
                }
                Verb::Conic => {
                    let conic = Conic {
                        pts: [pts[0], pts[1], pts[2]],
                        w,
                    };
                    let prev_d = distance;
                    distance = compute_conic_segs(
                        &conic,
                        distance,
                        0,
                        conic.pts[0],
                        K_MAX_T_VALUE as i32,
                        conic.pts[2],
                        pt_index as u32,
                        0,
                        self.tolerance,
                        &mut segments,
                    );
                    if distance > prev_d {
                        f_pts.push(Point::new(conic.w, 0.0));
                        f_pts.push(pts[1]);
                        f_pts.push(pts[2]);
                        pt_index += 3;
                    }
                }
                Verb::Cubic => {
                    let prev_d = distance;
                    let cpts = [pts[0], pts[1], pts[2], pts[3]];
                    distance = compute_cubic_segs(
                        &cpts,
                        distance,
                        0,
                        K_MAX_T_VALUE as i32,
                        pt_index as u32,
                        0,
                        self.tolerance,
                        &mut segments,
                    );
                    if distance > prev_d {
                        f_pts.push(pts[1]);
                        f_pts.push(pts[2]);
                        f_pts.push(pts[3]);
                        pt_index += 3;
                    }
                }
                Verb::Close => {
                    have_seen_close = true;
                }
            }
        }

        let (v_idx, p_idx, c_idx) = iter.position();
        self.verb_idx = v_idx;
        self.pt_idx = p_idx;
        self.conic_idx = c_idx;

        if !distance.is_finite() {
            return None;
        }
        if segments.is_empty() {
            return None;
        }

        if have_seen_close {
            let prev_d = distance;
            let first_pt = f_pts[0];
            distance = compute_line_seg(
                f_pts[pt_index as usize],
                first_pt,
                distance,
                pt_index as u32,
                &mut segments,
            );
            if distance > prev_d {
                f_pts.push(first_pt);
            }
        }

        Some(ContourMeasure {
            segments,
            pts: f_pts,
            length: distance,
            is_closed: have_seen_close,
        })
    }
}

fn compute_line_seg(
    p0: Point,
    p1: Point,
    mut distance: f32,
    pt_index: u32,
    segments: &mut Vec<ContourSegment>,
) -> f32 {
    let d = point_distance(p0, p1);
    let prev_d = distance;
    distance += d;
    if distance > prev_d {
        segments.push(ContourSegment::new(
            distance,
            pt_index,
            K_MAX_T_VALUE,
            K_LINE_SEG_TYPE,
        ));
    }
    distance
}

#[allow(clippy::too_many_arguments)]
fn compute_quad_segs(
    pts: &[Point; 3],
    mut distance: f32,
    mint: i32,
    maxt: i32,
    pt_index: u32,
    recursion_depth: i32,
    tolerance: f32,
    segments: &mut Vec<ContourSegment>,
) -> f32 {
    if recursion_depth < K_MAX_RECURSION_DEPTH
        && tspan_big_enough(maxt - mint)
        && quad_too_curvy(pts, tolerance)
    {
        let mut tmp = [Point::ZERO; 5];
        let halft = (mint + maxt) >> 1;
        chop_quad_at_half(pts, &mut tmp);
        let depth = recursion_depth + 1;
        let first = [tmp[0], tmp[1], tmp[2]];
        let second = [tmp[2], tmp[3], tmp[4]];
        distance = compute_quad_segs(&first, distance, mint, halft, pt_index, depth, tolerance, segments);
        distance = compute_quad_segs(&second, distance, halft, maxt, pt_index, depth, tolerance, segments);
    } else {
        let d = point_distance(pts[0], pts[2]);
        let prev_d = distance;
        distance += d;
        if distance > prev_d {
            segments.push(ContourSegment::new(
                distance,
                pt_index,
                maxt as u32,
                K_QUAD_SEG_TYPE,
            ));
        }
    }
    distance
}

#[allow(clippy::too_many_arguments)]
fn compute_conic_segs(
    conic: &Conic,
    mut distance: f32,
    mint: i32,
    min_pt: Point,
    maxt: i32,
    max_pt: Point,
    pt_index: u32,
    recursion_depth: i32,
    tolerance: f32,
    segments: &mut Vec<ContourSegment>,
) -> f32 {
    let halft = (mint + maxt) >> 1;
    let half_pt = conic.eval_at(t_value_to_scalar(halft as u32));
    if !half_pt.is_finite() {
        return distance;
    }
    if recursion_depth < K_MAX_RECURSION_DEPTH
        && tspan_big_enough(maxt - mint)
        && conic_too_curvy(min_pt, half_pt, max_pt, tolerance)
    {
        let depth = recursion_depth + 1;
        distance = compute_conic_segs(
            conic, distance, mint, min_pt, halft, half_pt, pt_index, depth, tolerance, segments,
        );
        distance = compute_conic_segs(
            conic, distance, halft, half_pt, maxt, max_pt, pt_index, depth, tolerance, segments,
        );
    } else {
        let d = point_distance(min_pt, max_pt);
        let prev_d = distance;
        distance += d;
        if distance > prev_d {
            segments.push(ContourSegment::new(
                distance,
                pt_index,
                maxt as u32,
                K_CONIC_SEG_TYPE,
            ));
        }
    }
    distance
}

#[allow(clippy::too_many_arguments)]
fn compute_cubic_segs(
    pts: &[Point; 4],
    mut distance: f32,
    mint: i32,
    maxt: i32,
    pt_index: u32,
    recursion_depth: i32,
    tolerance: f32,
    segments: &mut Vec<ContourSegment>,
) -> f32 {
    if recursion_depth < K_MAX_RECURSION_DEPTH
        && tspan_big_enough(maxt - mint)
        && cubic_too_curvy(pts, tolerance)
    {
        let mut tmp = [Point::ZERO; 7];
        let halft = (mint + maxt) >> 1;
        chop_cubic_at_half(pts, &mut tmp);
        let depth = recursion_depth + 1;
        let first = [tmp[0], tmp[1], tmp[2], tmp[3]];
        let second = [tmp[3], tmp[4], tmp[5], tmp[6]];
        distance = compute_cubic_segs(&first, distance, mint, halft, pt_index, depth, tolerance, segments);
        distance = compute_cubic_segs(&second, distance, halft, maxt, pt_index, depth, tolerance, segments);
    } else {
        let d = point_distance(pts[0], pts[3]);
        let prev_d = distance;
        distance += d;
        if distance > prev_d {
            segments.push(ContourSegment::new(
                distance,
                pt_index,
                maxt as u32,
                K_CUBIC_SEG_TYPE,
            ));
        }
    }
    distance
}

/// Convenience wrapper matching `SkPathMeasure`.
#[derive(Clone, Debug)]
pub struct PathMeasure {
    iter: ContourMeasureIter,
    contour: Option<ContourMeasure>,
}

impl PathMeasure {
    pub fn new(raw: &PathRaw<'_>, force_closed: bool, res_scale: f32) -> Self {
        let mut iter = ContourMeasureIter::new(raw, force_closed, res_scale);
        let contour = iter.next_contour();
        Self { iter, contour }
    }

    pub fn get_length(&self) -> f32 {
        self.contour.as_ref().map_or(0.0, |c| c.length())
    }

    pub fn is_closed(&self) -> bool {
        self.contour.as_ref().is_some_and(|c| c.is_closed())
    }

    pub fn get_pos_tan(
        &self,
        distance: f32,
        pos: Option<&mut Point>,
        tangent: Option<&mut Vector>,
    ) -> bool {
        self.contour
            .as_ref()
            .is_some_and(|c| c.get_pos_tan(distance, pos, tangent))
    }

    pub fn get_matrix(&self, distance: f32, flags: u32) -> Option<Matrix> {
        self.contour.as_ref()?.get_matrix(distance, flags)
    }

    pub fn get_segment(
        &self,
        start_d: f32,
        stop_d: f32,
        dst: &mut PathBuilder,
        start_with_move_to: bool,
    ) -> bool {
        self.contour
            .as_ref()
            .is_some_and(|c| c.get_segment(start_d, stop_d, dst, start_with_move_to))
    }

    pub fn next_contour(&mut self) -> bool {
        self.contour = self.iter.next_contour();
        self.contour.is_some()
    }

    pub fn current_contour(&self) -> Option<&ContourMeasure> {
        self.contour.as_ref()
    }
}
