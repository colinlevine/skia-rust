#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's WebP decoder/animation and encoder state machines (`P2.6`).

pub mod animation;
pub mod container;
pub mod decoder;
pub mod encoder;
pub mod types;

#[cfg(test)]
mod tests {
    use skia_rs_codec_core::types::{
        Blend, CodecResult, DisposalMethod, EncodedAlpha, EncodedColor, REPETITION_COUNT_INFINITE,
    };
    use skia_rs_codec_meta::color_profile::DataSpace;
    use skia_rs_color::image_info::{AlphaType, ColorType};
    use skia_rs_geometry::{IRect, ISize};

    use crate::animation::*;
    use crate::container::*;
    use crate::decoder::*;
    use crate::encoder::*;
    use crate::types::*;

    fn push_chunk(buf: &mut Vec<u8>, fourcc: &[u8; 4], payload: &[u8]) {
        buf.extend_from_slice(fourcc);
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            buf.push(0);
        }
    }

    fn make_riff(chunks: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + chunks.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((4 + chunks.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WEBP");
        out.extend_from_slice(chunks);
        out
    }

    #[test]
    fn sniff_webp_signature() {
        assert!(is_webp(b"RIFF1234WEBPVP8 "));
        assert!(is_webp(b"RIFF\x00\x00\x00\x00WEBPVP"));
        assert!(is_webp(b"RIFFxxxxWEBPVPL "));
        assert!(is_webp(b"RIFFxxxxWEBPVPX "));
        assert!(!is_webp(b"RIFF1234WEBPV")); // 13 bytes
        assert!(!is_webp(b""));
        assert!(!is_webp(b"RIFX1234WEBPVP8 "));
        assert!(!is_webp(b"RIFF1234WEBPXX8 "));
    }

    #[test]
    fn riff_header_reader_vp8_and_vp8l() {
        // Simple VP8 lossy stream with 100 bytes of payload; reader stops after 12 + 8 + 10 = 30 bytes.
        let mut chunks = Vec::new();
        push_chunk(&mut chunks, b"VP8 ", &[0xAAu8; 100]);
        let webp = make_riff(&chunks);
        assert_eq!(scan_riff_header_slice(&webp), Some((true, 30)));

        // Short VP8 payload with 3 bytes (minimum allowed) -> succeeds.
        let mut short_vp8 = Vec::new();
        short_vp8.extend_from_slice(b"VP8 ");
        short_vp8.extend_from_slice(&3u32.to_le_bytes());
        short_vp8.extend_from_slice(&[1, 2, 3]);
        assert_eq!(scan_riff_header_slice(&make_riff(&short_vp8)), Some((true, 23)));

        // Too-short VP8 payload (2 bytes) -> invalid.
        let mut bad_vp8 = Vec::new();
        bad_vp8.extend_from_slice(b"VP8 ");
        bad_vp8.extend_from_slice(&2u32.to_le_bytes());
        bad_vp8.extend_from_slice(&[1, 2]);
        assert_eq!(scan_riff_header_slice(&make_riff(&bad_vp8)), None);

        // Simple VP8L lossless stream stops after 12 + 8 + 5 = 25 bytes.
        let mut chunks_l = Vec::new();
        push_chunk(&mut chunks_l, b"VP8L", &[0x2Fu8; 40]);
        let webp_l = make_riff(&chunks_l);
        assert_eq!(scan_riff_header_slice(&webp_l), Some((true, 25)));

        // Truncated VP8L (< 5 bytes) -> invalid.
        let mut bad_vp8l = Vec::new();
        bad_vp8l.extend_from_slice(b"VP8L");
        bad_vp8l.extend_from_slice(&4u32.to_le_bytes());
        bad_vp8l.extend_from_slice(&[0x2F, 1, 2, 3]);
        assert_eq!(scan_riff_header_slice(&make_riff(&bad_vp8l)), None);
    }

    #[test]
    fn riff_header_reader_vp8x_exif_anim_and_errors() {
        // VP8X with EXIF flag must read the ENTIRE stream and report only_header_parsed = false.
        let mut vp8x_exif_payload = [0u8; 10];
        vp8x_exif_payload[0] = EXIF_FLAG as u8;
        let mut chunks = Vec::new();
        push_chunk(&mut chunks, b"VP8X", &vp8x_exif_payload);
        push_chunk(&mut chunks, b"VP8 ", &[1u8; 50]);
        push_chunk(&mut chunks, b"EXIF", &[2u8; 25]); // odd size -> padded to 26
        let full = make_riff(&chunks);
        assert_eq!(scan_riff_header_slice(&full), Some((false, full.len())));

        // VP8X animated: reads VP8X + ANIM + first ANMF (with odd padding), stops before 2nd ANMF!
        let mut vp8x_anim_payload = [0u8; 10];
        vp8x_anim_payload[0] = ANIMATION_FLAG as u8;
        let mut anim_chunks = Vec::new();
        push_chunk(&mut anim_chunks, b"VP8X", &vp8x_anim_payload);
        push_chunk(&mut anim_chunks, b"ANIM", &[0u8; 6]);
        push_chunk(&mut anim_chunks, b"ANMF", &[0x11u8; 21]); // odd -> 22 bytes payload
        let expected_stop = 12 + anim_chunks.len();
        push_chunk(&mut anim_chunks, b"ANMF", &[0x22u8; 30]);
        let anim_webp = make_riff(&anim_chunks);
        assert_eq!(
            scan_riff_header_slice(&anim_webp),
            Some((true, expected_stop))
        );

        // VP8X with ICCP (odd length) followed by VP8 -> reads through VP8 prefix.
        let mut icc_chunks = Vec::new();
        push_chunk(&mut icc_chunks, b"VP8X", &[ICCP_FLAG as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        push_chunk(&mut icc_chunks, b"ICCP", &[0xABu8; 13]); // padded to 14
        let vp8_start = 12 + icc_chunks.len();
        push_chunk(&mut icc_chunks, b"VP8 ", &[0xCDu8; 64]);
        let icc_webp = make_riff(&icc_chunks);
        assert_eq!(
            scan_riff_header_slice(&icc_webp),
            Some((true, vp8_start + 8 + 10))
        );

        // First chunk is unknown (not VP8/VP8L/VP8X) -> invalid.
        let mut bad_first = Vec::new();
        push_chunk(&mut bad_first, b"ICCP", &[0u8; 8]);
        assert_eq!(scan_riff_header_slice(&make_riff(&bad_first)), None);

        // Duplicate VP8X -> invalid.
        let mut dup_vp8x = Vec::new();
        push_chunk(&mut dup_vp8x, b"VP8X", &[0u8; 10]);
        push_chunk(&mut dup_vp8x, b"VP8X", &[0u8; 10]);
        assert_eq!(scan_riff_header_slice(&make_riff(&dup_vp8x)), None);

        // Wrong VP8X payload size (8 != 10) -> invalid.
        let mut bad_vp8x_size = Vec::new();
        push_chunk(&mut bad_vp8x_size, b"VP8X", &[0u8; 8]);
        assert_eq!(scan_riff_header_slice(&make_riff(&bad_vp8x_size)), None);

        // Truncated chunk payload -> invalid.
        let mut trunc = Vec::new();
        push_chunk(&mut trunc, b"VP8X", &[0u8; 10]);
        trunc.extend_from_slice(b"ICCP");
        trunc.extend_from_slice(&20u32.to_le_bytes());
        trunc.extend_from_slice(&[0u8; 5]);
        assert_eq!(scan_riff_header_slice(&make_riff(&trunc)), None);

        // Valid VP8X with no image data reaches EOF -> returns (true, len) matching C++.
        let mut no_img = Vec::new();
        push_chunk(&mut no_img, b"VP8X", &[0u8; 10]);
        let no_img_webp = make_riff(&no_img);
        assert_eq!(
            scan_riff_header_slice(&no_img_webp),
            Some((true, no_img_webp.len()))
        );
    }

    #[test]
    fn decoder_header_classification_and_validation() {
        assert_eq!(
            map_demux_state(WebpDemuxState::ParseError),
            Some(CodecResult::InvalidInput)
        );
        assert_eq!(
            map_demux_state(WebpDemuxState::ParsingHeader),
            Some(CodecResult::IncompleteInput)
        );
        assert_eq!(map_demux_state(WebpDemuxState::ParsedHeader), None);
        assert_eq!(map_demux_state(WebpDemuxState::Done), None);

        assert!(validate_canvas_dimensions(100, 100));
        assert!(validate_canvas_dimensions(16383, 16383));
        assert!(!validate_canvas_dimensions(30000, 30000)); // 900M > 536M
        assert!(!validate_canvas_dimensions(i32::MAX, 2));

        assert!(should_keep_color_profile(DataSpace::Rgb));
        assert!(!should_keep_color_profile(DataSpace::Cmyk));
        assert!(!should_keep_color_profile(DataSpace::Gray));

        assert_eq!(map_features_status(Vp8StatusCode::Ok), CodecResult::Success);
        assert_eq!(
            map_features_status(Vp8StatusCode::Suspended),
            CodecResult::IncompleteInput
        );
        assert_eq!(
            map_features_status(Vp8StatusCode::NotEnoughData),
            CodecResult::IncompleteInput
        );
        assert_eq!(
            map_features_status(Vp8StatusCode::BitstreamError),
            CodecResult::InvalidInput
        );

        // Lossless opaque vs alpha vs partial first frame
        assert_eq!(
            classify_encoded_info(100, 100, 100, 100, false, 2),
            Some((EncodedColor::Bgrx, EncodedAlpha::Opaque))
        );
        assert_eq!(
            classify_encoded_info(100, 100, 100, 100, true, 2),
            Some((EncodedColor::Bgra, EncodedAlpha::Unpremul))
        );
        assert_eq!(
            classify_encoded_info(100, 100, 50, 100, false, 0),
            Some((EncodedColor::Bgra, EncodedAlpha::Unpremul))
        );
        // Lossy opaque vs alpha
        assert_eq!(
            classify_encoded_info(100, 100, 100, 100, false, 1),
            Some((EncodedColor::Yuv, EncodedAlpha::Opaque))
        );
        assert_eq!(
            classify_encoded_info(100, 100, 100, 100, true, 1),
            Some((EncodedColor::Yuva, EncodedAlpha::Unpremul))
        );
        // Invalid format
        assert_eq!(classify_encoded_info(100, 100, 100, 100, false, 3), None);
    }

    #[test]
    fn decode_mode_subset_and_scaling() {
        assert_eq!(webp_decode_mode(ColorType::Bgra8888, false), WebpCspMode::Bgra);
        assert_eq!(webp_decode_mode(ColorType::Bgra8888, true), WebpCspMode::BgrAPremul);
        assert_eq!(webp_decode_mode(ColorType::Rgba8888, false), WebpCspMode::Rgba);
        assert_eq!(webp_decode_mode(ColorType::Rgba8888, true), WebpCspMode::RgbAPremul);
        assert_eq!(webp_decode_mode(ColorType::Rgb565, false), WebpCspMode::Rgb565);
        assert_eq!(webp_decode_mode(ColorType::Rgb565, true), WebpCspMode::Rgb565);
        assert_eq!(webp_decode_mode(ColorType::RgbaF16, false), WebpCspMode::Last);

        assert!(is_8888(ColorType::Rgba8888));
        assert!(is_8888(ColorType::Bgra8888));
        assert!(!is_8888(ColorType::Rgb565));

        // Valid subset snaps odd left/top down to even, leaves right/bottom unchanged.
        let bounds = IRect::make_ltrb(0, 0, 100, 100);
        let mut sub = IRect::make_ltrb(3, 5, 51, 53);
        assert!(get_valid_subset(bounds, &mut sub));
        assert_eq!(sub, IRect::make_ltrb(2, 4, 51, 53));

        // Out-of-bounds subset rejected.
        let mut oob = IRect::make_ltrb(50, 50, 101, 100);
        assert!(!get_valid_subset(bounds, &mut oob));

        // Empty subset rejected.
        let mut empty = IRect::make_ltrb(10, 10, 10, 20);
        assert!(!get_valid_subset(bounds, &mut empty));

        // Scaling helpers
        let dims = ISize { width: 100, height: 50 };
        assert_eq!(
            get_scaled_dimensions(dims, 0.5),
            ISize { width: 50, height: 25 }
        );
        assert_eq!(
            get_scaled_dimensions(dims, 0.0001),
            ISize { width: 1, height: 1 }
        );
        assert!(dimensions_supported(dims, ISize { width: 1, height: 1 }));
        assert!(dimensions_supported(dims, ISize { width: 100, height: 50 }));
        assert!(!dimensions_supported(dims, ISize { width: 0, height: 50 }));
        assert!(!dimensions_supported(dims, ISize { width: 101, height: 50 }));
    }

    #[test]
    fn plan_get_pixels_crop_scale_xform_and_blend() {
        let canvas = ISize { width: 100, height: 80 };

        // 1. Direct full-frame RGBA premul decode, no xform, no blend.
        let p1 = plan_get_pixels(
            canvas,
            IRect::make_ltrb(0, 0, 100, 80),
            true,
            WebpMuxAnimBlend::Blend,
            true,
            None,
            canvas,
            ColorType::Rgba8888,
            AlphaType::Premul,
            400,
            false,
        );
        assert!(!p1.should_fill_bg);
        assert!(!p1.early_return_success);
        assert!(!p1.use_cropping);
        assert!(!p1.use_scaling);
        assert!(!p1.blend_with_prev_frame);
        assert!(!p1.needs_temp_decode_buffer);
        assert_eq!(p1.csp_mode, WebpCspMode::RgbAPremul as i32);
        assert_eq!(p1.dst_byte_offset, 0);

        // 2. Independent partial frame with disjoint subset -> MUST still request fill before early return!
        let p2 = plan_get_pixels(
            canvas,
            IRect::make_ltrb(10, 10, 30, 30),
            true,
            WebpMuxAnimBlend::NoBlend,
            true,
            Some(IRect::make_ltrb(40, 40, 80, 80)),
            ISize { width: 40, height: 40 },
            ColorType::Rgba8888,
            AlphaType::Premul,
            160,
            false,
        );
        assert!(p2.should_fill_bg);
        assert!(p2.early_return_success);

        // 3. Partial frame + overlapping subset + 0.5x scaling.
        let p3 = plan_get_pixels(
            canvas,
            IRect::make_ltrb(20, 10, 60, 50),
            false,
            WebpMuxAnimBlend::NoBlend,
            true,
            Some(IRect::make_ltrb(10, 20, 50, 60)),
            ISize { width: 20, height: 20 },
            ColorType::Rgba8888,
            AlphaType::Premul,
            80,
            false,
        );
        assert!(p3.should_fill_bg);
        assert!(!p3.early_return_success);
        assert!(p3.use_cropping);
        // min_x=10, min_y=10 => dst_x=10, dst_y=0 before scale; sub=(0, 10, 40, 50), frame=(10, 0, 50, 40)
        // inter=(10, 10, 40, 40) => subset_w=30, subset_h=30
        assert_eq!(p3.crop_left, 0);
        assert_eq!(p3.crop_top, 10);
        assert_eq!(p3.crop_width, 30);
        assert_eq!(p3.crop_height, 30);
        assert!(p3.use_scaling);
        // scale = 0.5 => dst_x=5, dst_y=0, scaled_w=15, scaled_h=15
        assert_eq!(p3.dst_x, 5);
        assert_eq!(p3.dst_y, 0);
        assert_eq!(p3.scaled_width, 15);
        assert_eq!(p3.scaled_height, 15);
        assert_eq!(p3.dst_byte_offset, 20);
        // Opaque frame forces webp_at = Opaque => MODE_RGBA (not premul)
        assert_eq!(p3.webp_at, AlphaType::Opaque as i32);
        assert_eq!(p3.csp_mode, WebpCspMode::Rgba as i32);

        // 4. Dependent frame with SrcOver blend + colorXform into F16 dst.
        let p4 = plan_get_pixels(
            canvas,
            IRect::make_ltrb(4, 6, 44, 46),
            true,
            WebpMuxAnimBlend::Blend,
            false,
            None,
            canvas,
            ColorType::RgbaF16,
            AlphaType::Unpremul,
            800,
            true,
        );
        assert!(!p4.should_fill_bg);
        assert!(p4.blend_with_prev_frame);
        assert!(p4.needs_temp_decode_buffer);
        assert_eq!(p4.webp_ct, ColorType::Bgra8888 as i32);
        assert_eq!(p4.webp_at, AlphaType::Unpremul as i32);
        assert_eq!(p4.csp_mode, WebpCspMode::Bgra as i32);
        assert_eq!(p4.dst_byte_offset, 4 * 8 + 6 * 800);
        assert!(p4.blend_plan.active);
        assert!(p4.blend_plan.use_tmp_xform_row);
        assert_eq!(p4.blend_plan.blend_src_ct, ColorType::RgbaF16 as i32);
        assert!(p4.blend_plan.premul_dst);
        assert!(p4.blend_plan.premul_src);
        assert!(p4.blend_plan.unpremul_dst);

        // 5. Incremental decode update outcomes
        let ok = interpret_idec_update(Vp8StatusCode::Ok, 40, 10, false, 0);
        assert_eq!(ok.result, CodecResult::Success as i32);
        assert_eq!(ok.rows_decoded, 40);
        assert!(!ok.write_rows_decoded_ptr);

        let partial = interpret_idec_update(Vp8StatusCode::Suspended, 40, 10, true, 12);
        assert_eq!(partial.result, CodecResult::IncompleteInput as i32);
        assert_eq!(partial.rows_decoded, 12);
        assert!(partial.write_rows_decoded_ptr);
        assert_eq!(partial.rows_decoded_out, 22);

        let susp_zero = interpret_idec_update(Vp8StatusCode::Suspended, 40, 10, true, 0);
        assert_eq!(susp_zero.result, CodecResult::InvalidInput as i32);

        let err = interpret_idec_update(Vp8StatusCode::BitstreamError, 40, 10, true, 20);
        assert_eq!(err.result, CodecResult::InvalidInput as i32);
    }

    #[test]
    fn animation_metadata_and_srcover_blend() {
        assert!(!is_animated(0));
        assert!(is_animated(ANIMATION_FLAG));
        assert_eq!(compute_repetition_count(0, 0), 0);
        assert_eq!(compute_repetition_count(0, 5), 0);
        assert_eq!(
            compute_repetition_count(ANIMATION_FLAG, 0),
            REPETITION_COUNT_INFINITE
        );
        assert_eq!(compute_repetition_count(ANIMATION_FLAG, 1), 0);
        assert_eq!(compute_repetition_count(ANIMATION_FLAG, 4), 3);

        let m1 = map_frame_metadata(true, WebpMuxAnimDispose::Background, WebpMuxAnimBlend::NoBlend);
        assert_eq!(m1.reported_alpha, EncodedAlpha::Unpremul as i32);
        assert_eq!(m1.disposal_method, DisposalMethod::RestoreBgColor as i32);
        assert_eq!(m1.blend, Blend::Src as i32);

        let m2 = map_frame_metadata(false, WebpMuxAnimDispose::None, WebpMuxAnimBlend::Blend);
        assert_eq!(m2.reported_alpha, EncodedAlpha::Opaque as i32);
        assert_eq!(m2.disposal_method, DisposalMethod::Keep as i32);
        assert_eq!(m2.blend, Blend::SrcOver as i32);

        // SrcOver blend test: 50% red (unpremul) over opaque blue (premul)
        let mut dst = [0u8, 0, 255, 255, 10, 20, 30, 255];
        let src = [255u8, 0, 0, 128, 0, 0, 0, 0];
        blend_src_over_row_8888(
            &mut dst,
            &src,
            2,
            ColorType::Rgba8888,
            ColorType::Rgba8888,
            AlphaType::Premul,
            true,
        );
        // Pixel 0: sa=128/255, sr=128/255, dr=0, db=1 -> out_r=128, out_g=0, out_b=127, out_a=255
        assert_eq!(&dst[0..4], &[128, 0, 127, 255]);
        // Pixel 1: transparent src leaves dst untouched
        assert_eq!(&dst[4..8], &[10, 20, 30, 255]);

        // Unpremul BGRA dst with RGBA src: 100% transparent over 100% transparent stays 0
        let mut dst_u = [0u8, 0, 0, 0];
        let src_u = [100u8, 200, 50, 0];
        blend_src_over_row_8888(
            &mut dst_u,
            &src_u,
            1,
            ColorType::Bgra8888,
            ColorType::Rgba8888,
            AlphaType::Unpremul,
            true,
        );
        assert_eq!(dst_u, [0, 0, 0, 0]);
    }

    #[test]
    fn encoder_planning_scanline_conversion_and_animation() {
        // Valid lossy RGBA unpremul -> direct RGBA, method 3, use_argb 0
        let p = plan_encode_picture(
            64,
            64,
            ColorType::Rgba8888,
            AlphaType::Unpremul,
            true,
            256,
            WebpCompression::Lossy,
            80.0,
            -1,
            false,
            false,
        );
        assert!(p.valid);
        assert_eq!(p.lossless, 0);
        assert!(p.set_method);
        assert_eq!(p.method, 3);
        assert_eq!(p.use_argb, 0);
        assert_eq!(p.import_strategy, WebpImportStrategy::DirectRgba as i32);

        // SK_WEBP_ENCODER_USE_DEFAULT_METHOD skips overriding method on lossy with fMethod < 0
        let p_def = plan_encode_picture(
            64,
            64,
            ColorType::Rgb888x,
            AlphaType::Opaque,
            true,
            256,
            WebpCompression::Lossy,
            75.0,
            -1,
            true,
            false,
        );
        assert!(p_def.valid);
        assert!(!p_def.set_method);
        assert_eq!(p_def.import_strategy, WebpImportStrategy::DirectRgbx as i32);

        // Lossless BGRA unpremul without WebPPictureImportBGRA macro -> Rust converter
        let p_ll = plan_encode_picture(
            32,
            16,
            ColorType::Bgra8888,
            AlphaType::Unpremul,
            true,
            128,
            WebpCompression::Lossless,
            100.0,
            6,
            false,
            false,
        );
        assert!(p_ll.valid);
        assert_eq!(p_ll.lossless, 1);
        assert_eq!(p_ll.method, 6);
        assert_eq!(p_ll.use_argb, 1);
        assert_eq!(
            p_ll.import_strategy,
            WebpImportStrategy::RustConvertToRgba8888 as i32
        );
        assert_eq!(
            p_ll.convert_kind,
            WebpScanlineConvertKind::Bgra8888UnpremulToRgba8888 as i32
        );

        // Premul RGBA falls back to SkPixmap::readPixels for bit-exact float unpremul
        let p_pm = plan_encode_picture(
            32,
            16,
            ColorType::Rgba8888,
            AlphaType::Premul,
            true,
            128,
            WebpCompression::Lossless,
            100.0,
            -1,
            false,
            false,
        );
        assert!(p_pm.valid);
        assert_eq!(p_pm.method, 0);
        assert_eq!(
            p_pm.import_strategy,
            WebpImportStrategy::ReadPixelsToRgba8888 as i32
        );

        // Invalid inputs: alpha-only, bad method, bad quality, short row_bytes, null addr
        assert!(
            !plan_encode_picture(
                10,
                10,
                ColorType::Alpha8,
                AlphaType::Premul,
                true,
                10,
                WebpCompression::Lossy,
                80.0,
                -1,
                false,
                false,
            )
            .valid
        );
        assert!(
            !plan_encode_picture(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Unpremul,
                true,
                40,
                WebpCompression::Lossy,
                80.0,
                7,
                false,
                false,
            )
            .valid
        );
        assert!(
            !plan_encode_picture(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Unpremul,
                true,
                40,
                WebpCompression::Lossy,
                101.0,
                -1,
                false,
                false,
            )
            .valid
        );
        assert!(
            !plan_encode_picture(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Unpremul,
                true,
                39,
                WebpCompression::Lossy,
                80.0,
                -1,
                false,
                false,
            )
            .valid
        );
        assert!(!validate_quality(f32::NAN));
        assert_eq!(clamp_quality(-5.0), 0.0);
        assert_eq!(clamp_quality(150.0), 100.0);
        assert_eq!(clamp_quality(f32::NAN), 0.0);

        // Scanline & 2D image conversion tests
        let bgra_src = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let mut rgba_dst = [0u8; 8];
        assert!(convert_scanline_to_rgba8888(
            WebpScanlineConvertKind::Bgra8888UnpremulToRgba8888,
            &mut rgba_dst,
            &bgra_src,
            2,
        ));
        assert_eq!(rgba_dst, [30, 20, 10, 40, 70, 60, 50, 80]);

        assert!(convert_scanline_to_rgba8888(
            WebpScanlineConvertKind::Bgra8888OpaqueToRgba8888,
            &mut rgba_dst,
            &bgra_src,
            2,
        ));
        assert_eq!(rgba_dst, [30, 20, 10, 0xFF, 70, 60, 50, 0xFF]);

        // 2x2 Gray8 with stride=3 -> dst stride=12
        let gray_src = [11u8, 22, 0xEE, 33, 44, 0xEE];
        let mut gray_dst = [0x99u8; 24];
        assert!(convert_image_to_rgba8888(
            WebpScanlineConvertKind::Gray8ToRgba8888,
            &mut gray_dst,
            12,
            &gray_src,
            3,
            2,
            2,
        ));
        assert_eq!(&gray_dst[0..8], &[11, 11, 11, 255, 22, 22, 22, 255]);
        assert_eq!(&gray_dst[8..12], &[0x99; 4]);
        assert_eq!(&gray_dst[12..20], &[33, 33, 33, 255, 44, 44, 44, 255]);

        // Mux plan
        assert!(!plan_mux(false, false, false).use_mux);
        assert!(plan_mux(true, false, false).write_iccp_chunk);
        assert!(plan_mux(false, true, false).use_mux);
        assert!(plan_mux(false, false, true).write_xmp_chunk);

        // Animated encode state
        let mut anim = WebpAnimatedEncodeState::new(32, 24);
        assert_eq!(anim.next_frame_timestamp(32, 24, 100), Some(0));
        assert_eq!(anim.next_frame_timestamp(32, 24, 50), Some(100));
        assert_eq!(anim.timestamp, 150);
        assert_eq!(anim.next_frame_timestamp(16, 24, 50), None);
    }
}
