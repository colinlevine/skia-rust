//! FFI exports for `skia-rs-picture` (Phase 1 Layer 5.2).
//!
//! Covers:
//! - `SkLatticeIter`
//! - `SkDrawable`, `SkRecordedDrawable`, `SkAnnotation`
//! - `SkPtrSet` (`SkPtrRecorder`)
//! - `SkRecord`, `SkRecords`, `SkRecordCanvas`, `SkRecordDraw`, `SkRecordOpts`
//! - `SkPicture`, `SkPictureData`, `SkPictureFlat`, `SkPicturePlayback`,
//!   `SkPictureRecord`, `SkPictureRecorder`, `SkPictureImageGenerator`
//! - `SkMultiPictureDocument`

use std::ffi::{c_char, c_void, CStr};

use skia_rs_geometry::{IRect, M44, Matrix, Point, Rect, Size};
use skia_rs_picture::{
    drawable,
    lattice::{self, LatticeNextOut, LatticePlan},
    multi_picture_document as mpd,
    picture_format::{
        self, ClipOpAction, DrawAtlasFlatLayout, DrawPatchFlatLayout, OptionalPaintResult,
        PictInfo, SaveLayerFlatLayout, UnpackedClipRegionOp, UnpackedOpHeader,
    },
    ptr_recorder::{self, PtrInsertResult, PtrPair},
    record::{self, RecordSlot},
    record_draw::{self, EdgeAAImageSetEntryInfo, FillBoundsState},
    record_opts::{self, LayerFoldDecision, PaintOptInfo},
};

// ============================================================================
// SkLatticeIter
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_valid(
    width: i32,
    height: i32,
    bounds: *const IRect,
    x_divs: *const i32,
    x_count: i32,
    y_divs: *const i32,
    y_count: i32,
) -> bool {
    if bounds.is_null() {
        return false;
    }
    let xs = if x_count > 0 && !x_divs.is_null() {
        std::slice::from_raw_parts(x_divs, x_count as usize)
    } else {
        &[]
    };
    let ys = if y_count > 0 && !y_divs.is_null() {
        std::slice::from_raw_parts(y_divs, y_count as usize)
    } else {
        &[]
    };
    lattice::valid_lattice(width, height, *bounds, xs, ys)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_valid_nine(
    width: i32,
    height: i32,
    center: *const IRect,
) -> bool {
    if center.is_null() {
        return false;
    }
    lattice::valid_nine(width, height, *center)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_plan_create(
    bounds: *const IRect,
    x_divs: *const i32,
    x_count: i32,
    y_divs: *const i32,
    y_count: i32,
    rect_types: *const u8,
    colors: *const u32,
    dst: *const Rect,
) -> *mut LatticePlan {
    if bounds.is_null() || dst.is_null() {
        return std::ptr::null_mut();
    }
    let xs = if x_count > 0 && !x_divs.is_null() {
        std::slice::from_raw_parts(x_divs, x_count as usize)
    } else {
        &[]
    };
    let ys = if y_count > 0 && !y_divs.is_null() {
        std::slice::from_raw_parts(y_divs, y_count as usize)
    } else {
        &[]
    };
    let orig_cells = (xs.len() + 1) * (ys.len() + 1);
    let rt_opt = if !rect_types.is_null() {
        Some(std::slice::from_raw_parts(rect_types, orig_cells))
    } else {
        None
    };
    let col_opt = if !colors.is_null() {
        Some(std::slice::from_raw_parts(colors, orig_cells))
    } else {
        None
    };
    let plan = LatticePlan::from_lattice(*bounds, xs, ys, rt_opt, col_opt, *dst);
    Box::into_raw(Box::new(plan))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_plan_create_nine(
    w: i32,
    h: i32,
    center: *const IRect,
    dst: *const Rect,
) -> *mut LatticePlan {
    if center.is_null() || dst.is_null() {
        return std::ptr::null_mut();
    }
    let plan = LatticePlan::from_nine(w, h, *center, *dst);
    Box::into_raw(Box::new(plan))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_plan_info(
    plan: *const LatticePlan,
    out_x_len: *mut usize,
    out_y_len: *mut usize,
    out_rt_len: *mut usize,
    out_num_rects: *mut i32,
    out_num_to_draw: *mut i32,
) {
    if plan.is_null() {
        return;
    }
    let p = &*plan;
    if !out_x_len.is_null() {
        *out_x_len = p.src_x.len();
    }
    if !out_y_len.is_null() {
        *out_y_len = p.src_y.len();
    }
    if !out_rt_len.is_null() {
        *out_rt_len = p.rect_types.len();
    }
    if !out_num_rects.is_null() {
        *out_num_rects = p.num_rects_in_lattice;
    }
    if !out_num_to_draw.is_null() {
        *out_num_to_draw = p.num_rects_to_draw;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_plan_copy(
    plan: *const LatticePlan,
    out_src_x: *mut i32,
    out_dst_x: *mut f32,
    out_src_y: *mut i32,
    out_dst_y: *mut f32,
    out_rect_types: *mut u8,
    out_colors: *mut u32,
) {
    if plan.is_null() {
        return;
    }
    let p = &*plan;
    if !out_src_x.is_null() && !p.src_x.is_empty() {
        std::ptr::copy_nonoverlapping(p.src_x.as_ptr(), out_src_x, p.src_x.len());
    }
    if !out_dst_x.is_null() && !p.dst_x.is_empty() {
        std::ptr::copy_nonoverlapping(p.dst_x.as_ptr(), out_dst_x, p.dst_x.len());
    }
    if !out_src_y.is_null() && !p.src_y.is_empty() {
        std::ptr::copy_nonoverlapping(p.src_y.as_ptr(), out_src_y, p.src_y.len());
    }
    if !out_dst_y.is_null() && !p.dst_y.is_empty() {
        std::ptr::copy_nonoverlapping(p.dst_y.as_ptr(), out_dst_y, p.dst_y.len());
    }
    if !out_rect_types.is_null() && !p.rect_types.is_empty() {
        std::ptr::copy_nonoverlapping(p.rect_types.as_ptr(), out_rect_types, p.rect_types.len());
    }
    if !out_colors.is_null() && !p.colors.is_empty() {
        std::ptr::copy_nonoverlapping(p.colors.as_ptr(), out_colors, p.colors.len());
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_plan_destroy(plan: *mut LatticePlan) {
    if !plan.is_null() {
        drop(Box::from_raw(plan));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_next(
    src_x: *const i32,
    src_x_len: usize,
    src_y: *const i32,
    src_y_len: usize,
    dst_x: *const f32,
    dst_y: *const f32,
    rect_types: *const u8,
    rect_types_len: usize,
    colors: *const u32,
    num_rects_in_lattice: i32,
    curr_x: *mut i32,
    curr_y: *mut i32,
    out: *mut LatticeNextOut,
) -> bool {
    if src_x.is_null()
        || src_y.is_null()
        || dst_x.is_null()
        || dst_y.is_null()
        || curr_x.is_null()
        || curr_y.is_null()
        || out.is_null()
    {
        return false;
    }
    let sx = std::slice::from_raw_parts(src_x, src_x_len);
    let sy = std::slice::from_raw_parts(src_y, src_y_len);
    let dx = std::slice::from_raw_parts(dst_x, src_x_len);
    let dy = std::slice::from_raw_parts(dst_y, src_y_len);
    let rt = if rect_types_len > 0 && !rect_types.is_null() {
        std::slice::from_raw_parts(rect_types, rect_types_len)
    } else {
        &[]
    };
    let cols = if rect_types_len > 0 && !colors.is_null() {
        std::slice::from_raw_parts(colors, rect_types_len)
    } else {
        &[]
    };
    match lattice::lattice_next(
        sx,
        sy,
        dx,
        dy,
        rt,
        cols,
        num_rects_in_lattice,
        &mut *curr_x,
        &mut *curr_y,
    ) {
        Some(res) => {
            *out = res;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_lattice_map_dst_scale_translate(
    dst_x: *mut f32,
    dst_x_len: usize,
    dst_y: *mut f32,
    dst_y_len: usize,
    matrix: *const Matrix,
) {
    if matrix.is_null() {
        return;
    }
    let dx = if dst_x_len > 0 && !dst_x.is_null() {
        std::slice::from_raw_parts_mut(dst_x, dst_x_len)
    } else {
        &mut []
    };
    let dy = if dst_y_len > 0 && !dst_y.is_null() {
        std::slice::from_raw_parts_mut(dst_y, dst_y_len)
    } else {
        &mut []
    };
    lattice::map_dst_scale_translate(dx, dy, &*matrix);
}

// ============================================================================
// SkDrawable, SkRecordedDrawable, SkAnnotation
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_drawable_next_generation_id() -> u32 {
    drawable::next_drawable_generation_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_annotation_key_url() -> *const c_char {
    drawable::ANNOTATION_KEY_URL.as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_annotation_key_define_named_dest() -> *const c_char {
    drawable::ANNOTATION_KEY_DEFINE_NAMED_DEST.as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_annotation_key_link_named_dest() -> *const c_char {
    drawable::ANNOTATION_KEY_LINK_NAMED_DEST.as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_annotate_named_dest_rect(point: Point) -> Rect {
    drawable::annotate_named_dest_rect(point)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_recorded_drawable_approx_bytes(
    self_size: usize,
    record_bytes: usize,
    bbh_bytes: usize,
    drawables_bytes: usize,
) -> usize {
    drawable::recorded_drawable_approx_bytes(self_size, record_bytes, bbh_bytes, drawables_bytes)
}

// ============================================================================
// SkPtrSet (SkPtrRecorder)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ptr_set_less(a: PtrPair, b: PtrPair) -> bool {
    ptr_recorder::ptr_pair_less(a, b)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ptr_set_find(
    pairs: *const PtrPair,
    count: usize,
    target: usize,
) -> u32 {
    let slice = if count > 0 && !pairs.is_null() {
        std::slice::from_raw_parts(pairs, count)
    } else {
        &[]
    };
    ptr_recorder::find_ptr(slice, target)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ptr_set_find_or_insert(
    pairs: *const PtrPair,
    count: usize,
    target: usize,
) -> PtrInsertResult {
    let slice = if count > 0 && !pairs.is_null() {
        std::slice::from_raw_parts(pairs, count)
    } else {
        &[]
    };
    ptr_recorder::find_or_insert_pos(slice, target)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_ptr_set_copy_to_array(
    pairs: *const PtrPair,
    count: usize,
    out: *mut usize,
) {
    if count == 0 || pairs.is_null() || out.is_null() {
        return;
    }
    let slice = std::slice::from_raw_parts(pairs, count);
    let dst = std::slice::from_raw_parts_mut(out, count);
    ptr_recorder::copy_to_array(slice, dst);
}

// ============================================================================
// SkRecord & SkRecordCanvas
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_next_capacity(reserved: i32) -> i32 {
    record::record_next_capacity(reserved)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_bytes_used(
    approx_bytes_allocated: usize,
    sizeof_record: usize,
) -> usize {
    record::record_bytes_used(approx_bytes_allocated, sizeof_record)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_defrag(slots: *mut RecordSlot, count: usize) -> usize {
    if count == 0 || slots.is_null() {
        return 0;
    }
    let slice = std::slice::from_raw_parts_mut(slots, count);
    record::record_defrag(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_safe_picture_bounds(bounds: Rect) -> IRect {
    record::safe_picture_bounds(bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_lattice_flag_count(
    has_rect_types: bool,
    x_count: i32,
    y_count: i32,
) -> i32 {
    record::lattice_flag_count(has_rect_types, x_count, y_count)
}

// ============================================================================
// SkRecordOpts
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_opts_noop_save_restores(
    op_types: *mut u32,
    noop_mask: *mut bool,
    count: usize,
) {
    if count == 0 || op_types.is_null() || noop_mask.is_null() {
        return;
    }
    let ops = std::slice::from_raw_parts_mut(op_types, count);
    let mask = std::slice::from_raw_parts_mut(noop_mask, count);
    record_opts::run_noop_save_restores(ops, mask);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_opts_find_save_layer_draw_restore(
    op_types: *const u32,
    count: usize,
    out_begins: *mut i32,
    out_cap: usize,
) -> usize {
    if count == 0 || op_types.is_null() {
        return 0;
    }
    let ops = std::slice::from_raw_parts(op_types, count);
    let matches = record_opts::find_save_layer_draw_restore_matches(ops);
    if !out_begins.is_null() && out_cap > 0 {
        let n = matches.len().min(out_cap);
        std::ptr::copy_nonoverlapping(matches.as_ptr(), out_begins, n);
    }
    matches.len()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_opts_eval_save_layer_draw_restore(
    has_backdrop: bool,
    has_filters: bool,
    layer_paint: *const PaintOptInfo,
    draw_paint: *const PaintOptInfo,
) -> LayerFoldDecision {
    let lp = if layer_paint.is_null() {
        PaintOptInfo::default()
    } else {
        *layer_paint
    };
    let dp = if draw_paint.is_null() {
        PaintOptInfo::default()
    } else {
        *draw_paint
    };
    record_opts::eval_save_layer_draw_restore(has_backdrop, has_filters, &lp, &dp)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_opts_find_svg_opacity_filter(
    op_types: *const u32,
    count: usize,
    out_begins: *mut i32,
    out_cap: usize,
) -> usize {
    if count == 0 || op_types.is_null() {
        return 0;
    }
    let ops = std::slice::from_raw_parts(op_types, count);
    let matches = record_opts::find_svg_opacity_filter_matches(ops);
    if !out_begins.is_null() && out_cap > 0 {
        let n = matches.len().min(out_cap);
        std::ptr::copy_nonoverlapping(matches.as_ptr(), out_begins, n);
    }
    matches.len()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_record_opts_eval_svg_opacity_filter(
    outer_has_backdrop: bool,
    outer_has_filters: bool,
    inner_has_filters: bool,
    opacity_paint: *const PaintOptInfo,
    filter_layer_paint: *const PaintOptInfo,
) -> LayerFoldDecision {
    let op = if opacity_paint.is_null() {
        PaintOptInfo::default()
    } else {
        *opacity_paint
    };
    let fp = if filter_layer_paint.is_null() {
        PaintOptInfo::default()
    } else {
        *filter_layer_paint
    };
    record_opts::eval_svg_opacity_filter_merge(
        outer_has_backdrop,
        outer_has_filters,
        inner_has_filters,
        &op,
        &fp,
    )
}

// ============================================================================
// SkRecordDraw / FillBounds
// ============================================================================

pub type AdjustPaintCb = unsafe extern "C" fn(ctx: *mut c_void, paint_token: usize, rect: *mut Rect) -> bool;

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_paint_may_affect_transparent_black(
    has_paint: bool,
    if_affects: bool,
    cf_affects: bool,
    blend_mode: i32,
) -> bool {
    record_draw::paint_may_affect_transparent_black(has_paint, if_affects, cf_affects, blend_mode)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_create(
    cull_rect: Rect,
    count: usize,
) -> *mut FillBoundsState {
    Box::into_raw(Box::new(FillBoundsState::new(cull_rect, count)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_set_current_op(
    fb: *mut FillBoundsState,
    op: usize,
) {
    if !fb.is_null() {
        (*fb).set_current_op(op);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_get_ctm(
    fb: *const FillBoundsState,
    out_ctm: *mut Matrix,
) {
    if !fb.is_null() && !out_ctm.is_null() {
        *out_ctm = *(*fb).ctm();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_update_ctm_set_matrix(
    fb: *mut FillBoundsState,
    m: *const Matrix,
) {
    if !fb.is_null() && !m.is_null() {
        (*fb).update_ctm_set_matrix(&*m);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_update_ctm_set_m44(
    fb: *mut FillBoundsState,
    m: *const M44,
) {
    if !fb.is_null() && !m.is_null() {
        (*fb).update_ctm_set_m44(&*m);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_update_ctm_concat(
    fb: *mut FillBoundsState,
    m: *const Matrix,
) {
    if !fb.is_null() && !m.is_null() {
        (*fb).update_ctm_concat(&*m);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_update_ctm_concat44(
    fb: *mut FillBoundsState,
    m: *const M44,
) {
    if !fb.is_null() && !m.is_null() {
        (*fb).update_ctm_concat44(&*m);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_update_ctm_scale(
    fb: *mut FillBoundsState,
    sx: f32,
    sy: f32,
) {
    if !fb.is_null() {
        (*fb).update_ctm_scale(sx, sy);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_update_ctm_translate(
    fb: *mut FillBoundsState,
    dx: f32,
    dy: f32,
) {
    if !fb.is_null() {
        (*fb).update_ctm_translate(dx, dy);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_push_save_block(
    fb: *mut FillBoundsState,
    paint_token: usize,
    has_backdrop: bool,
    paint_affects_transparent_black: bool,
) {
    if !fb.is_null() {
        (*fb).push_save_block(paint_token, has_backdrop, paint_affects_transparent_black);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_track_restore(
    fb: *mut FillBoundsState,
    restored_ctm: *const Matrix,
) {
    if !fb.is_null() && !restored_ctm.is_null() {
        (*fb).track_restore(&*restored_ctm);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_push_control(fb: *mut FillBoundsState) {
    if !fb.is_null() {
        (*fb).push_control();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_cull_rect_draw(fb: *mut FillBoundsState) {
    if !fb.is_null() {
        (*fb).record_cull_rect_draw();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_noop(fb: *mut FillBoundsState) {
    if !fb.is_null() {
        (*fb).record_noop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_rect_draw(
    fb: *mut FillBoundsState,
    local_rect: Rect,
    paint_token: usize,
    adjust_fn: Option<AdjustPaintCb>,
    ctx: *mut c_void,
) {
    if fb.is_null() {
        return;
    }
    (*fb).record_rect_draw(local_rect, paint_token, |tok, rect| {
        if let Some(f) = adjust_fn {
            f(ctx, tok, rect as *mut Rect)
        } else {
            true
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_points_draw(
    fb: *mut FillBoundsState,
    pts: *const Point,
    count: usize,
    stroke_width: f32,
    paint_token: usize,
    adjust_fn: Option<AdjustPaintCb>,
    ctx: *mut c_void,
) {
    if fb.is_null() {
        return;
    }
    let slice = if count > 0 && !pts.is_null() {
        std::slice::from_raw_parts(pts, count)
    } else {
        &[]
    };
    (*fb).record_points_draw(slice, stroke_width, paint_token, |tok, rect| {
        if let Some(f) = adjust_fn {
            f(ctx, tok, rect as *mut Rect)
        } else {
            true
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_picture_draw(
    fb: *mut FillBoundsState,
    pic_cull: Rect,
    pic_matrix: *const Matrix,
    paint_token: usize,
    adjust_fn: Option<AdjustPaintCb>,
    ctx: *mut c_void,
) {
    if fb.is_null() || pic_matrix.is_null() {
        return;
    }
    (*fb).record_picture_draw(pic_cull, &*pic_matrix, paint_token, |tok, rect| {
        if let Some(f) = adjust_fn {
            f(ctx, tok, rect as *mut Rect)
        } else {
            true
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_text_blob_draw(
    fb: *mut FillBoundsState,
    blob_bounds: Rect,
    x: f32,
    y: f32,
    paint_token: usize,
    adjust_fn: Option<AdjustPaintCb>,
    ctx: *mut c_void,
) {
    if fb.is_null() {
        return;
    }
    (*fb).record_text_blob_draw(blob_bounds, x, y, paint_token, |tok, rect| {
        if let Some(f) = adjust_fn {
            f(ctx, tok, rect as *mut Rect)
        } else {
            true
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_edge_aa_quad_draw(
    fb: *mut FillBoundsState,
    rect: Rect,
    clip_quad: *const Point,
    adjust_fn: Option<AdjustPaintCb>,
    ctx: *mut c_void,
) {
    if fb.is_null() {
        return;
    }
    let quad_opt = if !clip_quad.is_null() {
        let s = std::slice::from_raw_parts(clip_quad, 4);
        Some([s[0], s[1], s[2], s[3]])
    } else {
        None
    };
    (*fb).record_edge_aa_quad_draw(rect, quad_opt.as_ref(), |tok, r| {
        if let Some(f) = adjust_fn {
            f(ctx, tok, r as *mut Rect)
        } else {
            true
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_record_edge_aa_image_set_draw(
    fb: *mut FillBoundsState,
    entries: *const EdgeAAImageSetEntryInfo,
    entry_count: usize,
    dst_clips: *const Point,
    clip_count: usize,
    pre_view_matrices: *const Matrix,
    matrix_count: usize,
    adjust_fn: Option<AdjustPaintCb>,
    ctx: *mut c_void,
) {
    if fb.is_null() {
        return;
    }
    let e = if entry_count > 0 && !entries.is_null() {
        std::slice::from_raw_parts(entries, entry_count)
    } else {
        &[]
    };
    let c = if clip_count > 0 && !dst_clips.is_null() {
        std::slice::from_raw_parts(dst_clips, clip_count)
    } else {
        &[]
    };
    let m = if matrix_count > 0 && !pre_view_matrices.is_null() {
        std::slice::from_raw_parts(pre_view_matrices, matrix_count)
    } else {
        &[]
    };
    (*fb).record_edge_aa_image_set_draw(e, c, m, |tok, r| {
        if let Some(f) = adjust_fn {
            f(ctx, tok, r as *mut Rect)
        } else {
            true
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_fill_bounds_finish_and_destroy(
    fb: *mut FillBoundsState,
    out_bounds: *mut Rect,
    out_is_draw: *mut bool,
    count: usize,
) {
    if fb.is_null() {
        return;
    }
    let state = Box::from_raw(fb);
    let (bounds, is_draw) = state.finish();
    let n = count.min(bounds.len());
    if !out_bounds.is_null() && n > 0 {
        std::ptr::copy_nonoverlapping(bounds.as_ptr(), out_bounds, n);
    }
    if !out_is_draw.is_null() && n > 0 {
        std::ptr::copy_nonoverlapping(is_draw.as_ptr(), out_is_draw, n);
    }
}

// ============================================================================
// SkPicture, SkPictureData, SkPictureRecord, SkPicturePlayback,
// SkPictureRecorder, SkPictureImageGenerator
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_next_id() -> u32 {
    picture_format::next_picture_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_create_header(cull_rect: Rect) -> PictInfo {
    picture_format::create_header(cull_rect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_is_valid_pict_info(info: *const PictInfo) -> bool {
    if info.is_null() {
        return false;
    }
    picture_format::is_valid_pict_info(&*info)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_encode_pict_info(
    info: *const PictInfo,
    out_bytes: *mut u8,
) {
    if info.is_null() || out_bytes.is_null() {
        return;
    }
    let encoded = picture_format::encode_pict_info(&*info);
    std::ptr::copy_nonoverlapping(encoded.as_ptr(), out_bytes, encoded.len());
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_decode_pict_info(
    bytes: *const u8,
    out_info: *mut PictInfo,
) -> bool {
    if bytes.is_null() || out_info.is_null() {
        return false;
    }
    let mut arr = [0u8; 28];
    std::ptr::copy_nonoverlapping(bytes, arr.as_mut_ptr(), 28);
    let info = picture_format::decode_pict_info(&arr);
    *out_info = info;
    picture_format::is_valid_pict_info(&info)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_negate_to_size_t(val: i32) -> usize {
    picture_format::negate_to_size_t(val)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_custom_serialize_size(size: usize) -> bool {
    picture_format::validate_custom_serialize_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_pad32_bytes(size: usize) -> usize {
    picture_format::pad32_bytes(size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_should_use_bbh(
    local_clip: Rect,
    cull_rect: Rect,
) -> bool {
    picture_format::should_use_bbh(local_clip, cull_rect)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_approx_bytes_used(
    is_placeholder: bool,
    self_size: usize,
    record_bytes: usize,
    sub_pic_bytes: usize,
    bbh_bytes: usize,
) -> usize {
    picture_format::picture_approx_bytes_used(
        is_placeholder,
        self_size,
        record_bytes,
        sub_pic_bytes,
        bbh_bytes,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_normalize_recording_cull_rect(user_cull: Rect) -> Rect {
    picture_format::normalize_recording_cull_rect(user_cull)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_compute_bbh_union(
    bounds: *const Rect,
    count: usize,
) -> Rect {
    let slice = if count > 0 && !bounds.is_null() {
        std::slice::from_raw_parts(bounds, count)
    } else {
        &[]
    };
    picture_format::compute_bbh_union(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_compute_factory_chunk_size(
    name_lens: *const usize,
    count: usize,
) -> usize {
    let slice = if count > 0 && !name_lens.is_null() {
        std::slice::from_raw_parts(name_lens, count)
    } else {
        &[]
    };
    picture_format::compute_factory_chunk_size(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_optional_paint_index(
    index: i32,
    table_len: i32,
) -> OptionalPaintResult {
    picture_format::validate_optional_paint_index(index, table_len)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_unpack_op_word(bits: u32) -> UnpackedOpHeader {
    picture_format::unpack_op_word(bits)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_op_and_size(op: u32, size: u32) -> bool {
    picture_format::validate_op_and_size(op, size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_offset_to_restore(
    offset_to_restore: usize,
    reader_offset: usize,
) -> bool {
    picture_format::validate_offset_to_restore(offset_to_restore, reader_offset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_clip_params_pack(op: u32, do_aa: bool) -> u32 {
    picture_format::clip_params_pack(op, do_aa)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_clip_params_unpack_do_aa(packed: u32) -> bool {
    picture_format::clip_params_unpack_do_aa(packed)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_clip_params_unpack_region_op(
    packed: u32,
    version: u32,
) -> UnpackedClipRegionOp {
    picture_format::clip_params_unpack_region_op(packed, version)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_map_region_op_to_clip_action(
    region_op: u32,
) -> ClipOpAction {
    picture_format::map_region_op_to_clip_action(region_op)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_compute_save_layer_flat_layout(
    has_bounds: bool,
    has_paint: bool,
    has_backdrop: bool,
    save_layer_flags: u32,
    backdrop_scale: f32,
    filter_count: u32,
    backdrop_tile_mode_is_clamp: bool,
) -> SaveLayerFlatLayout {
    picture_format::compute_save_layer_flat_layout(
        has_bounds,
        has_paint,
        has_backdrop,
        save_layer_flags,
        backdrop_scale,
        filter_count,
        backdrop_tile_mode_is_clamp,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_compute_draw_patch_flat_layout(
    has_colors: bool,
    has_tex_coords: bool,
    is_modulate_blend: bool,
) -> DrawPatchFlatLayout {
    picture_format::compute_draw_patch_flat_layout(has_colors, has_tex_coords, is_modulate_blend)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_compute_draw_atlas_flat_layout(
    count: usize,
    has_colors: bool,
    has_cull: bool,
    sampling_flat_size: usize,
) -> DrawAtlasFlatLayout {
    picture_format::compute_draw_atlas_flat_layout(count, has_colors, has_cull, sampling_flat_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_edge_aa_entries_avail(
    cnt: i32,
    available: usize,
) -> bool {
    picture_format::validate_edge_aa_entries_avail(cnt, available)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_edge_aa_matrices_avail(
    matrix_count: i32,
    max_matrix_index: i32,
    available: usize,
) -> bool {
    picture_format::validate_edge_aa_matrices_avail(matrix_count, max_matrix_index, available)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_validate_save_layer_filter_count(
    filter_count: i32,
) -> bool {
    picture_format::validate_save_layer_filter_count(filter_count)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_find_id_linear(
    ids: *const u64,
    count: usize,
    target: u64,
) -> i32 {
    if count == 0 || ids.is_null() {
        return -1;
    }
    let slice = std::slice::from_raw_parts(ids, count);
    match picture_format::find_id_linear(slice, target) {
        Some(idx) => idx as i32,
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_picture_image_gen_validate(
    has_picture: bool,
    has_color_space: bool,
    width: i32,
    height: i32,
    is_f16_bit_depth: bool,
) -> i32 {
    picture_format::validate_picture_image_generator(
        has_picture,
        has_color_space,
        width,
        height,
        is_f16_bit_depth,
    )
}

// ============================================================================
// SkMultiPictureDocument
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_join_page_sizes(sizes: *const Size, count: usize) -> Size {
    let slice = if count > 0 && !sizes.is_null() {
        std::slice::from_raw_parts(sizes, count)
    } else {
        &[]
    };
    mpd::join_page_sizes(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_encode_header(
    sizes: *const Size,
    count: usize,
    out_buf: *mut u8,
    out_cap: usize,
) -> usize {
    let slice = if count > 0 && !sizes.is_null() {
        std::slice::from_raw_parts(sizes, count)
    } else {
        &[]
    };
    let encoded = mpd::encode_mpd_header(slice);
    if !out_buf.is_null() && out_cap >= encoded.len() {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), out_buf, encoded.len());
    }
    encoded.len()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_parse_header_prefix(header: *const u8, len: usize) -> i32 {
    if header.is_null() {
        return -1;
    }
    let slice = std::slice::from_raw_parts(header, len);
    mpd::parse_mpd_header_prefix(slice).unwrap_or(-1)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_validate_page_count_and_length(
    page_count: i32,
    dst_array_count: i32,
    has_length: bool,
    stream_length: usize,
) -> bool {
    mpd::validate_mpd_page_count_and_length(page_count, dst_array_count, has_length, stream_length)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_is_valid_page_size(size: Size) -> bool {
    mpd::is_valid_mpd_page_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_end_page_key() -> *const c_char {
    mpd::MPD_END_PAGE_CSTR.as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mpd_is_end_page_annotation(key: *const c_char) -> bool {
    if key.is_null() {
        return false;
    }
    let cstr = CStr::from_ptr(key);
    mpd::is_end_page_annotation(cstr.to_bytes())
}
