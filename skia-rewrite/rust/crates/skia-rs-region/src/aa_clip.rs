#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkAAClip.cpp`.
//!
//! Implements Skia's exact RLE antialiased clip format (`YOffset` table +
//! `[count: u8, alpha: u8]` horizontal runs), `AAClipBuilder`, boolean ops
//! (`Intersect` / `Difference`), bounds trimming, `SkRegion` conversion, mask
//! expansion, and all `SkAAClipBlitter` scanline blending/merging routines.

use crate::region::Region;
use crate::{irect_contains_rect, irect_make_offset, mul_div_255_round, ClipOp};
use skia_rs_geometry::IRect;

const MAX_INT32: i32 = 0x7FFF_FFFF;

/// Matches `struct YOffset { int32_t fY; uint32_t fOffset; };` in `SkAAClip.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct YOffset {
    pub y: i32,
    pub offset: u32,
}

/// Safe Rust representation of `SkAAClip`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AAClip {
    pub bounds: IRect,
    pub yoffsets: Vec<YOffset>,
    pub data: Vec<u8>,
}

/// Computes the RLE row byte size for a uniform row of `width` pixels.
/// Matches `SkAAClip::RunHead::ComputeRowSizeForWidth`.
#[inline]
pub fn compute_row_size_for_width(mut width: i32) -> usize {
    let mut segments = 0usize;
    while width > 0 {
        segments += 1;
        width -= width.min(255);
    }
    segments * 2
}

/// Appends `[count, alpha]` RLE segments (splitting `count` into chunks <= 255).
#[inline]
pub fn append_run(data: &mut Vec<u8>, alpha: u8, mut count: i32) {
    debug_assert!(count >= 0);
    while count > 0 {
        let n = count.min(255);
        data.push(n as u8);
        data.push(alpha);
        count -= n;
    }
}

/// Validates an RLE row's byte length for a given `width`, returning the byte count if valid.
pub fn validate_row_length(row: &[u8], mut width: i32) -> Option<usize> {
    let mut pos = 0usize;
    while width > 0 {
        if pos + 2 > row.len() {
            return None;
        }
        let n = row[pos] as i32;
        if n <= 0 || n > width {
            return None;
        }
        pos += 2;
        width -= n;
    }
    if width == 0 {
        Some(pos)
    } else {
        None
    }
}

/// Validates an `AAClip`'s internal invariants (matches `SkAAClip::validate`).
pub fn validate_aa_clip_parts(bounds: IRect, yoffsets: &[YOffset], data: &[u8]) -> bool {
    if yoffsets.is_empty() {
        return bounds.is_empty();
    }
    if bounds.is_empty() {
        return false;
    }
    let last_y = bounds.height() - 1;
    let width = bounds.width();
    let mut prev_y: i32 = -1;
    let mut prev_offset: i64 = -1;

    for yoff in yoffsets {
        if prev_y >= yoff.y || yoff.y > last_y {
            return false;
        }
        prev_y = yoff.y;
        if prev_offset >= yoff.offset as i64 {
            return false;
        }
        prev_offset = yoff.offset as i64;
        let off = yoff.offset as usize;
        if off > data.len() {
            return false;
        }
        let Some(row_len) = validate_row_length(&data[off..], width) else {
            return false;
        };
        if off + row_len > data.len() {
            return false;
        }
    }
    yoffsets.last().map_or(false, |last| last.y == last_y)
}

/// Matches `SkAAClip::isRect()` (preserving Skia's exact `yoff.y == bounds.bottom - 1` check).
pub fn is_rect_parts(bounds: IRect, yoffsets: &[YOffset], data: &[u8]) -> bool {
    if yoffsets.len() != 1 {
        return false;
    }
    let yoff = yoffsets[0];
    if yoff.y != bounds.bottom - 1 {
        return false;
    }
    let mut pos = yoff.offset as usize;
    let mut width = bounds.width();
    while width > 0 {
        if pos + 2 > data.len() {
            return false;
        }
        if data[pos + 1] != 0xFF {
            return false;
        }
        let n = data[pos] as i32;
        width -= n;
        pos += 2;
    }
    true
}

/// Matches `SkAAClip::findRow(int y, int* lastYForRow)`.
/// Returns `Some((data_offset, last_y_for_row))` if `y` is in `[bounds.top, bounds.bottom)`.
pub fn find_row_offset(bounds: IRect, yoffsets: &[YOffset], y: i32) -> Option<(usize, i32)> {
    if yoffsets.is_empty() || y < bounds.top || y >= bounds.bottom {
        return None;
    }
    let rel_y = y - bounds.top;
    for yoff in yoffsets {
        if yoff.y >= rel_y {
            return Some((yoff.offset as usize, bounds.top + yoff.y));
        }
    }
    None
}

/// Matches `SkAAClip::findX(const uint8_t data[], int x, int* initialCount)`.
/// `row_data` starts at the beginning of the scanline's RLE data.
/// Returns `(byte_offset_within_row_data, initial_count)`.
pub fn find_x_offset(bounds: IRect, row_data: &[u8], x: i32) -> (usize, i32) {
    debug_assert!(x >= bounds.left && x < bounds.right);
    let mut rel_x = x - bounds.left;
    let mut pos = 0usize;
    loop {
        let n = row_data[pos] as i32;
        if rel_x < n {
            return (pos, n - rel_x);
        }
        pos += 2;
        rel_x -= n;
    }
}

/// Matches `SkAAClip::quickContains(int left, int top, int right, int bottom)`.
pub fn quick_contains_parts(
    bounds: IRect,
    yoffsets: &[YOffset],
    data: &[u8],
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> bool {
    if yoffsets.is_empty() {
        return false;
    }
    if !irect_contains_rect(bounds, IRect::make_ltrb(left, top, right, bottom)) {
        return false;
    }
    let Some((row_off, last_y)) = find_row_offset(bounds, yoffsets, top) else {
        return false;
    };
    if last_y < bottom {
        return false;
    }
    let row_slice = &data[row_off..];
    let (mut pos, mut count) = find_x_offset(bounds, row_slice, left);
    let mut rect_width = right - left;
    while row_slice[pos + 1] == 0xFF {
        if count >= rect_width {
            return true;
        }
        rect_width -= count;
        pos += 2;
        count = row_slice[pos] as i32;
    }
    false
}

// ---------------------------------------------------------------------------
// Trimming (`count_left_right_zeros`, `trim_row_left_right`, `trim_*`)
// ---------------------------------------------------------------------------

/// Matches `count_left_right_zeros` in `SkAAClip.cpp`.
pub fn count_left_right_zeros(row: &[u8], mut width: i32) -> (i32, i32) {
    let mut pos = 0usize;
    let mut zeros = 0i32;
    while width > 0 {
        if row[pos + 1] != 0 {
            break;
        }
        let n = row[pos] as i32;
        zeros += n;
        pos += 2;
        width -= n;
    }
    let left_z = zeros;
    if width == 0 {
        return (left_z, left_z);
    }

    zeros = 0;
    while width > 0 {
        let n = row[pos] as i32;
        if row[pos + 1] == 0 {
            zeros += n;
        } else {
            zeros = 0;
        }
        pos += 2;
        width -= n;
    }
    (left_z, zeros)
}

/// Matches `trim_row_left_right` in `SkAAClip.cpp`.
/// Modifies `row` in place and returns the number of leading bytes eliminated.
pub fn trim_row_left_right(row: &mut [u8], mut width: i32, mut left_z: i32, mut rite_z: i32) -> usize {
    let mut pos = 0usize;
    let mut trim = 0usize;
    while left_z > 0 {
        debug_assert_eq!(row[pos + 1], 0);
        let n = row[pos] as i32;
        width -= n;
        pos += 2;
        if n > left_z {
            row[pos - 2] = (n - left_z) as u8;
            break;
        }
        trim += 2;
        left_z -= n;
    }

    if rite_z > 0 {
        while width > 0 {
            let n = row[pos] as i32;
            width -= n;
            pos += 2;
        }
        while rite_z > 0 {
            pos -= 2;
            debug_assert_eq!(row[pos + 1], 0);
            let n = row[pos] as i32;
            if n > rite_z {
                row[pos] = (n - rite_z) as u8;
                break;
            }
            rite_z -= n;
        }
    }

    trim
}

/// Matches `row_is_all_zeros` in `SkAAClip.cpp`.
pub fn row_is_all_zeros(row: &[u8], mut width: i32) -> bool {
    debug_assert!(width > 0);
    let mut pos = 0usize;
    while width > 0 {
        if row[pos + 1] != 0 {
            return false;
        }
        let n = row[pos] as i32;
        width -= n;
        pos += 2;
    }
    true
}

/// Trims empty rows from top and bottom in-place (matches `SkAAClip::trimTopBottom`).
/// Returns `false` if the clip became empty (caller should clear to empty).
pub fn trim_top_bottom_in_place(
    bounds: &mut IRect,
    yoffsets: &mut [YOffset],
    data: &[u8],
) -> Option<usize> {
    let row_count = yoffsets.len();
    if row_count == 0 {
        return None;
    }
    let width = bounds.width();

    let mut skip = 0usize;
    while skip < row_count {
        let off = yoffsets[skip].offset as usize;
        if !row_is_all_zeros(&data[off..], width) {
            break;
        }
        skip += 1;
    }
    if skip == row_count {
        return None;
    }

    let mut cur_count = row_count;
    if skip > 0 {
        let dy = yoffsets[skip - 1].y + 1;
        for i in skip..cur_count {
            yoffsets[i].y -= dy;
        }
        yoffsets.copy_within(skip..cur_count, 0);
        bounds.top += dy;
        cur_count -= skip;
    }

    // Trim from bottom
    let mut end_idx = cur_count;
    loop {
        end_idx -= 1;
        let off = yoffsets[end_idx].offset as usize;
        if !row_is_all_zeros(&data[off..], width) {
            break;
        }
    }
    let tail_skip = cur_count - 1 - end_idx;
    if tail_skip > 0 {
        bounds.bottom = bounds.top + yoffsets[end_idx].y + 1;
        cur_count -= tail_skip;
    }

    Some(cur_count)
}

/// Trims empty columns from left and right in-place (matches `SkAAClip::trimLeftRight`).
/// Returns `false` if the clip became empty.
pub fn trim_left_right_in_place(
    bounds: &mut IRect,
    yoffsets: &mut [YOffset],
    data: &mut [u8],
) -> bool {
    if yoffsets.is_empty() {
        return false;
    }
    let width = bounds.width();
    let mut left_zeros = width;
    let mut rite_zeros = width;
    for yoff in yoffsets.iter() {
        let (l, r) = count_left_right_zeros(&data[yoff.offset as usize..], width);
        if l < left_zeros {
            left_zeros = l;
        }
        if r < rite_zeros {
            rite_zeros = r;
        }
        if (left_zeros | rite_zeros) == 0 {
            return true;
        }
    }
    if left_zeros == width {
        return false;
    }

    bounds.left += left_zeros;
    bounds.right -= rite_zeros;

    for yoff in yoffsets.iter_mut() {
        let off = yoff.offset as usize;
        let trim = trim_row_left_right(&mut data[off..], width, left_zeros, rite_zeros);
        yoff.offset += trim as u32;
    }
    true
}

// ---------------------------------------------------------------------------
// Internal iterators for `operateY` / `operateX` / `copyToMask`
// ---------------------------------------------------------------------------

struct RowIter<'a> {
    data: Option<&'a [u8]>,
    pos: usize,
    left: i32,
    right: i32,
    bounds_right: i32,
    done: bool,
    alpha: u8,
}

impl<'a> RowIter<'a> {
    fn new(row: Option<&'a [u8]>, bounds: IRect) -> Self {
        match row {
            Some(r) => Self {
                data: Some(r),
                pos: 0,
                left: bounds.left,
                right: bounds.left + r[0] as i32,
                bounds_right: bounds.right,
                done: false,
                alpha: r[1],
            },
            None => Self {
                data: None,
                pos: 0,
                left: bounds.left,
                right: MAX_INT32,
                bounds_right: bounds.right,
                done: true,
                alpha: 0,
            },
        }
    }

    #[inline]
    fn next(&mut self) {
        if !self.done {
            self.left = self.right;
            if self.right == self.bounds_right {
                self.done = true;
                self.right = MAX_INT32;
                self.alpha = 0;
            } else {
                self.pos += 2;
                let r = self.data.unwrap();
                self.right += r[self.pos] as i32;
                self.alpha = r[self.pos + 1];
            }
        }
    }
}

struct YIter<'a> {
    yoffsets: &'a [YOffset],
    data: &'a [u8],
    idx: usize,
    top: i32,
    bottom: i32,
    done: bool,
}

impl<'a> YIter<'a> {
    fn new(bounds: IRect, yoffsets: &'a [YOffset], data: &'a [u8]) -> Self {
        if yoffsets.is_empty() {
            Self {
                yoffsets,
                data,
                idx: 0,
                top: MAX_INT32,
                bottom: MAX_INT32,
                done: true,
            }
        } else {
            Self {
                yoffsets,
                data,
                idx: 0,
                top: bounds.top,
                bottom: bounds.top + yoffsets[0].y + 1,
                done: false,
            }
        }
    }

    #[inline]
    fn row_data(&self) -> Option<&'a [u8]> {
        if self.done {
            None
        } else {
            Some(&self.data[self.yoffsets[self.idx].offset as usize..])
        }
    }

    #[inline]
    fn next(&mut self) {
        if !self.done {
            let prev = self.yoffsets[self.idx];
            self.idx += 1;
            self.top = self.bottom;
            if self.idx >= self.yoffsets.len() {
                self.done = true;
                self.bottom = MAX_INT32;
            } else {
                let curr = self.yoffsets[self.idx];
                self.bottom += curr.y - prev.y;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `AAClipBuilder` (safe Rust port of `SkAAClip::Builder` + `Builder::Blitter`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct BuilderRow {
    y: i32,
    width: i32,
    data: Vec<u8>,
}

/// Safe Rust port of `SkAAClip::Builder` (including `Builder::Blitter` scanline ingestion).
#[derive(Clone, Debug)]
pub struct AAClipBuilder {
    bounds: IRect,
    rows: Vec<BuilderRow>,
    has_curr_row: bool,
    prev_y: i32,
    width: i32,
    min_y: i32,
    // Blitter tracking state
    blitter_last_y: i32,
    blitter_min_y: i32,
}

impl AAClipBuilder {
    pub fn new(bounds: IRect) -> Self {
        Self {
            bounds,
            rows: Vec::new(),
            has_curr_row: false,
            prev_y: -1,
            width: bounds.width(),
            min_y: bounds.top,
            blitter_last_y: -i32::MAX,
            blitter_min_y: i32::MAX,
        }
    }

    pub fn bounds(&self) -> IRect {
        self.bounds
    }

    fn flush_row_h(row: &mut BuilderRow, target_width: i32) {
        if row.width < target_width {
            append_run(&mut row.data, 0, target_width - row.width);
            row.width = target_width;
        }
    }

    fn flush_row(&mut self, ready_for_another: bool) {
        let count = self.rows.len();
        if count > 0 {
            Self::flush_row_h(&mut self.rows[count - 1], self.width);
        }
        if count > 1 {
            let same = self.rows[count - 2].data == self.rows[count - 1].data;
            if same {
                self.rows[count - 2].y = self.rows[count - 1].y;
                if ready_for_another {
                    self.rows[count - 1].data.clear();
                    self.rows[count - 1].width = 0;
                } else {
                    self.rows.pop();
                }
            } else if ready_for_another {
                self.rows.push(BuilderRow {
                    y: 0,
                    width: 0,
                    data: Vec::new(),
                });
            }
        } else if ready_for_another {
            self.rows.push(BuilderRow {
                y: 0,
                width: 0,
                data: Vec::new(),
            });
        }
    }

    /// Matches `SkAAClip::Builder::addRun`.
    pub fn add_run(&mut self, mut x: i32, mut y: i32, alpha: u8, count: i32) {
        debug_assert!(count > 0);
        x -= self.bounds.left;
        y -= self.bounds.top;

        if y != self.prev_y {
            debug_assert!(y > self.prev_y);
            self.prev_y = y;
            self.flush_row(true);
            let row = self.rows.last_mut().unwrap();
            row.y = y;
            row.width = 0;
            self.has_curr_row = true;
        }

        let row = self.rows.last_mut().unwrap();
        let gap = x - row.width;
        if gap > 0 {
            append_run(&mut row.data, 0, gap);
            row.width += gap;
        }
        append_run(&mut row.data, alpha, count);
        row.width += count;
    }

    /// Matches `SkAAClip::Builder::addColumn`.
    pub fn add_column(&mut self, x: i32, y: i32, alpha: u8, height: i32) {
        self.add_run(x, y, alpha, 1);
        let target_w = self.width;
        let row = self.rows.last_mut().unwrap();
        Self::flush_row_h(row, target_w);
        let rel_y = y - self.bounds.top;
        row.y = rel_y + height - 1;
    }

    /// Matches `SkAAClip::Builder::addRectRun`.
    pub fn add_rect_run(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.add_run(x, y, 0xFF, width);
        let target_w = self.width;
        let row = self.rows.last_mut().unwrap();
        Self::flush_row_h(row, target_w);
        let rel_y = y - self.bounds.top;
        row.y = rel_y + height - 1;
    }

    /// Matches `SkAAClip::Builder::addAntiRectRun`.
    pub fn add_anti_rect_run(
        &mut self,
        mut x: i32,
        y: i32,
        mut width: i32,
        height: i32,
        left_alpha: u8,
        right_alpha: u8,
    ) {
        if left_alpha == 0xFF {
            width += 1;
        } else if left_alpha > 0 {
            self.add_run(x, y, left_alpha, 1);
            x += 1;
        } else {
            x += 1;
        }
        if right_alpha == 0xFF {
            width += 1;
        }
        if width > 0 {
            self.add_run(x, y, 0xFF, width);
        }
        if right_alpha > 0 && right_alpha < 255 {
            self.add_run(x + width, y, right_alpha, 1);
        }

        if self.has_curr_row {
            let target_w = self.width;
            let row = self.rows.last_mut().unwrap();
            Self::flush_row_h(row, target_w);
            let rel_y = y - self.bounds.top;
            row.y = rel_y + height - 1;
        }
    }

    // --- Blitter methods (matching `SkAAClip::Builder::Blitter`) ---

    #[inline]
    fn record_min_y(&mut self, y: i32) {
        if y < self.blitter_min_y {
            self.blitter_min_y = y;
        }
    }

    #[inline]
    fn check_for_y_gap(&mut self, y: i32) {
        if self.blitter_last_y > -i32::MAX {
            let gap = y - self.blitter_last_y;
            if gap > 1 {
                self.add_run(self.bounds.left, y - 1, 0, self.bounds.width());
            }
        }
        self.blitter_last_y = y;
    }

    pub fn blit_h(&mut self, x: i32, y: i32, width: i32) {
        self.record_min_y(y);
        self.check_for_y_gap(y);
        self.add_run(x, y, 0xFF, width);
    }

    pub fn blit_v(&mut self, x: i32, y: i32, height: i32, alpha: u8) {
        if height == 1 {
            let alphas = [alpha, 0];
            let runs = [1i16, 0];
            self.blit_anti_h(x, y, &alphas, &runs);
        } else {
            self.record_min_y(y);
            self.add_column(x, y, alpha, height);
            self.blitter_last_y = y + height - 1;
        }
    }

    pub fn blit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.record_min_y(y);
        self.check_for_y_gap(y);
        self.add_rect_run(x, y, width, height);
        self.blitter_last_y = y + height - 1;
    }

    pub fn blit_anti_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        left_alpha: u8,
        right_alpha: u8,
    ) {
        self.record_min_y(y);
        self.check_for_y_gap(y);
        self.add_anti_rect_run(x, y, width, height, left_alpha, right_alpha);
        self.blitter_last_y = y + height - 1;
    }

    pub fn blit_anti_h(&mut self, mut x: i32, y: i32, alphas: &[u8], runs: &[i16]) {
        self.record_min_y(y);
        self.check_for_y_gap(y);
        let left_bound = self.bounds.left;
        let right_bound = self.bounds.right;
        let mut idx = 0usize;
        loop {
            let count = runs[idx] as i32;
            if count <= 0 {
                return;
            }
            let alpha = alphas[idx];
            let mut local_x = x;
            let mut local_count = count;
            if x < left_bound {
                let gap = left_bound - x;
                local_x += gap;
                local_count -= gap;
            }
            let right = x + count;
            if right > right_bound {
                local_count -= right - right_bound;
            }
            if local_count > 0 {
                self.add_run(local_x, y, alpha, local_count);
            }
            idx += count as usize;
            x += count;
        }
    }

    pub fn finish_blitter(&mut self) {
        if self.blitter_min_y < i32::MAX {
            self.min_y = self.blitter_min_y;
        }
    }

    // --- Boolean op sweep (`operateX`, `operateY`) ---

    fn operate_x(
        &mut self,
        last_y: i32,
        iter_a: &mut RowIter<'_>,
        iter_b: &mut RowIter<'_>,
        is_diff: bool,
    ) {
        let mut left_a = iter_a.left;
        let mut rite_a = iter_a.right;
        let mut left_b = iter_b.left;
        let mut rite_b = iter_b.right;
        let mut prev_rite = self.bounds.left;

        loop {
            let mut alpha_a: u8 = 0;
            let mut alpha_b: u8 = 0;
            let left: i32;
            let mut rite: i32;

            if left_a < left_b {
                left = left_a;
                alpha_a = iter_a.alpha;
                if rite_a <= left_b {
                    rite = rite_a;
                } else {
                    rite = left_b;
                    left_a = left_b;
                }
            } else if left_b < left_a {
                left = left_b;
                alpha_b = iter_b.alpha;
                if rite_b <= left_a {
                    rite = rite_b;
                } else {
                    rite = left_a;
                    left_b = left_a;
                }
            } else {
                left = left_a;
                rite = rite_a.min(rite_b);
                left_a = rite;
                left_b = rite;
                alpha_a = iter_a.alpha;
                alpha_b = iter_b.alpha;
            }

            if left >= self.bounds.right {
                break;
            }
            if rite > self.bounds.right {
                rite = self.bounds.right;
            }

            if left >= self.bounds.left {
                let merged = if is_diff {
                    mul_div_255_round(alpha_a as u32, (0xFF - alpha_b) as u32)
                } else {
                    mul_div_255_round(alpha_a as u32, alpha_b as u32)
                };
                self.add_run(left, last_y, merged, rite - left);
                prev_rite = rite;
            }

            if rite == rite_a {
                iter_a.next();
                left_a = iter_a.left;
                rite_a = iter_a.right;
            }
            if rite == rite_b {
                iter_b.next();
                left_b = iter_b.left;
                rite_b = iter_b.right;
            }

            if iter_a.done && iter_b.done {
                break;
            }
        }

        if prev_rite < self.bounds.right {
            self.add_run(prev_rite, last_y, 0, self.bounds.right - prev_rite);
        }
    }

    pub fn operate_y(
        &mut self,
        a_bounds: IRect,
        a_yoffsets: &[YOffset],
        a_data: &[u8],
        b_bounds: IRect,
        b_yoffsets: &[YOffset],
        b_data: &[u8],
        op: ClipOp,
    ) {
        let is_diff = op == ClipOp::Difference;
        let mut iter_a = YIter::new(a_bounds, a_yoffsets, a_data);
        let mut iter_b = YIter::new(b_bounds, b_yoffsets, b_data);
        if iter_a.done || iter_b.done {
            return;
        }

        let mut top_a = iter_a.top;
        let mut bot_a = iter_a.bottom;
        let mut top_b = iter_b.top;
        let mut bot_b = iter_b.bottom;

        loop {
            let mut row_a: Option<&[u8]> = None;
            let mut row_b: Option<&[u8]> = None;
            let top: i32;
            let mut bot: i32;

            if top_a < top_b {
                top = top_a;
                row_a = iter_a.row_data();
                if bot_a <= top_b {
                    bot = bot_a;
                } else {
                    bot = top_b;
                    top_a = top_b;
                }
            } else if top_b < top_a {
                top = top_b;
                row_b = iter_b.row_data();
                if bot_b <= top_a {
                    bot = bot_b;
                } else {
                    bot = top_a;
                    top_b = top_a;
                }
            } else {
                top = top_a;
                bot = bot_a.min(bot_b);
                top_a = bot;
                top_b = bot;
                row_a = iter_a.row_data();
                row_b = iter_b.row_data();
            }

            if top >= self.bounds.bottom {
                break;
            }
            if bot > self.bounds.bottom {
                bot = self.bounds.bottom;
            }

            if row_a.is_none() && row_b.is_none() {
                self.add_run(self.bounds.left, bot - 1, 0, self.bounds.width());
            } else if top >= self.bounds.top {
                let mut riter_a = RowIter::new(row_a, if row_a.is_some() { a_bounds } else { self.bounds });
                let mut riter_b = RowIter::new(row_b, if row_b.is_some() { b_bounds } else { self.bounds });
                self.operate_x(bot - 1, &mut riter_a, &mut riter_b, is_diff);
            }

            if bot == bot_a {
                iter_a.next();
                top_a = bot_a;
                bot_a = iter_a.bottom;
            }
            if bot == bot_b {
                iter_b.next();
                top_b = bot_b;
                bot_b = iter_b.bottom;
            }

            if iter_a.done && iter_b.done {
                break;
            }
        }
    }

    /// Finalizes the builder, packs `YOffset` and RLE `data`, and trims bounds.
    /// Matches `SkAAClip::Builder::finish`.
    pub fn finish(mut self) -> AAClip {
        self.flush_row(false);

        let data_size: usize = self.rows.iter().map(|r| r.data.len()).sum();
        if data_size == 0 {
            return AAClip::default();
        }

        let adjust_y = self.min_y - self.bounds.top;
        let mut bounds = self.bounds;
        bounds.top = self.min_y;

        let mut yoffsets = Vec::with_capacity(self.rows.len());
        let mut data = Vec::with_capacity(data_size);

        for row in &self.rows {
            yoffsets.push(YOffset {
                y: row.y - adjust_y,
                offset: data.len() as u32,
            });
            data.extend_from_slice(&row.data);
        }

        let mut clip = AAClip {
            bounds,
            yoffsets,
            data,
        };
        clip.trim_bounds();
        clip
    }
}

// ---------------------------------------------------------------------------
// High-level `AAClip` methods
// ---------------------------------------------------------------------------

impl AAClip {
    pub fn new_empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.yoffsets.is_empty()
    }

    pub fn bounds(&self) -> IRect {
        self.bounds
    }

    pub fn is_rect(&self) -> bool {
        is_rect_parts(self.bounds, &self.yoffsets, &self.data)
    }

    pub fn set_empty(&mut self) -> bool {
        self.bounds = IRect::make_empty();
        self.yoffsets.clear();
        self.data.clear();
        false
    }

    pub fn set_rect(&mut self, bounds: IRect) -> bool {
        if bounds.is_empty() {
            return self.set_empty();
        }
        let width = bounds.width();
        let row_size = compute_row_size_for_width(width);
        let mut data = Vec::with_capacity(row_size);
        append_run(&mut data, 0xFF, width);
        self.bounds = bounds;
        self.yoffsets = vec![YOffset {
            y: bounds.height() - 1,
            offset: 0,
        }];
        self.data = data;
        true
    }

    /// Converts a `Region` (or sequence of Y->X sorted rects with bounds) into an `AAClip`.
    /// Matches `SkAAClip::setRegion`.
    pub fn set_region(&mut self, rgn: &Region) -> bool {
        if rgn.is_empty() {
            return self.set_empty();
        }
        if rgn.is_rect() {
            return self.set_rect(rgn.bounds());
        }
        *self = build_from_region_rects(rgn.bounds(), rgn.iter());
        true
    }

    pub fn trim_bounds(&mut self) -> bool {
        if self.is_empty() {
            return false;
        }
        let last_y = self.yoffsets.last().unwrap().y;
        self.bounds.bottom = self.bounds.top + last_y + 1;

        let Some(new_len) =
            trim_top_bottom_in_place(&mut self.bounds, &mut self.yoffsets, &self.data)
        else {
            return self.set_empty();
        };
        self.yoffsets.truncate(new_len);

        if !trim_left_right_in_place(&mut self.bounds, &mut self.yoffsets, &mut self.data) {
            return self.set_empty();
        }
        true
    }

    pub fn op_aa_clip(&mut self, other: &Self, op: ClipOp) -> bool {
        if self.is_empty() {
            return false;
        }
        let mut bounds = self.bounds;
        match op {
            ClipOp::Difference => {
                if other.is_empty() || !IRect::intersects(self.bounds, other.bounds) {
                    return true;
                }
            }
            ClipOp::Intersect => {
                if other.is_empty() || !bounds.intersect_pair(self.bounds, other.bounds) {
                    return self.set_empty();
                }
            }
        }
        let mut builder = AAClipBuilder::new(bounds);
        builder.operate_y(
            self.bounds,
            &self.yoffsets,
            &self.data,
            other.bounds,
            &other.yoffsets,
            &other.data,
            op,
        );
        *self = builder.finish();
        !self.is_empty()
    }

    pub fn op_irect(&mut self, rect: IRect, op: ClipOp) -> bool {
        let mut pixel_bounds = IRect::make_empty();
        if !pixel_bounds.intersect_pair(self.bounds, rect) {
            return match op {
                ClipOp::Difference => !self.is_empty(),
                ClipOp::Intersect => self.set_empty(),
            };
        }
        if pixel_bounds == self.bounds {
            return match op {
                ClipOp::Difference => self.set_empty(),
                ClipOp::Intersect => !self.is_empty(),
            };
        }
        if op == ClipOp::Intersect && self.quick_contains_rect(pixel_bounds) {
            return self.set_rect(pixel_bounds);
        }
        let mut tmp = AAClip::new_empty();
        tmp.set_rect(rect);
        self.op_aa_clip(&tmp, op)
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Self {
        if self.is_empty() {
            return Self::new_empty();
        }
        let mut out = self.clone();
        out.bounds = irect_make_offset(out.bounds, dx, dy);
        out
    }

    pub fn quick_contains(&self, left: i32, top: i32, right: i32, bottom: i32) -> bool {
        quick_contains_parts(self.bounds, &self.yoffsets, &self.data, left, top, right, bottom)
    }

    pub fn quick_contains_rect(&self, r: IRect) -> bool {
        self.quick_contains(r.left, r.top, r.right, r.bottom)
    }

    pub fn copy_to_mask(&self) -> Vec<u8> {
        if self.is_empty() {
            return Vec::new();
        }
        let width = self.bounds.width() as usize;
        let height = self.bounds.height() as usize;
        let mut out = vec![0u8; width * height];
        expand_clip_to_mask(self.bounds, &self.yoffsets, &self.data, &mut out, width);
        out
    }

    pub fn is_valid(&self) -> bool {
        validate_aa_clip_parts(self.bounds, &self.yoffsets, &self.data)
    }
}

/// Builds an `AAClip` from a complex region's bounds and Y->X sorted rectangles.
/// Matches `SkAAClip::setRegion`.
pub fn build_from_region_rects<I: Iterator<Item = IRect>>(bounds: IRect, rects: I) -> AAClip {
    let offset_x = bounds.left;
    let offset_y = bounds.top;
    let total_w = bounds.width();

    let mut y_array: Vec<YOffset> = Vec::new();
    let mut x_array: Vec<u8> = Vec::new();
    let mut prev_right = 0i32;
    let mut prev_bot = 0i32;
    let mut has_curr_y = false;

    for r in rects {
        let bot = r.bottom - offset_y;
        if bot > prev_bot {
            if has_curr_y {
                append_run(&mut x_array, 0, total_w - prev_right);
            }
            let top = r.top - offset_y;
            if top > prev_bot {
                y_array.push(YOffset {
                    y: top - 1,
                    offset: x_array.len() as u32,
                });
                append_run(&mut x_array, 0, total_w);
            }
            y_array.push(YOffset {
                y: bot - 1,
                offset: x_array.len() as u32,
            });
            has_curr_y = true;
            prev_right = 0;
            prev_bot = bot;
        }
        let x = r.left - offset_x;
        append_run(&mut x_array, 0, x - prev_right);
        let w = r.right - r.left;
        append_run(&mut x_array, 0xFF, w);
        prev_right = x + w;
    }

    if has_curr_y {
        append_run(&mut x_array, 0, total_w - prev_right);
    }

    AAClip {
        bounds,
        yoffsets: y_array,
        data: x_array,
    }
}

/// Expands an `AAClip` into a contiguous A8 mask buffer (matches `SkAAClip::copyToMask`).
pub fn expand_clip_to_mask(
    bounds: IRect,
    yoffsets: &[YOffset],
    data: &[u8],
    dst: &mut [u8],
    row_bytes: usize,
) {
    if yoffsets.is_empty() {
        return;
    }
    let width = bounds.width();
    let mut iter = YIter::new(bounds, yoffsets, data);
    let mut y = bounds.top;
    let mut dst_row_start = 0usize;

    while !iter.done {
        let row = iter.row_data().unwrap();
        let bottom = iter.bottom;
        loop {
            expand_row_to_mask(&mut dst[dst_row_start..dst_row_start + width as usize], row, width);
            dst_row_start += row_bytes;
            y += 1;
            if y >= bottom {
                break;
            }
        }
        iter.next();
    }
}

fn expand_row_to_mask(dst: &mut [u8], row: &[u8], mut width: i32) {
    let mut dst_pos = 0usize;
    let mut row_pos = 0usize;
    while width > 0 {
        let n = row[row_pos] as usize;
        let alpha = row[row_pos + 1];
        dst[dst_pos..dst_pos + n].fill(alpha);
        dst_pos += n;
        row_pos += 2;
        width -= n as i32;
    }
}

// ---------------------------------------------------------------------------
// `SkAAClipBlitter` safe Rust scanline algorithms
// ---------------------------------------------------------------------------

/// Matches `expandToRuns` in `SkAAClip.cpp`.
pub fn expand_to_runs(
    row: &[u8],
    initial_count: i32,
    mut width: i32,
    runs: &mut [i16],
    aa: &mut [u8],
) {
    let mut n = initial_count;
    let mut row_pos = 0usize;
    let mut out_pos = 0usize;
    loop {
        if n > width {
            n = width;
        }
        debug_assert!(n > 0);
        runs[out_pos] = n as i16;
        aa[out_pos] = row[row_pos + 1];
        out_pos += n as usize;
        row_pos += 2;
        width -= n;
        if width == 0 {
            break;
        }
        n = row[row_pos] as i32;
    }
    runs[out_pos] = 0; // sentinel
}

/// Matches `merge()` in `SkAAClip.cpp` (`SkAAClipBlitter::blitAntiH`).
pub fn merge_anti_h(
    row: &[u8],
    mut row_n: i32,
    src_aa: &[u8],
    src_runs: &[i16],
    dst_aa: &mut [u8],
    dst_runs: &mut [i16],
) {
    let mut src_n = src_runs[0] as i32;
    if src_n == 0 {
        dst_runs[0] = 0;
        return;
    }

    let mut row_pos = 0usize;
    let mut src_pos = 0usize;
    let mut dst_pos = 0usize;

    loop {
        debug_assert!(row_n > 0 && src_n > 0);
        let new_alpha = mul_div_255_round(src_aa[src_pos] as u32, row[row_pos + 1] as u32);
        let min_n = src_n.min(row_n);
        dst_runs[dst_pos] = min_n as i16;
        dst_aa[dst_pos] = new_alpha;
        dst_pos += min_n as usize;

        src_n -= min_n;
        if src_n == 0 {
            let adv = src_runs[src_pos] as usize;
            src_pos += adv;
            src_n = src_runs[src_pos] as i32;
            if src_n == 0 {
                break;
            }
        }
        row_n -= min_n;
        if row_n == 0 {
            row_pos += 2;
            row_n = row[row_pos] as i32;
        }
    }
    dst_runs[dst_pos] = 0;
}

/// Matches `mergeT<uint8_t>` in `SkAAClip.cpp` (`kA8_Format` / `k3D_Format`).
pub fn merge_mask_row_u8(src: &[u8], row: &[u8], mut row_n: i32, dst: &mut [u8]) {
    let mut src_n = src.len() as i32;
    let mut src_pos = 0usize;
    let mut row_pos = 0usize;
    loop {
        let n = row_n.min(src_n) as usize;
        let row_a = row[row_pos + 1];
        if row_a == 0xFF {
            dst[src_pos..src_pos + n].copy_from_slice(&src[src_pos..src_pos + n]);
        } else if row_a == 0 {
            dst[src_pos..src_pos + n].fill(0);
        } else {
            let ra = row_a as u32;
            for i in 0..n {
                dst[src_pos + i] = mul_div_255_round(src[src_pos + i] as u32, ra);
            }
        }
        src_n -= n as i32;
        if src_n == 0 {
            break;
        }
        src_pos += n;
        row_pos += 2;
        row_n = row[row_pos] as i32;
    }
}

/// Matches `mergeOne(uint16_t, unsigned)` for RGB565 (`SK_R16_BITS=5, G=6, B=5`).
#[inline]
fn merge_one_rgb16(value: u16, alpha: u32) -> u16 {
    let r = ((value >> 11) & 0x1F) as u32;
    let g = ((value >> 5) & 0x3F) as u32;
    let b = (value & 0x1F) as u32;
    let nr = mul_div_255_round(r, alpha) as u16;
    let ng = mul_div_255_round(g, alpha) as u16;
    let nb = mul_div_255_round(b, alpha) as u16;
    (nr << 11) | (ng << 5) | nb
}

/// Matches `mergeT<uint16_t>` in `SkAAClip.cpp` (`kLCD16_Format`).
pub fn merge_mask_row_u16(src: &[u16], row: &[u8], mut row_n: i32, dst: &mut [u16]) {
    let mut src_n = src.len() as i32;
    let mut src_pos = 0usize;
    let mut row_pos = 0usize;
    loop {
        let n = row_n.min(src_n) as usize;
        let row_a = row[row_pos + 1];
        if row_a == 0xFF {
            dst[src_pos..src_pos + n].copy_from_slice(&src[src_pos..src_pos + n]);
        } else if row_a == 0 {
            dst[src_pos..src_pos + n].fill(0);
        } else {
            let ra = row_a as u32;
            for i in 0..n {
                dst[src_pos + i] = merge_one_rgb16(src[src_pos + i], ra);
            }
        }
        src_n -= n as i32;
        if src_n == 0 {
            break;
        }
        src_pos += n;
        row_pos += 2;
        row_n = row[row_pos] as i32;
    }
}

/// Matches `upscaleBW2A8` in `SkAAClip.cpp`.
pub fn upscale_bw_to_a8(
    src: &[u8],
    src_rb: usize,
    dst: &mut [u8],
    dst_rb: usize,
    width: usize,
    height: usize,
) {
    let whole_bytes = width >> 3;
    let left_over_bits = width & 7;

    for y in 0..height {
        let s = &src[y * src_rb..];
        let d = &mut dst[y * dst_rb..];
        for i in 0..whole_bytes {
            let b = s[i];
            let base = i * 8;
            d[base] = if (b & (1 << 7)) != 0 { 0xFF } else { 0 };
            d[base + 1] = if (b & (1 << 6)) != 0 { 0xFF } else { 0 };
            d[base + 2] = if (b & (1 << 5)) != 0 { 0xFF } else { 0 };
            d[base + 3] = if (b & (1 << 4)) != 0 { 0xFF } else { 0 };
            d[base + 4] = if (b & (1 << 3)) != 0 { 0xFF } else { 0 };
            d[base + 5] = if (b & (1 << 2)) != 0 { 0xFF } else { 0 };
            d[base + 6] = if (b & (1 << 1)) != 0 { 0xFF } else { 0 };
            d[base + 7] = if (b & 1) != 0 { 0xFF } else { 0 };
        }
        if left_over_bits > 0 {
            let mut b = s[whole_bytes];
            let base = whole_bytes * 8;
            for x in 0..left_over_bits {
                d[base + x] = if (b & 0x80) != 0 { 0xFF } else { 0 };
                b <<= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_and_ops() {
        let mut a = AAClip::new_empty();
        assert!(a.set_rect(IRect::make_ltrb(0, 0, 20, 20)));
        assert!(a.is_valid());
        assert!(a.is_rect());
        assert_eq!(a.copy_to_mask(), vec![0xFF; 400]);

        // Difference out a center hole
        assert!(a.op_irect(IRect::make_ltrb(5, 5, 15, 15), ClipOp::Difference));
        assert!(a.is_valid());
        assert!(!a.is_rect());
        let mask = a.copy_to_mask();
        assert_eq!(mask[0], 0xFF);
        assert_eq!(mask[10 * 20 + 10], 0x00);

        // Intersect with left strip -> becomes a clean rect again!
        assert!(a.op_irect(IRect::make_ltrb(0, 0, 5, 20), ClipOp::Intersect));
        assert!(a.is_valid());
        assert!(a.is_rect());
        assert_eq!(a.bounds(), IRect::make_ltrb(0, 0, 5, 20));
    }

    #[test]
    fn builder_partial_alpha_and_trimming() {
        let mut b = AAClipBuilder::new(IRect::make_ltrb(0, 0, 10, 10));
        // Empty rows at y=0, 1; partial alpha at y=2..4, x=3..7
        b.blit_anti_rect(3, 2, 2, 3, 128, 64);
        b.finish_blitter();
        let clip = b.finish();
        assert!(clip.is_valid());
        assert_eq!(clip.bounds(), IRect::make_ltrb(3, 2, 7, 5));
        let mask = clip.copy_to_mask();
        assert_eq!(mask.len(), 4 * 3);
        for row in mask.chunks_exact(4) {
            assert_eq!(row, &[128, 255, 255, 64]);
        }
    }

    #[test]
    fn set_region_complex() {
        let mut rgn = Region::Empty;
        rgn.set_rects(&[
            IRect::make_ltrb(0, 0, 10, 10),
            IRect::make_ltrb(20, 0, 30, 10),
            IRect::make_ltrb(5, 15, 25, 20),
        ]);
        let mut aa = AAClip::new_empty();
        assert!(aa.set_region(&rgn));
        assert!(aa.is_valid());
        assert_eq!(aa.bounds(), IRect::make_ltrb(0, 0, 30, 20));
        let mask = aa.copy_to_mask();
        // Inside first rect
        assert_eq!(mask[5 * 30 + 5], 0xFF);
        // Horizontal gap at y=5, x=15
        assert_eq!(mask[5 * 30 + 15], 0x00);
        // Vertical gap at y=12, x=10
        assert_eq!(mask[12 * 30 + 10], 0x00);
        // Bottom rect at y=17, x=10
        assert_eq!(mask[17 * 30 + 10], 0xFF);
    }

    #[test]
    fn blitter_expand_merge_and_upscale() {
        // Row: 3px @ 128, 4px @ 255, 3px @ 0
        let row = [3u8, 128, 4, 255, 3, 0];
        let mut runs = [0i16; 11];
        let mut aa = [0u8; 10];
        expand_to_runs(&row, 3, 10, &mut runs, &mut aa);
        assert_eq!(runs[0], 3);
        assert_eq!(aa[0], 128);
        assert_eq!(runs[3], 4);
        assert_eq!(aa[3], 255);
        assert_eq!(runs[7], 3);
        assert_eq!(aa[7], 0);
        assert_eq!(runs[10], 0);

        // merge_anti_h with 5px @ 200, 5px @ 100
        let mut src_runs = [0i16; 11];
        let mut src_aa = [0u8; 10];
        src_runs[0] = 5;
        src_aa[0] = 200;
        src_runs[5] = 5;
        src_aa[5] = 100;
        let mut dst_runs = [0i16; 11];
        let mut dst_aa = [0u8; 10];
        merge_anti_h(&row, 3, &src_aa, &src_runs, &mut dst_aa, &mut dst_runs);
        assert_eq!(dst_runs[0], 3);
        assert_eq!(dst_aa[0], mul_div_255_round(200, 128));
        assert_eq!(dst_runs[3], 2);
        assert_eq!(dst_aa[3], 200);
        assert_eq!(dst_runs[5], 2);
        assert_eq!(dst_aa[5], 100);
        assert_eq!(dst_runs[7], 3);
        assert_eq!(dst_aa[7], 0);
        assert_eq!(dst_runs[10], 0);

        // upscale_bw_to_a8
        let bw = [0b10110000u8, 0b10000000u8];
        let mut a8 = [0u8; 10];
        upscale_bw_to_a8(&bw, 2, &mut a8, 10, 10, 1);
        assert_eq!(a8, [255, 0, 255, 255, 0, 0, 0, 0, 255, 0]);
    }
}
