//! Safe Rust implementation of `SkLatticeIter` (`src/core/SkLatticeIter.cpp`).

use skia_rs_geometry::{
    matrix::{MSCALE_X, MSCALE_Y, MTRANS_X, MTRANS_Y},
    IRect, Matrix, Rect,
};

/// Matches `SkCanvas::Lattice::RectType`.
pub const RECT_TYPE_DEFAULT: u8 = 0;
pub const RECT_TYPE_TRANSPARENT: u8 = 1;
pub const RECT_TYPE_FIXED_COLOR: u8 = 2;

/// Divs must be in strictly increasing order with no duplicates, within `[start, end]`.
#[inline]
pub fn valid_divs(divs: &[i32], start: i32, end: i32) -> bool {
    let mut prev = start - 1;
    for &d in divs {
        if prev >= d || d > end {
            return false;
        }
        prev = d;
    }
    true
}

/// Checks whether `total` contains `sub` (matching `SkIRect::contains(const SkIRect&)`).
#[inline]
fn irect_contains(total: IRect, sub: IRect) -> bool {
    !sub.is_empty()
        && !total.is_empty()
        && total.left <= sub.left
        && total.top <= sub.top
        && total.right >= sub.right
        && total.bottom >= sub.bottom
}

/// Matches `SkLatticeIter::Valid(int width, int height, const SkCanvas::Lattice& lattice)`.
pub fn valid_lattice(
    width: i32,
    height: i32,
    bounds: IRect,
    x_divs: &[i32],
    y_divs: &[i32],
) -> bool {
    let total_bounds = IRect::from_wh(width, height);
    if !irect_contains(total_bounds, bounds) {
        return false;
    }

    let zero_x_divs = x_divs.is_empty() || (x_divs.len() == 1 && bounds.left == x_divs[0]);
    let zero_y_divs = y_divs.is_empty() || (y_divs.len() == 1 && bounds.top == y_divs[0]);
    if zero_x_divs && zero_y_divs {
        return false;
    }

    valid_divs(x_divs, bounds.left, bounds.right) && valid_divs(y_divs, bounds.top, bounds.bottom)
}

/// Matches `SkLatticeIter::Valid(int width, int height, const SkIRect& center)`.
pub fn valid_nine(width: i32, height: i32, center: IRect) -> bool {
    !center.is_empty() && irect_contains(IRect::from_wh(width, height), center)
}

/// Count the number of pixels in "scalable" patches along one axis.
pub fn count_scalable_pixels(
    divs: &[i32],
    first_is_scalable: bool,
    start: i32,
    end: i32,
) -> i32 {
    if divs.is_empty() {
        return if first_is_scalable { end - start } else { 0 };
    }

    let (mut count, mut i) = if first_is_scalable {
        (divs[0] - start, 1usize)
    } else {
        (0i32, 0usize)
    };

    while i < divs.len() {
        let left = divs[i];
        let right = if i + 1 < divs.len() { divs[i + 1] } else { end };
        count += right - left;
        i += 2;
    }

    count
}

/// Compute `src` and `dst` coordinate arrays along one axis.
#[allow(clippy::too_many_arguments)]
fn set_points(
    divs: &[i32],
    src_fixed: i32,
    src_scalable: i32,
    src_start: i32,
    src_end: i32,
    dst_start: f32,
    dst_end: f32,
    mut is_scalable: bool,
) -> (Vec<i32>, Vec<f32>) {
    let div_count = divs.len();
    let dst_len = dst_end - dst_start;
    let scale = if (src_fixed as f32) <= dst_len {
        (dst_len - (src_fixed as f32)) / (src_scalable as f32)
    } else {
        dst_len / (src_fixed as f32)
    };

    let mut src = vec![0i32; div_count + 2];
    let mut dst = vec![0.0f32; div_count + 2];
    src[0] = src_start;
    dst[0] = dst_start;

    for i in 0..div_count {
        src[i + 1] = divs[i];
        let src_delta = (src[i + 1] - src[i]) as f32;
        let dst_delta = if (src_fixed as f32) <= dst_len {
            if is_scalable {
                scale * src_delta
            } else {
                src_delta
            }
        } else if is_scalable {
            0.0f32
        } else {
            scale * src_delta
        };
        dst[i + 1] = dst[i] + dst_delta;
        is_scalable = !is_scalable;
    }

    src[div_count + 1] = src_end;
    dst[div_count + 1] = dst_end;
    (src, dst)
}

/// Precomputed lattice arrays and iteration counts matching `SkLatticeIter`.
#[derive(Clone, Debug, PartialEq)]
pub struct LatticePlan {
    pub src_x: Vec<i32>,
    pub src_y: Vec<i32>,
    pub dst_x: Vec<f32>,
    pub dst_y: Vec<f32>,
    pub rect_types: Vec<u8>,
    pub colors: Vec<u32>,
    pub num_rects_in_lattice: i32,
    pub num_rects_to_draw: i32,
}

impl LatticePlan {
    /// Builds a `LatticePlan` matching `SkLatticeIter::SkLatticeIter(const SkCanvas::Lattice&, const SkRect&)`.
    pub fn from_lattice(
        bounds: IRect,
        x_divs_in: &[i32],
        y_divs_in: &[i32],
        rect_types_in: Option<&[u8]>,
        colors_in: Option<&[u32]>,
        dst: Rect,
    ) -> Self {
        let orig_x_count = x_divs_in.len();
        let orig_y_count = y_divs_in.len();

        let x_is_scalable = orig_x_count > 0 && bounds.left == x_divs_in[0];
        let x_divs = if x_is_scalable {
            &x_divs_in[1..]
        } else {
            x_divs_in
        };
        let x_count = x_divs.len();

        let y_is_scalable = orig_y_count > 0 && bounds.top == y_divs_in[0];
        let y_divs = if y_is_scalable {
            &y_divs_in[1..]
        } else {
            y_divs_in
        };
        let y_count = y_divs.len();

        let x_count_scalable =
            count_scalable_pixels(x_divs, x_is_scalable, bounds.left, bounds.right);
        let x_count_fixed = bounds.width() - x_count_scalable;
        let y_count_scalable =
            count_scalable_pixels(y_divs, y_is_scalable, bounds.top, bounds.bottom);
        let y_count_fixed = bounds.height() - y_count_scalable;

        let (src_x, dst_x) = set_points(
            x_divs,
            x_count_fixed,
            x_count_scalable,
            bounds.left,
            bounds.right,
            dst.left,
            dst.right,
            x_is_scalable,
        );
        let (src_y, dst_y) = set_points(
            y_divs,
            y_count_fixed,
            y_count_scalable,
            bounds.top,
            bounds.bottom,
            dst.top,
            dst.bottom,
            y_is_scalable,
        );

        let num_rects_in_lattice = ((x_count + 1) * (y_count + 1)) as i32;
        let mut num_rects_to_draw = num_rects_in_lattice;

        let mut rect_types = Vec::new();
        let mut colors = Vec::new();

        if let Some(in_flags) = rect_types_in {
            let n = num_rects_in_lattice as usize;
            rect_types.resize(n, 0);
            colors.resize(n, 0);

            let has_pad_row = y_count != orig_y_count;
            let has_pad_col = x_count != orig_x_count;
            let mut idx_in = if has_pad_row { orig_x_count + 1 } else { 0 };
            let mut i = 0usize;

            for _y in 0..(y_count + 1) {
                for x in 0..(orig_x_count + 1) {
                    if x == 0 && has_pad_col {
                        idx_in += 1;
                        continue;
                    }
                    let flag = in_flags[idx_in];
                    rect_types[i] = flag;
                    colors[i] = if flag == RECT_TYPE_FIXED_COLOR {
                        colors_in.map_or(0, |c| c[idx_in])
                    } else {
                        0
                    };
                    idx_in += 1;
                    i += 1;
                }
            }

            for &rt in &rect_types {
                if rt == RECT_TYPE_TRANSPARENT {
                    num_rects_to_draw -= 1;
                }
            }
        }

        Self {
            src_x,
            src_y,
            dst_x,
            dst_y,
            rect_types,
            colors,
            num_rects_in_lattice,
            num_rects_to_draw,
        }
    }

    /// Builds a 9-patch `LatticePlan` matching `SkLatticeIter::SkLatticeIter(int w, int h, const SkIRect& c, const SkRect& dst)`.
    pub fn from_nine(w: i32, h: i32, c: IRect, dst: Rect) -> Self {
        let src_x = vec![0, c.left, c.right, w];
        let src_y = vec![0, c.top, c.bottom, h];

        let mut dst_x = vec![
            dst.left,
            dst.left + (c.left as f32),
            dst.right - ((w - c.right) as f32),
            dst.right,
        ];
        let mut dst_y = vec![
            dst.top,
            dst.top + (c.top as f32),
            dst.bottom - ((h - c.bottom) as f32),
            dst.bottom,
        ];

        if dst_x[1] > dst_x[2] {
            dst_x[1] = dst_x[0]
                + (dst_x[3] - dst_x[0]) * (c.left as f32) / ((w - c.width()) as f32);
            dst_x[2] = dst_x[1];
        }
        if dst_y[1] > dst_y[2] {
            dst_y[1] = dst_y[0]
                + (dst_y[3] - dst_y[0]) * (c.top as f32) / ((h - c.height()) as f32);
            dst_y[2] = dst_y[1];
        }

        Self {
            src_x,
            src_y,
            dst_x,
            dst_y,
            rect_types: Vec::new(),
            colors: Vec::new(),
            num_rects_in_lattice: 9,
            num_rects_to_draw: 9,
        }
    }
}

/// Output of a single `lattice_next` step.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatticeNextOut {
    pub src: IRect,
    pub dst: Rect,
    pub is_fixed_color: bool,
    pub fixed_color: u32,
}

/// Advances `(curr_x, curr_y)` across the lattice, skipping transparent cells.
/// Matches `SkLatticeIter::next`.
#[allow(clippy::too_many_arguments)]
pub fn lattice_next(
    src_x: &[i32],
    src_y: &[i32],
    dst_x: &[f32],
    dst_y: &[f32],
    rect_types: &[u8],
    colors: &[u32],
    num_rects_in_lattice: i32,
    curr_x: &mut i32,
    curr_y: &mut i32,
) -> Option<LatticeNextOut> {
    let cols = (src_x.len() as i32) - 1;
    loop {
        let curr_rect = *curr_x + *curr_y * cols;
        if curr_rect == num_rects_in_lattice {
            return None;
        }

        let x = *curr_x as usize;
        let y = *curr_y as usize;

        *curr_x += 1;
        if cols == *curr_x {
            *curr_x = 0;
            *curr_y += 1;
        }

        let rect_idx = curr_rect as usize;
        if !rect_types.is_empty() && rect_types[rect_idx] == RECT_TYPE_TRANSPARENT {
            continue;
        }

        let src = IRect::make_ltrb(src_x[x], src_y[y], src_x[x + 1], src_y[y + 1]);
        let dst = Rect::make_ltrb(dst_x[x], dst_y[y], dst_x[x + 1], dst_y[y + 1]);
        let is_fixed_color = !rect_types.is_empty() && rect_types[rect_idx] == RECT_TYPE_FIXED_COLOR;
        let fixed_color = if is_fixed_color { colors[rect_idx] } else { 0 };

        return Some(LatticeNextOut {
            src,
            dst,
            is_fixed_color,
            fixed_color,
        });
    }
}

/// Applies scale+translate matrix components to `dst_x` and `dst_y`.
/// Matches `SkLatticeIter::mapDstScaleTranslate`.
pub fn map_dst_scale_translate(dst_x: &mut [f32], dst_y: &mut [f32], matrix: &Matrix) {
    let sx = matrix.mat[MSCALE_X];
    let tx = matrix.mat[MTRANS_X];
    for v in dst_x.iter_mut() {
        *v = *v * sx + tx;
    }

    let sy = matrix.mat[MSCALE_Y];
    let ty = matrix.mat[MTRANS_Y];
    for v in dst_y.iter_mut() {
        *v = *v * sy + ty;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_divs_and_lattice() {
        assert!(valid_divs(&[1, 3, 5], 0, 10));
        assert!(valid_divs(&[0, 10], 0, 10));
        assert!(!valid_divs(&[3, 3], 0, 10));
        assert!(!valid_divs(&[5, 3], 0, 10));
        assert!(!valid_divs(&[-1, 5], 0, 10));
        assert!(!valid_divs(&[1, 11], 0, 10));

        let b = IRect::from_wh(10, 10);
        assert!(valid_lattice(10, 10, b, &[3, 7], &[2, 8]));
        // Both axes zero divs -> invalid
        assert!(!valid_lattice(10, 10, b, &[], &[]));
        assert!(!valid_lattice(10, 10, b, &[0], &[0]));
        // One axis has divs -> valid
        assert!(valid_lattice(10, 10, b, &[0], &[5]));
        // Out of bounds -> invalid
        assert!(!valid_lattice(10, 10, IRect::from_wh(12, 10), &[5], &[5]));
    }

    #[test]
    fn test_valid_nine_and_from_nine() {
        assert!(valid_nine(10, 10, IRect::make_ltrb(2, 2, 8, 8)));
        assert!(!valid_nine(10, 10, IRect::make_ltrb(5, 2, 5, 8)));
        assert!(!valid_nine(10, 10, IRect::make_ltrb(0, 0, 11, 10)));

        // Normal 9-patch expansion
        let plan = LatticePlan::from_nine(
            10,
            10,
            IRect::make_ltrb(2, 3, 7, 8),
            Rect::make_ltrb(0.0, 0.0, 100.0, 50.0),
        );
        assert_eq!(plan.src_x, vec![0, 2, 7, 10]);
        assert_eq!(plan.dst_x, vec![0.0, 2.0, 97.0, 100.0]);
        assert_eq!(plan.dst_y, vec![0.0, 3.0, 48.0, 50.0]);
        assert_eq!(plan.num_rects_to_draw, 9);

        // Collapsed 9-patch (dst smaller than fixed margins)
        let small = LatticePlan::from_nine(
            10,
            10,
            IRect::make_ltrb(4, 4, 6, 6),
            Rect::make_ltrb(0.0, 0.0, 4.0, 4.0),
        );
        assert_eq!(small.dst_x, vec![0.0, 2.0, 2.0, 4.0]);
        assert_eq!(small.dst_y, vec![0.0, 2.0, 2.0, 4.0]);
    }

    #[test]
    fn test_lattice_transparent_and_fixed_color() {
        let bounds = IRect::from_wh(10, 10);
        // x_divs starts at 0 => first col is degenerate (padded), so 2 cols remain: [0..5], [5..10]
        // y_divs = [5] => 2 rows: [0..5], [5..10]
        let x_divs = [0, 5];
        let y_divs = [5];
        // orig grid is (2+1)x(1+1) = 3x2 = 6 cells; col 0 is skipped -> 4 active cells
        let rect_types = [
            RECT_TYPE_DEFAULT,
            RECT_TYPE_TRANSPARENT,
            RECT_TYPE_FIXED_COLOR,
            RECT_TYPE_DEFAULT,
            RECT_TYPE_DEFAULT,
            RECT_TYPE_FIXED_COLOR,
        ];
        let colors = [0, 0, 0xFF112233, 0, 0, 0xFF445566];
        let mut plan = LatticePlan::from_lattice(
            bounds,
            &x_divs,
            &y_divs,
            Some(&rect_types),
            Some(&colors),
            Rect::make_ltrb(0.0, 0.0, 20.0, 20.0),
        );
        assert_eq!(plan.num_rects_in_lattice, 4);
        assert_eq!(plan.num_rects_to_draw, 3);

        let mut cx = 0;
        let mut cy = 0;
        let mut emitted = Vec::new();
        while let Some(out) = lattice_next(
            &plan.src_x,
            &plan.src_y,
            &plan.dst_x,
            &plan.dst_y,
            &plan.rect_types,
            &plan.colors,
            plan.num_rects_in_lattice,
            &mut cx,
            &mut cy,
        ) {
            emitted.push(out);
        }
        assert_eq!(emitted.len(), 3);
        assert!(emitted[0].is_fixed_color);
        assert_eq!(emitted[0].fixed_color, 0xFF112233);
        assert!(!emitted[1].is_fixed_color);
        assert!(emitted[2].is_fixed_color);
        assert_eq!(emitted[2].fixed_color, 0xFF445566);

        let m = Matrix::scale_translate(2.0, 3.0, 10.0, 20.0);
        let orig_x0 = plan.dst_x[0];
        let orig_y0 = plan.dst_y[0];
        map_dst_scale_translate(&mut plan.dst_x, &mut plan.dst_y, &m);
        assert_eq!(plan.dst_x[0], orig_x0 * 2.0 + 10.0);
        assert_eq!(plan.dst_y[0], orig_y0 * 3.0 + 20.0);
    }

    #[test]
    fn test_lattice_shrink_fixed_patches_when_dst_smaller() {
        // bounds [0..10], xDivs = [3, 7] => fixed: [0..3] (3) + [7..10] (3) = 6, scalable: [3..7] (4)
        // dst width = 3.0 < srcFixed (6) => scalable collapses to 0, fixed scales by 3/6 = 0.5
        let plan = LatticePlan::from_lattice(
            IRect::from_wh(10, 10),
            &[3, 7],
            &[2, 8],
            None,
            None,
            Rect::make_ltrb(10.0, 20.0, 13.0, 22.0),
        );
        assert_eq!(plan.dst_x, vec![10.0, 11.5, 11.5, 13.0]);
        assert_eq!(plan.dst_y, vec![20.0, 21.0, 21.0, 22.0]);
    }
}
