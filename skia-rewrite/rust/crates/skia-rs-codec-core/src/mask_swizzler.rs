//! Safe Rust port of `src/codec/SkMaskSwizzler.cpp`.

use skia_rs_color::color::mul_div_255_round;
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_color::masks::Masks;
use skia_rs_geometry::IRect;

use crate::sampler::{get_sampled_dimension, get_start_coord};

/// Identifies one of the 21 mask swizzle row procedures in `SkMaskSwizzler`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MaskSwizzleProcId {
    Mask16ToRgbaOpaque = 0,
    Mask16ToBgraOpaque = 1,
    Mask16ToRgbaUnpremul = 2,
    Mask16ToBgraUnpremul = 3,
    Mask16ToRgbaPremul = 4,
    Mask16ToBgraPremul = 5,
    Mask16To565 = 6,

    Mask24ToRgbaOpaque = 7,
    Mask24ToBgraOpaque = 8,
    Mask24ToRgbaUnpremul = 9,
    Mask24ToBgraUnpremul = 10,
    Mask24ToRgbaPremul = 11,
    Mask24ToBgraPremul = 12,
    Mask24To565 = 13,

    Mask32ToRgbaOpaque = 14,
    Mask32ToBgraOpaque = 15,
    Mask32ToRgbaUnpremul = 16,
    Mask32ToBgraUnpremul = 17,
    Mask32ToRgbaPremul = 18,
    Mask32ToBgraPremul = 19,
    Mask32To565 = 20,
}

impl MaskSwizzleProcId {
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Mask16ToRgbaOpaque),
            1 => Some(Self::Mask16ToBgraOpaque),
            2 => Some(Self::Mask16ToRgbaUnpremul),
            3 => Some(Self::Mask16ToBgraUnpremul),
            4 => Some(Self::Mask16ToRgbaPremul),
            5 => Some(Self::Mask16ToBgraPremul),
            6 => Some(Self::Mask16To565),
            7 => Some(Self::Mask24ToRgbaOpaque),
            8 => Some(Self::Mask24ToBgraOpaque),
            9 => Some(Self::Mask24ToRgbaUnpremul),
            10 => Some(Self::Mask24ToBgraUnpremul),
            11 => Some(Self::Mask24ToRgbaPremul),
            12 => Some(Self::Mask24ToBgraPremul),
            13 => Some(Self::Mask24To565),
            14 => Some(Self::Mask32ToRgbaOpaque),
            15 => Some(Self::Mask32ToBgraOpaque),
            16 => Some(Self::Mask32ToRgbaUnpremul),
            17 => Some(Self::Mask32ToBgraUnpremul),
            18 => Some(Self::Mask32ToRgbaPremul),
            19 => Some(Self::Mask32ToBgraPremul),
            20 => Some(Self::Mask32To565),
            _ => None,
        }
    }

    /// Source bytes per pixel (2, 3, or 4).
    #[inline]
    pub const fn src_bytes_per_pixel(self) -> usize {
        match self as i32 {
            0..=6 => 2,
            7..=13 => 3,
            _ => 4,
        }
    }

    /// Destination bytes per pixel (2 for RGB565, 4 for RGBA/BGRA8888).
    #[inline]
    pub const fn dst_bytes_per_pixel(self) -> usize {
        match self {
            Self::Mask16To565 | Self::Mask24To565 | Self::Mask32To565 => 2,
            _ => 4,
        }
    }
}

/// Initialization parameters returned for `SkMaskSwizzler::CreateMaskSwizzler`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MaskSwizzlerInit {
    /// `-1` if `proc` is `nullptr` (unsupported color/alpha combo with valid bpp).
    pub proc_id: i32,
    pub src_offset: i32,
    pub subset_width: i32,
}

/// Selects the mask swizzle procedure matching `SkMaskSwizzler::CreateMaskSwizzler`.
/// Returns `None` if `bits_per_pixel` is not 16, 24, or 32.
pub fn select_mask_swizzler(
    dst_ct: ColorType,
    dst_at: AlphaType,
    dst_width: i32,
    src_is_opaque: bool,
    bits_per_pixel: u32,
    subset: Option<IRect>,
) -> Option<MaskSwizzlerInit> {
    let proc: Option<MaskSwizzleProcId> = match bits_per_pixel {
        16 => match dst_ct {
            ColorType::Rgba8888 => {
                if src_is_opaque {
                    Some(MaskSwizzleProcId::Mask16ToRgbaOpaque)
                } else {
                    match dst_at {
                        AlphaType::Unpremul => Some(MaskSwizzleProcId::Mask16ToRgbaUnpremul),
                        AlphaType::Premul => Some(MaskSwizzleProcId::Mask16ToRgbaPremul),
                        _ => None,
                    }
                }
            }
            ColorType::Bgra8888 => {
                if src_is_opaque {
                    Some(MaskSwizzleProcId::Mask16ToBgraOpaque)
                } else {
                    match dst_at {
                        AlphaType::Unpremul => Some(MaskSwizzleProcId::Mask16ToBgraUnpremul),
                        AlphaType::Premul => Some(MaskSwizzleProcId::Mask16ToBgraPremul),
                        _ => None,
                    }
                }
            }
            ColorType::Rgb565 => Some(MaskSwizzleProcId::Mask16To565),
            _ => None,
        },
        24 => match dst_ct {
            ColorType::Rgba8888 => {
                if src_is_opaque {
                    Some(MaskSwizzleProcId::Mask24ToRgbaOpaque)
                } else {
                    match dst_at {
                        AlphaType::Unpremul => Some(MaskSwizzleProcId::Mask24ToRgbaUnpremul),
                        AlphaType::Premul => Some(MaskSwizzleProcId::Mask24ToRgbaPremul),
                        _ => None,
                    }
                }
            }
            ColorType::Bgra8888 => {
                if src_is_opaque {
                    Some(MaskSwizzleProcId::Mask24ToBgraOpaque)
                } else {
                    match dst_at {
                        AlphaType::Unpremul => Some(MaskSwizzleProcId::Mask24ToBgraUnpremul),
                        AlphaType::Premul => Some(MaskSwizzleProcId::Mask24ToBgraPremul),
                        _ => None,
                    }
                }
            }
            ColorType::Rgb565 => Some(MaskSwizzleProcId::Mask24To565),
            _ => None,
        },
        32 => match dst_ct {
            ColorType::Rgba8888 => {
                if src_is_opaque {
                    Some(MaskSwizzleProcId::Mask32ToRgbaOpaque)
                } else {
                    match dst_at {
                        AlphaType::Unpremul => Some(MaskSwizzleProcId::Mask32ToRgbaUnpremul),
                        AlphaType::Premul => Some(MaskSwizzleProcId::Mask32ToRgbaPremul),
                        _ => None,
                    }
                }
            }
            ColorType::Bgra8888 => {
                if src_is_opaque {
                    Some(MaskSwizzleProcId::Mask32ToBgraOpaque)
                } else {
                    match dst_at {
                        AlphaType::Unpremul => Some(MaskSwizzleProcId::Mask32ToBgraUnpremul),
                        AlphaType::Premul => Some(MaskSwizzleProcId::Mask32ToBgraPremul),
                        _ => None,
                    }
                }
            }
            ColorType::Rgb565 => Some(MaskSwizzleProcId::Mask32To565),
            _ => None,
        },
        _ => return None,
    };

    let (src_offset, subset_width) = match subset {
        Some(r) => (r.left, r.width()),
        None => (0, dst_width),
    };

    Some(MaskSwizzlerInit {
        proc_id: proc.map_or(-1, |p| p as i32),
        src_offset,
        subset_width,
    })
}

/// Computes `(x0, dst_width)` for `SkMaskSwizzler::onSetSampleX`.
#[inline]
pub const fn compute_mask_sample_x(src_offset: i32, subset_width: i32, sample_x: i32) -> (i32, i32) {
    let x0 = get_start_coord(sample_x) + src_offset;
    let dst_width = get_sampled_dimension(subset_width, sample_x);
    (x0, dst_width)
}

#[inline]
const fn pack_rgba(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

#[inline]
const fn pack_bgra(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[inline]
const fn premul_rgba(a: u8, mut r: u8, mut g: u8, mut b: u8) -> u32 {
    if a != 255 {
        r = mul_div_255_round(r as u32, a as u32) as u8;
        g = mul_div_255_round(g as u32, a as u32) as u8;
        b = mul_div_255_round(b as u32, a as u32) as u8;
    }
    pack_rgba(a, r, g, b)
}

#[inline]
const fn premul_bgra(a: u8, mut r: u8, mut g: u8, mut b: u8) -> u32 {
    if a != 255 {
        r = mul_div_255_round(r as u32, a as u32) as u8;
        g = mul_div_255_round(g as u32, a as u32) as u8;
        b = mul_div_255_round(b as u32, a as u32) as u8;
    }
    pack_bgra(a, r, g, b)
}

#[inline]
const fn pack_888_to_rgb16(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3)
}

#[inline]
fn read_pixel_16(src: &[u8], byte_offset: usize) -> u32 {
    u16::from_ne_bytes([src[byte_offset], src[byte_offset + 1]]) as u32
}

#[inline]
fn read_pixel_24(src: &[u8], byte_offset: usize) -> u32 {
    (src[byte_offset] as u32)
        | ((src[byte_offset + 1] as u32) << 8)
        | ((src[byte_offset + 2] as u32) << 16)
}

#[inline]
fn read_pixel_32(src: &[u8], byte_offset: usize) -> u32 {
    u32::from_ne_bytes([
        src[byte_offset],
        src[byte_offset + 1],
        src[byte_offset + 2],
        src[byte_offset + 3],
    ])
}

/// Executes a mask swizzle row procedure in safe Rust.
pub fn swizzle_mask_row(
    proc: MaskSwizzleProcId,
    dst: &mut [u8],
    src: &[u8],
    width: usize,
    masks: &Masks,
    start_x: usize,
    sample_x: usize,
) {
    if width == 0 {
        return;
    }
    let src_bpp = proc.src_bytes_per_pixel();
    let stride = src_bpp * sample_x;
    let mut src_pos = src_bpp * start_x;

    match proc {
        MaskSwizzleProcId::Mask16ToRgbaOpaque => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_16(src, src_pos);
                let c = pack_rgba(0xFF, masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask16ToBgraOpaque => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_16(src, src_pos);
                let c = pack_bgra(0xFF, masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask16ToRgbaUnpremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_16(src, src_pos);
                let c = pack_rgba(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask16ToBgraUnpremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_16(src, src_pos);
                let c = pack_bgra(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask16ToRgbaPremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_16(src, src_pos);
                let c = premul_rgba(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask16ToBgraPremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_16(src, src_pos);
                let c = premul_bgra(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask16To565 => {
            for out in dst[..width * 2].chunks_exact_mut(2) {
                let p = read_pixel_16(src, src_pos);
                let c = pack_888_to_rgb16(masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }

        MaskSwizzleProcId::Mask24ToRgbaOpaque => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_24(src, src_pos);
                let c = pack_rgba(0xFF, masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask24ToBgraOpaque => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_24(src, src_pos);
                let c = pack_bgra(0xFF, masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask24ToRgbaUnpremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_24(src, src_pos);
                let c = pack_rgba(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask24ToBgraUnpremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_24(src, src_pos);
                let c = pack_bgra(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask24ToRgbaPremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_24(src, src_pos);
                let c = premul_rgba(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask24ToBgraPremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_24(src, src_pos);
                let c = premul_bgra(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask24To565 => {
            for out in dst[..width * 2].chunks_exact_mut(2) {
                let p = read_pixel_24(src, src_pos);
                let c = pack_888_to_rgb16(masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }

        MaskSwizzleProcId::Mask32ToRgbaOpaque => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_32(src, src_pos);
                let c = pack_rgba(0xFF, masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask32ToBgraOpaque => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_32(src, src_pos);
                let c = pack_bgra(0xFF, masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask32ToRgbaUnpremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_32(src, src_pos);
                let c = pack_rgba(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask32ToBgraUnpremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_32(src, src_pos);
                let c = pack_bgra(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask32ToRgbaPremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_32(src, src_pos);
                let c = premul_rgba(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask32ToBgraPremul => {
            for out in dst[..width * 4].chunks_exact_mut(4) {
                let p = read_pixel_32(src, src_pos);
                let c = premul_bgra(
                    masks.get_alpha(p),
                    masks.get_red(p),
                    masks.get_green(p),
                    masks.get_blue(p),
                );
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
        MaskSwizzleProcId::Mask32To565 => {
            for out in dst[..width * 2].chunks_exact_mut(2) {
                let p = read_pixel_32(src, src_pos);
                let c = pack_888_to_rgb16(masks.get_red(p), masks.get_green(p), masks.get_blue(p));
                out.copy_from_slice(&c.to_ne_bytes());
                src_pos += stride;
            }
        }
    }
}
