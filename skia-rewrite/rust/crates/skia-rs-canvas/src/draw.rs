//! Safe Rust drawing pipeline algorithms matching:
//! - `src/core/SkDraw.cpp`
//! - `src/core/SkDraw_atlas.cpp`
//! - `src/core/SkDraw_text.cpp`
//! - `src/core/SkDraw_vertices.cpp`

use skia_rs_core_types::align4;
use skia_rs_geometry::{
    float_midpoint, scalar_nearly_zero, scalar_round_to_int, IRect, Matrix, Point, Point3, RSXform,
    Rect, Vector, SK_SCALAR_NEARLY_ZERO,
};

use crate::{
    ieee_float_divide, matrix_map_rect, rect_fits_in_fixed, rect_from_irect, rect_from_wh,
    rect_too_big_for_math, scalar_to_fixed, BLEND_MODE_DST, BLEND_MODE_SRC, PAINT_CAP_ROUND,
    PAINT_JOIN_MITER, PAINT_STYLE_FILL, PAINT_STYLE_STROKE_AND_FILL, POINT_MODE_LINES,
    POINT_MODE_POINTS, POINT_MODE_POLYGON, SK_SCALAR_SQRT2,
};

/// Matches `skcpu::Draw::RectType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DrawRectType {
    Hair = 0,
    Fill = 1,
    Stroke = 2,
    Path = 3,
}

/// Action plan for `skcpu::Draw::drawRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DrawRectPlan {
    /// `0` = skip (clipped / too big), `1` = fallback to `draw_rect_as_path`, `2` = scan directly.
    pub action: i32,
    pub rect_type: i32,
    pub dev_rect: Rect,
    pub stroke_size: Point,
    pub bbox_ir: IRect,
}

/// Selected point-drawing proc matching `PtProcRec::chooseProc`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PtProcKind {
    None = 0,
    AaSquare = 1,
    AaLineHair = 2,
    AaPolyHair = 3,
    BwPtHair = 4,
    BwLineHair = 5,
    BwPolyHair = 6,
    BwSquare = 7,
}

/// Result of `PtProcRec::init`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PtProcInitResult {
    pub valid: bool,
    pub radius: f32,
    pub clip_bounds: Rect,
}

/// Fixed-point rect matching `SkXRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct XRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Result of `modifyPaintForHairlines`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HairlinePaintPlan {
    pub modify: bool,
    pub set_alpha: bool,
    pub new_alpha: u8,
}

/// Result of `drawBitmapAsMask` transformed mask geometry.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TransformedMaskGeometry {
    pub valid: bool,
    pub bounds: IRect,
    pub row_bytes: u32,
}

/// Result of `drawAtlas` per-quad transform matrix.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AtlasQuadMatrix {
    pub invertible: bool,
    pub mx: Matrix,
    pub inv: Matrix,
}

/// Result of `load_color` in `SkDraw_atlas.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct UniformColorRgba {
    pub rgba_u16: [u16; 4],
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Result of `clipTriangle` in `SkDraw_vertices.cpp` (0, 3, or 4 projected 2D vertices).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClippedTriangle2D {
    pub count: i32,
    pub pts: [Point; 4],
}

/// Result of vertex blend-mode simplification in `Draw::drawFixedVertices`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VerticesBlendPlan {
    pub drop_colors: bool,
    pub blender_is_dst: bool,
    pub drop_shader_and_tex: bool,
}

/// Matches `fast_len` in `SkDraw.cpp`.
#[inline]
pub fn fast_len(vec: Vector) -> f32 {
    let mut x = vec.x.abs();
    let mut y = vec.y.abs();
    if x < y {
        core::mem::swap(&mut x, &mut y);
    }
    x + (y / 2.0_f32)
}

/// Matches `skcpu::DrawTreatAAStrokeAsHairline`.
pub fn draw_treat_aa_stroke_as_hairline(
    stroke_width: f32,
    matrix: &Matrix,
) -> Option<f32> {
    if !(stroke_width > 0.0) || matrix.has_perspective() {
        return None;
    }
    let src = [
        Vector { x: stroke_width, y: 0.0 },
        Vector { x: 0.0, y: stroke_width },
    ];
    let mut dst = [Vector::default(); 2];
    matrix.map_vectors(&mut dst, &src);
    let len0 = fast_len(dst[0]);
    let len1 = fast_len(dst[1]);
    if len0 <= 1.0_f32 && len1 <= 1.0_f32 {
        Some(float_midpoint(len0, len1))
    } else {
        None
    }
}

/// Matches `skcpu::DrawTreatAsHairline(paint, matrix, coverage)`.
pub fn draw_treat_as_hairline(
    style: i32,
    stroke_width: f32,
    is_aa: bool,
    matrix: &Matrix,
) -> Option<f32> {
    if style != crate::PAINT_STYLE_STROKE {
        return None;
    }
    if stroke_width == 0.0 {
        return Some(1.0);
    }
    if !is_aa {
        return None;
    }
    draw_treat_aa_stroke_as_hairline(stroke_width, matrix)
}

/// Matches `modifyPaintForHairlines` in `SkDraw.cpp`.
pub fn hairline_paint_plan(
    style: i32,
    stroke_width: f32,
    is_aa: bool,
    alpha: u8,
    supports_coverage_as_alpha: bool,
    matrix: &Matrix,
) -> HairlinePaintPlan {
    if let Some(coverage) = draw_treat_as_hairline(style, stroke_width, is_aa, matrix) {
        if coverage == 1.0 {
            return HairlinePaintPlan {
                modify: true,
                set_alpha: false,
                new_alpha: alpha,
            };
        } else if supports_coverage_as_alpha {
            // Exact legacy Skia formula:
            // int scale = (int)(coverage * 256);
            // newAlpha = origPaint.getAlpha() * scale >> 8;
            let scale = (coverage * 256.0_f32) as i32;
            let new_alpha = (((alpha as i32) * scale) >> 8) as u8;
            return HairlinePaintPlan {
                modify: true,
                set_alpha: true,
                new_alpha,
            };
        }
    }
    HairlinePaintPlan {
        modify: false,
        set_alpha: false,
        new_alpha: alpha,
    }
}

/// Matches `compute_stroke_size` in `SkDraw.cpp`.
#[inline]
pub fn compute_stroke_size(stroke_width: f32, matrix: &Matrix) -> Point {
    let src = [Point {
        x: stroke_width,
        y: stroke_width,
    }];
    let mut dst = [Point::default(); 1];
    matrix.map_vectors(&mut dst, &src);
    Point {
        x: dst[0].x.abs(),
        y: dst[0].y.abs(),
    }
}

/// Matches `easy_rect_join` in `SkDraw.cpp`.
#[inline]
pub fn easy_rect_join(
    rect: Rect,
    stroke_join: i32,
    stroke_miter: f32,
    stroke_width: f32,
    matrix: &Matrix,
) -> Option<Point> {
    if rect.is_empty() || stroke_join != PAINT_JOIN_MITER || stroke_miter < SK_SCALAR_SQRT2 {
        return None;
    }
    Some(compute_stroke_size(stroke_width, matrix))
}

/// Matches `skcpu::Draw::ComputeRectType`.
pub fn compute_rect_type(
    rect: Rect,
    mut style: i32,
    stroke_width: f32,
    stroke_join: i32,
    stroke_miter: f32,
    has_path_effect: bool,
    has_mask_filter: bool,
    matrix: &Matrix,
) -> (DrawRectType, Point) {
    let zero_width = stroke_width == 0.0;
    if style == PAINT_STYLE_STROKE_AND_FILL && zero_width {
        style = PAINT_STYLE_FILL;
    }

    if has_path_effect
        || has_mask_filter
        || !matrix.rect_stays_rect()
        || style == PAINT_STYLE_STROKE_AND_FILL
    {
        return (DrawRectType::Path, Point::default());
    }
    if style == PAINT_STYLE_FILL {
        return (DrawRectType::Fill, Point::default());
    }
    if zero_width {
        return (DrawRectType::Hair, Point::default());
    }
    if let Some(stroke_size) = easy_rect_join(rect, stroke_join, stroke_miter, stroke_width, matrix)
    {
        return (DrawRectType::Stroke, stroke_size);
    }
    (DrawRectType::Path, Point::default())
}

/// Full geometric planning for `skcpu::Draw::drawRect`.
#[allow(clippy::too_many_arguments)]
pub fn compute_rect_draw_plan(
    pre_paint_rect: Rect,
    paint_rect: Rect,
    style: i32,
    stroke_width: f32,
    stroke_join: i32,
    stroke_miter: f32,
    has_path_effect: bool,
    has_mask_filter: bool,
    ctm: &Matrix,
) -> DrawRectPlan {
    let (rtype, stroke_size) = compute_rect_type(
        pre_paint_rect,
        style,
        stroke_width,
        stroke_join,
        stroke_miter,
        has_path_effect,
        has_mask_filter,
        ctm,
    );

    if rtype == DrawRectType::Path {
        return DrawRectPlan {
            action: 1,
            rect_type: rtype as i32,
            dev_rect: Rect::default(),
            stroke_size,
            bbox_ir: IRect::default(),
        };
    }

    // Map the 2 corners of paint_rect as 2 points and sort (exact SkDraw.cpp behavior).
    let src_pts = [
        Point { x: paint_rect.left, y: paint_rect.top },
        Point { x: paint_rect.right, y: paint_rect.bottom },
    ];
    let mut dst_pts = [Point::default(); 2];
    ctm.map_points(&mut dst_pts, &src_pts);
    let dev_rect = Rect {
        left: dst_pts[0].x,
        top: dst_pts[0].y,
        right: dst_pts[1].x,
        bottom: dst_pts[1].y,
    }
    .make_sorted();

    let mut bbox = dev_rect;
    if style != PAINT_STYLE_FILL {
        if stroke_width == 0.0 {
            bbox = bbox.make_inset(-1.0, -1.0);
        } else {
            let ssize = if rtype == DrawRectType::Stroke {
                stroke_size
            } else {
                compute_stroke_size(stroke_width, ctm)
            };
            bbox = bbox.make_inset(-(ssize.x / 2.0_f32), -(ssize.y / 2.0_f32));
        }
    }

    if rect_too_big_for_math(bbox) {
        return DrawRectPlan {
            action: 0,
            rect_type: rtype as i32,
            dev_rect,
            stroke_size,
            bbox_ir: IRect::default(),
        };
    }

    if !rect_fits_in_fixed(bbox) && rtype != DrawRectType::Hair {
        return DrawRectPlan {
            action: 1,
            rect_type: rtype as i32,
            dev_rect,
            stroke_size,
            bbox_ir: IRect::default(),
        };
    }

    let bbox_ir = bbox.round_out();
    DrawRectPlan {
        action: 2,
        rect_type: rtype as i32,
        dev_rect,
        stroke_size,
        bbox_ir,
    }
}

/// Matches `PtProcRec::init` in `SkDraw.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn pt_proc_init(
    mode: i32,
    has_path_effect: bool,
    has_mask_filter: bool,
    stroke_width: f32,
    stroke_cap: i32,
    matrix: &Matrix,
    clip_bounds_i: IRect,
) -> PtProcInitResult {
    if (mode as u32) > (POINT_MODE_POLYGON as u32) {
        return PtProcInitResult {
            valid: false,
            radius: -1.0,
            clip_bounds: Rect::default(),
        };
    }
    if has_path_effect || has_mask_filter {
        return PtProcInitResult {
            valid: false,
            radius: -1.0,
            clip_bounds: Rect::default(),
        };
    }
    let mut radius = -1.0_f32;
    if stroke_width == 0.0 {
        radius = 0.5_f32;
    } else if stroke_cap != PAINT_CAP_ROUND
        && matrix.is_scale_translate()
        && mode == POINT_MODE_POINTS
    {
        let sx = matrix.mat[0];
        let sy = matrix.mat[4];
        if scalar_nearly_zero(sx - sy, SK_SCALAR_NEARLY_ZERO) {
            radius = (stroke_width * sx.abs()) / 2.0_f32;
        }
    }
    if radius > 0.0 {
        let clip_bounds = rect_from_irect(clip_bounds_i);
        if !rect_fits_in_fixed(clip_bounds) {
            return PtProcInitResult {
                valid: false,
                radius: -1.0,
                clip_bounds: Rect::default(),
            };
        }
        return PtProcInitResult {
            valid: true,
            radius,
            clip_bounds,
        };
    }
    PtProcInitResult {
        valid: false,
        radius: -1.0,
        clip_bounds: Rect::default(),
    }
}

/// Matches `PtProcRec::chooseProc` decision logic in `SkDraw.cpp`.
pub fn pt_proc_choose(
    mode: i32,
    is_aa: bool,
    stroke_width: f32,
    stroke_cap: i32,
    radius: f32,
) -> PtProcKind {
    if is_aa {
        if stroke_width == 0.0 {
            match mode {
                POINT_MODE_POINTS => PtProcKind::AaSquare,
                POINT_MODE_LINES => PtProcKind::AaLineHair,
                POINT_MODE_POLYGON => PtProcKind::AaPolyHair,
                _ => PtProcKind::None,
            }
        } else if stroke_cap != PAINT_CAP_ROUND {
            PtProcKind::AaSquare
        } else {
            PtProcKind::None
        }
    } else if radius <= 0.5_f32 {
        match mode {
            POINT_MODE_POINTS => PtProcKind::BwPtHair,
            POINT_MODE_LINES => PtProcKind::BwLineHair,
            POINT_MODE_POLYGON => PtProcKind::BwPolyHair,
            _ => PtProcKind::None,
        }
    } else {
        PtProcKind::BwSquare
    }
}

/// Matches `make_square_rad` in `SkDraw.cpp`.
#[inline]
pub fn make_square_rad(center: Point, radius: f32) -> Rect {
    Rect {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius,
        bottom: center.y + radius,
    }
}

/// Matches `make_xrect` in `SkDraw.cpp`.
#[inline]
pub fn make_xrect(r: Rect) -> XRect {
    XRect {
        left: scalar_to_fixed(r.left),
        top: scalar_to_fixed(r.top),
        right: scalar_to_fixed(r.right),
        bottom: scalar_to_fixed(r.bottom),
    }
}

/// Matches `Draw::computeConservativeLocalClipBounds`.
pub fn compute_conservative_local_clip_bounds(
    ctm: &Matrix,
    rc_bounds: IRect,
    rc_empty: bool,
) -> Option<Rect> {
    if rc_empty {
        return None;
    }
    let inverse = ctm.invert()?;
    let dev_bounds = IRect {
        left: rc_bounds.left.saturating_sub(1),
        top: rc_bounds.top.saturating_sub(1),
        right: rc_bounds.right.saturating_add(1),
        bottom: rc_bounds.bottom.saturating_add(1),
    };
    Some(matrix_map_rect(&inverse, rect_from_irect(dev_bounds)))
}

/// Matches `compute_mask_bounds` initial outset (`makeOutset(0.5, 0.5).roundOut()`).
#[inline]
pub fn compute_mask_bounds_init(dev_path_bounds: Rect) -> IRect {
    dev_path_bounds.make_inset(-0.5_f32, -0.5_f32).round_out()
}

/// Matches `compute_mask_bounds` final trim with `kMaxMargin = 128`.
pub fn trim_mask_bounds(
    mut bounds: IRect,
    clip_bounds: IRect,
    margin_x: i32,
    margin_y: i32,
) -> Option<IRect> {
    const MAX_MARGIN: i32 = 128;
    let mx = margin_x.min(MAX_MARGIN);
    let my = margin_y.min(MAX_MARGIN);
    let outset_clip = IRect {
        left: clip_bounds.left.saturating_sub(mx),
        top: clip_bounds.top.saturating_sub(my),
        right: clip_bounds.right.saturating_add(mx),
        bottom: clip_bounds.bottom.saturating_add(my),
    };
    let mut out = IRect::default();
    if out.intersect_pair(bounds, outset_clip) {
        bounds = out;
        Some(bounds)
    } else {
        None
    }
}

/// Matches `Draw::drawBitmapAsMask` transformed mask bounds and rowBytes calculation.
pub fn compute_transformed_bitmap_mask(
    bm_width: i32,
    bm_height: i32,
    ctm: &Matrix,
    dst_bounds: IRect,
    rc_bounds: IRect,
) -> TransformedMaskGeometry {
    let r = matrix_map_rect(ctm, rect_from_wh(bm_width as f32, bm_height as f32));
    let mask_r = r.round();

    let rc_outset = IRect {
        left: rc_bounds.left.saturating_sub(1),
        top: rc_bounds.top.saturating_sub(1),
        right: rc_bounds.right.saturating_add(1),
        bottom: rc_bounds.bottom.saturating_add(1),
    };
    let mut dev_bounds = IRect::default();
    if !dev_bounds.intersect_pair(dst_bounds, rc_outset) {
        return TransformedMaskGeometry {
            valid: false,
            bounds: IRect::default(),
            row_bytes: 0,
        };
    }

    let mut final_bounds = IRect::default();
    if !final_bounds.intersect_pair(mask_r, dev_bounds) {
        return TransformedMaskGeometry {
            valid: false,
            bounds: IRect::default(),
            row_bytes: 0,
        };
    }

    let row_bytes = align4(final_bounds.width() as usize) as u32;
    TransformedMaskGeometry {
        valid: true,
        bounds: final_bounds,
        row_bytes,
    }
}

/// Matches `clipped_out(matrix, clip, width, height)` in `SkDraw.cpp` — returns `dstR.roundOut()`.
#[inline]
pub fn mapped_bitmap_round_out(ctm: &Matrix, prematrix: &Matrix, width: i32, height: i32) -> (Matrix, IRect) {
    let mut combined = Matrix::identity();
    combined.set_concat(ctm, prematrix);
    let src_r = rect_from_wh(width as f32, height as f32);
    let dst_r = matrix_map_rect(&combined, src_r);
    (combined, dst_r.round_out())
}

/// Matches `Draw::drawAtlas` per-quad matrix setup & inversion.
pub fn draw_atlas_quad_matrix(xform: RSXform, tex: Rect, ctm: &Matrix) -> AtlasQuadMatrix {
    let mut mx = Matrix::identity();
    mx.set_rsxform(xform);
    mx.pre_translate(-tex.left, -tex.top);
    mx.post_concat(ctm);
    if let Some(inv) = mx.invert() {
        AtlasQuadMatrix {
            invertible: true,
            mx,
            inv,
        }
    } else {
        AtlasQuadMatrix {
            invertible: false,
            mx,
            inv: Matrix::identity(),
        }
    }
}

/// Matches `load_color` in `SkDraw_atlas.cpp`.
#[inline]
pub fn load_uniform_color_rgba(rgba: [f32; 4]) -> UniformColorRgba {
    UniformColorRgba {
        rgba_u16: [
            scalar_round_to_int(rgba[0] * 255.0_f32) as u16,
            scalar_round_to_int(rgba[1] * 255.0_f32) as u16,
            scalar_round_to_int(rgba[2] * 255.0_f32) as u16,
            scalar_round_to_int(rgba[3] * 255.0_f32) as u16,
        ],
        r: rgba[0],
        g: rgba[1],
        b: rgba[2],
        a: rgba[3],
    }
}

/// Matches `check_glyph_position` in `SkDraw_text.cpp` (NaN-safe!).
#[inline]
pub fn check_glyph_position(pos: Point) -> bool {
    const MAX_COORD: i32 = i32::MAX - ((i16::MAX as i32) + (u16::MAX as i32));
    const MIN_COORD: i32 = i32::MIN - (i16::MIN as i32);
    let gt = |a: f32, b: i32| !(a <= b as f32);
    let lt = |a: f32, b: i32| !(a >= b as f32);
    !(gt(pos.x, MAX_COORD) || lt(pos.x, MIN_COORD) || gt(pos.y, MAX_COORD) || lt(pos.y, MIN_COORD))
}

/// Matches `compute_is_opaque` in `SkDraw_vertices.cpp`.
#[inline]
pub fn compute_is_opaque_colors(colors: &[u32]) -> bool {
    let mut c: u32 = !0u32;
    for &col in colors {
        c &= col;
    }
    ((c >> 24) & 0xFF) == 0xFF
}

/// Matches `clipTriangle` in `fill_triangle_3` (`SkDraw_vertices.cpp`).
pub fn clip_triangle_3d(p0: Point3, p1: Point3, p2: Point3) -> ClippedTriangle2D {
    const TOL: f32 = 0.05_f32;

    let clip_edge = |curr: Point3, next: Point3| -> Point3 {
        let a = curr.z - TOL;
        let b = next.z - TOL;
        let t = a / (a - b);
        Point3 {
            x: curr.x + t * (next.x - curr.x),
            y: curr.y + t * (next.y - curr.y),
            z: curr.z + t * (next.z - curr.z),
        }
    };

    let pts = [p0, p1, p2];
    let mut out3 = [Point3::default(); 4];
    let mut count = 0usize;

    for i in 0..3 {
        let curr = pts[i];
        let next = pts[(i + 1) % 3];
        if curr.z > TOL {
            out3[count] = curr;
            count += 1;
            if next.z <= TOL {
                out3[count] = clip_edge(curr, next);
                count += 1;
            }
        } else if next.z > TOL {
            out3[count] = clip_edge(curr, next);
            count += 1;
        }
    }

    let mut out2 = [Point::default(); 4];
    for i in 0..count {
        let scale = ieee_float_divide(1.0_f32, out3[i].z);
        out2[i] = Point {
            x: out3[i].x * scale,
            y: out3[i].y * scale,
        };
    }
    ClippedTriangle2D {
        count: count as i32,
        pts: out2,
    }
}

/// Matches `texture_to_matrix` in `SkDraw_vertices.cpp`.
pub fn texture_to_matrix_3pts(src: &[Point; 3], dst: &[Point; 3]) -> Option<Matrix> {
    Matrix::poly_to_poly(src, dst)
}

/// Matches blend-mode simplification in `Draw::drawFixedVertices`.
#[inline]
pub fn vertices_blend_simplify(has_blend_mode: bool, blend_mode: i32, has_colors: bool) -> VerticesBlendPlan {
    if has_blend_mode && has_colors {
        if blend_mode == BLEND_MODE_SRC {
            return VerticesBlendPlan {
                drop_colors: true,
                blender_is_dst: false,
                drop_shader_and_tex: false,
            };
        } else if blend_mode == BLEND_MODE_DST {
            return VerticesBlendPlan {
                drop_colors: false,
                blender_is_dst: true,
                drop_shader_and_tex: true,
            };
        }
    }
    VerticesBlendPlan {
        drop_colors: false,
        blender_is_dst: false,
        drop_shader_and_tex: false,
    }
}

/// Matches early-out check in `Draw::drawVertices`.
#[inline]
pub fn vertices_early_out(vertex_count: usize, index_count: usize, rc_empty: bool) -> bool {
    vertex_count < 3 || (index_count > 0 && index_count < 3) || rc_empty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PAINT_CAP_BUTT, PAINT_JOIN_BEVEL, PAINT_STYLE_STROKE};

    #[test]
    fn test_fast_len_and_hairline() {
        assert_eq!(fast_len(Vector { x: 3.0, y: 4.0 }), 5.5);
        assert_eq!(fast_len(Vector { x: -4.0, y: -2.0 }), 5.0);

        let id = Matrix::identity();
        let cov = draw_treat_aa_stroke_as_hairline(0.5, &id).unwrap();
        assert_eq!(cov, 0.5);
        assert!(draw_treat_aa_stroke_as_hairline(2.0, &id).is_none());

        let plan = hairline_paint_plan(PAINT_STYLE_STROKE, 0.5, true, 200, true, &id);
        assert!(plan.modify);
        assert!(plan.set_alpha);
        assert_eq!(plan.new_alpha, 100);
    }

    #[test]
    fn test_compute_rect_type() {
        let r = Rect::from_ltrb(0.0, 0.0, 10.0, 10.0);
        let id = Matrix::identity();
        let (rt, _) = compute_rect_type(r, PAINT_STYLE_FILL, 0.0, PAINT_JOIN_MITER, 4.0, false, false, &id);
        assert_eq!(rt, DrawRectType::Fill);

        let (rt, _) = compute_rect_type(r, PAINT_STYLE_STROKE, 0.0, PAINT_JOIN_MITER, 4.0, false, false, &id);
        assert_eq!(rt, DrawRectType::Hair);

        let (rt, sz) = compute_rect_type(r, PAINT_STYLE_STROKE, 4.0, PAINT_JOIN_MITER, 4.0, false, false, &id);
        assert_eq!(rt, DrawRectType::Stroke);
        assert_eq!(sz, Point { x: 4.0, y: 4.0 });

        let (rt, _) = compute_rect_type(r, PAINT_STYLE_STROKE, 4.0, PAINT_JOIN_BEVEL, 4.0, false, false, &id);
        assert_eq!(rt, DrawRectType::Path);
    }

    #[test]
    fn test_pt_proc() {
        let id = Matrix::identity();
        let clip = IRect::from_ltrb(0, 0, 100, 100);
        let init = pt_proc_init(POINT_MODE_POINTS, false, false, 4.0, PAINT_CAP_BUTT, &id, clip);
        assert!(init.valid);
        assert_eq!(init.radius, 2.0);
        assert_eq!(
            pt_proc_choose(POINT_MODE_POINTS, true, 4.0, PAINT_CAP_BUTT, 2.0),
            PtProcKind::AaSquare
        );
        assert_eq!(
            pt_proc_choose(POINT_MODE_POINTS, false, 4.0, PAINT_CAP_BUTT, 2.0),
            PtProcKind::BwSquare
        );
        assert_eq!(
            pt_proc_choose(POINT_MODE_POINTS, false, 0.0, PAINT_CAP_BUTT, 0.5),
            PtProcKind::BwPtHair
        );
    }

    #[test]
    fn test_glyph_position_nan_safe() {
        assert!(check_glyph_position(Point { x: 100.0, y: -50.0 }));
        assert!(!check_glyph_position(Point { x: f32::NAN, y: 0.0 }));
        assert!(!check_glyph_position(Point { x: f32::INFINITY, y: 0.0 }));
        assert!(!check_glyph_position(Point { x: 0.0, y: f32::NEG_INFINITY }));
    }

    #[test]
    fn test_clip_triangle_3d() {
        let p0 = Point3 { x: 0.0, y: 0.0, z: 1.0 };
        let p1 = Point3 { x: 10.0, y: 0.0, z: 1.0 };
        let p2 = Point3 { x: 0.0, y: 10.0, z: 1.0 };
        let res = clip_triangle_3d(p0, p1, p2);
        assert_eq!(res.count, 3);
        assert_eq!(res.pts[1], Point { x: 10.0, y: 0.0 });

        // One vertex behind near plane => quad (4 vertices)
        let q = clip_triangle_3d(
            Point3 { x: 0.0, y: 0.0, z: -1.0 },
            Point3 { x: 10.0, y: 0.0, z: 1.0 },
            Point3 { x: 0.0, y: 10.0, z: 1.0 },
        );
        assert_eq!(q.count, 4);

        // All behind => 0 vertices
        let e = clip_triangle_3d(
            Point3 { x: 0.0, y: 0.0, z: -1.0 },
            Point3 { x: 1.0, y: 0.0, z: 0.0 },
            Point3 { x: 0.0, y: 1.0, z: 0.05 },
        );
        assert_eq!(e.count, 0);
    }

    #[test]
    fn test_compute_is_opaque_colors() {
        assert!(compute_is_opaque_colors(&[0xFF112233, 0xFF445566]));
        assert!(!compute_is_opaque_colors(&[0xFF112233, 0xFE445566]));
        assert!(compute_is_opaque_colors(&[]));
    }
}
