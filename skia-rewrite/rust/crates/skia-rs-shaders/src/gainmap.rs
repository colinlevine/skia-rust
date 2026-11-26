//! Safe Rust port of `SkGainmapShader.cpp` luminance/HDR ratio & uniform math.

use skia_rs_color::color::Color4f;
use skia_rs_color::image_info::{color_type_channel_flags, ColorType};
use skia_rs_geometry::{Matrix, Rect};

use crate::rect_to_rect_or_identity;

// Channel flag constants matching `SkColorChannelFlag` in `include/core/SkColor.h`.
const RED_CHANNEL_FLAG: u32 = 1 << 0;
const ALPHA_CHANNEL_FLAG: u32 = 1 << 3;
const GRAY_CHANNEL_FLAG: u32 = 1 << 4;

/// Mirrors `SkGainmapInfo::BaseImageType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GainmapBaseImageType {
    Sdr = 0,
    Hdr = 1,
}

/// Mirrors `SkGainmapInfo::Type`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GainmapType {
    Default = 0,
    Apple = 1,
}

/// Flat POD input describing the numeric fields of `SkGainmapInfo`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GainmapInfoInput {
    pub gainmap_ratio_min: Color4f,
    pub gainmap_ratio_max: Color4f,
    pub gainmap_gamma: Color4f,
    pub epsilon_sdr: Color4f,
    pub epsilon_hdr: Color4f,
    pub display_ratio_sdr: f32,
    pub display_ratio_hdr: f32,
    pub base_image_type: GainmapBaseImageType,
    pub gainmap_type: GainmapType,
}

/// Computed uniform and transform parameters for `SkGainmapShader::Make`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct GainmapUniforms {
    pub base_rect_to_dst: Matrix,
    pub gainmap_rect_to_dst: Matrix,
    pub weight: f32,
    pub log_ratio_min: Color4f,
    pub log_ratio_max: Color4f,
    pub gainmap_gamma: Color4f,
    pub epsilon_base: Color4f,
    pub epsilon_other: Color4f,
    pub no_gamma: i32,
    pub single_channel: i32,
    pub gainmap_is_alpha: i32,
    pub gainmap_is_red: i32,
    pub is_apple: i32,
    pub apple_g: f32,
    pub apple_h: f32,
}

#[inline]
fn all_channels_equal(c: Color4f) -> bool {
    c.r == c.g && c.r == c.b
}

/// Computes all gainmap weight, log-ratio, channel-flag, and matrix uniforms
/// matching `SkGainmapShader::Make`.
pub fn compute_gainmap_uniforms(
    base_rect: Rect,
    gainmap_rect: Rect,
    dst_rect: Rect,
    gainmap_color_type: ColorType,
    info: &GainmapInfoInput,
    dst_hdr_ratio: f32,
) -> GainmapUniforms {
    let base_rect_to_dst = rect_to_rect_or_identity(base_rect, dst_rect);
    let gainmap_rect_to_dst = rect_to_rect_or_identity(gainmap_rect, dst_rect);

    let mut w = 0.0_f32;
    if dst_hdr_ratio > info.display_ratio_sdr {
        if dst_hdr_ratio < info.display_ratio_hdr {
            w = (dst_hdr_ratio.ln() - info.display_ratio_sdr.ln())
                / (info.display_ratio_hdr.ln() - info.display_ratio_sdr.ln());
        } else {
            w = 1.0_f32;
        }
    }

    let base_image_is_hdr = info.base_image_type == GainmapBaseImageType::Hdr;
    if base_image_is_hdr {
        w -= 1.0_f32;
    }

    let log_ratio_min = Color4f {
        r: info.gainmap_ratio_min.r.ln(),
        g: info.gainmap_ratio_min.g.ln(),
        b: info.gainmap_ratio_min.b.ln(),
        a: 1.0,
    };
    let log_ratio_max = Color4f {
        r: info.gainmap_ratio_max.r.ln(),
        g: info.gainmap_ratio_max.g.ln(),
        b: info.gainmap_ratio_max.b.ln(),
        a: 1.0,
    };

    let no_gamma = i32::from(
        info.gainmap_gamma.r == 1.0
            && info.gainmap_gamma.g == 1.0
            && info.gainmap_gamma.b == 1.0,
    );

    let color_type_flags = color_type_channel_flags(gainmap_color_type);
    let gainmap_is_alpha = i32::from(color_type_flags == ALPHA_CHANNEL_FLAG);
    let gainmap_is_red = i32::from(color_type_flags == RED_CHANNEL_FLAG);
    let single_channel = i32::from(
        all_channels_equal(info.gainmap_gamma)
            && all_channels_equal(info.gainmap_ratio_min)
            && all_channels_equal(info.gainmap_ratio_max)
            && (color_type_flags == GRAY_CHANNEL_FLAG
                || color_type_flags == ALPHA_CHANNEL_FLAG
                || color_type_flags == RED_CHANNEL_FLAG),
    );

    let (epsilon_base, epsilon_other) = if base_image_is_hdr {
        (info.epsilon_hdr, info.epsilon_sdr)
    } else {
        (info.epsilon_sdr, info.epsilon_hdr)
    };

    let is_apple = i32::from(info.gainmap_type == GainmapType::Apple);
    let apple_g = 1.961_f32;
    let apple_h = info.display_ratio_hdr;

    GainmapUniforms {
        base_rect_to_dst,
        gainmap_rect_to_dst,
        weight: w,
        log_ratio_min,
        log_ratio_max,
        gainmap_gamma: info.gainmap_gamma,
        epsilon_base,
        epsilon_other,
        no_gamma,
        single_channel,
        gainmap_is_alpha,
        gainmap_is_red,
        is_apple,
        apple_g,
        apple_h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_info() -> GainmapInfoInput {
        GainmapInfoInput {
            gainmap_ratio_min: Color4f::new(1.0, 1.0, 1.0, 1.0),
            gainmap_ratio_max: Color4f::new(4.0, 4.0, 4.0, 1.0),
            gainmap_gamma: Color4f::new(1.0, 1.0, 1.0, 1.0),
            epsilon_sdr: Color4f::new(0.01, 0.01, 0.01, 1.0),
            epsilon_hdr: Color4f::new(0.02, 0.02, 0.02, 1.0),
            display_ratio_sdr: 1.0,
            display_ratio_hdr: 4.0,
            base_image_type: GainmapBaseImageType::Sdr,
            gainmap_type: GainmapType::Default,
        }
    }

    #[test]
    fn gainmap_weight_sdr_and_hdr_base() {
        let r = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut info = default_info();

        // At SDR display ratio -> W = 0
        let u0 = compute_gainmap_uniforms(r, r, r, ColorType::Gray8, &info, 1.0);
        assert_eq!(u0.weight, 0.0);
        assert_eq!(u0.single_channel, 1);
        assert_eq!(u0.no_gamma, 1);

        // At geometric mean (2.0 between 1.0 and 4.0) -> W = 0.5
        let u_mid = compute_gainmap_uniforms(r, r, r, ColorType::Alpha8, &info, 2.0);
        assert!((u_mid.weight - 0.5).abs() < 1e-6);
        assert_eq!(u_mid.gainmap_is_alpha, 1);
        assert_eq!(u_mid.single_channel, 1);

        // At HDR display ratio -> W = 1.0
        let u_hi = compute_gainmap_uniforms(r, r, r, ColorType::Rgba8888, &info, 8.0);
        assert_eq!(u_hi.weight, 1.0);
        assert_eq!(u_hi.single_channel, 0);

        // HDR base image: W = 1 - 1 = 0 at HDR display, W = 0 - 1 = -1 at SDR display.
        info.base_image_type = GainmapBaseImageType::Hdr;
        let u_hdr_at_sdr = compute_gainmap_uniforms(r, r, r, ColorType::R8Unorm, &info, 1.0);
        assert_eq!(u_hdr_at_sdr.weight, -1.0);
        assert_eq!(u_hdr_at_sdr.gainmap_is_red, 1);
        assert_eq!(u_hdr_at_sdr.epsilon_base, info.epsilon_hdr);
        assert_eq!(u_hdr_at_sdr.epsilon_other, info.epsilon_sdr);
    }
}
