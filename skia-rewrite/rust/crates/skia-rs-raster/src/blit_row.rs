//! Safe Rust port of `src/core/SkBlitRow_D32.cpp`, `src/core/SkBlitRow_opts.cpp`,
//! and `src/opts/SkBlitRow_opts.h`.

use crate::memset::memset32;

/// Flags for `SkBlitRow::Factory32`.
pub const GLOBAL_ALPHA_FLAG32: u32 = 1 << 0;
pub const SRC_PIXEL_ALPHA_FLAG32: u32 = 1 << 1;

/// Exact port of `SkAlpha255To256(alpha)` = `alpha + 1`.
#[inline]
pub const fn alpha_255_to_256(alpha: u32) -> u32 {
    alpha + 1
}

/// Exact port of `SkAlphaMulQ(c, scale)`: scales all 4 bytes of a packed 32-bit
/// color by `scale` in `[0, 256]`.
#[inline]
pub const fn alpha_mul_q(c: u32, scale: u32) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    let rb = ((c & MASK).wrapping_mul(scale) >> 8) & MASK;
    let ag = (((c >> 8) & MASK).wrapping_mul(scale)) & !MASK;
    rb | ag
}

/// Exact port of `SkAlphaMulInv256(value, alpha256)`:
/// computes `256 - (value * alpha256) / 255` in `[0, 256]`.
#[inline]
pub const fn alpha_mul_inv256(value: u32, alpha256: u32) -> u32 {
    let prod = 0xFFFFu32.wrapping_sub(value.wrapping_mul(alpha256));
    (prod.wrapping_add(prod >> 8)) >> 8
}

/// Exact port of `SkAlphaBlend(src, dst, scale256)`: `dst + (((src - dst) * scale256) >> 8)`.
#[inline]
pub const fn alpha_blend(src: i32, dst: i32, scale256: i32) -> i32 {
    dst + (((src - dst) * scale256) >> 8)
}

/// Exact port of `SkFastFourByteInterp256(src, dst, scale)` (`scale` in `[0, 256]`).
/// Also matches `SkPMLerp`, `SkPMLerp_SSE2`, and `SkFourByteInterp256` bit-for-bit.
#[inline]
pub const fn fast_four_byte_interp256(src: u32, dst: u32, scale: u32) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    let inv = 256u32.wrapping_sub(scale);
    let rb = (src & MASK)
        .wrapping_mul(scale)
        .wrapping_add((dst & MASK).wrapping_mul(inv));
    let ag = ((src >> 8) & MASK)
        .wrapping_mul(scale)
        .wrapping_add(((dst >> 8) & MASK).wrapping_mul(inv));
    ((rb >> 8) & MASK) | (ag & !MASK)
}

/// Exact port of `SkFastFourByteInterp(src, dst, srcWeight)` (`srcWeight` in `[0, 255]`).
#[inline]
pub const fn fast_four_byte_interp(src: u32, dst: u32, src_weight: u32) -> u32 {
    fast_four_byte_interp256(src, dst, src_weight + (src_weight >> 7))
}

/// Exact port of `SkFourByteInterp(src, dst, srcWeight)` (`srcWeight` in `[0, 255]`).
#[inline]
pub const fn four_byte_interp(src: u32, dst: u32, src_weight: u32) -> u32 {
    fast_four_byte_interp256(src, dst, alpha_255_to_256(src_weight))
}

/// Exact port of `SkPMLerp(src, dst, scale)` (`scale` in `[0, 256]`).
#[inline]
pub const fn pm_lerp(src: u32, dst: u32, scale: u32) -> u32 {
    fast_four_byte_interp256(src, dst, scale)
}

/// Exact port of `SkBlendARGB32(src, dst, aa)` (`aa` in `[0, 255]`).
/// Bit-exact match with both scalar `SkBlendARGB32` and `SkBlendARGB32_SSE2`.
#[inline]
pub const fn blend_argb32(src: u32, dst: u32, aa: u32) -> u32 {
    let src_scale = alpha_255_to_256(aa);
    let dst_scale = alpha_mul_inv256(src >> 24, src_scale);

    const MASK: u32 = 0x00FF_00FF;
    let src_rb = (src & MASK).wrapping_mul(src_scale);
    let src_ag = ((src >> 8) & MASK).wrapping_mul(src_scale);
    let dst_rb = (dst & MASK).wrapping_mul(dst_scale);
    let dst_ag = ((dst >> 8) & MASK).wrapping_mul(dst_scale);

    (((src_rb.wrapping_add(dst_rb)) >> 8) & MASK) | ((src_ag.wrapping_add(dst_ag)) & !MASK)
}

/// Exact port of `SkPMSrcOver(src, dst)` (bit-exact with scalar, SSE2, and AVX2).
#[inline]
pub const fn pm_src_over(src: u32, dst: u32) -> u32 {
    let scale = alpha_255_to_256(255 - (src >> 24));

    const MASK: u32 = 0x00FF_00FF;
    let mut rb = (((dst & MASK).wrapping_mul(scale)) >> 8) & MASK;
    let mut ag = (((dst >> 8) & MASK).wrapping_mul(scale)) & !MASK;

    rb = rb.wrapping_add(src & MASK);
    ag = ag.wrapping_add(src & !MASK);

    let b0 = if (rb & 0x0000_01FF) < 0x0000_00FF {
        rb & 0x0000_01FF
    } else {
        0x0000_00FF
    };
    let b1 = if (ag & 0x0001_FF00) < 0x0000_FF00 {
        ag & 0x0001_FF00
    } else {
        0x0000_FF00
    };
    let b2 = if (rb & 0x01FF_0000) < 0x00FF_0000 {
        rb & 0x01FF_0000
    } else {
        0x00FF_0000
    };
    b0 | b1 | b2 | (ag & 0xFF00_0000)
}

/// Exact port of `blit_row_s32_opaque` (flags = 0).
#[inline]
pub fn blit_row_s32_opaque(dst: &mut [u32], src: &[u32]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}

/// Exact port of `blit_row_s32_blend` (flags = `kGlobalAlpha_Flag32`).
#[inline]
pub fn blit_row_s32_blend(dst: &mut [u32], src: &[u32], alpha: u32) {
    let scale = alpha_255_to_256(alpha & 0xFF);
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = pm_lerp(s, *d, scale);
    }
}

/// Exact port of `SkOpts::blit_row_s32a_opaque` (flags = `kSrcPixelAlpha_Flag32`).
#[inline]
pub fn blit_row_s32a_opaque(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = pm_src_over(s, *d);
    }
}

/// Exact port of `blit_row_s32a_blend` (flags = `kGlobalAlpha_Flag32 | kSrcPixelAlpha_Flag32`).
#[inline]
pub fn blit_row_s32a_blend(dst: &mut [u32], src: &[u32], alpha: u32) {
    let aa = alpha & 0xFF;
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = blend_argb32(s, *d, aa);
    }
}

/// Exact port of `SkOpts::blit_row_color32`.
/// Matches `src/opts/SkBlitRow_opts.h` per-byte `(mull(s, invA) >> 8) + c` truncation.
#[inline]
pub fn blit_row_color32(dst: &mut [u32], color: u32) {
    let inv_a = ((256u32 - (color >> 24)) & 0xFF) as u16;
    let c = color.to_le_bytes();
    for px in dst.iter_mut() {
        let d = px.to_le_bytes();
        let out = [
            (((d[0] as u16 * inv_a) >> 8) as u8).wrapping_add(c[0]),
            (((d[1] as u16 * inv_a) >> 8) as u8).wrapping_add(c[1]),
            (((d[2] as u16 * inv_a) >> 8) as u8).wrapping_add(c[2]),
            (((d[3] as u16 * inv_a) >> 8) as u8).wrapping_add(c[3]),
        ];
        *px = u32::from_le_bytes(out);
    }
}

/// Exact port of `SkBlitRow::Color32`.
#[inline]
pub fn color32(dst: &mut [u32], color: u32) {
    match color >> 24 {
        0 => {}
        255 => memset32(dst, color),
        _ => blit_row_color32(dst, color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pm_lerp_matches_sse2_and_alpha_blend_exhaustively_on_samples() {
        // Verify algebraic identity between fast_four_byte_interp256,
        // SkAlphaBlend, and SkPMLerp_SSE2 across all 256 alpha values and edge channels.
        let vals = [0u8, 1, 2, 17, 127, 128, 254, 255];
        for &s in &vals {
            for &d in &vals {
                for alpha in 0u32..=255 {
                    let scale = alpha_255_to_256(alpha);
                    let src = u32::from_le_bytes([s, s, s, s]);
                    let dst = u32::from_le_bytes([d, d, d, d]);
                    let got = pm_lerp(src, dst, scale).to_le_bytes()[0];

                    // Scalar SkAlphaBlend:
                    let expected_blend = alpha_blend(s as i32, d as i32, scale as i32) as u8;
                    // SSE2 SkPMLerp_SSE2:
                    let diff16 = (s as i16 - d as i16).wrapping_mul(scale as i16);
                    let expected_sse2 = d.wrapping_add(((diff16 as u16) >> 8) as u8);

                    assert_eq!(got, expected_blend);
                    assert_eq!(got, expected_sse2);
                }
            }
        }
    }

    #[test]
    fn pm_src_over_matches_per_byte_saturating_add() {
        let samples = [0u8, 1, 64, 127, 128, 200, 254, 255];
        for &sa in &samples {
            for &sc in &samples {
                for &da in &samples {
                    for &dc in &samples {
                        let src = u32::from_le_bytes([sc, sc, sc, sa]);
                        let dst = u32::from_le_bytes([dc, dc, dc, da]);
                        let got = pm_src_over(src, dst).to_le_bytes();

                        let scale = 256u16 - sa as u16;
                        let exp_c = sc.saturating_add(((dc as u16 * scale) >> 8) as u8);
                        let exp_a = sa.saturating_add(((da as u16 * scale) >> 8) as u8);
                        assert_eq!(got, [exp_c, exp_c, exp_c, exp_a]);
                    }
                }
            }
        }
    }

    #[test]
    fn blend_argb32_extremes() {
        let src = 0x8040_2010u32;
        let dst = 0xFF10_3050u32;
        assert_eq!(blend_argb32(src, dst, 0), dst);
        // aa = 255 -> src_scale = 256, dst_scale = alpha_mul_inv256(0x80, 256)
        let r255 = blend_argb32(src, dst, 255);
        assert_eq!(r255, pm_src_over(src, dst));
    }

    #[test]
    fn color32_transparent_opaque_translucent() {
        let mut buf = [0xFF11_2233u32; 8];
        color32(&mut buf, 0x00AA_BBCC);
        assert_eq!(buf, [0xFF11_2233; 8]);

        color32(&mut buf, 0xFF44_5566);
        assert_eq!(buf, [0xFF44_5566; 8]);

        let mut buf2 = [0xFF80_4020u32; 5];
        color32(&mut buf2, 0x8040_2010);
        let inv_a = 256 - 0x80;
        let expected = 0x8040_2010 + alpha_mul_q(0xFF80_4020, inv_a);
        assert_eq!(buf2, [expected; 5]);
    }

    #[test]
    fn row_blits_various_lengths() {
        let src = [0x8040_2010u32, 0xFF10_2030, 0x0000_0000, 0x4020_1008, 0xC060_3018];
        let mut dst = [0xFFAA_BBCCu32; 5];
        blit_row_s32_opaque(&mut dst, &src);
        assert_eq!(dst, src);

        let mut dst2 = [0xFFAA_BBCCu32; 5];
        blit_row_s32a_opaque(&mut dst2, &src);
        for i in 0..5 {
            assert_eq!(dst2[i], pm_src_over(src[i], 0xFFAA_BBCC));
        }

        let mut dst3 = [0xFFAA_BBCCu32; 5];
        blit_row_s32_blend(&mut dst3, &src, 128);
        for i in 0..5 {
            assert_eq!(dst3[i], pm_lerp(src[i], 0xFFAA_BBCC, 129));
        }

        let mut dst4 = [0xFFAA_BBCCu32; 5];
        blit_row_s32a_blend(&mut dst4, &src, 128);
        for i in 0..5 {
            assert_eq!(dst4[i], blend_argb32(src[i], 0xFFAA_BBCC, 128));
        }
    }
}
