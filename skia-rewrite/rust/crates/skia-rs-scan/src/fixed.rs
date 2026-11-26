#![forbid(unsafe_code)]

//! Fixed-point (16.16 `SkFixed`, 26.6 `SkFDot6`, 24.8 `FDot8`) and integer math helpers
//! matching Skia's C++ `SkFixed.h`, `SkFDot6.h`, `SkSafe32.h`, `SkMath.h`, and `SkFloatingPoint.h`.

use skia_rs_geometry::{float_saturate2int, IRect, Rect};

pub type SkFixed = i32;
pub type SkFDot6 = i32;
pub type FDot8 = i32;
pub type XRect = IRect;

pub const SK_FIXED1: SkFixed = 1 << 16;
pub const SK_FIXED_HALF: SkFixed = 1 << 15;
pub const SK_FIXED_QUARTER: SkFixed = 1 << 14;

pub const SK_FDOT6_ONE: SkFDot6 = 64;
pub const SK_FDOT6_HALF: SkFDot6 = 32;

pub const SK_MAX_S16: i16 = i16::MAX;
pub const SK_MIN_S16: i16 = -SK_MAX_S16;

pub const SK_MAX_S32: i32 = i32::MAX;
pub const SK_MIN_S32: i32 = -SK_MAX_S32;
pub const SK_NAN32: i32 = i32::MIN;

pub const SK_SUPERSAMPLE_SHIFT: i32 = 2;

#[inline]
pub const fn left_shift_i32(value: i32, shift: u32) -> i32 {
    ((value as u32).wrapping_shl(shift)) as i32
}

#[inline]
pub const fn left_shift_i64(value: i64, shift: u32) -> i64 {
    ((value as u64).wrapping_shl(shift)) as i64
}

#[inline]
pub const fn pin_64_to_s32(x: i64) -> i32 {
    if x < SK_MIN_S32 as i64 {
        SK_MIN_S32
    } else if x > SK_MAX_S32 as i64 {
        SK_MAX_S32
    } else {
        x as i32
    }
}

#[inline]
pub const fn sat_add_i32(a: i32, b: i32) -> i32 {
    pin_64_to_s32((a as i64) + (b as i64))
}

#[inline]
pub const fn sat_sub_i32(a: i32, b: i32) -> i32 {
    pin_64_to_s32((a as i64) - (b as i64))
}

#[inline]
pub const fn abs32(value: i32) -> i32 {
    if value < 0 {
        value.wrapping_neg()
    } else {
        value
    }
}

#[inline]
pub const fn clz_u32(x: u32) -> i32 {
    x.leading_zeros() as i32
}

#[inline]
pub const fn int_to_fixed(n: i32) -> SkFixed {
    left_shift_i32(n, 16)
}

#[inline]
pub fn float_to_fixed(x: f32) -> SkFixed {
    float_saturate2int(x * (SK_FIXED1 as f32))
}

#[inline]
pub const fn fixed_to_float(x: SkFixed) -> f32 {
    (x as f32) * 1.525_878_906_25e-5_f32
}

#[inline]
pub const fn fixed_round_to_int(x: SkFixed) -> i32 {
    x.wrapping_add(SK_FIXED_HALF) >> 16
}

#[inline]
pub const fn fixed_ceil_to_int(x: SkFixed) -> i32 {
    x.wrapping_add(SK_FIXED1 - 1) >> 16
}

#[inline]
pub const fn fixed_floor_to_int(x: SkFixed) -> i32 {
    x >> 16
}

#[inline]
pub const fn fixed_round_to_fixed(x: SkFixed) -> SkFixed {
    ((x.wrapping_add(SK_FIXED_HALF) as u32) & 0xFFFF_0000) as SkFixed
}

#[inline]
pub const fn fixed_ceil_to_fixed(x: SkFixed) -> SkFixed {
    ((x.wrapping_add(SK_FIXED1 - 1) as u32) & 0xFFFF_0000) as SkFixed
}

#[inline]
pub const fn fixed_floor_to_fixed(x: SkFixed) -> SkFixed {
    ((x as u32) & 0xFFFF_0000) as SkFixed
}

#[inline]
pub const fn fixed_mul(a: SkFixed, b: SkFixed) -> SkFixed {
    (((a as i64) * (b as i64)) >> 16) as SkFixed
}

#[inline]
pub const fn fixed_div(numer: SkFixed, denom: SkFixed) -> SkFixed {
    debug_assert!(denom != 0);
    let q = left_shift_i64(numer as i64, 16) / (denom as i64);
    pin_64_to_s32(q)
}

#[inline]
pub const fn int_to_fdot6(x: i32) -> SkFDot6 {
    left_shift_i32(x, 6)
}

/// Matches x86_64 C++ `(int32_t)v` (`cvttss2si`): out-of-range/NaN yields `i32::MIN` (`SK_NaN32`).
#[inline]
pub fn float_to_i32_c(v: f32) -> i32 {
    if v >= -2_147_483_648.0_f32 && v < 2_147_483_648.0_f32 {
        v as i32
    } else {
        i32::MIN
    }
}

#[inline]
pub fn float_to_fdot6(x: f32) -> SkFDot6 {
    float_to_i32_c(x * (SK_FDOT6_ONE as f32))
}

#[inline]
pub const fn fdot6_floor(x: SkFDot6) -> i32 {
    x >> 6
}

#[inline]
pub const fn fdot6_ceil(x: SkFDot6) -> i32 {
    x.wrapping_add(63) >> 6
}

#[inline]
pub const fn fdot6_round(x: SkFDot6) -> i32 {
    x.wrapping_add(SK_FDOT6_HALF) >> 6
}

#[inline]
pub const fn fixed_to_fdot6(x: SkFixed) -> SkFDot6 {
    x >> 10
}

#[inline]
pub const fn fdot6_to_fixed(x: SkFDot6) -> SkFixed {
    left_shift_i32(x, 10)
}

#[inline]
pub const fn fdot6_to_fixed_div2(value: SkFDot6) -> SkFixed {
    left_shift_i32(value, 16 - 6 - 1)
}

#[inline]
pub const fn fdot6_div(a: SkFDot6, b: SkFDot6) -> SkFixed {
    debug_assert!(b != 0);
    if a >= (i16::MIN as i32) && a <= (i16::MAX as i32) && !(a == (i16::MIN as i32) && b == -1) {
        left_shift_i32(a, 16) / b
    } else {
        fixed_div(a, b)
    }
}

#[inline]
pub const fn edge_compute_dy(top: i32, y0: SkFDot6) -> SkFDot6 {
    left_shift_i32(top, 6).wrapping_add(32).wrapping_sub(y0)
}

#[inline]
pub fn double_saturate2int(mut x: f64) -> i32 {
    if x.is_nan() {
        return SK_MAX_S32;
    }
    if x >= SK_MAX_S32 as f64 {
        x = SK_MAX_S32 as f64;
    }
    if x <= SK_MIN_S32 as f64 {
        x = SK_MIN_S32 as f64;
    }
    x as i32
}

#[inline]
pub fn float_floor2int(x: f32) -> i32 {
    float_saturate2int(x.floor())
}

#[inline]
pub fn float_ceil2int(x: f32) -> i32 {
    float_saturate2int(x.ceil())
}

#[inline]
pub fn float_round2int(x: f32) -> i32 {
    float_saturate2int((x + 0.5_f32).floor())
}

#[inline]
pub const fn xrect_from_irect(src: IRect) -> XRect {
    IRect::make_ltrb(
        int_to_fixed(src.left),
        int_to_fixed(src.top),
        int_to_fixed(src.right),
        int_to_fixed(src.bottom),
    )
}

#[inline]
pub fn xrect_from_rect(src: Rect) -> XRect {
    IRect::make_ltrb(
        float_to_fixed(src.left),
        float_to_fixed(src.top),
        float_to_fixed(src.right),
        float_to_fixed(src.bottom),
    )
}

#[inline]
pub const fn xrect_round(xr: XRect) -> IRect {
    IRect::make_ltrb(
        fixed_round_to_int(xr.left),
        fixed_round_to_int(xr.top),
        fixed_round_to_int(xr.right),
        fixed_round_to_int(xr.bottom),
    )
}

#[inline]
pub const fn xrect_round_out(xr: XRect) -> IRect {
    IRect::make_ltrb(
        fixed_floor_to_int(xr.left),
        fixed_floor_to_int(xr.top),
        fixed_ceil_to_int(xr.right),
        fixed_ceil_to_int(xr.bottom),
    )
}

#[inline]
pub const fn alpha_mul(value: u32, alpha256: u32) -> u32 {
    (value * alpha256) >> 8
}

#[inline]
pub const fn mul_div255_round(a: u32, b: u32) -> u8 {
    let prod = a * b + 128;
    ((prod + (prod >> 8)) >> 8) as u8
}

#[inline]
pub const fn align4(x: i32) -> i32 {
    (x + 3) & !3
}

/// Matches `SkIRect::contains(const SkIRect& r)`.
#[inline]
pub fn irect_contains(outer: IRect, inner: IRect) -> bool {
    !inner.is_empty()
        && !outer.is_empty()
        && outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Matches `SkIRect::makeSorted()`.
#[inline]
pub fn irect_make_sorted(r: IRect) -> IRect {
    IRect::make_ltrb(
        r.left.min(r.right),
        r.top.min(r.bottom),
        r.left.max(r.right),
        r.top.max(r.bottom),
    )
}

/// Exact safe Rust port of `SkTQSort` (`src/core/SkTSort.h`) so tie-breaking on
/// equal edge keys matches C++ Skia bit-for-bit.
pub fn sk_tqsort<T, F>(slice: &mut [T], less_than: &F)
where
    F: Fn(&T, &T) -> bool,
{
    let n = slice.len();
    if n <= 1 {
        return;
    }
    let depth = 2 * ((32 - ((n - 1) as u32).leading_zeros()) as i32);
    sk_tintro_sort(depth, slice, less_than);
}

fn sk_theap_sort_sift_up<T, F>(arr: &mut [T], mut root: usize, bottom: usize, less_than: &F)
where
    F: Fn(&T, &T) -> bool,
{
    let start = root;
    let mut j = root << 1;
    while j <= bottom {
        if j < bottom && less_than(&arr[j - 1], &arr[j]) {
            j += 1;
        }
        arr.swap(root - 1, j - 1);
        root = j;
        j = root << 1;
    }
    j = root >> 1;
    while j >= start {
        if less_than(&arr[j - 1], &arr[root - 1]) {
            arr.swap(root - 1, j - 1);
            root = j;
            j = root >> 1;
        } else {
            break;
        }
    }
}

fn sk_theap_sort_sift_down<T, F>(arr: &mut [T], mut root: usize, bottom: usize, less_than: &F)
where
    F: Fn(&T, &T) -> bool,
{
    let mut child = root << 1;
    while child <= bottom {
        if child < bottom && less_than(&arr[child - 1], &arr[child]) {
            child += 1;
        }
        if less_than(&arr[root - 1], &arr[child - 1]) {
            arr.swap(root - 1, child - 1);
            root = child;
            child = root << 1;
        } else {
            break;
        }
    }
}

fn sk_theap_sort<T, F>(arr: &mut [T], less_than: &F)
where
    F: Fn(&T, &T) -> bool,
{
    let count = arr.len();
    let mut i = count >> 1;
    while i > 0 {
        sk_theap_sort_sift_down(arr, i, count, less_than);
        i -= 1;
    }
    let mut j = count - 1;
    while j > 0 {
        arr.swap(0, j);
        sk_theap_sort_sift_up(arr, 1, j, less_than);
        j -= 1;
    }
}

fn sk_tinsertion_sort<T, F>(arr: &mut [T], less_than: &F)
where
    F: Fn(&T, &T) -> bool,
{
    for next in 1..arr.len() {
        if !less_than(&arr[next], &arr[next - 1]) {
            continue;
        }
        let mut hole = next;
        while hole > 0 && less_than(&arr[hole], &arr[hole - 1]) {
            arr.swap(hole, hole - 1);
            hole -= 1;
        }
    }
}

fn sk_tqsort_partition<T, F>(arr: &mut [T], pivot: usize, less_than: &F) -> usize
where
    F: Fn(&T, &T) -> bool,
{
    let right = arr.len() - 1;
    arr.swap(pivot, right);
    let mut new_pivot = 0usize;
    for left in 0..right {
        if less_than(&arr[left], &arr[right]) {
            arr.swap(left, new_pivot);
            new_pivot += 1;
        }
    }
    arr.swap(new_pivot, right);
    new_pivot
}

fn sk_tintro_sort<T, F>(mut depth: i32, mut arr: &mut [T], less_than: &F)
where
    F: Fn(&T, &T) -> bool,
{
    loop {
        let count = arr.len();
        if count <= 32 {
            sk_tinsertion_sort(arr, less_than);
            return;
        }
        if depth == 0 {
            sk_theap_sort(arr, less_than);
            return;
        }
        depth -= 1;

        let middle = (count - 1) >> 1;
        let pivot = sk_tqsort_partition(arr, middle, less_than);
        let (left_part, rest) = arr.split_at_mut(pivot);
        sk_tintro_sort(depth, left_part, less_than);
        arr = &mut rest[1..];
    }
}
