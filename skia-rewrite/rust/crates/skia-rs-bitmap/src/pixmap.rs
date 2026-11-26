//! Safe Rust port of `src/core/SkPixmap.cpp` and `src/core/SkPixmapDraw.cpp`.

use skia_rs_color::{
    color::{color_set_argb, Color, Color4f},
    image_info::{
        color_type_bytes_per_pixel, color_type_shift_per_pixel, AlphaType, ColorType,
    },
    unpremultiply::pmcolor_to_color,
};
use skia_rs_geometry::{pin_f32, IRect, Matrix, Rect, matrix::ScaleToFit};
use skia_rs_math::half::half_to_float;

const SK_HALF_1: u16 = 0x3C00;

#[inline]
const fn swap_rb(c: u32) -> u32 {
    (c & 0xFF00_FF00) | ((c & 0x00FF_0000) >> 16) | ((c & 0x0000_00FF) << 16)
}

#[inline]
const fn swizzle_rgba_to_pmcolor(c: u32, is_bgra: bool) -> u32 {
    if is_bgra {
        swap_rb(c)
    } else {
        c
    }
}

#[inline]
const fn swizzle_bgra_to_pmcolor(c: u32, is_bgra: bool) -> u32 {
    if is_bgra {
        c
    } else {
        swap_rb(c)
    }
}

#[inline]
const fn to_color(maybe_premul: u32, needs_unpremul: bool, is_bgra: bool) -> Color {
    if needs_unpremul {
        pmcolor_to_color(maybe_premul, is_bgra)
    } else {
        swizzle_bgra_to_pmcolor(maybe_premul, is_bgra)
    }
}

#[inline]
const fn pixel16_to_color(src: u16) -> Color {
    let r5 = ((src as u32) >> 11) & 0x1F;
    let g6 = ((src as u32) >> 5) & 0x3F;
    let b5 = (src as u32) & 0x1F;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g6 << 2) | (g6 >> 4);
    let b = (b5 << 3) | (b5 >> 2);
    color_set_argb(0xFF, r, g, b)
}

#[inline]
const fn pixel4444_to_pixel32(c: u16, is_bgra: bool) -> u32 {
    let a = (c as u32) & 0xF;
    let r = ((c as u32) >> 12) & 0xF;
    let g = ((c as u32) >> 8) & 0xF;
    let b = ((c as u32) >> 4) & 0xF;
    let (r_shift, b_shift) = if is_bgra { (16, 0) } else { (0, 16) };
    let d = (a << 24) | (r << r_shift) | (g << 8) | (b << b_shift);
    d | (d << 4)
}

#[inline]
fn srgb_to_linear(x: f32) -> f32 {
    if x <= 0.04045_f32 {
        x * (1.0_f32 / 12.92_f32)
    } else {
        (x * (1.0_f32 / 1.055_f32) + (0.055_f32 / 1.055_f32)).powf(2.4_f32)
    }
}

#[inline]
fn read_u16_ne(bytes: &[u8]) -> u16 {
    u16::from_ne_bytes([bytes[0], bytes[1]])
}

#[inline]
fn read_u32_ne(bytes: &[u8]) -> u32 {
    u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[inline]
fn read_u64_ne(bytes: &[u8]) -> u64 {
    u64::from_ne_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[inline]
fn read_f32_ne(bytes: &[u8]) -> f32 {
    f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Result of `SkPixmap::extractSubset`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PixmapSubsetResult {
    pub clipped: IRect,
    pub byte_offset: usize,
}

/// Exact port of `SkPixmap::extractSubset` geometry and pixel offset calculation.
pub fn pixmap_extract_subset(
    width: i32,
    height: i32,
    ct: ColorType,
    row_bytes: usize,
    subset: IRect,
) -> Option<PixmapSubsetResult> {
    let src_rect = IRect::from_wh(width, height);
    let mut r = IRect::make_empty();
    if !r.intersect_pair(src_rect, subset) {
        return None;
    }
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let byte_offset = (r.top as usize) * row_bytes + (r.left as usize) * bpp;
    Some(PixmapSubsetResult {
        clipped: r,
        byte_offset,
    })
}

/// Exact port of `SkPixmap::getAlphaf(int x, int y)` given the slice starting at `(x, y)`.
pub fn pixmap_get_alphaf(ct: ColorType, pixel: &[u8]) -> f32 {
    match ct {
        ColorType::Unknown => 0.0,
        ColorType::Gray8
        | ColorType::R8G8Unorm
        | ColorType::R16Unorm
        | ColorType::R16Float
        | ColorType::R16G16Unorm
        | ColorType::R16G16Float
        | ColorType::Rgb565
        | ColorType::Rgb888x
        | ColorType::Bgr888x
        | ColorType::Rgb101010x
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::RgbF16F16F16x
        | ColorType::R8Unorm => 1.0,
        ColorType::Alpha8 => (pixel[0] as f32) * (1.0_f32 / 255.0_f32),
        ColorType::A16Unorm => (read_u16_ne(pixel) as f32) * (1.0_f32 / 65535.0_f32),
        ColorType::A16Float => half_to_float(read_u16_ne(pixel)),
        ColorType::Argb4444 => {
            let u16_val = read_u16_ne(pixel);
            ((u16_val & 0xF) as f32) * (1.0_f32 / 15.0_f32)
        }
        ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::Srgba8888 => {
            (pixel[3] as f32) * (1.0_f32 / 255.0_f32)
        }
        ColorType::Rgba1010102 | ColorType::Bgra1010102 => {
            let u32_val = read_u32_ne(pixel);
            ((u32_val >> 30) as f32) * (1.0_f32 / 3.0_f32)
        }
        ColorType::Bgra10101010Xr => {
            let u64_val = read_u64_ne(pixel);
            ((u64_val >> 54).wrapping_sub(384) as f32) / 510.0_f32
        }
        ColorType::Rgba10x6 => {
            let u64_val = read_u64_ne(pixel);
            ((u64_val >> 54) as f32) * (1.0_f32 / 1023.0_f32)
        }
        ColorType::R16G16B16A16Unorm => {
            let u64_val = read_u64_ne(pixel);
            ((u64_val >> 48) as f32) * (1.0_f32 / 65535.0_f32)
        }
        ColorType::RgbaF16Norm | ColorType::RgbaF16 => half_to_float(read_u16_ne(&pixel[6..8])),
        ColorType::RgbaF32 => read_f32_ne(&pixel[12..16]),
    }
}

/// Exact port of `SkPixmap::getColor(int x, int y)` given the slice starting at `(x, y)`.
pub fn pixmap_get_color(ct: ColorType, at: AlphaType, pixel: &[u8], is_bgra: bool) -> Color {
    let needs_unpremul = at == AlphaType::Premul;
    match ct {
        ColorType::Gray8 => {
            let v = pixel[0] as u32;
            color_set_argb(0xFF, v, v, v)
        }
        ColorType::R8Unorm => color_set_argb(0xFF, pixel[0] as u32, 0, 0),
        ColorType::Alpha8 => color_set_argb(pixel[0] as u32, 0, 0, 0),
        ColorType::A16Unorm => {
            let a = ((read_u16_ne(pixel) as f32) * (255.0_f32 / 65535.0_f32)) as u32;
            color_set_argb(a, 0, 0, 0)
        }
        ColorType::A16Float => {
            let a = (255.0_f32 * half_to_float(read_u16_ne(pixel))) as u32;
            color_set_argb(a, 0, 0, 0)
        }
        ColorType::R16Float => {
            let r = (255.0_f32 * half_to_float(read_u16_ne(pixel))) as u32;
            color_set_argb(0xFF, r, 0, 0)
        }
        ColorType::Rgb565 => pixel16_to_color(read_u16_ne(pixel)),
        ColorType::Argb4444 => {
            let c = pixel4444_to_pixel32(read_u16_ne(pixel), is_bgra);
            to_color(c, needs_unpremul, is_bgra)
        }
        ColorType::R8G8Unorm => {
            let value = read_u16_ne(pixel);
            color_set_argb(0xFF, (value & 0xFF) as u32, ((value >> 8) & 0xFF) as u32, 0)
        }
        ColorType::R16Unorm => {
            let value = read_u16_ne(pixel);
            let r = ((value as f32) * (255.0_f32 / 65535.0_f32)) as u32;
            color_set_argb(0xFF, r, 0, 0)
        }
        ColorType::R16G16Unorm => {
            let value = read_u32_ne(pixel);
            let r = (((value & 0xFFFF) as f32) * (255.0_f32 / 65535.0_f32)) as u8 as u32;
            let g = ((((value >> 16) & 0xFFFF) as f32) * (255.0_f32 / 65535.0_f32)) as u8 as u32;
            color_set_argb(0xFF, r, g, 0)
        }
        ColorType::R16G16Float => {
            let value = read_u32_ne(pixel);
            let r = half_to_float((value & 0xFFFF) as u16);
            let g = half_to_float(((value >> 16) & 0xFFFF) as u16);
            color_set_argb(0xFF, (255.0_f32 * r) as u8 as u32, (255.0_f32 * g) as u8 as u32, 0)
        }
        ColorType::Rgb888x => {
            let value = read_u32_ne(pixel);
            to_color(swizzle_rgba_to_pmcolor(value | 0xFF00_0000, is_bgra), needs_unpremul, is_bgra)
        }
        ColorType::Bgra8888 => {
            let value = read_u32_ne(pixel);
            to_color(swizzle_bgra_to_pmcolor(value, is_bgra), needs_unpremul, is_bgra)
        }
        ColorType::Bgr888x => {
            let value = read_u32_ne(pixel);
            to_color(swizzle_bgra_to_pmcolor(value | 0xFF00_0000, is_bgra), needs_unpremul, is_bgra)
        }
        ColorType::Rgba8888 => {
            let value = read_u32_ne(pixel);
            to_color(swizzle_rgba_to_pmcolor(value, is_bgra), needs_unpremul, is_bgra)
        }
        ColorType::Srgba8888 => {
            let value = read_u32_ne(pixel);
            let mut r = ((value & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            let mut g = (((value >> 8) & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            let mut b = (((value >> 16) & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            let mut a = (((value >> 24) & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            r = srgb_to_linear(r);
            g = srgb_to_linear(g);
            b = srgb_to_linear(b);
            if a != 0.0 && needs_unpremul {
                r = pin_f32(r / a, 0.0, 1.0);
                g = pin_f32(g / a, 0.0, 1.0);
                b = pin_f32(b / a, 0.0, 1.0);
            }
            r *= 255.0_f32;
            g *= 255.0_f32;
            b *= 255.0_f32;
            a *= 255.0_f32;
            color_set_argb(a as u32, r as u32, g as u32, b as u32)
        }
        ColorType::Rgb101010x => {
            let value = read_u32_ne(pixel);
            let r = (((value & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let g = ((((value >> 10) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let b = ((((value >> 20) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            color_set_argb(0xFF, r, g, b)
        }
        ColorType::Bgr101010xXr | ColorType::Bgra10101010Xr | ColorType::Unknown => 0,
        ColorType::Bgr101010x => {
            let value = read_u32_ne(pixel);
            let r = ((((value >> 20) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let g = ((((value >> 10) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let b = (((value & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            color_set_argb(0xFF, r, g, b)
        }
        ColorType::Bgra1010102 | ColorType::Rgba1010102 => {
            let value = read_u32_ne(pixel);
            let mut r = ((value & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32);
            let mut g = (((value >> 10) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32);
            let mut b = (((value >> 20) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32);
            let mut a = (((value >> 30) & 0x3) as f32) * (1.0_f32 / 3.0_f32);
            if ct == ColorType::Bgra1010102 {
                core::mem::swap(&mut r, &mut b);
            }
            if a != 0.0 && needs_unpremul {
                r = pin_f32(r / a, 0.0, 1.0);
                g = pin_f32(g / a, 0.0, 1.0);
                b = pin_f32(b / a, 0.0, 1.0);
            }
            b *= 255.0_f32;
            g *= 255.0_f32;
            r *= 255.0_f32;
            a *= 255.0_f32;
            color_set_argb(a as u32, r as u32, g as u32, b as u32)
        }
        ColorType::Rgba10x6 => {
            let value = read_u64_ne(pixel);
            let a = ((((value >> 54) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let r = ((((value >> 6) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let g = ((((value >> 22) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            let b = ((((value >> 38) & 0x3FF) as f32) * (255.0_f32 / 1023.0_f32)) as u32;
            color_set_argb(a, r, g, b)
        }
        ColorType::R16G16B16A16Unorm => {
            let value = read_u64_ne(pixel);
            let mut r = ((value & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let mut g = (((value >> 16) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let mut b = (((value >> 32) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let mut a = (((value >> 48) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            if a != 0.0 && needs_unpremul {
                let inva = 1.0_f32 / a;
                r *= inva;
                g *= inva;
                b *= inva;
            }
            r *= 255.0_f32;
            g *= 255.0_f32;
            b *= 255.0_f32;
            a *= 255.0_f32;
            color_set_argb(a as u32, r as u32, g as u32, b as u32)
        }
        ColorType::RgbF16F16F16x => {
            let r = half_to_float(read_u16_ne(&pixel[0..2]));
            let g = half_to_float(read_u16_ne(&pixel[2..4]));
            let b = half_to_float(read_u16_ne(&pixel[4..6]));
            Color4f::new(r, g, b, 1.0_f32).to_sk_color()
        }
        ColorType::RgbaF16Norm | ColorType::RgbaF16 => {
            let mut r = half_to_float(read_u16_ne(&pixel[0..2]));
            let mut g = half_to_float(read_u16_ne(&pixel[2..4]));
            let mut b = half_to_float(read_u16_ne(&pixel[4..6]));
            let a = half_to_float(read_u16_ne(&pixel[6..8]));
            if a != 0.0 && needs_unpremul {
                let inva = 1.0_f32 / a;
                r *= inva;
                g *= inva;
                b *= inva;
            }
            Color4f::new(r, g, b, a).to_sk_color()
        }
        ColorType::RgbaF32 => {
            let mut r = read_f32_ne(&pixel[0..4]);
            let mut g = read_f32_ne(&pixel[4..8]);
            let mut b = read_f32_ne(&pixel[8..12]);
            let a = read_f32_ne(&pixel[12..16]);
            if a != 0.0 && needs_unpremul {
                let inva = 1.0_f32 / a;
                r *= inva;
                g *= inva;
                b *= inva;
            }
            Color4f::new(r, g, b, a).to_sk_color()
        }
    }
}

/// Exact port of `SkPixmap::getColor4f(int x, int y)` given the slice starting at `(x, y)`.
pub fn pixmap_get_color4f(ct: ColorType, at: AlphaType, pixel: &[u8], is_bgra: bool) -> Color4f {
    let needs_unpremul = at == AlphaType::Premul;
    match ct {
        ColorType::Gray8 => {
            let v = (pixel[0] as f32) / 255.0_f32;
            Color4f::new(v, v, v, 1.0)
        }
        ColorType::R8Unorm => Color4f::new((pixel[0] as f32) / 255.0_f32, 0.0, 0.0, 1.0),
        ColorType::Alpha8 => Color4f::new(0.0, 0.0, 0.0, (pixel[0] as f32) / 255.0_f32),
        ColorType::R16Unorm => Color4f::new((read_u16_ne(pixel) as f32) / 65535.0_f32, 0.0, 0.0, 1.0),
        ColorType::A16Unorm => Color4f::new(0.0, 0.0, 0.0, (read_u16_ne(pixel) as f32) / 65535.0_f32),
        ColorType::R16Float => Color4f::new(half_to_float(read_u16_ne(pixel)), 0.0, 0.0, 1.0),
        ColorType::A16Float => Color4f::new(0.0, 0.0, 0.0, half_to_float(read_u16_ne(pixel))),
        ColorType::Rgb565 => Color4f::from_color(pixel16_to_color(read_u16_ne(pixel))),
        ColorType::Argb4444 => {
            let c = pixel4444_to_pixel32(read_u16_ne(pixel), is_bgra);
            Color4f::from_color(to_color(c, needs_unpremul, is_bgra))
        }
        ColorType::R8G8Unorm => {
            let value = read_u16_ne(pixel);
            Color4f::from_color(color_set_argb(
                0xFF,
                (value & 0xFF) as u32,
                ((value >> 8) & 0xFF) as u32,
                0,
            ))
        }
        ColorType::R16G16Unorm => {
            let value = read_u32_ne(pixel);
            let r = ((value & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let g = (((value >> 16) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            Color4f::new(r, g, 0.0, 1.0)
        }
        ColorType::R16G16Float => {
            let value = read_u32_ne(pixel);
            let r = half_to_float((value & 0xFFFF) as u16);
            let g = half_to_float(((value >> 16) & 0xFFFF) as u16);
            Color4f::new(r, g, 0.0, 1.0)
        }
        ColorType::Rgb888x => {
            let c = swizzle_rgba_to_pmcolor(read_u32_ne(pixel) | 0xFF00_0000, is_bgra);
            Color4f::from_color(to_color(c, needs_unpremul, is_bgra))
        }
        ColorType::Bgra8888 => {
            let c = swizzle_bgra_to_pmcolor(read_u32_ne(pixel), is_bgra);
            Color4f::from_color(to_color(c, needs_unpremul, is_bgra))
        }
        ColorType::Bgr888x => {
            let c = swizzle_bgra_to_pmcolor(read_u32_ne(pixel) | 0xFF00_0000, is_bgra);
            Color4f::from_color(to_color(c, needs_unpremul, is_bgra))
        }
        ColorType::Rgba8888 => {
            let c = swizzle_rgba_to_pmcolor(read_u32_ne(pixel), is_bgra);
            Color4f::from_color(to_color(c, needs_unpremul, is_bgra))
        }
        ColorType::Srgba8888 => {
            let value = read_u32_ne(pixel);
            let mut r = ((value & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            let mut g = (((value >> 8) & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            let mut b = (((value >> 16) & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            let a = (((value >> 24) & 0xFF) as f32) * (1.0_f32 / 255.0_f32);
            r = srgb_to_linear(r);
            g = srgb_to_linear(g);
            b = srgb_to_linear(b);
            if a != 0.0 && needs_unpremul {
                r = pin_f32(r / a, 0.0, 1.0);
                g = pin_f32(g / a, 0.0, 1.0);
                b = pin_f32(b / a, 0.0, 1.0);
            }
            Color4f::new(r, g, b, a)
        }
        ColorType::Bgr101010xXr | ColorType::Bgra10101010Xr | ColorType::Unknown => {
            Color4f::new(0.0, 0.0, 0.0, 0.0)
        }
        ColorType::Rgb101010x => {
            let value = read_u32_ne(pixel);
            Color4f::new(
                ((value & 0x3FF) as f32) / 1023.0_f32,
                (((value >> 10) & 0x3FF) as f32) / 1023.0_f32,
                (((value >> 20) & 0x3FF) as f32) / 1023.0_f32,
                1.0,
            )
        }
        ColorType::Bgr101010x => {
            let value = read_u32_ne(pixel);
            Color4f::new(
                (((value >> 20) & 0x3FF) as f32) / 1023.0_f32,
                (((value >> 10) & 0x3FF) as f32) / 1023.0_f32,
                ((value & 0x3FF) as f32) / 1023.0_f32,
                1.0,
            )
        }
        ColorType::Rgba1010102 | ColorType::Bgra1010102 => {
            let value = read_u32_ne(pixel);
            let mut r = ((value & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32);
            let mut g = (((value >> 10) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32);
            let mut b = (((value >> 20) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32);
            let a = (((value >> 30) & 0x3) as f32) * (1.0_f32 / 3.0_f32);
            if ct == ColorType::Bgra1010102 {
                core::mem::swap(&mut r, &mut b);
            }
            if a != 0.0 && needs_unpremul {
                r = pin_f32(r / a, 0.0, 1.0);
                g = pin_f32(g / a, 0.0, 1.0);
                b = pin_f32(b / a, 0.0, 1.0);
            }
            Color4f::new(r, g, b, a)
        }
        ColorType::Rgba10x6 => {
            let value = read_u64_ne(pixel);
            Color4f::new(
                (((value >> 6) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32),
                (((value >> 22) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32),
                (((value >> 38) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32),
                (((value >> 54) & 0x3FF) as f32) * (1.0_f32 / 1023.0_f32),
            )
        }
        ColorType::R16G16B16A16Unorm => {
            let value = read_u64_ne(pixel);
            let mut r = ((value & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let mut g = (((value >> 16) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let mut b = (((value >> 32) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            let a = (((value >> 48) & 0xFFFF) as f32) * (1.0_f32 / 65535.0_f32);
            if a != 0.0 && needs_unpremul {
                let inva = 1.0_f32 / a;
                r *= inva;
                g *= inva;
                b *= inva;
            }
            Color4f::new(r, g, b, a)
        }
        ColorType::RgbaF16Norm | ColorType::RgbaF16 => {
            let mut r = half_to_float(read_u16_ne(&pixel[0..2]));
            let mut g = half_to_float(read_u16_ne(&pixel[2..4]));
            let mut b = half_to_float(read_u16_ne(&pixel[4..6]));
            let a = half_to_float(read_u16_ne(&pixel[6..8]));
            if a != 0.0 && needs_unpremul {
                let inva = 1.0_f32 / a;
                r *= inva;
                g *= inva;
                b *= inva;
            }
            Color4f::new(r, g, b, a)
        }
        ColorType::RgbF16F16F16x => {
            let r = half_to_float(read_u16_ne(&pixel[0..2]));
            let g = half_to_float(read_u16_ne(&pixel[2..4]));
            let b = half_to_float(read_u16_ne(&pixel[4..6]));
            Color4f::new(r, g, b, 1.0_f32)
        }
        ColorType::RgbaF32 => {
            let mut r = read_f32_ne(&pixel[0..4]);
            let mut g = read_f32_ne(&pixel[4..8]);
            let mut b = read_f32_ne(&pixel[8..12]);
            let a = read_f32_ne(&pixel[12..16]);
            if a != 0.0 && needs_unpremul {
                let inva = 1.0_f32 / a;
                r *= inva;
                g *= inva;
                b *= inva;
            }
            Color4f::new(r, g, b, a)
        }
    }
}

/// Exact port of `SkPixmap::computeIsOpaque()`.
pub fn pixmap_compute_is_opaque(
    ct: ColorType,
    width: i32,
    height: i32,
    row_bytes: usize,
    pixels: &[u8],
) -> bool {
    let w = width.max(0) as usize;
    let h = height.max(0) as usize;

    match ct {
        ColorType::Alpha8 => {
            let mut a = 0xFFu8;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for &v in &row[..w] {
                    a &= v;
                }
                if a != 0xFF {
                    return false;
                }
            }
            true
        }
        ColorType::A16Unorm => {
            let mut a = 0xFFFFu16;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    a &= read_u16_ne(&row[x * 2..x * 2 + 2]);
                }
                if a != 0xFFFF {
                    return false;
                }
            }
            true
        }
        ColorType::A16Float => {
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    if read_u16_ne(&row[x * 2..x * 2 + 2]) < SK_HALF_1 {
                        return false;
                    }
                }
            }
            true
        }
        ColorType::Rgb565
        | ColorType::Gray8
        | ColorType::R8G8Unorm
        | ColorType::R16Unorm
        | ColorType::R16Float
        | ColorType::R16G16Unorm
        | ColorType::R16G16Float
        | ColorType::Rgb888x
        | ColorType::Bgr888x
        | ColorType::Rgb101010x
        | ColorType::Bgr101010x
        | ColorType::RgbF16F16F16x
        | ColorType::Bgr101010xXr
        | ColorType::R8Unorm => true,
        ColorType::Argb4444 => {
            let mut c = 0xFFFFu16;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    c &= read_u16_ne(&row[x * 2..x * 2 + 2]);
                }
                if (c & 0xF) != 0xF {
                    return false;
                }
            }
            true
        }
        ColorType::Bgra8888 | ColorType::Rgba8888 | ColorType::Srgba8888 => {
            let mut c = !0u32;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    c &= read_u32_ne(&row[x * 4..x * 4 + 4]);
                }
                if (c >> 24) != 0xFF {
                    return false;
                }
            }
            true
        }
        ColorType::RgbaF16Norm | ColorType::RgbaF16 => {
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    if read_u16_ne(&row[x * 8 + 6..x * 8 + 8]) < SK_HALF_1 {
                        return false;
                    }
                }
            }
            true
        }
        ColorType::RgbaF32 => {
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    if read_f32_ne(&row[x * 16 + 12..x * 16 + 16]) < 1.0_f32 {
                        return false;
                    }
                }
            }
            true
        }
        ColorType::Rgba1010102 | ColorType::Bgra1010102 => {
            let mut c = !0u32;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    c &= read_u32_ne(&row[x * 4..x * 4 + 4]);
                }
                if (c >> 30) != 0b11 {
                    return false;
                }
            }
            true
        }
        ColorType::Bgra10101010Xr => {
            const K_ONE: u64 = 510 + 384;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    if (read_u64_ne(&row[x * 8..x * 8 + 8]) >> 54) < K_ONE {
                        return false;
                    }
                }
            }
            true
        }
        ColorType::Rgba10x6 => {
            let mut acc = 0xFFC0u16;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    acc &= (read_u64_ne(&row[x * 8..x * 8 + 8]) >> 48) as u16;
                }
                if acc != 0xFFC0 {
                    return false;
                }
            }
            true
        }
        ColorType::R16G16B16A16Unorm => {
            let mut acc = 0xFFFFu16;
            for y in 0..h {
                let row = &pixels[y * row_bytes..];
                for x in 0..w {
                    acc &= (read_u64_ne(&row[x * 8..x * 8 + 8]) >> 48) as u16;
                }
                if acc != 0xFFFF {
                    return false;
                }
            }
            true
        }
        ColorType::Unknown => false,
    }
}

/// Exact port of `SkPixmap::erase` clip validation.
pub fn pixmap_erase_clip(
    ct: ColorType,
    width: i32,
    height: i32,
    subset: Option<IRect>,
) -> Option<IRect> {
    if ct == ColorType::Unknown {
        return None;
    }
    let bounds = IRect::from_wh(width, height);
    let clip = match subset {
        Some(sub) => {
            let mut r = IRect::make_empty();
            if !r.intersect_pair(bounds, sub) {
                return None;
            }
            r
        }
        None => bounds,
    };
    Some(clip)
}

/// Fills `clip` inside `pixels` with repeated copies of the converted `dst_pixel` bytes.
pub fn pixmap_fill_rect(
    ct: ColorType,
    pixels: &mut [u8],
    row_bytes: usize,
    clip: IRect,
    dst_pixel: &[u8; 16],
) {
    let shift = color_type_shift_per_pixel(ct) as u32;
    let bpp = 1usize << shift;
    if clip.is_empty() {
        return;
    }
    let count = clip.width() as usize;
    let left_bytes = (clip.left as usize) << shift;
    let pattern = &dst_pixel[..bpp];
    for y in clip.top..clip.bottom {
        let row_start = (y as usize) * row_bytes + left_bytes;
        let dst_row = &mut pixels[row_start..row_start + count * bpp];
        if bpp == 1 {
            dst_row.fill(pattern[0]);
        } else {
            for chunk in dst_row.chunks_exact_mut(bpp) {
                chunk.copy_from_slice(pattern);
            }
        }
    }
}

/// Decision for `SkPixmap::scalePixels` (`SkPixmapDraw.cpp`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScalePixelsAction {
    /// Empty src or dst: return false.
    Fail = 0,
    /// Same dimensions: delegate directly to `src.readPixels(dst)`.
    DirectCopy = 1,
    /// Perform shader-based scale draw.
    DrawScale = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalePixelsPlan {
    pub action: ScalePixelsAction,
    pub src_alpha_type: AlphaType,
    pub dst_alpha_type: AlphaType,
    pub clamp_as_if_unpremul: bool,
    pub scale_matrix: Matrix,
}

/// Exact port of the planning logic in `SkPixmap::scalePixels` (`SkPixmapDraw.cpp`).
pub fn pixmap_plan_scale(
    src_w: i32,
    src_h: i32,
    src_at: AlphaType,
    dst_w: i32,
    dst_h: i32,
    dst_at: AlphaType,
) -> ScalePixelsPlan {
    if src_w <= 0 || src_h <= 0 || dst_w <= 0 || dst_h <= 0 {
        return ScalePixelsPlan {
            action: ScalePixelsAction::Fail,
            src_alpha_type: src_at,
            dst_alpha_type: dst_at,
            clamp_as_if_unpremul: false,
            scale_matrix: Matrix::identity(),
        };
    }
    if src_w == dst_w && src_h == dst_h {
        return ScalePixelsPlan {
            action: ScalePixelsAction::DirectCopy,
            src_alpha_type: src_at,
            dst_alpha_type: dst_at,
            clamp_as_if_unpremul: false,
            scale_matrix: Matrix::identity(),
        };
    }

    let (eff_src_at, eff_dst_at, clamp_as_if_unpremul) =
        if src_at == AlphaType::Unpremul && dst_at == AlphaType::Unpremul {
            (AlphaType::Premul, AlphaType::Opaque, true)
        } else {
            (src_at, dst_at, false)
        };

    let src_rect = Rect::from_ltrb(0.0, 0.0, src_w as f32, src_h as f32);
    let dst_rect = Rect::from_ltrb(0.0, 0.0, dst_w as f32, dst_h as f32);
    let scale_matrix =
        Matrix::rect_to_rect(src_rect, dst_rect, ScaleToFit::Fill).unwrap_or_else(Matrix::identity);

    ScalePixelsPlan {
        action: ScalePixelsAction::DrawScale,
        src_alpha_type: eff_src_at,
        dst_alpha_type: eff_dst_at,
        clamp_as_if_unpremul,
        scale_matrix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_math::half::float_to_half;

    #[test]
    fn pixmap_rgba_and_bgra_get_color_and_alpha() {
        // Unpremul RGBA: R=0x10, G=0x20, B=0x30, A=0x80
        let px_rgba = [0x10u8, 0x20, 0x30, 0x80];
        for is_bgra in [true, false] {
            let c = pixmap_get_color(ColorType::Rgba8888, AlphaType::Unpremul, &px_rgba, is_bgra);
            assert_eq!(c, color_set_argb(0x80, 0x10, 0x20, 0x30));
            let c4 = pixmap_get_color4f(ColorType::Rgba8888, AlphaType::Unpremul, &px_rgba, is_bgra);
            assert!((c4.r - 0x10 as f32 / 255.0).abs() < 1e-6);
            assert!((c4.a - 0x80 as f32 / 255.0).abs() < 1e-6);
        }
        assert!((pixmap_get_alphaf(ColorType::Rgba8888, &px_rgba) - 128.0 / 255.0).abs() < 1e-6);

        // BGRA: B=0x30, G=0x20, R=0x10, A=0xFF
        let px_bgra = [0x30u8, 0x20, 0x10, 0xFF];
        for is_bgra in [true, false] {
            let c = pixmap_get_color(ColorType::Bgra8888, AlphaType::Opaque, &px_bgra, is_bgra);
            assert_eq!(c, color_set_argb(0xFF, 0x10, 0x20, 0x30));
        }
    }

    #[test]
    fn pixmap_f16_and_f32_get_color_and_opaque() {
        let mut f16_px = [0u8; 8];
        f16_px[0..2].copy_from_slice(&float_to_half(0.5).to_ne_bytes());
        f16_px[2..4].copy_from_slice(&float_to_half(0.25).to_ne_bytes());
        f16_px[4..6].copy_from_slice(&float_to_half(0.0).to_ne_bytes());
        f16_px[6..8].copy_from_slice(&float_to_half(0.5).to_ne_bytes());

        let c4 = pixmap_get_color4f(ColorType::RgbaF16, AlphaType::Premul, &f16_px, true);
        assert!((c4.r - 1.0).abs() < 1e-3);
        assert!((c4.g - 0.5).abs() < 1e-3);
        assert!((c4.b - 0.0).abs() < 1e-3);
        assert!((c4.a - 0.5).abs() < 1e-3);

        assert!(!pixmap_compute_is_opaque(ColorType::RgbaF16, 1, 1, 8, &f16_px));
        f16_px[6..8].copy_from_slice(&SK_HALF_1.to_ne_bytes());
        assert!(pixmap_compute_is_opaque(ColorType::RgbaF16, 1, 1, 8, &f16_px));
    }

    #[test]
    fn pixmap_fill_rect_and_opaque() {
        let mut buf = vec![0u8; 4 * 4 * 4];
        let mut pixel = [0u8; 16];
        pixel[0..4].copy_from_slice(&0xFF11_2233u32.to_ne_bytes());
        pixmap_fill_rect(ColorType::Rgba8888, &mut buf, 16, IRect::from_wh(4, 4), &pixel);
        assert!(pixmap_compute_is_opaque(ColorType::Rgba8888, 4, 4, 16, &buf));
        buf[3] = 0xFE;
        assert!(!pixmap_compute_is_opaque(ColorType::Rgba8888, 4, 4, 16, &buf));
    }

    #[test]
    fn pixmap_plan_scale_unpremul_trick() {
        let plan = pixmap_plan_scale(10, 10, AlphaType::Unpremul, 20, 20, AlphaType::Unpremul);
        assert_eq!(plan.action, ScalePixelsAction::DrawScale);
        assert_eq!(plan.src_alpha_type, AlphaType::Premul);
        assert_eq!(plan.dst_alpha_type, AlphaType::Opaque);
        assert!(plan.clamp_as_if_unpremul);
        assert_eq!(plan.scale_matrix.mat[0], 2.0);
        assert_eq!(plan.scale_matrix.mat[4], 2.0);

        let same = pixmap_plan_scale(10, 10, AlphaType::Premul, 10, 10, AlphaType::Premul);
        assert_eq!(same.action, ScalePixelsAction::DirectCopy);

        let empty = pixmap_plan_scale(0, 10, AlphaType::Premul, 10, 10, AlphaType::Premul);
        assert_eq!(empty.action, ScalePixelsAction::Fail);
    }

    #[test]
    fn pixmap_all_color_types_get_alpha_color_and_opaque() {
        // Gray8
        assert_eq!(pixmap_get_alphaf(ColorType::Gray8, &[0x80]), 1.0);
        assert_eq!(pixmap_get_color(ColorType::Gray8, AlphaType::Opaque, &[0x80], true), color_set_argb(0xFF, 0x80, 0x80, 0x80));
        assert!(pixmap_compute_is_opaque(ColorType::Gray8, 1, 1, 1, &[0x00]));

        // Alpha8
        assert!((pixmap_get_alphaf(ColorType::Alpha8, &[0xFF]) - 1.0).abs() < 1e-6);
        assert_eq!(pixmap_get_color(ColorType::Alpha8, AlphaType::Premul, &[0x7F], true), color_set_argb(0x7F, 0, 0, 0));
        assert!(!pixmap_compute_is_opaque(ColorType::Alpha8, 2, 1, 2, &[0xFF, 0xFE]));
        assert!(pixmap_compute_is_opaque(ColorType::Alpha8, 2, 1, 2, &[0xFF, 0xFF]));

        // Rgb565: R=31, G=0, B=0 -> 0xF800
        let px565 = 0xF800u16.to_ne_bytes();
        assert_eq!(pixmap_get_color(ColorType::Rgb565, AlphaType::Opaque, &px565, true), color_set_argb(0xFF, 0xFF, 0, 0));
        assert!(pixmap_compute_is_opaque(ColorType::Rgb565, 1, 1, 2, &px565));

        // Argb4444: R=0xF, G=0x0, B=0xF, A=0xF -> 0xF0FF
        let px4444 = 0xF0FFu16.to_ne_bytes();
        assert_eq!(pixmap_get_alphaf(ColorType::Argb4444, &px4444), 1.0);
        assert_eq!(pixmap_get_color(ColorType::Argb4444, AlphaType::Premul, &px4444, true), color_set_argb(0xFF, 0xFF, 0, 0xFF));
        assert!(pixmap_compute_is_opaque(ColorType::Argb4444, 1, 1, 2, &px4444));
        let px4444_trans = 0xF0F8u16.to_ne_bytes();
        assert!(!pixmap_compute_is_opaque(ColorType::Argb4444, 1, 1, 2, &px4444_trans));

        // Rgb888x / Bgr888x
        let px888x = [0x11u8, 0x22, 0x33, 0x00];
        assert_eq!(pixmap_get_alphaf(ColorType::Rgb888x, &px888x), 1.0);
        assert_eq!(pixmap_get_color(ColorType::Rgb888x, AlphaType::Opaque, &px888x, true), color_set_argb(0xFF, 0x11, 0x22, 0x33));
        assert_eq!(pixmap_get_color(ColorType::Bgr888x, AlphaType::Opaque, &px888x, true), color_set_argb(0xFF, 0x33, 0x22, 0x11));

        // Rgba1010102 & Bgra1010102
        let px10: u32 = (3u32 << 30) | (1023u32 << 20) | (512u32 << 10) | 0u32;
        let px10_b = px10.to_ne_bytes();
        assert_eq!(pixmap_get_alphaf(ColorType::Rgba1010102, &px10_b), 1.0);
        assert!(pixmap_compute_is_opaque(ColorType::Rgba1010102, 1, 1, 4, &px10_b));
        let c_rgba10 = pixmap_get_color4f(ColorType::Rgba1010102, AlphaType::Unpremul, &px10_b, true);
        let c_bgra10 = pixmap_get_color4f(ColorType::Bgra1010102, AlphaType::Unpremul, &px10_b, true);
        assert_eq!(c_rgba10.r, c_bgra10.b);
        assert_eq!(c_rgba10.b, c_bgra10.r);

        // R16G16B16A16Unorm
        let px64: u64 = (65535u64 << 48) | (65535u64 << 32) | (0u64 << 16) | 32768u64;
        let px64_b = px64.to_ne_bytes();
        assert!(pixmap_compute_is_opaque(ColorType::R16G16B16A16Unorm, 1, 1, 8, &px64_b));
        let c64 = pixmap_get_color4f(ColorType::R16G16B16A16Unorm, AlphaType::Unpremul, &px64_b, true);
        assert!((c64.r - 0.5).abs() < 1e-4);
        assert_eq!(c64.b, 1.0);
        assert_eq!(c64.a, 1.0);

        // Srgba8888
        let px_srgb = [0xFFu8, 0x00, 0x00, 0xFF];
        let c_srgb = pixmap_get_color4f(ColorType::Srgba8888, AlphaType::Premul, &px_srgb, true);
        assert!((c_srgb.r - 1.0).abs() < 1e-5);
        assert_eq!(c_srgb.g, 0.0);
        assert_eq!(c_srgb.a, 1.0);

        // RgbaF32
        let mut px_f32 = [0u8; 16];
        px_f32[0..4].copy_from_slice(&0.25f32.to_ne_bytes());
        px_f32[4..8].copy_from_slice(&0.5f32.to_ne_bytes());
        px_f32[8..12].copy_from_slice(&0.0f32.to_ne_bytes());
        px_f32[12..16].copy_from_slice(&0.5f32.to_ne_bytes());
        let cf32 = pixmap_get_color4f(ColorType::RgbaF32, AlphaType::Premul, &px_f32, true);
        assert!((cf32.r - 0.5).abs() < 1e-6);
        assert!((cf32.g - 1.0).abs() < 1e-6);
        assert!(!pixmap_compute_is_opaque(ColorType::RgbaF32, 1, 1, 16, &px_f32));

        // Unknown
        assert_eq!(pixmap_get_alphaf(ColorType::Unknown, &[]), 0.0);
        assert!(!pixmap_compute_is_opaque(ColorType::Unknown, 1, 1, 0, &[]));
    }

    #[test]
    fn pixmap_subset_and_erase_clip() {
        let sub = pixmap_extract_subset(10, 10, ColorType::Rgba8888, 48, IRect::make_ltrb(2, 3, 20, 8)).unwrap();
        assert_eq!(sub.clipped, IRect::make_ltrb(2, 3, 10, 8));
        assert_eq!(sub.byte_offset, 3 * 48 + 2 * 4);
        assert!(pixmap_extract_subset(10, 10, ColorType::Rgba8888, 40, IRect::make_ltrb(10, 0, 15, 5)).is_none());

        assert!(pixmap_erase_clip(ColorType::Unknown, 10, 10, None).is_none());
        assert_eq!(pixmap_erase_clip(ColorType::Alpha8, 10, 10, None), Some(IRect::from_wh(10, 10)));
        assert_eq!(
            pixmap_erase_clip(ColorType::Alpha8, 10, 10, Some(IRect::make_ltrb(-5, 2, 4, 20))),
            Some(IRect::make_ltrb(0, 2, 4, 10))
        );
        assert!(pixmap_erase_clip(ColorType::Alpha8, 10, 10, Some(IRect::make_ltrb(20, 20, 30, 30))).is_none());

        // Fill rect with 1-byte and 16-byte formats + padded rowBytes
        let mut buf1 = vec![0u8; 3 * 8];
        let mut p1 = [0u8; 16];
        p1[0] = 0xAB;
        pixmap_fill_rect(ColorType::Alpha8, &mut buf1, 8, IRect::make_ltrb(1, 1, 4, 3), &p1);
        assert_eq!(buf1[0], 0);
        assert_eq!(&buf1[8 + 1..8 + 4], &[0xAB, 0xAB, 0xAB]);
        assert_eq!(&buf1[16 + 1..16 + 4], &[0xAB, 0xAB, 0xAB]);
    }
}
