//! Safe Rust canvas algorithms matching:
//! - `src/core/SkCanvas.cpp`
//! - `src/core/SkCanvas_Raster.cpp`
//! - `src/core/SkCanvasPriv.cpp`
//! - `src/core/SkOverdrawCanvas.cpp`

use skia_rs_core_types::align4;
use skia_rs_geometry::{IRect, ISize, Matrix, Point, Rect, M44};

use crate::{
    ieee_float_divide, irect_from_xywh, m44_as_m33, matrix_map_rect, pin_i64_to_i32,
    rect_from_irect, rect_from_wh, COLOR_TYPE_BGRA_8888, COLOR_TYPE_RGBA_8888, PAINT_STYLE_FILL,
    PAINT_STYLE_STROKE_AND_FILL,
};

/// `SkCanvas::drawTextBlob` max glyph count (`1 << 21`).
pub const MAX_TEXT_BLOB_GLYPHS: i32 = 1 << 21;

/// `kMinDimThreshold = 2048` in `get_layer_mapping_and_bounds`.
pub const LAYER_MIN_DIM_THRESHOLD: i32 = 2048;

/// Action for `SkCanvas::drawDRRect`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DrawDRRectAction {
    Skip = 0,
    DrawOuterOnly = 1,
    DrawDRRect = 2,
}

/// Output of `SkCanvasPriv::GetDstClipAndMatrixCounts`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DstClipMatrixCounts {
    pub total_dst_clip_count: i32,
    pub total_matrix_count: i32,
}

/// Output of `SkCanvas::writePixels` preflight check.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WritePixelsPreflight {
    pub intersects: bool,
    pub complete_overwrite: bool,
}

/// Output of `padded_layer_bounds` overflow-safe outset check.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PaddedLayerBoundsResult {
    pub padded: bool,
    pub bounds: IRect,
}

/// Output of `clean_sampling_for_constraint`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CleanedSamplingFlags {
    pub changed: bool,
    /// `0` = keep original, `1` = `SkSamplingOptions(filter)` (strip mipmap),
    /// `2` = `SkSamplingOptions(kLinear)` (strip aniso).
    pub mode: i32,
}

/// Matches `compute_decomposition_center` in `SkCanvas.cpp`.
pub fn compute_decomposition_center(
    dst_to_local: &M44,
    content_bounds: Option<Rect>,
    target_output: IRect,
) -> Point {
    let rect = content_bounds.unwrap_or_else(|| rect_from_irect(target_output));
    let mut center = Point {
        x: rect.center_x(),
        y: rect.center_y(),
    };
    if content_bounds.is_none() {
        let mapped = dst_to_local.map(center.x, center.y, 0.0, 1.0);
        center = Point {
            x: ieee_float_divide(mapped.x, mapped.w),
            y: ieee_float_divide(mapped.y, mapped.w),
        };
    }
    center
}

/// Matches `maxLayerDim` calculation in `get_layer_mapping_and_bounds`.
pub fn compute_max_layer_dim(target_output: IRect) -> i32 {
    let w64 = (target_output.right as i64) - (target_output.left as i64);
    let h64 = (target_output.bottom as i64) - (target_output.top as i64);
    let max_wh = w64.max(h64);
    let doubled = pin_i64_to_i32(max_wh.saturating_mul(2));
    doubled.max(LAYER_MIN_DIM_THRESHOLD)
}

/// Matches `image_filter_color_type` in `SkCanvas.cpp`.
#[inline]
pub fn image_filter_color_type(color_type: i32, bytes_per_pixel: i32, n32_color_type: i32) -> i32 {
    if bytes_per_pixel <= 4
        && color_type != COLOR_TYPE_RGBA_8888
        && color_type != COLOR_TYPE_BGRA_8888
    {
        n32_color_type
    } else {
        color_type
    }
}

/// Matches `SkCanvas::internalSaveLayer` overflow-safe padding outset check.
pub fn compute_padded_layer_bounds(layer_bounds: IRect, pad: i32) -> PaddedLayerBoundsResult {
    let padded_bounds = IRect {
        left: layer_bounds.left.saturating_sub(pad),
        top: layer_bounds.top.saturating_sub(pad),
        right: layer_bounds.right.saturating_add(pad),
        bottom: layer_bounds.bottom.saturating_add(pad),
    };
    if padded_bounds.left < layer_bounds.left
        && padded_bounds.top < layer_bounds.top
        && padded_bounds.right > layer_bounds.right
        && padded_bounds.bottom > layer_bounds.bottom
    {
        PaddedLayerBoundsResult {
            padded: true,
            bounds: padded_bounds,
        }
    } else {
        PaddedLayerBoundsResult {
            padded: false,
            bounds: layer_bounds,
        }
    }
}

/// Matches `SkCanvas::wouldOverwriteEntireSurface` geometry & paint eligibility check.
#[allow(clippy::too_many_arguments)]
pub fn would_overwrite_check_geometry_and_paint(
    base_size: ISize,
    is_root_device: bool,
    root_clip_wide_open: bool,
    rect: Option<Rect>,
    total_matrix: &Matrix,
    has_paint: bool,
    paint_style: i32,
    has_mask_filter: bool,
    has_path_effect: bool,
    has_image_filter: bool,
) -> bool {
    if !is_root_device || !root_clip_wide_open {
        return false;
    }
    let bounds = rect_from_wh(base_size.width as f32, base_size.height as f32);
    if let Some(r) = rect {
        if !total_matrix.is_scale_translate() {
            return false;
        }
        let dev_rect = total_matrix.map_rect_scale_translate(r);
        if !dev_rect.contains_rect(bounds) {
            return false;
        }
    }
    if has_paint {
        if !(paint_style == PAINT_STYLE_FILL || paint_style == PAINT_STYLE_STROKE_AND_FILL) {
            return false;
        }
        if has_mask_filter || has_path_effect || has_image_filter {
            return false;
        }
    }
    true
}

/// Matches `SkCanvas::quickReject(const SkRect&)`.
#[inline]
pub fn quick_reject_rect(m44: &M44, src: Rect, quick_reject_bounds: Rect) -> bool {
    let dev_rect = m44.map_rect(src);
    !dev_rect.is_finite() || !Rect::intersects(dev_rect, quick_reject_bounds)
}

/// Matches `SkCanvas::computeDeviceClipBounds`.
pub fn compute_device_clip_bounds(
    clip_empty: bool,
    device_to_global: &M44,
    dev_clip_bounds: IRect,
    outset_for_aa: bool,
) -> Rect {
    if clip_empty {
        return Rect::default();
    }
    let mut r = device_to_global.map_rect(rect_from_irect(dev_clip_bounds));
    if outset_for_aa {
        r = r.make_inset(-1.0, -1.0);
    }
    r
}

/// Matches `SkCanvas::getLocalClipBounds`.
pub fn compute_local_clip_bounds(device_clip_bounds: IRect, ctm: &M44) -> Rect {
    if device_clip_bounds.is_empty() {
        return Rect::default();
    }
    let m33 = m44_as_m33(ctm);
    let Some(inv) = m33.invert() else {
        return Rect::default();
    };
    let outset = IRect {
        left: device_clip_bounds.left.saturating_sub(1),
        top: device_clip_bounds.top.saturating_sub(1),
        right: device_clip_bounds.right.saturating_add(1),
        bottom: device_clip_bounds.bottom.saturating_add(1),
    };
    matrix_map_rect(&inv, rect_from_irect(outset))
}

/// Matches `fillable(const SkRect&)` in `SkCanvas.cpp`.
#[inline]
pub fn fillable_rect(r: Rect) -> bool {
    let w = r.width();
    let h = r.height();
    w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0
}

/// Matches `clean_sampling_for_constraint` in `SkCanvas.cpp`.
#[inline]
pub fn clean_sampling_for_constraint(
    is_strict: bool,
    has_mipmap: bool,
    is_aniso: bool,
) -> CleanedSamplingFlags {
    if is_strict {
        if has_mipmap {
            return CleanedSamplingFlags {
                changed: true,
                mode: 1,
            };
        }
        if is_aniso {
            return CleanedSamplingFlags {
                changed: true,
                mode: 2,
            };
        }
    }
    CleanedSamplingFlags {
        changed: false,
        mode: 0,
    }
}

/// Matches single-entry check in `SkCanvas::experimental_DrawEdgeAAImageSet`.
#[inline]
pub fn edge_aa_single_entry_can_map_dst(
    has_clip: bool,
    matrix_index: i32,
    pre_view_matrix: Option<&Matrix>,
) -> bool {
    if has_clip {
        return false;
    }
    if matrix_index < 0 {
        return true;
    }
    if let Some(m) = pre_view_matrix {
        m.is_scale_translate() && m.mat[0] > 0.0 && m.mat[4] > 0.0
    } else {
        false
    }
}

/// Matches `SkCanvas::drawDRRect` early-out and delegation logic.
#[inline]
pub fn draw_drrect_action(
    outer_empty: bool,
    inner_empty: bool,
    outer_bounds: Rect,
    inner_bounds: Rect,
) -> DrawDRRectAction {
    if outer_empty {
        return DrawDRRectAction::Skip;
    }
    if inner_empty {
        return DrawDRRectAction::DrawOuterOnly;
    }
    if !outer_bounds.contains_rect(inner_bounds) {
        return DrawDRRectAction::Skip;
    }
    DrawDRRectAction::DrawDRRect
}

/// Matches `SkCanvas::drawCircle` bounding rect calculation.
#[inline]
pub fn draw_circle_bounds(cx: f32, cy: f32, mut radius: f32) -> Rect {
    if radius < 0.0 {
        radius = 0.0;
    }
    Rect::from_ltrb(cx - radius, cy - radius, cx + radius, cy + radius)
}

/// Matches `SkCanvas::drawRoundRect` rrect vs rect decision.
#[inline]
pub fn draw_round_rect_is_rrect(rx: f32, ry: f32) -> bool {
    rx > 0.0 && ry > 0.0
}

/// Matches `SkCanvas::drawArc` skip decision.
#[inline]
pub fn draw_arc_should_skip(oval: Rect, sweep_angle: f32) -> bool {
    oval.is_empty() || sweep_angle == 0.0
}

/// Matches `SkCanvas::drawTextBlob` glyph count budget check (`kMaxGlyphCount = 1 << 21`).
#[inline]
pub fn text_blob_add_glyph_count(total: i32, run_glyph_count: i32) -> Option<i32> {
    let glyphs_left = MAX_TEXT_BLOB_GLYPHS - total;
    if run_glyph_count <= glyphs_left {
        Some(total + run_glyph_count)
    } else {
        None
    }
}

/// Matches `SkCanvas::restoreToCount` loop iteration count.
#[inline]
pub fn restore_to_count_steps(save_count: i32, target_count: i32) -> i32 {
    let clamped = target_count.max(1);
    (save_count - clamped).max(0)
}

/// Matches `SkCanvas::writePixels` preflight rect intersection and overwrite check.
pub fn write_pixels_preflight(
    src_w: i32,
    src_h: i32,
    x: i32,
    y: i32,
    dev_w: i32,
    dev_h: i32,
) -> WritePixelsPreflight {
    let src_rect = irect_from_xywh(x, y, src_w, src_h);
    let dev_rect = IRect::from_ltrb(0, 0, dev_w, dev_h);
    let mut inter = IRect::default();
    if !inter.intersect_pair(src_rect, dev_rect) {
        return WritePixelsPreflight {
            intersects: false,
            complete_overwrite: false,
        };
    }
    let complete_overwrite = inter.width() == dev_w && inter.height() == dev_h;
    WritePixelsPreflight {
        intersects: true,
        complete_overwrite,
    }
}

/// Matches `SkCanvasPriv::ReadLattice` flagCount validation.
#[inline]
pub fn lattice_validate_flag_count(x_count: i32, y_count: i32, flag_count: i32) -> bool {
    if flag_count == 0 {
        return true;
    }
    if flag_count < 0 || x_count < 0 || y_count < 0 {
        return false;
    }
    let expected = ((x_count as u64) + 1) * ((y_count as u64) + 1);
    (flag_count as u64) == expected
}

/// Matches `SkCanvasPriv::WriteLattice` serialized byte size calculation.
#[inline]
pub fn lattice_serialized_size(x_count: i32, y_count: i32, has_rect_types: bool) -> (i32, usize) {
    let flag_count = if has_rect_types {
        (x_count + 1) * (y_count + 1)
    } else {
        0
    };
    let header_and_divs = (1usize + (x_count as usize) + 1 + (y_count as usize) + 1) * 4;
    let rect_types_bytes = align4(flag_count as usize);
    let colors_bytes = align4((flag_count as usize) * 4);
    let bounds_bytes = 16usize; // sizeof(SkIRect)
    (
        flag_count,
        header_and_divs + rect_types_bytes + colors_bytes + bounds_bytes,
    )
}

/// Matches `SkCanvasPriv::GetDstClipAndMatrixCounts`.
pub fn get_dst_clip_and_matrix_counts(
    has_clips: &[u8],
    matrix_indices: &[i32],
) -> DstClipMatrixCounts {
    let mut dst_clip_count: i32 = 0;
    let mut max_matrix_index: i32 = -1;
    let n = has_clips.len().min(matrix_indices.len());
    for i in 0..n {
        if has_clips[i] != 0 {
            dst_clip_count += 4;
        }
        if matrix_indices[i] > max_matrix_index {
            max_matrix_index = matrix_indices[i];
        }
    }
    DstClipMatrixCounts {
        total_dst_clip_count: dst_clip_count,
        total_matrix_count: max_matrix_index + 1,
    }
}

/// Matches `SkOverdrawCanvas` constructor 4x5 color matrix (`kIncrementAlpha`).
pub const fn overdraw_alpha_increment_matrix() -> [f32; 20] {
    [
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 1.0 / 255.0,
    ]
}

/// Matches `SkOverdrawCanvas::onDrawPaint` clear-detection check.
#[inline]
pub fn overdraw_is_clear_paint(color: u32, has_color_filter: bool, has_shader: bool) -> bool {
    color == 0 && !has_color_filter && !has_shader
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_max_layer_dim() {
        assert_eq!(compute_max_layer_dim(IRect::from_ltrb(0, 0, 100, 100)), 2048);
        assert_eq!(compute_max_layer_dim(IRect::from_ltrb(0, 0, 2000, 500)), 4000);
        assert_eq!(
            compute_max_layer_dim(IRect::from_ltrb(i32::MIN, 0, i32::MAX, 10)),
            i32::MAX
        );
    }

    #[test]
    fn test_image_filter_color_type() {
        assert_eq!(image_filter_color_type(2, 2, 6), 6); // RGB_565 -> N32
        assert_eq!(image_filter_color_type(4, 4, 6), 4); // RGBA_8888 stays
        assert_eq!(image_filter_color_type(6, 4, 6), 6); // BGRA_8888 stays
        assert_eq!(image_filter_color_type(10, 8, 6), 10); // F16 (8 bpp) stays
    }

    #[test]
    fn test_padded_layer_bounds() {
        let ok = compute_padded_layer_bounds(IRect::from_ltrb(10, 20, 30, 40), 1);
        assert!(ok.padded);
        assert_eq!(ok.bounds, IRect::from_ltrb(9, 19, 31, 41));

        let sat = compute_padded_layer_bounds(IRect::from_ltrb(i32::MIN, 0, 10, 10), 1);
        assert!(!sat.padded);
        assert_eq!(sat.bounds, IRect::from_ltrb(i32::MIN, 0, 10, 10));
    }

    #[test]
    fn test_quick_reject_and_local_clip() {
        let qr = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        assert!(!quick_reject_rect(&M44::identity(), Rect::from_ltrb(10.0, 10.0, 20.0, 20.0), qr));
        assert!(quick_reject_rect(&M44::identity(), Rect::from_ltrb(110.0, 10.0, 120.0, 20.0), qr));
        assert!(quick_reject_rect(&M44::identity(), Rect::from_ltrb(f32::NAN, 0.0, 10.0, 10.0), qr));

        let local = compute_local_clip_bounds(IRect::from_ltrb(10, 20, 50, 60), &M44::identity());
        assert_eq!(local, Rect::from_ltrb(9.0, 19.0, 51.0, 61.0));
    }

    #[test]
    fn test_lattice_and_image_set_counts() {
        assert!(lattice_validate_flag_count(2, 3, 12));
        assert!(!lattice_validate_flag_count(2, 3, 11));
        assert!(lattice_validate_flag_count(2, 3, 0));

        let (fc, sz) = lattice_serialized_size(2, 2, true);
        assert_eq!(fc, 9);
        // (1+2+1+2+1)*4 = 28, align4(9) = 12, align4(36) = 36, 16 => 92
        assert_eq!(sz, 92);

        let counts = get_dst_clip_and_matrix_counts(&[1, 0, 1], &[-1, 2, 0]);
        assert_eq!(counts.total_dst_clip_count, 8);
        assert_eq!(counts.total_matrix_count, 3);
    }

    #[test]
    fn test_write_pixels_preflight_and_drrect() {
        let wp = write_pixels_preflight(100, 100, 0, 0, 100, 100);
        assert!(wp.intersects && wp.complete_overwrite);

        let wp2 = write_pixels_preflight(50, 50, 10, 10, 100, 100);
        assert!(wp2.intersects && !wp2.complete_overwrite);

        let wp3 = write_pixels_preflight(10, 10, 200, 200, 100, 100);
        assert!(!wp3.intersects);

        assert_eq!(
            draw_drrect_action(
                false,
                false,
                Rect::from_ltrb(0.0, 0.0, 10.0, 10.0),
                Rect::from_ltrb(2.0, 2.0, 8.0, 8.0)
            ),
            DrawDRRectAction::DrawDRRect
        );
        assert_eq!(
            draw_drrect_action(
                false,
                false,
                Rect::from_ltrb(0.0, 0.0, 10.0, 10.0),
                Rect::from_ltrb(2.0, 2.0, 12.0, 8.0)
            ),
            DrawDRRectAction::Skip
        );
    }
}
