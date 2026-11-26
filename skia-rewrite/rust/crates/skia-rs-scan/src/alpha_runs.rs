#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkAlphaRuns.h` and `src/core/SkAlphaRuns.cpp`.

/// Return 0..=255 given 0..=256 (matching `SkAlphaRuns::CatchOverflow`).
#[inline]
pub const fn catch_overflow(alpha: i32) -> u8 {
    debug_assert!(alpha >= 0 && alpha <= 256);
    (alpha - (alpha >> 8)) as u8
}

/// Reset a sparse run-length-encoded alpha scanline of length `width` (`runs.len() > width`, `alpha.len() >= width`).
pub fn reset_slice(runs: &mut [i16], alpha: &mut [u8], width: usize) {
    debug_assert!(width > 0);
    debug_assert!(runs.len() > width);
    debug_assert!(alpha.len() >= width);
    runs[0] = width as i16;
    runs[width] = 0;
    alpha[0] = 0;
}

/// Returns true if the scanline contains only a single run of alpha value 0.
#[inline]
pub fn is_empty_slice(runs: &[i16], alpha: &[u8]) -> bool {
    let r0 = runs[0] as usize;
    debug_assert!(runs[0] > 0);
    alpha[0] == 0 && runs[r0] == 0
}

/// Cut (at offset `x`) a run into two shorter runs with matching alpha values.
/// Matches `SkAlphaRuns::BreakAt`.
pub fn break_at_slice(runs: &mut [i16], alpha: &mut [u8], mut x: usize) {
    let mut pos = 0usize;
    while x > 0 {
        let n = runs[pos] as usize;
        debug_assert!(n > 0);
        if x < n {
            alpha[pos + x] = alpha[pos];
            runs[pos] = x as i16;
            runs[pos + x] = (n - x) as i16;
            break;
        }
        pos += n;
        x -= n;
    }
}

/// Break the runs in the buffer at offsets `x` and `x + count`.
/// Matches `SkAlphaRuns::Break`.
pub fn break_runs_slice(runs: &mut [i16], alpha: &mut [u8], x: usize, count: usize) {
    debug_assert!(count > 0);
    let mut pos = 0usize;
    let mut rem = x;
    while rem > 0 {
        let n = runs[pos] as usize;
        debug_assert!(n > 0);
        if rem < n {
            alpha[pos + rem] = alpha[pos];
            runs[pos] = rem as i16;
            runs[pos + rem] = (n - rem) as i16;
            break;
        }
        pos += n;
        rem -= n;
    }

    let mut pos2 = x;
    let mut rem2 = count;
    loop {
        let n = runs[pos2] as usize;
        debug_assert!(n > 0);
        if rem2 < n {
            alpha[pos2 + rem2] = alpha[pos2];
            runs[pos2] = rem2 as i16;
            runs[pos2 + rem2] = (n - rem2) as i16;
            break;
        }
        if rem2 <= n {
            break;
        }
        rem2 -= n;
        pos2 += n;
    }
}

/// Insert a run into `(runs, alpha)` starting at `x` with hint `offset_x`.
/// Matches `SkAlphaRuns::add` bit-for-bit.
pub fn add_slice(
    runs: &mut [i16],
    alpha: &mut [u8],
    x: usize,
    start_alpha: u8,
    mut middle_count: usize,
    stop_alpha: u8,
    max_value: u8,
    offset_x: usize,
) -> usize {
    debug_assert!(runs[offset_x] >= 0);

    let mut base = offset_x;
    let mut last_base = offset_x;
    let mut rel_x = x - offset_x;

    if start_alpha != 0 {
        break_runs_slice(&mut runs[base..], &mut alpha[base..], rel_x, 1);
        let tmp = (alpha[base + rel_x] as u32) + (start_alpha as u32);
        debug_assert!(tmp <= 256);
        alpha[base + rel_x] = (tmp - (tmp >> 8)) as u8;
        base += rel_x + 1;
        rel_x = 0;
    }

    if middle_count != 0 {
        break_runs_slice(&mut runs[base..], &mut alpha[base..], rel_x, middle_count);
        base += rel_x;
        rel_x = 0;
        while middle_count > 0 {
            let sum = (alpha[base] as i32) + (max_value as i32);
            alpha[base] = catch_overflow(sum);
            let n = runs[base] as usize;
            debug_assert!(n <= middle_count);
            base += n;
            middle_count -= n;
        }
        last_base = base;
    }

    if stop_alpha != 0 {
        break_runs_slice(&mut runs[base..], &mut alpha[base..], rel_x, 1);
        base += rel_x;
        alpha[base] = alpha[base].wrapping_add(stop_alpha);
        last_base = base;
    }

    last_base
}

/// Validate a scanline's run-length encoding (matches `SkAlphaRuns::validate`).
pub fn validate_slice(runs: &[i16], width: usize) -> bool {
    if width == 0 || runs.len() <= width {
        return false;
    }
    let mut count = 0usize;
    let mut idx = 0usize;
    while runs[idx] != 0 {
        let r = runs[idx];
        if r <= 0 {
            return false;
        }
        count += r as usize;
        if count > width {
            return false;
        }
        idx += r as usize;
    }
    count == width
}

/// Validate alpha bounds for supersampling (matches `SkAlphaRuns::assertValid`).
pub fn assert_valid_slice(runs: &[i16], alpha: &[u8], y: i32, max_step: i32) -> bool {
    let max_val = (y + 1) * max_step - if y == max_step - 1 { 1 } else { 0 };
    let mut idx = 0usize;
    while idx < runs.len() && runs[idx] != 0 {
        if (alpha[idx] as i32) > max_val {
            return false;
        }
        let step = runs[idx];
        if step <= 0 {
            return false;
        }
        idx += step as usize;
    }
    true
}

/// Owned safe Rust `SkAlphaRuns` buffer used by `RunBasedAdditiveBlitter`.
#[derive(Clone, Debug)]
pub struct AlphaRuns {
    pub runs: Vec<i16>,
    pub alpha: Vec<u8>,
    pub width: usize,
}

impl AlphaRuns {
    pub fn new(width: usize) -> Self {
        let alloc_w = width.max(1);
        let mut s = Self {
            runs: vec![0i16; alloc_w + 1],
            alpha: vec![0u8; alloc_w],
            width,
        };
        if width > 0 {
            s.reset(width);
        }
        s
    }

    pub fn reset(&mut self, width: usize) {
        self.width = width;
        if width == 0 {
            self.runs[0] = 0;
            self.alpha[0] = 0;
            return;
        }
        if self.runs.len() <= width {
            self.runs.resize(width + 1, 0);
        }
        if self.alpha.len() < width {
            self.alpha.resize(width, 0);
        }
        reset_slice(&mut self.runs, &mut self.alpha, width);
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        if self.width == 0 {
            return true;
        }
        is_empty_slice(&self.runs, &self.alpha)
    }

    #[inline]
    pub fn add(
        &mut self,
        x: usize,
        start_alpha: u8,
        middle_count: usize,
        stop_alpha: u8,
        max_value: u8,
        offset_x: usize,
    ) -> usize {
        add_slice(
            &mut self.runs,
            &mut self.alpha,
            x,
            start_alpha,
            middle_count,
            stop_alpha,
            max_value,
            offset_x,
        )
    }
}
