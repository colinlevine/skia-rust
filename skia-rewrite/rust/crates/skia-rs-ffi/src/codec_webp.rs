//! C++/Rust FFI bindings for `skia-rs-codec-webp` (`P2.6`).

use skia_rs_codec_core::types::{CodecResult, EncodedAlpha, EncodedColor};
use skia_rs_codec_meta::color_profile::DataSpace;
use skia_rs_codec_webp::{
    animation::{self, WebpFrameMeta},
    container::{self, RiffHeaderReader, RiffReadStep, RiffStepKind},
    decoder::{self, WebpGetPixelsPlan, WebpIdecUpdateOutcome},
    encoder::{
        self, WebpAnimatedEncodeState, WebpEncodePlan, WebpMuxPlan, WebpScanlineConvertKind,
    },
    types::{
        Vp8StatusCode, WebpCompression, WebpCspMode, WebpDemuxState, WebpMuxAnimBlend,
        WebpMuxAnimDispose,
    },
};
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::{IRect, ISize};

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
// WebP Container / Sniffing / Streaming RIFF Header Reader (`webp_decode`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_is_webp(buf: *const u8, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    container::is_webp(slice)
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_riff_reader_new() -> *mut RiffHeaderReader {
    Box::into_raw(Box::new(RiffHeaderReader::new()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_riff_reader_free(reader: *mut RiffHeaderReader) {
    if !reader.is_null() {
        drop(Box::from_raw(reader));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_riff_reader_next_step(
    reader: *mut RiffHeaderReader,
    stream_at_end: bool,
    out_step: *mut RiffReadStep,
) -> bool {
    if reader.is_null() || out_step.is_null() {
        return false;
    }
    *out_step = (*reader).next_step(stream_at_end);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_riff_reader_feed(
    reader: *mut RiffHeaderReader,
    bytes: *const u8,
    len: usize,
    stream_at_end: bool,
    out_step: *mut RiffReadStep,
) -> bool {
    if reader.is_null() || out_step.is_null() {
        return false;
    }
    let slice = if bytes.is_null() || len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(bytes, len)
    };
    *out_step = (*reader).feed(slice, stream_at_end);
    (*out_step).kind != RiffStepKind::Invalid as i32
}

// ============================================================================
// WebP Decoder Header, Subset, Animation, and GetPixels Planning (`webp_decode`)
// ============================================================================

/// Returns `SkCodec::kSuccess` (0) if demux state is `ParsedHeader` / `Done`,
/// or the appropriate error `SkCodec::Result` otherwise.
#[no_mangle]
pub extern "C" fn skia_rs_webp_map_demux_state(state: i32) -> i32 {
    match WebpDemuxState::from_i32(state) {
        Some(s) => match decoder::map_demux_state(s) {
            None => CodecResult::Success as i32,
            Some(err) => err as i32,
        },
        None => CodecResult::InvalidInput as i32,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_validate_canvas_dimensions(width: i32, height: i32) -> bool {
    decoder::validate_canvas_dimensions(width, height)
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_should_keep_color_profile(data_space: i32) -> bool {
    match data_space_from_i32(data_space) {
        Some(ds) => decoder::should_keep_color_profile(ds),
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_map_features_status(status: i32) -> i32 {
    match Vp8StatusCode::from_i32(status) {
        Some(s) => decoder::map_features_status(s) as i32,
        None => CodecResult::InvalidInput as i32,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_classify_encoded_info(
    canvas_width: i32,
    canvas_height: i32,
    frame_width: i32,
    frame_height: i32,
    frame_has_alpha: bool,
    bitstream_format: i32,
    out_color: *mut i32,
    out_alpha: *mut i32,
) -> bool {
    if out_color.is_null() || out_alpha.is_null() {
        return false;
    }
    match decoder::classify_encoded_info(
        canvas_width,
        canvas_height,
        frame_width,
        frame_height,
        frame_has_alpha,
        bitstream_format,
    ) {
        Some((color, alpha)) => {
            *out_color = color as i32;
            *out_alpha = alpha as i32;
            true
        }
        None => {
            *out_color = EncodedColor::Bgra as i32;
            *out_alpha = EncodedAlpha::Unpremul as i32;
            false
        }
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_decode_mode(dst_ct: i32, premultiply: bool) -> i32 {
    match ColorType::from_i32(dst_ct) {
        Some(ct) => decoder::webp_decode_mode(ct, premultiply) as i32,
        None => WebpCspMode::Last as i32,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_get_valid_subset(
    bounds_w: i32,
    bounds_h: i32,
    left: *mut i32,
    top: *mut i32,
    right: *mut i32,
    bottom: *mut i32,
) -> bool {
    if left.is_null() || top.is_null() || right.is_null() || bottom.is_null() {
        return false;
    }
    let bounds = IRect::make_ltrb(0, 0, bounds_w, bounds_h);
    let mut desired = IRect::make_ltrb(*left, *top, *right, *bottom);
    if !decoder::get_valid_subset(bounds, &mut desired) {
        return false;
    }
    *left = desired.left;
    *top = desired.top;
    *right = desired.right;
    *bottom = desired.bottom;
    true
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_is_animated(format_flags: u32) -> bool {
    animation::is_animated(format_flags)
}

#[no_mangle]
pub extern "C" fn skia_rs_webp_repetition_count(format_flags: u32, loop_count: i32) -> i32 {
    animation::compute_repetition_count(format_flags, loop_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_map_frame_metadata(
    has_alpha: bool,
    dispose_method: i32,
    blend_method: i32,
    out_meta: *mut WebpFrameMeta,
) -> bool {
    if out_meta.is_null() {
        return false;
    }
    let dispose =
        WebpMuxAnimDispose::from_i32(dispose_method).unwrap_or(WebpMuxAnimDispose::None);
    let blend = WebpMuxAnimBlend::from_i32(blend_method).unwrap_or(WebpMuxAnimBlend::NoBlend);
    *out_meta = animation::map_frame_metadata(has_alpha, dispose, blend);
    true
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn skia_rs_webp_plan_get_pixels(
    canvas_w: i32,
    canvas_h: i32,
    frame_x: i32,
    frame_y: i32,
    frame_w: i32,
    frame_h: i32,
    frame_has_alpha: bool,
    frame_blend_method: i32,
    independent: bool,
    has_subset: bool,
    subset_left: i32,
    subset_top: i32,
    subset_right: i32,
    subset_bottom: i32,
    dst_w: i32,
    dst_h: i32,
    dst_ct: i32,
    dst_at: i32,
    dst_row_bytes: usize,
    has_color_xform: bool,
    out_plan: *mut WebpGetPixelsPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let (Some(ct), Some(at)) = (ColorType::from_i32(dst_ct), AlphaType::from_i32(dst_at)) else {
        return false;
    };
    let blend = WebpMuxAnimBlend::from_i32(frame_blend_method).unwrap_or(WebpMuxAnimBlend::NoBlend);
    let frame_rect = IRect::make_ltrb(
        frame_x,
        frame_y,
        frame_x.saturating_add(frame_w),
        frame_y.saturating_add(frame_h),
    );
    let subset = if has_subset {
        Some(IRect::make_ltrb(
            subset_left,
            subset_top,
            subset_right,
            subset_bottom,
        ))
    } else {
        None
    };

    *out_plan = decoder::plan_get_pixels(
        ISize {
            width: canvas_w,
            height: canvas_h,
        },
        frame_rect,
        frame_has_alpha,
        blend,
        independent,
        subset,
        ISize {
            width: dst_w,
            height: dst_h,
        },
        ct,
        at,
        dst_row_bytes,
        has_color_xform,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_interpret_idec_update(
    status: i32,
    scaled_height: i32,
    dst_y: i32,
    got_rgb: bool,
    idec_rows_decoded: i32,
    out_outcome: *mut WebpIdecUpdateOutcome,
) -> bool {
    if out_outcome.is_null() {
        return false;
    }
    let st = Vp8StatusCode::from_i32(status).unwrap_or(Vp8StatusCode::BitstreamError);
    *out_outcome =
        decoder::interpret_idec_update(st, scaled_height, dst_y, got_rgb, idec_rows_decoded);
    true
}

// ============================================================================
// WebP Encoder Planning, Scanline Conversion, Mux, and Animation (`webp_encode`)
// ============================================================================

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn skia_rs_webp_encoder_plan_picture(
    width: i32,
    height: i32,
    ct: i32,
    at: i32,
    has_addr: bool,
    row_bytes: usize,
    compression: i32,
    quality: f32,
    opt_method: i32,
    use_default_method_macro: bool,
    has_import_bgra_macro: bool,
    out_plan: *mut WebpEncodePlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    let (Some(color_type), Some(alpha_type), Some(comp)) = (
        ColorType::from_i32(ct),
        AlphaType::from_i32(at),
        WebpCompression::from_i32(compression),
    ) else {
        return false;
    };
    *out_plan = encoder::plan_encode_picture(
        width,
        height,
        color_type,
        alpha_type,
        has_addr,
        row_bytes,
        comp,
        quality,
        opt_method,
        use_default_method_macro,
        has_import_bgra_macro,
    );
    (*out_plan).valid
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_encoder_convert_image_to_rgba8888(
    convert_kind: i32,
    dst: *mut u8,
    dst_len: usize,
    dst_row_bytes: usize,
    src: *const u8,
    src_len: usize,
    src_row_bytes: usize,
    width: usize,
    height: usize,
) -> bool {
    if dst.is_null() || src.is_null() {
        return false;
    }
    let Some(kind) = WebpScanlineConvertKind::from_i32(convert_kind) else {
        return false;
    };
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let src_slice = std::slice::from_raw_parts(src, src_len);
    encoder::convert_image_to_rgba8888(
        kind,
        dst_slice,
        dst_row_bytes,
        src_slice,
        src_row_bytes,
        width,
        height,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_encoder_plan_mux(
    has_icc: bool,
    has_exif: bool,
    has_xmp: bool,
    out_plan: *mut WebpMuxPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    *out_plan = encoder::plan_mux(has_icc, has_exif, has_xmp);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_encoder_anim_init(
    canvas_width: i32,
    canvas_height: i32,
    out_state: *mut WebpAnimatedEncodeState,
) -> bool {
    if out_state.is_null() {
        return false;
    }
    *out_state = WebpAnimatedEncodeState::new(canvas_width, canvas_height);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_webp_encoder_anim_next_frame(
    state: *mut WebpAnimatedEncodeState,
    frame_w: i32,
    frame_h: i32,
    duration: i32,
    out_timestamp: *mut i32,
) -> bool {
    if state.is_null() || out_timestamp.is_null() {
        return false;
    }
    match (*state).next_frame_timestamp(frame_w, frame_h, duration) {
        Some(ts) => {
            *out_timestamp = ts;
            true
        }
        None => false,
    }
}
