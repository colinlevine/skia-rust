#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's core codec framework, swizzlers, samplers,
//! Android codec adapters, and encoder base (`P2.1`).

pub mod android_codec;
pub mod codec;
pub mod encoded_info;
pub mod encoder;
pub mod frame_holder;
pub mod mask_swizzler;
pub mod palette;
pub mod pixmap_utils;
pub mod sampler;
pub mod swizzler;
pub mod types;

#[cfg(test)]
mod tests {
    use skia_rs_color::color::mul_div_255_round;
    use skia_rs_color::image_info::{AlphaType, ColorType};
    use skia_rs_color::masks::{InputMasks, Masks};
    use skia_rs_geometry::{IRect, ISize};

    use crate::android_codec::*;
    use crate::codec::*;
    use crate::encoded_info::*;
    use crate::encoder::*;
    use crate::frame_holder::*;
    use crate::mask_swizzler::*;
    use crate::palette::*;
    use crate::pixmap_utils::*;
    use crate::sampler::*;
    use crate::swizzler::*;
    use crate::types::*;

    #[test]
    fn exhaustive_mul_div_255_round_matches_exact() {
        for a in 0u32..=255 {
            for b in 0u32..=255 {
                let fast = mul_div_255_round(a, b);
                let exact = (a * b + 127) / 255;
                assert_eq!(fast, exact, "mismatch for a={a}, b={b}");
            }
        }
    }

    #[test]
    fn palette_copy_and_limits() {
        let src = [0xFF010203u32, 0xFF040506, 0xFF070809];
        let pal = ColorPalette::new(&src).unwrap();
        assert_eq!(pal.count(), 3);
        assert_eq!(pal.get(1), Some(0xFF040506));
        assert_eq!(pal.get(3), None);

        let mut dst = [0u32; 3];
        assert!(copy_palette(&mut dst, &src, 3));
        assert_eq!(dst, src);

        let too_big = vec![0u32; 257];
        assert!(ColorPalette::new(&too_big).is_none());
        assert!(!copy_palette(&mut dst, &src, 257));
    }

    #[test]
    fn encoded_info_types_and_bpp() {
        assert_eq!(
            make_image_info_types(EncodedColor::Gray, EncodedAlpha::Opaque, false),
            (ColorType::Gray8, AlphaType::Opaque)
        );
        assert_eq!(
            make_image_info_types(EncodedColor::XAlpha, EncodedAlpha::Unpremul, false),
            (ColorType::Alpha8, AlphaType::Unpremul)
        );
        assert_eq!(
            make_image_info_types(EncodedColor::Rgb565, EncodedAlpha::Opaque, false),
            (ColorType::Rgb565, AlphaType::Opaque)
        );
        assert_eq!(
            make_image_info_types(EncodedColor::Rgba, EncodedAlpha::Binary, true),
            (ColorType::Bgra8888, AlphaType::Unpremul)
        );
        assert_eq!(bits_per_pixel(EncodedColor::Gray, 1), 1);
        assert_eq!(bits_per_pixel(EncodedColor::GrayAlpha, 8), 16);
        assert_eq!(bits_per_pixel(EncodedColor::Rgb, 16), 48);
        assert_eq!(bits_per_pixel(EncodedColor::Rgba, 16), 64);
        assert!(verify_color(EncodedColor::Gray, EncodedAlpha::Opaque, 1));
        assert!(!verify_color(EncodedColor::Gray, EncodedAlpha::Unpremul, 8));
        assert!(!verify_color(EncodedColor::Palette, EncodedAlpha::Opaque, 16));
        assert!(verify_color(EncodedColor::XAlpha, EncodedAlpha::Unpremul, 8));
    }

    #[test]
    fn sampler_fill_and_xr_zero_init() {
        // RGBA8888 skips when zeroInit == Yes
        let mut buf = vec![0xAAu8; 32];
        fill(ColorType::Rgba8888, 2, 2, &mut buf, 16, ZeroInitialized::Yes);
        assert!(buf.iter().all(|&b| b == 0xAA));

        // RGBA8888 fills only row width (8 bytes), leaves padding intact
        fill(ColorType::Rgba8888, 2, 2, &mut buf, 16, ZeroInitialized::No);
        assert!(buf[0..8].iter().all(|&b| b == 0));
        assert!(buf[8..16].iter().all(|&b| b == 0xAA));
        assert!(buf[16..24].iter().all(|&b| b == 0));
        assert!(buf[24..32].iter().all(|&b| b == 0xAA));

        // BGR_101010x_XR MUST fill biased zero even when zeroInit == Yes!
        let mut xr32 = vec![0u8; 8];
        fill(ColorType::Bgr101010xXr, 2, 1, &mut xr32, 8, ZeroInitialized::Yes);
        let expected32 = BGR_101010X_XR_ZERO.to_ne_bytes();
        assert_eq!(&xr32[0..4], &expected32);
        assert_eq!(&xr32[4..8], &expected32);

        // BGRA_10101010_XR MUST fill biased zero even when zeroInit == Yes!
        let mut xr64 = vec![0u8; 16];
        fill(ColorType::Bgra10101010Xr, 2, 1, &mut xr64, 16, ZeroInitialized::Yes);
        let expected64 = BGRA_10101010_XR_ZERO.to_ne_bytes();
        assert_eq!(&xr64[0..8], &expected64);
        assert_eq!(&xr64[8..16], &expected64);

        // Sampling math
        assert_eq!(get_sampled_dimension(10, 3), 3);
        assert_eq!(get_sampled_dimension(2, 5), 1);
        assert_eq!(get_sampled_dimension(10, 0), 0);
        assert_eq!(get_start_coord(3), 1);
        assert_eq!(get_start_coord(4), 2);
        assert!(is_coord_necessary(1, 3, 3));
        assert!(!is_coord_necessary(0, 3, 3));
        assert!(is_coord_necessary(4, 3, 3));
        assert!(is_coord_necessary(7, 3, 3));
        assert!(!is_coord_necessary(10, 3, 3));
    }

    #[test]
    fn swizzler_bit_and_small_index() {
        // 1-bit grayscale: 0b1011_0100 = 0xB4
        let src = [0xB4u8];
        let mut gray = [0u8; 8];
        swizzle_row(SwizzleProcId::BitToGrayscale, &mut gray, &src, 8, 1, 1, 0, &[], false);
        assert_eq!(gray, [0xFF, 0, 0xFF, 0xFF, 0, 0xFF, 0, 0]);

        // Sample every 2 bits starting at bit offset 1 -> bits 1, 3, 5, 7 -> 0, 1, 1, 0
        let mut gray_s = [0u8; 4];
        swizzle_row(SwizzleProcId::BitToGrayscale, &mut gray_s, &src, 4, 1, 2, 1, &[], false);
        assert_eq!(gray_s, [0, 0xFF, 0xFF, 0]);

        // BitToF16
        let mut f16 = [0u8; 16];
        swizzle_row(SwizzleProcId::BitToF16, &mut f16, &src, 2, 1, 1, 0, &[], false);
        let p0 = u64::from_ne_bytes(f16[0..8].try_into().unwrap());
        let p1 = u64::from_ne_bytes(f16[8..16].try_into().unwrap());
        assert_eq!(p0, 0x3C00_3C00_3C00_3C00);
        assert_eq!(p1, 0x3C00_0000_0000_0000);

        // 4-bit small index to N32 and 565 (RGBA vs BGRA)
        let ctable = [
            0xFF_00_00_F8u32, // in RGBA: R=0xF8, G=0, B=0; in BGRA: B=0xF8, G=0, R=0
            0xFF_00_FC_00u32, // G=0xFC
            0xFF_F8_00_00u32, // in RGBA: B=0xF8; in BGRA: R=0xF8
            0xFF_FF_FF_FFu32,
        ];
        let idx4 = [0x01u8, 0x23u8]; // indices: 0, 1, 2, 3
        let mut dst32 = [0u8; 16];
        swizzle_row(SwizzleProcId::SmallIndexToN32, &mut dst32, &idx4, 4, 4, 4, 0, &ctable, false);
        for i in 0..4 {
            assert_eq!(
                u32::from_ne_bytes(dst32[i * 4..i * 4 + 4].try_into().unwrap()),
                ctable[i]
            );
        }

        let mut dst565_rgba = [0u8; 8];
        swizzle_row(
            SwizzleProcId::SmallIndexTo565,
            &mut dst565_rgba,
            &idx4,
            4,
            4,
            4,
            0,
            &ctable,
            false,
        );
        assert_eq!(u16::from_ne_bytes(dst565_rgba[0..2].try_into().unwrap()), 0xF800);
        assert_eq!(u16::from_ne_bytes(dst565_rgba[2..4].try_into().unwrap()), 0x07E0);
        assert_eq!(u16::from_ne_bytes(dst565_rgba[4..6].try_into().unwrap()), 0x001F);
        assert_eq!(u16::from_ne_bytes(dst565_rgba[6..8].try_into().unwrap()), 0xFFFF);

        let mut dst565_bgra = [0u8; 8];
        swizzle_row(
            SwizzleProcId::SmallIndexTo565,
            &mut dst565_bgra,
            &idx4,
            4,
            4,
            4,
            0,
            &ctable,
            true,
        );
        assert_eq!(u16::from_ne_bytes(dst565_bgra[0..2].try_into().unwrap()), 0x001F);
        assert_eq!(u16::from_ne_bytes(dst565_bgra[2..4].try_into().unwrap()), 0x07E0);
        assert_eq!(u16::from_ne_bytes(dst565_bgra[4..6].try_into().unwrap()), 0xF800);
        assert_eq!(u16::from_ne_bytes(dst565_bgra[6..8].try_into().unwrap()), 0xFFFF);
    }

    #[test]
    fn swizzler_rgb_rgba_16_cmyk_skipz() {
        // RGBA premul with leading zero skipped (verify dst[0] untouched!)
        let src_rgba = [0u8, 0, 0, 0, 255, 128, 64, 128];
        let mut dst_rgba = [0xEEu8; 8];
        swizzle_row(
            SwizzleProcId::FastRgbaToRgbaPremulSkipZ,
            &mut dst_rgba,
            &src_rgba,
            2,
            4,
            4,
            0,
            &[],
            false,
        );
        // First pixel skipped -> untouched 0xEE
        assert_eq!(&dst_rgba[0..4], &[0xEE, 0xEE, 0xEE, 0xEE]);
        let p1 = u32::from_ne_bytes(dst_rgba[4..8].try_into().unwrap());
        // R = mul_div(255,128)=128, G = mul_div(128,128)=64, B = mul_div(64,128)=32, A = 128
        assert_eq!(p1, (128 << 24) | (32 << 16) | (64 << 8) | 128);

        // 16-bit big-endian RGBA -> BGRA unpremul
        let src16 = [0x12u8, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let mut dst16 = [0u8; 4];
        swizzle_row(
            SwizzleProcId::Rgba16ToBgraUnpremul,
            &mut dst16,
            &src16,
            1,
            8,
            8,
            0,
            &[],
            false,
        );
        // BGRA: B=0x9A, G=0x56, R=0x12, A=0xDE
        assert_eq!(
            u32::from_ne_bytes(dst16),
            (0xDE << 24) | (0x12 << 16) | (0x56 << 8) | 0x9A
        );

        // Inverted CMYK -> RGBA
        let src_cmyk = [255u8, 128, 0, 128];
        let mut dst_cmyk = [0u8; 4];
        swizzle_row(
            SwizzleProcId::FastCmykToRgba,
            &mut dst_cmyk,
            &src_cmyk,
            1,
            4,
            4,
            0,
            &[],
            false,
        );
        assert_eq!(
            u32::from_ne_bytes(dst_cmyk),
            (0xFF << 24) | (0 << 16) | (64 << 8) | 128
        );

        // GrayAlpha -> A8
        let src_ga = [0x11u8, 0x22, 0x33, 0x44];
        let mut dst_a8 = [0u8; 2];
        swizzle_row(
            SwizzleProcId::GrayAlphaToA8,
            &mut dst_a8,
            &src_ga,
            2,
            2,
            2,
            0,
            &[],
            false,
        );
        assert_eq!(dst_a8, [0x22, 0x44]);

        // SampleX frame subset clamping (safety net)
        let sx = compute_sample_x(2, 0, 9, 2, 10, 1, 4, true);
        assert_eq!(sx.swizzle_width, 1);
        assert_eq!(sx.allocated_width, 5);
        assert_eq!(sx.dst_offset_bytes, 16);
        assert!(!sx.use_fast_proc);
    }

    #[test]
    fn mask_swizzler_16_24_32() {
        // 16-bit RGB565 masks
        let masks16 = Masks::create(
            InputMasks {
                red: 0xF800,
                green: 0x07E0,
                blue: 0x001F,
                alpha: 0,
            },
            2,
        )
        .unwrap();
        let init = select_mask_swizzler(
            ColorType::Rgba8888,
            AlphaType::Opaque,
            2,
            true,
            16,
            None,
        )
        .unwrap();
        assert_eq!(init.proc_id, MaskSwizzleProcId::Mask16ToRgbaOpaque as i32);

        let mut src16 = [0u8; 4];
        src16[0..2].copy_from_slice(&0xF800u16.to_ne_bytes()); // pure red
        src16[2..4].copy_from_slice(&0x001Fu16.to_ne_bytes()); // pure blue
        let mut dst = [0u8; 8];
        swizzle_mask_row(
            MaskSwizzleProcId::Mask16ToRgbaOpaque,
            &mut dst,
            &src16,
            2,
            &masks16,
            0,
            1,
        );
        assert_eq!(u32::from_ne_bytes(dst[0..4].try_into().unwrap()), 0xFF_00_00_FF);
        assert_eq!(u32::from_ne_bytes(dst[4..8].try_into().unwrap()), 0xFF_FF_00_00);

        // 32-bit ARGB masks with premul
        let masks32 = Masks::create(
            InputMasks {
                red: 0x00FF0000,
                green: 0x0000FF00,
                blue: 0x000000FF,
                alpha: 0xFF000000,
            },
            4,
        )
        .unwrap();
        let src32 = 0x80_FF_80_40u32.to_ne_bytes();
        let mut dst32 = [0u8; 4];
        swizzle_mask_row(
            MaskSwizzleProcId::Mask32ToBgraPremul,
            &mut dst32,
            &src32,
            1,
            &masks32,
            0,
            1,
        );
        // BGRA: A=0x80, R=0x80, G=0x40, B=0x20
        assert_eq!(u32::from_ne_bytes(dst32), 0x80_80_40_20);

        // 24-bit BGR masks
        let masks24 = Masks::create(
            InputMasks {
                red: 0xFF0000,
                green: 0x00FF00,
                blue: 0x0000FF,
                alpha: 0,
            },
            3,
        )
        .unwrap();
        let src24 = [0x10u8, 0x20, 0x30, 0xAA, 0xBB, 0xCC];
        let mut dst24 = [0u8; 4];
        swizzle_mask_row(
            MaskSwizzleProcId::Mask24ToRgbaOpaque,
            &mut dst24,
            &src24,
            1,
            &masks24,
            1,
            1,
        );
        // pixel 1: B=0xAA, G=0xBB, R=0xCC -> RGBA: 0xFF_AA_BB_CC
        assert_eq!(u32::from_ne_bytes(dst24), 0xFF_AA_BB_CC);
    }

    #[test]
    fn frame_holder_ind_and_dep_rules() {
        let screen_w = 100;
        let screen_h = 100;

        // IND1: frame 0 partial rect -> has_alpha = true, required = NO_FRAME
        let f0 = FrameSummary {
            frame_id: 0,
            reported_alpha: EncodedAlpha::Opaque as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            disposal_method: DisposalMethod::Keep as i32,
            blend: Blend::SrcOver as i32,
            frame_rect: IRect::make_ltrb(0, 0, 50, 50),
        };
        let r0 = compute_alpha_and_required_frame(screen_w, screen_h, f0, |_| unreachable!());
        assert_eq!(r0, FrameDependencyResult { has_alpha: true, required_frame: NO_FRAME });

        let mut frames = vec![FrameSummary {
            has_alpha: r0.has_alpha,
            required_frame: r0.required_frame,
            ..f0
        }];

        // IND2: frame 1 covers full screen with kSrc -> independent
        let f1 = FrameSummary {
            frame_id: 1,
            reported_alpha: EncodedAlpha::Unpremul as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            disposal_method: DisposalMethod::Keep as i32,
            blend: Blend::Src as i32,
            frame_rect: IRect::make_ltrb(0, 0, 100, 100),
        };
        let r1 = compute_alpha_and_required_frame(screen_w, screen_h, f1, |id| frames[id as usize]);
        assert_eq!(r1, FrameDependencyResult { has_alpha: true, required_frame: NO_FRAME });
        frames.push(FrameSummary {
            has_alpha: r1.has_alpha,
            required_frame: r1.required_frame,
            ..f1
        });

        // DEP5: frame 2 reports alpha and blends SrcOver -> depends on frame 1
        let f2 = FrameSummary {
            frame_id: 2,
            reported_alpha: EncodedAlpha::Unpremul as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            disposal_method: DisposalMethod::RestorePrevious as i32,
            blend: Blend::SrcOver as i32,
            frame_rect: IRect::make_ltrb(10, 10, 40, 40),
        };
        let r2 = compute_alpha_and_required_frame(screen_w, screen_h, f2, |id| frames[id as usize]);
        assert_eq!(r2, FrameDependencyResult { has_alpha: true, required_frame: 1 });
        frames.push(FrameSummary {
            has_alpha: r2.has_alpha,
            required_frame: r2.required_frame,
            ..f2
        });

        // Frame 3 after RestorePrevious skips frame 2 and depends on frame 1 (DEP7)
        let f3 = FrameSummary {
            frame_id: 3,
            reported_alpha: EncodedAlpha::Opaque as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            disposal_method: DisposalMethod::RestoreBgColor as i32,
            blend: Blend::SrcOver as i32,
            frame_rect: IRect::make_ltrb(10, 10, 20, 20),
        };
        let r3 = compute_alpha_and_required_frame(screen_w, screen_h, f3, |id| frames[id as usize]);
        assert_eq!(r3, FrameDependencyResult { has_alpha: true, required_frame: 1 });
        frames.push(FrameSummary {
            has_alpha: r3.has_alpha,
            required_frame: r3.required_frame,
            ..f3
        });

        // IND6: frame 4 covers frame 3's rect AND frame 1's rect (full screen via kSrcOver opaque -> IND2,
        // so test a smaller frame 0/1 scenario for IND6)
        let base_frames = vec![
            FrameSummary {
                frame_id: 0,
                reported_alpha: EncodedAlpha::Opaque as i32,
                has_alpha: true,
                required_frame: NO_FRAME,
                disposal_method: DisposalMethod::Keep as i32,
                blend: Blend::SrcOver as i32,
                frame_rect: IRect::make_ltrb(10, 10, 30, 30),
            },
            FrameSummary {
                frame_id: 1,
                reported_alpha: EncodedAlpha::Opaque as i32,
                has_alpha: false,
                required_frame: NO_FRAME,
                disposal_method: DisposalMethod::Keep as i32,
                blend: Blend::SrcOver as i32,
                frame_rect: IRect::make_ltrb(5, 5, 35, 35),
            },
        ];
        let r_ind6 = compute_alpha_and_required_frame(
            screen_w,
            screen_h,
            base_frames[1],
            |id| base_frames[id as usize],
        );
        assert_eq!(r_ind6, FrameDependencyResult { has_alpha: true, required_frame: NO_FRAME });

        // IND3: frame 0 has RestorePrevious -> frame 1 is independent
        let rp0 = FrameSummary {
            frame_id: 0,
            reported_alpha: EncodedAlpha::Opaque as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            disposal_method: DisposalMethod::RestorePrevious as i32,
            blend: Blend::SrcOver as i32,
            frame_rect: IRect::make_ltrb(0, 0, 100, 100),
        };
        let rp1 = FrameSummary {
            frame_id: 1,
            reported_alpha: EncodedAlpha::Opaque as i32,
            has_alpha: false,
            required_frame: NO_FRAME,
            disposal_method: DisposalMethod::Keep as i32,
            blend: Blend::SrcOver as i32,
            frame_rect: IRect::make_ltrb(0, 0, 50, 50),
        };
        let r_ind3 = compute_alpha_and_required_frame(screen_w, screen_h, rp1, |_| rp0);
        assert_eq!(r_ind3, FrameDependencyResult { has_alpha: true, required_frame: NO_FRAME });

        // IND4: prevFrame is independent + RestoreBgColor
        let bg0 = FrameSummary {
            disposal_method: DisposalMethod::RestoreBgColor as i32,
            ..rp0
        };
        let r_ind4 = compute_alpha_and_required_frame(screen_w, screen_h, rp1, |_| bg0);
        assert_eq!(r_ind4, FrameDependencyResult { has_alpha: true, required_frame: NO_FRAME });
    }

    #[test]
    fn codec_validation_and_xform() {
        assert_eq!(result_to_cstr(CodecResult::Success as i32), b"success\0");
        assert_eq!(result_to_cstr(999), b"bogus result value\0");

        assert!(conversion_supported(
            EncodedColor::Gray,
            ColorType::Gray8,
            AlphaType::Opaque,
            true
        ));
        assert!(!conversion_supported(
            EncodedColor::Rgb,
            ColorType::Gray8,
            AlphaType::Opaque,
            true
        ));
        assert!(conversion_supported(
            EncodedColor::XAlpha,
            ColorType::Alpha8,
            AlphaType::Unpremul,
            false
        ));
        assert!(!conversion_supported(
            EncodedColor::Rgba,
            ColorType::Rgb565,
            AlphaType::Unpremul,
            false
        ));

        // SelectXformFormat
        assert_eq!(
            select_xform_format(ColorType::Rgb565, false, true),
            Some(skcms_pixel_format::BGR_565)
        );
        assert_eq!(
            select_xform_format(ColorType::Rgb565, true, true),
            Some(skcms_pixel_format::RGBA_8888)
        );
        assert_eq!(
            select_xform_format(ColorType::Rgb565, true, false),
            Some(skcms_pixel_format::BGRA_8888)
        );

        // Budget
        let mut budget = 100usize;
        assert!(allocate_from_budget(&mut budget, 40));
        assert_eq!(budget, 60);
        assert!(!allocate_from_budget(&mut budget, 61));
        assert_eq!(budget, 60);

        // Zero rect with scaling 100x100 -> 50x50
        let zp = compute_zero_rect(
            ISize { width: 50, height: 50 },
            ISize { width: 100, height: 100 },
            IRect::make_ltrb(10, 20, 30, 40),
            4,
            200,
        );
        assert!(zp.ok);
        assert!(zp.should_fill);
        assert_eq!(zp.rect, IRect::make_ltrb(5, 10, 15, 20));
        assert_eq!(zp.byte_offset, 5 * 4 + 10 * 200);

        // Scanline order
        assert_eq!(output_scanline(ScanlineOrder::TopDown, 10, 3), 3);
        assert_eq!(output_scanline(ScanlineOrder::BottomUp, 10, 3), 6);

        // Generator info adjust
        let adj = adjust_generator_info(
            100,
            200,
            AlphaType::Unpremul,
            EncodedOrigin::RightTop,
            false,
            AlphaType::Unknown,
        );
        assert_eq!(adj.width, 200);
        assert_eq!(adj.height, 100);
        assert_eq!(adj.alpha_type, AlphaType::Premul as i32);
    }

    #[test]
    fn pixmap_utils_and_encoder() {
        assert_eq!(
            validate_orient(
                ColorType::Rgba8888,
                ColorType::Bgra8888,
                10,
                20,
                10,
                20,
                EncodedOrigin::TopLeft,
                false
            ),
            OrientAction::Invalid
        );
        assert_eq!(
            validate_orient(
                ColorType::Rgba8888,
                ColorType::Rgba8888,
                10,
                20,
                20,
                10,
                EncodedOrigin::RightTop,
                false
            ),
            OrientAction::Draw
        );
        assert_eq!(
            validate_orient(
                ColorType::Rgba8888,
                ColorType::Rgba8888,
                10,
                20,
                10,
                20,
                EncodedOrigin::TopLeft,
                true
            ),
            OrientAction::NoOp
        );
        assert_eq!(
            validate_orient(
                ColorType::Rgba8888,
                ColorType::Rgba8888,
                10,
                20,
                10,
                20,
                EncodedOrigin::BottomRight,
                true
            ),
            OrientAction::Invalid
        );

        assert_eq!(clamp_encode_rows(0, 10, 0), None);
        assert_eq!(clamp_encode_rows(10, 10, 1), None);
        assert_eq!(clamp_encode_rows(7, 10, 5), Some(3));
        assert_eq!(clamp_encode_rows(2, 10, 4), Some(4));
    }

    #[test]
    fn android_and_sampled_codec_math() {
        assert_eq!(
            select_android_codec_kind(EncodedImageFormat::Png),
            AndroidCodecKind::Sampled
        );
        assert_eq!(
            select_android_codec_kind(EncodedImageFormat::Webp),
            AndroidCodecKind::Adapter
        );
        assert_eq!(
            select_android_codec_kind(EncodedImageFormat::Astc),
            AndroidCodecKind::Unsupported
        );

        // High precision defaults to F16, 10-bit opaque to 1010102
        assert_eq!(
            compute_output_color_type(
                ColorType::Unknown,
                ColorType::Rgba8888,
                AlphaType::Opaque,
                16,
                16,
                false
            ),
            ColorType::RgbaF16
        );
        assert_eq!(
            compute_output_color_type(
                ColorType::Unknown,
                ColorType::Rgba8888,
                AlphaType::Opaque,
                16,
                10,
                false
            ),
            ColorType::Rgba1010102
        );
        assert_eq!(
            compute_output_color_type(
                ColorType::Alpha8,
                ColorType::Gray8,
                AlphaType::Opaque,
                8,
                8,
                false
            ),
            ColorType::Gray8
        );

        // computeSampleSize on a 100x80 image with simple floor division sampler
        let orig = ISize { width: 100, height: 80 };
        let mut desired = ISize { width: 30, height: 25 };
        let ss = compute_sample_size(
            orig,
            &mut desired,
            false,
            || false,
            |s| ISize {
                width: get_sampled_dimension(orig.width, s),
                height: get_sampled_dimension(orig.height, s),
            },
        );
        assert_eq!(ss, 3);
        assert_eq!(desired, ISize { width: 33, height: 26 });

        // Animated WebP cannot downscale
        let mut webp_desired = ISize { width: 50, height: 40 };
        let ss_webp = compute_sample_size(
            orig,
            &mut webp_desired,
            true,
            || true,
            |_| unreachable!(),
        );
        assert_eq!(ss_webp, 1);
        assert_eq!(webp_desired, orig);

        // Native scaling plan for JPEG
        let p4 = plan_native_scaling(EncodedImageFormat::Jpeg, 4);
        assert!(p4.use_native_scale);
        assert_eq!(p4.remaining_sample_size, 1);
        assert_eq!(p4.native_sample_size, 1);

        let p24 = plan_native_scaling(EncodedImageFormat::Jpeg, 24);
        assert!(p24.use_native_scale);
        assert_eq!(p24.native_sample_size, 8);
        assert_eq!(p24.remaining_sample_size, 3);

        let p6 = plan_native_scaling(EncodedImageFormat::Jpeg, 6);
        assert!(p6.use_native_scale);
        assert_eq!(p6.native_sample_size, 2);
        assert_eq!(p6.remaining_sample_size, 3);

        let p_png = plan_native_scaling(EncodedImageFormat::Png, 8);
        assert!(!p_png.use_native_scale);
        assert_eq!(p_png.remaining_sample_size, 8);

        // Sampled decode params with subset
        let params = compute_sampled_decode_params(
            ISize { width: 50, height: 40 },
            ISize { width: 10, height: 10 },
            2,
            Some(IRect::make_ltrb(20, 10, 60, 50)),
        );
        assert_eq!(params.subset_y, 5);
        assert_eq!(params.subset_width, 20);
        assert_eq!(params.subset_height, 20);
        assert_eq!(params.sample_x, 2);
        assert_eq!(params.sample_y, 2);
        assert_eq!(params.start_y, 6);
    }

    #[test]
    fn swizzler_copy_and_sample_widths() {
        let src: Vec<u8> = (0u8..32).collect();

        let mut dst1 = [0u8; 4];
        swizzle_row(SwizzleProcId::Sample1, &mut dst1, &src, 4, 1, 2, 1, &[], false);
        assert_eq!(dst1, [1, 3, 5, 7]);

        let mut dst2 = [0u8; 4];
        swizzle_row(SwizzleProcId::Sample2, &mut dst2, &src, 2, 2, 4, 2, &[], false);
        assert_eq!(dst2, [2, 3, 6, 7]);

        let mut dst4 = [0u8; 8];
        swizzle_row(SwizzleProcId::Sample4, &mut dst4, &src, 2, 4, 8, 4, &[], false);
        assert_eq!(dst4, [4, 5, 6, 7, 12, 13, 14, 15]);

        let mut dst6 = [0u8; 12];
        swizzle_row(SwizzleProcId::Sample6, &mut dst6, &src, 2, 6, 12, 0, &[], false);
        assert_eq!(dst6, [0, 1, 2, 3, 4, 5, 12, 13, 14, 15, 16, 17]);

        let mut dst8 = [0u8; 16];
        swizzle_row(SwizzleProcId::Sample8, &mut dst8, &src, 2, 8, 16, 0, &[], false);
        assert_eq!(&dst8[0..8], &src[0..8]);
        assert_eq!(&dst8[8..16], &src[16..24]);

        let mut dst_copy = [0u8; 6];
        swizzle_row(SwizzleProcId::Copy, &mut dst_copy, &src, 2, 3, 3, 3, &[], false);
        assert_eq!(dst_copy, [3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn swizzler_all_gray_index_rgb_rgba_variants() {
        // Index8 SkipZ leaves transparent entries untouched
        let mut ctable = [0u32; 256];
        ctable[0] = 0;
        ctable[1] = 0xFF112233;
        let idx = [0u8, 1, 0, 1];
        let mut dst = [0xAAu8; 16];
        swizzle_row(SwizzleProcId::IndexToN32SkipZ, &mut dst, &idx, 4, 1, 1, 0, &ctable, false);
        assert_eq!(&dst[0..4], &[0xAA; 4]);
        assert_eq!(u32::from_ne_bytes(dst[4..8].try_into().unwrap()), 0xFF112233);
        assert_eq!(&dst[8..12], &[0xAA; 4]);
        assert_eq!(u32::from_ne_bytes(dst[12..16].try_into().unwrap()), 0xFF112233);

        // Gray8 -> N32 (fast & sampled) and 565
        let gray = [0x00u8, 0x80, 0xFF];
        let mut dst_gn = [0u8; 12];
        swizzle_row(SwizzleProcId::FastGrayToN32, &mut dst_gn, &gray, 3, 1, 1, 0, &[], false);
        assert_eq!(u32::from_ne_bytes(dst_gn[0..4].try_into().unwrap()), 0xFF000000);
        assert_eq!(u32::from_ne_bytes(dst_gn[4..8].try_into().unwrap()), 0xFF808080);
        assert_eq!(u32::from_ne_bytes(dst_gn[8..12].try_into().unwrap()), 0xFFFFFFFF);

        let mut dst_g565 = [0u8; 4];
        swizzle_row(SwizzleProcId::GrayTo565, &mut dst_g565, &gray, 2, 1, 2, 0, &[], false);
        assert_eq!(u16::from_ne_bytes(dst_g565[0..2].try_into().unwrap()), 0x0000);
        assert_eq!(u16::from_ne_bytes(dst_g565[2..4].try_into().unwrap()), 0xFFFF);

        // GrayAlpha leading zero skip (premul & unpremul)
        let ga = [0u8, 0, 200, 128];
        let mut dst_ga = [0xBBu8; 8];
        swizzle_row(
            SwizzleProcId::FastGrayAlphaToN32PremulSkipZ,
            &mut dst_ga,
            &ga,
            2,
            2,
            2,
            0,
            &[],
            false,
        );
        assert_eq!(&dst_ga[0..4], &[0xBB; 4]);
        let pm = mul_div_255_round(200, 128);
        assert_eq!(
            u32::from_ne_bytes(dst_ga[4..8].try_into().unwrap()),
            (128 << 24) | (pm << 16) | (pm << 8) | pm
        );

        // RGB / BGR -> RGBA / BGRA / 565
        let rgb = [0xF8u8, 0x04, 0x08];
        let mut out4 = [0u8; 4];
        swizzle_row(SwizzleProcId::FastRgbToRgba, &mut out4, &rgb, 1, 3, 3, 0, &[], false);
        assert_eq!(u32::from_ne_bytes(out4), 0xFF_08_04_F8);
        swizzle_row(SwizzleProcId::FastRgbToBgra, &mut out4, &rgb, 1, 3, 3, 0, &[], false);
        assert_eq!(u32::from_ne_bytes(out4), 0xFF_F8_04_08);

        let mut out2 = [0u8; 2];
        swizzle_row(SwizzleProcId::RgbTo565, &mut out2, &rgb, 1, 3, 3, 0, &[], false);
        assert_eq!(u16::from_ne_bytes(out2), (0x1F << 11) | (0x01 << 5) | 0x01);
        swizzle_row(SwizzleProcId::BgrTo565, &mut out2, &rgb, 1, 3, 3, 0, &[], false);
        assert_eq!(u16::from_ne_bytes(out2), (0x01 << 11) | (0x01 << 5) | 0x1F);

        // RGBA16 premul
        let rgba16 = [0xFFu8, 0x00, 0x80, 0x00, 0x40, 0x00, 0x80, 0x00];
        swizzle_row(SwizzleProcId::Rgba16ToRgbaPremul, &mut out4, &rgba16, 1, 8, 8, 0, &[], false);
        assert_eq!(u32::from_ne_bytes(out4), (0x80 << 24) | (0x20 << 16) | (0x40 << 8) | 0x80);
    }

    #[test]
    fn swizzler_selection_matrix() {
        // Palette without ctable fails
        assert!(select_swizzler(
            EncodedColor::Palette,
            EncodedAlpha::Opaque,
            8,
            false,
            ColorType::Rgba8888,
            AlphaType::Opaque,
            10,
            ZeroInitialized::No,
            None,
            None
        )
        .is_none());

        // BGRX uses slow proc without fast proc and src_bpp = 4
        let bgrx = select_swizzler(
            EncodedColor::Bgrx,
            EncodedAlpha::Opaque,
            8,
            false,
            ColorType::Bgra8888,
            AlphaType::Opaque,
            10,
            ZeroInitialized::No,
            None,
            None,
        )
        .unwrap();
        assert_eq!(bgrx.fast_proc_id, -1);
        assert_eq!(bgrx.slow_proc_id, SwizzleProcId::RgbToRgba as i32);
        assert_eq!(bgrx.src_bpp, 4);

        // 1-bit Gray -> F16 has src_bpp = 1 (bits), dst_bpp = 8 (bytes)
        let bit_f16 = select_swizzler(
            EncodedColor::Gray,
            EncodedAlpha::Opaque,
            1,
            false,
            ColorType::RgbaF16,
            AlphaType::Opaque,
            16,
            ZeroInitialized::No,
            None,
            None,
        )
        .unwrap();
        assert_eq!(bit_f16.slow_proc_id, SwizzleProcId::BitToF16 as i32);
        assert_eq!(bit_f16.src_bpp, 1);
        assert_eq!(bit_f16.dst_bpp, 8);

        // MakeSimple supports 1,2,4,6,8 and rejects 3,5,7
        for bpp in [1, 2, 4, 6, 8] {
            assert!(select_simple_swizzler(bpp, ColorType::Rgba8888, 10, None, None).is_some());
        }
        for bpp in [0, 3, 5, 7, 16] {
            assert!(select_simple_swizzler(bpp, ColorType::Rgba8888, 10, None, None).is_none());
        }
    }

    #[test]
    fn fill_incomplete_and_incremental_validation() {
        // Incomplete image with prior required frame skips fill
        let p_skip = plan_fill_incomplete_image(
            ColorType::Rgba8888,
            100,
            400,
            ZeroInitialized::No,
            50,
            10,
            true,
            0, // required frame != NO_FRAME
            true,
            -1,
            -1,
            ScanlineOrder::TopDown,
        );
        assert!(!p_skip.should_fill);

        // TopDown fills starting at lines_decoded * row_bytes using sampler width
        let p_top = plan_fill_incomplete_image(
            ColorType::Rgba8888,
            100,
            400,
            ZeroInitialized::No,
            50,
            10,
            false,
            NO_FRAME,
            true,
            25,
            -1,
            ScanlineOrder::TopDown,
        );
        assert!(p_top.should_fill);
        assert_eq!(p_top.fill_width, 25);
        assert_eq!(p_top.lines_remaining, 40);
        assert_eq!(p_top.byte_offset, 10 * 400);

        // BottomUp fills starting at byte_offset 0
        let p_bot = plan_fill_incomplete_image(
            ColorType::Rgba8888,
            100,
            400,
            ZeroInitialized::No,
            50,
            10,
            false,
            NO_FRAME,
            true,
            -1,
            60,
            ScanlineOrder::BottomUp,
        );
        assert!(p_bot.should_fill);
        assert_eq!(p_bot.fill_width, 60);
        assert_eq!(p_bot.byte_offset, 0);

        // Incremental subset validation
        assert_eq!(
            validate_start_incremental_decode(
                false,
                ColorType::Rgba8888,
                true,
                ISize { width: 10, height: 10 },
                None
            ),
            CodecResult::Unimplemented
        );
        assert_eq!(
            validate_start_incremental_decode(
                true,
                ColorType::Rgba8888,
                true,
                ISize { width: 10, height: 10 },
                Some(IRect::make_ltrb(0, 2, 10, 8))
            ),
            CodecResult::Success
        );
        assert_eq!(
            validate_start_incremental_decode(
                true,
                ColorType::Rgba8888,
                true,
                ISize { width: 10, height: 10 },
                Some(IRect::make_ltrb(0, 2, 11, 8))
            ),
            CodecResult::InvalidParameters
        );

        // Scanline decode subset must span full height
        assert_eq!(
            validate_start_scanline_decode(
                ISize { width: 10, height: 10 },
                Some(IRect::make_ltrb(2, 0, 8, 10)),
                0
            ),
            CodecResult::Success
        );
        assert_eq!(
            validate_start_scanline_decode(
                ISize { width: 10, height: 10 },
                Some(IRect::make_ltrb(2, 1, 8, 10)),
                0
            ),
            CodecResult::InvalidInput
        );
        assert_eq!(
            validate_start_scanline_decode(ISize { width: 10, height: 10 }, None, 1),
            CodecResult::Unimplemented
        );
    }

    #[test]
    fn compute_sample_size_tighter_and_larger_loops() {
        // Simulate JPEG-like discrete scaled dimensions: 1/1=100, 1/2=50, 1/4=25, 1/8=13, else 13
        let orig = ISize { width: 100, height: 100 };
        let jpeg_dims = |s: i32| -> ISize {
            let d = match s {
                1 => 100,
                2 => 50,
                3 => 34,
                4 => 25,
                _ => (100 / s).max(1),
            };
            ISize { width: d, height: d }
        };

        // Request 34x34: initial sampleSize = 100/34 = 2 -> computed=50 > 34 -> tighter fit finds 3!
        let mut d1 = ISize { width: 34, height: 34 };
        assert_eq!(compute_sample_size(orig, &mut d1, false, || false, jpeg_dims), 3);
        assert_eq!(d1, ISize { width: 34, height: 34 });

        // Request 35x35: initial sampleSize = 2 -> computed=50 > 35; sampleSize+1=3 gives 34 < 35 -> stops at 2!
        let mut d2 = ISize { width: 35, height: 35 };
        assert_eq!(compute_sample_size(orig, &mut d2, false, || false, jpeg_dims), 2);
        assert_eq!(d2, ISize { width: 50, height: 50 });

        // Bad input <= 0 gets clamped to 1
        let mut d3 = ISize { width: 0, height: -5 };
        let s3 = compute_sample_size(orig, &mut d3, false, || false, jpeg_dims);
        assert_eq!(s3, 100);
        assert_eq!(d3, ISize { width: 1, height: 1 });
    }
}
