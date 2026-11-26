#![forbid(unsafe_code)]

//! Safe Rust blitter and region-clip traits used by all scan converters,
//! plus `RectClipBlitter` (exact port of `SkRectClipBlitter` for `do_anti_hairline`).

use crate::alpha_runs::break_at_slice;
use crate::fixed::{irect_contains, SkFixed};
use skia_rs_geometry::{IRect, Rect};

/// Safe Rust equivalent of `SkBlitter` virtual interface.
///
/// Note: `blit_anti_h` takes `&mut [u8]` and `&mut [i16]` because C++ wrapper blitters
/// (`SkRectClipBlitter`, `SkRgnClipBlitter`) cast away `const` and mutate `runs[]`/`aa[]`
/// in-place when clipping spans.
pub trait Blitter {
    fn blit_h(&mut self, x: i32, y: i32, width: i32);

    fn blit_anti_h(&mut self, x: i32, y: i32, antialias: &mut [u8], runs: &mut [i16]);

    fn blit_v(&mut self, x: i32, y: i32, height: i32, alpha: u8) {
        if alpha == 0 {
            return;
        }
        let mut runs = [1i16, 0i16];
        let mut aa = [alpha];
        for row in y..y.wrapping_add(height) {
            runs[0] = 1;
            runs[1] = 0;
            aa[0] = alpha;
            self.blit_anti_h(x, row, &mut aa, &mut runs);
        }
    }

    fn blit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        for row in y..y.wrapping_add(height) {
            self.blit_h(x, row, width);
        }
    }

    fn blit_anti_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        left_alpha: u8,
        right_alpha: u8,
    ) {
        self.blit_v(x, y, height, left_alpha);
        if width > 0 {
            self.blit_rect(x + 1, y, width, height);
        }
        self.blit_v(x + 1 + width, y, height, right_alpha);
    }

    fn blit_anti_h2(&mut self, x: i32, y: i32, a0: u8, a1: u8) {
        let mut runs = [1i16, 1i16, 0i16];
        let mut aa = [a0, a1];
        self.blit_anti_h(x, y, &mut aa, &mut runs);
    }

    fn blit_anti_v2(&mut self, x: i32, y: i32, a0: u8, a1: u8) {
        let mut runs = [1i16, 0i16];
        let mut aa = [a0];
        self.blit_anti_h(x, y, &mut aa, &mut runs);
        runs[0] = 1;
        runs[1] = 0;
        aa[0] = a1;
        self.blit_anti_h(x, y + 1, &mut aa, &mut runs);
    }

    /// Blit an A8 mask clipped to `clip`. `image` begins at `(bounds.left, bounds.top)` with stride `row_bytes`.
    fn blit_mask_a8(&mut self, image: &[u8], bounds: IRect, row_bytes: u32, clip: IRect);

    /// Fast-path for fat AA rectangles (called by `try_blit_fat_anti_rect` in AAA).
    fn blit_fat_anti_rect(&mut self, rect: Rect);

    fn request_rows_preserved(&self) -> i32 {
        1
    }

    /// Optional fast direct-blit horizontal hairline loop (matches `horiline` in `SkScan_Hairline.cpp`).
    fn blit_hori_line(&mut self, mut x: i32, stopx: i32, mut fy: SkFixed, dy: SkFixed) {
        debug_assert!(x < stopx);
        loop {
            self.blit_h(x, fy >> 16, 1);
            fy = fy.wrapping_add(dy);
            x += 1;
            if x >= stopx {
                break;
            }
        }
    }

    /// Optional fast direct-blit vertical hairline loop (matches `vertline` in `SkScan_Hairline.cpp`).
    fn blit_vert_line(&mut self, mut y: i32, stopy: i32, mut fx: SkFixed, dx: SkFixed) {
        debug_assert!(y < stopy);
        loop {
            self.blit_h(fx >> 16, y, 1);
            fx = fx.wrapping_add(dx);
            y += 1;
            if y >= stopy {
                break;
            }
        }
    }
}

/// Abstract region clip interface matching `SkRegion` queries and `SkRegion::Cliperator`.
pub trait RegionClip {
    fn is_empty(&self) -> bool;
    fn is_rect(&self) -> bool;
    fn bounds(&self) -> IRect;
    fn quick_contains(&self, r: IRect) -> bool;
    fn quick_reject(&self, r: IRect) -> bool {
        self.is_empty() || !IRect::intersects(self.bounds(), r)
    }
    fn for_each_clip_rect(&self, query: IRect, f: &mut dyn FnMut(IRect));
}

/// Simple rectangular clip implementing `RegionClip` (useful in tests and simple callers).
#[derive(Copy, Clone, Debug)]
pub struct RectRegionClip {
    pub rect: IRect,
}

impl RegionClip for RectRegionClip {
    fn is_empty(&self) -> bool {
        self.rect.is_empty()
    }

    fn is_rect(&self) -> bool {
        !self.rect.is_empty()
    }

    fn bounds(&self) -> IRect {
        self.rect
    }

    fn quick_contains(&self, r: IRect) -> bool {
        irect_contains(self.rect, r)
    }

    fn for_each_clip_rect(&self, query: IRect, f: &mut dyn FnMut(IRect)) {
        let mut sect = IRect::EMPTY;
        if sect.intersect_pair(self.rect, query) {
            f(sect);
        }
    }
}

#[inline]
fn compute_anti_width(runs: &[i16]) -> i32 {
    let mut width = 0i32;
    let mut idx = 0usize;
    loop {
        if idx >= runs.len() {
            break;
        }
        let count = runs[idx] as i32;
        debug_assert!(count >= 0);
        if count == 0 {
            break;
        }
        width += count;
        idx += count as usize;
    }
    width
}

#[inline]
fn y_in_rect(y: i32, rect: IRect) -> bool {
    (y.wrapping_sub(rect.top) as u32) < (rect.height() as u32)
}

#[inline]
fn x_in_rect(x: i32, rect: IRect) -> bool {
    (x.wrapping_sub(rect.left) as u32) < (rect.width() as u32)
}

/// Safe Rust equivalent of `SkRectClipBlitter` (used inside `do_anti_hairline`).
pub struct RectClipBlitter<'a, B: Blitter + ?Sized> {
    blitter: &'a mut B,
    clip_rect: IRect,
}

impl<'a, B: Blitter + ?Sized> RectClipBlitter<'a, B> {
    pub fn new(blitter: &'a mut B, clip_rect: IRect) -> Self {
        debug_assert!(!clip_rect.is_empty());
        Self { blitter, clip_rect }
    }
}

impl<'a, B: Blitter + ?Sized> Blitter for RectClipBlitter<'a, B> {
    fn blit_h(&mut self, mut left: i32, y: i32, mut width: i32) {
        debug_assert!(width > 0);
        if !y_in_rect(y, self.clip_rect) {
            return;
        }
        let mut right = left + width;
        if left < self.clip_rect.left {
            left = self.clip_rect.left;
        }
        if right > self.clip_rect.right {
            right = self.clip_rect.right;
        }
        width = right - left;
        if width > 0 {
            self.blitter.blit_h(left, y, width);
        }
    }

    fn blit_anti_h(&mut self, mut left: i32, y: i32, aa: &mut [u8], runs: &mut [i16]) {
        if y < self.clip_rect.top || y >= self.clip_rect.bottom || left >= self.clip_rect.right {
            return;
        }
        let mut width = compute_anti_width(runs);
        if width <= 0 {
            return;
        }
        let mut offset = 0usize;
        if left < self.clip_rect.left {
            let dx64 = (self.clip_rect.left as i64) - (left as i64);
            if dx64 >= width as i64 {
                return;
            }
            let dx = dx64 as usize;
            break_at_slice(runs, aa, dx);
            offset = dx;
            left = self.clip_rect.left;
            width -= dx as i32;
        }
        let max_width = self.clip_rect.right - left;
        if width > max_width {
            width = max_width;
            break_at_slice(&mut runs[offset..], &mut aa[offset..], width as usize);
            runs[offset + width as usize] = 0;
        }
        if width > 0 {
            self.blitter
                .blit_anti_h(left, y, &mut aa[offset..], &mut runs[offset..]);
        }
    }

    fn blit_v(&mut self, x: i32, y: i32, height: i32, alpha: u8) {
        debug_assert!(height > 0);
        if !x_in_rect(x, self.clip_rect) {
            return;
        }
        let y0 = y.max(self.clip_rect.top);
        let y1 = y.wrapping_add(height).min(self.clip_rect.bottom);
        if y0 < y1 {
            self.blitter.blit_v(x, y0, y1 - y0, alpha);
        }
    }

    fn blit_rect(&mut self, left: i32, y: i32, width: i32, height: i32) {
        let mut r = IRect::make_ltrb(left, y, left.wrapping_add(width), y.wrapping_add(height));
        let mut sect = IRect::EMPTY;
        if sect.intersect_pair(r, self.clip_rect) {
            r = sect;
            self.blitter.blit_rect(r.left, r.top, r.width(), r.height());
        }
    }

    fn blit_anti_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        left_alpha: u8,
        right_alpha: u8,
    ) {
        let mut r = IRect::make_ltrb(x, y, x + width + 2, y + height);
        let mut sect = IRect::EMPTY;
        if !sect.intersect_pair(r, self.clip_rect) {
            return;
        }
        r = sect;
        let top = r.top;
        let h = r.height();
        if r.left == x {
            if left_alpha != 0 {
                self.blitter.blit_v(x, top, h, left_alpha);
            }
            r.left += 1;
        }
        if r.right == x + width + 2 {
            if right_alpha != 0 {
                self.blitter.blit_v(x + width + 1, top, h, right_alpha);
            }
            r.right -= 1;
        }
        if r.left < r.right {
            self.blitter.blit_rect(r.left, top, r.width(), h);
        }
    }

    fn blit_mask_a8(&mut self, image: &[u8], bounds: IRect, row_bytes: u32, clip: IRect) {
        let mut sect = IRect::EMPTY;
        if sect.intersect_pair(clip, self.clip_rect) {
            self.blitter.blit_mask_a8(image, bounds, row_bytes, sect);
        }
    }

    fn blit_fat_anti_rect(&mut self, rect: Rect) {
        self.blitter.blit_fat_anti_rect(rect);
    }

    fn request_rows_preserved(&self) -> i32 {
        self.blitter.request_rows_preserved()
    }
}
