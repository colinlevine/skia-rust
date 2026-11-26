//! FFI exports for `skia-rs-region` (Layer 2.3).
//!
//! `unsafe` is strictly restricted to this FFI boundary; all region, antialiased
//! clip, raster clip, clip stack, and R-Tree algorithms run in safe Rust inside
//! `skia-rs-region` (`#![forbid(unsafe_code)]`).

use core::ffi::c_void;
use skia_rs_geometry::{IRect, Rect};
use skia_rs_region::{
    aa_clip::{self, AAClip, AAClipBuilder, YOffset},
    clip_stack::{
        self, BoundUpdateResult, BoundsType, DeviceSpaceType, FillCombo, PriorBoundInfo,
        PushDecision,
    },
    raster_clip,
    region::{self, NormalizedRuns, Region, RegionOp, RunBoundsInfo, RECT_REGION_RUNS},
    region_path::{self, PathCmd, PathTileGrid, RegionBuilder},
    rtree::{self, RTree, RTreeBranch, RTreeNode, MAX_CHILDREN},
    ClipOp,
};

// ===========================================================================
// Helpers for opaque Rust-owned buffers
// ===========================================================================

pub struct RegionRunsBuf(Vec<i32>);
pub struct PathCmdBuf(Vec<PathCmd>);
pub struct AAClipBuf(AAClip);

unsafe fn write_region_result(
    res: Region,
    out_bounds: *mut IRect,
    out_y_spans: *mut i32,
    out_intervals: *mut i32,
    out_buf: *mut *mut RegionRunsBuf,
) -> i32 {
    if !out_buf.is_null() {
        *out_buf = core::ptr::null_mut();
    }
    match res {
        Region::Empty => {
            if !out_bounds.is_null() {
                *out_bounds = IRect::EMPTY;
            }
            0
        }
        Region::Rect(b) => {
            if !out_bounds.is_null() {
                *out_bounds = b;
            }
            1
        }
        Region::Complex {
            bounds,
            y_span_count,
            interval_count,
            runs,
        } => {
            if !out_bounds.is_null() {
                *out_bounds = bounds;
            }
            if !out_y_spans.is_null() {
                *out_y_spans = y_span_count;
            }
            if !out_intervals.is_null() {
                *out_intervals = interval_count;
            }
            if !out_buf.is_null() {
                *out_buf = Box::into_raw(Box::new(RegionRunsBuf(runs)));
            }
            2
        }
    }
}

unsafe fn write_normalized_result(
    res: NormalizedRuns,
    out_bounds: *mut IRect,
    out_y_spans: *mut i32,
    out_intervals: *mut i32,
    out_buf: *mut *mut RegionRunsBuf,
) -> i32 {
    if !out_buf.is_null() {
        *out_buf = core::ptr::null_mut();
    }
    match res {
        NormalizedRuns::Empty => {
            if !out_bounds.is_null() {
                *out_bounds = IRect::EMPTY;
            }
            0
        }
        NormalizedRuns::Rect(b) => {
            if !out_bounds.is_null() {
                *out_bounds = b;
            }
            1
        }
        NormalizedRuns::Complex {
            bounds,
            y_span_count,
            interval_count,
            runs,
        } => {
            if !out_bounds.is_null() {
                *out_bounds = bounds;
            }
            if !out_y_spans.is_null() {
                *out_y_spans = y_span_count;
            }
            if !out_intervals.is_null() {
                *out_intervals = interval_count;
            }
            if !out_buf.is_null() {
                *out_buf = Box::into_raw(Box::new(RegionRunsBuf(runs)));
            }
            2
        }
    }
}

unsafe fn slice_from_raw<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(ptr, len)
    }
}

unsafe fn slice_from_raw_mut<'a, T>(ptr: *mut T, len: usize) -> &'a mut [T] {
    if ptr.is_null() || len == 0 {
        &mut []
    } else {
        core::slice::from_raw_parts_mut(ptr, len)
    }
}

// ===========================================================================
// SkRegion (`skia_rs_region::region`)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_runs_buf_data(
    buf: *const RegionRunsBuf,
    out_len: *mut usize,
) -> *const i32 {
    if buf.is_null() {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return core::ptr::null();
    }
    let v = &(*buf).0;
    if !out_len.is_null() {
        *out_len = v.len();
    }
    v.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_runs_buf_free(buf: *mut RegionRunsBuf) {
    if !buf.is_null() {
        drop(Box::from_raw(buf));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_build_rect_runs(
    bounds: *const IRect,
    out_runs: *mut i32,
) {
    let arr = region::build_rect_runs(*bounds);
    let dst = core::slice::from_raw_parts_mut(out_runs, RECT_REGION_RUNS);
    dst.copy_from_slice(&arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_runs_are_a_rect(
    runs: *const i32,
    count: i32,
    out_bounds: *mut IRect,
) -> bool {
    if count <= 0 || runs.is_null() {
        return false;
    }
    let slice = core::slice::from_raw_parts(runs, count as usize);
    match region::runs_are_a_rect(slice) {
        Some(b) => {
            if !out_bounds.is_null() {
                *out_bounds = b;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_complex_contains_xy(
    runs: *const i32,
    run_count: usize,
    x: i32,
    y: i32,
) -> bool {
    let slice = slice_from_raw(runs, run_count);
    region::complex_runs_contains_xy(slice, x, y)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_complex_contains_rect(
    runs: *const i32,
    run_count: usize,
    r: *const IRect,
) -> bool {
    let slice = slice_from_raw(runs, run_count);
    region::complex_runs_contains_rect(slice, *r)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_complex_intersects_rect(
    runs: *const i32,
    run_count: usize,
    sect: *const IRect,
) -> bool {
    let slice = slice_from_raw(runs, run_count);
    region::complex_runs_intersects_rect(slice, *sect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_pin_offset_s32(min: i32, max: i32, offset: i32) -> i32 {
    region::pin_offset_s32(min, max, offset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_translate_runs(
    src: *const i32,
    dst: *mut i32,
    run_count: usize,
    dx: i32,
    dy: i32,
) {
    if run_count == 0 || dst.is_null() {
        return;
    }
    if src == dst as *const i32 {
        let slice = core::slice::from_raw_parts_mut(dst, run_count);
        region::translate_runs_in_place(slice, dx, dy);
    } else {
        let s = core::slice::from_raw_parts(src, run_count);
        let d = core::slice::from_raw_parts_mut(dst, run_count);
        region::translate_runs(s, d, dx, dy);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_compute_run_bounds(
    runs: *const i32,
    run_count: usize,
    out_info: *mut RunBoundsInfo,
) {
    let slice = slice_from_raw(runs, run_count);
    *out_info = region::compute_run_bounds(slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_validate_runs(
    runs: *const i32,
    run_count: usize,
    given_bounds: *const IRect,
    y_span_count: i32,
    interval_count: i32,
) -> bool {
    let slice = slice_from_raw(runs, run_count);
    region::validate_runs(slice, *given_bounds, y_span_count, interval_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_normalize_runs(
    runs: *mut i32,
    count: usize,
    out_bounds: *mut IRect,
    out_y_spans: *mut i32,
    out_intervals: *mut i32,
    out_buf: *mut *mut RegionRunsBuf,
) -> i32 {
    let slice = slice_from_raw_mut(runs, count);
    let res = region::normalize_runs(slice);
    write_normalized_result(res, out_bounds, out_y_spans, out_intervals, out_buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_operate_runs(
    a_runs: *const i32,
    a_len: usize,
    b_runs: *const i32,
    b_len: usize,
    op: i32,
    want_result: bool,
    out_bounds: *mut IRect,
    out_y_spans: *mut i32,
    out_intervals: *mut i32,
    out_buf: *mut *mut RegionRunsBuf,
) -> i32 {
    let Some(rop) = RegionOp::from_i32(op) else {
        if !out_buf.is_null() {
            *out_buf = core::ptr::null_mut();
        }
        return 0;
    };
    let a = slice_from_raw(a_runs, a_len);
    let b = slice_from_raw(b_runs, b_len);
    let mut dst_runs = Vec::new();
    let non_empty = region::operate_runs(a, b, rop, !want_result, &mut dst_runs);
    if !non_empty {
        if !out_buf.is_null() {
            *out_buf = core::ptr::null_mut();
        }
        return 0;
    }
    if !want_result {
        if !out_buf.is_null() {
            *out_buf = core::ptr::null_mut();
        }
        return 1;
    }
    let norm = region::normalize_runs(&mut dst_runs);
    write_normalized_result(norm, out_bounds, out_y_spans, out_intervals, out_buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_set_rects(
    rects: *const IRect,
    count: usize,
    out_bounds: *mut IRect,
    out_y_spans: *mut i32,
    out_intervals: *mut i32,
    out_buf: *mut *mut RegionRunsBuf,
) -> i32 {
    let slice = slice_from_raw(rects, count);
    let mut rgn = Region::Empty;
    rgn.set_rects(slice);
    write_region_result(rgn, out_bounds, out_y_spans, out_intervals, out_buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_iter_next(
    runs: *const i32,
    run_count: usize,
    offset: usize,
    rect: *mut IRect,
    out_next_offset: *mut usize,
) -> bool {
    let slice = slice_from_raw(runs, run_count);
    match region::complex_iter_next(slice, offset, &mut *rect) {
        Some(next_off) => {
            *out_next_offset = next_off;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_spanerator_init(
    runs: *const i32,
    run_count: usize,
    y: i32,
    left: i32,
    right: i32,
    out_offset: *mut usize,
) -> bool {
    let slice = slice_from_raw(runs, run_count);
    match region::complex_spanerator_init(slice, y, left, right) {
        Some(off) => {
            *out_offset = off;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_visit_spans(
    runs: *const i32,
    run_count: usize,
    cb: extern "C" fn(*const IRect, *mut c_void),
    ctx: *mut c_void,
) {
    let slice = slice_from_raw(runs, run_count);
    region::visit_complex_runs_spans(slice, |r| {
        cb(&r as *const IRect, ctx);
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_serialized_size(
    is_empty: bool,
    is_rect: bool,
    run_count: usize,
) -> usize {
    region::serialized_size(is_empty, is_rect, run_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_write_to_memory(
    is_empty: bool,
    is_rect: bool,
    bounds: *const IRect,
    y_span_count: i32,
    interval_count: i32,
    runs: *const i32,
    run_count: usize,
    dst: *mut u8,
    dst_len: usize,
) -> usize {
    let b = if bounds.is_null() {
        IRect::EMPTY
    } else {
        *bounds
    };
    let run_slice = slice_from_raw(runs, run_count);
    let out = if dst.is_null() {
        None
    } else {
        Some(core::slice::from_raw_parts_mut(dst, dst_len))
    };
    region::write_to_memory(is_empty, is_rect, b, y_span_count, interval_count, run_slice, out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_read_from_memory(
    src: *const u8,
    src_len: usize,
    out_kind: *mut i32,
    out_bounds: *mut IRect,
    out_y_spans: *mut i32,
    out_intervals: *mut i32,
    out_buf: *mut *mut RegionRunsBuf,
) -> usize {
    if !out_buf.is_null() {
        *out_buf = core::ptr::null_mut();
    }
    let slice = slice_from_raw(src, src_len);
    match region::read_from_memory(slice) {
        Some((rgn, bytes_read)) => {
            let kind = write_region_result(rgn, out_bounds, out_y_spans, out_intervals, out_buf);
            if !out_kind.is_null() {
                *out_kind = kind;
            }
            bytes_read
        }
        None => 0,
    }
}

// ===========================================================================
// SkRegion_path (`skia_rs_region::region_path`)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_verb_to_initial_last_index(verb: u8) -> u8 {
    region_path::verb_to_initial_last_index(verb)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_verb_to_max_edges(verb: u8) -> u32 {
    region_path::verb_to_max_edges(verb)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_compute_path_tile_grid(
    path_bounds: *const IRect,
    clip_bounds: *const IRect,
    out_grid: *mut PathTileGrid,
) -> bool {
    match region_path::compute_path_tile_grid(*path_bounds, *clip_bounds) {
        Some(grid) => {
            if !out_grid.is_null() {
                *out_grid = grid;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_new(
    max_height: i32,
    max_transitions: i32,
    path_is_inverse: bool,
) -> *mut RegionBuilder {
    match RegionBuilder::with_capacity(max_height, max_transitions, path_is_inverse) {
        Some(b) => Box::into_raw(Box::new(b)),
        None => core::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_free(b: *mut RegionBuilder) {
    if !b.is_null() {
        drop(Box::from_raw(b));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_blit_h(
    b: *mut RegionBuilder,
    x: i32,
    y: i32,
    width: i32,
) {
    (*b).blit_h(x, y, width);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_done(b: *mut RegionBuilder) {
    (*b).done();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_compute_run_count(b: *const RegionBuilder) -> i32 {
    (*b).compute_run_count() as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_to_rect(
    b: *const RegionBuilder,
    out_rect: *mut IRect,
) -> bool {
    match (*b).to_rect() {
        Some(r) => {
            *out_rect = r;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_builder_copy_to_runs(
    b: *const RegionBuilder,
    dst: *mut i32,
    len: usize,
) {
    let slice = slice_from_raw_mut(dst, len);
    (*b).copy_to_runs(slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_extract_boundary_path(
    runs: *const i32,
    run_count: usize,
) -> *mut PathCmdBuf {
    let slice = slice_from_raw(runs, run_count);
    let cmds = region_path::extract_boundary_path_from_complex_runs(slice);
    Box::into_raw(Box::new(PathCmdBuf(cmds)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_path_cmd_buf_data(
    buf: *const PathCmdBuf,
    out_len: *mut usize,
) -> *const PathCmd {
    if buf.is_null() {
        if !out_len.is_null() {
            *out_len = 0;
        }
        return core::ptr::null();
    }
    let v = &(*buf).0;
    if !out_len.is_null() {
        *out_len = v.len();
    }
    v.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_region_path_cmd_buf_free(buf: *mut PathCmdBuf) {
    if !buf.is_null() {
        drop(Box::from_raw(buf));
    }
}

// ===========================================================================
// SkAAClip & SkAAClipBlitter (`skia_rs_region::aa_clip`)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_buf_info(
    buf: *const AAClipBuf,
    out_bounds: *mut IRect,
    out_yoffsets: *mut *const YOffset,
    out_row_count: *mut usize,
    out_data: *mut *const u8,
    out_data_len: *mut usize,
) -> bool {
    if buf.is_null() {
        return false;
    }
    let clip = &(*buf).0;
    if clip.is_empty() {
        return false;
    }
    if !out_bounds.is_null() {
        *out_bounds = clip.bounds;
    }
    if !out_yoffsets.is_null() {
        *out_yoffsets = clip.yoffsets.as_ptr();
    }
    if !out_row_count.is_null() {
        *out_row_count = clip.yoffsets.len();
    }
    if !out_data.is_null() {
        *out_data = clip.data.as_ptr();
    }
    if !out_data_len.is_null() {
        *out_data_len = clip.data.len();
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_buf_free(buf: *mut AAClipBuf) {
    if !buf.is_null() {
        drop(Box::from_raw(buf));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_compute_row_size_for_width(width: i32) -> usize {
    aa_clip::compute_row_size_for_width(width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_validate(
    bounds: *const IRect,
    yoffsets: *const YOffset,
    row_count: usize,
    data: *const u8,
    data_len: usize,
) -> bool {
    let yoffs = slice_from_raw(yoffsets, row_count);
    let bytes = slice_from_raw(data, data_len);
    aa_clip::validate_aa_clip_parts(*bounds, yoffs, bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_is_rect(
    bounds: *const IRect,
    yoffsets: *const YOffset,
    row_count: usize,
    data: *const u8,
    data_len: usize,
) -> bool {
    let yoffs = slice_from_raw(yoffsets, row_count);
    let bytes = slice_from_raw(data, data_len);
    aa_clip::is_rect_parts(*bounds, yoffs, bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_find_row(
    bounds: *const IRect,
    yoffsets: *const YOffset,
    row_count: usize,
    y: i32,
    out_offset: *mut usize,
    out_last_y: *mut i32,
) -> bool {
    let yoffs = slice_from_raw(yoffsets, row_count);
    match aa_clip::find_row_offset(*bounds, yoffs, y) {
        Some((off, last_y)) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            if !out_last_y.is_null() {
                *out_last_y = last_y;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_find_x(
    bounds: *const IRect,
    row_data: *const u8,
    row_len: usize,
    x: i32,
    out_offset: *mut usize,
    out_initial_count: *mut i32,
) {
    let row = slice_from_raw(row_data, row_len);
    let (off, init_count) = aa_clip::find_x_offset(*bounds, row, x);
    if !out_offset.is_null() {
        *out_offset = off;
    }
    if !out_initial_count.is_null() {
        *out_initial_count = init_count;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_quick_contains(
    bounds: *const IRect,
    yoffsets: *const YOffset,
    row_count: usize,
    data: *const u8,
    data_len: usize,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> bool {
    let yoffs = slice_from_raw(yoffsets, row_count);
    let bytes = slice_from_raw(data, data_len);
    aa_clip::quick_contains_parts(*bounds, yoffs, bytes, left, top, right, bottom)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_trim_top_bottom(
    bounds: *mut IRect,
    yoffsets: *mut YOffset,
    row_count: *mut i32,
    data: *const u8,
    data_len: usize,
) -> bool {
    let count = *row_count;
    if count <= 0 {
        return false;
    }
    let yoffs = core::slice::from_raw_parts_mut(yoffsets, count as usize);
    let bytes = slice_from_raw(data, data_len);
    match aa_clip::trim_top_bottom_in_place(&mut *bounds, yoffs, bytes) {
        Some(new_count) => {
            *row_count = new_count as i32;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_trim_left_right(
    bounds: *mut IRect,
    yoffsets: *mut YOffset,
    row_count: usize,
    data: *mut u8,
    data_len: usize,
) -> bool {
    let yoffs = slice_from_raw_mut(yoffsets, row_count);
    let bytes = slice_from_raw_mut(data, data_len);
    aa_clip::trim_left_right_in_place(&mut *bounds, yoffs, bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_from_region_rects(
    bounds: *const IRect,
    rects: *const IRect,
    rect_count: usize,
) -> *mut AAClipBuf {
    let slice = slice_from_raw(rects, rect_count);
    let clip = aa_clip::build_from_region_rects(*bounds, slice.iter().copied());
    Box::into_raw(Box::new(AAClipBuf(clip)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_operate(
    target_bounds: *const IRect,
    a_bounds: *const IRect,
    a_yoff: *const YOffset,
    a_rows: usize,
    a_data: *const u8,
    a_data_len: usize,
    b_bounds: *const IRect,
    b_yoff: *const YOffset,
    b_rows: usize,
    b_data: *const u8,
    b_data_len: usize,
    op: i32,
) -> *mut AAClipBuf {
    let Some(clip_op) = ClipOp::from_i32(op) else {
        return Box::into_raw(Box::new(AAClipBuf(AAClip::new_empty())));
    };
    let mut builder = AAClipBuilder::new(*target_bounds);
    builder.operate_y(
        *a_bounds,
        slice_from_raw(a_yoff, a_rows),
        slice_from_raw(a_data, a_data_len),
        *b_bounds,
        slice_from_raw(b_yoff, b_rows),
        slice_from_raw(b_data, b_data_len),
        clip_op,
    );
    let result = builder.finish();
    Box::into_raw(Box::new(AAClipBuf(result)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_expand_to_mask(
    bounds: *const IRect,
    yoffsets: *const YOffset,
    row_count: usize,
    data: *const u8,
    data_len: usize,
    dst: *mut u8,
    row_bytes: usize,
) {
    if row_count == 0 || dst.is_null() {
        return;
    }
    let h = (*bounds).height().max(0) as usize;
    let total = row_bytes.saturating_mul(h);
    let out = core::slice::from_raw_parts_mut(dst, total);
    aa_clip::expand_clip_to_mask(
        *bounds,
        slice_from_raw(yoffsets, row_count),
        slice_from_raw(data, data_len),
        out,
        row_bytes,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_new(bounds: *const IRect) -> *mut AAClipBuilder {
    Box::into_raw(Box::new(AAClipBuilder::new(*bounds)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_free(b: *mut AAClipBuilder) {
    if !b.is_null() {
        drop(Box::from_raw(b));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_blit_h(
    b: *mut AAClipBuilder,
    x: i32,
    y: i32,
    width: i32,
) {
    (*b).blit_h(x, y, width);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_blit_v(
    b: *mut AAClipBuilder,
    x: i32,
    y: i32,
    height: i32,
    alpha: u8,
) {
    (*b).blit_v(x, y, height, alpha);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_blit_rect(
    b: *mut AAClipBuilder,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    (*b).blit_rect(x, y, width, height);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_blit_anti_rect(
    b: *mut AAClipBuilder,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    left_alpha: u8,
    right_alpha: u8,
) {
    (*b).blit_anti_rect(x, y, width, height, left_alpha, right_alpha);
}

/// Scans a 0-terminated `int16_t runs[]` array to determine its total element length including sentinel.
unsafe fn scan_runs_len(runs: *const i16) -> usize {
    let mut pos = 0usize;
    loop {
        let count = *runs.add(pos) as i32;
        if count <= 0 {
            return pos + 1;
        }
        pos += count as usize;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_blit_anti_h(
    b: *mut AAClipBuilder,
    x: i32,
    y: i32,
    alphas: *const u8,
    runs: *const i16,
) {
    let len = scan_runs_len(runs);
    let alpha_slice = core::slice::from_raw_parts(alphas, len);
    let runs_slice = core::slice::from_raw_parts(runs, len);
    (*b).blit_anti_h(x, y, alpha_slice, runs_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_builder_finish(b: *mut AAClipBuilder) -> *mut AAClipBuf {
    let mut builder = *Box::from_raw(b);
    builder.finish_blitter();
    let clip = builder.finish();
    Box::into_raw(Box::new(AAClipBuf(clip)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_expand_to_runs(
    row: *const u8,
    row_len: usize,
    initial_count: i32,
    width: i32,
    runs: *mut i16,
    aa: *mut u8,
) {
    if width <= 0 {
        if !runs.is_null() {
            *runs = 0;
        }
        return;
    }
    let w = width as usize;
    let row_slice = slice_from_raw(row, row_len);
    let runs_slice = core::slice::from_raw_parts_mut(runs, w + 1);
    let aa_slice = core::slice::from_raw_parts_mut(aa, w + 1);
    aa_clip::expand_to_runs(row_slice, initial_count, width, runs_slice, aa_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_merge_anti_h(
    row: *const u8,
    row_len: usize,
    row_n: i32,
    src_aa: *const u8,
    src_runs: *const i16,
    dst_aa: *mut u8,
    dst_runs: *mut i16,
    max_width: i32,
) {
    let src_len = scan_runs_len(src_runs);
    let dst_cap = (max_width.max(0) as usize) + 1;
    let row_slice = slice_from_raw(row, row_len);
    let src_aa_slice = core::slice::from_raw_parts(src_aa, src_len);
    let src_runs_slice = core::slice::from_raw_parts(src_runs, src_len);
    let dst_aa_slice = core::slice::from_raw_parts_mut(dst_aa, dst_cap);
    let dst_runs_slice = core::slice::from_raw_parts_mut(dst_runs, dst_cap);
    aa_clip::merge_anti_h(
        row_slice,
        row_n,
        src_aa_slice,
        src_runs_slice,
        dst_aa_slice,
        dst_runs_slice,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_merge_mask_row_u8(
    src: *const u8,
    width: i32,
    row: *const u8,
    row_len: usize,
    row_n: i32,
    dst: *mut u8,
) {
    if width <= 0 {
        return;
    }
    let w = width as usize;
    let src_slice = core::slice::from_raw_parts(src, w);
    let row_slice = slice_from_raw(row, row_len);
    let dst_slice = core::slice::from_raw_parts_mut(dst, w);
    aa_clip::merge_mask_row_u8(src_slice, row_slice, row_n, dst_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_merge_mask_row_u16(
    src: *const u16,
    width: i32,
    row: *const u8,
    row_len: usize,
    row_n: i32,
    dst: *mut u16,
) {
    if width <= 0 {
        return;
    }
    let w = width as usize;
    let src_slice = core::slice::from_raw_parts(src, w);
    let row_slice = slice_from_raw(row, row_len);
    let dst_slice = core::slice::from_raw_parts_mut(dst, w);
    aa_clip::merge_mask_row_u16(src_slice, row_slice, row_n, dst_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_aaclip_upscale_bw_to_a8(
    src: *const u8,
    src_rb: usize,
    dst: *mut u8,
    dst_rb: usize,
    width: i32,
    height: i32,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let src_len = src_rb.saturating_mul(h);
    let dst_len = dst_rb.saturating_mul(h);
    let src_slice = core::slice::from_raw_parts(src, src_len);
    let dst_slice = core::slice::from_raw_parts_mut(dst, dst_len);
    aa_clip::upscale_bw_to_a8(src_slice, src_rb, dst_slice, dst_rb, w, h);
}

// ===========================================================================
// SkRasterClip (`skia_rs_region::raster_clip`)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raster_clip_nearly_integral(x: f32) -> bool {
    raster_clip::nearly_integral(x)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_raster_clip_rect_is_nearly_integral(r: *const Rect) -> bool {
    raster_clip::rect_is_nearly_integral(*r)
}

// ===========================================================================
// SkClipStack & SkClipStackUtils (`skia_rs_region::clip_stack`)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_next_gen_id() -> u32 {
    clip_stack::get_next_gen_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_can_be_intersected_in_place(
    prior_type: i32,
    prior_save_count: i32,
    prior_op: i32,
    prior_is_replace: bool,
    save_count: i32,
    op: i32,
) -> bool {
    let Some(pt) = DeviceSpaceType::from_i32(prior_type) else {
        return false;
    };
    let Some(po) = ClipOp::from_i32(prior_op) else {
        return false;
    };
    let Some(co) = ClipOp::from_i32(op) else {
        return false;
    };
    clip_stack::can_be_intersected_in_place(pt, prior_save_count, po, prior_is_replace, save_count, co)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_rect_rect_intersect_allowed(
    existing: *const Rect,
    existing_aa: bool,
    new_rect: *const Rect,
    new_aa: bool,
) -> bool {
    clip_stack::rect_rect_intersect_allowed(*existing, existing_aa, *new_rect, new_aa)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_combine_bounds_diff(
    combination: i32,
    prev_finite: *const Rect,
    finite_bound: *mut Rect,
    finite_bound_type: *mut i32,
    gen_id: *mut u32,
) {
    let combo = match combination {
        0 => FillCombo::PrevCur,
        1 => FillCombo::PrevInvCur,
        2 => FillCombo::InvPrevCur,
        _ => FillCombo::InvPrevInvCur,
    };
    let mut bt = BoundsType::from_i32(*finite_bound_type).unwrap_or(BoundsType::Normal);
    clip_stack::combine_bounds_diff(combo, *prev_finite, &mut *finite_bound, &mut bt, &mut *gen_id);
    *finite_bound_type = bt as i32;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_combine_bounds_intersection(
    combination: i32,
    prev_finite: *const Rect,
    finite_bound: *mut Rect,
    finite_bound_type: *mut i32,
) -> bool {
    let combo = match combination {
        0 => FillCombo::PrevCur,
        1 => FillCombo::PrevInvCur,
        2 => FillCombo::InvPrevCur,
        _ => FillCombo::InvPrevInvCur,
    };
    let mut bt = BoundsType::from_i32(*finite_bound_type).unwrap_or(BoundsType::Normal);
    let became_empty =
        clip_stack::combine_bounds_intersection(combo, *prev_finite, &mut *finite_bound, &mut bt);
    *finite_bound_type = bt as i32;
    became_empty
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_compute_bound_and_gen_id(
    cur_type: i32,
    cur_bounds: *const Rect,
    cur_op: i32,
    cur_do_aa: bool,
    cur_is_replace: bool,
    cur_path_is_inverse: bool,
    prior: *const PriorBoundInfo,
    out: *mut BoundUpdateResult,
) {
    let ct = DeviceSpaceType::from_i32(cur_type).unwrap_or(DeviceSpaceType::Empty);
    let op = ClipOp::from_i32(cur_op).unwrap_or(ClipOp::Intersect);
    let prior_opt = if prior.is_null() { None } else { Some(*prior) };
    *out = clip_stack::compute_bound_and_gen_id(
        ct,
        *cur_bounds,
        op,
        cur_do_aa,
        cur_is_replace,
        cur_path_is_inverse,
        prior_opt,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_decide_push(
    has_prior: bool,
    prior_type: i32,
    prior_save_count: i32,
    prior_op: i32,
    prior_is_replace: bool,
    prior_do_aa: bool,
    prior_bounds: *const Rect,
    save_count: i32,
    new_type: i32,
    new_op: i32,
    new_is_replace: bool,
    new_do_aa: bool,
    new_bounds: *const Rect,
) -> PushDecision {
    let pt = DeviceSpaceType::from_i32(prior_type).unwrap_or(DeviceSpaceType::Empty);
    let po = ClipOp::from_i32(prior_op).unwrap_or(ClipOp::Intersect);
    let nt = DeviceSpaceType::from_i32(new_type).unwrap_or(DeviceSpaceType::Empty);
    let no = ClipOp::from_i32(new_op).unwrap_or(ClipOp::Intersect);
    let pb = if prior_bounds.is_null() {
        Rect::EMPTY
    } else {
        *prior_bounds
    };
    clip_stack::decide_push(
        has_prior,
        pt,
        prior_save_count,
        po,
        prior_is_replace,
        prior_do_aa,
        pb,
        save_count,
        nt,
        no,
        new_is_replace,
        new_do_aa,
        *new_bounds,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_compute_clipped_bounds(
    finite_bound: *const Rect,
    bound_type: i32,
    device_bounds: *const IRect,
) -> Rect {
    let bt = BoundsType::from_i32(bound_type).unwrap_or(BoundsType::Normal);
    clip_stack::compute_clipped_bounds(*finite_bound, bt, *device_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_compute_conservative_bounds(
    finite_bound: *const Rect,
    bound_type: i32,
    offset_x: i32,
    offset_y: i32,
    max_width: i32,
    max_height: i32,
) -> Rect {
    let bt = BoundsType::from_i32(bound_type).unwrap_or(BoundsType::Normal);
    clip_stack::compute_conservative_bounds(
        *finite_bound,
        bt,
        offset_x,
        offset_y,
        max_width,
        max_height,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_compute_topmost_gen_id(
    is_empty_deque: bool,
    back_type: i32,
    back_bound_type: i32,
    back_finite_bound: *const Rect,
    back_gen_id: u32,
) -> u32 {
    let bt = DeviceSpaceType::from_i32(back_type).unwrap_or(DeviceSpaceType::Empty);
    let bbt = BoundsType::from_i32(back_bound_type).unwrap_or(BoundsType::Normal);
    let fb = if back_finite_bound.is_null() {
        Rect::EMPTY
    } else {
        *back_finite_bound
    };
    clip_stack::compute_topmost_gen_id(is_empty_deque, bt, bbt, fb, back_gen_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_clip_stack_classify_as_path_step(
    dev_type: i32,
    is_replace: bool,
) -> i32 {
    let dt = DeviceSpaceType::from_i32(dev_type).unwrap_or(DeviceSpaceType::Empty);
    clip_stack::classify_as_path_step(dt, is_replace) as i32
}

// ===========================================================================
// SkRTree (`skia_rs_region::rtree`)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rtree_count_nodes(branches: i32) -> i32 {
    if branches <= 0 {
        return 0;
    }
    rtree::count_nodes(branches as usize) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rtree_build(bounds: *const Rect, n: i32) -> *mut RTree {
    let slice = if n <= 0 || bounds.is_null() {
        &[]
    } else {
        core::slice::from_raw_parts(bounds, n as usize)
    };
    Box::into_raw(Box::new(RTree::build(slice)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rtree_info(
    tree: *const RTree,
    out_count: *mut i32,
    out_root_bounds: *mut Rect,
    out_root_node: *mut u32,
    out_nodes: *mut *const RTreeNode,
    out_nodes_len: *mut usize,
) {
    let t = &*tree;
    if !out_count.is_null() {
        *out_count = t.count as i32;
    }
    if !out_root_bounds.is_null() {
        *out_root_bounds = t.root_bounds;
    }
    if !out_root_node.is_null() {
        *out_root_node = t.root_node;
    }
    if !out_nodes.is_null() {
        *out_nodes = t.nodes.as_ptr();
    }
    if !out_nodes_len.is_null() {
        *out_nodes_len = t.nodes.len();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rtree_free(tree: *mut RTree) {
    if !tree.is_null() {
        drop(Box::from_raw(tree));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rtree_search_with(
    root_bounds: *const Rect,
    root_node: usize,
    count: usize,
    query: *const Rect,
    read_node: extern "C" fn(usize, *mut u16, *mut u16, *mut RTreeBranch, *mut c_void),
    on_hit: extern "C" fn(i32, *mut c_void),
    ctx: *mut c_void,
) {
    rtree::search_with(
        *root_bounds,
        root_node,
        count,
        *query,
        &|idx| {
            let mut level = 0u16;
            let mut num_children = 0u16;
            let mut children = [RTreeBranch::default(); MAX_CHILDREN];
            read_node(
                idx,
                &mut level as *mut u16,
                &mut num_children as *mut u16,
                children.as_mut_ptr(),
                ctx,
            );
            (level, num_children, children)
        },
        &mut |op_idx| {
            on_hit(op_idx, ctx);
        },
    );
}
