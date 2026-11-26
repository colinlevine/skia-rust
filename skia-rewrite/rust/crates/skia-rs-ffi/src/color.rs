//! FFI exports for `skia-rs-color` (Layer 2.1).

use std::ffi::c_char;

use skia_rs_color::{
    blend_mode::{self, BlendMode},
    color::{self, Color4f},
    color_space::{
        self, ColorSpacePrimaries, Matrix3x3, TransferFunction, SERIALIZED_COLOR_SPACE_BYTES,
        TF_LINEAR, TF_SRGB,
    },
    color_space_xform_steps::{ColorSpaceXformSteps, XformStepsFlags, XformStepsInput},
    color_table,
    convert_pixels,
    image_info::{self, AlphaType, ColorType},
    masks::{InputMasks, Masks},
    swizzle,
    unpremultiply,
    yuv_math,
    yuva_info::{self, ISize, PlaneConfig, Siting, Subsampling, YuvaInfo, YuvaLocation, MAX_PLANES},
    yuva_pixmaps::{self, ValidatedYuvaPixmapInfo, YuvaDataType},
};

// ============================================================================
// SkColor
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_premultiply_argb(
    a: u32,
    r: u32,
    g: u32,
    b: u32,
    is_bgra: bool,
) -> u32 {
    color::premultiply_argb(a, r, g, b, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_premultiply_color(c: u32, is_bgra: bool) -> u32 {
    color::premultiply_color(c, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pmcolor_set_argb(
    a: u8,
    r: u8,
    g: u8,
    b: u8,
    is_bgra: bool,
) -> u32 {
    color::pmcolor_set_argb(a as u32, r as u32, g as u32, b as u32, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pmcolor_get_a(c: u32) -> u8 {
    color::pmcolor_get_a(c)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pmcolor_get_r(c: u32, is_bgra: bool) -> u8 {
    color::pmcolor_get_r(c, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pmcolor_get_g(c: u32) -> u8 {
    color::pmcolor_get_g(c)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pmcolor_get_b(c: u32, is_bgra: bool) -> u8 {
    color::pmcolor_get_b(c, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_rgb_to_hsv(r: u32, g: u32, b: u32, out_hsv: *mut f32) {
    let hsv = color::rgb_to_hsv(r, g, b);
    let out = std::slice::from_raw_parts_mut(out_hsv, 3);
    out.copy_from_slice(&hsv);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_hsv_to_color(a: u32, hsv: *const f32) -> u32 {
    let s = std::slice::from_raw_parts(hsv, 3);
    color::hsv_to_color(a, [s[0], s[1], s[2]])
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color4f_from_color(bgra: u32) -> Color4f {
    Color4f::from_color(bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color4f_to_sk_color(c: *const Color4f) -> u32 {
    (*c).to_sk_color()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color4f_to_bytes_rgba(c: *const Color4f) -> u32 {
    (*c).to_bytes_rgba()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color4f_from_bytes_rgba(c: u32) -> Color4f {
    Color4f::from_bytes_rgba(c)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pmcolor4f_from_pmcolor(c: u32, is_bgra: bool) -> Color4f {
    Color4f::from_pm_color(c, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_convert_to_4f(
    src: *const u32,
    dst: *mut Color4f,
    count: usize,
) {
    if count == 0 || src.is_null() || dst.is_null() {
        return;
    }
    let s = std::slice::from_raw_parts(src, count);
    let d = std::slice::from_raw_parts_mut(dst, count);
    color::convert_colors_to_4f(s, d);
}

// ============================================================================
// SkUnPreMultiply
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_unpremultiply_pmcolor_to_color(c: u32, is_bgra: bool) -> u32 {
    unpremultiply::pmcolor_to_color(c, is_bgra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_unpremultiply_scale_table() -> *const u32 {
    unpremultiply::UNPREMULTIPLY_TABLE.as_ptr()
}

// ============================================================================
// SkColorTable
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_table_fill(
    table_a: *const u8,
    table_r: *const u8,
    table_g: *const u8,
    table_b: *const u8,
    out_argb: *mut u8,
) -> bool {
    let as_arr = |p: *const u8| -> Option<&[u8; 256]> {
        if p.is_null() {
            None
        } else {
            Some(&*(p as *const [u8; 256]))
        }
    };
    let out = &mut *(out_argb as *mut [u8; 1024]);
    color_table::fill_color_table(
        as_arr(table_a),
        as_arr(table_r),
        as_arr(table_g),
        as_arr(table_b),
        out,
    )
}

// ============================================================================
// SkMasks
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_masks_create(
    masks: InputMasks,
    bytes_per_pixel: i32,
    out: *mut Masks,
) -> bool {
    match Masks::create(masks, bytes_per_pixel) {
        Some(m) => {
            *out = m;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_masks_get_red(m: *const Masks, pixel: u32) -> u8 {
    (*m).get_red(pixel)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_masks_get_green(m: *const Masks, pixel: u32) -> u8 {
    (*m).get_green(pixel)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_masks_get_blue(m: *const Masks, pixel: u32) -> u8 {
    (*m).get_blue(pixel)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_masks_get_alpha(m: *const Masks, pixel: u32) -> u8 {
    (*m).get_alpha(pixel)
}

// ============================================================================
// SkBlendMode & SkBlendModeBlender
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_should_pre_scale_coverage(
    mode: i32,
    rgb_coverage: bool,
) -> bool {
    match BlendMode::from_i32(mode) {
        Some(bm) => blend_mode::should_pre_scale_coverage(bm, rgb_coverage),
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_supports_coverage_as_alpha(mode: i32) -> bool {
    match BlendMode::from_i32(mode) {
        Some(bm) => blend_mode::supports_coverage_as_alpha(bm),
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_as_coeff(
    mode: i32,
    src_out: *mut i32,
    dst_out: *mut i32,
) -> bool {
    let Some(bm) = BlendMode::from_i32(mode) else {
        return false;
    };
    let Some((src, dst)) = blend_mode::as_coeff(bm) else {
        return false;
    };
    if !src_out.is_null() {
        *src_out = src as i32;
    }
    if !dst_out.is_null() {
        *dst_out = dst as i32;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_name(mode: i32) -> *const c_char {
    match BlendMode::from_i32(mode) {
        Some(bm) => blend_mode::name_cstr(bm).as_ptr() as *const c_char,
        None => b"\0".as_ptr() as *const c_char,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_check_fast_path(
    has_mode: bool,
    mode: i32,
    just_solid_color: bool,
    dst_is_opaque: bool,
) -> i32 {
    let bm = if has_mode {
        BlendMode::from_i32(mode)
    } else {
        None
    };
    blend_mode::check_fast_path(bm, just_solid_color, dst_is_opaque) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_apply_fast(
    mode: i32,
    src: *const Color4f,
    dst: *const Color4f,
    out: *mut Color4f,
) -> bool {
    let Some(bm) = BlendMode::from_i32(mode) else {
        return false;
    };
    match blend_mode::apply_fast(bm, *src, *dst) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blend_mode_affects_transparent_black(
    has_mode: bool,
    mode: i32,
) -> bool {
    let bm = if has_mode {
        BlendMode::from_i32(mode)
    } else {
        None
    };
    blend_mode::affects_transparent_black(bm)
}

// ============================================================================
// SkImageInfo
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_type_bytes_per_pixel(ct: i32) -> i32 {
    ColorType::from_i32(ct)
        .map(image_info::color_type_bytes_per_pixel)
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_type_shift_per_pixel(ct: i32) -> i32 {
    ColorType::from_i32(ct)
        .map(image_info::color_type_shift_per_pixel)
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_type_channel_flags(ct: i32) -> u32 {
    ColorType::from_i32(ct)
        .map(image_info::color_type_channel_flags)
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_type_is_always_opaque(ct: i32) -> bool {
    ColorType::from_i32(ct)
        .map(image_info::color_type_is_always_opaque)
        .unwrap_or(true)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_type_compute_offset(
    ct: i32,
    x: i32,
    y: i32,
    row_bytes: usize,
) -> usize {
    ColorType::from_i32(ct)
        .map(|c| image_info::color_type_compute_offset(c, x, y, row_bytes))
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_info_compute_byte_size(
    width: i32,
    height: i32,
    ct: i32,
    row_bytes: usize,
) -> usize {
    let ct = ColorType::from_i32(ct).unwrap_or(ColorType::Unknown);
    image_info::compute_byte_size(width, height, ct, row_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_type_validate_alpha_type(
    ct: i32,
    at: i32,
    canonical_out: *mut i32,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct) else {
        return false;
    };
    let Some(at) = AlphaType::from_i32(at) else {
        return false;
    };
    match image_info::validate_alpha_type(ct, at) {
        Some(canon) => {
            if !canonical_out.is_null() {
                *canonical_out = canon as i32;
            }
            true
        }
        None => false,
    }
}

// ============================================================================
// SkColorSpace
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_get_cicp_primaries(
    primaries: u8,
    out_m: *mut Matrix3x3,
) -> bool {
    match color_space::get_cicp_primaries(primaries) {
        Some(m) => {
            *out_m = m;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_get_cicp_from_matrix(
    m: *const Matrix3x3,
    out_primaries: *mut u8,
) -> bool {
    match color_space::get_cicp_from_matrix(&*m) {
        Some(id) => {
            *out_primaries = id;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_get_cicp_transfer_fn(
    transfer_characteristics: u8,
    out_tf: *mut TransferFunction,
) -> bool {
    match color_space::get_cicp_transfer_fn(transfer_characteristics) {
        Some(tf) => {
            *out_tf = tf;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_primaries_to_xyzd50(
    p: *const ColorSpacePrimaries,
    out_m: *mut Matrix3x3,
) -> bool {
    match (*p).to_xyzd50() {
        Some(m) => {
            *out_m = m;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_hash_tf(tf: *const TransferFunction) -> u32 {
    (*tf).hash32()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_hash_xyz(m: *const Matrix3x3) -> u32 {
    (*m).hash32()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_canonicalize_rgb(
    tf: *const TransferFunction,
    to_xyz: *const Matrix3x3,
    out_tf: *mut TransferFunction,
) -> i32 {
    let (kind, canon_tf) = color_space::canonicalize_rgb(&*tf, &*to_xyz);
    *out_tf = canon_tf;
    kind as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix3x3_invert(
    src: *const Matrix3x3,
    dst: *mut Matrix3x3,
) -> bool {
    match (*src).invert() {
        Some(inv) => {
            *dst = inv;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix3x3_concat(
    a: *const Matrix3x3,
    b: *const Matrix3x3,
) -> Matrix3x3 {
    (*a).concat(&*b)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_spin_matrix(to_xyz: *const Matrix3x3) -> Matrix3x3 {
    color_space::color_spin_matrix(&*to_xyz)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_is_srgb_tf(tf: *const TransferFunction) -> bool {
    (*tf).bit_eq(&TF_SRGB)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_is_linear_tf(tf: *const TransferFunction) -> bool {
    (*tf).bit_eq(&TF_LINEAR)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_serialize(
    tf: *const TransferFunction,
    to_xyz: *const Matrix3x3,
    out: *mut u8,
) -> usize {
    if !out.is_null() {
        let buf = &mut *(out as *mut [u8; SERIALIZED_COLOR_SPACE_BYTES]);
        color_space::serialize_color_space(&*tf, &*to_xyz, buf);
    }
    SERIALIZED_COLOR_SPACE_BYTES
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_deserialize(
    data: *const u8,
    len: usize,
    out_tf: *mut TransferFunction,
    out_xyz: *mut Matrix3x3,
) -> bool {
    if data.is_null() || len < SERIALIZED_COLOR_SPACE_BYTES {
        return false;
    }
    let slice = std::slice::from_raw_parts(data, len);
    match color_space::deserialize_color_space(slice) {
        Some((tf, xyz)) => {
            *out_tf = tf;
            *out_xyz = xyz;
            true
        }
        None => false,
    }
}

// ============================================================================
// SkColorSpaceXformSteps
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_xform_steps_build(
    input: *const XformStepsInput,
    out: *mut ColorSpaceXformSteps,
) {
    *out = ColorSpaceXformSteps::build(&*input);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_xform_steps_apply_unpremul(
    steps: *const ColorSpaceXformSteps,
    rgba: *mut f32,
) {
    let arr = &mut *(rgba as *mut [f32; 4]);
    (*steps).apply_unpremul(arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_xform_steps_apply_mid(
    steps: *const ColorSpaceXformSteps,
    rgba: *mut f32,
) {
    let arr = &mut *(rgba as *mut [f32; 4]);
    (*steps).apply_mid(arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_space_xform_steps_apply_premul(
    steps: *const ColorSpaceXformSteps,
    rgba: *mut f32,
) {
    let arr = &mut *(rgba as *mut [f32; 4]);
    (*steps).apply_premul(arr);
}

// ============================================================================
// SkSwizzle & SkSwizzler_opts
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_to_bgra(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_to_bgra_in_place(std::slice::from_raw_parts_mut(dst, n));
    } else {
        swizzle::rgba_to_bgra(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_to_rgba_premul(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_to_rgba_premul_in_place(std::slice::from_raw_parts_mut(dst, n));
    } else {
        swizzle::rgba_to_rgba_premul(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_to_bgra_premul(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_to_bgra_premul_in_place(std::slice::from_raw_parts_mut(dst, n));
    } else {
        swizzle::rgba_to_bgra_premul(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_premul_to_rgba(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_premul_to_rgba_in_place(std::slice::from_raw_parts_mut(dst, n), false);
    } else {
        swizzle::rgba_premul_to_rgba(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
            false,
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_premul_to_rgba_fast(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_premul_to_rgba_in_place(std::slice::from_raw_parts_mut(dst, n), true);
    } else {
        swizzle::rgba_premul_to_rgba(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
            true,
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_premul_to_bgra(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_premul_to_bgra_in_place(std::slice::from_raw_parts_mut(dst, n), false);
    } else {
        swizzle::rgba_premul_to_bgra(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
            false,
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgba_premul_to_bgra_fast(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::rgba_premul_to_bgra_in_place(std::slice::from_raw_parts_mut(dst, n), true);
    } else {
        swizzle::rgba_premul_to_bgra(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
            true,
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_inverted_cmyk_to_rgb1(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::inverted_cmyk_to_rgb1_in_place(std::slice::from_raw_parts_mut(dst, n));
    } else {
        swizzle::inverted_cmyk_to_rgb1(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_inverted_cmyk_to_bgr1(
    dst: *mut u32,
    src: *const u32,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    if dst as *const u32 == src {
        swizzle::inverted_cmyk_to_bgr1_in_place(std::slice::from_raw_parts_mut(dst, n));
    } else {
        swizzle::inverted_cmyk_to_bgr1(
            std::slice::from_raw_parts_mut(dst, n),
            std::slice::from_raw_parts(src, n),
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgb_to_rgb1(
    dst: *mut u32,
    src: *const u8,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    swizzle::rgb_to_rgb1(
        std::slice::from_raw_parts_mut(dst, n),
        std::slice::from_raw_parts(src, n * 3),
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_rgb_to_bgr1(
    dst: *mut u32,
    src: *const u8,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    swizzle::rgb_to_bgr1(
        std::slice::from_raw_parts_mut(dst, n),
        std::slice::from_raw_parts(src, n * 3),
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_gray_to_rgb1(
    dst: *mut u32,
    src: *const u8,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    swizzle::gray_to_rgb1(
        std::slice::from_raw_parts_mut(dst, n),
        std::slice::from_raw_parts(src, n),
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_graya_to_rgba(
    dst: *mut u32,
    src: *const u8,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    swizzle::graya_to_rgba(
        std::slice::from_raw_parts_mut(dst, n),
        std::slice::from_raw_parts(src, n * 2),
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_swizzle_graya_to_rgba_premul(
    dst: *mut u32,
    src: *const u8,
    count: i32,
) {
    if count <= 0 || dst.is_null() || src.is_null() {
        return;
    }
    let n = count as usize;
    swizzle::graya_to_rgba_premul(
        std::slice::from_raw_parts_mut(dst, n),
        std::slice::from_raw_parts(src, n * 2),
    );
}

// ============================================================================
// SkConvertPixels
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_convert_validate_strides(
    src_ct: i32,
    src_rb: usize,
    dst_ct: i32,
    dst_rb: usize,
    same_buffer: bool,
    out_src_stride: *mut i32,
    out_dst_stride: *mut i32,
) -> bool {
    let Some(sct) = ColorType::from_i32(src_ct) else {
        return false;
    };
    let Some(dct) = ColorType::from_i32(dst_ct) else {
        return false;
    };
    match convert_pixels::validate_convert_strides(sct, src_rb, dct, dst_rb, same_buffer) {
        Some((ss, ds)) => {
            *out_src_stride = ss;
            *out_dst_stride = ds;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_convert_select_fast_path(
    src_ct: i32,
    dst_ct: i32,
    flags: XformStepsFlags,
    allow_unpremul_swizzle: bool,
) -> i32 {
    let Some(sct) = ColorType::from_i32(src_ct) else {
        return convert_pixels::FastPathKind::None as i32;
    };
    let Some(dct) = ColorType::from_i32(dst_ct) else {
        return convert_pixels::FastPathKind::None as i32;
    };
    convert_pixels::select_fast_path(sct, dct, flags, allow_unpremul_swizzle) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_convert_row_to_alpha8(
    src_ct: i32,
    src_row: *const u8,
    src_row_len: usize,
    dst_row: *mut u8,
    width: usize,
) -> bool {
    if width == 0 {
        return true;
    }
    if src_row.is_null() || dst_row.is_null() {
        return false;
    }
    let Some(sct) = ColorType::from_i32(src_ct) else {
        return false;
    };
    let src = std::slice::from_raw_parts(src_row, src_row_len);
    let dst = std::slice::from_raw_parts_mut(dst_row, width);
    convert_pixels::convert_row_to_alpha8(sct, src, dst, width)
}

// ============================================================================
// SkYUVMath
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuv_color_matrix_rgb2yuv(
    cs: i32,
    m: *mut f32,
    high_precision: bool,
) {
    let out = &mut *(m as *mut [f32; 20]);
    yuv_math::color_matrix_rgb2yuv(cs, out, high_precision);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuv_color_matrix_yuv2rgb(
    cs: i32,
    m: *mut f32,
    high_precision: bool,
) {
    let out = &mut *(m as *mut [f32; 20]);
    yuv_math::color_matrix_yuv2rgb(cs, out, high_precision);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuv_color_space_is_limited_range(cs: i32) -> bool {
    yuv_math::yuv_color_space_is_limited_range(cs)
}

// ============================================================================
// SkYUVAInfo
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_subsampling_factors(
    subsampling: i32,
    out_x: *mut i32,
    out_y: *mut i32,
) {
    let (x, y) = Subsampling::from_i32(subsampling)
        .map(Subsampling::factors)
        .unwrap_or((0, 0));
    *out_x = x;
    *out_y = y;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_plane_subsampling_factors(
    plane_config: i32,
    subsampling: i32,
    plane_idx: i32,
    out_x: *mut i32,
    out_y: *mut i32,
) {
    let (x, y) = match (
        PlaneConfig::from_i32(plane_config),
        Subsampling::from_i32(subsampling),
    ) {
        (Some(pc), Some(ss)) => yuva_info::plane_subsampling_factors(pc, ss, plane_idx),
        _ => (0, 0),
    };
    *out_x = x;
    *out_y = y;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_plane_dimensions(
    dims: ISize,
    plane_config: i32,
    subsampling: i32,
    origin: i32,
    out_dims: *mut ISize,
) -> i32 {
    let out = &mut *(out_dims as *mut [ISize; MAX_PLANES]);
    match (
        PlaneConfig::from_i32(plane_config),
        Subsampling::from_i32(subsampling),
    ) {
        (Some(pc), Some(ss)) => yuva_info::plane_dimensions(dims, pc, ss, origin, out),
        _ => {
            out.fill(ISize::new(0, 0));
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_get_locations(
    plane_config: i32,
    plane_channel_flags: *const u32,
    out_locs: *mut YuvaLocation,
) {
    let out = &mut *(out_locs as *mut [YuvaLocation; 4]);
    let Some(pc) = PlaneConfig::from_i32(plane_config) else {
        *out = [YuvaLocation::default(); 4];
        return;
    };
    let n = pc.num_planes() as usize;
    let flags = if n == 0 || plane_channel_flags.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(plane_channel_flags, n)
    };
    *out = yuva_info::get_yuva_locations(pc, flags);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_has_alpha(plane_config: i32) -> bool {
    PlaneConfig::from_i32(plane_config)
        .map(PlaneConfig::has_alpha)
        .unwrap_or(false)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_info_init(
    dims: ISize,
    plane_config: i32,
    subsampling: i32,
    yuv_cs: i32,
    origin: i32,
    siting_x: i32,
    siting_y: i32,
    out: *mut YuvaInfo,
) {
    let pc = PlaneConfig::from_i32(plane_config).unwrap_or(PlaneConfig::Unknown);
    let ss = Subsampling::from_i32(subsampling).unwrap_or(Subsampling::Unknown);
    let _ = (siting_x, siting_y);
    *out = YuvaInfo::new(dims, pc, ss, yuv_cs, origin, Siting::Centered, Siting::Centered);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_info_compute_total_bytes(
    info: *const YuvaInfo,
    row_bytes: *const usize,
    plane_sizes: *mut usize,
) -> usize {
    let info_ref = &*info;
    let n = if info_ref.is_valid() {
        info_ref.num_planes() as usize
    } else {
        return 0;
    };
    let mut rb = [0usize; MAX_PLANES];
    if !row_bytes.is_null() && n > 0 {
        let src = std::slice::from_raw_parts(row_bytes, n);
        rb[..n].copy_from_slice(src);
    }
    let ps_opt = if plane_sizes.is_null() {
        None
    } else {
        Some(&mut *(plane_sizes as *mut [usize; MAX_PLANES]))
    };
    info_ref.compute_total_bytes(&rb, ps_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_info_equals(
    a: *const YuvaInfo,
    b: *const YuvaInfo,
) -> bool {
    *a == *b
}

// ============================================================================
// SkYUVAPixmaps
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_supported_data_type_bit_index(
    data_type: i32,
    num_channels: i32,
) -> i32 {
    let Some(dt) = YuvaDataType::from_i32(data_type) else {
        return -1;
    };
    match yuva_pixmaps::supported_data_type_bit_index(dt, num_channels) {
        Some(idx) => idx as i32,
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_num_channels_and_data_type(
    ct: i32,
    out_channels: *mut i32,
    out_dt: *mut i32,
) {
    let ct = ColorType::from_i32(ct).unwrap_or(ColorType::Unknown);
    let (ch, dt) = yuva_pixmaps::num_channels_and_data_type(ct);
    *out_channels = ch;
    *out_dt = dt as i32;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_validate_pixmap_info(
    info: *const YuvaInfo,
    color_types: *const i32,
    row_bytes: *const usize,
    out: *mut ValidatedYuvaPixmapInfo,
) -> bool {
    let info_ref = &*info;
    let n = if info_ref.is_valid() {
        info_ref.num_planes() as usize
    } else {
        return false;
    };
    let mut cts = [ColorType::Unknown; MAX_PLANES];
    if !color_types.is_null() && n > 0 {
        let src = std::slice::from_raw_parts(color_types, n);
        for i in 0..n {
            cts[i] = ColorType::from_i32(src[i]).unwrap_or(ColorType::Unknown);
        }
    }
    let mut rb_buf = [0usize; MAX_PLANES];
    let rb_opt = if row_bytes.is_null() {
        None
    } else {
        if n > 0 {
            let src = std::slice::from_raw_parts(row_bytes, n);
            rb_buf[..n].copy_from_slice(src);
        }
        Some(&rb_buf)
    };
    match yuva_pixmaps::validate_yuva_pixmap_info(info_ref, &cts, rb_opt) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_validate_pixmap_info_from_dt(
    info: *const YuvaInfo,
    data_type: i32,
    row_bytes: *const usize,
    default_red: bool,
    out: *mut ValidatedYuvaPixmapInfo,
) -> bool {
    let info_ref = &*info;
    let Some(dt) = YuvaDataType::from_i32(data_type) else {
        return false;
    };
    let n = if info_ref.is_valid() {
        info_ref.num_planes() as usize
    } else {
        return false;
    };
    let mut rb_buf = [0usize; MAX_PLANES];
    let rb_opt = if row_bytes.is_null() {
        None
    } else {
        if n > 0 {
            let src = std::slice::from_raw_parts(row_bytes, n);
            rb_buf[..n].copy_from_slice(src);
        }
        Some(&rb_buf)
    };
    match yuva_pixmaps::validate_yuva_pixmap_info_from_data_type(info_ref, dt, rb_opt, default_red)
    {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_compute_plane_offsets(
    info: *const YuvaInfo,
    row_bytes: *const usize,
    out_offsets: *mut usize,
    out_num_planes: *mut i32,
) -> bool {
    let info_ref = &*info;
    let n = if info_ref.is_valid() {
        info_ref.num_planes() as usize
    } else {
        return false;
    };
    let mut rb = [0usize; MAX_PLANES];
    if !row_bytes.is_null() && n > 0 {
        let src = std::slice::from_raw_parts(row_bytes, n);
        rb[..n].copy_from_slice(src);
    }
    match yuva_pixmaps::compute_plane_offsets(info_ref, &rb) {
        Some((offsets, count)) => {
            let dst = std::slice::from_raw_parts_mut(out_offsets, MAX_PLANES);
            dst.copy_from_slice(&offsets);
            *out_num_planes = count as i32;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_yuva_recommended_rgba_color_type(data_type: i32) -> i32 {
    let dt = YuvaDataType::from_i32(data_type).unwrap_or(YuvaDataType::Unorm8);
    yuva_pixmaps::recommended_rgba_color_type(dt) as i32
}
