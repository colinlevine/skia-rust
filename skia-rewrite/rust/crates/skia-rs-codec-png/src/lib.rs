#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's PNG decoder and encoder state machines,
//! chunk/HDR/gainmap/ICC/palette/format-planning logic (`P2.4`).

pub mod chunks;
pub mod codec_base;
pub mod decoder;
pub mod encoder;
pub mod encoder_base;

#[cfg(test)]
mod tests {
    use skia_rs_codec_core::types::{
        skcms_pixel_format, CodecResult, EncodedAlpha, EncodedColor, ZeroInitialized,
    };
    use skia_rs_codec_meta::color_profile::DataSpace;
    use skia_rs_color::color::mul_div_255_round;
    use skia_rs_color::color_space::{GAMUT_SRGB, TF_SRGB};
    use skia_rs_color::image_info::{AlphaType, ColorType};

    use crate::chunks::*;
    use crate::codec_base::*;
    use crate::decoder::*;
    use crate::encoder::*;
    use crate::encoder_base::*;

    // =========================================================================
    // Chunks, signature, color profile, unknown chunk classification
    // =========================================================================

    #[test]
    fn png_signature_validation() {
        assert!(is_png(&PNG_SIGNATURE));
        let mut with_extra = Vec::from(PNG_SIGNATURE);
        with_extra.extend_from_slice(b"IHDR");
        assert!(is_png(&with_extra));

        assert!(!is_png(&[]));
        assert!(!is_png(&PNG_SIGNATURE[..7]));
        let mut bad = PNG_SIGNATURE;
        bad[0] = 0x00;
        assert!(!is_png(&bad));
        let mut bad_end = PNG_SIGNATURE;
        bad_end[7] = 0x00;
        assert!(!is_png(&bad_end));
    }

    #[test]
    fn chunk_header_roundtrip_and_tags() {
        let idat = encode_idat_header(0x0001_2345);
        let parsed = parse_chunk_header(&idat).unwrap();
        assert_eq!(parsed.length, 0x0001_2345);
        assert!(parsed.is_idat());
        assert!(!parsed.is_iend());
        assert_eq!(parsed.payload_plus_crc_len(), Some(0x0001_2349));

        let iend = encode_chunk_header(0, *b"IEND");
        let parsed_iend = parse_chunk_header(&iend).unwrap();
        assert_eq!(parsed_iend.length, 0);
        assert!(parsed_iend.is_iend());
        assert!(!parsed_iend.is_idat());
        assert_eq!(parsed_iend.payload_plus_crc_len(), Some(4));

        assert!(parse_chunk_header(&[0; 7]).is_none());
    }

    #[test]
    fn unknown_chunk_classification() {
        assert_eq!(classify_unknown_chunk(b"gmAP", 16), UnknownChunkKind::GainmapInfo);
        assert_eq!(classify_unknown_chunk(b"gdAT", 64), UnknownChunkKind::GainmapData);
        assert_eq!(
            classify_unknown_chunk(b"cLLI", 8),
            UnknownChunkKind::ContentLightLevelInfo
        );
        assert_eq!(
            classify_unknown_chunk(b"mDCV", 24),
            UnknownChunkKind::MasteringDisplayColorVolume
        );
        // Empty payload must be ignored
        assert_eq!(classify_unknown_chunk(b"gmAP", 0), UnknownChunkKind::Ignored);
        assert_eq!(classify_unknown_chunk(b"gdAT", 0), UnknownChunkKind::Ignored);
        assert_eq!(classify_unknown_chunk(b"cLLI", 0), UnknownChunkKind::Ignored);
        assert_eq!(classify_unknown_chunk(b"mDCV", 0), UnknownChunkKind::Ignored);
        // Unknown tags ignored
        assert_eq!(classify_unknown_chunk(b"npTc", 32), UnknownChunkKind::Ignored);
        assert_eq!(classify_unknown_chunk(b"eXIf", 32), UnknownChunkKind::Ignored);
    }

    #[test]
    fn color_profile_compatibility() {
        // No profile is always compatible
        assert!(is_compatible_color_profile_and_type(None, EncodedColor::Rgb));
        assert!(is_compatible_color_profile_and_type(None, EncodedColor::Gray));
        // CMYK profile is never compatible with PNG
        assert!(!is_compatible_color_profile_and_type(
            Some(DataSpace::Cmyk),
            EncodedColor::Rgba
        ));
        assert!(!is_compatible_color_profile_and_type(
            Some(DataSpace::Cmyk),
            EncodedColor::Gray
        ));
        // Gray profile only compatible with Gray and GrayAlpha
        assert!(is_compatible_color_profile_and_type(
            Some(DataSpace::Gray),
            EncodedColor::Gray
        ));
        assert!(is_compatible_color_profile_and_type(
            Some(DataSpace::Gray),
            EncodedColor::GrayAlpha
        ));
        assert!(!is_compatible_color_profile_and_type(
            Some(DataSpace::Gray),
            EncodedColor::Rgb
        ));
        assert!(!is_compatible_color_profile_and_type(
            Some(DataSpace::Gray),
            EncodedColor::Rgba
        ));
        assert!(!is_compatible_color_profile_and_type(
            Some(DataSpace::Gray),
            EncodedColor::Palette
        ));
        // RGB and Other profiles are compatible with all PNG types
        assert!(is_compatible_color_profile_and_type(
            Some(DataSpace::Rgb),
            EncodedColor::Gray
        ));
        assert!(is_compatible_color_profile_and_type(
            Some(DataSpace::Other),
            EncodedColor::Rgba
        ));
    }

    #[test]
    fn color_profile_priority_and_chrm_gama_math() {
        let zero_chrm = [0i32; 8];
        // iCCP wins over sRGB/cHRM/gAMA
        let p = plan_color_profile(true, true, true, &zero_chrm, true, 45455);
        assert_eq!(p.kind, PngColorProfileKind::UseIcc);

        // sRGB wins over cHRM/gAMA
        let p = plan_color_profile(false, true, true, &zero_chrm, true, 45455);
        assert_eq!(p.kind, PngColorProfileKind::UseSrgbNull);

        // Neither iCCP nor sRGB nor cHRM nor gAMA -> defaults to sRGB TF + sRGB gamut
        let p = plan_color_profile(false, false, false, &zero_chrm, false, 0);
        assert_eq!(p.kind, PngColorProfileKind::UseCustomTfAndMatrix);
        assert!(p.transfer_fn.bit_eq(&TF_SRGB));
        assert!(p.to_xyzd50.bit_eq(&GAMUT_SRGB));

        // gAMA = 45455 (approx 1/2.2)
        let p = plan_color_profile(false, false, false, &zero_chrm, true, 45455);
        assert_eq!(p.kind, PngColorProfileKind::UseCustomTfAndMatrix);
        assert!((p.transfer_fn.g - (1.0 / 0.45455)).abs() < 1e-5);
        assert_eq!(p.transfer_fn.a, 1.0);
        assert_eq!(p.transfer_fn.b, 0.0);
        assert!(p.to_xyzd50.bit_eq(&GAMUT_SRGB));

        // Valid Display P3 cHRM primaries (wx=0.3127, wy=0.3290, rx=0.68, ry=0.32, gx=0.265, gy=0.69, bx=0.15, by=0.06)
        let p3_chrm = [31270, 32900, 68000, 32000, 26500, 69000, 15000, 6000];
        let p = plan_color_profile(false, false, true, &p3_chrm, false, 0);
        assert_eq!(p.kind, PngColorProfileKind::UseCustomTfAndMatrix);
        assert!(!p.to_xyzd50.bit_eq(&GAMUT_SRGB));
        assert!(p.to_xyzd50.is_finite());

        // Invalid cHRM (wy == 0) falls back to GAMUT_SRGB
        let bad_chrm = [31270, 0, 68000, 32000, 26500, 69000, 15000, 6000];
        let p = plan_color_profile(false, false, true, &bad_chrm, false, 0);
        assert!(p.to_xyzd50.bit_eq(&GAMUT_SRGB));
    }

    // =========================================================================
    // SkPngCodecBase: skcms format, xform/swizzler plans, palette table
    // =========================================================================

    #[test]
    fn skcms_pixel_format_selection() {
        assert_eq!(
            encoded_info_to_skcms_pixel_format(EncodedColor::Rgba, 16),
            SKCMS_PIXEL_FORMAT_RGBA_16161616BE
        );
        assert_eq!(
            encoded_info_to_skcms_pixel_format(EncodedColor::Rgb, 16),
            SKCMS_PIXEL_FORMAT_RGB_161616BE
        );
        assert_eq!(
            encoded_info_to_skcms_pixel_format(EncodedColor::Gray, 8),
            skcms_pixel_format::G_8
        );
        assert_eq!(
            encoded_info_to_skcms_pixel_format(EncodedColor::Rgba, 8),
            skcms_pixel_format::RGBA_8888
        );
        assert_eq!(
            encoded_info_to_skcms_pixel_format(EncodedColor::Rgb, 8),
            skcms_pixel_format::RGBA_8888
        );
        assert_eq!(
            encoded_info_to_skcms_pixel_format(EncodedColor::Palette, 8),
            skcms_pixel_format::RGBA_8888
        );
    }

    #[test]
    fn xform_and_swizzler_plans() {
        // Mismatched frame_width + subset -> InvalidParameters
        let bad = plan_xform_init(EncodedColor::Rgba, 8, 32, 100, 50, true, false);
        assert_eq!(bad.result, CodecResult::InvalidParameters);

        // 8-bit RGBA with color xform and no subset -> ColorOnly
        let p = plan_xform_init(EncodedColor::Rgba, 8, 32, 100, 100, false, true);
        assert_eq!(p.result, CodecResult::Success);
        assert_eq!(p.encoded_row_bytes, 400);
        assert!(p.skip_format_conversion);
        assert!(p.color_only_mode);
        assert!(!p.need_color_table);
        assert_eq!(compute_storage_bytes(XformMode::ColorOnly, 32, 100), 400);

        // 16-bit RGB with color xform and subset -> needs simple swizzler with srcBPP = 6
        let p = plan_xform_init(EncodedColor::Rgb, 16, 48, 100, 100, true, true);
        assert!(p.skip_format_conversion);
        assert!(!p.color_only_mode);
        let sw = plan_swizzler(
            EncodedColor::Rgb,
            16,
            ColorType::RgbaF16,
            AlphaType::Opaque,
            ZeroInitialized::Yes,
            true,
            true,
            true,
            true,
        );
        assert_eq!(sw.xform_mode, XformMode::SwizzleColor);
        assert!(sw.use_simple_swizzler);
        assert_eq!(sw.simple_src_bpp, 6);
        assert!(!sw.use_frame_rect);
        assert_eq!(sw.swizzler_zero_init, ZeroInitialized::No);
        assert_eq!(compute_storage_bytes(XformMode::SwizzleColor, 48, 100), 600);

        // 8-bit RGB with color xform -> does NOT skip format conversion (skcms expects RGBA_8888)
        let p = plan_xform_init(EncodedColor::Rgb, 8, 24, 100, 100, false, true);
        assert!(!p.skip_format_conversion);
        assert!(!p.color_only_mode);

        // Palette with color xform -> needs color table, no skipFormatConversion
        let p = plan_xform_init(EncodedColor::Palette, 8, 8, 64, 64, false, true);
        assert!(p.need_color_table);
        assert!(!p.skip_format_conversion);

        // Gray8 with color xform -> swizzler uses Gray8 and srcBPP = 1
        let sw = plan_swizzler(
            EncodedColor::Gray,
            8,
            ColorType::Rgba8888,
            AlphaType::Opaque,
            ZeroInitialized::Yes,
            true,
            true,
            true,
            false,
        );
        assert_eq!(sw.swizzler_ct, ColorType::Gray8);
        assert_eq!(sw.simple_src_bpp, 1);
        assert!(sw.use_frame_rect);
    }

    #[test]
    fn palette_color_table_and_trns_premul_and_padding() {
        // 3 palette entries, 2 tRNS alpha entries, 4-bit palette (max_colors = 16)
        let plte = [
            200u8, 100, 50, // entry 0
            10, 20, 30, // entry 1
            255, 128, 0, // entry 2 (no tRNS -> opaque)
        ];
        let trns = [128u8, 0];
        let mut table = [0u32; MAX_PALETTE_ENTRIES];

        // Premul RGBA without colorXform
        let res = populate_color_table_entries(
            &plte,
            Some(&trns),
            4,
            EncodedAlpha::Unpremul,
            ColorType::Rgba8888,
            AlphaType::Premul,
            false,
            false,
            true,
            &mut table,
        );
        assert_eq!(res.num_colors, 3);
        assert_eq!(res.max_colors, 16);
        assert!(!res.should_xform_table);

        let r0 = mul_div_255_round(200, 128);
        let g0 = mul_div_255_round(100, 128);
        let b0 = mul_div_255_round(50, 128);
        assert_eq!(table[0], (128 << 24) | (b0 << 16) | (g0 << 8) | r0);
        assert_eq!(table[1], 0); // alpha 0 premul -> 0
        assert_eq!(table[2], (0xFF << 24) | (0 << 16) | (128 << 8) | 255);

        pad_color_table(&mut table, res.num_colors, res.max_colors);
        for &slot in &table[3..16] {
            assert_eq!(slot, table[2]);
        }

        // With colorXform active (e.g. Codec_PngPaletteTrnsF16Premul):
        // Table must NOT premultiply because applyColorXform handles premul!
        let res_xform = populate_color_table_entries(
            &plte,
            Some(&trns),
            8,
            EncodedAlpha::Unpremul,
            ColorType::RgbaF16,
            AlphaType::Premul,
            true,
            true, // xformOnDecode for F16
            true,
            &mut table,
        );
        assert!(!res_xform.should_xform_table);
        assert_eq!(table[0], (128 << 24) | (50 << 16) | (100 << 8) | 200);

        // Empty palette pads with SK_ColorBLACK
        let mut empty_table = [0x1234_5678u32; MAX_PALETTE_ENTRIES];
        pad_color_table(&mut empty_table, 0, 4);
        assert_eq!(&empty_table[0..4], &[COLOR_BLACK; 4]);
    }

    // =========================================================================
    // SkPngCodec: header plan, normal decoder, interlaced decoder
    // =========================================================================

    #[test]
    fn decode_header_plans_all_color_types_and_sbit() {
        // 16-bit Gray -> stripped to 8-bit Gray
        let p = plan_decode_header(PNG_COLOR_TYPE_GRAY, 16, false, false, 0, 0, 0, 0, 0);
        assert!(p.strip_16);
        assert_eq!(p.output_bit_depth, 8);
        assert_eq!(p.final_encoded_color, EncodedColor::Gray);
        assert_eq!(p.encoded_alpha, EncodedAlpha::Opaque);

        // 4-bit Gray + tRNS -> expanded to 8-bit + tRNS to alpha -> GrayAlpha Binary
        let p = plan_decode_header(PNG_COLOR_TYPE_GRAY, 4, true, false, 0, 0, 0, 0, 0);
        assert!(p.expand_gray_1_2_4_to_8);
        assert!(p.trns_to_alpha);
        assert_eq!(p.output_bit_depth, 8);
        assert_eq!(p.final_encoded_color, EncodedColor::GrayAlpha);
        assert_eq!(p.encoded_alpha, EncodedAlpha::Binary);

        // 2-bit Palette + tRNS -> packing + Unpremul
        let p = plan_decode_header(PNG_COLOR_TYPE_PALETTE, 2, true, false, 0, 0, 0, 0, 0);
        assert!(p.set_packing);
        assert_eq!(p.output_bit_depth, 8);
        assert_eq!(p.final_encoded_color, EncodedColor::Palette);
        assert_eq!(p.encoded_alpha, EncodedAlpha::Unpremul);

        // 16-bit RGB without tRNS -> stays 16-bit RGB
        let p = plan_decode_header(PNG_COLOR_TYPE_RGB, 16, false, false, 0, 0, 0, 0, 0);
        assert!(!p.strip_16);
        assert_eq!(p.output_bit_depth, 16);
        assert_eq!(p.final_encoded_color, EncodedColor::Rgb);

        // 8-bit RGB + 565 sBIT -> profile check is Rgb, final is Rgb565
        let p = plan_decode_header(PNG_COLOR_TYPE_RGB, 8, false, true, 5, 6, 5, 0, 0);
        assert_eq!(p.profile_check_color, EncodedColor::Rgb);
        assert_eq!(p.final_encoded_color, EncodedColor::Rgb565);
        assert!(p.android_565_sbit_warning);

        // 8-bit GrayAlpha + Skia A8 sBIT (gray=1, alpha=8) -> profile check is GrayAlpha, final is XAlpha
        let p = plan_decode_header(PNG_COLOR_TYPE_GRAY_ALPHA, 8, false, true, 0, 0, 0, 1, 8);
        assert_eq!(p.profile_check_color, EncodedColor::GrayAlpha);
        assert_eq!(p.final_encoded_color, EncodedColor::XAlpha);
        assert_eq!(p.encoded_alpha, EncodedAlpha::Unpremul);
    }

    #[test]
    fn normal_decoder_state_machine() {
        let mut st = NormalDecodeState::new();
        st.set_range(2, 7); // 6 rows: 2..=7
        st.prepare_decode(Some(2)); // sampled dim = 6/2 = 3 rows needed
        assert_eq!(st.rows_needed, 3);

        assert_eq!(st.relative_row(0), None);
        assert_eq!(st.relative_row(1), None);
        assert_eq!(st.on_row_callback(1, true), NormalRowAction::Ignore);

        assert_eq!(st.relative_row(2), Some(0));
        assert_eq!(st.on_row_callback(2, false), NormalRowAction::Ignore);
        assert_eq!(
            st.on_row_callback(3, true),
            NormalRowAction::TransformAndContinue
        );
        assert_eq!(
            st.on_row_callback(5, true),
            NormalRowAction::TransformAndContinue
        );
        assert_eq!(
            st.on_row_callback(7, true),
            NormalRowAction::TransformAndStop
        );
        assert_eq!(st.finish_decode(true), (CodecResult::Success, 3));

        // Partial decode failure returns IncompleteInput if stream ended cleanly, ErrorInInput on error
        let mut st2 = NormalDecodeState::new();
        st2.start_all_rows(10);
        st2.on_all_rows_callback(0);
        st2.on_all_rows_callback(1);
        assert_eq!(st2.finish_all_rows(true, 10), (CodecResult::IncompleteInput, 2));
        assert_eq!(st2.finish_all_rows(false, 10), (CodecResult::ErrorInInput, 2));
    }

    #[test]
    fn interlaced_decoder_state_machine() {
        assert_eq!(InterlacedDecodeState::compute_buffer_size(100, 10), Some(1000));
        assert_eq!(InterlacedDecodeState::compute_buffer_size(usize::MAX, 2), None);
        assert_eq!(InterlacedDecodeState::compute_buffer_size(16, -1), None);

        let mut st = InterlacedDecodeState::new(7, 10);
        st.reset_range(2, 5, 32); // subset rows 2..=5 (height 4)

        // Row outside subset is ignored
        let a = st.on_row(1, 0, 1);
        assert!(a.ignore);

        // Pass 0 initializes all 4 rows
        for r in 2..=5 {
            let act = st.on_row(r, 0, 1);
            assert!(!act.ignore);
            assert_eq!(act.buffer_offset, ((r - 2) as usize) * 32);
            assert!(!act.stop_decoding);
        }
        assert_eq!(st.lines_decoded, 4);

        // Last pass (6), last row (5) != image_height - 1 (9) -> stop_decoding = true
        let act = st.on_row(5, 6, 1);
        assert!(!act.ignore);
        assert!(act.stop_decoding);
        assert!(st.interlaced_complete);

        // Output schedule with sampleY = 2 -> start=1, step=2, rows_to_write=2 (src rows 1, 3)
        let sched = st.output_schedule(2);
        assert!(sched.has_lines);
        assert_eq!(sched.start_src_row, 1);
        assert_eq!(sched.step, 2);
        assert_eq!(sched.rows_to_write, 2);
        assert_eq!(st.finish(true, sched.rows_to_write), (CodecResult::Success, 2));
    }

    // =========================================================================
    // SkPngEncoderBase & SkPngEncoderImpl
    // =========================================================================

    #[test]
    fn encoder_target_info_exhaustive_color_types() {
        // Gray8 -> Gray 8-bit
        let t = get_target_info(16, ColorType::Gray8, AlphaType::Opaque);
        assert!(t.valid && !t.is_alpha8);
        assert_eq!(t.dst_encoded_color, EncodedColor::Gray);
        assert_eq!(t.dst_bits_per_component, 8);
        assert_eq!(t.dst_row_ct, ColorType::Gray8);
        assert_eq!(t.dst_row_size, 16);

        // Alpha8 -> GrayAlpha 8-bit (special alpha8 path)
        let t = get_target_info(16, ColorType::Alpha8, AlphaType::Premul);
        assert!(t.valid && t.is_alpha8);
        assert_eq!(t.dst_encoded_color, EncodedColor::GrayAlpha);
        assert_eq!(t.dst_bits_per_component, 8);
        assert_eq!(t.dst_row_size, 32);

        // Alpha8 with negative width -> invalid
        assert!(!get_target_info(-1, ColorType::Alpha8, AlphaType::Premul).valid);

        // Unsupported 1-channel / 2-channel types
        for ct in [
            ColorType::Unknown,
            ColorType::R8Unorm,
            ColorType::A16Unorm,
            ColorType::A16Float,
            ColorType::R16Unorm,
            ColorType::R16Float,
            ColorType::R8G8Unorm,
            ColorType::R16G16Unorm,
            ColorType::R16G16Float,
        ] {
            assert!(!get_target_info(8, ct, AlphaType::Opaque).valid, "{ct:?}");
        }

        // 3-channel <= 8 bpc (RGB565, RGB888x, BGR888x) -> RGBA 8-bit w/ dstRowInfo RGB_888x Opaque
        for ct in [ColorType::Rgb565, ColorType::Rgb888x, ColorType::Bgr888x] {
            let t = get_target_info(10, ct, AlphaType::Opaque);
            assert!(t.valid);
            assert_eq!(t.dst_encoded_color, EncodedColor::Rgba);
            assert_eq!(t.dst_bits_per_component, 8);
            assert_eq!(t.dst_row_ct, ColorType::Rgb888x);
            assert_eq!(t.dst_row_at, AlphaType::Opaque);
            assert_eq!(t.dst_row_size, 40);
        }

        // 3-channel > 8 bpc (RGB101010x, RGB_F16x) -> RGBA 16-bit Opaque
        for ct in [
            ColorType::Rgb101010x,
            ColorType::Bgr101010x,
            ColorType::Bgr101010xXr,
            ColorType::RgbF16F16F16x,
        ] {
            let t = get_target_info(10, ct, AlphaType::Opaque);
            assert!(t.valid);
            assert_eq!(t.dst_bits_per_component, 16);
            assert_eq!(t.dst_row_ct, ColorType::R16G16B16A16Unorm);
            assert_eq!(t.dst_row_at, AlphaType::Opaque);
            assert_eq!(t.dst_row_size, 80);
        }

        // 4-channel <= 8 bpc (RGBA8888, BGRA8888, ARGB4444, SRGBA8888)
        for ct in [
            ColorType::Rgba8888,
            ColorType::Bgra8888,
            ColorType::Argb4444,
            ColorType::Srgba8888,
        ] {
            let t_opaque = get_target_info(10, ct, AlphaType::Opaque);
            assert_eq!(t_opaque.dst_row_ct, ColorType::Rgb888x);
            assert_eq!(t_opaque.dst_row_at, AlphaType::Opaque);

            let t_premul = get_target_info(10, ct, AlphaType::Premul);
            assert_eq!(t_premul.dst_row_ct, ColorType::Rgba8888);
            assert_eq!(t_premul.dst_row_at, AlphaType::Unpremul);

            assert!(!get_target_info(10, ct, AlphaType::Unknown).valid);
        }

        // 4-channel > 8 bpc (1010102, F16, F32, R16G16B16A16) -> 16-bit
        for ct in [
            ColorType::Rgba1010102,
            ColorType::Bgra1010102,
            ColorType::Bgra10101010Xr,
            ColorType::Rgba10x6,
            ColorType::RgbaF16Norm,
            ColorType::RgbaF16,
            ColorType::RgbaF32,
            ColorType::R16G16B16A16Unorm,
        ] {
            let t = get_target_info(4, ct, AlphaType::Premul);
            assert!(t.valid);
            assert_eq!(t.dst_bits_per_component, 16);
            assert_eq!(t.dst_row_ct, ColorType::R16G16B16A16Unorm);
            assert_eq!(t.dst_row_at, AlphaType::Unpremul);
            assert_eq!(t.dst_row_size, 32);
        }
    }

    #[test]
    fn encoder_header_sigbit_filters_zlib_and_kernels() {
        let h = plan_encode_header(
            EncodedColor::Rgba,
            8,
            true,
            ColorType::Rgb565,
            PNG_FILTER_SUB | PNG_FILTER_PAETH,
            12, // clamped to 9
        );
        assert!(h.valid);
        assert_eq!(h.png_color_type, PNG_COLOR_TYPE_RGB);
        assert!(h.sig_bit.set);
        assert_eq!((h.sig_bit.red, h.sig_bit.green, h.sig_bit.blue), (5, 6, 5));
        assert_eq!(h.filters, PNG_FILTER_SUB | PNG_FILTER_PAETH);
        assert!(h.filters_exact);
        assert_eq!(h.zlib_level, 9);
        assert!(!h.zlib_level_exact);
        assert!(h.needs_filler_after);
        assert!(!h.needs_byte_swap);

        // Alpha8 -> GrayAlpha with gray=1, alpha=8
        let h_a8 = plan_encode_header(
            EncodedColor::GrayAlpha,
            8,
            false,
            ColorType::Alpha8,
            PNG_ALL_FILTERS,
            6,
        );
        assert_eq!(h_a8.png_color_type, PNG_COLOR_TYPE_GRAY_ALPHA);
        assert!(h_a8.sig_bit.set);
        assert_eq!(h_a8.sig_bit.gray, 1);
        assert_eq!(h_a8.sig_bit.alpha, 8);
        assert!(!h_a8.needs_filler_after);

        // 16-bit F16 -> needs_byte_swap
        let h_f16 = plan_encode_header(
            EncodedColor::Rgba,
            16,
            false,
            ColorType::RgbaF16,
            PNG_ALL_FILTERS,
            -1,
        );
        assert_eq!(h_f16.png_color_type, PNG_COLOR_TYPE_RGBA);
        assert_eq!(h_f16.zlib_level, 0);
        assert!(h_f16.needs_byte_swap);

        // Comments & keyword clipping
        assert!(validate_comments_count(0));
        assert!(validate_comments_count(4));
        assert!(!validate_comments_count(3));
        assert!(!validate_comments_count(-2));
        assert_eq!(clip_text_keyword_len(10), 10);
        assert_eq!(clip_text_keyword_len(120), PNG_KEYWORD_MAX_LENGTH);

        // ColorSpace & Gainmap plans
        assert_eq!(
            plan_encode_color_space(true, true, false),
            PngColorSpaceEncodeAction::WriteSrgb
        );
        assert_eq!(
            plan_encode_color_space(true, true, true),
            PngColorSpaceEncodeAction::WriteIcc
        );
        assert_eq!(
            plan_encode_color_space(false, false, false),
            PngColorSpaceEncodeAction::WriteIcc
        );
        assert_eq!(plan_encode_gainmap(true, true), PngGainmapEncodeMode::BaseWithGainmap);
        assert_eq!(plan_encode_gainmap(true, false), PngGainmapEncodeMode::GainmapInfoOnly);
        assert_eq!(plan_encode_gainmap(false, true), PngGainmapEncodeMode::None);
        assert!(gainmap_supports_rgb_icc(EncodedColor::Rgba));
        assert!(!gainmap_supports_rgb_icc(EncodedColor::Gray));
        assert!(!gainmap_supports_rgb_icc(EncodedColor::GrayAlpha));

        // Row kernels
        let src_a8 = [0x11u8, 0x22, 0xFF];
        let mut dst_ga = [0xAAu8; 6];
        transform_scanline_a8_to_gray_alpha(&mut dst_ga, &src_a8);
        assert_eq!(dst_ga, [0, 0x11, 0, 0x22, 0, 0xFF]);

        let mut be = [0x01u8, 0x02, 0x03, 0x04];
        swap_16bit_row_endianness(&mut be);
        assert_eq!(be, [0x02, 0x01, 0x04, 0x03]);

        let mut rgb = [0u8; 6];
        strip_rgbx8_to_rgb8(&mut rgb, &[1, 2, 3, 255, 4, 5, 6, 0]);
        assert_eq!(rgb, [1, 2, 3, 4, 5, 6]);

        let mut rgb16be = [0u8; 6];
        strip_rgba16le_to_rgb16be(&mut rgb16be, &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0xFF, 0xFF]);
        assert_eq!(rgb16be, [0x02, 0x01, 0x04, 0x03, 0x06, 0x05]);
    }
}
