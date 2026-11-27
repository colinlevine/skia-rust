//! Safe Rust port of `src/codec/SkBmpStandardCodec.cpp`.

use skia_rs_codec_core::{
    sampler::get_start_coord,
    types::{CodecResult, EncodedAlpha, EncodedColor, ScanlineOrder},
};
use skia_rs_color::{
    color::{pmcolor_set_argb, premultiply_argb},
    image_info::{AlphaType, ColorType},
};
use skia_rs_geometry::ISize;

use crate::{bmp_header::bmp_dst_row, stream::ByteReader};

/// Matches `SkCodecPriv::IsRGBA(colorType)`.
#[inline]
pub(crate) const fn is_rgba_pack(color_type: ColorType, pmcolor_is_bgra: bool) -> bool {
    if pmcolor_is_bgra {
        matches!(color_type, ColorType::Rgba8888)
    } else {
        !matches!(color_type, ColorType::Bgra8888)
    }
}

/// Packs `(a, r, g, b)` matching `SkCodecPriv::ChoosePackColorProc(is_premul, color_type)`.
#[inline]
pub(crate) const fn pack_color_argb(
    a: u8,
    r: u8,
    g: u8,
    b: u8,
    is_premul: bool,
    color_type: ColorType,
    pmcolor_is_bgra: bool,
) -> u32 {
    let pack_as_bgra = !is_rgba_pack(color_type, pmcolor_is_bgra);
    if is_premul {
        premultiply_argb(a as u32, r as u32, g as u32, b as u32, pack_as_bgra)
    } else {
        pmcolor_set_argb(a as u32, r as u32, g as u32, b as u32, pack_as_bgra)
    }
}

/// Exact port of `SkBmpStandardCodec::onGetPixels` parameter validation.
#[inline]
pub const fn validate_standard_get_pixels(
    has_subset: bool,
    dst_dims: ISize,
    src_dims: ISize,
) -> CodecResult {
    if has_subset {
        return CodecResult::Unimplemented;
    }
    if dst_dims.width != src_dims.width || dst_dims.height != src_dims.height {
        return CodecResult::InvalidScale;
    }
    CodecResult::Success
}

/// Exact port of `SkBmpStandardCodec::createColorTable`.
///
/// On success, returns `Some(max_colors)` where `max_colors` is `1 << bits_per_pixel`
/// for `bits_per_pixel <= 8` (with `out_colors[..max_colors]` populated) or `0` for
/// `bits_per_pixel > 8`. Returns `None` on stream read/skip failure or invalid offset.
#[allow(clippy::too_many_arguments)]
pub fn build_standard_color_table(
    bits_per_pixel: u16,
    num_colors: u32,
    bytes_per_color: u32,
    offset: u32,
    is_opaque: bool,
    in_ico: bool,
    dst_color_type: ColorType,
    dst_alpha_type: AlphaType,
    has_color_xform: bool,
    pmcolor_is_bgra: bool,
    reader: &mut impl ByteReader,
    out_colors: &mut [u32; 256],
) -> Option<u32> {
    let mut color_bytes: u32 = 0;
    let mut max_colors: u32 = 0;

    if bits_per_pixel <= 8 {
        max_colors = 1u32 << bits_per_pixel;
        let num_colors_to_read = if num_colors == 0 {
            max_colors
        } else {
            num_colors.min(max_colors)
        };

        color_bytes = num_colors_to_read.checked_mul(bytes_per_color)?;
        // Maximum possible color_bytes is 256 * 4 = 1024 bytes.
        let mut c_buf = [0u8; 256 * 4];
        let c_slice = &mut c_buf[..color_bytes as usize];
        if reader.read(c_slice) != color_bytes as usize {
            return None;
        }

        let (pack_ct, pack_at) = if has_color_xform {
            (ColorType::Bgra8888, AlphaType::Unpremul)
        } else {
            (dst_color_type, dst_alpha_type)
        };
        let is_premul = pack_at == AlphaType::Premul && !is_opaque;
        let bpc = bytes_per_color as usize;

        for i in 0..(num_colors_to_read as usize) {
            let base = i * bpc;
            let blue = c_slice[base];
            let green = c_slice[base + 1];
            let red = c_slice[base + 2];
            let alpha = if is_opaque || bpc < 4 {
                0xFF
            } else {
                c_slice[base + 3]
            };
            out_colors[i] =
                pack_color_argb(alpha, red, green, blue, is_premul, pack_ct, pmcolor_is_bgra);
        }

        // Fill remaining palette entries with opaque black (SkPackARGB32(0xFF, 0, 0, 0)).
        for slot in &mut out_colors[(num_colors_to_read as usize)..(max_colors as usize)] {
            *slot = 0xFF00_0000;
        }
    }

    if !in_ico {
        if offset < color_bytes {
            return None;
        }
        let skip_bytes = (offset - color_bytes) as usize;
        if reader.skip(skip_bytes) != skip_bytes {
            return None;
        }
    }

    Some(max_colors)
}

/// Swizzler input format descriptor returned by [`standard_swizzler_info`].
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StandardSwizzlerEncodedInfo {
    pub color: EncodedColor,
    pub alpha: EncodedAlpha,
    pub bits_per_component: u8,
}

/// Exact port of `SkBmpStandardCodec::swizzlerInfo`.
#[inline]
pub const fn standard_swizzler_info(
    in_ico: bool,
    bits_per_pixel: u16,
    encoded_color: EncodedColor,
    encoded_alpha: EncodedAlpha,
    encoded_bpc: u8,
) -> StandardSwizzlerEncodedInfo {
    if in_ico {
        if bits_per_pixel <= 8 {
            return StandardSwizzlerEncodedInfo {
                color: EncodedColor::Palette,
                alpha: encoded_alpha,
                bits_per_component: bits_per_pixel as u8,
            };
        }
        if bits_per_pixel == 24 {
            return StandardSwizzlerEncodedInfo {
                color: EncodedColor::Bgr,
                alpha: EncodedAlpha::Opaque,
                bits_per_component: 8,
            };
        }
    }
    StandardSwizzlerEncodedInfo {
        color: encoded_color,
        alpha: encoded_alpha,
        bits_per_component: encoded_bpc,
    }
}

/// Computes the byte offset and remaining length of the ICO AND-mask substream during
/// scanline decode (exact port of `SkBmpStandardCodec::decodeRows` lines 282-297).
#[inline]
pub fn compute_ico_and_mask_substream(
    total_height: i32,
    start_scanline: i32,
    count: i32,
    src_row_bytes: usize,
    and_mask_row_bytes: usize,
    curr_position: usize,
    stream_length: usize,
) -> Option<(usize, usize)> {
    let remaining_scanlines = total_height.checked_sub(start_scanline)?.checked_sub(count)?;
    if remaining_scanlines < 0 || start_scanline < 0 {
        return None;
    }
    let skip_xor = (remaining_scanlines as usize).checked_mul(src_row_bytes)?;
    let skip_and = (start_scanline as usize).checked_mul(and_mask_row_bytes)?;
    let sub_start = curr_position.checked_add(skip_xor)?.checked_add(skip_and)?;
    if sub_start >= stream_length {
        return None;
    }
    Some((sub_start, stream_length - sub_start))
}

/// Applies one decoded 1-bit ICO AND-mask row onto a destination pixel row.
///
/// Matches `SkBmpStandardCodec::decodeIcoMask` pixel loop: where the 1-bit mask is `1`,
/// the destination pixel (4 bytes for RGBA8888/BGRA8888, 8 bytes for RGBA_F16) is zeroed;
/// where the mask bit is `0`, the destination pixel is left unchanged.
pub fn apply_ico_and_mask_row(
    mask_row: &[u8],
    dst_row: &mut [u8],
    dst_color_type: ColorType,
    src_start_x: i32,
    sample_x: i32,
    swizzle_width: i32,
) {
    if swizzle_width <= 0 {
        return;
    }
    let pixel_bytes: usize = if dst_color_type == ColorType::RgbaF16 {
        8
    } else {
        4
    };

    let mut src_x = src_start_x;
    for dst_x in 0..(swizzle_width as usize) {
        let quotient = (src_x / 8) as usize;
        let modulus = (src_x % 8) as u32;
        let shift = 7 - modulus;
        let alpha_bit = (mask_row[quotient] >> shift) & 0x1;
        if alpha_bit != 0 {
            let off = dst_x * pixel_bytes;
            dst_row[off..off + pixel_bytes].fill(0);
        }
        src_x += sample_x;
    }
}

/// Exact port of `SkBmpStandardCodec::decodeIcoMask`.
#[allow(clippy::too_many_arguments)]
pub fn decode_ico_mask(
    reader: &mut impl ByteReader,
    mask_buf: &mut [u8],
    and_mask_row_bytes: usize,
    dst: &mut [u8],
    dst_row_bytes: usize,
    dst_height: i32,
    dst_color_type: ColorType,
    row_order: ScanlineOrder,
    sample_x: i32,
    swizzle_width: i32,
    src_offset: i32,
) {
    debug_assert!(matches!(
        dst_color_type,
        ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::RgbaF16
    ));
    let src_start_x = get_start_coord(sample_x) + src_offset;
    let pixel_bytes: usize = if dst_color_type == ColorType::RgbaF16 {
        8
    } else {
        4
    };
    let row_slice_len = if swizzle_width > 0 {
        (swizzle_width as usize) * pixel_bytes
    } else {
        0
    };

    let mask_slice = &mut mask_buf[..and_mask_row_bytes];
    for y in 0..dst_height {
        if reader.read(mask_slice) != and_mask_row_bytes {
            return;
        }
        if row_slice_len > 0 {
            let row = bmp_dst_row(row_order, y, dst_height) as usize;
            let row_start = row * dst_row_bytes;
            apply_ico_and_mask_row(
                mask_slice,
                &mut dst[row_start..row_start + row_slice_len],
                dst_color_type,
                src_start_x,
                sample_x,
                swizzle_width,
            );
        }
    }
}
