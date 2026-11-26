//! FFI exports for `skia-rs-geometry` (Layer 1.2).

use skia_rs_geometry::{
    m44::{M44, V3, V4},
    matrix22::compute_givens_rotation_sin_cos,
    matrix_invert::{invert_2x2_matrix, invert_3x3_matrix, invert_4x4_matrix},
    point::Point,
    point3::Point3,
    rect::{IRect, Rect},
    rsxform::RSXform,
    vert_state,
};

// --- SkPoint ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point_length(dx: f32, dy: f32) -> f32 {
    Point::length(dx, dy)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point_set_length(
    pt: *mut Point,
    x: f32,
    y: f32,
    length: f32,
) -> bool {
    (*pt).set_length(x, y, length)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point_normalize_with_length(pt: *mut Point) -> f32 {
    (*pt).normalize_with_length()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point_distance_to_line_between_sqd(
    pt: Point,
    a: Point,
    b: Point,
    side_out: *mut i32,
) -> f32 {
    let (dist_sq, side) = Point::distance_to_line_between_sqd(pt, a, b);
    if !side_out.is_null() {
        *side_out = side;
    }
    dist_sq
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point_distance_to_line_segment_between_sqd(
    pt: Point,
    a: Point,
    b: Point,
) -> f32 {
    Point::distance_to_line_segment_between_sqd(pt, a, b)
}

// --- SkPoint3 ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point3_length(x: f32, y: f32, z: f32) -> f32 {
    Point3::length(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_point3_normalize(pt: *mut Point3) -> bool {
    (*pt).normalize()
}

// --- SkIRect & SkRect ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_irect_intersect(
    dst: *mut IRect,
    a: *const IRect,
    b: *const IRect,
) -> bool {
    (*dst).intersect_pair(*a, *b)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_irect_join(dst: *mut IRect, r: *const IRect) {
    (*dst).join(*r);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_irect_subtract(
    a: *const IRect,
    b: *const IRect,
    out: *mut IRect,
) -> bool {
    let (res, exact) = IRect::subtract(*a, *b);
    *out = res;
    exact
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_irect_closest_disjoint_edge(
    src: *const IRect,
    dst: *const IRect,
) -> IRect {
    IRect::closest_disjoint_edge(*src, *dst)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_bounds(
    pts: *const Point,
    count: usize,
    out: *mut Rect,
) -> bool {
    let slice = if count == 0 || pts.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(pts, count)
    };
    match Rect::bounds(slice) {
        Some(r) => {
            *out = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_intersect(dst: *mut Rect, r: *const Rect) -> bool {
    (*dst).intersect(*r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_intersect_pair(
    dst: *mut Rect,
    a: *const Rect,
    b: *const Rect,
) -> bool {
    (*dst).intersect_pair(*a, *b)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_join(dst: *mut Rect, r: *const Rect) {
    (*dst).join(*r);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_subtract(
    a: *const Rect,
    b: *const Rect,
    out: *mut Rect,
) -> bool {
    let (res, exact) = Rect::subtract(*a, *b);
    *out = res;
    exact
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rect_quad_contains_rect_mask(
    m: *const M44,
    a: *const Rect,
    b: *const Rect,
    tol: f32,
    out_mask: *mut i32,
) {
    let mask = Rect::quad_contains_rect_mask(&*m, *a, *b, tol);
    std::ptr::copy_nonoverlapping(mask.as_ptr(), out_mask, 4);
}

// --- SkRSXform ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rsxform_to_quad(
    xform: *const RSXform,
    width: f32,
    height: f32,
    quad: *mut Point,
) {
    let q = (*xform).to_quad(width, height);
    std::ptr::copy_nonoverlapping(q.as_ptr(), quad, 4);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rsxform_to_tri_strip(
    xform: *const RSXform,
    width: f32,
    height: f32,
    strip: *mut Point,
) {
    let s = (*xform).to_tri_strip(width, height);
    std::ptr::copy_nonoverlapping(s.as_ptr(), strip, 4);
}

// --- SkMatrixInvert ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_invert_2x2_matrix(
    in_mat: *const f32,
    out_mat: *mut f32,
) -> f32 {
    let inp: &[f32; 4] = &*(in_mat as *const [f32; 4]);
    let out = if out_mat.is_null() {
        None
    } else {
        Some(&mut *(out_mat as *mut [f32; 4]))
    };
    invert_2x2_matrix(inp, out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_invert_3x3_matrix(
    in_mat: *const f32,
    out_mat: *mut f32,
) -> f32 {
    let inp: &[f32; 9] = &*(in_mat as *const [f32; 9]);
    let out = if out_mat.is_null() {
        None
    } else {
        Some(&mut *(out_mat as *mut [f32; 9]))
    };
    invert_3x3_matrix(inp, out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_invert_4x4_matrix(
    in_mat: *const f32,
    out_mat: *mut f32,
) -> f32 {
    let inp: &[f32; 16] = &*(in_mat as *const [f32; 16]);
    let out = if out_mat.is_null() {
        None
    } else {
        Some(&mut *(out_mat as *mut [f32; 16]))
    };
    invert_4x4_matrix(inp, out)
}

// --- SkM44 ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_eq(a: *const M44, b: *const M44) -> bool {
    *a == *b
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_get_row_major(m: *const M44, dst: *mut f32) {
    let r = (*m).get_row_major();
    std::ptr::copy_nonoverlapping(r.as_ptr(), dst, 16);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_set_concat(
    dst: *mut M44,
    a: *const M44,
    b: *const M44,
) {
    let a_val = *a;
    let b_val = *b;
    (*dst).set_concat(&a_val, &b_val);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_pre_concat_m33(dst: *mut M44, b: *const f32) {
    let b_arr: &[f32; 9] = &*(b as *const [f32; 9]);
    (*dst).pre_concat_m33(b_arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_pre_translate(dst: *mut M44, x: f32, y: f32, z: f32) {
    (*dst).pre_translate(x, y, z);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_post_translate(dst: *mut M44, x: f32, y: f32, z: f32) {
    (*dst).post_translate(x, y, z);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_pre_scale_xy(dst: *mut M44, x: f32, y: f32) {
    (*dst).pre_scale_xy(x, y);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_pre_scale_xyz(dst: *mut M44, x: f32, y: f32, z: f32) {
    (*dst).pre_scale_xyz(x, y, z);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_map(m: *const M44, x: f32, y: f32, z: f32, w: f32) -> V4 {
    (*m).map(x, y, z, w)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_map_rect(m: *const M44, src: *const Rect) -> Rect {
    (*m).map_rect(*src)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_normalize_perspective(dst: *mut M44) {
    (*dst).normalize_perspective();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_invert(m: *const M44, out_inv: *mut M44) -> bool {
    match (*m).invert() {
        Some(inv) => {
            if !out_inv.is_null() {
                *out_inv = inv;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_transpose(m: *const M44, out: *mut M44) {
    *out = (*m).transpose();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_set_rotate_unit_sin_cos(
    dst: *mut M44,
    axis: V3,
    sin_angle: f32,
    cos_angle: f32,
) {
    (*dst).set_rotate_unit_sin_cos(axis, sin_angle, cos_angle);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_set_rotate(dst: *mut M44, axis: V3, radians: f32) {
    (*dst).set_rotate(axis, radians);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_rect_to_rect(
    src: *const Rect,
    dst: *const Rect,
    out: *mut M44,
) {
    *out = M44::rect_to_rect(*src, *dst);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_look_at(
    eye: *const V3,
    center: *const V3,
    up: *const V3,
    out: *mut M44,
) {
    *out = M44::look_at(*eye, *center, *up);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_m44_perspective(
    near: f32,
    far: f32,
    angle: f32,
    out: *mut M44,
) {
    *out = M44::perspective(near, far, angle);
}

// --- SkMatrix22 ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_compute_givens_rotation(
    h: Point,
    sin_out: *mut f32,
    cos_out: *mut f32,
) {
    let (s, c) = compute_givens_rotation_sin_cos(h);
    *sin_out = s;
    *cos_out = c;
}

// --- SkVertState ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_vert_state_step(
    mode: i32,
    indices: *const u16,
    count: i32,
    curr_index: *mut i32,
    f0: *mut i32,
    f1: *mut i32,
    f2: *mut i32,
) -> bool {
    let step = if indices.is_null() {
        match mode {
            0 => vert_state::step_triangles(*curr_index, count),
            1 => vert_state::step_triangle_strip(*curr_index, count),
            2 => vert_state::step_triangle_fan(*curr_index, count),
            _ => None,
        }
    } else {
        let len = if count < 0 { 0 } else { count as usize };
        let slice = std::slice::from_raw_parts(indices, len);
        match mode {
            0 => vert_state::step_triangles_indexed(*curr_index, slice),
            1 => vert_state::step_triangle_strip_indexed(*curr_index, slice),
            2 => vert_state::step_triangle_fan_indexed(*curr_index, slice),
            _ => None,
        }
    };
    if let Some(s) = step {
        *f0 = s.f0;
        *f1 = s.f1;
        *f2 = s.f2;
        *curr_index = s.next_index;
        true
    } else {
        false
    }
}
