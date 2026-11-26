//! FFI exports for `SkMatrix` and `SkRRect` (Layer 1.2).

use skia_rs_geometry::{
    matrix::{Matrix, ScaleToFit},
    point::{ISize, Point, Size},
    point3::Point3,
    rect::Rect,
    rrect::{RRect, SIZE_IN_MEMORY},
    rsxform::RSXform,
};

// --- SkMatrix ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_do_normalize_perspective(m: *mut Matrix) {
    (*m).do_normalize_perspective();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_reset(m: *mut Matrix) {
    (*m).reset();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set9(m: *mut Matrix, buf: *const f32) {
    let arr: &[f32; 9] = &*(buf as *const [f32; 9]);
    (*m).set9(arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_affine(m: *mut Matrix, buf: *const f32) {
    let arr: &[f32; 6] = &*(buf as *const [f32; 6]);
    (*m).set_affine(arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_affine_identity(buf: *mut f32) {
    let arr: &mut [f32; 6] = &mut *(buf as *mut [f32; 6]);
    Matrix::set_affine_identity(arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_compute_perspective_type_mask(m: *const Matrix) -> u8 {
    (*m).compute_perspective_type_mask()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_compute_type_mask(m: *const Matrix) -> u8 {
    (*m).compute_type_mask()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_eq(a: *const Matrix, b: *const Matrix) -> bool {
    *a == *b
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_is_similarity(m: *const Matrix, tol: f32) -> bool {
    (*m).is_similarity(tol)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_preserves_right_angles(m: *const Matrix, tol: f32) -> bool {
    (*m).preserves_right_angles(tol)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_translate(m: *mut Matrix, dx: f32, dy: f32) {
    (*m).set_translate(dx, dy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_translate(m: *mut Matrix, dx: f32, dy: f32) {
    (*m).pre_translate(dx, dy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_translate(m: *mut Matrix, dx: f32, dy: f32) {
    (*m).post_translate(dx, dy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_scale(m: *mut Matrix, sx: f32, sy: f32) {
    (*m).set_scale(sx, sy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_scale_pivot(
    m: *mut Matrix,
    sx: f32,
    sy: f32,
    px: f32,
    py: f32,
) {
    (*m).set_scale_pivot(sx, sy, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_scale(m: *mut Matrix, sx: f32, sy: f32) {
    (*m).pre_scale(sx, sy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_scale_pivot(
    m: *mut Matrix,
    sx: f32,
    sy: f32,
    px: f32,
    py: f32,
) {
    (*m).pre_scale_pivot(sx, sy, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_scale(m: *mut Matrix, sx: f32, sy: f32) {
    (*m).post_scale(sx, sy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_scale_pivot(
    m: *mut Matrix,
    sx: f32,
    sy: f32,
    px: f32,
    py: f32,
) {
    (*m).post_scale_pivot(sx, sy, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_idiv(m: *mut Matrix, divx: i32, divy: i32) -> bool {
    (*m).post_idiv(divx, divy)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_sin_cos(m: *mut Matrix, sin_v: f32, cos_v: f32) {
    (*m).set_sin_cos(sin_v, cos_v);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_sin_cos_pivot(
    m: *mut Matrix,
    sin_v: f32,
    cos_v: f32,
    px: f32,
    py: f32,
) {
    (*m).set_sin_cos_pivot(sin_v, cos_v, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_rsxform(m: *mut Matrix, xform: *const RSXform) {
    (*m).set_rsxform(*xform);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_rotate(m: *mut Matrix, degrees: f32) {
    (*m).set_rotate(degrees);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_rotate_pivot(
    m: *mut Matrix,
    degrees: f32,
    px: f32,
    py: f32,
) {
    (*m).set_rotate_pivot(degrees, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_rotate(m: *mut Matrix, degrees: f32) {
    (*m).pre_rotate(degrees);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_rotate_pivot(
    m: *mut Matrix,
    degrees: f32,
    px: f32,
    py: f32,
) {
    (*m).pre_rotate_pivot(degrees, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_rotate(m: *mut Matrix, degrees: f32) {
    (*m).post_rotate(degrees);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_rotate_pivot(
    m: *mut Matrix,
    degrees: f32,
    px: f32,
    py: f32,
) {
    (*m).post_rotate_pivot(degrees, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_skew(m: *mut Matrix, sx: f32, sy: f32) {
    (*m).set_skew(sx, sy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_skew_pivot(
    m: *mut Matrix,
    sx: f32,
    sy: f32,
    px: f32,
    py: f32,
) {
    (*m).set_skew_pivot(sx, sy, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_skew(m: *mut Matrix, sx: f32, sy: f32) {
    (*m).pre_skew(sx, sy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_skew_pivot(
    m: *mut Matrix,
    sx: f32,
    sy: f32,
    px: f32,
    py: f32,
) {
    (*m).pre_skew_pivot(sx, sy, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_skew(m: *mut Matrix, sx: f32, sy: f32) {
    (*m).post_skew(sx, sy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_skew_pivot(
    m: *mut Matrix,
    sx: f32,
    sy: f32,
    px: f32,
    py: f32,
) {
    (*m).post_skew_pivot(sx, sy, px, py);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_scale_translate(
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
    out: *mut Matrix,
) {
    *out = Matrix::scale_translate(sx, sy, tx, ty);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_rect_to_rect(
    src: *const Rect,
    dst: *const Rect,
    stf: i32,
    out: *mut Matrix,
) -> bool {
    let mode = match stf {
        0 => ScaleToFit::Fill,
        1 => ScaleToFit::Start,
        2 => ScaleToFit::Center,
        _ => ScaleToFit::End,
    };
    match Matrix::rect_to_rect(*src, *dst, mode) {
        Some(m) => {
            *out = m;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_set_concat(
    dst: *mut Matrix,
    a: *const Matrix,
    b: *const Matrix,
) {
    let a_val = *a;
    let b_val = *b;
    (*dst).set_concat(&a_val, &b_val);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_pre_concat(dst: *mut Matrix, m: *const Matrix) {
    let m_val = *m;
    (*dst).pre_concat(&m_val);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_post_concat(dst: *mut Matrix, m: *const Matrix) {
    let m_val = *m;
    (*dst).post_concat(&m_val);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_as_affine(m: *const Matrix, affine: *mut f32) -> bool {
    let out = if affine.is_null() {
        None
    } else {
        Some(&mut *(affine as *mut [f32; 6]))
    };
    (*m).as_affine(out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_compute_inv(
    dst: *mut f32,
    src: *const f32,
    inv_det: f64,
    is_persp: bool,
) {
    let dst_arr = &mut *(dst as *mut [f32; 9]);
    let src_arr = &*(src as *const [f32; 9]);
    Matrix::compute_inv(dst_arr, src_arr, inv_det, is_persp);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_invert(m: *const Matrix, out: *mut Matrix) -> bool {
    match (*m).invert() {
        Some(inv) => {
            *out = inv;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_point_perspective(
    m: *const Matrix,
    pt: Point,
) -> Point {
    (*m).map_point_perspective(pt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_pts_proc(
    proc_kind: i32,
    m: *const Matrix,
    dst: *mut Point,
    src: *const Point,
    count: i32,
) {
    if count <= 0 {
        return;
    }
    let n = count as usize;
    let m_ref = &*m;
    if dst as *const Point == src {
        let slice = std::slice::from_raw_parts_mut(dst, n);
        match proc_kind {
            0 => {}
            1 => {
                let tx = m_ref.mat[2];
                let ty = m_ref.mat[5];
                for p in slice.iter_mut() {
                    p.x += tx;
                    p.y += ty;
                }
            }
            2 => {
                let sx = m_ref.mat[0];
                let sy = m_ref.mat[4];
                let tx = m_ref.mat[2];
                let ty = m_ref.mat[5];
                for p in slice.iter_mut() {
                    p.x = p.x * sx + tx;
                    p.y = p.y * sy + ty;
                }
            }
            3 => {
                let sx = m_ref.mat[0];
                let kx = m_ref.mat[1];
                let tx = m_ref.mat[2];
                let ky = m_ref.mat[3];
                let sy = m_ref.mat[4];
                let ty = m_ref.mat[5];
                for p in slice.iter_mut() {
                    let x = p.x;
                    let y = p.y;
                    p.x = x * sx + y * kx + tx;
                    p.y = y * sy + x * ky + ty;
                }
            }
            _ => {
                for p in slice.iter_mut() {
                    *p = m_ref.map_point_perspective(*p);
                }
            }
        }
    } else {
        let dst_s = std::slice::from_raw_parts_mut(dst, n);
        let src_s = std::slice::from_raw_parts(src, n);
        match proc_kind {
            0 => Matrix::identity_pts(dst_s, src_s),
            1 => m_ref.trans_pts(dst_s, src_s),
            2 => m_ref.scale_pts(dst_s, src_s),
            3 => m_ref.affine_vpts(dst_s, src_s),
            _ => m_ref.persp_pts(dst_s, src_s),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_homogeneous_with_stride(
    m: *const Matrix,
    dst: *mut u8,
    dst_stride: usize,
    src: *const u8,
    src_stride: usize,
    count: i32,
) {
    if count <= 0 {
        return;
    }
    let m_ref = &*m;
    if m_ref.is_identity() {
        if src != dst {
            for i in 0..(count as usize) {
                let s = *(src.add(i * src_stride) as *const Point3);
                *(dst.add(i * dst_stride) as *mut Point3) = s;
            }
        }
        return;
    }
    for i in 0..(count as usize) {
        let s = *(src.add(i * src_stride) as *const Point3);
        *(dst.add(i * dst_stride) as *mut Point3) = m_ref.map_homogeneous_point(s);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_points_to_homogeneous(
    m: *const Matrix,
    dst: *mut Point3,
    src: *const Point,
    count: usize,
) {
    if count == 0 {
        return;
    }
    let dst_s = std::slice::from_raw_parts_mut(dst, count);
    let src_s = std::slice::from_raw_parts(src, count);
    (*m).map_points_to_homogeneous(dst_s, src_s);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_vectors(
    m: *const Matrix,
    dst: *mut Point,
    src: *const Point,
    count: usize,
) {
    if count == 0 {
        return;
    }
    if dst as *const Point == src {
        let slice = std::slice::from_raw_parts_mut(dst, count);
        (*m).map_vectors_in_place(slice);
    } else {
        let dst_s = std::slice::from_raw_parts_mut(dst, count);
        let src_s = std::slice::from_raw_parts(src, count);
        (*m).map_vectors(dst_s, src_s);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_rect_scale_translate(
    m: *const Matrix,
    dst: *mut Rect,
    src: *const Rect,
) {
    *dst = (*m).map_rect_scale_translate(*src);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_rect_non_perspective(
    m: *const Matrix,
    dst: *mut Rect,
    src: *const Rect,
    rect_stays_rect_out: *mut bool,
) -> bool {
    match (*m).map_rect_non_perspective(*src) {
        Some((r, stays)) => {
            *dst = r;
            *rect_stays_rect_out = stays;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_map_radius(m: *const Matrix, radius: f32) -> f32 {
    (*m).map_radius(radius)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_poly_to_poly(
    src: *const Point,
    src_len: usize,
    dst: *const Point,
    dst_len: usize,
    out: *mut Matrix,
) -> bool {
    let s = if src_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(src, src_len)
    };
    let d = if dst_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(dst, dst_len)
    };
    match Matrix::poly_to_poly(s, d) {
        Some(m) => {
            *out = m;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_get_min_scale(m: *const Matrix) -> f32 {
    (*m).get_min_scale()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_get_max_scale(m: *const Matrix) -> f32 {
    (*m).get_max_scale()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_get_min_max_scales(
    m: *const Matrix,
    out: *mut f32,
) -> bool {
    match (*m).get_min_max_scales() {
        Some(scales) => {
            *out = scales[0];
            *out.add(1) = scales[1];
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_decompose_scale(
    m: *const Matrix,
    scale_out: *mut Size,
    remaining_out: *mut Matrix,
) -> bool {
    match (*m).decompose_scale() {
        Some((sz, rem)) => {
            if !scale_out.is_null() {
                *scale_out = sz;
            }
            if !remaining_out.is_null() {
                *remaining_out = rem;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_decompose_upper_2x2(
    m: *const Matrix,
    rot1: *mut Point,
    scale: *mut Point,
    rot2: *mut Point,
) -> bool {
    match (*m).decompose_upper_2x2() {
        Some((r1, sc, r2)) => {
            if !rot1.is_null() {
                *rot1 = r1;
            }
            if !scale.is_null() {
                *scale = sc;
            }
            if !rot2.is_null() {
                *rot2 = r2;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_treat_as_sprite(
    m: *const Matrix,
    size: ISize,
    no_change_with_identity: bool,
    is_linear_filter: bool,
    is_anti_alias: bool,
) -> bool {
    (*m).treat_as_sprite(size, no_change_with_identity, is_linear_filter, is_anti_alias)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_differential_area_scale(
    m: *const Matrix,
    p: Point,
) -> f32 {
    (*m).differential_area_scale(p)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_nearly_affine(
    m: *const Matrix,
    bounds: *const Rect,
    tolerance: f32,
) -> bool {
    (*m).nearly_affine(*bounds, tolerance)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_matrix_compute_res_scale_for_stroking(m: *const Matrix) -> f32 {
    (*m).compute_res_scale_for_stroking()
}

// --- SkRRect ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_initialize_rect(rr: *mut RRect, rect: *const Rect) -> bool {
    (*rr).initialize_rect(*rect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_set_oval(rr: *mut RRect, oval: *const Rect) {
    (*rr).set_oval(*oval);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_set_rect_xy(
    rr: *mut RRect,
    rect: *const Rect,
    x_rad: f32,
    y_rad: f32,
) {
    (*rr).set_rect_xy(*rect, x_rad, y_rad);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_set_nine_patch(
    rr: *mut RRect,
    rect: *const Rect,
    left_rad: f32,
    top_rad: f32,
    right_rad: f32,
    bottom_rad: f32,
) {
    (*rr).set_nine_patch(*rect, left_rad, top_rad, right_rad, bottom_rad);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_set_rect_radii(
    rr: *mut RRect,
    rect: *const Rect,
    radii: *const Point,
) {
    let r_arr = &*(radii as *const [Point; 4]);
    (*rr).set_rect_radii(*rect, r_arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_scale_radii(rr: *mut RRect) -> bool {
    (*rr).scale_radii()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_compute_type(rr: *mut RRect) {
    (*rr).compute_type();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_check_corner_containment(
    rr: *const RRect,
    x: f32,
    y: f32,
) -> bool {
    (*rr).check_corner_containment(x, y)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_contains_point(rr: *const RRect, pt: Point) -> bool {
    (*rr).contains_point(pt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_contains_rect(rr: *const RRect, rect: *const Rect) -> bool {
    (*rr).contains_rect(*rect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_inset(
    rr: *const RRect,
    dx: f32,
    dy: f32,
    dst: *mut RRect,
) {
    *dst = (*rr).inset(dx, dy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_offset(rr: *mut RRect, dx: f32, dy: f32) {
    (*rr).offset(dx, dy);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_is_valid(rr: *const RRect) -> bool {
    (*rr).is_valid()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_are_rect_and_radii_valid(
    rect: *const Rect,
    radii: *const Point,
) -> bool {
    let r_arr = &*(radii as *const [Point; 4]);
    RRect::are_rect_and_radii_valid(*rect, r_arr)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_is_nearly_simple_circular(
    rr: *const RRect,
    tolerance: f32,
) -> bool {
    (*rr).is_nearly_simple_circular(tolerance)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_all_corners_circular(
    rr: *const RRect,
    tolerance: f32,
) -> bool {
    (*rr).all_corners_circular(tolerance)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_is_relatively_circular(
    rx: f32,
    ry: f32,
    tolerance: f32,
) -> bool {
    RRect::is_relatively_circular(rx, ry, tolerance)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_all_corners_relatively_circular(
    rr: *const RRect,
    tolerance: f32,
) -> bool {
    (*rr).all_corners_relatively_circular(tolerance)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_inner_bounds(rr: *const RRect) -> Rect {
    (*rr).inner_bounds()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_conservative_intersect(
    a: *const RRect,
    b: *const RRect,
    out: *mut RRect,
) {
    *out = RRect::conservative_intersect(&*a, &*b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_write_to_memory(rr: *const RRect, dst: *mut u8) -> usize {
    let arr = &mut *(dst as *mut [u8; SIZE_IN_MEMORY]);
    (*rr).write_to_bytes(arr);
    SIZE_IN_MEMORY
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rrect_read_from_memory(
    rr: *mut RRect,
    src: *const u8,
    len: usize,
) -> usize {
    if len < SIZE_IN_MEMORY || src.is_null() {
        return 0;
    }
    let slice = std::slice::from_raw_parts(src, len);
    (*rr).read_from_bytes(slice)
}
