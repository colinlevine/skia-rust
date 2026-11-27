//! RAW/DNG scaling, dimension validation, `dng_render` sizing, `dng_pixel_buffer`
//! scanline layout, and `onGetPixels` row-conversion planning.

use skia_rs_codec_core::codec::select_xform_format;
use skia_rs_codec_core::types::{skcms_alpha_format, CodecResult};
use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::ISize;

use crate::types::{
    DngPixelBufferLayout, DngRect, RawGetPixelsPlan, DNG_TT_BYTE, RAW_MAX_RENDER_DIFF_RATIO,
    RAW_MIN_SHORT_EDGE, SKCMS_PIXEL_FORMAT_RGB_888,
};

/// DNG SDK preserves aspect ratio, so `dng_host::SetPreferredSize` only needs the longer edge.
#[inline]
pub const fn preferred_render_size(width: i32, height: i32) -> i32 {
    if width >= height {
        width
    } else {
        height
    }
}

/// `dng_render::SetMaximumSize` uses `max(stage3_size.h, stage3_size.v)`.
#[inline]
pub const fn max_render_size(stage3_h: i32, stage3_v: i32) -> i32 {
    if stage3_h >= stage3_v {
        stage3_h
    } else {
        stage3_v
    }
}

/// Exact port of `SkRawCodec::onGetScaledDimensions(float desiredScale)`.
///
/// Uses IEEE-754 `f32` arithmetic identical to Skia C++:
/// - Non-scalable images (no CFA mosaic) return full `dim`.
/// - Clamps `desired_scale` up to `80.0 / min(width, height)` (minimum 80 px short edge).
/// - For X-Trans sensors (6x6 CFA), half-size scaling is unsupported; any scale in
///   `(1/3, 1)` snaps down to `1/3`.
/// - Rounds to integer downscale factor `final_scale = floor(1.0 / desired_scale)` and
///   returns `(floor(width / final_scale), floor(height / final_scale))`.
pub fn get_scaled_dimensions(
    dim: ISize,
    mut desired_scale: f32,
    is_scalable: bool,
    is_xtrans_image: bool,
) -> ISize {
    if !is_scalable {
        return dim;
    }

    // Limits the minimum size to be 80 on the short edge.
    let short_edge = dim.width.min(dim.height) as f32;
    let min_scale = RAW_MIN_SHORT_EDGE / short_edge;
    if desired_scale < min_scale {
        desired_scale = min_scale;
    }

    // For X-Trans images, the integer-factor scaling does not support the half-size scaling case
    // (stronger downscalings are fine). In this case, returns the factor "3" scaling instead.
    if is_xtrans_image && desired_scale > 1.0_f32 / 3.0_f32 && desired_scale < 1.0_f32 {
        desired_scale = 1.0_f32 / 3.0_f32;
    }

    // Round to integer-factors.
    let final_scale = (1.0_f32 / desired_scale).floor();
    ISize {
        width: (dim.width as f32 / final_scale).floor() as i32,
        height: (dim.height as f32 / final_scale).floor() as i32,
    }
}

/// Exact port of `SkRawCodec::onDimensionsSupported(const SkISize& dim)`.
pub fn dimensions_supported(
    full_dim: ISize,
    target_dim: ISize,
    is_scalable: bool,
    is_xtrans_image: bool,
) -> bool {
    let full_short_edge = full_dim.width.min(full_dim.height) as f32;
    let short_edge = target_dim.width.min(target_dim.height) as f32;

    let ratio = full_short_edge / short_edge;
    let size_floor =
        get_scaled_dimensions(full_dim, 1.0_f32 / ratio.floor(), is_scalable, is_xtrans_image);
    let size_ceil =
        get_scaled_dimensions(full_dim, 1.0_f32 / ratio.ceil(), is_scalable, is_xtrans_image);

    size_floor == target_dim || size_ceil == target_dim
}

/// Exact port of the rendered-size check in `SkRawCodec::onGetPixels`:
/// allows up to 1.03x larger rendered dimensions than requested (only the overlapping
/// region is converted), and rejects smaller or >1.03x larger outputs with `kInvalidScale`.
#[inline]
pub fn validate_rendered_size(
    rendered_h: i32,
    rendered_v: i32,
    requested_width: i32,
    requested_height: i32,
) -> CodecResult {
    if requested_width <= 0 || requested_height <= 0 {
        return CodecResult::InvalidScale;
    }
    if (rendered_h as f32) / (requested_width as f32) > RAW_MAX_RENDER_DIFF_RATIO
        || rendered_h < requested_width
        || (rendered_v as f32) / (requested_height as f32) > RAW_MAX_RENDER_DIFF_RATIO
        || rendered_v < requested_height
    {
        return CodecResult::InvalidScale;
    }
    CodecResult::Success
}

/// Computes the `dng_pixel_buffer` plane/stride layout for a single RGB888 scanline of `width`
/// pixels, using checked arithmetic to prevent `width * 3` overflow.
#[inline]
pub const fn build_dng_pixel_buffer_layout(width: i32) -> Option<DngPixelBufferLayout> {
    if width <= 0 {
        return None;
    }
    let Some(row_step) = width.checked_mul(3) else {
        return None;
    };
    let Some(src_row_bytes) = (width as usize).checked_mul(3) else {
        return None;
    };
    Some(DngPixelBufferLayout {
        plane: 0,
        planes: 3,
        col_step: 3,
        plane_step: 1,
        pixel_type: DNG_TT_BYTE,
        pixel_size: 1,
        row_step,
        src_row_bytes,
    })
}

/// Computes the 1-row `dng_rect` (`t = row_index, l = 0, b = row_index + 1, r = width`)
/// passed to `dng_image::Get`.
#[inline]
pub const fn row_area(row_index: i32, width: i32) -> DngRect {
    DngRect {
        t: row_index,
        l: 0,
        b: row_index + 1,
        r: width,
    }
}

/// Validates rendered DNG dimensions, selects the destination `skcms_PixelFormat`, and builds
/// the `dng_pixel_buffer` scanline layout for `SkRawCodec::onGetPixels`.
pub fn plan_get_pixels(
    rendered_h: i32,
    rendered_v: i32,
    dst_width: i32,
    dst_height: i32,
    dst_color_type: ColorType,
    pmcolor_is_rgba: bool,
) -> RawGetPixelsPlan {
    let invalid = |result: CodecResult| RawGetPixelsPlan {
        result: result as i32,
        src_format: SKCMS_PIXEL_FORMAT_RGB_888,
        dst_format: 0,
        src_alpha_format: skcms_alpha_format::UNPREMUL,
        dst_alpha_format: skcms_alpha_format::UNPREMUL,
        row_count: 0,
        pixel_count: 0,
        buffer_layout: DngPixelBufferLayout::default(),
    };

    let size_res = validate_rendered_size(rendered_h, rendered_v, dst_width, dst_height);
    if size_res != CodecResult::Success {
        return invalid(size_res);
    }

    let Some(buffer_layout) = build_dng_pixel_buffer_layout(dst_width) else {
        return invalid(CodecResult::InvalidParameters);
    };

    let Some(dst_format) = select_xform_format(dst_color_type, false, pmcolor_is_rgba) else {
        return invalid(CodecResult::InvalidConversion);
    };

    RawGetPixelsPlan {
        result: CodecResult::Success as i32,
        src_format: SKCMS_PIXEL_FORMAT_RGB_888,
        dst_format,
        src_alpha_format: skcms_alpha_format::UNPREMUL,
        dst_alpha_format: skcms_alpha_format::UNPREMUL,
        row_count: dst_height,
        pixel_count: dst_width,
        buffer_layout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_codec_core::types::skcms_pixel_format;

    #[inline]
    const fn isize(width: i32, height: i32) -> ISize {
        ISize { width, height }
    }

    #[test]
    fn scaled_dimensions_bayer_and_non_scalable() {
        let full = isize(4000, 3000);

        // Non-scalable always returns full dimensions.
        assert_eq!(get_scaled_dimensions(full, 0.5, false, false), full);
        assert_eq!(get_scaled_dimensions(full, 0.1, false, false), full);

        // Scalable Bayer: integer downscale factors.
        assert_eq!(get_scaled_dimensions(full, 1.0, true, false), isize(4000, 3000));
        assert_eq!(get_scaled_dimensions(full, 0.5, true, false), isize(2000, 1500));
        assert_eq!(get_scaled_dimensions(full, 0.4, true, false), isize(2000, 1500));
        assert_eq!(get_scaled_dimensions(full, 0.25, true, false), isize(1000, 750));

        // Clamped to short edge >= 80: 80 / 3000 -> factor floor(3000 / 80) = 37 -> (108, 81).
        let min_scaled = get_scaled_dimensions(full, 0.001, true, false);
        assert_eq!(min_scaled, isize(108, 81));
        assert!(min_scaled.height >= 80);
    }

    #[test]
    fn scaled_dimensions_xtrans_skips_half_scale() {
        let full = isize(4896, 3264);

        // Scale == 1.0 stays full size.
        assert_eq!(get_scaled_dimensions(full, 1.0, true, true), full);

        // Half-size (0.5) is in (1/3, 1) -> snaps to 1/3 -> factor 3.
        assert_eq!(
            get_scaled_dimensions(full, 0.5, true, true),
            isize(4896 / 3, 3264 / 3)
        );
        assert_eq!(
            get_scaled_dimensions(full, 0.9, true, true),
            isize(4896 / 3, 3264 / 3)
        );

        // Stronger downscale (0.25 -> factor 4) is allowed directly.
        assert_eq!(
            get_scaled_dimensions(full, 0.25, true, true),
            isize(4896 / 4, 3264 / 4)
        );
    }

    #[test]
    fn dimensions_supported_checks() {
        let full = isize(4000, 3000);

        // Non-scalable only supports exact full dimensions.
        assert!(dimensions_supported(full, full, false, false));
        assert!(!dimensions_supported(full, isize(2000, 1500), false, false));

        // Scalable Bayer supports exact integer-factor divisions down to short edge 80.
        assert!(dimensions_supported(full, isize(4000, 3000), true, false));
        assert!(dimensions_supported(full, isize(2000, 1500), true, false));
        assert!(dimensions_supported(full, isize(1333, 1000), true, false));
        assert!(dimensions_supported(full, isize(1000, 750), true, false));
        assert!(!dimensions_supported(full, isize(1600, 1200), true, false));
        assert!(!dimensions_supported(full, isize(40, 30), true, false));
        assert!(!dimensions_supported(full, isize(0, 0), true, false));

        // X-Trans rejects 1/2 scale, accepts 1/1, 1/3, 1/4.
        let xfull = isize(4896, 3264);
        assert!(dimensions_supported(xfull, xfull, true, true));
        assert!(!dimensions_supported(xfull, isize(2448, 1632), true, true));
        assert!(dimensions_supported(xfull, isize(1632, 1088), true, true));
        assert!(dimensions_supported(xfull, isize(1224, 816), true, true));
    }

    #[test]
    fn render_size_and_validation() {
        assert_eq!(preferred_render_size(4000, 3000), 4000);
        assert_eq!(preferred_render_size(3000, 4000), 4000);
        assert_eq!(max_render_size(2000, 1500), 2000);
        assert_eq!(max_render_size(1500, 2000), 2000);

        // Exact match -> Success.
        assert_eq!(validate_rendered_size(1000, 1000, 1000, 1000), CodecResult::Success);
        // Up to 1.03x larger -> Success.
        assert_eq!(validate_rendered_size(1030, 1030, 1000, 1000), CodecResult::Success);
        // Strictly > 1.03x larger -> InvalidScale.
        assert_eq!(validate_rendered_size(1031, 1000, 1000, 1000), CodecResult::InvalidScale);
        assert_eq!(validate_rendered_size(1000, 1031, 1000, 1000), CodecResult::InvalidScale);
        // Smaller than requested -> InvalidScale.
        assert_eq!(validate_rendered_size(999, 1000, 1000, 1000), CodecResult::InvalidScale);
        assert_eq!(validate_rendered_size(1000, 999, 1000, 1000), CodecResult::InvalidScale);
    }

    #[test]
    fn pixel_buffer_layout_and_plan_get_pixels() {
        let layout = build_dng_pixel_buffer_layout(1024).unwrap();
        assert_eq!(layout.plane, 0);
        assert_eq!(layout.planes, 3);
        assert_eq!(layout.col_step, 3);
        assert_eq!(layout.plane_step, 1);
        assert_eq!(layout.pixel_type, DNG_TT_BYTE);
        assert_eq!(layout.pixel_size, 1);
        assert_eq!(layout.row_step, 3072);
        assert_eq!(layout.src_row_bytes, 3072);

        assert!(build_dng_pixel_buffer_layout(0).is_none());
        assert!(build_dng_pixel_buffer_layout(-5).is_none());
        assert!(build_dng_pixel_buffer_layout(i32::MAX).is_none());

        assert_eq!(row_area(7, 1024), DngRect::new(7, 0, 8, 1024));

        // Valid RGBA_8888 decode plan.
        let plan = plan_get_pixels(1020, 760, 1000, 750, ColorType::Rgba8888, true);
        assert_eq!(plan.result, CodecResult::Success as i32);
        assert_eq!(plan.src_format, SKCMS_PIXEL_FORMAT_RGB_888);
        assert_eq!(plan.dst_format, skcms_pixel_format::RGBA_8888);
        assert_eq!(plan.src_alpha_format, skcms_alpha_format::UNPREMUL);
        assert_eq!(plan.dst_alpha_format, skcms_alpha_format::UNPREMUL);
        assert_eq!(plan.row_count, 750);
        assert_eq!(plan.pixel_count, 1000);
        assert_eq!(plan.buffer_layout.row_step, 3000);

        // RGB_565 uses BGR_565 (for_color_table = false).
        let plan_565 = plan_get_pixels(100, 100, 100, 100, ColorType::Rgb565, true);
        assert_eq!(plan_565.result, CodecResult::Success as i32);
        assert_eq!(plan_565.dst_format, skcms_pixel_format::BGR_565);

        // Unsupported color type (Alpha8) -> InvalidConversion.
        let plan_bad = plan_get_pixels(100, 100, 100, 100, ColorType::Alpha8, true);
        assert_eq!(plan_bad.result, CodecResult::InvalidConversion as i32);

        // Scale mismatch checked before color conversion -> InvalidScale.
        let plan_scale = plan_get_pixels(200, 100, 100, 100, ColorType::Alpha8, true);
        assert_eq!(plan_scale.result, CodecResult::InvalidScale as i32);
    }

    #[test]
    fn scaled_dimensions_portrait_and_all_xform_color_types() {
        // Portrait orientation (short edge is width).
        let portrait = isize(3000, 4000);
        let min_p = get_scaled_dimensions(portrait, 0.001, true, false);
        assert_eq!(min_p, isize(81, 108));
        assert!(min_p.width >= 80);
        assert!(dimensions_supported(portrait, isize(1500, 2000), true, false));
        assert!(dimensions_supported(portrait, isize(81, 108), true, false));

        // Verify all supported destination ColorTypes in plan_get_pixels.
        let cases = [
            (ColorType::Rgba8888, skcms_pixel_format::RGBA_8888),
            (ColorType::Bgra8888, skcms_pixel_format::BGRA_8888),
            (ColorType::Rgb565, skcms_pixel_format::BGR_565),
            (ColorType::RgbaF16, skcms_pixel_format::RGBA_HHHH),
            (ColorType::Rgba1010102, skcms_pixel_format::RGBA_1010102),
            (ColorType::Bgr101010xXr, skcms_pixel_format::BGR_101010X_XR),
            (ColorType::Gray8, skcms_pixel_format::G_8),
        ];
        for (ct, expected_fmt) in cases {
            let p = plan_get_pixels(640, 480, 640, 480, ct, false);
            assert_eq!(p.result, CodecResult::Success as i32);
            assert_eq!(p.dst_format, expected_fmt);
        }
        assert_eq!(
            plan_get_pixels(640, 480, 640, 480, ColorType::Unknown, true).result,
            CodecResult::InvalidConversion as i32
        );
    }
}
