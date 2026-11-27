#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's BMP, WBMP, and ICO image decoders (`P2.3`).

pub mod bmp_header;
pub mod bmp_mask;
pub mod bmp_rle;
pub mod bmp_standard;
pub mod ico;
pub mod stream;
pub mod wbmp;

#[cfg(test)]
mod tests {
    use skia_rs_codec_core::types::{CodecResult, EncodedAlpha, EncodedColor, ScanlineOrder};
    use skia_rs_color::{
        image_info::{AlphaType, ColorType},
        masks::InputMasks,
    };
    use skia_rs_geometry::ISize;

    use crate::{
        bmp_header::{
            bmp_and_mask_row_bytes, bmp_dst_row, bmp_src_row_bytes, compression, get_header_type,
            is_bmp, read_bmp_header, BmpHeaderType, BmpInputFormat, BMP_HEADER_BYTES,
            BMP_INFO_V1_BYTES, BMP_INFO_V2_BYTES, BMP_INFO_V3_BYTES, BMP_INFO_V4_BYTES,
            BMP_INFO_V5_BYTES, BMP_OS2_V1_BYTES, BMP_OS2_V2_BYTES,
        },
        bmp_mask::{prepare_mask_swizzler_info, validate_mask_get_pixels, MaskSwizzlerTargetInfo},
        bmp_rle::{
            compute_rle_f16_xform_layout, consume_rle_lines_to_skip, pack_888_to_rgb16,
            pixel32_to_pixel16, BmpRleDecoder, RLE_BUFFER_SIZE,
        },
        bmp_standard::{
            build_standard_color_table, compute_ico_and_mask_substream, decode_ico_mask,
            standard_swizzler_info, validate_standard_get_pixels, StandardSwizzlerEncodedInfo,
        },
        ico::{
            ico_choose_codec, ico_get_scaled_dimensions, ico_select_largest_codec, is_ico,
            is_png_payload, parse_ico_directory, IcoEntry,
        },
        stream::{ByteReader, SliceReader},
        wbmp::{
            is_wbmp, read_mbf, read_wbmp_header, wbmp_conversion_supported, wbmp_src_row_bytes,
        },
    };

    /// Chunked reader to exercise `check_for_more_data` streaming refills.
    struct ChunkedReader<'a> {
        data: &'a [u8],
        pos: usize,
        max_chunk: usize,
    }

    impl<'a> ChunkedReader<'a> {
        fn new(data: &'a [u8], max_chunk: usize) -> Self {
            Self {
                data,
                pos: 0,
                max_chunk,
            }
        }
    }

    impl ByteReader for ChunkedReader<'_> {
        fn read(&mut self, buf: &mut [u8]) -> usize {
            let avail = self.data.len() - self.pos;
            let n = buf.len().min(avail).min(self.max_chunk);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            n
        }

        fn skip(&mut self, count: usize) -> usize {
            let avail = self.data.len() - self.pos;
            let n = count.min(avail);
            self.pos += n;
            n
        }
    }

    fn make_file_header(total_bytes: u32, offset: u32) -> [u8; 14] {
        let mut h = [0u8; 14];
        h[0] = b'B';
        h[1] = b'M';
        h[2..6].copy_from_slice(&total_bytes.to_le_bytes());
        h[10..14].copy_from_slice(&offset.to_le_bytes());
        h
    }

    fn make_info_header(
        size: u32,
        width: i32,
        height: i32,
        bpp: u16,
        comp: u32,
        num_colors: u32,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; size as usize];
        buf[0..4].copy_from_slice(&size.to_le_bytes());
        buf[4..8].copy_from_slice(&width.to_le_bytes());
        buf[8..12].copy_from_slice(&height.to_le_bytes());
        buf[12..14].copy_from_slice(&1u16.to_le_bytes()); // planes
        buf[14..16].copy_from_slice(&bpp.to_le_bytes());
        if size >= 20 {
            buf[16..20].copy_from_slice(&comp.to_le_bytes());
        }
        if size >= 36 {
            buf[32..36].copy_from_slice(&num_colors.to_le_bytes());
        }
        buf
    }

    #[test]
    fn bmp_signature_and_header_types() {
        assert!(is_bmp(b"BM\x00\x00"));
        assert!(!is_bmp(b"B"));
        assert!(!is_bmp(b"BA\x00\x00"));
        assert!(!is_bmp(b""));

        assert_eq!(get_header_type(11), BmpHeaderType::Unknown);
        assert_eq!(get_header_type(BMP_OS2_V1_BYTES), BmpHeaderType::Os2V1);
        assert_eq!(get_header_type(15), BmpHeaderType::Os2V1);
        assert_eq!(get_header_type(BMP_INFO_V1_BYTES), BmpHeaderType::InfoV1);
        assert_eq!(get_header_type(BMP_INFO_V2_BYTES), BmpHeaderType::InfoV2);
        assert_eq!(get_header_type(BMP_INFO_V3_BYTES), BmpHeaderType::InfoV3);
        assert_eq!(get_header_type(BMP_INFO_V4_BYTES), BmpHeaderType::InfoV4);
        assert_eq!(get_header_type(BMP_INFO_V5_BYTES), BmpHeaderType::InfoV5);
        assert_eq!(get_header_type(BMP_OS2_V2_BYTES), BmpHeaderType::Os2Vx);
        assert_eq!(get_header_type(16), BmpHeaderType::Os2Vx);
        assert_eq!(get_header_type(44), BmpHeaderType::Unknown);

        assert_eq!(bmp_src_row_bytes(1, 1), 4);
        assert_eq!(bmp_src_row_bytes(33, 1), 8);
        assert_eq!(bmp_src_row_bytes(3, 24), 12); // 9 aligned to 12
        assert_eq!(bmp_src_row_bytes(2, 32), 8);
        assert_eq!(bmp_and_mask_row_bytes(16), 4);
        assert_eq!(bmp_and_mask_row_bytes(33), 8);

        assert_eq!(bmp_dst_row(ScanlineOrder::TopDown, 2, 10), 2);
        assert_eq!(bmp_dst_row(ScanlineOrder::BottomUp, 2, 10), 7);
    }

    #[test]
    fn bmp_parse_os2_v1_and_info_v1_through_v5() {
        // OS/2 v1 (12-byte DIB header), 8-bit palette
        let mut os2 = Vec::new();
        os2.extend_from_slice(&make_file_header(200, 14 + 12 + 3 * 4));
        os2.extend_from_slice(&BMP_OS2_V1_BYTES.to_le_bytes());
        os2.extend_from_slice(&16u16.to_le_bytes()); // width
        os2.extend_from_slice(&8u16.to_le_bytes()); // height
        os2.extend_from_slice(&1u16.to_le_bytes()); // planes
        os2.extend_from_slice(&8u16.to_le_bytes()); // bpp
        let mut r = SliceReader::new(&os2);
        let h = read_bmp_header(&mut r, false, true).unwrap();
        assert_eq!(h.input_format, BmpInputFormat::Standard);
        assert_eq!(h.width, 16);
        assert_eq!(h.height, 8);
        assert_eq!(h.bits_per_pixel, 8);
        assert_eq!(h.bytes_per_color, 3);
        assert_eq!(h.encoded_color, EncodedColor::Palette);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Opaque);
        assert_eq!(h.row_order, ScanlineOrder::BottomUp);
        assert_eq!(h.pixel_data_offset, 12);

        // Info V1, top-down (negative height), 24-bit BGR
        let mut v1 = Vec::new();
        v1.extend_from_slice(&make_file_header(100, BMP_HEADER_BYTES + BMP_INFO_V1_BYTES));
        v1.extend_from_slice(&make_info_header(BMP_INFO_V1_BYTES, 10, -6, 24, compression::NONE, 0));
        let mut r = SliceReader::new(&v1);
        let h = read_bmp_header(&mut r, false, true).unwrap();
        assert_eq!(h.width, 10);
        assert_eq!(h.height, 6);
        assert_eq!(h.row_order, ScanlineOrder::TopDown);
        assert_eq!(h.encoded_color, EncodedColor::Bgr);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Opaque);
        assert_eq!(h.pixel_data_offset, 0);

        // Info V1, 16-bit NONE -> implicit 555 BitMask
        let mut v1_16 = Vec::new();
        v1_16.extend_from_slice(&make_file_header(100, BMP_HEADER_BYTES + BMP_INFO_V1_BYTES));
        v1_16.extend_from_slice(&make_info_header(BMP_INFO_V1_BYTES, 4, 4, 16, compression::NONE, 0));
        let mut r = SliceReader::new(&v1_16);
        let h = read_bmp_header(&mut r, false, true).unwrap();
        assert_eq!(h.input_format, BmpInputFormat::BitMask);
        assert_eq!(
            h.input_masks,
            InputMasks {
                red: 0x7C00,
                green: 0x03E0,
                blue: 0x001F,
                alpha: 0
            }
        );
        assert_eq!(h.encoded_color, EncodedColor::Bgr);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Opaque);

        // Info V3 (56-byte) with BitMasks: alpha mask ignored when !in_ico
        let mut v3 = Vec::new();
        v3.extend_from_slice(&make_file_header(200, BMP_HEADER_BYTES + BMP_INFO_V3_BYTES));
        let mut dib3 = make_info_header(BMP_INFO_V3_BYTES, 4, 4, 32, compression::BIT_MASKS, 0);
        dib3[40..44].copy_from_slice(&0x00FF_0000u32.to_le_bytes()); // R
        dib3[44..48].copy_from_slice(&0x0000_FF00u32.to_le_bytes()); // G
        dib3[48..52].copy_from_slice(&0x0000_00FFu32.to_le_bytes()); // B
        dib3[52..56].copy_from_slice(&0xFF00_0000u32.to_le_bytes()); // A (ignored for V3 non-ico)
        v3.extend_from_slice(&dib3);
        let mut r = SliceReader::new(&v3);
        let h = read_bmp_header(&mut r, false, true).unwrap();
        assert_eq!(h.input_format, BmpInputFormat::BitMask);
        assert_eq!(h.input_masks.alpha, 0);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Opaque);

        // Info V4 (108-byte) & V5 (124-byte) with alpha mask -> Bgra + Unpremul
        for &hdr_size in &[BMP_INFO_V4_BYTES, BMP_INFO_V5_BYTES] {
            let mut buf = Vec::new();
            buf.extend_from_slice(&make_file_header(300, BMP_HEADER_BYTES + hdr_size + 4));
            let mut dib = make_info_header(hdr_size, 8, 8, 32, compression::BIT_MASKS, 0);
            dib[40..44].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
            dib[44..48].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
            dib[48..52].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
            dib[52..56].copy_from_slice(&0xFF00_0000u32.to_le_bytes());
            buf.extend_from_slice(&dib);
            buf.extend_from_slice(&[0u8; 4]); // padding skipped to pixel offset
            let mut r = SliceReader::new(&buf);
            let h = read_bmp_header(&mut r, false, true).unwrap();
            assert_eq!(h.input_format, BmpInputFormat::BitMask);
            assert_eq!(h.input_masks.alpha, 0xFF00_0000);
            assert_eq!(h.encoded_color, EncodedColor::Bgra);
            assert_eq!(h.encoded_alpha, EncodedAlpha::Unpremul);
            assert_eq!(r.remaining(), 0);
        }

        // Info V1 with trailing 12-byte bit masks, overlapping masks rejected
        let mut v1_bad_mask = Vec::new();
        v1_bad_mask.extend_from_slice(&make_file_header(
            100,
            BMP_HEADER_BYTES + BMP_INFO_V1_BYTES + 12,
        ));
        v1_bad_mask.extend_from_slice(&make_info_header(
            BMP_INFO_V1_BYTES,
            4,
            4,
            16,
            compression::BIT_MASKS,
            0,
        ));
        v1_bad_mask.extend_from_slice(&0xFF00u32.to_le_bytes());
        v1_bad_mask.extend_from_slice(&0x0FF0u32.to_le_bytes()); // overlaps red
        v1_bad_mask.extend_from_slice(&0x000Fu32.to_le_bytes());
        let mut r = SliceReader::new(&v1_bad_mask);
        assert_eq!(
            read_bmp_header(&mut r, false, true),
            Err(CodecResult::InvalidInput)
        );
    }

    #[test]
    fn bmp_header_ico_and_error_cases() {
        // BMP-in-ICO 32-bit: height halved, Unpremul alpha, is_opaque = false
        let dib32 = make_info_header(BMP_INFO_V1_BYTES, 32, 64, 32, compression::NONE, 0);
        let mut r = SliceReader::new(&dib32);
        let h = read_bmp_header(&mut r, true, true).unwrap();
        assert_eq!(h.width, 32);
        assert_eq!(h.height, 32);
        assert_eq!(h.encoded_color, EncodedColor::Bgra);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Unpremul);
        assert!(!h.is_opaque);

        // BMP-in-ICO 24-bit: Binary alpha, is_opaque = true
        let dib24 = make_info_header(BMP_INFO_V1_BYTES, 16, 32, 24, compression::NONE, 0);
        let mut r = SliceReader::new(&dib24);
        let h = read_bmp_header(&mut r, true, true).unwrap();
        assert_eq!(h.height, 16);
        assert_eq!(h.encoded_color, EncodedColor::Bgra);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Binary);
        assert!(h.is_opaque);

        // BMP-in-ICO rejects bitmask format
        let dib16 = make_info_header(BMP_INFO_V1_BYTES, 16, 32, 16, compression::NONE, 0);
        let mut r = SliceReader::new(&dib16);
        assert_eq!(
            read_bmp_header(&mut r, true, true),
            Err(CodecResult::InvalidInput)
        );

        // INT32_MIN height rejected
        let mut bad_h = Vec::new();
        bad_h.extend_from_slice(&make_file_header(100, 54));
        bad_h.extend_from_slice(&make_info_header(BMP_INFO_V1_BYTES, 4, i32::MIN, 24, 0, 0));
        assert_eq!(
            read_bmp_header(&mut SliceReader::new(&bad_h), false, true),
            Err(CodecResult::InvalidInput)
        );

        // RLE8 with mismatched bpp corrected to 8, requires total_bytes > offset
        let mut rle_ok = Vec::new();
        rle_ok.extend_from_slice(&make_file_header(100, 54));
        rle_ok.extend_from_slice(&make_info_header(
            BMP_INFO_V1_BYTES,
            10,
            10,
            4, // wrong bpp, corrected to 8
            compression::RLE_8BIT,
            2,
        ));
        let h = read_bmp_header(&mut SliceReader::new(&rle_ok), false, true).unwrap();
        assert_eq!(h.input_format, BmpInputFormat::Rle);
        assert_eq!(h.bits_per_pixel, 8);
        assert_eq!(h.encoded_color, EncodedColor::Bgra);
        assert_eq!(h.encoded_alpha, EncodedAlpha::Binary);

        // RLE with total_bytes <= offset fails
        let mut rle_bad = Vec::new();
        rle_bad.extend_from_slice(&make_file_header(54, 54));
        rle_bad.extend_from_slice(&make_info_header(
            BMP_INFO_V1_BYTES,
            10,
            10,
            8,
            compression::RLE_8BIT,
            0,
        ));
        assert_eq!(
            read_bmp_header(&mut SliceReader::new(&rle_bad), false, true),
            Err(CodecResult::InvalidInput)
        );

        // JPEG with 24 bpp -> RLE24; JPEG with 8 bpp -> Unimplemented; PNG/CMYK -> Unimplemented
        let mut rle24 = Vec::new();
        rle24.extend_from_slice(&make_file_header(200, 54));
        rle24.extend_from_slice(&make_info_header(
            BMP_INFO_V1_BYTES,
            4,
            4,
            24,
            compression::JPEG,
            0,
        ));
        let h24 = read_bmp_header(&mut SliceReader::new(&rle24), false, true).unwrap();
        assert_eq!(h24.input_format, BmpInputFormat::Rle);
        assert_eq!(h24.bits_per_pixel, 24);

        let mut png_bmp = Vec::new();
        png_bmp.extend_from_slice(&make_file_header(200, 54));
        png_bmp.extend_from_slice(&make_info_header(
            BMP_INFO_V1_BYTES,
            4,
            4,
            24,
            compression::PNG,
            0,
        ));
        assert_eq!(
            read_bmp_header(&mut SliceReader::new(&png_bmp), false, true),
            Err(CodecResult::Unimplemented)
        );

        // Truncated file header -> IncompleteInput
        assert_eq!(
            read_bmp_header(&mut SliceReader::new(b"BM\x00\x00"), false, true),
            Err(CodecResult::IncompleteInput)
        );
    }

    #[test]
    fn bmp_standard_palette_and_ico_and_mask() {
        // 2-bit palette (maxColors=4), numColors=2, 4 bytes/color (B,G,R,A), plus 4 extra offset bytes
        let stream_data = [
            0x10u8, 0x20, 0x30, 0x80, // color 0: B=0x10, G=0x20, R=0x30
            0xAA, 0xBB, 0xCC, 0x40, // color 1: B=0xAA, G=0xBB, R=0xCC
            0xDE, 0xAD, 0xBE, 0xEF, // padding before pixel array
        ];
        let mut table = [0u32; 256];
        let mut r = SliceReader::new(&stream_data);
        let count = build_standard_color_table(
            2,
            2,
            4,
            12,
            true,
            false,
            ColorType::Rgba8888,
            AlphaType::Opaque,
            false,
            false,
            &mut r,
            &mut table,
        )
        .unwrap();
        assert_eq!(count, 4);
        assert_eq!(r.remaining(), 0);
        assert_eq!(table[0], 0xFF_10_20_30);
        assert_eq!(table[1], 0xFF_AA_BB_CC);
        assert_eq!(table[2], 0xFF_00_00_00);
        assert_eq!(table[3], 0xFF_00_00_00);

        // Swizzler info for ICO vs standard
        assert_eq!(
            standard_swizzler_info(true, 4, EncodedColor::Bgra, EncodedAlpha::Binary, 8),
            StandardSwizzlerEncodedInfo {
                color: EncodedColor::Palette,
                alpha: EncodedAlpha::Binary,
                bits_per_component: 4,
            }
        );
        assert_eq!(
            standard_swizzler_info(true, 24, EncodedColor::Bgra, EncodedAlpha::Binary, 8),
            StandardSwizzlerEncodedInfo {
                color: EncodedColor::Bgr,
                alpha: EncodedAlpha::Opaque,
                bits_per_component: 8,
            }
        );

        // ICO AND-mask substream math
        assert_eq!(
            compute_ico_and_mask_substream(4, 1, 2, 16, 4, 100, 200),
            Some((100 + 16 + 4, 80))
        );
        assert_eq!(
            compute_ico_and_mask_substream(4, 1, 2, 16, 4, 190, 200),
            None
        );

        // Decode ICO 1-bit AND mask on 4x2 bottom-up RGBA8888 image:
        // Row 0 in stream (maps to dst row 1): bits 1010_0000 -> pixels 0 and 2 transparent
        // Row 1 in stream (maps to dst row 0): bits 0101_0000 -> pixels 1 and 3 transparent
        let mask_stream = [
            0b1010_0000u8, 0, 0, 0,
            0b0101_0000u8, 0, 0, 0,
        ];
        let mut dst = vec![0xFFu8; 4 * 4 * 2];
        let mut mask_buf = [0u8; 16];
        decode_ico_mask(
            &mut SliceReader::new(&mask_stream),
            &mut mask_buf,
            4,
            &mut dst,
            16,
            2,
            ColorType::Rgba8888,
            ScanlineOrder::BottomUp,
            1,
            4,
            0,
        );
        let px = |row: usize, col: usize| -> u32 {
            let off = row * 16 + col * 4;
            u32::from_ne_bytes(dst[off..off + 4].try_into().unwrap())
        };
        // dst row 0 came from mask row 1 (0101)
        assert_eq!(px(0, 0), 0xFFFF_FFFF);
        assert_eq!(px(0, 1), 0);
        assert_eq!(px(0, 2), 0xFFFF_FFFF);
        assert_eq!(px(0, 3), 0);
        // dst row 1 came from mask row 0 (1010)
        assert_eq!(px(1, 0), 0);
        assert_eq!(px(1, 1), 0xFFFF_FFFF);
        assert_eq!(px(1, 2), 0);
        assert_eq!(px(1, 3), 0xFFFF_FFFF);

        // F16 AND-mask with sampleX=2, srcOffset=0 (tests srcX = 1, 3)
        let f16_mask = [0b0100_0000u8, 0, 0, 0]; // bit 1 = 1, bit 3 = 0
        let mut f16_dst = vec![0xABu8; 2 * 8];
        decode_ico_mask(
            &mut SliceReader::new(&f16_mask),
            &mut mask_buf,
            4,
            &mut f16_dst,
            16,
            1,
            ColorType::RgbaF16,
            ScanlineOrder::TopDown,
            2,
            2,
            0,
        );
        assert!(f16_dst[0..8].iter().all(|&b| b == 0));
        assert!(f16_dst[8..16].iter().all(|&b| b == 0xAB));

        // Validation helpers
        assert_eq!(
            validate_standard_get_pixels(true, ISize { width: 4, height: 4 }, ISize { width: 4, height: 4 }),
            CodecResult::Unimplemented
        );
        assert_eq!(
            validate_standard_get_pixels(false, ISize { width: 2, height: 4 }, ISize { width: 4, height: 4 }),
            CodecResult::InvalidScale
        );
        assert_eq!(
            validate_mask_get_pixels(false, ISize { width: 4, height: 4 }, ISize { width: 4, height: 4 }),
            CodecResult::Success
        );
        assert_eq!(
            prepare_mask_swizzler_info(ColorType::RgbaF16, AlphaType::Premul, true),
            MaskSwizzlerTargetInfo {
                color_type: ColorType::Bgra8888,
                alpha_type: AlphaType::Unpremul,
            }
        );
    }

    #[test]
    fn bmp_rle8_encoded_absolute_delta_and_scanline_skip() {
        // 4x4 top-down RLE8 with 3-entry palette:
        // 0: red (0,0,255), 1: green (0,255,0), 2: blue (255,0,0)
        let mut palette = Vec::new();
        palette.extend_from_slice(&[0, 0, 255, 0]); // idx 0: R=255
        palette.extend_from_slice(&[0, 255, 0, 0]); // idx 1: G=255
        palette.extend_from_slice(&[255, 0, 0, 0]); // idx 2: B=255

        // Row 0: encoded run of 2 red at (0,0),(1,0), then delta(+0, +2) -> (x=2, y=2)
        // Row 2: wait: let's test delta(+1, +2) -> (x=3, y=2) on a 6x4 image so we can also test
        // an odd-length absolute run of 3 pixels!
        let rle_stream: Vec<u8> = vec![
            2, 0, // 2 pixels of idx 0 at (0,0), (1,0)
            0, 2, 1, 2, // delta dx=1, dy=2 -> x=3, y=2
            0, 3, 2, 1, 0, 0, // absolute 3 pixels (B,G,R) at (3,2),(4,2),(5,2) + 1 pad byte
            0, 0, // EOL -> x=0, y=3
            6, 1, // 6 pixels of idx 1 on row 3
            0, 1, // EOF
        ];

        let mut full_data = palette.clone();
        full_data.extend_from_slice(&rle_stream);

        // Full decode with normal SliceReader
        let mut reader = SliceReader::new(&full_data);
        let mut dec = BmpRleDecoder::new(6, 8, 3, 4, 12, ScanlineOrder::TopDown);
        assert_eq!(
            dec.prepare(false, ColorType::Rgba8888, false, false, &mut reader),
            CodecResult::Success
        );
        let mut dst = vec![0u8; 6 * 4 * 4];
        let rows = dec.decode_rows(&mut reader, Some(&mut dst), 4, 24, ColorType::Rgba8888, false);
        assert_eq!(rows, 4);

        let px = |buf: &[u8], x: usize, y: usize| -> u32 {
            let off = y * 24 + x * 4;
            u32::from_ne_bytes(buf[off..off + 4].try_into().unwrap())
        };
        let red = 0xFF_00_00_FF;
        let green = 0xFF_00_FF_00;
        let blue = 0xFF_FF_00_00;
        assert_eq!(px(&dst, 0, 0), red);
        assert_eq!(px(&dst, 1, 0), red);
        assert_eq!(px(&dst, 2, 0), 0);
        for x in 0..6 {
            assert_eq!(px(&dst, x, 1), 0);
        }
        assert_eq!(px(&dst, 2, 2), 0);
        assert_eq!(px(&dst, 3, 2), blue);
        assert_eq!(px(&dst, 4, 2), green);
        assert_eq!(px(&dst, 5, 2), red);
        for x in 0..6 {
            assert_eq!(px(&dst, x, 3), green);
        }

        // Same RLE stream fed via 8-byte chunks to exercise `check_for_more_data` compaction & refill
        let mut pal_reader = SliceReader::new(&palette);
        let mut chunk_reader = ChunkedReader::new(&rle_stream, 8);
        let mut dec_c = BmpRleDecoder::new(6, 8, 3, 4, 12, ScanlineOrder::TopDown);
        let count = crate::bmp_rle::build_rle_color_table(
            8,
            3,
            4,
            12,
            ColorType::Rgba8888,
            false,
            false,
            &mut pal_reader,
            &mut dec_c.color_table,
        )
        .unwrap();
        dec_c.color_count = count as usize;
        {
            let mut cursor = crate::bmp_rle::BmpRleCursor {
                stream_buffer: &mut dec_c.stream_buffer,
                bytes_buffered: &mut dec_c.bytes_buffered,
                curr_rle_byte: &mut dec_c.curr_rle_byte,
                lines_to_skip: &mut dec_c.lines_to_skip,
            };
            assert!(cursor.initialize_stream_buffer(&mut chunk_reader));
        }
        let mut dst_c = vec![0u8; 6 * 4 * 4];
        assert_eq!(
            dec_c.decode_rows(&mut chunk_reader, Some(&mut dst_c), 4, 24, ColorType::Rgba8888, false),
            4
        );
        assert_eq!(dst_c, dst);

        // Scanline-by-scanline decode with delta jumping across requested height (`fLinesToSkip`)
        let mut reader2 = SliceReader::new(&full_data);
        let mut dec2 = BmpRleDecoder::new(6, 8, 3, 4, 12, ScanlineOrder::TopDown);
        assert_eq!(
            dec2.prepare(false, ColorType::Rgba8888, false, false, &mut reader2),
            CodecResult::Success
        );
        let mut row0 = [0u8; 24];
        assert_eq!(
            dec2.decode_rows(&mut reader2, Some(&mut row0), 1, 24, ColorType::Rgba8888, false),
            1
        );
        assert_eq!(dec2.lines_to_skip, 1); // jumped from y=0 to y=2 with height=1 -> 1 line to skip
        let mut row1 = [0u8; 24];
        assert_eq!(
            dec2.decode_rows(&mut reader2, Some(&mut row1), 1, 24, ColorType::Rgba8888, false),
            1
        );
        assert_eq!(dec2.lines_to_skip, 0);
        assert!(row1.iter().all(|&b| b == 0));

        // Out-of-bounds delta x > width returns y - dy
        let mut bad_delta = palette;
        bad_delta.extend_from_slice(&[
            0, 0, // EOL -> y=1
            0, 2, 10, 1, // dx=10 > width=6, dy=1 -> returns y - dy = 1
        ]);
        let mut r_bad = SliceReader::new(&bad_delta);
        let mut dec_bad = BmpRleDecoder::new(6, 8, 3, 4, 12, ScanlineOrder::TopDown);
        assert_eq!(
            dec_bad.prepare(false, ColorType::Rgba8888, false, false, &mut r_bad),
            CodecResult::Success
        );
        let mut dst_bad = vec![0u8; 6 * 4 * 4];
        assert_eq!(
            dec_bad.decode_rows(&mut r_bad, Some(&mut dst_bad), 4, 24, ColorType::Rgba8888, false),
            1
        );
    }

    #[test]
    fn bmp_rle4_and_rle24_and_incomplete_inputs() {
        // RLE4: 4x2 top-down, palette [0]=R, [1]=G, [2]=B, [3]=White
        let mut data = Vec::new();
        data.extend_from_slice(&[0, 0, 255, 0]);
        data.extend_from_slice(&[0, 255, 0, 0]);
        data.extend_from_slice(&[255, 0, 0, 0]);
        data.extend_from_slice(&[255, 255, 255, 0]);
        // Row 0: encoded 4 pixels alternating 0 and 1 (0x01), EOL
        // Row 1: absolute 3 pixels: 2, 3, 0 -> bytes 0x23, 0x00 (2 bytes, already even), EOF
        data.extend_from_slice(&[
            4, 0x01,
            0, 0,
            0, 3, 0x23, 0x00,
            0, 1,
        ]);

        let mut r = SliceReader::new(&data);
        let mut dec = BmpRleDecoder::new(4, 4, 4, 4, 16, ScanlineOrder::TopDown);
        assert_eq!(
            dec.prepare(false, ColorType::Rgb565, false, false, &mut r),
            CodecResult::Success
        );
        let mut dst = vec![0u8; 4 * 2 * 2];
        assert_eq!(
            dec.decode_rows(&mut r, Some(&mut dst), 2, 8, ColorType::Rgb565, false),
            2
        );
        let p16 = |x: usize, y: usize| -> u16 {
            let off = y * 8 + x * 2;
            u16::from_ne_bytes(dst[off..off + 2].try_into().unwrap())
        };
        assert_eq!(p16(0, 0), pack_888_to_rgb16(255, 0, 0));
        assert_eq!(p16(1, 0), pack_888_to_rgb16(0, 255, 0));
        assert_eq!(p16(2, 0), pack_888_to_rgb16(255, 0, 0));
        assert_eq!(p16(3, 0), pack_888_to_rgb16(0, 255, 0));
        assert_eq!(p16(0, 1), pack_888_to_rgb16(0, 0, 255));
        assert_eq!(p16(1, 1), pack_888_to_rgb16(255, 255, 255));
        assert_eq!(p16(2, 1), pack_888_to_rgb16(255, 0, 0));
        assert_eq!(p16(3, 1), 0);

        // RLE24: 2x2 bottom-up to BGRA8888, truncated before row 1 completes
        let rle24_data = [
            2u8, 0x10, 0x20, 0x30, // row 0: 2 pixels BGR=(0x10, 0x20, 0x30)
            0, 0, // EOL -> y=1
            0, 3, 0xAA, // truncated absolute run of 3 pixels (needs 9+1=10 bytes, only 1 available)
        ];
        let mut r24 = SliceReader::new(&rle24_data);
        let mut dec24 = BmpRleDecoder::new(2, 24, 0, 4, 0, ScanlineOrder::BottomUp);
        assert_eq!(
            dec24.prepare(false, ColorType::Bgra8888, false, true, &mut r24),
            CodecResult::Success
        );
        let mut dst24 = vec![0u8; 2 * 4 * 2];
        let rows24 = dec24.decode_rows(&mut r24, Some(&mut dst24), 2, 8, ColorType::Bgra8888, true);
        assert_eq!(rows24, 1);
        // Bottom-up y=0 writes to dst row 1
        let px0 = u32::from_ne_bytes(dst24[8..12].try_into().unwrap());
        assert_eq!(px0, 0xFF_30_20_10);

        // Helper sanity checks
        assert_eq!(pixel32_to_pixel16(0xFF_00_00_FF, false), 0xF800);
        assert_eq!(pixel32_to_pixel16(0xFF_FF_00_00, true), 0xF800);
        assert_eq!(compute_rle_f16_xform_layout(10, 20), Some((200, 800, 40)));
        assert_eq!(compute_rle_f16_xform_layout(i32::MAX, 2), None);

        let mut skip = 3;
        let p = consume_rle_lines_to_skip(2, &mut skip);
        assert!(p.fully_skipped);
        assert_eq!(p.remaining_height, 2);
        assert_eq!(skip, 1);

        // Streaming across > RLE_BUFFER_SIZE bytes
        let mut big = Vec::new();
        big.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00]); // 1 color (blue)
        let num_rows = 2500i32;
        for _ in 0..num_rows {
            big.extend_from_slice(&[2, 0, 0, 0]); // 4 bytes/row -> 10,000 bytes > 4096
        }
        assert!(big.len() > RLE_BUFFER_SIZE * 2);
        let mut rb = SliceReader::new(&big);
        let mut decb = BmpRleDecoder::new(2, 8, 1, 4, 4, ScanlineOrder::TopDown);
        assert_eq!(
            decb.prepare(false, ColorType::Rgba8888, false, false, &mut rb),
            CodecResult::Success
        );
        // Skip all rows (`dst = None`)
        assert_eq!(
            decb.decode_rows(&mut rb, None, num_rows, 0, ColorType::Rgba8888, false),
            num_rows
        );
    }

    #[test]
    fn wbmp_mbf_header_and_conversion() {
        // Multi-byte integer decoding
        assert_eq!(read_mbf(&mut SliceReader::new(&[0x00])), Some(0));
        assert_eq!(read_mbf(&mut SliceReader::new(&[0x7F])), Some(127));
        assert_eq!(read_mbf(&mut SliceReader::new(&[0x81, 0x00])), Some(128));
        assert_eq!(
            read_mbf(&mut SliceReader::new(&[0x83, 0xFF, 0x7F])),
            Some(0xFFFF)
        );
        // Truncated continuation bit
        assert_eq!(read_mbf(&mut SliceReader::new(&[0x81])), None);
        // 64-bit overflow (> 9 continuation bytes with non-zero top bits)
        let overflow = [0xFFu8; 10];
        assert_eq!(read_mbf(&mut SliceReader::new(&overflow)), None);

        // Valid WBMP: type=0, fixed=0, width=200 (0x81 0x48), height=100 (0x64)
        let valid = [0x00u8, 0x00, 0x81, 0x48, 0x64];
        assert!(is_wbmp(&valid));
        assert_eq!(
            read_wbmp_header(&mut SliceReader::new(&valid)),
            Some(ISize {
                width: 200,
                height: 100
            })
        );

        // Fixed header bits 0x60 allowed, 0x9F rejected
        assert!(is_wbmp(&[0x00, 0x60, 0x08, 0x08]));
        assert!(!is_wbmp(&[0x00, 0x80, 0x08, 0x08]));
        assert!(!is_wbmp(&[0x00, 0x01, 0x08, 0x08]));
        assert!(!is_wbmp(&[0x01, 0x00, 0x08, 0x08])); // non-zero type
        assert!(!is_wbmp(&[0x00, 0x00, 0x00, 0x08])); // zero width
        assert!(!is_wbmp(&[0x00, 0x00, 0x84, 0x80, 0x00, 0x08])); // width = 0x10000 > 0xFFFF

        assert_eq!(wbmp_src_row_bytes(1), 1);
        assert_eq!(wbmp_src_row_bytes(8), 1);
        assert_eq!(wbmp_src_row_bytes(9), 2);

        assert!(wbmp_conversion_supported(
            ColorType::Gray8,
            AlphaType::Opaque,
            false,
            true
        ));
        assert!(!wbmp_conversion_supported(
            ColorType::RgbaF16,
            AlphaType::Opaque,
            false,
            true
        ));
        assert!(wbmp_conversion_supported(
            ColorType::RgbaF16,
            AlphaType::Premul,
            true,
            true
        ));
        assert!(!wbmp_conversion_supported(
            ColorType::Alpha8,
            AlphaType::Premul,
            false,
            true
        ));
        assert!(!wbmp_conversion_supported(
            ColorType::Rgba8888,
            AlphaType::Unknown,
            false,
            true
        ));
    }

    #[test]
    fn ico_directory_parsing_sorting_and_selection() {
        assert!(is_ico(&[0, 0, 1, 0, 1, 0]));
        assert!(is_ico(&[0, 0, 2, 0, 1, 0]));
        assert!(!is_ico(&[0, 0, 3, 0]));
        assert!(!is_ico(&[0, 0, 1]));

        assert!(is_png_payload(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]));
        assert!(!is_png_payload(&[0x28, 0, 0, 0, 16, 0, 0, 0]));

        // Truncated header -> IncompleteInput
        assert_eq!(parse_ico_directory(&[0, 0, 1, 0]), Err(CodecResult::IncompleteInput));
        // Zero images -> InvalidInput
        assert_eq!(
            parse_ico_directory(&[0, 0, 1, 0, 0, 0]),
            Err(CodecResult::InvalidInput)
        );

        // Build ICO with 3 entries stored out-of-order, one overlapping entry skipped
        // Directory size = 6 + 3 * 16 = 54
        let mut ico = vec![0u8; 120];
        ico[0..6].copy_from_slice(&[0, 0, 1, 0, 3, 0]);
        // Entry 0: offset=80, size=20
        ico[6 + 8..6 + 12].copy_from_slice(&20u32.to_le_bytes());
        ico[6 + 12..6 + 16].copy_from_slice(&80u32.to_le_bytes());
        // Entry 1: offset=54, size=26
        ico[22 + 8..22 + 12].copy_from_slice(&26u32.to_le_bytes());
        ico[22 + 12..22 + 16].copy_from_slice(&54u32.to_le_bytes());
        // Entry 2: offset=60 (overlaps Entry 1 [54..80]), size=10 -> must be skipped
        ico[38 + 8..38 + 12].copy_from_slice(&10u32.to_le_bytes());
        ico[38 + 12..38 + 16].copy_from_slice(&60u32.to_le_bytes());

        let dir = parse_ico_directory(&ico).unwrap();
        assert_eq!(dir.default_result, CodecResult::InvalidInput);
        assert_eq!(
            dir.entries,
            vec![
                IcoEntry { offset: 54, size: 26 },
                IcoEntry { offset: 80, size: 20 },
            ]
        );

        // Entry extending past EOF sets default_result = IncompleteInput
        let mut trunc_ico = vec![0u8; 60];
        trunc_ico[0..6].copy_from_slice(&[0, 0, 1, 0, 1, 0]);
        trunc_ico[6 + 8..6 + 12].copy_from_slice(&50u32.to_le_bytes()); // 22 + 50 > 60
        trunc_ico[6 + 12..6 + 16].copy_from_slice(&22u32.to_le_bytes());
        let trunc_dir = parse_ico_directory(&trunc_ico).unwrap();
        assert!(trunc_dir.entries.is_empty());
        assert_eq!(trunc_dir.default_result, CodecResult::IncompleteInput);

        // Embedded codec selection
        assert_eq!(ico_select_largest_codec(&[1024, 4096, 256]), 1);
        let dims = [
            ISize { width: 16, height: 16 },
            ISize { width: 32, height: 32 },
            ISize { width: 64, height: 64 },
            ISize { width: 32, height: 32 },
        ];
        assert_eq!(
            ico_get_scaled_dimensions(ISize { width: 64, height: 64 }, 0.25, &dims),
            ISize { width: 32, height: 32 }
        );
        assert_eq!(
            ico_get_scaled_dimensions(ISize { width: 64, height: 64 }, 0.05, &dims),
            ISize { width: 16, height: 16 }
        );
        assert_eq!(ico_choose_codec(&dims, ISize { width: 32, height: 32 }, 0), 1);
        assert_eq!(ico_choose_codec(&dims, ISize { width: 32, height: 32 }, 2), 3);
        assert_eq!(ico_choose_codec(&dims, ISize { width: 48, height: 48 }, 0), -1);
    }
}
