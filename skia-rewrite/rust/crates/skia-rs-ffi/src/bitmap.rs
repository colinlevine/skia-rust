//! FFI exports for `skia-rs-bitmap` (Layer 3.4a).
//!
//! Covers:
//! - `SkPixmap.cpp`, `SkPixmapDraw.cpp`
//! - `SkPixelRef.cpp`, `SkMallocPixelRef.cpp`, `SkBitmapCache.cpp`
//! - `SkBitmap.cpp`
//! - `SkBitmapProcState.cpp`, `SkBitmapProcState_matrixProcs.cpp`, `SkBitmapProcState_opts.cpp`
//! - `SkMipmap.cpp`, `SkMipmapAccessor.cpp`, `SkMipmapBuilder.cpp`,
//!   `SkMipmapDrawDownSampler.cpp`, `SkMipmapHQDownSampler.cpp`
//! - `SkSpecialImage.cpp`

use skia_rs_bitmap::{
    bitmap::{
        self, BitmapExtractSubsetResult, BitmapSetInfoResult, ExtractAlphaMaskSetup,
    },
    bitmap_proc_state::{
        self, ChooseProcsResult, MatrixProcKind, ProcStateParams, TileMode,
    },
    mipmap::{self, MipmapAccessorPlan, MipmapMode},
    pixel_ref::{self, MallocAllocPlan},
    pixmap::{self, PixmapSubsetResult, ScalePixelsPlan},
    special_image,
};
use skia_rs_color::{
    color::Color4f,
    image_info::{color_type_bytes_per_pixel, AlphaType, ColorType},
};
use skia_rs_geometry::{IPoint, IRect, ISize, Matrix, Rect, Size};

#[inline]
fn pixmap_byte_len(width: i32, height: i32, row_bytes: usize, bpp: usize) -> usize {
    if width <= 0 || height <= 0 || bpp == 0 {
        return 0;
    }
    ((height as usize) - 1)
        .saturating_mul(row_bytes)
        .saturating_add((width as usize).saturating_mul(bpp))
}

#[inline]
unsafe fn slice_from_raw<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}

#[inline]
unsafe fn slice_from_raw_mut<'a, T>(ptr: *mut T, len: usize) -> &'a mut [T] {
    if ptr.is_null() || len == 0 {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(ptr, len)
    }
}

// ============================================================================
// SkPixmap & SkPixmapDraw
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_extract_subset(
    width: i32,
    height: i32,
    ct_raw: i32,
    row_bytes: usize,
    subset: *const IRect,
    out: *mut PixmapSubsetResult,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    if subset.is_null() || out.is_null() {
        return false;
    }
    match pixmap::pixmap_extract_subset(width, height, ct, row_bytes, *subset) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_get_alphaf(ct_raw: i32, pixel: *const u8) -> f32 {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return 0.0;
    };
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let slice = slice_from_raw(pixel, bpp);
    pixmap::pixmap_get_alphaf(ct, slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_get_color(
    ct_raw: i32,
    at_raw: i32,
    pixel: *const u8,
    is_bgra: bool,
) -> u32 {
    let (Some(ct), Some(at)) = (ColorType::from_i32(ct_raw), AlphaType::from_i32(at_raw)) else {
        return 0;
    };
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let slice = slice_from_raw(pixel, bpp);
    pixmap::pixmap_get_color(ct, at, slice, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_get_color4f(
    ct_raw: i32,
    at_raw: i32,
    pixel: *const u8,
    is_bgra: bool,
) -> Color4f {
    let (Some(ct), Some(at)) = (ColorType::from_i32(ct_raw), AlphaType::from_i32(at_raw)) else {
        return Color4f::new(0.0, 0.0, 0.0, 0.0);
    };
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let slice = slice_from_raw(pixel, bpp);
    pixmap::pixmap_get_color4f(ct, at, slice, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_compute_is_opaque(
    ct_raw: i32,
    width: i32,
    height: i32,
    row_bytes: usize,
    pixels: *const u8,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let len = pixmap_byte_len(width, height, row_bytes, bpp);
    let slice = slice_from_raw(pixels, len);
    pixmap::pixmap_compute_is_opaque(ct, width, height, row_bytes, slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_erase_clip(
    ct_raw: i32,
    width: i32,
    height: i32,
    subset: *const IRect,
    out_clip: *mut IRect,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    if out_clip.is_null() {
        return false;
    }
    let sub_opt = if subset.is_null() {
        None
    } else {
        Some(*subset)
    };
    match pixmap::pixmap_erase_clip(ct, width, height, sub_opt) {
        Some(clip) => {
            *out_clip = clip;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_fill_rect(
    ct_raw: i32,
    width: i32,
    height: i32,
    pixels: *mut u8,
    row_bytes: usize,
    clip: *const IRect,
    dst_pixel: *const u8,
) {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return;
    };
    if pixels.is_null() || clip.is_null() || dst_pixel.is_null() {
        return;
    }
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let len = pixmap_byte_len(width, height, row_bytes, bpp);
    let buf = slice_from_raw_mut(pixels, len);
    let mut pat = [0u8; 16];
    let src_pat = std::slice::from_raw_parts(dst_pixel, 16);
    pat.copy_from_slice(src_pat);
    pixmap::pixmap_fill_rect(ct, buf, row_bytes, *clip, &pat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pixmap_plan_scale(
    src_w: i32,
    src_h: i32,
    src_at_raw: i32,
    dst_w: i32,
    dst_h: i32,
    dst_at_raw: i32,
    out: *mut ScalePixelsPlan,
) {
    if out.is_null() {
        return;
    }
    let src_at = AlphaType::from_i32(src_at_raw).unwrap_or(AlphaType::Unknown);
    let dst_at = AlphaType::from_i32(dst_at_raw).unwrap_or(AlphaType::Unknown);
    *out = pixmap::pixmap_plan_scale(src_w, src_h, src_at, dst_w, dst_h, dst_at);
}

// ============================================================================
// SkPixelRef, SkMallocPixelRef, SkBitmapCache
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_next_image_id() -> u32 {
    pixel_ref::next_image_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_make_resource_cache_shared_id_for_bitmap(
    bitmap_gen_id: u32,
) -> u64 {
    pixel_ref::make_resource_cache_shared_id_for_bitmap(bitmap_gen_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_validate_malloc_allocate(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    fuzzer_limit: bool,
    out: *mut MallocAllocPlan,
) -> bool {
    if out.is_null() {
        return false;
    }
    match pixel_ref::validate_malloc_allocate(width, height, ct_raw, at_raw, row_bytes, fuzzer_limit)
    {
        Some(plan) => {
            *out = plan;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_validate_make_with_data(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    data_size: usize,
) -> bool {
    pixel_ref::validate_make_with_data(width, height, ct_raw, at_raw, row_bytes, data_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_validate_bitmap_cache_alloc(
    width: i32,
    height: i32,
    ct_raw: i32,
    out: *mut MallocAllocPlan,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    match pixel_ref::validate_bitmap_cache_alloc(width, height, ct) {
        Some(plan) => {
            *out = plan;
            true
        }
        None => false,
    }
}

// ============================================================================
// SkBitmap
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_validate_set_info(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    out: *mut BitmapSetInfoResult,
) -> bool {
    let (Some(ct), Some(at)) = (ColorType::from_i32(ct_raw), AlphaType::from_i32(at_raw)) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    match bitmap::bitmap_validate_set_info(width, height, ct, at, row_bytes) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_pixel_ref_origin(
    byte_offset: usize,
    row_bytes: usize,
    ct_raw: i32,
) -> IPoint {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return IPoint { x: 0, y: 0 };
    };
    bitmap::bitmap_pixel_ref_origin(byte_offset, row_bytes, ct)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_addr_offset(
    x: i32,
    y: i32,
    row_bytes: usize,
    ct_raw: i32,
) -> usize {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return 0;
    };
    bitmap::bitmap_addr_offset(x, y, row_bytes, ct)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_pixel_ref_byte_offset(
    dx: i32,
    dy: i32,
    row_bytes: usize,
    ct_raw: i32,
) -> usize {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return 0;
    };
    bitmap::bitmap_pixel_ref_byte_offset(dx, dy, row_bytes, ct)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_extract_subset(
    width: i32,
    height: i32,
    origin: IPoint,
    subset: *const IRect,
    out: *mut BitmapExtractSubsetResult,
) -> bool {
    if subset.is_null() || out.is_null() {
        return false;
    }
    match bitmap::bitmap_extract_subset(width, height, origin, *subset) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_extract_alpha_setup(
    width: i32,
    height: i32,
    out: *mut ExtractAlphaMaskSetup,
) -> bool {
    if out.is_null() {
        return false;
    }
    match bitmap::bitmap_extract_alpha_setup(width, height) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_zero_alpha_buffer(
    alpha: *mut u8,
    width: usize,
    height: usize,
    alpha_row_bytes: usize,
) {
    if alpha.is_null() || width == 0 || height == 0 {
        return;
    }
    let len = (height - 1)
        .saturating_mul(alpha_row_bytes)
        .saturating_add(width);
    let buf = slice_from_raw_mut(alpha, len);
    bitmap::zero_alpha_buffer(buf, width, height, alpha_row_bytes);
}

// ============================================================================
// SkBitmapProcState, matrixProcs, opts
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_init_adjust_matrix_and_bilerp(
    inv_matrix: *mut Matrix,
    width: i32,
    height: i32,
    tile_x_raw: i32,
    tile_y_raw: i32,
    bilerp: *mut bool,
) {
    if inv_matrix.is_null() || bilerp.is_null() {
        return;
    }
    let tile_x = TileMode::from_i32(tile_x_raw).unwrap_or(TileMode::Clamp);
    let tile_y = TileMode::from_i32(tile_y_raw).unwrap_or(TileMode::Clamp);
    bitmap_proc_state::init_adjust_matrix_and_bilerp(
        &mut *inv_matrix,
        width,
        height,
        tile_x,
        tile_y,
        &mut *bilerp,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_choose_procs(
    inv_matrix: *const Matrix,
    ct_raw: i32,
    n32_ct_raw: i32,
    width: i32,
    height: i32,
    tile_x_raw: i32,
    tile_y_raw: i32,
    bilerp: bool,
    paint_alpha: u8,
    filter_one_x: i32,
    filter_one_y: i32,
    out: *mut ChooseProcsResult,
) {
    if inv_matrix.is_null() || out.is_null() {
        return;
    }
    let ct = ColorType::from_i32(ct_raw).unwrap_or(ColorType::Unknown);
    let n32_ct = ColorType::from_i32(n32_ct_raw).unwrap_or(ColorType::Unknown);
    let tile_x = TileMode::from_i32(tile_x_raw).unwrap_or(TileMode::Clamp);
    let tile_y = TileMode::from_i32(tile_y_raw).unwrap_or(TileMode::Clamp);
    *out = bitmap_proc_state::choose_procs(
        &*inv_matrix,
        ct,
        n32_ct,
        width,
        height,
        tile_x,
        tile_y,
        bilerp,
        paint_alpha,
        filter_one_x,
        filter_one_y,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_choose_matrix_proc(
    inv_matrix: *const Matrix,
    width: i32,
    height: i32,
    tile_x_raw: i32,
    tile_y_raw: i32,
    bilerp: bool,
    translate_only_matrix: bool,
    filter_one_x: *mut i32,
    filter_one_y: *mut i32,
) -> i32 {
    if inv_matrix.is_null() || filter_one_x.is_null() || filter_one_y.is_null() {
        return MatrixProcKind::None as i32;
    }
    let tile_x = TileMode::from_i32(tile_x_raw).unwrap_or(TileMode::Clamp);
    let tile_y = TileMode::from_i32(tile_y_raw).unwrap_or(TileMode::Clamp);
    bitmap_proc_state::choose_matrix_proc(
        &*inv_matrix,
        width,
        height,
        tile_x,
        tile_y,
        bilerp,
        translate_only_matrix,
        &mut *filter_one_x,
        &mut *filter_one_y,
    ) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_choose_shader_proc32(
    inv_matrix: *const Matrix,
    ct_raw: i32,
    n32_ct_raw: i32,
    width: i32,
    tile_x_raw: i32,
    tile_y_raw: i32,
    bilerp: bool,
    alpha_scale: u16,
    filter_one_x: *mut i32,
    filter_one_y: *mut i32,
) -> i32 {
    if inv_matrix.is_null() || filter_one_x.is_null() || filter_one_y.is_null() {
        return 0;
    }
    let ct = ColorType::from_i32(ct_raw).unwrap_or(ColorType::Unknown);
    let n32_ct = ColorType::from_i32(n32_ct_raw).unwrap_or(ColorType::Unknown);
    let tile_x = TileMode::from_i32(tile_x_raw).unwrap_or(TileMode::Clamp);
    let tile_y = TileMode::from_i32(tile_y_raw).unwrap_or(TileMode::Clamp);
    bitmap_proc_state::choose_shader_proc32(
        &*inv_matrix,
        ct,
        n32_ct,
        width,
        tile_x,
        tile_y,
        bilerp,
        alpha_scale,
        &mut *filter_one_x,
        &mut *filter_one_y,
    ) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_setup_for_translate(
    inv_matrix: *const Matrix,
    bilerp: bool,
    filter_one_x: i32,
    filter_one_y: i32,
    out_fx: *mut i32,
    out_fy: *mut i32,
) -> bool {
    if inv_matrix.is_null() || out_fx.is_null() || out_fy.is_null() {
        return false;
    }
    match bitmap_proc_state::setup_for_translate(&*inv_matrix, bilerp, filter_one_x, filter_one_y) {
        Some((fx, fy)) => {
            *out_fx = fx;
            *out_fy = fy;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_max_count_for_buffer_size(
    buffer_size: usize,
    is_scale_translate: bool,
    bilerp: bool,
) -> i32 {
    bitmap_proc_state::max_count_for_buffer_size(buffer_size, is_scale_translate, bilerp)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_pack_clamp(f: i32, max: u32) -> u32 {
    bitmap_proc_state::pack_clamp(f, max)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_pack_repeat(f: i32, max: u32, width: usize) -> u32 {
    bitmap_proc_state::pack_repeat(f, max, width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_pack_mirror(f: i32, max: u32, width: usize) -> u32 {
    bitmap_proc_state::pack_mirror(f, max, width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_run_matrix_proc(
    kind_raw: i32,
    s: *const ProcStateParams,
    xy: *mut u32,
    count: i32,
    x: i32,
    y: i32,
) {
    let Some(kind) = MatrixProcKind::from_i32(kind_raw) else {
        return;
    };
    if s.is_null() || xy.is_null() || count <= 0 {
        return;
    }
    let cnt = count as usize;
    let xy_len = match kind {
        MatrixProcKind::None => return,
        MatrixProcKind::ClampNofilterTrans
        | MatrixProcKind::RepeatNofilterTrans
        | MatrixProcKind::MirrorNofilterTrans
        | MatrixProcKind::ClampNofilterScale
        | MatrixProcKind::RepeatNofilterScale
        | MatrixProcKind::MirrorNofilterScale => 1 + (cnt + 1) / 2,
        MatrixProcKind::ClampFilterScale
        | MatrixProcKind::RepeatFilterScale
        | MatrixProcKind::MirrorFilterScale => 1 + cnt,
        MatrixProcKind::ClampNofilterAffine
        | MatrixProcKind::RepeatNofilterAffine
        | MatrixProcKind::MirrorNofilterAffine => cnt,
        MatrixProcKind::ClampFilterAffine
        | MatrixProcKind::RepeatFilterAffine
        | MatrixProcKind::MirrorFilterAffine => 2 * cnt,
    };
    let xy_slice = std::slice::from_raw_parts_mut(xy, xy_len);
    bitmap_proc_state::run_matrix_proc(kind, &*s, xy_slice, count, x, y);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_validate_matrix_proc_output(
    xy: *const u32,
    count: usize,
    width: u32,
    height: u32,
    is_scale_translate: bool,
    bilerp: bool,
) -> bool {
    if xy.is_null() || count == 0 {
        return false;
    }
    let xy_len = if is_scale_translate {
        if bilerp {
            1 + count
        } else {
            1 + (count + 1) / 2
        }
    } else if bilerp {
        2 * count
    } else {
        count
    };
    let xy_slice = std::slice::from_raw_parts(xy, xy_len);
    bitmap_proc_state::validate_matrix_proc_output(
        xy_slice,
        count,
        width,
        height,
        is_scale_translate,
        bilerp,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_s32_alpha_d32_filter_dx(
    pixels: *const u8,
    width: i32,
    height: i32,
    row_bytes: usize,
    alpha_scale: u32,
    xy: *const u32,
    count: i32,
    colors: *mut u32,
) {
    if pixels.is_null() || xy.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let cnt = count as usize;
    let px_len = pixmap_byte_len(width, height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let xy_slice = std::slice::from_raw_parts(xy, 1 + cnt);
    let out = std::slice::from_raw_parts_mut(colors, cnt);
    bitmap_proc_state::s32_alpha_d32_filter_dx(px, row_bytes, alpha_scale, xy_slice, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_s32_alpha_d32_filter_dxdy(
    pixels: *const u8,
    width: i32,
    height: i32,
    row_bytes: usize,
    alpha_scale: u32,
    xy: *const u32,
    count: i32,
    colors: *mut u32,
) {
    if pixels.is_null() || xy.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let cnt = count as usize;
    let px_len = pixmap_byte_len(width, height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let xy_slice = std::slice::from_raw_parts(xy, 2 * cnt);
    let out = std::slice::from_raw_parts_mut(colors, cnt);
    bitmap_proc_state::s32_alpha_d32_filter_dxdy(px, row_bytes, alpha_scale, xy_slice, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_s32_alpha_d32_nofilter_dx(
    pixels: *const u8,
    width: i32,
    height: i32,
    row_bytes: usize,
    alpha_scale: u32,
    xy: *const u32,
    count: i32,
    colors: *mut u32,
) {
    if pixels.is_null() || xy.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let cnt = count as usize;
    let px_len = pixmap_byte_len(width, height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let xy_slice = std::slice::from_raw_parts(xy, 1 + (cnt + 1) / 2);
    let out = std::slice::from_raw_parts_mut(colors, cnt);
    bitmap_proc_state::s32_alpha_d32_nofilter_dx(px, width, row_bytes, alpha_scale, xy_slice, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_s32_alpha_d32_nofilter_dxdy(
    pixels: *const u8,
    width: i32,
    height: i32,
    row_bytes: usize,
    alpha_scale: u32,
    xy: *const u32,
    count: i32,
    colors: *mut u32,
) {
    if pixels.is_null() || xy.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let cnt = count as usize;
    let px_len = pixmap_byte_len(width, height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let xy_slice = std::slice::from_raw_parts(xy, cnt);
    let out = std::slice::from_raw_parts_mut(colors, cnt);
    bitmap_proc_state::s32_alpha_d32_nofilter_dxdy(px, row_bytes, alpha_scale, xy_slice, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_clamp_s32_opaque_d32_nofilter_dx_shaderproc(
    s: *const ProcStateParams,
    pixels: *const u8,
    row_bytes: usize,
    x: i32,
    y: i32,
    dst: *mut u32,
    count: i32,
) {
    if s.is_null() || pixels.is_null() || dst.is_null() || count <= 0 {
        return;
    }
    let params = &*s;
    let px_len = pixmap_byte_len(params.width, params.height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let out = std::slice::from_raw_parts_mut(dst, count as usize);
    bitmap_proc_state::clamp_s32_opaque_d32_nofilter_dx_shaderproc(params, px, row_bytes, x, y, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_clamp_s32_d32_nofilter_trans_shaderproc(
    s: *const ProcStateParams,
    pixels: *const u8,
    row_bytes: usize,
    x: i32,
    y: i32,
    colors: *mut u32,
    count: i32,
) {
    if s.is_null() || pixels.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let params = &*s;
    let px_len = pixmap_byte_len(params.width, params.height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let out = std::slice::from_raw_parts_mut(colors, count as usize);
    bitmap_proc_state::clamp_s32_d32_nofilter_trans_shaderproc(params, px, row_bytes, x, y, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_repeat_s32_d32_nofilter_trans_shaderproc(
    s: *const ProcStateParams,
    pixels: *const u8,
    row_bytes: usize,
    x: i32,
    y: i32,
    colors: *mut u32,
    count: i32,
) {
    if s.is_null() || pixels.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let params = &*s;
    let px_len = pixmap_byte_len(params.width, params.height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let out = std::slice::from_raw_parts_mut(colors, count as usize);
    bitmap_proc_state::repeat_s32_d32_nofilter_trans_shaderproc(params, px, row_bytes, x, y, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_s32_d32_const_x_shaderproc(
    s: *const ProcStateParams,
    matrix_proc_kind_raw: i32,
    pixels: *const u8,
    row_bytes: usize,
    x: i32,
    y: i32,
    colors: *mut u32,
    count: i32,
) {
    if s.is_null() || pixels.is_null() || colors.is_null() || count <= 0 {
        return;
    }
    let params = &*s;
    let kind = MatrixProcKind::from_i32(matrix_proc_kind_raw).unwrap_or(MatrixProcKind::None);
    let px_len = pixmap_byte_len(params.width, params.height, row_bytes, 4);
    let px = slice_from_raw(pixels, px_len);
    let out = std::slice::from_raw_parts_mut(colors, count as usize);
    bitmap_proc_state::s32_d32_const_x_shaderproc(params, kind, px, row_bytes, x, y, out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bps_do_nothing_shaderproc(colors: *mut u32, count: i32) {
    if colors.is_null() || count <= 0 {
        return;
    }
    let out = std::slice::from_raw_parts_mut(colors, count as usize);
    bitmap_proc_state::do_nothing_shaderproc(out);
}

// ============================================================================
// SkMipmap, SkMipmapAccessor, SkMipmapBuilder, DownSamplers
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_compute_level_count(
    base_width: i32,
    base_height: i32,
) -> i32 {
    mipmap::compute_level_count(base_width, base_height)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_compute_level_size(
    base_width: i32,
    base_height: i32,
    level: i32,
) -> ISize {
    mipmap::compute_level_size(base_width, base_height, level)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_alloc_levels_size(
    level_count: i32,
    level_struct_size: usize,
    pixel_size: usize,
) -> usize {
    mipmap::alloc_levels_size(level_count, level_struct_size, pixel_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_compute_total_pixel_size(
    base_width: i32,
    base_height: i32,
    ct_raw: i32,
) -> usize {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return 0;
    };
    mipmap::compute_total_pixel_size(base_width, base_height, ct)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_compute_level(
    scale_size: Size,
    legacy_anisotropic: bool,
) -> f32 {
    mipmap::compute_level(scale_size, legacy_anisotropic)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_select_extract_level(
    scale_size: Size,
    level_count: i32,
    legacy_anisotropic: bool,
    out_index: *mut usize,
) -> bool {
    match mipmap::select_extract_level(scale_size, level_count, legacy_anisotropic) {
        Some(idx) => {
            if !out_index.is_null() {
                *out_index = idx;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_valid_for_root_level(
    root_width: i32,
    root_height: i32,
    root_ct_raw: i32,
    root_at_raw: i32,
    level0_width: i32,
    level0_height: i32,
    level_cts: *const i32,
    level_ats: *const i32,
    level_count: usize,
) -> bool {
    let (Some(root_ct), Some(root_at)) = (
        ColorType::from_i32(root_ct_raw),
        AlphaType::from_i32(root_at_raw),
    ) else {
        return false;
    };
    let cts = slice_from_raw(level_cts, level_count);
    let ats = slice_from_raw(level_ats, level_count);
    mipmap::valid_for_root_level(
        root_width,
        root_height,
        root_ct,
        root_at,
        level0_width,
        level0_height,
        cts,
        ats,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_plan_accessor(
    inv: *const Matrix,
    requested_mode_raw: i32,
    legacy_anisotropic: bool,
    out: *mut MipmapAccessorPlan,
) {
    if inv.is_null() || out.is_null() {
        return;
    }
    let mode = MipmapMode::from_i32(requested_mode_raw).unwrap_or(MipmapMode::None);
    *out = mipmap::plan_mipmap_accessor(&*inv, mode, legacy_anisotropic);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_draw_downsampler_use_bilerp(
    dst_w: i32,
    dst_h: i32,
    src_w: i32,
    src_h: i32,
) -> bool {
    mipmap::draw_downsampler_use_bilerp(dst_w, dst_h, src_w, src_h)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_hq_downsample_supported(ct_raw: i32) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    mipmap::hq_downsample_supported(ct)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mipmap_hq_downsample_level(
    ct_raw: i32,
    src_w: i32,
    src_h: i32,
    src_rb: usize,
    src: *const u8,
    dst_w: i32,
    dst_h: i32,
    dst_rb: usize,
    dst: *mut u8,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    if src.is_null() || dst.is_null() {
        return false;
    }
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let src_len = pixmap_byte_len(src_w, src_h, src_rb, bpp);
    let dst_len = pixmap_byte_len(dst_w, dst_h, dst_rb, bpp);
    let src_slice = slice_from_raw(src, src_len);
    let dst_slice = slice_from_raw_mut(dst, dst_len);
    mipmap::hq_downsample_level(ct, src_w, src_h, src_rb, src_slice, dst_w, dst_h, dst_rb, dst_slice)
}

// ============================================================================
// SkSpecialImage
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_special_image_ganesh_should_downgrade_sampling(
    is_exact_fit: bool,
    max_aniso: i32,
    use_cubic: bool,
    mipmap_mode: i32,
) -> bool {
    special_image::ganesh_should_downgrade_sampling(is_exact_fit, max_aniso, use_cubic, mipmap_mode)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_special_image_draw_dst_rect(
    x: f32,
    y: f32,
    subset: *const IRect,
) -> Rect {
    if subset.is_null() {
        return Rect::make_empty();
    }
    special_image::draw_dst_rect(x, y, *subset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_special_image_shader_subset_matrix(
    subset: *const IRect,
    lm: *const Matrix,
    out: *mut Matrix,
) {
    if subset.is_null() || lm.is_null() || out.is_null() {
        return;
    }
    *out = special_image::shader_subset_matrix(*subset, &*lm);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_special_image_copy_from_raster_subset(
    subset: *const IRect,
) -> IRect {
    if subset.is_null() {
        return IRect::make_empty();
    }
    special_image::copy_from_raster_subset(*subset)
}
