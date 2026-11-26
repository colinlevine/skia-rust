//! FFI exports for `skia-rs-imagefilters` (Phase 1 Layer 4.4).

use std::ffi::c_void;

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_geometry::{IPoint, IRect, ISize, Matrix, Point, Point3, Rect, Size, M44};
use skia_rs_imagefilters::{
    cache::{CacheKey, CacheKeyBytes, LruCacheIndex},
    core_filter::{self, LegacyCropResult, LocalMatrixDecision},
    effects::{
        self, BlendBoundsRule, BlendFactoryAction, BlendNullShaderAction, LightType,
        LightingUniforms, MagnifierFilterPlan, MagnifierValidation, MaterialType,
        MatrixConvKernelTier, MorphDirection, MorphPassStep, MorphType, MorphValidation,
    },
    filter_result::{
        self, AsShaderPlan, BlurPlan, ColorFilterPlan, CropPlan, FilterResultMeta, LowResBlurPlan,
        MakeFromImagePlan, RescaleSetup, RescaleStep, ResolvePlan, ShaderViewPlan, SubsetPlan,
        TiledBorderDraws, TransformPlan,
    },
    types::{
        self, BoundsScope, FVector, IVector, Mapping, MatrixCapability, PixelBoundary,
        SamplingOptions, TileMode,
    },
};

// ============================================================================
// skif::Mapping & LayerSpace geometric math
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_round_out(r: Rect) -> IRect {
    types::round_out(r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_round_in(r: Rect) -> IRect {
    types::round_in(r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_size_round(s: Size) -> ISize {
    types::size_round(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_size_ceil(s: Size) -> ISize {
    types::size_ceil(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_size_floor(s: Size) -> ISize {
    types::size_floor(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_is_nearly_integer_translation(
    m: *const Matrix,
    out_origin: *mut IPoint,
) -> bool {
    if m.is_null() {
        return false;
    }
    match types::is_nearly_integer_translation(&*m) {
        Some(pt) => {
            if !out_origin.is_null() {
                *out_origin = pt;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_decompose_ctm(
    mapping: *mut Mapping,
    ctm: *const M44,
    capability: i32,
    representative_pt: Point,
) -> bool {
    if mapping.is_null() || ctm.is_null() {
        return false;
    }
    let Some(cap) = MatrixCapability::from_i32(capability) else {
        return false;
    };
    (*mapping).decompose_ctm(&*ctm, cap, representative_pt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_adjust_layer_space(
    mapping: *mut Mapping,
    layer: *const M44,
) -> bool {
    if mapping.is_null() || layer.is_null() {
        return false;
    }
    (*mapping).adjust_layer_space(&*layer)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_rect(geom: Rect, matrix: *const Matrix) -> Rect {
    if matrix.is_null() {
        return Rect::make_empty();
    }
    types::map_rect(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_irect(
    geom: IRect,
    matrix: *const Matrix,
) -> IRect {
    if matrix.is_null() {
        return IRect::make_empty();
    }
    types::map_irect(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_ipoint(
    geom: IPoint,
    matrix: *const Matrix,
) -> IPoint {
    if matrix.is_null() {
        return IPoint { x: 0, y: 0 };
    }
    types::map_ipoint(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_point(
    geom: Point,
    matrix: *const Matrix,
) -> Point {
    if matrix.is_null() {
        return Point::new(0.0, 0.0);
    }
    types::map_point(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_vector(
    geom: FVector,
    matrix: *const Matrix,
) -> FVector {
    if matrix.is_null() {
        return FVector { x: 0.0, y: 0.0 };
    }
    types::map_vector(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_ivector(
    geom: IVector,
    matrix: *const Matrix,
) -> IVector {
    if matrix.is_null() {
        return IVector { x: 0, y: 0 };
    }
    types::map_ivector(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_size(geom: Size, matrix: *const Matrix) -> Size {
    if matrix.is_null() {
        return Size {
            width: 0.0,
            height: 0.0,
        };
    }
    types::map_size(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_isize(
    geom: ISize,
    matrix: *const Matrix,
) -> ISize {
    if matrix.is_null() {
        return ISize {
            width: 0,
            height: 0,
        };
    }
    types::map_isize(geom, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_mapping_map_matrix(
    m: *const Matrix,
    matrix: *const Matrix,
    out: *mut Matrix,
) {
    if m.is_null() || matrix.is_null() || out.is_null() {
        return;
    }
    *out = types::map_matrix(&*m, &*matrix);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_inverse_map_rect(
    m: *const Matrix,
    r: Rect,
    out: *mut Rect,
) -> bool {
    if m.is_null() || out.is_null() {
        return false;
    }
    match types::inverse_map_rect(&*m, r) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_inverse_map_irect(
    m: *const Matrix,
    r: IRect,
    out: *mut IRect,
) -> bool {
    if m.is_null() || out.is_null() {
        return false;
    }
    match types::inverse_map_irect(&*m, r) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_relevant_subset(
    src: IRect,
    dst: IRect,
    tile_mode: i32,
) -> IRect {
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    types::relevant_subset(src, dst, tm)
}

// ============================================================================
// FilterResult & Builder planning
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_auto_surface_padded_bounds(
    dst_bounds: IRect,
    boundary: i32,
    out_padded: *mut IRect,
) -> bool {
    if out_padded.is_null() {
        return false;
    }
    let pb = PixelBoundary::from_i32(boundary).unwrap_or(PixelBoundary::Unknown);
    match filter_result::auto_surface_padded_bounds(dst_bounds, pb) {
        Some(r) => {
            *out_padded = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_analyze_bounds(
    meta: *const FilterResultMeta,
    xtra_transform: *const Matrix,
    dst_bounds: IRect,
    scope: i32,
) -> u32 {
    if meta.is_null() || xtra_transform.is_null() {
        return 0;
    }
    let sc = BoundsScope::from_i32(scope).unwrap_or(BoundsScope::Deferred);
    filter_result::analyze_bounds(&*meta, &*xtra_transform, dst_bounds, sc)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_apply_crop(
    meta: *const FilterResultMeta,
    desired_output: IRect,
    crop: IRect,
    tile_mode: i32,
    out_plan: *mut CropPlan,
) {
    if meta.is_null() || out_plan.is_null() {
        return;
    }
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    *out_plan = filter_result::plan_apply_crop(&*meta, desired_output, crop, tm);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_apply_color_filter(
    meta: *const FilterResultMeta,
    desired_output: IRect,
    new_cf_affects_transparent_black: bool,
) -> ColorFilterPlan {
    if meta.is_null() {
        return ColorFilterPlan {
            action: filter_result::ColorFilterAction::Empty,
            bounds: IRect::make_empty(),
        };
    }
    filter_result::plan_apply_color_filter(&*meta, desired_output, new_cf_affects_transparent_black)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_apply_transform(
    meta: *const FilterResultMeta,
    desired_output: IRect,
    transform: *const Matrix,
    sampling: SamplingOptions,
    out_plan: *mut TransformPlan,
) {
    if meta.is_null() || transform.is_null() || out_plan.is_null() {
        return;
    }
    *out_plan = filter_result::plan_apply_transform(&*meta, desired_output, &*transform, sampling);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_resolve(
    meta: *const FilterResultMeta,
    dst_bounds: IRect,
    preserve_dst_bounds: bool,
) -> ResolvePlan {
    if meta.is_null() {
        return ResolvePlan {
            action: filter_result::ResolveAction::Empty,
            dst_bounds: IRect::make_empty(),
            origin: IPoint { x: 0, y: 0 },
            boundary: PixelBoundary::Unknown,
        };
    }
    filter_result::plan_resolve(&*meta, dst_bounds, preserve_dst_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_subset(
    image_dims: ISize,
    known_origin: IPoint,
    subset_bounds: IRect,
    clamp_src_if_disjoint: bool,
) -> SubsetPlan {
    filter_result::plan_subset(image_dims, known_origin, subset_bounds, clamp_src_if_disjoint)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_subset_pixel_boundary(
    orig_subset: IRect,
    result_subset: IRect,
    orig_boundary: i32,
) -> i32 {
    let pb = PixelBoundary::from_i32(orig_boundary).unwrap_or(PixelBoundary::Unknown);
    filter_result::subset_pixel_boundary(orig_subset, result_subset, pb) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_as_shader(
    meta: *const FilterResultMeta,
    xtra_sampling: SamplingOptions,
    flags: u32,
    sample_bounds: IRect,
    cf_is_simple_coeff_mode: bool,
    color_spaces_equal: bool,
    out_plan: *mut AsShaderPlan,
) {
    if meta.is_null() || out_plan.is_null() {
        return;
    }
    *out_plan = filter_result::plan_as_shader(
        &*meta,
        xtra_sampling,
        flags,
        sample_bounds,
        cf_is_simple_coeff_mode,
        color_spaces_equal,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_shader_view(
    meta: *const FilterResultMeta,
    analysis: u32,
    out_plan: *mut ShaderViewPlan,
) {
    if meta.is_null() || out_plan.is_null() {
        return;
    }
    *out_plan = filter_result::plan_shader_view(&*meta, analysis);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_compute_rescale_setup(
    meta: *const FilterResultMeta,
    desired_output: IRect,
    scale: Size,
    enforce_decal: bool,
    allow_overscaling: bool,
    color_type_matches: bool,
    color_space_matches: bool,
    out_setup: *mut RescaleSetup,
) {
    if meta.is_null() || out_setup.is_null() {
        return;
    }
    *out_setup = filter_result::compute_rescale_setup(
        &*meta,
        desired_output,
        scale,
        enforce_decal,
        allow_overscaling,
        color_type_matches,
        color_space_matches,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_compute_rescale_step(
    src_rect: IRect,
    step_bounds_f: Rect,
    x_steps: i32,
    y_steps: i32,
    final_scale_x: f32,
    final_scale_y: f32,
    tile_mode: i32,
    defer_periodic_tiling: bool,
    out_step: *mut RescaleStep,
) {
    if out_step.is_null() {
        return;
    }
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    *out_step = filter_result::compute_rescale_step(
        src_rect,
        step_bounds_f,
        x_steps,
        y_steps,
        final_scale_x,
        final_scale_y,
        tm,
        defer_periodic_tiling,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_compute_cf_border_rects(border: IRect, out_rects: *mut IRect) {
    if out_rects.is_null() {
        return;
    }
    let rects = filter_result::compute_cf_border_rects(border);
    std::slice::from_raw_parts_mut(out_rects, 4).copy_from_slice(&rects);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_compute_tiled_border_draws(
    tile_mode: i32,
    src_to_dst: *const Matrix,
    src_border: Rect,
    dst_border: Rect,
    out_draws: *mut TiledBorderDraws,
) {
    if src_to_dst.is_null() || out_draws.is_null() {
        return;
    }
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Clamp);
    *out_draws =
        filter_result::compute_tiled_border_draws(tm, &*src_to_dst, src_border, dst_border);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_make_from_image(
    mapping: *const Mapping,
    desired_output: IRect,
    image_dims: ISize,
    src_rect: Rect,
    dst_rect: Rect,
    out_plan: *mut MakeFromImagePlan,
) {
    if mapping.is_null() || out_plan.is_null() {
        return;
    }
    *out_plan = filter_result::plan_make_from_image(
        &*mapping,
        desired_output,
        image_dims,
        src_rect,
        dst_rect,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_builder_blur(
    desired_output: IRect,
    input_layer_bounds: IRect,
    sigma: Size,
    max_sigma: f32,
    out_plan: *mut BlurPlan,
) {
    if out_plan.is_null() {
        return;
    }
    *out_plan = filter_result::plan_builder_blur(desired_output, input_layer_bounds, sigma, max_sigma);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_plan_low_res_blur(
    sigma: Size,
    max_sigma: f32,
    supports_only_decal: bool,
    output_bounds: IRect,
    low_res_dims: ISize,
    low_res_transform: *const Matrix,
    low_res_tile_mode: i32,
    low_res_boundary: i32,
    out_plan: *mut LowResBlurPlan,
) {
    if low_res_transform.is_null() || out_plan.is_null() {
        return;
    }
    let tm = TileMode::from_i32(low_res_tile_mode).unwrap_or(TileMode::Decal);
    let pb = PixelBoundary::from_i32(low_res_boundary).unwrap_or(PixelBoundary::Unknown);
    *out_plan = filter_result::plan_low_res_blur(
        sigma,
        max_sigma,
        supports_only_decal,
        output_bounds,
        low_res_dims,
        &*low_res_transform,
        tm,
        pb,
    );
}

// ============================================================================
// SkImageFilterCache LRU Index
// ============================================================================

pub struct SkiaRsIfCache(LruCacheIndex);

pub type SkiaRsIfCacheEvictFn = extern "C" fn(*mut c_void, u64);

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_create(max_bytes: usize) -> *mut SkiaRsIfCache {
    Box::into_raw(Box::new(SkiaRsIfCache(LruCacheIndex::new(max_bytes))))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_destroy(cache: *mut SkiaRsIfCache) {
    if !cache.is_null() {
        drop(Box::from_raw(cache));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_count(cache: *const SkiaRsIfCache) -> i32 {
    if cache.is_null() {
        return 0;
    }
    (*cache).0.count() as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_get(
    cache: *mut SkiaRsIfCache,
    key: *const CacheKey,
    out_handle: *mut u64,
) -> bool {
    if cache.is_null() || key.is_null() || out_handle.is_null() {
        return false;
    }
    let kb = CacheKeyBytes::from_key(&*key);
    match (*cache).0.get(&kb) {
        Some(h) => {
            *out_handle = h;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_set(
    cache: *mut SkiaRsIfCache,
    key: *const CacheKey,
    filter_tag: usize,
    bytes: usize,
    evict_cb: Option<SkiaRsIfCacheEvictFn>,
    user_data: *mut c_void,
) -> u64 {
    if cache.is_null() || key.is_null() {
        return 0;
    }
    let kb = CacheKeyBytes::from_key(&*key);
    let (handle, evicted) = (*cache).0.set(kb, filter_tag, bytes);
    if let Some(cb) = evict_cb {
        for h in evicted {
            cb(user_data, h);
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_purge(
    cache: *mut SkiaRsIfCache,
    evict_cb: Option<SkiaRsIfCacheEvictFn>,
    user_data: *mut c_void,
) {
    if cache.is_null() {
        return;
    }
    let evicted = (*cache).0.purge();
    if let Some(cb) = evict_cb {
        for h in evicted {
            cb(user_data, h);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_cache_purge_by_filter(
    cache: *mut SkiaRsIfCache,
    filter_tag: usize,
    evict_cb: Option<SkiaRsIfCacheEvictFn>,
    user_data: *mut c_void,
) {
    if cache.is_null() {
        return;
    }
    let evicted = (*cache).0.purge_by_filter(filter_tag);
    if let Some(cb) = evict_cb {
        for h in evicted {
            cb(user_data, h);
        }
    }
}

// ============================================================================
// SkImageFilter / SkImageFilter_Base / SkLocalMatrixImageFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_next_unique_id() -> u64 {
    core_filter::next_image_filter_unique_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_validate_common_input_count(
    count: i32,
    expected_count: i32,
) -> bool {
    core_filter::validate_common_input_count(count, expected_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_validate_legacy_crop(
    rect: Rect,
    flags: u32,
) -> LegacyCropResult {
    core_filter::validate_legacy_crop(rect, flags)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_child_input_layer_bounds_null(
    desired_output: IRect,
    has_content_bounds: bool,
    content_bounds: IRect,
) -> IRect {
    let cb = if has_content_bounds {
        Some(content_bounds)
    } else {
        None
    };
    core_filter::child_input_layer_bounds_null(desired_output, cb)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_combine_ctm_capability(a: i32, b: i32) -> i32 {
    let ca = MatrixCapability::from_i32(a).unwrap_or(MatrixCapability::Translate);
    let cb = MatrixCapability::from_i32(b).unwrap_or(MatrixCapability::Translate);
    core_filter::combine_ctm_capability(ca, cb) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_validate_local_matrix(
    local_matrix: *const Matrix,
    input_capability: i32,
    out_decision: *mut LocalMatrixDecision,
) {
    if local_matrix.is_null() || out_decision.is_null() {
        return;
    }
    let cap = MatrixCapability::from_i32(input_capability).unwrap_or(MatrixCapability::Translate);
    *out_decision = core_filter::validate_local_matrix(&*local_matrix, cap);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_local_matrix_map_rect(m: *const Matrix, r: Rect) -> Rect {
    if m.is_null() {
        return Rect::make_empty();
    }
    core_filter::local_matrix_map_rect(&*m, r)
}

// ============================================================================
// Effect Image Filters (16 filters)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blend_classify_factory(mode: i32) -> BlendFactoryAction {
    match BlendMode::from_i32(mode) {
        Some(bm) => effects::classify_blend_factory(bm),
        None => BlendFactoryAction::Keep,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blend_affects_transparent_black(
    has_blend_mode: bool,
    has_arith: bool,
    k3: f32,
) -> bool {
    effects::blend_affects_transparent_black(has_blend_mode, has_arith, k3)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blend_null_shader_action(
    has_bg: bool,
    has_fg: bool,
    affects_transparent_black: bool,
    has_blend_mode: bool,
    mode: i32,
) -> BlendNullShaderAction {
    let bm = if has_blend_mode {
        BlendMode::from_i32(mode)
    } else {
        None
    };
    effects::blend_null_shader_action(has_bg, has_fg, affects_transparent_black, bm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blend_classify_bounds_rule(
    has_blend_mode: bool,
    mode: i32,
    has_arith: bool,
    arith_k: *const f32,
    include_sc: bool,
) -> BlendBoundsRule {
    let bm = if has_blend_mode {
        BlendMode::from_i32(mode)
    } else {
        None
    };
    let k = if has_arith && !arith_k.is_null() {
        let s = std::slice::from_raw_parts(arith_k, 4);
        Some([s[0], s[1], s[2], s[3]])
    } else {
        None
    };
    effects::classify_blend_bounds_rule(bm, k, include_sc)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blend_combine_output_bounds(
    rule: BlendBoundsRule,
    has_fg: bool,
    fg: IRect,
    has_bg: bool,
    bg: IRect,
    out_rect: *mut IRect,
) -> bool {
    if out_rect.is_null() {
        return false;
    }
    let fg_opt = if has_fg { Some(fg) } else { None };
    let bg_opt = if has_bg { Some(bg) } else { None };
    match effects::combine_blend_output_bounds(rule, fg_opt, bg_opt) {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blend_combine_fast_bounds(
    rule: BlendBoundsRule,
    fg: Rect,
    bg: Rect,
) -> Rect {
    effects::combine_blend_fast_bounds(rule, fg, bg)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blur_validate(sigma_x: f32, sigma_y: f32) -> bool {
    effects::validate_blur_params(sigma_x, sigma_y)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blur_map_sigma(
    mapping: *const Mapping,
    param_sigma: Size,
) -> Size {
    if mapping.is_null() {
        return Size {
            width: 0.0,
            height: 0.0,
        };
    }
    effects::map_blur_sigma(&*mapping, param_sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blur_kernel_bounds(
    mapping: *const Mapping,
    param_sigma: Size,
    bounds: IRect,
) -> IRect {
    if mapping.is_null() {
        return bounds;
    }
    effects::blur_kernel_bounds(&*mapping, param_sigma, bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_blur_fast_bounds(child_bounds: Rect, param_sigma: Size) -> Rect {
    effects::blur_fast_bounds(child_bounds, param_sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_crop_validate_rect(rect: Rect) -> bool {
    effects::validate_crop_rect(rect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_crop_layer_rect(
    mapping: *const Mapping,
    param_crop: Rect,
    tile_mode: i32,
) -> IRect {
    if mapping.is_null() {
        return IRect::make_empty();
    }
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    effects::crop_layer_rect(&*mapping, param_crop, tm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_crop_required_input(
    mapping: *const Mapping,
    param_crop: Rect,
    tile_mode: i32,
    output_bounds: IRect,
) -> IRect {
    if mapping.is_null() {
        return IRect::make_empty();
    }
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    effects::crop_required_input(&*mapping, param_crop, tm, output_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_crop_output_bounds(
    mapping: *const Mapping,
    param_crop: Rect,
    tile_mode: i32,
    has_child: bool,
    child_output: IRect,
    out_rect: *mut IRect,
) -> bool {
    if mapping.is_null() || out_rect.is_null() {
        return false;
    }
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    let co = if has_child { Some(child_output) } else { None };
    match effects::crop_output_layer_bounds(&*mapping, param_crop, tm, co) {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_crop_fast_bounds(
    input_bounds: Rect,
    param_crop: Rect,
    tile_mode: i32,
) -> Rect {
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Decal);
    effects::crop_fast_bounds(input_bounds, param_crop, tm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_displacement_validate(
    x_channel: i32,
    y_channel: i32,
    scale: f32,
) -> bool {
    effects::validate_displacement_params(x_channel, y_channel, scale)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_displacement_channel_selector(channel: i32, out_v4: *mut f32) {
    if out_v4.is_null() {
        return;
    }
    let v = effects::displacement_channel_selector(channel);
    std::slice::from_raw_parts_mut(out_v4, 4).copy_from_slice(&v);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_displacement_outset_bounds(
    mapping: *const Mapping,
    scale: f32,
    bounds: IRect,
) -> IRect {
    if mapping.is_null() {
        return bounds;
    }
    effects::displacement_outset_bounds(&*mapping, scale, bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_displacement_layer_scale(
    mapping: *const Mapping,
    scale: f32,
) -> FVector {
    if mapping.is_null() {
        return FVector { x: 0.0, y: 0.0 };
    }
    effects::displacement_layer_scale(&*mapping, scale)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_displacement_fast_bounds(
    color_bounds: Rect,
    scale: f32,
) -> Rect {
    effects::displacement_fast_bounds(color_bounds, scale)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_image_adjust_rects(
    image_dims: ISize,
    src_rect: Rect,
    dst_rect: Rect,
    out_src: *mut Rect,
    out_dst: *mut Rect,
) -> bool {
    if out_src.is_null() || out_dst.is_null() {
        return false;
    }
    match effects::adjust_image_filter_rects(image_dims, src_rect, dst_rect) {
        Some((s, d)) => {
            *out_src = s;
            *out_dst = d;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_lighting_validate(
    k: f32,
    shininess: f32,
    surface_depth: f32,
    loc_xy: Point,
    loc_z: f32,
    dir_xy: FVector,
    dir_z: f32,
    falloff_exponent: f32,
    cos_cutoff_angle: f32,
) -> bool {
    effects::validate_lighting_params(
        k,
        shininess,
        surface_depth,
        loc_xy,
        loc_z,
        dir_xy,
        dir_z,
        falloff_exponent,
        cos_cutoff_angle,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_lighting_spot_dir_and_cos(
    location: Point3,
    target: Point3,
    cutoff_angle_deg: f32,
    out_dir: *mut Point3,
) -> f32 {
    let (dir, cos_c) = effects::spot_light_dir_and_cos_cutoff(location, target, cutoff_angle_deg);
    if !out_dir.is_null() {
        *out_dir = dir;
    }
    cos_c
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_lighting_clamp_rect(
    child_layer_bounds: IRect,
    required_input: IRect,
    desired_output: IRect,
) -> IRect {
    effects::lighting_clamp_rect(child_layer_bounds, required_input, desired_output)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_lighting_required_input(desired_output: IRect) -> IRect {
    effects::lighting_required_input(desired_output)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_lighting_compute_uniforms(
    mapping: *const Mapping,
    clamp_rect: IRect,
    light_type: i32,
    light_color_argb: u32,
    loc_xy: Point,
    loc_z: f32,
    dir_xy: FVector,
    dir_z: f32,
    falloff_exponent: f32,
    cos_cutoff_angle: f32,
    mat_type: i32,
    surface_depth: f32,
    k: f32,
    shininess: f32,
    out_uniforms: *mut LightingUniforms,
) {
    if mapping.is_null() || out_uniforms.is_null() {
        return;
    }
    let lt = match light_type {
        0 => LightType::Distant,
        1 => LightType::Point,
        _ => LightType::Spot,
    };
    let mt = match mat_type {
        0 => MaterialType::Diffuse,
        1 => MaterialType::Specular,
        _ => MaterialType::EmbossSpecular,
    };
    *out_uniforms = effects::compute_lighting_uniforms(
        &*mapping,
        clamp_rect,
        lt,
        light_color_argb,
        loc_xy,
        loc_z,
        dir_xy,
        dir_z,
        falloff_exponent,
        cos_cutoff_angle,
        mt,
        surface_depth,
        k,
        shininess,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_magnifier_validate(
    lens_bounds: Rect,
    zoom_amount: f32,
    inset: f32,
) -> MagnifierValidation {
    effects::validate_magnifier_params(lens_bounds, zoom_amount, inset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_magnifier_plan_filter(
    mapping: *const Mapping,
    desired_output: IRect,
    param_lens_bounds: Rect,
    zoom_amount: f32,
    param_inset: f32,
    has_child_output: bool,
    child_output: IRect,
    out_plan: *mut MagnifierFilterPlan,
) {
    if mapping.is_null() || out_plan.is_null() {
        return;
    }
    let co = if has_child_output {
        Some(child_output)
    } else {
        None
    };
    *out_plan = effects::plan_magnifier_filter(
        &*mapping,
        desired_output,
        param_lens_bounds,
        zoom_amount,
        param_inset,
        co,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_magnifier_output_bounds(
    mapping: *const Mapping,
    param_lens_bounds: Rect,
    has_child_output: bool,
    child_output: IRect,
) -> IRect {
    if mapping.is_null() {
        return IRect::make_empty();
    }
    let co = if has_child_output {
        Some(child_output)
    } else {
        None
    };
    effects::magnifier_output_bounds(&*mapping, param_lens_bounds, co)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_magnifier_fast_bounds(
    child_bounds: Rect,
    param_lens_bounds: Rect,
) -> Rect {
    effects::magnifier_fast_bounds(child_bounds, param_lens_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matconv_validate(
    kernel_size: ISize,
    has_kernel: bool,
    kernel_offset: IPoint,
) -> bool {
    effects::validate_matrix_convolution_params(kernel_size, has_kernel, kernel_offset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matconv_quantize(
    kernel_length: i32,
    out_quantized_size: *mut i32,
) -> MatrixConvKernelTier {
    let (sz, tier) = effects::quantize_matrix_conv_kernel(kernel_length);
    if !out_quantized_size.is_null() {
        *out_quantized_size = sz;
    }
    tier
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matconv_encode_kernel_a8(
    kernel: *const f32,
    kernel_len: usize,
    out_pixels: *mut u8,
    quantized_size: usize,
    out_inner_gain: *mut f32,
    out_inner_bias: *mut f32,
) {
    if kernel.is_null() || out_pixels.is_null() {
        return;
    }
    let kslice = std::slice::from_raw_parts(kernel, kernel_len);
    let pslice = std::slice::from_raw_parts_mut(out_pixels, quantized_size);
    let (gain, bias) = effects::encode_matrix_conv_kernel_a8(kslice, pslice);
    if !out_inner_gain.is_null() {
        *out_inner_gain = gain;
    }
    if !out_inner_bias.is_null() {
        *out_inner_bias = bias;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matconv_bounds_sampled(
    bounds: IRect,
    kernel_size: ISize,
    kernel_offset: IPoint,
) -> IRect {
    effects::matrix_conv_bounds_sampled(bounds, kernel_size, kernel_offset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matconv_bounds_affected(
    bounds: IRect,
    kernel_size: ISize,
    kernel_offset: IPoint,
) -> IRect {
    effects::matrix_conv_bounds_affected(bounds, kernel_size, kernel_offset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matconv_gain_and_bias(gain: f32, bias: f32, out_v2: *mut f32) {
    if out_v2.is_null() {
        return;
    }
    let v = effects::matrix_conv_gain_and_bias(gain, bias);
    std::slice::from_raw_parts_mut(out_v2, 2).copy_from_slice(&v);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matxform_required_input(
    mapping: *const Mapping,
    param_transform: *const Matrix,
    desired_output: IRect,
    is_default_nearest: bool,
) -> IRect {
    if mapping.is_null() || param_transform.is_null() {
        return IRect::make_empty();
    }
    effects::matrix_transform_required_input(
        &*mapping,
        &*param_transform,
        desired_output,
        is_default_nearest,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_matxform_output_bounds(
    mapping: *const Mapping,
    param_transform: *const Matrix,
    has_child: bool,
    child_output: IRect,
    out_rect: *mut IRect,
) -> bool {
    if mapping.is_null() || param_transform.is_null() || out_rect.is_null() {
        return false;
    }
    let co = if has_child { Some(child_output) } else { None };
    match effects::matrix_transform_output_bounds(&*mapping, &*param_transform, co) {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_morph_validate(radii: Size) -> MorphValidation {
    effects::validate_morphology_params(radii)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_morph_map_radii(
    mapping: *const Mapping,
    param_radii: Size,
) -> ISize {
    if mapping.is_null() {
        return ISize {
            width: 0,
            height: 0,
        };
    }
    effects::morphology_map_radii(&*mapping, param_radii)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_morph_required_input(
    mapping: *const Mapping,
    param_radii: Size,
    bounds: IRect,
) -> IRect {
    if mapping.is_null() {
        return bounds;
    }
    effects::morphology_required_input(&*mapping, param_radii, bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_morph_kernel_output_bounds(
    morph_type: i32,
    mapping: *const Mapping,
    param_radii: Size,
    bounds: IRect,
) -> IRect {
    if mapping.is_null() {
        return bounds;
    }
    let mt = if morph_type == 0 {
        MorphType::Erode
    } else {
        MorphType::Dilate
    };
    effects::morphology_kernel_output_bounds(mt, &*mapping, param_radii, bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_morph_fast_bounds(
    morph_type: i32,
    param_radii: Size,
    child_bounds: Rect,
) -> Rect {
    let mt = if morph_type == 0 {
        MorphType::Erode
    } else {
        MorphType::Dilate
    };
    effects::morphology_fast_bounds(mt, param_radii, child_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_morph_plan_passes(
    dir: i32,
    radius: i32,
    desired_output: IRect,
    out_steps: *mut MorphPassStep,
    max_steps: usize,
) -> usize {
    let md = if dir == 0 {
        MorphDirection::X
    } else {
        MorphDirection::Y
    };
    let steps = effects::plan_morphology_passes(md, radius, desired_output);
    let n = steps.len().min(max_steps);
    if !out_steps.is_null() && n > 0 {
        std::slice::from_raw_parts_mut(out_steps, n).copy_from_slice(&steps[..n]);
    }
    steps.len()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_if_picture_intersect_cull_rect(
    pic_cull: Rect,
    target_rect: Rect,
    out_rect: *mut Rect,
) -> bool {
    if out_rect.is_null() {
        return false;
    }
    match effects::picture_intersect_cull_rect(pic_cull, target_rect) {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}
