//! Safe Rust port of `src/core/SkBlitter.cpp`, `src/core/SkBlitter_Sprite.cpp`,
//! and `src/core/SkSpriteBlitter_ARGB32.cpp`.

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::rect::{IRect, Rect};

use crate::blit_row::{GLOBAL_ALPHA_FLAG32, SRC_PIXEL_ALPHA_FLAG32};

/// Exact port of `scalar_to_alpha` from `SkBlitter.cpp`.
#[inline]
pub fn scalar_to_alpha(a: f32) -> u8 {
    let alpha = (a * 255.0f32) as u8;
    if alpha > 247 {
        0xFF
    } else if alpha < 8 {
        0
    } else {
        alpha
    }
}

/// Precomputed scanline parameters for `SkBlitter::blitFatAntiRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FatAntiRectParams {
    pub bounds: IRect,
    pub top_l: u8,
    pub top_m: u8,
    pub top_r: u8,
    pub mid_l: u8,
    pub mid_r: u8,
    pub bot_l: u8,
    pub bot_m: u8,
    pub bot_r: u8,
}

/// Compute `SkBlitter::blitFatAntiRect` parameters, or `None` if `bounds.height() == 0`.
pub fn compute_fat_anti_rect(rect: Rect) -> Option<FatAntiRectParams> {
    let bounds = rect.round_out();
    let h = bounds.height();
    if h == 0 {
        return None;
    }

    let partial_l = (bounds.left + 1) as f32 - rect.left;
    let partial_r = rect.right - (bounds.right - 1) as f32;
    let mut partial_t = (bounds.top + 1) as f32 - rect.top;
    let partial_b = rect.bottom - (bounds.bottom - 1) as f32;

    if h == 1 {
        partial_t = rect.bottom - rect.top;
    }

    Some(FatAntiRectParams {
        bounds,
        top_l: scalar_to_alpha(partial_l * partial_t),
        top_m: scalar_to_alpha(partial_t),
        top_r: scalar_to_alpha(partial_r * partial_t),
        mid_l: scalar_to_alpha(partial_l),
        mid_r: scalar_to_alpha(partial_r),
        bot_l: scalar_to_alpha(partial_l * partial_b),
        bot_m: scalar_to_alpha(partial_b),
        bot_r: scalar_to_alpha(partial_r * partial_b),
    })
}

/// Exact port of `generate_right_mask` from `SkBlitter.cpp`.
/// `mask_bit_count` must be in `1..=8`.
#[inline]
pub const fn generate_right_mask(mask_bit_count: i32) -> u8 {
    ((0xFF00u32 >> mask_bit_count) & 0xFF) as u8
}

/// Parameters describing one clipped row of a 1-bit BW mask in `SkBlitter::blitMask`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BwMaskRowParams {
    pub bits_left: i32,
    pub row_bytes: usize,
    pub left_mask: u8,
    pub right_mask: u8,
}

/// Compute row parameters for `SkBlitter::blitMask` (`kBW_Format`).
pub const fn compute_bw_mask_row_params(
    clip_left: i32,
    clip_right: i32,
    mask_left: i32,
    mask_right: i32,
) -> BwMaskRowParams {
    if clip_left == mask_left && clip_right == mask_right {
        let affected_right_bit = (mask_right - mask_left) - 1;
        let row_bytes = ((affected_right_bit >> 3) + 1) as usize;
        let right_mask = generate_right_mask((affected_right_bit & 7) + 1);
        BwMaskRowParams {
            bits_left: clip_left,
            row_bytes,
            left_mask: 0xFF,
            right_mask,
        }
    } else {
        let bits_left = clip_left - ((clip_left - mask_left) & 7);
        let left_edge = clip_left - bits_left;
        let right_edge = clip_right - bits_left;
        let left_mask = (0xFFu32 >> (left_edge & 7)) as u8;
        let affected_right_bit = right_edge - 1;
        let right_mask = generate_right_mask((affected_right_bit & 7) + 1);
        let row_bytes = ((affected_right_bit >> 3) + 1) as usize;
        BwMaskRowParams {
            bits_left,
            row_bytes,
            left_mask,
            right_mask,
        }
    }
}

/// Exact port of `bits_to_runs` from `SkBlitter.cpp`, emitting `(x, width)` spans via callback.
pub fn bw_bits_to_runs<F: FnMut(i32, i32)>(
    mut x: i32,
    bits: &[u8],
    mut left_mask: u8,
    right_mask: u8,
    mut emit: F,
) {
    let mut in_fill = false;
    let mut pos = 0i32;
    let total = bits.len();

    for (idx, &raw_byte) in bits.iter().enumerate() {
        let mut b = raw_byte & left_mask;
        if idx + 1 == total {
            b &= right_mask;
        }

        let mut test = 0x80u8;
        while test != 0 {
            if (b & test) != 0 {
                if !in_fill {
                    pos = x;
                    in_fill = true;
                }
            } else if in_fill {
                emit(pos, x - pos);
                in_fill = false;
            }
            x += 1;
            test >>= 1;
        }
        left_mask = 0xFF;
    }

    if in_fill {
        emit(pos, x - pos);
    }
}

/// Exact port of `compute_anti_width` from `SkBlitter.cpp`.
pub fn compute_anti_width(runs: &[i16]) -> i32 {
    let mut pos = 0usize;
    let mut width = 0i32;
    while pos < runs.len() {
        let count = runs[pos] as i32;
        if count <= 0 {
            break;
        }
        width += count;
        pos += count as usize;
    }
    width
}

/// Exact port of `SkAlphaRuns::BreakAt`.
pub fn alpha_runs_break_at(runs: &mut [i16], alpha: &mut [u8], mut x: usize) {
    let mut pos = 0usize;
    while x > 0 {
        let n = runs[pos] as usize;
        if n == 0 {
            break;
        }
        if x < n {
            alpha[pos + x] = alpha[pos];
            runs[pos] = x as i16;
            runs[pos + x] = (n - x) as i16;
            break;
        }
        pos += n;
        x -= n;
    }
}

/// Exact port of `SkAlphaRuns::Break`.
pub fn alpha_runs_break(runs: &mut [i16], alpha: &mut [u8], x: usize, count: usize) {
    alpha_runs_break_at(runs, alpha, x);
    alpha_runs_break_at(&mut runs[x..], &mut alpha[x..], count);
}

/// Result of planning an anti-aliased span clip against a bounding `IRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClipAntiSpanPlan {
    pub new_x: i32,
    pub dx: i32,
    pub width: i32,
    pub need_break_left: bool,
    pub need_break_right: bool,
}

/// Exact port of `clip_anti_span` coordinate logic from `SkBlitter.cpp`, using 64-bit math
/// for `clip.left - x` so extreme `i32::MIN` / `i32::MAX` coordinates never overflow.
pub fn plan_clip_anti_span(
    mut x: i32,
    y: i32,
    mut width: i32,
    clip: IRect,
    trim_right: bool,
) -> ClipAntiSpanPlan {
    let empty = ClipAntiSpanPlan {
        new_x: x,
        dx: 0,
        width: 0,
        need_break_left: false,
        need_break_right: false,
    };
    if y < clip.top || y >= clip.bottom || x >= clip.right || width <= 0 {
        return empty;
    }

    let mut dx = 0i32;
    let mut need_break_left = false;
    if x < clip.left {
        let dx64 = clip.left as i64 - x as i64;
        if dx64 >= width as i64 {
            return empty;
        }
        dx = dx64 as i32;
        x = clip.left;
        width -= dx;
        need_break_left = true;
    }

    let max_width = clip.right - x;
    let mut need_break_right = false;
    if width > max_width {
        width = max_width;
        if trim_right {
            need_break_right = true;
        }
    }

    ClipAntiSpanPlan {
        new_x: x,
        dx,
        width,
        need_break_left,
        need_break_right,
    }
}

/// Write a zero-alpha gap run into `runs`/`aa` starting at `offset`, chunking gaps larger
/// than `i16::MAX` (32767) into multiple runs (matching `SkRgnClipBlitter::blitAntiH`).
pub fn write_rgn_clip_gap(runs: &mut [i16], aa: &mut [u8], offset: usize, mut gap: usize) {
    let mut index = offset;
    while gap > i16::MAX as usize {
        aa[index] = 0;
        runs[index] = i16::MAX;
        index += i16::MAX as usize;
        gap -= i16::MAX as usize;
    }
    if gap > 0 {
        aa[index] = 0;
        runs[index] = gap as i16;
    }
}

/// Exact port of `y_in_rect` from `SkBlitter.cpp`.
#[inline]
pub const fn y_in_rect(y: i32, rect: IRect) -> bool {
    (y.wrapping_sub(rect.top) as u32) < (rect.bottom.wrapping_sub(rect.top) as u32)
}

/// Exact port of `x_in_rect` from `SkBlitter.cpp`.
#[inline]
pub const fn x_in_rect(x: i32, rect: IRect) -> bool {
    (x.wrapping_sub(rect.left) as u32) < (rect.right.wrapping_sub(rect.left) as u32)
}

/// Exact port of `SkRectClipBlitter::blitH` clipping.
#[inline]
pub const fn clip_blit_h(mut left: i32, y: i32, width: i32, clip: IRect) -> Option<(i32, i32)> {
    if !y_in_rect(y, clip) {
        return None;
    }
    let mut right = left.wrapping_add(width);
    if left < clip.left {
        left = clip.left;
    }
    if right > clip.right {
        right = clip.right;
    }
    let w = right - left;
    if w > 0 {
        Some((left, w))
    } else {
        None
    }
}

/// Exact port of `SkRectClipBlitter::blitV` clipping.
#[inline]
pub const fn clip_blit_v(x: i32, y: i32, height: i32, clip: IRect) -> Option<(i32, i32)> {
    if !x_in_rect(x, clip) {
        return None;
    }
    let mut y0 = y;
    let mut y1 = y.wrapping_add(height);
    if y0 < clip.top {
        y0 = clip.top;
    }
    if y1 > clip.bottom {
        y1 = clip.bottom;
    }
    if y0 < y1 {
        Some((y0, y1 - y0))
    } else {
        None
    }
}

/// Action to perform for a clipped sub-rect of `blitAntiRect` (shared by `SkRectClipBlitter`
/// and `SkRgnClipBlitter`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AntiRectDispatch {
    /// 0 = None, 1 = BlitRect, 2 = BlitV, 3 = BlitAntiRect
    pub kind: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub left_alpha: u8,
    pub right_alpha: u8,
}

impl AntiRectDispatch {
    pub const NONE: Self = Self {
        kind: 0,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        left_alpha: 0,
        right_alpha: 0,
    };
}

/// Compute dispatch for one clipped piece `r` of an anti-aliased rect originally at
/// `(orig_x, orig_width, left_alpha, right_alpha)` (true width is `orig_width + 2`).
pub fn dispatch_clipped_anti_rect_piece(
    orig_x: i32,
    orig_width: i32,
    r: IRect,
    left_alpha: u8,
    right_alpha: u8,
) -> AntiRectDispatch {
    let eff_left = if r.left == orig_x { left_alpha } else { 255 };
    let eff_right = if r.right == orig_x + orig_width + 2 {
        right_alpha
    } else {
        255
    };

    if eff_left == 255 && eff_right == 255 {
        AntiRectDispatch {
            kind: 1,
            x: r.left,
            y: r.top,
            width: r.width(),
            height: r.height(),
            left_alpha: 255,
            right_alpha: 255,
        }
    } else if r.width() == 1 {
        let alpha = if r.left == orig_x {
            eff_left
        } else {
            eff_right
        };
        AntiRectDispatch {
            kind: 2,
            x: r.left,
            y: r.top,
            width: 1,
            height: r.height(),
            left_alpha: alpha,
            right_alpha: alpha,
        }
    } else {
        AntiRectDispatch {
            kind: 3,
            x: r.left,
            y: r.top,
            width: r.width() - 2,
            height: r.height(),
            left_alpha: eff_left,
            right_alpha: eff_right,
        }
    }
}

/// Exact port of `SkRectClipBlitter::blitAntiRect`.
pub fn clip_rect_blit_anti_rect(
    left: i32,
    y: i32,
    width: i32,
    height: i32,
    left_alpha: u8,
    right_alpha: u8,
    clip: IRect,
) -> AntiRectDispatch {
    let mut r = IRect::make_empty();
    let orig = IRect::make_ltrb(left, y, left + width + 2, y + height);
    if !r.intersect_pair(orig, clip) {
        return AntiRectDispatch::NONE;
    }
    dispatch_clipped_anti_rect_piece(left, width, r, left_alpha, right_alpha)
}

/// Decision result for `SkBlitterClipper::apply`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlitterClipKind {
    Passthrough = 0,
    Null = 1,
    Rect = 2,
    Region = 3,
}

/// Exact port of `SkBlitterClipper::apply` decision logic.
pub fn choose_blitter_clip_kind(
    clip_is_empty: bool,
    clip_is_rect: bool,
    clip_bounds: IRect,
    ir: Option<IRect>,
) -> BlitterClipKind {
    if clip_is_empty {
        return BlitterClipKind::Null;
    }
    if let Some(r) = ir {
        if !IRect::intersects(clip_bounds, r) {
            return BlitterClipKind::Null;
        }
    }
    if clip_is_rect {
        if let Some(r) = ir {
            let contains = !r.is_empty()
                && !clip_bounds.is_empty()
                && clip_bounds.left <= r.left
                && clip_bounds.top <= r.top
                && clip_bounds.right >= r.right
                && clip_bounds.bottom >= r.bottom;
            if contains {
                return BlitterClipKind::Passthrough;
            }
        }
        BlitterClipKind::Rect
    } else {
        BlitterClipKind::Region
    }
}

/// Exact port of `SkBlitter::UseLegacyBlitter`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub const fn use_legacy_blitter(
    force_raster_pipeline: bool,
    is_dither: bool,
    dst_alpha_type: AlphaType,
    is_src_over: bool,
    has_3d_mask_filter: bool,
    has_color_space: bool,
    is_srgb_color_space: bool,
    has_shader: bool,
    color4f_fits_in_bytes: bool,
    is_n32: bool,
) -> bool {
    if force_raster_pipeline || is_dither {
        return false;
    }
    if matches!(dst_alpha_type, AlphaType::Unpremul) || !is_src_over || has_3d_mask_filter {
        return false;
    }
    if has_color_space && !has_shader && (!color4f_fits_in_bytes || !is_srgb_color_space) {
        return false;
    }
    is_n32
}

/// Exact port of `SkSpriteBlitter_Memcpy::Supports`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub const fn sprite_memcpy_supports(
    dst_ct: ColorType,
    src_ct: ColorType,
    has_mask_filter: bool,
    has_color_filter: bool,
    has_image_filter: bool,
    paint_alpha: u8,
    blend_mode: Option<BlendMode>,
    src_is_opaque: bool,
) -> bool {
    if dst_ct as i32 != src_ct as i32 {
        return false;
    }
    if has_mask_filter || has_color_filter || has_image_filter {
        return false;
    }
    if paint_alpha != 0xFF {
        return false;
    }
    match blend_mode {
        Some(BlendMode::Src) => true,
        Some(BlendMode::SrcOver) => src_is_opaque,
        _ => false,
    }
}

/// Exact port of `SkSpriteBlitter_Memcpy::blitRect` row copy loop.
pub fn sprite_memcpy_blit_rect(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    bytes_to_copy: usize,
    height: usize,
) {
    if bytes_to_copy == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let d_start = row * dst_rb;
        let s_start = row * src_rb;
        if d_start + bytes_to_copy <= dst.len() && s_start + bytes_to_copy <= src.len() {
            dst[d_start..d_start + bytes_to_copy]
                .copy_from_slice(&src[s_start..s_start + bytes_to_copy]);
        }
    }
}

/// Exact port of `SkSpriteBlitter::ChooseL32` eligibility check.
#[inline]
pub const fn sprite_choose_l32_supports(
    has_color_filter: bool,
    has_mask_filter: bool,
    src_is_n32: bool,
    is_src_over: bool,
) -> bool {
    !has_color_filter && !has_mask_filter && src_is_n32 && is_src_over
}

/// Exact port of `Sprite_D32_S32` constructor flag selection.
#[inline]
pub const fn sprite_d32_s32_flags(alpha: u8, src_is_opaque: bool) -> u32 {
    let mut flags = 0u32;
    if alpha != 255 {
        flags |= GLOBAL_ALPHA_FLAG32;
    }
    if !src_is_opaque {
        flags |= SRC_PIXEL_ALPHA_FLAG32;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_to_alpha_thresholds() {
        assert_eq!(scalar_to_alpha(0.0), 0);
        assert_eq!(scalar_to_alpha(7.0 / 255.0), 0);
        assert_eq!(scalar_to_alpha(8.5 / 255.0), 8);
        assert_eq!(scalar_to_alpha(0.5), 127);
        assert_eq!(scalar_to_alpha(247.5 / 255.0), 247);
        assert_eq!(scalar_to_alpha(248.5 / 255.0), 255);
        assert_eq!(scalar_to_alpha(1.0), 255);
    }

    #[test]
    fn bw_bits_to_runs_emits_expected_spans() {
        // 0b1101_0011, 0b1010_0000 with left_mask=0x7F (clear bit 0), right_mask=0xE0 (keep top 3 bits)
        // effective bits: 0,1,0,1,0,0,1,1, 1,0,1,0,0,0,0,0
        let bits = [0b1101_0011u8, 0b1010_0000u8];
        let mut out = Vec::new();
        bw_bits_to_runs(10, &bits, 0x7F, 0xE0, |x, w| out.push((x, w)));
        assert_eq!(out, vec![(11, 1), (13, 1), (16, 3), (20, 1)]);
    }

    #[test]
    fn clip_anti_span_handles_extreme_i32_coords_and_large_gaps() {
        let clip = IRect::make_ltrb(5, 0, 25, 10);
        assert_eq!(plan_clip_anti_span(i32::MIN, 0, 30, clip, true).width, 0);
        assert_eq!(plan_clip_anti_span(i32::MAX, 0, 30, clip, true).width, 0);
        assert_eq!(plan_clip_anti_span(0, i32::MIN, 30, clip, true).width, 0);

        let plan = plan_clip_anti_span(0, 0, 30, clip, true);
        assert_eq!(plan.new_x, 5);
        assert_eq!(plan.dx, 5);
        assert_eq!(plan.width, 20);
        assert!(plan.need_break_left);
        assert!(plan.need_break_right);

        // Gap chunking > i16::MAX
        let gap = 40000usize;
        let mut runs = vec![0i16; gap + 1];
        let mut aa = vec![0xFFu8; gap];
        write_rgn_clip_gap(&mut runs, &mut aa, 0, gap);
        assert_eq!(runs[0], i16::MAX);
        assert_eq!(aa[0], 0);
        assert_eq!(runs[i16::MAX as usize], (gap - i16::MAX as usize) as i16);
        assert_eq!(aa[i16::MAX as usize], 0);
        assert_eq!(compute_anti_width(&runs), gap as i32);
    }

    #[test]
    fn alpha_runs_break_and_break_at() {
        // A4 B4 -> Break(2, 5) -> A2 A2 B3 B1
        let mut runs = [0i16; 9];
        let mut aa = [0u8; 8];
        runs[0] = 4;
        aa[0] = 0xAA;
        runs[4] = 4;
        aa[4] = 0xBB;
        runs[8] = 0;

        alpha_runs_break(&mut runs, &mut aa, 2, 5);
        assert_eq!(runs[0], 2);
        assert_eq!(aa[0], 0xAA);
        assert_eq!(runs[2], 2);
        assert_eq!(aa[2], 0xAA);
        assert_eq!(runs[4], 3);
        assert_eq!(aa[4], 0xBB);
        assert_eq!(runs[7], 1);
        assert_eq!(aa[7], 0xBB);
        assert_eq!(compute_anti_width(&runs), 8);
    }

    #[test]
    fn clip_rect_anti_rect_dispatch_cases() {
        let clip = IRect::make_ltrb(5, 0, 15, 10);
        // Full inside: x=6, width=4 (true right=12)
        let d1 = clip_rect_blit_anti_rect(6, 1, 4, 5, 100, 200, clip);
        assert_eq!(d1.kind, 3);
        assert_eq!((d1.x, d1.width, d1.left_alpha, d1.right_alpha), (6, 4, 100, 200));

        // Clipped on both sides -> solid BlitRect
        let d2 = clip_rect_blit_anti_rect(0, 1, 20, 5, 100, 200, clip);
        assert_eq!(d2.kind, 1);
        assert_eq!((d2.x, d2.width), (5, 10));

        // Clipped to left 1px column -> BlitV with left_alpha
        let d3 = clip_rect_blit_anti_rect(14, 1, 5, 5, 100, 200, clip);
        assert_eq!(d3.kind, 2);
        assert_eq!((d3.x, d3.left_alpha), (14, 100));

        // Clipped to right 1px column -> BlitV with right_alpha
        let d4 = clip_rect_blit_anti_rect(0, 1, 4, 5, 100, 200, clip);
        assert_eq!(d4.kind, 2);
        assert_eq!((d4.x, d4.left_alpha), (5, 200));
    }

    #[test]
    fn sprite_helpers() {
        assert!(sprite_memcpy_supports(
            ColorType::Rgba8888,
            ColorType::Rgba8888,
            false,
            false,
            false,
            0xFF,
            Some(BlendMode::Src),
            false
        ));
        assert!(!sprite_memcpy_supports(
            ColorType::Rgba8888,
            ColorType::Rgba8888,
            false,
            false,
            false,
            0xFF,
            Some(BlendMode::SrcOver),
            false
        ));
        assert!(sprite_memcpy_supports(
            ColorType::Rgba8888,
            ColorType::Rgba8888,
            false,
            false,
            false,
            0xFF,
            Some(BlendMode::SrcOver),
            true
        ));
        assert_eq!(sprite_d32_s32_flags(255, true), 0);
        assert_eq!(sprite_d32_s32_flags(128, true), GLOBAL_ALPHA_FLAG32);
        assert_eq!(sprite_d32_s32_flags(255, false), SRC_PIXEL_ALPHA_FLAG32);
        assert_eq!(
            sprite_d32_s32_flags(128, false),
            GLOBAL_ALPHA_FLAG32 | SRC_PIXEL_ALPHA_FLAG32
        );
    }
}
