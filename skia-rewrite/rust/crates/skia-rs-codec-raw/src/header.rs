//! TIFF/DNG header checks, EXIF orientation mapping, PIEX preview selection,
//! and `SkRawCodec` metadata / `SkEncodedInfo` construction.

use skia_rs_codec_core::types::{
    skcms_pixel_format, CodecResult, EncodedAlpha, EncodedColor, EncodedImageFormat, EncodedOrigin,
};

use crate::types::{
    PiexColorSpace, PiexDecisionKind, PiexError, PiexImageFormat, PiexPreviewPlan,
    RawEncodedInfoSpec, RawImageMeta, TIFF_HEADER_SIZE, TIFF_MAGIC,
};

/// Matches `SkRawDecoder::IsRaw`: RAW files may require inspecting >10 KiB to detect
/// (e.g. Sony ARW), so Skia always returns `true` and tries RAW last.
#[inline]
pub const fn is_raw_sniff() -> bool {
    true
}

/// Matches `SkCodecPriv::IsValidEndianMarker`:
/// - `b"II"` -> `Some(true)` (little endian)
/// - `b"MM"` -> `Some(false)` (big endian)
/// - anything else -> `None`
#[inline]
pub fn is_valid_endian_marker(header: &[u8]) -> Option<bool> {
    if header.len() < 2 {
        return None;
    }
    match (header[0], header[1]) {
        (b'I', b'I') => Some(true),
        (b'M', b'M') => Some(false),
        _ => None,
    }
}

/// Matches `SkCodecPriv::GetEndianShort`.
#[inline]
pub fn get_endian_short(bytes: &[u8], little_endian: bool) -> Option<u16> {
    if bytes.len() < 2 {
        return None;
    }
    Some(if little_endian {
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_be_bytes([bytes[0], bytes[1]])
    })
}

/// Exact port of `SkDngImage::IsTiffHeaderValid`:
/// verifies a 4-byte TIFF header (`II` or `MM` followed by endian-matched magic `0x002A`).
#[inline]
pub fn is_tiff_header_valid(header: &[u8]) -> bool {
    if header.len() < TIFF_HEADER_SIZE {
        return false;
    }
    let Some(little_endian) = is_valid_endian_marker(header) else {
        return false;
    };
    matches!(get_endian_short(&header[2..4], little_endian), Some(TIFF_MAGIC))
}

/// Maps a raw EXIF orientation tag value (`1..=8`) to [`EncodedOrigin`], falling back to
/// `EncodedOrigin::TopLeft` (`kDefault_SkEncodedOrigin`) for out-of-range values.
///
/// Exact match to `SkRawCodec::MakeFromStream`.
#[inline]
pub const fn exif_orientation_to_origin(exif_orientation: u32) -> EncodedOrigin {
    if exif_orientation >= EncodedOrigin::TopLeft as u32
        && exif_orientation <= EncodedOrigin::LeftBottom as u32
    {
        match EncodedOrigin::from_i32(exif_orientation as i32) {
            Some(origin) => origin,
            None => EncodedOrigin::TopLeft,
        }
    } else {
        EncodedOrigin::TopLeft
    }
}

/// Evaluates the result of `::piex::GetPreviewImageData` and chooses whether to fail,
/// delegate an embedded JPEG preview to `SkJpegCodec`, or fall back to DNG decoding.
///
/// Exact match to `SkRawCodec::MakeFromStream`.
pub const fn evaluate_piex_preview(
    error: PiexError,
    color_space: PiexColorSpace,
    preview_offset: u32,
    preview_length: u32,
    preview_format: PiexImageFormat,
    exif_orientation: u32,
) -> PiexPreviewPlan {
    if matches!(error, PiexError::Fail) {
        return PiexPreviewPlan {
            decision: PiexDecisionKind::Fail as i32,
            result: CodecResult::InvalidInput as i32,
            preview_offset: 0,
            preview_length: 0,
            use_adobe_rgb: false,
            origin: EncodedOrigin::TopLeft as i32,
        };
    }

    let use_adobe_rgb = matches!(color_space, PiexColorSpace::AdobeRgb);

    if matches!(error, PiexError::Ok)
        && preview_length > 0
        && matches!(preview_format, PiexImageFormat::JpegCompressed)
    {
        return PiexPreviewPlan {
            decision: PiexDecisionKind::UseJpegPreview as i32,
            result: CodecResult::Success as i32,
            preview_offset,
            preview_length,
            use_adobe_rgb,
            origin: exif_orientation_to_origin(exif_orientation) as i32,
        };
    }

    PiexPreviewPlan {
        decision: PiexDecisionKind::FallbackToDng as i32,
        result: CodecResult::Success as i32,
        preview_offset: 0,
        preview_length: 0,
        use_adobe_rgb: false,
        origin: EncodedOrigin::TopLeft as i32,
    }
}

/// Exact port of `SkDngImage::init(width, height, cfaPatternSize)`:
/// - The DNG SDK scales only during demosaicing, so scaling is only possible when
///   mosaic info is present (`cfa_pattern_v != 0 && cfa_pattern_h != 0`).
/// - X-Trans sensor arrays have a 6x6 CFA pattern (`is_scalable && v == 6 && h == 6`).
/// - Valid iff `width > 0 && height > 0`.
#[inline]
pub const fn init_raw_image_meta(
    width: i32,
    height: i32,
    cfa_pattern_v: i32,
    cfa_pattern_h: i32,
) -> RawImageMeta {
    let is_scalable = cfa_pattern_v != 0 && cfa_pattern_h != 0;
    let is_xtrans_image = is_scalable && cfa_pattern_v == 6 && cfa_pattern_h == 6;
    let valid = width > 0 && height > 0;
    RawImageMeta {
        width,
        height,
        is_scalable,
        is_xtrans_image,
        valid,
    }
}

/// Exact `SkEncodedInfo` and `SkCodec` constructor specification for `SkRawCodec`.
#[inline]
pub const fn encoded_info_spec(width: i32, height: i32) -> RawEncodedInfoSpec {
    RawEncodedInfoSpec {
        width,
        height,
        encoded_color: EncodedColor::Rgb as i32,
        encoded_alpha: EncodedAlpha::Opaque as i32,
        bits_per_component: 8,
        codec_xform_format: skcms_pixel_format::RGBA_8888,
        encoded_format: EncodedImageFormat::Dng as i32,
        uses_color_xform: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiff_header_little_and_big_endian() {
        assert!(is_tiff_header_valid(&[b'I', b'I', 0x2A, 0x00]));
        assert!(is_tiff_header_valid(&[b'M', b'M', 0x00, 0x2A]));
        assert!(is_tiff_header_valid(&[b'I', b'I', 0x2A, 0x00, 0x08, 0x00]));

        // Swapped magic for endianness must fail.
        assert!(!is_tiff_header_valid(&[b'I', b'I', 0x00, 0x2A]));
        assert!(!is_tiff_header_valid(&[b'M', b'M', 0x2A, 0x00]));

        // Bad endian marker / short / empty / BigTIFF (43) must fail.
        assert!(!is_tiff_header_valid(&[b'I', b'M', 0x2A, 0x00]));
        assert!(!is_tiff_header_valid(&[b'I', b'I', 0x2B, 0x00]));
        assert!(!is_tiff_header_valid(&[b'I', b'I', 0x2A]));
        assert!(!is_tiff_header_valid(&[]));
        assert!(!is_tiff_header_valid(&[0xFF, 0xD8, 0xFF, 0xE0]));
    }

    #[test]
    fn exif_orientation_mapping() {
        assert_eq!(exif_orientation_to_origin(0), EncodedOrigin::TopLeft);
        assert_eq!(exif_orientation_to_origin(1), EncodedOrigin::TopLeft);
        assert_eq!(exif_orientation_to_origin(2), EncodedOrigin::TopRight);
        assert_eq!(exif_orientation_to_origin(3), EncodedOrigin::BottomRight);
        assert_eq!(exif_orientation_to_origin(4), EncodedOrigin::BottomLeft);
        assert_eq!(exif_orientation_to_origin(5), EncodedOrigin::LeftTop);
        assert_eq!(exif_orientation_to_origin(6), EncodedOrigin::RightTop);
        assert_eq!(exif_orientation_to_origin(7), EncodedOrigin::RightBottom);
        assert_eq!(exif_orientation_to_origin(8), EncodedOrigin::LeftBottom);
        assert_eq!(exif_orientation_to_origin(9), EncodedOrigin::TopLeft);
        assert_eq!(exif_orientation_to_origin(u32::MAX), EncodedOrigin::TopLeft);
    }

    #[test]
    fn piex_preview_selection_rules() {
        // kFail -> Fail(InvalidInput) regardless of preview fields.
        let p = evaluate_piex_preview(
            PiexError::Fail,
            PiexColorSpace::AdobeRgb,
            100,
            5000,
            PiexImageFormat::JpegCompressed,
            6,
        );
        assert_eq!(p.decision, PiexDecisionKind::Fail as i32);
        assert_eq!(p.result, CodecResult::InvalidInput as i32);

        // kOk + JPEG + non-zero length -> UseJpegPreview with AdobeRGB & mapped origin.
        let p = evaluate_piex_preview(
            PiexError::Ok,
            PiexColorSpace::AdobeRgb,
            128,
            4096,
            PiexImageFormat::JpegCompressed,
            6,
        );
        assert_eq!(p.decision, PiexDecisionKind::UseJpegPreview as i32);
        assert_eq!(p.preview_offset, 128);
        assert_eq!(p.preview_length, 4096);
        assert!(p.use_adobe_rgb);
        assert_eq!(p.origin, EncodedOrigin::RightTop as i32);

        // kOk + sRGB + out-of-range orientation -> clamps origin to TopLeft.
        let p = evaluate_piex_preview(
            PiexError::Ok,
            PiexColorSpace::Srgb,
            64,
            1024,
            PiexImageFormat::JpegCompressed,
            99,
        );
        assert_eq!(p.decision, PiexDecisionKind::UseJpegPreview as i32);
        assert!(!p.use_adobe_rgb);
        assert_eq!(p.origin, EncodedOrigin::TopLeft as i32);

        // kOk + zero length -> FallbackToDng.
        let p = evaluate_piex_preview(
            PiexError::Ok,
            PiexColorSpace::Srgb,
            64,
            0,
            PiexImageFormat::JpegCompressed,
            1,
        );
        assert_eq!(p.decision, PiexDecisionKind::FallbackToDng as i32);

        // kOk + UncompressedRgb -> FallbackToDng (SkRawCodec only handles JPEG previews).
        let p = evaluate_piex_preview(
            PiexError::Ok,
            PiexColorSpace::Srgb,
            64,
            2048,
            PiexImageFormat::UncompressedRgb,
            3,
        );
        assert_eq!(p.decision, PiexDecisionKind::FallbackToDng as i32);

        // kUnsupported -> FallbackToDng.
        let p = evaluate_piex_preview(
            PiexError::Unsupported,
            PiexColorSpace::Srgb,
            64,
            2048,
            PiexImageFormat::JpegCompressed,
            3,
        );
        assert_eq!(p.decision, PiexDecisionKind::FallbackToDng as i32);
    }

    #[test]
    fn raw_image_meta_and_encoded_info() {
        // Bayer 2x2 mosaic -> scalable, not X-Trans.
        let m = init_raw_image_meta(4000, 3000, 2, 2);
        assert!(m.valid);
        assert!(m.is_scalable);
        assert!(!m.is_xtrans_image);

        // Fujifilm X-Trans 6x6 mosaic -> scalable & X-Trans.
        let m = init_raw_image_meta(4896, 3264, 6, 6);
        assert!(m.valid);
        assert!(m.is_scalable);
        assert!(m.is_xtrans_image);

        // Linear DNG (no CFA mosaic) -> not scalable, not X-Trans.
        let m = init_raw_image_meta(4000, 3000, 0, 0);
        assert!(m.valid);
        assert!(!m.is_scalable);
        assert!(!m.is_xtrans_image);

        // Partial zero CFA dim -> not scalable.
        let m = init_raw_image_meta(4000, 3000, 6, 0);
        assert!(m.valid);
        assert!(!m.is_scalable);
        assert!(!m.is_xtrans_image);

        // Invalid dimensions.
        assert!(!init_raw_image_meta(0, 3000, 2, 2).valid);
        assert!(!init_raw_image_meta(4000, 0, 2, 2).valid);
        assert!(!init_raw_image_meta(-10, 100, 2, 2).valid);

        let spec = encoded_info_spec(4000, 3000);
        assert_eq!(spec.width, 4000);
        assert_eq!(spec.height, 3000);
        assert_eq!(spec.encoded_color, EncodedColor::Rgb as i32);
        assert_eq!(spec.encoded_alpha, EncodedAlpha::Opaque as i32);
        assert_eq!(spec.bits_per_component, 8);
        assert_eq!(spec.codec_xform_format, skcms_pixel_format::RGBA_8888);
        assert_eq!(spec.encoded_format, EncodedImageFormat::Dng as i32);
        assert!(!spec.uses_color_xform);
        assert!(is_raw_sniff());
    }
}
