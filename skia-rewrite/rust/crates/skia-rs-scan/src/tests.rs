#![forbid(unsafe_code)]

use crate::aaa_path::aaa_fill_path;
use crate::alpha_runs::{
    assert_valid_slice, break_at_slice, catch_overflow, validate_slice, AlphaRuns,
};
use crate::analytic_edge::{
    snap_y, AnalyticCubicState, AnalyticEdgeBase, AnalyticQuadState,
};
use crate::antihair::{
    anti_fill_rect, anti_fill_xrect, anti_frame_rect_draw, anti_frame_rect_setup,
    anti_hair_line_rgn,
};
use crate::blitter::{Blitter, RectRegionClip, RegionClip};
use crate::distance_field::{
    compute_distance_field_size, generate_distance_field_from_a8_image,
    generate_distance_field_from_bw_image, generate_distance_field_from_lcd16_mask,
    DISTANCE_FIELD_PAD,
};
use crate::edge::{CubicEdgeState, EdgeBase, EdgeWinding, QuadEdgeState};
use crate::edge_builder::{AnalyticEdgeBuilder, BasicEdgeBuilder};
use crate::fixed::{fixed_to_float, float_to_fixed, irect_contains, xrect_from_rect};
use crate::hairline::{
    frame_rect, hair_line_rgn, hair_path_clipped, hair_rect_bounds, hair_rect_emit, HairCapStyle,
};
use crate::scan_path::{fill_path_clipped, fill_triangle_clipped};
use crate::scan_rect::{
    classify_anti_fill_path, conservative_round_to_int, fill_irect, fill_rect, fill_xrect,
    overflows_short_shift, path_requires_tiling, AntiFillPathAction,
};
use skia_rs_geometry::{IRect, Point, Rect};
use skia_rs_path::{Convexity, FillType, PathRaw, Verb, SEG_CUBIC, SEG_LINE, SEG_QUAD};

/// Simple software A8 canvas implementing `Blitter` for rasterization tests.
struct CanvasBlitter {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
    fat_anti_rects: Vec<Rect>,
}

impl CanvasBlitter {
    fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height) as usize],
            fat_anti_rects: Vec::new(),
        }
    }

    fn get(&self, x: i32, y: i32) -> u8 {
        assert!(x >= 0 && x < self.width && y >= 0 && y < self.height);
        self.pixels[(y * self.width + x) as usize]
    }

    fn set_max(&mut self, x: i32, y: i32, alpha: u8) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            let idx = (y * self.width + x) as usize;
            // Source-over on black or simple overwrite; for non-overlapping blits overwrite matches Skia.
            if alpha > 0 {
                self.pixels[idx] = self.pixels[idx].saturating_add(alpha);
            }
        }
    }

    fn non_zero_count(&self) -> usize {
        self.pixels.iter().filter(|&&p| p > 0).count()
    }
}

impl Blitter for CanvasBlitter {
    fn blit_h(&mut self, x: i32, y: i32, width: i32) {
        for col in x..(x + width) {
            self.set_max(col, y, 0xFF);
        }
    }

    fn blit_anti_h(&mut self, mut x: i32, y: i32, antialias: &mut [u8], runs: &mut [i16]) {
        let mut idx = 0usize;
        while idx < runs.len() && runs[idx] != 0 {
            let count = runs[idx] as i32;
            let a = antialias[idx];
            if a != 0 {
                for col in x..(x + count) {
                    self.set_max(col, y, a);
                }
            }
            x += count;
            idx += count as usize;
        }
    }

    fn blit_v(&mut self, x: i32, y: i32, height: i32, alpha: u8) {
        if alpha == 0 {
            return;
        }
        for row in y..(y + height) {
            self.set_max(x, row, alpha);
        }
    }

    fn blit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        for row in y..(y + height) {
            for col in x..(x + width) {
                self.set_max(col, row, 0xFF);
            }
        }
    }

    fn blit_mask_a8(&mut self, image: &[u8], bounds: IRect, row_bytes: u32, clip: IRect) {
        let rb = row_bytes as usize;
        for y in clip.top..clip.bottom {
            let row = ((y - bounds.top) as usize) * rb;
            for x in clip.left..clip.right {
                let a = image[row + (x - bounds.left) as usize];
                if a != 0 {
                    self.set_max(x, y, a);
                }
            }
        }
    }

    fn blit_fat_anti_rect(&mut self, rect: Rect) {
        self.fat_anti_rects.push(rect);
        anti_fill_rect::<Self, RectRegionClip>(rect, None, self);
    }
}

struct MultiRectRegion {
    rects: Vec<IRect>,
    bounds: IRect,
}

impl MultiRectRegion {
    fn new(rects: Vec<IRect>) -> Self {
        let mut bounds = IRect::EMPTY;
        for &r in &rects {
            bounds.join(r);
        }
        Self { rects, bounds }
    }
}

impl RegionClip for MultiRectRegion {
    fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
    fn is_rect(&self) -> bool {
        self.rects.len() == 1
    }
    fn bounds(&self) -> IRect {
        self.bounds
    }
    fn quick_contains(&self, r: IRect) -> bool {
        self.rects.len() == 1 && irect_contains(self.bounds, r)
    }
    fn for_each_clip_rect(&self, query: IRect, f: &mut dyn FnMut(IRect)) {
        for &r in &self.rects {
            let mut s = IRect::EMPTY;
            if s.intersect_pair(r, query) {
                f(s);
            }
        }
    }
}

#[test]
fn test_edge_cw_and_ccw_lines() {
    let mut e = EdgeBase::default();
    assert!(e.set_line(Point::new(0.0, 4.0), Point::new(10.0, 20.0), None));
    assert_eq!(e.winding(), EdgeWinding::Cw);
    assert_eq!(e.dx_dy, float_to_fixed(0.625));
    assert_eq!(e.x, float_to_fixed(0.3125));
    assert_eq!(e.first_y, 4);
    assert_eq!(e.last_y, 19);
    assert!(!e.has_next_segment());

    let mut e2 = EdgeBase::default();
    assert!(e2.set_line(Point::new(0.0, -4.0), Point::new(10.0, -20.0), None));
    assert_eq!(e2.winding(), EdgeWinding::Ccw);
    assert_eq!(e2.dx_dy, float_to_fixed(-0.625));
    assert_eq!(e2.x, float_to_fixed(9.6875));
    assert_eq!(e2.first_y, -20);
    assert_eq!(e2.last_y, -5);
    assert!(!e2.has_next_segment());

    // Clipped line
    let clip = IRect::make_ltrb(0, 10, 20, 15);
    let mut e3 = EdgeBase::default();
    assert!(e3.set_line(Point::new(0.0, 4.0), Point::new(10.0, 20.0), Some(clip)));
    assert_eq!(e3.first_y, 10);
    assert_eq!(e3.x, float_to_fixed(0.3125 + 6.0 * 0.625));
}

#[test]
fn test_quad_and_cubic_edges() {
    let pts = [Point::new(0.0, 0.0), Point::new(2.5, 15.0), Point::new(10.0, 20.0)];
    let mut qe = QuadEdgeState::default();
    assert!(qe.set_quadratic(&pts));
    assert_eq!(qe.base.winding(), EdgeWinding::Cw);
    assert_eq!(qe.base.segment_count, 3); // 4 segments total -> 3 remaining
    let mut segs = 1;
    while qe.base.has_next_segment() {
        assert!(qe.next_segment());
        segs += 1;
    }
    assert_eq!(segs, 4);
    assert_eq!(qe.base.last_y, 19);

    // Mostly flat quad skips zero-height segments
    let flat_pts = [Point::new(4.0, 19.0), Point::new(5.5, 18.2), Point::new(10.0, 18.0)];
    let mut qf = QuadEdgeState::default();
    assert!(qf.set_quadratic(&flat_pts));
    assert_eq!(qf.base.winding(), EdgeWinding::Ccw);
    assert_eq!(qf.base.first_y, 18);
    assert_eq!(qf.base.last_y, 18);
    assert!(qf.base.dx_dy < 0);
    assert!(!qf.base.has_next_segment());

    // Horizontalish quad returns false
    let horiz_pts = [Point::new(4.0, 18.0), Point::new(5.5, 18.2), Point::new(10.0, 18.4)];
    let mut qh = QuadEdgeState::default();
    assert!(!qh.set_quadratic(&horiz_pts));

    // Cubic edge
    let cpts = [
        Point::new(0.0, 0.0),
        Point::new(2.0, 13.0),
        Point::new(8.0, 6.0),
        Point::new(10.0, 20.0),
    ];
    let mut ce = CubicEdgeState::default();
    assert!(ce.set_cubic(&cpts));
    assert_eq!(ce.base.winding(), EdgeWinding::Cw);
    assert_eq!(ce.base.segment_count, 7); // 8 segments total -> 7 remaining
    let mut csegs = 1;
    while ce.base.has_next_segment() {
        assert!(ce.next_segment());
        csegs += 1;
    }
    assert_eq!(csegs, 8);
    assert_eq!(ce.base.last_y, 19);
}

#[test]
fn test_analytic_edges() {
    let mut ae = AnalyticEdgeBase::default();
    assert!(ae.set_line(Point::new(0.0, 0.0), Point::new(10.0, 20.0)));
    assert_eq!(ae.winding(), EdgeWinding::Cw);
    assert_eq!(ae.upper_y, 0);
    assert_eq!(ae.lower_y, float_to_fixed(20.0));
    assert!((fixed_to_float(ae.dx) - 0.5).abs() < 0.01);
    ae.go_y(float_to_fixed(10.0));
    assert!((fixed_to_float(ae.x) - 5.0).abs() < 0.05);

    assert_eq!(snap_y(float_to_fixed(1.24)), float_to_fixed(1.25));
    assert_eq!(snap_y(float_to_fixed(1.1)), float_to_fixed(1.0));

    let qpts = [Point::new(0.0, 0.0), Point::new(5.0, 10.0), Point::new(10.0, 20.0)];
    let mut aq = AnalyticQuadState::default();
    assert!(aq.set_quadratic(&qpts));
    while aq.base.curve_count > 0 {
        let _ = aq.update_quadratic();
    }

    let cpts = [
        Point::new(0.0, 0.0),
        Point::new(2.0, 10.0),
        Point::new(8.0, 12.0),
        Point::new(10.0, 20.0),
    ];
    let mut ac = AnalyticCubicState::default();
    assert!(ac.set_cubic(&cpts));
    while ac.base.curve_count < 0 {
        let _ = ac.update_cubic();
    }
}

#[test]
fn test_basic_edge_builder_trapezoid() {
    let pts = [
        Point::new(5.0, 0.0),
        Point::new(0.0, 20.0),
        Point::new(20.0, 20.0),
        Point::new(10.0, 0.0),
    ];
    let verbs = [Verb::Move, Verb::Line, Verb::Line, Verb::Line, Verb::Close];
    let raw = PathRaw {
        points: &pts,
        verbs: &verbs,
        conics: &[],
        bounds: Rect::make_ltrb(0.0, 0.0, 20.0, 20.0),
        fill_type: FillType::Winding,
        convexity: Convexity::ConvexCw,
        segment_mask: SEG_LINE,
    };

    let mut eb = BasicEdgeBuilder::new();
    let n = eb.build_edges(&raw, None);
    assert_eq!(n, 2);
    let mut left = *eb.edges[0].base();
    let mut right = *eb.edges[1].base();
    if left.x > right.x {
        core::mem::swap(&mut left, &mut right);
    }
    assert_eq!(left.winding(), EdgeWinding::Cw);
    assert_eq!(left.dx_dy, float_to_fixed(-0.25));
    assert_eq!(left.x, float_to_fixed(4.875));
    assert_eq!(left.first_y, 0);
    assert_eq!(left.last_y, 19);

    assert_eq!(right.winding(), EdgeWinding::Ccw);
    assert_eq!(right.dx_dy, float_to_fixed(0.5));
    assert_eq!(right.x, float_to_fixed(10.25));
    assert_eq!(right.first_y, 0);
    assert_eq!(right.last_y, 19);

    let mut aeb = AnalyticEdgeBuilder::new();
    assert_eq!(aeb.build_edges(&raw, None), 2);
}

#[test]
fn test_alpha_runs() {
    assert_eq!(catch_overflow(0), 0);
    assert_eq!(catch_overflow(255), 255);
    assert_eq!(catch_overflow(256), 255);

    let mut ar = AlphaRuns::new(16);
    assert!(ar.is_empty());
    assert!(validate_slice(&ar.runs, 16));

    let off = ar.add(2, 64, 5, 128, 255, 0);
    assert!(!ar.is_empty());
    assert!(validate_slice(&ar.runs, 16));
    assert_eq!(ar.alpha[0], 0);
    assert_eq!(ar.runs[0], 2);
    assert_eq!(ar.alpha[2], 64);
    assert_eq!(ar.runs[2], 1);
    assert_eq!(ar.alpha[3], 255);
    assert_eq!(ar.runs[3], 5);
    assert_eq!(ar.alpha[8], 128);
    assert_eq!(ar.runs[8], 1);
    assert!(off <= 16);

    break_at_slice(&mut ar.runs, &mut ar.alpha, 5);
    assert!(validate_slice(&ar.runs, 16));
    assert_eq!(ar.runs[3], 2);
    assert_eq!(ar.runs[5], 3);
    assert_eq!(ar.alpha[5], 255);
    assert!(assert_valid_slice(&ar.runs, &ar.alpha, 3, 64));
}

#[test]
fn test_scan_rects_and_bounds() {
    let mut canvas = CanvasBlitter::new(16, 16);
    let clip = RectRegionClip {
        rect: IRect::make_ltrb(2, 2, 10, 10),
    };
    fill_irect(IRect::make_ltrb(0, 0, 6, 6), Some(&clip), &mut canvas);
    for y in 0..16 {
        for x in 0..16 {
            let inside = (2..6).contains(&x) && (2..6).contains(&y);
            assert_eq!(canvas.get(x, y), if inside { 255 } else { 0 });
        }
    }

    let mut canvas2 = CanvasBlitter::new(16, 16);
    let multi = MultiRectRegion::new(vec![
        IRect::make_ltrb(1, 1, 4, 4),
        IRect::make_ltrb(6, 1, 9, 4),
    ]);
    fill_rect(Rect::make_ltrb(0.0, 0.0, 10.0, 10.0), Some(&multi), &mut canvas2);
    assert_eq!(canvas2.get(2, 2), 255);
    assert_eq!(canvas2.get(5, 2), 0);
    assert_eq!(canvas2.get(7, 2), 255);

    let mut canvas3 = CanvasBlitter::new(16, 16);
    fill_xrect(xrect_from_rect(Rect::make_ltrb(2.0, 3.0, 5.0, 6.0)), None::<&RectRegionClip>, &mut canvas3);
    assert_eq!(canvas3.non_zero_count(), 9);

    assert!(!path_requires_tiling(IRect::make_ltrb(0, 0, 100, 100)));
    assert!(path_requires_tiling(IRect::make_ltrb(0, 0, 20000, 100)));
    assert_eq!(overflows_short_shift(8191, 2), 0);
    assert_ne!(overflows_short_shift(8192, 2), 0);
    assert_eq!(overflows_short_shift(32767, 0), 0);
    assert_ne!(overflows_short_shift(32768, 0), 0);

    let cr = conservative_round_to_int(Rect::make_ltrb(1.0, 2.0, 5.0, 6.0));
    assert!(cr.left <= 1 && cr.top <= 2 && cr.right >= 5 && cr.bottom >= 6);

    let (action, _) = classify_anti_fill_path(
        Rect::make_ltrb(0.0, 0.0, 10.0, 10.0),
        IRect::make_ltrb(0, 0, 100, 100),
        false,
    );
    assert_eq!(action, AntiFillPathAction::RunAAA);
    let (action_bw, _) = classify_anti_fill_path(
        Rect::make_ltrb(0.0, 0.0, 9000.0, 10.0),
        IRect::make_ltrb(0, 0, 10000, 100),
        false,
    );
    assert_eq!(action_bw, AntiFillPathAction::FallbackToBW);
}

#[test]
fn test_scan_path_and_triangle() {
    let mut canvas = CanvasBlitter::new(20, 20);
    let pts = [
        Point::new(2.0, 2.0),
        Point::new(12.0, 2.0),
        Point::new(12.0, 12.0),
        Point::new(2.0, 12.0),
    ];
    let verbs = [Verb::Move, Verb::Line, Verb::Line, Verb::Line, Verb::Close];
    let raw = PathRaw {
        points: &pts,
        verbs: &verbs,
        conics: &[],
        bounds: Rect::make_ltrb(2.0, 2.0, 12.0, 12.0),
        fill_type: FillType::Winding,
        convexity: Convexity::ConvexCw,
        segment_mask: SEG_LINE,
    };
    fill_path_clipped(&raw, IRect::make_ltrb(0, 0, 20, 20), &mut canvas, 2, 12, true);
    assert_eq!(canvas.non_zero_count(), 100);
    assert_eq!(canvas.get(2, 2), 255);
    assert_eq!(canvas.get(11, 11), 255);
    assert_eq!(canvas.get(12, 12), 0);

    // Concave / EvenOdd donut
    let donut_pts = [
        Point::new(2.0, 2.0),
        Point::new(12.0, 2.0),
        Point::new(12.0, 12.0),
        Point::new(2.0, 12.0),
        Point::new(5.0, 5.0),
        Point::new(9.0, 5.0),
        Point::new(9.0, 9.0),
        Point::new(5.0, 9.0),
    ];
    let donut_verbs = [
        Verb::Move,
        Verb::Line,
        Verb::Line,
        Verb::Line,
        Verb::Close,
        Verb::Move,
        Verb::Line,
        Verb::Line,
        Verb::Line,
        Verb::Close,
    ];
    let donut = PathRaw {
        points: &donut_pts,
        verbs: &donut_verbs,
        conics: &[],
        bounds: Rect::make_ltrb(2.0, 2.0, 12.0, 12.0),
        fill_type: FillType::EvenOdd,
        convexity: Convexity::Concave,
        segment_mask: SEG_LINE,
    };
    let mut canvas_eo = CanvasBlitter::new(20, 20);
    fill_path_clipped(&donut, IRect::make_ltrb(0, 0, 20, 20), &mut canvas_eo, 2, 12, true);
    assert_eq!(canvas_eo.non_zero_count(), 100 - 16);
    assert_eq!(canvas_eo.get(7, 7), 0);
    assert_eq!(canvas_eo.get(3, 3), 255);

    // Triangle
    let mut canvas_tri = CanvasBlitter::new(20, 20);
    let tri = [Point::new(2.0, 2.0), Point::new(12.0, 2.0), Point::new(2.0, 12.0)];
    fill_triangle_clipped(&tri, None, &mut canvas_tri, IRect::make_ltrb(2, 2, 12, 12));
    assert_eq!(canvas_tri.get(3, 3), 255);
    assert_eq!(canvas_tri.get(10, 10), 0);
}

#[test]
fn test_aaa_path_mask_rle_and_fat_rect() {
    // 1. Fat rect fast path (small rect >= 3 wide)
    let rect_pts = [
        Point::new(2.5, 2.5),
        Point::new(10.5, 2.5),
        Point::new(10.5, 10.5),
        Point::new(2.5, 10.5),
    ];
    let rect_verbs = [Verb::Move, Verb::Line, Verb::Line, Verb::Line, Verb::Close];
    let rect_raw = PathRaw {
        points: &rect_pts,
        verbs: &rect_verbs,
        conics: &[],
        bounds: Rect::make_ltrb(2.5, 2.5, 10.5, 10.5),
        fill_type: FillType::Winding,
        convexity: Convexity::ConvexCw,
        segment_mask: SEG_LINE,
    };
    let mut c_fat = CanvasBlitter::new(20, 20);
    aaa_fill_path(
        &rect_raw,
        &mut c_fat,
        IRect::make_ltrb(2, 2, 11, 11),
        IRect::make_ltrb(0, 0, 20, 20),
        false,
    );
    assert_eq!(c_fat.fat_anti_rects.len(), 1);
    assert_eq!(c_fat.get(6, 6), 255);

    // 2. MaskAdditiveBlitter on small triangle
    let tri_pts = [
        Point::new(2.0, 2.0),
        Point::new(14.0, 2.0),
        Point::new(8.0, 14.0),
    ];
    let tri_verbs = [Verb::Move, Verb::Line, Verb::Line, Verb::Close];
    let tri_raw = PathRaw {
        points: &tri_pts,
        verbs: &tri_verbs,
        conics: &[],
        bounds: Rect::make_ltrb(2.0, 2.0, 14.0, 14.0),
        fill_type: FillType::Winding,
        convexity: Convexity::ConvexCw,
        segment_mask: SEG_LINE,
    };
    let mut c_mask = CanvasBlitter::new(20, 20);
    aaa_fill_path(
        &tri_raw,
        &mut c_mask,
        IRect::make_ltrb(2, 2, 14, 14),
        IRect::make_ltrb(0, 0, 20, 20),
        false,
    );
    assert_eq!(c_mask.get(8, 5), 255);
    assert!(c_mask.get(5, 8) > 0 && c_mask.get(5, 8) < 255);

    // 3. RunBasedAdditiveBlitter (force_rle = true) on same triangle -> matches mask!
    let mut c_rle = CanvasBlitter::new(20, 20);
    aaa_fill_path(
        &tri_raw,
        &mut c_rle,
        IRect::make_ltrb(2, 2, 14, 14),
        IRect::make_ltrb(0, 0, 20, 20),
        true,
    );
    assert_eq!(c_rle.get(8, 5), 255);
    assert!(c_rle.get(5, 8) > 0 && c_rle.get(5, 8) < 255);

    // 4. SafeRLEAdditiveBlitter on concave quadratic curve
    let q_pts = [
        Point::new(2.0, 2.0),
        Point::new(20.0, 10.0),
        Point::new(2.0, 18.0),
    ];
    let q_verbs = [Verb::Move, Verb::Quad, Verb::Close];
    let q_raw = PathRaw {
        points: &q_pts,
        verbs: &q_verbs,
        conics: &[],
        bounds: Rect::make_ltrb(2.0, 2.0, 20.0, 18.0),
        fill_type: FillType::Winding,
        convexity: Convexity::Concave,
        segment_mask: SEG_QUAD,
    };
    let mut c_quad = CanvasBlitter::new(24, 24);
    aaa_fill_path(
        &q_raw,
        &mut c_quad,
        IRect::make_ltrb(2, 2, 20, 18),
        IRect::make_ltrb(0, 0, 24, 24),
        true,
    );
    assert_eq!(c_quad.get(5, 10), 255);
}

#[test]
fn test_hairline_and_antihair() {
    let mut canvas = CanvasBlitter::new(32, 32);
    let line = [Point::new(2.5, 2.5), Point::new(20.5, 10.5)];
    hair_line_rgn::<_, CanvasBlitter, RectRegionClip>(&line, None, &mut canvas, None);
    assert!(canvas.non_zero_count() >= 18);

    // HairRect
    let mut c_hr = CanvasBlitter::new(20, 20);
    let hr = hair_rect_bounds(Rect::make_ltrb(2.0, 2.0, 8.0, 8.0), IRect::make_ltrb(0, 0, 20, 20))
        .unwrap();
    hair_rect_emit(hr, &mut c_hr);
    assert_eq!(c_hr.get(2, 2), 255);
    assert_eq!(c_hr.get(8, 8), 255);
    assert_eq!(c_hr.get(5, 5), 0);

    // HairPath with cubic and round caps
    let cpts = [
        Point::new(2.0, 10.0),
        Point::new(8.0, 2.0),
        Point::new(16.0, 18.0),
        Point::new(22.0, 10.0),
    ];
    let cverbs = [Verb::Move, Verb::Cubic];
    let craw = PathRaw {
        points: &cpts,
        verbs: &cverbs,
        conics: &[],
        bounds: Rect::make_ltrb(2.0, 2.0, 22.0, 18.0),
        fill_type: FillType::Winding,
        convexity: Convexity::Concave,
        segment_mask: SEG_CUBIC,
    };
    let mut c_hp = CanvasBlitter::new(32, 32);
    hair_path_clipped::<_, CanvasBlitter, RectRegionClip>(
        &craw,
        None,
        true,
        &mut c_hp,
        None,
        HairCapStyle::Round,
        true,
    );
    assert!(c_hp.non_zero_count() > 20);

    // AntiHairLineRgn horizontal, vertical, diagonal, clipped
    let mut c_ah = CanvasBlitter::new(32, 32);
    let clip = RectRegionClip {
        rect: IRect::make_ltrb(0, 0, 32, 32),
    };
    anti_hair_line_rgn(&[Point::new(2.0, 5.25), Point::new(18.0, 5.25)], Some(&clip), &mut c_ah);
    anti_hair_line_rgn(&[Point::new(10.5, 2.0), Point::new(10.5, 20.0)], Some(&clip), &mut c_ah);
    anti_hair_line_rgn(&[Point::new(2.0, 2.0), Point::new(28.0, 25.0)], Some(&clip), &mut c_ah);
    assert!(c_ah.non_zero_count() > 40);

    // AntiFillRect & AntiFrameRect & FrameRect
    let mut c_af = CanvasBlitter::new(20, 20);
    anti_fill_rect(Rect::make_ltrb(2.5, 2.5, 8.5, 8.5), Some(&clip), &mut c_af);
    assert_eq!(c_af.get(5, 5), 255);
    assert!(c_af.get(2, 5) > 0 && c_af.get(2, 5) < 255);

    let mut c_ax = CanvasBlitter::new(20, 20);
    anti_fill_xrect(
        xrect_from_rect(Rect::make_ltrb(2.5, 2.5, 8.5, 8.5)),
        Some(&clip),
        &mut c_ax,
    );
    assert_eq!(c_af.pixels, c_ax.pixels);

    let mut c_frame = CanvasBlitter::new(20, 20);
    let r = Rect::make_ltrb(4.0, 4.0, 14.0, 14.0);
    let stroke = Point::new(2.0, 2.0);
    let params = anti_frame_rect_setup(r, stroke);
    anti_frame_rect_draw(r, stroke, &params, &mut c_frame);
    assert_eq!(c_frame.get(4, 4), 255);
    assert_eq!(c_frame.get(9, 9), 0);

    let mut c_fr = CanvasBlitter::new(20, 20);
    frame_rect(r, stroke, Some(&clip), &mut c_fr);
    assert_eq!(c_fr.get(4, 4), 255);
    assert_eq!(c_fr.get(9, 9), 0);
}

#[test]
fn test_distance_field_compute_size_and_gen() {
    assert_eq!(compute_distance_field_size(0, 0), 0);
    assert_eq!(compute_distance_field_size(-1, 10), 0);
    assert_eq!(compute_distance_field_size(10, -1), 0);
    assert_eq!(compute_distance_field_size(-10, -10), 0);

    let pad = DISTANCE_FIELD_PAD as usize;
    assert_eq!(compute_distance_field_size(16, 16), (16 + 2 * pad) * (16 + 2 * pad));

    assert_eq!(compute_distance_field_size(i32::MAX, i32::MAX), 0);
    assert_eq!(compute_distance_field_size(i32::MAX, 1), 0);
    assert_eq!(compute_distance_field_size(1, i32::MAX), 0);
    assert_eq!(compute_distance_field_size(1 << 30, 1 << 30), 0);
    assert_eq!(compute_distance_field_size(i32::MAX - 1, 10), 0);
    assert_eq!(compute_distance_field_size(4071, 65527), 0);

    // Exact DFScratch total buffer boundary from tests/DistanceFieldTest.cpp
    assert_eq!(
        compute_distance_field_size(48067, 2472),
        (48067 + 2 * pad) * (2472 + 2 * pad)
    );
    assert_eq!(compute_distance_field_size(48068, 2472), 0);

    // A8 generation
    let w = 8;
    let h = 8;
    let mut a8 = vec![0u8; w * h];
    for y in 2..6 {
        for x in 2..6 {
            a8[y * w + x] = 0xFF;
        }
    }
    let df_size = compute_distance_field_size(w as i32, h as i32);
    let mut df_a8 = vec![0u8; df_size];
    assert!(generate_distance_field_from_a8_image(&mut df_a8, &a8, w as i32, h as i32, w));

    // Inside center of 4x4 square should be > 128, far outside should be < 128
    let out_w = w + 2 * pad;
    let center = df_a8[(4 + pad) * out_w + (4 + pad)];
    let corner = df_a8[0];
    assert!(center > 128, "center={center}");
    assert!(corner < 128, "corner={corner}");

    // LCD16 generation (0xFFFF -> 255) should match A8 identically!
    let mut lcd = vec![0u8; w * h * 2];
    for y in 2..6 {
        for x in 2..6 {
            let off = (y * w + x) * 2;
            lcd[off] = 0xFF;
            lcd[off + 1] = 0xFF;
        }
    }
    let mut df_lcd = vec![0u8; df_size];
    assert!(generate_distance_field_from_lcd16_mask(
        &mut df_lcd,
        &lcd,
        w as i32,
        h as i32,
        w * 2
    ));
    assert_eq!(df_a8, df_lcd);

    // BW generation (0x3C = bits 2..6 set) should match A8 identically!
    let mut bw = vec![0u8; h];
    for row in &mut bw[2..6] {
        *row = 0x3C;
    }
    let mut df_bw = vec![0u8; df_size];
    assert!(generate_distance_field_from_bw_image(&mut df_bw, &bw, w as i32, h as i32, 1));
    assert_eq!(df_a8, df_bw);
}
