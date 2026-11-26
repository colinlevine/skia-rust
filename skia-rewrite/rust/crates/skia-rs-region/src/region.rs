#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkRegion.cpp`.
//!
//! Implements Skia's exact RLE region representation:
//! ```text
//! [Top-Y]
//!   [Bottom-Y, IntervalCount (N), Left-1, Right-1, ..., Left-N, Right-N, X-Sentinel]
//!   ...
//! [Y-Sentinel]
//! ```
//! where `RUN_TYPE_SENTINEL = 0x7FFFFFFF`.

use crate::{irect_contains_no_empty_check, irect_contains_rect, irect_contains_xy, irect_make_offset};
use skia_rs_geometry::IRect;

pub const RUN_TYPE_SENTINEL: i32 = 0x7FFF_FFFF;
pub const RECT_REGION_RUNS: usize = 7;

/// Matches `SkRegion::Op`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionOp {
    Difference = 0,
    Intersect = 1,
    Union = 2,
    Xor = 3,
    ReverseDifference = 4,
    Replace = 5,
}

impl RegionOp {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Difference),
            1 => Some(Self::Intersect),
            2 => Some(Self::Union),
            3 => Some(Self::Xor),
            4 => Some(Self::ReverseDifference),
            5 => Some(Self::Replace),
            _ => None,
        }
    }
}

/// Safe Rust representation of `SkRegion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Region {
    Empty,
    Rect(IRect),
    Complex {
        bounds: IRect,
        y_span_count: i32,
        interval_count: i32,
        runs: Vec<i32>,
    },
}

impl Default for Region {
    fn default() -> Self {
        Self::Empty
    }
}

/// Metadata computed from a valid complex run array.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RunBoundsInfo {
    pub bounds: IRect,
    pub y_span_count: i32,
    pub interval_count: i32,
}

/// Outcome of normalizing raw runs (`setRuns`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizedRuns {
    Empty,
    Rect(IRect),
    Complex {
        bounds: IRect,
        y_span_count: i32,
        interval_count: i32,
        runs: Vec<i32>,
    },
}

/// Matches `SkRegion::BuildRectRuns`.
#[inline]
pub fn build_rect_runs(bounds: IRect) -> [i32; RECT_REGION_RUNS] {
    [
        bounds.top,
        bounds.bottom,
        1,
        bounds.left,
        bounds.right,
        RUN_TYPE_SENTINEL,
        RUN_TYPE_SENTINEL,
    ]
}

/// Matches `SkRegion::RunsAreARect`.
#[inline]
pub fn runs_are_a_rect(runs: &[i32]) -> Option<IRect> {
    if runs.len() == RECT_REGION_RUNS {
        Some(IRect::make_ltrb(runs[3], runs[0], runs[4], runs[1]))
    } else {
        None
    }
}

/// Matches `SkRegion::RunHead::SkipEntireScanline`.
/// Given `offset` pointing at a scanline's `Bottom-Y`, returns the offset of the next scanline.
#[inline]
pub fn skip_entire_scanline(runs: &[i32], offset: usize) -> usize {
    let intervals = runs[offset + 1] as usize;
    offset + 2 + intervals * 2 + 1
}

/// Matches `SkRegion::RunHead::findScanline(int y)`.
/// Returns the index in `runs` of the scanline (starting at its Bottom-Y) containing `y`.
/// Caller must ensure `y` is within the region's vertical bounds `[runs[0], bounds.bottom)`.
pub fn find_scanline_offset(runs: &[i32], y: i32) -> usize {
    let mut idx = 1; // skip top-Y
    loop {
        let bottom = runs[idx];
        debug_assert!(bottom < RUN_TYPE_SENTINEL);
        if y < bottom {
            return idx;
        }
        idx = skip_entire_scanline(runs, idx);
    }
}

/// Checks whether a complex region's `runs` contains `(x, y)` (assuming `bounds.contains(x, y)`).
pub fn complex_runs_contains_xy(runs: &[i32], x: i32, y: i32) -> bool {
    let mut idx = find_scanline_offset(runs, y) + 2; // skip Bottom and IntervalCount
    loop {
        let left = runs[idx];
        if x < left {
            return false;
        }
        if x < runs[idx + 1] {
            return true;
        }
        idx += 2;
    }
}

fn scanline_contains_range(runs: &[i32], scanline: usize, left: i32, right: i32) -> bool {
    let mut idx = scanline + 2; // skip Bottom and IntervalCount
    loop {
        let run_left = runs[idx];
        if left < run_left {
            return false;
        }
        if right <= runs[idx + 1] {
            return true;
        }
        idx += 2;
    }
}

/// Checks whether a complex region's `runs` completely contains `r` (assuming `bounds.contains(r)`).
pub fn complex_runs_contains_rect(runs: &[i32], r: IRect) -> bool {
    let mut scanline = find_scanline_offset(runs, r.top);
    loop {
        if !scanline_contains_range(runs, scanline, r.left, r.right) {
            return false;
        }
        if r.bottom <= runs[scanline] {
            return true;
        }
        scanline = skip_entire_scanline(runs, scanline);
    }
}

fn scanline_intersects_range(runs: &[i32], scanline: usize, left: i32, right: i32) -> bool {
    let mut idx = scanline + 2; // skip Bottom and IntervalCount
    loop {
        let run_left = runs[idx];
        if right <= run_left {
            return false;
        }
        if left < runs[idx + 1] {
            return true;
        }
        idx += 2;
    }
}

/// Checks whether a complex region's `runs` intersects `sect` (where `sect = bounds ∩ r` is non-empty).
pub fn complex_runs_intersects_rect(runs: &[i32], sect: IRect) -> bool {
    let mut scanline = find_scanline_offset(runs, sect.top);
    loop {
        if scanline_intersects_range(runs, scanline, sect.left, sect.right) {
            return true;
        }
        if sect.bottom <= runs[scanline] {
            return false;
        }
        scanline = skip_entire_scanline(runs, scanline);
    }
}

/// Matches `pin_offset_s32` in `SkRegion.cpp`.
#[inline]
pub fn pin_offset_s32(min: i32, max: i32, mut offset: i32) -> i32 {
    debug_assert!(min <= max);
    let lo = i32::MIN as i64;
    let hi = i32::MAX as i64;
    if (min as i64) + (offset as i64) < lo {
        offset = (lo - (min as i64)) as i32;
    }
    if (max as i64) + (offset as i64) > hi {
        offset = (hi - (max as i64)) as i32;
    }
    offset
}

/// Translates complex `src_runs` by `(dx, dy)` into `dst_runs` (which must have the same length).
/// Caller pins `(dx, dy)` via `pin_offset_s32` against bounds first.
pub fn translate_runs(src_runs: &[i32], dst_runs: &mut [i32], dx: i32, dy: i32) {
    debug_assert_eq!(src_runs.len(), dst_runs.len());
    let mut si = 0usize;
    let mut di = 0usize;

    dst_runs[di] = src_runs[si].wrapping_add(dy); // top
    si += 1;
    di += 1;

    loop {
        let bottom = src_runs[si];
        si += 1;
        if bottom == RUN_TYPE_SENTINEL {
            break;
        }
        dst_runs[di] = bottom.wrapping_add(dy);
        di += 1;
        dst_runs[di] = src_runs[si]; // intervalCount
        si += 1;
        di += 1;
        loop {
            let x = src_runs[si];
            si += 1;
            if x == RUN_TYPE_SENTINEL {
                break;
            }
            dst_runs[di] = x.wrapping_add(dx);
            di += 1;
            dst_runs[di] = src_runs[si].wrapping_add(dx);
            si += 1;
            di += 1;
        }
        dst_runs[di] = RUN_TYPE_SENTINEL;
        di += 1;
    }
    dst_runs[di] = RUN_TYPE_SENTINEL;
    di += 1;
    debug_assert_eq!(si, src_runs.len());
    debug_assert_eq!(di, dst_runs.len());
}

/// In-place version of `translate_runs` (avoids aliasing `&[i32]` and `&mut [i32]`).
pub fn translate_runs_in_place(runs: &mut [i32], dx: i32, dy: i32) {
    let mut i = 0usize;
    runs[i] = runs[i].wrapping_add(dy); // top
    i += 1;
    loop {
        let bottom = runs[i];
        if bottom == RUN_TYPE_SENTINEL {
            break;
        }
        runs[i] = bottom.wrapping_add(dy);
        i += 2; // bottom + intervalCount
        loop {
            let x = runs[i];
            if x == RUN_TYPE_SENTINEL {
                i += 1;
                break;
            }
            runs[i] = x.wrapping_add(dx);
            runs[i + 1] = runs[i + 1].wrapping_add(dx);
            i += 2;
        }
    }
}

/// Matches `SkRegion::RunHead::computeRunBounds`.
pub fn compute_run_bounds(runs: &[i32]) -> RunBoundsInfo {
    let mut idx = 0usize;
    let top = runs[idx];
    idx += 1;

    let mut bot: i32;
    let mut y_span_count: i32 = 0;
    let mut interval_count: i32 = 0;
    let mut left: i32 = i32::MAX;
    let mut right: i32 = i32::MIN;

    loop {
        bot = runs[idx];
        idx += 1;
        y_span_count += 1;

        let intervals = runs[idx];
        idx += 1;
        if intervals > 0 {
            let l = runs[idx];
            if left > l {
                left = l;
            }
            idx += (intervals as usize) * 2;
            let r = runs[idx - 1];
            if right < r {
                right = r;
            }
            interval_count += intervals;
        }
        debug_assert_eq!(runs[idx], RUN_TYPE_SENTINEL);
        idx += 1; // skip x-sentinel

        if runs[idx] >= RUN_TYPE_SENTINEL {
            break;
        }
    }

    RunBoundsInfo {
        bounds: IRect::make_ltrb(left, top, right, bot),
        y_span_count,
        interval_count,
    }
}

/// Matches `validate_run_count` in `SkRegion.cpp`.
pub fn validate_run_count(y_span_count: i32, interval_count: i32, run_count: i32) -> bool {
    if y_span_count < 1 || interval_count < 2 {
        return false;
    }
    let sum = (2_i32)
        .checked_add(y_span_count)
        .and_then(|s| s.checked_add(y_span_count))
        .and_then(|s| s.checked_add(y_span_count))
        .and_then(|s| s.checked_add(interval_count))
        .and_then(|s| s.checked_add(interval_count));
    sum == Some(run_count)
}

/// Matches `validate_run` in `SkRegion.cpp`.
/// Strictly validates untrusted run sequences without out-of-bounds reads.
pub fn validate_runs(
    runs: &[i32],
    given_bounds: IRect,
    mut y_span_count: i32,
    mut interval_count: i32,
) -> bool {
    let run_count = match i32::try_from(runs.len()) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if !validate_run_count(y_span_count, interval_count, run_count) {
        return false;
    }
    if runs.len() < RECT_REGION_RUNS {
        return false;
    }
    if runs[runs.len() - 1] != RUN_TYPE_SENTINEL || runs[runs.len() - 2] != RUN_TYPE_SENTINEL {
        return false;
    }

    let end = runs.len();
    let mut idx = 0usize;
    let mut bounds = IRect::make_empty();
    let mut rect = IRect::make_empty();
    let mut prev_was_empty = true;

    rect.top = runs[idx];
    idx += 1;
    if rect.top == RUN_TYPE_SENTINEL || rect.top != given_bounds.top {
        return false;
    }

    loop {
        y_span_count -= 1;
        if y_span_count < 0 {
            return false;
        }
        if idx + 2 > end {
            return false;
        }
        rect.bottom = runs[idx];
        idx += 1;
        if rect.bottom == RUN_TYPE_SENTINEL
            || rect.bottom > given_bounds.bottom
            || rect.bottom <= rect.top
        {
            return false;
        }

        let mut x_intervals = runs[idx];
        idx += 1;
        if x_intervals < 0 || x_intervals > interval_count {
            return false;
        }
        let needed = match (x_intervals as usize).checked_mul(2).and_then(|v| v.checked_add(1)) {
            Some(n) => n,
            None => return false,
        };
        if idx.checked_add(needed).map_or(true, |p| p > end) {
            return false;
        }

        if x_intervals == 0 {
            if prev_was_empty {
                return false;
            }
            prev_was_empty = true;
        } else {
            prev_was_empty = false;
        }
        interval_count -= x_intervals;

        let mut first_interval = true;
        let mut last_right = 0_i32;
        while x_intervals > 0 {
            x_intervals -= 1;
            rect.left = runs[idx];
            rect.right = runs[idx + 1];
            idx += 2;
            if rect.left == RUN_TYPE_SENTINEL
                || rect.right == RUN_TYPE_SENTINEL
                || rect.left >= rect.right
                || (!first_interval && rect.left <= last_right)
            {
                return false;
            }
            last_right = rect.right;
            first_interval = false;
            bounds.join(rect);
        }

        if runs[idx] != RUN_TYPE_SENTINEL {
            return false;
        }
        idx += 1;
        rect.top = rect.bottom;
        if idx >= end {
            return false;
        }
        if runs[idx] == RUN_TYPE_SENTINEL {
            break;
        }
    }

    idx += 1;
    if y_span_count != 0 || interval_count != 0 || given_bounds != bounds {
        return false;
    }
    debug_assert_eq!(idx, end);
    true
}

/// Trims leading/trailing empty spans in-place inside `runs`, matching `SkRegion::setRuns`.
/// Returns `NormalizedRuns`.
pub fn normalize_runs(runs: &mut [i32]) -> NormalizedRuns {
    let mut start = 0usize;
    let mut stop = runs.len();
    if stop <= 2 {
        return NormalizedRuns::Empty;
    }

    if stop > RECT_REGION_RUNS {
        // Leading empty span?
        if runs[start + 3] == RUN_TYPE_SENTINEL {
            let prev_bot = runs[start + 1];
            start += 3;
            runs[start] = prev_bot;
        }
        // Trailing empty span?
        if stop - start >= 5 && runs[stop - 5] == RUN_TYPE_SENTINEL {
            runs[stop - 4] = RUN_TYPE_SENTINEL;
            stop -= 3;
        }
    }

    let trimmed = &runs[start..stop];
    if let Some(rect) = runs_are_a_rect(trimmed) {
        if rect.is_empty()
            || rect.right == RUN_TYPE_SENTINEL
            || rect.bottom == RUN_TYPE_SENTINEL
        {
            return NormalizedRuns::Empty;
        }
        return NormalizedRuns::Rect(rect);
    }

    let info = compute_run_bounds(trimmed);
    if info.bounds.is_empty() {
        return NormalizedRuns::Empty;
    }

    NormalizedRuns::Complex {
        bounds: info.bounds,
        y_span_count: info.y_span_count,
        interval_count: info.interval_count,
        runs: trimmed.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Boolean operation engine (`spanRec`, `operate_on_span`, `RgnOper`, `operate`)
// ---------------------------------------------------------------------------

struct SpanRec<'a> {
    a_runs: &'a [i32],
    b_runs: &'a [i32],
    a_idx: usize,
    b_idx: usize,
    a_left: i32,
    a_rite: i32,
    b_left: i32,
    b_rite: i32,
    left: i32,
    rite: i32,
    inside: u8,
}

impl<'a> SpanRec<'a> {
    #[inline]
    fn read_pair(runs: &[i32], idx: &mut usize) -> (i32, i32) {
        let l = runs[*idx];
        *idx += 1;
        if l == RUN_TYPE_SENTINEL {
            (RUN_TYPE_SENTINEL, RUN_TYPE_SENTINEL)
        } else {
            let r = runs[*idx];
            *idx += 1;
            (l, r)
        }
    }

    fn init(a_runs: &'a [i32], mut a_idx: usize, b_runs: &'a [i32], mut b_idx: usize) -> Self {
        let (a_left, a_rite) = Self::read_pair(a_runs, &mut a_idx);
        let (b_left, b_rite) = Self::read_pair(b_runs, &mut b_idx);
        Self {
            a_runs,
            b_runs,
            a_idx,
            b_idx,
            a_left,
            a_rite,
            b_left,
            b_rite,
            left: 0,
            rite: 0,
            inside: 0,
        }
    }

    #[inline]
    fn done(&self) -> bool {
        self.a_left == RUN_TYPE_SENTINEL && self.b_left == RUN_TYPE_SENTINEL
    }

    #[inline]
    fn next(&mut self) {
        let mut a_flush = false;
        let mut b_flush = false;

        let mut a_left = self.a_left;
        let mut a_rite = self.a_rite;
        let mut b_left = self.b_left;
        let mut b_rite = self.b_rite;

        let inside: u8;
        let left: i32;
        let mut rite: i32 = 0;

        if a_left < b_left {
            inside = 1;
            left = a_left;
            if a_rite <= b_left {
                rite = a_rite;
                a_flush = true;
            } else {
                rite = b_left;
                a_left = b_left;
            }
        } else if b_left < a_left {
            inside = 2;
            left = b_left;
            if b_rite <= a_left {
                rite = b_rite;
                b_flush = true;
            } else {
                rite = a_left;
                b_left = a_left;
            }
        } else {
            inside = 3;
            left = a_left;
            if a_rite <= b_rite {
                rite = a_rite;
                b_left = a_rite;
                a_flush = true;
            }
            if b_rite <= a_rite {
                rite = b_rite;
                a_left = b_rite;
                b_flush = true;
            }
        }

        if a_flush {
            let (l, r) = Self::read_pair(self.a_runs, &mut self.a_idx);
            a_left = l;
            a_rite = r;
        }
        if b_flush {
            let (l, r) = Self::read_pair(self.b_runs, &mut self.b_idx);
            b_left = l;
            b_rite = r;
        }

        self.a_left = a_left;
        self.a_rite = a_rite;
        self.b_left = b_left;
        self.b_rite = b_rite;
        self.left = left;
        self.rite = rite;
        self.inside = inside;
    }
}

fn operate_on_span(
    a_runs: &[i32],
    a_idx: usize,
    b_runs: &[i32],
    b_idx: usize,
    dst: &mut Vec<i32>,
    dst_offset: usize,
    min: u8,
    max: u8,
) -> usize {
    dst.truncate(dst_offset);

    let mut rec = SpanRec::init(a_runs, a_idx, b_runs, b_idx);
    let mut first_interval = true;

    while !rec.done() {
        rec.next();
        let left = rec.left;
        let rite = rec.rite;

        if rec.inside.wrapping_sub(min) <= max.wrapping_sub(min) && left < rite {
            if first_interval || *dst.last().unwrap() < left {
                dst.push(left);
                dst.push(rite);
                first_interval = false;
            } else {
                *dst.last_mut().unwrap() = rite;
            }
        }
    }
    dst.push(RUN_TYPE_SENTINEL);
    dst.len()
}

const OP_MIN_MAX: [(u8, u8); 4] = [
    (1, 1), // Difference
    (3, 3), // Intersect
    (1, 3), // Union
    (1, 2), // Xor
];

struct RgnOper<'a> {
    min: u8,
    max: u8,
    array: &'a mut Vec<i32>,
    prev_dst: usize,
    prev_len: usize,
    top: i32,
}

impl<'a> RgnOper<'a> {
    fn new(top: i32, array: &'a mut Vec<i32>, op: RegionOp) -> Self {
        let (min, max) = OP_MIN_MAX[op as usize];
        array.clear();
        array.push(top); // slot 0: top
        Self {
            min,
            max,
            array,
            prev_dst: 1,
            prev_len: 0,
            top,
        }
    }

    fn add_span(&mut self, bottom: i32, a_runs: &[i32], a_idx: usize, b_runs: &[i32], b_idx: usize) {
        let start = self.prev_dst + self.prev_len + 2;
        if self.array.len() < start {
            self.array.resize(start, 0);
        }
        let stop = operate_on_span(a_runs, a_idx, b_runs, b_idx, self.array, start, self.min, self.max);
        let len = stop - start;
        debug_assert!(len >= 1 && (len & 1) == 1);
        debug_assert_eq!(self.array[stop - 1], RUN_TYPE_SENTINEL);

        let same_as_prev = self.prev_len == len
            && (len == 1
                || self.array[self.prev_dst..self.prev_dst + len - 1]
                    == self.array[start..start + len - 1]);

        if same_as_prev {
            self.array[self.prev_dst - 2] = bottom;
            self.array.truncate(self.prev_dst + self.prev_len);
        } else if len == 1 && self.prev_len == 0 {
            self.top = bottom;
            self.array.truncate(1);
        } else {
            self.array[start - 2] = bottom;
            self.array[start - 1] = (len >> 1) as i32;
            self.prev_dst = start;
            self.prev_len = len;
        }
    }

    fn flush(self) -> usize {
        self.array[0] = self.top;
        let end = self.prev_dst + self.prev_len;
        self.array.truncate(end);
        self.array.push(RUN_TYPE_SENTINEL);
        self.array.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.prev_len == 0
    }
}

#[inline]
fn skip_intervals_from_x(runs: &[i32], x_idx: usize) -> usize {
    let intervals = runs[x_idx - 1] as usize;
    x_idx + intervals * 2 + 1
}

/// Core boolean run-sweep engine matching `operate()` in `SkRegion.cpp`.
///
/// `op` must be one of `Difference`, `Intersect`, `Union`, `Xor`.
/// If `quick_exit` is true, returns `Ok(true)` immediately once any non-empty span is produced
/// (and `Ok(false)` if the result is empty), leaving `dst` unspecified.
/// If `quick_exit` is false, populates `dst` with the resulting raw runs (to be passed to
/// `normalize_runs`) and returns `true` iff `dst.len() > 2`.
pub fn operate_runs(
    a_runs: &[i32],
    b_runs: &[i32],
    op: RegionOp,
    quick_exit: bool,
    dst: &mut Vec<i32>,
) -> bool {
    const EMPTY_SCANLINE: [i32; 3] = [0, 0, RUN_TYPE_SENTINEL];
    const SENTINEL_IDX: usize = 2;

    let mut a_top = a_runs[0];
    let mut a_bot = a_runs[1];
    let mut a_idx = 3usize; // points to first interval (past top, bot, intervalCount)

    let mut b_top = b_runs[0];
    let mut b_bot = b_runs[1];
    let mut b_idx = 3usize;

    let mut oper = RgnOper::new(a_top.min(b_top), dst, op);
    let mut prev_bot = RUN_TYPE_SENTINEL;

    while a_bot < RUN_TYPE_SENTINEL || b_bot < RUN_TYPE_SENTINEL {
        let mut run0_slice: &[i32] = &EMPTY_SCANLINE;
        let mut run0_idx: usize = SENTINEL_IDX;
        let mut run1_slice: &[i32] = &EMPTY_SCANLINE;
        let mut run1_idx: usize = SENTINEL_IDX;
        let mut a_flush = false;
        let mut b_flush = false;

        let top: i32;
        let mut bot: i32 = 0;

        if a_top < b_top {
            top = a_top;
            run0_slice = a_runs;
            run0_idx = a_idx;
            if a_bot <= b_top {
                bot = a_bot;
                a_flush = true;
            } else {
                bot = b_top;
                a_top = b_top;
            }
        } else if b_top < a_top {
            top = b_top;
            run1_slice = b_runs;
            run1_idx = b_idx;
            if b_bot <= a_top {
                bot = b_bot;
                b_flush = true;
            } else {
                bot = a_top;
                b_top = a_top;
            }
        } else {
            top = a_top;
            run0_slice = a_runs;
            run0_idx = a_idx;
            run1_slice = b_runs;
            run1_idx = b_idx;
            if a_bot <= b_bot {
                bot = a_bot;
                b_top = a_bot;
                a_flush = true;
            }
            if b_bot <= a_bot {
                bot = b_bot;
                a_top = b_bot;
                b_flush = true;
            }
        }

        if top > prev_bot {
            oper.add_span(top, &EMPTY_SCANLINE, SENTINEL_IDX, &EMPTY_SCANLINE, SENTINEL_IDX);
        }
        oper.add_span(bot, run0_slice, run0_idx, run1_slice, run1_idx);

        if quick_exit && !oper.is_empty() {
            return true;
        }

        if a_flush {
            a_idx = skip_intervals_from_x(a_runs, a_idx);
            a_top = a_bot;
            a_bot = a_runs[a_idx];
            a_idx += 1;
            if a_bot == RUN_TYPE_SENTINEL {
                a_top = a_bot;
            } else {
                a_idx += 1; // skip intervalCount
            }
        }
        if b_flush {
            b_idx = skip_intervals_from_x(b_runs, b_idx);
            b_top = b_bot;
            b_bot = b_runs[b_idx];
            b_idx += 1;
            if b_bot == RUN_TYPE_SENTINEL {
                b_top = b_bot;
            } else {
                b_idx += 1; // skip intervalCount
            }
        }

        prev_bot = bot;
    }

    let count = oper.flush();
    count > 2
}

// ---------------------------------------------------------------------------
// High-level safe Rust `Region` methods
// ---------------------------------------------------------------------------

impl Region {
    pub const fn new_empty() -> Self {
        Self::Empty
    }

    pub fn from_rect(r: IRect) -> Self {
        let mut rgn = Self::Empty;
        rgn.set_rect(r);
        rgn
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn is_rect(&self) -> bool {
        matches!(self, Self::Rect(_))
    }

    pub fn is_complex(&self) -> bool {
        matches!(self, Self::Complex { .. })
    }

    pub fn bounds(&self) -> IRect {
        match *self {
            Self::Empty => IRect::make_empty(),
            Self::Rect(r) => r,
            Self::Complex { bounds, .. } => bounds,
        }
    }

    pub fn compute_region_complexity(&self) -> i32 {
        match *self {
            Self::Empty => 0,
            Self::Rect(_) => 1,
            Self::Complex { interval_count, .. } => interval_count,
        }
    }

    pub fn set_empty(&mut self) -> bool {
        *self = Self::Empty;
        false
    }

    pub fn set_rect(&mut self, r: IRect) -> bool {
        if r.is_empty() || r.right == RUN_TYPE_SENTINEL || r.bottom == RUN_TYPE_SENTINEL {
            return self.set_empty();
        }
        *self = Self::Rect(r);
        true
    }

    pub fn set_rects(&mut self, rects: &[IRect]) -> bool {
        if rects.is_empty() {
            return self.set_empty();
        }
        if rects.len() == 1 {
            return self.set_rect(rects[0]);
        }
        let mid = rects.len() / 2;
        let mut left = Region::Empty;
        let mut right = Region::Empty;
        left.set_rects(&rects[..mid]);
        right.set_rects(&rects[mid..]);
        self.op_regions(&left, &right, RegionOp::Union)
    }

    pub fn quick_contains(&self, r: IRect) -> bool {
        match *self {
            Self::Rect(b) => {
                r.left < r.right
                    && r.top < r.bottom
                    && b.left <= r.left
                    && b.top <= r.top
                    && b.right >= r.right
                    && b.bottom >= r.bottom
            }
            _ => false,
        }
    }

    pub fn quick_reject_rect(&self, r: IRect) -> bool {
        self.is_empty() || r.is_empty() || !IRect::intersects(self.bounds(), r)
    }

    pub fn quick_reject_region(&self, other: &Self) -> bool {
        self.is_empty() || other.is_empty() || !IRect::intersects(self.bounds(), other.bounds())
    }

    pub fn contains_xy(&self, x: i32, y: i32) -> bool {
        if !irect_contains_xy(self.bounds(), x, y) {
            return false;
        }
        match self {
            Self::Empty => false,
            Self::Rect(_) => true,
            Self::Complex { runs, .. } => complex_runs_contains_xy(runs, x, y),
        }
    }

    pub fn contains_rect(&self, r: IRect) -> bool {
        if !irect_contains_rect(self.bounds(), r) {
            return false;
        }
        match self {
            Self::Empty => false,
            Self::Rect(_) => true,
            Self::Complex { runs, .. } => complex_runs_contains_rect(runs, r),
        }
    }

    pub fn contains_region(&self, other: &Self) -> bool {
        if self.is_empty()
            || other.is_empty()
            || !irect_contains_rect(self.bounds(), other.bounds())
        {
            return false;
        }
        if self.is_rect() {
            return true;
        }
        if other.is_rect() {
            return self.contains_rect(other.bounds());
        }
        !Self::oper(other, self, RegionOp::Difference, None)
    }

    pub fn intersects_rect(&self, r: IRect) -> bool {
        if self.is_empty() || r.is_empty() {
            return false;
        }
        let mut sect = IRect::make_empty();
        if !sect.intersect_pair(self.bounds(), r) {
            return false;
        }
        match self {
            Self::Empty => false,
            Self::Rect(_) => true,
            Self::Complex { runs, .. } => complex_runs_intersects_rect(runs, sect),
        }
    }

    pub fn intersects_region(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        if !IRect::intersects(self.bounds(), other.bounds()) {
            return false;
        }
        match (self.is_rect(), other.is_rect()) {
            (true, true) => true,
            (true, false) => other.intersects_rect(self.bounds()),
            (false, true) => self.intersects_rect(other.bounds()),
            (false, false) => Self::oper(self, other, RegionOp::Intersect, None),
        }
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Rect(b) => {
                let dx = pin_offset_s32(b.left, b.right, dx);
                let dy = pin_offset_s32(b.top, b.bottom, dy);
                Self::from_rect(irect_make_offset(*b, dx, dy))
            }
            Self::Complex {
                bounds,
                y_span_count,
                interval_count,
                runs,
            } => {
                let dx = pin_offset_s32(bounds.left, bounds.right, dx);
                let dy = pin_offset_s32(bounds.top, bounds.bottom, dy);
                let mut dst_runs = vec![0_i32; runs.len()];
                translate_runs(runs, &mut dst_runs, dx, dy);
                Self::Complex {
                    bounds: irect_make_offset(*bounds, dx, dy),
                    y_span_count: *y_span_count,
                    interval_count: *interval_count,
                    runs: dst_runs,
                }
            }
        }
    }

    pub fn op_regions(&mut self, a: &Self, b: &Self, op: RegionOp) -> bool {
        Self::oper(a, b, op, Some(self))
    }

    pub fn op_rect(&mut self, r: IRect, op: RegionOp) -> bool {
        if let Self::Rect(b) = *self {
            if op == RegionOp::Intersect {
                let mut out = IRect::make_empty();
                if !out.intersect_pair(b, r) {
                    return self.set_empty();
                }
                *self = Self::Rect(out);
                return true;
            }
        }
        let lhs = self.clone();
        let rhs = Self::from_rect(r);
        Self::oper(&lhs, &rhs, op, Some(self))
    }

    pub fn oper(a_orig: &Self, b_orig: &Self, mut op: RegionOp, mut result: Option<&mut Self>) -> bool {
        if op == RegionOp::Replace {
            if let Some(dst) = result {
                *dst = b_orig.clone();
                return !dst.is_empty();
            }
            return !b_orig.is_empty();
        }

        let (rgna, rgnb) = if op == RegionOp::ReverseDifference {
            op = RegionOp::Difference;
            (b_orig, a_orig)
        } else {
            (a_orig, b_orig)
        };

        let a_empty = rgna.is_empty();
        let b_empty = rgnb.is_empty();
        let a_rect = rgna.is_rect();
        let b_rect = rgnb.is_rect();
        let a_bounds = rgna.bounds();
        let b_bounds = rgnb.bounds();

        match op {
            RegionOp::Difference => {
                if a_empty {
                    if let Some(dst) = result {
                        return dst.set_empty();
                    }
                    return false;
                }
                if b_empty || !IRect::intersects(a_bounds, b_bounds) {
                    if let Some(dst) = result {
                        *dst = rgna.clone();
                        return !dst.is_empty();
                    }
                    return !a_empty;
                }
                if b_rect && irect_contains_no_empty_check(b_bounds, a_bounds) {
                    if let Some(dst) = result {
                        return dst.set_empty();
                    }
                    return false;
                }
            }
            RegionOp::Intersect => {
                let mut sect = IRect::make_empty();
                if a_empty || b_empty || !sect.intersect_pair(a_bounds, b_bounds) {
                    if let Some(dst) = result {
                        return dst.set_empty();
                    }
                    return false;
                }
                if a_rect && b_rect {
                    if let Some(dst) = result {
                        return dst.set_rect(sect);
                    }
                    return !sect.is_empty();
                }
                if a_rect && irect_contains_rect(a_bounds, b_bounds) {
                    if let Some(dst) = result {
                        *dst = rgnb.clone();
                        return !dst.is_empty();
                    }
                    return !b_empty;
                }
                if b_rect && irect_contains_rect(b_bounds, a_bounds) {
                    if let Some(dst) = result {
                        *dst = rgna.clone();
                        return !dst.is_empty();
                    }
                    return !a_empty;
                }
            }
            RegionOp::Union => {
                if a_empty {
                    if let Some(dst) = result {
                        *dst = rgnb.clone();
                        return !dst.is_empty();
                    }
                    return !b_empty;
                }
                if b_empty {
                    if let Some(dst) = result {
                        *dst = rgna.clone();
                        return !dst.is_empty();
                    }
                    return !a_empty;
                }
                if a_rect && irect_contains_rect(a_bounds, b_bounds) {
                    if let Some(dst) = result {
                        *dst = rgna.clone();
                        return !dst.is_empty();
                    }
                    return !a_empty;
                }
                if b_rect && irect_contains_rect(b_bounds, a_bounds) {
                    if let Some(dst) = result {
                        *dst = rgnb.clone();
                        return !dst.is_empty();
                    }
                    return !b_empty;
                }
            }
            RegionOp::Xor => {
                if a_empty {
                    if let Some(dst) = result {
                        *dst = rgnb.clone();
                        return !dst.is_empty();
                    }
                    return !b_empty;
                }
                if b_empty {
                    if let Some(dst) = result {
                        *dst = rgna.clone();
                        return !dst.is_empty();
                    }
                    return !a_empty;
                }
            }
            RegionOp::ReverseDifference | RegionOp::Replace => unreachable!(),
        }

        let tmp_a = build_rect_runs(a_bounds);
        let tmp_b = build_rect_runs(b_bounds);
        let a_runs: &[i32] = match rgna {
            Self::Complex { runs, .. } => runs,
            Self::Rect(_) => &tmp_a,
            Self::Empty => &[RUN_TYPE_SENTINEL],
        };
        let b_runs: &[i32] = match rgnb {
            Self::Complex { runs, .. } => runs,
            Self::Rect(_) => &tmp_b,
            Self::Empty => &[RUN_TYPE_SENTINEL],
        };

        let mut buf = Vec::new();
        let quick_exit = result.is_none();
        let non_empty = operate_runs(a_runs, b_runs, op, quick_exit, &mut buf);

        if let Some(dst) = result.take() {
            match normalize_runs(&mut buf) {
                NormalizedRuns::Empty => dst.set_empty(),
                NormalizedRuns::Rect(r) => dst.set_rect(r),
                NormalizedRuns::Complex {
                    bounds,
                    y_span_count,
                    interval_count,
                    runs,
                } => {
                    *dst = Self::Complex {
                        bounds,
                        y_span_count,
                        interval_count,
                        runs,
                    };
                    true
                }
            }
        } else {
            non_empty
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Rect(b) => !b.is_empty(),
            Self::Complex {
                bounds,
                y_span_count,
                interval_count,
                runs,
            } => {
                !bounds.is_empty() && validate_runs(runs, *bounds, *y_span_count, *interval_count)
            }
        }
    }

    pub fn iter(&self) -> RegionIterator<'_> {
        RegionIterator::new(self)
    }

    pub fn visit_spans<F: FnMut(IRect)>(&self, mut f: F) {
        match self {
            Self::Empty => {}
            Self::Rect(b) => f(*b),
            Self::Complex { runs, .. } => visit_complex_runs_spans(runs, f),
        }
    }
}

/// Visits spans of a complex run array in Y -> X ascending order, matching `SkRegionPriv::VisitSpans`.
pub fn visit_complex_runs_spans<F: FnMut(IRect)>(runs: &[i32], mut visitor: F) {
    let mut idx = 0usize;
    let mut top = runs[idx];
    idx += 1;
    let mut bot = runs[idx];
    idx += 1;
    loop {
        let pair_count = runs[idx] as usize;
        idx += 1;
        if pair_count == 1 {
            visitor(IRect::make_ltrb(runs[idx], top, runs[idx + 1], bot));
            idx += 2;
        } else if pair_count > 1 {
            let pairs = &runs[idx..idx + pair_count * 2];
            for y in top..bot {
                for p in pairs.chunks_exact(2) {
                    visitor(IRect::make_ltrb(p[0], y, p[1], y + 1));
                }
            }
            idx += pair_count * 2;
        }
        debug_assert_eq!(runs[idx], RUN_TYPE_SENTINEL);
        idx += 1;
        top = bot;
        bot = runs[idx];
        idx += 1;
        if bot == RUN_TYPE_SENTINEL {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Iterators (`RegionIterator`, `Cliperator`, `Spanerator`)
// ---------------------------------------------------------------------------

/// Step helper matching `SkRegion::Iterator::next()` for complex run arrays.
///
/// `offset` points to the next interval or X-sentinel.
/// Updates `rect`, returns `Some(new_offset)` if another rect was found, or `None` when done.
pub fn complex_iter_next(runs: &[i32], mut offset: usize, rect: &mut IRect) -> Option<usize> {
    if runs[offset] < RUN_TYPE_SENTINEL {
        rect.left = runs[offset];
        rect.right = runs[offset + 1];
        Some(offset + 2)
    } else {
        offset += 1;
        if runs[offset] < RUN_TYPE_SENTINEL {
            let intervals = runs[offset + 1];
            if intervals == 0 {
                rect.top = runs[offset];
                offset += 3;
            } else {
                rect.top = rect.bottom;
            }
            rect.bottom = runs[offset];
            rect.left = runs[offset + 2];
            rect.right = runs[offset + 3];
            Some(offset + 4)
        } else {
            None
        }
    }
}

/// Safe Rust equivalent of `SkRegion::Iterator`.
pub struct RegionIterator<'a> {
    runs: Option<&'a [i32]>,
    offset: usize,
    rect: IRect,
    done: bool,
}

impl<'a> RegionIterator<'a> {
    pub fn new(rgn: &'a Region) -> Self {
        match rgn {
            Region::Empty => Self {
                runs: None,
                offset: 0,
                rect: IRect::make_empty(),
                done: true,
            },
            Region::Rect(b) => Self {
                runs: None,
                offset: 0,
                rect: *b,
                done: false,
            },
            Region::Complex { runs, .. } => Self {
                runs: Some(runs),
                offset: 5,
                rect: IRect::make_ltrb(runs[3], runs[0], runs[4], runs[1]),
                done: false,
            },
        }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn rect(&self) -> IRect {
        self.rect
    }

    pub fn advance(&mut self) {
        if self.done {
            return;
        }
        match self.runs {
            None => {
                self.done = true;
            }
            Some(runs) => match complex_iter_next(runs, self.offset, &mut self.rect) {
                Some(next_off) => self.offset = next_off,
                None => self.done = true,
            },
        }
    }
}

impl<'a> Iterator for RegionIterator<'a> {
    type Item = IRect;
    fn next(&mut self) -> Option<IRect> {
        if self.done {
            return None;
        }
        let r = self.rect;
        self.advance();
        Some(r)
    }
}

/// Safe Rust equivalent of `SkRegion::Cliperator`.
pub struct Cliperator<'a> {
    iter: RegionIterator<'a>,
    clip: IRect,
    rect: IRect,
    done: bool,
}

impl<'a> Cliperator<'a> {
    pub fn new(rgn: &'a Region, clip: IRect) -> Self {
        let mut iter = RegionIterator::new(rgn);
        let mut rect = IRect::make_empty();
        let mut done = true;
        while !iter.done() {
            let r = iter.rect();
            if r.top >= clip.bottom {
                break;
            }
            if rect.intersect_pair(clip, r) {
                done = false;
                break;
            }
            iter.advance();
        }
        Self {
            iter,
            clip,
            rect,
            done,
        }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn rect(&self) -> IRect {
        self.rect
    }

    pub fn advance(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        self.iter.advance();
        while !self.iter.done() {
            let r = self.iter.rect();
            if r.top >= self.clip.bottom {
                break;
            }
            if self.rect.intersect_pair(self.clip, r) {
                self.done = false;
                break;
            }
            self.iter.advance();
        }
    }
}

impl<'a> Iterator for Cliperator<'a> {
    type Item = IRect;
    fn next(&mut self) -> Option<IRect> {
        if self.done {
            return None;
        }
        let r = self.rect;
        self.advance();
        Some(r)
    }
}

/// Finds the initial interval offset for `SkRegion::Spanerator` in a complex run array.
pub fn complex_spanerator_init(runs: &[i32], y: i32, left: i32, right: i32) -> Option<usize> {
    let mut idx = find_scanline_offset(runs, y) + 2;
    loop {
        if runs[idx] >= right {
            return None;
        }
        if runs[idx + 1] <= left {
            idx += 2;
            continue;
        }
        return Some(idx);
    }
}

/// Safe Rust equivalent of `SkRegion::Spanerator`.
pub struct Spanerator<'a> {
    runs: Option<&'a [i32]>,
    offset: usize,
    left: i32,
    right: i32,
    done: bool,
}

impl<'a> Spanerator<'a> {
    pub fn new(rgn: &'a Region, y: i32, mut left: i32, mut right: i32) -> Self {
        let r = rgn.bounds();
        if !rgn.is_empty() && y >= r.top && y < r.bottom && right > r.left && left < r.right {
            match rgn {
                Region::Rect(_) => {
                    if left < r.left {
                        left = r.left;
                    }
                    if right > r.right {
                        right = r.right;
                    }
                    return Self {
                        runs: None,
                        offset: 0,
                        left,
                        right,
                        done: false,
                    };
                }
                Region::Complex { runs, .. } => {
                    if let Some(off) = complex_spanerator_init(runs, y, left, right) {
                        return Self {
                            runs: Some(runs),
                            offset: off,
                            left,
                            right,
                            done: false,
                        };
                    }
                }
                Region::Empty => {}
            }
        }
        Self {
            runs: None,
            offset: 0,
            left: 0,
            right: 0,
            done: true,
        }
    }

    pub fn next_span(&mut self) -> Option<(i32, i32)> {
        if self.done {
            return None;
        }
        match self.runs {
            None => {
                self.done = true;
                Some((self.left, self.right))
            }
            Some(runs) => {
                if runs[self.offset] >= self.right {
                    self.done = true;
                    return None;
                }
                let l = self.left.max(runs[self.offset]);
                let r = self.right.min(runs[self.offset + 1]);
                self.offset += 2;
                Some((l, r))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Serialization (`writeToMemory` / `readFromMemory`)
// ---------------------------------------------------------------------------

/// Computes serialized byte size matching `SkRegion::writeToMemory(nullptr)`.
pub fn serialized_size(is_empty: bool, is_rect: bool, run_count: usize) -> usize {
    let mut size = 4usize; // count (-1, 0, or runCount)
    if !is_empty {
        size += 16; // IRect bounds
        if !is_rect {
            size += 8; // ySpanCount + intervalCount
            size += run_count * 4;
        }
    }
    size
}

/// Serializes a region into `dst` (matching `SkRegion::writeToMemory`).
/// Returns total bytes written.
pub fn write_to_memory(
    is_empty: bool,
    is_rect: bool,
    bounds: IRect,
    y_span_count: i32,
    interval_count: i32,
    runs: &[i32],
    dst: Option<&mut [u8]>,
) -> usize {
    let needed = serialized_size(is_empty, is_rect, runs.len());
    let Some(buf) = dst else {
        return needed;
    };
    let mut pos = 0usize;
    let write_i32 = |buf: &mut [u8], pos: &mut usize, val: i32| {
        buf[*pos..*pos + 4].copy_from_slice(&val.to_ne_bytes());
        *pos += 4;
    };

    if is_empty {
        write_i32(buf, &mut pos, -1);
    } else {
        let count_field = if is_rect { 0 } else { runs.len() as i32 };
        write_i32(buf, &mut pos, count_field);
        write_i32(buf, &mut pos, bounds.left);
        write_i32(buf, &mut pos, bounds.top);
        write_i32(buf, &mut pos, bounds.right);
        write_i32(buf, &mut pos, bounds.bottom);
        if !is_rect {
            write_i32(buf, &mut pos, y_span_count);
            write_i32(buf, &mut pos, interval_count);
            for &r in runs {
                write_i32(buf, &mut pos, r);
            }
        }
    }
    pos
}

/// Deserializes a region from `src` (matching `SkRegion::readFromMemory`).
/// Returns `Some((region, bytes_read))` on success, or `None` on failure (short/invalid data).
pub fn read_from_memory(src: &[u8]) -> Option<(Region, usize)> {
    let mut pos = 0usize;
    let read_i32 = |pos: &mut usize| -> Option<i32> {
        if *pos + 4 > src.len() {
            return None;
        }
        let bytes: [u8; 4] = src[*pos..*pos + 4].try_into().ok()?;
        *pos += 4;
        Some(i32::from_ne_bytes(bytes))
    };

    let count = read_i32(&mut pos)?;
    if count < -1 {
        return None;
    }
    if count == -1 {
        return Some((Region::Empty, pos));
    }

    let bounds = IRect {
        left: read_i32(&mut pos)?,
        top: read_i32(&mut pos)?,
        right: read_i32(&mut pos)?,
        bottom: read_i32(&mut pos)?,
    };
    if bounds.is_empty() {
        return None;
    }

    if count == 0 {
        return Some((Region::Rect(bounds), pos));
    }

    let y_span_count = read_i32(&mut pos)?;
    let interval_count = read_i32(&mut pos)?;
    let run_count = count as usize;
    let run_bytes = run_count.checked_mul(4)?;
    if src.len() - pos < run_bytes {
        return None;
    }

    let mut runs = Vec::with_capacity(run_count);
    for _ in 0..run_count {
        runs.push(read_i32(&mut pos)?);
    }

    if !validate_runs(&runs, bounds, y_span_count, interval_count) {
        return None;
    }

    Some((
        Region::Complex {
            bounds,
            y_span_count,
            interval_count,
            runs,
        },
        pos,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_rect_and_empty() {
        let mut r = Region::new_empty();
        assert!(r.is_empty());
        assert_eq!(r.compute_region_complexity(), 0);
        assert!(!r.set_rect(IRect::make_ltrb(10, 10, 10, 20)));
        assert!(!r.set_rect(IRect::make_ltrb(0, 0, RUN_TYPE_SENTINEL, 10)));
        assert!(r.set_rect(IRect::make_ltrb(10, 20, 30, 40)));
        assert!(r.is_rect());
        assert_eq!(r.compute_region_complexity(), 1);
        assert!(r.contains_xy(10, 20));
        assert!(r.contains_xy(29, 39));
        assert!(!r.contains_xy(30, 20));
        assert!(!r.contains_xy(10, 40));
    }

    #[test]
    fn union_intersect_diff_xor_complex() {
        // L-shape union
        let a = Region::from_rect(IRect::make_ltrb(0, 0, 10, 20));
        let b = Region::from_rect(IRect::make_ltrb(0, 10, 20, 20));
        let mut u = Region::Empty;
        assert!(u.op_regions(&a, &b, RegionOp::Union));
        assert!(u.is_complex());
        assert!(u.is_valid());
        assert_eq!(u.bounds(), IRect::make_ltrb(0, 0, 20, 20));
        assert!(u.contains_xy(5, 5));
        assert!(!u.contains_xy(15, 5));
        assert!(u.contains_xy(15, 15));
        assert!(u.contains_rect(IRect::make_ltrb(0, 0, 10, 20)));
        assert!(!u.contains_rect(IRect::make_ltrb(0, 0, 20, 20)));
        assert!(u.contains_region(&a));
        assert!(u.contains_region(&b));
        assert!(!a.contains_region(&u));

        // Difference: hole in middle
        let big = Region::from_rect(IRect::make_ltrb(0, 0, 30, 30));
        let hole = Region::from_rect(IRect::make_ltrb(10, 10, 20, 20));
        let mut ring = Region::Empty;
        assert!(ring.op_regions(&big, &hole, RegionOp::Difference));
        assert!(ring.is_complex());
        assert!(ring.is_valid());
        assert_eq!(ring.compute_region_complexity(), 4);
        assert!(!ring.contains_xy(15, 15));
        assert!(ring.contains_xy(5, 15));
        assert!(ring.contains_xy(25, 15));
        assert!(!ring.intersects_rect(hole.bounds()));

        // XOR: ring ^ hole == big (collapses back to single rect!)
        let mut restored = Region::Empty;
        assert!(restored.op_regions(&ring, &hole, RegionOp::Xor));
        assert!(restored.is_rect());
        assert_eq!(restored.bounds(), big.bounds());

        // ReverseDifference: hole - big == empty
        let mut empty = Region::Empty;
        assert!(!empty.op_regions(&big, &hole, RegionOp::ReverseDifference));
        assert!(empty.is_empty());
    }

    #[test]
    fn set_rects_iterators_and_serialization() {
        let rects = [
            IRect::make_ltrb(0, 0, 10, 10),
            IRect::make_ltrb(20, 0, 30, 10),
            IRect::make_ltrb(10, 20, 25, 30),
        ];
        let mut rgn = Region::Empty;
        assert!(rgn.set_rects(&rects));
        assert!(rgn.is_complex());
        assert!(rgn.is_valid());

        let iterated: Vec<IRect> = rgn.iter().collect();
        assert_eq!(iterated, rects);

        let clipped: Vec<IRect> = Cliperator::new(&rgn, IRect::make_ltrb(5, 0, 25, 25)).collect();
        assert_eq!(
            clipped,
            vec![
                IRect::make_ltrb(5, 0, 10, 10),
                IRect::make_ltrb(20, 0, 25, 10),
                IRect::make_ltrb(10, 20, 25, 25),
            ]
        );

        let mut span = Spanerator::new(&rgn, 5, 2, 28);
        assert_eq!(span.next_span(), Some((2, 10)));
        assert_eq!(span.next_span(), Some((20, 28)));
        assert_eq!(span.next_span(), None);

        // Round-trip serialization
        if let Region::Complex {
            bounds,
            y_span_count,
            interval_count,
            ref runs,
        } = rgn
        {
            let sz = write_to_memory(false, false, bounds, y_span_count, interval_count, runs, None);
            let mut buf = vec![0u8; sz];
            let written = write_to_memory(
                false,
                false,
                bounds,
                y_span_count,
                interval_count,
                runs,
                Some(&mut buf),
            );
            assert_eq!(written, sz);
            let (decoded, read_len) = read_from_memory(&buf).expect("valid decode");
            assert_eq!(read_len, sz);
            assert_eq!(decoded, rgn);
        } else {
            panic!("expected complex region");
        }
    }

    #[test]
    fn translate_and_pin_overflow() {
        let mut rgn = Region::Empty;
        rgn.set_rects(&[
            IRect::make_ltrb(0, 0, 10, 10),
            IRect::make_ltrb(20, 20, 30, 30),
        ]);
        let moved = rgn.translate(100, -50);
        assert!(moved.is_valid());
        assert_eq!(moved.bounds(), IRect::make_ltrb(100, -50, 130, -20));
        assert!(moved.contains_xy(105, -45));
        assert!(moved.contains_xy(125, -25));
    }
}
