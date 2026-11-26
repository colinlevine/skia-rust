#![forbid(unsafe_code)]

use crate::builder::PathBuilder;
use crate::iter::LegacyPathIter;
use crate::measure::PathMeasure;
use crate::shapes::is_rect_contour;
use crate::stroke::StrokeRec;
use crate::types::{
    ieee_float_divide, point_distance, rotate_ccw, sk_float_ceil2int, Cap, FillType, Join,
    PathRaw, StrokeStyle, Verb,
};
use skia_rs_geometry::{Point, Rect, Vector, SK_SCALAR_NEARLY_ZERO};

pub const K_MAX_DASH_COUNT: f32 = 1_000_000.0;

#[inline]
fn is_even(x: usize) -> bool {
    (x & 1) == 0
}

#[inline]
fn scalar_mod(x: f32, y: f32) -> f32 {
    x % y
}

fn find_first_interval(intervals: &[f32], mut phase: f32) -> (f32, usize) {
    for (i, &gap) in intervals.iter().enumerate() {
        if phase > gap || (phase == gap && gap != 0.0) {
            phase -= gap;
        } else {
            return (gap - phase, i);
        }
    }
    (intervals[0], 0)
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DashParameters {
    pub initial_dash_length: f32,
    pub initial_dash_index: usize,
    pub interval_length: f32,
    pub adjusted_phase: f32,
}

/// Matches `SkDashPath::ValidDashPath`.
pub fn valid_dash_path(phase: f32, intervals: &[f32]) -> bool {
    if intervals.len() < 2 || !is_even(intervals.len()) {
        return false;
    }
    let mut length = 0.0_f32;
    for &interval in intervals {
        if interval < 0.0 {
            return false;
        }
        length += interval;
    }
    length > 0.0 && phase.is_finite() && length.is_finite()
}

/// Matches `SkDashPath::CalcDashParameters`.
/// If `adjust_phase` is true, adjusts `phase` into `[0, interval_length)` and stores it in `adjusted_phase`.
pub fn calc_dash_parameters(
    mut phase: f32,
    intervals: &[f32],
    adjust_phase: bool,
) -> DashParameters {
    let mut len = 0.0_f32;
    for &interval in intervals {
        len += interval;
    }

    if adjust_phase {
        if phase < 0.0 {
            phase = -phase;
            if phase > len {
                phase = scalar_mod(phase, len);
            }
            phase = len - phase;
            if phase == len {
                phase = 0.0;
            }
        } else if phase >= len {
            phase = scalar_mod(phase, len);
        }
    }

    let (initial_dash_length, initial_dash_index) = find_first_interval(intervals, phase);
    DashParameters {
        initial_dash_length,
        initial_dash_index,
        interval_length: len,
        adjusted_phase: phase,
    }
}

fn outset_for_stroke(rect: Rect, rec: &StrokeRec) -> Rect {
    let mut radius = rec.width / 2.0;
    if radius == 0.0 {
        radius = 1.0; // hairlines
    }
    if rec.join == Join::Miter {
        radius *= rec.miter_limit;
    }
    rect.make_inset(-radius, -radius)
}

fn adjust_zero_length_line(pts: &mut [Point; 2]) {
    debug_assert_eq!(pts[0], pts[1]);
    pts[1].x += 1.001_f32.max(pts[1].x) * SK_SCALAR_NEARLY_ZERO;
}

fn clip_line(
    pts: &mut [Point; 2],
    bounds: Rect,
    interval_length: f32,
    prior_phase: f32,
) -> bool {
    let dxy = pts[1] - pts[0];
    if dxy.x != 0.0 && dxy.y != 0.0 {
        return false;
    }
    let vertical = dxy.y != 0.0;

    let (mut min_xy, mut max_xy) = if vertical {
        (pts[0].y, pts[1].y)
    } else {
        (pts[0].x, pts[1].x)
    };
    let swapped = max_xy < min_xy;
    if swapped {
        core::mem::swap(&mut min_xy, &mut max_xy);
    }

    let (left_top, right_bottom) = if vertical {
        (bounds.top, bounds.bottom)
    } else {
        (bounds.left, bounds.right)
    };
    if max_xy < left_top || min_xy > right_bottom {
        return false;
    }

    if min_xy < left_top {
        min_xy = left_top - scalar_mod(left_top - min_xy, interval_length);
        if !swapped {
            min_xy -= prior_phase;
        }
    }
    if max_xy > right_bottom {
        max_xy = right_bottom + scalar_mod(max_xy - right_bottom, interval_length);
        if swapped {
            max_xy += prior_phase;
        }
    }

    if swapped {
        core::mem::swap(&mut min_xy, &mut max_xy);
    }
    if vertical {
        pts[0].y = min_xy;
        pts[1].y = max_xy;
    } else {
        pts[0].x = min_xy;
        pts[1].x = max_xy;
    }

    if min_xy == max_xy {
        adjust_zero_length_line(pts);
    }
    true
}

fn is_path_line(points: &[Point], verbs: &[Verb]) -> Option<[Point; 2]> {
    if verbs.len() == 2 && verbs[0] == Verb::Move && verbs[1] == Verb::Line && points.len() == 2 {
        Some([points[0], points[1]])
    } else {
        None
    }
}

fn cull_path(
    src: &PathRaw<'_>,
    rec: &StrokeRec,
    cull_rect: Option<Rect>,
    interval_length: f32,
    builder: &mut PathBuilder,
) -> bool {
    let Some(cull) = cull_rect else {
        if let Some(mut pts) = is_path_line(src.points, src.verbs) {
            if pts[0] == pts[1] {
                adjust_zero_length_line(&mut pts);
                builder.move_to(pts[0]);
                builder.line_to(pts[1]);
                return true;
            }
        }
        return false;
    };

    let bounds = outset_for_stroke(cull, rec);

    if let Some(mut pts) = is_path_line(src.points, src.verbs) {
        if clip_line(&mut pts, bounds, interval_length, 0.0) {
            builder.move_to(pts[0]);
            builder.line_to(pts[1]);
            return true;
        }
        return false;
    }

    if is_rect_contour(src.points, src.verbs, src.segment_mask as u32, false).is_some() {
        let mut iter = LegacyPathIter::new(src.points, src.verbs, src.conics, false);
        let first = iter.next_rec();
        debug_assert!(first.as_ref().is_some_and(|r| r.verb == Verb::Move));

        let mut accum = 0.0_f64;
        while let Some(it) = iter.next_rec() {
            if it.verb != Verb::Line {
                break;
            }
            let v = it.points[1] - it.points[0];
            let mut pts = [it.points[0], it.points[1]];
            let prior = (accum % (interval_length as f64)) as f32;
            if clip_line(&mut pts, bounds, interval_length, prior) {
                let maybe_last = builder.get_last_pt();
                if maybe_last != Some(pts[0]) {
                    builder.move_to(pts[0]);
                }
                builder.line_to(pts[1]);
            }
            accum += (v.x + v.y).abs() as f64;
        }
        return !builder.is_empty();
    }

    false
}

struct SpecialLineRec {
    pts: [Point; 2],
    tangent: Vector,
    normal: Vector,
    path_length: f32,
}

impl SpecialLineRec {
    fn init(
        src: &PathRaw<'_>,
        dst: &mut PathBuilder,
        rec: &mut StrokeRec,
        interval_count: usize,
        interval_length: f32,
    ) -> Option<Self> {
        if rec.is_hairline_style() {
            return None;
        }
        let pts = is_path_line(src.points, src.verbs)?;
        if rec.cap != Cap::Butt {
            return None;
        }
        let path_length = point_distance(pts[0], pts[1]);
        let mut tangent = pts[1] - pts[0];
        if tangent.x == 0.0 && tangent.y == 0.0 {
            return None;
        }
        tangent = tangent * ieee_float_divide(1.0, path_length);
        if !tangent.is_finite() {
            return None;
        }
        let normal = rotate_ccw(tangent) * (rec.width / 2.0);

        let pt_count = (path_length * (interval_count as f32) / interval_length).min(K_MAX_DASH_COUNT);
        if pt_count.is_nan() {
            return None;
        }
        let n = (sk_float_ceil2int(pt_count).max(0) as usize) << 2;
        dst.inc_reserve(n, 0, 0);

        rec.set_fill_style();
        Some(Self {
            pts,
            tangent,
            normal,
            path_length,
        })
    }

    fn add_segment(&self, d0: f32, mut d1: f32, path: &mut PathBuilder) {
        if d1 > self.path_length {
            d1 = self.path_length;
        }
        let x0 = self.pts[0].x + self.tangent.x * d0;
        let x1 = self.pts[0].x + self.tangent.x * d1;
        let y0 = self.pts[0].y + self.tangent.y * d0;
        let y1 = self.pts[0].y + self.tangent.y * d1;

        let pts = [
            Point::new(x0 + self.normal.x, y0 + self.normal.y),
            Point::new(x1 + self.normal.x, y1 + self.normal.y),
            Point::new(x1 - self.normal.x, y1 - self.normal.y),
            Point::new(x0 - self.normal.x, y0 - self.normal.y),
        ];
        path.add_polygon(&pts, false);
    }
}

/// Matches `SkDashPath::InternalFilter`.
#[allow(clippy::too_many_arguments)]
pub fn internal_filter(
    dst: &mut PathBuilder,
    src: &PathRaw<'_>,
    rec: &mut StrokeRec,
    cull_rect: Option<Rect>,
    intervals: &[f32],
    initial_dash_length: f32,
    initial_dash_index: usize,
    interval_length: f32,
    start_phase: f32,
    allow_stroke_rec_application: bool,
) -> bool {
    if src.points.is_empty() {
        return true;
    }
    let count = intervals.len();
    debug_assert!(is_even(count));

    let style = rec.get_style();
    if style == StrokeStyle::Fill || style == StrokeStyle::StrokeAndFill {
        return false;
    }

    let mut dash_count = 0.0_f32;
    let mut cull_builder = PathBuilder::new(FillType::Winding);
    let culled_buf = if cull_path(src, rec, cull_rect, interval_length, &mut cull_builder) {
        let is_rect =
            is_rect_contour(src.points, src.verbs, src.segment_mask as u32, false).is_some();
        let is_last_closed = src.verbs.last() == Some(&Verb::Close);
        if is_rect && is_last_closed && is_even(initial_dash_index) {
            let path_length = PathMeasure::new(src, false, rec.res_scale).get_length();
            let mut end_phase = scalar_mod(path_length + start_phase, interval_length);
            let mut index = 0usize;
            while end_phase > intervals[index] {
                end_phase -= intervals[index];
                index += 1;
                if index == count {
                    end_phase = 0.0;
                    break;
                }
            }
            if is_even(index) == (end_phase > 0.0) {
                let mid_point = src.points[0];
                let mut last = src.points.len() - 1;
                while last > 0 && mid_point == src.points[last] {
                    last -= 1;
                }
                let mut next = 1usize;
                while next < last && mid_point == src.points[next] {
                    next += 1;
                }
                let mut v = (mid_point - src.points[last]) * SK_SCALAR_NEARLY_ZERO;
                cull_builder.move_to(mid_point - v);
                cull_builder.line_to(mid_point);
                v = (mid_point - src.points[next]) * SK_SCALAR_NEARLY_ZERO;
                cull_builder.line_to(mid_point - v);
            }
        }
        Some(cull_builder.into_buf())
    } else {
        None
    };

    let active_raw_storage;
    let active_raw: &PathRaw<'_> = if let Some(ref buf) = culled_buf {
        active_raw_storage = buf.as_raw(false).unwrap_or(PathRaw::empty(buf.fill_type));
        &active_raw_storage
    } else {
        src
    };

    let special_line = if allow_stroke_rec_application {
        SpecialLineRec::init(active_raw, dst, rec, count >> 1, interval_length)
    } else {
        None
    };

    let mut meas = PathMeasure::new(active_raw, false, rec.res_scale);
    loop {
        let mut skip_first_segment = meas.is_closed();
        let mut added_segment = false;
        let length = meas.get_length();
        let mut index = initial_dash_index;

        dash_count += length * ((count >> 1) as f32) / interval_length;
        if dash_count > K_MAX_DASH_COUNT {
            dst.reset();
            return false;
        }

        let mut distance = 0.0_f64;
        let mut dlen = initial_dash_length as f64;

        while distance < length as f64 {
            added_segment = false;
            if is_even(index) && !skip_first_segment {
                added_segment = true;
                let d0 = distance as f32;
                let d1 = (distance + dlen) as f32;
                if let Some(ref line_rec) = special_line {
                    line_rec.add_segment(d0, d1, dst);
                } else {
                    meas.get_segment(d0, d1, dst, true);
                }
            }
            distance += dlen;
            skip_first_segment = false;
            index += 1;
            if index == count {
                index = 0;
            }
            dlen = intervals[index] as f64;
        }

        if meas.is_closed() && is_even(initial_dash_index) && initial_dash_length >= 0.0 {
            meas.get_segment(0.0, initial_dash_length, dst, !added_segment);
        }

        if !meas.next_contour() {
            break;
        }
    }

    true
}

/// Matches `SkDashPath::FilterDashPath`.
pub fn filter_dash_path(
    dst: &mut PathBuilder,
    src: &PathRaw<'_>,
    rec: &mut StrokeRec,
    cull_rect: Option<Rect>,
    phase: f32,
    intervals: &[f32],
) -> bool {
    if !valid_dash_path(phase, intervals) {
        return false;
    }
    let params = calc_dash_parameters(phase, intervals, false);
    internal_filter(
        dst,
        src,
        rec,
        cull_rect,
        intervals,
        params.initial_dash_length,
        params.initial_dash_index,
        params.interval_length,
        phase,
        true,
    )
}
