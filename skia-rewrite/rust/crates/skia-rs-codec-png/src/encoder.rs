//! Safe Rust port of `src/encode/SkPngEncoderImpl.cpp`:
//! PNG IHDR/sBIT/filter/zlib/comment/colorspace/HDR/gainmap/writeInfo planning and validation.

use skia_rs_codec_core::types::EncodedColor;
use skia_rs_color::image_info::{color_type_bytes_per_pixel, AlphaType, ColorType};

use crate::decoder::{
    GRAY_SIG_BIT_GRAY_ALPHA_IS_JUST_ALPHA, PNG_COLOR_TYPE_GRAY, PNG_COLOR_TYPE_GRAY_ALPHA,
    PNG_COLOR_TYPE_RGB, PNG_COLOR_TYPE_RGBA,
};

/// Matches `PNG_FILTER_NONE` / `SkPngEncoder::FilterFlag::kNone`.
pub const PNG_FILTER_NONE: i32 = 0x08;
/// Matches `PNG_FILTER_SUB` / `SkPngEncoder::FilterFlag::kSub`.
pub const PNG_FILTER_SUB: i32 = 0x10;
/// Matches `PNG_FILTER_UP` / `SkPngEncoder::FilterFlag::kUp`.
pub const PNG_FILTER_UP: i32 = 0x20;
/// Matches `PNG_FILTER_AVG` / `SkPngEncoder::FilterFlag::kAvg`.
pub const PNG_FILTER_AVG: i32 = 0x40;
/// Matches `PNG_FILTER_PAETH` / `SkPngEncoder::FilterFlag::kPaeth`.
pub const PNG_FILTER_PAETH: i32 = 0x80;
/// Matches `PNG_ALL_FILTERS` / `SkPngEncoder::FilterFlag::kAll`.
pub const PNG_ALL_FILTERS: i32 =
    PNG_FILTER_NONE | PNG_FILTER_SUB | PNG_FILTER_UP | PNG_FILTER_AVG | PNG_FILTER_PAETH;

/// Matches `PNG_KEYWORD_MAX_LENGTH` in `png.h`.
pub const PNG_KEYWORD_MAX_LENGTH: usize = 79;

/// Significant bits (`sBIT` chunk) planned for encoding.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PngSigBit {
    pub set: bool,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub gray: u8,
    pub alpha: u8,
}

/// Complete encoding header & row-transform plan matching `SkPngEncoderMgr::setHeader` + `writeInfo`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PngEncodeHeaderPlan {
    pub valid: bool,
    pub png_color_type: i32,
    pub sig_bit: PngSigBit,
    pub filters: i32,
    pub filters_exact: bool,
    pub zlib_level: i32,
    pub zlib_level_exact: bool,
    /// True when `writeInfo` must call `png_set_filler(ptr, 0, PNG_FILLER_AFTER)` to strip
    /// the unused 4th channel from 4/8-byte RGBx/RGBA16 opaque rows.
    pub needs_filler_after: bool,
    /// True when `onEncodeRow` must call `png_set_swap` for 16-bit big-endian PNG output.
    pub needs_byte_swap: bool,
}

/// Computes PNG `sBIT` significant bits for `src_ct`, matching `SkPngEncoderMgr::setHeader`.
pub const fn plan_sig_bit(src_ct: ColorType) -> PngSigBit {
    match src_ct {
        ColorType::RgbaF16Norm | ColorType::RgbaF16 | ColorType::RgbaF32 => PngSigBit {
            set: true,
            red: 16,
            green: 16,
            blue: 16,
            gray: 0,
            alpha: 16,
        },
        ColorType::RgbF16F16F16x => PngSigBit {
            set: true,
            red: 16,
            green: 16,
            blue: 16,
            gray: 0,
            alpha: 0,
        },
        ColorType::Gray8 => PngSigBit {
            set: true,
            red: 0,
            green: 0,
            blue: 0,
            gray: 8,
            alpha: 0,
        },
        ColorType::Rgb888x | ColorType::Bgr888x => PngSigBit {
            set: true,
            red: 8,
            green: 8,
            blue: 8,
            gray: 0,
            alpha: 0,
        },
        ColorType::Argb4444 => PngSigBit {
            set: true,
            red: 4,
            green: 4,
            blue: 4,
            gray: 0,
            alpha: 4,
        },
        ColorType::Rgb565 => PngSigBit {
            set: true,
            red: 5,
            green: 6,
            blue: 5,
            gray: 0,
            alpha: 0,
        },
        ColorType::Alpha8 => PngSigBit {
            set: true,
            red: 0,
            green: 0,
            blue: 0,
            gray: GRAY_SIG_BIT_GRAY_ALPHA_IS_JUST_ALPHA,
            alpha: 8,
        },
        ColorType::Rgba1010102 | ColorType::Bgra1010102 => PngSigBit {
            set: true,
            red: 10,
            green: 10,
            blue: 10,
            gray: 0,
            alpha: 2,
        },
        ColorType::Bgr101010xXr | ColorType::Rgb101010x | ColorType::Bgr101010x => PngSigBit {
            set: true,
            red: 10,
            green: 10,
            blue: 10,
            gray: 0,
            alpha: 0,
        },
        ColorType::Bgra10101010Xr => PngSigBit {
            set: true,
            red: 10,
            green: 10,
            blue: 10,
            gray: 0,
            alpha: 10,
        },
        ColorType::Rgba8888 | ColorType::Bgra8888 => PngSigBit {
            set: true,
            red: 8,
            green: 8,
            blue: 8,
            gray: 0,
            alpha: 8,
        },
        _ => PngSigBit {
            set: false,
            red: 0,
            green: 0,
            blue: 0,
            gray: 0,
            alpha: 0,
        },
    }
}

/// Exact port of `SkPngEncoderMgr::setHeader` and `SkPngEncoderMgr::writeInfo` planning.
pub const fn plan_encode_header(
    dst_color: EncodedColor,
    dst_bpc: i32,
    dst_row_is_opaque: bool,
    src_ct: ColorType,
    filter_flags: i32,
    zlib_level: i32,
) -> PngEncodeHeaderPlan {
    let png_color_type = match dst_color {
        EncodedColor::Rgb => PNG_COLOR_TYPE_RGB,
        EncodedColor::Rgba => {
            if dst_row_is_opaque {
                PNG_COLOR_TYPE_RGB
            } else {
                PNG_COLOR_TYPE_RGBA
            }
        }
        EncodedColor::Gray => PNG_COLOR_TYPE_GRAY,
        EncodedColor::GrayAlpha => PNG_COLOR_TYPE_GRAY_ALPHA,
        _ => {
            return PngEncodeHeaderPlan {
                valid: false,
                png_color_type: 0,
                sig_bit: PngSigBit {
                    set: false,
                    red: 0,
                    green: 0,
                    blue: 0,
                    gray: 0,
                    alpha: 0,
                },
                filters: 0,
                filters_exact: false,
                zlib_level: 0,
                zlib_level_exact: false,
                needs_filler_after: false,
                needs_byte_swap: false,
            };
        }
    };

    let sig_bit = plan_sig_bit(src_ct);
    let filters = filter_flags & PNG_ALL_FILTERS;
    let clamped_zlib = if zlib_level < 0 {
        0
    } else if zlib_level > 9 {
        9
    } else {
        zlib_level
    };

    PngEncodeHeaderPlan {
        valid: true,
        png_color_type,
        sig_bit,
        filters,
        filters_exact: filters == filter_flags,
        zlib_level: clamped_zlib,
        zlib_level_exact: clamped_zlib == zlib_level,
        needs_filler_after: matches!(dst_color, EncodedColor::Rgba) && dst_row_is_opaque,
        needs_byte_swap: dst_bpc == 16,
    }
}

/// Exact port of `SkPngEncoderMgr::setHeader` tEXt comments count validation.
#[inline]
pub const fn validate_comments_count(count: i32) -> bool {
    count >= 0 && (count % 2) == 0
}

/// Clips a PNG `tEXt` keyword length to `PNG_KEYWORD_MAX_LENGTH` (79).
#[inline]
pub const fn clip_text_keyword_len(keyword_len: usize) -> usize {
    if keyword_len <= PNG_KEYWORD_MAX_LENGTH {
        keyword_len
    } else {
        PNG_KEYWORD_MAX_LENGTH
    }
}

/// Color space encoding decision for `SkPngEncoderMgr::setColorSpace`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PngColorSpaceEncodeAction {
    /// Emit `sRGB` chunk (`PNG_sRGB_INTENT_PERCEPTUAL`).
    WriteSrgb = 0,
    /// Emit `iCCP` chunk via `SkWriteICCProfile(info.colorSpace(), options.fHdrMetadata)`.
    WriteIcc = 1,
}

/// Exact port of `SkPngEncoderMgr::setColorSpace` policy.
#[inline]
pub const fn plan_encode_color_space(
    has_color_space: bool,
    color_space_is_srgb: bool,
    has_hdr_metadata: bool,
) -> PngColorSpaceEncodeAction {
    if has_color_space && color_space_is_srgb && !has_hdr_metadata {
        PngColorSpaceEncodeAction::WriteSrgb
    } else {
        PngColorSpaceEncodeAction::WriteIcc
    }
}

/// Gainmap encoding mode for `SkPngEncoderMgr::setHdrMetadata`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PngGainmapEncodeMode {
    /// No gainmap metadata to encode.
    None = 0,
    /// Base image with gainmap pixels & info: emit `gmAP` version + `gdAT` encoded PNG gainmap.
    BaseWithGainmap = 1,
    /// Recursive gainmap image pass (info only, no nested pixels): emit `gmAP` serialized info.
    GainmapInfoOnly = 2,
}

/// Exact port of `SkPngEncoderMgr::setHdrMetadata` gainmap branch selection.
#[inline]
pub const fn plan_encode_gainmap(
    has_gainmap_info: bool,
    has_gainmap_pixels: bool,
) -> PngGainmapEncodeMode {
    if has_gainmap_info && has_gainmap_pixels {
        PngGainmapEncodeMode::BaseWithGainmap
    } else if has_gainmap_info {
        PngGainmapEncodeMode::GainmapInfoOnly
    } else {
        PngGainmapEncodeMode::None
    }
}

/// Returns true if the gainmap's encoded PNG color format supports an RGB ICC profile.
/// Grayscale PNGs (`Gray`, `GrayAlpha`) do not support RGB ICC profiles and must scrub
/// `fGainmapMathColorSpace`.
#[inline]
pub const fn gainmap_supports_rgb_icc(dst_color: EncodedColor) -> bool {
    !matches!(dst_color, EncodedColor::Gray | EncodedColor::GrayAlpha)
}

/// Exact port of `SkPixmapIsValid` used by `SkPngEncoder::Make`.
#[inline]
pub fn validate_pixmap_for_encode(
    width: i32,
    height: i32,
    ct: ColorType,
    at: AlphaType,
    has_addr: bool,
    row_bytes: usize,
) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    const MAX_DIM: i32 = i32::MAX >> 2;
    if width > MAX_DIM || height > MAX_DIM {
        return false;
    }
    if matches!(ct, ColorType::Unknown) || matches!(at, AlphaType::Unknown) {
        return false;
    }
    if !has_addr {
        return false;
    }
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let Some(min_row_bytes) = (width as usize).checked_mul(bpp) else {
        return false;
    };
    row_bytes >= min_row_bytes
}
