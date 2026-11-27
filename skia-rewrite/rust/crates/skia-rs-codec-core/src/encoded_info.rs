//! Safe Rust port of `src/codec/SkEncodedInfo.cpp` and `include/private/SkEncodedInfo.h` helpers.

use skia_rs_color::image_info::{AlphaType, ColorType};

use crate::types::{EncodedAlpha, EncodedColor};

/// Recommended `(ColorType, AlphaType)` matching `SkEncodedInfo::makeImageInfo()`.
#[inline]
pub const fn make_image_info_types(
    color: EncodedColor,
    alpha: EncodedAlpha,
    pmcolor_is_bgra: bool,
) -> (ColorType, AlphaType) {
    let ct = match color {
        EncodedColor::Gray => ColorType::Gray8,
        EncodedColor::XAlpha => ColorType::Alpha8,
        EncodedColor::Rgb565 => ColorType::Rgb565,
        _ => {
            if pmcolor_is_bgra {
                ColorType::Bgra8888
            } else {
                ColorType::Rgba8888
            }
        }
    };
    let at = match alpha {
        EncodedAlpha::Opaque => AlphaType::Opaque,
        EncodedAlpha::Unpremul | EncodedAlpha::Binary => AlphaType::Unpremul,
    };
    (ct, at)
}

/// Exact port of `SkEncodedInfo::bitsPerPixel()`.
#[inline]
pub const fn bits_per_pixel(color: EncodedColor, bits_per_component: u8) -> u8 {
    match color {
        EncodedColor::Gray | EncodedColor::Palette => bits_per_component,
        EncodedColor::XAlpha | EncodedColor::GrayAlpha => 2 * bits_per_component,
        EncodedColor::Rgb | EncodedColor::Bgr | EncodedColor::Yuv | EncodedColor::Rgb565 => {
            3 * bits_per_component
        }
        EncodedColor::Rgba
        | EncodedColor::Bgra
        | EncodedColor::Bgrx
        | EncodedColor::Yuva
        | EncodedColor::InvertedCmyk
        | EncodedColor::Ycck => 4 * bits_per_component,
    }
}

/// Exact port of `SkEncodedInfo::VerifyColor` + `bitsPerComponent` validation in `SkEncodedInfo::Make`.
#[inline]
pub const fn verify_color(color: EncodedColor, alpha: EncodedAlpha, bits_per_component: i32) -> bool {
    let valid_bpc = matches!(bits_per_component, 1 | 2 | 4 | 8 | 16);
    if !valid_bpc {
        return false;
    }
    match color {
        EncodedColor::Gray => matches!(alpha, EncodedAlpha::Opaque),
        EncodedColor::GrayAlpha => !matches!(alpha, EncodedAlpha::Opaque),
        EncodedColor::Palette => bits_per_component != 16,
        EncodedColor::Rgb | EncodedColor::Bgr | EncodedColor::Bgrx => {
            matches!(alpha, EncodedAlpha::Opaque) && bits_per_component >= 8
        }
        EncodedColor::Yuv | EncodedColor::InvertedCmyk | EncodedColor::Ycck => {
            matches!(alpha, EncodedAlpha::Opaque) && bits_per_component == 8
        }
        EncodedColor::Rgba => bits_per_component >= 8,
        EncodedColor::Bgra | EncodedColor::Yuva => bits_per_component == 8,
        EncodedColor::XAlpha => matches!(alpha, EncodedAlpha::Unpremul) && bits_per_component == 8,
        EncodedColor::Rgb565 => matches!(alpha, EncodedAlpha::Opaque) && bits_per_component == 8,
    }
}
