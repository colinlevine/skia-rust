//! Safe Rust port of `src/core/SkBlitMask_opts.cpp` and `src/opts/SkBlitMask_opts.h`.

use skia_rs_color::color::{color_get_a, premultiply_color};

use crate::blit_row::alpha_mul_q;

/// `SK_ColorBLACK` = `0xFF000000`.
pub const COLOR_BLACK: u32 = 0xFF00_0000;

/// Exact port of `skvx::approx_scale(x, y)` = `(x * y + x) / 256` = `(x * (y + 1)) >> 8`.
#[inline]
pub const fn approx_scale_u8(x: u8, y: u8) -> u8 {
    (((x as u16) * (y as u16) + (x as u16)) >> 8) as u8
}

/// Exact port of `skvx::div255(x)` = `(x + 127) / 255`.
#[inline]
pub const fn div255_u16(x: u16) -> u8 {
    ((x as u32 + 127) / 255) as u8
}

/// Exact port of `blit_mask_d32_a8_black` for a single row.
///
/// Formula (matching `SkBlitMask_opts.h` both `Sk4px` and NEON):
/// `d' = (aa << 24) + SkAlphaMulQ(d, 256 - aa)`
#[inline]
pub fn blit_mask_d32_a8_row_black(dst: &mut [u32], mask: &[u8]) {
    for (d, &aa) in dst.iter_mut().zip(mask.iter()) {
        let a = aa as u32;
        *d = (a << 24) + alpha_mul_q(*d, 256 - a);
    }
}

/// Exact port of `blit_mask_d32_a8_opaque` for a single row (`pmc` is premul opaque color).
///
/// Formula (matching `SkBlitMask_opts.h` both `Sk4px` and NEON):
/// `d' = SkAlphaMulQ(pmc, aa + 1) + SkAlphaMulQ(d, 256 - aa)`
#[inline]
pub fn blit_mask_d32_a8_row_opaque(dst: &mut [u32], mask: &[u8], pmc: u32) {
    for (d, &aa) in dst.iter_mut().zip(mask.iter()) {
        let a = aa as u32;
        *d = alpha_mul_q(pmc, a + 1) + alpha_mul_q(*d, 256 - a);
    }
}

/// Exact port of `blit_mask_d32_a8_general` for a single row (`pmc` is premul color).
///
/// Formula (matching `SkBlitMask_opts.h` both `Sk4px` and NEON):
/// `left = SkAlphaMulQ(pmc, aa + 1)`
/// `right = SkAlphaMulQ(d, 256 - (left >> 24))`
/// `d' = left + right`
#[inline]
pub fn blit_mask_d32_a8_row_general(dst: &mut [u32], mask: &[u8], pmc: u32) {
    for (d, &aa) in dst.iter_mut().zip(mask.iter()) {
        let left = alpha_mul_q(pmc, aa as u32 + 1);
        let scale = 256 - (left >> 24);
        *d = left + alpha_mul_q(*d, scale);
    }
}

/// Dispatch mode for `blit_mask_d32_a8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlitMaskMode {
    Black,
    Opaque(u32),
    General(u32),
}

/// Precompute the premultiplied color and specialized mode for `blit_mask_d32_a8`.
#[inline]
pub const fn blit_mask_mode(color: u32, is_bgra: bool) -> BlitMaskMode {
    if color == COLOR_BLACK {
        BlitMaskMode::Black
    } else if color_get_a(color) == 0xFF {
        BlitMaskMode::Opaque(premultiply_color(color, is_bgra))
    } else {
        BlitMaskMode::General(premultiply_color(color, is_bgra))
    }
}

/// Blit a single A8 mask row onto a 32-bit premultiplied destination row.
#[inline]
pub fn blit_mask_d32_a8_row(dst: &mut [u32], mask: &[u8], mode: BlitMaskMode) {
    match mode {
        BlitMaskMode::Black => blit_mask_d32_a8_row_black(dst, mask),
        BlitMaskMode::Opaque(pmc) => blit_mask_d32_a8_row_opaque(dst, mask, pmc),
        BlitMaskMode::General(pmc) => blit_mask_d32_a8_row_general(dst, mask, pmc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blit_row::color32;

    #[test]
    fn blit_mask_matches_anti_h_for_black_opaque_and_translucent() {
        // Mirrors Skia's CoreBlittersTest: blitAntiH and blitMask must be bit-identical
        // for all 256 mask alpha values across black, opaque, and translucent colors.
        let bg_colors = [
            0xFFFF_FFFFu32,
            0xFF00_0000,
            0xFF88_8888,
            0xFFFF_0000,
            0xFF00_FF00,
            0xFF00_00FF,
            0xFF3F_557F,
        ];
        let paint_colors = [
            0xFF00_0000u32,
            0xFFFF_FFFF,
            0xFFFF_0000,
            0xFF3F_557F,
            0x00FF_0000,
            0x0A3F_557F,
            0x64FF_8040,
            0xC83F_557F,
            0xF512_3456,
        ];

        for is_bgra in [true, false] {
            for &bg_color in &bg_colors {
                let bg_pm = premultiply_color(bg_color, is_bgra);
                for &paint_color in &paint_colors {
                    let pmc = premultiply_color(paint_color, is_bgra);
                    let src_a = color_get_a(paint_color);
                    let mode = blit_mask_mode(paint_color, is_bgra);

                    for aa in 0u8..=255 {
                        let mut dst_mask = [bg_pm; 19];
                        let mask = [aa; 19];
                        blit_mask_d32_a8_row(&mut dst_mask, &mask, mode);

                        let mut dst_anti = [bg_pm; 19];
                        if paint_color == COLOR_BLACK {
                            if aa == 255 {
                                dst_anti.fill(0xFF00_0000);
                            } else if aa > 0 {
                                let src = (aa as u32) << 24;
                                let dst_scale = 256 - aa as u32;
                                for d in &mut dst_anti {
                                    *d = src + alpha_mul_q(*d, dst_scale);
                                }
                            }
                        } else if src_a == 0xFF {
                            if aa == 255 {
                                dst_anti.fill(pmc);
                            } else if aa > 0 {
                                let sc = alpha_mul_q(pmc, aa as u32 + 1);
                                color32(&mut dst_anti, sc);
                            }
                        } else if src_a > 0 && aa > 0 {
                            let sc = alpha_mul_q(pmc, aa as u32 + 1);
                            color32(&mut dst_anti, sc);
                        }

                        assert_eq!(
                            dst_mask, dst_anti,
                            "mismatch is_bgra={is_bgra} bg={bg_color:#010x} paint={paint_color:#010x} aa={aa}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn approx_scale_exact_at_endpoints() {
        for x in 0u8..=255 {
            assert_eq!(approx_scale_u8(x, 0), 0);
            assert_eq!(approx_scale_u8(x, 255), x);
            assert_eq!(approx_scale_u8(0, x), 0);
            assert_eq!(approx_scale_u8(255, x), x);
        }
    }
}
