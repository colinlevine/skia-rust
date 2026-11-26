//! Safe Rust port of `src/core/SkBlitter_ARGB32.cpp`.

use skia_rs_color::color::{
    color_get_a, color_get_b, color_get_g, color_get_r, pmcolor_get_a, pmcolor_get_b,
    pmcolor_get_g, pmcolor_get_r, pmcolor_set_argb, premultiply_color,
};

use crate::blit_mask::{approx_scale_u8, div255_u16};
use crate::blit_row::{
    alpha_255_to_256, alpha_mul_q, blend_argb32, blit_row_s32a_blend, blit_row_s32a_opaque,
    color32, fast_four_byte_interp, four_byte_interp,
};
use crate::memset::memset32;

/// Exact port of `upscale_31_to_32(value)` (`value` in `0..=31`).
#[inline]
pub const fn upscale_31_to_32(value: i32) -> i32 {
    value + (value >> 4)
}

/// Exact port of `blend_32(src, dst, scale)` (`scale` in `0..=32`).
#[inline]
pub const fn blend_32(src: i32, dst: i32, scale: i32) -> i32 {
    dst + (((src - dst) * scale) >> 5)
}

/// Exact port of `upscale_31_to_255(v)` used in shader LCD16 blending.
#[inline]
const fn upscale_31_to_255(v: i32) -> i32 {
    (v << 3) | (v >> 2)
}

/// Exact port of `src_alpha_blend` used in shader LCD16 blending.
#[inline]
const fn src_alpha_blend(s: i32, d: i32, sa: i32, m: i32) -> i32 {
    d + (((s - ((sa * d) >> 8)) * m) >> 8)
}

/// Exact port of `blend_lcd16` (`src_a256 = SkAlpha255To256(srcA)`).
#[inline]
pub const fn blend_lcd16(
    src_a256: i32,
    src_r: i32,
    src_g: i32,
    src_b: i32,
    dst: u32,
    mask: u16,
    is_bgra: bool,
) -> u32 {
    if mask == 0 {
        return dst;
    }

    let mut mask_r = ((mask >> 11) & 0x1F) as i32;
    let mut mask_g = (((mask >> 5) & 0x3F) >> 1) as i32;
    let mut mask_b = (mask & 0x1F) as i32;

    mask_r = upscale_31_to_32(mask_r);
    mask_g = upscale_31_to_32(mask_g);
    mask_b = upscale_31_to_32(mask_b);

    mask_r = (mask_r * src_a256) >> 8;
    mask_g = (mask_g * src_a256) >> 8;
    mask_b = (mask_b * src_a256) >> 8;

    let dst_a = pmcolor_get_a(dst) as i32;
    let dst_r = pmcolor_get_r(dst, is_bgra) as i32;
    let dst_g = pmcolor_get_g(dst) as i32;
    let dst_b = pmcolor_get_b(dst, is_bgra) as i32;

    let mask_a = if (src_a256 - 1) < dst_a {
        let m = if mask_g < mask_b { mask_g } else { mask_b };
        if mask_r < m { mask_r } else { m }
    } else {
        let m = if mask_g > mask_b { mask_g } else { mask_b };
        if mask_r > m { mask_r } else { m }
    };

    pmcolor_set_argb(
        blend_32(0xFF, dst_a, mask_a) as u32,
        blend_32(src_r, dst_r, mask_r) as u32,
        blend_32(src_g, dst_g, mask_g) as u32,
        blend_32(src_b, dst_b, mask_b) as u32,
        is_bgra,
    )
}

/// Exact port of `blend_lcd16_opaque`.
#[inline]
pub const fn blend_lcd16_opaque(
    src_r: i32,
    src_g: i32,
    src_b: i32,
    dst: u32,
    mask: u16,
    opaque_dst: u32,
    is_bgra: bool,
) -> u32 {
    if mask == 0 {
        return dst;
    }
    if mask == 0xFFFF {
        return opaque_dst;
    }

    let mask_r = upscale_31_to_32(((mask >> 11) & 0x1F) as i32);
    let mask_g = upscale_31_to_32((((mask >> 5) & 0x3F) >> 1) as i32);
    let mask_b = upscale_31_to_32((mask & 0x1F) as i32);

    let dst_a = pmcolor_get_a(dst) as i32;
    let dst_r = pmcolor_get_r(dst, is_bgra) as i32;
    let dst_g = pmcolor_get_g(dst) as i32;
    let dst_b = pmcolor_get_b(dst, is_bgra) as i32;

    let m = if mask_g > mask_b { mask_g } else { mask_b };
    let mask_a = if mask_r > m { mask_r } else { m };

    pmcolor_set_argb(
        blend_32(0xFF, dst_a, mask_a) as u32,
        blend_32(src_r, dst_r, mask_r) as u32,
        blend_32(src_g, dst_g, mask_g) as u32,
        blend_32(src_b, dst_b, mask_b) as u32,
        is_bgra,
    )
}

/// Exact port of `blit_row_lcd16`.
#[inline]
pub fn blit_row_lcd16(dst: &mut [u32], mask: &[u16], src: u32, is_bgra: bool) {
    let src_a = alpha_255_to_256(color_get_a(src)) as i32;
    let src_r = color_get_r(src) as i32;
    let src_g = color_get_g(src) as i32;
    let src_b = color_get_b(src) as i32;

    for (d, &m) in dst.iter_mut().zip(mask.iter()) {
        *d = blend_lcd16(src_a, src_r, src_g, src_b, *d, m, is_bgra);
    }
}

/// Exact port of `blit_row_lcd16_opaque`.
#[inline]
pub fn blit_row_lcd16_opaque(
    dst: &mut [u32],
    mask: &[u16],
    src: u32,
    opaque_dst: u32,
    is_bgra: bool,
) {
    let src_r = color_get_r(src) as i32;
    let src_g = color_get_g(src) as i32;
    let src_b = color_get_b(src) as i32;

    for (d, &m) in dst.iter_mut().zip(mask.iter()) {
        *d = blend_lcd16_opaque(src_r, src_g, src_b, *d, m, opaque_dst, is_bgra);
    }
}

/// Exact port of `blit_color` LCD16 branch (single row dispatch).
#[inline]
pub fn blit_row_lcd16_auto(dst: &mut [u32], mask: &[u16], color: u32, is_bgra: bool) {
    if color_get_a(color) == 0xFF {
        let opaque_dst = premultiply_color(color, is_bgra);
        blit_row_lcd16_opaque(dst, mask, color, opaque_dst, is_bgra);
    } else {
        blit_row_lcd16(dst, mask, color, is_bgra);
    }
}

/// Exact port of one row of `SkARGB32_BlitBW` (`SkBlitBWMaskTemplate.h` solid write).
///
/// `bits` starts at the byte containing `clip.left`, and `bit_offset = (clip.left - mask.left) & 7`.
#[inline]
pub fn blit_bw_row_solid(dst: &mut [u32], bits: &[u8], bit_offset: usize, color: u32) {
    let mut bit_idx = bit_offset;
    for d in dst.iter_mut() {
        let byte = bits[bit_idx >> 3];
        if (byte & (0x80u8 >> (bit_idx & 7))) != 0 {
            *d = color;
        }
        bit_idx += 1;
    }
}

/// Exact port of one row of `SkARGB32_BlendBW` (`SkBlitBWMaskTemplate.h` translucent blend).
///
/// `dst_scale = SkAlpha255To256(255 - srcA)`.
#[inline]
pub fn blit_bw_row_blend(
    dst: &mut [u32],
    bits: &[u8],
    bit_offset: usize,
    sc: u32,
    dst_scale: u32,
) {
    let mut bit_idx = bit_offset;
    for d in dst.iter_mut() {
        let byte = bits[bit_idx >> 3];
        if (byte & (0x80u8 >> (bit_idx & 7))) != 0 {
            *d = sc.wrapping_add(alpha_mul_q(*d, dst_scale));
        }
        bit_idx += 1;
    }
}

/// Exact port of one row of `SkARGB32_Blit32`.
#[inline]
pub fn blit_mask_argb32_row(dst: &mut [u32], src: &[u32], src_color: u32) {
    let alpha = src_color >> 24;
    if alpha == 255 {
        blit_row_s32a_opaque(dst, src);
    } else {
        blit_row_s32a_blend(dst, src, alpha);
    }
}

/// Exact port of a single run of `SkARGB32_Blitter::blitAntiH`.
#[inline]
pub fn argb32_blit_anti_run(dst: &mut [u32], pm_color: u32, aa: u8) {
    if aa != 0 {
        let sc = alpha_mul_q(pm_color, alpha_255_to_256(aa as u32));
        color32(dst, sc);
    }
}

/// Exact port of a single run of `SkARGB32_Opaque_Blitter::blitAntiH`.
#[inline]
pub fn argb32_opaque_blit_anti_run(dst: &mut [u32], pm_color: u32, aa: u8) {
    if aa == 255 {
        memset32(dst, pm_color);
    } else if aa > 0 {
        let sc = alpha_mul_q(pm_color, alpha_255_to_256(aa as u32));
        color32(dst, sc);
    }
}

/// Exact port of a single run of `SkARGB32_Black_Blitter::blitAntiH`.
#[inline]
pub fn argb32_black_blit_anti_run(dst: &mut [u32], aa: u8) {
    const BLACK: u32 = 0xFF00_0000;
    if aa == 255 {
        memset32(dst, BLACK);
    } else if aa > 0 {
        let src = (aa as u32) << 24;
        let dst_scale = alpha_255_to_256(255 - aa as u32);
        for d in dst.iter_mut() {
            *d = src.wrapping_add(alpha_mul_q(*d, dst_scale));
        }
    }
}

/// Exact port of `SkARGB32_Blitter::blitAntiH2` pixel blend.
#[inline]
pub const fn argb32_blend_pixel_aa(pm_color: u32, dst: u32, aa: u32) -> u32 {
    blend_argb32(pm_color, dst, aa)
}

/// Exact port of `SkARGB32_Opaque_Blitter::blitAntiH2` / `blitAntiV2` pixel blend.
#[inline]
pub const fn argb32_opaque_blend_pixel_aa(pm_color: u32, dst: u32, aa: u32) -> u32 {
    fast_four_byte_interp(pm_color, dst, aa)
}

/// Exact port of `SkARGB32_Black_Blitter::blitAntiH2` / `blitAntiV2` pixel blend.
#[inline]
pub const fn argb32_black_blend_pixel_aa(dst: u32, aa: u32) -> u32 {
    (aa << 24).wrapping_add(alpha_mul_q(dst, 256 - aa))
}

/// Precompute `(scaled_color, dst_scale)` for `SkARGB32_Blitter::blitV`.
#[inline]
pub const fn argb32_blit_v_params(pm_color: u32, alpha: u8) -> (u32, u32) {
    let color = if alpha != 255 {
        alpha_mul_q(pm_color, alpha_255_to_256(alpha as u32))
    } else {
        pm_color
    };
    let dst_scale = alpha_255_to_256(255 - (color >> 24));
    (color, dst_scale)
}

/// Blend a single pixel in `SkARGB32_Blitter::blitV`.
#[inline]
pub const fn argb32_blit_v_pixel(dst: u32, color: u32, dst_scale: u32) -> u32 {
    color.wrapping_add(alpha_mul_q(dst, dst_scale))
}

/// Exact port of `SkARGB32_Shader_Blitter::blitMask` `blend_row_A8`.
pub fn shader_blend_row_a8(dst: &mut [u32], mask: &[u8], src: &[u32]) {
    for ((d, &m), &s) in dst.iter_mut().zip(mask.iter()).zip(src.iter()) {
        let sb = s.to_le_bytes();
        let db = d.to_le_bytes();
        let s_aa = [
            approx_scale_u8(sb[0], m),
            approx_scale_u8(sb[1], m),
            approx_scale_u8(sb[2], m),
            approx_scale_u8(sb[3], m),
        ];
        let inv_a = 255u8 - s_aa[3];
        let out = [
            s_aa[0].wrapping_add(approx_scale_u8(db[0], inv_a)),
            s_aa[1].wrapping_add(approx_scale_u8(db[1], inv_a)),
            s_aa[2].wrapping_add(approx_scale_u8(db[2], inv_a)),
            s_aa[3].wrapping_add(approx_scale_u8(db[3], inv_a)),
        ];
        *d = u32::from_le_bytes(out);
    }
}

/// Exact port of `SkARGB32_Shader_Blitter::blitMask` `blend_row_A8_opaque`.
pub fn shader_blend_row_a8_opaque(dst: &mut [u32], mask: &[u8], src: &[u32]) {
    for ((d, &m), &s) in dst.iter_mut().zip(mask.iter()).zip(src.iter()) {
        let sb = s.to_le_bytes();
        let db = d.to_le_bytes();
        let c = m as u16;
        let inv_c = (255 - m) as u16;
        let out = [
            div255_u16(sb[0] as u16 * c + db[0] as u16 * inv_c),
            div255_u16(sb[1] as u16 * c + db[1] as u16 * inv_c),
            div255_u16(sb[2] as u16 * c + db[2] as u16 * inv_c),
            div255_u16(sb[3] as u16 * c + db[3] as u16 * inv_c),
        ];
        *d = u32::from_le_bytes(out);
    }
}

/// Exact port of `SkARGB32_Shader_Blitter::blitMask` `blend_row_lcd16`.
pub fn shader_blend_row_lcd16(dst: &mut [u32], mask: &[u16], src: &[u32], is_bgra: bool) {
    for ((d_ref, &m), &s) in dst.iter_mut().zip(mask.iter()).zip(src.iter()) {
        if m == 0 {
            continue;
        }
        let d = *d_ref;

        let mut src_a = pmcolor_get_a(s) as i32;
        let src_r = pmcolor_get_r(s, is_bgra) as i32;
        let src_g = pmcolor_get_g(s) as i32;
        let src_b = pmcolor_get_b(s, is_bgra) as i32;

        src_a += src_a >> 7;

        let mask_r = upscale_31_to_255(((m >> 11) & 0x1F) as i32);
        let mask_g = upscale_31_to_255((((m >> 5) & 0x3F) >> 1) as i32);
        let mask_b = upscale_31_to_255((m & 0x1F) as i32);

        let dst_r = pmcolor_get_r(d, is_bgra) as i32;
        let dst_g = pmcolor_get_g(d) as i32;
        let dst_b = pmcolor_get_b(d, is_bgra) as i32;

        *d_ref = pmcolor_set_argb(
            0xFF,
            src_alpha_blend(src_r, dst_r, src_a, mask_r) as u32,
            src_alpha_blend(src_g, dst_g, src_a, mask_g) as u32,
            src_alpha_blend(src_b, dst_b, src_a, mask_b) as u32,
            is_bgra,
        );
    }
}

/// Exact port of `SkARGB32_Shader_Blitter::blitMask` `blend_row_LCD16_opaque`.
pub fn shader_blend_row_lcd16_opaque(dst: &mut [u32], mask: &[u16], src: &[u32], is_bgra: bool) {
    for ((d_ref, &m), &s) in dst.iter_mut().zip(mask.iter()).zip(src.iter()) {
        if m == 0 {
            continue;
        }
        let d = *d_ref;

        let src_r = pmcolor_get_r(s, is_bgra) as i32;
        let src_g = pmcolor_get_g(s) as i32;
        let src_b = pmcolor_get_b(s, is_bgra) as i32;

        let mask_r = upscale_31_to_32(((m >> 11) & 0x1F) as i32);
        let mask_g = upscale_31_to_32((((m >> 5) & 0x3F) >> 1) as i32);
        let mask_b = upscale_31_to_32((m & 0x1F) as i32);

        let dst_r = pmcolor_get_r(d, is_bgra) as i32;
        let dst_g = pmcolor_get_g(d) as i32;
        let dst_b = pmcolor_get_b(d, is_bgra) as i32;

        *d_ref = pmcolor_set_argb(
            0xFF,
            blend_32(src_r, dst_r, mask_r) as u32,
            blend_32(src_g, dst_g, mask_g) as u32,
            blend_32(src_b, dst_b, mask_b) as u32,
            is_bgra,
        );
    }
}

/// Exact port of `SkARGB32_Shader_Blitter::blitV` direct opaque non-255 pixel blend.
#[inline]
pub const fn shader_blit_v_interp(src: u32, dst: u32, alpha: u32) -> u32 {
    four_byte_interp(src, dst, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcd16_zero_and_full_masks() {
        let dst = 0xFF10_2030u32;
        let color = 0xFFAA_BBCCu32;
        for is_bgra in [true, false] {
            let opaque_dst = premultiply_color(color, is_bgra);
            assert_eq!(
                blend_lcd16_opaque(0xAA, 0xBB, 0xCC, dst, 0, opaque_dst, is_bgra),
                dst
            );
            assert_eq!(
                blend_lcd16_opaque(0xAA, 0xBB, 0xCC, dst, 0xFFFF, opaque_dst, is_bgra),
                opaque_dst
            );
            assert_eq!(
                blend_lcd16(256, 0xAA, 0xBB, 0xCC, dst, 0, is_bgra),
                dst
            );
            assert_eq!(
                blend_lcd16(256, 0xAA, 0xBB, 0xCC, dst, 0xFFFF, is_bgra),
                opaque_dst
            );
        }
    }

    #[test]
    fn bw_row_solid_and_blend_respect_bit_offset() {
        // bits: 0b1011_0100, 0b0110_0000 -> starting at bit_offset=2, width=8:
        // bits[2..10] = 1,1,0,1,0,0, 0,1
        let bits = [0b1011_0100u8, 0b0110_0000u8];
        let mut dst = [0x1111_1111u32; 8];
        blit_bw_row_solid(&mut dst, &bits, 2, 0xFFFF_FFFF);
        assert_eq!(
            dst,
            [
                0xFFFF_FFFF,
                0xFFFF_FFFF,
                0x1111_1111,
                0xFFFF_FFFF,
                0x1111_1111,
                0x1111_1111,
                0x1111_1111,
                0xFFFF_FFFF,
            ]
        );

        let mut dst2 = [0xFF80_4020u32; 8];
        let sc = 0x8040_2010u32;
        let scale = 256 - 0x80;
        let expected = sc + alpha_mul_q(0xFF80_4020, scale);
        blit_bw_row_blend(&mut dst2, &bits, 2, sc, scale);
        assert_eq!(dst2[0], expected);
        assert_eq!(dst2[1], expected);
        assert_eq!(dst2[2], 0xFF80_4020);
        assert_eq!(dst2[3], expected);
        assert_eq!(dst2[7], expected);
    }

    #[test]
    fn shader_a8_and_lcd16_rows() {
        let src = [0x8040_2010u32, 0xFF80_4020];
        let mask_a8 = [0u8, 255];
        let mut dst = [0xFF10_2030u32; 2];
        shader_blend_row_a8(&mut dst, &mask_a8, &src);
        assert_eq!(dst[0], 0xFF10_2030);
        assert_eq!(dst[1], 0xFF80_4020);

        let mut dst_op = [0xFF10_2030u32; 2];
        shader_blend_row_a8_opaque(&mut dst_op, &mask_a8, &src);
        assert_eq!(dst_op[0], 0xFF10_2030);
        assert_eq!(dst_op[1], 0xFF80_4020);

        let mask_lcd = [0u16, 0xFFFF];
        let mut dst_lcd = [0xFF10_2030u32; 2];
        shader_blend_row_lcd16_opaque(&mut dst_lcd, &mask_lcd, &src, false);
        assert_eq!(dst_lcd[0], 0xFF10_2030);
        assert_eq!(dst_lcd[1], 0xFF80_4020);

        let mut dst_lcd2 = [0xFF10_2030u32; 2];
        shader_blend_row_lcd16(&mut dst_lcd2, &mask_lcd, &src, false);
        assert_eq!(dst_lcd2[0], 0xFF10_2030);
        // Exact SkAlphaMul(s - SkAlphaMul(256, d), 255) behavior on full mask 0xFFFF:
        let exp_r = src_alpha_blend(0x20, 0x30, 256, 255) as u32;
        let exp_g = src_alpha_blend(0x40, 0x20, 256, 255) as u32;
        let exp_b = src_alpha_blend(0x80, 0x10, 256, 255) as u32;
        assert_eq!(dst_lcd2[1], pmcolor_set_argb(0xFF, exp_r, exp_g, exp_b, false));
    }

    #[test]
    fn argb32_black_and_opaque_anti_runs() {
        let mut dst = [0xFFFF_FFFFu32; 4];
        argb32_black_blit_anti_run(&mut dst, 255);
        assert_eq!(dst, [0xFF00_0000; 4]);

        let mut dst2 = [0xFFFF_FFFFu32; 4];
        argb32_opaque_blit_anti_run(&mut dst2, 0xFF12_3456, 255);
        assert_eq!(dst2, [0xFF12_3456; 4]);
    }
}
