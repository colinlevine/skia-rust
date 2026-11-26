#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkScan_AAAPath.cpp`.

use crate::alpha_runs::{catch_overflow, AlphaRuns};
use crate::analytic_edge::{AnalyticEdge, AnalyticEdgeBase, K_DEFAULT_ACCURACY};
use crate::blitter::Blitter;
use crate::edge_builder::AnalyticEdgeBuilder;
use crate::fixed::{
    abs32, align4, fixed_ceil_to_fixed, fixed_ceil_to_int, fixed_floor_to_fixed,
    fixed_floor_to_int, fixed_mul, fixed_round_to_int, int_to_fixed, irect_contains,
    left_shift_i32, sat_add_i32, sat_sub_i32, sk_tqsort, SkFixed, SK_FIXED1, SK_MAX_S32,
    SK_MIN_S32,
};
use skia_rs_geometry::{IRect, Rect};
use skia_rs_path::shapes::is_rect_contour;
use skia_rs_path::{FillType, PathRaw};

const NONE: usize = usize::MAX;
const HEAD_IDX: usize = 0;
const TAIL_IDX: usize = 1;

const MASK_MAX_WIDTH: i32 = 32;
const MASK_MAX_STORAGE: i64 = 1024;

#[inline]
fn add_alpha(alpha: &mut u8, delta: u8) {
    let sum = (*alpha as i32) + (delta as i32);
    debug_assert!(sum <= 256);
    *alpha = catch_overflow(sum);
}

#[inline]
fn safely_add_alpha(alpha: &mut u8, delta: u8) {
    *alpha = alpha.saturating_add(delta);
}

#[inline]
fn snap_alpha(alpha: u8) -> u8 {
    if alpha > 247 {
        0xFF
    } else if alpha < 8 {
        0x00
    } else {
        alpha
    }
}

/// Safe Rust equivalent of `MaskAdditiveBlitter`.
struct MaskAdditiveBlitter {
    bounds: IRect,
    clip_rect: IRect,
    row_bytes: usize,
    // storage[1..] corresponds to fMask.image(); 1 guard byte at each end matches C++.
    storage: Vec<u8>,
}

impl MaskAdditiveBlitter {
    fn can_handle_rect(bounds: IRect) -> bool {
        let width = bounds.width();
        if width > MASK_MAX_WIDTH || width <= 0 || bounds.height() <= 0 {
            return false;
        }
        let rb = align4(width) as i64;
        let storage = rb * (bounds.height() as i64);
        width <= MASK_MAX_WIDTH && storage <= MASK_MAX_STORAGE
    }

    fn new(ir: IRect, clip_bounds: IRect) -> Self {
        let mut clip_rect = IRect::EMPTY;
        if !clip_rect.intersect_pair(ir, clip_bounds) {
            clip_rect = IRect::EMPTY;
        }
        let row_bytes = ir.width().max(0) as usize;
        let height = ir.height().max(0) as usize;
        let total = height * row_bytes + 2;
        Self {
            bounds: ir,
            clip_rect,
            row_bytes,
            storage: vec![0u8; total],
        }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        let dy = y - self.bounds.top;
        if dy < 0 || dy >= self.bounds.height() {
            return None;
        }
        let dx = x - self.bounds.left;
        let off = 1isize + (dy as isize) * (self.row_bytes as isize) + (dx as isize);
        if off >= 0 && (off as usize) < self.storage.len() {
            Some(off as usize)
        } else {
            None
        }
    }

    #[inline]
    fn set_px(&mut self, x: i32, y: i32, alpha: u8) {
        if let Some(i) = self.idx(x, y) {
            self.storage[i] = alpha;
        }
    }

    #[inline]
    fn safely_add_px(&mut self, x: i32, y: i32, alpha: u8) {
        if let Some(i) = self.idx(x, y) {
            safely_add_alpha(&mut self.storage[i], alpha);
        }
    }

    fn blit_anti_h_single(&mut self, x: i32, y: i32, alpha: u8) {
        if let Some(i) = self.idx(x, y) {
            add_alpha(&mut self.storage[i], alpha);
        }
    }

    fn blit_anti_h_run(&mut self, x: i32, y: i32, width: i32, alpha: u8) {
        for i in 0..width {
            self.blit_anti_h_single(x + i, y, alpha);
        }
    }

    fn blit_v(&mut self, x: i32, y: i32, height: i32, alpha: u8) {
        if alpha == 0 {
            return;
        }
        for row in 0..height {
            self.set_px(x, y + row, alpha);
        }
    }

    fn blit_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        for row in 0..height {
            for col in 0..width {
                self.set_px(x + col, y + row, 0xFF);
            }
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
        self.blit_v(x + 1 + width, y, height, right_alpha);
        self.blit_rect(x + 1, y, width, height);
    }

    fn finish<B: Blitter + ?Sized>(&self, real_blitter: &mut B) {
        if !self.clip_rect.is_empty() {
            let img_len = (self.bounds.height() as usize) * self.row_bytes;
            real_blitter.blit_mask_a8(
                &self.storage[1..1 + img_len],
                self.bounds,
                self.row_bytes as u32,
                self.clip_rect,
            );
        }
    }
}

/// Safe Rust equivalent of `RunBasedAdditiveBlitter` and `SafeRLEAdditiveBlitter`.
struct RunBasedAdditiveBlitter {
    safe_clamp: bool,
    curr_y: i32,
    width: i32,
    left: i32,
    top: i32,
    runs_ring: Vec<AlphaRuns>,
    current_run: usize,
    offset_x: usize,
}

impl RunBasedAdditiveBlitter {
    fn new(
        ir: IRect,
        clip_bounds: IRect,
        is_inverse: bool,
        safe_clamp: bool,
        rows_preserved: i32,
    ) -> Self {
        let sect_bounds = if is_inverse {
            clip_bounds
        } else {
            let mut s = IRect::EMPTY;
            if s.intersect_pair(ir, clip_bounds) {
                s
            } else {
                IRect::EMPTY
            }
        };
        let left = sect_bounds.left;
        let width = (sect_bounds.right - left).max(0);
        let top = sect_bounds.top;
        let ring_len = rows_preserved.max(1) as usize;
        let mut runs_ring = Vec::with_capacity(ring_len);
        for _ in 0..ring_len {
            runs_ring.push(AlphaRuns::new(width as usize));
        }
        Self {
            safe_clamp,
            curr_y: top - 1,
            width,
            left,
            top,
            runs_ring,
            current_run: 0,
            offset_x: 0,
        }
    }

    #[inline]
    fn check(&self, x: i32, w: i32) -> bool {
        x >= 0 && w >= 0 && x + w <= self.width
    }

    fn advance_runs(&mut self) {
        self.current_run = (self.current_run + 1) % self.runs_ring.len();
        self.runs_ring[self.current_run].reset(self.width as usize);
    }

    fn flush<B: Blitter + ?Sized>(&mut self, real_blitter: &mut B) {
        if self.curr_y >= self.top && self.width > 0 {
            let cur = &mut self.runs_ring[self.current_run];
            let mut x = 0usize;
            while cur.runs[x] != 0 {
                cur.alpha[x] = snap_alpha(cur.alpha[x]);
                x += cur.runs[x] as usize;
            }
            if !cur.is_empty() {
                real_blitter.blit_anti_h(
                    self.left,
                    self.curr_y,
                    &mut cur.alpha,
                    &mut cur.runs,
                );
                self.advance_runs();
                self.offset_x = 0;
            }
            self.curr_y = self.top - 1;
        }
    }

    #[inline]
    fn check_y<B: Blitter + ?Sized>(&mut self, real_blitter: &mut B, y: i32) {
        if y != self.curr_y {
            self.flush(real_blitter);
            self.curr_y = y;
        }
    }

    #[inline]
    fn flush_if_y_changed<B: Blitter + ?Sized>(
        &mut self,
        real_blitter: &mut B,
        y: SkFixed,
        next_y: SkFixed,
    ) {
        if fixed_floor_to_int(y) != fixed_floor_to_int(next_y) {
            self.flush(real_blitter);
        }
    }

    fn blit_anti_h_slice<B: Blitter + ?Sized>(
        &mut self,
        real_blitter: &mut B,
        mut x: i32,
        y: i32,
        mut antialias: &[u8],
    ) {
        self.check_y(real_blitter, y);
        x -= self.left;
        let mut len = antialias.len() as i32;
        if x < 0 {
            let skip = (-x) as usize;
            if skip >= antialias.len() {
                return;
            }
            antialias = &antialias[skip..];
            len += x;
            x = 0;
        }
        len = len.min(self.width - x);
        if len <= 0 {
            return;
        }
        let ux = x as usize;
        let ulen = len as usize;
        if ux < self.offset_x {
            self.offset_x = 0;
        }
        let cur = &mut self.runs_ring[self.current_run];
        self.offset_x = cur.add(ux, 0, ulen, 0, 0, self.offset_x);
        let mut i = 0usize;
        while i < ulen {
            let r = cur.runs[ux + i] as usize;
            let a = cur.alpha[ux + i];
            for j in 1..r {
                cur.runs[ux + i + j] = 1;
                cur.alpha[ux + i + j] = a;
            }
            cur.runs[ux + i] = 1;
            i += r;
        }
        if self.safe_clamp {
            for k in 0..ulen {
                safely_add_alpha(&mut cur.alpha[ux + k], antialias[k]);
            }
        } else {
            for k in 0..ulen {
                add_alpha(&mut cur.alpha[ux + k], antialias[k]);
            }
        }
    }

    fn blit_anti_h_single<B: Blitter + ?Sized>(
        &mut self,
        real_blitter: &mut B,
        mut x: i32,
        y: i32,
        alpha: u8,
    ) {
        self.check_y(real_blitter, y);
        x -= self.left;
        if x < 0 || !self.check(x, 1) {
            return;
        }
        let ux = x as usize;
        if ux < self.offset_x {
            self.offset_x = 0;
        }
        let cur = &mut self.runs_ring[self.current_run];
        if self.safe_clamp {
            self.offset_x = cur.add(ux, 0, 1, 0, 0, self.offset_x);
            safely_add_alpha(&mut cur.alpha[ux], alpha);
        } else {
            self.offset_x = cur.add(ux, 0, 1, 0, alpha, self.offset_x);
        }
    }

    fn blit_anti_h_run<B: Blitter + ?Sized>(
        &mut self,
        real_blitter: &mut B,
        mut x: i32,
        y: i32,
        mut width: i32,
        alpha: u8,
    ) {
        self.check_y(real_blitter, y);
        x -= self.left;
        if x < 0 {
            width += x;
            x = 0;
        }
        width = width.min(self.width - x);
        if width <= 0 || !self.check(x, width) {
            return;
        }
        let ux = x as usize;
        let uw = width as usize;
        if ux < self.offset_x {
            self.offset_x = 0;
        }
        let cur = &mut self.runs_ring[self.current_run];
        if self.safe_clamp {
            self.offset_x = cur.add(ux, 0, uw, 0, 0, self.offset_x);
            let mut i = ux;
            while i < ux + uw {
                safely_add_alpha(&mut cur.alpha[i], alpha);
                i += cur.runs[i] as usize;
            }
        } else {
            self.offset_x = cur.add(ux, 0, uw, 0, alpha, self.offset_x);
        }
    }
}

enum AdditiveTarget {
    Mask(MaskAdditiveBlitter),
    Rle(RunBasedAdditiveBlitter),
}

impl AdditiveTarget {
    #[inline]
    fn is_mask(&self) -> bool {
        matches!(self, Self::Mask(_))
    }

    #[inline]
    fn flush_if_y_changed<B: Blitter + ?Sized>(
        &mut self,
        real: &mut B,
        y: SkFixed,
        next_y: SkFixed,
    ) {
        if let Self::Rle(rle) = self {
            rle.flush_if_y_changed(real, y, next_y);
        }
    }

    #[inline]
    fn blit_anti_h_single<B: Blitter + ?Sized>(
        &mut self,
        real: &mut B,
        x: i32,
        y: i32,
        alpha: u8,
    ) {
        match self {
            Self::Mask(m) => m.blit_anti_h_single(x, y, alpha),
            Self::Rle(r) => r.blit_anti_h_single(real, x, y, alpha),
        }
    }

    #[inline]
    fn blit_anti_h_run<B: Blitter + ?Sized>(
        &mut self,
        real: &mut B,
        x: i32,
        y: i32,
        width: i32,
        alpha: u8,
    ) {
        match self {
            Self::Mask(m) => m.blit_anti_h_run(x, y, width, alpha),
            Self::Rle(r) => r.blit_anti_h_run(real, x, y, width, alpha),
        }
    }

    #[inline]
    fn blit_rect_direct<B: Blitter + ?Sized>(
        &mut self,
        real: &mut B,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) {
        match self {
            Self::Mask(m) => m.blit_rect(x, y, width, height),
            Self::Rle(_) => real.blit_rect(x, y, width, height),
        }
    }

    #[inline]
    fn blit_v_direct<B: Blitter + ?Sized>(
        &mut self,
        real: &mut B,
        x: i32,
        y: i32,
        height: i32,
        alpha: u8,
    ) {
        match self {
            Self::Mask(m) => m.blit_v(x, y, height, alpha),
            Self::Rle(_) => real.blit_v(x, y, height, alpha),
        }
    }

    #[inline]
    fn blit_anti_rect_direct<B: Blitter + ?Sized>(
        &mut self,
        real: &mut B,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        left_alpha: u8,
        right_alpha: u8,
    ) {
        match self {
            Self::Mask(m) => m.blit_anti_rect(x, y, width, height, left_alpha, right_alpha),
            Self::Rle(_) => real.blit_anti_rect(x, y, width, height, left_alpha, right_alpha),
        }
    }

    fn finish<B: Blitter + ?Sized>(&mut self, real: &mut B) {
        match self {
            Self::Mask(m) => m.finish(real),
            Self::Rle(r) => r.flush(real),
        }
    }
}

#[inline]
fn trapezoid_to_alpha(l1: SkFixed, l2: SkFixed) -> u8 {
    debug_assert!(l1 >= 0 && l2 >= 0);
    let area = (l1.wrapping_add(l2)) / 2;
    (area >> 8) as u8
}

#[inline]
fn partial_triangle_to_alpha(a: SkFixed, b: SkFixed) -> u8 {
    let area = (a >> 11).wrapping_mul(a >> 11).wrapping_mul(b >> 11);
    ((area >> 8) & 0xFF) as u8
}

#[inline]
fn get_partial_alpha_fixed(alpha: u8, partial_height: SkFixed) -> u8 {
    fixed_round_to_int((alpha as i32).wrapping_mul(partial_height)) as u8
}

#[inline]
fn get_partial_alpha_u8(alpha: u8, full_alpha: u8) -> u8 {
    (((alpha as u32) * (full_alpha as u32)) >> 8) as u8
}

#[inline]
fn fixed_to_alpha(f: SkFixed) -> u8 {
    get_partial_alpha_fixed(0xFF, f)
}

#[inline]
fn approximate_intersection(
    mut l1: SkFixed,
    mut r1: SkFixed,
    mut l2: SkFixed,
    mut r2: SkFixed,
) -> SkFixed {
    if l1 > r1 {
        core::mem::swap(&mut l1, &mut r1);
    }
    if l2 > r2 {
        core::mem::swap(&mut l2, &mut r2);
    }
    l1.max(l2).wrapping_add(r1.min(r2)) / 2
}

fn compute_alpha_above_line(
    alphas: &mut [u8],
    l: SkFixed,
    r: SkFixed,
    dy: SkFixed,
    full_alpha: u8,
) {
    debug_assert!(l <= r);
    let r_int = fixed_ceil_to_int(r);
    if r_int <= 0 {
        return;
    } else if r_int == 1 {
        let a = ((left_shift_i32(r_int, 17).wrapping_sub(l).wrapping_sub(r)) >> 9) as u8;
        alphas[0] = get_partial_alpha_u8(a, full_alpha);
    } else {
        let first = SK_FIXED1 - l;
        let last = r.wrapping_sub(left_shift_i32(r_int - 1, 16));
        let first_h = fixed_mul(first, dy);
        alphas[0] = (fixed_mul(first, first_h) >> 9) as u8;
        let mut alpha16 = sat_add_i32(first_h, dy >> 1);
        for i in 1..(r_int - 1) as usize {
            alphas[i] = (alpha16 >> 8) as u8;
            alpha16 = sat_add_i32(alpha16, dy);
        }
        alphas[(r_int - 1) as usize] = full_alpha.wrapping_sub(partial_triangle_to_alpha(last, dy));
    }
}

fn compute_alpha_below_line(
    alphas: &mut [u8],
    l: SkFixed,
    r: SkFixed,
    dy: SkFixed,
    full_alpha: u8,
) {
    debug_assert!(l <= r);
    let r_int = fixed_ceil_to_int(r);
    if r_int <= 0 {
        return;
    } else if r_int == 1 {
        alphas[0] = get_partial_alpha_u8(trapezoid_to_alpha(l, r), full_alpha);
    } else {
        let first = SK_FIXED1 - l;
        let last = r.wrapping_sub(left_shift_i32(r_int - 1, 16));
        let last_h = fixed_mul(last, dy);
        alphas[(r_int - 1) as usize] = (fixed_mul(last, last_h) >> 9) as u8;
        let mut alpha16 = sat_add_i32(last_h, dy >> 1);
        for i in (1..(r_int - 1) as usize).rev() {
            alphas[i] = ((alpha16 >> 8) & 0xFF) as u8;
            alpha16 = sat_add_i32(alpha16, dy);
        }
        alphas[0] = full_alpha.wrapping_sub(partial_triangle_to_alpha(first, dy));
    }
}

fn blit_single_alpha<B: Blitter + ?Sized>(
    target: &mut AdditiveTarget,
    real: &mut B,
    y: i32,
    x: i32,
    alpha: u8,
    full_alpha: u8,
    no_real_blitter: bool,
) {
    match target {
        AdditiveTarget::Mask(m) => {
            if full_alpha == 0xFF && !no_real_blitter {
                m.set_px(x, y, alpha);
            } else {
                m.safely_add_px(x, y, get_partial_alpha_u8(alpha, full_alpha));
            }
        }
        AdditiveTarget::Rle(rle) => {
            if full_alpha == 0xFF && !no_real_blitter {
                real.blit_v(x, y, 1, alpha);
            } else {
                rle.blit_anti_h_single(real, x, y, get_partial_alpha_u8(alpha, full_alpha));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_two_alphas<B: Blitter + ?Sized>(
    target: &mut AdditiveTarget,
    real: &mut B,
    y: i32,
    x: i32,
    a1: u8,
    a2: u8,
    full_alpha: u8,
    no_real_blitter: bool,
) {
    match target {
        AdditiveTarget::Mask(m) => {
            m.safely_add_px(x, y, a1);
            m.safely_add_px(x + 1, y, a2);
        }
        AdditiveTarget::Rle(rle) => {
            if full_alpha == 0xFF && !no_real_blitter {
                real.blit_anti_h2(x, y, a1, a2);
            } else {
                rle.blit_anti_h_single(real, x, y, a1);
                rle.blit_anti_h_single(real, x + 1, y, a2);
            }
        }
    }
}

fn blit_full_alpha<B: Blitter + ?Sized>(
    target: &mut AdditiveTarget,
    real: &mut B,
    y: i32,
    x: i32,
    len: i32,
    full_alpha: u8,
    no_real_blitter: bool,
) {
    match target {
        AdditiveTarget::Mask(m) => {
            for i in 0..len {
                m.safely_add_px(x + i, y, full_alpha);
            }
        }
        AdditiveTarget::Rle(rle) => {
            if full_alpha == 0xFF && !no_real_blitter {
                real.blit_h(x, y, len);
            } else {
                rle.blit_anti_h_run(real, x, y, len, full_alpha);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_aaa_trapezoid_row<B: Blitter + ?Sized>(
    target: &mut AdditiveTarget,
    real: &mut B,
    y: i32,
    ul: SkFixed,
    ur: SkFixed,
    ll: SkFixed,
    lr: SkFixed,
    l_dy: SkFixed,
    r_dy: SkFixed,
    full_alpha: u8,
    no_real_blitter: bool,
) {
    let l_int = fixed_floor_to_int(ul);
    let r_int = fixed_ceil_to_int(lr);
    let len = r_int - l_int;
    if len <= 0 {
        return;
    }

    if len == 1 {
        let alpha = trapezoid_to_alpha(ur - ul, lr - ll);
        blit_single_alpha(target, real, y, l_int, alpha, full_alpha, no_real_blitter);
        return;
    }

    const QUICK_LEN: usize = 31;
    let ulen = len as usize;

    let mut stack_alphas = [0u8; QUICK_LEN + 1];
    let mut stack_temp = [0u8; QUICK_LEN + 1];
    let mut stack_runs = [0i16; QUICK_LEN + 1];
    let mut heap_alphas = Vec::new();
    let mut heap_temp = Vec::new();
    let mut heap_runs = Vec::new();

    let (alphas, temp_alphas, runs): (&mut [u8], &mut [u8], &mut [i16]) = if ulen <= QUICK_LEN {
        (
            &mut stack_alphas[..=ulen],
            &mut stack_temp[..=ulen],
            &mut stack_runs[..=ulen],
        )
    } else {
        heap_alphas.resize(ulen + 1, 0);
        heap_temp.resize(ulen + 1, 0);
        heap_runs.resize(ulen + 1, 0);
        (&mut heap_alphas, &mut heap_temp, &mut heap_runs)
    };

    for i in 0..ulen {
        runs[i] = 1;
        alphas[i] = full_alpha;
    }
    runs[ulen] = 0;

    let u_l = fixed_floor_to_int(ul);
    let l_l = fixed_ceil_to_int(ll);
    if u_l + 2 == l_l {
        let first = int_to_fixed(u_l).wrapping_add(SK_FIXED1).wrapping_sub(ul);
        let second = ll.wrapping_sub(ul).wrapping_sub(first);
        let a1 = full_alpha.wrapping_sub(partial_triangle_to_alpha(first, l_dy));
        let a2 = partial_triangle_to_alpha(second, l_dy);
        alphas[0] = alphas[0].saturating_sub(a1);
        alphas[1] = alphas[1].saturating_sub(a2);
    } else if l_l > u_l {
        let start = (u_l - l_int) as usize;
        compute_alpha_below_line(
            &mut temp_alphas[start..],
            ul.wrapping_sub(int_to_fixed(u_l)),
            ll.wrapping_sub(int_to_fixed(u_l)),
            l_dy,
            full_alpha,
        );
        for i in u_l..l_l {
            let idx = (i - l_int) as usize;
            alphas[idx] = alphas[idx].saturating_sub(temp_alphas[idx]);
        }
    }

    let u_r = fixed_floor_to_int(ur);
    let l_r = fixed_ceil_to_int(lr);
    if u_r + 2 == l_r {
        let first = int_to_fixed(u_r).wrapping_add(SK_FIXED1).wrapping_sub(ur);
        let second = lr.wrapping_sub(ur).wrapping_sub(first);
        let a1 = partial_triangle_to_alpha(first, r_dy);
        let a2 = full_alpha.wrapping_sub(partial_triangle_to_alpha(second, r_dy));
        alphas[ulen - 2] = alphas[ulen - 2].saturating_sub(a1);
        alphas[ulen - 1] = alphas[ulen - 1].saturating_sub(a2);
    } else if l_r > u_r {
        let start = (u_r - l_int) as usize;
        compute_alpha_above_line(
            &mut temp_alphas[start..],
            ur.wrapping_sub(int_to_fixed(u_r)),
            lr.wrapping_sub(int_to_fixed(u_r)),
            r_dy,
            full_alpha,
        );
        for i in u_r..l_r {
            let idx = (i - l_int) as usize;
            alphas[idx] = alphas[idx].saturating_sub(temp_alphas[idx]);
        }
    }

    match target {
        AdditiveTarget::Mask(m) => {
            for (i, &a) in alphas[..ulen].iter().enumerate() {
                m.safely_add_px(l_int + i as i32, y, a);
            }
        }
        AdditiveTarget::Rle(rle) => {
            if full_alpha == 0xFF && !no_real_blitter {
                real.blit_anti_h(l_int, y, &mut alphas[..ulen], runs);
            } else {
                rle.blit_anti_h_slice(real, l_int, y, &alphas[..ulen]);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_trapezoid_row<B: Blitter + ?Sized>(
    target: &mut AdditiveTarget,
    real: &mut B,
    y: i32,
    mut ul: SkFixed,
    mut ur: SkFixed,
    mut ll: SkFixed,
    mut lr: SkFixed,
    l_dy: SkFixed,
    r_dy: SkFixed,
    full_alpha: u8,
    no_real_blitter: bool,
) {
    debug_assert!(l_dy >= 0 && r_dy >= 0);

    if ul > ur {
        return;
    }

    if ll > lr {
        let x = approximate_intersection(ul, ll, ur, lr);
        ll = x;
        lr = x;
    }

    if ul == ur && ll == lr {
        return;
    }

    if ul > ll {
        core::mem::swap(&mut ul, &mut ll);
    }
    if ur > lr {
        core::mem::swap(&mut ur, &mut lr);
    }

    let join_left = fixed_ceil_to_fixed(ll);
    let join_rite = fixed_floor_to_fixed(ur);
    if join_left <= join_rite {
        if ul < join_left {
            let len = fixed_ceil_to_int(join_left - ul);
            if len == 1 {
                let alpha = trapezoid_to_alpha(join_left - ul, join_left - ll);
                blit_single_alpha(target, real, y, ul >> 16, alpha, full_alpha, no_real_blitter);
            } else if len == 2 {
                let first = join_left - SK_FIXED1 - ul;
                let second = ll - ul - first;
                let a1 = partial_triangle_to_alpha(first, l_dy);
                let a2 = full_alpha.wrapping_sub(partial_triangle_to_alpha(second, l_dy));
                blit_two_alphas(target, real, y, ul >> 16, a1, a2, full_alpha, no_real_blitter);
            } else {
                blit_aaa_trapezoid_row(
                    target,
                    real,
                    y,
                    ul,
                    join_left,
                    ll,
                    join_left,
                    l_dy,
                    SK_MAX_S32,
                    full_alpha,
                    no_real_blitter,
                );
            }
        }
        if join_left < join_rite {
            blit_full_alpha(
                target,
                real,
                y,
                fixed_floor_to_int(join_left),
                fixed_floor_to_int(join_rite - join_left),
                full_alpha,
                no_real_blitter,
            );
        }
        if lr > join_rite {
            let len = fixed_ceil_to_int(lr - join_rite);
            if len == 1 {
                let alpha = trapezoid_to_alpha(ur - join_rite, lr - join_rite);
                blit_single_alpha(
                    target,
                    real,
                    y,
                    join_rite >> 16,
                    alpha,
                    full_alpha,
                    no_real_blitter,
                );
            } else if len == 2 {
                let first = join_rite + SK_FIXED1 - ur;
                let second = lr - ur - first;
                let a1 = full_alpha.wrapping_sub(partial_triangle_to_alpha(first, r_dy));
                let a2 = partial_triangle_to_alpha(second, r_dy);
                blit_two_alphas(
                    target,
                    real,
                    y,
                    join_rite >> 16,
                    a1,
                    a2,
                    full_alpha,
                    no_real_blitter,
                );
            } else {
                blit_aaa_trapezoid_row(
                    target,
                    real,
                    y,
                    join_rite,
                    ur,
                    join_rite,
                    lr,
                    SK_MAX_S32,
                    r_dy,
                    full_alpha,
                    no_real_blitter,
                );
            }
        }
    } else {
        blit_aaa_trapezoid_row(
            target,
            real,
            y,
            ul,
            ur,
            ll,
            lr,
            l_dy,
            r_dy,
            full_alpha,
            no_real_blitter,
        );
    }
}

struct AEdgeNode {
    edge: AnalyticEdge,
    prev: usize,
    next: usize,
}

#[inline]
fn remove_aedge(nodes: &mut [AEdgeNode], idx: usize) {
    let prev = nodes[idx].prev;
    let next = nodes[idx].next;
    nodes[prev].next = next;
    nodes[next].prev = prev;
}

#[inline]
fn insert_aedge_after(nodes: &mut [AEdgeNode], idx: usize, after_me: usize) {
    let after_next = nodes[after_me].next;
    nodes[idx].prev = after_me;
    nodes[idx].next = after_next;
    nodes[after_next].prev = idx;
    nodes[after_me].next = idx;
}

#[inline]
fn backward_insert_start_a(nodes: &[AEdgeNode], mut prev: usize, x: SkFixed) -> usize {
    while nodes[prev].prev != NONE && nodes[prev].edge.base().x > x {
        prev = nodes[prev].prev;
    }
    prev
}

#[inline]
fn backward_insert_aedge_based_on_x(nodes: &mut [AEdgeNode], idx: usize) {
    let x = nodes[idx].edge.base().x;
    let mut prev = nodes[idx].prev;
    while nodes[prev].prev != NONE && nodes[prev].edge.base().x > x {
        prev = nodes[prev].prev;
    }
    if nodes[prev].next != idx {
        remove_aedge(nodes, idx);
        insert_aedge_after(nodes, idx, prev);
    }
}

fn is_smooth_enough_single(this_edge: &AnalyticEdge, next_edge: &AnalyticEdge) -> bool {
    let cc = this_edge.base().curve_count;
    if cc < 0 {
        if let AnalyticEdge::Cubic(c) = this_edge {
            let ddshift = c.base.curve_shift as i32;
            let dshift = c.to_fixed_shift as i32;
            return (abs32(c.c_dx) >> 1) >= (abs32(c.c_ddx) >> ddshift)
                && (abs32(c.c_dy) >> 1) >= (abs32(c.c_ddy) >> ddshift)
                && ((c.c_dy.wrapping_sub(c.c_ddy >> ddshift)) >> dshift) >= SK_FIXED1;
        }
    } else if cc > 0 {
        if let AnalyticEdge::Quad(q) = this_edge {
            let shift = q.base.curve_shift as i32;
            return (abs32(q.q_dx) >> 1) >= abs32(q.q_ddx)
                && (abs32(q.q_dy) >> 1) >= abs32(q.q_ddy)
                && ((q.q_dy.wrapping_sub(q.q_ddy)) >> shift) >= SK_FIXED1;
        }
    }
    abs32(sat_sub_i32(next_edge.base().dx, this_edge.base().dx)) <= SK_FIXED1
        && next_edge.base().lower_y.wrapping_sub(next_edge.base().upper_y) >= SK_FIXED1
}

fn is_smooth_enough(
    nodes: &[AEdgeNode],
    left_e: usize,
    rite_e: usize,
    mut curr_e: usize,
    stop_y: i32,
) -> bool {
    if nodes[curr_e].edge.base().upper_y >= left_shift_i32(stop_y, 16) {
        return false;
    }
    let left_low = nodes[left_e].edge.base().lower_y;
    let rite_low = nodes[rite_e].edge.base().lower_y;
    if left_low.wrapping_add(SK_FIXED1) < rite_low {
        return is_smooth_enough_single(&nodes[left_e].edge, &nodes[curr_e].edge);
    } else if left_low > rite_low.wrapping_add(SK_FIXED1) {
        return is_smooth_enough_single(&nodes[rite_e].edge, &nodes[curr_e].edge);
    }

    let mut next_curr_e = nodes[curr_e].next;
    if next_curr_e == NONE || nodes[next_curr_e].edge.base().upper_y >= left_shift_i32(stop_y, 16) {
        return false;
    }
    if nodes[next_curr_e].edge.base().upper_x < nodes[curr_e].edge.base().upper_x {
        core::mem::swap(&mut curr_e, &mut next_curr_e);
    }
    is_smooth_enough_single(&nodes[left_e].edge, &nodes[curr_e].edge)
        && is_smooth_enough_single(&nodes[rite_e].edge, &nodes[next_curr_e].edge)
}

#[inline]
fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[allow(clippy::too_many_arguments)]
fn aaa_walk_convex_edges<B: Blitter + ?Sized>(
    nodes: &mut [AEdgeNode],
    target: &mut AdditiveTarget,
    real: &mut B,
    stop_y: i32,
    left_bound: SkFixed,
    rite_bound: SkFixed,
) {
    let mut left_e = nodes[HEAD_IDX].next;
    let mut rite_e = nodes[left_e].next;
    let mut curr_e = nodes[rite_e].next;

    let mut y = nodes[left_e]
        .edge
        .base()
        .upper_y
        .max(nodes[rite_e].edge.base().upper_y);

    'outer: loop {
        while nodes[left_e].edge.base().lower_y <= y || nodes[rite_e].edge.base().lower_y <= y {
            while nodes[left_e].edge.base().lower_y <= y {
                if !nodes[left_e].edge.update(y) {
                    if fixed_floor_to_int(nodes[curr_e].edge.base().upper_y) >= stop_y {
                        break 'outer;
                    }
                    left_e = curr_e;
                    curr_e = nodes[curr_e].next;
                }
            }
            while nodes[rite_e].edge.base().lower_y <= y {
                if !nodes[rite_e].edge.update(y) {
                    if fixed_floor_to_int(nodes[curr_e].edge.base().upper_y) >= stop_y {
                        break 'outer;
                    }
                    rite_e = curr_e;
                    curr_e = nodes[curr_e].next;
                }
            }
            let prev_y = y;
            y = y
                .max(nodes[left_e].edge.base().upper_y)
                .max(nodes[rite_e].edge.base().upper_y);
            target.flush_if_y_changed(real, prev_y, y);
        }

        if fixed_floor_to_int(y) >= stop_y {
            break;
        }

        nodes[left_e].edge.base_mut().go_y(y);
        nodes[rite_e].edge.base_mut().go_y(y);

        let lx = nodes[left_e].edge.base().x;
        let rx = nodes[rite_e].edge.base().x;
        let ldx = nodes[left_e].edge.base().dx;
        let rdx = nodes[rite_e].edge.base().dx;
        if lx > rx || (lx == rx && ldx > rdx) {
            core::mem::swap(&mut left_e, &mut rite_e);
        }

        let mut local_bot_fixed = nodes[left_e]
            .edge
            .base()
            .lower_y
            .min(nodes[rite_e].edge.base().lower_y);
        if is_smooth_enough(nodes, left_e, rite_e, curr_e, stop_y) {
            local_bot_fixed = fixed_ceil_to_fixed(local_bot_fixed);
        }
        local_bot_fixed = local_bot_fixed.min(int_to_fixed(stop_y));

        let mut left = clamp_i32(nodes[left_e].edge.base().x, left_bound, rite_bound);
        let d_left = nodes[left_e].edge.base().dx;
        let mut rite = clamp_i32(nodes[rite_e].edge.base().x, left_bound, rite_bound);
        let d_rite = nodes[rite_e].edge.base().dx;
        let left_dy = nodes[left_e].edge.base().dy;
        let rite_dy = nodes[rite_e].edge.base().dy;

        if (d_left | d_rite) == 0 {
            let full_left = fixed_ceil_to_int(left);
            let full_rite = fixed_floor_to_int(rite);
            let partial_left = int_to_fixed(full_left) - left;
            let partial_rite = rite - int_to_fixed(full_rite);
            let full_top = fixed_ceil_to_int(y);
            let full_bot = fixed_floor_to_int(local_bot_fixed);
            let mut partial_top = int_to_fixed(full_top) - y;
            let mut partial_bot = local_bot_fixed - int_to_fixed(full_bot);
            if full_top > full_bot {
                partial_top -= SK_FIXED1 - partial_bot;
                partial_bot = 0;
            }

            if full_rite >= full_left {
                if partial_top > 0 {
                    if partial_left > 0 {
                        target.blit_anti_h_single(
                            real,
                            full_left - 1,
                            full_top - 1,
                            fixed_to_alpha(fixed_mul(partial_top, partial_left)),
                        );
                    }
                    target.blit_anti_h_run(
                        real,
                        full_left,
                        full_top - 1,
                        full_rite - full_left,
                        fixed_to_alpha(partial_top),
                    );
                    if partial_rite > 0 {
                        target.blit_anti_h_single(
                            real,
                            full_rite,
                            full_top - 1,
                            fixed_to_alpha(fixed_mul(partial_top, partial_rite)),
                        );
                    }
                    target.flush_if_y_changed(real, y, y + partial_top);
                }

                if full_bot > full_top
                    && (full_rite > full_left
                        || fixed_to_alpha(partial_left) > 0
                        || fixed_to_alpha(partial_rite) > 0)
                {
                    target.blit_anti_rect_direct(
                        real,
                        full_left - 1,
                        full_top,
                        full_rite - full_left,
                        full_bot - full_top,
                        fixed_to_alpha(partial_left),
                        fixed_to_alpha(partial_rite),
                    );
                }

                if partial_bot > 0 {
                    if partial_left > 0 {
                        target.blit_anti_h_single(
                            real,
                            full_left - 1,
                            full_bot,
                            fixed_to_alpha(fixed_mul(partial_bot, partial_left)),
                        );
                    }
                    target.blit_anti_h_run(
                        real,
                        full_left,
                        full_bot,
                        full_rite - full_left,
                        fixed_to_alpha(partial_bot),
                    );
                    if partial_rite > 0 {
                        target.blit_anti_h_single(
                            real,
                            full_rite,
                            full_bot,
                            fixed_to_alpha(fixed_mul(partial_bot, partial_rite)),
                        );
                    }
                }
            } else {
                let width = rite - left;
                if width > 0 {
                    if partial_top > 0 {
                        target.blit_anti_h_run(
                            real,
                            full_left - 1,
                            full_top - 1,
                            1,
                            fixed_to_alpha(fixed_mul(partial_top, width)),
                        );
                        target.flush_if_y_changed(real, y, y + partial_top);
                    }
                    if full_bot > full_top {
                        target.blit_v_direct(
                            real,
                            full_left - 1,
                            full_top,
                            full_bot - full_top,
                            fixed_to_alpha(width),
                        );
                    }
                    if partial_bot > 0 {
                        target.blit_anti_h_run(
                            real,
                            full_left - 1,
                            full_bot,
                            1,
                            fixed_to_alpha(fixed_mul(partial_bot, width)),
                        );
                    }
                }
            }

            y = local_bot_fixed;
        } else {
            const K_SNAP_DIGIT: SkFixed = SK_FIXED1 >> 4;
            const K_SNAP_HALF: SkFixed = K_SNAP_DIGIT >> 1;
            const K_SNAP_MASK: SkFixed = -1 ^ (K_SNAP_DIGIT - 1);
            left = left.wrapping_add(K_SNAP_HALF);
            rite = rite.wrapping_add(K_SNAP_HALF);
            let min_x = left_bound.wrapping_add(K_SNAP_HALF);
            let max_x = rite_bound.wrapping_add(K_SNAP_HALF);

            let mut count = fixed_ceil_to_int(local_bot_fixed) - fixed_floor_to_int(y);

            if count > 1 {
                if (y & (0xFFFF_0000u32 as i32)) != y {
                    count -= 1;
                    let next_y = fixed_ceil_to_fixed(y + 1);
                    let dy = next_y - y;
                    let next_left = clamp_i32(sat_add_i32(left, fixed_mul(d_left, dy)), min_x, max_x);
                    let next_rite = clamp_i32(sat_add_i32(rite, fixed_mul(d_rite, dy)), min_x, max_x);
                    blit_trapezoid_row(
                        target,
                        real,
                        y >> 16,
                        left & K_SNAP_MASK,
                        rite & K_SNAP_MASK,
                        next_left & K_SNAP_MASK,
                        next_rite & K_SNAP_MASK,
                        left_dy,
                        rite_dy,
                        get_partial_alpha_fixed(0xFF, dy),
                        false,
                    );
                    target.flush_if_y_changed(real, y, next_y);
                    left = next_left;
                    rite = next_rite;
                    y = next_y;
                }

                while count > 1 {
                    count -= 1;
                    let next_y = y + SK_FIXED1;
                    let next_left = clamp_i32(sat_add_i32(left, d_left), min_x, max_x);
                    let next_rite = clamp_i32(sat_add_i32(rite, d_rite), min_x, max_x);
                    blit_trapezoid_row(
                        target,
                        real,
                        y >> 16,
                        left & K_SNAP_MASK,
                        rite & K_SNAP_MASK,
                        next_left & K_SNAP_MASK,
                        next_rite & K_SNAP_MASK,
                        left_dy,
                        rite_dy,
                        0xFF,
                        false,
                    );
                    target.flush_if_y_changed(real, y, next_y);
                    left = next_left;
                    rite = next_rite;
                    y = next_y;
                }
            }

            let dy = local_bot_fixed - y;
            let next_left = clamp_i32(sat_add_i32(left, fixed_mul(d_left, dy)), min_x, max_x);
            let next_rite = clamp_i32(sat_add_i32(rite, fixed_mul(d_rite, dy)), min_x, max_x);
            blit_trapezoid_row(
                target,
                real,
                y >> 16,
                left & K_SNAP_MASK,
                rite & K_SNAP_MASK,
                next_left & K_SNAP_MASK,
                next_rite & K_SNAP_MASK,
                left_dy,
                rite_dy,
                get_partial_alpha_fixed(0xFF, dy),
                false,
            );
            target.flush_if_y_changed(real, y, local_bot_fixed);
            left = next_left.wrapping_sub(K_SNAP_HALF);
            rite = next_rite.wrapping_sub(K_SNAP_HALF);
            y = local_bot_fixed;
        }

        nodes[left_e].edge.base_mut().x = left;
        nodes[rite_e].edge.base_mut().x = rite;
        nodes[left_e].edge.base_mut().y = y;
        nodes[rite_e].edge.base_mut().y = y;
    }
}

#[inline]
fn update_next_next_y(y: SkFixed, next_y: SkFixed, next_next_y: &mut SkFixed) {
    if y > next_y && y < *next_next_y {
        *next_next_y = y;
    }
}

#[inline]
fn check_intersection(nodes: &[AEdgeNode], edge: usize, next_y: SkFixed, next_next_y: &mut SkFixed) {
    let prev = nodes[edge].prev;
    if prev != NONE && nodes[prev].prev != NONE {
        let pb = nodes[prev].edge.base();
        let eb = nodes[edge].edge.base();
        if pb.x.wrapping_add(pb.dx) > eb.x.wrapping_add(eb.dx) {
            *next_next_y = next_y.wrapping_add(SK_FIXED1 >> K_DEFAULT_ACCURACY);
        }
    }
}

#[inline]
fn check_intersection_fwd(
    nodes: &[AEdgeNode],
    edge: usize,
    next_y: SkFixed,
    next_next_y: &mut SkFixed,
) {
    let next = nodes[edge].next;
    if next != NONE && nodes[next].next != NONE {
        let eb = nodes[edge].edge.base();
        let nb = nodes[next].edge.base();
        if eb.x.wrapping_add(eb.dx) > nb.x.wrapping_add(nb.dx) {
            *next_next_y = next_y.wrapping_add(SK_FIXED1 >> K_DEFAULT_ACCURACY);
        }
    }
}

fn insert_new_aedges(
    nodes: &mut [AEdgeNode],
    mut new_edge: usize,
    y: SkFixed,
    next_next_y: &mut SkFixed,
) {
    if nodes[new_edge].edge.base().upper_y > y {
        update_next_next_y(nodes[new_edge].edge.base().upper_y, y, next_next_y);
        return;
    }
    let prev = nodes[new_edge].prev;
    if nodes[prev].edge.base().x <= nodes[new_edge].edge.base().x {
        while nodes[new_edge].edge.base().upper_y <= y {
            check_intersection(nodes, new_edge, y, next_next_y);
            update_next_next_y(nodes[new_edge].edge.base().lower_y, y, next_next_y);
            new_edge = nodes[new_edge].next;
        }
        update_next_next_y(nodes[new_edge].edge.base().upper_y, y, next_next_y);
        return;
    }

    let mut start = backward_insert_start_a(nodes, prev, nodes[new_edge].edge.base().x);
    loop {
        let next = nodes[new_edge].next;
        let new_x = nodes[new_edge].edge.base().x;
        let mut need_move = true;
        loop {
            if nodes[start].next == new_edge {
                need_move = false;
                break;
            }
            let after = nodes[start].next;
            if nodes[after].edge.base().x >= new_x {
                break;
            }
            start = after;
        }
        if need_move {
            remove_aedge(nodes, new_edge);
            insert_aedge_after(nodes, new_edge, start);
        }
        check_intersection(nodes, new_edge, y, next_next_y);
        check_intersection_fwd(nodes, new_edge, y, next_next_y);
        update_next_next_y(nodes[new_edge].edge.base().lower_y, y, next_next_y);
        start = new_edge;
        new_edge = next;
        if nodes[new_edge].edge.base().upper_y > y {
            break;
        }
    }
    update_next_next_y(nodes[new_edge].edge.base().upper_y, y, next_next_y);
}

#[inline]
fn edges_too_close(nodes: &[AEdgeNode], prev: usize, next: usize, lower_y: SkFixed) -> bool {
    const SLACK: SkFixed = SK_FIXED1;
    if prev == NONE || next == NONE {
        return false;
    }
    let pb = nodes[prev].edge.base();
    let nb = nodes[next].edge.base();
    nb.upper_y < lower_y && pb.x.wrapping_add(SLACK) >= nb.x.wrapping_sub(abs32(nb.dx))
}

#[inline]
fn edges_too_close_prev_rite(prev_rite: i32, ul: SkFixed, ll: SkFixed) -> bool {
    prev_rite > fixed_floor_to_int(ul) || prev_rite > fixed_floor_to_int(ll)
}

#[allow(clippy::too_many_arguments)]
fn aaa_walk_edges<B: Blitter + ?Sized>(
    nodes: &mut [AEdgeNode],
    fill_type: FillType,
    target: &mut AdditiveTarget,
    real: &mut B,
    mut start_y: i32,
    stop_y: i32,
    left_clip: SkFixed,
    right_clip: SkFixed,
    force_rle: bool,
    skip_intersect: bool,
) {
    nodes[HEAD_IDX].edge.base_mut().x = left_clip;
    nodes[HEAD_IDX].edge.base_mut().upper_x = left_clip;
    nodes[TAIL_IDX].edge.base_mut().x = right_clip;
    nodes[TAIL_IDX].edge.base_mut().upper_x = right_clip;

    let first_edge = nodes[HEAD_IDX].next;
    let mut y = nodes[first_edge]
        .edge
        .base()
        .upper_y
        .max(int_to_fixed(start_y));
    let mut next_next_y = SK_MAX_S32;

    {
        let mut edge = first_edge;
        while nodes[edge].edge.base().upper_y <= y {
            nodes[edge].edge.base_mut().go_y(y);
            update_next_next_y(nodes[edge].edge.base().lower_y, y, &mut next_next_y);
            edge = nodes[edge].next;
        }
        update_next_next_y(nodes[edge].edge.base().upper_y, y, &mut next_next_y);
    }

    let winding_mask: i32 = if fill_type.is_even_odd() { 1 } else { -1 };
    let is_inverse = fill_type.is_inverse();

    if is_inverse && int_to_fixed(start_y) != y {
        let width = fixed_floor_to_int(right_clip - left_clip);
        if fixed_floor_to_int(y) != start_y {
            target.blit_rect_direct(
                real,
                fixed_floor_to_int(left_clip),
                start_y,
                width,
                fixed_floor_to_int(y) - start_y,
            );
            start_y = fixed_floor_to_int(y);
        }
        blit_full_alpha(
            target,
            real,
            start_y,
            fixed_floor_to_int(left_clip),
            width,
            fixed_to_alpha(y - int_to_fixed(start_y)),
            false,
        );
    }

    loop {
        let mut w = 0i32;
        let mut in_interval = is_inverse;
        let mut prev_x = nodes[HEAD_IDX].edge.base().x;
        let mut next_y = next_next_y.min(fixed_ceil_to_fixed(y + 1));
        let mut curr_e = nodes[HEAD_IDX].next;
        let mut left_e = HEAD_IDX;
        let mut left = left_clip;
        let mut left_dy = 0;
        let mut prev_rite = fixed_floor_to_int(left_clip);

        next_next_y = SK_MAX_S32;

        let mut y_shift = 0i32;
        if ((next_y - y) & (SK_FIXED1 >> 2)) != 0 {
            y_shift = 2;
            next_y = y + (SK_FIXED1 >> 2);
        } else if ((next_y - y) & (SK_FIXED1 >> 1)) != 0 {
            y_shift = 1;
        }

        let full_alpha = fixed_to_alpha(next_y - y);
        let no_real_blitter = force_rle;

        while nodes[curr_e].edge.base().upper_y <= y {
            w += nodes[curr_e].edge.base().winding as i32;
            let prev_in_interval = in_interval;
            in_interval = ((w & winding_mask) == 0) == is_inverse;

            let is_left = in_interval && !prev_in_interval;
            let is_rite = !in_interval && prev_in_interval;

            if is_rite {
                let mut rite = nodes[curr_e].edge.base().x;
                nodes[curr_e].edge.base_mut().go_y_shift(next_y, y_shift);
                let left_e_x = nodes[left_e].edge.base().x;
                let next_left = left_clip.max(left_e_x);
                rite = right_clip.min(rite);
                let curr_x = nodes[curr_e].edge.base().x;
                let next_rite = right_clip.min(curr_x);
                let curr_dy = nodes[curr_e].edge.base().dy;
                let curr_next = nodes[curr_e].next;
                let close = no_real_blitter
                    || (full_alpha == 0xFF
                        && (edges_too_close_prev_rite(prev_rite, left, left_e_x)
                            || edges_too_close(nodes, curr_e, curr_next, next_y)));
                blit_trapezoid_row(
                    target,
                    real,
                    y >> 16,
                    left,
                    rite,
                    next_left,
                    next_rite,
                    left_dy,
                    curr_dy,
                    full_alpha,
                    close,
                );
                prev_rite = fixed_ceil_to_int(rite.max(curr_x));
            } else {
                if is_left {
                    left = nodes[curr_e].edge.base().x.max(left_clip);
                    left_dy = nodes[curr_e].edge.base().dy;
                    left_e = curr_e;
                }
                nodes[curr_e].edge.base_mut().go_y_shift(next_y, y_shift);
            }

            let next = nodes[curr_e].next;

            while nodes[curr_e].edge.base().lower_y <= next_y {
                let cc = nodes[curr_e].edge.base().curve_count;
                if cc < 0 {
                    if let AnalyticEdge::Cubic(ref mut c) = nodes[curr_e].edge {
                        c.keep_continuous();
                        if !c.update_cubic() {
                            break;
                        }
                    } else {
                        break;
                    }
                } else if cc > 0 {
                    if let AnalyticEdge::Quad(ref mut q) = nodes[curr_e].edge {
                        q.keep_continuous();
                        if !q.update_quadratic() {
                            break;
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if nodes[curr_e].edge.base().lower_y <= next_y {
                remove_aedge(nodes, curr_e);
            } else {
                update_next_next_y(nodes[curr_e].edge.base().lower_y, next_y, &mut next_next_y);
                let new_x = nodes[curr_e].edge.base().x;
                if new_x < prev_x {
                    backward_insert_aedge_based_on_x(nodes, curr_e);
                } else {
                    prev_x = new_x;
                }
                if !skip_intersect {
                    check_intersection(nodes, curr_e, next_y, &mut next_next_y);
                }
            }

            curr_e = next;
        }

        if in_interval {
            let left_e_x = nodes[left_e].edge.base().x;
            let left_e_prev = nodes[left_e].prev;
            let close = no_real_blitter
                || (full_alpha == 0xFF && edges_too_close(nodes, left_e_prev, left_e, next_y));
            blit_trapezoid_row(
                target,
                real,
                y >> 16,
                left,
                right_clip,
                left_clip.max(left_e_x),
                right_clip,
                left_dy,
                0,
                full_alpha,
                close,
            );
        }

        if force_rle {
            target.flush_if_y_changed(real, y, next_y);
        }

        y = next_y;
        if y >= int_to_fixed(stop_y) {
            break;
        }

        insert_new_aedges(nodes, curr_e, y, &mut next_next_y);
    }
}

fn build_sorted_aedge_nodes(mut edges: Vec<AnalyticEdge>) -> Vec<AEdgeNode> {
    sk_tqsort(&mut edges, &|a, b| {
        let ba = a.base();
        let bb = b.base();
        if ba.upper_y != bb.upper_y {
            ba.upper_y < bb.upper_y
        } else if ba.x != bb.x {
            ba.x < bb.x
        } else {
            ba.dx < bb.dx
        }
    });

    let count = edges.len();
    let mut nodes = Vec::with_capacity(count + 2);

    let mut head_base = AnalyticEdgeBase::default();
    head_base.upper_y = SK_MIN_S32;
    head_base.lower_y = SK_MIN_S32;
    head_base.x = SK_MIN_S32;
    head_base.dx = 0;
    head_base.dy = SK_MAX_S32;
    head_base.upper_x = SK_MIN_S32;
    nodes.push(AEdgeNode {
        edge: AnalyticEdge::Line(head_base),
        prev: NONE,
        next: 2,
    });

    let mut tail_base = AnalyticEdgeBase::default();
    tail_base.upper_y = SK_MAX_S32;
    tail_base.lower_y = SK_MAX_S32;
    tail_base.x = SK_MAX_S32;
    tail_base.dx = 0;
    tail_base.dy = SK_MAX_S32;
    tail_base.upper_x = SK_MAX_S32;
    nodes.push(AEdgeNode {
        edge: AnalyticEdge::Line(tail_base),
        prev: 2 + count - 1,
        next: NONE,
    });

    for (i, e) in edges.into_iter().enumerate() {
        let idx = 2 + i;
        let prev = if i == 0 { HEAD_IDX } else { idx - 1 };
        let next = if i + 1 == count { TAIL_IDX } else { idx + 1 };
        nodes.push(AEdgeNode {
            edge: e,
            prev,
            next,
        });
    }

    nodes
}

#[allow(clippy::too_many_arguments)]
fn aaa_fill_path_inner<B: Blitter + ?Sized>(
    path: &PathRaw<'_>,
    clip_rect: IRect,
    target: &mut AdditiveTarget,
    real: &mut B,
    mut start_y: i32,
    mut stop_y: i32,
    path_contained_in_clip: bool,
    force_rle: bool,
) {
    let mut builder = AnalyticEdgeBuilder::new();
    let count = builder.build_edges(
        path,
        if path_contained_in_clip {
            None
        } else {
            Some(clip_rect)
        },
    );

    if count == 0 {
        if path.fill_type.is_inverse() {
            let mut rect = clip_rect;
            if rect.top < start_y {
                rect.top = start_y;
            }
            if rect.bottom > stop_y {
                rect.bottom = stop_y;
            }
            if !rect.is_empty() {
                target.blit_rect_direct(real, rect.left, rect.top, rect.width(), rect.height());
            }
        }
        return;
    }

    let mut nodes = build_sorted_aedge_nodes(builder.edges);

    if !path_contained_in_clip && start_y < clip_rect.top {
        start_y = clip_rect.top;
    }
    if !path_contained_in_clip && stop_y > clip_rect.bottom {
        stop_y = clip_rect.bottom;
    }

    let mut left_bound = int_to_fixed(clip_rect.left);
    let mut right_bound = int_to_fixed(clip_rect.right);
    if target.is_mask() {
        let ir = path.bounds.round_out();
        left_bound = left_bound.max(int_to_fixed(ir.left));
        right_bound = right_bound.min(int_to_fixed(ir.right));
    }

    if !path.fill_type.is_inverse() && path.convexity.is_convex() && count >= 2 {
        aaa_walk_convex_edges(&mut nodes, target, real, stop_y, left_bound, right_bound);
    } else {
        let skip_intersect = path.points.len() > ((stop_y - start_y).max(0) as usize) * 2;
        aaa_walk_edges(
            &mut nodes,
            path.fill_type,
            target,
            real,
            start_y,
            stop_y,
            left_bound,
            right_bound,
            force_rle,
            skip_intersect,
        );
    }
}

fn try_blit_fat_anti_rect<B: Blitter + ?Sized>(
    blitter: &mut B,
    raw: &PathRaw<'_>,
    clip: IRect,
) -> bool {
    let rc = match is_rect_contour(raw.points, raw.verbs, raw.segment_mask as u32, false) {
        Some(r) => r,
        None => return false,
    };
    let mut rect = Rect::EMPTY;
    if !rect.intersect_pair(rc.rect, Rect::from_irect(clip)) {
        return true;
    }
    let bounds = rect.round_out();
    if bounds.width() < 3 {
        return false;
    }
    blitter.blit_fat_anti_rect(rect);
    true
}

/// Matches `SkScan::AAAFillPath`.
pub fn aaa_fill_path<B: Blitter + ?Sized>(
    path: &PathRaw<'_>,
    blitter: &mut B,
    ir: IRect,
    clip_bounds: IRect,
    force_rle: bool,
) {
    let contained_in_clip = irect_contains(clip_bounds, ir);
    let is_inverse = path.fill_type.is_inverse();

    if MaskAdditiveBlitter::can_handle_rect(ir) && !is_inverse && !force_rle {
        if !try_blit_fat_anti_rect(blitter, path, clip_bounds) {
            let mut target = AdditiveTarget::Mask(MaskAdditiveBlitter::new(ir, clip_bounds));
            aaa_fill_path_inner(
                path,
                clip_bounds,
                &mut target,
                blitter,
                ir.top,
                ir.bottom,
                contained_in_clip,
                force_rle,
            );
            target.finish(blitter);
        }
    } else if !is_inverse && path.convexity.is_convex() {
        let rows = blitter.request_rows_preserved();
        let mut target = AdditiveTarget::Rle(RunBasedAdditiveBlitter::new(
            ir,
            clip_bounds,
            is_inverse,
            false,
            rows,
        ));
        aaa_fill_path_inner(
            path,
            clip_bounds,
            &mut target,
            blitter,
            ir.top,
            ir.bottom,
            contained_in_clip,
            force_rle,
        );
        target.finish(blitter);
    } else {
        let rows = blitter.request_rows_preserved();
        let mut target = AdditiveTarget::Rle(RunBasedAdditiveBlitter::new(
            ir,
            clip_bounds,
            is_inverse,
            true,
            rows,
        ));
        aaa_fill_path_inner(
            path,
            clip_bounds,
            &mut target,
            blitter,
            ir.top,
            ir.bottom,
            contained_in_clip,
            force_rle,
        );
        target.finish(blitter);
    }
}
