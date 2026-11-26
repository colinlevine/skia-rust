//! Safe Rust algorithms for `SkBlurMask`.

use crate::mask::{
    MASK_FORMAT_A8, MASK_FORMAT_ARGB32, MASK_FORMAT_BW, MASK_FORMAT_LCD16,
};
use crate::{alpha_255_to_256, alpha_mul, mul_div_255_round, scalar_ceil_to_int};

pub const BLUR_SIGMA_SCALE: f32 = 0.57735_f32;

// SkBlurStyle enum values matching `include/core/SkBlurTypes.h`.
pub const BLUR_STYLE_NORMAL: i32 = 0;
pub const BLUR_STYLE_SOLID: i32 = 1;
pub const BLUR_STYLE_OUTER: i32 = 2;
pub const BLUR_STYLE_INNER: i32 = 3;

/// Matches `SkBlurMask::ConvertRadiusToSigma`.
#[inline]
pub fn convert_radius_to_sigma(radius: f32) -> f32 {
    if radius > 0.0 {
        BLUR_SIGMA_SCALE * radius + 0.5_f32
    } else {
        0.0_f32
    }
}

/// Matches `SkBlurMask::ConvertSigmaToRadius`.
#[inline]
pub fn convert_sigma_to_radius(sigma: f32) -> f32 {
    if sigma > 0.5_f32 {
        (sigma - 0.5_f32) / BLUR_SIGMA_SCALE
    } else {
        0.0_f32
    }
}

/// Extracts coverage alpha at `(x, row_bytes)` from a scanline in the given `SkMask::Format`.
#[inline]
fn read_alpha_from_row(row: &[u8], x: usize, format: u8, a32_shift: u32) -> u8 {
    match format {
        MASK_FORMAT_BW => {
            let byte = row[x >> 3];
            let bit = 7 - (x & 7);
            if ((byte >> bit) & 1) != 0 {
                0xFF
            } else {
                0
            }
        }
        MASK_FORMAT_A8 => row[x],
        MASK_FORMAT_ARGB32 => {
            let off = x * 4;
            let packed = u32::from_ne_bytes([row[off], row[off + 1], row[off + 2], row[off + 3]]);
            ((packed.wrapping_shl(24 - a32_shift)) >> 24) as u8
        }
        MASK_FORMAT_LCD16 => {
            let off = x * 2;
            let packed = u16::from_ne_bytes([row[off], row[off + 1]]) as u32;
            let r5 = (packed >> 11) & 0x1F;
            let g6 = (packed >> 5) & 0x3F;
            let b5 = packed & 0x1F;
            let r = (r5 << 3) | (r5 >> 2);
            let g = (g6 << 2) | (g6 >> 4);
            let b = (b5 << 3) | (b5 >> 2);
            ((r + g + b) / 3) as u8
        }
        _ => 0,
    }
}

/// Matches `merge_src_with_blur` in `SkBlurMask.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn merge_src_with_blur(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    src_format: u8,
    blur: &[u8],
    blur_rb: usize,
    sw: usize,
    sh: usize,
    a32_shift: u32,
) {
    for y in 0..sh {
        let dst_row = &mut dst[y * dst_rb..y * dst_rb + sw];
        let src_row = &src[y * src_rb..];
        let blur_row = &blur[y * blur_rb..y * blur_rb + sw];
        for x in 0..sw {
            let sa = read_alpha_from_row(src_row, x, src_format, a32_shift);
            dst_row[x] = alpha_mul(blur_row[x], alpha_255_to_256(sa));
        }
    }
}

/// Matches `clamp_solid_with_orig` in `SkBlurMask.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn clamp_solid_with_orig(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    src_format: u8,
    sw: usize,
    sh: usize,
    a32_shift: u32,
) {
    for y in 0..sh {
        let dst_row = &mut dst[y * dst_rb..y * dst_rb + sw];
        let src_row = &src[y * src_rb..];
        for x in 0..sw {
            let s = read_alpha_from_row(src_row, x, src_format, a32_shift);
            let d = dst_row[x];
            let v = (s as i32) + (d as i32) - (mul_div_255_round(s, d) as i32);
            dst_row[x] = v as u8;
        }
    }
}

/// Matches `clamp_outer_with_orig` in `SkBlurMask.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn clamp_outer_with_orig(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    src_format: u8,
    sw: usize,
    sh: usize,
    a32_shift: u32,
) {
    for y in 0..sh {
        let dst_row = &mut dst[y * dst_rb..y * dst_rb + sw];
        let src_row = &src[y * src_rb..];
        for x in 0..sw {
            let src_val = read_alpha_from_row(src_row, x, src_format, a32_shift);
            if src_val != 0 {
                dst_row[x] = alpha_mul(dst_row[x], alpha_255_to_256(255 - src_val));
            }
        }
    }
}

/// Piecewise cubic integral of a 3x box-convolved step function (`gaussianIntegral` in `SkBlurMask.cpp`).
#[inline]
pub fn gaussian_integral(x: f32) -> f32 {
    if x > 1.5_f32 {
        return 0.0_f32;
    }
    if x < -1.5_f32 {
        return 1.0_f32;
    }
    let x2 = x * x;
    let x3 = x2 * x;
    if x > 0.5_f32 {
        0.5625_f32 - (x3 / 6.0_f32 - 3.0_f32 * x2 * 0.25_f32 + 1.125_f32 * x)
    } else if x > -0.5_f32 {
        0.5_f32 - (0.75_f32 * x - x3 / 3.0_f32)
    } else {
        0.4375_f32 + (-x3 / 6.0_f32 - 3.0_f32 * x2 * 0.25_f32 - 1.125_f32 * x)
    }
}

/// Matches `SkBlurMask::ComputeBlurProfile(uint8_t* profile, int size, SkScalar sigma)`.
pub fn compute_blur_profile(profile: &mut [u8], sigma: f32) {
    let size = profile.len() as i32;
    debug_assert_eq!(scalar_ceil_to_int(6.0_f32 * sigma), size);
    if profile.is_empty() {
        return;
    }
    let center = size >> 1;
    let invr = 1.0_f32 / (2.0_f32 * sigma);
    profile[0] = 255;
    for x in 1..profile.len() {
        let scaled_x = ((center - (x as i32)) as f32 - 0.5_f32) * invr;
        let gi = gaussian_integral(scaled_x);
        profile[x] = 255u8.wrapping_sub((255.0_f32 * gi) as u8);
    }
}

/// Matches `SkBlurMask::ProfileLookup(const uint8_t* profile, int loc, int blurredWidth, int sharpWidth)`.
#[inline]
pub fn profile_lookup(profile: &[u8], loc: i32, blurred_width: i32, sharp_width: i32) -> u8 {
    let diff = ((loc << 1) + 1) - blurred_width;
    let dx = diff.abs() - sharp_width;
    let ox = (dx >> 1).max(0) as usize;
    profile[ox]
}

/// Matches `SkBlurMask::ComputeBlurredScanline(uint8_t* pixels, const uint8_t* profile, unsigned int width, SkScalar sigma)`.
pub fn compute_blurred_scanline(pixels: &mut [u8], profile: &[u8], sigma: f32) {
    let width = pixels.len() as u32;
    let profile_size = scalar_ceil_to_int(6.0_f32 * sigma) as u32;
    let sw = width.wrapping_sub(profile_size);
    let center = ((profile_size & !1u32) as i32) - 1;
    let w = (sw as i32).wrapping_sub(center);

    for (x_idx, px) in pixels.iter_mut().enumerate() {
        let x = x_idx as u32;
        if profile_size <= sw {
            *px = profile_lookup(profile, x as i32, width as i32, w);
        } else {
            let span = (sw as f32) / (2.0_f32 * sigma);
            let gi_x = 1.5_f32 - ((x as f32) + 0.5_f32) / (2.0_f32 * sigma);
            *px = (255.0_f32 * (gaussian_integral(gi_x) - gaussian_integral(gi_x + span))) as u8;
        }
    }
}

/// Renders analytic blurred rectangle pixels into `dst` (dimensions `dst_w x dst_h`)
/// and applies `kOuter` or `kSolid` masking directly in place.
/// For `kInner`, fills `dst` with the full blur so the caller can crop `inner_dst` (or call `crop_inner_blur_rect`).
#[allow(clippy::too_many_arguments)]
pub fn blur_rect_render_full(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    sigma: f32,
    style: i32,
    pad: usize,
    sw: usize,
) {
    let profile_size = scalar_ceil_to_int(6.0_f32 * sigma) as usize;
    let mut profile = vec![0u8; profile_size];
    compute_blur_profile(&mut profile, sigma);

    let mut horiz = vec![0u8; dst_w];
    let mut vert = vec![0u8; dst_h];
    compute_blurred_scanline(&mut horiz, &profile, sigma);
    compute_blurred_scanline(&mut vert, &profile, sigma);

    let mut out_idx = 0usize;
    for &vy in &vert {
        for &hx in &horiz {
            dst[out_idx] = mul_div_255_round(hx, vy);
            out_idx += 1;
        }
    }

    if style == BLUR_STYLE_OUTER {
        if dst_h > 2 * pad {
            for y in pad..(dst_h - pad) {
                let start = y * dst_w + pad;
                let end = (start + sw).min(dst.len());
                dst[start..end].fill(0);
            }
        }
    } else if style == BLUR_STYLE_SOLID && dst_h > 2 * pad {
        for y in pad..(dst_h - pad) {
            let start = y * dst_w + pad;
            let end = (start + sw).min(dst.len());
            dst[start..end].fill(0xFF);
        }
    }
}

/// Crops the inner region of an analytic blurred rectangle for `kInner_SkBlurStyle`.
pub fn crop_inner_blur_rect(
    full_blur: &[u8],
    dst_w: usize,
    pad: usize,
    inner_dst: &mut [u8],
    sw: usize,
    sh: usize,
) {
    for y in 0..sh {
        let src_start = (y + pad) * dst_w + pad;
        let dst_start = y * sw;
        inner_dst[dst_start..dst_start + sw].copy_from_slice(&full_blur[src_start..src_start + sw]);
    }
}

/// Computes the half-window (pad) size for `SkBlurMask::BlurGroundTruth`.
#[inline]
pub fn ground_truth_pad(sigma: f32) -> i32 {
    let mut window_size = scalar_ceil_to_int(sigma * 6.0_f32);
    window_size |= 1;
    window_size >> 1
}

/// Performs the full discrete 2D separable Gaussian convolution for `SkBlurMask::BlurGroundTruth`.
/// `src` is A8 (`src_w x src_h`, tightly packed at stride `src_w` matching `SkBlurMask.cpp:579`,
/// and `src_rb` is used for style post-processing).
/// `dst` has dimensions `(src_w + 2*pad) x (src_h + 2*pad)` with rowBytes `dst_w = src_w + 2*pad`.
#[allow(clippy::too_many_arguments)]
pub fn blur_ground_truth_convolve(
    sigma: f32,
    src: &[u8],
    src_w: usize,
    src_h: usize,
    src_rb: usize,
    dst: &mut [u8],
    dst_w: usize,
    style: i32,
) {
    let variance = sigma * sigma;
    let window_size = (scalar_ceil_to_int(sigma * 6.0_f32) | 1) as usize;
    let half_window = window_size >> 1;
    let pad = half_window;

    let mut gauss_window = vec![0.0_f32; window_size];
    gauss_window[half_window] = 1.0_f32;
    let mut window_sum = 1.0_f32;
    for x in 1..=half_window {
        let xf = x as f32;
        let gaussian = (-xf * xf / (2.0_f32 * variance)).exp();
        gauss_window[half_window + x] = gaussian;
        gauss_window[half_window - x] = gaussian;
        window_sum += 2.0_f32 * gaussian;
    }

    // Padded copy of source: (src_w + 4*pad) x src_h
    let pad_w = src_w + 4 * pad;
    let pad_h = src_h;
    let mut pad_pixels = vec![0u8; pad_w * pad_h];
    for y in 0..src_h {
        let dst_off = y * pad_w + 2 * pad;
        let src_off = y * src_w;
        pad_pixels[dst_off..dst_off + src_w].copy_from_slice(&src[src_off..src_off + src_w]);
    }

    // Blur in X, transposing into double-padded float buffer: (pad_h + 4*pad) x (pad_w - 2*pad)
    let tmp_w = pad_h + 4 * pad;
    let tmp_h = pad_w - 2 * pad;
    let mut tmp_image = vec![0.0_f32; tmp_w * tmp_h];

    for y in 0..pad_h {
        let row_base = y * pad_w;
        for x in pad..(pad_w - pad) {
            let mut acc = 0.0_f32;
            for k in 0..window_size {
                // i = k - pad, so x + i = x + k - pad
                acc += gauss_window[k] * (pad_pixels[row_base + x + k - pad] as f32);
            }
            tmp_image[(x - pad) * tmp_w + y + 2 * pad] = acc / window_sum;
        }
    }

    // Blur in Y, transposing back into dst
    for y in 0..tmp_h {
        let row_base = y * tmp_w;
        for x in pad..(tmp_w - pad) {
            let mut final_value = 0.0_f32;
            for k in 0..window_size {
                final_value += gauss_window[k] * tmp_image[row_base + x + k - pad];
            }
            final_value /= window_sum;
            let integer_pixel = (final_value + 0.5_f32) as i32;
            let clamped = integer_pixel.clamp(0, 255) as u8;
            dst[(x - pad) * dst_w + y] = clamped;
        }
    }

    match style {
        BLUR_STYLE_SOLID => {
            let start = pad * dst_w + pad;
            clamp_solid_with_orig(
                &mut dst[start..],
                dst_w,
                src,
                src_rb,
                MASK_FORMAT_A8,
                src_w,
                src_h,
                24,
            );
        }
        BLUR_STYLE_OUTER => {
            let start = pad * dst_w + pad;
            clamp_outer_with_orig(
                &mut dst[start..],
                dst_w,
                src,
                src_rb,
                MASK_FORMAT_A8,
                src_w,
                src_h,
                24,
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radius_sigma_conversion() {
        assert_eq!(convert_radius_to_sigma(0.0), 0.0);
        assert_eq!(convert_radius_to_sigma(-5.0), 0.0);
        let s = convert_radius_to_sigma(10.0);
        assert!((s - (0.57735_f32 * 10.0 + 0.5)).abs() < 1e-6);
        assert!((convert_sigma_to_radius(s) - 10.0).abs() < 1e-5);
        assert_eq!(convert_sigma_to_radius(0.5), 0.0);
    }

    #[test]
    fn test_gaussian_integral_and_profile() {
        assert_eq!(gaussian_integral(2.0), 0.0);
        assert_eq!(gaussian_integral(-2.0), 1.0);
        assert!((gaussian_integral(0.0) - 0.5).abs() < 1e-6);
        // Symmetry: g(x) + g(-x) == 1
        for &x in &[0.1_f32, 0.4, 0.5, 0.8, 1.2, 1.49] {
            assert!((gaussian_integral(x) + gaussian_integral(-x) - 1.0).abs() < 1e-5);
        }

        let sigma = 2.0_f32;
        let size = scalar_ceil_to_int(6.0 * sigma) as usize;
        let mut profile = vec![0u8; size];
        compute_blur_profile(&mut profile, sigma);
        assert_eq!(profile[0], 255);
        // Profile should decrease monotonically from 255 toward 0.
        for w in profile.windows(2) {
            assert!(w[0] >= w[1]);
        }
    }

    #[test]
    fn test_blur_rect_and_ground_truth() {
        let sigma = 1.5_f32;
        let sw = 10usize;
        let sh = 10usize;
        let profile_size = scalar_ceil_to_int(6.0 * sigma) as usize;
        let pad = profile_size / 2;
        let dst_w = sw + 2 * pad;
        let dst_h = sh + 2 * pad;
        let mut dst = vec![0u8; dst_w * dst_h];
        blur_rect_render_full(&mut dst, dst_w, dst_h, sigma, BLUR_STYLE_NORMAL, pad, sw);
        // Center pixel should be close to 255, corner pixel close to 0.
        assert!(dst[(dst_h / 2) * dst_w + (dst_w / 2)] > 240);
        assert!(dst[0] < 10);

        // Ground truth on solid 10x10 square
        let src = vec![255u8; sw * sh];
        let gt_pad = ground_truth_pad(sigma) as usize;
        let gt_w = sw + 2 * gt_pad;
        let gt_h = sh + 2 * gt_pad;
        let mut gt_dst = vec![0u8; gt_w * gt_h];
        blur_ground_truth_convolve(sigma, &src, sw, sh, sw, &mut gt_dst, gt_w, BLUR_STYLE_NORMAL);
        assert!(gt_dst[(gt_h / 2) * gt_w + (gt_w / 2)] > 240);
        assert!(gt_dst[0] < 10);
    }

    #[test]
    fn test_style_combiners_formats() {
        // Test BW format solid clamp
        let bw_src = [0b1010_0000u8]; // pixels 0 and 2 are 255, 1 and 3 are 0
        let mut dst = [100u8; 4];
        clamp_solid_with_orig(&mut dst, 4, &bw_src, 1, MASK_FORMAT_BW, 4, 1, 24);
        assert_eq!(dst, [255, 100, 255, 100]);

        // Test BW format outer clamp
        let mut dst2 = [100u8; 4];
        clamp_outer_with_orig(&mut dst2, 4, &bw_src, 1, MASK_FORMAT_BW, 4, 1, 24);
        assert_eq!(dst2, [0, 100, 0, 100]);

        // Test ARGB32 merge
        let argb: [u32; 2] = [0xFF00_0000, 0x8000_0000];
        let mut argb_bytes = [0u8; 8];
        argb_bytes[0..4].copy_from_slice(&argb[0].to_ne_bytes());
        argb_bytes[4..8].copy_from_slice(&argb[1].to_ne_bytes());
        let blur = [200u8, 200u8];
        let mut out = [0u8; 2];
        merge_src_with_blur(&mut out, 2, &argb_bytes, 8, MASK_FORMAT_ARGB32, &blur, 2, 2, 1, 24);
        assert_eq!(out[0], 200);
        assert_eq!(out[1], alpha_mul(200, alpha_255_to_256(0x80)));
    }
}
