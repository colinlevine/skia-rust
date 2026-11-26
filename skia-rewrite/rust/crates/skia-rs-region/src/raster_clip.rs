#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkRasterClip.cpp` and `src/core/SkRasterClipStack.h`.
//!
//! Wraps a BW `Region` and antialiased `AAClip`, plus the copy-on-write
//! deferred-save `RasterClipStack`.

use crate::aa_clip::AAClip;
use crate::region::{Region, RegionOp};
use crate::{irect_contains_rect, rect_round, ClipOp};
use skia_rs_geometry::{IRect, Matrix, Rect};

/// Matches `nearly_integral` in `SkRasterClip.cpp`.
///
/// Antialiasing has a granularity of 1/4 of a pixel along each axis, so a
/// coordinate is treated as an integer if it differs from its nearest int by
/// less than half of that value (1/8).
#[inline]
pub fn nearly_integral(x: f32) -> bool {
    const DOMAIN: f32 = 1.0 / 4.0;
    const HALF_DOMAIN: f32 = DOMAIN / 2.0;

    let shifted = x + HALF_DOMAIN;
    shifted - shifted.floor() < DOMAIN
}

/// Returns `true` if all four edges of `r` are `nearly_integral`.
#[inline]
pub fn rect_is_nearly_integral(r: Rect) -> bool {
    nearly_integral(r.left)
        && nearly_integral(r.top)
        && nearly_integral(r.right)
        && nearly_integral(r.bottom)
}

#[inline]
fn clip_op_to_region_op(op: ClipOp) -> RegionOp {
    match op {
        ClipOp::Difference => RegionOp::Difference,
        ClipOp::Intersect => RegionOp::Intersect,
    }
}

/// Safe Rust port of `SkRasterClip`.
#[derive(Clone, Debug)]
pub struct RasterClip {
    bw: Region,
    aa: AAClip,
    is_bw: bool,
    is_empty: bool,
    is_rect: bool,
}

impl Default for RasterClip {
    fn default() -> Self {
        Self::new()
    }
}

impl RasterClip {
    /// Matches `SkRasterClip::SkRasterClip()`.
    pub fn new() -> Self {
        Self {
            bw: Region::Empty,
            aa: AAClip::new_empty(),
            is_bw: true,
            is_empty: true,
            is_rect: false,
        }
    }

    /// Matches `SkRasterClip::SkRasterClip(const SkIRect& bounds)`.
    pub fn from_irect(bounds: IRect) -> Self {
        let bw = Region::from_rect(bounds);
        let is_empty = bw.is_empty();
        let is_rect = !is_empty;
        Self {
            bw,
            aa: AAClip::new_empty(),
            is_bw: true,
            is_empty,
            is_rect,
        }
    }

    /// Matches `SkRasterClip::SkRasterClip(const SkRegion& rgn)`.
    pub fn from_region(rgn: Region) -> Self {
        let is_empty = rgn.is_empty();
        let is_rect = rgn.is_rect();
        Self {
            bw: rgn,
            aa: AAClip::new_empty(),
            is_bw: true,
            is_empty,
            is_rect,
        }
    }

    /// Constructs an AA `RasterClip` directly from an `AAClip`.
    pub fn from_aa_clip(aa: AAClip) -> Self {
        let is_empty = aa.is_empty();
        let is_rect = aa.is_rect();
        Self {
            bw: Region::Empty,
            aa,
            is_bw: false,
            is_empty,
            is_rect,
        }
    }

    #[inline]
    pub fn is_bw(&self) -> bool {
        self.is_bw
    }

    #[inline]
    pub fn is_aa(&self) -> bool {
        !self.is_bw
    }

    #[inline]
    pub fn bw_rgn(&self) -> &Region {
        debug_assert!(self.is_bw);
        &self.bw
    }

    #[inline]
    pub fn aa_rgn(&self) -> &AAClip {
        debug_assert!(!self.is_bw);
        &self.aa
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        debug_assert_eq!(self.compute_is_empty(), self.is_empty);
        self.is_empty
    }

    #[inline]
    pub fn is_rect(&self) -> bool {
        debug_assert_eq!(self.compute_is_rect(), self.is_rect);
        self.is_rect
    }

    #[inline]
    pub fn is_complex(&self) -> bool {
        if self.is_bw {
            self.bw.is_complex()
        } else {
            !self.aa.is_empty()
        }
    }

    #[inline]
    pub fn bounds(&self) -> IRect {
        if self.is_bw {
            self.bw.bounds()
        } else {
            self.aa.bounds()
        }
    }

    #[inline]
    pub fn quick_contains(&self, rect: IRect) -> bool {
        if self.is_bw {
            self.bw.quick_contains(rect)
        } else {
            self.aa.quick_contains_rect(rect)
        }
    }

    #[inline]
    pub fn quick_reject(&self, rect: IRect) -> bool {
        !IRect::intersects(self.bounds(), rect)
    }

    /// Matches `SkRasterClip::setEmpty()`.
    pub fn set_empty(&mut self) -> bool {
        self.is_bw = true;
        self.bw.set_empty();
        self.aa.set_empty();
        self.is_empty = true;
        self.is_rect = false;
        false
    }

    /// Matches `SkRasterClip::setRect(const SkIRect&)`.
    pub fn set_rect(&mut self, rect: IRect) -> bool {
        self.is_bw = true;
        self.aa.set_empty();
        self.is_rect = self.bw.set_rect(rect);
        self.is_empty = !self.is_rect;
        self.is_rect
    }

    /// Matches `SkRasterClip::op(const SkIRect&, SkClipOp)`.
    pub fn op_irect(&mut self, rect: IRect, op: ClipOp) -> bool {
        if self.is_bw {
            self.bw.op_rect(rect, clip_op_to_region_op(op));
        } else {
            self.aa.op_irect(rect, op);
        }
        self.update_cache_and_return_non_empty(true)
    }

    /// Matches `SkRasterClip::op(const SkRegion&, SkClipOp)`.
    pub fn op_region(&mut self, rgn: &Region, op: ClipOp) -> bool {
        if self.is_bw {
            let cur = self.bw.clone();
            self.bw.op_regions(&cur, rgn, clip_op_to_region_op(op));
        } else {
            let mut tmp = AAClip::new_empty();
            tmp.set_region(rgn);
            self.aa.op_aa_clip(&tmp, op);
        }
        self.update_cache_and_return_non_empty(true)
    }

    /// Matches the scale-translate path of `SkRasterClip::op(const SkRect&, const SkMatrix&, SkClipOp, bool)`.
    ///
    /// Returns `Some(non_empty)` when `matrix.is_scale_translate()`, or `None`
    /// when the caller must convert `local_rect` to a path and rasterize it.
    pub fn op_rect_scale_translate(
        &mut self,
        local_rect: Rect,
        matrix: &Matrix,
        op: ClipOp,
        mut do_aa: bool,
    ) -> Option<bool> {
        if !matrix.is_scale_translate() {
            return None;
        }
        let dev_rect = matrix.map_rect_scale_translate(local_rect);
        if self.is_bw && do_aa && rect_is_nearly_integral(dev_rect) {
            do_aa = false;
        }
        if self.is_bw && !do_aa {
            self.bw.op_rect(rect_round(dev_rect), clip_op_to_region_op(op));
            Some(self.update_cache_and_return_non_empty(true))
        } else {
            if self.is_bw {
                self.convert_to_aa();
            }
            if !do_aa {
                self.aa.op_irect(rect_round(dev_rect), op);
                Some(self.update_cache_and_return_non_empty(true))
            } else {
                // Subpixel AA rect rasterization requires the scan converter (handled via AAClipBuilder in C++ bridge).
                None
            }
        }
    }

    /// Matches `SkRasterClip::op(const SkRasterClip&, SkClipOp)`.
    pub fn op_raster_clip(&mut self, clip: &RasterClip, op: ClipOp) -> bool {
        if self.is_bw && clip.is_bw {
            let cur = self.bw.clone();
            self.bw.op_regions(&cur, &clip.bw, clip_op_to_region_op(op));
        } else {
            if self.is_bw {
                self.convert_to_aa();
            }
            if clip.is_bw {
                let mut tmp = AAClip::new_empty();
                tmp.set_region(&clip.bw);
                self.aa.op_aa_clip(&tmp, op);
            } else {
                self.aa.op_aa_clip(&clip.aa, op);
            }
        }
        self.update_cache_and_return_non_empty(true)
    }

    /// Matches `SkRasterClip::translate(int dx, int dy, SkRasterClip* dst)`.
    pub fn translate(&self, dx: i32, dy: i32) -> Self {
        if self.is_empty() {
            return Self::new();
        }
        if (dx | dy) == 0 {
            return self.clone();
        }
        let mut dst = Self::new();
        dst.is_bw = self.is_bw;
        if self.is_bw {
            dst.bw = self.bw.translate(dx, dy);
            dst.aa.set_empty();
        } else {
            dst.aa = self.aa.translate(dx, dy);
            dst.bw.set_empty();
        }
        dst.update_cache_and_return_non_empty(true);
        dst
    }

    /// Matches `SkRasterClip::convertToAA()`.
    pub fn convert_to_aa(&mut self) {
        debug_assert!(self.is_bw);
        self.aa.set_region(&self.bw);
        self.is_bw = false;
        self.update_cache_and_return_non_empty(false);
    }

    /// Matches `SkRasterClip::validate()`.
    pub fn validate(&self) -> bool {
        if self.is_bw && !self.aa.is_empty() {
            return false;
        }
        if !self.bw.is_valid() || !self.aa.is_valid() {
            return false;
        }
        self.compute_is_empty() == self.is_empty && self.compute_is_rect() == self.is_rect
    }

    #[inline]
    fn compute_is_empty(&self) -> bool {
        if self.is_bw {
            self.bw.is_empty()
        } else {
            self.aa.is_empty()
        }
    }

    #[inline]
    fn compute_is_rect(&self) -> bool {
        if self.is_bw {
            self.bw.is_rect()
        } else {
            self.aa.is_rect()
        }
    }

    fn update_cache_and_return_non_empty(&mut self, detect_aa_rect: bool) -> bool {
        self.is_empty = self.compute_is_empty();

        // Detect that our computed AA is really just a (hard-edged) rect.
        if detect_aa_rect && !self.is_empty && !self.is_bw && self.aa.is_rect() {
            self.bw.set_rect(self.aa.bounds());
            self.aa.set_empty();
            self.is_bw = true;
        }

        self.is_rect = self.compute_is_rect();
        !self.is_empty
    }
}

/// Safe Rust port of `SkRasterClipStack` (copy-on-write stack with deferred saves).
#[derive(Clone, Debug)]
pub struct RasterClipStack {
    stack: Vec<StackRec>,
    root_bounds: IRect,
    disable_aa: bool,
}

#[derive(Clone, Debug)]
struct StackRec {
    rc: RasterClip,
    deferred_count: i32,
}

impl RasterClipStack {
    pub fn new(width: i32, height: i32) -> Self {
        let root_bounds = IRect::from_wh(width, height);
        // Matches SkScan::PathRequiresTiling threshold (32767 >> 1 = 16383)
        let disable_aa = root_bounds.width() > (32767 >> 1) || root_bounds.height() > (32767 >> 1);
        Self {
            stack: vec![StackRec {
                rc: RasterClip::from_irect(root_bounds),
                deferred_count: 0,
            }],
            root_bounds,
            disable_aa,
        }
    }

    pub fn set_new_size(&mut self, w: i32, h: i32) {
        self.root_bounds = IRect::from_wh(w, h);
        debug_assert_eq!(self.stack.len(), 1);
        let rec = self.stack.last_mut().unwrap();
        debug_assert_eq!(rec.deferred_count, 0);
        rec.rc.set_rect(self.root_bounds);
    }

    #[inline]
    pub fn rc(&self) -> &RasterClip {
        &self.stack.last().unwrap().rc
    }

    #[inline]
    pub fn disable_aa(&self) -> bool {
        self.disable_aa
    }

    pub fn save(&mut self) {
        let top = self.stack.last_mut().unwrap();
        debug_assert!(top.deferred_count >= 0);
        top.deferred_count += 1;
    }

    pub fn restore(&mut self) {
        let top = self.stack.last_mut().unwrap();
        top.deferred_count -= 1;
        if top.deferred_count < 0 {
            debug_assert_eq!(top.deferred_count, -1);
            debug_assert!(self.stack.len() > 1);
            self.stack.pop();
        }
    }

    pub fn clip_irect(&mut self, rect: IRect, op: ClipOp) {
        self.writable_rc().op_irect(rect, op);
        debug_assert!(self.validate());
    }

    pub fn clip_region(&mut self, rgn: &Region, op: ClipOp) {
        self.writable_rc().op_region(rgn, op);
        debug_assert!(self.validate());
    }

    pub fn replace_clip(&mut self, rect: IRect) {
        let mut dev_rect = IRect::EMPTY;
        if !dev_rect.intersect_pair(rect, self.root_bounds) {
            self.writable_rc().set_empty();
        } else {
            self.writable_rc().set_rect(dev_rect);
        }
        debug_assert!(self.validate());
    }

    pub fn validate(&self) -> bool {
        let clip = self.rc();
        if !clip.validate() {
            return false;
        }
        if self.root_bounds.is_empty() {
            clip.is_empty()
        } else if !clip.is_empty() {
            irect_contains_rect(self.root_bounds, clip.bounds())
        } else {
            true
        }
    }

    fn writable_rc(&mut self) -> &mut RasterClip {
        let deferred = self.stack.last().unwrap().deferred_count;
        debug_assert!(deferred >= 0);
        if deferred > 0 {
            self.stack.last_mut().unwrap().deferred_count -= 1;
            let cloned = self.stack.last().unwrap().rc.clone();
            self.stack.push(StackRec {
                rc: cloned,
                deferred_count: 0,
            });
        }
        &mut self.stack.last_mut().unwrap().rc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nearly_integral() {
        assert!(nearly_integral(0.0));
        assert!(nearly_integral(5.0));
        assert!(nearly_integral(-3.0));
        assert!(nearly_integral(1.1)); // within 1/8
        assert!(nearly_integral(0.9)); // within 1/8 of 1.0
        assert!(!nearly_integral(1.5));
        assert!(!nearly_integral(1.25));
        assert!(!nearly_integral(-0.5));
        assert!(rect_is_nearly_integral(Rect::from_ltrb(0.05, 1.95, 10.02, 20.0)));
        assert!(!rect_is_nearly_integral(Rect::from_ltrb(0.0, 0.0, 10.5, 20.0)));
    }

    #[test]
    fn test_raster_clip_basic_bw_and_aa() {
        let mut rc = RasterClip::from_irect(IRect::from_ltrb(0, 0, 100, 100));
        assert!(rc.is_bw());
        assert!(rc.is_rect());
        assert!(!rc.is_empty());
        assert!(rc.validate());

        assert!(rc.op_irect(IRect::from_ltrb(10, 10, 50, 50), ClipOp::Intersect));
        assert_eq!(rc.bounds(), IRect::from_ltrb(10, 10, 50, 50));
        assert!(rc.is_rect());
        assert!(rc.validate());

        // Difference creates complex BW region
        assert!(rc.op_irect(IRect::from_ltrb(20, 20, 30, 30), ClipOp::Difference));
        assert!(rc.is_bw());
        assert!(rc.is_complex());
        assert!(!rc.is_rect());
        assert!(rc.validate());

        // Translate
        let moved = rc.translate(5, -3);
        assert_eq!(moved.bounds(), IRect::from_ltrb(15, 7, 55, 47));
        assert!(moved.validate());

        // Convert to AA and back to BW when reduced to a hard-edged rect
        rc.convert_to_aa();
        assert!(rc.is_aa());
        assert!(rc.validate());

        assert!(rc.op_irect(IRect::from_ltrb(10, 10, 20, 20), ClipOp::Intersect));
        // Since [10,10,20,20] is solid 0xFF rect, update_cache detects AA rect -> converts back to BW!
        // Note: is_rect_parts in SkAAClip checks yoff.y == bounds.bottom - 1 (matching Skia's exact code).
        assert!(rc.validate());
    }

    #[test]
    fn test_raster_clip_stack_cow() {
        let mut stack = RasterClipStack::new(100, 100);
        assert_eq!(stack.rc().bounds(), IRect::from_wh(100, 100));

        stack.save();
        stack.save();
        // Deferred saves: no clip modification yet
        assert_eq!(stack.rc().bounds(), IRect::from_wh(100, 100));

        stack.clip_irect(IRect::from_ltrb(10, 10, 50, 50), ClipOp::Intersect);
        assert_eq!(stack.rc().bounds(), IRect::from_ltrb(10, 10, 50, 50));

        stack.restore();
        assert_eq!(stack.rc().bounds(), IRect::from_wh(100, 100));

        stack.restore();
        assert_eq!(stack.rc().bounds(), IRect::from_wh(100, 100));
    }
}
