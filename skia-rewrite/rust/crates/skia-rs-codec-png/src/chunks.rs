//! PNG signature check, chunk header parsing/encoding, unknown chunk classification
//! (gainmap `gmAP`/`gdAT`, HDR `cLLI`/`mDCV`), and color profile planning (`iCCP`/`sRGB`/`cHRM`/`gAMA`).

use skia_rs_codec_core::types::EncodedColor;
use skia_rs_codec_meta::color_profile::DataSpace;
use skia_rs_color::color_space::{
    ColorSpacePrimaries, Matrix3x3, TransferFunction, GAMUT_SRGB, TF_SRGB,
};

/// Standard 8-byte PNG file signature.
pub const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Exact port of `SkPngCodecBase::IsPng`.
#[inline]
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= PNG_SIGNATURE.len() && bytes[..PNG_SIGNATURE.len()] == PNG_SIGNATURE
}

/// Parsed 8-byte PNG chunk header (big-endian payload length + 4-byte ASCII type tag).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PngChunkHeader {
    pub length: u32,
    pub tag: [u8; 4],
}

impl PngChunkHeader {
    /// Returns true if this chunk's 4-byte tag equals `tag`.
    #[inline]
    pub const fn is_tag(&self, tag: &[u8; 4]) -> bool {
        self.tag[0] == tag[0]
            && self.tag[1] == tag[1]
            && self.tag[2] == tag[2]
            && self.tag[3] == tag[3]
    }

    /// Returns true if this chunk is `IDAT`.
    #[inline]
    pub const fn is_idat(&self) -> bool {
        self.is_tag(b"IDAT")
    }

    /// Returns true if this chunk is `IEND`.
    #[inline]
    pub const fn is_iend(&self) -> bool {
        self.is_tag(b"IEND")
    }

    /// Returns `length + 4` (payload + 4-byte CRC), checking for overflow.
    #[inline]
    pub const fn payload_plus_crc_len(&self) -> Option<usize> {
        (self.length as usize).checked_add(4)
    }
}

/// Parses an 8-byte PNG chunk header (`[len_be: 4][tag: 4]`).
#[inline]
pub fn parse_chunk_header(bytes: &[u8]) -> Option<PngChunkHeader> {
    if bytes.len() < 8 {
        return None;
    }
    let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let tag = [bytes[4], bytes[5], bytes[6], bytes[7]];
    Some(PngChunkHeader { length, tag })
}

/// Encodes an 8-byte PNG chunk header (`[len_be: 4][tag: 4]`).
#[inline]
pub const fn encode_chunk_header(length: u32, tag: [u8; 4]) -> [u8; 8] {
    let b = length.to_be_bytes();
    [b[0], b[1], b[2], b[3], tag[0], tag[1], tag[2], tag[3]]
}

/// Encodes an 8-byte `IDAT` chunk header for feeding back into libpng progressive reader.
#[inline]
pub const fn encode_idat_header(length: u32) -> [u8; 8] {
    encode_chunk_header(length, *b"IDAT")
}

/// Classification of unknown chunks handled by `SkPngCompositeChunkReader`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnknownChunkKind {
    Ignored = 0,
    GainmapInfo = 1,
    GainmapData = 2,
    ContentLightLevelInfo = 3,
    MasteringDisplayColorVolume = 4,
}

/// Exact port of `SkPngCompositeChunkReader::readChunk` chunk-tag dispatch.
///
/// `tag` is the chunk name bytes (typically 4 bytes, or a C string slice before NUL).
/// Returns `UnknownChunkKind::Ignored` if `data_len == 0` or the tag is not one of
/// `gmAP`, `gdAT`, `cLLI`, or `mDCV`.
#[inline]
pub fn classify_unknown_chunk(tag: &[u8], data_len: usize) -> UnknownChunkKind {
    if data_len == 0 {
        return UnknownChunkKind::Ignored;
    }
    match tag {
        b"gmAP" => UnknownChunkKind::GainmapInfo,
        b"gdAT" => UnknownChunkKind::GainmapData,
        b"cLLI" => UnknownChunkKind::ContentLightLevelInfo,
        b"mDCV" => UnknownChunkKind::MasteringDisplayColorVolume,
        _ => UnknownChunkKind::Ignored,
    }
}

/// Exact port of `SkPngCodecBase::isCompatibleColorProfileAndType`.
///
/// `data_space` is `None` when no color profile is present.
#[inline]
pub const fn is_compatible_color_profile_and_type(
    data_space: Option<DataSpace>,
    color: EncodedColor,
) -> bool {
    match data_space {
        Some(DataSpace::Cmyk) => false,
        Some(DataSpace::Gray) => matches!(color, EncodedColor::Gray | EncodedColor::GrayAlpha),
        Some(DataSpace::Rgb | DataSpace::Other) | None => true,
    }
}

/// Exact port of `png_fixed_point_to_float` in `SkPngCodec.cpp`.
#[inline]
pub fn png_fixed_point_to_float(x: i32) -> f32 {
    (x as f32) * 0.00001_f32
}

/// Exact port of `png_inverted_fixed_point_to_float` in `SkPngCodec.cpp`.
/// Used because PNG `gAMA` stores `1 / gamma` in fixed-point x100000.
#[inline]
pub fn png_inverted_fixed_point_to_float(x: i32) -> f32 {
    1.0_f32 / png_fixed_point_to_float(x)
}

/// Result action of `plan_color_profile`, matching `read_color_profile` in `SkPngCodec.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PngColorProfileKind {
    /// Use the decompressed `iCCP` ICC profile bytes.
    UseIcc = 0,
    /// `sRGB` chunk present (and no `iCCP`): return `nullptr` (default sRGB).
    UseSrgbNull = 1,
    /// Construct `SkCodecs::ColorProfile::Make(transfer_fn, to_xyzd50)` from `cHRM`/`gAMA` (or sRGB defaults).
    UseCustomTfAndMatrix = 2,
}

/// Planned color profile output for `read_color_profile`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PngColorProfilePlan {
    pub kind: PngColorProfileKind,
    pub transfer_fn: TransferFunction,
    pub to_xyzd50: Matrix3x3,
}

/// Exact port of `read_color_profile` priority and math in `SkPngCodec.cpp`.
///
/// Priority order:
/// 1. `iCCP` -> `UseIcc`
/// 2. `sRGB` -> `UseSrgbNull` (Skia returns `nullptr` for sRGB)
/// 3. `cHRM` + `gAMA` (with sRGB gamut / sRGB transfer function fallbacks) -> `UseCustomTfAndMatrix`
///
/// `chrm_fixed` order matches `png_get_cHRM_fixed`:
/// `[white_x, white_y, red_x, red_y, green_x, green_y, blue_x, blue_y]`.
pub fn plan_color_profile(
    has_iccp: bool,
    has_srgb: bool,
    has_chrm: bool,
    chrm_fixed: &[i32; 8],
    has_gama: bool,
    gama_fixed: i32,
) -> PngColorProfilePlan {
    if has_iccp {
        return PngColorProfilePlan {
            kind: PngColorProfileKind::UseIcc,
            transfer_fn: TF_SRGB,
            to_xyzd50: GAMUT_SRGB,
        };
    }

    if has_srgb {
        return PngColorProfilePlan {
            kind: PngColorProfileKind::UseSrgbNull,
            transfer_fn: TF_SRGB,
            to_xyzd50: GAMUT_SRGB,
        };
    }

    let mut to_xyzd50 = GAMUT_SRGB;
    if has_chrm {
        let primaries = ColorSpacePrimaries {
            wx: png_fixed_point_to_float(chrm_fixed[0]),
            wy: png_fixed_point_to_float(chrm_fixed[1]),
            rx: png_fixed_point_to_float(chrm_fixed[2]),
            ry: png_fixed_point_to_float(chrm_fixed[3]),
            gx: png_fixed_point_to_float(chrm_fixed[4]),
            gy: png_fixed_point_to_float(chrm_fixed[5]),
            bx: png_fixed_point_to_float(chrm_fixed[6]),
            by: png_fixed_point_to_float(chrm_fixed[7]),
        };
        if let Some(mat) = primaries.to_xyzd50() {
            to_xyzd50 = mat;
        }
    }

    let transfer_fn = if has_gama {
        TransferFunction {
            g: png_inverted_fixed_point_to_float(gama_fixed),
            a: 1.0_f32,
            b: 0.0_f32,
            c: 0.0_f32,
            d: 0.0_f32,
            e: 0.0_f32,
            f: 0.0_f32,
        }
    } else {
        TF_SRGB
    };

    PngColorProfilePlan {
        kind: PngColorProfileKind::UseCustomTfAndMatrix,
        transfer_fn,
        to_xyzd50,
    }
}
