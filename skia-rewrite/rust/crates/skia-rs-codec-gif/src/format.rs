//! GIF pixel-format mapping, encoded-info classification, disposal/duration/repetition
//! conversions, scaling helpers, and one-pass vs two-pass decode selection.

use skia_rs_codec_core::types::{
    CodecResult, DisposalMethod, EncodedAlpha, EncodedColor, SelectionPolicy,
    REPETITION_COUNT_INFINITE,
};
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::ISize;

use crate::types::{
    wuffs_pixel_format, WuffsAnimationDisposal, WuffsStatusKind, WUFFS_FLICKS_PER_MILLISECOND,
};

/// Encoded image metadata classified from Wuffs' `image_config`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GifEncodedInfo {
    pub width: i32,
    pub height: i32,
    pub color: i32,
    pub alpha: i32,
    pub bits_per_component: u8,
}

/// Decision for whether incremental decode can run directly into destination memory (one-pass)
/// or must decode into an intermediate pixel buffer and composite/scale (two-pass).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OnePassDecision {
    pub one_pass: bool,
    pub pixel_format: u32,
    pub bytes_per_pixel: usize,
}

/// Exact port of `SkGifDecoder::MakeFromStream`'s stream-copy decision for non-seekable streams.
///
/// When `selection_policy != PreferStillImage` (e.g., `PreferAnimation`), a non-seekable stream
/// must be copied into a seekable memory stream so all animation frames can be visited.
#[inline]
pub const fn should_copy_stream_to_memory(selection_policy: SelectionPolicy, can_seek: bool) -> bool {
    !matches!(selection_policy, SelectionPolicy::PreferStillImage) && !can_seek
}

/// Maps Skia's native `kN32_SkColorType` (`BGRA_8888` or `RGBA_8888`) to the Wuffs pixel format
/// used for the codec's internal `image_config` / two-pass pixel buffer.
#[inline]
pub const fn n32_color_type_to_wuffs_pixfmt(n32_ct: ColorType) -> Result<u32, CodecResult> {
    match n32_ct {
        ColorType::Bgra8888 => Ok(wuffs_pixel_format::BGRA_NONPREMUL),
        ColorType::Rgba8888 => Ok(wuffs_pixel_format::RGBA_NONPREMUL),
        _ => Err(CodecResult::InternalError),
    }
}

/// Maps a requested destination `SkColorType` to a Wuffs pixel format and byte stride per pixel.
///
/// Exact port of `SkWuffsCodec::onStartIncrementalDecode`'s switch statement.
#[inline]
pub const fn dst_color_type_to_wuffs_pixfmt(dst_ct: ColorType) -> (u32, usize) {
    match dst_ct {
        ColorType::Rgb565 => (wuffs_pixel_format::BGR_565, 2),
        ColorType::Bgra8888 => (wuffs_pixel_format::BGRA_NONPREMUL, 4),
        ColorType::Rgba8888 => (wuffs_pixel_format::RGBA_NONPREMUL, 4),
        _ => (wuffs_pixel_format::INVALID, 0),
    }
}

/// Decides whether incremental decode can use the fast one-pass path.
///
/// Exact port of `SkWuffsCodec::onStartIncrementalDecode`:
/// one-pass is allowed iff Wuffs natively supports `dst_ct`, there is no color profile,
/// and `codec_dims == dst_dims` (no scaling).
#[inline]
pub const fn decide_one_pass(
    dst_ct: ColorType,
    has_color_profile: bool,
    codec_dims: ISize,
    dst_dims: ISize,
) -> OnePassDecision {
    let (pixel_format, bytes_per_pixel) = dst_color_type_to_wuffs_pixfmt(dst_ct);
    let one_pass = (pixel_format != wuffs_pixel_format::INVALID)
        && !has_color_profile
        && codec_dims.width == dst_dims.width
        && codec_dims.height == dst_dims.height;
    OnePassDecision {
        one_pass,
        pixel_format,
        bytes_per_pixel,
    }
}

/// Validates GIF image dimensions and computes `SkEncodedInfo` fields.
///
/// Exact port of `SkGifDecoder::MakeFromStream` lines 1086-1115.
#[inline]
pub const fn classify_encoded_info(
    width: u32,
    height: u32,
    pixfmt: u32,
    first_frame_is_opaque: bool,
) -> Result<GifEncodedInfo, CodecResult> {
    if width == 0 || width > i32::MAX as u32 || height == 0 || height > i32::MAX as u32 {
        return Err(CodecResult::InvalidInput);
    }
    let color = if pixfmt == wuffs_pixel_format::BGRA_NONPREMUL {
        EncodedColor::Bgra
    } else {
        EncodedColor::Rgba
    };
    let alpha = if first_frame_is_opaque {
        EncodedAlpha::Opaque
    } else {
        EncodedAlpha::Binary
    };
    Ok(GifEncodedInfo {
        width: width as i32,
        height: height as i32,
        color: color as i32,
        alpha: alpha as i32,
        bits_per_component: 8,
    })
}

/// Validates a Wuffs 64-bit buffer length (workbuf or two-pass pixbuf) fits in `usize`.
#[inline]
pub const fn validate_buffer_len(len_u64: u64) -> Result<usize, CodecResult> {
    if len_u64 > usize::MAX as u64 {
        Err(CodecResult::InternalError)
    } else {
        Ok(len_u64 as usize)
    }
}

/// Exact port of `to_alpha_type(bool opaque)` in `SkWuffsCodec.cpp`.
#[inline]
pub const fn to_alpha_type(opaque: bool) -> AlphaType {
    if opaque {
        AlphaType::Opaque
    } else {
        AlphaType::Premul
    }
}

/// Exact port of `wuffs_disposal_to_skia_disposal` in `SkWuffsCodec.cpp`.
#[inline]
pub const fn wuffs_disposal_to_skia(disposal: u8) -> DisposalMethod {
    match WuffsAnimationDisposal::from_u8(disposal) {
        Some(WuffsAnimationDisposal::RestoreBackground) => DisposalMethod::RestoreBgColor,
        Some(WuffsAnimationDisposal::RestorePrevious) => DisposalMethod::RestorePrevious,
        _ => DisposalMethod::Keep,
    }
}

/// Exact port of `fc->duration() / WUFFS_BASE__FLICKS_PER_MILLISECOND` cast to `int`.
///
/// In C++, `wuffs_base__flicks` (`int64_t`) divided by `WUFFS_BASE__FLICKS_PER_MILLISECOND`
/// (`uint64_t`) performs unsigned 64-bit division and converts to `int`.
#[inline]
pub const fn wuffs_duration_to_ms(duration_flicks: i64) -> i32 {
    ((duration_flicks as u64) / WUFFS_FLICKS_PER_MILLISECOND) as i32
}

/// Exact port of `SkWuffsCodec::onGetRepetitionCount`.
///
/// Wuffs' `num_animation_loops` is total plays (`0` = infinite).
/// Skia's repetition count is plays *after* the first (`-1` = infinite, clamped to `i32::MAX`).
#[inline]
pub const fn wuffs_loops_to_repetition_count(num_animation_loops: u32) -> i32 {
    if num_animation_loops == 0 {
        return REPETITION_COUNT_INFINITE;
    }
    let n = num_animation_loops - 1;
    if n < i32::MAX as u32 {
        n as i32
    } else {
        i32::MAX
    }
}

/// Maps `bits_per_pixel` to whole bytes per pixel, matching `SkWuffsCodec`'s validation.
#[inline]
pub const fn bits_per_pixel_to_bytes(bits_per_pixel: u32) -> Result<usize, CodecResult> {
    if bits_per_pixel == 0 || bits_per_pixel % 8 != 0 {
        Err(CodecResult::InternalError)
    } else {
        Ok((bits_per_pixel / 8) as usize)
    }
}

/// Maps a Wuffs status returned by `decode_image_config` / `decode_frame_config` / `decode_frame`
/// (after exhausting any `short_read` buffer fills) to an `SkCodec::Result`.
#[inline]
pub const fn map_wuffs_decode_status(status: WuffsStatusKind) -> CodecResult {
    match status {
        WuffsStatusKind::Ok => CodecResult::Success,
        WuffsStatusKind::ShortRead => CodecResult::IncompleteInput,
        WuffsStatusKind::EndOfData | WuffsStatusKind::Error => CodecResult::ErrorInInput,
    }
}

const MAX_S32_FITS_IN_FLOAT: f32 = 2_147_483_520.0;
const MIN_S32_FITS_IN_FLOAT: f32 = -2_147_483_648.0;

#[inline]
fn sk_float_round2int(x: f32) -> i32 {
    let mut v = (x + 0.5).floor();
    v = if v < MAX_S32_FITS_IN_FLOAT {
        v
    } else {
        MAX_S32_FITS_IN_FLOAT
    };
    v = if v > MIN_S32_FITS_IN_FLOAT {
        v
    } else {
        MIN_S32_FITS_IN_FLOAT
    };
    v as i32
}

/// Exact port of `SkScalingCodec::onGetScaledDimensions`.
#[inline]
pub fn scaled_dimensions(codec_dims: ISize, desired_scale: f32) -> ISize {
    let w = sk_float_round2int(desired_scale * codec_dims.width as f32);
    let h = sk_float_round2int(desired_scale * codec_dims.height as f32);
    ISize {
        width: w.max(1),
        height: h.max(1),
    }
}

/// Exact port of `SkScalingCodec::onDimensionsSupported`.
#[inline]
pub const fn dimensions_supported(codec_dims: ISize, requested: ISize) -> bool {
    requested.width >= 1
        && requested.width <= codec_dims.width
        && requested.height >= 1
        && requested.height <= codec_dims.height
}
