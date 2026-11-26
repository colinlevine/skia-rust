#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkEdge.h` and `src/core/SkEdge.cpp`.

use crate::fixed::{
    abs32, clz_u32, edge_compute_dy, fdot6_div, fdot6_round, fdot6_to_fixed, fdot6_to_fixed_div2,
    fixed_mul, fixed_to_fdot6, float_to_fdot6, left_shift_i32, SkFDot6, SkFixed,
};
use skia_rs_geometry::{IRect, Point};

pub const MAX_COEFF_SHIFT: i32 = 6;

#[repr(i8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum EdgeType {
    #[default]
    Line = 0,
    Quad = 1,
    Cubic = 2,
}

impl EdgeType {
    #[inline]
    pub const fn from_i8(v: i8) -> Self {
        match v {
            1 => Self::Quad,
            2 => Self::Cubic,
            _ => Self::Line,
        }
    }
}

#[repr(i8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum EdgeWinding {
    #[default]
    Cw = 1,
    Ccw = -1,
}

impl EdgeWinding {
    #[inline]
    pub const fn from_i8(v: i8) -> Self {
        if v < 0 {
            Self::Ccw
        } else {
            Self::Cw
        }
    }

    #[inline]
    pub const fn as_i32(self) -> i32 {
        self as i8 as i32
    }
}

/// Core fields of `SkEdge` (without intrusive linked-list pointers or C++ vtable).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EdgeBase {
    pub x: SkFixed,
    pub dx_dy: SkFixed,
    pub first_y: i32,
    pub last_y: i32,
    pub edge_type: i8,
    pub winding: i8,
    pub segment_count: u8,
    pub curve_shift: u8,
}

/// Subclass fields of `SkQuadraticEdge`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct QuadEdgeState {
    pub base: EdgeBase,
    pub qx: SkFixed,
    pub qy: SkFixed,
    pub q_dx_dt: SkFixed,
    pub q_dy_dt: SkFixed,
    pub q_d2x_dt2: SkFixed,
    pub q_d2y_dt2: SkFixed,
    pub q_last_x: SkFixed,
    pub q_last_y: SkFixed,
}

/// Subclass fields of `SkCubicEdge`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct CubicEdgeState {
    pub base: EdgeBase,
    pub cx: SkFixed,
    pub cy: SkFixed,
    pub c_dx_dt: SkFixed,
    pub c_dy_dt: SkFixed,
    pub c_d2x_dt2: SkFixed,
    pub c_d2y_dt2: SkFixed,
    pub c_d3x_dt3: SkFixed,
    pub c_d3y_dt3: SkFixed,
    pub c_last_x: SkFixed,
    pub c_last_y: SkFixed,
    pub to_fixed_shift: u8,
    pub _pad: [u8; 3],
}

#[inline]
fn cheap_distance(mut dx: SkFDot6, mut dy: SkFDot6) -> SkFDot6 {
    dx = abs32(dx);
    dy = abs32(dy);
    if dx > dy {
        dx.wrapping_add(dy / 2)
    } else {
        dy.wrapping_add(dx / 2)
    }
}

#[inline]
fn diff_to_steps(dx: SkFDot6, dy: SkFDot6, accuracy: i32) -> i32 {
    let mut dist = cheap_distance(dx, dy);
    dist = dist.wrapping_add(1 << (2 + accuracy)) >> (3 + accuracy);
    (32 - clz_u32(dist as u32)) >> 1
}

#[inline]
fn fdot6_up_shift(x: SkFDot6, up_shift: i32) -> i32 {
    left_shift_i32(x, up_shift as u32)
}

#[inline]
fn cubic_delta_from_line(a: SkFDot6, b: SkFDot6, c: SkFDot6, d: SkFDot6) -> SkFDot6 {
    let one_third = (a
        .wrapping_mul(8)
        .wrapping_sub(b.wrapping_mul(15))
        .wrapping_add(c.wrapping_mul(6))
        .wrapping_add(d)
        .wrapping_mul(19))
        >> 9;
    let two_third = (a
        .wrapping_add(b.wrapping_mul(6))
        .wrapping_sub(c.wrapping_mul(15))
        .wrapping_add(d.wrapping_mul(8))
        .wrapping_mul(19))
        >> 9;
    abs32(one_third).max(abs32(two_third))
}

impl EdgeBase {
    #[inline]
    pub const fn edge_type(&self) -> EdgeType {
        EdgeType::from_i8(self.edge_type)
    }

    #[inline]
    pub const fn winding(&self) -> EdgeWinding {
        EdgeWinding::from_i8(self.winding)
    }

    #[inline]
    pub const fn has_next_segment(&self) -> bool {
        self.segment_count != 0
    }

    /// Matches `SkEdge::setLine(p0, p1, clip)`.
    pub fn set_line(&mut self, p0: Point, p1: Point, clip: Option<IRect>) -> bool {
        let mut x0 = float_to_fdot6(p0.x);
        let mut y0 = float_to_fdot6(p0.y);
        let mut x1 = float_to_fdot6(p1.x);
        let mut y1 = float_to_fdot6(p1.y);

        let mut winding = EdgeWinding::Cw;
        if y0 > y1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
            winding = EdgeWinding::Ccw;
        }

        let top = fdot6_round(y0);
        let bot = fdot6_round(y1);

        if top == bot {
            return false;
        }
        if let Some(c) = clip {
            if top >= c.bottom || bot <= c.top {
                return false;
            }
        }

        let slope = fdot6_div(x1.wrapping_sub(x0), y1.wrapping_sub(y0));
        let dy = edge_compute_dy(top, y0);

        self.x = fdot6_to_fixed(x0.wrapping_add(fixed_mul(slope, dy)));
        self.dx_dy = slope;
        self.first_y = top;
        self.last_y = bot - 1;
        self.edge_type = EdgeType::Line as i8;
        self.segment_count = 0;
        self.winding = winding as i8;
        self.curve_shift = 0;

        if let Some(c) = clip {
            self.chop_line_with_clip(c);
        }
        true
    }

    /// Matches `SkEdge::chopLineWithClip`.
    pub fn chop_line_with_clip(&mut self, clip: IRect) {
        let top = self.first_y;
        debug_assert!(top < clip.bottom);
        if top < clip.top {
            debug_assert!(self.last_y >= clip.top);
            self.x = self.x.wrapping_add(self.dx_dy.wrapping_mul(clip.top - top));
            self.first_y = clip.top;
        }
    }

    /// Matches `SkEdge::updateLine`.
    pub fn update_line(
        &mut self,
        x_start: SkFixed,
        y_start: SkFixed,
        x_end: SkFixed,
        y_end: SkFixed,
    ) -> bool {
        debug_assert!(self.winding == 1 || self.winding == -1);
        debug_assert!(self.segment_count != 0);

        let y0 = fixed_to_fdot6(y_start);
        let y1 = fixed_to_fdot6(y_end);
        debug_assert!(y0 <= y1);

        let top = fdot6_round(y0);
        let bot = fdot6_round(y1);
        if top == bot {
            return false;
        }

        let x0 = fixed_to_fdot6(x_start);
        let x1 = fixed_to_fdot6(x_end);

        let slope = fdot6_div(x1.wrapping_sub(x0), y1.wrapping_sub(y0));
        let dy = edge_compute_dy(top, y0);

        self.x = fdot6_to_fixed(x0.wrapping_add(fixed_mul(slope, dy)));
        self.dx_dy = slope;
        self.first_y = top;
        self.last_y = bot - 1;
        true
    }
}

impl QuadEdgeState {
    /// Matches `SkQuadraticEdge::setQuadratic`.
    pub fn set_quadratic(&mut self, pts: &[Point; 3]) -> bool {
        let mut x0 = float_to_fdot6(pts[0].x);
        let mut y0 = float_to_fdot6(pts[0].y);
        let x1 = float_to_fdot6(pts[1].x);
        let y1 = float_to_fdot6(pts[1].y);
        let mut x2 = float_to_fdot6(pts[2].x);
        let mut y2 = float_to_fdot6(pts[2].y);

        let mut winding = EdgeWinding::Cw;
        if y0 > y2 {
            core::mem::swap(&mut x0, &mut x2);
            core::mem::swap(&mut y0, &mut y2);
            winding = EdgeWinding::Ccw;
        }

        let top = fdot6_round(y0);
        let bot = fdot6_round(y2);
        if top == bot {
            return false;
        }

        let delta_x = x1.wrapping_mul(2).wrapping_sub(x0).wrapping_sub(x2) >> 2;
        let delta_y = y1.wrapping_mul(2).wrapping_sub(y0).wrapping_sub(y2) >> 2;
        let mut shift = diff_to_steps(delta_x, delta_y, 0);
        debug_assert!(shift >= 0);

        if shift == 0 {
            shift = 1;
        } else if shift > MAX_COEFF_SHIFT {
            shift = MAX_COEFF_SHIFT;
        }

        self.base.winding = winding as i8;
        self.base.edge_type = EdgeType::Quad as i8;
        self.base.segment_count = (1u8) << shift;
        self.base.curve_shift = (shift - 1) as u8;

        let a_half_x = fdot6_to_fixed_div2(
            x0.wrapping_sub(x1).wrapping_sub(x1).wrapping_add(x2),
        );
        let b_half_x = fdot6_to_fixed(x1.wrapping_sub(x0));
        self.q_dx_dt = b_half_x.wrapping_add(a_half_x >> shift);
        self.q_d2x_dt2 = a_half_x >> (shift - 1);

        let a_half_y = fdot6_to_fixed_div2(
            y0.wrapping_sub(y1).wrapping_sub(y1).wrapping_add(y2),
        );
        let b_half_y = fdot6_to_fixed(y1.wrapping_sub(y0));
        self.q_dy_dt = b_half_y.wrapping_add(a_half_y >> shift);
        self.q_d2y_dt2 = a_half_y >> (shift - 1);

        self.qx = fdot6_to_fixed(x0);
        self.qy = fdot6_to_fixed(y0);
        self.q_last_x = fdot6_to_fixed(x2);
        self.q_last_y = fdot6_to_fixed(y2);

        self.next_segment()
    }

    /// Matches `SkQuadraticEdge::nextSegment`.
    pub fn next_segment(&mut self) -> bool {
        let mut count = self.base.segment_count as i32;
        let mut oldx = self.qx;
        let mut oldy = self.qy;
        let mut dx = self.q_dx_dt;
        let mut dy = self.q_dy_dt;
        let shift = self.base.curve_shift as i32;

        debug_assert!(count > 0);

        let mut newx;
        let mut newy;
        let mut success;
        loop {
            count -= 1;
            if count > 0 {
                newx = oldx.wrapping_add(dx >> shift);
                dx = dx.wrapping_add(self.q_d2x_dt2);
                newy = oldy.wrapping_add(dy >> shift);
                dy = dy.wrapping_add(self.q_d2y_dt2);
            } else {
                newx = self.q_last_x;
                newy = self.q_last_y;
            }
            success = self.base.update_line(oldx, oldy, newx, newy);
            oldx = newx;
            oldy = newy;
            if !(count > 0 && !success) {
                break;
            }
        }

        self.qx = newx;
        self.qy = newy;
        self.q_dx_dt = dx;
        self.q_dy_dt = dy;
        self.base.segment_count = count as u8;
        success
    }
}

impl CubicEdgeState {
    /// Matches `SkCubicEdge::setCubic`.
    pub fn set_cubic(&mut self, pts: &[Point; 4]) -> bool {
        let mut x0 = float_to_fdot6(pts[0].x);
        let mut y0 = float_to_fdot6(pts[0].y);
        let mut x1 = float_to_fdot6(pts[1].x);
        let mut y1 = float_to_fdot6(pts[1].y);
        let mut x2 = float_to_fdot6(pts[2].x);
        let mut y2 = float_to_fdot6(pts[2].y);
        let mut x3 = float_to_fdot6(pts[3].x);
        let mut y3 = float_to_fdot6(pts[3].y);

        let mut winding = EdgeWinding::Cw;
        if y0 > y3 {
            core::mem::swap(&mut x0, &mut x3);
            core::mem::swap(&mut x1, &mut x2);
            core::mem::swap(&mut y0, &mut y3);
            core::mem::swap(&mut y1, &mut y2);
            winding = EdgeWinding::Ccw;
        }

        let top = fdot6_round(y0);
        let bot = fdot6_round(y3);
        if top == bot {
            return false;
        }

        let dx = cubic_delta_from_line(x0, x1, x2, x3);
        let dy = cubic_delta_from_line(y0, y1, y2, y3);
        let mut step_exponent = diff_to_steps(dx, dy, 2) + 1;
        debug_assert!(step_exponent > 0);
        if step_exponent > MAX_COEFF_SHIFT {
            step_exponent = MAX_COEFF_SHIFT;
        }

        let mut precision_up_scale = 6;
        let mut to_fixed_shift = step_exponent + precision_up_scale - 10;
        if to_fixed_shift < 0 {
            to_fixed_shift = 0;
            precision_up_scale = 10 - step_exponent;
        }

        self.base.winding = winding as i8;
        self.base.edge_type = EdgeType::Cubic as i8;
        self.base.segment_count = left_shift_i32(1, step_exponent as u32) as u8;
        self.base.curve_shift = step_exponent as u8;
        self.to_fixed_shift = to_fixed_shift as u8;

        let a_x = fdot6_up_shift(
            x3.wrapping_add(3i32.wrapping_mul(x1.wrapping_sub(x2)))
                .wrapping_sub(x0),
            precision_up_scale,
        );
        let b_x = fdot6_up_shift(
            3i32.wrapping_mul(
                x0.wrapping_sub(2i32.wrapping_mul(x1)).wrapping_add(x2),
            ),
            precision_up_scale,
        );
        let c_x = fdot6_up_shift(3i32.wrapping_mul(x1.wrapping_sub(x0)), precision_up_scale);

        self.c_dx_dt = (a_x >> (2 * step_exponent))
            .wrapping_add(b_x >> step_exponent)
            .wrapping_add(c_x);
        self.c_d2x_dt2 = (3i32.wrapping_mul(a_x) >> (step_exponent - 1))
            .wrapping_add(2i32.wrapping_mul(b_x));
        self.c_d3x_dt3 = 3i32.wrapping_mul(a_x) >> (step_exponent - 1);

        let a_y = fdot6_up_shift(
            y3.wrapping_add(3i32.wrapping_mul(y1.wrapping_sub(y2)))
                .wrapping_sub(y0),
            precision_up_scale,
        );
        let b_y = fdot6_up_shift(
            3i32.wrapping_mul(
                y0.wrapping_sub(2i32.wrapping_mul(y1)).wrapping_add(y2),
            ),
            precision_up_scale,
        );
        let c_y = fdot6_up_shift(3i32.wrapping_mul(y1.wrapping_sub(y0)), precision_up_scale);

        self.c_dy_dt = (a_y >> (2 * step_exponent))
            .wrapping_add(b_y >> step_exponent)
            .wrapping_add(c_y);
        self.c_d2y_dt2 = (3i32.wrapping_mul(a_y) >> (step_exponent - 1))
            .wrapping_add(2i32.wrapping_mul(b_y));
        self.c_d3y_dt3 = 3i32.wrapping_mul(a_y) >> (step_exponent - 1);

        self.cx = fdot6_to_fixed(x0);
        self.cy = fdot6_to_fixed(y0);
        self.c_last_x = fdot6_to_fixed(x3);
        self.c_last_y = fdot6_to_fixed(y3);

        self.next_segment()
    }

    /// Matches `SkCubicEdge::nextSegment`.
    pub fn next_segment(&mut self) -> bool {
        let mut count = self.base.segment_count as i32;
        let mut oldx = self.cx;
        let mut oldy = self.cy;
        let step_exponent = self.base.curve_shift as i32;
        let to_fixed_shift = self.to_fixed_shift as i32;

        debug_assert!(count > 0);

        let mut newx;
        let mut newy;
        let mut success;
        loop {
            count -= 1;
            if count > 0 {
                newx = oldx.wrapping_add(self.c_dx_dt >> to_fixed_shift);
                self.c_dx_dt = self.c_dx_dt.wrapping_add(self.c_d2x_dt2 >> step_exponent);
                self.c_d2x_dt2 = self.c_d2x_dt2.wrapping_add(self.c_d3x_dt3);

                newy = oldy.wrapping_add(self.c_dy_dt >> to_fixed_shift);
                self.c_dy_dt = self.c_dy_dt.wrapping_add(self.c_d2y_dt2 >> step_exponent);
                self.c_d2y_dt2 = self.c_d2y_dt2.wrapping_add(self.c_d3y_dt3);
            } else {
                newx = self.c_last_x;
                newy = self.c_last_y;
            }

            if newy < oldy {
                newy = oldy;
            }

            success = self.base.update_line(oldx, oldy, newx, newy);
            oldx = newx;
            oldy = newy;
            if !(count > 0 && !success) {
                break;
            }
        }

        self.cx = newx;
        self.cy = newy;
        self.base.segment_count = count as u8;
        success
    }
}

/// Polymorphic safe Rust edge used internally by `BasicEdgeBuilder` and `scan_path`.
#[derive(Copy, Clone, Debug)]
pub enum BasicEdge {
    Line(EdgeBase),
    Quad(QuadEdgeState),
    Cubic(CubicEdgeState),
}

impl BasicEdge {
    #[inline]
    pub fn base(&self) -> &EdgeBase {
        match self {
            Self::Line(b) => b,
            Self::Quad(q) => &q.base,
            Self::Cubic(c) => &c.base,
        }
    }

    #[inline]
    pub fn base_mut(&mut self) -> &mut EdgeBase {
        match self {
            Self::Line(b) => b,
            Self::Quad(q) => &mut q.base,
            Self::Cubic(c) => &mut c.base,
        }
    }

    #[inline]
    pub fn has_next_segment(&self) -> bool {
        self.base().has_next_segment()
    }

    #[inline]
    pub fn next_segment(&mut self) -> bool {
        match self {
            Self::Line(_) => false,
            Self::Quad(q) => q.next_segment(),
            Self::Cubic(c) => c.next_segment(),
        }
    }
}
