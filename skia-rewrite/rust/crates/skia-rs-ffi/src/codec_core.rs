//! C++/Rust FFI bindings for `skia-rs-codec-core` (`P2.1`).

use std::ffi::{c_char, c_void};

use skia_rs_codec_core::{
    android_codec::{
        self, AndroidPixelsValidation, NativeScalingPlan, SampledDecodeParams, ScaledSubsetPlan,
    },
    codec::{self, ColorXformPlan, FillIncompletePlan, GeneratorInfoAdjust, ZeroRectPlan},
    encoded_info, encoder,
    frame_holder::{self, FrameDependencyResult, FrameInfoFields, FrameSummary},
    mask_swizzler::{self, MaskSwizzleProcId, MaskSwizzlerInit},
    palette, pixmap_utils, sampler,
    swizzler::{self, SwizzleProcId, SwizzlerInit, SwizzlerSampleXResult},
    types::{
        CodecResult, DisposalMethod, EncodedAlpha, EncodedColor, EncodedImageFormat, EncodedOrigin,
        ScanlineOrder, ZeroInitialized,
    },
};
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_color::masks::Masks;
use skia_rs_geometry::{IRect, ISize};

#[inline]
unsafe fn opt_irect(ptr: *const IRect) -> Option<IRect> {
    if ptr.is_null() {
        None
    } else {
        Some(*ptr)
    }
}

// ============================================================================
// SkColorPalette
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_palette_copy(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) -> bool {
    if count <= 0 {
        return count == 0;
    }
    if dst.is_null() || src.is_null() {
        return false;
    }
    let n = count as usize;
    let dst_slice = std::slice::from_raw_parts_mut(dst, n);
    let src_slice = std::slice::from_raw_parts(src, n);
    palette::copy_palette(dst_slice, src_slice, n)
}

// ============================================================================
// SkEncodedInfo
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_encoded_info_make_image_info_types(
    color: i32,
    alpha: i32,
    pmcolor_is_bgra: bool,
    out_ct: *mut i32,
    out_at: *mut i32,
) -> bool {
    let (Some(c), Some(a)) = (EncodedColor::from_i32(color), EncodedAlpha::from_i32(alpha)) else {
        return false;
    };
    let (ct, at) = encoded_info::make_image_info_types(c, a, pmcolor_is_bgra);
    if !out_ct.is_null() {
        *out_ct = ct as i32;
    }
    if !out_at.is_null() {
        *out_at = at as i32;
    }
    true
}

#[no_mangle]
pub extern "C" fn skia_rs_encoded_info_verify_color(
    color: i32,
    alpha: i32,
    bits_per_component: i32,
) -> bool {
    let (Some(c), Some(a)) = (EncodedColor::from_i32(color), EncodedAlpha::from_i32(alpha)) else {
        return false;
    };
    encoded_info::verify_color(c, a, bits_per_component)
}

// ============================================================================
// SkSampler
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sampler_fill(
    color_type: i32,
    width: i32,
    num_rows: i32,
    dst: *mut u8,
    row_bytes: usize,
    zero_init: i32,
) {
    if dst.is_null() || width <= 0 || num_rows <= 0 {
        return;
    }
    let Some(ct) = ColorType::from_i32(color_type) else {
        return;
    };
    let zi = ZeroInitialized::from_i32(zero_init).unwrap_or(ZeroInitialized::No);
    if zi == ZeroInitialized::Yes && sampler::fill_is_zero_bytes(ct) {
        return;
    }
    let row_fill = sampler::fill_row_bytes(ct, width as usize);
    let total_len = ((num_rows as usize) - 1) * row_bytes + row_fill;
    let slice = std::slice::from_raw_parts_mut(dst, total_len);
    sampler::fill(ct, width, num_rows, slice, row_bytes, zi);
}

// ============================================================================
// SkSwizzler
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzler_select_simple(
    src_bpp: i32,
    dst_ct: i32,
    dst_width: i32,
    subset: *const IRect,
    frame: *const IRect,
    out_init: *mut SwizzlerInit,
) -> bool {
    let Some(ct) = ColorType::from_i32(dst_ct) else {
        return false;
    };
    let Some(init) = swizzler::select_simple_swizzler(
        src_bpp,
        ct,
        dst_width,
        opt_irect(subset),
        opt_irect(frame),
    ) else {
        return false;
    };
    if !out_init.is_null() {
        *out_init = init;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzler_select(
    encoded_color: i32,
    encoded_alpha: i32,
    encoded_bpc: u8,
    has_ctable: bool,
    dst_ct: i32,
    dst_at: i32,
    dst_width: i32,
    zero_init: i32,
    subset: *const IRect,
    frame: *const IRect,
    out_init: *mut SwizzlerInit,
) -> bool {
    let (Some(ec), Some(ea), Some(ct), Some(at), Some(zi)) = (
        EncodedColor::from_i32(encoded_color),
        EncodedAlpha::from_i32(encoded_alpha),
        ColorType::from_i32(dst_ct),
        AlphaType::from_i32(dst_at),
        ZeroInitialized::from_i32(zero_init),
    ) else {
        return false;
    };
    let Some(init) = swizzler::select_swizzler(
        ec,
        ea,
        encoded_bpc,
        has_ctable,
        ct,
        at,
        dst_width,
        zi,
        opt_irect(subset),
        opt_irect(frame),
    ) else {
        return false;
    };
    if !out_init.is_null() {
        *out_init = init;
    }
    true
}

#[no_mangle]
pub extern "C" fn skia_rs_swizzler_compute_sample_x(
    sample_x: i32,
    src_offset: i32,
    dst_offset: i32,
    src_width: i32,
    dst_width: i32,
    src_bpp: i32,
    dst_bpp: i32,
    has_fast_proc: bool,
) -> SwizzlerSampleXResult {
    swizzler::compute_sample_x(
        sample_x,
        src_offset,
        dst_offset,
        src_width,
        dst_width,
        src_bpp,
        dst_bpp,
        has_fast_proc,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzler_run(
    proc_id: i32,
    dst: *mut u8,
    src: *const u8,
    dst_width: i32,
    bpp: i32,
    delta_src: i32,
    offset: i32,
    ctable: *const u32,
    pmcolor_is_bgra: bool,
) {
    if dst.is_null() || src.is_null() || dst_width <= 0 || bpp <= 0 {
        return;
    }
    let Some(proc) = SwizzleProcId::from_i32(proc_id) else {
        return;
    };
    let w = dst_width as usize;
    let bpp_u = bpp as usize;
    let delta_u = delta_src as usize;
    let off_u = offset as usize;

    let dst_len = w * proc.dst_bytes_per_pixel(bpp_u);
    let src_len = if proc.is_bit_packed() {
        (off_u + (w - 1) * delta_u + bpp_u - 1) / 8 + 1
    } else {
        off_u + (w - 1) * delta_u + bpp_u
    };

    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let src_slice = std::slice::from_raw_parts(src, src_len);
    let ct_len = proc.ctable_len(bpp_u);
    let ctable_slice = if ct_len > 0 && !ctable.is_null() {
        std::slice::from_raw_parts(ctable, ct_len)
    } else {
        &[]
    };

    swizzler::swizzle_row(
        proc,
        dst_slice,
        src_slice,
        w,
        bpp_u,
        delta_u,
        off_u,
        ctable_slice,
        pmcolor_is_bgra,
    );
}

// ============================================================================
// SkMaskSwizzler
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_swizzler_select(
    dst_ct: i32,
    dst_at: i32,
    dst_width: i32,
    src_is_opaque: bool,
    bits_per_pixel: u32,
    subset: *const IRect,
    out_init: *mut MaskSwizzlerInit,
) -> bool {
    let (Some(ct), Some(at)) = (ColorType::from_i32(dst_ct), AlphaType::from_i32(dst_at)) else {
        return false;
    };
    let Some(init) = mask_swizzler::select_mask_swizzler(
        ct,
        at,
        dst_width,
        src_is_opaque,
        bits_per_pixel,
        opt_irect(subset),
    ) else {
        return false;
    };
    if !out_init.is_null() {
        *out_init = init;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_swizzler_compute_sample_x(
    src_offset: i32,
    subset_width: i32,
    sample_x: i32,
    out_x0: *mut i32,
    out_dst_width: *mut i32,
) {
    let (x0, dst_w) = mask_swizzler::compute_mask_sample_x(src_offset, subset_width, sample_x);
    if !out_x0.is_null() {
        *out_x0 = x0;
    }
    if !out_dst_width.is_null() {
        *out_dst_width = dst_w;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_swizzler_run(
    proc_id: i32,
    dst: *mut u8,
    src: *const u8,
    width: i32,
    masks: *const Masks,
    start_x: u32,
    sample_x: u32,
) {
    if dst.is_null() || src.is_null() || masks.is_null() || width <= 0 {
        return;
    }
    let Some(proc) = MaskSwizzleProcId::from_i32(proc_id) else {
        return;
    };
    let w = width as usize;
    let sx = start_x as usize;
    let step = sample_x as usize;
    let dst_len = w * proc.dst_bytes_per_pixel();
    let src_bpp = proc.src_bytes_per_pixel();
    let src_len = (sx + (w - 1) * step) * src_bpp + src_bpp;

    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let src_slice = std::slice::from_raw_parts(src, src_len);
    mask_swizzler::swizzle_mask_row(proc, dst_slice, src_slice, w, &*masks, sx, step);
}

// ============================================================================
// SkCodec / SkFrame / SkFrameHolder / SkCodecImageGenerator
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_codec_result_to_string(result: i32) -> *const c_char {
    codec::result_to_cstr(result).as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_is_valid_selection_policy(policy: i32) -> bool {
    codec::is_valid_selection_policy(policy)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_codec_classify_decoder_id(id: *const u8, len: usize) -> i32 {
    let bytes = if id.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(id, len)
    };
    codec::classify_decoder_id(bytes) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_no_decoder_match_result(bytes_read: usize) -> i32 {
    codec::no_decoder_match_result(bytes_read) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_conversion_supported(
    encoded_color: i32,
    dst_ct: i32,
    dst_at: i32,
    src_is_opaque: bool,
) -> bool {
    let (Some(ec), Some(ct), Some(at)) = (
        EncodedColor::from_i32(encoded_color),
        ColorType::from_i32(dst_ct),
        AlphaType::from_i32(dst_at),
    ) else {
        return false;
    };
    codec::conversion_supported(ec, ct, at, src_is_opaque)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_codec_select_xform_format(
    color_type: i32,
    for_color_table: bool,
    pmcolor_is_rgba: bool,
    out_format: *mut i32,
) -> bool {
    let Some(ct) = ColorType::from_i32(color_type) else {
        return false;
    };
    let Some(fmt) = codec::select_xform_format(ct, for_color_table, pmcolor_is_rgba) else {
        return false;
    };
    if !out_format.is_null() {
        *out_format = fmt;
    }
    true
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_color_type_forces_xform(dst_ct: i32) -> bool {
    ColorType::from_i32(dst_ct).is_some_and(codec::color_type_forces_xform)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_finalize_color_xform(
    encoded_color: i32,
    encoded_alpha: i32,
    dst_ct: i32,
    dst_at: i32,
    pmcolor_is_rgba: bool,
) -> ColorXformPlan {
    let (Some(ec), Some(ea), Some(ct), Some(at)) = (
        EncodedColor::from_i32(encoded_color),
        EncodedAlpha::from_i32(encoded_alpha),
        ColorType::from_i32(dst_ct),
        AlphaType::from_i32(dst_at),
    ) else {
        return ColorXformPlan {
            success: false,
            xform_time: 0,
            dst_xform_format: 0,
            dst_xform_alpha_format: 1,
        };
    };
    codec::finalize_color_xform(ec, ea, ct, at, pmcolor_is_rgba)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_codec_allocate_from_budget(
    budget: *mut usize,
    num_bytes: usize,
) -> bool {
    if budget.is_null() {
        return false;
    }
    codec::allocate_from_budget(&mut *budget, num_bytes)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_compute_zero_rect(
    dst_dims: ISize,
    src_dims: ISize,
    prev_rect: IRect,
    bpp: usize,
    row_bytes: usize,
) -> ZeroRectPlan {
    codec::compute_zero_rect(dst_dims, src_dims, prev_rect, bpp, row_bytes)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_validate_get_pixels(
    dst_ct: i32,
    has_pixels: bool,
    row_bytes: usize,
    min_row_bytes: usize,
) -> i32 {
    let ct = ColorType::from_i32(dst_ct).unwrap_or(ColorType::Unknown);
    codec::validate_get_pixels_params(ct, has_pixels, row_bytes, min_row_bytes) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_codec_validate_start_incremental(
    supports_incremental: bool,
    dst_ct: i32,
    has_pixels: bool,
    dst_dims: ISize,
    subset: *const IRect,
) -> i32 {
    let ct = ColorType::from_i32(dst_ct).unwrap_or(ColorType::Unknown);
    codec::validate_start_incremental_decode(
        supports_incremental,
        ct,
        has_pixels,
        dst_dims,
        opt_irect(subset),
    ) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_codec_validate_start_scanline(
    dst_dims: ISize,
    subset: *const IRect,
    frame_index: i32,
) -> i32 {
    codec::validate_start_scanline_decode(dst_dims, opt_irect(subset), frame_index) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_validate_get_scanlines(
    curr_scanline: i32,
    dst_height: i32,
    count_lines: i32,
) -> bool {
    codec::validate_get_scanlines(curr_scanline, dst_height, count_lines)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_validate_skip_scanlines(
    curr_scanline: i32,
    dst_height: i32,
    count_lines: i32,
) -> bool {
    codec::validate_skip_scanlines(curr_scanline, dst_height, count_lines)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_output_scanline(
    order: i32,
    encoded_height: i32,
    input_scanline: i32,
) -> i32 {
    let Some(ord) = ScanlineOrder::from_i32(order) else {
        return 0;
    };
    codec::output_scanline(ord, encoded_height, input_scanline)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_validate_nonzero_frame_index(
    frame_index: i32,
    has_subset: bool,
    frame_count: i32,
) -> i32 {
    codec::validate_nonzero_frame_index(frame_index, has_subset, frame_count) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_validate_prior_frame_range(
    prior_frame: i32,
    required_frame: i32,
    frame_index: i32,
) -> bool {
    codec::validate_prior_frame_range(prior_frame, required_frame, frame_index)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_classify_prior_frame_disposal(
    disposal: i32,
    prepped_frame_id: i32,
    required_frame: i32,
) -> i32 {
    let disp = DisposalMethod::from_i32(disposal).unwrap_or(DisposalMethod::Keep);
    codec::classify_prior_frame_disposal(disp, prepped_frame_id, required_frame) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_plan_fill_incomplete_image(
    dst_ct: i32,
    dst_width: i32,
    row_bytes: usize,
    zero_init: i32,
    lines_requested: i32,
    lines_decoded: i32,
    has_frame: bool,
    frame_required: i32,
    frame_rect_matches_bounds: bool,
    sampler_fill_width: i32,
    subset_width: i32,
    scanline_order: i32,
) -> FillIncompletePlan {
    let ct = ColorType::from_i32(dst_ct).unwrap_or(ColorType::Unknown);
    let zi = ZeroInitialized::from_i32(zero_init).unwrap_or(ZeroInitialized::No);
    let ord = ScanlineOrder::from_i32(scanline_order).unwrap_or(ScanlineOrder::TopDown);
    codec::plan_fill_incomplete_image(
        ct,
        dst_width,
        row_bytes,
        zi,
        lines_requested,
        lines_decoded,
        has_frame,
        frame_required,
        frame_rect_matches_bounds,
        sampler_fill_width,
        subset_width,
        ord,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_frame_fill_in(
    required_frame: i32,
    duration: i32,
    fully_received: bool,
    has_alpha: bool,
    reported_alpha: i32,
    disposal_method: i32,
    blend: i32,
    frame_rect: IRect,
) -> FrameInfoFields {
    let ra = EncodedAlpha::from_i32(reported_alpha).unwrap_or(EncodedAlpha::Opaque);
    frame_holder::fill_frame_info(
        required_frame,
        duration,
        fully_received,
        has_alpha,
        ra,
        disposal_method,
        blend,
        frame_rect,
    )
}

pub type GetFrameSummaryFn = extern "C" fn(ctx: *const c_void, id: i32) -> FrameSummary;

#[no_mangle]
pub extern "C" fn skia_rs_frame_holder_compute_dependency(
    screen_width: i32,
    screen_height: i32,
    frame: FrameSummary,
    ctx: *const c_void,
    get_frame: GetFrameSummaryFn,
) -> FrameDependencyResult {
    frame_holder::compute_alpha_and_required_frame(screen_width, screen_height, frame, |id| {
        get_frame(ctx, id)
    })
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_adjust_generator_info(
    width: i32,
    height: i32,
    alpha_type: i32,
    origin: i32,
    has_requested_alpha: bool,
    requested_alpha: i32,
) -> GeneratorInfoAdjust {
    let at = AlphaType::from_i32(alpha_type).unwrap_or(AlphaType::Unknown);
    let req = AlphaType::from_i32(requested_alpha).unwrap_or(AlphaType::Unknown);
    let org = EncodedOrigin::from_i32(origin).unwrap_or(EncodedOrigin::TopLeft);
    codec::adjust_generator_info(width, height, at, org, has_requested_alpha, req)
}

#[no_mangle]
pub extern "C" fn skia_rs_codec_is_partial_or_full_success(result: i32) -> bool {
    CodecResult::from_i32(result).is_some_and(codec::is_partial_or_full_success)
}

// ============================================================================
// SkPixmapUtils
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_pixmap_utils_validate_orient(
    src_ct: i32,
    dst_ct: i32,
    src_w: i32,
    src_h: i32,
    dst_w: i32,
    dst_h: i32,
    origin: i32,
    same_addr: bool,
) -> i32 {
    let (Some(sct), Some(dct), Some(org)) = (
        ColorType::from_i32(src_ct),
        ColorType::from_i32(dst_ct),
        EncodedOrigin::from_i32(origin),
    ) else {
        return pixmap_utils::OrientAction::Invalid as i32;
    };
    pixmap_utils::validate_orient(sct, dct, src_w, src_h, dst_w, dst_h, org, same_addr) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_pixmap_utils_swap_wh(size: ISize) -> ISize {
    pixmap_utils::swap_wh(size)
}

// ============================================================================
// SkEncoder
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_encoder_clamp_rows(
    curr_row: i32,
    height: i32,
    num_rows: i32,
    out_clamped: *mut i32,
) -> bool {
    let Some(clamped) = encoder::clamp_encode_rows(curr_row, height, num_rows) else {
        return false;
    };
    if !out_clamped.is_null() {
        *out_clamped = clamped;
    }
    true
}

// ============================================================================
// SkAndroidCodec / SkAndroidCodecAdapter / SkSampledCodec
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_select_kind(format: i32) -> i32 {
    let Some(fmt) = EncodedImageFormat::from_i32(format) else {
        return android_codec::AndroidCodecKind::Unsupported as i32;
    };
    android_codec::select_android_codec_kind(fmt) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_compute_output_color_type(
    requested_ct: i32,
    info_ct: i32,
    info_at: i32,
    bits_per_component: u8,
    color_depth: u8,
    pmcolor_is_bgra: bool,
) -> i32 {
    let req = ColorType::from_i32(requested_ct).unwrap_or(ColorType::Unknown);
    let ict = ColorType::from_i32(info_ct).unwrap_or(ColorType::Unknown);
    let iat = AlphaType::from_i32(info_at).unwrap_or(AlphaType::Unknown);
    android_codec::compute_output_color_type(
        req,
        ict,
        iat,
        bits_per_component,
        color_depth,
        pmcolor_is_bgra,
    ) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_compute_output_alpha_type(
    info_at: i32,
    requested_unpremul: bool,
) -> i32 {
    let iat = AlphaType::from_i32(info_at).unwrap_or(AlphaType::Unknown);
    android_codec::compute_output_alpha_type(iat, requested_unpremul) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_supports_output_color_space(output_ct: i32) -> bool {
    ColorType::from_i32(output_ct).is_some_and(android_codec::supports_output_color_space)
}

pub type IsAnimatedWebpFn = extern "C" fn(ctx: *const c_void) -> bool;
pub type GetSampledDimsFn = extern "C" fn(ctx: *const c_void, sample_size: i32) -> ISize;

#[no_mangle]
pub unsafe extern "C" fn skia_rs_android_codec_compute_sample_size(
    orig_dims: ISize,
    desired_size: *mut ISize,
    is_webp: bool,
    ctx: *const c_void,
    is_animated_webp: IsAnimatedWebpFn,
    get_sampled_dims: GetSampledDimsFn,
) -> i32 {
    if desired_size.is_null() {
        return 1;
    }
    android_codec::compute_sample_size(
        orig_dims,
        &mut *desired_size,
        is_webp,
        || is_animated_webp(ctx),
        |ss| get_sampled_dims(ctx, ss),
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_is_valid_sample_size(sample_size: i32) -> bool {
    android_codec::is_valid_sample_size(sample_size)
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_get_sampled_dimension(
    src_dimension: i32,
    sample_size: i32,
) -> i32 {
    sampler::get_sampled_dimension(src_dimension, sample_size)
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_get_scale_from_sample_size(sample_size: i32) -> f32 {
    sampler::get_scale_from_sample_size(sample_size)
}

#[no_mangle]
pub extern "C" fn skia_rs_android_codec_is_valid_subset(subset: IRect, image_dims: ISize) -> bool {
    sampler::is_valid_subset(subset, image_dims)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_android_codec_validate_pixels(
    has_pixels: bool,
    row_bytes: usize,
    min_row_bytes: usize,
    codec_dims: ISize,
    subset: *const IRect,
) -> AndroidPixelsValidation {
    android_codec::validate_android_pixels(
        has_pixels,
        row_bytes,
        min_row_bytes,
        codec_dims,
        opt_irect(subset),
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_sampled_codec_plan_native_scaling(
    format: i32,
    sample_size: i32,
) -> NativeScalingPlan {
    let fmt = EncodedImageFormat::from_i32(format).unwrap_or(EncodedImageFormat::Png);
    android_codec::plan_native_scaling(fmt, sample_size)
}

#[no_mangle]
pub extern "C" fn skia_rs_sampled_codec_compute_scaled_subset(
    subset: IRect,
    sample_size: i32,
    dst_dims: ISize,
    scaled_size: ISize,
) -> ScaledSubsetPlan {
    android_codec::compute_scaled_subset(subset, sample_size, dst_dims, scaled_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sampled_codec_compute_decode_params(
    native_size: ISize,
    dst_dims: ISize,
    native_sample_size: i32,
    subset: *const IRect,
) -> SampledDecodeParams {
    android_codec::compute_sampled_decode_params(
        native_size,
        dst_dims,
        native_sample_size,
        opt_irect(subset),
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_sampled_codec_is_coord_necessary(
    src_coord: i32,
    sample_factor: i32,
    scaled_dim: i32,
) -> bool {
    sampler::is_coord_necessary(src_coord, sample_factor, scaled_dim)
}

#[no_mangle]
pub extern "C" fn skia_rs_sampled_codec_get_dst_coord(src_coord: i32, sample_factor: i32) -> i32 {
    sampler::get_dst_coord(src_coord, sample_factor)
}
