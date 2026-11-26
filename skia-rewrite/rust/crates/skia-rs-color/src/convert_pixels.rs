//! Safe Rust port of `src/core/SkConvertPixels.cpp` fast paths and validation.

use crate::color_space_xform_steps::XformStepsFlags;
use crate::image_info::{color_type_bytes_per_pixel, ColorType};
use skia_rs_math::half::half_to_float;

/// Outcome of the Rust fast-path pixel conversion check/execution.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConvertFastPath {
    /// Strides or in-place layout are invalid; caller must return `false`.
    Invalid = 0,
    /// Conversion was completed by a fast path (memcpy / swizzle / convert_to_alpha8).
    Done = 1,
    /// Valid inputs, but requires `SkRasterPipeline` fallback.
    NeedsPipeline = 2,
}

/// Action selected when no color matrix is present.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FastPathKind {
    None = 0,
    RectMemcpy = 1,
    SwizzleRgbaToBgra = 2,
    SwizzleRgbaToRgbaPremul = 3,
    SwizzleRgbaToBgraPremul = 4,
    SwizzleRgbaPremulToRgba = 5,
    SwizzleRgbaPremulToBgra = 6,
    ConvertToAlpha8 = 7,
}

/// Validate strides and in-place aliasing constraint from `convert_pixels_common`.
/// Returns `Some((src_stride_pixels, dst_stride_pixels))` if valid.
pub fn validate_convert_strides(
    src_ct: ColorType,
    src_rb: usize,
    dst_ct: ColorType,
    dst_rb: usize,
    same_buffer: bool,
) -> Option<(i32, i32)> {
    let src_bpp = color_type_bytes_per_pixel(src_ct) as usize;
    let dst_bpp = color_type_bytes_per_pixel(dst_ct) as usize;
    if src_bpp == 0 || dst_bpp == 0 {
        return None;
    }
    let src_stride = (src_rb / src_bpp) as i32;
    let dst_stride = (dst_rb / dst_bpp) as i32;
    if (src_stride as usize).wrapping_mul(src_bpp) != src_rb
        || (dst_stride as usize).wrapping_mul(dst_bpp) != dst_rb
    {
        return None;
    }
    if same_buffer && src_bpp != dst_bpp {
        return None;
    }
    Some((src_stride, dst_stride))
}

/// Select which fast path applies (when `src_matrix` and `dst_matrix` are both absent).
pub fn select_fast_path(
    src_ct: ColorType,
    dst_ct: ColorType,
    flags: XformStepsFlags,
    allow_unpremul_swizzle: bool,
) -> FastPathKind {
    // 1. rect_memcpy
    if dst_ct == src_ct && (dst_ct == ColorType::Alpha8 || flags.mask() == 0) {
        return FastPathKind::RectMemcpy;
    }

    // 2. swizzle_or_premul
    let is_8888 = |ct: ColorType| matches!(ct, ColorType::Rgba8888 | ColorType::Bgra8888);
    if is_8888(dst_ct)
        && is_8888(src_ct)
        && !flags.linearize
        && !flags.gamut_transform
        && (allow_unpremul_swizzle || !flags.unpremul)
        && !flags.encode
    {
        let swap_rb = dst_ct != src_ct;
        if flags.premul {
            return if swap_rb {
                FastPathKind::SwizzleRgbaToBgraPremul
            } else {
                FastPathKind::SwizzleRgbaToRgbaPremul
            };
        } else if flags.unpremul {
            return if swap_rb {
                FastPathKind::SwizzleRgbaPremulToBgra
            } else {
                FastPathKind::SwizzleRgbaPremulToRgba
            };
        } else if swap_rb {
            return FastPathKind::SwizzleRgbaToBgra;
        }
    }

    // 3. convert_to_alpha8
    if dst_ct == ColorType::Alpha8 && !matches!(src_ct, ColorType::Unknown | ColorType::Alpha8) {
        return FastPathKind::ConvertToAlpha8;
    }

    FastPathKind::None
}

/// Safe row-by-row `convert_to_alpha8` for a single row (`width` pixels).
/// Returns `true` if `src_ct` was handled.
pub fn convert_row_to_alpha8(src_ct: ColorType, src_row: &[u8], dst_row: &mut [u8], width: usize) -> bool {
    let dst = &mut dst_row[..width];
    match src_ct {
        ColorType::Unknown | ColorType::Alpha8 => false,

        ColorType::A16Unorm => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(2)) {
                let v = u16::from_ne_bytes([chunk[0], chunk[1]]);
                *d = (v >> 8) as u8;
            }
            true
        }

        ColorType::Gray8
        | ColorType::Rgb565
        | ColorType::R8G8Unorm
        | ColorType::R16Unorm
        | ColorType::R16Float
        | ColorType::R16G16Unorm
        | ColorType::R16G16Float
        | ColorType::Rgb888x
        | ColorType::Bgr888x
        | ColorType::Rgb101010x
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::RgbF16F16F16x
        | ColorType::R8Unorm => {
            dst.fill(0xFF);
            true
        }

        ColorType::Argb4444 => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(2)) {
                let v = u16::from_ne_bytes([chunk[0], chunk[1]]);
                let nib = (v & 0xF) as u8;
                *d = (nib << 4) | nib;
            }
            true
        }

        ColorType::Bgra8888 | ColorType::Rgba8888 | ColorType::Srgba8888 => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(4)) {
                let v = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                *d = (v >> 24) as u8;
            }
            true
        }

        ColorType::Rgba1010102 | ColorType::Bgra1010102 => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(4)) {
                let v = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                *d = ((v >> 30) * 0x55) as u8;
            }
            true
        }

        ColorType::RgbaF16Norm | ColorType::RgbaF16 => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(8)) {
                let v = u64::from_ne_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                let h = (v >> 48) as u16;
                *d = (255.0_f32 * half_to_float(h)) as u8;
            }
            true
        }

        ColorType::RgbaF32 => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(16)) {
                let a = f32::from_ne_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);
                *d = (255.0_f32 * a) as u8;
            }
            true
        }

        ColorType::A16Float => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(2)) {
                let h = u16::from_ne_bytes([chunk[0], chunk[1]]);
                *d = (255.0_f32 * half_to_float(h)) as u8;
            }
            true
        }

        ColorType::Bgra10101010Xr => {
            const ZERO: i64 = 384;
            const RANGE: i64 = 510;
            const MAX_U8: i64 = 0xFF;
            const MIN_U8: i64 = 0x00;
            const DIVISOR: i64 = RANGE / MAX_U8;
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(8)) {
                let v = u64::from_ne_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                let raw_alpha = (v >> 54) as i64;
                let alpha = ((raw_alpha - ZERO) / DIVISOR).clamp(MIN_U8, MAX_U8);
                *d = alpha as u8;
            }
            true
        }

        ColorType::Rgba10x6 | ColorType::R16G16B16A16Unorm => {
            for (d, chunk) in dst.iter_mut().zip(src_row.chunks_exact(8)) {
                let v = u64::from_ne_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                *d = ((v >> 48) >> 8) as u8;
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_validation() {
        assert_eq!(
            validate_convert_strides(ColorType::Rgba8888, 16, ColorType::Bgra8888, 16, true),
            Some((4, 4))
        );
        // Unaligned rowBytes rejected
        assert_eq!(
            validate_convert_strides(ColorType::Rgba8888, 15, ColorType::Bgra8888, 16, false),
            None
        );
        // In-place different bpp rejected
        assert_eq!(
            validate_convert_strides(ColorType::Rgba8888, 16, ColorType::Alpha8, 4, true),
            None
        );
    }

    #[test]
    fn fast_path_selection() {
        let empty = XformStepsFlags::default();
        assert_eq!(
            select_fast_path(ColorType::Rgba8888, ColorType::Rgba8888, empty, false),
            FastPathKind::RectMemcpy
        );
        assert_eq!(
            select_fast_path(ColorType::Rgba8888, ColorType::Bgra8888, empty, false),
            FastPathKind::SwizzleRgbaToBgra
        );
        assert_eq!(
            select_fast_path(ColorType::Rgba8888, ColorType::Alpha8, empty, false),
            FastPathKind::ConvertToAlpha8
        );
    }

    #[test]
    fn convert_alpha8_rows() {
        let mut dst = [0u8; 2];
        let src_rgba = [0x11u8, 0x22, 0x33, 0xAB, 0x44, 0x55, 0x66, 0xCD];
        assert!(convert_row_to_alpha8(ColorType::Rgba8888, &src_rgba, &mut dst, 2));
        assert_eq!(dst, [0xAB, 0xCD]);

        let src_565 = [0u8; 4];
        assert!(convert_row_to_alpha8(ColorType::Rgb565, &src_565, &mut dst, 2));
        assert_eq!(dst, [0xFF, 0xFF]);

        let p0 = (3u32 << 30).to_ne_bytes();
        let p1 = (1u32 << 30).to_ne_bytes();
        let src_1010102 = [p0[0], p0[1], p0[2], p0[3], p1[0], p1[1], p1[2], p1[3]];
        assert!(convert_row_to_alpha8(ColorType::Rgba1010102, &src_1010102, &mut dst, 2));
        assert_eq!(dst, [0xFF, 0x55]);
    }
}
