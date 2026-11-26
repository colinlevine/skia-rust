#![forbid(unsafe_code)]

use crate::{float_midpoint, scalar_nearly_zero, Point, Rect, SK_SCALAR_NEARLY_ZERO};

pub const LINE_CLIPPER_MAX_POINTS: usize = 4;

#[inline]
fn pin_unsorted_f64(value: f64, mut limit0: f64, mut limit1: f64) -> f64 {
    if limit1 < limit0 {
        std::mem::swap(&mut limit0, &mut limit1);
    }
    if value < limit0 {
        limit0
    } else if value > limit1 {
        limit1
    } else {
        value
    }
}

#[inline]
fn pin_unsorted_f32(value: f32, mut limit0: f32, mut limit1: f32) -> f32 {
    if limit1 < limit0 {
        std::mem::swap(&mut limit0, &mut limit1);
    }
    if value < limit0 {
        limit0
    } else if value > limit1 {
        limit1
    } else {
        value
    }
}

fn sect_with_horizontal(src: &[Point; 2], y: f32) -> f32 {
    let dy = src[1].y - src[0].y;
    if scalar_nearly_zero(dy, SK_SCALAR_NEARLY_ZERO) {
        float_midpoint(src[0].x, src[1].x)
    } else {
        let x0 = src[0].x as f64;
        let y0 = src[0].y as f64;
        let x1 = src[1].x as f64;
        let y1 = src[1].y as f64;
        let result = x0 + ((y as f64) - y0) * (x1 - x0) / (y1 - y0);
        pin_unsorted_f64(result, x0, x1) as f32
    }
}

fn sect_with_vertical(src: &[Point; 2], x: f32) -> f32 {
    let dx = src[1].x - src[0].x;
    if scalar_nearly_zero(dx, SK_SCALAR_NEARLY_ZERO) {
        float_midpoint(src[0].y, src[1].y)
    } else {
        let x0 = src[0].x as f64;
        let y0 = src[0].y as f64;
        let x1 = src[1].x as f64;
        let y1 = src[1].y as f64;
        let result = y0 + ((x as f64) - x0) * (y1 - y0) / (x1 - x0);
        result as f32
    }
}

fn sect_clamp_with_vertical(src: &[Point; 2], x: f32) -> f32 {
    let y = sect_with_vertical(src, x);
    pin_unsorted_f32(y, src[0].y, src[1].y)
}

#[inline]
fn nested_lt(a: f32, b: f32, dim: f32) -> bool {
    a <= b && (a < b || dim > 0.0)
}

#[inline]
fn contains_no_empty_check(outer: Rect, inner: Rect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

#[inline]
fn rect_from_two_points(p0: Point, p1: Point) -> Rect {
    Rect::make_ltrb(p0.x, p0.y, p1.x, p1.y).make_sorted()
}

pub fn intersect_line(src: &[Point; 2], clip: Rect, dst: &mut [Point; 2]) -> bool {
    let bounds = rect_from_two_points(src[0], src[1]);
    if contains_no_empty_check(clip, bounds) {
        *dst = *src;
        return true;
    }
    if nested_lt(bounds.right, clip.left, bounds.width())
        || nested_lt(clip.right, bounds.left, bounds.width())
        || nested_lt(bounds.bottom, clip.top, bounds.height())
        || nested_lt(clip.bottom, bounds.top, bounds.height())
    {
        return false;
    }

    let (mut index0, mut index1) = if src[0].y < src[1].y { (0, 1) } else { (1, 0) };
    let mut tmp = *src;

    if tmp[index0].y < clip.top {
        tmp[index0].set(sect_with_horizontal(src, clip.top), clip.top);
    }
    if tmp[index1].y > clip.bottom {
        tmp[index1].set(sect_with_horizontal(src, clip.bottom), clip.bottom);
    }

    if tmp[0].x < tmp[1].x {
        index0 = 0;
        index1 = 1;
    } else {
        index0 = 1;
        index1 = 0;
    }

    if tmp[index1].x <= clip.left || tmp[index0].x >= clip.right {
        if tmp[0].x != tmp[1].x || tmp[0].x < clip.left || tmp[0].x > clip.right {
            return false;
        }
    }

    if tmp[index0].x < clip.left {
        tmp[index0].set(clip.left, sect_with_vertical(&tmp, clip.left));
    }
    if tmp[index1].x > clip.right {
        tmp[index1].set(clip.right, sect_with_vertical(&tmp, clip.right));
    }
    *dst = tmp;
    true
}

pub fn clip_line(
    pts: &[Point; 2],
    clip: Rect,
    lines: &mut [Point; LINE_CLIPPER_MAX_POINTS],
    can_cull_to_the_right: bool,
) -> i32 {
    let (mut index0, mut index1) = if pts[0].y < pts[1].y { (0, 1) } else { (1, 0) };

    if pts[index1].y <= clip.top || pts[index0].y >= clip.bottom {
        return 0;
    }

    let mut tmp = *pts;
    if pts[index0].y < clip.top {
        tmp[index0].set(sect_with_horizontal(pts, clip.top), clip.top);
    }
    if tmp[index1].y > clip.bottom {
        tmp[index1].set(sect_with_horizontal(pts, clip.bottom), clip.bottom);
    }

    let mut result = [Point::ZERO; LINE_CLIPPER_MAX_POINTS];
    let line_count: usize;
    let mut reverse: bool;

    if pts[0].x < pts[1].x {
        index0 = 0;
        index1 = 1;
        reverse = false;
    } else {
        index0 = 1;
        index1 = 0;
        reverse = true;
    }

    if tmp[index1].x <= clip.left {
        tmp[0].x = clip.left;
        tmp[1].x = clip.left;
        result[0] = tmp[0];
        result[1] = tmp[1];
        line_count = 1;
        reverse = false;
    } else if tmp[index0].x >= clip.right {
        if can_cull_to_the_right {
            return 0;
        }
        tmp[0].x = clip.right;
        tmp[1].x = clip.right;
        result[0] = tmp[0];
        result[1] = tmp[1];
        line_count = 1;
        reverse = false;
    } else {
        let mut r = 0usize;
        if tmp[index0].x < clip.left {
            result[r].set(clip.left, tmp[index0].y);
            r += 1;
            result[r].set(clip.left, sect_clamp_with_vertical(&tmp, clip.left));
        } else {
            result[r] = tmp[index0];
        }
        r += 1;
        if tmp[index1].x > clip.right {
            result[r].set(clip.right, sect_clamp_with_vertical(&tmp, clip.right));
            r += 1;
            result[r].set(clip.right, tmp[index1].y);
        } else {
            result[r] = tmp[index1];
        }
        line_count = r;
    }

    if reverse {
        for i in 0..=line_count {
            lines[line_count - i] = result[i];
        }
    } else {
        lines[..=line_count].copy_from_slice(&result[..=line_count]);
    }
    line_count as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_clipper() {
        let clip = Rect::make_ltrb(0.0, 0.0, 10.0, 10.0);
        let src = [Point::new(-5.0, 5.0), Point::new(15.0, 5.0)];
        let mut dst = [Point::ZERO; 2];
        assert!(intersect_line(&src, clip, &mut dst));
        assert_eq!(dst, [Point::new(0.0, 5.0), Point::new(10.0, 5.0)]);
    }
}
