//! FFI exports for `skia-rs-effects` (Phase 1 Layer 4.3).
//!
//! Covers:
//! - Path effects (`Sk1DPathEffect`, `Sk2DPathEffect`, `SkCornerPathEffect`,
//!   `SkDashPathEffect`, `SkDiscretePathEffect`, `SkTrimPathEffect`)
//! - Mask filters (`SkMaskFilterBase`, `SkBlurMaskFilterImpl`, `SkMaskBlurFilter`,
//!   `SkEmbossMask`, `SkEmbossMaskFilter`, `SkShaderMaskFilterImpl`, `SkTableMaskFilter`)
//! - Blur engine (`SkBlurEngine`, `SkShaderBlurAlgorithm`)
//! - Shadows (`SkDrawShadowInfo`, `SkShadowTessellator`, `SkShadowUtils`)

use skia_rs_effects::{
    blur_engine::{self, RenderBlurPlan, MAX_SAMPLES},
    mask_blur_filter::{MaskBlurFilter, MaskFormat},
    mask_filters::{
        self, EmbossLight, NinePatchLayout, RRectNinePatchPlan, RectsNinePatchPlan,
    },
    path_effects::{
        self, CornerOutCmd, CornerPathState, CornerVerb, DashPointsResult, DiscreteContourPlan,
        LcgRandom, TrimMode, TrimSpanPlan,
    },
    shadow_info::{self, DrawShadowRec},
    shadow_tessellator::{self, ShadowMesh, ShadowPolygonState, SpotFactoryParams},
};
use skia_rs_geometry::{IPoint, IRect, ISize, Matrix, Point, Point3, Rect, Size, Vector, V2, V4};

// ============================================================================
// Path Effects
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_1d_is_valid(
    advance: f32,
    phase: f32,
    path_is_empty: bool,
) -> bool {
    path_effects::path_1d_is_valid(advance, phase, path_is_empty)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_1d_initial_offset(advance: f32, phase: f32) -> f32 {
    path_effects::path_1d_initial_offset(advance, phase)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_1d_morph_point(
    sx: f32,
    sy: f32,
    pos: Point,
    tangent: Vector,
) -> Point {
    path_effects::path_1d_morph_point(sx, sy, pos, tangent)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_1d_line_midpoint(p0: Point, p1: Point) -> Point {
    path_effects::path_1d_line_midpoint(p0, p1)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_line_2d_is_valid(width: f32) -> bool {
    path_effects::line_2d_is_valid(width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_line_2d_span_endpoints(
    u: i32,
    v: i32,
    ucount: i32,
    mat: *const Matrix,
    out_pts: *mut Point,
) -> bool {
    if mat.is_null() || out_pts.is_null() {
        return false;
    }
    match path_effects::line_2d_span_endpoints(u, v, ucount, &*mat) {
        Some(pts) => {
            let dst = std::slice::from_raw_parts_mut(out_pts, 2);
            dst[0] = pts[0];
            dst[1] = pts[1];
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_2d_cell_location(
    x: i32,
    y: i32,
    mat: *const Matrix,
) -> Point {
    if mat.is_null() {
        return Point::new(0.0, 0.0);
    }
    path_effects::path_2d_cell_location(x, y, &*mat)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_corner_is_valid(radius: f32) -> bool {
    path_effects::corner_is_valid(radius)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_corner_state_new(radius: f32) -> *mut CornerPathState {
    Box::into_raw(Box::new(CornerPathState::new(radius)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_corner_state_process(
    state: *mut CornerPathState,
    verb: u8,
    pts: *const Point,
    pt_count: usize,
    conic_weight: f32,
    is_closed_contour: bool,
    out_cmds: *mut CornerOutCmd,
) -> usize {
    if state.is_null() || out_cmds.is_null() {
        return 0;
    }
    let corner_verb = match verb {
        0 => CornerVerb::Move,
        1 => CornerVerb::Line,
        2 => CornerVerb::Quad,
        3 => CornerVerb::Conic,
        4 => CornerVerb::Cubic,
        5 => CornerVerb::Close,
        _ => return 0,
    };
    let mut pt_arr = [Point::new(0.0, 0.0); 4];
    if !pts.is_null() && pt_count > 0 {
        let n = pt_count.min(4);
        let src = std::slice::from_raw_parts(pts, n);
        pt_arr[..n].copy_from_slice(src);
    }
    let out_arr = &mut *(out_cmds as *mut [CornerOutCmd; 3]);
    (*state).process_verb(corner_verb, &pt_arr, conic_weight, is_closed_contour, out_arr)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_corner_state_finish(
    state: *const CornerPathState,
    out_pt: *mut Point,
) -> bool {
    if state.is_null() || out_pt.is_null() {
        return false;
    }
    match (*state).finish() {
        Some(p) => {
            *out_pt = p;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_corner_state_free(state: *mut CornerPathState) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_dash_as_points(
    intervals: *const f32,
    interval_count: usize,
    initial_dash_length: f32,
    initial_dash_index: usize,
    interval_length: f32,
    line_pts: *const Point,
    stroke_width: f32,
    cap: i32,
    is_miter_join: bool,
    miter: f32,
    matrix: *const Matrix,
    cull_rect: *const Rect,
    populate_results: bool,
) -> *mut DashPointsResult {
    if intervals.is_null() || line_pts.is_null() || matrix.is_null() {
        return std::ptr::null_mut();
    }
    let ivs = std::slice::from_raw_parts(intervals, interval_count);
    let lp = std::slice::from_raw_parts(line_pts, 2);
    let cull = if cull_rect.is_null() {
        None
    } else {
        Some(*cull_rect)
    };
    match path_effects::dash_as_points(
        ivs,
        initial_dash_length,
        initial_dash_index,
        interval_length,
        [lp[0], lp[1]],
        stroke_width,
        cap,
        is_miter_join,
        miter,
        &*matrix,
        cull,
        populate_results,
    ) {
        Some(res) => Box::into_raw(Box::new(res)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_dash_points_extract(
    res: *const DashPointsResult,
    out_size: *mut Point,
    out_flags: *mut u32,
    out_pts_ptr: *mut *const Point,
    out_pts_len: *mut usize,
    out_has_first: *mut bool,
    out_first_rect: *mut Rect,
    out_has_last: *mut bool,
    out_last_rect: *mut Rect,
) {
    if res.is_null() {
        return;
    }
    let r = &*res;
    if !out_size.is_null() {
        *out_size = r.size;
    }
    if !out_flags.is_null() {
        *out_flags = r.flags;
    }
    if !out_pts_ptr.is_null() {
        *out_pts_ptr = r.points.as_ptr();
    }
    if !out_pts_len.is_null() {
        *out_pts_len = r.points.len();
    }
    if !out_has_first.is_null() {
        *out_has_first = r.first_rect.is_some();
    }
    if !out_first_rect.is_null() {
        if let Some(fr) = r.first_rect {
            *out_first_rect = fr;
        }
    }
    if !out_has_last.is_null() {
        *out_has_last = r.last_rect.is_some();
    }
    if !out_last_rect.is_null() {
        if let Some(lr) = r.last_rect {
            *out_last_rect = lr;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_dash_points_free(res: *mut DashPointsResult) {
    if !res.is_null() {
        drop(Box::from_raw(res));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_discrete_is_valid(seg_length: f32, deviation: f32) -> bool {
    path_effects::discrete_is_valid(seg_length, deviation)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_discrete_rng_init(
    seed_assist: u32,
    first_contour_length: f32,
) -> u32 {
    LcgRandom::from_contour(seed_assist, first_contour_length).seed()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_discrete_rng_next(seed: *mut u32) -> f32 {
    if seed.is_null() {
        return 0.0;
    }
    let mut rng = LcgRandom::new(*seed);
    let val = rng.next_sscalar1();
    *seed = rng.seed();
    val
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_discrete_perturb(
    p: Point,
    tangent: Vector,
    scale: f32,
) -> Point {
    path_effects::discrete_perturb(p, tangent, scale)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_discrete_contour_plan(
    seg_length: f32,
    length: f32,
    do_fill: bool,
    is_closed: bool,
) -> DiscreteContourPlan {
    path_effects::discrete_contour_plan(seg_length, length, do_fill, is_closed)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_discrete_compute_fast_bounds(
    bounds: Rect,
    deviation: f32,
) -> Rect {
    path_effects::discrete_compute_fast_bounds(bounds, deviation)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_trim_validate_and_pin(
    start_t: f32,
    stop_t: f32,
    mode: u32,
    out_start: *mut f32,
    out_stop: *mut f32,
) -> bool {
    let m = if mode == 1 {
        TrimMode::Inverted
    } else {
        TrimMode::Normal
    };
    match path_effects::trim_validate_and_pin(start_t, stop_t, m) {
        Some((s, e)) => {
            if !out_start.is_null() {
                *out_start = s;
            }
            if !out_stop.is_null() {
                *out_stop = e;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_trim_compute_spans(
    total_len: f32,
    start_t: f32,
    stop_t: f32,
    mode: u32,
) -> TrimSpanPlan {
    let m = if mode == 1 {
        TrimMode::Inverted
    } else {
        TrimMode::Normal
    };
    path_effects::trim_compute_spans(total_len, start_t, stop_t, m)
}

// ============================================================================
// Mask Filters & MaskBlurFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_emboss_make_light(
    blur_sigma: f32,
    light: EmbossLight,
    out_light: *mut EmbossLight,
) -> bool {
    if out_light.is_null() {
        return false;
    }
    match mask_filters::emboss_make_light(blur_sigma, light) {
        Some(l) => {
            *out_light = l;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_emboss_transform_light(
    light: EmbossLight,
    matrix: *const Matrix,
) -> EmbossLight {
    if matrix.is_null() {
        return light;
    }
    mask_filters::emboss_transform_light(light, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_emboss_margin(sigma: f32) -> IPoint {
    mask_filters::emboss_margin(sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_emboss_mask(
    planes: *mut u8,
    plane_size: usize,
    width: i32,
    height: i32,
    row_bytes: usize,
    light: EmbossLight,
) {
    if planes.is_null() || plane_size == 0 || width <= 0 || height <= 0 {
        return;
    }
    let Some(total) = plane_size.checked_mul(3) else {
        return;
    };
    let slice = std::slice::from_raw_parts_mut(planes, total);
    mask_filters::emboss_mask(slice, plane_size, width, height, row_bytes, light);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_table_make_gamma(table: *mut u8, gamma: f32) {
    if table.is_null() {
        return;
    }
    let arr = &mut *(table as *mut [u8; 256]);
    mask_filters::make_gamma_table(arr, gamma);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_table_make_clip(table: *mut u8, min: u8, max: u8) {
    if table.is_null() {
        return;
    }
    let arr = &mut *(table as *mut [u8; 256]);
    mask_filters::make_clip_table(arr, min, max);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_table_apply_mask(
    src: *const u8,
    src_rb: usize,
    dst: *mut u8,
    dst_rb: usize,
    width: usize,
    height: usize,
    table: *const u8,
) {
    if src.is_null() || dst.is_null() || table.is_null() || width == 0 || height == 0 {
        return;
    }
    let src_len = (height - 1) * src_rb + width;
    let dst_len = height * dst_rb;
    let src_slice = std::slice::from_raw_parts(src, src_len);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let tbl = &*(table as *const [u8; 256]);
    mask_filters::apply_table_mask(src_slice, src_rb, dst_slice, dst_rb, width, height, tbl);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_rect_copy_bytes(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    copy_bytes: usize,
    rows: usize,
) {
    if dst.is_null() || src.is_null() || copy_bytes == 0 || rows == 0 {
        return;
    }
    let src_len = (rows - 1) * src_rb + copy_bytes;
    let dst_len = (rows - 1) * dst_rb + copy_bytes;
    let src_slice = std::slice::from_raw_parts(src, src_len);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    mask_filters::rect_copy_bytes(dst_slice, dst_rb, src_slice, src_rb, copy_bytes, rows);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_is_valid_sigma(sigma: f32) -> bool {
    mask_filters::blur_mask_filter_is_valid_sigma(sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_compute_device_sigma(
    sigma: f32,
    ignore_xform: bool,
    ctm: *const Matrix,
) -> f32 {
    if ctm.is_null() {
        return sigma;
    }
    mask_filters::blur_compute_xformed_device_sigma(sigma, ignore_xform, &*ctm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_compute_local_sigma(
    sigma: f32,
    ignore_xform: bool,
    ctm: *const Matrix,
) -> V2 {
    if ctm.is_null() {
        return V2 { x: 0.0, y: 0.0 };
    }
    mask_filters::blur_compute_xformed_local_sigma(sigma, ignore_xform, &*ctm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_as_image_filter_sigma(
    sigma: f32,
    ignore_xform: bool,
    ctm: *const Matrix,
) -> V2 {
    if ctm.is_null() {
        return V2 { x: sigma, y: sigma };
    }
    mask_filters::blur_as_image_filter_sigma(sigma, ignore_xform, &*ctm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_compute_fast_bounds(src: Rect, sigma: f32) -> Rect {
    mask_filters::blur_compute_fast_bounds(src, sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_rect_exceeds(r: Rect, v: f32) -> bool {
    mask_filters::rect_exceeds(r, v)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_plan_rrect_nine_patch(
    rect: Rect,
    radii: *const Vector,
    margin: IPoint,
) -> RRectNinePatchPlan {
    if radii.is_null() {
        return RRectNinePatchPlan {
            valid: false,
            _pad: [0; 3],
            total_small_width: 0,
            total_small_height: 0,
            center: IPoint { x: 0, y: 0 },
        };
    }
    let arr = &*(radii as *const [Vector; 4]);
    mask_filters::plan_rrect_nine_patch(rect, arr, margin)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mf_plan_rects_nine_patch(
    r0: Rect,
    has_r1: bool,
    r1: Rect,
    src_bounds: IRect,
    dst_bounds: IRect,
) -> RectsNinePatchPlan {
    let opt_r1 = if has_r1 { Some(r1) } else { None };
    mask_filters::plan_rects_nine_patch(r0, opt_r1, src_bounds, dst_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_filter_compute_nine_patch_layout(
    mask_bounds: IRect,
    outer_r: IRect,
    center: IPoint,
) -> NinePatchLayout {
    mask_filters::compute_nine_patch_layout(mask_bounds, outer_r, center)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_blur_pin_sigma(sigma: f64) -> f64 {
    MaskBlurFilter::new(sigma, sigma).sigma_w()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_blur_has_no_blur(sigma_w: f64, sigma_h: f64) -> bool {
    MaskBlurFilter::new(sigma_w, sigma_h).has_no_blur()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_blur_compute_border(sigma_w: f64, sigma_h: f64) -> IPoint {
    MaskBlurFilter::new(sigma_w, sigma_h).compute_border()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_blur_pixels(
    sigma_w: f64,
    sigma_h: f64,
    src_format: u8,
    src: *const u8,
    src_len: usize,
    src_rb: usize,
    src_w: usize,
    src_h: usize,
    dst: *mut u8,
    dst_len: usize,
    dst_rb: usize,
    dst_w: usize,
    dst_h: usize,
) -> IPoint {
    if src.is_null() || dst.is_null() || src_len == 0 || dst_len == 0 {
        return IPoint { x: 0, y: 0 };
    }
    let Some(fmt) = MaskFormat::from_u8(src_format) else {
        return IPoint { x: 0, y: 0 };
    };
    let src_slice = std::slice::from_raw_parts(src, src_len);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    MaskBlurFilter::new(sigma_w, sigma_h).blur_pixels(
        fmt, src_slice, src_rb, src_w, src_h, dst_slice, dst_rb, dst_w, dst_h,
    )
}

// ============================================================================
// Blur Engine & ShaderBlurAlgorithm
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_compute_2d_kernel(
    sigma: Size,
    radius: ISize,
    kernel: *mut f32,
    kernel_len: usize,
) {
    if kernel.is_null() || kernel_len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts_mut(kernel, kernel_len);
    blur_engine::compute_2d_blur_kernel(sigma, radius, slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_compute_2d_offsets(
    radius: ISize,
    offsets: *mut V4,
) {
    if offsets.is_null() {
        return;
    }
    let arr = &mut *(offsets as *mut [V4; MAX_SAMPLES / 2]);
    blur_engine::compute_2d_blur_offsets(radius, arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_compute_1d_linear_kernel(
    sigma: f32,
    radius: i32,
    offsets_and_kernel: *mut V4,
) {
    if offsets_and_kernel.is_null() {
        return;
    }
    let arr = &mut *(offsets_and_kernel as *mut [V4; MAX_SAMPLES / 2]);
    blur_engine::compute_1d_blur_linear_kernel(sigma, radius, arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_stable_key_offset(kernel_width: i32) -> u32 {
    blur_engine::stable_key_offset(kernel_width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_plan_render_blur_rects(
    src_rect: IRect,
    backing_dims: ISize,
    radii: ISize,
    dst_rect: IRect,
    tile_mode: i32,
) -> RenderBlurPlan {
    blur_engine::plan_render_blur_rects(src_rect, backing_dims, radii, dst_rect, tile_mode)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_plan_intermediate_dst(
    src_rect: IRect,
    dst_rect: IRect,
    radius_y: i32,
    tile_mode: i32,
    out_rect: *mut IRect,
) -> bool {
    if out_rect.is_null() {
        return false;
    }
    match blur_engine::plan_shader_blur_intermediate_dst(src_rect, dst_rect, radius_y, tile_mode) {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_raster_a8_window(sigma: f32) -> i32 {
    blur_engine::raster_a8_window(sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_raster_8888_window(sigma: f32) -> i32 {
    blur_engine::raster_8888_window(sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_eval_a8(
    sigma: Size,
    src: *const u8,
    src_len: usize,
    src_row_pixels: usize,
    orig_src_bounds: IRect,
    dst: *mut u8,
    dst_len: usize,
    dst_row_pixels: usize,
    alloc_dst_bounds: IRect,
    orig_dst_bounds: IRect,
) {
    if src.is_null() || dst.is_null() || dst_len == 0 {
        return;
    }
    let src_slice = if src_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(src, src_len)
    };
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    blur_engine::eval_blur_passes_a8(
        sigma,
        src_slice,
        src_row_pixels,
        orig_src_bounds,
        dst_slice,
        dst_row_pixels,
        alloc_dst_bounds,
        orig_dst_bounds,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_engine_eval_8888(
    sigma: Size,
    src: *const u8,
    src_len: usize,
    src_row_pixels: usize,
    orig_src_bounds: IRect,
    dst: *mut u8,
    dst_len: usize,
    dst_row_pixels: usize,
    alloc_dst_bounds: IRect,
    orig_dst_bounds: IRect,
) {
    if src.is_null() || dst.is_null() || dst_len == 0 {
        return;
    }
    let src_slice = if src_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(src, src_len)
    };
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    blur_engine::eval_blur_passes_8888(
        sigma,
        src_slice,
        src_row_pixels,
        orig_src_bounds,
        dst_slice,
        dst_row_pixels,
        alloc_dst_bounds,
        orig_dst_bounds,
    );
}

// ============================================================================
// Shadows (SkDrawShadowInfo, SkShadowTessellator, SkShadowUtils)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_get_spot_transform(
    light_pos: Point3,
    light_radius: f32,
    ctm: *const Matrix,
    z_plane_params: Point3,
    path_bounds: Rect,
    directional: bool,
    out_transform: *mut Matrix,
    out_radius: *mut f32,
) -> bool {
    if ctm.is_null() || out_transform.is_null() || out_radius.is_null() {
        return false;
    }
    match shadow_info::get_spot_shadow_transform(
        light_pos,
        light_radius,
        &*ctm,
        z_plane_params,
        path_bounds,
        directional,
    ) {
        Some((mat, radius)) => {
            *out_transform = mat;
            *out_radius = radius;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_get_local_bounds(
    path_bounds: Rect,
    rec: *const DrawShadowRec,
    ctm: *const Matrix,
) -> Rect {
    if rec.is_null() || ctm.is_null() {
        return Rect::make_empty();
    }
    shadow_info::get_local_bounds(path_bounds, &*rec, &*ctm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_validate_ambient_args(
    bounds: Rect,
    ctm: *const Matrix,
    z_plane: Point3,
) -> bool {
    if ctm.is_null() {
        return false;
    }
    shadow_tessellator::validate_ambient_args(bounds, &*ctm, z_plane)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_validate_spot_args(
    bounds: Rect,
    ctm: *const Matrix,
    z_plane: Point3,
    light_pos: Point3,
    light_radius: f32,
) -> bool {
    if ctm.is_null() {
        return false;
    }
    shadow_tessellator::validate_spot_args(bounds, &*ctm, z_plane, light_pos, light_radius)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_ambient_inset_outset(
    z_plane: Point3,
    bounds: Rect,
    out_inset: *mut f32,
    out_outset: *mut f32,
) {
    let (inset, outset) = shadow_tessellator::ambient_inset_outset(z_plane, bounds);
    if !out_inset.is_null() {
        *out_inset = inset;
    }
    if !out_outset.is_null() {
        *out_outset = outset;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_spot_clip_quad_mid(
    p0: Point,
    p1: Point,
    p2: Point,
) -> Point {
    shadow_tessellator::spot_clip_quad_mid(p0, p1, p2)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_spot_clip_conic_mid(
    p0: Point,
    p1: Point,
    p2: Point,
    w: f32,
) -> Point {
    shadow_tessellator::spot_clip_conic_mid(p0, p1, p2, w)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_spot_clip_cubic_mids(
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    out_mids: *mut Point,
) {
    if out_mids.is_null() {
        return;
    }
    let mids = shadow_tessellator::spot_clip_cubic_mids(p0, p1, p2, p3);
    let dst = std::slice::from_raw_parts_mut(out_mids, 2);
    dst[0] = mids[0];
    dst[1] = mids[1];
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_poly_state_new() -> *mut ShadowPolygonState {
    Box::into_raw(Box::new(ShadowPolygonState::new()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_poly_state_handle_line(
    state: *mut ShadowPolygonState,
    p: Point,
) {
    if !state.is_null() {
        (*state).handle_line(p);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_poly_state_add_to_clip(
    state: *mut ShadowPolygonState,
    p: Point,
) {
    if !state.is_null() {
        (*state).add_to_clip(p);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_poly_state_finish(state: *mut ShadowPolygonState) {
    if !state.is_null() {
        (*state).finish_path_polygon();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_poly_state_info(
    state: *const ShadowPolygonState,
    out_path_pts: *mut *const Point,
    out_path_len: *mut usize,
    out_clip_pts: *mut *const Point,
    out_clip_len: *mut usize,
    out_centroid: *mut Point,
    out_area: *mut f32,
    out_is_convex: *mut bool,
    out_direction: *mut f32,
) {
    if state.is_null() {
        return;
    }
    let st = &*state;
    if !out_path_pts.is_null() {
        *out_path_pts = st.path_polygon.as_ptr();
    }
    if !out_path_len.is_null() {
        *out_path_len = st.path_polygon.len();
    }
    if !out_clip_pts.is_null() {
        *out_clip_pts = st.clip_polygon.as_ptr();
    }
    if !out_clip_len.is_null() {
        *out_clip_len = st.clip_polygon.len();
    }
    if !out_centroid.is_null() {
        *out_centroid = st.centroid;
    }
    if !out_area.is_null() {
        *out_area = st.area;
    }
    if !out_is_convex.is_null() {
        *out_is_convex = st.is_convex;
    }
    if !out_direction.is_null() {
        *out_direction = st.direction;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_poly_state_free(state: *mut ShadowPolygonState) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_convex_prepare_inset(
    path_pts: *const Point,
    path_len: usize,
    centroid: Point,
    inset: f32,
    out_inset: *mut f32,
    out_umbra_color: *mut u32,
) {
    let slice = if path_pts.is_null() || path_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(path_pts, path_len)
    };
    let (new_inset, umbra_color) =
        shadow_tessellator::convex_prepare_inset_and_color(slice, centroid, inset);
    if !out_inset.is_null() {
        *out_inset = new_inset;
    }
    if !out_umbra_color.is_null() {
        *out_umbra_color = umbra_color;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_build_convex_mesh(
    path_pts: *const Point,
    path_len: usize,
    umbra_pts: *const Point,
    umbra_len: usize,
    valid_umbra: bool,
    clip_pts: *const Point,
    clip_len: usize,
    centroid: Point,
    direction: f32,
    outset: f32,
    umbra_color: u32,
    transparent: bool,
    do_clip: bool,
) -> *mut ShadowMesh {
    if path_pts.is_null() || path_len < 3 {
        return std::ptr::null_mut();
    }
    let path_slice = std::slice::from_raw_parts(path_pts, path_len);
    let umbra_slice = if umbra_pts.is_null() || umbra_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(umbra_pts, umbra_len)
    };
    let clip_slice = if clip_pts.is_null() || clip_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(clip_pts, clip_len)
    };
    match shadow_tessellator::build_convex_shadow_mesh(
        path_slice,
        umbra_slice,
        valid_umbra,
        clip_slice,
        centroid,
        direction,
        outset,
        umbra_color,
        transparent,
        do_clip,
    ) {
        Some(mesh) => Box::into_raw(Box::new(mesh)),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_concave_clamp_inset(inset: f32, path_bounds: Rect) -> f32 {
    shadow_tessellator::concave_clamp_inset(inset, path_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_stitch_concave_rings(
    path_poly_len: usize,
    umbra_pts: *const Point,
    umbra_len: usize,
    umbra_indices: *mut i32,
    penumbra_pts: *const Point,
    penumbra_len: usize,
    penumbra_indices: *mut i32,
) -> *mut ShadowMesh {
    if umbra_pts.is_null()
        || umbra_indices.is_null()
        || penumbra_pts.is_null()
        || penumbra_indices.is_null()
        || umbra_len == 0
        || penumbra_len == 0
    {
        return std::ptr::null_mut();
    }
    let u_pts = std::slice::from_raw_parts(umbra_pts, umbra_len);
    let u_idx = std::slice::from_raw_parts_mut(umbra_indices, umbra_len);
    let p_pts = std::slice::from_raw_parts(penumbra_pts, penumbra_len);
    let p_idx = std::slice::from_raw_parts_mut(penumbra_indices, penumbra_len);
    let mesh =
        shadow_tessellator::stitch_concave_rings(path_poly_len, u_pts, u_idx, p_pts, p_idx);
    Box::into_raw(Box::new(mesh))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_mesh_extract(
    mesh: *const ShadowMesh,
    out_pos: *mut *const Point,
    out_colors: *mut *const u32,
    out_vert_count: *mut usize,
    out_indices: *mut *const u16,
    out_index_count: *mut usize,
    out_index_map: *mut *const u16,
    out_index_map_len: *mut usize,
) {
    if mesh.is_null() {
        return;
    }
    let m = &*mesh;
    if !out_pos.is_null() {
        *out_pos = m.positions.as_ptr();
    }
    if !out_colors.is_null() {
        *out_colors = m.colors.as_ptr();
    }
    if !out_vert_count.is_null() {
        *out_vert_count = m.positions.len();
    }
    if !out_indices.is_null() {
        *out_indices = m.indices.as_ptr();
    }
    if !out_index_count.is_null() {
        *out_index_count = m.indices.len();
    }
    if !out_index_map.is_null() {
        *out_index_map = m.index_map.as_ptr();
    }
    if !out_index_map_len.is_null() {
        *out_index_map_len = m.index_map.len();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_mesh_free(mesh: *mut ShadowMesh) {
    if !mesh.is_null() {
        drop(Box::from_raw(mesh));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_compute_tonal_colors(
    in_ambient: u32,
    in_spot: u32,
    out_ambient: *mut u32,
    out_spot: *mut u32,
) {
    let (amb, spot) = shadow_tessellator::compute_tonal_colors(in_ambient, in_spot);
    if !out_ambient.is_null() {
        *out_ambient = amb;
    }
    if !out_spot.is_null() {
        *out_spot = spot;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_fill_rec(
    z_plane: Point3,
    light_pos: Point3,
    light_radius: f32,
    ambient_color: u32,
    spot_color: u32,
    flags: u32,
    ctm: *const Matrix,
    out_rec: *mut DrawShadowRec,
) -> bool {
    if ctm.is_null() || out_rec.is_null() {
        return false;
    }
    match shadow_tessellator::fill_shadow_rec(
        z_plane,
        light_pos,
        light_radius,
        ambient_color,
        spot_color,
        flags,
        &*ctm,
    ) {
        Some(rec) => {
            *out_rec = rec;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_validate_rec(rec: *const DrawShadowRec) -> bool {
    if rec.is_null() {
        return false;
    }
    shadow_tessellator::validate_shadow_rec(&*rec)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_is_z_plane_tilted(z_plane: Point3) -> bool {
    shadow_tessellator::is_z_plane_tilted(z_plane)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_ambient_blur_fallback_params(
    z_plane_z: f32,
    out_blur_radius: *mut f32,
    out_stroke_width: *mut f32,
) {
    let (r, sw) = shadow_tessellator::ambient_blur_fallback_params(z_plane_z);
    if !out_blur_radius.is_null() {
        *out_blur_radius = r;
    }
    if !out_stroke_width.is_null() {
        *out_stroke_width = sw;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shadow_compute_spot_factory_params(
    path_bounds: Rect,
    path_is_convex: bool,
    view_matrix: *const Matrix,
    z_plane_z: f32,
    dev_light_pos: Point3,
    light_radius: f32,
    transparent: bool,
    directional: bool,
) -> SpotFactoryParams {
    if view_matrix.is_null() {
        return SpotFactoryParams {
            local_center: Point::new(0.0, 0.0),
            offset: Vector::new(0.0, 0.0),
            occluder_type: 0,
        };
    }
    shadow_tessellator::compute_spot_factory_params(
        path_bounds,
        path_is_convex,
        &*view_matrix,
        z_plane_z,
        dev_light_pos,
        light_radius,
        transparent,
        directional,
    )
}
