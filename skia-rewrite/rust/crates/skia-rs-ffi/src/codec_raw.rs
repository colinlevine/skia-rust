//! C++/Rust FFI bindings for `skia-rs-codec-raw` (`P2.8`).

use skia_rs_codec_core::types::{CodecResult, EncodedOrigin};
use skia_rs_codec_raw::{
    header, scaling,
    stream::{self, BufferMorePlan, BufferedTransferPlan, RawBufferedStream},
    tiling,
    types::{
        DngPoint, DngRect, PiexColorSpace, PiexDecisionKind, PiexError, PiexImageFormat,
        PiexPreviewPlan, RawEncodedInfoSpec, RawGetPixelsPlan, RawImageMeta,
    },
};
use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::ISize;

/// Opaque container holding task areas computed by `tiling::compute_task_areas`.
pub struct RawTaskAreas {
    rects: Vec<DngRect>,
}

// ============================================================================
// Header / TIFF / EXIF Orientation / PIEX Preview / EncodedInfo
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_is_tiff_header_valid(header_ptr: *const u8, len: usize) -> bool {
    if header_ptr.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(header_ptr, len);
    header::is_tiff_header_valid(slice)
}

#[no_mangle]
pub extern "C" fn skia_rs_raw_exif_orientation_to_origin(exif_orientation: u32) -> i32 {
    header::exif_orientation_to_origin(exif_orientation) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_evaluate_piex_preview(
    error: i32,
    color_space: i32,
    preview_offset: u32,
    preview_length: u32,
    preview_format: i32,
    exif_orientation: u32,
    out_plan: *mut PiexPreviewPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let Some(err) = PiexError::from_i32(error) else {
        *out_plan = PiexPreviewPlan {
            decision: PiexDecisionKind::Fail as i32,
            result: CodecResult::InvalidInput as i32,
            preview_offset: 0,
            preview_length: 0,
            use_adobe_rgb: false,
            origin: EncodedOrigin::TopLeft as i32,
        };
        return true;
    };
    let cs = PiexColorSpace::from_i32(color_space).unwrap_or(PiexColorSpace::Srgb);
    let fmt = PiexImageFormat::from_i32(preview_format).unwrap_or(PiexImageFormat::UncompressedRgb);
    *out_plan = header::evaluate_piex_preview(
        err,
        cs,
        preview_offset,
        preview_length,
        fmt,
        exif_orientation,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_init_image_meta(
    width: i32,
    height: i32,
    cfa_pattern_v: i32,
    cfa_pattern_h: i32,
    out_meta: *mut RawImageMeta,
) -> bool {
    if out_meta.is_null() {
        return false;
    }
    let meta = header::init_raw_image_meta(width, height, cfa_pattern_v, cfa_pattern_h);
    *out_meta = meta;
    meta.valid
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_encoded_info_spec(
    width: i32,
    height: i32,
    out_spec: *mut RawEncodedInfoSpec,
) -> bool {
    if out_spec.is_null() {
        return false;
    }
    *out_spec = header::encoded_info_spec(width, height);
    true
}

// ============================================================================
// Scaling / Dimensions / Render Sizing / Pixel Buffer & Row Planning
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_raw_preferred_render_size(width: i32, height: i32) -> i32 {
    scaling::preferred_render_size(width, height)
}

#[no_mangle]
pub extern "C" fn skia_rs_raw_max_render_size(stage3_h: i32, stage3_v: i32) -> i32 {
    scaling::max_render_size(stage3_h, stage3_v)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_get_scaled_dimensions(
    width: i32,
    height: i32,
    desired_scale: f32,
    is_scalable: bool,
    is_xtrans_image: bool,
    out_width: *mut i32,
    out_height: *mut i32,
) -> bool {
    if out_width.is_null() || out_height.is_null() {
        return false;
    }
    let scaled = scaling::get_scaled_dimensions(
        ISize { width, height },
        desired_scale,
        is_scalable,
        is_xtrans_image,
    );
    *out_width = scaled.width;
    *out_height = scaled.height;
    true
}

#[no_mangle]
pub extern "C" fn skia_rs_raw_dimensions_supported(
    full_width: i32,
    full_height: i32,
    target_width: i32,
    target_height: i32,
    is_scalable: bool,
    is_xtrans_image: bool,
) -> bool {
    scaling::dimensions_supported(
        ISize {
            width: full_width,
            height: full_height,
        },
        ISize {
            width: target_width,
            height: target_height,
        },
        is_scalable,
        is_xtrans_image,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_plan_get_pixels(
    rendered_h: i32,
    rendered_v: i32,
    dst_width: i32,
    dst_height: i32,
    dst_color_type: i32,
    pmcolor_is_rgba: bool,
    out_plan: *mut RawGetPixelsPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let ct = ColorType::from_i32(dst_color_type).unwrap_or(ColorType::Unknown);
    *out_plan = scaling::plan_get_pixels(
        rendered_h,
        rendered_v,
        dst_width,
        dst_height,
        ct,
        pmcolor_is_rgba,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_row_area(
    row_index: i32,
    width: i32,
    out_rect: *mut DngRect,
) -> bool {
    if out_rect.is_null() {
        return false;
    }
    *out_rect = scaling::row_area(row_index, width);
    true
}

// ============================================================================
// Stream Classification, Overflow Checks & Buffered Stream State Machine
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_raw_is_asset_stream(has_length: bool, has_position: bool) -> bool {
    stream::is_asset_stream(has_length, has_position)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_safe_add_usize(
    arg1: usize,
    arg2: usize,
    out_sum: *mut usize,
) -> bool {
    if out_sum.is_null() {
        return false;
    }
    match stream::safe_add_usize(arg1, arg2) {
        Some(sum) => {
            *out_sum = sum;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_validate_dng_read(
    count: u32,
    offset: u64,
    out_offset: *mut usize,
    out_count: *mut usize,
) -> bool {
    if out_offset.is_null() || out_count.is_null() {
        return false;
    }
    match stream::validate_dng_read(count, offset) {
        Some((off, cnt)) => {
            *out_offset = off;
            *out_count = cnt;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_raw_validate_asset_read(offset: usize, length: usize) -> bool {
    stream::validate_asset_read(offset, length).is_some()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_plan_asset_transfer(
    stream_length: usize,
    offset: usize,
    size: usize,
    out_bytes_to_read: *mut usize,
) -> bool {
    if out_bytes_to_read.is_null() {
        return false;
    }
    match stream::plan_asset_transfer(stream_length, offset, size) {
        Some(n) => {
            *out_bytes_to_read = n;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_raw_buffered_stream_new() -> *mut RawBufferedStream {
    Box::into_raw(Box::new(RawBufferedStream::new()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_free(state: *mut RawBufferedStream) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_bytes_written(
    state: *const RawBufferedStream,
) -> usize {
    if state.is_null() {
        return 0;
    }
    (*state).bytes_written()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_write(
    state: *mut RawBufferedStream,
    data: *const u8,
    len: usize,
) -> bool {
    if state.is_null() {
        return false;
    }
    if len == 0 {
        return true;
    }
    if data.is_null() {
        return false;
    }
    let slice = std::slice::from_raw_parts(data, len);
    (*state).write(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_mark_eof(state: *mut RawBufferedStream) {
    if !state.is_null() {
        (*state).mark_whole_stream_read();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_read_buffered(
    state: *const RawBufferedStream,
    offset: usize,
    dst: *mut u8,
    len: usize,
) -> bool {
    if state.is_null() {
        return false;
    }
    if len == 0 {
        return true;
    }
    if dst.is_null() {
        return false;
    }
    let out = std::slice::from_raw_parts_mut(dst, len);
    (*state).read_buffered(offset, out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_plan_buffer_more(
    state: *const RawBufferedStream,
    new_size: usize,
    out_plan: *mut BufferMorePlan,
) -> bool {
    if state.is_null() || out_plan.is_null() {
        return false;
    }
    *out_plan = (*state).plan_buffer_more(new_size);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_commit_chunk(
    state: *mut RawBufferedStream,
    data: *const u8,
    len: usize,
    size_requested: usize,
) -> bool {
    if state.is_null() {
        return false;
    }
    let slice = if data.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(data, len)
    };
    (*state).commit_chunk(slice, size_requested)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_plan_transfer(
    state: *const RawBufferedStream,
    offset: usize,
    size: usize,
    out_plan: *mut BufferedTransferPlan,
) -> bool {
    if state.is_null() || out_plan.is_null() {
        return false;
    }
    *out_plan = (*state).plan_transfer(offset, size);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_buffered_stream_finalize_transfer_size(
    already_buffered: usize,
    remaining: usize,
    bytes_read: usize,
    out_size: *mut usize,
) -> bool {
    if out_size.is_null() {
        return false;
    }
    match RawBufferedStream::finalize_transfer_size(already_buffered, remaining, bytes_read) {
        Some(n) => {
            *out_size = n;
            true
        }
        None => false,
    }
}

// ============================================================================
// Tile / Area Task Partitioning (`SkDngHost`)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_raw_perform_area_task_threads(
    is_android: bool,
    max_mp_threads: u32,
) -> u32 {
    tiling::perform_area_task_threads(is_android, max_mp_threads)
}

#[no_mangle]
pub extern "C" fn skia_rs_raw_task_areas_compute(
    max_tasks: i32,
    area: DngRect,
    tile_size: DngPoint,
) -> *mut RawTaskAreas {
    match tiling::compute_task_areas(max_tasks, area, tile_size) {
        Some(rects) => Box::into_raw(Box::new(RawTaskAreas { rects })),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_task_areas_len(areas: *const RawTaskAreas) -> usize {
    if areas.is_null() {
        return 0;
    }
    (*areas).rects.len()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_task_areas_data(areas: *const RawTaskAreas) -> *const DngRect {
    if areas.is_null() {
        return std::ptr::null();
    }
    (*areas).rects.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raw_task_areas_free(areas: *mut RawTaskAreas) {
    if !areas.is_null() {
        drop(Box::from_raw(areas));
    }
}
