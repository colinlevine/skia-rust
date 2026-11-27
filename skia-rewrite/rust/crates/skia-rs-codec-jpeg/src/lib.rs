#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's JPEG segment scanner, MPF/XMP/metadata/gainmap
//! parsers, JPEG codec state machine, and JPEG/gainmap encoder logic (`P2.5`).

pub mod constants;
pub mod decoder;
pub mod encoder;
pub mod metadata;
pub mod multi_picture;
pub mod segment_scan;
pub mod source_mgr;
pub mod xmp;

#[cfg(test)]
mod tests {
    use skia_rs_codec_core::types::EncodedColor;
    use skia_rs_codec_meta::color_profile::DataSpace;
    use skia_rs_codec_meta::gainmap::{BaseImageType, GainmapInfo};
    use skia_rs_color::image_info::{AlphaType, ColorType};
    use skia_rs_color::yuva_info::{PlaneConfig, Subsampling};
    use skia_rs_math::md5::Md5State;

    use crate::constants::*;
    use crate::decoder::*;
    use crate::encoder::*;
    use crate::metadata::*;
    use crate::multi_picture::*;
    use crate::segment_scan::*;
    use crate::source_mgr::*;
    use crate::xmp::*;

    fn make_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let len = (payload.len() + 2) as u16;
        let mut out = Vec::with_capacity(4 + payload.len());
        out.push(0xFF);
        out.push(marker);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn make_minimal_jpeg(extra_segments: &[(u8, &[u8])]) -> Vec<u8> {
        let mut out = vec![0xFF, MARKER_START_OF_IMAGE];
        for &(m, payload) in extra_segments {
            out.extend_from_slice(&make_segment(m, payload));
        }
        // SOS with 4 bytes payload + entropy bytes containing stuffed 0xFF 0x00 and 0xFF fill + EOI.
        out.extend_from_slice(&make_segment(MARKER_START_OF_SCAN, &[1, 2, 3, 4]));
        out.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56, 0xFF, 0xFF, 0xFF]);
        out.push(MARKER_END_OF_IMAGE);
        out
    }

    #[test]
    fn segment_scanner_basic_and_chunked() {
        let jpeg = make_minimal_jpeg(&[(MARKER_APP0 + 1, b"Exif\0\0hello"), (MARKER_APP0 + 2, b"MPF\0abc")]);

        // Scan in one shot to EOI.
        let mut scan_all = SegmentScanner::new(MARKER_END_OF_IMAGE);
        scan_all.on_bytes(&jpeg);
        assert!(scan_all.is_done());
        assert!(!scan_all.had_error());
        let segs = scan_all.segments();
        assert_eq!(segs.len(), 6); // SOI, APP1, APP2, SOS, RST0, EOI
        assert_eq!(segs[0], JpegSegment { offset: 0, marker: 0xD8, parameter_length: 0 });
        assert_eq!(segs[1], JpegSegment { offset: 2, marker: 0xE1, parameter_length: 13 });
        assert_eq!(segs[2], JpegSegment { offset: 17, marker: 0xE2, parameter_length: 9 });
        assert_eq!(segs[3].marker, MARKER_START_OF_SCAN);
        assert_eq!(segs[3].parameter_length, 6);
        assert_eq!(segs[4].marker, 0xD0); // RST0 standalone
        assert_eq!(segs[4].parameter_length, 0);
        assert_eq!(segs[5].marker, MARKER_END_OF_IMAGE);
        assert_eq!(segs[5].parameter_length, 0);

        // Scan 1 byte at a time: must produce identical segments!
        let mut scan_1b = SegmentScanner::new(MARKER_END_OF_IMAGE);
        for b in &jpeg {
            scan_1b.on_bytes(std::slice::from_ref(b));
        }
        assert!(scan_1b.is_done());
        assert_eq!(scan_1b.segments(), segs);

        // Scan with stopMarker = SOS.
        let mut scan_sos = SegmentScanner::new(MARKER_START_OF_SCAN);
        scan_sos.on_bytes(&jpeg);
        assert!(scan_sos.is_done());
        assert_eq!(scan_sos.segments().len(), 4);
        assert_eq!(scan_sos.segments()[3].marker, MARKER_START_OF_SCAN);
        assert_eq!(scan_sos.segments()[3].parameter_length, 0);

        // Verify parameter range & memory header check.
        let (off, len) = segment_parameter_range(jpeg.len(), segs[1]).unwrap();
        assert_eq!(&jpeg[off..off + len], b"Exif\0\0hello");
        let (moff, mlen) = verify_memory_segment_parameters(&jpeg, segs[2]).unwrap();
        assert_eq!(&jpeg[moff..moff + mlen], b"MPF\0abc");
        assert!(verify_memory_segment_parameters(&jpeg, segs[0]).is_none());
    }

    #[test]
    fn segment_scanner_error_cases() {
        // Bad SOI
        let mut s = SegmentScanner::new_to_eoi();
        s.on_bytes(&[0x00, 0xD8, 0xFF]);
        assert!(s.had_error());

        // Second marker 0xFF 0x00 invalid
        let mut s = SegmentScanner::new_to_eoi();
        s.on_bytes(&[0xFF, 0xD8, 0xFF, 0x00]);
        assert!(s.had_error());

        // Parameter length < 2 invalid
        let mut s = SegmentScanner::new_to_eoi();
        s.on_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x01]);
        assert!(s.had_error());

        // Fill bytes followed by 0x00 invalid
        let mut s = SegmentScanner::new_to_eoi();
        s.on_bytes(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xFF, 0x00]);
        assert!(s.had_error());
    }

    #[test]
    fn mpf_serialize_parse_roundtrip_and_offsets() {
        let mut params = MultiPictureParameters::with_image_count(3);
        params.images[0] = MpImage { size: 1000, data_offset: 0 };
        params.images[1] = MpImage { size: 500, data_offset: 960 };
        params.images[2] = MpImage { size: 250, data_offset: 1460 };

        let serialized0 = params.serialize(0);
        assert!(serialized0.starts_with(MPF_SIG));
        let parsed = MultiPictureParameters::parse(&serialized0).expect("valid MPF");
        assert_eq!(parsed, params);

        // Secondary image MPF has only version tag (not an Index IFD with entries -> parse returns None).
        let serialized1 = params.serialize(1);
        assert!(serialized1.starts_with(MPF_SIG));
        assert!(MultiPictureParameters::parse(&serialized1).is_none());

        // Offset math: mp_segment_offset = 20 -> header_abs = 20 + 2 + 2 + 4 = 28.
        assert_eq!(get_image_absolute_offset(0, 20), 0);
        assert_eq!(get_image_absolute_offset(100, 20), 128);
        assert_eq!(get_image_data_offset(0, 20), 0);
        assert_eq!(get_image_data_offset(128, 20), 100);
    }

    #[test]
    fn mpf_rejects_bad_inputs() {
        assert!(MultiPictureParameters::parse(b"NOTMPF").is_none());
        assert!(MultiPictureParameters::parse(b"MPF\0short").is_none());

        // Primary image with non-zero dataOffset must be rejected.
        let mut bad = MultiPictureParameters::with_image_count(2);
        bad.images[0] = MpImage { size: 100, data_offset: 42 };
        bad.images[1] = MpImage { size: 50, data_offset: 100 };
        let bytes = bad.serialize(0);
        assert!(MultiPictureParameters::parse(&bytes).is_none());
    }

    #[test]
    fn xmp_standard_and_extended_reassembly() {
        let std_xml = b"<x:xmpmeta>standard</x:xmpmeta>";
        let mut std_seg = Vec::new();
        std_seg.extend_from_slice(XMP_STANDARD_SIG);
        std_seg.extend_from_slice(std_xml);

        let ext_payload = b"<x:xmpmeta>extended-part-one-and-part-two</x:xmpmeta>";
        let mut md5 = Md5State::new();
        md5.update(ext_payload);
        let digest = md5.finish();
        let guid: String = digest.iter().map(|b| format!("{:02X}", b)).collect();
        let guid_bytes = guid.as_bytes();
        assert_eq!(parse_guid_ascii(guid_bytes), Some(digest));
        // Lowercase hex must fail.
        assert!(parse_guid_ascii(guid.to_ascii_lowercase().as_bytes()).is_none());

        // Split extended payload into 2 out-of-order chunks.
        let split = 19;
        let mut chunk1 = Vec::new();
        chunk1.extend_from_slice(XMP_EXTENDED_SIG);
        chunk1.extend_from_slice(guid_bytes);
        chunk1.extend_from_slice(&(ext_payload.len() as u32).to_be_bytes());
        chunk1.extend_from_slice(&(split as u32).to_be_bytes());
        chunk1.extend_from_slice(&ext_payload[split..]);

        let mut chunk0 = Vec::new();
        chunk0.extend_from_slice(XMP_EXTENDED_SIG);
        chunk0.extend_from_slice(guid_bytes);
        chunk0.extend_from_slice(&(ext_payload.len() as u32).to_be_bytes());
        chunk0.extend_from_slice(&0u32.to_be_bytes());
        chunk0.extend_from_slice(&ext_payload[..split]);

        let app1s: Vec<&[u8]> = vec![&std_seg, &chunk1, &chunk0];
        let (idx, off) = find_xmp_standard(&app1s).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(&app1s[idx][off..], std_xml);

        let reassembled = read_xmp_extended(&app1s, guid_bytes).unwrap();
        assert_eq!(reassembled.as_slice(), ext_payload);

        // Wrong GUID fails MD5 verification.
        assert!(read_xmp_extended(&app1s, b"00000000000000000000000000000000").is_none());

        // Gap in chunks fails contiguity check.
        let app1s_gap: Vec<&[u8]> = vec![&chunk1];
        assert!(read_xmp_extended(&app1s_gap, guid_bytes).is_none());
    }

    #[test]
    fn metadata_icc_multichunk_and_exif_iso_jumbf() {
        // 2-chunk ICC profile provided out of order (2/2 before 1/2).
        let mut icc2 = Vec::new();
        icc2.extend_from_slice(ICC_SIG);
        icc2.extend_from_slice(&[2, 2, 0xCC, 0xDD]);
        let mut icc1 = Vec::new();
        icc1.extend_from_slice(ICC_SIG);
        icc1.extend_from_slice(&[1, 2, 0xAA, 0xBB]);

        let mut exif = Vec::new();
        exif.extend_from_slice(EXIF_SIG);
        exif.push(0);
        exif.extend_from_slice(b"MM\0*");

        let mut iso = Vec::new();
        iso.extend_from_slice(ISO_GAINMAP_SIG);
        iso.extend_from_slice(&[0, 0, 0, 0]);

        let mut jumbf = Vec::new();
        jumbf.extend_from_slice(JUMBF_SIG);
        jumbf.extend_from_slice(b"boxdata");

        let markers = [
            JpegMarkerRef { marker: ICC_MARKER, data: &icc2 },
            JpegMarkerRef { marker: EXIF_MARKER, data: &exif },
            JpegMarkerRef { marker: ICC_MARKER, data: &icc1 },
            JpegMarkerRef { marker: ISO_GAINMAP_MARKER, data: &iso },
            JpegMarkerRef { marker: JUMBF_MARKER, data: &jumbf },
        ];

        assert_eq!(
            read_icc_profile_data(&markers, false),
            MetadataReadResult::Owned(vec![0xAA, 0xBB, 0xCC, 0xDD])
        );
        assert_eq!(
            read_exif_metadata(&markers, false),
            MetadataReadResult::SingleRef { marker_index: 1, offset: 6, len: 4 }
        );
        assert_eq!(
            read_exif_metadata(&markers, true),
            MetadataReadResult::Owned(b"MM\0*".to_vec())
        );
        assert_eq!(
            read_iso_gainmap_metadata(&markers, true),
            MetadataReadResult::Owned(vec![0, 0, 0, 0])
        );
        assert_eq!(
            read_jumbf_metadata(&markers, true),
            MetadataReadResult::Owned(b"boxdata".to_vec())
        );

        // Duplicate ICC chunk fails.
        let dup_markers = [
            JpegMarkerRef { marker: ICC_MARKER, data: &icc1 },
            JpegMarkerRef { marker: ICC_MARKER, data: &icc1 },
        ];
        assert_eq!(read_icc_profile_data(&dup_markers, true), MetadataReadResult::NotFoundOrInvalid);

        // Missing ICC chunk fails.
        let partial_markers = [JpegMarkerRef { marker: ICC_MARKER, data: &icc1 }];
        assert_eq!(read_icc_profile_data(&partial_markers, true), MetadataReadResult::NotFoundOrInvalid);
    }

    #[test]
    fn source_mgr_selection_and_unseekable_subset_plans() {
        assert_eq!(select_source_mgr_kind(true, false, true, true), SourceMgrKind::Unseekable);
        assert_eq!(select_source_mgr_kind(false, false, true, true), SourceMgrKind::Memory);
        assert_eq!(select_source_mgr_kind(true, true, true, true), SourceMgrKind::Memory);
        assert_eq!(select_source_mgr_kind(true, true, false, false), SourceMgrKind::Buffered);

        // Unseekable subset: already passed offset -> fail
        let p_past = plan_unseekable_subset(100, 50, 99, 10);
        assert!(!p_past.ok);

        // Unseekable subset: entirely inside buffer -> early return keeps last_read_offset/size
        let p_in = plan_unseekable_subset(100, 50, 110, 20);
        assert!(p_in.ok);
        assert_eq!(p_in.copy_from_buffer_offset, 10);
        assert_eq!(p_in.copy_from_buffer_len, 20);
        assert_eq!(p_in.stream_bytes_to_read, 0);
        assert_eq!(p_in.next_last_read_offset, 100);
        assert_eq!(p_in.next_last_read_size, 50);

        // Unseekable subset: partially in buffer, rest from stream
        let p_straddle = plan_unseekable_subset(100, 50, 140, 30);
        assert!(p_straddle.ok);
        assert_eq!(p_straddle.copy_from_buffer_offset, 40);
        assert_eq!(p_straddle.copy_from_buffer_len, 10);
        assert_eq!(p_straddle.stream_bytes_to_read, 20);
        assert_eq!(p_straddle.next_last_read_offset, 150);
        assert_eq!(p_straddle.next_last_read_size, 0);

        // Unseekable subset: beyond buffer -> skip stream
        let p_beyond = plan_unseekable_subset(100, 50, 200, 25);
        assert!(p_beyond.ok);
        assert_eq!(p_beyond.stream_bytes_to_skip, 50);
        assert_eq!(p_beyond.copy_from_buffer_len, 0);
        assert_eq!(p_beyond.stream_bytes_to_read, 25);
        assert_eq!(p_beyond.next_last_read_offset, 150);
        assert_eq!(p_beyond.next_last_read_size, 0);
    }

    #[test]
    fn decoder_policy_scaling_conversion_yuva() {
        assert!(is_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_jpeg(&[0xFF, 0xD8]));
        assert!(!is_jpeg(&[0x89, 0x50, 0x4E, 0x47]));

        assert_eq!(encoded_color_from_jpeg_color_space(JColorSpace::YCbCr).encoded_color, EncodedColor::Yuv as i32);
        assert!(encoded_color_from_jpeg_color_space(JColorSpace::Rgb).android_rgb_safety_log);
        assert!(!encoded_color_from_jpeg_color_space(JColorSpace::Unknown).valid);

        assert!(is_icc_profile_compatible(JColorSpace::Cmyk, DataSpace::Cmyk));
        assert!(!is_icc_profile_compatible(JColorSpace::Cmyk, DataSpace::Rgb));
        assert!(is_icc_profile_compatible(JColorSpace::Grayscale, DataSpace::Gray));
        assert!(is_icc_profile_compatible(JColorSpace::Grayscale, DataSpace::Rgb));
        assert!(!is_icc_profile_compatible(JColorSpace::YCbCr, DataSpace::Gray));

        assert_eq!(scale_numerator_for_desired_scale(1.0), 8);
        assert_eq!(scale_numerator_for_desired_scale(0.9375), 8);
        assert_eq!(scale_numerator_for_desired_scale(0.5), 4);
        assert_eq!(scale_numerator_for_desired_scale(0.1), 1);
        assert_eq!(scaled_dimension_eighths(100, 4), 50);
        assert_eq!(scaled_dimension_eighths(101, 4), 51);
        assert_eq!(find_supported_scale_numerator(100, 80, 50, 40), Some(4));
        assert_eq!(find_supported_scale_numerator(100, 80, 51, 40), None);

        // Conversion plans
        let p565 = plan_conversion(JColorSpace::YCbCr, ColorType::Rgb565, AlphaType::Opaque, false);
        assert!(p565.supported);
        assert_eq!(p565.out_color_space, JColorSpace::Rgb565 as i32);
        assert!(p565.set_dither_none);

        let p565_xf = plan_conversion(JColorSpace::YCbCr, ColorType::Rgb565, AlphaType::Opaque, true);
        assert!(p565_xf.supported);
        assert_eq!(p565_xf.out_color_space, JColorSpace::ExtRgba as i32);
        assert!(!p565_xf.set_dither_none);

        let pgray_bad = plan_conversion(JColorSpace::YCbCr, ColorType::Gray8, AlphaType::Opaque, false);
        assert!(!pgray_bad.supported);

        let pcmyk = plan_conversion(JColorSpace::Ycck, ColorType::Rgba8888, AlphaType::Opaque, false);
        assert!(pcmyk.supported);
        assert_eq!(pcmyk.out_color_space, JColorSpace::Cmyk as i32);

        // YUVA subsampling
        let yuv420 = query_yuva_info(JColorSpace::YCbCr, [2, 1, 1], [2, 1, 1], [10, 5, 5], true, true);
        assert!(yuv420.supported);
        assert_eq!(yuv420.subsampling, Subsampling::S420 as i32);
        assert_eq!(yuv420.row_bytes, [80, 40, 40, 0]);

        let yuv410 = query_yuva_info(JColorSpace::YCbCr, [4, 1, 1], [2, 1, 1], [8, 2, 2], false, false);
        assert!(yuv410.supported);
        assert_eq!(yuv410.subsampling, Subsampling::S410 as i32);

        let yuv_bad = query_yuva_info(JColorSpace::YCbCr, [2, 2, 1], [2, 1, 1], [8, 4, 4], false, false);
        assert!(!yuv_bad.supported);

        let planes = plan_yuva_planes(35, 2, 18);
        assert_eq!(planes.num_y_rows_per_block, 16);
        assert_eq!(planes.num_iters, 2);
        assert_eq!(planes.remaining_rows, 3);
        assert_eq!(planes.remaining_uv_rows, 2);

        // Storage, subset crop, CMYK swizzler, and memory budget
        assert!(needs_swizzler_to_convert_from_cmyk(JColorSpace::Cmyk, false, DataSpace::Rgb, false));
        assert!(!needs_swizzler_to_convert_from_cmyk(JColorSpace::Cmyk, true, DataSpace::Cmyk, true));
        assert!(needs_swizzler_to_convert_from_cmyk(JColorSpace::Cmyk, true, DataSpace::Cmyk, false));
        assert!(!needs_swizzler_to_convert_from_cmyk(JColorSpace::ExtRgba, false, DataSpace::Rgb, false));

        let st = plan_storage(100, 2, true, 50, 200, true);
        assert_eq!(st.swizzle_bytes, 200);
        assert_eq!(st.xform_bytes, 200);
        assert_eq!(st.total_bytes, 400);

        let crop = plan_subset_after_crop(10, 30, 20, 8, 32);
        assert!(crop.needs_swizzler_for_subset);
        assert_eq!((crop.swizzler_subset_x, crop.swizzler_subset_y, crop.swizzler_subset_w, crop.swizzler_subset_h), (2, 0, 30, 20));
        assert!(!plan_subset_after_crop(8, 32, 20, 8, 32).needs_swizzler_for_subset);

        assert_eq!(estimate_libjpeg_memory(true, 100, 50), 30000);
        assert_eq!(estimate_libjpeg_memory(false, 100, 50), 3400);
        assert!(should_abort_progress(100));
        assert!(!should_abort_progress(99));
    }

    #[test]
    fn metadata_mpf_skip_count_and_container_offset() {
        let mut mp = MultiPictureParameters::with_image_count(2);
        mp.images[0] = MpImage { size: 200, data_offset: 0 };
        mp.images[1] = MpImage { size: 100, data_offset: 180 };
        let valid_mpf = mp.serialize(0);

        // First APP2 is ICC (skipped), second APP2 is valid MPF
        let markers = [
            JpegMarkerRef { marker: MPF_MARKER, data: b"ICC_PROFILE\0\x01\x01abc" },
            JpegMarkerRef { marker: MPF_MARKER, data: &valid_mpf },
        ];
        assert!(might_have_gainmap_image(&markers));
        let (parsed, skipped) = find_mp_params_in_markers(&markers).unwrap();
        assert_eq!(skipped, 1);
        assert_eq!(parsed, mp);

        let segs = [
            JpegSegment { offset: 0, marker: MARKER_START_OF_IMAGE, parameter_length: 0 },
            JpegSegment { offset: 2, marker: MPF_MARKER, parameter_length: 19 },
            JpegSegment { offset: 23, marker: MPF_MARKER, parameter_length: 90 },
            JpegSegment { offset: 500, marker: MARKER_END_OF_IMAGE, parameter_length: 0 },
        ];
        let matched = find_mp_segment_by_skip_count(&segs, skipped).unwrap();
        assert_eq!(matched.offset, 23);
        assert_eq!(compute_container_gainmap_offset(&segs, 10), 512);

        // Skip plans
        let ms = plan_memory_skip(40, 100);
        assert!(ms.ok);
        assert_eq!((ms.advance, ms.new_bytes_in_buffer), (40, 60));
        assert!(!plan_memory_skip(101, 100).ok);

        let bs1 = plan_buffered_skip(30, 100);
        assert!(!bs1.reset_buffer_ptr);
        assert_eq!((bs1.advance_in_buffer, bs1.new_bytes_in_buffer, bs1.stream_bytes_to_skip), (30, 70, 0));
        let bs2 = plan_buffered_skip(130, 100);
        assert!(bs2.reset_buffer_ptr);
        assert_eq!((bs2.advance_in_buffer, bs2.new_bytes_in_buffer, bs2.stream_bytes_to_skip), (0, 0, 30));
    }

    #[test]
    fn encoder_plans_yuva_pack_and_gainmap_mpf() {
        // RGB encode plans
        let p_rgba = plan_rgb_encode(64, ColorType::Rgba8888, AlphaType::Unpremul, AlphaOption::Ignore, Downsample::S422);
        assert!(p_rgba.valid);
        assert_eq!(p_rgba.jpeg_color_space, JColorSpace::ExtRgba as i32);
        assert!(!p_rgba.use_color_xform);
        assert_eq!((p_rgba.y_h_samp, p_rgba.y_v_samp), (2, 1));
        assert_eq!(p_rgba.storage_bytes, 0);

        let p_premul = plan_rgb_encode(64, ColorType::Rgba8888, AlphaType::Unpremul, AlphaOption::BlendOnBlack, Downsample::S444);
        assert!(p_premul.valid);
        assert!(p_premul.use_color_xform);
        assert_eq!(p_premul.xform_dst_at, AlphaType::Premul as i32);
        assert_eq!((p_premul.y_h_samp, p_premul.y_v_samp), (1, 1));
        assert_eq!(p_premul.storage_bytes, 256);

        let p_a8 = plan_rgb_encode(32, ColorType::Alpha8, AlphaType::Premul, AlphaOption::Ignore, Downsample::S420);
        assert!(p_a8.valid);
        assert_eq!(p_a8.jpeg_color_space, JColorSpace::Grayscale as i32);
        assert_eq!(p_a8.num_components, 1);

        let p_a16 = plan_rgb_encode(32, ColorType::A16Unorm, AlphaType::Premul, AlphaOption::Ignore, Downsample::S420);
        assert!(!p_a16.valid);

        let yuv_ok = plan_yuv_encode(128, YUV_COLOR_SPACE_JPEG_FULL, YUVA_DATA_TYPE_UNORM8, PlaneConfig::Y_U_V, Subsampling::S420);
        assert!(yuv_ok.valid);
        assert_eq!((yuv_ok.y_h_samp, yuv_ok.y_v_samp), (2, 2));
        assert_eq!(yuv_ok.storage_bytes, 384);
        assert!(!plan_yuv_encode(128, 1, YUVA_DATA_TYPE_UNORM8, PlaneConfig::Y_U_V, Subsampling::S420).valid);
        assert!(!plan_yuv_encode(128, YUV_COLOR_SPACE_JPEG_FULL, 1, PlaneConfig::Y_U_V, Subsampling::S420).valid);
        assert!(!plan_yuv_encode(128, YUV_COLOR_SPACE_JPEG_FULL, YUVA_DATA_TYPE_UNORM8, PlaneConfig::Y_V_U, Subsampling::S420).valid);

        // YUV row packing
        let y = [10u8, 20, 30, 40];
        let u = [100u8, 110];
        let v = [200u8, 210];
        let mut dst = [0u8; 12];
        yuva_copy_row_planar(&y, &u, &v, 2, 2, &mut dst);
        assert_eq!(dst, [10, 100, 200, 20, 100, 200, 30, 110, 210, 40, 110, 210]);

        let uv = [100u8, 200, 110, 210];
        let mut dst2 = [0u8; 12];
        yuva_copy_row_semi_planar(&y, &uv, 2, &mut dst2);
        assert_eq!(dst2, dst);

        // Origin EXIF segment roundtrip
        let exif_seg = build_origin_exif_segment_params(6).unwrap();
        assert!(exif_seg.starts_with(EXIF_SIG));
        assert_eq!(exif_seg[5], 0);
        assert_eq!(skia_rs_codec_meta::exif::parse_encoded_origin(&exif_seg[6..]), Some(6));
        assert!(build_origin_exif_segment_params(0).is_none());
        assert!(build_origin_exif_segment_params(9).is_none());

        // Gainmap XMP (single channel + multi-channel)
        let mut gm = GainmapInfo::default();
        gm.gainmap_ratio_min = [1.0, 1.0, 1.0, 1.0];
        gm.gainmap_ratio_max = [4.0, 4.0, 4.0, 1.0];
        gm.gainmap_gamma = [2.0, 1.0, 0.5, 1.0];
        gm.display_ratio_sdr = 1.0;
        gm.display_ratio_hdr = 4.0;
        gm.base_image_type = BaseImageType::Hdr;
        let gm_xmp = String::from_utf8(build_gainmap_image_xmp_metadata(&gm)).unwrap();
        assert!(gm_xmp.contains("hdrgm:GainMapMin=\"0\""));
        assert!(gm_xmp.contains("hdrgm:GainMapMax=\"2\""));
        assert!(gm_xmp.contains("hdrgm:BaseRenditionIsHDR=\"True\""));
        assert!(gm_xmp.contains("<hdrgm:Gamma>"));
        assert!(gm_xmp.contains("<rdf:li>0.5</rdf:li>"));
        assert!(gm_xmp.contains("<rdf:li>2</rdf:li>"));

        let base_xmp = String::from_utf8(build_base_image_xmp_metadata(12345)).unwrap();
        assert!(base_xmp.contains("Item:Length=\"12345\""));

        // Assemble MPF with 2 minimal JPEGs and verify offsets & parsing!
        let img0 = make_minimal_jpeg(&[(MARKER_APP0 + 1, &build_minimal_exif_params())]);
        let img1 = make_minimal_jpeg(&[(MARKER_APP0 + 1, &build_minimal_exif_params())]);
        let mpf_blob = assemble_mpf(&[&img0, &img1]).unwrap();

        let mut scan = SegmentScanner::new_to_eoi();
        scan.on_bytes(&mpf_blob);
        assert!(scan.is_done());
        let mpf_seg = scan.segments().iter().find(|s| s.marker == MPF_MARKER).unwrap();
        let (poff, plen) = segment_parameter_range(mpf_blob.len(), *mpf_seg).unwrap();
        let parsed = MultiPictureParameters::parse(&mpf_blob[poff..poff + plen]).unwrap();
        assert_eq!(parsed.images.len(), 2);
        assert_eq!(parsed.images[0].data_offset, 0);
        let img1_abs = get_image_absolute_offset(parsed.images[1].data_offset, mpf_seg.offset);
        assert_eq!(img1_abs, parsed.images[0].size as usize);
        assert_eq!(img1_abs + (parsed.images[1].size as usize), mpf_blob.len());
        // Verify secondary image starts with SOI and contains its own MPF APP2!
        assert_eq!(&mpf_blob[img1_abs..img1_abs + 3], &JPEG_SIG);
    }
}
