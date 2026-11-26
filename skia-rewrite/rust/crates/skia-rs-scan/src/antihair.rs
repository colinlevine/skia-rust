#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkScan_Antihair.cpp`.

use crate::blitter::{Blitter, RectClipBlitter, RegionClip};
use crate::fixed::{
    abs32, alpha_mul, fdot6_ceil, fdot6_floor, fdot6_to_fixed, fixed_ceil_to_int,
    fixed_floor_to_int, float_to_fdot6, float_to_i32_c, int_to_fdot6, irect_contains,
    left_shift_i32, mul_div255_round, xrect_from_irect, xrect_from_rect, xrect_round_out, FDot8,
    SkFDot6, SkFixed, XRect, SK_FDOT6_HALF, SK_FDOT6_ONE, SK_FIXED_HALF,
};
use skia_rs_geometry::line_clipper::intersect_line;
use skia_rs_geometry::{IRect, Point, Rect};

const HLINE_STACK_BUFFER: usize = 100;

#[inline]
const fn scale_alpha_by_coverage(value: u8, coverage: SkFDot6) -> u8 {
    (((value as i32) * coverage) >> 6) as u8
}

#[inline]
const fn fixed_to_alpha_antihair(f: SkFixed) -> u8 {
    ((f >> 8) & 0xFF) as u8
}

fn call_hline_blitter<B: Blitter + ?Sized>(
    blitter: &mut B,
    mut x: i32,
    y: i32,
    mut count: i32,
    alpha: u8,
) {
    debug_assert!(count > 0);
    let mut runs = [0i16; HLINE_STACK_BUFFER + 1];
    let mut aa = [0u8; HLINE_STACK_BUFFER];

    while count > 0 {
        aa[0] = alpha;
        let n = (count as usize).min(HLINE_STACK_BUFFER);
        runs[0] = n as i16;
        runs[n] = 0;
        blitter.blit_anti_h(x, y, &mut aa[..n], &mut runs[..=n]);
        x += n as i32;
        count -= n as i32;
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum AntiHairMode {
    HLine,
    Horish,
    VLine,
    Vertish,
}

impl AntiHairMode {
    fn draw_cap<B: Blitter + ?Sized>(
        self,
        blitter: &mut B,
        pos: i32,
        mut f: SkFixed,
        slope: SkFixed,
        coverage: SkFDot6,
    ) -> SkFixed {
        match self {
            Self::HLine => {
                f = f.wrapping_add(SK_FIXED_HALF);
                let y = fixed_floor_to_int(f);
                let a = fixed_to_alpha_antihair(f);
                let ma = scale_alpha_by_coverage(a, coverage);
                if ma != 0 {
                    call_hline_blitter(blitter, pos, y, 1, ma);
                }
                let ma2 = scale_alpha_by_coverage(255 - a, coverage);
                if ma2 != 0 {
                    call_hline_blitter(blitter, pos, y - 1, 1, ma2);
                }
                f.wrapping_sub(SK_FIXED_HALF)
            }
            Self::Horish => {
                f = f.wrapping_add(SK_FIXED_HALF);
                let lower_y = fixed_floor_to_int(f);
                let a = fixed_to_alpha_antihair(f);
                let a0 = scale_alpha_by_coverage(255 - a, coverage);
                let a1 = scale_alpha_by_coverage(a, coverage);
                blitter.blit_anti_v2(pos, lower_y - 1, a0, a1);
                f.wrapping_add(slope).wrapping_sub(SK_FIXED_HALF)
            }
            Self::VLine => {
                f = f.wrapping_add(SK_FIXED_HALF);
                let x = fixed_floor_to_int(f);
                let a = fixed_to_alpha_antihair(f);
                let ma = scale_alpha_by_coverage(a, coverage);
                if ma != 0 {
                    blitter.blit_v(x, pos, 1, ma);
                }
                let ma2 = scale_alpha_by_coverage(255 - a, coverage);
                if ma2 != 0 {
                    blitter.blit_v(x - 1, pos, 1, ma2);
                }
                f.wrapping_sub(SK_FIXED_HALF)
            }
            Self::Vertish => {
                f = f.wrapping_add(SK_FIXED_HALF);
                let x = fixed_floor_to_int(f);
                let a = fixed_to_alpha_antihair(f);
                blitter.blit_anti_h2(
                    x - 1,
                    pos,
                    scale_alpha_by_coverage(255 - a, coverage),
                    scale_alpha_by_coverage(a, coverage),
                );
                f.wrapping_add(slope).wrapping_sub(SK_FIXED_HALF)
            }
        }
    }

    fn draw_line<B: Blitter + ?Sized>(
        self,
        blitter: &mut B,
        mut start: i32,
        stop: i32,
        mut f: SkFixed,
        slope: SkFixed,
    ) -> SkFixed {
        debug_assert!(start < stop);
        match self {
            Self::HLine => {
                let count = stop - start;
                f = f.wrapping_add(SK_FIXED_HALF);
                let y = fixed_floor_to_int(f);
                let a = fixed_to_alpha_antihair(f);
                if a != 0 {
                    call_hline_blitter(blitter, start, y, count, a);
                }
                let a2 = 255 - a;
                if a2 != 0 {
                    call_hline_blitter(blitter, start, y - 1, count, a2);
                }
                f.wrapping_sub(SK_FIXED_HALF)
            }
            Self::Horish => {
                f = f.wrapping_add(SK_FIXED_HALF);
                loop {
                    let lower_y = fixed_floor_to_int(f);
                    let a = fixed_to_alpha_antihair(f);
                    blitter.blit_anti_v2(start, lower_y - 1, 255 - a, a);
                    f = f.wrapping_add(slope);
                    start += 1;
                    if start >= stop {
                        break;
                    }
                }
                f.wrapping_sub(SK_FIXED_HALF)
            }
            Self::VLine => {
                f = f.wrapping_add(SK_FIXED_HALF);
                let x = fixed_floor_to_int(f);
                let a = fixed_to_alpha_antihair(f);
                if a != 0 {
                    blitter.blit_v(x, start, stop - start, a);
                }
                let a2 = 255 - a;
                if a2 != 0 {
                    blitter.blit_v(x - 1, start, stop - start, a2);
                }
                f.wrapping_sub(SK_FIXED_HALF)
            }
            Self::Vertish => {
                f = f.wrapping_add(SK_FIXED_HALF);
                loop {
                    let x = fixed_floor_to_int(f);
                    let a = fixed_to_alpha_antihair(f);
                    blitter.blit_anti_h2(x - 1, start, 255 - a, a);
                    f = f.wrapping_add(slope);
                    start += 1;
                    if start >= stop {
                        break;
                    }
                }
                f.wrapping_sub(SK_FIXED_HALF)
            }
        }
    }
}

#[inline]
const fn fastfixdiv(a: SkFDot6, b: SkFDot6) -> SkFixed {
    debug_assert!(b != 0);
    left_shift_i32(a, 16) / b
}

#[inline]
const fn any_bad_ints(a: i32, b: i32, c: i32, d: i32) -> bool {
    a == i32::MIN || b == i32::MIN || c == i32::MIN || d == i32::MIN
}

#[inline]
const fn fd6_frac(x: SkFDot6) -> SkFDot6 {
    x & (SK_FDOT6_ONE - 1)
}

#[inline]
const fn partial_pixel_coverage(pos: SkFDot6) -> SkFDot6 {
    fd6_frac(pos.wrapping_sub(1)) + 1
}

fn do_anti_hairline<B: Blitter + ?Sized>(
    mut x0: SkFDot6,
    mut y0: SkFDot6,
    mut x1: SkFDot6,
    mut y1: SkFDot6,
    mut clip: Option<IRect>,
    blitter: &mut B,
) {
    if any_bad_ints(x0, y0, x1, y1) {
        return;
    }

    if abs32(x1.wrapping_sub(x0)) > int_to_fdot6(511)
        || abs32(y1.wrapping_sub(y0)) > int_to_fdot6(511)
    {
        let hx = (x0 >> 1) + (x1 >> 1);
        let hy = (y0 >> 1) + (y1 >> 1);
        do_anti_hairline(x0, y0, hx, hy, clip, blitter);
        do_anti_hairline(hx, hy, x1, y1, clip, blitter);
        return;
    }

    let mut start_coverage: i32;
    let mut stop_coverage: i32;
    let mut istart: i32;
    let mut istop: i32;
    let mut fstart: SkFixed;
    let slope: SkFixed;
    let mode: AntiHairMode;

    if abs32(x1.wrapping_sub(x0)) > abs32(y1.wrapping_sub(y0)) {
        if x0 > x1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
        }

        istart = fdot6_floor(x0);
        istop = fdot6_ceil(x1);
        if y0 == y1 {
            slope = 0;
            mode = AntiHairMode::HLine;
            fstart = fdot6_to_fixed(y0);
        } else {
            slope = fastfixdiv(y1 - y0, x1 - x0);
            let dx_to_center = SK_FDOT6_HALF - fd6_frac(x0);
            fstart = fdot6_to_fixed(y0)
                .wrapping_add(slope.wrapping_mul(dx_to_center).wrapping_add(SK_FDOT6_HALF) >> 6);
            mode = AntiHairMode::Horish;
        }

        if istop - istart == 1 {
            start_coverage = x1 - x0;
            stop_coverage = 0;
        } else {
            start_coverage = SK_FDOT6_ONE - fd6_frac(x0);
            stop_coverage = fd6_frac(x1);
        }

        if let Some(c) = clip {
            if istart >= c.right || istop <= c.left {
                return;
            }
            if istart < c.left {
                fstart = fstart.wrapping_add(slope.wrapping_mul(c.left - istart));
                istart = c.left;
                start_coverage = SK_FDOT6_ONE;
                if istop - istart == 1 {
                    start_coverage = partial_pixel_coverage(x1);
                    stop_coverage = 0;
                }
            }
            if istop > c.right {
                istop = c.right;
                stop_coverage = 0;
            }
            if istart == istop {
                return;
            }
            let (mut top, mut bottom) = if slope >= 0 {
                (
                    fixed_floor_to_int(fstart.wrapping_sub(SK_FIXED_HALF)),
                    fixed_ceil_to_int(
                        fstart
                            .wrapping_add((istop - istart - 1).wrapping_mul(slope))
                            .wrapping_add(SK_FIXED_HALF),
                    ),
                )
            } else {
                (
                    fixed_floor_to_int(
                        fstart
                            .wrapping_add((istop - istart - 1).wrapping_mul(slope))
                            .wrapping_sub(SK_FIXED_HALF),
                    ),
                    fixed_ceil_to_int(fstart.wrapping_add(SK_FIXED_HALF)),
                )
            };
            top -= 1;
            bottom += 1;

            if top >= c.bottom || bottom <= c.top {
                return;
            }
            if c.top <= top && c.bottom >= bottom {
                clip = None;
            }
        }
    } else {
        if y0 > y1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
        }

        istart = fdot6_floor(y0);
        istop = fdot6_ceil(y1);
        if x0 == x1 {
            if y0 == y1 {
                return;
            }
            slope = 0;
            mode = AntiHairMode::VLine;
            fstart = fdot6_to_fixed(x0);
        } else {
            slope = fastfixdiv(x1 - x0, y1 - y0);
            let dy_to_center = SK_FDOT6_HALF - fd6_frac(y0);
            fstart = fdot6_to_fixed(x0)
                .wrapping_add(slope.wrapping_mul(dy_to_center).wrapping_add(SK_FDOT6_HALF) >> 6);
            mode = AntiHairMode::Vertish;
        }

        if istop - istart == 1 {
            start_coverage = y1 - y0;
            stop_coverage = 0;
        } else {
            start_coverage = SK_FDOT6_ONE - fd6_frac(y0);
            stop_coverage = fd6_frac(y1);
        }

        if let Some(c) = clip {
            if istart >= c.bottom || istop <= c.top {
                return;
            }
            if istart < c.top {
                fstart = fstart.wrapping_add(slope.wrapping_mul(c.top - istart));
                istart = c.top;
                start_coverage = SK_FDOT6_ONE;
                if istop - istart == 1 {
                    start_coverage = partial_pixel_coverage(y1);
                    stop_coverage = 0;
                }
            }
            if istop > c.bottom {
                istop = c.bottom;
                stop_coverage = 0;
            }
            if istart == istop {
                return;
            }
            let (mut left, mut right) = if slope >= 0 {
                (
                    fixed_floor_to_int(fstart.wrapping_sub(SK_FIXED_HALF)),
                    fixed_ceil_to_int(
                        fstart
                            .wrapping_add((istop - istart - 1).wrapping_mul(slope))
                            .wrapping_add(SK_FIXED_HALF),
                    ),
                )
            } else {
                (
                    fixed_floor_to_int(
                        fstart
                            .wrapping_add((istop - istart - 1).wrapping_mul(slope))
                            .wrapping_sub(SK_FIXED_HALF),
                    ),
                    fixed_ceil_to_int(fstart.wrapping_add(SK_FIXED_HALF)),
                )
            };
            left -= 1;
            right += 1;

            if left >= c.right || right <= c.left {
                return;
            }
            if c.left <= left && c.right >= right {
                clip = None;
            }
        }
    }

    fn emit<B: Blitter + ?Sized>(
        b: &mut B,
        mode: AntiHairMode,
        mut istart: i32,
        istop: i32,
        mut fstart: SkFixed,
        slope: SkFixed,
        start_coverage: i32,
        stop_coverage: i32,
    ) {
        fstart = mode.draw_cap(b, istart, fstart, slope, start_coverage);
        istart += 1;
        let full_spans = istop - istart - if stop_coverage > 0 { 1 } else { 0 };
        if full_spans > 0 {
            fstart = mode.draw_line(b, istart, istart + full_spans, fstart, slope);
        }
        if stop_coverage > 0 {
            mode.draw_cap(b, istop - 1, fstart, slope, stop_coverage);
        }
    }

    if let Some(c) = clip {
        let mut rect_clipper = RectClipBlitter::new(blitter, c);
        emit(
            &mut rect_clipper,
            mode,
            istart,
            istop,
            fstart,
            slope,
            start_coverage,
            stop_coverage,
        );
    } else {
        emit(
            blitter,
            mode,
            istart,
            istop,
            fstart,
            slope,
            start_coverage,
            stop_coverage,
        );
    }
}

/// Matches `SkScan::AntiHairLineRgn`.
pub fn anti_hair_line_rgn<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    src: &[Point],
    clip: Option<&R>,
    blitter: &mut B,
) {
    if src.len() < 2 {
        return;
    }
    if let Some(c) = clip {
        if c.is_empty() {
            return;
        }
    }

    let max = 32767.0_f32;
    let fixed_bounds = Rect::make_ltrb(-max, -max, max, max);
    let clip_bounds = clip.map(|c| {
        let b = Rect::from_irect(c.bounds());
        Rect::make_ltrb(b.left - 1.0, b.top - 1.0, b.right + 1.0, b.bottom + 1.0)
    });

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

        let x0 = float_to_fdot6(pts[0].x);
        let y0 = float_to_fdot6(pts[0].y);
        let x1 = float_to_fdot6(pts[1].x);
        let y1 = float_to_fdot6(pts[1].y);

        if let Some(c) = clip {
            let left = x0.min(x1);
            let top = y0.min(y1);
            let right = x0.max(x1);
            let bottom = y0.max(y1);
            let ir = IRect::make_ltrb(
                fdot6_floor(left) - 1,
                fdot6_floor(top) - 1,
                fdot6_ceil(right) + 1,
                fdot6_ceil(bottom) + 1,
            );
            if c.quick_reject(ir) {
                continue;
            }
            if !c.quick_contains(ir) {
                c.for_each_clip_rect(ir, &mut |r| {
                    do_anti_hairline(x0, y0, x1, y1, Some(r), blitter);
                });
                continue;
            }
        }
        do_anti_hairline(x0, y0, x1, y1, None, blitter);
    }
}

///////////////////////////////////////////////////////////////////////////////
// AntiFillXRect, AntiFillRect, AntiFrameRect

#[inline]
const fn fixed_to_fdot8(x: SkFixed) -> FDot8 {
    x.wrapping_add(0x80) >> 8
}

#[inline]
fn scalar_to_fdot8(x: f32) -> FDot8 {
    float_to_i32_c(x * 256.0_f32)
}

#[inline]
const fn fdot8_floor(x: FDot8) -> i32 {
    x >> 8
}

#[inline]
const fn fdot8_ceil(x: FDot8) -> i32 {
    x.wrapping_add(0xFF) >> 8
}

fn do_scanline<B: Blitter + ?Sized>(l: FDot8, top: i32, r: FDot8, alpha: u8, blitter: &mut B) {
    debug_assert!(l < r);

    if (l >> 8) == ((r - 1) >> 8) {
        let a = alpha_mul(alpha as u32, (r - l) as u32) as u8;
        blitter.blit_v(l >> 8, top, 1, a);
        return;
    }

    let mut left = l >> 8;
    if (l & 0xFF) != 0 {
        let a = alpha_mul(alpha as u32, (256 - (l & 0xFF)) as u32) as u8;
        blitter.blit_v(left, top, 1, a);
        left += 1;
    }

    let rite = r >> 8;
    let width = rite - left;
    if width > 0 {
        call_hline_blitter(blitter, left, top, width, alpha);
    }
    if (r & 0xFF) != 0 {
        let a = alpha_mul(alpha as u32, (r & 0xFF) as u32) as u8;
        blitter.blit_v(rite, top, 1, a);
    }
}

fn antifilldot8<B: Blitter + ?Sized>(
    l: FDot8,
    t: FDot8,
    r: FDot8,
    b: FDot8,
    blitter: &mut B,
    fill_inner: bool,
) {
    if l >= r || t >= b {
        return;
    }
    let mut top = t >> 8;
    if top == ((b - 1) >> 8) {
        do_scanline(l, top, r, (b - t - 1) as u8, blitter);
        return;
    }

    if (t & 0xFF) != 0 {
        do_scanline(l, top, r, (256 - (t & 0xFF)) as u8, blitter);
        top += 1;
    }

    let bot = b >> 8;
    let height = bot - top;
    if height > 0 {
        let mut left = l >> 8;
        if left == ((r - 1) >> 8) {
            blitter.blit_v(left, top, height, (r - l - 1) as u8);
        } else {
            if (l & 0xFF) != 0 {
                blitter.blit_v(left, top, height, (256 - (l & 0xFF)) as u8);
                left += 1;
            }
            let rite = r >> 8;
            let width = rite - left;
            if width > 0 && fill_inner {
                blitter.blit_rect(left, top, width, height);
            }
            if (r & 0xFF) != 0 {
                blitter.blit_v(rite, top, height, (r & 0xFF) as u8);
            }
        }
    }

    if (b & 0xFF) != 0 {
        do_scanline(l, bot, r, (b & 0xFF) as u8, blitter);
    }
}

#[inline]
fn antifillxrect_unclipped<B: Blitter + ?Sized>(xr: XRect, blitter: &mut B) {
    antifilldot8(
        fixed_to_fdot8(xr.left),
        fixed_to_fdot8(xr.top),
        fixed_to_fdot8(xr.right),
        fixed_to_fdot8(xr.bottom),
        blitter,
        true,
    );
}

#[inline]
fn antifillrect_unclipped<B: Blitter + ?Sized>(r: Rect, blitter: &mut B) {
    antifillxrect_unclipped(xrect_from_rect(r), blitter);
}

/// Matches `SkScan::AntiFillXRect(const SkXRect&, const SkRegion*, SkBlitter*)`.
pub fn anti_fill_xrect<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    xr: XRect,
    clip: Option<&R>,
    blitter: &mut B,
) {
    let Some(clip) = clip else {
        antifillxrect_unclipped(xr, blitter);
        return;
    };
    let outer_bounds = xrect_round_out(xr);
    if clip.is_rect() {
        let clip_bounds = clip.bounds();
        if irect_contains(clip_bounds, outer_bounds) {
            antifillxrect_unclipped(xr, blitter);
        } else {
            let mut tmp_r = IRect::EMPTY;
            if tmp_r.intersect_pair(xrect_from_irect(clip_bounds), xr) {
                antifillxrect_unclipped(tmp_r, blitter);
            }
        }
    } else {
        clip.for_each_clip_rect(outer_bounds, &mut |rr| {
            let mut tmp_r = IRect::EMPTY;
            if tmp_r.intersect_pair(xrect_from_irect(rr), xr) {
                antifillxrect_unclipped(tmp_r, blitter);
            }
        });
    }
}

/// Matches `SkScan::AntiFillRect(const SkRect&, const SkRegion*, SkBlitter*)`.
pub fn anti_fill_rect<B: Blitter + ?Sized, R: RegionClip + ?Sized>(
    orig_r: Rect,
    clip: Option<&R>,
    blitter: &mut B,
) {
    let Some(clip) = clip else {
        antifillrect_unclipped(orig_r, blitter);
        return;
    };
    let mut new_r = Rect::EMPTY;
    if !new_r.intersect_pair(Rect::from_irect(clip.bounds()), orig_r) {
        return;
    }
    let outer_bounds = new_r.round_out();
    if clip.is_rect() {
        antifillrect_unclipped(new_r, blitter);
    } else {
        clip.for_each_clip_rect(outer_bounds, &mut |rr| {
            let mut sect = Rect::EMPTY;
            if sect.intersect_pair(Rect::from_irect(rr), orig_r) {
                antifillrect_unclipped(sect, blitter);
            }
        });
    }
}

#[inline]
fn fillcheckrect<B: Blitter + ?Sized>(l: i32, t: i32, r: i32, b: i32, blitter: &mut B) {
    if l < r && t < b {
        blitter.blit_rect(l, t, r - l, b - t);
    }
}

#[inline]
const fn inv_alpha_mul(a: u8, b: u8) -> u8 {
    let prod = mul_div255_round(a as u32, b as u32) as u32;
    ((a as u32) + (b as u32) - prod) as u8
}

fn inner_scanline<B: Blitter + ?Sized>(l: FDot8, top: i32, r: FDot8, alpha: u8, blitter: &mut B) {
    debug_assert!(l < r);

    if (l >> 8) == ((r - 1) >> 8) {
        let mut wid_clamp = r - l;
        wid_clamp = wid_clamp - (wid_clamp >> 8);
        blitter.blit_v(l >> 8, top, 1, inv_alpha_mul(alpha, wid_clamp as u8));
        return;
    }

    let mut left = l >> 8;
    if (l & 0xFF) != 0 {
        blitter.blit_v(left, top, 1, inv_alpha_mul(alpha, (l & 0xFF) as u8));
        left += 1;
    }

    let rite = r >> 8;
    let width = rite - left;
    if width > 0 {
        call_hline_blitter(blitter, left, top, width, alpha);
    }

    if (r & 0xFF) != 0 {
        blitter.blit_v(rite, top, 1, inv_alpha_mul(alpha, ((!r) & 0xFF) as u8));
    }
}

fn innerstrokedot8<B: Blitter + ?Sized>(
    l: FDot8,
    t: FDot8,
    r: FDot8,
    b: FDot8,
    blitter: &mut B,
) {
    debug_assert!(l < r && t < b);

    let mut top = t >> 8;
    if top == ((b - 1) >> 8) {
        let alpha = 256 - (b - t);
        if alpha != 0 {
            inner_scanline(l, top, r, alpha as u8, blitter);
        }
        return;
    }

    if (t & 0xFF) != 0 {
        inner_scanline(l, top, r, (t & 0xFF) as u8, blitter);
        top += 1;
    }

    let bot = b >> 8;
    let height = bot - top;
    if height > 0 {
        if (l & 0xFF) != 0 {
            blitter.blit_v(l >> 8, top, height, (l & 0xFF) as u8);
        }
        if (r & 0xFF) != 0 {
            blitter.blit_v(r >> 8, top, height, ((!r) & 0xFF) as u8);
        }
    }

    if (b & 0xFF) != 0 {
        inner_scanline(l, bot, r, ((!b) & 0xFF) as u8, blitter);
    }
}

#[inline]
fn align_thin_stroke(edge1: &mut FDot8, edge2: &mut FDot8) {
    debug_assert!(*edge1 <= *edge2);
    if fdot8_floor(*edge1) == fdot8_floor(*edge2) {
        *edge2 -= *edge1 & 0xFF;
        *edge1 &= !0xFF;
    }
}

/// Precomputed outer bounds for `SkScan::AntiFrameRect` (so C++ can apply `SkBlitterClipper`).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AntiFrameRectParams {
    pub outer_rect: IRect,
    pub outer_l: FDot8,
    pub outer_t: FDot8,
    pub outer_r: FDot8,
    pub outer_b: FDot8,
    pub rx: f32,
    pub ry: f32,
}

/// Stage 1 of `SkScan::AntiFrameRect`: computes outer hull and bounding `SkIRect`.
pub fn anti_frame_rect_setup(r: Rect, stroke_size: Point) -> AntiFrameRectParams {
    let mut rx = stroke_size.x / 2.0_f32;
    let mut ry = stroke_size.y / 2.0_f32;
    if r.width() == 0.0 {
        ry = 0.0;
    }
    if r.height() == 0.0 {
        rx = 0.0;
    }
    let outer_l = scalar_to_fdot8(r.left - rx);
    let outer_t = scalar_to_fdot8(r.top - ry);
    let outer_r = scalar_to_fdot8(r.right + rx);
    let outer_b = scalar_to_fdot8(r.bottom + ry);
    let outer_rect = IRect::make_ltrb(
        fdot8_floor(outer_l),
        fdot8_floor(outer_t),
        fdot8_ceil(outer_r),
        fdot8_ceil(outer_b),
    );
    AntiFrameRectParams {
        outer_rect,
        outer_l,
        outer_t,
        outer_r,
        outer_b,
        rx,
        ry,
    }
}

/// Stage 2 of `SkScan::AntiFrameRect`: renders the outer hull, solid middle, and inner stroke
/// using the (already-clipped) `blitter`.
pub fn anti_frame_rect_draw<B: Blitter + ?Sized>(
    r: Rect,
    stroke_size: Point,
    params: &AntiFrameRectParams,
    blitter: &mut B,
) {
    let mut outer_l = params.outer_l;
    let mut outer_t = params.outer_t;
    let mut outer_r = params.outer_r;
    let mut outer_b = params.outer_b;

    let rx = stroke_size.x - params.rx;
    let ry = stroke_size.y - params.ry;

    let mut inner_l = scalar_to_fdot8(r.left + rx);
    let mut inner_t = scalar_to_fdot8(r.top + ry);
    let mut inner_r = scalar_to_fdot8(r.right - rx);
    let mut inner_b = scalar_to_fdot8(r.bottom - ry);

    if stroke_size.x < 1.0 || stroke_size.y < 1.0 {
        align_thin_stroke(&mut outer_l, &mut inner_l);
        align_thin_stroke(&mut outer_t, &mut inner_t);
        align_thin_stroke(&mut inner_r, &mut outer_r);
        align_thin_stroke(&mut inner_b, &mut outer_b);
    }

    antifilldot8(outer_l, outer_t, outer_r, outer_b, blitter, false);

    let outer = IRect::make_ltrb(
        fdot8_ceil(outer_l),
        fdot8_ceil(outer_t),
        fdot8_floor(outer_r),
        fdot8_floor(outer_b),
    );

    if inner_l >= inner_r || inner_t >= inner_b {
        fillcheckrect(outer.left, outer.top, outer.right, outer.bottom, blitter);
    } else {
        let inner = IRect::make_ltrb(
            fdot8_floor(inner_l),
            fdot8_floor(inner_t),
            fdot8_ceil(inner_r),
            fdot8_ceil(inner_b),
        );
        fillcheckrect(outer.left, outer.top, outer.right, inner.top, blitter);
        fillcheckrect(outer.left, inner.top, inner.left, inner.bottom, blitter);
        fillcheckrect(inner.right, inner.top, outer.right, inner.bottom, blitter);
        fillcheckrect(outer.left, inner.bottom, outer.right, outer.bottom, blitter);

        innerstrokedot8(inner_l, inner_t, inner_r, inner_b, blitter);
    }
}
