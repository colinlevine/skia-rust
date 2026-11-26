//! Safe Rust implementation of `SkMaskBlurFilter` (`src/core/SkMaskBlurFilter.cpp`):
//! - Small-sigma (< 2.0) exact 8.8 fixed-point separable Gaussian blur (radius 1..=4)
//! - Large-sigma (>= 2.0) 3-box Gaussian approximation (`PlanGauss`) with transpose
//! - Supports `kBW_Format`, `kA8_Format`, `kARGB32_Format`, `kLCD16_Format`

use skia_rs_core_types::gauss_filter::{GaussFilter, GAUSS_ARRAY_MAX};
use skia_rs_geometry::IPoint;

/// Mask format matching `SkMask::Format`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskFormat {
    BW = 0,
    A8 = 1,
    ThreeD = 2,
    ARGB32 = 3,
    LCD16 = 4,
    SDF = 5,
}

impl MaskFormat {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::BW),
            1 => Some(Self::A8),
            2 => Some(Self::ThreeD),
            3 => Some(Self::ARGB32),
            4 => Some(Self::LCD16),
            5 => Some(Self::SDF),
            _ => None,
        }
    }
}

/// Safe Rust mirror of `SkMaskBlurFilter`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskBlurFilter {
    sigma_w: f64,
    sigma_h: f64,
}

#[inline]
fn pin_f64(x: f64, lo: f64, hi: f64) -> f64 {
    let m = if hi < x { hi } else { x };
    if m < lo { lo } else { m }
}

impl MaskBlurFilter {
    /// Pins `sigma_w` and `sigma_h` to `[0.0, 135.0]`, matching `SkMaskBlurFilter::SkMaskBlurFilter`.
    #[inline]
    pub fn new(sigma_w: f64, sigma_h: f64) -> Self {
        Self {
            sigma_w: pin_f64(sigma_w, 0.0, 135.0),
            sigma_h: pin_f64(sigma_h, 0.0, 135.0),
        }
    }

    #[inline]
    pub fn sigma_w(&self) -> f64 {
        self.sigma_w
    }

    #[inline]
    pub fn sigma_h(&self) -> f64 {
        self.sigma_h
    }

    /// Exact port of `SkMaskBlurFilter::hasNoBlur`.
    #[inline]
    pub fn has_no_blur(&self) -> bool {
        const NO_WINDOW_SIGMA: f64 = 1.0 / 3.0;
        self.sigma_w < NO_WINDOW_SIGMA && self.sigma_h <= NO_WINDOW_SIGMA
    }

    /// Computes `(border_w, border_h)` required to prepare the destination mask.
    pub fn compute_border(&self) -> IPoint {
        if self.sigma_w < 2.0 && self.sigma_h < 2.0 {
            let fx = GaussFilter::new(self.sigma_w);
            let fy = GaussFilter::new(self.sigma_h);
            IPoint { x: fx.radius() as i32, y: fy.radius() as i32 }
        } else {
            let pw = PlanGauss::new(self.sigma_w);
            let ph = PlanGauss::new(self.sigma_h);
            IPoint { x: pw.border(), y: ph.border() }
        }
    }

    /// Executes the blur from `src` into an already-allocated A8 `dst` buffer
    /// (with dimensions `dst_w = src_w + 2 * border.x`, `dst_h = src_h + 2 * border.y`).
    /// Returns the border `(border_w, border_h)` on success, or `IPoint::new(0, 0)` on overflow.
    #[allow(clippy::too_many_arguments)]
    pub fn blur_pixels(
        &self,
        src_format: MaskFormat,
        src: &[u8],
        src_rb: usize,
        src_w: usize,
        src_h: usize,
        dst: &mut [u8],
        dst_rb: usize,
        dst_w: usize,
        dst_h: usize,
    ) -> IPoint {
        if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
            return IPoint { x: 0, y: 0 };
        }
        if self.sigma_w < 2.0 && self.sigma_h < 2.0 {
            small_blur_pixels(
                self.sigma_w,
                self.sigma_h,
                src_format,
                src,
                src_rb,
                src_w,
                src_h,
                dst,
                dst_rb,
                dst_w,
                dst_h,
            )
        } else {
            plan_gauss_blur_pixels(
                self.sigma_w,
                self.sigma_h,
                src_format,
                src,
                src_rb,
                src_w,
                src_h,
                dst,
                dst_rb,
                dst_w,
                dst_h,
            )
        }
    }
}

// ============================================================================
// Pixel format to A8 helpers (exact match to SkMask.h & SkMaskBlurFilter.cpp)
// ============================================================================

#[inline]
fn lcd16_to_a8_pixel(rgb: u16) -> u8 {
    let r5 = ((rgb >> 11) & 0x1F) as u32;
    let g6 = ((rgb >> 5) & 0x3F) as u32;
    let b5 = (rgb & 0x1F) as u32;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g6 << 2) | (g6 >> 4);
    let b = (b5 << 3) | (b5 >> 2);
    ((r + g + b) / 3) as u8
}

#[inline]
fn argb32_to_a8_pixel(rgba: u32) -> u8 {
    (rgba >> 24) as u8
}

#[inline]
fn read_src_alpha(format: MaskFormat, row: &[u8], x: usize) -> u8 {
    match format {
        MaskFormat::BW => {
            let byte = row[x >> 3];
            let bit = 7 - (x & 7);
            if ((byte >> bit) & 1) != 0 {
                0xFF
            } else {
                0x00
            }
        }
        MaskFormat::A8 => row[x],
        MaskFormat::ARGB32 => {
            let off = x * 4;
            let rgba = u32::from_ne_bytes([row[off], row[off + 1], row[off + 2], row[off + 3]]);
            argb32_to_a8_pixel(rgba)
        }
        MaskFormat::LCD16 => {
            let off = x * 2;
            let rgb = u16::from_ne_bytes([row[off], row[off + 1]]);
            lcd16_to_a8_pixel(rgb)
        }
        MaskFormat::ThreeD | MaskFormat::SDF => 0,
    }
}

// ============================================================================
// Small-sigma (< 2.0) exact 8.8 fixed-point separable Gaussian blur
// ============================================================================

type Fp88 = [u16; 8];

const K_HALF: u16 = 0x80;

#[inline]
fn fp88_splat(v: u16) -> Fp88 {
    [v; 8]
}

#[inline]
fn fp88_mulhi(a: Fp88, b: Fp88) -> Fp88 {
    let mut out = [0u16; 8];
    for i in 0..8 {
        out[i] = (((a[i] as u32) * (b[i] as u32)) >> 16) as u16;
    }
    out
}

#[inline]
fn fp88_add(a: Fp88, b: Fp88) -> Fp88 {
    let mut out = [0u16; 8];
    for i in 0..8 {
        out[i] = a[i].wrapping_add(b[i]);
    }
    out
}

#[inline]
fn fp88_add_scalar(a: Fp88, s: u16) -> Fp88 {
    let mut out = [0u16; 8];
    for i in 0..8 {
        out[i] = a[i].wrapping_add(s);
    }
    out
}

#[inline]
fn load_fp88_from_format(format: MaskFormat, row: &[u8], x_start: usize, width: usize) -> Fp88 {
    let mut out = [0u16; 8];
    for i in 0..width {
        out[i] = (read_src_alpha(format, row, x_start + i) as u16) << 8;
    }
    out
}

#[inline]
fn load_fp88_a8(slice: &[u8], width: usize) -> Fp88 {
    let mut out = [0u16; 8];
    for i in 0..width {
        out[i] = (slice[i] as u16) << 8;
    }
    out
}

#[inline]
fn store_fp88(dst: &mut [u8], v: Fp88, width: usize) {
    for i in 0..width {
        dst[i] = (v[i] >> 8) as u8;
    }
}

#[inline]
fn blur_x_radius_1(s0: Fp88, g: &[Fp88; 5], d0: &mut Fp88, d8: &mut Fp88) {
    let v1 = fp88_mulhi(s0, g[1]);
    let v0 = fp88_mulhi(s0, g[0]);

    *d0 = fp88_add(*d0, v1);
    *d0 = fp88_add(*d0, [0, v0[0], v0[1], v0[2], v0[3], v0[4], v0[5], v0[6]]);
    *d8 = fp88_add(*d8, [v0[7], 0, 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, v1[0], v1[1], v1[2], v1[3], v1[4], v1[5]]);
    *d8 = fp88_add(*d8, [v1[6], v1[7], 0, 0, 0, 0, 0, 0]);
}

#[inline]
fn blur_x_radius_2(s0: Fp88, g: &[Fp88; 5], d0: &mut Fp88, d8: &mut Fp88) {
    let v0 = fp88_mulhi(s0, g[0]);
    let v1 = fp88_mulhi(s0, g[1]);
    let v2 = fp88_mulhi(s0, g[2]);

    *d0 = fp88_add(*d0, v2);
    *d0 = fp88_add(*d0, [0, v1[0], v1[1], v1[2], v1[3], v1[4], v1[5], v1[6]]);
    *d8 = fp88_add(*d8, [v1[7], 0, 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, v0[0], v0[1], v0[2], v0[3], v0[4], v0[5]]);
    *d8 = fp88_add(*d8, [v0[6], v0[7], 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, v1[0], v1[1], v1[2], v1[3], v1[4]]);
    *d8 = fp88_add(*d8, [v1[5], v1[6], v1[7], 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, v2[0], v2[1], v2[2], v2[3]]);
    *d8 = fp88_add(*d8, [v2[4], v2[5], v2[6], v2[7], 0, 0, 0, 0]);
}

#[inline]
fn blur_x_radius_3(s0: Fp88, g: &[Fp88; 5], d0: &mut Fp88, d8: &mut Fp88) {
    let v0 = fp88_mulhi(s0, g[0]);
    let v1 = fp88_mulhi(s0, g[1]);
    let v2 = fp88_mulhi(s0, g[2]);
    let v3 = fp88_mulhi(s0, g[3]);

    *d0 = fp88_add(*d0, v3);
    *d0 = fp88_add(*d0, [0, v2[0], v2[1], v2[2], v2[3], v2[4], v2[5], v2[6]]);
    *d8 = fp88_add(*d8, [v2[7], 0, 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, v1[0], v1[1], v1[2], v1[3], v1[4], v1[5]]);
    *d8 = fp88_add(*d8, [v1[6], v1[7], 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, v0[0], v0[1], v0[2], v0[3], v0[4]]);
    *d8 = fp88_add(*d8, [v0[5], v0[6], v0[7], 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, v1[0], v1[1], v1[2], v1[3]]);
    *d8 = fp88_add(*d8, [v1[4], v1[5], v1[6], v1[7], 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, 0, v2[0], v2[1], v2[2]]);
    *d8 = fp88_add(*d8, [v2[3], v2[4], v2[5], v2[6], v2[7], 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, 0, 0, v3[0], v3[1]]);
    *d8 = fp88_add(*d8, [v3[2], v3[3], v3[4], v3[5], v3[6], v3[7], 0, 0]);
}

#[inline]
fn blur_x_radius_4(s0: Fp88, g: &[Fp88; 5], d0: &mut Fp88, d8: &mut Fp88) {
    let v0 = fp88_mulhi(s0, g[0]);
    let v1 = fp88_mulhi(s0, g[1]);
    let v2 = fp88_mulhi(s0, g[2]);
    let v3 = fp88_mulhi(s0, g[3]);
    let v4 = fp88_mulhi(s0, g[4]);

    *d0 = fp88_add(*d0, v4);
    *d0 = fp88_add(*d0, [0, v3[0], v3[1], v3[2], v3[3], v3[4], v3[5], v3[6]]);
    *d8 = fp88_add(*d8, [v3[7], 0, 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, v2[0], v2[1], v2[2], v2[3], v2[4], v2[5]]);
    *d8 = fp88_add(*d8, [v2[6], v2[7], 0, 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, v1[0], v1[1], v1[2], v1[3], v1[4]]);
    *d8 = fp88_add(*d8, [v1[5], v1[6], v1[7], 0, 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, v0[0], v0[1], v0[2], v0[3]]);
    *d8 = fp88_add(*d8, [v0[4], v0[5], v0[6], v0[7], 0, 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, 0, v1[0], v1[1], v1[2]]);
    *d8 = fp88_add(*d8, [v1[3], v1[4], v1[5], v1[6], v1[7], 0, 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, 0, 0, v2[0], v2[1]]);
    *d8 = fp88_add(*d8, [v2[2], v2[3], v2[4], v2[5], v2[6], v2[7], 0, 0]);
    *d0 = fp88_add(*d0, [0, 0, 0, 0, 0, 0, 0, v3[0]]);
    *d8 = fp88_add(*d8, [v3[1], v3[2], v3[3], v3[4], v3[5], v3[6], v3[7], 0]);
    *d8 = fp88_add(*d8, v4);
}

#[inline]
fn blur_row_in_place(radius: usize, g: &[Fp88; 5], row: &mut [u8], src_w: usize, dst_w: usize) {
    let mut d0 = fp88_splat(K_HALF);
    let mut d8 = fp88_splat(K_HALF);

    let blur_step = |s0: Fp88, d0: &mut Fp88, d8: &mut Fp88| match radius {
        1 => blur_x_radius_1(s0, g, d0, d8),
        2 => blur_x_radius_2(s0, g, d0, d8),
        3 => blur_x_radius_3(s0, g, d0, d8),
        4 => blur_x_radius_4(s0, g, d0, d8),
        _ => unreachable!(),
    };

    let mut x = 0usize;
    while x + 8 <= src_w {
        let s0 = load_fp88_a8(&row[radius + x..radius + x + 8], 8);
        blur_step(s0, &mut d0, &mut d8);
        store_fp88(&mut row[x..x + 8], d0, 8);
        d0 = d8;
        d8 = fp88_splat(K_HALF);
        x += 8;
    }

    let src_tail = src_w - x;
    if src_tail > 0 {
        let s0 = load_fp88_a8(&row[radius + x..radius + x + src_tail], src_tail);
        blur_step(s0, &mut d0, &mut d8);
        let dst_tail = (dst_w - x).min(8);
        store_fp88(&mut row[x..x + dst_tail], d0, dst_tail);
        d0 = d8;
        x += dst_tail;
    }

    let dst_tail = dst_w - x;
    if dst_tail > 0 {
        store_fp88(&mut row[x..x + dst_tail], d0, dst_tail);
    }
}

#[inline]
fn blur_y_step(radius: usize, s0: Fp88, g: &[Fp88; 5], d: &mut [Fp88; 8]) -> Fp88 {
    match radius {
        1 => {
            let v0 = fp88_mulhi(s0, g[0]);
            let v1 = fp88_mulhi(s0, g[1]);
            let answer = fp88_add(d[0], v1);
            d[0] = fp88_add(d[1], v0);
            d[1] = fp88_add_scalar(v1, K_HALF);
            answer
        }
        2 => {
            let v0 = fp88_mulhi(s0, g[0]);
            let v1 = fp88_mulhi(s0, g[1]);
            let v2 = fp88_mulhi(s0, g[2]);
            let answer = fp88_add(d[0], v2);
            d[0] = fp88_add(d[1], v1);
            d[1] = fp88_add(d[2], v0);
            d[2] = fp88_add(d[3], v1);
            d[3] = fp88_add_scalar(v2, K_HALF);
            answer
        }
        3 => {
            let v0 = fp88_mulhi(s0, g[0]);
            let v1 = fp88_mulhi(s0, g[1]);
            let v2 = fp88_mulhi(s0, g[2]);
            let v3 = fp88_mulhi(s0, g[3]);
            let answer = fp88_add(d[0], v3);
            d[0] = fp88_add(d[1], v2);
            d[1] = fp88_add(d[2], v1);
            d[2] = fp88_add(d[3], v0);
            d[3] = fp88_add(d[4], v1);
            d[4] = fp88_add(d[5], v2);
            d[5] = fp88_add_scalar(v3, K_HALF);
            answer
        }
        4 => {
            let v0 = fp88_mulhi(s0, g[0]);
            let v1 = fp88_mulhi(s0, g[1]);
            let v2 = fp88_mulhi(s0, g[2]);
            let v3 = fp88_mulhi(s0, g[3]);
            let v4 = fp88_mulhi(s0, g[4]);
            let answer = fp88_add(d[0], v4);
            d[0] = fp88_add(d[1], v3);
            d[1] = fp88_add(d[2], v2);
            d[2] = fp88_add(d[3], v1);
            d[3] = fp88_add(d[4], v0);
            d[4] = fp88_add(d[5], v1);
            d[5] = fp88_add(d[6], v2);
            d[6] = fp88_add(d[7], v3);
            d[7] = fp88_add_scalar(v4, K_HALF);
            answer
        }
        _ => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
fn blur_column(
    format: MaskFormat,
    radius_y: usize,
    radius_x: usize,
    x_col: usize,
    width: usize,
    g: &[Fp88; 5],
    src: &[u8],
    src_rb: usize,
    src_h: usize,
    dst: &mut [u8],
    dst_rb: usize,
) {
    let mut d = [fp88_splat(K_HALF); 8];
    let mut dst_y = 0usize;

    for y in 0..src_h {
        let s = load_fp88_from_format(format, &src[y * src_rb..], x_col, width);
        let b = blur_y_step(radius_y, s, g, &mut d);
        let dst_off = dst_y * dst_rb + radius_x + x_col;
        store_fp88(&mut dst[dst_off..dst_off + width], b, width);
        dst_y += 1;
    }

    for pair in 0..radius_y {
        let off0 = dst_y * dst_rb + radius_x + x_col;
        store_fp88(&mut dst[off0..off0 + width], d[pair * 2], width);
        dst_y += 1;
        let off1 = dst_y * dst_rb + radius_x + x_col;
        store_fp88(&mut dst[off1..off1 + width], d[pair * 2 + 1], width);
        dst_y += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn small_blur_pixels(
    sigma_x: f64,
    sigma_y: f64,
    src_format: MaskFormat,
    src: &[u8],
    src_rb: usize,
    src_w: usize,
    src_h: usize,
    dst: &mut [u8],
    dst_rb: usize,
    dst_w: usize,
    dst_h: usize,
) -> IPoint {
    let filter_x = GaussFilter::new(sigma_x);
    let filter_y = GaussFilter::new(sigma_y);
    let radius_x = filter_x.radius();
    let radius_y = filter_y.radius();

    let mut gauss_x = [0u16; GAUSS_ARRAY_MAX];
    for (i, &d) in filter_x.factors().iter().enumerate() {
        gauss_x[i] = (d * ((1u32 << 16) as f64)).round() as u16;
    }
    let mut gauss_y = [0u16; GAUSS_ARRAY_MAX];
    for (i, &d) in filter_y.factors().iter().enumerate() {
        gauss_y[i] = (d * ((1u32 << 16) as f64)).round() as u16;
    }

    let gx = [
        fp88_splat(gauss_x[0]),
        fp88_splat(gauss_x[1]),
        fp88_splat(gauss_x[2]),
        fp88_splat(gauss_x[3]),
        fp88_splat(gauss_x[4]),
    ];
    let gy = [
        fp88_splat(gauss_y[0]),
        fp88_splat(gauss_y[1]),
        fp88_splat(gauss_y[2]),
        fp88_splat(gauss_y[3]),
        fp88_splat(gauss_y[4]),
    ];

    // Blur vertically into dst shifted right by radius_x.
    if radius_y == 0 {
        for y in 0..src_h {
            let src_row = &src[y * src_rb..];
            let dst_off = y * dst_rb + radius_x;
            for x in 0..src_w {
                dst[dst_off + x] = read_src_alpha(src_format, src_row, x);
            }
        }
    } else {
        let mut x = 0usize;
        while x + 8 <= src_w {
            blur_column(
                src_format,
                radius_y,
                radius_x,
                x,
                8,
                &gy,
                src,
                src_rb,
                src_h,
                dst,
                dst_rb,
            );
            x += 8;
        }
        let x_tail = src_w - x;
        if x_tail > 0 {
            blur_column(
                src_format,
                radius_y,
                radius_x,
                x,
                x_tail,
                &gy,
                src,
                src_rb,
                src_h,
                dst,
                dst_rb,
            );
        }
    }

    // Blur horizontally in place.
    if radius_x > 0 {
        for y in 0..dst_h {
            let row = &mut dst[y * dst_rb..y * dst_rb + dst_w];
            blur_row_in_place(radius_x, &gx, row, src_w, dst_w);
        }
    }

    IPoint { x: radius_x as i32, y: radius_y as i32 }
}

// ============================================================================
// Large-sigma (>= 2.0) PlanGauss 3-pass box filter with transpose
// ============================================================================

#[derive(Debug, Clone, Copy)]
struct PlanGauss {
    weight: u64,
    border: i32,
    sliding_window: i32,
    pass0_size: usize,
    pass1_size: usize,
    pass2_size: usize,
}

impl PlanGauss {
    fn new(sigma: f64) -> Self {
        let possible_window =
            (sigma * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5).floor() as i32;
        let window = possible_window.max(1);

        let pass0_size = (window - 1) as usize;
        let pass1_size = (window - 1) as usize;
        let pass2_size = if (window & 1) == 1 {
            (window - 1) as usize
        } else {
            window as usize
        };

        let border = if (window & 1) == 1 {
            3 * ((window - 1) / 2)
        } else {
            3 * (window / 2) - 1
        };
        let sliding_window = 2 * border + 1;

        let window2 = (window as i64) * (window as i64);
        let window3 = window2 * (window as i64);
        let divisor = if (window & 1) == 1 {
            window3
        } else {
            window3 + window2
        };

        let weight = ((1.0 / (divisor as f64)) * ((1u64 << 32) as f64)).round() as u64;

        Self {
            weight,
            border,
            sliding_window,
            pass0_size,
            pass1_size,
            pass2_size,
        }
    }

    #[inline]
    fn border(&self) -> i32 {
        self.border
    }

    #[inline]
    fn buffer_size(&self) -> usize {
        self.pass0_size + self.pass1_size + self.pass2_size
    }

    #[inline]
    fn final_scale(&self, sum: u32) -> u8 {
        const K_HALF: u64 = 1u64 << 31;
        ((self.weight.wrapping_mul(sum as u64).wrapping_add(K_HALF)) >> 32) as u8
    }

    fn blur_line<F>(
        &self,
        src_len: usize,
        read_src: F,
        dst: &mut [u8],
        dst_start: usize,
        dst_stride: usize,
        dst_count: usize,
        buffer: &mut [u32],
    ) where
        F: Fn(usize) -> u8,
    {
        let total = self.buffer_size();
        if total == 0 {
            let n = src_len.min(dst_count);
            for i in 0..n {
                dst[dst_start + i * dst_stride] = read_src(i);
            }
            return;
        }
        let p0_end = self.pass0_size;
        let p1_start = p0_end;
        let p1_end = p1_start + self.pass1_size;
        let p2_start = p1_end;
        let p2_end = p2_start + self.pass2_size;

        buffer[..total].fill(0);

        let mut c0 = 0usize;
        let mut c1 = p1_start;
        let mut c2 = p2_start;

        let mut sum0: u32 = 0;
        let mut sum1: u32 = 0;
        let mut sum2: u32 = 0;

        let mut dst_pos = dst_start;

        // Consume source generating pixels.
        for i in 0..src_len {
            let leading_edge = read_src(i) as u32;
            sum0 = sum0.wrapping_add(leading_edge);
            sum1 = sum1.wrapping_add(sum0);
            sum2 = sum2.wrapping_add(sum1);

            dst[dst_pos] = self.final_scale(sum2);
            dst_pos += dst_stride;

            sum2 = sum2.wrapping_sub(buffer[c2]);
            buffer[c2] = sum1;
            c2 = if c2 + 1 < p2_end { c2 + 1 } else { p2_start };

            sum1 = sum1.wrapping_sub(buffer[c1]);
            buffer[c1] = sum0;
            c1 = if c1 + 1 < p1_end { c1 + 1 } else { p1_start };

            sum0 = sum0.wrapping_sub(buffer[c0]);
            buffer[c0] = leading_edge;
            c0 = if c0 + 1 < p0_end { c0 + 1 } else { 0 };
        }

        // Leading edge off the right side of the mask.
        let no_change_count = if self.sliding_window > (src_len as i32) {
            (self.sliding_window - (src_len as i32)) as usize
        } else {
            0
        };
        for _ in 0..no_change_count {
            let leading_edge: u32 = 0;
            sum0 = sum0.wrapping_add(leading_edge);
            sum1 = sum1.wrapping_add(sum0);
            sum2 = sum2.wrapping_add(sum1);

            dst[dst_pos] = self.final_scale(sum2);
            dst_pos += dst_stride;

            sum2 = sum2.wrapping_sub(buffer[c2]);
            buffer[c2] = sum1;
            c2 = if c2 + 1 < p2_end { c2 + 1 } else { p2_start };

            sum1 = sum1.wrapping_sub(buffer[c1]);
            buffer[c1] = sum0;
            c1 = if c1 + 1 < p1_end { c1 + 1 } else { p1_start };

            sum0 = sum0.wrapping_sub(buffer[c0]);
            buffer[c0] = leading_edge;
            c0 = if c0 + 1 < p0_end { c0 + 1 } else { 0 };
        }

        // Starting from the right, fill in the rest of the buffer.
        buffer[..total].fill(0);
        sum0 = 0;
        sum1 = 0;
        sum2 = 0;

        let mut dst_cursor = dst_start + dst_stride * dst_count;
        let mut src_idx = src_len;
        while dst_cursor > dst_pos {
            dst_cursor -= dst_stride;
            src_idx -= 1;
            let leading_edge = read_src(src_idx) as u32;
            sum0 = sum0.wrapping_add(leading_edge);
            sum1 = sum1.wrapping_add(sum0);
            sum2 = sum2.wrapping_add(sum1);

            dst[dst_cursor] = self.final_scale(sum2);

            sum2 = sum2.wrapping_sub(buffer[c2]);
            buffer[c2] = sum1;
            c2 = if c2 + 1 < p2_end { c2 + 1 } else { p2_start };

            sum1 = sum1.wrapping_sub(buffer[c1]);
            buffer[c1] = sum0;
            c1 = if c1 + 1 < p1_end { c1 + 1 } else { p1_start };

            sum0 = sum0.wrapping_sub(buffer[c0]);
            buffer[c0] = leading_edge;
            c0 = if c0 + 1 < p0_end { c0 + 1 } else { 0 };
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_gauss_blur_pixels(
    sigma_w: f64,
    sigma_h: f64,
    src_format: MaskFormat,
    src: &[u8],
    src_rb: usize,
    src_w: usize,
    src_h: usize,
    dst: &mut [u8],
    dst_rb: usize,
    dst_w: usize,
    dst_h: usize,
) -> IPoint {
    let plan_w = PlanGauss::new(sigma_w);
    let plan_h = PlanGauss::new(sigma_h);
    let border_w = plan_w.border();
    let border_h = plan_h.border();

    let tmp_w = src_h;
    let tmp_h = dst_w;
    if tmp_w == 0 || tmp_h > (i32::MAX as usize) / tmp_w {
        return IPoint { x: 0, y: 0 };
    }

    let buffer_size = plan_w.buffer_size().max(plan_h.buffer_size());
    let mut buffer = vec![0u32; buffer_size];
    let mut tmp = vec![0u8; tmp_w * tmp_h];

    // Blur horizontally and transpose into tmp.
    for y in 0..src_h {
        let row = &src[y * src_rb..];
        plan_w.blur_line(
            src_w,
            |x| read_src_alpha(src_format, row, x),
            &mut tmp,
            y,
            tmp_w,
            tmp_h,
            &mut buffer,
        );
    }

    // Blur vertically and transpose back into dst.
    for y in 0..tmp_h {
        let row = &tmp[y * tmp_w..y * tmp_w + tmp_w];
        plan_h.blur_line(tmp_w, |x| row[x], dst, y, dst_rb, dst_h, &mut buffer);
    }

    IPoint { x: border_w, y: border_h }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_no_blur_and_pinning() {
        let f = MaskBlurFilter::new(0.1, 0.1);
        assert!(f.has_no_blur());
        let f2 = MaskBlurFilter::new(1.0 / 3.0, 0.1);
        assert!(!f2.has_no_blur());
        let f3 = MaskBlurFilter::new(0.1, 1.0 / 3.0);
        assert!(f3.has_no_blur());
        let f4 = MaskBlurFilter::new(500.0, 200.0);
        assert_eq!(f4.sigma_w(), 135.0);
        assert_eq!(f4.sigma_h(), 135.0);
    }

    #[test]
    fn test_small_blur_a8_symmetry_and_energy() {
        let filter = MaskBlurFilter::new(1.0, 1.0);
        let border = filter.compute_border();
        assert!(border.x >= 1 && border.x <= 4);

        let src_w = 5usize;
        let src_h = 5usize;
        let mut src = vec![0u8; src_w * src_h];
        src[2 * src_w + 2] = 255;

        let dst_w = src_w + 2 * (border.x as usize);
        let dst_h = src_h + 2 * (border.y as usize);
        let mut dst = vec![0u8; dst_w * dst_h];

        let out_border = filter.blur_pixels(
            MaskFormat::A8,
            &src,
            src_w,
            src_w,
            src_h,
            &mut dst,
            dst_w,
            dst_w,
            dst_h,
        );
        assert_eq!(out_border, border);

        // Peak must be at center and symmetric
        let cx = dst_w / 2;
        let cy = dst_h / 2;
        let center_val = dst[cy * dst_w + cx];
        assert!(center_val > 0);
        assert_eq!(dst[cy * dst_w + cx - 1], dst[cy * dst_w + cx + 1]);
        assert_eq!(dst[(cy - 1) * dst_w + cx], dst[(cy + 1) * dst_w + cx]);

        // Total energy approximately conserved (single 255 impulse)
        let sum: u32 = dst.iter().map(|&v| v as u32).sum();
        assert!(sum >= 240 && sum <= 270, "sum={sum}");
    }

    #[test]
    fn test_plan_gauss_large_sigma_symmetry() {
        let filter = MaskBlurFilter::new(4.0, 4.0);
        let border = filter.compute_border();
        assert!(border.x > 0 && border.y > 0);

        let src_w = 10usize;
        let src_h = 10usize;
        let src = vec![255u8; src_w * src_h];
        let dst_w = src_w + 2 * (border.x as usize);
        let dst_h = src_h + 2 * (border.y as usize);
        let mut dst = vec![0u8; dst_w * dst_h];

        let out_border = filter.blur_pixels(
            MaskFormat::A8,
            &src,
            src_w,
            src_w,
            src_h,
            &mut dst,
            dst_w,
            dst_w,
            dst_h,
        );
        assert_eq!(out_border, border);

        // Horizontal and vertical symmetry of blurred square
        for y in 0..dst_h {
            for x in 0..dst_w {
                assert_eq!(dst[y * dst_w + x], dst[y * dst_w + (dst_w - 1 - x)]);
                assert_eq!(dst[y * dst_w + x], dst[(dst_h - 1 - y) * dst_w + x]);
            }
        }
    }

    #[test]
    fn test_small_and_large_blur_other_formats() {
        // Test BW and ARGB32 and LCD16 formats
        for &sigma in &[0.8, 3.0] {
            let filter = MaskBlurFilter::new(sigma, sigma);
            let b = filter.compute_border();
            let dst_w = 4 + 2 * (b.x as usize);
            let dst_h = 4 + 2 * (b.y as usize);

            let bw = [0xF0u8; 4]; // 4x4 all-ones in top 4 bits
            let mut dst_bw = vec![0u8; dst_w * dst_h];
            filter.blur_pixels(MaskFormat::BW, &bw, 1, 4, 4, &mut dst_bw, dst_w, dst_w, dst_h);

            let a8 = [0xFFu8; 16];
            let mut dst_a8 = vec![0u8; dst_w * dst_h];
            filter.blur_pixels(MaskFormat::A8, &a8, 4, 4, 4, &mut dst_a8, dst_w, dst_w, dst_h);
            assert_eq!(dst_bw, dst_a8);

            let argb_px = 0xFF000000u32.to_ne_bytes();
            let mut argb = Vec::with_capacity(16 * 4);
            for _ in 0..16 {
                argb.extend_from_slice(&argb_px);
            }
            let mut dst_argb = vec![0u8; dst_w * dst_h];
            filter.blur_pixels(
                MaskFormat::ARGB32,
                &argb,
                16,
                4,
                4,
                &mut dst_argb,
                dst_w,
                dst_w,
                dst_h,
            );
            assert_eq!(dst_argb, dst_a8);

            let lcd_px = 0xFFFFu16.to_ne_bytes();
            let mut lcd = Vec::with_capacity(16 * 2);
            for _ in 0..16 {
                lcd.extend_from_slice(&lcd_px);
            }
            let mut dst_lcd = vec![0u8; dst_w * dst_h];
            filter.blur_pixels(
                MaskFormat::LCD16,
                &lcd,
                8,
                4,
                4,
                &mut dst_lcd,
                dst_w,
                dst_w,
                dst_h,
            );
            assert_eq!(dst_lcd, dst_a8);
        }
    }

    #[test]
    fn test_zero_sigma_and_asymmetric_no_panic() {
        let f_zero = MaskBlurFilter::new(0.0, 0.0);
        assert_eq!(f_zero.compute_border(), IPoint { x: 0, y: 0 });
        let src = [10u8, 20, 30, 40];
        let mut dst = [0u8; 4];
        f_zero.blur_pixels(MaskFormat::A8, &src, 2, 2, 2, &mut dst, 2, 2, 2);
        assert_eq!(dst, src);

        let f_asym = MaskBlurFilter::new(0.0, 3.0);
        let b = f_asym.compute_border();
        assert_eq!(b.x, 0);
        assert!(b.y > 0);
        let dst_h = 2 + 2 * (b.y as usize);
        let mut dst_asym = vec![0u8; 2 * dst_h];
        f_asym.blur_pixels(MaskFormat::A8, &src, 2, 2, 2, &mut dst_asym, 2, 2, dst_h);
        assert!(dst_asym.iter().any(|&v| v > 0));
    }
}
