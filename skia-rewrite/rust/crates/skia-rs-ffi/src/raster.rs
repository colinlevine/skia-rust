//! FFI exports for `skia-rs-raster` (Layer 3.3).

use std::slice;

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::rect::{IRect, Rect};
use skia_rs_raster::{
    blit_mask,
    blit_row,
    blitter::{
        self, AntiRectDispatch, BwMaskRowParams, ClipAntiSpanPlan, FatAntiRectParams,
    },
    blitter_a8::{self, A8BlendOp},
    blitter_argb32,
    memset,
    raster_pipeline::{self, ConstantColorClassification, MaskPlaneAddress},
};

// ============================================================================
// SkMemset_opts.cpp
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_memset16(dst: *mut u16, value: u16, count: i32) {
    if count <= 0 || dst.is_null() {
        return;
    }
    memset::memset16(slice::from_raw_parts_mut(dst, count as usize), value);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_memset32(dst: *mut u32, value: u32, count: i32) {
    if count <= 0 || dst.is_null() {
        return;
    }
    memset::memset32(slice::from_raw_parts_mut(dst, count as usize), value);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_memset64(dst: *mut u64, value: u64, count: i32) {
    if count <= 0 || dst.is_null() {
        return;
    }
    memset::memset64(slice::from_raw_parts_mut(dst, count as usize), value);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_memset16(
    dst: *mut u16,
    value: u16,
    count: i32,
    row_bytes: usize,
    height: i32,
) {
    if count <= 0 || height <= 0 || dst.is_null() {
        return;
    }
    let mut row = dst as *mut u8;
    let n = count as usize;
    for _ in 0..height {
        memset::memset16(slice::from_raw_parts_mut(row as *mut u16, n), value);
        row = row.add(row_bytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_memset32(
    dst: *mut u32,
    value: u32,
    count: i32,
    row_bytes: usize,
    height: i32,
) {
    if count <= 0 || height <= 0 || dst.is_null() {
        return;
    }
    let mut row = dst as *mut u8;
    let n = count as usize;
    for _ in 0..height {
        memset::memset32(slice::from_raw_parts_mut(row as *mut u32, n), value);
        row = row.add(row_bytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_memset64(
    dst: *mut u64,
    value: u64,
    count: i32,
    row_bytes: usize,
    height: i32,
) {
    if count <= 0 || height <= 0 || dst.is_null() {
        return;
    }
    let mut row = dst as *mut u8;
    let n = count as usize;
    for _ in 0..height {
        memset::memset64(slice::from_raw_parts_mut(row as *mut u64, n), value);
        row = row.add(row_bytes);
    }
}

// ============================================================================
// SkBlitMask_opts.cpp
// ============================================================================

#[inline]
unsafe fn blit_mask_d32_a8_impl(
    dst: *mut u32,
    dst_rb: usize,
    mask: *const u8,
    mask_rb: usize,
    color: u32,
    w: i32,
    h: i32,
    is_bgra: bool,
) {
    if w <= 0 || h <= 0 || dst.is_null() || mask.is_null() {
        return;
    }
    let mode = blit_mask::blit_mask_mode(color, is_bgra);
    let width = w as usize;
    let mut dst_row = dst as *mut u8;
    let mut mask_row = mask;
    for _ in 0..h {
        let d = slice::from_raw_parts_mut(dst_row as *mut u32, width);
        let m = slice::from_raw_parts(mask_row, width);
        blit_mask::blit_mask_d32_a8_row(d, m, mode);
        dst_row = dst_row.add(dst_rb);
        mask_row = mask_row.add(mask_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_mask_d32_a8_rgba(
    dst: *mut u32,
    dst_rb: usize,
    mask: *const u8,
    mask_rb: usize,
    color: u32,
    w: i32,
    h: i32,
) {
    blit_mask_d32_a8_impl(dst, dst_rb, mask, mask_rb, color, w, h, false);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_mask_d32_a8_bgra(
    dst: *mut u32,
    dst_rb: usize,
    mask: *const u8,
    mask_rb: usize,
    color: u32,
    w: i32,
    h: i32,
) {
    blit_mask_d32_a8_impl(dst, dst_rb, mask, mask_rb, color, w, h, true);
}

// ============================================================================
// SkBlitRow_opts.cpp & SkBlitRow_D32.cpp
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_row_color32(dst: *mut u32, count: i32, color: u32) {
    if count <= 0 || dst.is_null() {
        return;
    }
    blit_row::blit_row_color32(slice::from_raw_parts_mut(dst, count as usize), color);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_row_s32_opaque(
    dst: *mut u32,
    src: *const u32,
    count: i32,
    _alpha: u32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    blit_row::blit_row_s32_opaque(
        slice::from_raw_parts_mut(dst, count as usize),
        slice::from_raw_parts(src, count as usize),
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_row_s32_blend(
    dst: *mut u32,
    src: *const u32,
    count: i32,
    alpha: u32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    blit_row::blit_row_s32_blend(
        slice::from_raw_parts_mut(dst, count as usize),
        slice::from_raw_parts(src, count as usize),
        alpha,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_row_s32a_opaque(
    dst: *mut u32,
    src: *const u32,
    count: i32,
    _alpha: u32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    blit_row::blit_row_s32a_opaque(
        slice::from_raw_parts_mut(dst, count as usize),
        slice::from_raw_parts(src, count as usize),
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blit_row_s32a_blend(
    dst: *mut u32,
    src: *const u32,
    count: i32,
    alpha: u32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    blit_row::blit_row_s32a_blend(
        slice::from_raw_parts_mut(dst, count as usize),
        slice::from_raw_parts(src, count as usize),
        alpha,
    );
}

// ============================================================================
// SkBlitter_A8.cpp
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_choose_blitter(
    color_type: i32,
    has_shader: bool,
    has_color_filter: bool,
    has_clip_shader: bool,
    draw_coverage: bool,
    has_blend_mode: bool,
    blend_mode: i32,
) -> i32 {
    let ct = match ColorType::from_i32(color_type) {
        Some(ct) => ct,
        None => return 0,
    };
    let bm = if has_blend_mode {
        BlendMode::from_i32(blend_mode)
    } else {
        None
    };
    blitter_a8::choose_a8_blitter(
        ct,
        has_shader,
        has_color_filter,
        has_clip_shader,
        draw_coverage,
        bm,
    ) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_coverage_blit_h(dst: *mut u8, width: i32) {
    if width <= 0 || dst.is_null() {
        return;
    }
    blitter_a8::a8_coverage_blit_h(slice::from_raw_parts_mut(dst, width as usize));
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_coverage_blit_anti_h(
    mut dst: *mut u8,
    mut aa: *const u8,
    mut runs: *const i16,
) {
    if dst.is_null() || aa.is_null() || runs.is_null() {
        return;
    }
    loop {
        let count = *runs;
        if count <= 0 {
            return;
        }
        let n = count as usize;
        let alpha = *aa;
        blitter_a8::a8_coverage_blit_anti_run(slice::from_raw_parts_mut(dst, n), alpha);
        runs = runs.add(n);
        aa = aa.add(n);
        dst = dst.add(n);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_coverage_blit_v(
    dst: *mut u8,
    dst_rb: usize,
    height: i32,
    alpha: u8,
) {
    if alpha == 0 || height <= 0 || dst.is_null() {
        return;
    }
    let mut row = dst;
    for _ in 0..height {
        *row = alpha;
        row = row.add(dst_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_coverage_blit_rect(
    dst: *mut u8,
    dst_rb: usize,
    width: i32,
    height: i32,
) {
    if width <= 0 || height <= 0 || dst.is_null() {
        return;
    }
    let w = width as usize;
    let mut row = dst;
    for _ in 0..height {
        blitter_a8::a8_coverage_blit_h(slice::from_raw_parts_mut(row, w));
        row = row.add(dst_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_coverage_blit_mask_a8(
    dst: *mut u8,
    dst_rb: usize,
    mask: *const u8,
    mask_rb: usize,
    width: i32,
    height: i32,
) {
    if width <= 0 || height <= 0 || dst.is_null() || mask.is_null() {
        return;
    }
    let w = width as usize;
    let mut dst_row = dst;
    let mut mask_row = mask;
    for _ in 0..height {
        slice::from_raw_parts_mut(dst_row, w)
            .copy_from_slice(slice::from_raw_parts(mask_row, w));
        dst_row = dst_row.add(dst_rb);
        mask_row = mask_row.add(mask_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_blit_h(dst: *mut u8, width: i32, src: u8, op_kind: i32) {
    if width <= 0 || dst.is_null() {
        return;
    }
    if let Some(op) = A8BlendOp::from_i32(op_kind) {
        blitter_a8::a8_row_bw(slice::from_raw_parts_mut(dst, width as usize), src, op);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_blit_anti_h(
    mut dst: *mut u8,
    mut aa: *const u8,
    mut runs: *const i16,
    src: u8,
    op_kind: i32,
) {
    if dst.is_null() || aa.is_null() || runs.is_null() {
        return;
    }
    let op = match A8BlendOp::from_i32(op_kind) {
        Some(op) => op,
        None => return,
    };
    loop {
        let count = *runs;
        if count <= 0 {
            return;
        }
        let n = count as usize;
        let alpha = *aa;
        blitter_a8::a8_blit_anti_run(slice::from_raw_parts_mut(dst, n), src, alpha, op);
        runs = runs.add(n);
        aa = aa.add(n);
        dst = dst.add(n);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_blit_v(
    dst: *mut u8,
    dst_rb: usize,
    height: i32,
    src: u8,
    aa: u8,
    op_kind: i32,
) {
    if aa == 0 || height <= 0 || dst.is_null() {
        return;
    }
    let op = match A8BlendOp::from_i32(op_kind) {
        Some(op) => op,
        None => return,
    };
    let mut row = dst;
    if aa == 0xFF {
        for _ in 0..height {
            *row = op.blend_one(src, *row);
            row = row.add(dst_rb);
        }
    } else {
        for _ in 0..height {
            blitter_a8::a8_row_aa(slice::from_raw_parts_mut(row, 1), src, aa, op);
            row = row.add(dst_rb);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_blit_rect(
    dst: *mut u8,
    dst_rb: usize,
    width: i32,
    height: i32,
    src: u8,
    op_kind: i32,
) {
    if width <= 0 || height <= 0 || dst.is_null() {
        return;
    }
    let op = match A8BlendOp::from_i32(op_kind) {
        Some(op) => op,
        None => return,
    };
    let w = width as usize;
    let mut row = dst;
    for _ in 0..height {
        blitter_a8::a8_row_bw(slice::from_raw_parts_mut(row, w), src, op);
        row = row.add(dst_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_a8_blit_mask_a8(
    dst: *mut u8,
    dst_rb: usize,
    mask: *const u8,
    mask_rb: usize,
    width: i32,
    height: i32,
    src: u8,
    op_kind: i32,
) {
    if width <= 0 || height <= 0 || dst.is_null() || mask.is_null() {
        return;
    }
    let op = match A8BlendOp::from_i32(op_kind) {
        Some(op) => op,
        None => return,
    };
    let w = width as usize;
    let mut dst_row = dst;
    let mut mask_row = mask;
    for _ in 0..height {
        blitter_a8::a8_blit_mask_row(
            slice::from_raw_parts_mut(dst_row, w),
            slice::from_raw_parts(mask_row, w),
            src,
            op,
        );
        dst_row = dst_row.add(dst_rb);
        mask_row = mask_row.add(mask_rb);
    }
}

// ============================================================================
// SkBlitter_ARGB32.cpp
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_anti_h(
    mut dst: *mut u32,
    mut aa: *const u8,
    mut runs: *const i16,
    pm_color: u32,
    mode: i32, // 0 = general, 1 = opaque, 2 = black
) {
    if dst.is_null() || aa.is_null() || runs.is_null() {
        return;
    }
    loop {
        let count = *runs;
        if count <= 0 {
            return;
        }
        let n = count as usize;
        let alpha = *aa;
        let slice_dst = slice::from_raw_parts_mut(dst, n);
        match mode {
            0 => blitter_argb32::argb32_blit_anti_run(slice_dst, pm_color, alpha),
            1 => blitter_argb32::argb32_opaque_blit_anti_run(slice_dst, pm_color, alpha),
            2 => blitter_argb32::argb32_black_blit_anti_run(slice_dst, alpha),
            _ => {}
        }
        runs = runs.add(n);
        aa = aa.add(n);
        dst = dst.add(n);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_anti_h2(
    dst: *mut u32,
    pm_color: u32,
    a0: u32,
    a1: u32,
    mode: i32,
) {
    if dst.is_null() {
        return;
    }
    let s = slice::from_raw_parts_mut(dst, 2);
    match mode {
        0 => {
            s[0] = blitter_argb32::argb32_blend_pixel_aa(pm_color, s[0], a0);
            s[1] = blitter_argb32::argb32_blend_pixel_aa(pm_color, s[1], a1);
        }
        1 => {
            s[0] = blitter_argb32::argb32_opaque_blend_pixel_aa(pm_color, s[0], a0);
            s[1] = blitter_argb32::argb32_opaque_blend_pixel_aa(pm_color, s[1], a1);
        }
        2 => {
            s[0] = blitter_argb32::argb32_black_blend_pixel_aa(s[0], a0);
            s[1] = blitter_argb32::argb32_black_blend_pixel_aa(s[1], a1);
        }
        _ => {}
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_anti_v2(
    dst: *mut u32,
    dst_rb: usize,
    pm_color: u32,
    a0: u32,
    a1: u32,
    mode: i32,
) {
    if dst.is_null() {
        return;
    }
    let row0 = dst;
    let row1 = (dst as *mut u8).add(dst_rb) as *mut u32;
    match mode {
        0 => {
            *row0 = blitter_argb32::argb32_blend_pixel_aa(pm_color, *row0, a0);
            *row1 = blitter_argb32::argb32_blend_pixel_aa(pm_color, *row1, a1);
        }
        1 => {
            *row0 = blitter_argb32::argb32_opaque_blend_pixel_aa(pm_color, *row0, a0);
            *row1 = blitter_argb32::argb32_opaque_blend_pixel_aa(pm_color, *row1, a1);
        }
        2 => {
            *row0 = blitter_argb32::argb32_black_blend_pixel_aa(*row0, a0);
            *row1 = blitter_argb32::argb32_black_blend_pixel_aa(*row1, a1);
        }
        _ => {}
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_v(
    dst: *mut u32,
    dst_rb: usize,
    height: i32,
    pm_color: u32,
    alpha: u8,
) {
    if height <= 0 || alpha == 0 || dst.is_null() {
        return;
    }
    let (color, dst_scale) = blitter_argb32::argb32_blit_v_params(pm_color, alpha);
    let mut row = dst as *mut u8;
    for _ in 0..height {
        let px = row as *mut u32;
        *px = blitter_argb32::argb32_blit_v_pixel(*px, color, dst_scale);
        row = row.add(dst_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_rect(
    dst: *mut u32,
    dst_rb: usize,
    width: i32,
    height: i32,
    pm_color: u32,
) {
    if width <= 0 || height <= 0 || dst.is_null() {
        return;
    }
    let w = width as usize;
    let mut row = dst as *mut u8;
    for _ in 0..height {
        blit_row::color32(slice::from_raw_parts_mut(row as *mut u32, w), pm_color);
        row = row.add(dst_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_mask_lcd16(
    dst: *mut u32,
    dst_rb: usize,
    mask: *const u16,
    mask_rb: usize,
    color: u32,
    width: i32,
    height: i32,
    is_bgra: bool,
) {
    if width <= 0 || height <= 0 || dst.is_null() || mask.is_null() {
        return;
    }
    let w = width as usize;
    let mut dst_row = dst as *mut u8;
    let mut mask_row = mask as *const u8;
    for _ in 0..height {
        blitter_argb32::blit_row_lcd16_auto(
            slice::from_raw_parts_mut(dst_row as *mut u32, w),
            slice::from_raw_parts(mask_row as *const u16, w),
            color,
            is_bgra,
        );
        dst_row = dst_row.add(dst_rb);
        mask_row = mask_row.add(mask_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_mask_bw(
    dst: *mut u32,
    dst_rb: usize,
    bits: *const u8,
    bits_rb: usize,
    bit_offset: i32,
    width: i32,
    height: i32,
    pm_color: u32,
    opaque: bool,
) {
    if width <= 0 || height <= 0 || dst.is_null() || bits.is_null() {
        return;
    }
    let w = width as usize;
    let off = (bit_offset & 7) as usize;
    let bytes_needed = (off + w + 7) >> 3;
    let dst_scale = blit_row::alpha_255_to_256(255 - (pm_color >> 24));
    let mut dst_row = dst as *mut u8;
    let mut bits_row = bits;
    for _ in 0..height {
        let d = slice::from_raw_parts_mut(dst_row as *mut u32, w);
        let b = slice::from_raw_parts(bits_row, bytes_needed);
        if opaque {
            blitter_argb32::blit_bw_row_solid(d, b, off, pm_color);
        } else {
            blitter_argb32::blit_bw_row_blend(d, b, off, pm_color, dst_scale);
        }
        dst_row = dst_row.add(dst_rb);
        bits_row = bits_row.add(bits_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_blit_mask_argb32(
    dst: *mut u32,
    dst_rb: usize,
    src: *const u32,
    src_rb: usize,
    width: i32,
    height: i32,
    src_color: u32,
) {
    if width <= 0 || height <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let w = width as usize;
    let mut dst_row = dst as *mut u8;
    let mut src_row = src as *const u8;
    for _ in 0..height {
        blitter_argb32::blit_mask_argb32_row(
            slice::from_raw_parts_mut(dst_row as *mut u32, w),
            slice::from_raw_parts(src_row as *const u32, w),
            src_color,
        );
        dst_row = dst_row.add(dst_rb);
        src_row = src_row.add(src_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_shader_blend_row_a8(
    dst: *mut u32,
    mask: *const u8,
    src: *const u32,
    n: i32,
    opaque: bool,
) {
    if n <= 0 || dst.is_null() || mask.is_null() || src.is_null() {
        return;
    }
    let len = n as usize;
    let d = slice::from_raw_parts_mut(dst, len);
    let m = slice::from_raw_parts(mask, len);
    let s = slice::from_raw_parts(src, len);
    if opaque {
        blitter_argb32::shader_blend_row_a8_opaque(d, m, s);
    } else {
        blitter_argb32::shader_blend_row_a8(d, m, s);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_shader_blend_row_lcd16(
    dst: *mut u32,
    mask: *const u16,
    src: *const u32,
    n: i32,
    opaque: bool,
    is_bgra: bool,
) {
    if n <= 0 || dst.is_null() || mask.is_null() || src.is_null() {
        return;
    }
    let len = n as usize;
    let d = slice::from_raw_parts_mut(dst, len);
    let m = slice::from_raw_parts(mask, len);
    let s = slice::from_raw_parts(src, len);
    if opaque {
        blitter_argb32::shader_blend_row_lcd16_opaque(d, m, s, is_bgra);
    } else {
        blitter_argb32::shader_blend_row_lcd16(d, m, s, is_bgra);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_argb32_shader_blit_v_interp(
    src: u32,
    dst: u32,
    alpha: u32,
) -> u32 {
    blitter_argb32::shader_blit_v_interp(src, dst, alpha)
}

// ============================================================================
// SkBlitter.cpp, SkBlitter_Sprite.cpp, SkSpriteBlitter_ARGB32.cpp
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_compute_fat_anti_rect(
    rect: Rect,
    out: *mut FatAntiRectParams,
) -> bool {
    if out.is_null() {
        return false;
    }
    match blitter::compute_fat_anti_rect(rect) {
        Some(p) => {
            *out = p;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_bw_mask_row_params(
    clip_left: i32,
    clip_right: i32,
    mask_left: i32,
    mask_right: i32,
) -> BwMaskRowParams {
    blitter::compute_bw_mask_row_params(clip_left, clip_right, mask_left, mask_right)
}

pub type BwRunCallback = unsafe extern "C" fn(ctx: *mut std::ffi::c_void, x: i32, width: i32);

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_bw_bits_to_runs(
    x: i32,
    bits: *const u8,
    row_bytes: usize,
    left_mask: u8,
    right_mask: u8,
    cb: BwRunCallback,
    ctx: *mut std::ffi::c_void,
) {
    if bits.is_null() || row_bytes == 0 {
        return;
    }
    let slice_bits = slice::from_raw_parts(bits, row_bytes);
    blitter::bw_bits_to_runs(x, slice_bits, left_mask, right_mask, |rx, rw| {
        cb(ctx, rx, rw);
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_clip_anti_span(
    x: *mut i32,
    y: i32,
    aa: *mut *mut u8,
    runs: *mut *mut i16,
    clip: IRect,
    trim_right: bool,
) -> i32 {
    if x.is_null() || aa.is_null() || runs.is_null() {
        return 0;
    }
    let in_x = *x;
    if y < clip.top || y >= clip.bottom || in_x >= clip.right {
        return 0;
    }
    let runs_ptr = *runs;
    let aa_ptr = *aa;
    if runs_ptr.is_null() || aa_ptr.is_null() {
        return 0;
    }

    // Walk sparse zero-terminated runs to determine span width.
    let mut width: i32 = 0;
    let mut cur = runs_ptr;
    loop {
        let count = *cur as i32;
        if count <= 0 {
            break;
        }
        width += count;
        cur = cur.add(count as usize);
    }
    if width <= 0 {
        return 0;
    }

    let plan: ClipAntiSpanPlan = blitter::plan_clip_anti_span(in_x, y, width, clip, trim_right);
    if plan.width <= 0 {
        return 0;
    }

    if plan.need_break_left || plan.need_break_right {
        let total = width as usize;
        let runs_slice = slice::from_raw_parts_mut(runs_ptr, total + 1);
        let aa_slice = slice::from_raw_parts_mut(aa_ptr, total);
        let dx = plan.dx as usize;
        if plan.need_break_left {
            blitter::alpha_runs_break_at(runs_slice, aa_slice, dx);
        }
        if plan.need_break_right {
            let w = plan.width as usize;
            blitter::alpha_runs_break_at(&mut runs_slice[dx..], &mut aa_slice[dx..], w);
            runs_slice[dx + w] = 0;
        }
    }

    *x = plan.new_x;
    *runs = runs_ptr.add(plan.dx as usize);
    *aa = aa_ptr.add(plan.dx as usize);
    plan.width
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_alpha_runs_break_at(
    runs: *mut i16,
    aa: *mut u8,
    x: i32,
) {
    if x <= 0 || runs.is_null() || aa.is_null() {
        return;
    }
    // Walk until we pass x to find exact slice bound without reading past terminator.
    let mut pos = 0usize;
    let target = x as usize;
    while pos < target {
        let n = *runs.add(pos) as usize;
        if n == 0 {
            return;
        }
        pos += n;
    }
    let runs_slice = slice::from_raw_parts_mut(runs, pos + 1);
    let aa_slice = slice::from_raw_parts_mut(aa, pos);
    blitter::alpha_runs_break_at(runs_slice, aa_slice, target);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_alpha_runs_break(
    runs: *mut i16,
    aa: *mut u8,
    x: i32,
    count: i32,
) {
    if count <= 0 || x < 0 || runs.is_null() || aa.is_null() {
        return;
    }
    skia_rs_blitter_alpha_runs_break_at(runs, aa, x);
    skia_rs_blitter_alpha_runs_break_at(runs.add(x as usize), aa.add(x as usize), count);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_write_rgn_gap(
    runs: *mut i16,
    aa: *mut u8,
    offset: i32,
    gap: i32,
) {
    if offset < 0 || gap <= 0 || runs.is_null() || aa.is_null() {
        return;
    }
    let off = offset as usize;
    let g = gap as usize;
    let runs_slice = slice::from_raw_parts_mut(runs.add(off), g);
    let aa_slice = slice::from_raw_parts_mut(aa.add(off), g);
    blitter::write_rgn_clip_gap(runs_slice, aa_slice, 0, g);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_clip_h(
    left: i32,
    y: i32,
    width: i32,
    clip: IRect,
    out_left: *mut i32,
    out_width: *mut i32,
) -> bool {
    match blitter::clip_blit_h(left, y, width, clip) {
        Some((l, w)) => {
            *out_left = l;
            *out_width = w;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_clip_v(
    x: i32,
    y: i32,
    height: i32,
    clip: IRect,
    out_y: *mut i32,
    out_height: *mut i32,
) -> bool {
    match blitter::clip_blit_v(x, y, height, clip) {
        Some((ry, rh)) => {
            *out_y = ry;
            *out_height = rh;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_clip_rect_anti_rect(
    left: i32,
    y: i32,
    width: i32,
    height: i32,
    left_alpha: u8,
    right_alpha: u8,
    clip: IRect,
) -> AntiRectDispatch {
    blitter::clip_rect_blit_anti_rect(left, y, width, height, left_alpha, right_alpha, clip)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_dispatch_anti_rect_piece(
    orig_x: i32,
    orig_width: i32,
    piece: IRect,
    left_alpha: u8,
    right_alpha: u8,
) -> AntiRectDispatch {
    blitter::dispatch_clipped_anti_rect_piece(orig_x, orig_width, piece, left_alpha, right_alpha)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_choose_clip_kind(
    clip_is_empty: bool,
    clip_is_rect: bool,
    clip_bounds: IRect,
    ir: *const IRect,
) -> i32 {
    let opt_ir = if ir.is_null() { None } else { Some(*ir) };
    blitter::choose_blitter_clip_kind(clip_is_empty, clip_is_rect, clip_bounds, opt_ir) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blitter_use_legacy(
    force_raster_pipeline: bool,
    is_dither: bool,
    dst_alpha_type: i32,
    is_src_over: bool,
    has_3d_mask_filter: bool,
    has_color_space: bool,
    is_srgb_color_space: bool,
    has_shader: bool,
    color4f_fits_in_bytes: bool,
    is_n32: bool,
) -> bool {
    let at = AlphaType::from_i32(dst_alpha_type).unwrap_or(AlphaType::Unknown);
    blitter::use_legacy_blitter(
        force_raster_pipeline,
        is_dither,
        at,
        is_src_over,
        has_3d_mask_filter,
        has_color_space,
        is_srgb_color_space,
        has_shader,
        color4f_fits_in_bytes,
        is_n32,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sprite_memcpy_supports(
    dst_ct: i32,
    src_ct: i32,
    has_mask_filter: bool,
    has_color_filter: bool,
    has_image_filter: bool,
    paint_alpha: u8,
    has_blend_mode: bool,
    blend_mode: i32,
    src_is_opaque: bool,
) -> bool {
    let d_ct = match ColorType::from_i32(dst_ct) {
        Some(ct) => ct,
        None => return false,
    };
    let s_ct = match ColorType::from_i32(src_ct) {
        Some(ct) => ct,
        None => return false,
    };
    let bm = if has_blend_mode {
        BlendMode::from_i32(blend_mode)
    } else {
        None
    };
    blitter::sprite_memcpy_supports(
        d_ct,
        s_ct,
        has_mask_filter,
        has_color_filter,
        has_image_filter,
        paint_alpha,
        bm,
        src_is_opaque,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sprite_memcpy_blit_rect(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    bytes_to_copy: usize,
    height: i32,
) {
    if bytes_to_copy == 0 || height <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let mut dst_row = dst;
    let mut src_row = src;
    for _ in 0..height {
        slice::from_raw_parts_mut(dst_row, bytes_to_copy)
            .copy_from_slice(slice::from_raw_parts(src_row, bytes_to_copy));
        dst_row = dst_row.add(dst_rb);
        src_row = src_row.add(src_rb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sprite_choose_l32_supports(
    has_color_filter: bool,
    has_mask_filter: bool,
    src_is_n32: bool,
    is_src_over: bool,
) -> bool {
    blitter::sprite_choose_l32_supports(has_color_filter, has_mask_filter, src_is_n32, is_src_over)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sprite_d32_s32_flags(alpha: u8, src_is_opaque: bool) -> u32 {
    blitter::sprite_d32_s32_flags(alpha, src_is_opaque)
}

// ============================================================================
// SkRasterPipeline.cpp & SkRasterPipelineBlitter.cpp
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_is_bounded_rgb(rgb: *const f32) -> bool {
    if rgb.is_null() {
        return false;
    }
    let s = slice::from_raw_parts(rgb, 3);
    raster_pipeline::is_bounded_rgb([s[0], s[1], s[2]])
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_classify_constant_color(
    rgba: *const f32,
) -> ConstantColorClassification {
    if rgba.is_null() {
        return ConstantColorClassification {
            kind: 3,
            rgba16: [0; 4],
        };
    }
    let s = slice::from_raw_parts(rgba, 4);
    raster_pipeline::classify_constant_color([s[0], s[1], s[2], s[3]])
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_classify_matrix(
    type_mask: u32,
    has_perspective: bool,
) -> i32 {
    raster_pipeline::classify_matrix_stage(type_mask, has_perspective) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_classify_transfer_fn(
    tf_type: i32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
) -> i32 {
    raster_pipeline::classify_transfer_function(tf_type, a, b, c, d, e, f) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_color_type_is_normalized(ct: i32) -> bool {
    ColorType::from_i32(ct)
        .map(raster_pipeline::color_type_is_normalized)
        .unwrap_or(false)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_stages_needed(num_stages: i32, has_rewind_ctx: bool) -> i32 {
    raster_pipeline::stages_needed(num_stages, has_rewind_ctx)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_can_direct_blit(
    has_shader: bool,
    has_color_filter: bool,
    is_dither: bool,
    has_blend_mode: bool,
    blend_mode: i32,
    alphaf: f32,
) -> bool {
    let bm = if has_blend_mode {
        BlendMode::from_i32(blend_mode)
    } else {
        None
    };
    raster_pipeline::can_direct_blit(has_shader, has_color_filter, is_dither, bm, alphaf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_dither_rate(ct: i32) -> f32 {
    ColorType::from_i32(ct)
        .map(raster_pipeline::dither_rate_for_color_type)
        .unwrap_or(0.0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_can_use_srcover_8888(
    has_blend_mode: bool,
    blend_mode: i32,
    dst_ct: i32,
    has_dst_cs: bool,
    dst_at: i32,
    dither_rate: f32,
) -> bool {
    let bm = if has_blend_mode {
        BlendMode::from_i32(blend_mode)
    } else {
        None
    };
    let ct = match ColorType::from_i32(dst_ct) {
        Some(ct) => ct,
        None => return false,
    };
    let at = AlphaType::from_i32(dst_at).unwrap_or(AlphaType::Unknown);
    raster_pipeline::can_use_srcover_8888_fast_path(bm, ct, has_dst_cs, at, dither_rate)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_should_prescale_coverage(
    has_blend_mode: bool,
    blend_mode: i32,
    rgb_coverage: bool,
) -> bool {
    let bm = if has_blend_mode {
        BlendMode::from_i32(blend_mode)
    } else {
        None
    };
    raster_pipeline::rp_blitter_should_prescale_coverage(bm, rgb_coverage)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_mask_plane_address(
    bounds_left: i32,
    bounds_top: i32,
    row_bytes: usize,
    plane: i32,
    image_size: usize,
    is_lcd16: bool,
) -> MaskPlaneAddress {
    raster_pipeline::compute_mask_plane_address(
        bounds_left,
        bounds_top,
        row_bytes,
        plane,
        image_size,
        is_lcd16,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rp_can_use_memset2d(
    is_constant: bool,
    has_blend_mode: bool,
    blend_mode: i32,
    bytes_per_pixel: i32,
) -> bool {
    let bm = if has_blend_mode {
        BlendMode::from_i32(blend_mode)
    } else {
        None
    };
    raster_pipeline::can_use_memset2d(is_constant, bm, bytes_per_pixel)
}
