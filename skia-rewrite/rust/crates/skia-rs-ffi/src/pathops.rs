//! FFI exports for Phase 1 Layer 3.1 (`skia-rs-pathops`):
//!   All 32 `.cpp` files in `src/pathops/`.

use skia_rs_geometry::{Point, Rect};
use skia_rs_path::types::{
    compute_segment_mask, Convexity, Direction, FillType, PathDataBuf, PathIsAType, Verb,
};
use skia_rs_pathops::{
    arena::OpState,
    as_winding::as_winding,
    conic::DConic,
    cubic::{DCubic, SearchAxis},
    curve::{
        curve_dpoint_at_t, curve_dslope_at_t, curve_point_at_t, DCurve, ReduceOrder,
        ReduceQuadratics,
    },
    intersections::Intersections,
    line::DLine,
    op::op,
    op_builder::{fix_winding, OpBuilder},
    quad::DQuad,
    simplify::simplify,
    tight_bounds::compute_tight_bounds,
    types::{
        almost_bequal_ulps_f32, almost_between_ulps_f32, almost_dequal_ulps_f32,
        almost_dequal_ulps_f64, almost_equal_ulps_f32, almost_equal_ulps_no_normal_check_f32,
        almost_equal_ulps_pin_f32, almost_less_or_equal_ulps_f32, almost_less_ulps_f32,
        almost_pequal_ulps_f32, not_almost_dequal_ulps_f32, not_almost_equal_ulps_f32,
        not_almost_equal_ulps_pin_f32, roughly_equal_ulps_f32, ulps_distance_f32, DPoint, DRect,
        DVector, PathOp, PathOpsBounds,
    },
};

#[inline]
unsafe fn slice_from_raw<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 || ptr.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(ptr, len)
    }
}

#[inline]
unsafe fn slice_from_raw_mut<'a, T>(ptr: *mut T, len: usize) -> &'a mut [T] {
    if len == 0 || ptr.is_null() {
        &mut []
    } else {
        core::slice::from_raw_parts_mut(ptr, len)
    }
}

#[inline]
unsafe fn verbs_from_raw(ptr: *const u8, len: usize) -> Vec<Verb> {
    let raw = slice_from_raw(ptr, len);
    raw.iter()
        .filter_map(|&b| Verb::from_u8(b))
        .collect()
}

#[inline]
unsafe fn path_buf_from_raw(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
) -> PathDataBuf {
    let points = slice_from_raw(pts, pt_count).to_vec();
    let verbs = verbs_from_raw(vbs, vb_count);
    let conics = slice_from_raw(conics, conic_count).to_vec();
    let segment_mask = compute_segment_mask(&verbs);
    PathDataBuf {
        points,
        verbs,
        conics,
        fill_type: FillType::from_u8(fill_type).unwrap_or(FillType::Winding),
        convexity: Convexity::from_u8(convexity).unwrap_or(Convexity::Unknown),
        segment_mask,
        last_move_index: -1,
        is_a_type: PathIsAType::General,
        is_a_dir: Direction::Cw,
        is_a_start_index: 0,
        is_volatile: false,
    }
}

// ============================================================================
// Opaque PathOps output buffer
// ============================================================================

pub struct SkiaRsPathOpsBuf {
    buf: PathDataBuf,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_buf_points(
    buf: *const SkiaRsPathOpsBuf,
    out_len: *mut usize,
) -> *const Point {
    if buf.is_null() {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return core::ptr::null();
    }
    let pts = &(*buf).buf.points;
    if !out_len.is_null() {
        *out_len = pts.len();
    }
    pts.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_buf_verbs(
    buf: *const SkiaRsPathOpsBuf,
    out_len: *mut usize,
) -> *const u8 {
    if buf.is_null() {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return core::ptr::null();
    }
    let vbs = &(*buf).buf.verbs;
    if !out_len.is_null() {
        *out_len = vbs.len();
    }
    vbs.as_ptr() as *const u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_buf_conics(
    buf: *const SkiaRsPathOpsBuf,
    out_len: *mut usize,
) -> *const f32 {
    if buf.is_null() {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return core::ptr::null();
    }
    let cns = &(*buf).buf.conics;
    if !out_len.is_null() {
        *out_len = cns.len();
    }
    cns.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_buf_fill_type(buf: *const SkiaRsPathOpsBuf) -> u8 {
    if buf.is_null() {
        return 0;
    }
    (*buf).buf.fill_type as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_buf_free(buf: *mut SkiaRsPathOpsBuf) {
    if !buf.is_null() {
        drop(Box::from_raw(buf));
    }
}

// ============================================================================
// Top-level PathOps entry points: Op, Simplify, AsWinding, TightBounds, OpBuilder
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_op(
    p1_pts: *const Point,
    p1_pt_count: usize,
    p1_vbs: *const u8,
    p1_vb_count: usize,
    p1_cns: *const f32,
    p1_cn_count: usize,
    p1_ft: u8,
    p1_conv: u8,
    p2_pts: *const Point,
    p2_pt_count: usize,
    p2_vbs: *const u8,
    p2_vb_count: usize,
    p2_cns: *const f32,
    p2_cn_count: usize,
    p2_ft: u8,
    p2_conv: u8,
    path_op: u8,
) -> *mut SkiaRsPathOpsBuf {
    let op_enum = match PathOp::from_u8(path_op) {
        Some(o) => o,
        None => return core::ptr::null_mut(),
    };
    let one = path_buf_from_raw(
        p1_pts,
        p1_pt_count,
        p1_vbs,
        p1_vb_count,
        p1_cns,
        p1_cn_count,
        p1_ft,
        p1_conv,
    );
    let two = path_buf_from_raw(
        p2_pts,
        p2_pt_count,
        p2_vbs,
        p2_vb_count,
        p2_cns,
        p2_cn_count,
        p2_ft,
        p2_conv,
    );
    match op(&one, &two, op_enum) {
        Some(buf) => Box::into_raw(Box::new(SkiaRsPathOpsBuf { buf })),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_simplify(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    cns: *const f32,
    cn_count: usize,
    ft: u8,
    conv: u8,
) -> *mut SkiaRsPathOpsBuf {
    let path = path_buf_from_raw(pts, pt_count, vbs, vb_count, cns, cn_count, ft, conv);
    match simplify(&path) {
        Some(buf) => Box::into_raw(Box::new(SkiaRsPathOpsBuf { buf })),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_as_winding(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    cns: *const f32,
    cn_count: usize,
    ft: u8,
    conv: u8,
) -> *mut SkiaRsPathOpsBuf {
    let path = path_buf_from_raw(pts, pt_count, vbs, vb_count, cns, cn_count, ft, conv);
    match as_winding(&path) {
        Some(buf) => Box::into_raw(Box::new(SkiaRsPathOpsBuf { buf })),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_tight_bounds(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    cns: *const f32,
    cn_count: usize,
    ft: u8,
    out_rect: *mut Rect,
) -> bool {
    let path = path_buf_from_raw(
        pts,
        pt_count,
        vbs,
        vb_count,
        cns,
        cn_count,
        ft,
        Convexity::Unknown as u8,
    );
    match compute_tight_bounds(&path) {
        Some(r) => {
            if !out_rect.is_null() {
                *out_rect = r;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_fix_winding(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    cns: *const f32,
    cn_count: usize,
    ft: u8,
    conv: u8,
    out_buf: *mut *mut SkiaRsPathOpsBuf,
) -> bool {
    let mut path = path_buf_from_raw(pts, pt_count, vbs, vb_count, cns, cn_count, ft, conv);
    if fix_winding(&mut path) {
        if !out_buf.is_null() {
            *out_buf = Box::into_raw(Box::new(SkiaRsPathOpsBuf { buf: path }));
        }
        true
    } else {
        false
    }
}

#[repr(C)]
pub struct FfiPathInput {
    pub pts: *const Point,
    pub pt_count: usize,
    pub vbs: *const u8,
    pub vb_count: usize,
    pub cns: *const f32,
    pub cn_count: usize,
    pub fill_type: u8,
    pub convexity: u8,
    pub op: u8,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_builder_resolve(
    inputs: *const FfiPathInput,
    count: usize,
) -> *mut SkiaRsPathOpsBuf {
    let items = slice_from_raw(inputs, count);
    let mut builder = OpBuilder::new();
    for item in items {
        let p = path_buf_from_raw(
            item.pts,
            item.pt_count,
            item.vbs,
            item.vb_count,
            item.cns,
            item.cn_count,
            item.fill_type,
            item.convexity,
        );
        let op_enum = PathOp::from_u8(item.op).unwrap_or(PathOp::Union);
        builder.add(p, op_enum);
    }
    match builder.resolve() {
        Some(buf) => Box::into_raw(Box::new(SkiaRsPathOpsBuf { buf })),
        None => core::ptr::null_mut(),
    }
}

// ============================================================================
// ULP / Types helpers (SkPathOpsTypes.cpp)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_bequal_ulps(a: f32, b: f32) -> bool {
    almost_bequal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_pequal_ulps(a: f32, b: f32) -> bool {
    almost_pequal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_dequal_ulps_f32(a: f32, b: f32) -> bool {
    almost_dequal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_dequal_ulps_f64(a: f64, b: f64) -> bool {
    almost_dequal_ulps_f64(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_equal_ulps(a: f32, b: f32) -> bool {
    almost_equal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_equal_ulps_no_normal_check(a: f32, b: f32) -> bool {
    almost_equal_ulps_no_normal_check_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_equal_ulps_pin(a: f32, b: f32) -> bool {
    almost_equal_ulps_pin_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_not_almost_equal_ulps(a: f32, b: f32) -> bool {
    not_almost_equal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_not_almost_equal_ulps_pin(a: f32, b: f32) -> bool {
    not_almost_equal_ulps_pin_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_not_almost_dequal_ulps(a: f32, b: f32) -> bool {
    not_almost_dequal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_roughly_equal_ulps(a: f32, b: f32) -> bool {
    roughly_equal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_between_ulps(a: f32, b: f32, c: f32) -> bool {
    almost_between_ulps_f32(a, b, c)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_less_ulps(a: f32, b: f32) -> bool {
    almost_less_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_almost_less_or_equal_ulps(a: f32, b: f32) -> bool {
    almost_less_or_equal_ulps_f32(a, b)
}

#[no_mangle]
pub extern "C" fn skia_rs_pathops_ulps_distance(a: f32, b: f32) -> i32 {
    ulps_distance_f32(a, b)
}

// ============================================================================
// DLine / DQuad / DConic / DCubic / DCurve / DRect / ReduceOrder
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dline_pt_at_t(pts: *const DPoint, t: f64) -> DPoint {
    let p = slice_from_raw(pts, 2);
    DLine::new(p[0], p[1]).pt_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dline_exact_point(pts: *const DPoint, xy: DPoint) -> f64 {
    let p = slice_from_raw(pts, 2);
    DLine::new(p[0], p[1]).exact_point(xy)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dline_near_point(
    pts: *const DPoint,
    xy: DPoint,
    unequal: *mut bool,
) -> f64 {
    let p = slice_from_raw(pts, 2);
    let line = DLine::new(p[0], p[1]);
    if unequal.is_null() {
        line.near_point(xy, None)
    } else {
        line.near_point(xy, Some(&mut *unequal))
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dline_near_ray(pts: *const DPoint, xy: DPoint) -> bool {
    let p = slice_from_raw(pts, 2);
    DLine::new(p[0], p[1]).near_ray(xy)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_add_valid_ts(
    s: *const f64,
    real_roots: i32,
    t: *mut f64,
) -> i32 {
    let n = real_roots.max(0) as usize;
    let s_slice = slice_from_raw(s, n);
    let t_slice = slice_from_raw_mut(t, n.max(3));
    DQuad::add_valid_ts(s_slice, n, t_slice) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_roots_real(
    a: f64,
    b: f64,
    c: f64,
    s: *mut f64,
) -> i32 {
    let mut out = [0.0f64; 2];
    let n = DQuad::roots_real(a, b, c, &mut out);
    let dst = slice_from_raw_mut(s, 2);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_roots_valid_t(
    a: f64,
    b: f64,
    c: f64,
    t: *mut f64,
) -> i32 {
    let mut out = [0.0f64; 2];
    let n = DQuad::roots_valid_t(a, b, c, &mut out);
    let dst = slice_from_raw_mut(t, 2);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_pt_at_t(pts: *const DPoint, t: f64) -> DPoint {
    let p = slice_from_raw(pts, 3);
    DQuad::new(p[0], p[1], p[2]).pt_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_dxdy_at_t(pts: *const DPoint, t: f64) -> DVector {
    let p = slice_from_raw(pts, 3);
    DQuad::new(p[0], p[1], p[2]).dxdy_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_sub_divide(
    pts: *const DPoint,
    t1: f64,
    t2: f64,
    out_pts: *mut DPoint,
) {
    let p = slice_from_raw(pts, 3);
    let sub = DQuad::new(p[0], p[1], p[2]).sub_divide(t1, t2);
    let dst = slice_from_raw_mut(out_pts, 3);
    dst.copy_from_slice(&sub.pts);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dquad_find_extrema(
    a: f64,
    b: f64,
    c: f64,
    t_out: *mut f64,
) -> i32 {
    let mut t = [0.0f64; 1];
    let n = DQuad::find_extrema(a, b, c, &mut t);
    if n > 0 && !t_out.is_null() {
        *t_out = t[0];
    }
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dconic_pt_at_t(
    pts: *const DPoint,
    weight: f32,
    t: f64,
) -> DPoint {
    let p = slice_from_raw(pts, 3);
    DConic::new(p[0], p[1], p[2], weight).pt_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dconic_dxdy_at_t(
    pts: *const DPoint,
    weight: f32,
    t: f64,
) -> DVector {
    let p = slice_from_raw(pts, 3);
    DConic::new(p[0], p[1], p[2], weight).dxdy_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dconic_sub_divide(
    pts: *const DPoint,
    weight: f32,
    t1: f64,
    t2: f64,
    out_pts: *mut DPoint,
    out_weight: *mut f32,
) {
    let p = slice_from_raw(pts, 3);
    let sub = DConic::new(p[0], p[1], p[2], weight).sub_divide(t1, t2);
    let dst = slice_from_raw_mut(out_pts, 3);
    dst.copy_from_slice(&sub.pts.pts);
    if !out_weight.is_null() {
        *out_weight = sub.weight;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_coefficients(
    s0: f64,
    s1: f64,
    s2: f64,
    s3: f64,
    a: *mut f64,
    b: *mut f64,
    c: *mut f64,
    d: *mut f64,
) {
    let (ca, cb, cc, cd) = DCubic::coefficients(s0, s1, s2, s3);
    if !a.is_null() {
        *a = ca;
    }
    if !b.is_null() {
        *b = cb;
    }
    if !c.is_null() {
        *c = cc;
    }
    if !d.is_null() {
        *d = cd;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_find_extrema(
    s0: f64,
    s1: f64,
    s2: f64,
    s3: f64,
    t_values: *mut f64,
) -> i32 {
    let mut out = [0.0f64; 2];
    let n = DCubic::find_extrema(s0, s1, s2, s3, &mut out);
    let dst = slice_from_raw_mut(t_values, 2);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_roots_valid_t(
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    t: *mut f64,
) -> i32 {
    let mut out = [0.0f64; 3];
    let n = DCubic::roots_valid_t(a, b, c, d, &mut out);
    let dst = slice_from_raw_mut(t, 3);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_roots_real(
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    s: *mut f64,
) -> i32 {
    let mut out = [0.0f64; 3];
    let n = DCubic::roots_real(a, b, c, d, &mut out);
    let dst = slice_from_raw_mut(s, 3);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_complex_break(
    points: *const Point,
    t: *mut f32,
) -> i32 {
    let p = slice_from_raw(points, 4);
    let mut out = [0.0f32; 3];
    let n = DCubic::complex_break(&[p[0], p[1], p[2], p[3]], &mut out);
    let dst = slice_from_raw_mut(t, n);
    dst.copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_convex_hull(
    pts: *const DPoint,
    order: *mut i8,
) -> i32 {
    let p = slice_from_raw(pts, 4);
    let mut out = [0u8; 4];
    let n = DCubic::new(p[0], p[1], p[2], p[3]).convex_hull(&mut out);
    let dst = slice_from_raw_mut(order, 4);
    for i in 0..4 {
        dst[i] = out[i] as i8;
    }
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_search_roots(
    pts: *const DPoint,
    extremas: *mut f64,
    extrema_count: i32,
    axis_intercept: f64,
    axis: i32,
    valid_roots: *mut f64,
) -> i32 {
    let p = slice_from_raw(pts, 4);
    let c = DCubic::new(p[0], p[1], p[2], p[3]);
    let mut ext = [0.0f64; 6];
    let ec = (extrema_count.max(0) as usize).min(6);
    if !extremas.is_null() && ec > 0 {
        let src = slice_from_raw(extremas, ec);
        ext[..ec].copy_from_slice(src);
    }
    let sa = if axis == 0 {
        SearchAxis::XAxis
    } else {
        SearchAxis::YAxis
    };
    let mut vr = [0.0f64; 3];
    let n = c.search_roots(&mut ext, ec, axis_intercept, sa, &mut vr);
    if !valid_roots.is_null() && n > 0 {
        let dst = slice_from_raw_mut(valid_roots, n);
        dst.copy_from_slice(&vr[..n]);
    }
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_calc_precision(pts: *const DPoint) -> f64 {
    let p = slice_from_raw(pts, 4);
    DCubic::new(p[0], p[1], p[2], p[3]).calc_precision()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_find_inflections(
    pts: *const DPoint,
    t_values: *mut f64,
) -> i32 {
    let p = slice_from_raw(pts, 4);
    let mut out = [0.0f64; 2];
    let n = DCubic::new(p[0], p[1], p[2], p[3]).find_inflections(&mut out);
    let dst = slice_from_raw_mut(t_values, 2);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_find_max_curvature(
    pts: *const DPoint,
    t_values: *mut f64,
) -> i32 {
    let p = slice_from_raw(pts, 4);
    let mut out = [0.0f64; 3];
    let n = DCubic::new(p[0], p[1], p[2], p[3]).find_max_curvature(&mut out);
    let dst = slice_from_raw_mut(t_values, 3);
    dst[..n].copy_from_slice(&out[..n]);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_ends_are_extrema(pts: *const DPoint) -> bool {
    let p = slice_from_raw(pts, 4);
    DCubic::new(p[0], p[1], p[2], p[3]).ends_are_extrema_in_x_or_y()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_pt_at_t(pts: *const DPoint, t: f64) -> DPoint {
    let p = slice_from_raw(pts, 4);
    DCubic::new(p[0], p[1], p[2], p[3]).pt_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_dxdy_at_t(pts: *const DPoint, t: f64) -> DVector {
    let p = slice_from_raw(pts, 4);
    DCubic::new(p[0], p[1], p[2], p[3]).dxdy_at_t(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_chop_at(
    pts: *const DPoint,
    t: f64,
    out_pts: *mut DPoint,
) {
    let p = slice_from_raw(pts, 4);
    let pair = DCubic::new(p[0], p[1], p[2], p[3]).chop_at(t);
    let dst = slice_from_raw_mut(out_pts, 7);
    dst.copy_from_slice(&pair.pts);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_to_quad(
    pts: *const DPoint,
    out_pts: *mut DPoint,
) {
    let p = slice_from_raw(pts, 4);
    let q = DCubic::new(p[0], p[1], p[2], p[3]).to_quad();
    let dst = slice_from_raw_mut(out_pts, 3);
    dst.copy_from_slice(&q.pts);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcubic_sub_divide(
    pts: *const DPoint,
    t1: f64,
    t2: f64,
    out_pts: *mut DPoint,
) {
    let p = slice_from_raw(pts, 4);
    let sub = DCubic::new(p[0], p[1], p[2], p[3]).sub_divide(t1, t2);
    let dst = slice_from_raw_mut(out_pts, 4);
    dst.copy_from_slice(&sub.pts);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_drect_set_quad_bounds(
    curve: *const DPoint,
    sub: *const DPoint,
    t1: f64,
    t2: f64,
    out_rect: *mut DRect,
) {
    let cp = slice_from_raw(curve, 3);
    let sp = slice_from_raw(sub, 3);
    let q = DQuad::new(cp[0], cp[1], cp[2]);
    let sq = DQuad::new(sp[0], sp[1], sp[2]);
    if !out_rect.is_null() {
        *out_rect = q.set_bounds(&sq, t1, t2);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_drect_set_conic_bounds(
    curve: *const DPoint,
    cw: f32,
    sub: *const DPoint,
    sw: f32,
    t1: f64,
    t2: f64,
    out_rect: *mut DRect,
) {
    let cp = slice_from_raw(curve, 3);
    let sp = slice_from_raw(sub, 3);
    let c = DConic::new(cp[0], cp[1], cp[2], cw);
    let sc = DConic::new(sp[0], sp[1], sp[2], sw);
    if !out_rect.is_null() {
        *out_rect = c.set_bounds(&sc, t1, t2);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_drect_set_cubic_bounds(
    curve: *const DPoint,
    sub: *const DPoint,
    t1: f64,
    t2: f64,
    out_rect: *mut DRect,
) {
    let cp = slice_from_raw(curve, 4);
    let sp = slice_from_raw(sub, 4);
    let c = DCubic::new(cp[0], cp[1], cp[2], cp[3]);
    let sc = DCubic::new(sp[0], sp[1], sp[2], sp[3]);
    if !out_rect.is_null() {
        *out_rect = c.set_bounds(&sc, t1, t2);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcurve_set_quad_bounds(
    dcurve_pts: *const DPoint,
    curve: *const Point,
    t_start: f64,
    t_end: f64,
    out_bounds: *mut PathOpsBounds,
) {
    let dp = slice_from_raw(dcurve_pts, 3);
    let cp = slice_from_raw(curve, 3);
    let dc = DCurve::from_quad(DQuad::new(dp[0], dp[1], dp[2]));
    if !out_bounds.is_null() {
        *out_bounds = dc.set_quad_bounds([cp[0], cp[1], cp[2]], t_start, t_end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcurve_set_conic_bounds(
    dcurve_pts: *const DPoint,
    dcurve_w: f32,
    curve: *const Point,
    w: f32,
    t_start: f64,
    t_end: f64,
    out_bounds: *mut PathOpsBounds,
) {
    let dp = slice_from_raw(dcurve_pts, 3);
    let cp = slice_from_raw(curve, 3);
    let dc = DCurve::from_conic(DConic::new(dp[0], dp[1], dp[2], dcurve_w));
    if !out_bounds.is_null() {
        *out_bounds = dc.set_conic_bounds([cp[0], cp[1], cp[2]], w, t_start, t_end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_dcurve_set_cubic_bounds(
    dcurve_pts: *const DPoint,
    curve: *const Point,
    t_start: f64,
    t_end: f64,
    out_bounds: *mut PathOpsBounds,
) {
    let dp = slice_from_raw(dcurve_pts, 4);
    let cp = slice_from_raw(curve, 4);
    let dc = DCurve::from_cubic(DCubic::new(dp[0], dp[1], dp[2], dp[3]));
    if !out_bounds.is_null() {
        *out_bounds = dc.set_cubic_bounds([cp[0], cp[1], cp[2], cp[3]], t_start, t_end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_reduce_line(
    pts: *const DPoint,
    out_pts: *mut DPoint,
) -> i32 {
    let p = slice_from_raw(pts, 2);
    let mut ro = ReduceOrder::default();
    let n = ro.reduce_line(&DLine::new(p[0], p[1]));
    let dst = slice_from_raw_mut(out_pts, 4);
    dst.copy_from_slice(&ro.pts);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_reduce_quad(
    pts: *const DPoint,
    out_pts: *mut DPoint,
) -> i32 {
    let p = slice_from_raw(pts, 3);
    let mut ro = ReduceOrder::default();
    let n = ro.reduce_quad(&DQuad::new(p[0], p[1], p[2]));
    let dst = slice_from_raw_mut(out_pts, 4);
    dst.copy_from_slice(&ro.pts);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_reduce_cubic(
    pts: *const DPoint,
    allow_quadratics: i32,
    out_pts: *mut DPoint,
) -> i32 {
    let p = slice_from_raw(pts, 4);
    let mut ro = ReduceOrder::default();
    let aq = if allow_quadratics != 0 {
        ReduceQuadratics::AllowQuadratics
    } else {
        ReduceQuadratics::NoQuadratics
    };
    let n = ro.reduce_cubic(&DCubic::new(p[0], p[1], p[2], p[3]), aq);
    let dst = slice_from_raw_mut(out_pts, 4);
    dst.copy_from_slice(&ro.pts);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_reduce_conic_pts(
    pts: *const Point,
    w: f32,
    out_pts: *mut Point,
) -> u8 {
    let p = slice_from_raw(pts, 3);
    let dst = slice_from_raw_mut(out_pts, 3);
    ReduceOrder::conic_pts([p[0], p[1], p[2]], w, dst) as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_reduce_quad_pts(
    pts: *const Point,
    out_pts: *mut Point,
) -> u8 {
    let p = slice_from_raw(pts, 3);
    let dst = slice_from_raw_mut(out_pts, 3);
    ReduceOrder::quad_pts([p[0], p[1], p[2]], dst) as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_reduce_cubic_pts(
    pts: *const Point,
    out_pts: *mut Point,
) -> u8 {
    let p = slice_from_raw(pts, 4);
    let dst = slice_from_raw_mut(out_pts, 4);
    ReduceOrder::cubic_pts([p[0], p[1], p[2], p[3]], dst) as u8
}

// ============================================================================
// SkIntersections FFI
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct FfiIntersections {
    pub pt: [DPoint; 13],
    pub pt2: [DPoint; 2],
    pub t: [[f64; 13]; 2],
    pub is_coincident: [u16; 2],
    pub nearly_same: [bool; 2],
    pub used: u8,
    pub max: u8,
    pub allow_near: bool,
    pub swap: bool,
}

impl FfiIntersections {
    fn to_rust(&self) -> Intersections {
        Intersections {
            pt: self.pt,
            pt2: self.pt2,
            t: self.t,
            is_coincident: self.is_coincident,
            nearly_same: self.nearly_same,
            used: self.used,
            max: self.max,
            allow_near: self.allow_near,
            swap: self.swap,
        }
    }

    fn from_rust(&mut self, i: &Intersections) {
        self.pt = i.pt;
        self.pt2 = i.pt2;
        self.t = i.t;
        self.is_coincident = i.is_coincident;
        self.nearly_same = i.nearly_same;
        self.used = i.used;
        self.max = i.max;
        self.allow_near = i.allow_near;
        self.swap = i.swap;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_horizontal_line(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    left: f64,
    right: f64,
    y: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 2);
    let n = i.horizontal_line(&DLine::new(p[0], p[1]), left, right, y, flipped);
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_vertical_line(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    top: f64,
    bottom: f64,
    x: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 2);
    let n = i.vertical_line(&DLine::new(p[0], p[1]), top, bottom, x, flipped);
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_horizontal_quad(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    left: f64,
    right: f64,
    y: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 3);
    let n = i.horizontal_quad(&DQuad::new(p[0], p[1], p[2]), left, right, y, flipped);
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_vertical_quad(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    top: f64,
    bottom: f64,
    x: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 3);
    let n = i.vertical_quad(&DQuad::new(p[0], p[1], p[2]), top, bottom, x, flipped);
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_horizontal_conic(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    w: f32,
    left: f64,
    right: f64,
    y: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 3);
    let n = i.horizontal_conic(
        &DConic::new(p[0], p[1], p[2], w),
        left,
        right,
        y,
        flipped,
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_vertical_conic(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    w: f32,
    top: f64,
    bottom: f64,
    x: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 3);
    let n = i.vertical_conic(
        &DConic::new(p[0], p[1], p[2], w),
        top,
        bottom,
        x,
        flipped,
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_horizontal_cubic(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    left: f64,
    right: f64,
    y: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 4);
    let n = i.horizontal_cubic(&DCubic::new(p[0], p[1], p[2], p[3]), left, right, y, flipped);
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_vertical_cubic(
    io: *mut FfiIntersections,
    pts: *const DPoint,
    top: f64,
    bottom: f64,
    x: f64,
    flipped: bool,
) -> i32 {
    let mut i = (*io).to_rust();
    let p = slice_from_raw(pts, 4);
    let n = i.vertical_cubic(&DCubic::new(p[0], p[1], p[2], p[3]), top, bottom, x, flipped);
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_line_line(
    io: *mut FfiIntersections,
    a: *const DPoint,
    b: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pa = slice_from_raw(a, 2);
    let pb = slice_from_raw(b, 2);
    let n = i.intersect_line_line(&DLine::new(pa[0], pa[1]), &DLine::new(pb[0], pb[1]));
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_quad_line(
    io: *mut FfiIntersections,
    q: *const DPoint,
    l: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pq = slice_from_raw(q, 3);
    let pl = slice_from_raw(l, 2);
    let n = i.intersect_quad_line(&DQuad::new(pq[0], pq[1], pq[2]), &DLine::new(pl[0], pl[1]));
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_conic_line(
    io: *mut FfiIntersections,
    c: *const DPoint,
    w: f32,
    l: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pc = slice_from_raw(c, 3);
    let pl = slice_from_raw(l, 2);
    let n = i.intersect_conic_line(
        &DConic::new(pc[0], pc[1], pc[2], w),
        &DLine::new(pl[0], pl[1]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_cubic_line(
    io: *mut FfiIntersections,
    c: *const DPoint,
    l: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pc = slice_from_raw(c, 4);
    let pl = slice_from_raw(l, 2);
    let n = i.intersect_cubic_line(
        &DCubic::new(pc[0], pc[1], pc[2], pc[3]),
        &DLine::new(pl[0], pl[1]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_quad_quad(
    io: *mut FfiIntersections,
    q1: *const DPoint,
    q2: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let p1 = slice_from_raw(q1, 3);
    let p2 = slice_from_raw(q2, 3);
    let n = i.intersect_quad_quad(
        &DQuad::new(p1[0], p1[1], p1[2]),
        &DQuad::new(p2[0], p2[1], p2[2]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_conic_quad(
    io: *mut FfiIntersections,
    c: *const DPoint,
    w: f32,
    q: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pc = slice_from_raw(c, 3);
    let pq = slice_from_raw(q, 3);
    let n = i.intersect_conic_quad(
        &DConic::new(pc[0], pc[1], pc[2], w),
        &DQuad::new(pq[0], pq[1], pq[2]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_conic_conic(
    io: *mut FfiIntersections,
    c1: *const DPoint,
    w1: f32,
    c2: *const DPoint,
    w2: f32,
) -> i32 {
    let mut i = (*io).to_rust();
    let p1 = slice_from_raw(c1, 3);
    let p2 = slice_from_raw(c2, 3);
    let n = i.intersect_conic_conic(
        &DConic::new(p1[0], p1[1], p1[2], w1),
        &DConic::new(p2[0], p2[1], p2[2], w2),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_cubic_quad(
    io: *mut FfiIntersections,
    c: *const DPoint,
    q: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pc = slice_from_raw(c, 4);
    let pq = slice_from_raw(q, 3);
    let n = i.intersect_cubic_quad(
        &DCubic::new(pc[0], pc[1], pc[2], pc[3]),
        &DQuad::new(pq[0], pq[1], pq[2]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_cubic_conic(
    io: *mut FfiIntersections,
    cu: *const DPoint,
    co: *const DPoint,
    w: f32,
) -> i32 {
    let mut i = (*io).to_rust();
    let pcu = slice_from_raw(cu, 4);
    let pco = slice_from_raw(co, 3);
    let n = i.intersect_cubic_conic(
        &DCubic::new(pcu[0], pcu[1], pcu[2], pcu[3]),
        &DConic::new(pco[0], pco[1], pco[2], w),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_cubic_cubic(
    io: *mut FfiIntersections,
    c1: *const DPoint,
    c2: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let p1 = slice_from_raw(c1, 4);
    let p2 = slice_from_raw(c2, 4);
    let n = i.intersect_cubic_cubic(
        &DCubic::new(p1[0], p1[1], p1[2], p1[3]),
        &DCubic::new(p2[0], p2[1], p2[2], p2[3]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_ray_line(
    io: *mut FfiIntersections,
    a: *const DPoint,
    b: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pa = slice_from_raw(a, 2);
    let pb = slice_from_raw(b, 2);
    let n = i.intersect_ray_line_line(&DLine::new(pa[0], pa[1]), &DLine::new(pb[0], pb[1]));
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_ray_quad(
    io: *mut FfiIntersections,
    q: *const DPoint,
    l: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pq = slice_from_raw(q, 3);
    let pl = slice_from_raw(l, 2);
    let n = i.intersect_ray_quad_line(&DQuad::new(pq[0], pq[1], pq[2]), &DLine::new(pl[0], pl[1]));
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_ray_conic(
    io: *mut FfiIntersections,
    c: *const DPoint,
    w: f32,
    l: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pc = slice_from_raw(c, 3);
    let pl = slice_from_raw(l, 2);
    let n = i.intersect_ray_conic_line(
        &DConic::new(pc[0], pc[1], pc[2], w),
        &DLine::new(pl[0], pl[1]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_ray_cubic(
    io: *mut FfiIntersections,
    c: *const DPoint,
    l: *const DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let pc = slice_from_raw(c, 4);
    let pl = slice_from_raw(l, 2);
    let n = i.intersect_ray_cubic_line(
        &DCubic::new(pc[0], pc[1], pc[2], pc[3]),
        &DLine::new(pl[0], pl[1]),
    );
    (*io).from_rust(&i);
    n as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_insert(
    io: *mut FfiIntersections,
    one: f64,
    two: f64,
    pt: DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let res = i.insert(one, two, pt);
    (*io).from_rust(&i);
    res
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_insert_coincident(
    io: *mut FfiIntersections,
    one: f64,
    two: f64,
    pt: DPoint,
) -> i32 {
    let mut i = (*io).to_rust();
    let res = i.insert_coincident(one, two, pt);
    (*io).from_rust(&i);
    res
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_i_debug_coincident_used(
    io: *const FfiIntersections,
) -> i32 {
    (*io).to_rust().debug_coincident_used() as i32
}

// ============================================================================
// OpAngle & OpSegment helpers for PathOpsAngleTest / internal tests
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct FfiAngleDesc {
    pub pts: [Point; 4],
    pub weight: f32,
    pub verb: u8,
    pub start_t: f64,
    pub end_t: f64,
}

fn add_angle_to_state(state: &mut OpState, desc: &FfiAngleDesc) -> usize {
    let c = state.contour_head;
    let verb = Verb::from_u8(desc.verb).unwrap_or(Verb::Line);
    let seg = match verb {
        Verb::Line => state.add_line(c, [desc.pts[0], desc.pts[1]]),
        Verb::Quad => state.add_quad(c, [desc.pts[0], desc.pts[1], desc.pts[2]]),
        Verb::Conic => state.add_conic(c, [desc.pts[0], desc.pts[1], desc.pts[2]], desc.weight),
        Verb::Cubic => state.add_cubic(c, desc.pts),
        _ => state.add_line(c, [desc.pts[0], desc.pts[1]]),
    };
    let start_span = if desc.start_t == 0.0 {
        state.segments[seg].head
    } else if desc.start_t == 1.0 {
        state.segments[seg].tail
    } else {
        let pt_t = state.seg_add_t(seg, desc.start_t).unwrap();
        state.pt_ts[pt_t].span
    };
    let end_span = if desc.end_t == 0.0 {
        state.segments[seg].head
    } else if desc.end_t == 1.0 {
        state.segments[seg].tail
    } else {
        let pt_t = state.seg_add_t(seg, desc.end_t).unwrap();
        state.pt_ts[pt_t].span
    };
    let angle = state.alloc_angle();
    state.angle_set(angle, start_span, end_span);
    angle
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_angle_orderable(
    a: *const FfiAngleDesc,
    b: *const FfiAngleDesc,
) -> i32 {
    if a.is_null() || b.is_null() {
        return -1;
    }
    let mut state = OpState::new();
    let a_id = add_angle_to_state(&mut state, &*a);
    let b_id = add_angle_to_state(&mut state, &*b);
    state.angle_orderable(a_id, b_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_angle_after(
    a: *const FfiAngleDesc,
    b: *const FfiAngleDesc,
    b_next: *const FfiAngleDesc,
) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    let mut state = OpState::new();
    let a_id = add_angle_to_state(&mut state, &*a);
    let b_id = add_angle_to_state(&mut state, &*b);
    if !b_next.is_null() {
        let bn_id = add_angle_to_state(&mut state, &*b_next);
        state.angles[b_id].next = Some(bn_id);
    }
    state.angle_after(a_id, b_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_angle_line_on_one_side(
    a: *const FfiAngleDesc,
    b: *const FfiAngleDesc,
    use_original: bool,
) -> i32 {
    if a.is_null() || b.is_null() {
        return -1;
    }
    let mut state = OpState::new();
    let a_id = add_angle_to_state(&mut state, &*a);
    let b_id = add_angle_to_state(&mut state, &*b);
    state.angle_line_on_one_side(a_id, b_id, use_original)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_angle_convex_hull_overlaps(
    a: *const FfiAngleDesc,
    b: *const FfiAngleDesc,
) -> i32 {
    if a.is_null() || b.is_null() {
        return -1;
    }
    let mut state = OpState::new();
    let a_id = add_angle_to_state(&mut state, &*a);
    let b_id = add_angle_to_state(&mut state, &*b);
    state.angle_convex_hull_overlaps(a_id, b_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_angle_ends_intersect(
    a: *const FfiAngleDesc,
    b: *const FfiAngleDesc,
) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    let mut state = OpState::new();
    let a_id = add_angle_to_state(&mut state, &*a);
    let b_id = add_angle_to_state(&mut state, &*b);
    state.angle_ends_intersect(a_id, b_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_curve_point_at_t(
    pts: *const Point,
    weight: f32,
    verb: u8,
    t: f64,
) -> Point {
    let v = Verb::from_u8(verb).unwrap_or(Verb::Line);
    let count = match v {
        Verb::Line => 2,
        Verb::Quad | Verb::Conic => 3,
        Verb::Cubic => 4,
        _ => 2,
    };
    let p = slice_from_raw(pts, count);
    curve_point_at_t(p, weight, v, t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_curve_dpoint_at_t(
    pts: *const Point,
    weight: f32,
    verb: u8,
    t: f64,
) -> DPoint {
    let v = Verb::from_u8(verb).unwrap_or(Verb::Line);
    let count = match v {
        Verb::Line => 2,
        Verb::Quad | Verb::Conic => 3,
        Verb::Cubic => 4,
        _ => 2,
    };
    let p = slice_from_raw(pts, count);
    curve_dpoint_at_t(p, weight, v, t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_curve_dslope_at_t(
    pts: *const Point,
    weight: f32,
    verb: u8,
    t: f64,
) -> DVector {
    let v = Verb::from_u8(verb).unwrap_or(Verb::Line);
    let count = match v {
        Verb::Line => 2,
        Verb::Quad | Verb::Conic => 3,
        Verb::Cubic => 4,
        _ => 2,
    };
    let p = slice_from_raw(pts, count);
    curve_dslope_at_t(p, weight, v, t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_pathops_seg_match(
    pts: *const Point,
    weight: f32,
    verb: u8,
    same_seg: bool,
    base_t: f64,
    base_pt: Point,
    test_t: f64,
    test_pt: Point,
) -> bool {
    let v = Verb::from_u8(verb).unwrap_or(Verb::Line);
    let mut state = OpState::new();
    let c = state.contour_head;
    let p = slice_from_raw(pts, 4);
    let seg = match v {
        Verb::Line => state.add_line(c, [p[0], p[1]]),
        Verb::Quad => state.add_quad(c, [p[0], p[1], p[2]]),
        Verb::Conic => state.add_conic(c, [p[0], p[1], p[2]], weight),
        Verb::Cubic => state.add_cubic(c, [p[0], p[1], p[2], p[3]]),
        _ => state.add_line(c, [p[0], p[1]]),
    };
    let pt_t_id = state.spans[state.segments[seg].head].pt_t;
    state.pt_ts[pt_t_id].t = base_t;
    state.pt_ts[pt_t_id].pt = base_pt;
    let test_parent = if same_seg { seg } else { seg + 1 };
    state.seg_match(seg, pt_t_id, test_parent, test_t, test_pt)
}
