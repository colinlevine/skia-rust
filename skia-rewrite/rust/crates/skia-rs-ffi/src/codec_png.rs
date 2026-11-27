//! C++/Rust FFI bindings for `skia-rs-codec-png` (`P2.4`).

use std::ffi::{c_char, CStr};

use skia_rs_codec_core::types::{EncodedAlpha, EncodedColor, ZeroInitialized};
use skia_rs_codec_meta::color_profile::DataSpace;
use skia_rs_codec_png::{
    chunks::{self, PngColorProfilePlan, UnknownChunkKind},
    codec_base::{
        self, ColorTablePopulateResult, SwizzlerPlan, XformInitPlan, XformMode,
        MAX_PALETTE_ENTRIES,
    },
    decoder::{
        self, InterlacedDecodeState, InterlacedOutputSchedule, InterlacedRowAction,
        NormalDecodeState, PngDecodeHeaderPlan,
    },
    encoder::{self, PngEncodeHeaderPlan},
    encoder_base::{self, PngTargetInfoPlan},
};
use skia_rs_color::image_info::{AlphaType, ColorType};

#[inline]
const fn data_space_from_i32(v: i32) -> Option<DataSpace> {
    match v {
        0 => Some(DataSpace::Rgb),
        1 => Some(DataSpace::Cmyk),
        2 => Some(DataSpace::Gray),
        3 => Some(DataSpace::Other),
        _ => None,
    }
}

// ============================================================================
// SkPngCodecBase & SkPngCompositeChunkReader (png_decode_common)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_is_png(buf: *const u8, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    chunks::is_png(slice)
}

#[no_mangle]
pub extern "C" fn skia_rs_png_is_compatible_color_profile(
    has_profile: bool,
    data_space: i32,
    encoded_color: i32,
) -> bool {
    let Some(color) = EncodedColor::from_i32(encoded_color) else {
        return true;
    };
    let ds = if has_profile {
        data_space_from_i32(data_space)
    } else {
        None
    };
    chunks::is_compatible_color_profile_and_type(ds, color)
}

#[no_mangle]
pub extern "C" fn skia_rs_png_to_skcms_pixel_format(
    encoded_color: i32,
    bits_per_component: u8,
) -> i32 {
    let color = EncodedColor::from_i32(encoded_color).unwrap_or(EncodedColor::Rgba);
    codec_base::encoded_info_to_skcms_pixel_format(color, bits_per_component)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_plan_xform_init(
    encoded_color: i32,
    encoded_bpc: u8,
    encoded_bpp: u32,
    dst_width: i32,
    frame_width: i32,
    has_subset: bool,
    has_color_xform: bool,
    out_plan: *mut XformInitPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let Some(color) = EncodedColor::from_i32(encoded_color) else {
        return false;
    };
    *out_plan = codec_base::plan_xform_init(
        color,
        encoded_bpc,
        encoded_bpp,
        dst_width,
        frame_width,
        has_subset,
        has_color_xform,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_plan_swizzler(
    encoded_color: i32,
    encoded_bpc: u8,
    dst_ct: i32,
    dst_at: i32,
    zero_init: i32,
    has_color_xform: bool,
    xform_on_decode: bool,
    skip_format_conversion: bool,
    has_subset: bool,
    out_plan: *mut SwizzlerPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let (Some(color), Some(ct), Some(at), Some(zi)) = (
        EncodedColor::from_i32(encoded_color),
        ColorType::from_i32(dst_ct),
        AlphaType::from_i32(dst_at),
        ZeroInitialized::from_i32(zero_init),
    ) else {
        return false;
    };
    *out_plan = codec_base::plan_swizzler(
        color,
        encoded_bpc,
        ct,
        at,
        zi,
        has_color_xform,
        xform_on_decode,
        skip_format_conversion,
        has_subset,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_compute_dst_row_size(
    xform_width: i32,
    dst_bytes_per_pixel: u32,
    out_size: *mut usize,
) -> bool {
    if out_size.is_null() {
        return false;
    }
    match codec_base::compute_dst_row_size(xform_width, dst_bytes_per_pixel) {
        Some(sz) => {
            *out_size = sz;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_png_compute_storage_bytes(
    xform_mode: i32,
    encoded_bpp: u32,
    dst_width: i32,
) -> usize {
    let Some(mode) = XformMode::from_i32(xform_mode) else {
        return 0;
    };
    codec_base::compute_storage_bytes(mode, encoded_bpp, dst_width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_populate_color_table(
    plte_rgb: *const u8,
    plte_entries: usize,
    trns_alphas: *const u8,
    trns_len: usize,
    bits_per_component: u8,
    encoded_alpha: i32,
    dst_ct: i32,
    dst_at: i32,
    has_color_xform: bool,
    xform_on_decode: bool,
    pmcolor_is_rgba: bool,
    out_table: *mut u32,
    out_result: *mut ColorTablePopulateResult,
) -> bool {
    if out_table.is_null() || out_result.is_null() {
        return false;
    }
    let (Some(e_alpha), Some(ct), Some(at)) = (
        EncodedAlpha::from_i32(encoded_alpha),
        ColorType::from_i32(dst_ct),
        AlphaType::from_i32(dst_at),
    ) else {
        return false;
    };
    let clamped_entries = plte_entries.min(MAX_PALETTE_ENTRIES);
    let plte_slice = if clamped_entries == 0 || plte_rgb.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(plte_rgb, clamped_entries * 3)
    };
    let trns_opt = if trns_alphas.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(trns_alphas, trns_len))
    };
    let table_arr: &mut [u32; MAX_PALETTE_ENTRIES] =
        &mut *(out_table as *mut [u32; MAX_PALETTE_ENTRIES]);
    *out_result = codec_base::populate_color_table_entries(
        plte_slice,
        trns_opt,
        bits_per_component,
        e_alpha,
        ct,
        at,
        has_color_xform,
        xform_on_decode,
        pmcolor_is_rgba,
        table_arr,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_pad_color_table(
    table: *mut u32,
    num_colors: usize,
    max_colors: usize,
) {
    if table.is_null() {
        return;
    }
    let table_arr: &mut [u32; MAX_PALETTE_ENTRIES] =
        &mut *(table as *mut [u32; MAX_PALETTE_ENTRIES]);
    codec_base::pad_color_table(table_arr, num_colors, max_colors);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_classify_unknown_chunk(
    tag: *const c_char,
    data_is_null: bool,
    data_len: usize,
) -> i32 {
    if tag.is_null() || data_is_null || data_len == 0 {
        return UnknownChunkKind::Ignored as i32;
    }
    let tag_bytes = CStr::from_ptr(tag).to_bytes();
    chunks::classify_unknown_chunk(tag_bytes, data_len) as i32
}

// ============================================================================
// SkPngCodec (png_decode_libpng)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_parse_chunk_header(
    bytes: *const u8,
    len: usize,
    out_length: *mut u32,
    out_is_idat: *mut bool,
    out_is_iend: *mut bool,
    out_payload_plus_crc: *mut usize,
) -> bool {
    if bytes.is_null() || len < 8 {
        return false;
    }
    let slice = std::slice::from_raw_parts(bytes, len);
    let Some(hdr) = chunks::parse_chunk_header(slice) else {
        return false;
    };
    let Some(total) = hdr.payload_plus_crc_len() else {
        return false;
    };
    if !out_length.is_null() {
        *out_length = hdr.length;
    }
    if !out_is_idat.is_null() {
        *out_is_idat = hdr.is_idat();
    }
    if !out_is_iend.is_null() {
        *out_is_iend = hdr.is_iend();
    }
    if !out_payload_plus_crc.is_null() {
        *out_payload_plus_crc = total;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_encode_idat_header(length: u32, out_buf: *mut u8) {
    if out_buf.is_null() {
        return;
    }
    let header = chunks::encode_idat_header(length);
    std::ptr::copy_nonoverlapping(header.as_ptr(), out_buf, 8);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_plan_color_profile(
    has_iccp: bool,
    has_srgb: bool,
    has_chrm: bool,
    chrm_fixed: *const i32,
    has_gama: bool,
    gama_fixed: i32,
    out_plan: *mut PngColorProfilePlan,
) {
    if out_plan.is_null() {
        return;
    }
    let mut chrm = [0i32; 8];
    if has_chrm && !chrm_fixed.is_null() {
        let src = std::slice::from_raw_parts(chrm_fixed, 8);
        chrm.copy_from_slice(src);
    }
    *out_plan = chunks::plan_color_profile(
        has_iccp,
        has_srgb,
        has_chrm && !chrm_fixed.is_null(),
        &chrm,
        has_gama,
        gama_fixed,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_plan_decode_header(
    encoded_color_type: i32,
    bit_depth: i32,
    has_trns: bool,
    has_sbit: bool,
    sbit_red: u8,
    sbit_green: u8,
    sbit_blue: u8,
    sbit_gray: u8,
    sbit_alpha: u8,
    out_plan: *mut PngDecodeHeaderPlan,
) {
    if out_plan.is_null() {
        return;
    }
    *out_plan = decoder::plan_decode_header(
        encoded_color_type,
        bit_depth,
        has_trns,
        has_sbit,
        sbit_red,
        sbit_green,
        sbit_blue,
        sbit_gray,
        sbit_alpha,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_start_all_rows(
    state: *mut NormalDecodeState,
    height: i32,
) {
    if let Some(st) = state.as_mut() {
        st.start_all_rows(height);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_on_all_rows(
    state: *mut NormalDecodeState,
    row_num: i32,
) {
    if let Some(st) = state.as_mut() {
        st.on_all_rows_callback(row_num);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_finish_all_rows(
    state: *const NormalDecodeState,
    process_succeeded: bool,
    height: i32,
    out_rows_decoded: *mut i32,
) -> i32 {
    let Some(st) = state.as_ref() else {
        return decoder::decode_error_result(false) as i32;
    };
    let (res, rows) = st.finish_all_rows(process_succeeded, height);
    if !out_rows_decoded.is_null() {
        *out_rows_decoded = rows;
    }
    res as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_set_range(
    state: *mut NormalDecodeState,
    first_row: i32,
    last_row: i32,
) {
    if let Some(st) = state.as_mut() {
        st.set_range(first_row, last_row);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_prepare_decode(
    state: *mut NormalDecodeState,
    has_swizzler: bool,
    sample_y: i32,
) {
    if let Some(st) = state.as_mut() {
        st.prepare_decode(if has_swizzler { Some(sample_y) } else { None });
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_relative_row(
    state: *const NormalDecodeState,
    row_num: i32,
    out_rel_row: *mut i32,
) -> bool {
    let Some(st) = state.as_ref() else {
        return false;
    };
    match st.relative_row(row_num) {
        Some(rel) => {
            if !out_rel_row.is_null() {
                *out_rel_row = rel;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_on_row(
    state: *mut NormalDecodeState,
    row_num: i32,
    row_needed: bool,
) -> i32 {
    let Some(st) = state.as_mut() else {
        return 0;
    };
    st.on_row_callback(row_num, row_needed) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_normal_finish_decode(
    state: *const NormalDecodeState,
    process_succeeded: bool,
    out_rows_decoded: *mut i32,
) -> i32 {
    let Some(st) = state.as_ref() else {
        return decoder::decode_error_result(false) as i32;
    };
    let (res, rows) = st.finish_decode(process_succeeded);
    if !out_rows_decoded.is_null() {
        *out_rows_decoded = rows;
    }
    res as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_interlaced_init(
    state: *mut InterlacedDecodeState,
    number_passes: i32,
    image_height: i32,
) {
    if let Some(st) = state.as_mut() {
        *st = InterlacedDecodeState::new(number_passes, image_height);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_interlaced_buffer_size(
    png_rowbytes: usize,
    height: i32,
    out_size: *mut usize,
) -> bool {
    if out_size.is_null() {
        return false;
    }
    match InterlacedDecodeState::compute_buffer_size(png_rowbytes, height) {
        Some(sz) => {
            *out_size = sz;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_interlaced_reset_range(
    state: *mut InterlacedDecodeState,
    first_row: i32,
    last_row: i32,
    png_rowbytes: usize,
) {
    if let Some(st) = state.as_mut() {
        st.reset_range(first_row, last_row, png_rowbytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_interlaced_on_row(
    state: *mut InterlacedDecodeState,
    row_num: i32,
    pass: i32,
    sample_y: i32,
    out_action: *mut InterlacedRowAction,
) {
    if out_action.is_null() {
        return;
    }
    let Some(st) = state.as_mut() else {
        *out_action = InterlacedRowAction {
            ignore: true,
            buffer_offset: 0,
            stop_decoding: false,
        };
        return;
    };
    *out_action = st.on_row(row_num, pass, sample_y);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_interlaced_output_schedule(
    state: *const InterlacedDecodeState,
    sample_y: i32,
    out_schedule: *mut InterlacedOutputSchedule,
) {
    if out_schedule.is_null() {
        return;
    }
    let Some(st) = state.as_ref() else {
        *out_schedule = InterlacedOutputSchedule {
            has_lines: false,
            start_src_row: 0,
            step: sample_y,
            rows_to_write: 0,
        };
        return;
    };
    *out_schedule = st.output_schedule(sample_y);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_interlaced_finish(
    state: *const InterlacedDecodeState,
    process_succeeded: bool,
    rows_written: i32,
    out_rows_decoded: *mut i32,
) -> i32 {
    let Some(st) = state.as_ref() else {
        return decoder::decode_error_result(false) as i32;
    };
    let (res, rows) = st.finish(process_succeeded, rows_written);
    if !out_rows_decoded.is_null() {
        *out_rows_decoded = rows;
    }
    res as i32
}

// ============================================================================
// SkPngEncoderBase (png_encode_common)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_encoder_get_target_info(
    width: i32,
    src_ct: i32,
    src_at: i32,
    out_plan: *mut PngTargetInfoPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let (Some(ct), Some(at)) = (ColorType::from_i32(src_ct), AlphaType::from_i32(src_at)) else {
        return false;
    };
    let plan = encoder_base::get_target_info(width, ct, at);
    *out_plan = plan;
    plan.valid
}

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_validate_rows_args(
    width: i32,
    height: i32,
    num_rows: i32,
) -> bool {
    encoder_base::validate_encode_rows_args(width, height, num_rows)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_transform_a8_to_gray_alpha(
    dst: *mut u8,
    src: *const u8,
    width: i32,
) {
    if dst.is_null() || src.is_null() || width <= 0 {
        return;
    }
    let w = width as usize;
    let src_slice = std::slice::from_raw_parts(src, w);
    let dst_slice = std::slice::from_raw_parts_mut(dst, w * 2);
    encoder_base::transform_scanline_a8_to_gray_alpha(dst_slice, src_slice);
}

// ============================================================================
// SkPngEncoderImpl (png_encode_libpng)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_validate_pixmap(
    width: i32,
    height: i32,
    ct: i32,
    at: i32,
    has_addr: bool,
    row_bytes: usize,
) -> bool {
    let (Some(color_type), Some(alpha_type)) = (ColorType::from_i32(ct), AlphaType::from_i32(at))
    else {
        return false;
    };
    encoder::validate_pixmap_for_encode(width, height, color_type, alpha_type, has_addr, row_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_png_encoder_plan_header(
    dst_color: i32,
    dst_bpc: i32,
    dst_row_is_opaque: bool,
    src_ct: i32,
    filter_flags: i32,
    zlib_level: i32,
    out_plan: *mut PngEncodeHeaderPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let (Some(dc), Some(ct)) = (EncodedColor::from_i32(dst_color), ColorType::from_i32(src_ct))
    else {
        return false;
    };
    let plan = encoder::plan_encode_header(
        dc,
        dst_bpc,
        dst_row_is_opaque,
        ct,
        filter_flags,
        zlib_level,
    );
    *out_plan = plan;
    plan.valid
}

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_validate_comments_count(count: i32) -> bool {
    encoder::validate_comments_count(count)
}

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_clip_keyword_len(keyword_len: usize) -> usize {
    encoder::clip_text_keyword_len(keyword_len)
}

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_plan_color_space(
    has_cs: bool,
    cs_is_srgb: bool,
    has_hdr_metadata: bool,
) -> i32 {
    encoder::plan_encode_color_space(has_cs, cs_is_srgb, has_hdr_metadata) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_plan_gainmap(
    has_gainmap_info: bool,
    has_gainmap_pixels: bool,
) -> i32 {
    encoder::plan_encode_gainmap(has_gainmap_info, has_gainmap_pixels) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_png_encoder_gainmap_supports_rgb_icc(dst_color: i32) -> bool {
    match EncodedColor::from_i32(dst_color) {
        Some(c) => encoder::gainmap_supports_rgb_icc(c),
        None => false,
    }
}
