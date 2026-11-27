//! C++/Rust FFI bindings for `skia-rs-codec-bmp` (`P2.3`).

use std::ffi::c_void;

use skia_rs_codec_bmp::{
    bmp_header::{self, BmpHeaderInfo},
    bmp_mask::{self, MaskSwizzlerTargetInfo},
    bmp_rle::{self, BmpRleCursor, RleSkipPlan, RLE_BUFFER_SIZE},
    bmp_standard::{self, StandardSwizzlerEncodedInfo},
    ico::{self, IcoEntry},
    stream::ByteReader,
    wbmp,
};
use skia_rs_codec_core::types::{CodecResult, EncodedAlpha, EncodedColor, ScanlineOrder};
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::ISize;

pub type ReadStreamFn = unsafe extern "C" fn(ctx: *mut c_void, dst: *mut u8, size: usize) -> usize;
pub type SkipStreamFn = unsafe extern "C" fn(ctx: *mut c_void, count: usize) -> usize;

struct CallbackReader {
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
}

impl ByteReader for CallbackReader {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        unsafe { (self.read_fn)(self.ctx, buf.as_mut_ptr(), buf.len()) }
    }

    #[inline]
    fn skip(&mut self, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        unsafe { (self.skip_fn)(self.ctx, count) }
    }
}

// ============================================================================
// SkBmpCodec
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_is_bmp(buf: *const u8, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    bmp_header::is_bmp(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_read_header(
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    in_ico: bool,
    validate_masks: bool,
    out_info: *mut BmpHeaderInfo,
) -> i32 {
    if ctx.is_null() {
        return CodecResult::InvalidInput as i32;
    }
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    match bmp_header::read_bmp_header(&mut reader, in_ico, validate_masks) {
        Ok(info) => {
            if !out_info.is_null() {
                *out_info = info;
            }
            CodecResult::Success as i32
        }
        Err(err) => err as i32,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_bmp_src_row_bytes(width: i32, bits_per_pixel: u16) -> usize {
    bmp_header::bmp_src_row_bytes(width, bits_per_pixel)
}

#[no_mangle]
pub extern "C" fn skia_rs_bmp_and_mask_row_bytes(width: i32) -> usize {
    bmp_header::bmp_and_mask_row_bytes(width)
}

#[no_mangle]
pub extern "C" fn skia_rs_bmp_get_dst_row(row_order: i32, y: i32, height: i32) -> i32 {
    let order = ScanlineOrder::from_i32(row_order).unwrap_or(ScanlineOrder::BottomUp);
    bmp_header::bmp_dst_row(order, y, height)
}

// ============================================================================
// SkBmpStandardCodec
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_bmp_standard_validate_get_pixels(
    has_subset: bool,
    dst_dims: ISize,
    src_dims: ISize,
) -> i32 {
    bmp_standard::validate_standard_get_pixels(has_subset, dst_dims, src_dims) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_standard_build_color_table(
    bits_per_pixel: u16,
    num_colors: u32,
    bytes_per_color: u32,
    offset: u32,
    is_opaque: bool,
    in_ico: bool,
    dst_color_type: i32,
    dst_alpha_type: i32,
    has_color_xform: bool,
    pmcolor_is_bgra: bool,
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    out_colors: *mut u32,
    out_max_colors: *mut u32,
) -> bool {
    if ctx.is_null() || out_colors.is_null() {
        return false;
    }
    let (Some(ct), Some(at)) = (
        ColorType::from_i32(dst_color_type),
        AlphaType::from_i32(dst_alpha_type),
    ) else {
        return false;
    };
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    let mut colors = [0u32; 256];
    let Some(max_colors) = bmp_standard::build_standard_color_table(
        bits_per_pixel,
        num_colors,
        bytes_per_color,
        offset,
        is_opaque,
        in_ico,
        ct,
        at,
        has_color_xform,
        pmcolor_is_bgra,
        &mut reader,
        &mut colors,
    ) else {
        return false;
    };
    if max_colors > 0 {
        let out_slice = std::slice::from_raw_parts_mut(out_colors, max_colors as usize);
        out_slice.copy_from_slice(&colors[..max_colors as usize]);
    }
    if !out_max_colors.is_null() {
        *out_max_colors = max_colors;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_standard_swizzler_info(
    in_ico: bool,
    bits_per_pixel: u16,
    encoded_color: i32,
    encoded_alpha: i32,
    encoded_bpc: u8,
    out_info: *mut StandardSwizzlerEncodedInfo,
) -> bool {
    let (Some(color), Some(alpha)) = (
        EncodedColor::from_i32(encoded_color),
        EncodedAlpha::from_i32(encoded_alpha),
    ) else {
        return false;
    };
    let info =
        bmp_standard::standard_swizzler_info(in_ico, bits_per_pixel, color, alpha, encoded_bpc);
    if !out_info.is_null() {
        *out_info = info;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_standard_ico_and_mask_substream(
    total_height: i32,
    start_scanline: i32,
    count: i32,
    src_row_bytes: usize,
    and_mask_row_bytes: usize,
    curr_position: usize,
    stream_length: usize,
    out_start: *mut usize,
    out_length: *mut usize,
) -> bool {
    let Some((start, len)) = bmp_standard::compute_ico_and_mask_substream(
        total_height,
        start_scanline,
        count,
        src_row_bytes,
        and_mask_row_bytes,
        curr_position,
        stream_length,
    ) else {
        return false;
    };
    if !out_start.is_null() {
        *out_start = start;
    }
    if !out_length.is_null() {
        *out_length = len;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_standard_decode_ico_mask(
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    mask_buf: *mut u8,
    and_mask_row_bytes: usize,
    dst: *mut u8,
    dst_row_bytes: usize,
    dst_height: i32,
    dst_color_type: i32,
    row_order: i32,
    sample_x: i32,
    swizzle_width: i32,
    src_offset: i32,
) {
    if ctx.is_null() || mask_buf.is_null() || dst_height <= 0 {
        return;
    }
    let Some(ct) = ColorType::from_i32(dst_color_type) else {
        return;
    };
    let order = ScanlineOrder::from_i32(row_order).unwrap_or(ScanlineOrder::BottomUp);
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    let mask_slice = std::slice::from_raw_parts_mut(mask_buf, and_mask_row_bytes);

    let pixel_bytes: usize = if ct == ColorType::RgbaF16 { 8 } else { 4 };
    let dst_slice: &mut [u8] = if !dst.is_null() && swizzle_width > 0 {
        let total_bytes =
            ((dst_height as usize) - 1) * dst_row_bytes + (swizzle_width as usize) * pixel_bytes;
        std::slice::from_raw_parts_mut(dst, total_bytes)
    } else {
        &mut []
    };

    bmp_standard::decode_ico_mask(
        &mut reader,
        mask_slice,
        and_mask_row_bytes,
        dst_slice,
        dst_row_bytes,
        dst_height,
        ct,
        order,
        sample_x,
        swizzle_width,
        src_offset,
    );
}

// ============================================================================
// SkBmpMaskCodec
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_bmp_mask_validate_get_pixels(
    has_subset: bool,
    dst_dims: ISize,
    src_dims: ISize,
) -> i32 {
    bmp_mask::validate_mask_get_pixels(has_subset, dst_dims, src_dims) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_mask_prepare_swizzler_info(
    dst_color_type: i32,
    dst_alpha_type: i32,
    has_color_xform: bool,
    out_info: *mut MaskSwizzlerTargetInfo,
) -> bool {
    let (Some(ct), Some(at)) = (
        ColorType::from_i32(dst_color_type),
        AlphaType::from_i32(dst_alpha_type),
    ) else {
        return false;
    };
    let info = bmp_mask::prepare_mask_swizzler_info(ct, at, has_color_xform);
    if !out_info.is_null() {
        *out_info = info;
    }
    true
}

// ============================================================================
// SkBmpRLECodec
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_build_color_table(
    bits_per_pixel: u16,
    num_colors: u32,
    bytes_per_color: u32,
    offset: u32,
    dst_color_type: i32,
    has_color_xform: bool,
    pmcolor_is_bgra: bool,
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    out_colors: *mut u32,
    out_max_colors: *mut u32,
) -> bool {
    if ctx.is_null() || out_colors.is_null() {
        return false;
    }
    let Some(ct) = ColorType::from_i32(dst_color_type) else {
        return false;
    };
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    let mut colors = [0u32; 256];
    let Some(max_colors) = bmp_rle::build_rle_color_table(
        bits_per_pixel,
        num_colors,
        bytes_per_color,
        offset,
        ct,
        has_color_xform,
        pmcolor_is_bgra,
        &mut reader,
        &mut colors,
    ) else {
        return false;
    };
    if max_colors > 0 {
        let out_slice = std::slice::from_raw_parts_mut(out_colors, max_colors as usize);
        out_slice.copy_from_slice(&colors[..max_colors as usize]);
    }
    if !out_max_colors.is_null() {
        *out_max_colors = max_colors;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_init_stream_buffer(
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    stream_buf: *mut u8,
    bytes_buffered: *mut usize,
    curr_rle_byte: *mut u32,
) -> bool {
    if ctx.is_null() || stream_buf.is_null() || bytes_buffered.is_null() || curr_rle_byte.is_null() {
        return false;
    }
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    let mut dummy_skip = 0i32;
    let buf_ref = &mut *(stream_buf as *mut [u8; RLE_BUFFER_SIZE]);
    let mut cursor = BmpRleCursor {
        stream_buffer: buf_ref,
        bytes_buffered: &mut *bytes_buffered,
        curr_rle_byte: &mut *curr_rle_byte,
        lines_to_skip: &mut dummy_skip,
    };
    cursor.initialize_stream_buffer(&mut reader)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_check_for_more_data(
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    stream_buf: *mut u8,
    bytes_buffered: *mut usize,
    curr_rle_byte: *mut u32,
) -> usize {
    if ctx.is_null() || stream_buf.is_null() || bytes_buffered.is_null() || curr_rle_byte.is_null() {
        return 0;
    }
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    let mut dummy_skip = 0i32;
    let buf_ref = &mut *(stream_buf as *mut [u8; RLE_BUFFER_SIZE]);
    let mut cursor = BmpRleCursor {
        stream_buffer: buf_ref,
        bytes_buffered: &mut *bytes_buffered,
        curr_rle_byte: &mut *curr_rle_byte,
        lines_to_skip: &mut dummy_skip,
    };
    cursor.check_for_more_data(&mut reader)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_consume_lines_to_skip(
    height: i32,
    lines_to_skip: *mut i32,
) -> RleSkipPlan {
    if lines_to_skip.is_null() {
        return RleSkipPlan {
            rows_advanced: 0,
            remaining_height: height,
            fully_skipped: false,
        };
    }
    bmp_rle::consume_rle_lines_to_skip(height, &mut *lines_to_skip)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_compute_f16_xform_layout(
    dst_width: i32,
    height: i32,
    out_count: *mut i32,
    out_byte_size: *mut usize,
    out_row_bytes: *mut usize,
) -> bool {
    let Some((count, bytes, row_bytes)) = bmp_rle::compute_rle_f16_xform_layout(dst_width, height)
    else {
        return false;
    };
    if !out_count.is_null() {
        *out_count = count;
    }
    if !out_byte_size.is_null() {
        *out_byte_size = bytes;
    }
    if !out_row_bytes.is_null() {
        *out_row_bytes = row_bytes;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_set_pixel(
    dst: *mut u8,
    dst_row_bytes: usize,
    dst_width: i32,
    dst_height: i32,
    dst_color_type: i32,
    row_order: i32,
    sample_x: i32,
    color_table: *const u32,
    color_table_len: usize,
    pmcolor_is_bgra: bool,
    x: i32,
    y: i32,
    index: u8,
) {
    if dst.is_null() || color_table.is_null() || dst_width <= 0 || dst_height <= 0 {
        return;
    }
    let Some(ct) = ColorType::from_i32(dst_color_type) else {
        return;
    };
    let order = ScanlineOrder::from_i32(row_order).unwrap_or(ScanlineOrder::BottomUp);
    let pixel_bytes: usize = match ct {
        ColorType::Rgba8888 | ColorType::Bgra8888 => 4,
        ColorType::Rgb565 => 2,
        _ => return,
    };
    let total_bytes =
        ((dst_height as usize) - 1) * dst_row_bytes + (dst_width as usize) * pixel_bytes;
    let dst_slice = std::slice::from_raw_parts_mut(dst, total_bytes);
    let ctable = std::slice::from_raw_parts(color_table, color_table_len);
    bmp_rle::set_rle_pixel(
        Some(dst_slice),
        dst_row_bytes,
        dst_width,
        dst_height,
        ct,
        order,
        sample_x,
        ctable,
        pmcolor_is_bgra,
        x,
        y,
        index,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_set_rgb_pixel(
    dst: *mut u8,
    dst_row_bytes: usize,
    dst_width: i32,
    dst_height: i32,
    dst_color_type: i32,
    row_order: i32,
    sample_x: i32,
    x: i32,
    y: i32,
    red: u8,
    green: u8,
    blue: u8,
) {
    if dst.is_null() || dst_width <= 0 || dst_height <= 0 {
        return;
    }
    let Some(ct) = ColorType::from_i32(dst_color_type) else {
        return;
    };
    let order = ScanlineOrder::from_i32(row_order).unwrap_or(ScanlineOrder::BottomUp);
    let pixel_bytes: usize = match ct {
        ColorType::Rgba8888 | ColorType::Bgra8888 => 4,
        ColorType::Rgb565 => 2,
        _ => return,
    };
    let total_bytes =
        ((dst_height as usize) - 1) * dst_row_bytes + (dst_width as usize) * pixel_bytes;
    let dst_slice = std::slice::from_raw_parts_mut(dst, total_bytes);
    bmp_rle::set_rle_rgb_pixel(
        Some(dst_slice),
        dst_row_bytes,
        dst_width,
        dst_height,
        ct,
        order,
        sample_x,
        x,
        y,
        red,
        green,
        blue,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bmp_rle_decode(
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    stream_buf: *mut u8,
    bytes_buffered: *mut usize,
    curr_rle_byte: *mut u32,
    lines_to_skip: *mut i32,
    orig_width: i32,
    bits_per_pixel: u16,
    row_order: i32,
    sample_x: i32,
    color_table: *const u32,
    color_table_len: usize,
    pmcolor_is_bgra: bool,
    dst: *mut u8,
    dst_width: i32,
    dst_height: i32,
    dst_row_bytes: usize,
    dst_color_type: i32,
) -> i32 {
    if ctx.is_null()
        || stream_buf.is_null()
        || bytes_buffered.is_null()
        || curr_rle_byte.is_null()
        || lines_to_skip.is_null()
        || dst_height <= 0
    {
        return 0;
    }
    let Some(ct) = ColorType::from_i32(dst_color_type) else {
        return 0;
    };
    let order = ScanlineOrder::from_i32(row_order).unwrap_or(ScanlineOrder::BottomUp);
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    let buf_ref = &mut *(stream_buf as *mut [u8; RLE_BUFFER_SIZE]);
    let mut cursor = BmpRleCursor {
        stream_buffer: buf_ref,
        bytes_buffered: &mut *bytes_buffered,
        curr_rle_byte: &mut *curr_rle_byte,
        lines_to_skip: &mut *lines_to_skip,
    };
    let ctable: &[u32] = if !color_table.is_null() && color_table_len > 0 {
        std::slice::from_raw_parts(color_table, color_table_len)
    } else {
        &[]
    };
    let dst_opt: Option<&mut [u8]> = if !dst.is_null() && dst_width > 0 {
        let pixel_bytes: usize = match ct {
            ColorType::Rgba8888 | ColorType::Bgra8888 => 4,
            ColorType::Rgb565 => 2,
            _ => 0,
        };
        if pixel_bytes > 0 {
            let total_bytes =
                ((dst_height as usize) - 1) * dst_row_bytes + (dst_width as usize) * pixel_bytes;
            Some(std::slice::from_raw_parts_mut(dst, total_bytes))
        } else {
            None
        }
    } else {
        None
    };

    bmp_rle::decode_rle(
        &mut cursor,
        &mut reader,
        orig_width,
        bits_per_pixel,
        order,
        sample_x,
        ctable,
        pmcolor_is_bgra,
        dst_opt,
        dst_width,
        dst_height,
        dst_row_bytes,
        ct,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_bmp_rle_fill_width(orig_width: i32, sample_x: i32) -> i32 {
    bmp_rle::rle_fill_width(orig_width, sample_x)
}

// ============================================================================
// SkWbmpCodec
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_wbmp_is_wbmp(buf: *const u8, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    wbmp::is_wbmp(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_wbmp_read_header(
    ctx: *mut c_void,
    read_fn: ReadStreamFn,
    skip_fn: SkipStreamFn,
    out_size: *mut ISize,
) -> bool {
    if ctx.is_null() {
        return false;
    }
    let mut reader = CallbackReader {
        ctx,
        read_fn,
        skip_fn,
    };
    match wbmp::read_wbmp_header(&mut reader) {
        Some(size) => {
            if !out_size.is_null() {
                *out_size = size;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_wbmp_src_row_bytes(width: i32) -> usize {
    wbmp::wbmp_src_row_bytes(width)
}

#[no_mangle]
pub extern "C" fn skia_rs_wbmp_conversion_supported(
    dst_color_type: i32,
    dst_alpha_type: i32,
    has_color_space: bool,
    src_is_opaque: bool,
) -> bool {
    let (Some(ct), Some(at)) = (
        ColorType::from_i32(dst_color_type),
        AlphaType::from_i32(dst_alpha_type),
    ) else {
        return false;
    };
    wbmp::wbmp_conversion_supported(ct, at, has_color_space, src_is_opaque)
}

// ============================================================================
// SkIcoCodec
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ico_is_ico(buf: *const u8, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    ico::is_ico(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ico_parse_directory(
    data: *const u8,
    len: usize,
    out_entries: *mut *mut IcoEntry,
    out_count: *mut usize,
    out_default_result: *mut i32,
) -> i32 {
    let slice = if data.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(data, len)
    };
    match ico::parse_ico_directory(slice) {
        Ok(dir) => {
            let boxed: Box<[IcoEntry]> = dir.entries.into_boxed_slice();
            let count = boxed.len();
            let ptr = if count == 0 {
                std::ptr::null_mut()
            } else {
                Box::into_raw(boxed) as *mut IcoEntry
            };
            if !out_entries.is_null() {
                *out_entries = ptr;
            }
            if !out_count.is_null() {
                *out_count = count;
            }
            if !out_default_result.is_null() {
                *out_default_result = dir.default_result as i32;
            }
            CodecResult::Success as i32
        }
        Err(err) => err as i32,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ico_free_entries(entries: *mut IcoEntry, count: usize) {
    if entries.is_null() || count == 0 {
        return;
    }
    let slice_ptr = std::ptr::slice_from_raw_parts_mut(entries, count);
    drop(Box::from_raw(slice_ptr));
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ico_select_largest_codec(
    min_byte_sizes: *const usize,
    count: usize,
) -> usize {
    if min_byte_sizes.is_null() || count == 0 {
        return 0;
    }
    let slice = std::slice::from_raw_parts(min_byte_sizes, count);
    ico::ico_select_largest_codec(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ico_get_scaled_dimensions(
    orig: ISize,
    desired_scale: f32,
    embedded_dims: *const ISize,
    count: usize,
) -> ISize {
    if embedded_dims.is_null() || count == 0 {
        return orig;
    }
    let slice = std::slice::from_raw_parts(embedded_dims, count);
    ico::ico_get_scaled_dimensions(orig, desired_scale, slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ico_choose_codec(
    embedded_dims: *const ISize,
    count: usize,
    requested: ISize,
    start_index: i32,
) -> i32 {
    if embedded_dims.is_null() || count == 0 {
        return -1;
    }
    let slice = std::slice::from_raw_parts(embedded_dims, count);
    ico::ico_choose_codec(slice, requested, start_index)
}
