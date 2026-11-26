//! Safe Rust port of `src/core/SkSwizzle.cpp`, `src/core/SkSwizzler_opts.cpp`,
//! and `src/opts/SkSwizzler_opts.inc`.

use skia_rs_geometry::cpp_min_f32;

#[inline]
const fn premul_byte(c: u32, a: u32) -> u32 {
    (c * a + 127) / 255
}

#[inline]
const fn swap_rb_pixel(p: u32) -> u32 {
    let a = (p >> 24) & 0xFF;
    let b = (p >> 16) & 0xFF;
    let g = (p >> 8) & 0xFF;
    let r = p & 0xFF;
    (a << 24) | (r << 16) | (g << 8) | b
}

#[inline]
const fn rgba_to_rgba_premul_pixel(p: u32) -> u32 {
    let a = (p >> 24) & 0xFF;
    let b = premul_byte((p >> 16) & 0xFF, a);
    let g = premul_byte((p >> 8) & 0xFF, a);
    let r = premul_byte(p & 0xFF, a);
    (a << 24) | (b << 16) | (g << 8) | r
}

#[inline]
const fn rgba_to_bgra_premul_pixel(p: u32) -> u32 {
    let a = (p >> 24) & 0xFF;
    let b = premul_byte((p >> 16) & 0xFF, a);
    let g = premul_byte((p >> 8) & 0xFF, a);
    let r = premul_byte(p & 0xFF, a);
    (a << 24) | (r << 16) | (g << 8) | b
}

#[inline]
fn pixel_round_as_rp(n: f32) -> u32 {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64"))]
    {
        n.round_ties_even() as u32
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")))]
    {
        (n + 0.5_f32) as u32
    }
}

#[inline]
fn unpremul_simulating_rp(reciprocal_a: f32, c: f32) -> u32 {
    let normalized_c = c * (1.0_f32 / 255.0_f32);
    let answer = cpp_min_f32(255.0_f32, normalized_c * reciprocal_a * 255.0_f32);
    pixel_round_as_rp(answer)
}

#[inline]
fn unpremul_quick(reciprocal_a: f32, c: f32) -> u32 {
    cpp_min_f32(255.0_f32, c * reciprocal_a + 0.5_f32) as u32
}

#[inline]
fn rgba_premul_to_ccca(c00: f32, c08: f32, c16: f32, a: f32, fast_unpremul: bool) -> u32 {
    if fast_unpremul {
        let reciprocal_a = if a != 0.0 { 255.0_f32 / a } else { 0.0_f32 };
        ((a as u32) << 24)
            | (unpremul_quick(reciprocal_a, c16) << 16)
            | (unpremul_quick(reciprocal_a, c08) << 8)
            | unpremul_quick(reciprocal_a, c00)
    } else {
        let normalized_a = a * (1.0_f32 / 255.0_f32);
        let reciprocal_a = if normalized_a != 0.0 {
            1.0_f32 / normalized_a
        } else {
            0.0_f32
        };
        ((a as u32) << 24)
            | (unpremul_simulating_rp(reciprocal_a, c16) << 16)
            | (unpremul_simulating_rp(reciprocal_a, c08) << 8)
            | unpremul_simulating_rp(reciprocal_a, c00)
    }
}

#[inline]
fn rgba_premul_to_rgba_pixel(p: u32, fast_unpremul: bool) -> u32 {
    let a = ((p >> 24) & 0xFF) as f32;
    let b = ((p >> 16) & 0xFF) as f32;
    let g = ((p >> 8) & 0xFF) as f32;
    let r = (p & 0xFF) as f32;
    rgba_premul_to_ccca(r, g, b, a, fast_unpremul)
}

#[inline]
fn rgba_premul_to_bgra_pixel(p: u32, fast_unpremul: bool) -> u32 {
    let a = ((p >> 24) & 0xFF) as f32;
    let b = ((p >> 16) & 0xFF) as f32;
    let g = ((p >> 8) & 0xFF) as f32;
    let r = (p & 0xFF) as f32;
    rgba_premul_to_ccca(b, g, r, a, fast_unpremul)
}

#[inline]
const fn inverted_cmyk_to_rgb1_pixel(p: u32) -> u32 {
    let k = (p >> 24) & 0xFF;
    let y = (p >> 16) & 0xFF;
    let m = (p >> 8) & 0xFF;
    let c = p & 0xFF;
    let b = premul_byte(y, k);
    let g = premul_byte(m, k);
    let r = premul_byte(c, k);
    (0xFFu32 << 24) | (b << 16) | (g << 8) | r
}

#[inline]
const fn inverted_cmyk_to_bgr1_pixel(p: u32) -> u32 {
    let k = (p >> 24) & 0xFF;
    let y = (p >> 16) & 0xFF;
    let m = (p >> 8) & 0xFF;
    let c = p & 0xFF;
    let b = premul_byte(y, k);
    let g = premul_byte(m, k);
    let r = premul_byte(c, k);
    (0xFFu32 << 24) | (r << 16) | (g << 8) | b
}

// --- 32-to-32 swizzlers (disjoint and in-place variants) ---

pub fn rgba_to_bgra(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = swap_rb_pixel(s);
    }
}

pub fn rgba_to_bgra_in_place(buf: &mut [u32]) {
    for p in buf.iter_mut() {
        *p = swap_rb_pixel(*p);
    }
}

pub fn rgba_to_rgba_premul(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = rgba_to_rgba_premul_pixel(s);
    }
}

pub fn rgba_to_rgba_premul_in_place(buf: &mut [u32]) {
    for p in buf.iter_mut() {
        *p = rgba_to_rgba_premul_pixel(*p);
    }
}

pub fn rgba_to_bgra_premul(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = rgba_to_bgra_premul_pixel(s);
    }
}

pub fn rgba_to_bgra_premul_in_place(buf: &mut [u32]) {
    for p in buf.iter_mut() {
        *p = rgba_to_bgra_premul_pixel(*p);
    }
}

pub fn rgba_premul_to_rgba(dst: &mut [u32], src: &[u32], fast_unpremul: bool) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = rgba_premul_to_rgba_pixel(s, fast_unpremul);
    }
}

pub fn rgba_premul_to_rgba_in_place(buf: &mut [u32], fast_unpremul: bool) {
    for p in buf.iter_mut() {
        *p = rgba_premul_to_rgba_pixel(*p, fast_unpremul);
    }
}

pub fn rgba_premul_to_bgra(dst: &mut [u32], src: &[u32], fast_unpremul: bool) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = rgba_premul_to_bgra_pixel(s, fast_unpremul);
    }
}

pub fn rgba_premul_to_bgra_in_place(buf: &mut [u32], fast_unpremul: bool) {
    for p in buf.iter_mut() {
        *p = rgba_premul_to_bgra_pixel(*p, fast_unpremul);
    }
}

pub fn inverted_cmyk_to_rgb1(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = inverted_cmyk_to_rgb1_pixel(s);
    }
}

pub fn inverted_cmyk_to_rgb1_in_place(buf: &mut [u32]) {
    for p in buf.iter_mut() {
        *p = inverted_cmyk_to_rgb1_pixel(*p);
    }
}

pub fn inverted_cmyk_to_bgr1(dst: &mut [u32], src: &[u32]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = inverted_cmyk_to_bgr1_pixel(s);
    }
}

pub fn inverted_cmyk_to_bgr1_in_place(buf: &mut [u32]) {
    for p in buf.iter_mut() {
        *p = inverted_cmyk_to_bgr1_pixel(*p);
    }
}

// --- 8-bit packed to 32-bit swizzlers ---

pub fn rgb_to_rgb1(dst: &mut [u32], src: &[u8]) {
    for (d, chunk) in dst.iter_mut().zip(src.chunks_exact(3)) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        *d = (0xFFu32 << 24) | (b << 16) | (g << 8) | r;
    }
}

pub fn rgb_to_bgr1(dst: &mut [u32], src: &[u8]) {
    for (d, chunk) in dst.iter_mut().zip(src.chunks_exact(3)) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        *d = (0xFFu32 << 24) | (r << 16) | (g << 8) | b;
    }
}

pub fn gray_to_rgb1(dst: &mut [u32], src: &[u8]) {
    for (d, &g) in dst.iter_mut().zip(src.iter()) {
        let g = g as u32;
        *d = (0xFFu32 << 24) | (g << 16) | (g << 8) | g;
    }
}

pub fn graya_to_rgba(dst: &mut [u32], src: &[u8]) {
    for (d, chunk) in dst.iter_mut().zip(src.chunks_exact(2)) {
        let g = chunk[0] as u32;
        let a = chunk[1] as u32;
        *d = (a << 24) | (g << 16) | (g << 8) | g;
    }
}

pub fn graya_to_rgba_premul(dst: &mut [u32], src: &[u8]) {
    for (d, chunk) in dst.iter_mut().zip(src.chunks_exact(2)) {
        let g = premul_byte(chunk[0] as u32, chunk[1] as u32);
        let a = chunk[1] as u32;
        *d = (a << 24) | (g << 16) | (g << 8) | g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_and_premul() {
        let src = [0x80_10_20_FFu32, 0xFF_01_02_03u32];
        let mut dst = [0u32; 2];
        rgba_to_bgra(&mut dst, &src);
        assert_eq!(dst, [0x80_FF_20_10, 0xFF_03_02_01]);

        let mut inplace = src;
        rgba_to_bgra_in_place(&mut inplace);
        assert_eq!(inplace, dst);

        rgba_to_rgba_premul(&mut dst, &src);
        assert_eq!(dst[0], 0x80_08_10_80);
        assert_eq!(dst[1], 0xFF_01_02_03);

        let mut unp = [0u32; 2];
        rgba_premul_to_rgba(&mut unp, &dst, false);
        assert_eq!(unp[1], 0xFF_01_02_03);
        assert_eq!((unp[0] >> 24) & 0xFF, 0x80);
        assert_eq!(unp[0] & 0xFF, 0xFF);
    }

    #[test]
    fn rgb_gray_cmyk_swizzlers() {
        let rgb = [10u8, 20, 30, 40, 50, 60];
        let mut out = [0u32; 2];
        rgb_to_rgb1(&mut out, &rgb);
        assert_eq!(out, [0xFF_1E_14_0A, 0xFF_3C_32_28]);
        rgb_to_bgr1(&mut out, &rgb);
        assert_eq!(out, [0xFF_0A_14_1E, 0xFF_28_32_3C]);

        gray_to_rgb1(&mut out, &[0x42, 0xAB]);
        assert_eq!(out, [0xFF_42_42_42, 0xFF_AB_AB_AB]);

        graya_to_rgba(&mut out, &[0x80, 0x40, 0xFF, 0x00]);
        assert_eq!(out, [0x40_80_80_80, 0x00_FF_FF_FF]);

        graya_to_rgba_premul(&mut out, &[0xFF, 0x80, 0xFF, 0x00]);
        assert_eq!(out, [0x80_80_80_80, 0x00_00_00_00]);

        inverted_cmyk_to_rgb1(&mut out, &[0xFF_10_20_30, 0x00_FF_FF_FF]);
        assert_eq!(out, [0xFF_10_20_30, 0xFF_00_00_00]);
    }
}
