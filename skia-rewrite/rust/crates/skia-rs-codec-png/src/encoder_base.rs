//! Safe Rust port of `src/encode/SkPngEncoderBase.cpp`:
//! `getTargetInfo` mapping of every `SkColorType`/`SkAlphaType`, row encoding validation,
//! and safe Rust scanline transformation kernels.

use skia_rs_codec_core::encoded_info::bits_per_pixel;
use skia_rs_codec_core::types::{EncodedAlpha, EncodedColor};
use skia_rs_color::image_info::{
    color_type_bytes_per_pixel, color_type_channel_flags, AlphaType, ColorType,
    COLOR_CHANNEL_FLAGS_GRAY_ALPHA, COLOR_CHANNEL_FLAGS_RG, COLOR_CHANNEL_FLAGS_RGB,
    COLOR_CHANNEL_FLAGS_RGBA, COLOR_CHANNEL_FLAG_ALPHA, COLOR_CHANNEL_FLAG_GRAY,
    COLOR_CHANNEL_FLAG_RED,
};

/// Exact port of `SkColorTypeNumChannels` in `src/core/SkImageInfoPriv.h`.
#[inline]
pub const fn color_type_num_channels(ct: ColorType) -> i32 {
    match color_type_channel_flags(ct) {
        COLOR_CHANNEL_FLAG_RED | COLOR_CHANNEL_FLAG_ALPHA | COLOR_CHANNEL_FLAG_GRAY => 1,
        COLOR_CHANNEL_FLAGS_GRAY_ALPHA | COLOR_CHANNEL_FLAGS_RG => 2,
        COLOR_CHANNEL_FLAGS_RGB => 3,
        COLOR_CHANNEL_FLAGS_RGBA => 4,
        _ => 0,
    }
}

/// Exact port of `SkColorTypeMaxBitsPerChannel` in `src/core/SkImageInfoPriv.h`.
#[inline]
pub const fn color_type_max_bits_per_channel(ct: ColorType) -> i32 {
    match ct {
        ColorType::Unknown => 0,
        ColorType::Argb4444 => 4,
        ColorType::Rgb565 => 6,
        ColorType::Alpha8
        | ColorType::Rgba8888
        | ColorType::Rgb888x
        | ColorType::Bgra8888
        | ColorType::Bgr888x
        | ColorType::Gray8
        | ColorType::R8G8Unorm
        | ColorType::Srgba8888
        | ColorType::R8Unorm => 8,
        ColorType::Rgba1010102
        | ColorType::Rgb101010x
        | ColorType::Bgra1010102
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6 => 10,
        ColorType::RgbaF16Norm
        | ColorType::A16Unorm
        | ColorType::A16Float
        | ColorType::R16Unorm
        | ColorType::R16Float
        | ColorType::R16G16Unorm
        | ColorType::R16G16B16A16Unorm
        | ColorType::RgbaF16
        | ColorType::RgbF16F16F16x
        | ColorType::R16G16Float => 16,
        ColorType::RgbaF32 => 32,
    }
}

/// Result of `get_target_info`, matching `SkPngEncoderBase::TargetInfo`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PngTargetInfoPlan {
    pub valid: bool,
    /// True when `src_ct == Alpha8` (encoded directly as GrayAlpha via `transform_scanline_a8_to_gray_alpha`,
    /// with `fSrcRowInfo = nullopt`, `fDstRowInfo = nullopt`).
    pub is_alpha8: bool,
    pub dst_encoded_color: EncodedColor,
    pub dst_encoded_alpha: EncodedAlpha,
    pub dst_bits_per_component: i32,
    pub dst_row_ct: ColorType,
    pub dst_row_at: AlphaType,
    pub dst_row_size: usize,
}

impl PngTargetInfoPlan {
    const INVALID: Self = Self {
        valid: false,
        is_alpha8: false,
        dst_encoded_color: EncodedColor::Rgba,
        dst_encoded_alpha: EncodedAlpha::Unpremul,
        dst_bits_per_component: 0,
        dst_row_ct: ColorType::Unknown,
        dst_row_at: AlphaType::Unknown,
        dst_row_size: 0,
    };
}

#[inline]
const fn encoded_alpha_for_color(color: EncodedColor) -> EncodedAlpha {
    if matches!(color, EncodedColor::Gray | EncodedColor::Rgb) {
        EncodedAlpha::Opaque
    } else {
        EncodedAlpha::Unpremul
    }
}

#[inline]
fn make_target_plan(
    width: i32,
    dst_color: EncodedColor,
    bpc: i32,
    dst_ct: ColorType,
    dst_at: AlphaType,
) -> PngTargetInfoPlan {
    let dst_row_size = if width <= 0 {
        0
    } else {
        (width as usize) * (color_type_bytes_per_pixel(dst_ct) as usize)
    };
    PngTargetInfoPlan {
        valid: true,
        is_alpha8: false,
        dst_encoded_color: dst_color,
        dst_encoded_alpha: encoded_alpha_for_color(dst_color),
        dst_bits_per_component: bpc,
        dst_row_ct: dst_ct,
        dst_row_at: dst_at,
        dst_row_size,
    }
}

#[inline]
fn make_alpha8_target_plan(width: i32) -> PngTargetInfoPlan {
    let bpp = bits_per_pixel(EncodedColor::GrayAlpha, 8) as usize;
    let bytes_per_pixel = bpp / 8; // 2
    let Ok(w) = usize::try_from(width) else {
        return PngTargetInfoPlan::INVALID;
    };
    let Some(dst_row_size) = w.checked_mul(bytes_per_pixel) else {
        return PngTargetInfoPlan::INVALID;
    };
    PngTargetInfoPlan {
        valid: true,
        is_alpha8: true,
        dst_encoded_color: EncodedColor::GrayAlpha,
        dst_encoded_alpha: EncodedAlpha::Unpremul,
        dst_bits_per_component: 8,
        dst_row_ct: ColorType::Unknown,
        dst_row_at: AlphaType::Unknown,
        dst_row_size,
    }
}

/// Exact port of `SkPngEncoderBase::getTargetInfo`.
pub fn get_target_info(
    width: i32,
    src_ct: ColorType,
    src_at: AlphaType,
) -> PngTargetInfoPlan {
    match color_type_num_channels(src_ct) {
        1 => {
            if color_type_channel_flags(src_ct) == COLOR_CHANNEL_FLAG_GRAY {
                return make_target_plan(width, EncodedColor::Gray, 8, src_ct, src_at);
            }
            if matches!(src_ct, ColorType::Alpha8) {
                return make_alpha8_target_plan(width);
            }
            PngTargetInfoPlan::INVALID
        }
        3 => {
            if matches!(src_at, AlphaType::Unknown) {
                return PngTargetInfoPlan::INVALID;
            }
            let max_bpc = color_type_max_bits_per_channel(src_ct);
            if max_bpc <= 8 {
                make_target_plan(
                    width,
                    EncodedColor::Rgba,
                    8,
                    ColorType::Rgb888x,
                    AlphaType::Opaque,
                )
            } else if max_bpc <= 32 {
                make_target_plan(
                    width,
                    EncodedColor::Rgba,
                    16,
                    ColorType::R16G16B16A16Unorm,
                    AlphaType::Opaque,
                )
            } else {
                PngTargetInfoPlan::INVALID
            }
        }
        4 => {
            if matches!(src_at, AlphaType::Unknown) {
                return PngTargetInfoPlan::INVALID;
            }
            let max_bpc = color_type_max_bits_per_channel(src_ct);
            if max_bpc <= 8 {
                if matches!(src_at, AlphaType::Opaque) {
                    make_target_plan(
                        width,
                        EncodedColor::Rgba,
                        8,
                        ColorType::Rgb888x,
                        AlphaType::Opaque,
                    )
                } else {
                    make_target_plan(
                        width,
                        EncodedColor::Rgba,
                        8,
                        ColorType::Rgba8888,
                        AlphaType::Unpremul,
                    )
                }
            } else if max_bpc <= 32 {
                let dst_at = if matches!(src_at, AlphaType::Opaque) {
                    AlphaType::Opaque
                } else {
                    AlphaType::Unpremul
                };
                make_target_plan(
                    width,
                    EncodedColor::Rgba,
                    16,
                    ColorType::R16G16B16A16Unorm,
                    dst_at,
                )
            } else {
                PngTargetInfoPlan::INVALID
            }
        }
        _ => PngTargetInfoPlan::INVALID,
    }
}

/// Exact port of `SkPngEncoderBase::onEncodeRows` argument validation.
#[inline]
pub const fn validate_encode_rows_args(width: i32, height: i32, num_rows: i32) -> bool {
    width != 0 && height != 0 && num_rows >= 0
}

/// Safe Rust kernel matching `transform_scanline_A8_to_GrayAlpha`.
///
/// Converts `width` 8-bit alpha pixels into `2 * width` GrayAlpha bytes (`[0, a]`).
pub fn transform_scanline_a8_to_gray_alpha(dst: &mut [u8], src: &[u8]) {
    let count = src.len().min(dst.len() / 2);
    for i in 0..count {
        dst[i * 2] = 0;
        dst[i * 2 + 1] = src[i];
    }
}

/// Safe Rust row kernel: byte-swaps 16-bit channels in-place (LE <-> BE).
pub fn swap_16bit_row_endianness(row: &mut [u8]) {
    for pair in row.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
}

/// Safe Rust row kernel: strips 4-byte RGBx8 pixels into 3-byte RGB8 pixels.
pub fn strip_rgbx8_to_rgb8(dst: &mut [u8], src: &[u8]) {
    let pixels = (src.len() / 4).min(dst.len() / 3);
    for i in 0..pixels {
        dst[i * 3] = src[i * 4];
        dst[i * 3 + 1] = src[i * 4 + 1];
        dst[i * 3 + 2] = src[i * 4 + 2];
    }
}

/// Safe Rust row kernel: converts 16-bit little-endian RGBA pixels to 16-bit big-endian RGB pixels.
pub fn strip_rgba16le_to_rgb16be(dst: &mut [u8], src: &[u8]) {
    let pixels = (src.len() / 8).min(dst.len() / 6);
    for i in 0..pixels {
        let s = &src[i * 8..i * 8 + 6];
        let d = &mut dst[i * 6..i * 6 + 6];
        d[0] = s[1];
        d[1] = s[0];
        d[2] = s[3];
        d[3] = s[2];
        d[4] = s[5];
        d[5] = s[4];
    }
}
