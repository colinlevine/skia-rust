//! Safe Rust state machine for `SkWuffsCodec`:
//! - Incremental decode lifecycle (one-pass vs two-pass)
//! - Non-seekable vs seekable frame counting and `FrameInfo`
//! - Decoder suspension & reset coordination
//! - Two-pass intermediate pixel-buffer clearing, dirty-rect compositing, and memory reclamation

use skia_rs_codec_core::{
    frame_holder::FrameInfoFields,
    types::{CodecResult, EncodedAlpha, NO_FRAME},
};
use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::{IRect, ISize};

use crate::{
    format::{
        bits_per_pixel_to_bytes, decide_one_pass, map_wuffs_decode_status, to_alpha_type,
        validate_buffer_len,
    },
    frame_holder::{GifFrame, GifFrameHolder},
    types::{IsAnimated, WuffsFrameConfigInput, WuffsPixelBlend, WuffsRectU32, WuffsStatusKind},
};

/// Result of `GifCodecState::plan_get_frame_count`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GetFrameCountPlan {
    /// If `true`, caller must run the frame-config scan loop starting at `start_index`.
    pub need_scan: bool,
    /// If `need_scan == false`, the immediate frame count to return.
    pub immediate_count: i32,
    /// If `need_scan == true`, the starting frame index to seek to (`n ? n - 1 : 0`).
    pub start_index: i32,
}

/// Action returned by `GifCodecState::on_frame_count_step`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FrameCountStepAction {
    /// Frame was already recorded; continue loop.
    Continue = 0,
    /// New frame was appended; caller should mirror `appended_frame` in `SkWuffsFrameHolder`.
    AppendedFrame = 1,
    /// Reached `end_of_data`; `frames_complete` is now true and scan is finished.
    Completed = 2,
    /// Hit `short_read` or error; abort scan without marking `frames_complete`.
    Abort = 3,
}

/// Step result for `onGetFrameCountInternal`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FrameCountStepResult {
    pub action: i32,
    pub appended_frame: GifFrame,
}

/// Plan produced by `GifCodecState::begin_start_incremental_decode`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StartIncrDecodePlan {
    pub result: i32,
    pub one_pass: bool,
    /// Valid when `one_pass == true`: Wuffs pixel format for `dst`.
    pub one_pass_pixel_format: u32,
    /// Valid when `one_pass == true`: `dst_width * bytes_per_pixel`.
    pub one_pass_table_width_bytes: usize,
    /// Valid when `one_pass == true`: whether caller must call `SkSampler::Fill` on `dst`.
    pub one_pass_should_fill_dst: bool,
    /// `WuffsPixelBlend` to pass to `decode_frame`.
    pub pixel_blend: u8,
}

/// Plan for zeroing out `frame_rect` inside an already-allocated two-pass pixel buffer.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TwoPassClearPlan {
    pub result: i32,
    pub bounds_valid: bool,
    pub start_byte_offset: usize,
    pub row_byte_len: usize,
    pub num_rows: usize,
    pub contiguous: bool,
    pub contiguous_byte_len: usize,
}

/// Plan produced by `GifCodecState::plan_incremental_decode_two_pass`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TwoPassDecodePlan {
    pub result: i32,
    pub early_return: bool,
    pub should_fill_dst: bool,
    pub should_draw: bool,
    pub dirty_bounds_valid: bool,
    pub src_byte_offset: usize,
    pub dirty_width: i32,
    pub dirty_height: i32,
    pub translate_x: i32,
    pub translate_y: i32,
    pub src_alpha_type: i32,
    pub use_blend_src: bool,
    pub should_release_two_pass_pixbuf: bool,
}

/// Safe Rust core state machine for `SkWuffsCodec`.
#[derive(Clone, Debug)]
pub struct GifCodecState {
    screen_width: i32,
    screen_height: i32,
    can_seek: bool,
    first_frame_io_position: u64,
    frame_holder: GifFrameHolder,
    decoder_is_suspended: bool,

    // Incremental decode state.
    incr_dec_active: bool,
    incr_dec_one_pass: bool,
    incr_dec_pixel_blend: WuffsPixelBlend,
    first_call_to_incremental_decode: bool,
}

impl GifCodecState {
    pub fn new(
        screen_width: i32,
        screen_height: i32,
        can_seek: bool,
        first_frame_io_position: u64,
    ) -> Self {
        Self {
            screen_width,
            screen_height,
            can_seek,
            first_frame_io_position,
            frame_holder: GifFrameHolder::new(screen_width, screen_height),
            decoder_is_suspended: false,
            incr_dec_active: false,
            incr_dec_one_pass: false,
            incr_dec_pixel_blend: WuffsPixelBlend::Src,
            first_call_to_incremental_decode: false,
        }
    }

    #[inline]
    pub fn dimensions(&self) -> ISize {
        ISize {
            width: self.screen_width,
            height: self.screen_height,
        }
    }

    #[inline]
    pub fn bounds(&self) -> IRect {
        IRect::from_wh(self.screen_width, self.screen_height)
    }

    #[inline]
    pub fn can_seek(&self) -> bool {
        self.can_seek
    }

    #[inline]
    pub fn frame_holder(&self) -> &GifFrameHolder {
        &self.frame_holder
    }

    #[inline]
    pub fn frame_count(&self) -> i32 {
        self.frame_holder.len() as i32
    }

    #[inline]
    pub fn decoder_is_suspended(&self) -> bool {
        self.decoder_is_suspended
    }

    #[inline]
    pub fn incr_dec_active(&self) -> bool {
        self.incr_dec_active
    }

    #[inline]
    pub fn incr_dec_one_pass(&self) -> bool {
        self.incr_dec_one_pass
    }

    #[inline]
    pub fn incr_dec_pixel_blend(&self) -> WuffsPixelBlend {
        self.incr_dec_pixel_blend
    }

    #[inline]
    pub fn first_call_to_incremental_decode(&self) -> bool {
        self.first_call_to_incremental_decode
    }

    /// Exact port of `SkWuffsCodec::updateNumFullyReceivedFrames`.
    #[inline]
    pub fn update_num_fully_received_frames(&mut self, num_decoded_frames: u64) {
        self.frame_holder
            .update_num_fully_received_frames(num_decoded_frames);
    }

    /// Called after `decode_frame_config` or `decode_frame` completes its retry loop.
    #[inline]
    pub fn on_decode_step_finished(&mut self, is_complete: bool, num_decoded_frames: u64) {
        self.decoder_is_suspended = !is_complete;
        self.update_num_fully_received_frames(num_decoded_frames);
    }

    /// Exact port of `SkWuffsCodec::resetDecoder` result mapping (lines 950-957).
    #[inline]
    pub fn on_reset_decoder_result(&mut self, reset_result: CodecResult) -> CodecResult {
        if reset_result == CodecResult::IncompleteInput {
            return CodecResult::InternalError;
        }
        if reset_result != CodecResult::Success {
            return reset_result;
        }
        self.decoder_is_suspended = false;
        CodecResult::Success
    }

    /// Resolves the byte offset in the GIF stream for `frame_index` (`seekFrame` lines 920-929).
    #[inline]
    pub fn resolve_frame_io_position(&self, frame_index: i32) -> Result<u64, CodecResult> {
        if frame_index < 0 {
            Err(CodecResult::InternalError)
        } else if frame_index == 0 {
            Ok(self.first_frame_io_position)
        } else if let Some(frame) = self.frame_holder.get_frame(frame_index) {
            Ok(frame.io_position)
        } else {
            Err(CodecResult::InternalError)
        }
    }

    /// Exact port of `SkWuffsCodec::onGetFrameCount` decision logic (lines 801-848).
    pub fn plan_get_frame_count(&self) -> GetFrameCountPlan {
        if !self.can_seek {
            return GetFrameCountPlan {
                need_scan: false,
                immediate_count: 1,
                start_index: 0,
            };
        }
        if !self.frame_holder.frames_complete() && !self.incr_dec_active {
            let n = self.frame_holder.len();
            let start_index = if n > 0 { (n - 1) as i32 } else { 0 };
            return GetFrameCountPlan {
                need_scan: true,
                immediate_count: n as i32,
                start_index,
            };
        }
        GetFrameCountPlan {
            need_scan: false,
            immediate_count: self.frame_holder.len() as i32,
            start_index: 0,
        }
    }

    /// Exact port of one iteration of `SkWuffsCodec::onGetFrameCountInternal` (lines 852-868).
    pub fn on_frame_count_step(
        &mut self,
        index: i32,
        status: WuffsStatusKind,
        fc: WuffsFrameConfigInput,
    ) -> FrameCountStepResult {
        let empty_frame = GifFrame {
            frame_id: 0,
            io_position: 0,
            reported_alpha: EncodedAlpha::Unpremul as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            frame_rect: IRect::EMPTY,
            disposal_method: 1,
            duration_ms: 0,
            blend: 0,
        };

        match status {
            WuffsStatusKind::Ok => {
                if index >= 0 && (index as usize) < self.frame_holder.len() {
                    return FrameCountStepResult {
                        action: FrameCountStepAction::Continue as i32,
                        appended_frame: empty_frame,
                    };
                }
                let appended = self.frame_holder.push_from_wuffs_config(fc);
                FrameCountStepResult {
                    action: FrameCountStepAction::AppendedFrame as i32,
                    appended_frame: appended,
                }
            }
            WuffsStatusKind::EndOfData => {
                self.frame_holder.set_frames_complete(true);
                FrameCountStepResult {
                    action: FrameCountStepAction::Completed as i32,
                    appended_frame: empty_frame,
                }
            }
            WuffsStatusKind::ShortRead | WuffsStatusKind::Error => FrameCountStepResult {
                action: FrameCountStepAction::Abort as i32,
                appended_frame: empty_frame,
            },
        }
    }

    /// Called if the `onGetFrameCountInternal` loop finishes naturally at `INT_MAX`.
    #[inline]
    pub fn mark_frames_complete(&mut self) {
        self.frame_holder.set_frames_complete(true);
    }

    /// Exact port of `SkWuffsCodec::onGetFrameInfo` (lines 873-887).
    #[inline]
    pub fn get_frame_info(&self, index: i32) -> Option<FrameInfoFields> {
        if !self.can_seek {
            return None;
        }
        self.frame_holder.fill_frame_info(index)
    }

    /// Exact port of `SkWuffsCodec::onIsAnimated` (lines 902-910).
    #[inline]
    pub fn is_animated(&self) -> IsAnimated {
        self.frame_holder.is_animated()
    }

    /// Validates parameters to `onStartIncrementalDecode` (lines 464-469).
    #[inline]
    pub const fn validate_start_incremental_decode(has_dst: bool, has_subset: bool) -> CodecResult {
        if !has_dst {
            return CodecResult::InvalidParameters;
        }
        if has_subset {
            return CodecResult::Unimplemented;
        }
        CodecResult::Success
    }

    /// Computes the one-pass vs two-pass plan for `onStartIncrementalDecode` (lines 483-559, 620).
    pub fn begin_start_incremental_decode(
        &mut self,
        dst_ct: ColorType,
        dst_dims: ISize,
        has_color_profile: bool,
        frame_index: i32,
    ) -> StartIncrDecodePlan {
        let decision = decide_one_pass(dst_ct, has_color_profile, self.dimensions(), dst_dims);
        self.incr_dec_one_pass = decision.one_pass;

        if decision.one_pass {
            let is_dependent = frame_index != 0
                && self
                    .frame_holder
                    .get_frame(frame_index)
                    .map(|f| !f.is_independent())
                    .unwrap_or(false);
            let (pixel_blend, should_fill_dst) = if is_dependent {
                (WuffsPixelBlend::SrcOver, false)
            } else {
                (WuffsPixelBlend::Src, true)
            };
            self.incr_dec_pixel_blend = pixel_blend;
            let table_width_bytes = (dst_dims.width.max(0) as usize)
                .saturating_mul(decision.bytes_per_pixel);
            StartIncrDecodePlan {
                result: CodecResult::Success as i32,
                one_pass: true,
                one_pass_pixel_format: decision.pixel_format,
                one_pass_table_width_bytes: table_width_bytes,
                one_pass_should_fill_dst: should_fill_dst,
                pixel_blend: pixel_blend as u8,
            }
        } else {
            self.incr_dec_pixel_blend = WuffsPixelBlend::Src;
            StartIncrDecodePlan {
                result: CodecResult::Success as i32,
                one_pass: false,
                one_pass_pixel_format: decision.pixel_format,
                one_pass_table_width_bytes: 0,
                one_pass_should_fill_dst: false,
                pixel_blend: WuffsPixelBlend::Src as u8,
            }
        }
    }

    /// Computes exact zero-clear plan for a reused two-pass pixel buffer (lines 587-618).
    pub fn plan_two_pass_clear(
        src_bits_per_pixel: u32,
        frame_rect: WuffsRectU32,
        pixels_width_bytes: usize,
        pixels_height: usize,
        pixels_stride: usize,
    ) -> TwoPassClearPlan {
        let Ok(src_bytes_per_pixel) = bits_per_pixel_to_bytes(src_bits_per_pixel) else {
            return TwoPassClearPlan {
                result: CodecResult::InternalError as i32,
                bounds_valid: false,
                start_byte_offset: 0,
                row_byte_len: 0,
                num_rows: 0,
                contiguous: false,
                contiguous_byte_len: 0,
            };
        };

        let pixels_w = pixels_width_bytes / src_bytes_per_pixel;
        let pixels_h = pixels_height;
        if !frame_rect.fits_in_surface(pixels_w, pixels_h) {
            return TwoPassClearPlan {
                result: CodecResult::InternalError as i32,
                bounds_valid: false,
                start_byte_offset: 0,
                row_byte_len: 0,
                num_rows: 0,
                contiguous: false,
                contiguous_byte_len: 0,
            };
        }

        let start_byte_offset = (frame_rect.min_incl_y as usize)
            .saturating_mul(pixels_stride)
            .saturating_add((frame_rect.min_incl_x as usize).saturating_mul(src_bytes_per_pixel));
        let row_byte_len = (frame_rect.width() as usize).saturating_mul(src_bytes_per_pixel);
        let num_rows = frame_rect.height() as usize;
        let contiguous =
            (row_byte_len == pixels_stride) && (frame_rect.min_incl_y < frame_rect.max_excl_y);
        let contiguous_byte_len = if contiguous {
            row_byte_len.saturating_mul(num_rows)
        } else {
            0
        };

        TwoPassClearPlan {
            result: CodecResult::Success as i32,
            bounds_valid: true,
            start_byte_offset,
            row_byte_len,
            num_rows,
            contiguous,
            contiguous_byte_len,
        }
    }

    /// Commits incremental decode state after one-pass or two-pass setup succeeds (lines 520-523).
    #[inline]
    pub fn commit_start_incremental_decode(&mut self) {
        self.incr_dec_active = true;
        self.first_call_to_incremental_decode = true;
    }

    /// Validates that `onStartIncrementalDecode` has been called before `onIncrementalDecode`.
    #[inline]
    pub const fn check_incremental_decode_active(&self) -> CodecResult {
        if self.incr_dec_active {
            CodecResult::Success
        } else {
            CodecResult::InternalError
        }
    }

    /// Exact port of `SkWuffsCodec::onIncrementalDecodeOnePass` (lines 644-655).
    #[inline]
    pub const fn interpret_incremental_decode_one_pass(status: WuffsStatusKind) -> CodecResult {
        map_wuffs_decode_status(status)
    }

    /// Exact port of `SkWuffsCodec::onIncrementalDecodeTwoPass` (lines 657-799).
    #[allow(clippy::too_many_arguments)]
    pub fn plan_incremental_decode_two_pass(
        &mut self,
        status: WuffsStatusKind,
        frame_index: i32,
        encoded_info_opaque: bool,
        src_bits_per_pixel: u32,
        frame_rect: WuffsRectU32,
        dirty_rect: WuffsRectU32,
        pixels_width_bytes: usize,
        pixels_height: usize,
        pixels_stride: usize,
    ) -> TwoPassDecodePlan {
        let mut plan = TwoPassDecodePlan {
            result: CodecResult::Success as i32,
            early_return: false,
            should_fill_dst: false,
            should_draw: false,
            dirty_bounds_valid: true,
            src_byte_offset: 0,
            dirty_width: 0,
            dirty_height: 0,
            translate_x: 0,
            translate_y: 0,
            src_alpha_type: to_alpha_type(false) as i32,
            use_blend_src: false,
            should_release_two_pass_pixbuf: false,
        };

        let (independent, alpha_type) = if frame_index == 0 {
            (true, to_alpha_type(encoded_info_opaque))
        } else {
            let Some(f) = self.frame_holder.get_frame(frame_index) else {
                plan.result = CodecResult::InternalError as i32;
                plan.early_return = true;
                return plan;
            };
            (
                f.is_independent(),
                to_alpha_type(f.reported_alpha_enum() == EncodedAlpha::Opaque),
            )
        };
        plan.src_alpha_type = alpha_type as i32;

        let result = map_wuffs_decode_status(status);
        plan.result = result as i32;

        if result != CodecResult::Success && !independent {
            // For a dependent frame, we cannot blend the partial result, since
            // that will overwrite the contribution from prior frames.
            plan.early_return = true;
            return plan;
        }

        let Ok(src_bytes_per_pixel) = bits_per_pixel_to_bytes(src_bits_per_pixel) else {
            plan.result = CodecResult::InternalError as i32;
            plan.early_return = true;
            return plan;
        };

        if self.first_call_to_incremental_decode {
            if (frame_rect.width() as usize) > (usize::MAX / src_bytes_per_pixel) {
                plan.result = CodecResult::InternalError as i32;
                plan.early_return = true;
                return plan;
            }
            let bounds = frame_rect.to_irect();
            if independent && (bounds != self.bounds() || result != CodecResult::Success) {
                plan.should_fill_dst = true;
            }
            self.first_call_to_incremental_decode = false;
        }

        if !dirty_rect.is_empty() {
            let pixels_w = pixels_width_bytes / src_bytes_per_pixel;
            let pixels_h = pixels_height;
            if !dirty_rect.fits_in_surface(pixels_w, pixels_h) {
                plan.dirty_bounds_valid = false;
                plan.result = CodecResult::InternalError as i32;
                plan.early_return = true;
                return plan;
            }

            plan.should_draw = true;
            plan.src_byte_offset = (dirty_rect.min_incl_y as usize)
                .saturating_mul(pixels_stride)
                .saturating_add(
                    (dirty_rect.min_incl_x as usize).saturating_mul(src_bytes_per_pixel),
                );
            plan.dirty_width = dirty_rect.width() as i32;
            plan.dirty_height = dirty_rect.height() as i32;
            plan.translate_x = dirty_rect.min_incl_x as i32;
            plan.translate_y = dirty_rect.min_incl_y as i32;
            plan.use_blend_src = independent;
        }

        if result == CodecResult::Success
            && self.frame_holder.frames_complete()
            && !self.frame_holder.is_empty()
            && frame_index >= 0
            && (frame_index as usize) == self.frame_holder.len() - 1
        {
            plan.should_release_two_pass_pixbuf = true;
        }

        plan
    }

    /// Exact port of `SkWuffsCodec::onIncrementalDecode` cleanup on `kSuccess` (lines 635-640).
    #[inline]
    pub fn finish_incremental_decode(&mut self, result: CodecResult) {
        if result == CodecResult::Success {
            self.incr_dec_active = false;
            self.incr_dec_pixel_blend = WuffsPixelBlend::Src;
            self.incr_dec_one_pass = false;
        }
    }

    /// Validates a 64-bit two-pass pixel buffer allocation size.
    #[inline]
    pub const fn validate_two_pass_pixbuf_len(pixbuf_len: u64) -> Result<usize, CodecResult> {
        validate_buffer_len(pixbuf_len)
    }
}
