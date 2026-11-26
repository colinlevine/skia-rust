#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkScan_Hairline.cpp`.

use crate::antihair::anti_hair_line_rgn;
use crate::blitter::{Blitter, RegionClip};
use crate::edge_builder::conic_to_quads;
use crate::fixed::{
    abs32, clz_u32, fdot6_round, fdot6_to_fixed, fixed_div, float_ceil2int, float_floor2int,
    float_to_fdot6, int_to_fdot6, irect_contains, irect_make_sorted, SK_FDOT6_ONE,
};
use crate::scan_rect::fill_rect;
use skia_rs_geometry::geometry::chop_cubic_at_max_curvature;
use skia_rs_geometry::line_clipper::intersect_line;
use skia_rs_geometry::{IRect, Point, Rect, Vector, SK_SCALAR_PI};
use skia_rs_path::iter::PathIter;
use skia_rs_path::{
    is_cubic_degenerate, is_line_degenerate, is_quad_degenerate, PathRaw, Verb,
};

const MAX_CUBIC_SUBDIVIDE_LEVEL: usize = 9;
const MAX_QUAD_SUBDIVIDE_LEVEL: usize = 5;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HairCapStyle {
    Butt = 0,
    Round = 1,
    Square = 2,
}

impl HairCapStyle {
    #[inline]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Round,
            2 => Self::Square,
            _ => Self::Butt,
        }
    }
}

/// Matches `SkScan::HairLineRgn`.
///
/// `orig_blitter` is used when a line segment is unclipped; `clipped_blitter` (when provided)
/// is used when `clip` is `Some` and the segment isn't completely inside a rectangular clip.
pub fn hair_line_rgn<BOrig, BClip, R>(
    src: &[Point],
    clip: Option<&R>,
    orig_blitter: &mut BOrig,
    mut clipped_blitter: Option<&mut BClip>,
) where
    BOrig: Blitter + ?Sized,
    BClip: Blitter + ?Sized,
    R: RegionClip + ?Sized,
{
    if src.len() < 2 {
        return;
    }

    let max = 32767.0_f32;
    let fixed_bounds = Rect::make_ltrb(-max, -max, max, max);
    let clip_bounds = clip.map(|c| Rect::from_irect(c.bounds()));

    for i in 0..(src.len() - 1) {
        let mut pts = [Point::ZERO; 2];
        if !intersect_line(&[src[i], src[i + 1]], fixed_bounds, &mut pts) {
            continue;
        }
        if let Some(cb) = clip_bounds {
            let cur = pts;
            if !intersect_line(&cur, cb, &mut pts) {
                continue;
            }
        }

        let mut x0 = float_to_fdot6(pts[0].x);
        let mut y0 = float_to_fdot6(pts[0].y);
        let mut x1 = float_to_fdot6(pts[1].x);
        let mut y1 = float_to_fdot6(pts[1].y);

        let mut use_clipped = false;
        if let Some(c) = clip {
            let bounds = c.bounds();
            let clip_r = IRect::make_ltrb(
                int_to_fdot6(bounds.left),
                int_to_fdot6(bounds.top),
                int_to_fdot6(bounds.right),
                int_to_fdot6(bounds.bottom),
            );
            let mut pts_r = irect_make_sorted(IRect::make_ltrb(x0, y0, x1, y1));
            pts_r.right = pts_r.right.wrapping_add(SK_FDOT6_ONE);
            pts_r.bottom = pts_r.bottom.wrapping_add(SK_FDOT6_ONE);

            if !IRect::intersects(pts_r, clip_r) {
                continue;
            }
            if !c.is_rect() || !irect_contains(clip_r, pts_r) {
                use_clipped = true;
            }
        }

        let dx = x1.wrapping_sub(x0);
        let dy = y1.wrapping_sub(y0);

        if abs32(dx) > abs32(dy) {
            if x0 > x1 {
                core::mem::swap(&mut x0, &mut x1);
                core::mem::swap(&mut y0, &mut y1);
            }
            let ix0 = fdot6_round(x0);
            let ix1 = fdot6_round(x1);
            if ix0 == ix1 {
                continue;
            }
            let slope = fixed_div(dy, dx);
            let start_y = fdot6_to_fixed(y0)
                .wrapping_add(slope.wrapping_mul(32i32.wrapping_sub(x0) & 63) >> 6);
            if use_clipped {
                if let Some(ref mut cb) = clipped_blitter {
                    cb.blit_hori_line(ix0, ix1, start_y, slope);
                } else {
                    orig_blitter.blit_hori_line(ix0, ix1, start_y, slope);
                }
            } else {
                orig_blitter.blit_hori_line(ix0, ix1, start_y, slope);
            }
        } else {
            if y0 > y1 {
                core::mem::swap(&mut x0, &mut x1);
                core::mem::swap(&mut y0, &mut y1);
            }
            let iy0 = fdot6_round(y0);
            let iy1 = fdot6_round(y1);
            if iy0 == iy1 {
                continue;
            }
            let slope = fixed_div(dx, dy);
            let start_x = fdot6_to_fixed(x0)
                .wrapping_add(slope.wrapping_mul(32i32.wrapping_sub(y0) & 63) >> 6);
            if use_clipped {
                if let Some(ref mut cb) = clipped_blitter {
                    cb.blit_vert_line(iy0, iy1, start_x, slope);
                } else {
                    orig_blitter.blit_vert_line(iy0, iy1, start_x, slope);
                }
            } else {
                orig_blitter.blit_vert_line(iy0, iy1, start_x, slope);
            }
        }
    }
}

/// Exact match for Skia's `Sk32_sat_add` (pins to `[-SK_MaxS32, SK_MaxS32]`).
#[inline]
fn sk32_sat_add(a: i32, b: i32) -> i32 {
    let sum = (a as i64) + (b as i64);
    sum.clamp(-(i32::MAX as i64), i32::MAX as i64) as i32
}

/// Exact match for Skia's `Sk32_sat_sub` (pins to `[-SK_MaxS32, SK_MaxS32]`).
#[inline]
fn sk32_sat_sub(a: i32, b: i32) -> i32 {
    let diff = (a as i64) - (b as i64);
    diff.clamp(-(i32::MAX as i64), i32::MAX as i64) as i32
}

/// Computes the clipped integer bounds for `SkScan::HairRect`.
/// Returns `None` if empty after intersecting with `clip_bounds.make_outset(1, 1)`.
pub fn hair_rect_bounds(rect: Rect, clip_bounds: IRect) -> Option<IRect> {
    let r = IRect::make_ltrb(
        float_floor2int(rect.left),
        float_floor2int(rect.top),
        float_floor2int(rect.right + 1.0),
        float_floor2int(rect.bottom + 1.0),
    );
    let outset = IRect::make_ltrb(
        sk32_sat_sub(clip_bounds.left, 1),
        sk32_sat_sub(clip_bounds.top, 1),
        sk32_sat_add(clip_bounds.right, 1),
        sk32_sat_add(clip_bounds.bottom, 1),
    );
    let mut sect = IRect::EMPTY;
    if !sect.intersect_pair(r, outset) {
        return None;
    }
    Some(sect)
}

/// Emits the blits for `SkScan::HairRect` once `blitter` is clipped to `r`.
pub fn hair_rect_emit<B: Blitter + ?Sized>(r: IRect, blitter: &mut B) {
    let width = r.width();
    let height = r.height();
    if (width | height) == 0 {
        return;
    }
    if width <= 2 || height <= 2 {
        blitter.blit_rect(r.left, r.top, width, height);
        return;
    }
    blitter.blit_h(r.left, r.top, width);
    blitter.blit_rect(r.left, r.top + 1, 1, height - 2);
    blitter.blit_rect(r.right - 1, r.top + 1, 1, height - 2);
    blitter.blit_h(r.left, r.bottom - 1, width);
}

#[inline]
fn compute_int_quad_dist(pts: &[Point; 3]) -> u32 {
    let dx = (((pts[0].x + pts[2].x) / 2.0_f32) - pts[1].x).abs();
    let dy = (((pts[0].y + pts[2].y) / 2.0_f32) - pts[1].y).abs();
    let idx = float_ceil2int(dx) as u32;
    let idy = float_ceil2int(dy) as u32;
    if idx > idy {
        idx.wrapping_add(idy >> 1)
    } else {
        idy.wrapping_add(idx >> 1)
    }
}

#[inline]
fn compute_quad_level(pts: &[Point; 3]) -> usize {
    let d = compute_int_quad_dist(pts);
    let level = ((33 - clz_u32(d)) >> 1).max(0) as usize;
    level.min(MAX_QUAD_SUBDIVIDE_LEVEL)
}

#[inline]
fn compute_nocheck_quad_bounds(pts: &[Point; 3]) -> Rect {
    let min_x = pts[0].x.min(pts[1].x).min(pts[2].x);
    let min_y = pts[0].y.min(pts[1].y).min(pts[2].y);
    let max_x = pts[0].x.max(pts[1].x).max(pts[2].x);
    let max_y = pts[0].y.max(pts[1].y).max(pts[2].y);
    Rect::make_ltrb(min_x, min_y, max_x, max_y)
}

#[inline]
fn compute_nocheck_cubic_bounds(pts: &[Point; 4]) -> Rect {
    let min_x = pts[0].x.min(pts[1].x).min(pts[2].x).min(pts[3].x);
    let min_y = pts[0].y.min(pts[1].y).min(pts[2].y).min(pts[3].y);
    let max_x = pts[0].x.max(pts[1].x).max(pts[2].x).max(pts[3].x);
    let max_y = pts[0].y.max(pts[1].y).max(pts[2].y).max(pts[3].y);
    Rect::make_ltrb(min_x, min_y, max_x, max_y)
}

#[inline]
fn is_inverted(r: Rect) -> bool {
    r.left > r.right || r.top > r.bottom
}

#[inline]
fn geometric_overlap(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

#[inline]
fn geometric_contains(outer: Rect, inner: Rect) -> bool {
    inner.right <= outer.right
        && inner.left >= outer.left
        && inner.bottom <= outer.bottom
        && inner.top >= outer.top
}

#[inline]
fn compute_cubic_segs(pts: &[Point; 4]) -> usize {
    let one_third = 1.0_f32 / 3.0_f32;
    let two_third = 2.0_f32 / 3.0_f32;

    let p13 = Point::new(
        one_third * pts[3].x + two_third * pts[0].x,
        one_third * pts[3].y + two_third * pts[0].y,
    );
    let p23 = Point::new(
        one_third * pts[0].x + two_third * pts[3].x,
        one_third * pts[0].y + two_third * pts[3].y,
    );

    let dx = (pts[1].x - p13.x).abs().max((pts[2].x - p23.x).abs());
    let dy = (pts[1].y - p13.y).abs().max((pts[2].y - p23.y).abs());
    let diff = dx.max(dy);

    let mut tol = 1.0_f32 / 8.0_f32;
    for i in 0..MAX_CUBIC_SUBDIVIDE_LEVEL {
        if diff < tol {
            return 1usize << i;
        }
        tol *= 4.0_f32;
    }
    1usize << MAX_CUBIC_SUBDIVIDE_LEVEL
}

#[inline]
fn lt_90(p0: Point, pivot: Point, p2: Point) -> bool {
    (p0 - pivot).dot(p2 - pivot) >= 0.0
}

#[inline]
fn quick_cubic_niceness_check(pts: &[Point; 4]) -> bool {
    lt_90(pts[1], pts[0], pts[3])
        && lt_90(pts[2], pts[0], pts[3])
        && lt_90(pts[1], pts[3], pts[0])
        && lt_90(pts[2], pts[3], pts[0])
}

fn extend_pts(
    cap_style: HairCapStyle,
    prev_verb: Option<Verb>,
    next_verb: Option<Verb>,
    pts: &mut [Point],
) {
    debug_assert!(cap_style == HairCapStyle::Square || cap_style == HairCapStyle::Round);
    let cap_outset = if cap_style == HairCapStyle::Square {
        0.5_f32
    } else {
        SK_SCALAR_PI / 8.0_f32
    };

    if prev_verb == Some(Verb::Move) {
        let mut ctrl_idx = 0usize;
        let mut controls = pts.len() - 1;
        let mut tangent: Vector;
        loop {
            ctrl_idx += 1;
            tangent = pts[0] - pts[ctrl_idx];
            if !tangent.is_zero() {
                break;
            }
            controls -= 1;
            if controls == 0 {
                break;
            }
        }
        if tangent.is_zero() {
            tangent = Vector::new(1.0, 0.0);
            controls = pts.len() - 1;
        } else {
            tangent.normalize();
        }
        let mut first_idx = 0usize;
        loop {
            pts[first_idx].x += tangent.x * cap_outset;
            pts[first_idx].y += tangent.y * cap_outset;
            first_idx += 1;
            controls += 1;
            if controls >= pts.len() {
                break;
            }
        }
    }

    if matches!(next_verb, None | Some(Verb::Move) | Some(Verb::Close)) {
        let last_start = pts.len() - 1;
        let mut ctrl_idx = last_start;
        let mut controls = pts.len() - 1;
        let mut tangent: Vector;
        loop {
            ctrl_idx -= 1;
            tangent = pts[last_start] - pts[ctrl_idx];
            if !tangent.is_zero() {
                break;
            }
            controls -= 1;
            if controls == 0 {
                break;
            }
        }
        if tangent.is_zero() {
            tangent = Vector::new(-1.0, 0.0);
            controls = pts.len() - 1;
        } else {
            tangent.normalize();
        }
        let mut last_idx = last_start;
        loop {
            pts[last_idx].x += tangent.x * cap_outset;
            pts[last_idx].y += tangent.y * cap_outset;
            controls += 1;
            if controls >= pts.len() {
                break;
            }
            last_idx -= 1;
        }
    }
}

fn is_next_contour_closed(mut scanner: PathIter<'_>) -> bool {
    let rec = match scanner.next_rec() {
        Some(r) => r,
        None => return false,
    };
    if rec.verb == Verb::Close {
        return true;
    }
    if rec.verb == Verb::Move {
        return false;
    }
    while let Some(next_verb) = scanner.peek_next_verb() {
        if next_verb == Verb::Move {
            return false;
        }
        if next_verb == Verb::Close {
            return true;
        }
        scanner.next_rec();
    }
    false
}

struct HairDrawCtx<'a, BOrig: Blitter + ?Sized, BClip: Blitter + ?Sized, R: RegionClip + ?Sized> {
    clip: Option<&'a R>,
    inset_clip: Option<Rect>,
    outset_clip: Option<Rect>,
    orig_blitter: &'a mut BOrig,
    clipped_blitter: Option<&'a mut BClip>,
    is_anti: bool,
}

impl<'a, BOrig: Blitter + ?Sized, BClip: Blitter + ?Sized, R: RegionClip + ?Sized>
    HairDrawCtx<'a, BOrig, BClip, R>
{
    fn emit_line(&mut self, pts: &[Point], use_clip: bool) {
        let clip_opt = if use_clip { self.clip } else { None };
        if self.is_anti {
            anti_hair_line_rgn(pts, clip_opt, self.orig_blitter);
        } else {
            hair_line_rgn(
                pts,
                clip_opt,
                self.orig_blitter,
                self.clipped_blitter.as_deref_mut(),
            );
        }
    }

    fn hair_quad_subdivide(&mut self, pts: &[Point; 3], use_clip: bool, level: usize) {
        debug_assert!(level <= MAX_QUAD_SUBDIVIDE_LEVEL);
        let c = pts[0];
        let p1 = pts[1];
        let p2 = pts[2];
        let b = Point::new(2.0 * (p1.x - c.x), 2.0 * (p1.y - c.y));
        let a = Point::new(p2.x - 2.0 * p1.x + c.x, p2.y - 2.0 * p1.y + c.y);

        let lines = 1usize << level;
        let dt = 1.0_f32 / (lines as f32);
        let mut t = 0.0_f32;
        let mut tmp = [Point::ZERO; (1 << MAX_QUAD_SUBDIVIDE_LEVEL) + 1];
        tmp[0] = pts[0];
        let mut all_ok = true;
        for i in 1..lines {
            t += dt;
            let px = (a.x * t + b.x) * t + c.x;
            let py = (a.y * t + b.y) * t + c.y;
            all_ok &= px.is_finite() && py.is_finite();
            tmp[i] = Point::new(px, py);
        }
        if all_ok {
            tmp[lines] = pts[2];
            self.emit_line(&tmp[..=lines], use_clip);
        }
    }

    fn hairquad(&mut self, pts: &[Point; 3], level: usize) {
        let mut use_clip = self.clip.is_some();
        if let Some(inset) = self.inset_clip {
            let outset = self.outset_clip.unwrap();
            let bounds = compute_nocheck_quad_bounds(pts);
            if !geometric_overlap(outset, bounds) {
                return;
            } else if geometric_contains(inset, bounds) {
                use_clip = false;
            }
        }
        self.hair_quad_subdivide(pts, use_clip, level);
    }

    fn hairconic(&mut self, pts: &[Point; 3], weight: f32, quad_buf: &mut Vec<Point>) {
        let count = conic_to_quads(pts, weight, 0.25, quad_buf);
        for i in 0..count {
            let q = [quad_buf[i * 2], quad_buf[i * 2 + 1], quad_buf[i * 2 + 2]];
            let level = compute_quad_level(&q);
            self.hairquad(&q, level);
        }
    }

    fn hair_cubic_subdivide(&mut self, pts: &[Point; 4], use_clip: bool) {
        let lines = compute_cubic_segs(pts);
        if lines == 1 {
            self.emit_line(&[pts[0], pts[3]], use_clip);
            return;
        }

        let p0 = pts[0];
        let p1 = pts[1];
        let p2 = pts[2];
        let p3 = pts[3];
        let a = Point::new(
            p3.x + 3.0 * (p1.x - p2.x) - p0.x,
            p3.y + 3.0 * (p1.y - p2.y) - p0.y,
        );
        let b = Point::new(
            3.0 * (p2.x - 2.0 * p1.x + p0.x),
            3.0 * (p2.y - 2.0 * p1.y + p0.y),
        );
        let c = Point::new(3.0 * (p1.x - p0.x), 3.0 * (p1.y - p0.y));
        let d = p0;

        let dt = 1.0_f32 / (lines as f32);
        let mut t = 0.0_f32;
        let mut tmp = [Point::ZERO; (1 << MAX_CUBIC_SUBDIVIDE_LEVEL) + 1];
        tmp[0] = pts[0];
        let mut all_ok = true;
        for i in 1..lines {
            t += dt;
            let px = ((a.x * t + b.x) * t + c.x) * t + d.x;
            let py = ((a.y * t + b.y) * t + c.y) * t + d.y;
            all_ok &= px.is_finite() && py.is_finite();
            tmp[i] = Point::new(px, py);
        }
        if all_ok {
            tmp[lines] = pts[3];
            self.emit_line(&tmp[..=lines], use_clip);
        }
    }

    fn haircubic(&mut self, pts: &[Point; 4]) {
        let mut use_clip = self.clip.is_some();
        if let Some(inset) = self.inset_clip {
            let outset = self.outset_clip.unwrap();
            let bounds = compute_nocheck_cubic_bounds(pts);
            if !geometric_overlap(outset, bounds) {
                return;
            } else if geometric_contains(inset, bounds) {
                use_clip = false;
            }
        }

        if quick_cubic_niceness_check(pts) {
            self.hair_cubic_subdivide(pts, use_clip);
        } else {
            let mut tmp = [Point::ZERO; 13];
            let mut t_values = [0.0_f32; 3];
            let count = chop_cubic_at_max_curvature(pts, Some(&mut tmp), Some(&mut t_values));
            for i in 0..(count as usize) {
                let seg = [tmp[i * 3], tmp[i * 3 + 1], tmp[i * 3 + 2], tmp[i * 3 + 3]];
                self.hair_cubic_subdivide(&seg, use_clip);
            }
        }
    }
}

/// Computes the outset bounds for quickReject/quickContains in `hair_path`.
pub fn hair_path_bounds(raw_bounds: Rect, cap_style: HairCapStyle) -> IRect {
    let cap_out = if cap_style == HairCapStyle::Butt { 1 } else { 2 };
    let r = raw_bounds.round_out();
    IRect::make_ltrb(
        sk32_sat_sub(r.left, cap_out),
        sk32_sat_sub(r.top, cap_out),
        sk32_sat_add(r.right, cap_out),
        sk32_sat_add(r.bottom, cap_out),
    )
}

/// Safe Rust implementation of `hair_path<capStyle>` from `SkScan_Hairline.cpp`.
///
/// Caller has already done `quickReject` and (if `!quickContains`) set up `clip` and `clipped_blitter`.
/// `clip_is_rect` corresponds to `rclip.isRect()`.
pub fn hair_path_clipped<BOrig, BClip, R>(
    raw: &PathRaw<'_>,
    clip: Option<&R>,
    clip_is_rect: bool,
    orig_blitter: &mut BOrig,
    clipped_blitter: Option<&mut BClip>,
    cap_style: HairCapStyle,
    is_anti: bool,
) where
    BOrig: Blitter + ?Sized,
    BClip: Blitter + ?Sized,
    R: RegionClip + ?Sized,
{
    if raw.is_empty() {
        return;
    }

    let (inset_clip, outset_clip) = if let Some(c) = clip {
        let b = Rect::from_irect(c.bounds());
        let outset = Rect::make_ltrb(b.left - 1.0, b.top - 1.0, b.right + 1.0, b.bottom + 1.0);
        let mut inset = Rect::make_ltrb(b.left + 1.0, b.top + 1.0, b.right - 1.0, b.bottom - 1.0);
        if is_inverted(inset) {
            inset = Rect::EMPTY;
        }
        (if clip_is_rect { Some(inset) } else { None }, Some(outset))
    } else {
        (None, None)
    };

    let mut ctx = HairDrawCtx {
        clip,
        inset_clip,
        outset_clip,
        orig_blitter,
        clipped_blitter,
        is_anti,
    };

    let mut pts = [Point::ZERO; 4];
    let mut first_pt = Point::ZERO;
    let mut last_pt = Point::ZERO;
    let mut quad_buf = Vec::new();
    let mut is_closed = false;
    let is_butt_cap = cap_style == HairCapStyle::Butt;
    let mut prev_verb: Option<Verb> = None;

    let mut iter = PathIter::new(raw.points, raw.verbs, raw.conics);
    while let Some(rec) = iter.next_rec() {
        let src_pts = rec.pts();
        let verb = rec.verb;
        let next_verb = iter.peek_next_verb();

        match verb {
            Verb::Move => {
                first_pt = src_pts[0];
                last_pt = src_pts[0];
                is_closed = !is_butt_cap && is_next_contour_closed(iter.clone());
            }
            Verb::Line => {
                pts[0] = src_pts[0];
                pts[1] = src_pts[1];
                if !is_butt_cap && (!is_closed || is_line_degenerate(pts[0], pts[1], true)) {
                    extend_pts(cap_style, prev_verb, next_verb, &mut pts[..2]);
                }
                ctx.emit_line(&pts[..2], true);
                last_pt = pts[1];
            }
            Verb::Quad => {
                pts[0] = src_pts[0];
                pts[1] = src_pts[1];
                pts[2] = src_pts[2];
                if !is_butt_cap
                    && (!is_closed || is_quad_degenerate(pts[0], pts[1], pts[2], true))
                {
                    extend_pts(cap_style, prev_verb, next_verb, &mut pts[..3]);
                }
                let q = [pts[0], pts[1], pts[2]];
                let level = compute_quad_level(&q);
                ctx.hairquad(&q, level);
                last_pt = pts[2];
            }
            Verb::Conic => {
                pts[0] = src_pts[0];
                pts[1] = src_pts[1];
                pts[2] = src_pts[2];
                if !is_butt_cap
                    && (!is_closed || is_quad_degenerate(pts[0], pts[1], pts[2], true))
                {
                    extend_pts(cap_style, prev_verb, next_verb, &mut pts[..3]);
                }
                let q = [pts[0], pts[1], pts[2]];
                ctx.hairconic(&q, rec.conic_weight, &mut quad_buf);
                last_pt = pts[2];
            }
            Verb::Cubic => {
                pts[0] = src_pts[0];
                pts[1] = src_pts[1];
                pts[2] = src_pts[2];
                pts[3] = src_pts[3];
                if !is_butt_cap
                    && (!is_closed || is_cubic_degenerate(pts[0], pts[1], pts[2], pts[3], true))
                {
                    extend_pts(cap_style, prev_verb, next_verb, &mut pts[..4]);
                }
                ctx.haircubic(&pts);
                last_pt = pts[3];
            }
            Verb::Close => {
                pts[0] = last_pt;
                pts[1] = first_pt;
                if !is_butt_cap && prev_verb == Some(Verb::Move) {
                    extend_pts(cap_style, prev_verb, next_verb, &mut pts[..2]);
                }
                ctx.emit_line(&pts[..2], true);
            }
        }

        if !is_butt_cap {
            if prev_verb == Some(Verb::Move)
                && matches!(verb, Verb::Line | Verb::Quad | Verb::Conic | Verb::Cubic)
            {
                first_pt = pts[0];
            }
            prev_verb = Some(verb);
        }
    }
}

/// Matches `SkScan::FrameRect`.
pub fn frame_rect<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    r: Rect,
    stroke_size: Point,
    clip: Option<&R>,
    blitter: &mut B,
) {
    if stroke_size.x < 0.0 || stroke_size.y < 0.0 {
        return;
    }

    let dx = stroke_size.x;
    let dy = stroke_size.y;
    let rx = dx / 2.0_f32;
    let ry = dy / 2.0_f32;

    let mut outer = Rect::make_ltrb(r.left - rx, r.top - ry, r.right + rx, r.bottom + ry);

    if r.width() <= dx || r.height() <= dy {
        if r.width() == 0.0 {
            outer.top = r.top;
            outer.bottom = r.bottom;
        }
        if r.height() == 0.0 {
            outer.left = r.left;
            outer.right = r.right;
        }
        fill_rect(outer, clip, blitter);
        return;
    }

    let mut tmp = Rect::make_ltrb(outer.left, outer.top, outer.right, outer.top + dy);
    fill_rect(tmp, clip, blitter);
    tmp.top = outer.bottom - dy;
    tmp.bottom = outer.bottom;
    fill_rect(tmp, clip, blitter);

    tmp = Rect::make_ltrb(outer.left, outer.top + dy, outer.left + dx, outer.bottom - dy);
    fill_rect(tmp, clip, blitter);
    tmp.left = outer.right - dx;
    tmp.right = outer.right;
    fill_rect(tmp, clip, blitter);
}
