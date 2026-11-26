#![forbid(unsafe_code)]

use crate::{cpp_max_f32, cpp_min_f32, float_midpoint, pin_i32, M44, Point};

/// Matches `SkIRect` (`{ int32_t fLeft, fTop, fRight, fBottom; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Matches `SkRect` (`{ float fLeft, fTop, fRight, fBottom; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl IRect {
    #[inline]
    pub const fn make_empty() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        }
    }

    #[inline]
    pub const fn make_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        let w = (self.right as i64) - (self.left as i64);
        let h = (self.bottom as i64) - (self.top as i64);
        if w <= 0 || h <= 0 {
            return true;
        }
        (w | h) > (i32::MAX as i64)
    }

    #[inline]
    pub fn width(self) -> i32 {
        self.right.wrapping_sub(self.left)
    }

    #[inline]
    pub fn height(self) -> i32 {
        self.bottom.wrapping_sub(self.top)
    }

    #[inline]
    pub fn intersects(a: Self, b: Self) -> bool {
        let mut dummy = Self::make_empty();
        dummy.intersect_pair(a, b)
    }

    pub fn intersect_pair(&mut self, a: Self, b: Self) -> bool {
        let tmp = Self {
            left: a.left.max(b.left),
            top: a.top.max(b.top),
            right: a.right.min(b.right),
            bottom: a.bottom.min(b.bottom),
        };
        if tmp.is_empty() {
            return false;
        }
        *self = tmp;
        true
    }

    pub fn join(&mut self, r: Self) {
        if r.left >= r.right || r.top >= r.bottom {
            return;
        }
        if self.left >= self.right || self.top >= self.bottom {
            *self = r;
        } else {
            if r.left < self.left {
                self.left = r.left;
            }
            if r.top < self.top {
                self.top = r.top;
            }
            if r.right > self.right {
                self.right = r.right;
            }
            if r.bottom > self.bottom {
                self.bottom = r.bottom;
            }
        }
    }

    pub fn subtract(a: Self, b: Self) -> (Self, bool) {
        if a.is_empty() || b.is_empty() || !Self::intersects(a, b) {
            return (a, true);
        }
        let a_height = a.height() as f32;
        let a_width = a.width() as f32;
        let mut left_area = 0.0_f32;
        let mut right_area = 0.0_f32;
        let mut top_area = 0.0_f32;
        let mut bottom_area = 0.0_f32;
        let mut positive_count = 0;
        if b.left > a.left {
            left_area = (b.left.wrapping_sub(a.left) as f32) / a_width;
            positive_count += 1;
        }
        if a.right > b.right {
            right_area = (a.right.wrapping_sub(b.right) as f32) / a_width;
            positive_count += 1;
        }
        if b.top > a.top {
            top_area = (b.top.wrapping_sub(a.top) as f32) / a_height;
            positive_count += 1;
        }
        if a.bottom > b.bottom {
            bottom_area = (a.bottom.wrapping_sub(b.bottom) as f32) / a_height;
            positive_count += 1;
        }

        if positive_count == 0 {
            return (Self::make_empty(), true);
        }

        let mut out = a;
        if left_area > right_area && left_area > top_area && left_area > bottom_area {
            out.right = b.left;
        } else if right_area > top_area && right_area > bottom_area {
            out.left = b.right;
        } else if top_area > bottom_area {
            out.bottom = b.top;
        } else {
            out.top = b.bottom;
        }
        (out, positive_count == 1)
    }

    pub fn closest_disjoint_edge(src: Self, dst: Self) -> Self {
        if src.is_empty() || dst.is_empty() {
            return Self::make_empty();
        }
        let mut l = src.left;
        let mut r = src.right;
        if r <= dst.left {
            l = r.wrapping_sub(1);
        } else if l >= dst.right {
            r = l.wrapping_add(1);
        } else {
            l = pin_i32(l, dst.left, dst.right);
            r = pin_i32(r, dst.left, dst.right);
        }

        let mut t = src.top;
        let mut b = src.bottom;
        if b <= dst.top {
            t = b.wrapping_sub(1);
        } else if t >= dst.bottom {
            b = t.wrapping_add(1);
        } else {
            t = pin_i32(t, dst.top, dst.bottom);
            b = pin_i32(b, dst.top, dst.bottom);
        }
        Self::make_ltrb(l, t, r, b)
    }
}

impl Rect {
    #[inline]
    pub const fn make_empty() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        }
    }

    #[inline]
    pub const fn make_ltrb(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[inline]
    pub const fn from_irect(r: IRect) -> Self {
        Self {
            left: r.left as f32,
            top: r.top as f32,
            right: r.right as f32,
            bottom: r.bottom as f32,
        }
    }

    #[inline]
    pub fn set_ltrb(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.left = left;
        self.top = top;
        self.right = right;
        self.bottom = bottom;
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        !(self.left < self.right && self.top < self.bottom)
    }

    #[inline]
    pub fn is_sorted(self) -> bool {
        self.left <= self.right && self.top <= self.bottom
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        let mut accum = 0.0_f32;
        accum *= self.left;
        accum *= self.top;
        accum *= self.right;
        accum *= self.bottom;
        accum == 0.0
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    #[inline]
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }

    #[inline]
    pub fn half_width(self) -> f32 {
        float_midpoint(-self.left, self.right)
    }

    #[inline]
    pub fn half_height(self) -> f32 {
        float_midpoint(-self.top, self.bottom)
    }

    #[inline]
    pub fn center_x(self) -> f32 {
        float_midpoint(self.left, self.right)
    }

    #[inline]
    pub fn center_y(self) -> f32 {
        float_midpoint(self.top, self.bottom)
    }

    #[inline]
    pub fn make_sorted(self) -> Self {
        Self {
            left: cpp_min_f32(self.left, self.right),
            top: cpp_min_f32(self.top, self.bottom),
            right: cpp_max_f32(self.left, self.right),
            bottom: cpp_max_f32(self.top, self.bottom),
        }
    }

    #[inline]
    pub fn make_inset(self, dx: f32, dy: f32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right - dx,
            bottom: self.bottom - dy,
        }
    }

    #[inline]
    pub fn make_offset(self, dx: f32, dy: f32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }

    #[inline]
    pub fn to_quad(self) -> [Point; 4] {
        [
            Point::new(self.left, self.top),
            Point::new(self.right, self.top),
            Point::new(self.right, self.bottom),
            Point::new(self.left, self.bottom),
        ]
    }

    #[inline]
    pub fn contains_xy(self, x: f32, y: f32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    #[inline]
    pub fn contains_rect(self, r: Self) -> bool {
        !r.is_empty()
            && !self.is_empty()
            && self.left <= r.left
            && self.top <= r.top
            && self.right >= r.right
            && self.bottom >= r.bottom
    }

    #[inline]
    pub fn intersects(a: Self, b: Self) -> bool {
        let l = cpp_max_f32(a.left, b.left);
        let r = cpp_min_f32(a.right, b.right);
        let t = cpp_max_f32(a.top, b.top);
        let bot = cpp_min_f32(a.bottom, b.bottom);
        l < r && t < bot
    }

    pub fn bounds(points: &[Point]) -> Option<Self> {
        if points.is_empty() {
            return Some(Self::make_empty());
        }
        let mut l = points[0].x;
        let mut t = points[0].y;
        let mut r = points[0].x;
        let mut b = points[0].y;
        let mut nx = 0.0_f32;
        let mut ny = 0.0_f32;
        for &p in points {
            l = p.x.min(l);
            t = p.y.min(t);
            r = p.x.max(r);
            b = p.y.max(b);
            nx *= p.x;
            ny *= p.y;
        }
        if nx == 0.0 && ny == 0.0 {
            Some(Self::make_ltrb(l, t, r, b))
        } else {
            None
        }
    }

    #[inline]
    pub fn bounds_or_empty(points: &[Point]) -> Self {
        Self::bounds(points).unwrap_or_else(Self::make_empty)
    }

    pub fn set_bounds_check(&mut self, pts: &[Point]) -> bool {
        if let Some(b) = Self::bounds(pts) {
            *self = b;
            true
        } else {
            *self = Self::make_empty();
            false
        }
    }

    pub fn set_bounds_no_check(&mut self, pts: &[Point]) {
        if let Some(b) = Self::bounds(pts) {
            *self = b;
        } else {
            self.set_ltrb(f32::NAN, f32::NAN, f32::NAN, f32::NAN);
        }
    }

    pub fn intersect(&mut self, r: Self) -> bool {
        let l = cpp_max_f32(r.left, self.left);
        let rr = cpp_min_f32(r.right, self.right);
        let t = cpp_max_f32(r.top, self.top);
        let b = cpp_min_f32(r.bottom, self.bottom);
        if !(l < rr && t < b) {
            return false;
        }
        self.set_ltrb(l, t, rr, b);
        true
    }

    pub fn intersect_pair(&mut self, a: Self, b: Self) -> bool {
        let l = cpp_max_f32(a.left, b.left);
        let r = cpp_min_f32(a.right, b.right);
        let t = cpp_max_f32(a.top, b.top);
        let bot = cpp_min_f32(a.bottom, b.bottom);
        if !(l < r && t < bot) {
            return false;
        }
        self.set_ltrb(l, t, r, bot);
        true
    }

    pub fn join(&mut self, r: Self) {
        if r.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = r;
        } else {
            self.left = cpp_min_f32(self.left, r.left);
            self.top = cpp_min_f32(self.top, r.top);
            self.right = cpp_max_f32(self.right, r.right);
            self.bottom = cpp_max_f32(self.bottom, r.bottom);
        }
    }

    pub fn subtract(a: Self, b: Self) -> (Self, bool) {
        if a.is_empty() || b.is_empty() || !Self::intersects(a, b) {
            return (a, true);
        }
        let a_height = a.height();
        let a_width = a.width();
        let mut left_area = 0.0_f32;
        let mut right_area = 0.0_f32;
        let mut top_area = 0.0_f32;
        let mut bottom_area = 0.0_f32;
        let mut positive_count = 0;
        if b.left > a.left {
            left_area = (b.left - a.left) / a_width;
            positive_count += 1;
        }
        if a.right > b.right {
            right_area = (a.right - b.right) / a_width;
            positive_count += 1;
        }
        if b.top > a.top {
            top_area = (b.top - a.top) / a_height;
            positive_count += 1;
        }
        if a.bottom > b.bottom {
            bottom_area = (a.bottom - b.bottom) / a_height;
            positive_count += 1;
        }

        if positive_count == 0 {
            return (Self::make_empty(), true);
        }

        let mut out = a;
        if left_area > right_area && left_area > top_area && left_area > bottom_area {
            out.right = b.left;
        } else if right_area > top_area && right_area > bottom_area {
            out.left = b.right;
        } else if top_area > bottom_area {
            out.bottom = b.top;
        } else {
            out.top = b.bottom;
        }
        (out, positive_count == 1)
    }

    pub fn quad_contains_rect_mask(m: &M44, a: Self, b: Self, tol: f32) -> [i32; 4] {
        if a.is_empty() {
            return [0, 0, 0, 0];
        }
        let ax = [a.left, a.right, a.right, a.left];
        let ay = [a.top, a.top, a.bottom, a.bottom];

        let mut max = [0.0_f32; 4];
        let mut may = [0.0_f32; 4];
        let mut maw = [0.0_f32; 4];
        for i in 0..4 {
            max[i] = m.rc(0, 0) * ax[i] + m.rc(0, 1) * ay[i] + m.rc(0, 3);
            may[i] = m.rc(1, 0) * ax[i] + m.rc(1, 1) * ay[i] + m.rc(1, 3);
            maw[i] = m.rc(3, 0) * ax[i] + m.rc(3, 1) * ay[i] + m.rc(3, 3);
        }

        if maw.iter().all(|&w| w < 0.0) {
            return [0, 0, 0, 0];
        }

        let next = [1, 2, 3, 0];
        let mut l_a = [0.0_f32; 4];
        let mut l_b = [0.0_f32; 4];
        let mut l_c = [0.0_f32; 4];
        for i in 0..4 {
            let j = next[i];
            l_a[i] = may[i] * maw[j] - maw[i] * may[j];
            l_b[i] = maw[i] * max[j] - max[i] * maw[j];
            l_c[i] = max[i] * may[j] - may[i] * max[j];
        }

        let sign = if (l_a[0] * l_b[1] - l_b[0] * l_a[1]) < 0.0 {
            -1.0_f32
        } else {
            1.0_f32
        };

        let b_inset = b.make_inset(tol, tol);
        let mut mask = [0_i32; 4];
        for i in 0..4 {
            let d0 = sign * (l_a[i] * b_inset.left + l_b[i] * b_inset.top + l_c[i]);
            let d1 = sign * (l_a[i] * b_inset.right + l_b[i] * b_inset.top + l_c[i]);
            let d2 = sign * (l_a[i] * b_inset.right + l_b[i] * b_inset.bottom + l_c[i]);
            let d3 = sign * (l_a[i] * b_inset.left + l_b[i] * b_inset.bottom + l_c[i]);
            if d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0 {
                mask[i] = -1;
            }
        }
        mask
    }

    pub fn quad_contains_rect(m: &M44, a: Self, b: Self, tol: f32) -> bool {
        let mask = Self::quad_contains_rect_mask(m, a, b, tol);
        mask[0] != 0 && mask[1] != 0 && mask[2] != 0 && mask[3] != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_bounds_and_ops() {
        let pts = [Point::new(1.0, 5.0), Point::new(-2.0, 3.0), Point::new(4.0, 1.0)];
        let b = Rect::bounds(&pts).unwrap();
        assert_eq!(b, Rect::make_ltrb(-2.0, 1.0, 4.0, 5.0));

        let bad = [Point::new(1.0, f32::INFINITY)];
        assert!(Rect::bounds(&bad).is_none());

        let mut r = Rect::make_ltrb(0.0, 0.0, 10.0, 10.0);
        assert!(r.intersect(Rect::make_ltrb(5.0, 2.0, 15.0, 8.0)));
        assert_eq!(r, Rect::make_ltrb(5.0, 2.0, 10.0, 8.0));

        let (diff, exact) = Rect::subtract(
            Rect::make_ltrb(0.0, 0.0, 10.0, 10.0),
            Rect::make_ltrb(0.0, 0.0, 4.0, 10.0),
        );
        assert!(exact);
        assert_eq!(diff, Rect::make_ltrb(4.0, 0.0, 10.0, 10.0));
    }

    #[test]
    fn test_irect_closest_disjoint_edge() {
        let src = IRect::make_ltrb(0, 0, 10, 10);
        let dst = IRect::make_ltrb(20, 2, 30, 8);
        assert_eq!(IRect::closest_disjoint_edge(src, dst), IRect::make_ltrb(9, 2, 10, 8));
    }
}
