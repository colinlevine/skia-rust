#![forbid(unsafe_code)]

//! Port of `SkAddIntersections.h/.cpp` and `SkIntersectionHelper.h`.

use skia_rs_path::types::Verb;

use crate::arena::{ContourId, OpState, PtTId, SegmentId};
use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::intersections::Intersections;
use crate::quad::DQuad;
use crate::types::{almost_less_ulps_f32, PathOpsBounds};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SegmentType {
    HorizontalLine,
    VerticalLine,
    Line,
    Quad,
    Conic,
    Cubic,
}

impl OpState {
    fn seg_type(&self, seg: SegmentId) -> SegmentType {
        match self.segments[seg].verb {
            Verb::Line => {
                if self.seg_is_horizontal(seg) {
                    SegmentType::HorizontalLine
                } else if self.seg_is_vertical(seg) {
                    SegmentType::VerticalLine
                } else {
                    SegmentType::Line
                }
            }
            Verb::Quad => SegmentType::Quad,
            Verb::Conic => SegmentType::Conic,
            Verb::Cubic => SegmentType::Cubic,
            _ => SegmentType::Line,
        }
    }

    #[inline]
    fn seg_x_flipped(&self, seg: SegmentId) -> bool {
        self.segments[seg].bounds.left != self.segments[seg].pts[0].x
    }

    #[inline]
    fn seg_y_flipped(&self, seg: SegmentId) -> bool {
        self.segments[seg].bounds.top != self.segments[seg].pts[0].y
    }

    pub fn add_intersect_ts(&mut self, test: ContourId, next: ContourId) -> bool {
        if test != next {
            if almost_less_ulps_f32(
                self.contours[test].bounds.bottom,
                self.contours[next].bounds.top,
            ) {
                return false;
            }
            if !PathOpsBounds::intersects(&self.contours[test].bounds, &self.contours[next].bounds)
            {
                return true;
            }
        }
        let mut wt_opt = self.contours[test].head;
        while let Some(wt) = wt_opt {
            let mut wn_opt = if test == next {
                self.segments[wt].next
            } else {
                self.contours[next].head
            };
            while let Some(wn) = wn_opt {
                if PathOpsBounds::intersects(&self.segments[wt].bounds, &self.segments[wn].bounds) {
                    let wt_type = self.seg_type(wt);
                    let wn_type = self.seg_type(wn);
                    let wt_pts = self.segments[wt].pts;
                    let wn_pts = self.segments[wn].pts;
                    let wt_w = self.segments[wt].weight;
                    let wn_w = self.segments[wn].weight;
                    let wt_b = self.segments[wt].bounds;
                    let wn_b = self.segments[wn].bounds;
                    let wt_xf = self.seg_x_flipped(wt);
                    let wt_yf = self.seg_y_flipped(wt);
                    let wn_xf = self.seg_x_flipped(wn);
                    let wn_yf = self.seg_y_flipped(wn);

                    let mut ts = Intersections::new();
                    let mut swap = false;
                    let pts = match wt_type {
                        SegmentType::HorizontalLine => {
                            swap = true;
                            match wn_type {
                                SegmentType::HorizontalLine
                                | SegmentType::VerticalLine
                                | SegmentType::Line => ts.line_horizontal(
                                    [wn_pts[0], wn_pts[1]],
                                    wt_b.left,
                                    wt_b.right,
                                    wt_b.top,
                                    wt_xf,
                                ),
                                SegmentType::Quad => ts.quad_horizontal(
                                    [wn_pts[0], wn_pts[1], wn_pts[2]],
                                    wt_b.left,
                                    wt_b.right,
                                    wt_b.top,
                                    wt_xf,
                                ),
                                SegmentType::Conic => ts.conic_horizontal(
                                    [wn_pts[0], wn_pts[1], wn_pts[2]],
                                    wn_w,
                                    wt_b.left,
                                    wt_b.right,
                                    wt_b.top,
                                    wt_xf,
                                ),
                                SegmentType::Cubic => ts.cubic_horizontal(
                                    wn_pts,
                                    wt_b.left,
                                    wt_b.right,
                                    wt_b.top,
                                    wt_xf,
                                ),
                            }
                        }
                        SegmentType::VerticalLine => {
                            swap = true;
                            match wn_type {
                                SegmentType::HorizontalLine
                                | SegmentType::VerticalLine
                                | SegmentType::Line => ts.line_vertical(
                                    [wn_pts[0], wn_pts[1]],
                                    wt_b.top,
                                    wt_b.bottom,
                                    wt_b.left,
                                    wt_yf,
                                ),
                                SegmentType::Quad => ts.quad_vertical(
                                    [wn_pts[0], wn_pts[1], wn_pts[2]],
                                    wt_b.top,
                                    wt_b.bottom,
                                    wt_b.left,
                                    wt_yf,
                                ),
                                SegmentType::Conic => ts.conic_vertical(
                                    [wn_pts[0], wn_pts[1], wn_pts[2]],
                                    wn_w,
                                    wt_b.top,
                                    wt_b.bottom,
                                    wt_b.left,
                                    wt_yf,
                                ),
                                SegmentType::Cubic => ts.cubic_vertical(
                                    wn_pts,
                                    wt_b.top,
                                    wt_b.bottom,
                                    wt_b.left,
                                    wt_yf,
                                ),
                            }
                        }
                        SegmentType::Line => match wn_type {
                            SegmentType::HorizontalLine => ts.line_horizontal(
                                [wt_pts[0], wt_pts[1]],
                                wn_b.left,
                                wn_b.right,
                                wn_b.top,
                                wn_xf,
                            ),
                            SegmentType::VerticalLine => ts.line_vertical(
                                [wt_pts[0], wt_pts[1]],
                                wn_b.top,
                                wn_b.bottom,
                                wn_b.left,
                                wn_yf,
                            ),
                            SegmentType::Line => {
                                ts.line_line([wt_pts[0], wt_pts[1]], [wn_pts[0], wn_pts[1]])
                            }
                            SegmentType::Quad => {
                                swap = true;
                                ts.quad_line([wn_pts[0], wn_pts[1], wn_pts[2]], [wt_pts[0], wt_pts[1]])
                            }
                            SegmentType::Conic => {
                                swap = true;
                                ts.conic_line(
                                    [wn_pts[0], wn_pts[1], wn_pts[2]],
                                    wn_w,
                                    [wt_pts[0], wt_pts[1]],
                                )
                            }
                            SegmentType::Cubic => {
                                swap = true;
                                ts.cubic_line(wn_pts, [wt_pts[0], wt_pts[1]])
                            }
                        },
                        SegmentType::Quad => match wn_type {
                            SegmentType::HorizontalLine => ts.quad_horizontal(
                                [wt_pts[0], wt_pts[1], wt_pts[2]],
                                wn_b.left,
                                wn_b.right,
                                wn_b.top,
                                wn_xf,
                            ),
                            SegmentType::VerticalLine => ts.quad_vertical(
                                [wt_pts[0], wt_pts[1], wt_pts[2]],
                                wn_b.top,
                                wn_b.bottom,
                                wn_b.left,
                                wn_yf,
                            ),
                            SegmentType::Line => ts.quad_line(
                                [wt_pts[0], wt_pts[1], wt_pts[2]],
                                [wn_pts[0], wn_pts[1]],
                            ),
                            SegmentType::Quad => ts.intersect_quad_quad(
                                &DQuad::from_points([wt_pts[0], wt_pts[1], wt_pts[2]]),
                                &DQuad::from_points([wn_pts[0], wn_pts[1], wn_pts[2]]),
                            ),
                            SegmentType::Conic => {
                                swap = true;
                                ts.intersect_conic_quad(
                                    &DConic::from_points([wn_pts[0], wn_pts[1], wn_pts[2]], wn_w),
                                    &DQuad::from_points([wt_pts[0], wt_pts[1], wt_pts[2]]),
                                )
                            }
                            SegmentType::Cubic => {
                                swap = true;
                                ts.intersect_cubic_quad(
                                    &DCubic::from_points(wn_pts),
                                    &DQuad::from_points([wt_pts[0], wt_pts[1], wt_pts[2]]),
                                )
                            }
                        },
                        SegmentType::Conic => match wn_type {
                            SegmentType::HorizontalLine => ts.conic_horizontal(
                                [wt_pts[0], wt_pts[1], wt_pts[2]],
                                wt_w,
                                wn_b.left,
                                wn_b.right,
                                wn_b.top,
                                wn_xf,
                            ),
                            SegmentType::VerticalLine => ts.conic_vertical(
                                [wt_pts[0], wt_pts[1], wt_pts[2]],
                                wt_w,
                                wn_b.top,
                                wn_b.bottom,
                                wn_b.left,
                                wn_yf,
                            ),
                            SegmentType::Line => ts.conic_line(
                                [wt_pts[0], wt_pts[1], wt_pts[2]],
                                wt_w,
                                [wn_pts[0], wn_pts[1]],
                            ),
                            SegmentType::Quad => ts.intersect_conic_quad(
                                &DConic::from_points([wt_pts[0], wt_pts[1], wt_pts[2]], wt_w),
                                &DQuad::from_points([wn_pts[0], wn_pts[1], wn_pts[2]]),
                            ),
                            SegmentType::Conic => ts.intersect_conic_conic(
                                &DConic::from_points([wt_pts[0], wt_pts[1], wt_pts[2]], wt_w),
                                &DConic::from_points([wn_pts[0], wn_pts[1], wn_pts[2]], wn_w),
                            ),
                            SegmentType::Cubic => {
                                swap = true;
                                ts.intersect_cubic_conic(
                                    &DCubic::from_points(wn_pts),
                                    &DConic::from_points([wt_pts[0], wt_pts[1], wt_pts[2]], wt_w),
                                )
                            }
                        },
                        SegmentType::Cubic => match wn_type {
                            SegmentType::HorizontalLine => ts.cubic_horizontal(
                                wt_pts,
                                wn_b.left,
                                wn_b.right,
                                wn_b.top,
                                wn_xf,
                            ),
                            SegmentType::VerticalLine => ts.cubic_vertical(
                                wt_pts,
                                wn_b.top,
                                wn_b.bottom,
                                wn_b.left,
                                wn_yf,
                            ),
                            SegmentType::Line => ts.cubic_line(wt_pts, [wn_pts[0], wn_pts[1]]),
                            SegmentType::Quad => ts.intersect_cubic_quad(
                                &DCubic::from_points(wt_pts),
                                &DQuad::from_points([wn_pts[0], wn_pts[1], wn_pts[2]]),
                            ),
                            SegmentType::Conic => ts.intersect_cubic_conic(
                                &DCubic::from_points(wt_pts),
                                &DConic::from_points([wn_pts[0], wn_pts[1], wn_pts[2]], wn_w),
                            ),
                            SegmentType::Cubic => ts.intersect_cubic_cubic(
                                &DCubic::from_points(wt_pts),
                                &DCubic::from_points(wn_pts),
                            ),
                        },
                    };

                    let mut coin_index: i32 = -1;
                    let mut coin_pt_t: [PtTId; 2] = [0, 0];
                    for pt in 0..pts {
                        let i_pt = ts.pt[pt].as_point();
                        let i_pt_is_integral = (i_pt.x as f64) == (i_pt.x as f64).floor()
                            && (i_pt.y as f64) == (i_pt.y as f64).floor();
                        let wt_t = ts.t[usize::from(swap)][pt];
                        let wn_t = ts.t[usize::from(!swap)][pt];
                        let test_t_at = if i_pt_is_integral {
                            self.seg_add_t_pt(wt, wt_t, i_pt)
                        } else {
                            self.seg_add_t(wt, wt_t)
                        };
                        let next_t_at = if i_pt_is_integral {
                            self.seg_add_t_pt(wn, wn_t, i_pt)
                        } else {
                            self.seg_add_t(wn, wn_t)
                        };
                        let (mut test_t_at, mut next_t_at) = match (test_t_at, next_t_at) {
                            (Some(a), Some(b)) => (a, b),
                            _ => continue,
                        };
                        if !self.pt_t_contains(test_t_at, next_t_at) {
                            if let Some(opp_prev) = self.pt_t_opp_prev(test_t_at, next_t_at) {
                                let test_span = self.pt_ts[test_t_at].span;
                                let next_span = self.pt_ts[next_t_at].span;
                                self.span_merge_matches(test_span, next_span);
                                self.pt_t_add_opp(test_t_at, next_t_at, opp_prev);
                            }
                            if self.pt_ts[test_t_at].pt != self.pt_ts[next_t_at].pt {
                                let test_span = self.pt_ts[test_t_at].span;
                                let next_span = self.pt_ts[next_t_at].span;
                                self.spans[test_span].aligned = false;
                                self.spans[next_span].aligned = false;
                            }
                        }
                        if !ts.is_coincident(pt) {
                            continue;
                        }
                        if coin_index < 0 {
                            coin_pt_t[0] = test_t_at;
                            coin_pt_t[1] = next_t_at;
                            coin_index = pt as i32;
                            continue;
                        }
                        if self.pt_ts[coin_pt_t[0]].span == self.pt_ts[test_t_at].span {
                            coin_index = -1;
                            continue;
                        }
                        if self.pt_ts[coin_pt_t[1]].span == self.pt_ts[next_t_at].span {
                            coin_index = -1;
                            continue;
                        }
                        if swap {
                            coin_pt_t.swap(0, 1);
                            core::mem::swap(&mut test_t_at, &mut next_t_at);
                        }
                        if self.span_deleted(self.pt_ts[coin_pt_t[0]].span)
                            || self.span_deleted(self.pt_ts[test_t_at].span)
                        {
                            coin_index = -1;
                            continue;
                        }
                        self.coincidence_add(coin_pt_t[0], test_t_at, coin_pt_t[1], next_t_at);
                        coin_index = -1;
                    }
                }
                wn_opt = self.segments[wn].next;
            }
            wt_opt = self.segments[wt].next;
        }
        true
    }
}
