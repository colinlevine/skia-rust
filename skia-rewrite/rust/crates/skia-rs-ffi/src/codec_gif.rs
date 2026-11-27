//! C++/Rust FFI bindings for `skia-rs-codec-gif` (`P2.7`).

use skia_rs_codec_core::{
    frame_holder::FrameInfoFields,
    types::{CodecResult, SelectionPolicy},
};
use skia_rs_codec_gif::{
    format::{self, GifEncodedInfo},
    io_buf::{self, CompactPlan, IoBufferMeta, SeekBufferPlan},
    state_machine::{
        FrameCountStepResult, GetFrameCountPlan, GifCodecState, StartIncrDecodePlan,
        TwoPassClearPlan, TwoPassDecodePlan,
    },
    types::{self, IsAnimated, WuffsFrameConfigInput, WuffsRectU32, WuffsStatusKind},
};
use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::ISize;

// ============================================================================
// Sniffing, Stream Policy, Encoded Info, Format, and Scaling Helpers
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_is_gif(buf: *const u8, len: usize) -> bool {
    if buf.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    types::is_gif(slice)
}

#[no_mangle]
pub extern "C" fn skia_rs_gif_should_copy_stream_to_memory(
    selection_policy: i32,
    can_seek: bool,
) -> bool {
    let policy =
        SelectionPolicy::from_i32(selection_policy).unwrap_or(SelectionPolicy::PreferAnimation);
    format::should_copy_stream_to_memory(policy, can_seek)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_n32_wuffs_pixfmt(
    n32_ct: i32,
    out_pixfmt: *mut u32,
) -> i32 {
    if out_pixfmt.is_null() {
        return CodecResult::InternalError as i32;
    }
    let Some(ct) = ColorType::from_i32(n32_ct) else {
        return CodecResult::InternalError as i32;
    };
    match format::n32_color_type_to_wuffs_pixfmt(ct) {
        Ok(pf) => {
            *out_pixfmt = pf;
            CodecResult::Success as i32
        }
        Err(err) => err as i32,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_classify_encoded_info(
    width: u32,
    height: u32,
    pixfmt: u32,
    first_frame_is_opaque: bool,
    out_info: *mut GifEncodedInfo,
) -> i32 {
    if out_info.is_null() {
        return CodecResult::InternalError as i32;
    }
    match format::classify_encoded_info(width, height, pixfmt, first_frame_is_opaque) {
        Ok(info) => {
            *out_info = info;
            CodecResult::Success as i32
        }
        Err(err) => err as i32,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_validate_buffer_len(
    len_u64: u64,
    out_len: *mut usize,
) -> i32 {
    if out_len.is_null() {
        return CodecResult::InternalError as i32;
    }
    match format::validate_buffer_len(len_u64) {
        Ok(len) => {
            *out_len = len;
            CodecResult::Success as i32
        }
        Err(err) => err as i32,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_gif_map_wuffs_decode_status(status_kind: i32) -> i32 {
    let status = WuffsStatusKind::from_i32(status_kind).unwrap_or(WuffsStatusKind::Error);
    format::map_wuffs_decode_status(status) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_gif_loops_to_repetition_count(num_animation_loops: u32) -> i32 {
    format::wuffs_loops_to_repetition_count(num_animation_loops)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_scaled_dimensions(
    codec_w: i32,
    codec_h: i32,
    desired_scale: f32,
    out_w: *mut i32,
    out_h: *mut i32,
) {
    if out_w.is_null() || out_h.is_null() {
        return;
    }
    let scaled = format::scaled_dimensions(
        ISize {
            width: codec_w,
            height: codec_h,
        },
        desired_scale,
    );
    *out_w = scaled.width;
    *out_h = scaled.height;
}

#[no_mangle]
pub extern "C" fn skia_rs_gif_dimensions_supported(
    codec_w: i32,
    codec_h: i32,
    req_w: i32,
    req_h: i32,
) -> bool {
    format::dimensions_supported(
        ISize {
            width: codec_w,
            height: codec_h,
        },
        ISize {
            width: req_w,
            height: req_h,
        },
    )
}

// ============================================================================
// I/O Buffer Metadata, Compact, and Seek Planning
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_io_buffer_plan_seek(
    meta: *mut IoBufferMeta,
    target_pos: u64,
    out_plan: *mut SeekBufferPlan,
) -> bool {
    if meta.is_null() || out_plan.is_null() {
        return false;
    }
    *out_plan = io_buf::plan_seek_buffer(&mut *meta, target_pos);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_io_buffer_apply_stream_seek(
    meta: *mut IoBufferMeta,
    target_pos: u64,
) {
    if !meta.is_null() {
        io_buf::apply_stream_seek_success(&mut *meta, target_pos);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_io_buffer_on_bytes_read(
    meta: *mut IoBufferMeta,
    num_read: usize,
) -> bool {
    if meta.is_null() {
        return false;
    }
    (*meta).on_bytes_read(num_read)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_io_buffer_compact(
    meta: *mut IoBufferMeta,
    out_plan: *mut CompactPlan,
) -> bool {
    if meta.is_null() || out_plan.is_null() {
        return false;
    }
    *out_plan = io_buf::compact_io_buffer_meta(&mut *meta);
    true
}

// ============================================================================
// GifCodecState Lifecycle, Frame Holder, and Incremental Decode State Machine
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_gif_state_new(
    width: i32,
    height: i32,
    can_seek: bool,
    first_frame_io_pos: u64,
) -> *mut GifCodecState {
    Box::into_raw(Box::new(GifCodecState::new(
        width,
        height,
        can_seek,
        first_frame_io_pos,
    )))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_free(state: *mut GifCodecState) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_decoder_is_suspended(
    state: *const GifCodecState,
) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).decoder_is_suspended()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_on_decode_step_finished(
    state: *mut GifCodecState,
    is_complete: bool,
    num_decoded_frames: u64,
) {
    if !state.is_null() {
        (*state).on_decode_step_finished(is_complete, num_decoded_frames);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_update_num_fully_received_frames(
    state: *mut GifCodecState,
    num_decoded_frames: u64,
) {
    if !state.is_null() {
        (*state).update_num_fully_received_frames(num_decoded_frames);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_on_reset_decoder_result(
    state: *mut GifCodecState,
    reset_result: i32,
) -> i32 {
    if state.is_null() {
        return CodecResult::InternalError as i32;
    }
    let res = CodecResult::from_i32(reset_result).unwrap_or(CodecResult::InternalError);
    (*state).on_reset_decoder_result(res) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_resolve_frame_io_position(
    state: *const GifCodecState,
    frame_index: i32,
    out_pos: *mut u64,
) -> i32 {
    if state.is_null() || out_pos.is_null() {
        return CodecResult::InternalError as i32;
    }
    match (*state).resolve_frame_io_position(frame_index) {
        Ok(pos) => {
            *out_pos = pos;
            CodecResult::Success as i32
        }
        Err(err) => err as i32,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_plan_get_frame_count(
    state: *const GifCodecState,
    out_plan: *mut GetFrameCountPlan,
) -> bool {
    if state.is_null() || out_plan.is_null() {
        return false;
    }
    *out_plan = (*state).plan_get_frame_count();
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_on_frame_count_step(
    state: *mut GifCodecState,
    index: i32,
    status_kind: i32,
    fc: *const WuffsFrameConfigInput,
    out_step: *mut FrameCountStepResult,
) -> bool {
    if state.is_null() || out_step.is_null() {
        return false;
    }
    let status = WuffsStatusKind::from_i32(status_kind).unwrap_or(WuffsStatusKind::Error);
    let fc_val = if fc.is_null() {
        WuffsFrameConfigInput::default()
    } else {
        *fc
    };
    *out_step = (*state).on_frame_count_step(index, status, fc_val);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_mark_frames_complete(state: *mut GifCodecState) {
    if !state.is_null() {
        (*state).mark_frames_complete();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_frame_count(state: *const GifCodecState) -> i32 {
    if state.is_null() {
        return 0;
    }
    (*state).frame_count()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_get_frame_info(
    state: *const GifCodecState,
    index: i32,
    out_info: *mut FrameInfoFields,
) -> bool {
    if state.is_null() {
        return false;
    }
    match (*state).get_frame_info(index) {
        Some(info) => {
            if !out_info.is_null() {
                *out_info = info;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_is_animated(state: *const GifCodecState) -> i32 {
    if state.is_null() {
        return IsAnimated::Unknown as i32;
    }
    (*state).is_animated() as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_gif_validate_start_incremental_decode(
    has_dst: bool,
    has_subset: bool,
) -> i32 {
    GifCodecState::validate_start_incremental_decode(has_dst, has_subset) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_begin_start_incremental_decode(
    state: *mut GifCodecState,
    dst_ct: i32,
    dst_w: i32,
    dst_h: i32,
    has_color_profile: bool,
    frame_index: i32,
    out_plan: *mut StartIncrDecodePlan,
) -> bool {
    if state.is_null() || out_plan.is_null() {
        return false;
    }
    let ct = ColorType::from_i32(dst_ct).unwrap_or(ColorType::Unknown);
    *out_plan = (*state).begin_start_incremental_decode(
        ct,
        ISize {
            width: dst_w,
            height: dst_h,
        },
        has_color_profile,
        frame_index,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_plan_two_pass_clear(
    src_bits_per_pixel: u32,
    frame_rect: WuffsRectU32,
    pixels_width_bytes: usize,
    pixels_height: usize,
    pixels_stride: usize,
    out_plan: *mut TwoPassClearPlan,
) -> bool {
    if out_plan.is_null() {
        return false;
    }
    *out_plan = GifCodecState::plan_two_pass_clear(
        src_bits_per_pixel,
        frame_rect,
        pixels_width_bytes,
        pixels_height,
        pixels_stride,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_commit_start_incremental_decode(
    state: *mut GifCodecState,
) {
    if !state.is_null() {
        (*state).commit_start_incremental_decode();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_check_incremental_decode_active(
    state: *const GifCodecState,
) -> i32 {
    if state.is_null() {
        return CodecResult::InternalError as i32;
    }
    (*state).check_incremental_decode_active() as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_is_one_pass(state: *const GifCodecState) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).incr_dec_one_pass()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_pixel_blend(state: *const GifCodecState) -> u8 {
    if state.is_null() {
        return 0;
    }
    (*state).incr_dec_pixel_blend() as u8
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn skia_rs_gif_state_plan_two_pass_decode(
    state: *mut GifCodecState,
    status_kind: i32,
    frame_index: i32,
    encoded_info_opaque: bool,
    src_bits_per_pixel: u32,
    frame_rect: WuffsRectU32,
    dirty_rect: WuffsRectU32,
    pixels_width_bytes: usize,
    pixels_height: usize,
    pixels_stride: usize,
    out_plan: *mut TwoPassDecodePlan,
) -> bool {
    if state.is_null() || out_plan.is_null() {
        return false;
    }
    let status = WuffsStatusKind::from_i32(status_kind).unwrap_or(WuffsStatusKind::Error);
    *out_plan = (*state).plan_incremental_decode_two_pass(
        status,
        frame_index,
        encoded_info_opaque,
        src_bits_per_pixel,
        frame_rect,
        dirty_rect,
        pixels_width_bytes,
        pixels_height,
        pixels_stride,
    );
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gif_state_finish_incremental_decode(
    state: *mut GifCodecState,
    result: i32,
) {
    if !state.is_null() {
        let res = CodecResult::from_i32(result).unwrap_or(CodecResult::InternalError);
        (*state).finish_incremental_decode(res);
    }
}
