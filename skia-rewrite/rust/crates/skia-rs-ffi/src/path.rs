//! FFI exports for Layer 2.2 (`skia-rs-path`):
//!   SkPath, SkPathBuilder, SkPathData, SkPathDump, SkPathIter, SkPathRaw, SkPathRawShapes,
//!   SkPathPriv, SkPath_interpolate, SkPath_serial, SkContourMeasure, SkPathMeasure,
//!   SkStroke, SkStrokeRec, SkStrokerPriv, SkPathUtils, SkDashPath, SkParsePath.

use skia_rs_geometry::{Matrix, Point, RRect, Rect, Vector};
use skia_rs_path::{
    analysis,
    builder::{self, PathBuilder},
    dash,
    iter::{self, PathIterRec},
    measure::{self, ContourMeasure, ContourMeasureIter, ContourSegment},
    parse::{self, PathEncoding},
    serial_dump::{self, RRectSerialInfo, ReadPathResult},
    shapes,
    stroke::{self, Stroke, StrokeRec},
    types::{
        self, AddPathMode, ArcSize, ArcType, Cap, Convexity, Direction, FillType, FirstDirection,
        Join, PathDataBuf, PathIsAType, PathRaw, Verb,
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

/// Safely views a C `SkPathVerb` (`uint8_t`) buffer as `&[Verb]` after validating every byte is `0..=5`.
#[inline]
unsafe fn verbs_from_raw<'a>(ptr: *const u8, len: usize) -> Option<&'a [Verb]> {
    let raw = slice_from_raw(ptr, len);
    if raw.iter().all(|&b| b <= 5) {
        if raw.is_empty() {
            Some(&[])
        } else {
            Some(core::slice::from_raw_parts(ptr as *const Verb, len))
        }
    } else {
        None
    }
}

/// Helper to build a `PathRaw` from raw C pointers.
#[inline]
unsafe fn make_path_raw<'a>(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    bounds: Rect,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
) -> Option<PathRaw<'a>> {
    let points = slice_from_raw(pts, pt_count);
    let verbs = verbs_from_raw(vbs, vb_count)?;
    let conics = slice_from_raw(conics, conic_count);
    Some(PathRaw {
        points,
        verbs,
        conics,
        bounds,
        fill_type: FillType::from_u8(fill_type).unwrap_or(FillType::Winding),
        convexity: Convexity::from_u8(convexity).unwrap_or(Convexity::Unknown),
        segment_mask,
    })
}

/// Helper to reconstruct a `PathBuilder` from C++ `SkPathBuilder` fields.
#[inline]
unsafe fn builder_from_parts(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
) -> PathBuilder {
    let points = slice_from_raw(pts, pt_count).to_vec();
    let verbs = verbs_from_raw(vbs, vb_count).unwrap_or(&[]).to_vec();
    let conics = slice_from_raw(conics, conic_count).to_vec();
    PathBuilder::from_buf(PathDataBuf {
        points,
        verbs,
        conics,
        fill_type: FillType::from_u8(fill_type).unwrap_or(FillType::Winding),
        convexity: Convexity::from_u8(convexity).unwrap_or(Convexity::Unknown),
        segment_mask,
        last_move_index,
        is_a_type: PathIsAType::from_u8(is_a_type).unwrap_or(PathIsAType::General),
        is_a_dir: Direction::from_u8(is_a_dir).unwrap_or(Direction::Cw),
        is_a_start_index,
        is_volatile,
    })
}

// ============================================================================
// Owned Buffer / String Handles
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_buf_points(
    buf: *const PathDataBuf,
    out_len: *mut usize,
) -> *const Point {
    let b = &*buf;
    *out_len = b.points.len();
    b.points.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_buf_verbs(
    buf: *const PathDataBuf,
    out_len: *mut usize,
) -> *const u8 {
    let b = &*buf;
    *out_len = b.verbs.len();
    b.verbs.as_ptr() as *const u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_buf_conics(
    buf: *const PathDataBuf,
    out_len: *mut usize,
) -> *const f32 {
    let b = &*buf;
    *out_len = b.conics.len();
    b.conics.as_ptr()
}

#[repr(C)]
pub struct PathBufMeta {
    pub fill_type: u8,
    pub convexity: u8,
    pub segment_mask: u8,
    pub is_a_type: u8,
    pub is_a_dir: u8,
    pub is_a_start_index: u8,
    pub is_volatile: bool,
    pub _pad: u8,
    pub last_move_index: i32,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_buf_meta(buf: *const PathDataBuf) -> PathBufMeta {
    let b = &*buf;
    PathBufMeta {
        fill_type: b.fill_type as u8,
        convexity: b.convexity as u8,
        segment_mask: b.segment_mask,
        is_a_type: b.is_a_type as u8,
        is_a_dir: b.is_a_dir as u8,
        is_a_start_index: b.is_a_start_index,
        is_volatile: b.is_volatile,
        _pad: 0,
        last_move_index: b.last_move_index,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_buf_free(buf: *mut PathDataBuf) {
    if !buf.is_null() {
        drop(Box::from_raw(buf));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_data(
    s: *const String,
    out_len: *mut usize,
) -> *const u8 {
    let st = &*s;
    *out_len = st.len();
    st.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_free(s: *mut String) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}

// ============================================================================
// Types, Validation, Bounds, Segment Mask, Helpers
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_valid_data(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
) -> bool {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return false;
    };
    let conics = slice_from_raw(conics, conic_count);
    types::valid_path_data(points, verbs, conics)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_compute_segment_mask(
    vbs: *const u8,
    vb_count: usize,
) -> u8 {
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return 0;
    };
    types::compute_segment_mask(verbs)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_trimmed_bounds(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    out_rect: *mut Rect,
) -> bool {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return false;
    };
    match types::trimmed_bounds(points, verbs) {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_find_last_move_to_index(
    vbs: *const u8,
    vb_count: usize,
    pt_count: usize,
) -> i32 {
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return -1;
    };
    types::find_last_move_to_index(verbs, pt_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_axis_aligned(pts: *const Point, pt_count: usize) -> bool {
    types::is_axis_aligned(slice_from_raw(pts, pt_count))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_line_degenerate(p1: Point, p2: Point, exact: bool) -> bool {
    types::is_line_degenerate(p1, p2, exact)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_quad_degenerate(
    p1: Point,
    p2: Point,
    p3: Point,
    exact: bool,
) -> bool {
    types::is_quad_degenerate(p1, p2, p3, exact)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_cubic_degenerate(
    p1: Point,
    p2: Point,
    p3: Point,
    p4: Point,
    exact: bool,
) -> bool {
    types::is_cubic_degenerate(p1, p2, p3, p4, exact)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_zero_length_since_point(
    pts: *const Point,
    pt_count: usize,
    start_pt_index: usize,
) -> bool {
    types::is_zero_length_since_point(slice_from_raw(pts, pt_count), start_pt_index)
}

// ============================================================================
// Iterators (SkPathIter, SkPathContourIter, SkPath::Iter)
// ============================================================================

#[repr(C)]
pub struct FfiPathIterStep {
    pub has_rec: bool,
    pub verb: u8,
    pub is_close_storage: bool,
    pub pt_len: u8,
    pub pt_offset: usize,
    pub conic_weight: f32,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_iter_step(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    p_index: *mut usize,
    v_index: *mut usize,
    c_index: *mut usize,
    close_storage: *mut Point,
) -> FfiPathIterStep {
    let points = slice_from_raw(pts, pt_count);
    let verbs = slice_from_raw(vbs, vb_count);
    let conics = slice_from_raw(conics, conic_count);
    let cs = &mut *(close_storage as *mut [Point; 2]);
    match iter::path_iter_step_u8(
        points,
        verbs,
        conics,
        &mut *p_index,
        &mut *v_index,
        &mut *c_index,
        cs,
    ) {
        Some((verb, pts_start_or_sentinel, pt_len, w)) => {
            let is_close_storage = pts_start_or_sentinel == usize::MAX;
            let pt_offset = if is_close_storage {
                if verb == Verb::Move { 1 } else { 0 }
            } else {
                pts_start_or_sentinel
            };
            FfiPathIterStep {
                has_rec: true,
                verb: verb as u8,
                is_close_storage,
                pt_len: pt_len as u8,
                pt_offset,
                conic_weight: w,
            }
        }
        None => FfiPathIterStep {
            has_rec: false,
            verb: 0,
            is_close_storage: false,
            pt_len: 0,
            pt_offset: 0,
            conic_weight: -1.0,
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_iter_next_counts(
    vbs: *const u8,
    vb_count: usize,
    out_npts: *mut usize,
    out_nvbs: *mut usize,
    out_nws: *mut usize,
) -> bool {
    let verbs = slice_from_raw(vbs, vb_count);
    match iter::next_contour_counts_u8(verbs) {
        Some((npts, nvbs, nws)) => {
            *out_npts = npts;
            *out_nvbs = nvbs;
            *out_nws = nws;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_legacy_iter_is_closed_contour(
    vbs: *const u8,
    vb_count: usize,
    force_close: bool,
) -> bool {
    let verbs = slice_from_raw(vbs, vb_count);
    iter::legacy_iter_is_closed_contour_u8(verbs, force_close)
}

// ============================================================================
// Shapes (SkPathRawShapes, IsRectContour, IsNestedFillRects, IsSimpleRect, etc.)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shape_fill_rect_points(
    r: Rect,
    dir: u8,
    index: u32,
    out_pts: *mut Point,
) {
    let pts = shapes::fill_rect_points(r, Direction::from_u8(dir).unwrap_or(Direction::Cw), index);
    *(out_pts as *mut [Point; 4]) = pts;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shape_fill_oval_points(
    r: Rect,
    dir: u8,
    index: u32,
    out_pts: *mut Point,
) {
    let pts = shapes::fill_oval_points(r, Direction::from_u8(dir).unwrap_or(Direction::Cw), index);
    *(out_pts as *mut [Point; 9]) = pts;
}

#[repr(C)]
pub struct FfiRRectShapeResult {
    /// 0 = Rect (4 pts), 1 = Oval (9 pts), 2 = RRectLineStart (13 pts), 3 = RRectConicStart (12 pts)
    pub kind: u8,
    pub point_count: u8,
    pub convexity: u8,
    pub segment_mask: u8,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shape_build_rrect(
    rrect: *const RRect,
    dir: u8,
    index: u32,
    out_pts: *mut Point,
) -> FfiRRectShapeResult {
    let data = shapes::build_rrect_shape(
        &*rrect,
        Direction::from_u8(dir).unwrap_or(Direction::Cw),
        index,
    );
    let out = slice_from_raw_mut(out_pts, data.point_count);
    out.copy_from_slice(&data.storage[..data.point_count]);
    let kind = match data.kind {
        shapes::RRectShapeKind::Rect => 0,
        shapes::RRectShapeKind::Oval => 1,
        shapes::RRectShapeKind::RRectLineStart => 2,
        shapes::RRectShapeKind::RRectConicStart => 3,
    };
    FfiRRectShapeResult {
        kind,
        point_count: data.point_count as u8,
        convexity: data.convexity as u8,
        segment_mask: data.segment_mask,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_shape_tri_to_convexity(pts: *const Point) -> u8 {
    let arr = &*(pts as *const [Point; 3]);
    shapes::tri_to_convexity(arr) as u8
}

#[repr(C)]
pub struct FfiRectContour {
    pub found: bool,
    pub is_closed: bool,
    pub direction: u8,
    pub _pad: u8,
    pub rect: Rect,
    pub points_consumed: usize,
    pub verbs_consumed: usize,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_rect_contour(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    segment_mask: u32,
    allow_partial: bool,
) -> FfiRectContour {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return FfiRectContour {
            found: false,
            is_closed: false,
            direction: 0,
            _pad: 0,
            rect: Rect::EMPTY,
            points_consumed: 0,
            verbs_consumed: 0,
        };
    };
    match shapes::is_rect_contour(points, verbs, segment_mask, allow_partial) {
        Some(rc) => FfiRectContour {
            found: true,
            is_closed: rc.is_closed,
            direction: rc.direction as u8,
            _pad: 0,
            rect: rc.rect,
            points_consumed: rc.points_consumed,
            verbs_consumed: rc.verbs_consumed,
        },
        None => FfiRectContour {
            found: false,
            is_closed: false,
            direction: 0,
            _pad: 0,
            rect: Rect::EMPTY,
            points_consumed: 0,
            verbs_consumed: 0,
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_nested_fill_rects(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    bounds: Rect,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    out_rects: *mut Rect,
    out_dirs: *mut u8,
) -> bool {
    let Some(raw) = make_path_raw(
        pts, pt_count, vbs, vb_count, conics, conic_count, bounds, fill_type, convexity,
        segment_mask,
    ) else {
        return false;
    };
    match shapes::is_nested_fill_rects(&raw) {
        Some((rects, dirs)) => {
            if !out_rects.is_null() {
                let r = slice_from_raw_mut(out_rects, 2);
                r[0] = rects[0];
                r[1] = rects[1];
            }
            if !out_dirs.is_null() {
                let d = slice_from_raw_mut(out_dirs, 2);
                d[0] = dirs[0] as u8;
                d[1] = dirs[1] as u8;
            }
            true
        }
        None => false,
    }
}

#[repr(C)]
pub struct FfiPathRectInfo {
    pub found: bool,
    pub direction: u8,
    pub start_index: u8,
    pub _pad: u8,
    pub rect: Rect,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_is_simple_rect(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    segment_mask: u32,
    is_finite: bool,
    is_simple_fill: bool,
) -> FfiPathRectInfo {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return FfiPathRectInfo {
            found: false,
            direction: 0,
            start_index: 0,
            _pad: 0,
            rect: Rect::EMPTY,
        };
    };
    let conics = slice_from_raw(conics, conic_count);
    match shapes::is_simple_rect(points, verbs, conics, segment_mask, is_finite, is_simple_fill) {
        Some(info) => FfiPathRectInfo {
            found: true,
            direction: info.direction as u8,
            start_index: info.start_index,
            _pad: 0,
            rect: info.rect,
        },
        None => FfiPathRectInfo {
            found: false,
            direction: 0,
            start_index: 0,
            _pad: 0,
            rect: Rect::EMPTY,
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_transform_dir_and_start(
    matrix: *const Matrix,
    is_rrect: bool,
    dir: u8,
    start: u32,
    out_dir: *mut u8,
    out_start: *mut u32,
) {
    let (d, s) = shapes::transform_dir_and_start(
        &*matrix,
        is_rrect,
        Direction::from_u8(dir).unwrap_or(Direction::Cw),
        start,
    );
    *out_dir = d as u8;
    *out_start = s;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_deduce_rrect_from_contour(
    bounds: Rect,
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    out_rrect: *mut RRect,
) {
    let points = slice_from_raw(pts, pt_count);
    let verbs = verbs_from_raw(vbs, vb_count).unwrap_or(&[]);
    *out_rrect = shapes::deduce_rrect_from_contour(bounds, points, verbs);
}

// ============================================================================
// Analysis (Convexity, FirstDirection, TightBounds, Contains, PerspectiveClip)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_compute_convexity(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
) -> u8 {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return Convexity::Concave as u8;
    };
    let conics = slice_from_raw(conics, conic_count);
    analysis::compute_convexity(points, verbs, conics) as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_transform_convexity(
    matrix: *const Matrix,
    pts: *const Point,
    pt_count: usize,
    convexity: u8,
) -> u8 {
    let points = slice_from_raw(pts, pt_count);
    let c = Convexity::from_u8(convexity).unwrap_or(Convexity::Unknown);
    analysis::transform_convexity(&*matrix, points, c) as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_compute_first_direction(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    bounds: Rect,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
) -> u8 {
    let Some(raw) = make_path_raw(
        pts, pt_count, vbs, vb_count, conics, conic_count, bounds, fill_type, convexity,
        segment_mask,
    ) else {
        return FirstDirection::Unknown as u8;
    };
    analysis::compute_first_direction(&raw) as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_compute_tight_bounds(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
) -> Rect {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return Rect::EMPTY;
    };
    let conics = slice_from_raw(conics, conic_count);
    analysis::compute_tight_bounds(points, verbs, conics)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_contains(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    bounds: Rect,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    p: Point,
) -> bool {
    let Some(raw) = make_path_raw(
        pts, pt_count, vbs, vb_count, conics, conic_count, bounds, fill_type, convexity,
        segment_mask,
    ) else {
        return false;
    };
    analysis::contains(&raw, p)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_conservatively_contains_rect(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    convexity: u8,
    is_finite: bool,
    rect: Rect,
) -> bool {
    let points = slice_from_raw(pts, pt_count);
    let Some(verbs) = verbs_from_raw(vbs, vb_count) else {
        return false;
    };
    let conics = slice_from_raw(conics, conic_count);
    let c = Convexity::from_u8(convexity).unwrap_or(Convexity::Unknown);
    analysis::conservatively_contains_rect(points, verbs, conics, c, is_finite, rect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_perspective_clip(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    bounds: Rect,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    matrix: *const Matrix,
) -> *mut PathDataBuf {
    let Some(raw) = make_path_raw(
        pts, pt_count, vbs, vb_count, conics, conic_count, bounds, fill_type, convexity,
        segment_mask,
    ) else {
        return core::ptr::null_mut();
    };
    match analysis::perspective_clip(&raw, &*matrix) {
        Some(buf) => Box::into_raw(Box::new(buf)),
        None => core::ptr::null_mut(),
    }
}

// ============================================================================
// Builder operations (Arcs, DrawArc, Transform, Reverse, AddPath, Interpolate)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_path_draw_arc_is_convex(
    sweep_angle: f32,
    arc_type: u8,
    is_fill_no_path_effect: bool,
) -> bool {
    let at = if arc_type == 1 {
        ArcType::Wedge
    } else {
        ArcType::Arc
    };
    builder::draw_arc_is_convex(sweep_angle, at, is_fill_no_path_effect)
}

#[no_mangle]
pub extern "C" fn skia_rs_path_create_draw_arc(
    oval: Rect,
    start_angle: f32,
    sweep_angle: f32,
    arc_type: u8,
    is_fill_no_path_effect: bool,
) -> *mut PathDataBuf {
    let at = if arc_type == 1 {
        ArcType::Wedge
    } else {
        ArcType::Arc
    };
    let buf = builder::create_draw_arc_path(
        oval,
        start_angle,
        sweep_angle,
        at,
        is_fill_no_path_effect,
    );
    Box::into_raw(Box::new(buf))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_can_interpolate(
    from_pts_len: usize,
    from_vbs: *const u8,
    from_vb_len: usize,
    from_conics: *const f32,
    from_conic_len: usize,
    to_pts_len: usize,
    to_vbs: *const u8,
    to_vb_len: usize,
    to_conics: *const f32,
    to_conic_len: usize,
) -> bool {
    let Some(fv) = verbs_from_raw(from_vbs, from_vb_len) else {
        return false;
    };
    let Some(tv) = verbs_from_raw(to_vbs, to_vb_len) else {
        return false;
    };
    let fc = slice_from_raw(from_conics, from_conic_len);
    let tc = slice_from_raw(to_conics, to_conic_len);
    builder::can_interpolate(from_pts_len, fv, fc, to_pts_len, tv, tc)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_interpolate_points(
    from_pts: *const Point,
    to_pts: *const Point,
    count: usize,
    t: f32,
    out_pts: *mut Point,
) {
    let fp = slice_from_raw(from_pts, count);
    let tp = slice_from_raw(to_pts, count);
    let res = builder::interpolate_points(fp, tp, t);
    let out = slice_from_raw_mut(out_pts, count);
    out.copy_from_slice(&res);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_arc_to_oval(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    oval: Rect,
    start_angle: f32,
    sweep_angle: f32,
    force_move_to: bool,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    pb.arc_to_oval(oval, start_angle, sweep_angle, force_move_to);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_add_arc(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    oval: Rect,
    start_angle: f32,
    sweep_angle: f32,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    pb.add_arc(oval, start_angle, sweep_angle);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_arc_to_tangent(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    p1: Point,
    p2: Point,
    radius: f32,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    pb.arc_to_tangent(p1, p2, radius);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_arc_to_svg(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    rad: Point,
    angle: f32,
    arc_large: u8,
    sweep: u8,
    end_pt: Point,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    let size = if arc_large != 0 {
        ArcSize::Large
    } else {
        ArcSize::Small
    };
    let dir = Direction::from_u8(sweep).unwrap_or(Direction::Cw);
    pb.arc_to_svg(rad, angle, size, dir, end_pt);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_transform(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    matrix: *const Matrix,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    pb.transform(&*matrix);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_reverse_path_to(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    src_pts: *const Point,
    src_pt_count: usize,
    src_vbs: *const u8,
    src_vb_count: usize,
    src_conics: *const f32,
    src_conic_count: usize,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    let sp = slice_from_raw(src_pts, src_pt_count);
    let sv = verbs_from_raw(src_vbs, src_vb_count).unwrap_or(&[]);
    let sc = slice_from_raw(src_conics, src_conic_count);
    pb.reverse_path_to(sp, sv, sc);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_reverse_add_path(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    src_pts: *const Point,
    src_pt_count: usize,
    src_vbs: *const u8,
    src_vb_count: usize,
    src_conics: *const f32,
    src_conic_count: usize,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    let sp = slice_from_raw(src_pts, src_pt_count);
    let sv = verbs_from_raw(src_vbs, src_vb_count).unwrap_or(&[]);
    let sc = slice_from_raw(src_conics, src_conic_count);
    pb.reverse_add_path(sp, sv, sc);
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_builder_add_path(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    src_pts: *const Point,
    src_pt_count: usize,
    src_vbs: *const u8,
    src_vb_count: usize,
    src_conics: *const f32,
    src_conic_count: usize,
    src_seg_mask: u8,
    src_is_finite: bool,
    src_is_a_type: u8,
    src_is_a_dir: u8,
    src_is_a_start: u8,
    src_convexity: u8,
    src_is_volatile: bool,
    matrix: *const Matrix,
    mode: u8,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    let sp = slice_from_raw(src_pts, src_pt_count);
    let sv = verbs_from_raw(src_vbs, src_vb_count).unwrap_or(&[]);
    let sc = slice_from_raw(src_conics, src_conic_count);
    let add_mode = if mode == 1 {
        AddPathMode::Extend
    } else {
        AddPathMode::Append
    };
    pb.add_path(
        sp,
        sv,
        sc,
        src_seg_mask,
        src_is_finite,
        (
            PathIsAType::from_u8(src_is_a_type).unwrap_or(PathIsAType::General),
            Direction::from_u8(src_is_a_dir).unwrap_or(Direction::Cw),
            src_is_a_start,
        ),
        Convexity::from_u8(src_convexity).unwrap_or(Convexity::Unknown),
        src_is_volatile,
        &*matrix,
        add_mode,
    );
    Box::into_raw(Box::new(pb.into_buf()))
}

// ============================================================================
// Serialization & Dump (SkPath_serial.cpp, SkPathDump.cpp)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_write_to_memory_as_rrect(
    fill_type: u8,
    has_rrect: bool,
    rrect: *const RRect,
    direction: u8,
    start_index: u32,
    dst: *mut u8,
    dst_cap: usize,
) -> usize {
    if !has_rrect || rrect.is_null() {
        return 0;
    }
    let info = RRectSerialInfo {
        rrect: *rrect,
        direction: Direction::from_u8(direction).unwrap_or(Direction::Cw),
        start_index,
    };
    let ft = FillType::from_u8(fill_type).unwrap_or(FillType::Winding);
    if dst.is_null() {
        serial_dump::write_to_memory_as_rrect(ft, Some(&info), None)
    } else {
        let out = slice_from_raw_mut(dst, dst_cap);
        serial_dump::write_to_memory_as_rrect(ft, Some(&info), Some(out))
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_write_to_memory(
    fill_type: u8,
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    has_rrect: bool,
    rrect: *const RRect,
    rrect_dir: u8,
    rrect_start: u32,
    dst: *mut u8,
    dst_cap: usize,
) -> usize {
    let ft = FillType::from_u8(fill_type).unwrap_or(FillType::Winding);
    let points = slice_from_raw(pts, pt_count);
    let verbs = verbs_from_raw(vbs, vb_count).unwrap_or(&[]);
    let conics = slice_from_raw(conics, conic_count);
    let rr_info = if has_rrect && !rrect.is_null() {
        Some(RRectSerialInfo {
            rrect: *rrect,
            direction: Direction::from_u8(rrect_dir).unwrap_or(Direction::Cw),
            start_index: rrect_start,
        })
    } else {
        None
    };
    if dst.is_null() {
        serial_dump::write_to_memory(ft, points, verbs, conics, rr_info.as_ref(), None)
    } else {
        let out = slice_from_raw_mut(dst, dst_cap);
        serial_dump::write_to_memory(ft, points, verbs, conics, rr_info.as_ref(), Some(out))
    }
}

#[repr(C)]
pub struct FfiReadPathResult {
    /// 0 = failed, 1 = General (`general_buf` non-null), 2 = RRect
    pub kind: u8,
    pub fill_type: u8,
    pub rrect_dir: u8,
    pub _pad: u8,
    pub rrect_start: u32,
    pub bytes_read: usize,
    pub rrect: RRect,
    pub general_buf: *mut PathDataBuf,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_read_from_memory(
    src: *const u8,
    len: usize,
) -> FfiReadPathResult {
    let bytes = slice_from_raw(src, len);
    match serial_dump::read_from_memory(bytes) {
        Some(ReadPathResult::General { buf, bytes_read }) => FfiReadPathResult {
            kind: 1,
            fill_type: buf.fill_type as u8,
            rrect_dir: 0,
            _pad: 0,
            rrect_start: 0,
            bytes_read,
            rrect: RRect::make_empty(),
            general_buf: Box::into_raw(Box::new(buf)),
        },
        Some(ReadPathResult::RRect {
            rrect,
            direction,
            start_index,
            fill_type,
            bytes_read,
        }) => FfiReadPathResult {
            kind: 2,
            fill_type: fill_type as u8,
            rrect_dir: direction as u8,
            _pad: 0,
            rrect_start: start_index,
            bytes_read,
            rrect,
            general_buf: core::ptr::null_mut(),
        },
        None => FfiReadPathResult {
            kind: 0,
            fill_type: 0,
            rrect_dir: 0,
            _pad: 0,
            rrect_start: 0,
            bytes_read: 0,
            rrect: RRect::make_empty(),
            general_buf: core::ptr::null_mut(),
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_path_dump_to_string(
    fill_type: u8,
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    dump_as_hex: bool,
    is_builder: bool,
) -> *mut String {
    let ft = FillType::from_u8(fill_type).unwrap_or(FillType::Winding);
    let points = slice_from_raw(pts, pt_count);
    let verbs = verbs_from_raw(vbs, vb_count).unwrap_or(&[]);
    let conics = slice_from_raw(conics, conic_count);
    let s = if is_builder {
        serial_dump::dump_builder_to_string(ft, points, verbs, conics, dump_as_hex)
    } else {
        serial_dump::dump_path_to_string(ft, points, verbs, conics, dump_as_hex)
    };
    Box::into_raw(Box::new(s))
}

// ============================================================================
// ContourMeasure / PathMeasure (SkContourMeasure.cpp, SkPathMeasure.cpp)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_contour_t_value_to_scalar(t: u32) -> f32 {
    measure::t_value_to_scalar(t)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_iter_new(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    force_closed: bool,
    res_scale: f32,
) -> *mut ContourMeasureIter {
    let points = slice_from_raw(pts, pt_count);
    let verbs = verbs_from_raw(vbs, vb_count).unwrap_or(&[]);
    let conics = slice_from_raw(conics, conic_count);
    let iter = ContourMeasureIter::from_slices(points, verbs, conics, force_closed, res_scale);
    Box::into_raw(Box::new(iter))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_iter_free(iter: *mut ContourMeasureIter) {
    if !iter.is_null() {
        drop(Box::from_raw(iter));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_iter_has_next(iter: *const ContourMeasureIter) -> bool {
    if iter.is_null() {
        return false;
    }
    (*iter).has_next_segments()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_iter_next(
    iter: *mut ContourMeasureIter,
) -> *mut ContourMeasure {
    if iter.is_null() {
        return core::ptr::null_mut();
    }
    match (*iter).next_contour() {
        Some(cm) => Box::into_raw(Box::new(cm)),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_measure_segments(
    cm: *const ContourMeasure,
    out_len: *mut usize,
) -> *const ContourSegment {
    let c = &*cm;
    *out_len = c.segments.len();
    c.segments.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_measure_pts(
    cm: *const ContourMeasure,
    out_len: *mut usize,
) -> *const Point {
    let c = &*cm;
    *out_len = c.pts.len();
    c.pts.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_measure_length(cm: *const ContourMeasure) -> f32 {
    (*cm).length()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_measure_is_closed(cm: *const ContourMeasure) -> bool {
    (*cm).is_closed()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_measure_free(cm: *mut ContourMeasure) {
    if !cm.is_null() {
        drop(Box::from_raw(cm));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_get_pos_tan(
    segs: *const ContourSegment,
    seg_count: usize,
    pts: *const Point,
    pt_count: usize,
    length: f32,
    distance: f32,
    out_pos: *mut Point,
    out_tan: *mut Vector,
) -> bool {
    let segments = slice_from_raw(segs, seg_count);
    let points = slice_from_raw(pts, pt_count);
    let pos_opt = if out_pos.is_null() {
        None
    } else {
        Some(&mut *out_pos)
    };
    let tan_opt = if out_tan.is_null() {
        None
    } else {
        Some(&mut *out_tan)
    };
    measure::contour_get_pos_tan(segments, points, length, distance, pos_opt, tan_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_get_matrix(
    segs: *const ContourSegment,
    seg_count: usize,
    pts: *const Point,
    pt_count: usize,
    length: f32,
    distance: f32,
    flags: u32,
    out_matrix: *mut Matrix,
) -> bool {
    let segments = slice_from_raw(segs, seg_count);
    let points = slice_from_raw(pts, pt_count);
    match measure::contour_get_matrix(segments, points, length, distance, flags) {
        Some(m) => {
            if !out_matrix.is_null() {
                *out_matrix = m;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_get_segment(
    segs: *const ContourSegment,
    seg_count: usize,
    pts: *const Point,
    pt_count: usize,
    length: f32,
    start_d: f32,
    stop_d: f32,
    start_with_move_to: bool,
    dst_pts: *const Point,
    dst_pt_count: usize,
    dst_vbs: *const u8,
    dst_vb_count: usize,
    dst_conics: *const f32,
    dst_conic_count: usize,
    dst_fill_type: u8,
    dst_convexity: u8,
    dst_seg_mask: u8,
    dst_last_move: i32,
    dst_is_a_type: u8,
    dst_is_a_dir: u8,
    dst_is_a_start: u8,
    dst_is_volatile: bool,
) -> *mut PathDataBuf {
    let segments = slice_from_raw(segs, seg_count);
    let points = slice_from_raw(pts, pt_count);
    let mut pb = builder_from_parts(
        dst_pts, dst_pt_count, dst_vbs, dst_vb_count, dst_conics, dst_conic_count, dst_fill_type,
        dst_convexity, dst_seg_mask, dst_last_move, dst_is_a_type, dst_is_a_dir, dst_is_a_start,
        dst_is_volatile,
    );
    if measure::contour_get_segment(
        segments,
        points,
        length,
        start_d,
        stop_d,
        &mut pb,
        start_with_move_to,
    ) {
        Box::into_raw(Box::new(pb.into_buf()))
    } else {
        core::ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_contour_seg_to(
    pts: *const Point,
    pt_count: usize,
    seg_type: u8,
    start_t: f32,
    stop_t: f32,
    dst_pts: *const Point,
    dst_pt_count: usize,
    dst_vbs: *const u8,
    dst_vb_count: usize,
    dst_conics: *const f32,
    dst_conic_count: usize,
    dst_fill_type: u8,
    dst_convexity: u8,
    dst_seg_mask: u8,
    dst_last_move: i32,
    dst_is_a_type: u8,
    dst_is_a_dir: u8,
    dst_is_a_start: u8,
    dst_is_volatile: bool,
) -> *mut PathDataBuf {
    let points = slice_from_raw(pts, pt_count);
    let mut pb = builder_from_parts(
        dst_pts, dst_pt_count, dst_vbs, dst_vb_count, dst_conics, dst_conic_count, dst_fill_type,
        dst_convexity, dst_seg_mask, dst_last_move, dst_is_a_type, dst_is_a_dir, dst_is_a_start,
        dst_is_volatile,
    );
    measure::contour_measure_seg_to(points, seg_type, start_t, stop_t, &mut pb);
    Box::into_raw(Box::new(pb.into_buf()))
}

// ============================================================================
// Stroke / StrokeRec / StrokerPriv (SkStroke.cpp, SkStrokeRec.cpp, SkStrokerPriv.cpp)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_stroke_rec_inflation_radius(
    join: u8,
    miter_limit: f32,
    cap: u8,
    stroke_width: f32,
) -> f32 {
    StrokeRec::compute_inflation_radius(
        Join::from_u8(join).unwrap_or(Join::Miter),
        miter_limit,
        Cap::from_u8(cap).unwrap_or(Cap::Butt),
        stroke_width,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stroke_path(
    width: f32,
    miter_limit: f32,
    res_scale: f32,
    cap: u8,
    join: u8,
    do_fill: bool,
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    bounds: Rect,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
) -> *mut PathDataBuf {
    let Some(raw) = make_path_raw(
        pts, pt_count, vbs, vb_count, conics, conic_count, bounds, fill_type, convexity,
        segment_mask,
    ) else {
        return core::ptr::null_mut();
    };
    let stroker = Stroke {
        width,
        miter_limit,
        res_scale,
        cap: Cap::from_u8(cap).unwrap_or(Cap::Butt),
        join: Join::from_u8(join).unwrap_or(Join::Miter),
        do_fill,
    };
    let mut dst = PathBuilder::new(FillType::Winding);
    stroker.stroke_path(&raw, &mut dst);
    Box::into_raw(Box::new(dst.into_buf()))
}

#[no_mangle]
pub extern "C" fn skia_rs_stroke_rect(
    width: f32,
    miter_limit: f32,
    res_scale: f32,
    cap: u8,
    join: u8,
    do_fill: bool,
    rect: Rect,
    dir: u8,
) -> *mut PathDataBuf {
    let stroker = Stroke {
        width,
        miter_limit,
        res_scale,
        cap: Cap::from_u8(cap).unwrap_or(Cap::Butt),
        join: Join::from_u8(join).unwrap_or(Join::Miter),
        do_fill,
    };
    let mut dst = PathBuilder::new(FillType::Winding);
    stroker.stroke_rect(rect, &mut dst, Direction::from_u8(dir).unwrap_or(Direction::Cw));
    Box::into_raw(Box::new(dst.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stroker_apply_cap(
    cap: u8,
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    fill_type: u8,
    convexity: u8,
    segment_mask: u8,
    last_move_index: i32,
    is_a_type: u8,
    is_a_dir: u8,
    is_a_start_index: u8,
    is_volatile: bool,
    pivot: Point,
    normal: Vector,
    stop: Point,
    extend_last_pt: bool,
) -> *mut PathDataBuf {
    let mut pb = builder_from_parts(
        pts, pt_count, vbs, vb_count, conics, conic_count, fill_type, convexity, segment_mask,
        last_move_index, is_a_type, is_a_dir, is_a_start_index, is_volatile,
    );
    stroke::apply_cap(
        Cap::from_u8(cap).unwrap_or(Cap::Butt),
        &mut pb,
        pivot,
        normal,
        stop,
        extend_last_pt,
    );
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stroker_apply_join(
    join: u8,
    outer_pts: *const Point,
    outer_pt_count: usize,
    outer_vbs: *const u8,
    outer_vb_count: usize,
    outer_conics: *const f32,
    outer_conic_count: usize,
    outer_fill_type: u8,
    outer_convexity: u8,
    outer_seg_mask: u8,
    outer_last_move: i32,
    outer_is_a_type: u8,
    outer_is_a_dir: u8,
    outer_is_a_start: u8,
    outer_is_volatile: bool,
    inner_pts: *const Point,
    inner_pt_count: usize,
    inner_vbs: *const u8,
    inner_vb_count: usize,
    inner_conics: *const f32,
    inner_conic_count: usize,
    inner_fill_type: u8,
    inner_convexity: u8,
    inner_seg_mask: u8,
    inner_last_move: i32,
    inner_is_a_type: u8,
    inner_is_a_dir: u8,
    inner_is_a_start: u8,
    inner_is_volatile: bool,
    before_unit_normal: Vector,
    pivot: Point,
    after_unit_normal: Vector,
    radius: f32,
    inv_miter_limit: f32,
    prev_is_line: bool,
    curr_is_line: bool,
    out_outer: *mut *mut PathDataBuf,
    out_inner: *mut *mut PathDataBuf,
) {
    let mut outer = builder_from_parts(
        outer_pts, outer_pt_count, outer_vbs, outer_vb_count, outer_conics, outer_conic_count,
        outer_fill_type, outer_convexity, outer_seg_mask, outer_last_move, outer_is_a_type,
        outer_is_a_dir, outer_is_a_start, outer_is_volatile,
    );
    let mut inner = builder_from_parts(
        inner_pts, inner_pt_count, inner_vbs, inner_vb_count, inner_conics, inner_conic_count,
        inner_fill_type, inner_convexity, inner_seg_mask, inner_last_move, inner_is_a_type,
        inner_is_a_dir, inner_is_a_start, inner_is_volatile,
    );
    stroke::apply_join(
        Join::from_u8(join).unwrap_or(Join::Miter),
        &mut outer,
        &mut inner,
        before_unit_normal,
        pivot,
        after_unit_normal,
        radius,
        inv_miter_limit,
        prev_is_line,
        curr_is_line,
    );
    *out_outer = Box::into_raw(Box::new(outer.into_buf()));
    *out_inner = Box::into_raw(Box::new(inner.into_buf()));
}

// ============================================================================
// DashPath & ParsePath (src/utils/SkDashPath.cpp, src/utils/SkParsePath.cpp)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_dash_valid_path(
    phase: f32,
    intervals: *const f32,
    count: usize,
) -> bool {
    dash::valid_dash_path(phase, slice_from_raw(intervals, count))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_dash_calc_parameters(
    phase: f32,
    intervals: *const f32,
    count: usize,
    out_initial_dash_length: *mut f32,
    out_initial_dash_index: *mut usize,
    out_interval_length: *mut f32,
    out_adjusted_phase: *mut f32,
) {
    let adjust = !out_adjusted_phase.is_null();
    let params = dash::calc_dash_parameters(phase, slice_from_raw(intervals, count), adjust);
    *out_initial_dash_length = params.initial_dash_length;
    *out_initial_dash_index = params.initial_dash_index;
    *out_interval_length = params.interval_length;
    if adjust {
        *out_adjusted_phase = params.adjusted_phase;
    }
}

#[repr(C)]
pub struct FfiStrokeRec {
    pub res_scale: f32,
    pub width: f32,
    pub miter_limit: f32,
    pub cap: u8,
    pub join: u8,
    pub stroke_and_fill: bool,
    pub _pad: u8,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_dash_internal_filter(
    dst_pts: *const Point,
    dst_pt_count: usize,
    dst_vbs: *const u8,
    dst_vb_count: usize,
    dst_conics: *const f32,
    dst_conic_count: usize,
    dst_fill_type: u8,
    dst_convexity: u8,
    dst_seg_mask: u8,
    dst_last_move: i32,
    dst_is_a_type: u8,
    dst_is_a_dir: u8,
    dst_is_a_start: u8,
    dst_is_volatile: bool,
    src_pts: *const Point,
    src_pt_count: usize,
    src_vbs: *const u8,
    src_vb_count: usize,
    src_conics: *const f32,
    src_conic_count: usize,
    src_bounds: Rect,
    src_fill_type: u8,
    src_convexity: u8,
    src_seg_mask: u8,
    io_rec: *mut FfiStrokeRec,
    cull_rect: *const Rect,
    intervals: *const f32,
    interval_count: usize,
    initial_dash_length: f32,
    initial_dash_index: i32,
    interval_length: f32,
    start_phase: f32,
    allow_stroke_rec_application: bool,
    out_ok: *mut bool,
) -> *mut PathDataBuf {
    let Some(raw) = make_path_raw(
        src_pts, src_pt_count, src_vbs, src_vb_count, src_conics, src_conic_count, src_bounds,
        src_fill_type, src_convexity, src_seg_mask,
    ) else {
        *out_ok = false;
        return core::ptr::null_mut();
    };
    let mut pb = builder_from_parts(
        dst_pts, dst_pt_count, dst_vbs, dst_vb_count, dst_conics, dst_conic_count, dst_fill_type,
        dst_convexity, dst_seg_mask, dst_last_move, dst_is_a_type, dst_is_a_dir, dst_is_a_start,
        dst_is_volatile,
    );
    let r_in = &*io_rec;
    let mut rec = StrokeRec {
        res_scale: r_in.res_scale,
        width: r_in.width,
        miter_limit: r_in.miter_limit,
        cap: Cap::from_u8(r_in.cap).unwrap_or(Cap::Butt),
        join: Join::from_u8(r_in.join).unwrap_or(Join::Miter),
        stroke_and_fill: r_in.stroke_and_fill,
    };
    let cull = if cull_rect.is_null() {
        None
    } else {
        Some(*cull_rect)
    };
    let ok = dash::internal_filter(
        &mut pb,
        &raw,
        &mut rec,
        cull,
        slice_from_raw(intervals, interval_count),
        initial_dash_length,
        initial_dash_index.max(0) as usize,
        interval_length,
        start_phase,
        allow_stroke_rec_application,
    );
    (*io_rec).res_scale = rec.res_scale;
    (*io_rec).width = rec.width;
    (*io_rec).miter_limit = rec.miter_limit;
    (*io_rec).cap = rec.cap as u8;
    (*io_rec).join = rec.join as u8;
    (*io_rec).stroke_and_fill = rec.stroke_and_fill;
    *out_ok = ok;
    Box::into_raw(Box::new(pb.into_buf()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_parse_path_from_svg(
    data: *const u8,
    len: usize,
) -> *mut PathDataBuf {
    if data.is_null() {
        return core::ptr::null_mut();
    }
    let bytes = slice_from_raw(data, len);
    match parse::from_svg_string(bytes) {
        Some(buf) => Box::into_raw(Box::new(buf)),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_parse_path_to_svg(
    pts: *const Point,
    pt_count: usize,
    vbs: *const u8,
    vb_count: usize,
    conics: *const f32,
    conic_count: usize,
    encoding: u8,
) -> *mut String {
    let points = slice_from_raw(pts, pt_count);
    let verbs = verbs_from_raw(vbs, vb_count).unwrap_or(&[]);
    let conics = slice_from_raw(conics, conic_count);
    let enc = if encoding == 1 {
        PathEncoding::Relative
    } else {
        PathEncoding::Absolute
    };
    let s = parse::to_svg_string(points, verbs, conics, enc);
    Box::into_raw(Box::new(s))
}

// Keep unused-import-free verification of PathIterRec alignment
const _: () = {
    let _ = core::mem::size_of::<PathIterRec>();
};
