//! JPEG marker codes, segment signatures, and libjpeg-turbo constants.
//! Safe Rust port of `src/codec/SkJpegConstants.h`.

/// StartOfImage marker (first marker of all JPEG files).
pub const MARKER_START_OF_IMAGE: u8 = 0xD8;

/// EndOfImage marker (last marker of all JPEG files, excluding auxiliary data).
pub const MARKER_END_OF_IMAGE: u8 = 0xD9;

/// StartOfScan marker (header is all segments before the first StartOfScan).
pub const MARKER_START_OF_SCAN: u8 = 0xDA;

/// APP0 marker (metadata and auxiliary images live in APP1..APP15).
pub const MARKER_APP0: u8 = 0xE0;

/// Two-byte marker code (`0xFF`, marker).
pub const MARKER_CODE_SIZE: usize = 2;

/// Two-byte big-endian parameter length field (includes these 2 bytes).
pub const SEGMENT_PARAMETER_LENGTH_SIZE: usize = 2;

/// First 3 bytes of every valid JPEG file: `0xFF, 0xD8, 0xFF`.
pub const JPEG_SIG: [u8; 3] = [0xFF, MARKER_START_OF_IMAGE, 0xFF];

/// ICC profile segment marker (APP2) and signature.
pub const ICC_MARKER: u8 = MARKER_APP0 + 2;
pub const ICC_MARKER_HEADER_SIZE: usize = 14;
pub const ICC_MARKER_INDEX_SIZE: usize = 1;
pub const ICC_SIG: &[u8] = b"ICC_PROFILE\0";

/// XMP segment marker (APP1) and standard/extended signatures.
pub const XMP_MARKER: u8 = MARKER_APP0 + 1;
pub const XMP_STANDARD_SIG: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub const XMP_EXTENDED_SIG: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";

/// EXIF segment marker (APP1) and signature.
pub const EXIF_MARKER: u8 = MARKER_APP0 + 1;
pub const EXIF_SIG: &[u8] = b"Exif\0";

/// MPF segment marker (APP2) and signature.
pub const MPF_MARKER: u8 = MARKER_APP0 + 2;
pub const MPF_SIG: &[u8] = b"MPF\0";

/// ISO 21496-1 gainmap marker (APP2) and signature.
pub const ISO_GAINMAP_MARKER: u8 = MARKER_APP0 + 2;
pub const ISO_GAINMAP_SIG: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";

/// ISO 19566-5 JUMBF marker (APP11) and signature.
pub const JUMBF_MARKER: u8 = MARKER_APP0 + 11;
pub const JUMBF_SIG: &[u8] = b"JP";

/// JPEG DCT block size (libjpeg `DCTSIZE`).
pub const DCTSIZE: usize = 8;

/// Maximum progressive scans before aborting to prevent hangs (`SkJpegDecoderMgr.cpp`).
pub const MAX_PROGRESSIVE_SCANS: i32 = 100;

/// Maximum JPEG segment payload size (65535 - 2 length bytes).
pub const MAX_SEGMENT_PAYLOAD_SIZE: usize = 65533;

/// Mirrors libjpeg-turbo's `J_COLOR_SPACE` enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum JColorSpace {
    #[default]
    Unknown = 0,
    Grayscale = 1,
    Rgb = 2,
    YCbCr = 3,
    Cmyk = 4,
    Ycck = 5,
    ExtRgb = 6,
    ExtRgbx = 7,
    ExtBgr = 8,
    ExtBgrx = 9,
    ExtXbgr = 10,
    ExtXrgb = 11,
    ExtRgba = 12,
    ExtBgra = 13,
    ExtAbgr = 14,
    ExtArgb = 15,
    Rgb565 = 16,
}

impl JColorSpace {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::Grayscale),
            2 => Some(Self::Rgb),
            3 => Some(Self::YCbCr),
            4 => Some(Self::Cmyk),
            5 => Some(Self::Ycck),
            6 => Some(Self::ExtRgb),
            7 => Some(Self::ExtRgbx),
            8 => Some(Self::ExtBgr),
            9 => Some(Self::ExtBgrx),
            10 => Some(Self::ExtXbgr),
            11 => Some(Self::ExtXrgb),
            12 => Some(Self::ExtRgba),
            13 => Some(Self::ExtBgra),
            14 => Some(Self::ExtAbgr),
            15 => Some(Self::ExtArgb),
            16 => Some(Self::Rgb565),
            _ => None,
        }
    }
}
