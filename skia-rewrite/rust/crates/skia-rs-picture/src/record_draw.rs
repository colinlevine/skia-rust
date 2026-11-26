//! Safe Rust implementation of `SkRecords::FillBounds` (`src/core/SkRecordDraw.cpp`).

use skia_rs_geometry::{cpp_max_f32, M44, Matrix, Point, Rect};
use skia_rs_path::builder::PathBuilder;
use skia_rs_path::{Direction, FillType};

/// Maps a rect through a 3x3 `Matrix`, matching `SkMatrix::mapRect` bit-for-bit
/// (including perspective matrices via `PathBuilder::add_rect` + `transform` + `compute_bounds`).
pub fn matrix_map_rect(m: &Matrix, src: Rect) -> Rect {
    if let Some((dst, _stays_rect)) = m.map_rect_non_perspective(src) {
        return dst;
    }
    let mut builder = PathBuilder::new(FillType::Winding);
    builder.add_rect(src, Direction::Cw, 0);
    builder.transform(m);
    builder.compute_bounds()
}

/// Converts a 4x4 `M44` to 3x3 `Matrix` (matching `SkM44::asM33()`).
#[inline]
pub fn m44_as_m33(m: &M44) -> Matrix {
    Matrix::make_all(
        m.mat[0], m.mat[4], m.mat[12],
        m.mat[1], m.mat[5], m.mat[13],
        m.mat[3], m.mat[7], m.mat[15],
    )
}

/// Matches `SkBlendMode` numeric values used in `PaintMayAffectTransparentBlack`.
pub const BLEND_CLEAR: i32 = 0;
pub const BLEND_SRC: i32 = 1;
pub const BLEND_SRC_IN: i32 = 5;
pub const BLEND_DST_IN: i32 = 6;
pub const BLEND_SRC_OUT: i32 = 7;
pub const BLEND_DST_ATOP: i32 = 10;
pub const BLEND_MODULATE: i32 = 13;

/// Matches `FillBounds::PaintMayAffectTransparentBlack(const SkPaint* paint)`.
/// `blend_mode` is `< 0` when `paint->asBlendMode()` is empty (custom blender).
pub const fn paint_may_affect_transparent_black(
    has_paint: bool,
    image_filter_affects_transparent_black: bool,
    color_filter_affects_transparent_black: bool,
    blend_mode: i32,
) -> bool {
    if !has_paint {
        return false;
    }
    if image_filter_affects_transparent_black || color_filter_affects_transparent_black {
        return true;
    }
    if blend_mode < 0 {
        return true;
    }
    matches!(
        blend_mode,
        BLEND_CLEAR
            | BLEND_SRC
            | BLEND_SRC_IN
            | BLEND_DST_IN
            | BLEND_SRC_OUT
            | BLEND_DST_ATOP
            | BLEND_MODULATE
    )
}

/// Extracted fields from `SkCanvas::ImageSetEntry` needed for `FillBounds`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeAAImageSetEntryInfo {
    pub dst_rect: Rect,
    pub matrix_index: i32,
    pub has_clip: bool,
}

struct SaveBounds {
    control_ops: i32,
    bounds: Rect,
    /// Opaque token identifying the `SkPaint*` for this `SaveLayer` (0 = null).
    paint_token: usize,
    ctm: Matrix,
}

/// State machine implementing `SkRecords::FillBounds`.
pub struct FillBoundsState {
    cull_rect: Rect,
    ctm: Matrix,
    current_op: usize,
    bounds: Vec<Rect>,
    is_draw: Vec<bool>,
    save_stack: Vec<SaveBounds>,
    control_indices: Vec<usize>,
}

impl FillBoundsState {
    pub fn new(cull_rect: Rect, count: usize) -> Self {
        let ctm = Matrix::identity();
        let save_stack = vec![SaveBounds {
            control_ops: 0,
            bounds: Rect::make_empty(),
            paint_token: 0,
            ctm,
        }];
        Self {
            cull_rect,
            ctm,
            current_op: 0,
            bounds: vec![Rect::make_empty(); count],
            is_draw: vec![false; count],
            save_stack,
            control_indices: Vec::new(),
        }
    }

    #[inline]
    pub fn set_current_op(&mut self, op: usize) {
        self.current_op = op;
    }

    #[inline]
    pub fn ctm(&self) -> &Matrix {
        &self.ctm
    }

    // ---- CTM updates ----

    #[inline]
    pub fn update_ctm_set_matrix(&mut self, m: &Matrix) {
        self.ctm = *m;
    }

    #[inline]
    pub fn update_ctm_set_m44(&mut self, m: &M44) {
        self.ctm = m44_as_m33(m);
    }

    #[inline]
    pub fn update_ctm_concat(&mut self, m: &Matrix) {
        self.ctm.pre_concat(m);
    }

    #[inline]
    pub fn update_ctm_concat44(&mut self, m: &M44) {
        let m33 = m44_as_m33(m);
        self.ctm.pre_concat(&m33);
    }

    #[inline]
    pub fn update_ctm_scale(&mut self, sx: f32, sy: f32) {
        self.ctm.pre_scale(sx, sy);
    }

    #[inline]
    pub fn update_ctm_translate(&mut self, dx: f32, dy: f32) {
        self.ctm.pre_translate(dx, dy);
    }

    // ---- Save / Restore / Control ops ----

    pub fn push_save_block(
        &mut self,
        paint_token: usize,
        has_backdrop_filter: bool,
        paint_affects_transparent_black: bool,
    ) {
        let affects_full_cull = has_backdrop_filter || paint_affects_transparent_black;
        let sb = SaveBounds {
            control_ops: 0,
            bounds: if affects_full_cull {
                self.cull_rect
            } else {
                Rect::make_empty()
            },
            paint_token,
            ctm: self.ctm,
        };
        self.save_stack.push(sb);
        self.push_control();
    }

    pub fn track_restore(&mut self, restored_ctm: &Matrix) {
        self.ctm = *restored_ctm;
        let is_save_layer = self
            .save_stack
            .last()
            .is_some_and(|sb| sb.paint_token != 0);
        let b = self.pop_save_block();
        let idx = self.current_op;
        if idx < self.bounds.len() {
            self.bounds[idx] = b;
            self.is_draw[idx] = is_save_layer;
        }
    }

    #[inline]
    pub fn push_control(&mut self) {
        self.control_indices.push(self.current_op);
        if let Some(top) = self.save_stack.last_mut() {
            top.control_ops += 1;
        }
    }

    fn pop_control(&mut self, bounds: Rect) {
        if let Some(idx) = self.control_indices.pop() {
            if idx < self.bounds.len() {
                self.bounds[idx] = bounds;
                self.is_draw[idx] = false;
            }
        }
    }

    fn pop_save_block(&mut self) -> Rect {
        let Some(mut sb) = self.save_stack.pop() else {
            return Rect::make_empty();
        };
        while sb.control_ops > 0 {
            sb.control_ops -= 1;
            self.pop_control(sb.bounds);
        }
        self.update_save_bounds(sb.bounds);
        sb.bounds
    }

    #[inline]
    fn update_save_bounds(&mut self, bounds: Rect) {
        if let Some(top) = self.save_stack.last_mut() {
            top.bounds.join(bounds);
        }
    }

    // ---- Bounds computation & recording ----

    /// Adjusts `rect` for all active `SaveLayer` paints on `save_stack`.
    /// Note: matches C++ `FillBounds::adjustForSaveLayerPaints`, including inverting and
    /// re-mapping every stack entry's CTM even when `paint_token == 0`.
    pub fn adjust_for_save_layer_paints<F>(
        &self,
        rect: &mut Rect,
        saves_to_ignore: usize,
        mut adjust_paint: F,
    ) -> bool
    where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let end = self.save_stack.len().saturating_sub(saves_to_ignore);
        for i in (0..end).rev() {
            let sb = &self.save_stack[i];
            let Some(inv) = sb.ctm.invert() else {
                return false;
            };
            *rect = matrix_map_rect(&inv, *rect);
            if sb.paint_token != 0 && !adjust_paint(sb.paint_token, rect) {
                return false;
            }
            *rect = matrix_map_rect(&sb.ctm, *rect);
        }
        true
    }

    /// Matches `FillBounds::adjustAndMap(SkRect rect, const SkPaint* paint) const`.
    pub fn adjust_and_map<F>(
        &self,
        rect: Rect,
        paint_token: usize,
        mut adjust_paint: F,
    ) -> Rect
    where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let mut r = rect.make_sorted();

        if paint_token != 0 && !adjust_paint(paint_token, &mut r) {
            return self.cull_rect;
        }

        if !self.adjust_for_save_layer_paints(&mut r, 0, adjust_paint) {
            return self.cull_rect;
        }

        r = matrix_map_rect(&self.ctm, r);

        if !r.intersect(self.cull_rect) {
            return Rect::make_empty();
        }
        r
    }

    /// Stores computed draw bounds for `current_op` and unions into the active save block.
    #[inline]
    pub fn record_draw_bounds(&mut self, b: Rect) {
        let idx = self.current_op;
        if idx < self.bounds.len() {
            self.bounds[idx] = b;
            self.is_draw[idx] = true;
        }
        self.update_save_bounds(b);
    }

    /// Records a draw op whose bounds are the full cull rect (`DrawPaint`, `DrawBehind`,
    /// inverse-fill `DrawPath`, uncullled `DrawAtlas`).
    #[inline]
    pub fn record_cull_rect_draw(&mut self) {
        self.record_draw_bounds(self.cull_rect);
    }

    /// Records a `NoOp` op (empty bounds, `isDraw = true` matching `trackBounds(const T&)`).
    #[inline]
    pub fn record_noop(&mut self) {
        self.record_draw_bounds(Rect::make_empty());
    }

    /// Records a standard draw op by running `adjust_and_map(local_rect, paint_token)`.
    pub fn record_rect_draw<F>(&mut self, local_rect: Rect, paint_token: usize, adjust_paint: F)
    where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let b = self.adjust_and_map(local_rect, paint_token, adjust_paint);
        self.record_draw_bounds(b);
    }

    /// Records `DrawPoints`: computes point bounds, pads by `max(strokeWidth, 0.01f) / 2`,
    /// then adjusts and maps.
    pub fn record_points_draw<F>(
        &mut self,
        pts: &[Point],
        stroke_width: f32,
        paint_token: usize,
        adjust_paint: F,
    ) where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let mut dst = Rect::bounds_or_empty(pts);
        let stroke = cpp_max_f32(stroke_width, 0.01f32);
        dst = dst.make_inset(-stroke / 2.0, -stroke / 2.0);
        self.record_rect_draw(dst, paint_token, adjust_paint);
    }

    /// Records `DrawPicture`: maps picture cull rect through `pic_matrix`, then adjusts and maps.
    pub fn record_picture_draw<F>(
        &mut self,
        pic_cull: Rect,
        pic_matrix: &Matrix,
        paint_token: usize,
        adjust_paint: F,
    ) where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let dst = matrix_map_rect(pic_matrix, pic_cull);
        self.record_rect_draw(dst, paint_token, adjust_paint);
    }

    /// Records `DrawTextBlob`: offsets `blob_bounds` by `(x, y)`, then adjusts and maps.
    pub fn record_text_blob_draw<F>(
        &mut self,
        blob_bounds: Rect,
        x: f32,
        y: f32,
        paint_token: usize,
        adjust_paint: F,
    ) where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let dst = blob_bounds.make_offset(x, y);
        self.record_rect_draw(dst, paint_token, adjust_paint);
    }

    /// Records `DrawEdgeAAQuad`: uses clip quad bounds if present, else `rect`.
    pub fn record_edge_aa_quad_draw<F>(
        &mut self,
        rect: Rect,
        clip_quad: Option<&[Point; 4]>,
        adjust_paint: F,
    ) where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let b = match clip_quad {
            Some(q) => Rect::bounds_or_empty(q),
            None => rect,
        };
        self.record_rect_draw(b, 0, adjust_paint);
    }

    /// Records `DrawEdgeAAImageSet` over extracted per-entry metadata, `dst_clips`, and `pre_view_matrices`.
    pub fn record_edge_aa_image_set_draw<F>(
        &mut self,
        entries: &[EdgeAAImageSetEntryInfo],
        dst_clips: &[Point],
        pre_view_matrices: &[Matrix],
        mut adjust_paint: F,
    ) where
        F: FnMut(usize, &mut Rect) -> bool,
    {
        let mut rect = Rect::make_empty();
        let mut clip_index = 0usize;
        for entry in entries {
            let mut entry_bounds = entry.dst_rect;
            if entry.has_clip {
                if clip_index + 4 <= dst_clips.len() {
                    entry_bounds = Rect::bounds_or_empty(&dst_clips[clip_index..clip_index + 4]);
                } else {
                    entry_bounds = Rect::make_empty();
                }
                clip_index += 4;
            }
            if entry.matrix_index >= 0 {
                let midx = entry.matrix_index as usize;
                if midx < pre_view_matrices.len() {
                    entry_bounds = matrix_map_rect(&pre_view_matrices[midx], entry_bounds);
                }
            }
            rect.join(self.adjust_and_map(entry_bounds, 0, &mut adjust_paint));
        }
        self.record_draw_bounds(rect);
    }

    /// Finalizes `FillBounds` (matching `~FillBounds()`): unwinds lingering save blocks
    /// and any remaining control ops (giving them `fCullRect`), returning `(bounds, is_draw)`.
    pub fn finish(mut self) -> (Vec<Rect>, Vec<bool>) {
        while !self.save_stack.is_empty() {
            self.pop_save_block();
        }
        while !self.control_indices.is_empty() {
            self.pop_control(self.cull_rect);
        }
        (self.bounds, self.is_draw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paint_may_affect_transparent_black() {
        assert!(!paint_may_affect_transparent_black(false, true, true, BLEND_CLEAR));
        assert!(paint_may_affect_transparent_black(true, true, false, 3));
        assert!(paint_may_affect_transparent_black(true, false, true, 3));
        assert!(paint_may_affect_transparent_black(true, false, false, -1));
        assert!(paint_may_affect_transparent_black(true, false, false, BLEND_DST_IN));
        assert!(paint_may_affect_transparent_black(true, false, false, BLEND_MODULATE));
        assert!(!paint_may_affect_transparent_black(true, false, false, 3)); // kSrcOver
    }

    #[test]
    fn test_fill_bounds_save_translate_draw_restore() {
        let cull = Rect::make_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut fb = FillBoundsState::new(cull, 4);

        // 0: Save
        fb.set_current_op(0);
        fb.push_save_block(0, false, false);

        // 1: Translate(10, 20)
        fb.set_current_op(1);
        fb.update_ctm_translate(10.0, 20.0);
        fb.push_control();

        // 2: DrawRect(0, 0, 10, 10)
        fb.set_current_op(2);
        fb.record_rect_draw(Rect::make_ltrb(0.0, 0.0, 10.0, 10.0), 0, |_, _| true);

        // 3: Restore (ctm = I)
        fb.set_current_op(3);
        fb.track_restore(&Matrix::identity());

        let (bounds, is_draw) = fb.finish();
        let expected = Rect::make_ltrb(10.0, 20.0, 20.0, 30.0);
        assert_eq!(bounds, vec![expected, expected, expected, expected]);
        assert_eq!(is_draw, vec![false, false, true, false]);
    }

    #[test]
    fn test_fill_bounds_save_layer_affects_transparent_black() {
        let cull = Rect::make_ltrb(0.0, 0.0, 200.0, 200.0);
        let mut fb = FillBoundsState::new(cull, 2);

        // 0: SaveLayer with backdrop (affects full cull rect)
        fb.set_current_op(0);
        fb.push_save_block(1, true, false);

        // 1: Restore
        fb.set_current_op(1);
        fb.track_restore(&Matrix::identity());

        let (bounds, is_draw) = fb.finish();
        assert_eq!(bounds, vec![cull, cull]);
        assert_eq!(is_draw, vec![false, true]); // Restore of SaveLayer with paint has isDraw=true
    }

    #[test]
    fn test_fill_bounds_points_hairline_padding() {
        let cull = Rect::make_ltrb(0.0, 0.0, 100.0, 100.0);
        let mut fb = FillBoundsState::new(cull, 1);
        fb.set_current_op(0);
        fb.record_points_draw(&[Point::new(10.0, 10.0)], 0.0, 0, |_, _| true);
        let (bounds, _) = fb.finish();
        assert!(!bounds[0].is_empty());
        assert!((bounds[0].left - 9.995).abs() < 1e-5);
        assert!((bounds[0].right - 10.005).abs() < 1e-5);
    }

    #[test]
    fn test_fill_bounds_edge_aa_image_set_and_unpaired_save() {
        let cull = Rect::make_ltrb(0.0, 0.0, 200.0, 200.0);
        let mut fb = FillBoundsState::new(cull, 3);
        fb.set_current_op(0);
        fb.push_save_block(0, false, false);

        fb.set_current_op(1);
        fb.update_ctm_scale(2.0, 2.0);
        fb.push_control();

        fb.set_current_op(2);
        let entries = [
            EdgeAAImageSetEntryInfo {
                dst_rect: Rect::make_ltrb(5.0, 5.0, 15.0, 15.0),
                matrix_index: 0,
                has_clip: false,
            },
            EdgeAAImageSetEntryInfo {
                dst_rect: Rect::make_ltrb(0.0, 0.0, 1.0, 1.0),
                matrix_index: -1,
                has_clip: true,
            },
        ];
        let clips = [
            Point::new(20.0, 10.0),
            Point::new(30.0, 10.0),
            Point::new(30.0, 25.0),
            Point::new(20.0, 25.0),
        ];
        let mats = [Matrix::translate(5.0, 0.0)];
        fb.record_edge_aa_image_set_draw(&entries, &clips, &mats, |_, _| true);

        // No matching restore -> finish() must unwind lingering Save block
        let (bounds, is_draw) = fb.finish();
        // Entry 0: (5,5,15,15) + translate(5,0) = (10,5,20,15), *2 = (20,10,40,30)
        // Entry 1: clip (20,10,30,25), *2 = (40,20,60,50)
        // Union = (20,10,60,50)
        let expected = Rect::make_ltrb(20.0, 10.0, 60.0, 50.0);
        assert_eq!(bounds, vec![expected, expected, expected]);
        assert_eq!(is_draw, vec![false, false, true]);
    }

    #[test]
    fn test_fill_bounds_non_invertible_save_layer_ctm() {
        let cull = Rect::make_ltrb(0.0, 0.0, 50.0, 50.0);
        let mut fb = FillBoundsState::new(cull, 3);
        fb.set_current_op(0);
        fb.update_ctm_scale(0.0, 0.0); // singular matrix!
        fb.push_control();

        fb.set_current_op(1);
        fb.push_save_block(0, false, false);

        fb.set_current_op(2);
        fb.record_rect_draw(Rect::make_ltrb(1.0, 1.0, 2.0, 2.0), 0, |_, _| true);

        let (bounds, _) = fb.finish();
        // Singular CTM in save_stack causes adjust_for_save_layer_paints to return false -> cull rect!
        assert_eq!(bounds[2], cull);
    }
}
