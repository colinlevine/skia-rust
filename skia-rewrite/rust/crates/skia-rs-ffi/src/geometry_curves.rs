//! FFI exports for `SkGeometry`, clippers, and `SkVertices` (Layer 1.2 Batch 3).

use skia_rs_geometry::{
    cubic_clipper,
    edge_clipper::{EdgeClipOutput, EDGE_CLIPPER_MAX_POINTS, EDGE_CLIPPER_MAX_VERBS},
    geometry::{self, Conic, MAX_CONICS_FOR_ARC},
    line_clipper::{self, LINE_CLIPPER_MAX_POINTS},
    matrix::Matrix,
    point::Point,
    quad_clipper,
    rect::Rect,
    vertices::{self, VerticesSizes},
};

// --- SkGeometry: Quads ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_unit_quad_roots(
    a: f32,
    b: f32,
    c: f32,
    roots: *mut f32,
) -> i32 {
    let r = &mut *(roots as *mut [f32; 2]);
    geometry::find_unit_quad_roots(a, b, c, r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_eval_quad_at(src: *const Point, t: f32) -> Point {
    let s = &*(src as *const [Point; 3]);
    geometry::eval_quad_at(s, t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_eval_quad_tangent_at(src: *const Point, t: f32) -> Point {
    let s = &*(src as *const [Point; 3]);
    geometry::eval_quad_tangent_at(s, t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_quad_at(src: *const Point, dst: *mut Point, t: f32) {
    let s = &*(src as *const [Point; 3]);
    let d = &mut *(dst as *mut [Point; 5]);
    geometry::chop_quad_at(s, d, t);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_measure_angle_between_vectors(a: Point, b: Point) -> f32 {
    geometry::measure_angle_between_vectors(a, b)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_bisector(a: Point, b: Point) -> Point {
    geometry::find_bisector(a, b)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_quad_mid_tangent(src: *const Point) -> f32 {
    let s = &*(src as *const [Point; 3]);
    geometry::find_quad_mid_tangent(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_quad_extrema(
    a: f32,
    b: f32,
    c: f32,
    t_value: *mut f32,
) -> i32 {
    geometry::find_quad_extrema(a, b, c, &mut *t_value)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_quad_at_y_extrema(
    src: *const Point,
    dst: *mut Point,
) -> i32 {
    let s = &*(src as *const [Point; 3]);
    let d = &mut *(dst as *mut [Point; 5]);
    geometry::chop_quad_at_y_extrema(s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_quad_at_x_extrema(
    src: *const Point,
    dst: *mut Point,
) -> i32 {
    let s = &*(src as *const [Point; 3]);
    let d = &mut *(dst as *mut [Point; 5]);
    geometry::chop_quad_at_x_extrema(s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_quad_max_curvature(src: *const Point) -> f32 {
    let s = &*(src as *const [Point; 3]);
    geometry::find_quad_max_curvature(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_quad_at_max_curvature(
    src: *const Point,
    dst: *mut Point,
) -> i32 {
    let s = &*(src as *const [Point; 3]);
    let d = &mut *(dst as *mut [Point; 5]);
    geometry::chop_quad_at_max_curvature(s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_convert_quad_to_cubic(src: *const Point, dst: *mut Point) {
    let s = &*(src as *const [Point; 3]);
    let d = &mut *(dst as *mut [Point; 4]);
    geometry::convert_quad_to_cubic(s, d);
}

// --- SkGeometry: Cubics ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_eval_cubic_at(
    src: *const Point,
    t: f32,
    loc: *mut Point,
    tangent: *mut Point,
    curvature: *mut Point,
) {
    let s = &*(src as *const [Point; 4]);
    geometry::eval_cubic_at(
        s,
        t,
        if loc.is_null() { None } else { Some(&mut *loc) },
        if tangent.is_null() {
            None
        } else {
            Some(&mut *tangent)
        },
        if curvature.is_null() {
            None
        } else {
            Some(&mut *curvature)
        },
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_cubic_extrema(
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    t_values: *mut f32,
) -> i32 {
    let tv = &mut *(t_values as *mut [f32; 2]);
    geometry::find_cubic_extrema(a, b, c, d, tv)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_1(src: *const Point, dst: *mut Point, t: f32) {
    let s = &*(src as *const [Point; 4]);
    let d = &mut *(dst as *mut [Point; 7]);
    geometry::chop_cubic_at_1(s, d, t);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_2(
    src: *const Point,
    dst: *mut Point,
    t0: f32,
    t1: f32,
) {
    let s = &*(src as *const [Point; 4]);
    let d = &mut *(dst as *mut [Point; 10]);
    geometry::chop_cubic_at_2(s, d, t0, t1);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_many(
    src: *const Point,
    dst: *mut Point,
    t_values: *const f32,
    t_count: i32,
) {
    if dst.is_null() || t_count < 0 {
        return;
    }
    let s = &*(src as *const [Point; 4]);
    let tc = t_count as usize;
    let tv = if tc == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(t_values, tc)
    };
    let d = std::slice::from_raw_parts_mut(dst, 3 * tc + 4);
    geometry::chop_cubic_at_many(s, d, tv);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_measure_non_inflect_cubic_rotation(pts: *const Point) -> f32 {
    let s = &*(pts as *const [Point; 4]);
    geometry::measure_non_inflect_cubic_rotation(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_cubic_mid_tangent(src: *const Point) -> f32 {
    let s = &*(src as *const [Point; 4]);
    geometry::find_cubic_mid_tangent(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_y_extrema(
    src: *const Point,
    dst: *mut Point,
) -> i32 {
    let s = &*(src as *const [Point; 4]);
    let d = if dst.is_null() {
        None
    } else {
        Some(&mut *(dst as *mut [Point; 10]))
    };
    geometry::chop_cubic_at_y_extrema(s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_x_extrema(
    src: *const Point,
    dst: *mut Point,
) -> i32 {
    let s = &*(src as *const [Point; 4]);
    let d = if dst.is_null() {
        None
    } else {
        Some(&mut *(dst as *mut [Point; 10]))
    };
    geometry::chop_cubic_at_x_extrema(s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_cubic_inflections(
    src: *const Point,
    t_values: *mut f32,
) -> i32 {
    let s = &*(src as *const [Point; 4]);
    let tv = &mut *(t_values as *mut [f32; 2]);
    geometry::find_cubic_inflections(s, tv)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_inflections(
    src: *const Point,
    dst: *mut Point,
) -> i32 {
    let s = &*(src as *const [Point; 4]);
    let d = if dst.is_null() {
        None
    } else {
        Some(&mut *(dst as *mut [Point; 10]))
    };
    geometry::chop_cubic_at_inflections(s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_classify_cubic(
    p: *const Point,
    t: *mut f64,
    s: *mut f64,
    d: *mut f64,
) -> i32 {
    let pts = &*(p as *const [Point; 4]);
    let t_opt = if t.is_null() {
        None
    } else {
        Some(&mut *(t as *mut [f64; 2]))
    };
    let s_opt = if s.is_null() {
        None
    } else {
        Some(&mut *(s as *mut [f64; 2]))
    };
    let d_opt = if d.is_null() {
        None
    } else {
        Some(&mut *(d as *mut [f64; 4]))
    };
    geometry::classify_cubic(pts, t_opt, s_opt, d_opt) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_cubic_max_curvature(
    src: *const Point,
    t_values: *mut f32,
) -> i32 {
    let s = &*(src as *const [Point; 4]);
    let tv = &mut *(t_values as *mut [f32; 3]);
    geometry::find_cubic_max_curvature(s, tv)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_cubic_at_max_curvature(
    src: *const Point,
    dst: *mut Point,
    t_values: *mut f32,
) -> i32 {
    let s = &*(src as *const [Point; 4]);
    let d = if dst.is_null() {
        None
    } else {
        Some(&mut *(dst as *mut [Point; 13]))
    };
    let tv = if t_values.is_null() {
        None
    } else {
        Some(&mut *(t_values as *mut [f32; 3]))
    };
    geometry::chop_cubic_at_max_curvature(s, d, tv)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_find_cubic_cusp(src: *const Point) -> f32 {
    let s = &*(src as *const [Point; 4]);
    geometry::find_cubic_cusp(s)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_mono_cubic_at_y(
    src: *const Point,
    y: f32,
    dst: *mut Point,
) -> bool {
    let s = &*(src as *const [Point; 4]);
    let d = &mut *(dst as *mut [Point; 7]);
    geometry::chop_mono_cubic_at_y(s, y, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_chop_mono_cubic_at_x(
    src: *const Point,
    x: f32,
    dst: *mut Point,
) -> bool {
    let s = &*(src as *const [Point; 4]);
    let d = &mut *(dst as *mut [Point; 7]);
    geometry::chop_mono_cubic_at_x(s, x, d)
}

// --- SkGeometry: Conics ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_chop_at(
    conic: *const Conic,
    t: f32,
    dst: *mut Conic,
) -> bool {
    let d = &mut *(dst as *mut [Conic; 2]);
    (*conic).chop_at(t, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_chop_at_range(
    conic: *const Conic,
    t1: f32,
    t2: f32,
    dst: *mut Conic,
) {
    (*conic).chop_at_range(t1, t2, &mut *dst);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_eval_at(conic: *const Conic, t: f32) -> Point {
    (*conic).eval_at(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_eval_tangent_at(conic: *const Conic, t: f32) -> Point {
    (*conic).eval_tangent_at(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_chop(conic: *const Conic, dst: *mut Conic) {
    let d = &mut *(dst as *mut [Conic; 2]);
    (*conic).chop(d);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_compute_as_quad_error(conic: *const Conic) -> Point {
    (*conic).compute_as_quad_error()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_as_quad_tol(conic: *const Conic, tol: f32) -> bool {
    (*conic).as_quad_tol(tol)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_compute_quad_pow2(conic: *const Conic, tol: f32) -> i32 {
    (*conic).compute_quad_pow2(tol)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_chop_into_quads_pow2(
    conic: *const Conic,
    pts: *mut Point,
    pow2: i32,
) -> i32 {
    let max_pts = 1 + 2 * (1usize << (pow2.clamp(0, geometry::MAX_CONIC_TO_QUAD_POW2) as u32));
    let slice = std::slice::from_raw_parts_mut(pts, max_pts);
    (*conic).chop_into_quads_pow2(slice, pow2)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_find_mid_tangent(conic: *const Conic) -> f32 {
    (*conic).find_mid_tangent()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_find_x_extrema(
    conic: *const Conic,
    t_out: *mut f32,
) -> bool {
    match (*conic).find_x_extrema() {
        Some(t) => {
            *t_out = t;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_find_y_extrema(
    conic: *const Conic,
    t_out: *mut f32,
) -> bool {
    match (*conic).find_y_extrema() {
        Some(t) => {
            *t_out = t;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_chop_at_x_extrema(
    conic: *const Conic,
    dst: *mut Conic,
) -> bool {
    let d = &mut *(dst as *mut [Conic; 2]);
    (*conic).chop_at_x_extrema(d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_chop_at_y_extrema(
    conic: *const Conic,
    dst: *mut Conic,
) -> bool {
    let d = &mut *(dst as *mut [Conic; 2]);
    (*conic).chop_at_y_extrema(d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_compute_tight_bounds(conic: *const Conic) -> Rect {
    (*conic).compute_tight_bounds()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_compute_fast_bounds(conic: *const Conic) -> Rect {
    (*conic).compute_fast_bounds()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_transform_w(
    pts: *const Point,
    w: f32,
    matrix: *const Matrix,
) -> f32 {
    let p = &*(pts as *const [Point; 3]);
    Conic::transform_w(p, w, &*matrix)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_conic_build_unit_arc(
    u_start: Point,
    u_stop: Point,
    dir_is_ccw: bool,
    user_matrix: *const Matrix,
    dst: *mut Conic,
) -> i32 {
    let um = if user_matrix.is_null() {
        None
    } else {
        Some(&*user_matrix)
    };
    let d = &mut *(dst as *mut [Conic; MAX_CONICS_FOR_ARC]);
    Conic::build_unit_arc(u_start, u_stop, dir_is_ccw, um, d)
}

// --- Clippers ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_line_clipper_intersect_line(
    src: *const Point,
    clip: *const Rect,
    dst: *mut Point,
) -> bool {
    let s = &*(src as *const [Point; 2]);
    let mut tmp = [Point::ZERO; 2];
    let ok = line_clipper::intersect_line(s, *clip, &mut tmp);
    if ok {
        std::ptr::copy_nonoverlapping(tmp.as_ptr(), dst, 2);
    }
    ok
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_line_clipper_clip_line(
    pts: *const Point,
    clip: *const Rect,
    lines: *mut Point,
    can_cull_to_the_right: bool,
) -> i32 {
    let p = &*(pts as *const [Point; 2]);
    let out = &mut *(lines as *mut [Point; LINE_CLIPPER_MAX_POINTS]);
    line_clipper::clip_line(p, *clip, out, can_cull_to_the_right)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_quad_clipper_clip_quad(
    clip: *const Rect,
    src_pts: *const Point,
    dst: *mut Point,
) -> bool {
    let s = &*(src_pts as *const [Point; 3]);
    let d = &mut *(dst as *mut [Point; 3]);
    quad_clipper::clip_quad(*clip, s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_clipper_chop_mono_at_y(
    pts: *const Point,
    y: f32,
    t_out: *mut f32,
) -> bool {
    let p = &*(pts as *const [Point; 4]);
    match cubic_clipper::chop_mono_at_y(p, y) {
        Some(t) => {
            *t_out = t;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_clipper_clip_cubic(
    clip: *const Rect,
    src_pts: *const Point,
    dst: *mut Point,
) -> bool {
    let s = &*(src_pts as *const [Point; 4]);
    let d = &mut *(dst as *mut [Point; 4]);
    cubic_clipper::clip_cubic(*clip, s, d)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_clipper_clip_line(
    p0: Point,
    p1: Point,
    clip: *const Rect,
    can_cull_to_the_right: bool,
    verbs_out: *mut u8,
    verb_count_out: *mut usize,
    pts_out: *mut Point,
) -> bool {
    let mut out = EdgeClipOutput::new(can_cull_to_the_right);
    let ok = out.clip_line(p0, p1, *clip);
    std::ptr::copy_nonoverlapping(out.verbs.as_ptr(), verbs_out, out.verb_count.min(EDGE_CLIPPER_MAX_VERBS));
    std::ptr::copy_nonoverlapping(out.points.as_ptr(), pts_out, out.point_count.min(EDGE_CLIPPER_MAX_POINTS));
    *verb_count_out = out.verb_count;
    ok
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_clipper_clip_quad(
    src_pts: *const Point,
    clip: *const Rect,
    can_cull_to_the_right: bool,
    verbs_out: *mut u8,
    verb_count_out: *mut usize,
    pts_out: *mut Point,
) -> bool {
    let s = &*(src_pts as *const [Point; 3]);
    let mut out = EdgeClipOutput::new(can_cull_to_the_right);
    let ok = out.clip_quad(s, *clip);
    std::ptr::copy_nonoverlapping(out.verbs.as_ptr(), verbs_out, out.verb_count.min(EDGE_CLIPPER_MAX_VERBS));
    std::ptr::copy_nonoverlapping(out.points.as_ptr(), pts_out, out.point_count.min(EDGE_CLIPPER_MAX_POINTS));
    *verb_count_out = out.verb_count;
    ok
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_clipper_clip_cubic(
    src_pts: *const Point,
    clip: *const Rect,
    can_cull_to_the_right: bool,
    verbs_out: *mut u8,
    verb_count_out: *mut usize,
    pts_out: *mut Point,
) -> bool {
    let s = &*(src_pts as *const [Point; 4]);
    let mut out = EdgeClipOutput::new(can_cull_to_the_right);
    let ok = out.clip_cubic(s, *clip);
    std::ptr::copy_nonoverlapping(out.verbs.as_ptr(), verbs_out, out.verb_count.min(EDGE_CLIPPER_MAX_VERBS));
    std::ptr::copy_nonoverlapping(out.points.as_ptr(), pts_out, out.point_count.min(EDGE_CLIPPER_MAX_POINTS));
    *verb_count_out = out.verb_count;
    ok
}

// --- SkVertices helpers ---

#[no_mangle]
pub extern "C" fn skia_rs_vertices_next_id() -> u32 {
    vertices::next_vertices_id()
}

#[no_mangle]
pub extern "C" fn skia_rs_vertices_compute_sizes(
    obj_size: usize,
    mode: i32,
    vertex_count: i32,
    index_count: i32,
    has_texs: bool,
    has_colors: bool,
) -> VerticesSizes {
    VerticesSizes::compute(obj_size, mode, vertex_count, index_count, has_texs, has_colors)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_vertices_convert_tri_fan(
    src_indices: *const u16,
    src_index_count: i32,
    vertex_count: i32,
    dst_indices: *mut u16,
    dst_cap: usize,
) -> i32 {
    let dst = std::slice::from_raw_parts_mut(dst_indices, dst_cap);
    let src = if src_indices.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(src_indices, src_index_count as usize))
    };
    vertices::convert_tri_fan_indices(src, vertex_count, dst)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_vertices_clamp_indices(
    src: *const u16,
    dst: *mut u16,
    count: usize,
    vertex_count: i32,
) {
    if count == 0 {
        return;
    }
    let s = std::slice::from_raw_parts(src, count);
    let d = std::slice::from_raw_parts_mut(dst, count);
    vertices::clamp_indices(s, d, vertex_count);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_vertices_validate_indices(
    indices: *const u16,
    count: usize,
    vertex_count: i32,
) -> bool {
    if count == 0 {
        return true;
    }
    let s = std::slice::from_raw_parts(indices, count);
    vertices::validate_indices(s, vertex_count)
}
