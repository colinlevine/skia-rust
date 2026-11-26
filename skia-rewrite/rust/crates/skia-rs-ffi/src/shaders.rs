//! FFI exports for `skia-rs-shaders` (Phase 1 Layer 4.1).

use skia_rs_color::{
    blend_mode::BlendMode,
    color::Color4f,
    image_info::{AlphaType, ColorType},
};
use skia_rs_geometry::{ISize, Matrix, Point, Rect, Size, M44};
use skia_rs_shaders::{
    blend_shader::{self, BlendShaderAction},
    compose_shaders::{
        self, ColorFilterShaderAction, ColorFilterShaderStagePlan, CoordClampCtx,
        TriColorUpdateResult, WorkingColorSpaceAction,
    },
    gainmap::{self, GainmapInfoInput, GainmapUniforms},
    gradient_base::{
        self, DegenerateGradientResult, GradientInterpolation, IntermediateColorSpaceKind,
        InterpolatedToDstPlan, InterpolationColorSpace, UnpackedGradientFlags,
    },
    gradients::{
        self, ConicalCreateResult, ConicalFocalStagePlan, FocalData, LinearGradientAction,
        RadialGradientAction, SweepAngles, SweepGradientPlan, SweepTCoeff, TwoPointConicalAction,
    },
    image_shader::{
        self, DrawRectGeometry, ImageShaderFastPath, ImageShaderMakeAction, MipLevelInitParams,
    },
    perlin_noise::{self, PaintingDataTables, PerlinNoiseAction, PerlinNoiseType},
    picture_shader::{self, PictureCachedImageParams},
    shader_base::{self, MatrixRec, MatrixRecApplyResult, MatrixRecFpResult},
    SamplingOptions, TileMode,
};

// ============================================================================
// SkShader / SkShaderBase / SkColorShader / SkLocalMatrixShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shader_concat_local_matrices(
    parent_lm: *const Matrix,
    child_lm: *const Matrix,
    android_framework_order: bool,
    out: *mut Matrix,
) {
    if parent_lm.is_null() || child_lm.is_null() || out.is_null() {
        return;
    }
    *out = shader_base::concat_local_matrices(&*parent_lm, &*child_lm, android_framework_order);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_rec_init(ctm: *const Matrix, out: *mut MatrixRec) {
    if ctm.is_null() || out.is_null() {
        return;
    }
    *out = MatrixRec::from_ctm(*ctm);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_rec_apply(
    rec: *const MatrixRec,
    post_inv: *const Matrix,
    out: *mut MatrixRecApplyResult,
) {
    if rec.is_null() || post_inv.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).apply(&*post_inv);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_rec_apply_for_fp(
    rec: *const MatrixRec,
    post_inv: *const Matrix,
    out: *mut MatrixRecFpResult,
) {
    if rec.is_null() || post_inv.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).apply_for_fragment_processor(&*post_inv);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_rec_applied(rec: *const MatrixRec, out: *mut MatrixRec) {
    if rec.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).applied();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_rec_concat(
    rec: *const MatrixRec,
    m: *const Matrix,
    android_framework_order: bool,
    out: *mut MatrixRec,
) {
    if rec.is_null() || m.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).concat(&*m, android_framework_order);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shader_finalize_luminance_color(color: Color4f) -> Color4f {
    shader_base::finalize_luminance_color(color)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shader_context_matrix_is_eligible(
    total_matrix: *const Matrix,
) -> bool {
    if total_matrix.is_null() {
        return false;
    }
    shader_base::context_matrix_is_eligible(&*total_matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_shader_validate_and_pin(
    color: Color4f,
    out_pinned: *mut Color4f,
) -> bool {
    if out_pinned.is_null() {
        return false;
    }
    match shader_base::color_shader_validate_and_pin(color) {
        Some(c) => {
            *out_pinned = c;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_shader_from_sk_color(color: u32) -> Color4f {
    shader_base::color_shader_from_sk_color(color)
}

// ============================================================================
// SkBlendShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_shader_classify(
    mode: i32,
    has_dst: bool,
    has_src: bool,
) -> BlendShaderAction {
    blend_shader::classify_blend_shader(mode, has_dst, has_src)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_shader_is_opaque(
    mode: i32,
    src_is_opaque: bool,
    dst_is_opaque: bool,
) -> bool {
    match BlendMode::from_i32(mode) {
        Some(m) => blend_shader::blend_shader_is_opaque(m, src_is_opaque, dst_is_opaque),
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_shader_is_valid_mode(mode: u32) -> bool {
    blend_shader::is_valid_blend_mode(mode)
}

// ============================================================================
// SkColorFilterShader / SkCoordClampShader / SkWorkingColorSpaceShader /
// SkTransformShader / SkTriColorShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_shader_classify(
    has_shader: bool,
    has_filter: bool,
) -> ColorFilterShaderAction {
    compose_shaders::classify_color_filter_shader(has_shader, has_filter)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_shader_is_opaque(
    shader_is_opaque: bool,
    alpha: f32,
    filter_alpha_unchanged: bool,
) -> bool {
    compose_shaders::color_filter_shader_is_opaque(shader_is_opaque, alpha, filter_alpha_unchanged)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_shader_plan_stages(
    alpha: f32,
    shader_is_opaque: bool,
) -> ColorFilterShaderStagePlan {
    compose_shaders::plan_color_filter_shader_stages(alpha, shader_is_opaque)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_coord_clamp_is_valid(has_shader: bool, subset: Rect) -> bool {
    compose_shaders::coord_clamp_is_valid(has_shader, subset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_coord_clamp_make_ctx(subset: Rect) -> CoordClampCtx {
    compose_shaders::make_coord_clamp_ctx(subset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_working_cs_shader_classify(
    has_shader: bool,
    has_input_cs: bool,
    has_output_cs: bool,
    work_in_unpremul: bool,
) -> WorkingColorSpaceAction {
    compose_shaders::classify_working_color_space_shader(
        has_shader,
        has_input_cs,
        has_output_cs,
        work_in_unpremul,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_working_cs_make_opaque(color: Color4f) -> Color4f {
    compose_shaders::make_opaque_color4f(color)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_transform_shader_update(
    allow_perspective: bool,
    matrix: *const Matrix,
    out_storage: *mut f32,
) -> bool {
    if matrix.is_null() || out_storage.is_null() {
        return false;
    }
    match compose_shaders::transform_shader_update(allow_perspective, &*matrix) {
        Some(arr) => {
            std::slice::from_raw_parts_mut(out_storage, 9).copy_from_slice(&arr);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_tri_color_shader_update(
    use_persp: bool,
    ctm_inv: *const Matrix,
    p0: Point,
    p1: Point,
    p2: Point,
    c0: Color4f,
    c1: Color4f,
    c2: Color4f,
    out: *mut TriColorUpdateResult,
) {
    if ctm_inv.is_null() || out.is_null() {
        return;
    }
    *out = compose_shaders::tri_color_update(use_persp, &*ctm_inv, p0, p1, p2, c0, c1, c2);
}

// ============================================================================
// SkImageShader / SkBitmapProcShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_cubic_matrix(b: f32, c: f32, out: *mut M44) {
    if out.is_null() {
        return;
    }
    *out = image_shader::cubic_resampler_matrix(b, c);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_cubic_weights(b: f32, c: f32, out_weights: *mut f32) {
    if out_weights.is_null() {
        return;
    }
    let w = image_shader::cubic_resampler_weights_col_major(b, c);
    std::slice::from_raw_parts_mut(out_weights, 16).copy_from_slice(&w);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_optimize_tile_mode(
    tm: i32,
    dimension: i32,
    android_framework: bool,
) -> i32 {
    match TileMode::from_i32(tm) {
        Some(t) => image_shader::optimize_tile_mode(t, dimension, android_framework) as i32,
        None => tm,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_classify_make_subset(
    has_image: bool,
    img_w: i32,
    img_h: i32,
    subset: Rect,
    sampling: *const SamplingOptions,
) -> ImageShaderMakeAction {
    if sampling.is_null() {
        return ImageShaderMakeAction::ReturnNull;
    }
    image_shader::classify_make_subset(has_image, img_w, img_h, subset, &*sampling)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_classify_make_raw(
    has_image: bool,
    use_cubic: bool,
) -> ImageShaderMakeAction {
    image_shader::classify_make_raw(has_image, use_cubic)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_is_opaque(
    img_is_opaque: bool,
    tmx: i32,
    tmy: i32,
) -> bool {
    let x = TileMode::from_i32(tmx).unwrap_or(TileMode::Clamp);
    let y = TileMode::from_i32(tmy).unwrap_or(TileMode::Clamp);
    image_shader::image_shader_is_opaque(img_is_opaque, x, y)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_aniso_fallback(
    image_is_mipped: bool,
    out: *mut SamplingOptions,
) {
    if out.is_null() {
        return;
    }
    *out = image_shader::aniso_fallback(image_is_mipped);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_tweak_sampling(
    sampling: *const SamplingOptions,
    matrix: *const Matrix,
    out: *mut SamplingOptions,
) {
    if sampling.is_null() || matrix.is_null() || out.is_null() {
        return;
    }
    *out = image_shader::tweak_sampling(*sampling, &*matrix);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_draw_rect_geometry(
    img_w: i32,
    img_h: i32,
    img_ct: i32,
    src: Rect,
    dst: Rect,
    out: *mut DrawRectGeometry,
) {
    if out.is_null() {
        return;
    }
    let ct = ColorType::from_i32(img_ct).unwrap_or(ColorType::Unknown);
    *out = image_shader::compute_draw_rect_geometry(img_w, img_h, ct, src, dst);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_mip_level_init(
    pm_width: i32,
    pm_height: i32,
    sampling: *const SamplingOptions,
    tmx: i32,
    tmy: i32,
    out: *mut MipLevelInitParams,
) {
    if sampling.is_null() || out.is_null() {
        return;
    }
    let x = TileMode::from_i32(tmx).unwrap_or(TileMode::Clamp);
    let y = TileMode::from_i32(tmy).unwrap_or(TileMode::Clamp);
    *out = image_shader::compute_mip_level_init(pm_width, pm_height, &*sampling, x, y);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_classify_fast_path(
    ct: i32,
    sampling: *const SamplingOptions,
    tmx: i32,
    tmy: i32,
    dst_bounds: Rect,
    base_inv: *const Matrix,
    coords_already_seeded: bool,
    total_matrix_is_valid: bool,
    total_matrix: *const Matrix,
    total_inv: *const Matrix,
) -> ImageShaderFastPath {
    if sampling.is_null() || base_inv.is_null() || total_matrix.is_null() || total_inv.is_null() {
        return ImageShaderFastPath::None;
    }
    let color_type = ColorType::from_i32(ct).unwrap_or(ColorType::Unknown);
    let x = TileMode::from_i32(tmx).unwrap_or(TileMode::Clamp);
    let y = TileMode::from_i32(tmy).unwrap_or(TileMode::Clamp);
    image_shader::classify_image_fast_path(
        color_type,
        &*sampling,
        x,
        y,
        dst_bounds,
        &*base_inv,
        coords_already_seeded,
        total_matrix_is_valid,
        &*total_matrix,
        &*total_inv,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_cubic_uses_clamp_01(
    at: i32,
    clamp_as_if_unpremul: bool,
) -> bool {
    let alpha_type = AlphaType::from_i32(at).unwrap_or(AlphaType::Unknown);
    image_shader::cubic_uses_clamp_01(alpha_type, clamp_as_if_unpremul)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_legacy_can_handle(
    inv: *const Matrix,
    has_dxdy_opt: bool,
) -> bool {
    if inv.is_null() {
        return false;
    }
    image_shader::legacy_shader_can_handle(&*inv, has_dxdy_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_shader_legacy_sampling_supported(
    sampling: *const SamplingOptions,
) -> bool {
    if sampling.is_null() {
        return false;
    }
    image_shader::legacy_context_sampling_supported(&*sampling)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bitmap_proc_shader_flags(
    pixmap_is_opaque: bool,
    paint_alpha: u8,
) -> u32 {
    image_shader::bitmap_proc_shader_flags(pixmap_is_opaque, paint_alpha)
}

// ============================================================================
// SkPictureShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_shader_should_be_empty(
    has_picture: bool,
    cull_rect: Rect,
    has_tile: bool,
    tile: Rect,
) -> bool {
    picture_shader::picture_shader_should_be_empty(has_picture, cull_rect, has_tile, tile)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_shader_cached_image_info(
    bounds: Rect,
    total_m: *const Matrix,
    dst_color_type: i32,
    max_texture_size: i32,
    out: *mut PictureCachedImageParams,
) {
    if total_m.is_null() || out.is_null() {
        return;
    }
    let ct = ColorType::from_i32(dst_color_type).unwrap_or(ColorType::Unknown);
    *out = picture_shader::compute_picture_cached_image_info(
        bounds,
        &*total_m,
        ct,
        max_texture_size,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_shader_tile_local_matrix(
    tile_scale: Size,
    out: *mut Matrix,
) {
    if out.is_null() {
        return;
    }
    *out = picture_shader::picture_tile_local_matrix(tile_scale);
}

// ============================================================================
// SkPerlinNoiseShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_perlin_classify(
    noise_type: i32,
    base_x: f32,
    base_y: f32,
    num_octaves: i32,
    seed: f32,
    tile_size: *const ISize,
) -> PerlinNoiseAction {
    let ty = match PerlinNoiseType::from_i32(noise_type) {
        Some(t) => t,
        None => return PerlinNoiseAction::ReturnNull,
    };
    let ts = if tile_size.is_null() {
        None
    } else {
        Some(*tile_size)
    };
    perlin_noise::classify_perlin_noise(ty, base_x, base_y, num_octaves, seed, ts)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_perlin_compute_painting_data(
    tile_size: ISize,
    seed: f32,
    base_freq_x: f32,
    base_freq_y: f32,
    out: *mut PaintingDataTables,
) {
    if out.is_null() {
        return;
    }
    *out = perlin_noise::compute_painting_data(tile_size, seed, base_freq_x, base_freq_y);
}

// ============================================================================
// SkGainmapShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gainmap_compute_uniforms(
    base_rect: Rect,
    gainmap_rect: Rect,
    dst_rect: Rect,
    gainmap_color_type: i32,
    info: *const GainmapInfoInput,
    dst_hdr_ratio: f32,
    out: *mut GainmapUniforms,
) {
    if info.is_null() || out.is_null() {
        return;
    }
    let ct = ColorType::from_i32(gainmap_color_type).unwrap_or(ColorType::Unknown);
    *out = gainmap::compute_gainmap_uniforms(
        base_rect,
        gainmap_rect,
        dst_rect,
        ct,
        &*info,
        dst_hdr_ratio,
    );
}

// ============================================================================
// SkGradientBaseShader
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_pack_flags(
    has_positions: bool,
    has_color_space: bool,
    tile_mode: u32,
    interp: GradientInterpolation,
) -> u32 {
    gradient_base::pack_serialization_flags(has_positions, has_color_space, tile_mode, interp)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_unpack_flags(flags: u32) -> UnpackedGradientFlags {
    gradient_base::unpack_serialization_flags(flags)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_serialization_slice(
    color_count: usize,
    first_stop_is_implicit: bool,
    last_stop_is_implicit: bool,
    out_start: *mut usize,
    out_count: *mut usize,
) {
    if out_start.is_null() || out_count.is_null() {
        return;
    }
    let (s, c) = gradient_base::serialization_slice(
        color_count,
        first_stop_is_implicit,
        last_stop_is_implicit,
    );
    *out_start = s;
    *out_count = c;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_valid(
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
) -> bool {
    gradient_base::valid_gradient(has_colors_data, color_count, tile_mode, interp)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_is_opaque(
    colors_are_opaque: bool,
    tile_mode: i32,
) -> bool {
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Clamp);
    gradient_base::gradient_is_opaque(colors_are_opaque, tm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_luminance_color(
    colors: *const Color4f,
    count: usize,
) -> Color4f {
    if colors.is_null() || count == 0 {
        return Color4f::new(0.0, 0.0, 0.0, 1.0);
    }
    let slice = std::slice::from_raw_parts(colors, count);
    gradient_base::gradient_luminance_color(slice)
}

/// Output metadata for `skia_rs_gradient_prepare_stops`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PreparedStopsMeta {
    pub color_count: usize,
    pub has_positions: bool,
    pub first_stop_is_implicit: bool,
    pub last_stop_is_implicit: bool,
    pub colors_are_opaque: bool,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_max_stop_count(
    color_count: usize,
    pos: *const f32,
    pos_count: usize,
) -> usize {
    if pos.is_null() || pos_count == 0 {
        return color_count;
    }
    let p = std::slice::from_raw_parts(pos, pos_count);
    let first = p[0] > 0.0;
    let last = p[pos_count - 1] != 1.0;
    color_count + usize::from(first) + usize::from(last)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_prepare_stops(
    colors: *const Color4f,
    color_count: usize,
    pos: *const f32,
    pos_count: usize,
    tile_mode: i32,
    out_colors: *mut Color4f,
    out_pos: *mut f32,
    out_meta: *mut PreparedStopsMeta,
) {
    if colors.is_null() || color_count == 0 || out_colors.is_null() || out_meta.is_null() {
        return;
    }
    let c_slice = std::slice::from_raw_parts(colors, color_count);
    let p_slice = if pos.is_null() || pos_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(pos, pos_count)
    };
    let tm = TileMode::from_i32(tile_mode).unwrap_or(TileMode::Clamp);
    let prep = gradient_base::prepare_gradient_stops(c_slice, p_slice, tm);
    let n = prep.colors.len();
    std::slice::from_raw_parts_mut(out_colors, n).copy_from_slice(&prep.colors);
    let has_positions = match &prep.positions {
        Some(pv) => {
            if !out_pos.is_null() {
                std::slice::from_raw_parts_mut(out_pos, pv.len()).copy_from_slice(pv);
            }
            true
        }
        None => false,
    };
    *out_meta = PreparedStopsMeta {
        color_count: n,
        has_positions,
        first_stop_is_implicit: prep.first_stop_is_implicit,
        last_stop_is_implicit: prep.last_stop_is_implicit,
        colors_are_opaque: prep.colors_are_opaque,
    };
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_compute_2_stop(
    c_l: Color4f,
    c_r: Color4f,
    out_factor: *mut f32,
    out_bias: *mut f32,
) {
    if out_factor.is_null() || out_bias.is_null() {
        return;
    }
    let (f, b) = gradient_base::compute_2_stop_gradient(c_l, c_r);
    std::slice::from_raw_parts_mut(out_factor, 4).copy_from_slice(&f);
    std::slice::from_raw_parts_mut(out_bias, 4).copy_from_slice(&b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_fill_evenly_spaced(
    pm_colors: *const Color4f,
    count: usize,
    factor_r: *mut f32,
    factor_g: *mut f32,
    factor_b: *mut f32,
    factor_a: *mut f32,
    bias_r: *mut f32,
    bias_g: *mut f32,
    bias_b: *mut f32,
    bias_a: *mut f32,
    capacity: usize,
) -> usize {
    if pm_colors.is_null()
        || count < 2
        || factor_r.is_null()
        || factor_g.is_null()
        || factor_b.is_null()
        || factor_a.is_null()
        || bias_r.is_null()
        || bias_g.is_null()
        || bias_b.is_null()
        || bias_a.is_null()
        || capacity < count
    {
        return 0;
    }
    let colors = std::slice::from_raw_parts(pm_colors, count);
    let mut factors = [
        std::slice::from_raw_parts_mut(factor_r, capacity),
        std::slice::from_raw_parts_mut(factor_g, capacity),
        std::slice::from_raw_parts_mut(factor_b, capacity),
        std::slice::from_raw_parts_mut(factor_a, capacity),
    ];
    let mut biases = [
        std::slice::from_raw_parts_mut(bias_r, capacity),
        std::slice::from_raw_parts_mut(bias_g, capacity),
        std::slice::from_raw_parts_mut(bias_b, capacity),
        std::slice::from_raw_parts_mut(bias_a, capacity),
    ];
    gradient_base::fill_evenly_spaced_stops(colors, &mut factors, &mut biases)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_fill_arbitrary(
    pm_colors: *const Color4f,
    positions: *const f32,
    count: usize,
    factor_r: *mut f32,
    factor_g: *mut f32,
    factor_b: *mut f32,
    factor_a: *mut f32,
    bias_r: *mut f32,
    bias_g: *mut f32,
    bias_b: *mut f32,
    bias_a: *mut f32,
    ts: *mut f32,
    capacity: usize,
    ts_capacity: usize,
) -> usize {
    if pm_colors.is_null()
        || positions.is_null()
        || count < 2
        || factor_r.is_null()
        || factor_g.is_null()
        || factor_b.is_null()
        || factor_a.is_null()
        || bias_r.is_null()
        || bias_g.is_null()
        || bias_b.is_null()
        || bias_a.is_null()
        || ts.is_null()
        || capacity < count + 1
        || ts_capacity < count + 1
    {
        return 0;
    }
    let colors = std::slice::from_raw_parts(pm_colors, count);
    let pos = std::slice::from_raw_parts(positions, count);
    let mut factors = [
        std::slice::from_raw_parts_mut(factor_r, capacity),
        std::slice::from_raw_parts_mut(factor_g, capacity),
        std::slice::from_raw_parts_mut(factor_b, capacity),
        std::slice::from_raw_parts_mut(factor_a, capacity),
    ];
    let mut biases = [
        std::slice::from_raw_parts_mut(bias_r, capacity),
        std::slice::from_raw_parts_mut(bias_g, capacity),
        std::slice::from_raw_parts_mut(bias_b, capacity),
        std::slice::from_raw_parts_mut(bias_a, capacity),
    ];
    let ts_slice = std::slice::from_raw_parts_mut(ts, ts_capacity);
    gradient_base::fill_arbitrary_stops(colors, pos, &mut factors, &mut biases, ts_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_plan_interpolated_to_dst(
    colors_are_opaque: bool,
    interp: GradientInterpolation,
) -> InterpolatedToDstPlan {
    gradient_base::plan_interpolated_to_dst(colors_are_opaque, interp)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_decal_limit_x() -> f32 {
    gradient_base::gradient_decal_limit_x()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_intermediate_cs_kind(
    color_space: u8,
) -> IntermediateColorSpaceKind {
    let cs = InterpolationColorSpace::from_u8(color_space)
        .unwrap_or(InterpolationColorSpace::Destination);
    gradient_base::intermediate_color_space_kind(cs)
}

/// Output metadata for `skia_rs_gradient_xform_colors`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct XformColorsResult {
    pub out_count: usize,
    pub has_out_positions: bool,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_xform_colors(
    in_colors: *const Color4f,
    in_count: usize,
    orig_positions: *const f32,
    interp: GradientInterpolation,
    first_stop_is_implicit: bool,
    last_stop_is_implicit: bool,
    force_explicit_positions: bool,
    out_colors: *mut Color4f,
    out_positions: *mut f32,
    out_capacity: usize,
) -> XformColorsResult {
    if in_colors.is_null() || in_count == 0 || out_colors.is_null() || out_capacity < in_count {
        return XformColorsResult {
            out_count: 0,
            has_out_positions: false,
        };
    }
    let colors_vec = std::slice::from_raw_parts(in_colors, in_count).to_vec();
    let orig_pos = if orig_positions.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(orig_positions, in_count))
    };
    let (xformed, new_pos) = gradient_base::xform_intermediate_colors(
        colors_vec,
        orig_pos,
        interp,
        first_stop_is_implicit,
        last_stop_is_implicit,
        force_explicit_positions,
    );
    let n = xformed.len().min(out_capacity);
    std::slice::from_raw_parts_mut(out_colors, n).copy_from_slice(&xformed[..n]);
    let has_out_positions = match &new_pos {
        Some(pv) => {
            if !out_positions.is_null() {
                let pn = pv.len().min(out_capacity);
                std::slice::from_raw_parts_mut(out_positions, pn).copy_from_slice(&pv[..pn]);
            }
            true
        }
        None => false,
    };
    XformColorsResult {
        out_count: n,
        has_out_positions,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gradient_classify_degenerate(
    colors: *const Color4f,
    color_count: usize,
    pos: *const f32,
    pos_count: usize,
    tile_mode: i32,
) -> DegenerateGradientResult {
    if colors.is_null() || color_count == 0 {
        return DegenerateGradientResult {
            kind: 0,
            color: Color4f::new(0.0, 0.0, 0.0, 0.0),
        };
    }
    let c_slice = std::slice::from_raw_parts(colors, color_count);
    let p_slice = if pos.is_null() || pos_count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(pos, pos_count)
    };
    let tm = match TileMode::from_i32(tile_mode) {
        Some(t) => t,
        None => {
            return DegenerateGradientResult {
                kind: 0,
                color: Color4f::new(0.0, 0.0, 0.0, 0.0),
            };
        }
    };
    gradient_base::classify_degenerate_gradient(c_slice, p_slice, tm)
}

// ============================================================================
// SkLinearGradient / SkRadialGradient / SkSweepGradient / SkConicalGradient
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_linear_gradient_unit_matrix(
    p0: Point,
    p1: Point,
    out: *mut Matrix,
) {
    if out.is_null() {
        return;
    }
    *out = gradients::linear_pts_to_unit_matrix(p0, p1);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_linear_gradient_classify(
    has_pts: bool,
    p0: Point,
    p1: Point,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> LinearGradientAction {
    gradients::classify_linear_gradient(
        has_pts,
        p0,
        p1,
        has_colors_data,
        color_count,
        tile_mode,
        interp,
        has_lm,
        lm_invertible,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_radial_gradient_unit_matrix(
    center: Point,
    radius: f32,
    out: *mut Matrix,
) {
    if out.is_null() {
        return;
    }
    *out = gradients::radial_to_unit_matrix(center, radius);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_radial_gradient_classify(
    radius: f32,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> RadialGradientAction {
    gradients::classify_radial_gradient(
        radius,
        has_colors_data,
        color_count,
        tile_mode,
        interp,
        has_lm,
        lm_invertible,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sweep_gradient_unit_matrix(center: Point, out: *mut Matrix) {
    if out.is_null() {
        return;
    }
    *out = gradients::sweep_to_unit_matrix(center);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sweep_gradient_t_coeff(t0: f32, t1: f32) -> SweepTCoeff {
    gradients::sweep_t_coeff_from_t(t0, t1)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sweep_gradient_angles_from_t(
    t_bias: f32,
    t_scale: f32,
) -> SweepAngles {
    gradients::sweep_angles_from_t_coeff(t_bias, t_scale)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sweep_gradient_stage_matrix(
    t_bias: f32,
    t_scale: f32,
    out: *mut Matrix,
) {
    if out.is_null() {
        return;
    }
    *out = gradients::sweep_stage_matrix(t_bias, t_scale);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_sweep_gradient_classify(
    start_angle: f32,
    end_angle: f32,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> SweepGradientPlan {
    gradients::classify_sweep_gradient(
        start_angle,
        end_angle,
        has_colors_data,
        color_count,
        tile_mode,
        interp,
        has_lm,
        lm_invertible,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_focal_data_set(
    focal_data: *mut FocalData,
    r0: f32,
    r1: f32,
    matrix: *mut Matrix,
) -> bool {
    if focal_data.is_null() || matrix.is_null() {
        return false;
    }
    (*focal_data).set(r0, r1, &mut *matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_map_to_unit_x(
    start_center: Point,
    end_center: Point,
    out: *mut Matrix,
) -> bool {
    if out.is_null() {
        return false;
    }
    match gradients::conical_map_to_unit_x(start_center, end_center) {
        Some(m) => {
            *out = m;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_create_params(
    c0: Point,
    r0: f32,
    c1: Point,
    r1: f32,
    out: *mut ConicalCreateResult,
) {
    if out.is_null() {
        return;
    }
    *out = gradients::conical_create_params(c0, r0, c1, r1);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_classify_two_point(
    start: Point,
    start_radius: f32,
    end: Point,
    end_radius: f32,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> TwoPointConicalAction {
    gradients::classify_two_point_conical(
        start,
        start_radius,
        end,
        end_radius,
        has_colors_data,
        color_count,
        tile_mode,
        interp,
        has_lm,
        lm_invertible,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_radial_stage_matrix(
    r1: f32,
    r2: f32,
    out: *mut Matrix,
) {
    if out.is_null() {
        return;
    }
    *out = gradients::conical_radial_stage_matrix(r1, r2);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_strip_p0(r1: f32, c1: Point, c2: Point) -> f32 {
    gradients::conical_strip_p0(r1, c1, c2)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conical_plan_focal_stages(
    focal_data: *const FocalData,
    out: *mut ConicalFocalStagePlan,
) {
    if focal_data.is_null() || out.is_null() {
        return;
    }
    *out = gradients::plan_conical_focal_stages(&*focal_data);
}
