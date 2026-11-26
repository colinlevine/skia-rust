#![forbid(unsafe_code)]

use crate::iter::RangeIter;
use crate::types::{
    Convexity, Direction, FillType, PathRaw, Verb, FOUR_QUARTER_CIRCLE_CONICS, SEG_CONIC, SEG_LINE,
};
use skia_rs_geometry::matrix::{MSCALE_X, MSCALE_Y, MSKEW_X, MSKEW_Y};
use skia_rs_geometry::rrect::{
    CORNER_LL, CORNER_LR, CORNER_UL, CORNER_UR, TYPE_EMPTY, TYPE_OVAL, TYPE_RECT,
};
use skia_rs_geometry::{Matrix, Point, RRect, Rect, Vector};

pub const RECT_VERBS: [Verb; 5] = [Verb::Move, Verb::Line, Verb::Line, Verb::Line, Verb::Close];

pub const OVAL_VERBS: [Verb; 6] = [
    Verb::Move,
    Verb::Conic,
    Verb::Conic,
    Verb::Conic,
    Verb::Conic,
    Verb::Close,
];

pub const RRECT_VERBS_LINE_START: [Verb; 10] = [
    Verb::Move,
    Verb::Line,
    Verb::Conic,
    Verb::Line,
    Verb::Conic,
    Verb::Line,
    Verb::Conic,
    Verb::Line,
    Verb::Conic,
    Verb::Close,
];

pub const RRECT_VERBS_CONIC_START: [Verb; 9] = [
    Verb::Move,
    Verb::Conic,
    Verb::Line,
    Verb::Conic,
    Verb::Line,
    Verb::Conic,
    Verb::Line,
    Verb::Conic,
    Verb::Close,
];

pub const TRIANGLE_VERBS: [Verb; 4] = [Verb::Move, Verb::Line, Verb::Line, Verb::Close];

pub const RECT_SEG_MASK: u8 = SEG_LINE;
pub const OVAL_SEG_MASK: u8 = SEG_CONIC;
pub const RRECT_SEG_MASK: u8 = SEG_LINE | SEG_CONIC;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RRectAsEnum {
    Rect = 0,
    Oval = 1,
    RRect = 2,
}

#[inline]
pub fn simplify_rrect(rr: &RRect, start_index: u32) -> (RRectAsEnum, u32) {
    if rr.rtype == TYPE_RECT || rr.rtype == TYPE_EMPTY {
        (RRectAsEnum::Rect, (start_index + 1) / 2)
    } else if rr.rtype == TYPE_OVAL {
        (RRectAsEnum::Oval, start_index / 2)
    } else {
        (RRectAsEnum::RRect, start_index)
    }
}

struct PointIterator<const N: usize> {
    pts: [Point; N],
    current: usize,
    advance: usize,
}

impl<const N: usize> PointIterator<N> {
    fn new(pts: [Point; N], dir: Direction, start_index: u32) -> Self {
        Self {
            pts,
            current: (start_index as usize) % N,
            advance: if dir == Direction::Cw { 1 } else { N - 1 },
        }
    }

    #[inline]
    fn current(&self) -> Point {
        self.pts[self.current]
    }

    #[inline]
    fn next(&mut self) -> Point {
        self.current = (self.current + self.advance) % N;
        self.pts[self.current]
    }
}

fn rect_point_iter(rect: Rect, dir: Direction, start_index: u32) -> PointIterator<4> {
    PointIterator::new(
        [
            Point::new(rect.left, rect.top),
            Point::new(rect.right, rect.top),
            Point::new(rect.right, rect.bottom),
            Point::new(rect.left, rect.bottom),
        ],
        dir,
        start_index,
    )
}

fn oval_point_iter(oval: Rect, dir: Direction, start_index: u32) -> PointIterator<4> {
    let cx = oval.center_x();
    let cy = oval.center_y();
    PointIterator::new(
        [
            Point::new(cx, oval.top),
            Point::new(oval.right, cy),
            Point::new(cx, oval.bottom),
            Point::new(oval.left, cy),
        ],
        dir,
        start_index,
    )
}

fn rrect_point_iter(rrect: &RRect, dir: Direction, start_index: u32) -> PointIterator<8> {
    let b = rrect.rect;
    let l = b.left;
    let t = b.top;
    let r = b.right;
    let bot = b.bottom;
    let ul = rrect.radii[CORNER_UL];
    let ur = rrect.radii[CORNER_UR];
    let lr = rrect.radii[CORNER_LR];
    let ll = rrect.radii[CORNER_LL];
    PointIterator::new(
        [
            Point::new(l + ul.x, t),
            Point::new(r - ur.x, t),
            Point::new(r, t + ur.y),
            Point::new(r, bot - lr.y),
            Point::new(r - lr.x, bot),
            Point::new(l + ll.x, bot),
            Point::new(l, bot - ll.y),
            Point::new(l, t + ul.y),
        ],
        dir,
        start_index,
    )
}

/// Fills 4 points for `SkPathRawShapes::Rect`.
pub fn fill_rect_points(r: Rect, dir: Direction, index: u32) -> [Point; 4] {
    let mut iter = rect_point_iter(r, dir, index);
    let p0 = iter.current();
    let p1 = iter.next();
    let p2 = iter.next();
    let p3 = iter.next();
    [p0, p1, p2, p3]
}

/// Fills 9 points for `SkPathRawShapes::Oval`.
pub fn fill_oval_points(r: Rect, dir: Direction, index: u32) -> [Point; 9] {
    let mut oval_iter = oval_point_iter(r, dir, index);
    let rect_start = index.wrapping_add(if dir == Direction::Cw { 0 } else { 1 });
    let mut rect_iter = rect_point_iter(r, dir, rect_start);
    let mut storage = [Point::ZERO; 9];
    storage[0] = oval_iter.current();
    for i in 0..4 {
        storage[i * 2 + 1] = rect_iter.next();
        storage[i * 2 + 2] = oval_iter.next();
    }
    storage
}

#[derive(Copy, Clone, Debug)]
pub enum RRectShapeKind {
    Rect,
    Oval,
    RRectConicStart,
    RRectLineStart,
}

#[derive(Copy, Clone, Debug)]
pub struct RRectShapeData {
    pub storage: [Point; 13],
    pub point_count: usize,
    pub kind: RRectShapeKind,
    pub bounds: Rect,
    pub convexity: Convexity,
    pub segment_mask: u8,
}

impl RRectShapeData {
    pub fn as_raw(&self) -> PathRaw<'_> {
        let verbs: &'static [Verb] = match self.kind {
            RRectShapeKind::Rect => &RECT_VERBS,
            RRectShapeKind::Oval => &OVAL_VERBS,
            RRectShapeKind::RRectConicStart => &RRECT_VERBS_CONIC_START,
            RRectShapeKind::RRectLineStart => &RRECT_VERBS_LINE_START,
        };
        let conics: &'static [f32] = match self.kind {
            RRectShapeKind::Rect => &[],
            _ => &FOUR_QUARTER_CIRCLE_CONICS,
        };
        PathRaw {
            points: &self.storage[..self.point_count],
            verbs,
            conics,
            bounds: self.bounds,
            fill_type: FillType::Winding,
            convexity: self.convexity,
            segment_mask: self.segment_mask,
        }
    }
}

/// Matches `SkPathRawShapes::RRect`.
pub fn build_rrect_shape(rrect: &RRect, dir: Direction, index: u32) -> RRectShapeData {
    let bounds = rrect.rect;
    let mut storage = [Point::ZERO; 13];
    if rrect.rtype == TYPE_RECT || rrect.rtype == TYPE_EMPTY {
        let pts = fill_rect_points(bounds, dir, (index + 1) / 2);
        storage[..4].copy_from_slice(&pts);
        RRectShapeData {
            storage,
            point_count: 4,
            kind: RRectShapeKind::Rect,
            bounds,
            convexity: dir.to_convexity(),
            segment_mask: RECT_SEG_MASK,
        }
    } else if rrect.rtype == TYPE_OVAL {
        let pts = fill_oval_points(bounds, dir, index / 2);
        storage[..9].copy_from_slice(&pts);
        RRectShapeData {
            storage,
            point_count: 9,
            kind: RRectShapeKind::Oval,
            bounds,
            convexity: dir.to_convexity(),
            segment_mask: OVAL_SEG_MASK,
        }
    } else {
        let starts_with_conic = ((index & 1) != 0) == (dir == Direction::Cw);
        let npoints = if starts_with_conic { 12 } else { 13 };
        let mut rrect_iter = rrect_point_iter(rrect, dir, index);
        let rect_start = index / 2 + if dir == Direction::Cw { 0 } else { 1 };
        let mut rect_iter = rect_point_iter(bounds, dir, rect_start);

        storage[0] = rrect_iter.current();
        if starts_with_conic {
            for i in 0..3 {
                storage[i * 3 + 1] = rect_iter.next();
                storage[i * 3 + 2] = rrect_iter.next();
                storage[i * 3 + 3] = rrect_iter.next();
            }
            storage[10] = rect_iter.next();
            storage[11] = rrect_iter.next();
        } else {
            for i in 0..4 {
                storage[i * 3 + 1] = rrect_iter.next();
                storage[i * 3 + 2] = rect_iter.next();
                storage[i * 3 + 3] = rrect_iter.next();
            }
        }
        RRectShapeData {
            storage,
            point_count: npoints,
            kind: if starts_with_conic {
                RRectShapeKind::RRectConicStart
            } else {
                RRectShapeKind::RRectLineStart
            },
            bounds,
            convexity: dir.to_convexity(),
            segment_mask: RRECT_SEG_MASK,
        }
    }
}

/// Matches `tri_to_convexity` in `SkPathRawShapes.cpp`.
pub fn tri_to_convexity(pts: &[Point; 3]) -> Convexity {
    let u = pts[1] - pts[0];
    let v = pts[2] - pts[1];
    let cross = u.x * v.y - u.y * v.x;
    if cross > 0.0 {
        Convexity::ConvexCw
    } else if cross < 0.0 {
        Convexity::ConvexCcw
    } else {
        Convexity::ConvexDegenerate
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RectContour {
    pub rect: Rect,
    pub is_closed: bool,
    pub direction: Direction,
    pub points_consumed: usize,
    pub verbs_consumed: usize,
}

#[inline]
fn rect_make_dir(dx: f32, dy: f32) -> i8 {
    let h = i8::from(dx != 0.0);
    let pos = i8::from(dx > 0.0 || dy > 0.0) << 1;
    h | pos
}

fn trivial_rect(pts: &[Point], vbs: &[Verb]) -> Option<RectContour> {
    if pts.len() != 4 || vbs != RECT_VERBS {
        return None;
    }
    let v0 = pts[1] - pts[0];
    let v1 = pts[2] - pts[1];
    let v2 = pts[3] - pts[2];
    let v3 = pts[0] - pts[3];

    let axis_aligned_orthogonal =
        |a: Vector, b: Vector| ((a.x == 0.0) ^ (b.x == 0.0)) && ((a.y == 0.0) ^ (b.y == 0.0));

    if !(((v0.x == 0.0) ^ (v0.y == 0.0))
        && axis_aligned_orthogonal(v0, v1)
        && axis_aligned_orthogonal(v1, v2)
        && axis_aligned_orthogonal(v2, v3))
    {
        return None;
    }

    let rect = Rect::new(pts[0].x, pts[0].y, pts[2].x, pts[2].y).make_sorted();
    let dir = if v0.cross(v1) > 0.0 {
        Direction::Cw
    } else {
        Direction::Ccw
    };
    Some(RectContour {
        rect,
        is_closed: true,
        direction: dir,
        points_consumed: pts.len(),
        verbs_consumed: vbs.len(),
    })
}

/// Matching `SkPathPriv::IsRectContour`.
pub fn is_rect_contour(
    pts: &[Point],
    vbs: &[Verb],
    segment_mask: u32,
    allow_partial: bool,
) -> Option<RectContour> {
    if segment_mask != SEG_LINE as u32 || pts.len() < 4 || vbs.len() < 4 {
        return None;
    }
    if let Some(rc) = trivial_rect(pts, vbs) {
        return Some(rc);
    }

    let mut curr_verb = 0usize;
    let verb_cnt = vbs.len();

    let mut corners = 0usize;
    let mut line_start = Point::ZERO;
    let mut first_pt_idx: Option<usize> = None;
    let mut last_pt_idx: Option<usize> = None;
    let mut first_corner = Point::ZERO;
    let mut third_corner = Point::ZERO;
    let mut pt_idx = 0usize;
    let mut save_pts: Option<usize> = None;
    let mut directions = [-1i8; 5];
    let mut closed_or_moved = false;
    let mut auto_close = false;
    let mut insert_close = false;

    while curr_verb < verb_cnt && (!allow_partial || !auto_close) {
        let mut skip_increment = insert_close;
        let verb = if insert_close {
            Verb::Close
        } else {
            vbs[curr_verb]
        };
        match verb {
            Verb::Close | Verb::Line => {
                if verb == Verb::Close {
                    save_pts = Some(pt_idx);
                    auto_close = true;
                    insert_close = false;
                } else {
                    last_pt_idx = Some(pt_idx);
                }
                let line_end = if verb == Verb::Close {
                    pts[first_pt_idx?]
                } else {
                    let p = pts[pt_idx];
                    pt_idx += 1;
                    p
                };
                let line_delta = line_end - line_start;
                if line_delta.x != 0.0 && line_delta.y != 0.0 {
                    return None;
                }
                if !line_delta.is_finite() {
                    return None;
                }
                if line_start != line_end {
                    let next_direction = rect_make_dir(line_delta.x, line_delta.y);
                    if corners == 0 {
                        directions[0] = next_direction;
                        corners = 1;
                        closed_or_moved = false;
                        line_start = line_end;
                    } else if closed_or_moved {
                        return None;
                    } else if auto_close && next_direction == directions[0] {
                        // colinear with first
                    } else {
                        closed_or_moved = auto_close;
                        if directions[corners - 1] == next_direction {
                            if corners == 3 && verb == Verb::Line {
                                third_corner = line_end;
                            }
                            line_start = line_end;
                        } else {
                            directions[corners] = next_direction;
                            corners += 1;
                            match corners {
                                2 => first_corner = line_start,
                                3 => {
                                    if (directions[0] ^ directions[2]) != 2 {
                                        return None;
                                    }
                                    third_corner = line_end;
                                }
                                4 => {
                                    if (directions[1] ^ directions[3]) != 2 {
                                        return None;
                                    }
                                }
                                _ => return None,
                            }
                            line_start = line_end;
                        }
                    }
                }
            }
            Verb::Quad | Verb::Conic | Verb::Cubic => return None,
            Verb::Move => {
                if allow_partial && !auto_close && directions[0] >= 0 {
                    insert_close = true;
                    skip_increment = true;
                } else {
                    if corners == 0 {
                        first_pt_idx = Some(pt_idx);
                    } else {
                        let close_xy = pts[first_pt_idx?] - pts[last_pt_idx?];
                        if close_xy.x != 0.0 && close_xy.y != 0.0 {
                            return None;
                        }
                    }
                    line_start = pts[pt_idx];
                    pt_idx += 1;
                    closed_or_moved = true;
                }
            }
        }
        if !skip_increment {
            curr_verb += 1;
        }
    }

    if corners < 3 || corners > 4 {
        return None;
    }
    let close_xy = pts[first_pt_idx?] - pts[last_pt_idx?];
    if close_xy.x != 0.0 && close_xy.y != 0.0 {
        return None;
    }

    let rect = Rect::new(
        first_corner.x.min(third_corner.x),
        first_corner.y.min(third_corner.y),
        first_corner.x.max(third_corner.x),
        first_corner.y.max(third_corner.y),
    );
    let direction = if directions[0] == ((directions[1] + 1) & 3) {
        Direction::Cw
    } else {
        Direction::Ccw
    };
    Some(RectContour {
        rect,
        is_closed: auto_close,
        direction,
        points_consumed: save_pts.unwrap_or(0),
        verbs_consumed: curr_verb,
    })
}

/// Matching `SkPathPriv::IsNestedFillRects`.
pub fn is_nested_fill_rects(raw: &PathRaw<'_>) -> Option<([Rect; 2], [Direction; 2])> {
    let rc0 = is_rect_contour(raw.points, raw.verbs, raw.segment_mask as u32, true)?;
    let pts = &raw.points[rc0.points_consumed..];
    let vbs = &raw.verbs[rc0.verbs_consumed..];
    let rc1 = is_rect_contour(pts, vbs, raw.segment_mask as u32, false)?;

    if rc0.rect.contains_rect(rc1.rect) {
        Some(([rc0.rect, rc1.rect], [rc0.direction, rc1.direction]))
    } else if rc1.rect.contains_rect(rc0.rect) {
        Some(([rc1.rect, rc0.rect], [rc1.direction, rc0.direction]))
    } else {
        None
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PathRectInfo {
    pub rect: Rect,
    pub direction: Direction,
    pub start_index: u8,
}

/// Matching `SkPathPriv::IsSimpleRect`.
pub fn is_simple_rect(
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    segment_mask: u32,
    is_finite: bool,
    is_simple_fill: bool,
) -> Option<PathRectInfo> {
    if segment_mask != SEG_LINE as u32 {
        return None;
    }
    let mut rect_pts = [Point::ZERO; 5];
    let mut rect_pt_cnt = 0usize;
    let mut needs_close = !is_simple_fill;

    let iter_verbs = if is_finite { verbs } else { &[] };
    for (v, verb_pts, _) in RangeIter::new(points, iter_verbs, conics) {
        match v {
            Verb::Move => {
                if rect_pt_cnt != 0 {
                    return None;
                }
                rect_pts[0] = verb_pts[0];
                rect_pt_cnt += 1;
            }
            Verb::Line => {
                if rect_pt_cnt == 5 {
                    return None;
                }
                rect_pts[rect_pt_cnt] = verb_pts[1];
                rect_pt_cnt += 1;
            }
            Verb::Close => {
                if rect_pt_cnt == 4 {
                    rect_pts[4] = rect_pts[0];
                    rect_pt_cnt = 5;
                }
                needs_close = false;
            }
            Verb::Quad | Verb::Conic | Verb::Cubic => return None,
        }
    }

    if needs_close || rect_pt_cnt < 5 || rect_pts[0] != rect_pts[4] {
        return None;
    }

    let vec03_is_vertical = if rect_pts[0].x == rect_pts[3].x
        && rect_pts[1].x == rect_pts[2].x
        && rect_pts[0].y == rect_pts[1].y
        && rect_pts[3].y == rect_pts[2].y
    {
        if rect_pts[0].x == rect_pts[1].x || rect_pts[0].y == rect_pts[3].y {
            return None;
        }
        true
    } else if rect_pts[0].y == rect_pts[3].y
        && rect_pts[1].y == rect_pts[2].y
        && rect_pts[0].x == rect_pts[1].x
        && rect_pts[3].x == rect_pts[2].x
    {
        if rect_pts[0].y == rect_pts[1].y || rect_pts[0].x == rect_pts[3].x {
            return None;
        }
        false
    } else {
        return None;
    };

    let sort_flags = (if rect_pts[0].x < rect_pts[2].x { 0 } else { 1 })
        | (if rect_pts[0].y < rect_pts[2].y { 0 } else { 2 });

    let (rect, direction, start_index) = match sort_flags {
        0b00 => (
            Rect::new(rect_pts[0].x, rect_pts[0].y, rect_pts[2].x, rect_pts[2].y),
            if vec03_is_vertical {
                Direction::Cw
            } else {
                Direction::Ccw
            },
            0,
        ),
        0b01 => (
            Rect::new(rect_pts[2].x, rect_pts[0].y, rect_pts[0].x, rect_pts[2].y),
            if vec03_is_vertical {
                Direction::Ccw
            } else {
                Direction::Cw
            },
            1,
        ),
        0b10 => (
            Rect::new(rect_pts[0].x, rect_pts[2].y, rect_pts[2].x, rect_pts[0].y),
            if vec03_is_vertical {
                Direction::Ccw
            } else {
                Direction::Cw
            },
            3,
        ),
        _ => (
            Rect::new(rect_pts[2].x, rect_pts[2].y, rect_pts[0].x, rect_pts[0].y),
            if vec03_is_vertical {
                Direction::Cw
            } else {
                Direction::Ccw
            },
            2,
        ),
    };

    Some(PathRectInfo {
        rect,
        direction,
        start_index,
    })
}

/// Matching `SkPathPriv::TransformDirAndStart`.
pub fn transform_dir_and_start(
    matrix: &Matrix,
    is_rrect: bool,
    dir: Direction,
    mut start: u32,
) -> (Direction, u32) {
    if matrix.is_identity() {
        return (dir, start);
    }
    let mut in_start = start;
    let mut is_ccw = dir == Direction::Ccw;
    let mut rm = 0u32;
    if is_rrect {
        rm = in_start & 1;
        in_start /= 2;
    }

    let (anti_diag, top_neg, same_sign) = if matrix.mat[MSCALE_X] != 0.0 {
        let anti = 0b00u32;
        if matrix.mat[MSCALE_X] > 0.0 {
            (anti, 0b00u32, u32::from(matrix.mat[MSCALE_Y] > 0.0))
        } else {
            (anti, 0b10u32, u32::from(matrix.mat[MSCALE_Y] <= 0.0))
        }
    } else {
        let anti = 0b01u32;
        if matrix.mat[MSKEW_X] > 0.0 {
            (anti, 0b00u32, u32::from(matrix.mat[MSKEW_Y] > 0.0))
        } else {
            (anti, 0b10u32, u32::from(matrix.mat[MSKEW_Y] <= 0.0))
        }
    };

    if same_sign != anti_diag {
        start = (in_start + 4 - (top_neg | anti_diag)) % 4;
        if is_rrect {
            start = 2 * start + rm;
        }
    } else {
        is_ccw = !is_ccw;
        start = (6 + (top_neg | anti_diag) - in_start) % 4;
        if is_rrect {
            start = 2 * start + if rm != 0 { 0 } else { 1 };
        }
    }

    (if is_ccw { Direction::Ccw } else { Direction::Cw }, start)
}

/// Matching `SkPathPriv::DeduceRRectFromContour`.
pub fn deduce_rrect_from_contour(bounds: Rect, pts: &[Point], vbs: &[Verb]) -> RRect {
    if bounds.is_empty() {
        return RRect::make_empty();
    }
    debug_assert!(!vbs.is_empty());
    debug_assert!(vbs[0] == Verb::Move);

    let mut radii = [Point::ZERO; 4];
    let mut pt_index = 0usize;

    for &verb in vbs {
        match verb {
            Verb::Move => {
                debug_assert!(pt_index == 0);
                pt_index += 1;
            }
            Verb::Line => {
                pt_index += 1;
            }
            Verb::Quad | Verb::Cubic => {
                debug_assert!(false);
            }
            Verb::Conic => {
                let v1_0 = pts[pt_index] - pts[pt_index - 1];
                let v2_1 = pts[pt_index + 1] - pts[pt_index];
                let dxdy = if v1_0.x != 0.0 {
                    Point::new(v1_0.x.abs(), v2_1.y.abs())
                } else if v1_0.y == 0.0 {
                    Point::new(v2_1.x.abs(), v2_1.y.abs())
                } else {
                    Point::new(v2_1.x.abs(), v1_0.y.abs())
                };
                let corner = if pts[pt_index].x == bounds.left {
                    if pts[pt_index].y == bounds.top {
                        CORNER_UL
                    } else {
                        CORNER_LL
                    }
                } else if pts[pt_index].y == bounds.top {
                    CORNER_UR
                } else {
                    CORNER_LR
                };
                radii[corner] = dxdy;
                pt_index += 2;
            }
            Verb::Close => {}
        }
    }
    let mut rrect = RRect::make_empty();
    rrect.set_rect_radii(bounds, &radii);
    rrect
}
