#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's GIF (`SkWuffsCodec`) frame holder, disposal,
//! scaling, incremental decode, I/O buffer seek, and pixel-format state machine (`P2.7`).

pub mod format;
pub mod frame_holder;
pub mod io_buf;
pub mod state_machine;
pub mod types;

#[cfg(test)]
mod tests {
    use skia_rs_codec_core::types::{
        Blend, CodecResult, DisposalMethod, EncodedAlpha, EncodedColor, SelectionPolicy, NO_FRAME,
        REPETITION_COUNT_INFINITE,
    };
    use skia_rs_color::image_info::{AlphaType, ColorType};
    use skia_rs_geometry::{IRect, ISize};

    use crate::{
        format::{
            bits_per_pixel_to_bytes, classify_encoded_info, decide_one_pass, dimensions_supported,
            dst_color_type_to_wuffs_pixfmt, map_wuffs_decode_status,
            n32_color_type_to_wuffs_pixfmt, scaled_dimensions, should_copy_stream_to_memory,
            to_alpha_type, validate_buffer_len, wuffs_disposal_to_skia, wuffs_duration_to_ms,
            wuffs_loops_to_repetition_count,
        },
        frame_holder::GifFrameHolder,
        io_buf::{
            apply_stream_seek_success, compact_io_buffer_meta, plan_seek_buffer, IoBufferMeta,
            SeekBufferAction,
        },
        state_machine::{FrameCountStepAction, GifCodecState},
        types::{
            is_gif, parse_gif_version, wuffs_pixel_format, GifVersion, IsAnimated,
            WuffsAnimationDisposal, WuffsFrameConfigInput, WuffsPixelBlend, WuffsRectU32,
            WuffsStatusKind, WUFFS_FLICKS_PER_MILLISECOND,
        },
    };

    #[test]
    fn gif_signature_sniff_matches_skia_and_versions() {
        assert!(is_gif(b"GIF87a"));
        assert!(is_gif(b"GIF89a"));
        assert!(is_gif(b"GIF8"));
        assert!(is_gif(b"GIF89a\x01\x00\x01\x00"));
        assert!(!is_gif(b"GIF"));
        assert!(!is_gif(b"GIF9"));
        assert!(!is_gif(b""));
        assert!(!is_gif(b"\x89PNG\r\n\x1a\n"));

        assert_eq!(parse_gif_version(b"GIF87a"), Some(GifVersion::Gif87a));
        assert_eq!(parse_gif_version(b"GIF89a"), Some(GifVersion::Gif89a));
        assert_eq!(parse_gif_version(b"GIF88a"), None);
        assert_eq!(parse_gif_version(b"GIF89"), None);
    }

    #[test]
    fn pixel_format_and_one_pass_decisions() {
        assert_eq!(
            n32_color_type_to_wuffs_pixfmt(ColorType::Bgra8888),
            Ok(wuffs_pixel_format::BGRA_NONPREMUL)
        );
        assert_eq!(
            n32_color_type_to_wuffs_pixfmt(ColorType::Rgba8888),
            Ok(wuffs_pixel_format::RGBA_NONPREMUL)
        );
        assert_eq!(
            n32_color_type_to_wuffs_pixfmt(ColorType::Rgb565),
            Err(CodecResult::InternalError)
        );

        assert_eq!(
            dst_color_type_to_wuffs_pixfmt(ColorType::Rgb565),
            (wuffs_pixel_format::BGR_565, 2)
        );
        assert_eq!(
            dst_color_type_to_wuffs_pixfmt(ColorType::Bgra8888),
            (wuffs_pixel_format::BGRA_NONPREMUL, 4)
        );
        assert_eq!(
            dst_color_type_to_wuffs_pixfmt(ColorType::Rgba8888),
            (wuffs_pixel_format::RGBA_NONPREMUL, 4)
        );
        assert_eq!(
            dst_color_type_to_wuffs_pixfmt(ColorType::RgbaF16),
            (wuffs_pixel_format::INVALID, 0)
        );
        assert_eq!(
            dst_color_type_to_wuffs_pixfmt(ColorType::Gray8),
            (wuffs_pixel_format::INVALID, 0)
        );

        let dims = ISize {
            width: 320,
            height: 240,
        };
        let half = ISize {
            width: 160,
            height: 120,
        };

        // Supported format, no profile, identity size -> one-pass.
        let d = decide_one_pass(ColorType::Rgba8888, false, dims, dims);
        assert!(d.one_pass);
        assert_eq!(d.pixel_format, wuffs_pixel_format::RGBA_NONPREMUL);
        assert_eq!(d.bytes_per_pixel, 4);

        // RGB565 identity -> one-pass.
        let d = decide_one_pass(ColorType::Rgb565, false, dims, dims);
        assert!(d.one_pass);
        assert_eq!(d.bytes_per_pixel, 2);

        // Color profile present -> two-pass.
        let d = decide_one_pass(ColorType::Rgba8888, true, dims, dims);
        assert!(!d.one_pass);

        // Scaled -> two-pass.
        let d = decide_one_pass(ColorType::Rgba8888, false, dims, half);
        assert!(!d.one_pass);

        // RGBA_F16 -> two-pass.
        let d = decide_one_pass(ColorType::RgbaF16, false, dims, dims);
        assert!(!d.one_pass);
        assert_eq!(d.pixel_format, wuffs_pixel_format::INVALID);
    }

    #[test]
    fn encoded_info_and_stream_policy() {
        assert!(!should_copy_stream_to_memory(
            SelectionPolicy::PreferStillImage,
            false
        ));
        assert!(!should_copy_stream_to_memory(
            SelectionPolicy::PreferStillImage,
            true
        ));
        assert!(should_copy_stream_to_memory(
            SelectionPolicy::PreferAnimation,
            false
        ));
        assert!(!should_copy_stream_to_memory(
            SelectionPolicy::PreferAnimation,
            true
        ));

        let info =
            classify_encoded_info(320, 240, wuffs_pixel_format::BGRA_NONPREMUL, true).unwrap();
        assert_eq!(info.width, 320);
        assert_eq!(info.height, 240);
        assert_eq!(info.color, EncodedColor::Bgra as i32);
        assert_eq!(info.alpha, EncodedAlpha::Opaque as i32);
        assert_eq!(info.bits_per_component, 8);

        let info2 =
            classify_encoded_info(16, 16, wuffs_pixel_format::RGBA_NONPREMUL, false).unwrap();
        assert_eq!(info2.color, EncodedColor::Rgba as i32);
        assert_eq!(info2.alpha, EncodedAlpha::Binary as i32);

        // Invalid dimensions.
        assert_eq!(
            classify_encoded_info(0, 10, wuffs_pixel_format::RGBA_NONPREMUL, true),
            Err(CodecResult::InvalidInput)
        );
        assert_eq!(
            classify_encoded_info(10, 0, wuffs_pixel_format::RGBA_NONPREMUL, true),
            Err(CodecResult::InvalidInput)
        );
        assert_eq!(
            classify_encoded_info(
                (i32::MAX as u32) + 1,
                10,
                wuffs_pixel_format::RGBA_NONPREMUL,
                true
            ),
            Err(CodecResult::InvalidInput)
        );
    }

    #[test]
    fn disposal_duration_and_repetition_math() {
        assert_eq!(
            wuffs_disposal_to_skia(WuffsAnimationDisposal::None as u8),
            DisposalMethod::Keep
        );
        assert_eq!(
            wuffs_disposal_to_skia(WuffsAnimationDisposal::RestoreBackground as u8),
            DisposalMethod::RestoreBgColor
        );
        assert_eq!(
            wuffs_disposal_to_skia(WuffsAnimationDisposal::RestorePrevious as u8),
            DisposalMethod::RestorePrevious
        );
        assert_eq!(wuffs_disposal_to_skia(99), DisposalMethod::Keep);

        assert_eq!(wuffs_duration_to_ms(0), 0);
        assert_eq!(
            wuffs_duration_to_ms((100 * WUFFS_FLICKS_PER_MILLISECOND) as i64),
            100
        );
        assert_eq!(
            wuffs_duration_to_ms((33 * WUFFS_FLICKS_PER_MILLISECOND + 705_599) as i64),
            33
        );

        // Wuffs loops -> Skia repetition count.
        assert_eq!(wuffs_loops_to_repetition_count(0), REPETITION_COUNT_INFINITE);
        assert_eq!(wuffs_loops_to_repetition_count(1), 0);
        assert_eq!(wuffs_loops_to_repetition_count(2), 1);
        assert_eq!(wuffs_loops_to_repetition_count(10), 9);
        assert_eq!(wuffs_loops_to_repetition_count(i32::MAX as u32), i32::MAX - 1);
        assert_eq!(
            wuffs_loops_to_repetition_count((i32::MAX as u32) + 1),
            i32::MAX
        );
        assert_eq!(wuffs_loops_to_repetition_count(u32::MAX), i32::MAX);

        assert_eq!(to_alpha_type(true), AlphaType::Opaque);
        assert_eq!(to_alpha_type(false), AlphaType::Premul);
        assert_eq!(bits_per_pixel_to_bytes(32), Ok(4));
        assert_eq!(bits_per_pixel_to_bytes(16), Ok(2));
        assert_eq!(bits_per_pixel_to_bytes(0), Err(CodecResult::InternalError));
        assert_eq!(bits_per_pixel_to_bytes(12), Err(CodecResult::InternalError));
        assert_eq!(validate_buffer_len(1024), Ok(1024));
    }

    #[test]
    fn scaling_helpers_match_sk_scaling_codec() {
        let dims = ISize {
            width: 320,
            height: 240,
        };
        assert_eq!(
            scaled_dimensions(dims, 0.5),
            ISize {
                width: 160,
                height: 120
            }
        );
        assert_eq!(
            scaled_dimensions(dims, 0.0001),
            ISize {
                width: 1,
                height: 1
            }
        );
        assert!(dimensions_supported(dims, ISize { width: 1, height: 1 }));
        assert!(dimensions_supported(dims, dims));
        assert!(!dimensions_supported(dims, ISize { width: 0, height: 10 }));
        assert!(!dimensions_supported(
            dims,
            ISize {
                width: 321,
                height: 240
            }
        ));
    }

    #[test]
    fn io_buffer_seek_and_compact_logic() {
        let mut meta = IoBufferMeta {
            wi: 100,
            ri: 40,
            pos: 500,
            closed: false,
        };
        assert_eq!(meta.reader_io_position(), 540);

        // Seek within buffered window [500, 600] hits buffer directly.
        let plan = plan_seek_buffer(&mut meta, 520);
        assert_eq!(plan.action, SeekBufferAction::HitBuffer as i32);
        assert_eq!(meta.ri, 20);
        assert_eq!(meta.reader_io_position(), 520);

        // Seek right at upper boundary [500 + 100] also hits buffer.
        let plan = plan_seek_buffer(&mut meta, 600);
        assert_eq!(plan.action, SeekBufferAction::HitBuffer as i32);
        assert_eq!(meta.ri, 100);

        // Seek outside window requests stream seek.
        let plan = plan_seek_buffer(&mut meta, 128);
        assert_eq!(plan.action, SeekBufferAction::NeedStreamSeek as i32);
        assert_eq!(plan.stream_offset, 128);
        apply_stream_seek_success(&mut meta, 128);
        assert_eq!(meta.wi, 0);
        assert_eq!(meta.ri, 0);
        assert_eq!(meta.pos, 128);
        assert!(!meta.closed);

        // Compact updates pos/wi/ri accurately.
        assert!(meta.on_bytes_read(64));
        meta.ri = 24;
        let c = compact_io_buffer_meta(&mut meta);
        assert_eq!(c.src_offset, 24);
        assert_eq!(c.len, 40);
        assert_eq!(meta.ri, 0);
        assert_eq!(meta.wi, 40);
        assert_eq!(meta.pos, 152);
    }

    #[test]
    fn frame_holder_disposal_and_dependencies() {
        let mut holder = GifFrameHolder::new(100, 100);
        assert_eq!(holder.is_animated(), IsAnimated::Unknown);

        // Frame 0: full opaque screen, Keep.
        let f0 = holder.push_from_wuffs_config(WuffsFrameConfigInput {
            index: 0,
            io_position: 13,
            bounds: WuffsRectU32::new(0, 0, 100, 100),
            duration_flicks: (40 * WUFFS_FLICKS_PER_MILLISECOND) as i64,
            disposal: WuffsAnimationDisposal::None as u8,
            opaque_within_bounds: true,
            overwrite_instead_of_blend: false,
        });
        assert_eq!(f0.required_frame, NO_FRAME);
        assert!(!f0.has_alpha);
        assert_eq!(f0.duration_ms, 40);
        assert_eq!(f0.disposal_enum(), DisposalMethod::Keep);
        assert_eq!(f0.blend_enum(), Blend::SrcOver);

        // Frame 1: sub-rect with alpha, RestorePrevious -> depends on frame 0, stays opaque because f0 is opaque!
        let f1 = holder.push_from_wuffs_config(WuffsFrameConfigInput {
            index: 1,
            io_position: 256,
            bounds: WuffsRectU32::new(10, 10, 50, 50),
            duration_flicks: (50 * WUFFS_FLICKS_PER_MILLISECOND) as i64,
            disposal: WuffsAnimationDisposal::RestorePrevious as u8,
            opaque_within_bounds: false,
            overwrite_instead_of_blend: false,
        });
        assert_eq!(f1.required_frame, 0);
        assert!(!f1.has_alpha);
        assert_eq!(f1.frame_rect, IRect::make_ltrb(10, 10, 50, 50));
        assert_eq!(holder.is_animated(), IsAnimated::Yes);

        // Frame 2: sub-rect after RestorePrevious skips f1 and depends on f0.
        let f2 = holder.push_from_wuffs_config(WuffsFrameConfigInput {
            index: 2,
            io_position: 512,
            bounds: WuffsRectU32::new(20, 20, 40, 40),
            duration_flicks: (60 * WUFFS_FLICKS_PER_MILLISECOND) as i64,
            disposal: WuffsAnimationDisposal::RestoreBackground as u8,
            opaque_within_bounds: true,
            overwrite_instead_of_blend: true,
        });
        assert_eq!(f2.required_frame, 0);
        assert_eq!(f2.blend_enum(), Blend::Src);

        // Frame 3: after f2 (RestoreBgColor, dependent on 0) with full screen overwrite -> independent!
        let f3 = holder.push_from_wuffs_config(WuffsFrameConfigInput {
            index: 3,
            io_position: 768,
            bounds: WuffsRectU32::new(0, 0, 100, 100),
            duration_flicks: (70 * WUFFS_FLICKS_PER_MILLISECOND) as i64,
            disposal: WuffsAnimationDisposal::None as u8,
            opaque_within_bounds: true,
            overwrite_instead_of_blend: true,
        });
        assert_eq!(f3.required_frame, NO_FRAME);
        assert!(!f3.has_alpha);

        // Fully received frame tracking never decreases across seeks.
        holder.update_num_fully_received_frames(3);
        holder.update_num_fully_received_frames(1);
        assert_eq!(holder.num_fully_received_frames(), 3);
        assert!(holder.fill_frame_info(2).unwrap().fully_received);
        assert!(!holder.fill_frame_info(3).unwrap().fully_received);
    }

    #[test]
    fn nonseekable_codec_state_transitions() {
        let state = GifCodecState::new(320, 240, false, 13);
        let plan = state.plan_get_frame_count();
        assert!(!plan.need_scan);
        assert_eq!(plan.immediate_count, 1);
        assert!(state.get_frame_info(0).is_none());
        assert_eq!(state.resolve_frame_io_position(0), Ok(13));
        assert_eq!(
            state.resolve_frame_io_position(1),
            Err(CodecResult::InternalError)
        );
    }

    #[test]
    fn incremental_frame_discovery_and_two_pass_decode_state_machine() {
        let mut state = GifCodecState::new(100, 80, true, 13);

        // First scan finds frame 0 and hits short_read before frame 1.
        let plan = state.plan_get_frame_count();
        assert!(plan.need_scan);
        assert_eq!(plan.start_index, 0);

        let s0 = state.on_frame_count_step(
            0,
            WuffsStatusKind::Ok,
            WuffsFrameConfigInput {
                index: 0,
                io_position: 13,
                bounds: WuffsRectU32::new(0, 0, 100, 80),
                duration_flicks: (100 * WUFFS_FLICKS_PER_MILLISECOND) as i64,
                disposal: WuffsAnimationDisposal::None as u8,
                opaque_within_bounds: true,
                overwrite_instead_of_blend: false,
            },
        );
        assert_eq!(s0.action, FrameCountStepAction::AppendedFrame as i32);

        let s1_short = state.on_frame_count_step(
            1,
            WuffsStatusKind::ShortRead,
            WuffsFrameConfigInput::default(),
        );
        assert_eq!(s1_short.action, FrameCountStepAction::Abort as i32);
        assert_eq!(state.frame_count(), 1);
        assert_eq!(state.is_animated(), IsAnimated::Unknown);

        // Second scan resumes at start_index = n - 1 = 0, skips frame 0, adds frame 1, finishes.
        let plan2 = state.plan_get_frame_count();
        assert!(plan2.need_scan);
        assert_eq!(plan2.start_index, 0);

        let skip0 = state.on_frame_count_step(
            0,
            WuffsStatusKind::Ok,
            WuffsFrameConfigInput::default(),
        );
        assert_eq!(skip0.action, FrameCountStepAction::Continue as i32);

        let add1 = state.on_frame_count_step(
            1,
            WuffsStatusKind::Ok,
            WuffsFrameConfigInput {
                index: 1,
                io_position: 300,
                bounds: WuffsRectU32::new(10, 10, 60, 50),
                duration_flicks: (50 * WUFFS_FLICKS_PER_MILLISECOND) as i64,
                disposal: WuffsAnimationDisposal::None as u8,
                opaque_within_bounds: false,
                overwrite_instead_of_blend: false,
            },
        );
        assert_eq!(add1.action, FrameCountStepAction::AppendedFrame as i32);
        assert_eq!(add1.appended_frame.required_frame, 0);

        let done = state.on_frame_count_step(
            2,
            WuffsStatusKind::EndOfData,
            WuffsFrameConfigInput::default(),
        );
        assert_eq!(done.action, FrameCountStepAction::Completed as i32);
        assert_eq!(state.frame_count(), 2);
        assert_eq!(state.is_animated(), IsAnimated::Yes);

        // Start two-pass scaled decode for frame 1 (dependent!).
        let start = state.begin_start_incremental_decode(
            ColorType::Rgba8888,
            ISize {
                width: 50,
                height: 40,
            },
            false,
            1,
        );
        assert!(!start.one_pass);
        assert_eq!(start.pixel_blend, WuffsPixelBlend::Src as u8);
        state.commit_start_incremental_decode();
        assert!(state.incr_dec_active());

        // While incremental decode is active, getFrameCount must NOT scan!
        let mid_plan = state.plan_get_frame_count();
        assert!(!mid_plan.need_scan);
        assert_eq!(mid_plan.immediate_count, 2);

        // Partial decode of dependent frame 1 must early-return without drawing or clearing first_call.
        let partial = state.plan_incremental_decode_two_pass(
            WuffsStatusKind::ShortRead,
            1,
            true,
            32,
            WuffsRectU32::new(10, 10, 60, 50),
            WuffsRectU32::new(10, 10, 60, 30),
            400,
            80,
            400,
        );
        assert_eq!(partial.result, CodecResult::IncompleteInput as i32);
        assert!(partial.early_return);
        assert!(!partial.should_draw);
        assert!(state.first_call_to_incremental_decode());

        // Complete decode of last frame 1 draws dirty rect with SrcOver (dependent) and releases pixbuf!
        let complete = state.plan_incremental_decode_two_pass(
            WuffsStatusKind::Ok,
            1,
            true,
            32,
            WuffsRectU32::new(10, 10, 60, 50),
            WuffsRectU32::new(10, 10, 60, 50),
            400,
            80,
            400,
        );
        assert_eq!(complete.result, CodecResult::Success as i32);
        assert!(!complete.early_return);
        assert!(!complete.should_fill_dst);
        assert!(complete.should_draw);
        assert!(!complete.use_blend_src);
        assert_eq!(complete.src_alpha_type, AlphaType::Premul as i32);
        assert_eq!(complete.src_byte_offset, 10 * 400 + 10 * 4);
        assert_eq!(complete.dirty_width, 50);
        assert_eq!(complete.dirty_height, 40);
        assert_eq!(complete.translate_x, 10);
        assert_eq!(complete.translate_y, 10);
        assert!(complete.should_release_two_pass_pixbuf);

        state.finish_incremental_decode(CodecResult::Success);
        assert!(!state.incr_dec_active());
    }

    #[test]
    fn two_pass_clear_and_suspension_reset() {
        // Contiguous full-width clear.
        let full = GifCodecState::plan_two_pass_clear(
            32,
            WuffsRectU32::new(0, 5, 100, 25),
            400,
            80,
            400,
        );
        assert_eq!(full.result, CodecResult::Success as i32);
        assert!(full.bounds_valid);
        assert!(full.contiguous);
        assert_eq!(full.start_byte_offset, 5 * 400);
        assert_eq!(full.contiguous_byte_len, 20 * 400);

        // Non-contiguous sub-rect clear.
        let sub = GifCodecState::plan_two_pass_clear(
            32,
            WuffsRectU32::new(10, 5, 30, 15),
            400,
            80,
            400,
        );
        assert_eq!(sub.result, CodecResult::Success as i32);
        assert!(sub.bounds_valid);
        assert!(!sub.contiguous);
        assert_eq!(sub.start_byte_offset, 5 * 400 + 10 * 4);
        assert_eq!(sub.row_byte_len, 20 * 4);
        assert_eq!(sub.num_rows, 10);

        // Out-of-bounds frame rect rejected.
        let oob = GifCodecState::plan_two_pass_clear(
            32,
            WuffsRectU32::new(0, 0, 101, 80),
            400,
            80,
            400,
        );
        assert_eq!(oob.result, CodecResult::InternalError as i32);
        assert!(!oob.bounds_valid);

        // Suspension & reset result mapping.
        let mut state = GifCodecState::new(32, 32, true, 13);
        state.on_decode_step_finished(false, 1);
        assert!(state.decoder_is_suspended());
        assert_eq!(
            state.on_reset_decoder_result(CodecResult::IncompleteInput),
            CodecResult::InternalError
        );
        assert!(state.decoder_is_suspended());
        assert_eq!(
            state.on_reset_decoder_result(CodecResult::Success),
            CodecResult::Success
        );
        assert!(!state.decoder_is_suspended());

        assert_eq!(
            map_wuffs_decode_status(WuffsStatusKind::Ok),
            CodecResult::Success
        );
        assert_eq!(
            map_wuffs_decode_status(WuffsStatusKind::ShortRead),
            CodecResult::IncompleteInput
        );
        assert_eq!(
            map_wuffs_decode_status(WuffsStatusKind::Error),
            CodecResult::ErrorInInput
        );
    }

    #[test]
    fn one_pass_dependent_vs_independent_blend_and_start_validation() {
        assert_eq!(
            GifCodecState::validate_start_incremental_decode(false, false),
            CodecResult::InvalidParameters
        );
        assert_eq!(
            GifCodecState::validate_start_incremental_decode(true, true),
            CodecResult::Unimplemented
        );
        assert_eq!(
            GifCodecState::validate_start_incremental_decode(true, false),
            CodecResult::Success
        );

        let mut state = GifCodecState::new(64, 64, true, 13);
        state.on_frame_count_step(
            0,
            WuffsStatusKind::Ok,
            WuffsFrameConfigInput {
                index: 0,
                io_position: 13,
                bounds: WuffsRectU32::new(0, 0, 64, 64),
                duration_flicks: 0,
                disposal: WuffsAnimationDisposal::None as u8,
                opaque_within_bounds: true,
                overwrite_instead_of_blend: false,
            },
        );
        state.on_frame_count_step(
            1,
            WuffsStatusKind::Ok,
            WuffsFrameConfigInput {
                index: 1,
                io_position: 200,
                bounds: WuffsRectU32::new(8, 8, 32, 32),
                duration_flicks: 0,
                disposal: WuffsAnimationDisposal::None as u8,
                opaque_within_bounds: false,
                overwrite_instead_of_blend: false,
            },
        );

        // Frame 0 one-pass: SRC blend + fill dst.
        let p0 = state.begin_start_incremental_decode(
            ColorType::Bgra8888,
            ISize {
                width: 64,
                height: 64,
            },
            false,
            0,
        );
        assert!(p0.one_pass);
        assert!(p0.one_pass_should_fill_dst);
        assert_eq!(p0.pixel_blend, WuffsPixelBlend::Src as u8);
        assert_eq!(p0.one_pass_table_width_bytes, 64 * 4);

        // Frame 1 (dependent) one-pass: SRC_OVER blend + do NOT fill dst.
        let p1 = state.begin_start_incremental_decode(
            ColorType::Bgra8888,
            ISize {
                width: 64,
                height: 64,
            },
            false,
            1,
        );
        assert!(p1.one_pass);
        assert!(!p1.one_pass_should_fill_dst);
        assert_eq!(p1.pixel_blend, WuffsPixelBlend::SrcOver as u8);
    }

    #[test]
    fn two_pass_frame0_partial_then_complete_and_empty_dirty_rect() {
        let mut state = GifCodecState::new(100, 100, true, 13);
        state.on_frame_count_step(
            0,
            WuffsStatusKind::Ok,
            WuffsFrameConfigInput {
                index: 0,
                io_position: 13,
                bounds: WuffsRectU32::new(0, 0, 100, 100),
                duration_flicks: 0,
                disposal: WuffsAnimationDisposal::None as u8,
                opaque_within_bounds: true,
                overwrite_instead_of_blend: false,
            },
        );
        state.on_frame_count_step(
            1,
            WuffsStatusKind::EndOfData,
            WuffsFrameConfigInput::default(),
        );
        assert_eq!(state.is_animated(), IsAnimated::No);

        let start = state.begin_start_incremental_decode(
            ColorType::RgbaF16,
            ISize {
                width: 100,
                height: 100,
            },
            false,
            0,
        );
        assert!(!start.one_pass);
        state.commit_start_incremental_decode();

        // First call is partial (ShortRead): independent frame 0 MUST fill dst and draw dirty rows,
        // and must NOT release two-pass pixbuf yet.
        let p1 = state.plan_incremental_decode_two_pass(
            WuffsStatusKind::ShortRead,
            0,
            true,
            32,
            WuffsRectU32::new(0, 0, 100, 100),
            WuffsRectU32::new(0, 0, 100, 40),
            400,
            100,
            400,
        );
        assert_eq!(p1.result, CodecResult::IncompleteInput as i32);
        assert!(!p1.early_return);
        assert!(p1.should_fill_dst);
        assert!(p1.should_draw);
        assert!(p1.use_blend_src);
        assert_eq!(p1.src_alpha_type, AlphaType::Opaque as i32);
        assert!(!p1.should_release_two_pass_pixbuf);
        assert!(!state.first_call_to_incremental_decode());

        // Second call has empty dirty rect and finishes: must NOT fill dst again, must NOT draw,
        // and MUST release two-pass pixbuf because frame 0 is the last (and only) frame.
        let p2 = state.plan_incremental_decode_two_pass(
            WuffsStatusKind::Ok,
            0,
            true,
            32,
            WuffsRectU32::new(0, 0, 100, 100),
            WuffsRectU32::new(0, 0, 0, 0),
            400,
            100,
            400,
        );
        assert_eq!(p2.result, CodecResult::Success as i32);
        assert!(!p2.should_fill_dst);
        assert!(!p2.should_draw);
        assert!(p2.should_release_two_pass_pixbuf);
    }

    #[test]
    fn scaled_dimensions_edge_cases() {
        let dims = ISize {
            width: 320,
            height: 240,
        };
        assert_eq!(
            scaled_dimensions(dims, -1.0),
            ISize {
                width: 1,
                height: 1
            }
        );
        assert_eq!(
            scaled_dimensions(dims, f32::NAN),
            ISize {
                width: 2_147_483_520,
                height: 2_147_483_520
            }
        );
        assert_eq!(
            scaled_dimensions(dims, f32::INFINITY),
            ISize {
                width: 2_147_483_520,
                height: 2_147_483_520
            }
        );
    }
}
