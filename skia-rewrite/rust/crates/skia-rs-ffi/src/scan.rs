//! FFI exports for Phase 1 Layer 3.2 (`skia-rs-scan`, 11 C++ files):
//!   SkEdge, SkAnalyticEdge, SkEdgeBuilder, SkAlphaRuns, SkScan,
//!   SkScan_Path, SkScan_AntiPath, SkScan_AAAPath, SkScan_Hairline,
//!   SkScan_Antihair, SkDistanceFieldGen.

use core::ffi::c_void;
use skia_rs_geometry::{IRect, Point, Rect};
use skia_rs_path::types::{Convexity, FillType, PathRaw, Verb};
use skia_rs_scan::{
    aaa_path, alpha_runs,
    analytic_edge::{AnalyticCubicState, AnalyticEdgeBase, AnalyticQuadState},
    antihair::{self, AntiFrameRectParams},
    blitter::{Blitter, RegionClip},
    distance_field,
    edge::{CubicEdgeState, EdgeBase, QuadEdgeState},
    edge_builder::{self, EdgeSink},
    fixed::SkFixed,
    hairline::{self, HairCapStyle},
    scan_path, scan_rect,
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

/// C-compatible view of `SkPathRaw`.
#[repr(C)]
pub struct SkiaRsPathRawView {
    pub points: *const Point,
    pub point_count: usize,
    pub verbs: *const u8,
    pub verb_count: usize,
    pub conics: *const f32,
    pub conic_count: usize,
    pub bounds: Rect,
    pub fill_type: u8,
    pub convexity: u8,
    pub segment_mask: u8,
    pub _pad: [u8; 5],
}

impl SkiaRsPathRawView {
    #[inline]
    unsafe fn as_path_raw<'a>(&self) -> Option<PathRaw<'a>> {
        let points = slice_from_raw(self.points, self.point_count);
        let verbs = verbs_from_raw(self.verbs, self.verb_count)?;
        let conics = slice_from_raw(self.conics, self.conic_count);
        Some(PathRaw {
            points,
            verbs,
            conics,
            bounds: self.bounds,
            fill_type: FillType::from_u8(self.fill_type).unwrap_or(FillType::Winding),
            convexity: Convexity::from_u8(self.convexity).unwrap_or(Convexity::Unknown),
            segment_mask: self.segment_mask,
        })
    }
}

/// Callback vtable forwarding Rust `Blitter` calls back into C++ `SkBlitter`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsBlitterVTable {
    pub ctx: *mut c_void,
    pub blit_h: extern "C" fn(*mut c_void, i32, i32, i32),
    pub blit_anti_h: extern "C" fn(*mut c_void, i32, i32, *mut u8, *mut i16),
    pub blit_v: extern "C" fn(*mut c_void, i32, i32, i32, u8),
    pub blit_rect: extern "C" fn(*mut c_void, i32, i32, i32, i32),
    pub blit_anti_rect: extern "C" fn(*mut c_void, i32, i32, i32, i32, u8, u8),
    pub blit_anti_h2: extern "C" fn(*mut c_void, i32, i32, u8, u8),
    pub blit_anti_v2: extern "C" fn(*mut c_void, i32, i32, u8, u8),
    pub blit_mask_a8: extern "C" fn(*mut c_void, *const u8, *const IRect, u32, *const IRect),
    pub blit_fat_anti_rect: extern "C" fn(*mut c_void, *const Rect),
    pub request_rows_preserved: extern "C" fn(*mut c_void) -> i32,
    pub blit_hori_line: extern "C" fn(*mut c_void, i32, i32, i32, i32),
    pub blit_vert_line: extern "C" fn(*mut c_void, i32, i32, i32, i32),
}

struct FfiBlitter {
    vt: SkiaRsBlitterVTable,
}

impl Blitter for FfiBlitter {
    #[inline]
    fn blit_h(&mut self, x: i32, y: i32, width: i32) {
        (self.vt.blit_h)(self.vt.ctx, x, y, width);
    }

    #[inline]
    fn blit_anti_h(&mut self, x: i32, y: i32, antialias: &mut [u8], runs: &mut [i16]) {
        (self.vt.blit_anti_h)(self.vt.ctx, x, y, antialias.as_mut_ptr(), runs.as_mut_ptr());
    }

    #[inline]
    fn blit_v(&mut self, x: i32, y: i32, height: i32, alpha: u8) {
        (self.vt.blit_v)(self.vt.ctx, x, y, height, alpha);
    }

    #[inline]
    fn blit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        (self.vt.blit_rect)(self.vt.ctx, x, y, width, height);
    }

    #[inline]
    fn blit_anti_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        left_alpha: u8,
        right_alpha: u8,
    ) {
        (self.vt.blit_anti_rect)(self.vt.ctx, x, y, width, height, left_alpha, right_alpha);
    }

    #[inline]
    fn blit_anti_h2(&mut self, x: i32, y: i32, a0: u8, a1: u8) {
        (self.vt.blit_anti_h2)(self.vt.ctx, x, y, a0, a1);
    }

    #[inline]
    fn blit_anti_v2(&mut self, x: i32, y: i32, a0: u8, a1: u8) {
        (self.vt.blit_anti_v2)(self.vt.ctx, x, y, a0, a1);
    }

    #[inline]
    fn blit_mask_a8(&mut self, image: &[u8], bounds: IRect, row_bytes: u32, clip: IRect) {
        (self.vt.blit_mask_a8)(self.vt.ctx, image.as_ptr(), &bounds, row_bytes, &clip);
    }

    #[inline]
    fn blit_fat_anti_rect(&mut self, rect: Rect) {
        (self.vt.blit_fat_anti_rect)(self.vt.ctx, &rect);
    }

    #[inline]
    fn request_rows_preserved(&self) -> i32 {
        (self.vt.request_rows_preserved)(self.vt.ctx)
    }

    #[inline]
    fn blit_hori_line(&mut self, x: i32, stopx: i32, fy: SkFixed, dy: SkFixed) {
        (self.vt.blit_hori_line)(self.vt.ctx, x, stopx, fy, dy);
    }

    #[inline]
    fn blit_vert_line(&mut self, y: i32, stopy: i32, fx: SkFixed, dx: SkFixed) {
        (self.vt.blit_vert_line)(self.vt.ctx, y, stopy, fx, dx);
    }
}

/// Callback vtable forwarding Rust `RegionClip` queries back into C++ `SkRegion`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsRegionClipVTable {
    pub ctx: *const c_void,
    pub bounds: IRect,
    pub is_empty: bool,
    pub is_rect: bool,
    pub _pad: [u8; 6],
    pub quick_contains: extern "C" fn(*const c_void, *const IRect) -> bool,
    pub for_each_clip_rect: extern "C" fn(
        *const c_void,
        *const IRect,
        extern "C" fn(*mut c_void, *const IRect),
        *mut c_void,
    ),
}

struct FfiRegionClip {
    vt: SkiaRsRegionClipVTable,
}

impl RegionClip for FfiRegionClip {
    #[inline]
    fn is_empty(&self) -> bool {
        self.vt.is_empty
    }

    #[inline]
    fn is_rect(&self) -> bool {
        self.vt.is_rect
    }

    #[inline]
    fn bounds(&self) -> IRect {
        self.vt.bounds
    }

    #[inline]
    fn quick_contains(&self, r: IRect) -> bool {
        (self.vt.quick_contains)(self.vt.ctx, &r)
    }

    fn for_each_clip_rect(&self, query: IRect, f: &mut dyn FnMut(IRect)) {
        extern "C" fn trampoline(cb_ctx: *mut c_void, r: *const IRect) {
            if !cb_ctx.is_null() && !r.is_null() {
                unsafe {
                    let cb = &mut *(cb_ctx as *mut &mut dyn FnMut(IRect));
                    cb(*r);
                }
            }
        }
        let mut fat_ptr: &mut dyn FnMut(IRect) = f;
        let cb_ctx = (&mut fat_ptr as *mut &mut dyn FnMut(IRect)) as *mut c_void;
        (self.vt.for_each_clip_rect)(self.vt.ctx, &query, trampoline, cb_ctx);
    }
}

/// Callback vtable for C++ `SkEdgeBuilder::build` / `buildPoly`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsEdgeSinkVTable {
    pub ctx: *mut c_void,
    pub add_line: extern "C" fn(*mut c_void, *const Point),
    pub add_quad: extern "C" fn(*mut c_void, *const Point),
    pub add_cubic: extern "C" fn(*mut c_void, *const Point),
}

struct FfiEdgeSink {
    vt: SkiaRsEdgeSinkVTable,
}

impl EdgeSink for FfiEdgeSink {
    #[inline]
    fn add_line(&mut self, pts: &[Point; 2]) {
        (self.vt.add_line)(self.vt.ctx, pts.as_ptr());
    }

    #[inline]
    fn add_quad(&mut self, pts: &[Point; 3]) {
        (self.vt.add_quad)(self.vt.ctx, pts.as_ptr());
    }

    #[inline]
    fn add_cubic(&mut self, pts: &[Point; 4]) {
        (self.vt.add_cubic)(self.vt.ctx, pts.as_ptr());
    }
}

// ============================================================================
// 1. SkEdge / SkQuadraticEdge / SkCubicEdge
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_set_line(
    state: *mut EdgeBase,
    p0: *const Point,
    p1: *const Point,
    clip: *const IRect,
) -> bool {
    if state.is_null() || p0.is_null() || p1.is_null() {
        return false;
    }
    let clip_opt = if clip.is_null() { None } else { Some(*clip) };
    (*state).set_line(*p0, *p1, clip_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_update_line(
    state: *mut EdgeBase,
    x_start: i32,
    y_start: i32,
    x_end: i32,
    y_end: i32,
) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).update_line(x_start, y_start, x_end, y_end)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_chop_line_with_clip(
    state: *mut EdgeBase,
    clip: *const IRect,
) {
    if state.is_null() || clip.is_null() {
        return;
    }
    (*state).chop_line_with_clip(*clip);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_quad_edge_set_quadratic(
    state: *mut QuadEdgeState,
    pts: *const Point,
) -> bool {
    if state.is_null() || pts.is_null() {
        return false;
    }
    let p = &*(pts as *const [Point; 3]);
    (*state).set_quadratic(p)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_quad_edge_next_segment(state: *mut QuadEdgeState) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).next_segment()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_edge_set_cubic(
    state: *mut CubicEdgeState,
    pts: *const Point,
) -> bool {
    if state.is_null() || pts.is_null() {
        return false;
    }
    let p = &*(pts as *const [Point; 4]);
    (*state).set_cubic(p)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_edge_next_segment(state: *mut CubicEdgeState) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).next_segment()
}

// ============================================================================
// 2. SkAnalyticEdge / SkAnalyticQuadraticEdge / SkAnalyticCubicEdge
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_edge_set_line(
    state: *mut AnalyticEdgeBase,
    p0: *const Point,
    p1: *const Point,
) -> bool {
    if state.is_null() || p0.is_null() || p1.is_null() {
        return false;
    }
    (*state).set_line(*p0, *p1)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_edge_update_line(
    state: *mut AnalyticEdgeBase,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    slope: i32,
) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).update_line(x0, y0, x1, y1, slope)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_quad_set_without_update(
    state: *mut AnalyticQuadState,
    pts: *const Point,
    shift: i32,
) -> bool {
    if state.is_null() || pts.is_null() {
        return false;
    }
    let p = &*(pts as *const [Point; 3]);
    (*state).set_quadratic_without_update(p, shift)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_quad_set(
    state: *mut AnalyticQuadState,
    pts: *const Point,
) -> bool {
    if state.is_null() || pts.is_null() {
        return false;
    }
    let p = &*(pts as *const [Point; 3]);
    (*state).set_quadratic(p)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_quad_update(state: *mut AnalyticQuadState) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).update_quadratic()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_cubic_set_without_update(
    state: *mut AnalyticCubicState,
    pts: *const Point,
    shift: i32,
) -> bool {
    if state.is_null() || pts.is_null() {
        return false;
    }
    let p = &*(pts as *const [Point; 4]);
    (*state).set_cubic_without_update(p, shift)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_cubic_set(
    state: *mut AnalyticCubicState,
    pts: *const Point,
) -> bool {
    if state.is_null() || pts.is_null() {
        return false;
    }
    let p = &*(pts as *const [Point; 4]);
    (*state).set_cubic(p)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_analytic_cubic_update(state: *mut AnalyticCubicState) -> bool {
    if state.is_null() {
        return false;
    }
    (*state).update_cubic()
}

// ============================================================================
// 3. SkEdgeBuilder
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_builder_combine_vertical_basic(
    edge: *const EdgeBase,
    last: *mut EdgeBase,
    can_cull_to_the_right: bool,
) -> i32 {
    if edge.is_null() || last.is_null() {
        return 0;
    }
    edge_builder::combine_vertical_basic(&*edge, &mut *last, can_cull_to_the_right) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_builder_combine_vertical_analytic(
    edge: *const AnalyticEdgeBase,
    last: *mut AnalyticEdgeBase,
    can_cull_to_the_right: bool,
) -> i32 {
    if edge.is_null() || last.is_null() {
        return 0;
    }
    edge_builder::combine_vertical_analytic(&*edge, &mut *last, can_cull_to_the_right) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_edge_builder_drive(
    raw: *const SkiaRsPathRawView,
    iclip: *const IRect,
    can_cull_to_the_right: bool,
    sink: *const SkiaRsEdgeSinkVTable,
) -> bool {
    if raw.is_null() || sink.is_null() {
        return false;
    }
    let Some(path_raw) = (*raw).as_path_raw() else {
        return false;
    };
    let clip_opt = if iclip.is_null() { None } else { Some(*iclip) };
    let mut ffi_sink = FfiEdgeSink { vt: *sink };
    edge_builder::drive_edge_builder(&path_raw, clip_opt, can_cull_to_the_right, &mut ffi_sink)
}

// ============================================================================
// 4. SkAlphaRuns
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_alpha_runs_reset(
    runs: *mut i16,
    alpha: *mut u8,
    width: i32,
) {
    if runs.is_null() || alpha.is_null() || width <= 0 {
        return;
    }
    let w = width as usize;
    let r_slice = core::slice::from_raw_parts_mut(runs, w + 1);
    let a_slice = core::slice::from_raw_parts_mut(alpha, w);
    alpha_runs::reset_slice(r_slice, a_slice, w);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_alpha_runs_validate(runs: *const i16, width: i32) -> bool {
    if runs.is_null() || width <= 0 {
        return false;
    }
    let w = width as usize;
    let r_slice = core::slice::from_raw_parts(runs, w + 1);
    alpha_runs::validate_slice(r_slice, w)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_alpha_runs_assert_valid(
    runs: *const i16,
    alpha: *const u8,
    width: i32,
    y: i32,
    max_step: i32,
) -> bool {
    if runs.is_null() || alpha.is_null() || width <= 0 {
        return false;
    }
    let w = width as usize;
    let r_slice = core::slice::from_raw_parts(runs, w + 1);
    let a_slice = core::slice::from_raw_parts(alpha, w);
    alpha_runs::assert_valid_slice(r_slice, a_slice, y, max_step)
}

// ============================================================================
// 5 & 6. SkScan & SkScan_Path
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_fill_irect(
    r: *const IRect,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if r.is_null() || blitter.is_null() {
        return;
    }
    let mut b = FfiBlitter { vt: *blitter };
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    scan_rect::fill_irect(*r, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_fill_xrect(
    xr: *const IRect,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if xr.is_null() || blitter.is_null() {
        return;
    }
    let mut b = FfiBlitter { vt: *blitter };
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    scan_rect::fill_xrect(*xr, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_fill_rect(
    r: *const Rect,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if r.is_null() || blitter.is_null() {
        return;
    }
    let mut b = FfiBlitter { vt: *blitter };
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    scan_rect::fill_rect(*r, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_path_requires_tiling(bounds: *const IRect) -> bool {
    if bounds.is_null() {
        return false;
    }
    scan_rect::path_requires_tiling(*bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_clip_limit_rect(out: *mut IRect) {
    if !out.is_null() {
        *out = scan_rect::clip_limit_rect();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_compute_fill_path_bounds(
    raw_bounds: *const Rect,
    out_ir: *mut IRect,
    out_pre_clipped: *mut bool,
) {
    if raw_bounds.is_null() || out_ir.is_null() || out_pre_clipped.is_null() {
        return;
    }
    let (ir, pre) = scan_rect::compute_fill_path_bounds(*raw_bounds);
    *out_ir = ir;
    *out_pre_clipped = pre;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_conservative_round_to_int(
    src: *const Rect,
    out_ir: *mut IRect,
) {
    if src.is_null() || out_ir.is_null() {
        return;
    }
    *out_ir = scan_rect::conservative_round_to_int(*src);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_fill_path_clipped(
    raw: *const SkiaRsPathRawView,
    clip_rect: *const IRect,
    blitter: *const SkiaRsBlitterVTable,
    start_y: i32,
    stop_y: i32,
    path_contained_in_clip: bool,
) {
    if raw.is_null() || clip_rect.is_null() || blitter.is_null() {
        return;
    }
    let Some(path_raw) = (*raw).as_path_raw() else {
        return;
    };
    let mut b = FfiBlitter { vt: *blitter };
    scan_path::fill_path_clipped(
        &path_raw,
        *clip_rect,
        &mut b,
        start_y,
        stop_y,
        path_contained_in_clip,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_fill_triangle_clipped(
    pts: *const Point,
    clip_rect: *const IRect,
    blitter: *const SkiaRsBlitterVTable,
    ir: *const IRect,
) {
    if pts.is_null() || blitter.is_null() || ir.is_null() {
        return;
    }
    let tri = &*(pts as *const [Point; 3]);
    let clip_opt = if clip_rect.is_null() {
        None
    } else {
        Some(*clip_rect)
    };
    let mut b = FfiBlitter { vt: *blitter };
    scan_path::fill_triangle_clipped(tri, clip_opt, &mut b, *ir);
}

// ============================================================================
// 7 & 8. SkScan_AntiPath & SkScan_AAAPath
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_classify_anti_fill_path(
    path_bounds: *const Rect,
    clip_bounds: *const IRect,
    is_inverse: bool,
    out_ir: *mut IRect,
) -> i32 {
    if path_bounds.is_null() || clip_bounds.is_null() || out_ir.is_null() {
        return 0;
    }
    let (action, ir) = scan_rect::classify_anti_fill_path(*path_bounds, *clip_bounds, is_inverse);
    *out_ir = ir;
    action as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_aaa_fill_path(
    raw: *const SkiaRsPathRawView,
    blitter: *const SkiaRsBlitterVTable,
    ir: *const IRect,
    clip_bounds: *const IRect,
    force_rle: bool,
) {
    if raw.is_null() || blitter.is_null() || ir.is_null() || clip_bounds.is_null() {
        return;
    }
    let Some(path_raw) = (*raw).as_path_raw() else {
        return;
    };
    let mut b = FfiBlitter { vt: *blitter };
    aaa_path::aaa_fill_path(&path_raw, &mut b, *ir, *clip_bounds, force_rle);
}

// ============================================================================
// 9 & 10. SkScan_Hairline & SkScan_Antihair
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_hair_line_rgn(
    pts: *const Point,
    count: usize,
    clip: *const SkiaRsRegionClipVTable,
    orig_blitter: *const SkiaRsBlitterVTable,
    clipped_blitter: *const SkiaRsBlitterVTable,
) {
    if orig_blitter.is_null() || count < 2 || pts.is_null() {
        return;
    }
    let slice = slice_from_raw(pts, count);
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    let mut orig_b = FfiBlitter { vt: *orig_blitter };
    let mut clip_b = if clipped_blitter.is_null() {
        None
    } else {
        Some(FfiBlitter {
            vt: *clipped_blitter,
        })
    };
    hairline::hair_line_rgn(slice, clip_wrap.as_ref(), &mut orig_b, clip_b.as_mut());
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_hair_rect_bounds(
    rect: *const Rect,
    clip_bounds: *const IRect,
    out_r: *mut IRect,
) -> bool {
    if rect.is_null() || clip_bounds.is_null() || out_r.is_null() {
        return false;
    }
    match hairline::hair_rect_bounds(*rect, *clip_bounds) {
        Some(r) => {
            *out_r = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_hair_rect_emit(
    r: *const IRect,
    blitter: *const SkiaRsBlitterVTable,
) {
    if r.is_null() || blitter.is_null() {
        return;
    }
    let mut b = FfiBlitter { vt: *blitter };
    hairline::hair_rect_emit(*r, &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_hair_path_bounds(
    raw_bounds: *const Rect,
    cap_style: u8,
    out_ir: *mut IRect,
) {
    if raw_bounds.is_null() || out_ir.is_null() {
        return;
    }
    *out_ir = hairline::hair_path_bounds(*raw_bounds, HairCapStyle::from_u8(cap_style));
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_hair_path_clipped(
    raw: *const SkiaRsPathRawView,
    clip: *const SkiaRsRegionClipVTable,
    clip_is_rect: bool,
    orig_blitter: *const SkiaRsBlitterVTable,
    clipped_blitter: *const SkiaRsBlitterVTable,
    cap_style: u8,
    is_anti: bool,
) {
    if raw.is_null() || orig_blitter.is_null() {
        return;
    }
    let Some(path_raw) = (*raw).as_path_raw() else {
        return;
    };
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    let mut orig_b = FfiBlitter { vt: *orig_blitter };
    let mut clip_b = if clipped_blitter.is_null() {
        None
    } else {
        Some(FfiBlitter {
            vt: *clipped_blitter,
        })
    };
    hairline::hair_path_clipped(
        &path_raw,
        clip_wrap.as_ref(),
        clip_is_rect,
        &mut orig_b,
        clip_b.as_mut(),
        HairCapStyle::from_u8(cap_style),
        is_anti,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_frame_rect(
    r: *const Rect,
    stroke_size: *const Point,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if r.is_null() || stroke_size.is_null() || blitter.is_null() {
        return;
    }
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    let mut b = FfiBlitter { vt: *blitter };
    hairline::frame_rect(*r, *stroke_size, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_anti_hair_line_rgn(
    pts: *const Point,
    count: usize,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if blitter.is_null() || count < 2 || pts.is_null() {
        return;
    }
    let slice = slice_from_raw(pts, count);
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    let mut b = FfiBlitter { vt: *blitter };
    antihair::anti_hair_line_rgn(slice, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_anti_fill_xrect(
    xr: *const IRect,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if xr.is_null() || blitter.is_null() {
        return;
    }
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    let mut b = FfiBlitter { vt: *blitter };
    antihair::anti_fill_xrect(*xr, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_anti_fill_rect(
    r: *const Rect,
    clip: *const SkiaRsRegionClipVTable,
    blitter: *const SkiaRsBlitterVTable,
) {
    if r.is_null() || blitter.is_null() {
        return;
    }
    let clip_wrap = if clip.is_null() {
        None
    } else {
        Some(FfiRegionClip { vt: *clip })
    };
    let mut b = FfiBlitter { vt: *blitter };
    antihair::anti_fill_rect(*r, clip_wrap.as_ref(), &mut b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_anti_frame_rect_setup(
    r: *const Rect,
    stroke_size: *const Point,
    out_params: *mut AntiFrameRectParams,
) {
    if r.is_null() || stroke_size.is_null() || out_params.is_null() {
        return;
    }
    *out_params = antihair::anti_frame_rect_setup(*r, *stroke_size);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_scan_anti_frame_rect_draw(
    r: *const Rect,
    stroke_size: *const Point,
    params: *const AntiFrameRectParams,
    blitter: *const SkiaRsBlitterVTable,
) {
    if r.is_null() || stroke_size.is_null() || params.is_null() || blitter.is_null() {
        return;
    }
    let mut b = FfiBlitter { vt: *blitter };
    antihair::anti_frame_rect_draw(*r, *stroke_size, &*params, &mut b);
}

// ============================================================================
// 11. SkDistanceFieldGen
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_compute_distance_field_size(w: i32, h: i32) -> usize {
    distance_field::compute_distance_field_size(w, h)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_generate_distance_field_a8(
    distance_field: *mut u8,
    image: *const u8,
    w: i32,
    h: i32,
    row_bytes: usize,
) -> bool {
    let df_size = distance_field::compute_distance_field_size(w, h);
    if df_size == 0 || distance_field.is_null() || image.is_null() {
        return false;
    }
    let wu = w as usize;
    let hu = h as usize;
    let Some(img_len) = (hu - 1).checked_mul(row_bytes).and_then(|v| v.checked_add(wu)) else {
        return false;
    };
    let df_slice = core::slice::from_raw_parts_mut(distance_field, df_size);
    let img_slice = core::slice::from_raw_parts(image, img_len);
    distance_field::generate_distance_field_from_a8_image(df_slice, img_slice, w, h, row_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_generate_distance_field_lcd16(
    distance_field: *mut u8,
    image: *const u8,
    w: i32,
    h: i32,
    row_bytes: usize,
) -> bool {
    let df_size = distance_field::compute_distance_field_size(w, h);
    if df_size == 0 || distance_field.is_null() || image.is_null() {
        return false;
    }
    let wu = w as usize;
    let hu = h as usize;
    let Some(img_len) = (hu - 1)
        .checked_mul(row_bytes)
        .and_then(|v| wu.checked_mul(2).and_then(|w2| v.checked_add(w2)))
    else {
        return false;
    };
    let df_slice = core::slice::from_raw_parts_mut(distance_field, df_size);
    let img_slice = core::slice::from_raw_parts(image, img_len);
    distance_field::generate_distance_field_from_lcd16_mask(df_slice, img_slice, w, h, row_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_generate_distance_field_bw(
    distance_field: *mut u8,
    image: *const u8,
    w: i32,
    h: i32,
    row_bytes: usize,
) -> bool {
    let df_size = distance_field::compute_distance_field_size(w, h);
    if df_size == 0 || distance_field.is_null() || image.is_null() {
        return false;
    }
    let wu = w as usize;
    let hu = h as usize;
    let bytes_per_row = (wu + 7) >> 3;
    let Some(img_len) = (hu - 1)
        .checked_mul(row_bytes)
        .and_then(|v| v.checked_add(bytes_per_row))
    else {
        return false;
    };
    let df_slice = core::slice::from_raw_parts_mut(distance_field, df_size);
    let img_slice = core::slice::from_raw_parts(image, img_len);
    distance_field::generate_distance_field_from_bw_image(df_slice, img_slice, w, h, row_bytes)
}
