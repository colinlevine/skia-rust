//! Safe Rust implementations for Skia Mask Filters:
//! - `SkEmbossMask::Emboss` (fixed-point normal, diffuse, and specular lighting)
//! - `SkEmbossMaskFilter` (light normalization, matrix transform, margin)
//! - `SkTableMaskFilter` (gamma & clip LUT generation, table application)
//! - `SkBlurMaskFilterImpl` (sigma transforms, fast bounds, RRect/Rects nine-patch planning)
//! - `SkMaskFilterBase` (nine-patch sub-region geometry)

use crate::matrix_map_vector;
use crate::path_effects::{float_ceil_to_int, float_round_to_int};
use skia_rs_geometry::{cpp_min_f32, IPoint, IRect, Matrix, Point, Point3, Rect, Vector, V2};
use skia_rs_math::math_priv::sqrt_bits;

/// Maximum device-space blur sigma (`kMaxBlurDeviceSigma` in `SkBlurMaskFilterImpl.cpp`).
pub const MAX_BLUR_DEVICE_SIGMA: f32 = 128.0;

#[inline]
fn float_saturate_to_int(x: f32) -> i32 {
    // Matches sk_float_saturate2int: pin to [SK_MinS32FitsInFloat, SK_MaxS32FitsInFloat]
    const MAX_S32_FITS_IN_FLOAT: f32 = 2147483520.0_f32;
    const MIN_S32_FITS_IN_FLOAT: f32 = -(MAX_S32_FITS_IN_FLOAT);
    let clamped = if x < MIN_S32_FITS_IN_FLOAT {
        MIN_S32_FITS_IN_FLOAT
    } else if x > MAX_S32_FITS_IN_FLOAT {
        MAX_S32_FITS_IN_FLOAT
    } else if x.is_nan() {
        MAX_S32_FITS_IN_FLOAT
    } else {
        x
    };
    clamped as i32
}

#[inline]
fn scalar_to_fixed(x: f32) -> i32 {
    float_saturate_to_int(x * 65536.0)
}

#[inline]
fn div255(x: u32) -> u32 {
    debug_assert!(x <= 255 * 255);
    x.wrapping_mul((1u32 << 24) / 255) >> 24
}

// ============================================================================
// SkEmbossMask & SkEmbossMaskFilter
// ============================================================================

/// Mirrors `SkEmbossMaskFilter::Light` (`#[repr(C)]`, 16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmbossLight {
    pub direction: [f32; 3],
    pub pad: u16,
    pub ambient: u8,
    pub specular: u8,
}

/// Validates and normalizes `SkEmbossMaskFilter::Make` arguments.
/// Returns `Some(normalized_light)` or `None`.
pub fn emboss_make_light(blur_sigma: f32, light: EmbossLight) -> Option<EmbossLight> {
    if !blur_sigma.is_finite() || blur_sigma <= 0.0 {
        return None;
    }
    let mut dir = Point3::new(light.direction[0], light.direction[1], light.direction[2]);
    if !dir.normalize() {
        return None;
    }
    let mut out = light;
    out.direction = [dir.x, dir.y, dir.z];
    Some(out)
}

/// Transforms `light.direction` XY through `matrix` and restores the original XY length,
/// matching `SkEmbossMaskFilter::filterMask`.
pub fn emboss_transform_light(light: EmbossLight, matrix: &Matrix) -> EmbossLight {
    let mut out = light;
    let src_v = Vector::new(light.direction[0], light.direction[1]);
    let mapped = matrix_map_vector(matrix, src_v);
    let orig_len = Point::length(light.direction[0], light.direction[1]);
    let mut vec = mapped;
    vec.set_length(mapped.x, mapped.y, orig_len);
    out.direction[0] = vec.x;
    out.direction[1] = vec.y;
    out
}

/// Computes the margin for `SkEmbossMaskFilter::filterMask`.
#[inline]
pub fn emboss_margin(sigma: f32) -> IPoint {
    let m = float_ceil_to_int(3.0 * sigma);
    IPoint { x: m, y: m }
}

/// Exact port of `SkEmbossMask::Emboss`.
/// `planes` contains 3 consecutive planes of `plane_size` bytes: `[alpha | multiply | additive]`.
pub fn emboss_mask(
    planes: &mut [u8],
    plane_size: usize,
    width: i32,
    height: i32,
    row_bytes: usize,
    light: EmbossLight,
) {
    if width <= 0 || height <= 0 || plane_size == 0 || planes.len() < plane_size * 3 {
        return;
    }

    const K_DELTA: i32 = 32;

    let specular = light.specular as i32;
    let ambient = light.ambient as i32;
    let lx = scalar_to_fixed(light.direction[0]);
    let ly = scalar_to_fixed(light.direction[1]);
    let lz = scalar_to_fixed(light.direction[2]);
    let lz_dot_nz = lz.wrapping_mul(K_DELTA);
    let lz_dot8 = lz >> 8;

    let (alpha_plane, rest) = planes.split_at_mut(plane_size);
    let (multiply_plane, rest2) = rest.split_at_mut(plane_size);
    let additive_plane = &mut rest2[..plane_size];

    let maxy = (height - 1) as usize;
    let maxx = (width - 1) as usize;

    let mut prev_row: usize = 0;
    let mut row_off: usize = 0;
    for y in 0..=maxy {
        let next_row: usize = if y != maxy { row_bytes } else { 0 };

        for x in 0..=maxx {
            let x_plus = if x != maxx { x + 1 } else { x };
            let x_minus = if x != 0 { x - 1 } else { x };

            let nx = (alpha_plane[row_off + x_plus] as i32) - (alpha_plane[row_off + x_minus] as i32);
            let ny = (alpha_plane[row_off + next_row + x] as i32)
                - (alpha_plane[row_off - prev_row + x] as i32);

            let numer = lx.wrapping_mul(nx).wrapping_add(ly.wrapping_mul(ny)).wrapping_add(lz_dot_nz);
            let mut mul = ambient;
            let mut add = 0i32;

            if numer > 0 {
                let denom = sqrt_bits(nx * nx + ny * ny + K_DELTA * K_DELTA, 15);
                let mut dot = numer / denom;
                dot >>= 8;
                mul = (mul + dot).min(255);

                let mut hilite = ((2 * dot - lz_dot8) * lz_dot8) >> 8;
                if hilite > 0 {
                    hilite = hilite.min(255);
                    add = hilite;
                    for _ in 0..(specular >> 4) {
                        add = div255((add * hilite) as u32) as i32;
                    }
                }
            }
            multiply_plane[row_off + x] = mul as u8;
            additive_plane[row_off + x] = add as u8;
        }
        row_off += row_bytes;
        prev_row = row_bytes;
    }
}

// ============================================================================
// SkTableMaskFilter
// ============================================================================

/// Exact port of `SkTableMaskFilter::MakeGammaTable`.
pub fn make_gamma_table(table: &mut [u8; 256], gamma: f32) {
    let dx = 1.0_f32 / 255.0_f32;
    let g = gamma;
    let mut x = 0.0_f32;
    for slot in table.iter_mut() {
        let v = float_round_to_int(x.powf(g) * 255.0);
        *slot = v.clamp(0, 255) as u8;
        x += dx;
    }
}

/// Exact port of `SkTableMaskFilter::MakeClipTable`.
pub fn make_clip_table(table: &mut [u8; 256], mut min: u8, mut max: u8) {
    if max == 0 {
        max = 1;
    }
    if min >= max {
        min = max - 1;
    }
    debug_assert!(min < max);

    let scale: i32 = ((1i32 << 16) * 255) / ((max - min) as i32);
    for slot in table.iter_mut().take((min as usize) + 1) {
        *slot = 0;
    }
    for i in ((min as usize) + 1)..(max as usize) {
        let value = (scale * ((i as i32) - (min as i32)) + (1 << 15)) >> 16;
        debug_assert!(value <= 255);
        table[i] = value as u8;
    }
    for slot in table.iter_mut().skip(max as usize) {
        *slot = 255;
    }
}

/// Applies `table` to every pixel of an A8 mask and zeroes row-padding bytes,
/// matching `SkTableMaskFilterImpl::filterMask`.
pub fn apply_table_mask(
    src: &[u8],
    src_row_bytes: usize,
    dst: &mut [u8],
    dst_row_bytes: usize,
    width: usize,
    height: usize,
    table: &[u8; 256],
) {
    let extra_zeros = dst_row_bytes.saturating_sub(width);
    for y in 0..height {
        let src_row = &src[y * src_row_bytes..y * src_row_bytes + width];
        let dst_row_start = y * dst_row_bytes;
        let dst_row = &mut dst[dst_row_start..dst_row_start + width];
        for x in 0..width {
            dst_row[x] = table[src_row[x] as usize];
        }
        for z in 0..extra_zeros {
            dst[dst_row_start + width + z] = 0;
        }
    }
}

/// Copies a rectangular byte region row by row, matching `rect_memcpy` in
/// `SkShaderMaskFilterImpl.cpp`.
pub fn rect_copy_bytes(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    copy_bytes: usize,
    rows: usize,
) {
    for r in 0..rows {
        let s = &src[r * src_rb..r * src_rb + copy_bytes];
        let d = &mut dst[r * dst_rb..r * dst_rb + copy_bytes];
        d.copy_from_slice(s);
    }
}

// ============================================================================
// SkBlurMaskFilterImpl & SkMaskFilterBase NinePatch
// ============================================================================

/// Validates `SkMaskFilter::MakeBlur` sigma.
#[inline]
pub fn blur_mask_filter_is_valid_sigma(sigma: f32) -> bool {
    sigma.is_finite() && sigma > 0.0
}

/// Exact port of `SkBlurMaskFilterImpl::computeXformedDeviceSigma`.
#[inline]
pub fn blur_compute_xformed_device_sigma(sigma: f32, ignore_xform: bool, ctm: &Matrix) -> f32 {
    let xformed = if ignore_xform {
        sigma
    } else {
        ctm.map_radius(sigma)
    };
    cpp_min_f32(xformed, MAX_BLUR_DEVICE_SIGMA)
}

/// Exact port of `SkBlurMaskFilterImpl::computeXformedLocalSigma`.
pub fn blur_compute_xformed_local_sigma(sigma: f32, ignore_xform: bool, ctm: &Matrix) -> V2 {
    let mut local_sigma = V2 { x: 0.0, y: 0.0 };
    if let Some((scale, _)) = ctm.decompose_scale() {
        if ignore_xform {
            let clamped = cpp_min_f32(sigma, MAX_BLUR_DEVICE_SIGMA);
            local_sigma.x = clamped / scale.width;
            local_sigma.y = clamped / scale.height;
        } else {
            local_sigma.x = cpp_min_f32(sigma, MAX_BLUR_DEVICE_SIGMA / scale.width);
            local_sigma.y = cpp_min_f32(sigma, MAX_BLUR_DEVICE_SIGMA / scale.height);
        }
    }
    local_sigma
}

/// Exact port of sigma calculation in `SkBlurMaskFilterImpl::asImageFilter`.
pub fn blur_as_image_filter_sigma(sigma: f32, ignore_xform: bool, ctm: &Matrix) -> V2 {
    if !ignore_xform {
        return V2 { x: sigma, y: sigma };
    }
    let vx = matrix_map_vector(ctm, Vector::new(sigma, 0.0));
    let vy = matrix_map_vector(ctm, Vector::new(0.0, sigma));
    let x_scale = sigma / Point::length(vx.x, vx.y);
    let y_scale = sigma / Point::length(vy.x, vy.y);
    V2 {
        x: sigma * x_scale,
        y: sigma * y_scale,
    }
}

/// Exact port of `SkBlurMaskFilterImpl::computeFastBounds`.
#[inline]
pub fn blur_compute_fast_bounds(src: Rect, sigma: f32) -> Rect {
    let pad = 3.0 * sigma;
    Rect::make_ltrb(
        src.left - pad,
        src.top - pad,
        src.right + pad,
        src.bottom + pad,
    )
}

/// Exact port of `rect_exceeds` in `SkBlurMaskFilterImpl.cpp`.
#[inline]
pub fn rect_exceeds(r: Rect, v: f32) -> bool {
    r.left < -v
        || r.top < -v
        || r.right > v
        || r.bottom > v
        || r.width() > v
        || r.height() > v
}

/// Plan for RRect nine-patch blur (`SkBlurMaskFilterImpl::filterRRectToNine`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RRectNinePatchPlan {
    pub valid: bool,
    pub _pad: [u8; 3],
    pub total_small_width: i32,
    pub total_small_height: i32,
    pub center: IPoint,
}

/// Computes the small RRect dimensions and nine-patch center for
/// `SkBlurMaskFilterImpl::filterRRectToNine`.
pub fn plan_rrect_nine_patch(
    rect: Rect,
    radii: &[Vector; 4],
    margin: IPoint,
) -> RRectNinePatchPlan {
    let invalid = RRectNinePatchPlan {
        valid: false,
        _pad: [0; 3],
        total_small_width: 0,
        total_small_height: 0,
        center: IPoint { x: 0, y: 0 },
    };
    let ul = radii[0];
    let ur = radii[1];
    let lr = radii[2];
    let ll = radii[3];

    let max_left_rx = if ul.x < ll.x { ll.x } else { ul.x };
    let max_right_rx = if ur.x < lr.x { lr.x } else { ur.x };
    let left_unstretched = float_ceil_to_int(max_left_rx) + margin.x;
    let right_unstretched = float_ceil_to_int(max_right_rx) + margin.x;

    let stretch_size = 1;
    let total_small_width = left_unstretched + right_unstretched + stretch_size;
    if (total_small_width as f32) >= rect.width() {
        return invalid;
    }

    let max_top_ry = if ul.y < ur.y { ur.y } else { ul.y };
    let max_bot_ry = if ll.y < lr.y { lr.y } else { ll.y };
    let top_unstretched = float_ceil_to_int(max_top_ry) + margin.y;
    let bot_unstretched = float_ceil_to_int(max_bot_ry) + margin.y;

    let total_small_height = top_unstretched + bot_unstretched + stretch_size;
    if (total_small_height as f32) >= rect.height() {
        return invalid;
    }

    RRectNinePatchPlan {
        valid: true,
        _pad: [0; 3],
        total_small_width,
        total_small_height,
        center: IPoint {
            x: margin.x + left_unstretched,
            y: margin.y + top_unstretched,
        },
    }
}

/// Plan for 1- or 2-rect nine-patch blur (`SkBlurMaskFilterImpl::filterRectsToNine`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectsNinePatchPlan {
    pub valid: bool,
    pub _pad: [u8; 3],
    pub center: IPoint,
    pub small_r0: Rect,
    pub small_r1: Rect,
}

/// Computes `smallR[0]`, `smallR[1]`, and `center` for
/// `SkBlurMaskFilterImpl::filterRectsToNine`.
pub fn plan_rects_nine_patch(
    r0: Rect,
    r1: Option<Rect>,
    src_bounds: IRect,
    dst_bounds: IRect,
) -> RectsNinePatchPlan {
    let invalid = RectsNinePatchPlan {
        valid: false,
        _pad: [0; 3],
        center: IPoint { x: 0, y: 0 },
        small_r0: Rect::make_ltrb(0.0, 0.0, 0.0, 0.0),
        small_r1: Rect::make_ltrb(0.0, 0.0, 0.0, 0.0),
    };

    let mut small_w = dst_bounds.width() - src_bounds.width() + 2;
    let mut small_h = dst_bounds.height() - src_bounds.height() + 2;

    let (inner_ir, center) = if let Some(rect1) = r1 {
        let inner = rect1.round_in();
        let c = IPoint {
            x: small_w + (inner.left - src_bounds.left),
            y: small_h + (inner.top - src_bounds.top),
        };
        (inner, c)
    } else {
        (src_bounds, IPoint { x: small_w, y: small_h })
    };

    small_w += 1;
    small_h += 1;

    let dx = (inner_ir.width() - small_w) as f32;
    let dy = (inner_ir.height() - small_h) as f32;
    if dx < 0.0 || dy < 0.0 {
        return invalid;
    }

    let small_r0 = Rect::make_ltrb(r0.left, r0.top, r0.right - dx, r0.bottom - dy);
    if small_r0.width() < 2.0 || small_r0.height() < 2.0 {
        return invalid;
    }

    let small_r1 = if let Some(rect1) = r1 {
        Rect::make_ltrb(rect1.left, rect1.top, rect1.right - dx, rect1.bottom - dy)
    } else {
        Rect::make_ltrb(0.0, 0.0, 0.0, 0.0)
    };

    RectsNinePatchPlan {
        valid: true,
        _pad: [0; 3],
        center,
        small_r0,
        small_r1,
    }
}

/// Geometry of a nine-patch blit (`draw_nine_clipped` in `SkMaskFilterBase.cpp`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NinePatchLayout {
    pub corners_src: [IRect; 4],
    pub corners_dst_xy: [IPoint; 4],
    pub inner_r: IRect,
    pub top_r: IRect,
    pub bottom_r: IRect,
    pub left_r: IRect,
    pub right_r: IRect,
}

/// Computes all sub-rectangles for `draw_nine_clipped` in `SkMaskFilterBase.cpp`.
pub fn compute_nine_patch_layout(
    mask_bounds: IRect,
    outer_r: IRect,
    center: IPoint,
) -> NinePatchLayout {
    let cx = center.x;
    let cy = center.y;

    // 0: top-left, 1: top-right, 2: bottom-left, 3: bottom-right
    let tl = IRect::make_ltrb(mask_bounds.left, mask_bounds.top, cx, cy);
    let tr = IRect::make_ltrb(cx + 1, mask_bounds.top, mask_bounds.right, cy);
    let bl = IRect::make_ltrb(mask_bounds.left, cy + 1, cx, mask_bounds.bottom);
    let br = IRect::make_ltrb(cx + 1, cy + 1, mask_bounds.right, mask_bounds.bottom);

    let tl_xy = IPoint { x: outer_r.left, y: outer_r.top };
    let tr_xy = IPoint { x: outer_r.right - tr.width(), y: outer_r.top };
    let bl_xy = IPoint { x: outer_r.left, y: outer_r.bottom - bl.height() };
    let br_xy = IPoint { x: outer_r.right - br.width(), y: outer_r.bottom - br.height() };

    let inner_r = IRect::make_ltrb(
        outer_r.left + cx - mask_bounds.left,
        outer_r.top + cy - mask_bounds.top,
        outer_r.right + (cx + 1 - mask_bounds.right),
        outer_r.bottom + (cy + 1 - mask_bounds.bottom),
    );

    let top_r = IRect::make_ltrb(inner_r.left, outer_r.top, inner_r.right, inner_r.top);
    let bottom_r = IRect::make_ltrb(inner_r.left, inner_r.bottom, inner_r.right, outer_r.bottom);
    let left_r = IRect::make_ltrb(outer_r.left, inner_r.top, inner_r.left, inner_r.bottom);
    let right_r = IRect::make_ltrb(inner_r.right, inner_r.top, outer_r.right, inner_r.bottom);

    NinePatchLayout {
        corners_src: [tl, tr, bl, br],
        corners_dst_xy: [tl_xy, tr_xy, bl_xy, br_xy],
        inner_r,
        top_r,
        bottom_r,
        left_r,
        right_r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_gamma_table() {
        let mut table = [0u8; 256];
        make_gamma_table(&mut table, 1.0);
        assert_eq!(table[0], 0);
        assert_eq!(table[128], 128);
        assert_eq!(table[255], 255);

        make_gamma_table(&mut table, 2.0);
        assert_eq!(table[0], 0);
        assert!(table[128] < 128);
        assert_eq!(table[255], 255);
    }

    #[test]
    fn test_make_clip_table() {
        let mut table = [0u8; 256];
        make_clip_table(&mut table, 50, 200);
        assert_eq!(table[0], 0);
        assert_eq!(table[50], 0);
        assert!(table[125] > 120 && table[125] < 135);
        assert_eq!(table[200], 255);
        assert_eq!(table[255], 255);

        // Edge case: min >= max, max == 0
        make_clip_table(&mut table, 10, 0);
        assert_eq!(table[0], 0);
        assert_eq!(table[1], 255);
    }

    #[test]
    fn test_apply_table_mask() {
        let mut table = [0u8; 256];
        for i in 0..256 {
            table[i] = (255 - i) as u8;
        }
        let src = [10u8, 20, 30, 40, 50, 60];
        let mut dst = [0xAAu8; 8];
        apply_table_mask(&src, 3, &mut dst, 4, 3, 2, &table);
        assert_eq!(dst, [245, 235, 225, 0, 215, 205, 195, 0]);
    }

    #[test]
    fn test_emboss_mask_flat_and_sloped() {
        let light = emboss_make_light(
            2.0,
            EmbossLight {
                direction: [1.0, 1.0, 1.0],
                pad: 0,
                ambient: 64,
                specular: 32,
            },
        )
        .unwrap();
        let w = 4usize;
        let h = 4usize;
        let plane = w * h;
        let mut buf = vec![0u8; plane * 3];
        // Gradient in alpha
        for y in 0..h {
            for x in 0..w {
                buf[y * w + x] = ((x + y) * 32) as u8;
            }
        }
        emboss_mask(&mut buf, plane, w as i32, h as i32, w, light);
        // Every multiply value should be >= ambient (since gradient points toward positive light)
        for &m in &buf[plane..2 * plane] {
            assert!(m >= 64);
        }
    }

    #[test]
    fn test_blur_sigma_and_nine_patch() {
        assert!(blur_mask_filter_is_valid_sigma(1.5));
        assert!(!blur_mask_filter_is_valid_sigma(0.0));
        assert!(!blur_mask_filter_is_valid_sigma(f32::NAN));

        let mut m = Matrix::identity();
        m.set_scale(2.0, 2.0);
        assert_eq!(blur_compute_xformed_device_sigma(10.0, false, &m), 20.0);
        assert_eq!(blur_compute_xformed_device_sigma(10.0, true, &m), 10.0);
        assert_eq!(blur_compute_xformed_device_sigma(100.0, false, &m), 128.0);

        let radii = [
            Vector::new(2.5, 2.0),
            Vector::new(2.5, 2.0),
            Vector::new(2.5, 2.0),
            Vector::new(2.5, 2.0),
        ];
        let plan = plan_rrect_nine_patch(
            Rect::make_ltrb(0.0, 0.0, 16.0, 12.0),
            &radii,
            IPoint { x: 1, y: 1 },
        );
        assert!(plan.valid);
        assert_eq!(plan.total_small_width, 9);
        assert_eq!(plan.total_small_height, 7);
        assert_eq!(plan.center, IPoint { x: 5, y: 4 });
    }
}
