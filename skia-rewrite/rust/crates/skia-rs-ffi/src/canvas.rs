//! FFI exports for `skia-rs-canvas` (Phase 1 Layer 5.3).

use skia_rs_canvas::{
    canvas::{
        self, CleanedSamplingFlags, DstClipMatrixCounts, PaddedLayerBoundsResult,
        WritePixelsPreflight,
    },
    canvas_utils::{self, StateClipRect},
    device::{
        self, BitmapDeviceValidation, BitmapImageRectPlan, DeviceCoordSystem,
        DeviceGlobalCtmResult, DrawTilerInit, DrawTilerStep, NoPixelsClipState,
    },
    draw::{
        self, AtlasQuadMatrix, ClippedTriangle2D, DrawRectPlan, HairlinePaintPlan,
        PtProcInitResult, TransformedMaskGeometry, UniformColorRgba, VerticesBlendPlan, XRect,
    },
    mesh_doc::{self, DocumentTrimResult, MeshAttrDesc},
    pixel_recs::{self, PixelsRecTrimResult},
};
use skia_rs_geometry::{IPoint, IRect, ISize, Matrix, Point, Point3, RSXform, Rect, M44};
use std::slice;

// ============================================================================
// SkDraw / SkDraw_atlas / SkDraw_text / SkDraw_vertices
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_treat_aa_stroke_as_hairline(
    stroke_width: f32,
    matrix: *const Matrix,
    out_coverage: *mut f32,
) -> bool {
    let Some(m) = matrix.as_ref() else {
        return false;
    };
    if let Some(cov) = draw::draw_treat_aa_stroke_as_hairline(stroke_width, m) {
        if !out_coverage.is_null() {
            *out_coverage = cov;
        }
        true
    } else {
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_hairline_paint_plan(
    style: i32,
    stroke_width: f32,
    is_aa: bool,
    alpha: u8,
    supports_coverage_as_alpha: bool,
    matrix: *const Matrix,
) -> HairlinePaintPlan {
    let m = match matrix.as_ref() {
        Some(m) => m,
        None => return HairlinePaintPlan { modify: false, set_alpha: false, new_alpha: alpha },
    };
    draw::hairline_paint_plan(style, stroke_width, is_aa, alpha, supports_coverage_as_alpha, m)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_compute_rect_type(
    rect: Rect,
    style: i32,
    stroke_width: f32,
    stroke_join: i32,
    stroke_miter: f32,
    has_path_effect: bool,
    has_mask_filter: bool,
    matrix: *const Matrix,
    out_stroke_size: *mut Point,
) -> i32 {
    let Some(m) = matrix.as_ref() else {
        return draw::DrawRectType::Path as i32;
    };
    let (rtype, sz) = draw::compute_rect_type(
        rect,
        style,
        stroke_width,
        stroke_join,
        stroke_miter,
        has_path_effect,
        has_mask_filter,
        m,
    );
    if !out_stroke_size.is_null() {
        *out_stroke_size = sz;
    }
    rtype as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_compute_rect_plan(
    pre_paint_rect: Rect,
    paint_rect: Rect,
    style: i32,
    stroke_width: f32,
    stroke_join: i32,
    stroke_miter: f32,
    has_path_effect: bool,
    has_mask_filter: bool,
    ctm: *const Matrix,
) -> DrawRectPlan {
    let Some(m) = ctm.as_ref() else {
        return DrawRectPlan {
            action: 0,
            rect_type: draw::DrawRectType::Path as i32,
            dev_rect: Rect::default(),
            stroke_size: Point::default(),
            bbox_ir: IRect::default(),
        };
    };
    draw::compute_rect_draw_plan(
        pre_paint_rect,
        paint_rect,
        style,
        stroke_width,
        stroke_join,
        stroke_miter,
        has_path_effect,
        has_mask_filter,
        m,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_pt_proc_init(
    mode: i32,
    has_path_effect: bool,
    has_mask_filter: bool,
    stroke_width: f32,
    stroke_cap: i32,
    matrix: *const Matrix,
    clip_bounds: IRect,
) -> PtProcInitResult {
    let Some(m) = matrix.as_ref() else {
        return PtProcInitResult {
            valid: false,
            radius: -1.0,
            clip_bounds: Rect::default(),
        };
    };
    draw::pt_proc_init(
        mode,
        has_path_effect,
        has_mask_filter,
        stroke_width,
        stroke_cap,
        m,
        clip_bounds,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_pt_proc_choose(
    mode: i32,
    is_aa: bool,
    stroke_width: f32,
    stroke_cap: i32,
    radius: f32,
) -> i32 {
    draw::pt_proc_choose(mode, is_aa, stroke_width, stroke_cap, radius) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_make_square_rad(center: Point, radius: f32) -> Rect {
    draw::make_square_rad(center, radius)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_make_xrect(r: Rect) -> XRect {
    draw::make_xrect(r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_conservative_local_clip(
    ctm: *const Matrix,
    rc_bounds: IRect,
    rc_empty: bool,
    out_local: *mut Rect,
) -> bool {
    let (Some(m), Some(out)) = (ctm.as_ref(), out_local.as_mut()) else {
        return false;
    };
    if let Some(r) = draw::compute_conservative_local_clip_bounds(m, rc_bounds, rc_empty) {
        *out = r;
        true
    } else {
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_mask_bounds_init(dev_path_bounds: Rect) -> IRect {
    draw::compute_mask_bounds_init(dev_path_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_trim_mask_bounds(
    bounds: *mut IRect,
    clip_bounds: IRect,
    margin_x: i32,
    margin_y: i32,
) -> bool {
    let Some(b) = bounds.as_mut() else {
        return false;
    };
    if let Some(trimmed) = draw::trim_mask_bounds(*b, clip_bounds, margin_x, margin_y) {
        *b = trimmed;
        true
    } else {
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_transformed_bitmap_mask(
    bm_width: i32,
    bm_height: i32,
    ctm: *const Matrix,
    dst_bounds: IRect,
    rc_bounds: IRect,
) -> TransformedMaskGeometry {
    let Some(m) = ctm.as_ref() else {
        return TransformedMaskGeometry {
            valid: false,
            bounds: IRect::default(),
            row_bytes: 0,
        };
    };
    draw::compute_transformed_bitmap_mask(bm_width, bm_height, m, dst_bounds, rc_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_mapped_bitmap_round_out(
    ctm: *const Matrix,
    prematrix: *const Matrix,
    width: i32,
    height: i32,
    out_matrix: *mut Matrix,
    out_ir: *mut IRect,
) {
    let (Some(c), Some(p)) = (ctm.as_ref(), prematrix.as_ref()) else {
        return;
    };
    let (m, ir) = draw::mapped_bitmap_round_out(c, p, width, height);
    if !out_matrix.is_null() {
        *out_matrix = m;
    }
    if !out_ir.is_null() {
        *out_ir = ir;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_atlas_quad_matrix(
    xform: RSXform,
    tex: Rect,
    ctm: *const Matrix,
) -> AtlasQuadMatrix {
    let Some(m) = ctm.as_ref() else {
        return AtlasQuadMatrix {
            invertible: false,
            mx: Matrix::identity(),
            inv: Matrix::identity(),
        };
    };
    draw::draw_atlas_quad_matrix(xform, tex, m)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_load_uniform_color(
    rgba: *const f32,
) -> UniformColorRgba {
    if rgba.is_null() {
        return UniformColorRgba {
            rgba_u16: [0; 4],
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
    }
    let s = slice::from_raw_parts(rgba, 4);
    draw::load_uniform_color_rgba([s[0], s[1], s[2], s[3]])
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_check_glyph_position(pos: Point) -> bool {
    draw::check_glyph_position(pos)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_compute_is_opaque_colors(
    colors: *const u32,
    count: i32,
) -> bool {
    if colors.is_null() || count <= 0 {
        return true;
    }
    let s = slice::from_raw_parts(colors, count as usize);
    draw::compute_is_opaque_colors(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_clip_triangle_3d(
    p0: Point3,
    p1: Point3,
    p2: Point3,
) -> ClippedTriangle2D {
    draw::clip_triangle_3d(p0, p1, p2)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_texture_to_matrix(
    src: *const Point,
    dst: *const Point,
    out_matrix: *mut Matrix,
) -> bool {
    if src.is_null() || dst.is_null() || out_matrix.is_null() {
        return false;
    }
    let s = slice::from_raw_parts(src, 3);
    let d = slice::from_raw_parts(dst, 3);
    if let Some(m) = draw::texture_to_matrix_3pts(&[s[0], s[1], s[2]], &[d[0], d[1], d[2]]) {
        *out_matrix = m;
        true
    } else {
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_vertices_blend_simplify(
    has_blend_mode: bool,
    blend_mode: i32,
    has_colors: bool,
) -> VerticesBlendPlan {
    draw::vertices_blend_simplify(has_blend_mode, blend_mode, has_colors)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_vertices_early_out(
    vertex_count: usize,
    index_count: usize,
    rc_empty: bool,
) -> bool {
    draw::vertices_early_out(vertex_count, index_count, rc_empty)
}

// ============================================================================
// SkDevice / SkBitmapDevice / SkClipStackDevice / SkTiledImageUtils
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_set_coord_system(
    device_to_global: *const M44,
    global_to_device: *const M44,
    local_to_device: *const M44,
    buffer_origin_x: i32,
    buffer_origin_y: i32,
    out: *mut DeviceCoordSystem,
) {
    let (Some(d2g), Some(g2d), Some(l2d), Some(o)) = (
        device_to_global.as_ref(),
        global_to_device.as_ref(),
        local_to_device.as_ref(),
        out.as_mut(),
    ) else {
        return;
    };
    *o = device::set_device_coordinate_system(*d2g, *g2d, *l2d, buffer_origin_x, buffer_origin_y);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_set_global_ctm(
    ctm: *const M44,
    global_to_device: *const M44,
    out: *mut DeviceGlobalCtmResult,
) {
    let (Some(c), Some(g2d), Some(o)) = (ctm.as_ref(), global_to_device.as_ref(), out.as_mut())
    else {
        return;
    };
    *o = device::set_global_ctm(*c, g2d);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_is_pixel_aligned(
    device_to_global: *const M44,
) -> bool {
    device_to_global
        .as_ref()
        .is_some_and(device::is_pixel_aligned_to_global)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_get_origin(device_to_global: *const M44) -> IPoint {
    device_to_global
        .as_ref()
        .map(device::get_origin)
        .unwrap_or_default()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_get_relative_transform(
    src_device_to_global: *const M44,
    dst_global_to_device: *const M44,
    out: *mut M44,
) {
    let (Some(s), Some(d), Some(o)) = (
        src_device_to_global.as_ref(),
        dst_global_to_device.as_ref(),
        out.as_mut(),
    ) else {
        return;
    };
    *o = device::get_relative_transform(s, d);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_draw_region_needs_path(
    local_to_device: *const Matrix,
    paint_style: i32,
    has_mask_filter: bool,
    has_path_effect: bool,
    is_aa: bool,
) -> bool {
    local_to_device.as_ref().is_some_and(|m| {
        device::device_draw_region_needs_path(m, paint_style, has_mask_filter, has_path_effect, is_aa)
    })
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_draw_device_is_strict(
    is_nearest: bool,
    relative_transform: *const Matrix,
) -> bool {
    relative_transform
        .as_ref()
        .is_none_or(|m| device::device_draw_device_is_strict(is_nearest, m))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_lattice_modulate_color(
    color: u32,
    paint_alpha: u8,
) -> u32 {
    device::device_lattice_modulate_color(color, paint_alpha)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_quad_to_tris(
    quad: *const Point,
    tris_out: *mut Point,
) {
    if quad.is_null() || tris_out.is_null() {
        return;
    }
    let q = slice::from_raw_parts(quad, 4);
    let tris = device::quad_to_tris([q[0], q[1], q[2], q[3]]);
    let out = slice::from_raw_parts_mut(tris_out, 6);
    out.copy_from_slice(&tris);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_device_scaler_context_flags(
    has_cs: bool,
    gamma_is_linear: bool,
) -> u32 {
    device::scaler_context_flags(has_cs, gamma_is_linear)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_no_pixels_clip_op(
    state: NoPixelsClipState,
    op: i32,
    transform: *const M44,
    bounds: Rect,
    is_aa: bool,
    fills_bounds: bool,
) -> NoPixelsClipState {
    let Some(t) = transform.as_ref() else {
        return state;
    };
    device::no_pixels_clip_state_op(state, op, t, bounds, is_aa, fills_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_no_pixels_replace_clip(
    global_to_device: *const M44,
    rect: IRect,
    device_bounds: IRect,
) -> IRect {
    let Some(g2d) = global_to_device.as_ref() else {
        return IRect::default();
    };
    device::no_pixels_replace_clip_bounds(g2d, rect, device_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_tiler_needs_tiling(width: i32, height: i32) -> bool {
    device::draw_tiler_needs_tiling(width, height)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_tiler_init(
    clip_r: IRect,
    local_to_device: *const Matrix,
    bounds: *const Rect,
) -> DrawTilerInit {
    let Some(m) = local_to_device.as_ref() else {
        return DrawTilerInit {
            needs_tiling: false,
            done: true,
            src_bounds: IRect::default(),
            origin: IPoint::default(),
        };
    };
    device::draw_tiler_init(clip_r, m, bounds.as_ref().copied())
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_tiler_step(
    origin: IPoint,
    src_bounds: IRect,
    local_to_device: *const Matrix,
    out: *mut DrawTilerStep,
) {
    let (Some(m), Some(o)) = (local_to_device.as_ref(), out.as_mut()) else {
        return;
    };
    *o = device::draw_tiler_step(origin, src_bounds, m);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_valid_for_bitmap_device(
    width: i32,
    height: i32,
    color_type: i32,
    alpha_type: i32,
    color_type_always_opaque: bool,
) -> BitmapDeviceValidation {
    device::valid_for_bitmap_device(width, height, color_type, alpha_type, color_type_always_opaque)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_can_apply_dst_matrix_as_ctm(
    m: *const Matrix,
    has_mask_filter: bool,
) -> bool {
    m.as_ref()
        .is_some_and(|mat| device::can_apply_dst_matrix_as_ctm(mat, has_mask_filter))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_bitmap_image_rect_plan(
    bm_width: i32,
    bm_height: i32,
    src: *const Rect,
    dst: Rect,
    is_fast_constraint: bool,
    is_default_sampling: bool,
    out: *mut BitmapImageRectPlan,
) {
    let Some(o) = out.as_mut() else {
        return;
    };
    *o = device::bitmap_device_image_rect_plan(
        bm_width,
        bm_height,
        src.as_ref().copied(),
        dst,
        is_fast_constraint,
        is_default_sampling,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_bitmap_is_clip_wide_open(
    is_bw: bool,
    bw_is_rect: bool,
    bw_bounds: IRect,
    width: i32,
    height: i32,
) -> bool {
    device::bitmap_device_is_clip_wide_open(is_bw, bw_is_rect, bw_bounds, width, height)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_bitmap_is_clip_rect(
    rc_empty: bool,
    rc_is_rect: bool,
    has_clip_shader: bool,
) -> bool {
    device::bitmap_device_is_clip_rect(rc_empty, rc_is_rect, has_clip_shader)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_tiled_image_bitmap_keys(
    gen_id: u32,
    image_bounds: IRect,
    pixel_ref_origin: IPoint,
    out_keys: *mut u32,
) {
    if out_keys.is_null() {
        return;
    }
    let keys = device::tiled_image_bitmap_key_values(gen_id, image_bounds, pixel_ref_origin);
    let out = slice::from_raw_parts_mut(out_keys, 6);
    out.copy_from_slice(&keys);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_tiled_image_unique_id_keys(
    unique_id: u32,
    out_keys: *mut u32,
) {
    if out_keys.is_null() {
        return;
    }
    let keys = device::tiled_image_unique_id_key_values(unique_id);
    let out = slice::from_raw_parts_mut(out_keys, 6);
    out.copy_from_slice(&keys);
}

// ============================================================================
// SkCanvas / SkCanvasPriv / SkOverdrawCanvas
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_decomposition_center(
    dst_to_local: *const M44,
    content_bounds: *const Rect,
    target_output: IRect,
) -> Point {
    let Some(m) = dst_to_local.as_ref() else {
        return Point::default();
    };
    canvas::compute_decomposition_center(m, content_bounds.as_ref().copied(), target_output)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_max_layer_dim(target_output: IRect) -> i32 {
    canvas::compute_max_layer_dim(target_output)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_image_filter_color_type(
    color_type: i32,
    bytes_per_pixel: i32,
    n32_color_type: i32,
) -> i32 {
    canvas::image_filter_color_type(color_type, bytes_per_pixel, n32_color_type)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_padded_layer_bounds(
    layer_bounds: IRect,
    pad: i32,
) -> PaddedLayerBoundsResult {
    canvas::compute_padded_layer_bounds(layer_bounds, pad)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_would_overwrite_check(
    base_size: ISize,
    is_root_device: bool,
    root_clip_wide_open: bool,
    rect: *const Rect,
    total_matrix: *const Matrix,
    has_paint: bool,
    paint_style: i32,
    has_mask_filter: bool,
    has_path_effect: bool,
    has_image_filter: bool,
) -> bool {
    let Some(m) = total_matrix.as_ref() else {
        return false;
    };
    canvas::would_overwrite_check_geometry_and_paint(
        base_size,
        is_root_device,
        root_clip_wide_open,
        rect.as_ref().copied(),
        m,
        has_paint,
        paint_style,
        has_mask_filter,
        has_path_effect,
        has_image_filter,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_quick_reject_rect(
    m44: *const M44,
    src: Rect,
    quick_reject_bounds: Rect,
) -> bool {
    m44.as_ref()
        .is_none_or(|m| canvas::quick_reject_rect(m, src, quick_reject_bounds))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_compute_device_clip_bounds(
    clip_empty: bool,
    device_to_global: *const M44,
    dev_clip_bounds: IRect,
    outset_for_aa: bool,
) -> Rect {
    let Some(d2g) = device_to_global.as_ref() else {
        return Rect::default();
    };
    canvas::compute_device_clip_bounds(clip_empty, d2g, dev_clip_bounds, outset_for_aa)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_compute_local_clip_bounds(
    device_clip_bounds: IRect,
    ctm: *const M44,
) -> Rect {
    let Some(m) = ctm.as_ref() else {
        return Rect::default();
    };
    canvas::compute_local_clip_bounds(device_clip_bounds, m)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_fillable_rect(r: Rect) -> bool {
    canvas::fillable_rect(r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_clean_sampling_for_constraint(
    is_strict: bool,
    has_mipmap: bool,
    is_aniso: bool,
) -> CleanedSamplingFlags {
    canvas::clean_sampling_for_constraint(is_strict, has_mipmap, is_aniso)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_edge_aa_single_can_map_dst(
    has_clip: bool,
    matrix_index: i32,
    pre_view_matrix: *const Matrix,
) -> bool {
    canvas::edge_aa_single_entry_can_map_dst(has_clip, matrix_index, pre_view_matrix.as_ref())
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_drrect_action(
    outer_empty: bool,
    inner_empty: bool,
    outer_bounds: Rect,
    inner_bounds: Rect,
) -> i32 {
    canvas::draw_drrect_action(outer_empty, inner_empty, outer_bounds, inner_bounds) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_circle_bounds(cx: f32, cy: f32, radius: f32) -> Rect {
    canvas::draw_circle_bounds(cx, cy, radius)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_round_rect_is_rrect(rx: f32, ry: f32) -> bool {
    canvas::draw_round_rect_is_rrect(rx, ry)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_draw_arc_should_skip(oval: Rect, sweep_angle: f32) -> bool {
    canvas::draw_arc_should_skip(oval, sweep_angle)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_text_blob_add_glyph_count(
    total: i32,
    run_glyph_count: i32,
    out_total: *mut i32,
) -> bool {
    if let Some(next) = canvas::text_blob_add_glyph_count(total, run_glyph_count) {
        if !out_total.is_null() {
            *out_total = next;
        }
        true
    } else {
        false
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_restore_to_count_steps(
    save_count: i32,
    target_count: i32,
) -> i32 {
    canvas::restore_to_count_steps(save_count, target_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_write_pixels_preflight(
    src_w: i32,
    src_h: i32,
    x: i32,
    y: i32,
    dev_w: i32,
    dev_h: i32,
) -> WritePixelsPreflight {
    canvas::write_pixels_preflight(src_w, src_h, x, y, dev_w, dev_h)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_lattice_validate_flag_count(
    x_count: i32,
    y_count: i32,
    flag_count: i32,
) -> bool {
    canvas::lattice_validate_flag_count(x_count, y_count, flag_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_lattice_serialized_size(
    x_count: i32,
    y_count: i32,
    has_rect_types: bool,
    out_flag_count: *mut i32,
) -> usize {
    let (fc, sz) = canvas::lattice_serialized_size(x_count, y_count, has_rect_types);
    if !out_flag_count.is_null() {
        *out_flag_count = fc;
    }
    sz
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_dst_clip_and_matrix_counts(
    has_clips: *const u8,
    matrix_indices: *const i32,
    count: i32,
) -> DstClipMatrixCounts {
    if count <= 0 || has_clips.is_null() || matrix_indices.is_null() {
        return DstClipMatrixCounts {
            total_dst_clip_count: 0,
            total_matrix_count: 0,
        };
    }
    let hc = slice::from_raw_parts(has_clips, count as usize);
    let mi = slice::from_raw_parts(matrix_indices, count as usize);
    canvas::get_dst_clip_and_matrix_counts(hc, mi)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_overdraw_alpha_matrix(out20: *mut f32) {
    if out20.is_null() {
        return;
    }
    let mat = canvas::overdraw_alpha_increment_matrix();
    slice::from_raw_parts_mut(out20, 20).copy_from_slice(&mat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_overdraw_is_clear_paint(
    color: u32,
    has_color_filter: bool,
    has_shader: bool,
) -> bool {
    canvas::overdraw_is_clear_paint(color, has_color_filter, has_shader)
}

// ============================================================================
// SkReadPixelsRec / SkWritePixelsRec / SkPixelStorage / SkContext
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_trim_pixels_rec(
    has_pixels: bool,
    info_width: i32,
    info_height: i32,
    min_row_bytes: usize,
    bytes_per_pixel: usize,
    row_bytes: usize,
    x: i32,
    y: i32,
    bound_width: i32,
    bound_height: i32,
) -> PixelsRecTrimResult {
    pixel_recs::trim_pixels_rec(
        has_pixels,
        info_width,
        info_height,
        min_row_bytes,
        bytes_per_pixel,
        row_bytes,
        x,
        y,
        bound_width,
        bound_height,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_next_pixel_storage_id() -> u32 {
    pixel_recs::next_pixel_storage_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_context_select_recorder(
    backend: i32,
    has_gpu_context: bool,
) -> i32 {
    pixel_recs::context_select_recorder_backend(backend, has_gpu_context) as i32
}

// ============================================================================
// SkMesh / SkDocument
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_check_name(name: *const u8, len: usize) -> bool {
    if name.is_null() || len == 0 {
        return false;
    }
    mesh_doc::check_mesh_var_name(slice::from_raw_parts(name, len))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_attr_size(attr_type: u32) -> usize {
    mesh_doc::attribute_type_size(attr_type)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_check_layout(
    attrs: *const MeshAttrDesc,
    count: usize,
    stride: usize,
) -> i32 {
    let s = if attrs.is_null() || count == 0 {
        &[]
    } else {
        slice::from_raw_parts(attrs, count)
    };
    mesh_doc::check_vertex_offsets_and_stride(s, stride) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_uniform_size(
    has_uniforms: bool,
    last_offset: usize,
    last_size: usize,
) -> usize {
    mesh_doc::mesh_spec_uniform_size(has_uniforms, last_offset, last_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_spec_hash(
    vs: *const u8,
    vs_len: usize,
    fs: *const u8,
    fs_len: usize,
    attrs: *const MeshAttrDesc,
    attr_count: usize,
    stride: usize,
    cs_hash: u64,
    alpha_type: u32,
) -> u32 {
    let vs_s = if vs.is_null() || vs_len == 0 {
        &[]
    } else {
        slice::from_raw_parts(vs, vs_len)
    };
    let fs_s = if fs.is_null() || fs_len == 0 {
        &[]
    } else {
        slice::from_raw_parts(fs, fs_len)
    };
    let attr_s = if attrs.is_null() || attr_count == 0 {
        &[]
    } else {
        slice::from_raw_parts(attrs, attr_count)
    };
    mesh_doc::mesh_spec_compute_hash(vs_s, fs_s, attr_s, stride, cs_hash, alpha_type)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_validate_buffers(
    stride: usize,
    vcount: usize,
    voffset: usize,
    vb_size: usize,
    spec_uniform_size: usize,
    has_uniforms: bool,
    uniforms_size: usize,
    has_ib: bool,
    ib_size: usize,
    icount: usize,
    ioffset: usize,
    mode: i32,
) -> i32 {
    mesh_doc::validate_mesh_buffers(
        stride,
        vcount,
        voffset,
        vb_size,
        spec_uniform_size,
        has_uniforms,
        uniforms_size,
        has_ib,
        ib_size,
        icount,
        ioffset,
        mode,
    ) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_mesh_check_update(
    has_data: bool,
    offset: usize,
    size: usize,
    buffer_size: usize,
) -> bool {
    mesh_doc::check_mesh_buffer_update(has_data, offset, size, buffer_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_doc_can_begin_page(
    width: f32,
    height: f32,
    state: i32,
) -> bool {
    mesh_doc::document_can_begin_page(width, height, state)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_doc_trim_content(
    width: f32,
    height: f32,
    content: Rect,
) -> DocumentTrimResult {
    mesh_doc::document_trim_content(width, height, content)
}

// ============================================================================
// SkCanvasStack / SkCanvasStateUtils / SkPaintFilterCanvas
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_stack_local_subtract_bounds(
    canvas_bounds: IRect,
    new_origin: IPoint,
    existing_origin: IPoint,
) -> IRect {
    canvas_utils::canvas_stack_local_subtract_bounds(canvas_bounds, new_origin, existing_origin)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_stack_layer_matrix(
    origin: IPoint,
    mx: *const M44,
    out: *mut M44,
) {
    let (Some(m), Some(o)) = (mx.as_ref(), out.as_mut()) else {
        return;
    };
    *o = canvas_utils::canvas_stack_layer_matrix(origin, m);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_state_color_to_raster_config(
    color_type: i32,
    n32_color_type: i32,
) -> i32 {
    canvas_utils::canvas_state_color_type_to_raster_config(color_type, n32_color_type)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_state_raster_config_to_color(
    raster_config: i32,
    n32_color_type: i32,
) -> i32 {
    canvas_utils::canvas_state_raster_config_to_color_type(raster_config, n32_color_type)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_state_union_clip_rects(
    rects: *const StateClipRect,
    count: i32,
) -> IRect {
    if rects.is_null() || count <= 0 {
        return IRect::default();
    }
    canvas_utils::canvas_state_union_clip_rects(slice::from_raw_parts(rects, count as usize))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_canvas_paint_filter_can_restore_null(
    alphaf: f32,
    has_color_filter: bool,
    has_image_filter: bool,
    has_blend_mode: bool,
    blend_mode: i32,
) -> bool {
    canvas_utils::paint_filter_can_restore_null_paint(
        alphaf,
        has_color_filter,
        has_image_filter,
        has_blend_mode,
        blend_mode,
    )
}
