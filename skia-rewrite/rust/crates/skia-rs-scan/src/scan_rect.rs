#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkScan.cpp` and shared rect/bounds utilities
//! from `SkScan_Path.cpp` and `SkScan_AntiPath.cpp`.

use crate::blitter::{Blitter, RegionClip};
use crate::fixed::{
    double_saturate2int, irect_contains, left_shift_i32, xrect_round, XRect, SK_FDOT6_ONE,
    SK_MAX_S32, SK_SUPERSAMPLE_SHIFT,
};
use skia_rs_geometry::{IRect, Rect};

#[inline]
fn blitrect<B: Blitter + ?Sized>(blitter: &mut B, r: IRect) {
    blitter.blit_rect(r.left, r.top, r.width(), r.height());
}

/// Matches `SkScan::FillIRect(const SkIRect&, const SkRegion*, SkBlitter*)`.
pub fn fill_irect<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    r: IRect,
    clip: Option<&R>,
    blitter: &mut B,
) {
    if r.is_empty() {
        return;
    }
    if let Some(clip) = clip {
        if clip.is_rect() {
            let clip_bounds = clip.bounds();
            if irect_contains(clip_bounds, r) {
                blitrect(blitter, r);
            } else {
                let mut rr = IRect::EMPTY;
                if rr.intersect_pair(r, clip_bounds) {
                    blitrect(blitter, rr);
                }
            }
        } else {
            clip.for_each_clip_rect(r, &mut |rr| {
                blitrect(blitter, rr);
            });
        }
    } else {
        blitrect(blitter, r);
    }
}

/// Matches `SkScan::FillXRect(const SkXRect&, const SkRegion*, SkBlitter*)`.
pub fn fill_xrect<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    xr: XRect,
    clip: Option<&R>,
    blitter: &mut B,
) {
    let r = xrect_round(xr);
    fill_irect(r, clip, blitter);
}

/// Matches `SkScan::FillRect(const SkRect&, const SkRegion*, SkBlitter*)`.
pub fn fill_rect<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    r: Rect,
    clip: Option<&R>,
    blitter: &mut B,
) {
    let ir = r.round();
    fill_irect(ir, clip, blitter);
}

/// Matches `clip_to_limit` check in `SkScan_Path.cpp` and `SkScan::PathRequiresTiling`.
pub const fn clip_limit_rect() -> IRect {
    let limit = 32767 >> 1;
    IRect::make_ltrb(-limit, -limit, limit, limit)
}

/// Matches `SkScan::PathRequiresTiling(const SkIRect& bounds)`.
pub fn path_requires_tiling(bounds: IRect) -> bool {
    !irect_contains(clip_limit_rect(), bounds)
}

/// Matches `SkRectPriv::MakeILarge` / `MakeLargeS32`.
pub const fn make_large_s32_rect() -> Rect {
    let large = (1i32 << 29) as f32;
    Rect::make_ltrb(-large, -large, large, large)
}

const CONSERVATIVE_ROUND_BIAS: f64 = 0.5 + 1.5 / (SK_FDOT6_ONE as f64);

#[inline]
fn round_down_to_int(x: f32) -> i32 {
    let xx = (x as f64) - CONSERVATIVE_ROUND_BIAS;
    double_saturate2int(xx.ceil())
}

#[inline]
fn round_up_to_int(x: f32) -> i32 {
    let xx = (x as f64) + CONSERVATIVE_ROUND_BIAS;
    double_saturate2int(xx.floor())
}

/// Matches `conservative_round_to_int` in `SkScan_Path.cpp`.
#[inline]
pub fn conservative_round_to_int(src: Rect) -> IRect {
    IRect::make_ltrb(
        round_down_to_int(src.left),
        round_down_to_int(src.top),
        round_up_to_int(src.right),
        round_up_to_int(src.bottom),
    )
}

/// Computes `(ir, ir_pre_clipped)` for `SkScan::FillPath`.
pub fn compute_fill_path_bounds(raw_bounds: Rect) -> (IRect, bool) {
    let large = make_large_s32_rect();
    let mut bounds = raw_bounds;
    let mut ir_pre_clipped = false;
    if !large.contains_rect(bounds) {
        let mut sect = Rect::EMPTY;
        if sect.intersect_pair(bounds, large) {
            bounds = sect;
        } else {
            bounds = Rect::EMPTY;
        }
        ir_pre_clipped = true;
    }
    (conservative_round_to_int(bounds), ir_pre_clipped)
}

/// Matches `safeRoundOut` in `SkScan_AntiPath.cpp`.
pub fn safe_round_out(src: Rect) -> IRect {
    let mut dst = src.round_out();
    let limit = SK_MAX_S32 >> SK_SUPERSAMPLE_SHIFT;
    let lim_rect = IRect::make_ltrb(-limit, -limit, limit, limit);
    let mut sect = IRect::EMPTY;
    if sect.intersect_pair(dst, lim_rect) {
        dst = sect;
    }
    dst
}

#[inline]
pub const fn overflows_short_shift(value: i32, shift: i32) -> i32 {
    let s = (16 + shift) as u32;
    (left_shift_i32(value, s) >> s).wrapping_sub(value)
}

/// Matches `rect_overflows_short_shift` in `SkScan_AntiPath.cpp`.
#[inline]
pub const fn rect_overflows_short_shift(rect: IRect, shift: i32) -> bool {
    (overflows_short_shift(rect.left, shift)
        | overflows_short_shift(rect.right, shift)
        | overflows_short_shift(rect.top, shift)
        | overflows_short_shift(rect.bottom, shift))
        != 0
}

/// Classification result for `SkScan::AntiFillPath`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AntiFillPathAction {
    EmptyBounds = 0,
    ClippedOut = 1,
    FallbackToBW = 2,
    RunAAA = 3,
}

/// Matches the initial bounds & supersample overflow classification in `SkScan::AntiFillPath`.
pub fn classify_anti_fill_path(
    path_bounds: Rect,
    clip_bounds: IRect,
    is_inverse: bool,
) -> (AntiFillPathAction, IRect) {
    let ir = safe_round_out(path_bounds);
    if ir.is_empty() {
        return (AntiFillPathAction::EmptyBounds, ir);
    }
    let clipped_ir = if is_inverse {
        clip_bounds
    } else {
        let mut sect = IRect::EMPTY;
        if !sect.intersect_pair(ir, clip_bounds) {
            return (AntiFillPathAction::ClippedOut, ir);
        }
        sect
    };
    if rect_overflows_short_shift(clipped_ir, SK_SUPERSAMPLE_SHIFT) {
        return (AntiFillPathAction::FallbackToBW, ir);
    }
    (AntiFillPathAction::RunAAA, ir)
}
