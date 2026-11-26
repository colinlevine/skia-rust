//! Safe Rust port of `SkTextBlob.cpp`, `src/text/GlyphRun.cpp`, `SkGlyphRunPainter.cpp`,
//! and `src/utils/SkTextUtils.cpp` core algorithms.

use crate::glyph::pack_glyph_id_point;
use crate::scaler_context::{SCALER_CONTEXT_FLAG_BOOST_CONTRAST, SCALER_CONTEXT_FLAG_FAKE_GAMMA};
use crate::{GlyphPositioning, TextAlign};
use skia_rs_geometry::{cpp_max_f32, cpp_min_f32, IPoint, Matrix, Point, RSXform, Rect, Vector};

#[inline]
fn align_up_checked(x: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    let mask = alignment - 1;
    x.checked_add(mask).map(|v| v & !mask)
}

/// Mirrors `SkTextBlob::ScalarsPerGlyph`.
#[inline]
pub const fn scalars_per_glyph(pos: GlyphPositioning) -> u32 {
    match pos {
        GlyphPositioning::Default => 0,
        GlyphPositioning::Horizontal => 1,
        GlyphPositioning::Full => 2,
        GlyphPositioning::RSXform => 4,
    }
}

/// Mirrors `SkTextBlob::RunRecord::PosCount`.
pub fn run_record_pos_count(glyph_count: u32, pos: GlyphPositioning) -> Option<usize> {
    (glyph_count as usize).checked_mul(scalars_per_glyph(pos) as usize)
}

/// Mirrors `SkTextBlob::RunRecord::StorageSize`.
pub fn run_record_storage_size(
    run_record_size: usize,
    ptr_size: usize,
    glyph_count: u32,
    text_size: u32,
    pos: GlyphPositioning,
) -> Option<usize> {
    let glyph_size = (glyph_count as usize).checked_mul(2)?; // sizeof(SkGlyphID) == 2
    let pos_count = (glyph_count as usize).checked_mul(scalars_per_glyph(pos) as usize)?;
    let pos_size = pos_count.checked_mul(4)?; // sizeof(SkScalar) == 4

    let aligned_glyph = align_up_checked(glyph_size, 4)?;
    let mut size = run_record_size.checked_add(aligned_glyph)?;
    size = size.checked_add(pos_size)?;

    if text_size != 0 {
        size = size.checked_add(4)?; // sizeof(uint32_t)
        let cluster_size = (glyph_count as usize).checked_mul(4)?;
        size = size.checked_add(cluster_size)?;
        size = size.checked_add(text_size as usize)?;
    }

    align_up_checked(size, ptr_size)
}

/// Mirrors `SkTextBlobBuilder::TightRunBounds` for horizontal / full / RSXform runs
/// given pre-fetched per-glyph `glyph_bounds`.
pub fn tight_run_bounds(
    pos: GlyphPositioning,
    glyph_bounds: &[Rect],
    scalars: &[f32],
    offset: Point,
) -> Rect {
    let count = glyph_bounds.len();
    let mut bounds = Rect::make_empty();
    match pos {
        GlyphPositioning::RSXform => {
            for i in 0..count {
                let base = i * 4;
                let xf = RSXform::new(
                    scalars[base],
                    scalars[base + 1],
                    scalars[base + 2],
                    scalars[base + 3],
                );
                let mut m = Matrix::identity();
                m.set_rsxform(xf);
                if let Some((mapped, _)) = m.map_rect_non_perspective(glyph_bounds[i]) {
                    bounds.join(mapped);
                }
            }
        }
        GlyphPositioning::Full => {
            for i in 0..count {
                let r = glyph_bounds[i].make_offset(scalars[i * 2], scalars[i * 2 + 1]);
                bounds.join(r);
            }
        }
        GlyphPositioning::Horizontal => {
            for i in 0..count {
                let r = glyph_bounds[i].make_offset(scalars[i], 0.0);
                bounds.join(r);
            }
        }
        GlyphPositioning::Default => {}
    }
    bounds.make_offset(offset.x, offset.y)
}

/// Mirrors `SkTextBlobBuilder::ConservativeRunBounds` (when `font_bounds` is non-empty).
pub fn conservative_run_bounds(
    pos: GlyphPositioning,
    glyph_count: usize,
    scalars: &[f32],
    font_bounds: Rect,
    offset: Point,
) -> Rect {
    let mut bounds = Rect::make_empty();
    match pos {
        GlyphPositioning::Horizontal => {
            let mut min_x = scalars[0];
            let mut max_x = scalars[0];
            for &x in scalars.iter().take(glyph_count).skip(1) {
                min_x = cpp_min_f32(x, min_x);
                max_x = cpp_max_f32(x, max_x);
            }
            bounds = Rect::make_ltrb(min_x, 0.0, max_x, 0.0);
        }
        GlyphPositioning::Full => {
            let mut pts = Vec::with_capacity(glyph_count);
            for i in 0..glyph_count {
                pts.push(Point::new(scalars[i * 2], scalars[i * 2 + 1]));
            }
            if !bounds.set_bounds_check(&pts) {
                bounds = Rect::make_empty();
            }
        }
        GlyphPositioning::RSXform => {
            for i in 0..glyph_count {
                let base = i * 4;
                let xf = RSXform::new(
                    scalars[base],
                    scalars[base + 1],
                    scalars[base + 2],
                    scalars[base + 3],
                );
                let mut m = Matrix::identity();
                m.set_rsxform(xf);
                if let Some((mapped, _)) = m.map_rect_non_perspective(font_bounds) {
                    bounds.join(mapped);
                }
            }
        }
        GlyphPositioning::Default => {}
    }

    if !matches!(pos, GlyphPositioning::RSXform) {
        bounds.left += font_bounds.left;
        bounds.top += font_bounds.top;
        bounds.right += font_bounds.right;
        bounds.bottom += font_bounds.bottom;
    }

    bounds.make_offset(offset.x, offset.y)
}

/// Mirrors `SkTextBlobPriv::MakeFromBuffer` run payload byte-size validation.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RunDeserialSizes {
    pub valid: bool,
    pub glyph_size: usize,
    pub pos_size: usize,
    pub cluster_size: usize,
    pub total_size: usize,
}

pub fn validate_deserialized_run_sizes(
    glyph_count: i32,
    pos: GlyphPositioning,
    extended: bool,
    text_size: i32,
    available_bytes: usize,
) -> RunDeserialSizes {
    if glyph_count <= 0 || text_size < 0 {
        return RunDeserialSizes::default();
    }
    let gc = glyph_count as usize;
    let ts = text_size as usize;
    let Some(glyph_size) = gc.checked_mul(2) else {
        return RunDeserialSizes::default();
    };
    let Some(scalar_stride) = 4usize.checked_mul(scalars_per_glyph(pos) as usize) else {
        return RunDeserialSizes::default();
    };
    let Some(pos_size) = gc.checked_mul(scalar_stride) else {
        return RunDeserialSizes::default();
    };
    let cluster_size = if extended {
        let Some(cs) = gc.checked_mul(4) else {
            return RunDeserialSizes::default();
        };
        cs
    } else {
        0
    };
    let Some(gp) = glyph_size.checked_add(pos_size) else {
        return RunDeserialSizes::default();
    };
    let Some(ct) = cluster_size.checked_add(ts) else {
        return RunDeserialSizes::default();
    };
    let Some(total_size) = gp.checked_add(ct) else {
        return RunDeserialSizes::default();
    };
    if total_size > available_bytes {
        return RunDeserialSizes::default();
    }
    RunDeserialSizes {
        valid: true,
        glyph_size,
        pos_size,
        cluster_size,
        total_size,
    }
}

/// Validates cluster offsets in an extended run (`cluster < text_size`).
pub fn validate_clusters(clusters: &[u32], text_size: u32) -> bool {
    clusters.iter().all(|&c| c < text_size)
}

/// Mirrors `glyphrun_source_bounds` in `src/text/GlyphRun.cpp` (conservative path when `font_bounds` non-empty).
pub fn glyphrun_conservative_bounds(
    font_bounds: Rect,
    positions: &[Point],
    scaled_rotations: &[Vector],
) -> Rect {
    if scaled_rotations.is_empty() {
        let mut bounds = Rect::make_empty();
        if !bounds.set_bounds_check(positions) {
            bounds = Rect::make_empty();
        }
        bounds.left += font_bounds.left;
        bounds.top += font_bounds.top;
        bounds.right += font_bounds.right;
        bounds.bottom += font_bounds.bottom;
        bounds
    } else {
        let mut bounds = Rect::make_empty();
        for (&pos, &sr) in positions.iter().zip(scaled_rotations.iter()) {
            // Note: matches upstream GlyphRun.cpp line 180 exact constructor order:
            // const SkRSXform xform{pos.x(), pos.y(), scaleRotate.x(), scaleRotate.y()};
            let xf = RSXform::new(pos.x, pos.y, sr.x, sr.y);
            let mut m = Matrix::identity();
            m.set_rsxform(xf);
            if let Some((mapped, _)) = m.map_rect_non_perspective(font_bounds) {
                bounds.join(mapped);
            }
        }
        bounds
    }
}

/// Mirrors `glyphrun_source_bounds` tight path (when `font_bounds.isEmpty()`).
pub fn glyphrun_tight_bounds(
    glyph_rects: &[Rect],
    positions: &[Point],
    scaled_rotations: &[Vector],
    strike_to_source_scale: f32,
) -> Rect {
    let mut bounds = Rect::make_empty();
    if scaled_rotations.is_empty() {
        for (&r, &pos) in glyph_rects.iter().zip(positions.iter()) {
            if !r.is_empty() {
                let scaled = Rect::make_ltrb(
                    r.left * strike_to_source_scale + pos.x,
                    r.top * strike_to_source_scale + pos.y,
                    r.right * strike_to_source_scale + pos.x,
                    r.bottom * strike_to_source_scale + pos.y,
                );
                bounds.join(scaled);
            }
        }
    } else {
        for ((&r, &pos), &sr) in glyph_rects
            .iter()
            .zip(positions.iter())
            .zip(scaled_rotations.iter())
        {
            if !r.is_empty() {
                // Exact upstream GlyphRun.cpp line 158 field order:
                let xf = RSXform::new(pos.x, pos.y, sr.x, sr.y);
                let mut m = Matrix::identity();
                m.set_rsxform(xf);
                m.pre_scale(strike_to_source_scale, strike_to_source_scale);
                if let Some((mapped, _)) = m.map_rect_non_perspective(r) {
                    bounds.join(mapped);
                }
            }
        }
    }
    bounds
}

/// Mirrors `compute_scaler_context_flags` in `SkGlyphRunPainter.cpp`.
#[inline]
pub const fn compute_scaler_context_flags(gamma_is_linear: bool) -> u32 {
    if gamma_is_linear {
        SCALER_CONTEXT_FLAG_BOOST_CONTRAST
    } else {
        SCALER_CONTEXT_FLAG_FAKE_GAMMA | SCALER_CONTEXT_FLAG_BOOST_CONTRAST
    }
}

/// Result of quantizing a glyph position for direct mask drawing.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DirectMaskQuantized {
    pub finite: bool,
    pub packed_glyph_id: u32,
    pub rounded_pos: Point,
}

/// Mirrors the per-glyph mapping and rounding in `prepare_for_direct_mask_drawing`.
pub fn quantize_direct_mask_glyph(
    glyph_id: u16,
    pos: Point,
    position_matrix_with_rounding: &Matrix,
    ignore_position_field_mask: IPoint,
) -> DirectMaskQuantized {
    if !pos.x.is_finite() || !pos.y.is_finite() {
        return DirectMaskQuantized::default();
    }
    let mut pt_arr = [pos];
    position_matrix_with_rounding.map_points_in_place(&mut pt_arr);
    let mapped = pt_arr[0];
    let packed = pack_glyph_id_point(glyph_id, mapped, ignore_position_field_mask);
    DirectMaskQuantized {
        finite: true,
        packed_glyph_id: packed,
        rounded_pos: Point::new(mapped.x.floor(), mapped.y.floor()),
    }
}

/// Mirrors the max-scale calculation for perspective bitmap glyph fallback in
/// `GlyphRunListPainter::drawForBitmapDevice`.
pub fn compute_perspective_bitmap_max_scale(
    glyph_rects: &[Rect],
    positions: &[Point],
    draw_origin: Point,
    position_matrix: &Matrix,
    font_size: f32,
) -> f32 {
    let mut max_scale = -f32::MAX;
    for (&rect, &pos) in glyph_rects.iter().zip(positions.iter()) {
        if rect.is_empty() {
            continue;
        }
        // Note: matches upstream SkGlyphRunPainter.cpp line 355:
        // `rect.makeOffset(drawOrigin + pos);` (return value discarded in upstream!)
        let _ = (draw_origin, pos);
        let mut corners = rect.to_quad();
        position_matrix.map_points_in_place(&mut corners);

        let w = rect.width();
        let h = rect.height();
        let s0 = Point::new(corners[1].x - corners[0].x, corners[1].y - corners[0].y).mag() / w;
        max_scale = cpp_max_f32(max_scale, s0);
        let s1 = Point::new(corners[2].x - corners[1].x, corners[2].y - corners[1].y).mag() / h;
        max_scale = cpp_max_f32(max_scale, s1);
        let s2 = Point::new(corners[3].x - corners[2].x, corners[3].y - corners[2].y).mag() / w;
        max_scale = cpp_max_f32(max_scale, s2);
        let s3 = Point::new(corners[0].x - corners[3].x, corners[0].y - corners[3].y).mag() / h;
        max_scale = cpp_max_f32(max_scale, s3);
    }

    if max_scale <= 0.0 {
        return max_scale;
    }

    if max_scale * font_size > 256.0 {
        max_scale = 256.0 / font_size;
    }
    max_scale
}

/// Mirrors `get_glyph_run_intercepts` setup calculations in `SkTextBlob.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct InterceptSetup {
    pub scale: f32,
    pub stroke_width: f32,
}

pub fn compute_intercept_setup(
    has_path_effect: bool,
    font_size: f32,
    paint_style: u8,
    stroke_width: f32,
) -> InterceptSetup {
    if has_path_effect {
        return InterceptSetup {
            scale: 1.0,
            stroke_width,
        };
    }
    let scale = font_size / (crate::font::CANONICAL_TEXT_SIZE_FOR_PATHS as f32);
    let new_sw = if stroke_width > 0.0 && paint_style != 0 {
        stroke_width / scale
    } else {
        stroke_width
    };
    InterceptSetup {
        scale,
        stroke_width: new_sw,
    }
}

/// Mirrors `SkTextUtils::Draw` x-alignment adjustment.
#[inline]
pub fn align_text_x(x: f32, width: f32, align: TextAlign) -> f32 {
    match align {
        TextAlign::Left => x,
        TextAlign::Center => x - width * 0.5_f32,
        TextAlign::Right => x - width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_record_storage_size() {
        assert_eq!(scalars_per_glyph(GlyphPositioning::Default), 0);
        assert_eq!(scalars_per_glyph(GlyphPositioning::Horizontal), 1);
        assert_eq!(scalars_per_glyph(GlyphPositioning::Full), 2);
        assert_eq!(scalars_per_glyph(GlyphPositioning::RSXform), 4);

        let sz = run_record_storage_size(40, 8, 3, 0, GlyphPositioning::Full).unwrap();
        // 40 + align4(6)=8 + 3*2*4=24 = 72 (already 8-aligned)
        assert_eq!(sz, 72);

        // Overflow returns None
        assert!(run_record_storage_size(usize::MAX - 4, 8, 10, 0, GlyphPositioning::RSXform).is_none());
    }

    #[test]
    fn test_conservative_bounds_horizontal() {
        let fb = Rect::make_ltrb(-1.0, -10.0, 12.0, 3.0);
        let b = conservative_run_bounds(
            GlyphPositioning::Horizontal,
            3,
            &[10.0, 5.0, 25.0],
            fb,
            Point::new(0.0, 100.0),
        );
        assert_eq!(b, Rect::make_ltrb(4.0, 90.0, 37.0, 103.0));
    }

    #[test]
    fn test_align_text_x() {
        assert_eq!(align_text_x(50.0, 20.0, TextAlign::Left), 50.0);
        assert_eq!(align_text_x(50.0, 20.0, TextAlign::Center), 40.0);
        assert_eq!(align_text_x(50.0, 20.0, TextAlign::Right), 30.0);
    }

    #[test]
    fn test_validate_deserialized_run_sizes() {
        let ok = validate_deserialized_run_sizes(4, GlyphPositioning::Full, true, 10, 100);
        assert!(ok.valid);
        assert_eq!(ok.glyph_size, 8);
        assert_eq!(ok.pos_size, 32);
        assert_eq!(ok.cluster_size, 16);
        assert_eq!(ok.total_size, 66);

        let too_small = validate_deserialized_run_sizes(4, GlyphPositioning::Full, true, 10, 65);
        assert!(!too_small.valid);
    }
}
