//! FFI exports for `skia-rs-colorfilters` (Phase 1 Layer 4.2).

use skia_rs_color::{
    blend_mode::BlendMode,
    color::Color4f,
    color_space::{Matrix3x3, TransferFunction},
    image_info::AlphaType,
};
use skia_rs_colorfilters::{
    blend_mode_cf::{self, BlendSimplifyResult},
    blenders::{self, ArithmeticResult},
    color_filter,
    color_matrix::ColorMatrix,
    compose_cf,
    high_contrast::{self, HighContrastConfig, HighContrastUniforms},
    lighting::{self, LightingResult},
    matrix_cf::{self, MatrixClamp, MatrixDomain, MatrixStagePlan},
    table_cf::{self, TableStagePlan},
    working_format_cf::{ResolvedWorkingFormat, WorkingFormatCalculator},
    xform_cf::{self, XformStagePlan},
};

// ============================================================================
// SkBlenders
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blender_classify_arithmetic(
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
    enforce_premul: bool,
) -> ArithmeticResult {
    blenders::classify_arithmetic_ffi(k1, k2, k3, k4, enforce_premul)
}

// ============================================================================
// SkColorMatrix
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_rgb_to_yuv(
    cs: i32,
    high_precision: bool,
    out_mat: *mut f32,
) {
    if out_mat.is_null() {
        return;
    }
    let m = ColorMatrix::rgb_to_yuv(cs, high_precision);
    std::slice::from_raw_parts_mut(out_mat, 20).copy_from_slice(&m.mat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_yuv_to_rgb(
    cs: i32,
    high_precision: bool,
    out_mat: *mut f32,
) {
    if out_mat.is_null() {
        return;
    }
    let m = ColorMatrix::yuv_to_rgb(cs, high_precision);
    std::slice::from_raw_parts_mut(out_mat, 20).copy_from_slice(&m.mat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_set_identity(mat: *mut f32) {
    if mat.is_null() {
        return;
    }
    let mut m = ColorMatrix::identity();
    m.set_identity();
    std::slice::from_raw_parts_mut(mat, 20).copy_from_slice(&m.mat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_set_scale(
    mat: *mut f32,
    r_scale: f32,
    g_scale: f32,
    b_scale: f32,
    a_scale: f32,
) {
    if mat.is_null() {
        return;
    }
    let mut m = ColorMatrix::identity();
    m.set_scale(r_scale, g_scale, b_scale, a_scale);
    std::slice::from_raw_parts_mut(mat, 20).copy_from_slice(&m.mat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_post_translate(
    mat: *mut f32,
    dr: f32,
    dg: f32,
    db: f32,
    da: f32,
) {
    if mat.is_null() {
        return;
    }
    let slice = std::slice::from_raw_parts_mut(mat, 20);
    let mut arr = [0.0_f32; 20];
    arr.copy_from_slice(slice);
    let mut m = ColorMatrix::from_row_major(arr);
    m.post_translate(dr, dg, db, da);
    slice.copy_from_slice(&m.mat);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_set_concat(
    result: *mut f32,
    outer: *const f32,
    inner: *const f32,
) {
    if result.is_null() || outer.is_null() || inner.is_null() {
        return;
    }
    let mut outer_arr = [0.0_f32; 20];
    let mut inner_arr = [0.0_f32; 20];
    outer_arr.copy_from_slice(std::slice::from_raw_parts(outer, 20));
    inner_arr.copy_from_slice(std::slice::from_raw_parts(inner, 20));
    let out = ColorMatrix::concat_arrays(&outer_arr, &inner_arr);
    std::slice::from_raw_parts_mut(result, 20).copy_from_slice(&out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_matrix_set_saturation(mat: *mut f32, sat: f32) {
    if mat.is_null() {
        return;
    }
    let mut m = ColorMatrix::identity();
    m.set_saturation(sat);
    std::slice::from_raw_parts_mut(mat, 20).copy_from_slice(&m.mat);
}

// ============================================================================
// SkColorMatrixFilter (Lighting)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_classify_lighting(
    mul: u32,
    add: u32,
) -> LightingResult {
    lighting::classify_lighting_ffi(mul, add)
}

// ============================================================================
// SkHighContrastFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_high_contrast_compute_uniforms(
    config: *const HighContrastConfig,
    out_uniforms: *mut HighContrastUniforms,
) -> bool {
    if config.is_null() || out_uniforms.is_null() {
        return false;
    }
    match high_contrast::compute_high_contrast_uniforms(&*config) {
        Some(u) => {
            *out_uniforms = u;
            true
        }
        None => false,
    }
}

// ============================================================================
// SkColorFilter & SkColorFilterBase
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_finalize_color4f(pm: Color4f) -> Color4f {
    color_filter::finalize_filtered_color4f(pm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_unpremul_color4f(pm: Color4f) -> Color4f {
    color_filter::unpremul_color4f(pm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_pm_is_opaque(pm: Color4f) -> bool {
    color_filter::pm_color_is_opaque(pm)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_base_on_as_a_color_mode() -> bool {
    color_filter::base_on_as_a_color_mode()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_filter_base_on_as_a_color_matrix() -> bool {
    color_filter::base_on_as_a_color_matrix()
}

// ============================================================================
// SkBlendModeColorFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_cf_is_valid_mode(mode: i32) -> bool {
    blend_mode_cf::is_valid_blend_mode(mode)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_cf_pin_alpha(color: Color4f) -> Color4f {
    color_filter::pin_alpha(color)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_cf_simplify(
    srgb: Color4f,
    mode: i32,
) -> BlendSimplifyResult {
    blend_mode_cf::simplify_blend_filter_ffi(srgb, mode)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_cf_is_alpha_unchanged(mode: i32) -> bool {
    match BlendMode::from_i32(mode) {
        Some(m) => blend_mode_cf::is_alpha_unchanged(m),
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_cf_to_sk_color(color: Color4f) -> u32 {
    color.to_sk_color()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_cf_from_sk_color(color: u32) -> Color4f {
    Color4f::from_color(color)
}

// ============================================================================
// SkColorSpaceXformColorFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xform_cf_plan_stages(shader_is_opaque: bool) -> XformStagePlan {
    xform_cf::plan_xform_stages(shader_is_opaque)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xform_cf_decode_legacy_gamma_dir(dir: u32) -> i32 {
    match xform_cf::decode_legacy_gamma_dir(dir) {
        Some(d) => d as i32,
        None => -1,
    }
}

// ============================================================================
// SkComposeColorFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_compose_cf_is_alpha_unchanged(
    outer_unchanged: bool,
    inner_unchanged: bool,
) -> bool {
    compose_cf::compose_is_alpha_unchanged(outer_unchanged, inner_unchanged)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_compose_cf_inner_is_opaque(
    shader_is_opaque: bool,
    inner_unchanged: bool,
) -> bool {
    compose_cf::compose_inner_is_opaque(shader_is_opaque, inner_unchanged)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_compose_cf_classify(
    has_outer: bool,
    has_inner: bool,
) -> i32 {
    compose_cf::classify_compose(has_outer, has_inner) as i32
}

// ============================================================================
// SkMatrixColorFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_cf_is_valid(matrix: *const f32) -> bool {
    if matrix.is_null() {
        return false;
    }
    let mut arr = [0.0_f32; 20];
    arr.copy_from_slice(std::slice::from_raw_parts(matrix, 20));
    matrix_cf::is_valid_matrix(&arr)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_cf_is_alpha_unchanged(matrix: *const f32) -> bool {
    if matrix.is_null() {
        return false;
    }
    let mut arr = [0.0_f32; 20];
    arr.copy_from_slice(std::slice::from_raw_parts(matrix, 20));
    matrix_cf::is_alpha_unchanged(&arr)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_cf_plan_stages(
    shader_is_opaque: bool,
    alpha_is_unchanged: bool,
    domain: u8,
    clamp: bool,
) -> MatrixStagePlan {
    let dom = MatrixDomain::from_u8(domain).unwrap_or(MatrixDomain::Rgba);
    let cl = MatrixClamp::from_bool(clamp);
    matrix_cf::plan_matrix_stages(shader_is_opaque, alpha_is_unchanged, dom, cl)
}

// ============================================================================
// SkTableColorFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_table_cf_plan_stages(
    shader_is_opaque: bool,
    alpha_table_255: u8,
) -> TableStagePlan {
    table_cf::plan_table_stages(shader_is_opaque, alpha_table_255)
}

// ============================================================================
// SkWorkingFormatColorFilter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_working_format_init(
    tf: *const TransferFunction,
    gamut: *const Matrix3x3,
    at: *const AlphaType,
    out: *mut WorkingFormatCalculator,
) {
    if out.is_null() {
        return;
    }
    let tf_opt = if tf.is_null() { None } else { Some(*tf) };
    let gamut_opt = if gamut.is_null() { None } else { Some(*gamut) };
    let at_opt = if at.is_null() { None } else { Some(*at) };
    *out = WorkingFormatCalculator::new(tf_opt, gamut_opt, at_opt);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_working_format_resolve(
    calc: *const WorkingFormatCalculator,
    dst_tf: *const TransferFunction,
    dst_gamut: *const Matrix3x3,
    out: *mut ResolvedWorkingFormat,
) {
    if calc.is_null() || out.is_null() {
        return;
    }
    let d_tf = if dst_tf.is_null() {
        TransferFunction::default()
    } else {
        *dst_tf
    };
    let d_gamut = if dst_gamut.is_null() {
        Matrix3x3::default()
    } else {
        *dst_gamut
    };
    *out = (*calc).resolve(d_tf, d_gamut);
}
