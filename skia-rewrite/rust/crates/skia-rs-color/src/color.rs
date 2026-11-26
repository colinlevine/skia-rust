//! Safe Rust port of `src/core/SkColor.cpp`.

use skia_rs_geometry::{
    cpp_max_f32, cpp_min_f32, pin_f32, scalar_nearly_zero, scalar_round_to_int,
    SK_SCALAR_NEARLY_ZERO,
};

/// 32-bit unpremultiplied ARGB color (`0xAARRGGBB`), matching `SkColor`.
pub type Color = u32;

/// 32-bit premultiplied color in platform native order (RGBA or BGRA), matching `SkPMColor`.
pub type PMColor = u32;

/// 4-channel floating-point RGBA color (`SkColor4f` / `SkPMColor4f`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Color4f {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[inline]
pub const fn color_set_argb(a: u32, r: u32, g: u32, b: u32) -> Color {
    ((a & 0xFF) << 24) | ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}

#[inline]
pub const fn color_get_a(c: Color) -> u32 {
    (c >> 24) & 0xFF
}

#[inline]
pub const fn color_get_r(c: Color) -> u32 {
    (c >> 16) & 0xFF
}

#[inline]
pub const fn color_get_g(c: Color) -> u32 {
    (c >> 8) & 0xFF
}

#[inline]
pub const fn color_get_b(c: Color) -> u32 {
    c & 0xFF
}

/// Exact match for `SkMulDiv255Round(a, b)` -> `SkMul16ShiftRound(a, b, 8)`.
#[inline]
pub const fn mul_div_255_round(a: u32, b: u32) -> u32 {
    let prod = a * b + 128;
    (prod + (prod >> 8)) >> 8
}

/// Pack premultiplied ARGB components into `SkPMColor`.
/// When `is_bgra` is true, layout is `0xAARRGGBB` (`SK_R32_SHIFT == 16`).
/// When `is_bgra` is false, layout is `0xAABBGGRR` (`SK_R32_SHIFT == 0`).
#[inline]
pub const fn pmcolor_set_argb(a: u32, r: u32, g: u32, b: u32, is_bgra: bool) -> PMColor {
    if is_bgra {
        (a << 24) | (r << 16) | (g << 8) | b
    } else {
        (a << 24) | (b << 16) | (g << 8) | r
    }
}

#[inline]
pub const fn pmcolor_get_a(c: PMColor) -> u8 {
    ((c >> 24) & 0xFF) as u8
}

#[inline]
pub const fn pmcolor_get_r(c: PMColor, is_bgra: bool) -> u8 {
    if is_bgra {
        ((c >> 16) & 0xFF) as u8
    } else {
        (c & 0xFF) as u8
    }
}

#[inline]
pub const fn pmcolor_get_g(c: PMColor) -> u8 {
    ((c >> 8) & 0xFF) as u8
}

#[inline]
pub const fn pmcolor_get_b(c: PMColor, is_bgra: bool) -> u8 {
    if is_bgra {
        (c & 0xFF) as u8
    } else {
        ((c >> 16) & 0xFF) as u8
    }
}

/// Exact port of `SkPreMultiplyARGB`.
#[inline]
pub const fn premultiply_argb(a: u32, mut r: u32, mut g: u32, mut b: u32, is_bgra: bool) -> PMColor {
    if a != 255 {
        r = mul_div_255_round(r, a);
        g = mul_div_255_round(g, a);
        b = mul_div_255_round(b, a);
    }
    pmcolor_set_argb(a, r, g, b, is_bgra)
}

/// Exact port of `SkPreMultiplyColor`.
#[inline]
pub const fn premultiply_color(c: Color, is_bgra: bool) -> PMColor {
    premultiply_argb(
        color_get_a(c),
        color_get_r(c),
        color_get_g(c),
        color_get_b(c),
        is_bgra,
    )
}

#[inline]
fn byte_to_scalar(x: u32) -> f32 {
    (x as f32) / 255.0_f32
}

#[inline]
fn byte_div_to_scalar(numer: i32, denom: u32) -> f32 {
    (numer as f32) / (denom as i32 as f32)
}

/// Exact port of `SkRGBToHSV`.
pub fn rgb_to_hsv(r: u32, g: u32, b: u32) -> [f32; 3] {
    let min = r.min(g.min(b));
    let max = r.max(g.max(b));
    let delta = max - min;

    let v = byte_to_scalar(max);
    if delta == 0 {
        return [0.0, 0.0, v];
    }

    let s = byte_div_to_scalar(delta as i32, max);

    let mut h = if r == max {
        byte_div_to_scalar(g as i32 - b as i32, delta)
    } else if g == max {
        2.0_f32 + byte_div_to_scalar(b as i32 - r as i32, delta)
    } else {
        4.0_f32 + byte_div_to_scalar(r as i32 - g as i32, delta)
    };

    h *= 60.0_f32;
    if h < 0.0_f32 {
        h += 360.0_f32;
    }

    [h, s, v]
}

/// Exact port of `SkHSVToColor`.
pub fn hsv_to_color(a: u32, hsv: [f32; 3]) -> Color {
    let s = pin_f32(hsv[1], 0.0, 1.0);
    let v = pin_f32(hsv[2], 0.0, 1.0);

    let v_byte = scalar_round_to_int(v * 255.0_f32) as u32;

    if scalar_nearly_zero(s, SK_SCALAR_NEARLY_ZERO) {
        return color_set_argb(a, v_byte, v_byte, v_byte);
    }

    let hx = if hsv[0] < 0.0_f32 || hsv[0] >= 360.0_f32 {
        0.0_f32
    } else {
        hsv[0] / 60.0_f32
    };
    let w = hx.floor();
    let f = hx - w;

    let p = scalar_round_to_int((1.0_f32 - s) * v * 255.0_f32) as u32;
    let q = scalar_round_to_int((1.0_f32 - (s * f)) * v * 255.0_f32) as u32;
    let t = scalar_round_to_int((1.0_f32 - (s * (1.0_f32 - f))) * v * 255.0_f32) as u32;

    let (r, g, b) = match w as u32 {
        0 => (v_byte, t, p),
        1 => (q, v_byte, p),
        2 => (p, v_byte, t),
        3 => (p, q, v_byte),
        4 => (t, p, v_byte),
        _ => (v_byte, p, q),
    };
    color_set_argb(a, r, g, b)
}

#[inline]
fn swizzle_rb_4f(c: [f32; 4]) -> [f32; 4] {
    [c[2], c[1], c[0], c[3]]
}

#[inline]
fn f4_from_l32(px: u32) -> [f32; 4] {
    let inv255 = 1.0_f32 / 255.0_f32;
    [
        ((px & 0xFF) as f32) * inv255,
        (((px >> 8) & 0xFF) as f32) * inv255,
        (((px >> 16) & 0xFF) as f32) * inv255,
        (((px >> 24) & 0xFF) as f32) * inv255,
    ]
}

#[inline]
fn f4_to_l32(px: [f32; 4]) -> u32 {
    let b0 = cpp_max_f32(0.0_f32, cpp_min_f32(px[0] * 255.0_f32 + 0.5_f32, 255.0_f32)) as u8 as u32;
    let b1 = cpp_max_f32(0.0_f32, cpp_min_f32(px[1] * 255.0_f32 + 0.5_f32, 255.0_f32)) as u8 as u32;
    let b2 = cpp_max_f32(0.0_f32, cpp_min_f32(px[2] * 255.0_f32 + 0.5_f32, 255.0_f32)) as u8 as u32;
    let b3 = cpp_max_f32(0.0_f32, cpp_min_f32(px[3] * 255.0_f32 + 0.5_f32, 255.0_f32)) as u8 as u32;
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

impl Color4f {
    #[inline]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Exact port of `SkColor4f::FromColor(SkColor)`.
    #[inline]
    pub fn from_color(bgra: Color) -> Self {
        let v = swizzle_rb_4f(f4_from_l32(bgra));
        Self {
            r: v[0],
            g: v[1],
            b: v[2],
            a: v[3],
        }
    }

    /// Exact port of `SkColor4f::toSkColor() const`.
    #[inline]
    pub fn to_sk_color(self) -> Color {
        f4_to_l32(swizzle_rb_4f([self.r, self.g, self.b, self.a]))
    }

    /// Exact port of `SkColor4f::toBytes_RGBA() const` and `SkPMColor4f::toBytes_RGBA() const`.
    #[inline]
    pub fn to_bytes_rgba(self) -> u32 {
        f4_to_l32([self.r, self.g, self.b, self.a])
    }

    /// Exact port of `SkColor4f::FromBytes_RGBA(uint32_t)` and `SkPMColor4f::FromBytes_RGBA(uint32_t)`.
    #[inline]
    pub fn from_bytes_rgba(c: u32) -> Self {
        let v = f4_from_l32(c);
        Self {
            r: v[0],
            g: v[1],
            b: v[2],
            a: v[3],
        }
    }

    /// Exact port of `SkPMColor4f::FromPMColor(SkPMColor)`.
    #[inline]
    pub fn from_pm_color(c: PMColor, is_bgra: bool) -> Self {
        let raw = f4_from_l32(c);
        let v = if is_bgra { swizzle_rb_4f(raw) } else { raw };
        Self {
            r: v[0],
            g: v[1],
            b: v[2],
            a: v[3],
        }
    }
}

/// Bulk conversion used by `SkColorConverter`.
pub fn convert_colors_to_4f(src: &[Color], dst: &mut [Color4f]) {
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = Color4f::from_color(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_rgba_and_bgra() {
        let pm_bgra = premultiply_argb(128, 255, 100, 0, true);
        assert_eq!(pmcolor_get_a(pm_bgra), 128);
        assert_eq!(pmcolor_get_r(pm_bgra, true), 128);
        assert_eq!(pmcolor_get_g(pm_bgra), 50);
        assert_eq!(pmcolor_get_b(pm_bgra, true), 0);

        let pm_rgba = premultiply_argb(128, 255, 100, 0, false);
        assert_eq!(pmcolor_get_a(pm_rgba), 128);
        assert_eq!(pmcolor_get_r(pm_rgba, false), 128);
        assert_eq!(pmcolor_get_g(pm_rgba), 50);
        assert_eq!(pmcolor_get_b(pm_rgba, false), 0);

        assert_eq!(
            premultiply_color(color_set_argb(255, 10, 20, 30), true),
            pmcolor_set_argb(255, 10, 20, 30, true)
        );
    }

    #[test]
    fn rgb_hsv_roundtrip() {
        let cases = [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (255, 255, 0),
            (0, 255, 255),
            (255, 0, 255),
            (128, 128, 128),
            (0, 0, 0),
            (255, 255, 255),
            (42, 137, 211),
        ];
        for (r, g, b) in cases {
            let hsv = rgb_to_hsv(r, g, b);
            let c = hsv_to_color(0xAB, hsv);
            assert_eq!(c, color_set_argb(0xAB, r, g, b));
        }
    }

    #[test]
    fn color4f_conversions() {
        let c = color_set_argb(0x80, 0xFF, 0x40, 0x00);
        let c4 = Color4f::from_color(c);
        assert_eq!(c4.to_sk_color(), c);

        let rgba_bytes = c4.to_bytes_rgba();
        assert_eq!(rgba_bytes, 0x800040FF);
        let c4_back = Color4f::from_bytes_rgba(rgba_bytes);
        assert_eq!(c4, c4_back);

        let pm_bgra = pmcolor_set_argb(0x80, 0x40, 0x20, 0x10, true);
        let pm4 = Color4f::from_pm_color(pm_bgra, true);
        assert_eq!(pm4.to_bytes_rgba(), 0x80102040);

        let pm_rgba = pmcolor_set_argb(0x80, 0x40, 0x20, 0x10, false);
        let pm4_r = Color4f::from_pm_color(pm_rgba, false);
        assert_eq!(pm4_r.to_bytes_rgba(), 0x80102040);
    }
}
