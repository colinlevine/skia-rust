#![forbid(unsafe_code)]

//! Port of `SkPathOpsAsWinding.cpp`.

use skia_rs_geometry::{Point, Rect};
use skia_rs_path::builder::PathBuilder;
use skia_rs_path::iter::{LegacyPathIter, RangeIter};
use skia_rs_path::types::{FillType, PathDataBuf, Verb};

use crate::conic::DConic;
use crate::cubic::DCubic;
use crate::curve::{curve_dslope_at_t, curve_intercept_h, curve_point_at_t};
use crate::quad::DQuad;
use crate::simplify::path_is_convex;
use crate::types::{cpp_max_f32, cpp_min_f32, zero_or_one, SK_SCALAR_MAX};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
enum ContourDir {
    Ccw = -1,
    None = 0,
    Cw = 1,
}

impl ContourDir {
    fn negate(self) -> Self {
        match self {
            ContourDir::Ccw => ContourDir::Cw,
            ContourDir::None => ContourDir::None,
            ContourDir::Cw => ContourDir::Ccw,
        }
    }
}

#[derive(Clone, Debug)]
struct WindingContour {
    children: Vec<usize>,
    bounds: Rect,
    min_xy: Point,
    verb_start: i32,
    verb_end: i32,
    direction: ContourDir,
    contained: bool,
    reverse: bool,
}

impl WindingContour {
    fn new(bounds: Rect, verb_start: i32, verb_end: i32) -> Self {
        Self {
            children: Vec::new(),
            bounds,
            min_xy: Point::new(SK_SCALAR_MAX, SK_SCALAR_MAX),
            verb_start,
            verb_end,
            direction: ContourDir::None,
            contained: false,
            reverse: false,
        }
    }
}

#[inline]
fn verb_pt_count(verb: Verb) -> usize {
    match verb {
        Verb::Move => 1,
        Verb::Line => 1,
        Verb::Quad => 2,
        Verb::Conic => 2,
        Verb::Cubic => 3,
        Verb::Close => 0,
    }
}

#[inline]
fn verb_pt_index(verb: Verb) -> usize {
    match verb {
        Verb::Move | Verb::Close => 0,
        Verb::Line | Verb::Quad | Verb::Conic | Verb::Cubic => 1,
    }
}

#[inline]
fn to_direction(dy: f32) -> ContourDir {
    if dy > 0.0 {
        ContourDir::Ccw
    } else if dy < 0.0 {
        ContourDir::Cw
    } else {
        ContourDir::None
    }
}

#[inline]
fn join_possibly_empty_rect(a: &mut Rect, b: Rect) {
    a.left = cpp_min_f32(a.left, b.left);
    a.top = cpp_min_f32(a.top, b.top);
    a.right = cpp_max_f32(a.right, b.right);
    a.bottom = cpp_max_f32(a.bottom, b.bottom);
}

fn contains_edge(pts: &[Point], verb: Verb, weight: f32, edge: Point) -> i32 {
    let pt_cnt = verb_pt_count(verb);
    let bounds = Rect::bounds_or_empty(&pts[..=pt_cnt]);
    if bounds.top > edge.y || bounds.bottom <= edge.y || bounds.left >= edge.x {
        return 0;
    }
    let mut t_vals = [0.0f64; 3];
    let mut count = curve_intercept_h(pts, weight, verb, edge.y, &mut t_vals);
    let mut index = 0;
    while index < count {
        let intersect_x = curve_point_at_t(pts, weight, verb, t_vals[index]).x;
        if intersect_x < edge.x {
            index += 1;
            continue;
        }
        if intersect_x > edge.x {
            count -= 1;
            t_vals[index] = t_vals[count];
            continue;
        }
        if pts[0].x < edge.x && pts[pt_cnt].x < edge.x {
            index += 1;
            continue;
        }
        count -= 1;
        t_vals[index] = t_vals[count];
    }
    let mut directions = [ContourDir::None; 3];
    for idx in 0..count {
        let slope = curve_dslope_at_t(pts, weight, verb, t_vals[idx]).as_vector();
        directions[idx] = to_direction(slope.y);
    }
    let mut winding = 0;
    for idx in 0..count {
        if zero_or_one(t_vals[idx]) && directions[idx] != ContourDir::Ccw {
            continue;
        }
        winding += directions[idx] as i32;
    }
    winding
}

fn left_edge(pts: &[Point], verb: Verb, weight: f32) -> Point {
    match verb {
        Verb::Line => {
            if pts[0].x < pts[1].x {
                pts[0]
            } else {
                pts[1]
            }
        }
        Verb::Quad => {
            let quad = DQuad::from_points([pts[0], pts[1], pts[2]]);
            let mut t = [0.0f64; 1];
            let roots = if !quad.monotonic_in_x() {
                DQuad::find_extrema(quad.pts[0].x, quad.pts[1].x, quad.pts[2].x, &mut t)
            } else {
                0
            };
            if roots != 0 {
                quad.pt_at_t(t[0]).as_point()
            } else if pts[0].x < pts[2].x {
                pts[0]
            } else {
                pts[2]
            }
        }
        Verb::Conic => {
            let conic = DConic::from_points([pts[0], pts[1], pts[2]], weight);
            let mut t = [0.0f64; 1];
            let roots = if !conic.monotonic_in_x() {
                DConic::find_extrema(
                    conic.pts.pts[0].x,
                    conic.pts.pts[1].x,
                    conic.pts.pts[2].x,
                    weight,
                    &mut t,
                )
            } else {
                0
            };
            if roots != 0 {
                conic.pt_at_t(t[0]).as_point()
            } else if pts[0].x < pts[2].x {
                pts[0]
            } else {
                pts[2]
            }
        }
        Verb::Cubic => {
            let cubic = DCubic::from_points([pts[0], pts[1], pts[2], pts[3]]);
            let mut result = Point::ZERO;
            let mut roots = 0;
            if !cubic.monotonic_in_x() {
                let mut t_values = [0.0f64; 2];
                roots = DCubic::find_extrema(
                    cubic.pts[0].x,
                    cubic.pts[1].x,
                    cubic.pts[2].x,
                    cubic.pts[3].x,
                    &mut t_values,
                );
                for (idx, &tv) in t_values.iter().take(roots).enumerate() {
                    let temp = cubic.pt_at_t(tv).as_point();
                    if idx == 0 || result.x > temp.x {
                        result = temp;
                    }
                }
            }
            if roots != 0 {
                // Note: Skia C++ has `result = cubic.ptAtT(t).asSkPoint()` where t=0
                // (SK_INIT_TO_AVOID_WARNING). Using the actual computed minimum `result`
                // preserves the intended leftmost point cleanly.
                result
            } else if pts[0].x < pts[3].x {
                pts[0]
            } else {
                pts[3]
            }
        }
        _ => pts[0],
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum EdgeMode {
    Initial,
    Compare,
}

fn contour_bounds(path: &PathDataBuf, containers: &mut Vec<WindingContour>) {
    let mut bounds = Rect::EMPTY;
    let mut last_start = 0i32;
    let mut verb_start = 0i32;
    for (verb, pts, _w) in RangeIter::new(&path.points, &path.verbs, &path.conics) {
        if verb == Verb::Move {
            if !bounds.is_empty() {
                containers.push(WindingContour::new(bounds, last_start, verb_start));
                last_start = verb_start;
            }
            let idx = verb_pt_index(Verb::Move);
            bounds = Rect::bounds_or_empty(&pts[idx..idx + verb_pt_count(Verb::Move)]);
        }
        if matches!(verb, Verb::Line | Verb::Quad | Verb::Conic | Verb::Cubic) {
            let idx = verb_pt_index(verb);
            let verb_bounds = Rect::bounds_or_empty(&pts[idx..idx + verb_pt_count(verb)]);
            join_possibly_empty_rect(&mut bounds, verb_bounds);
        }
        verb_start += 1;
    }
    if !bounds.is_empty() {
        verb_start += 1;
        containers.push(WindingContour::new(bounds, last_start, verb_start));
    }
}

fn get_direction(path: &PathDataBuf, contour: &WindingContour) -> ContourDir {
    let mut iter = LegacyPathIter::new(&path.points, &path.verbs, &path.conics, true);
    let mut verb_count = -1i32;
    let mut total_signed_area = 0.0f32;
    while let Some(rec) = iter.next_rec() {
        verb_count += 1;
        if verb_count < contour.verb_start || verb_count >= contour.verb_end {
            continue;
        }
        let pts = rec.pts();
        match rec.verb {
            Verb::Line => {
                total_signed_area += (pts[0].y - pts[1].y) * (pts[0].x + pts[1].x);
            }
            Verb::Quad | Verb::Conic => {
                total_signed_area += (pts[0].y - pts[2].y) * (pts[0].x + pts[2].x);
            }
            Verb::Cubic => {
                total_signed_area += (pts[0].y - pts[3].y) * (pts[0].x + pts[3].x);
            }
            _ => {}
        }
    }
    if total_signed_area < 0.0 {
        ContourDir::Ccw
    } else {
        ContourDir::Cw
    }
}

fn next_edge(path: &PathDataBuf, contour: &mut WindingContour, edge: EdgeMode) -> i32 {
    let mut iter = LegacyPathIter::new(&path.points, &path.verbs, &path.conics, true);
    let mut verb_count = -1i32;
    let mut winding = 0i32;
    while let Some(rec) = iter.next_rec() {
        verb_count += 1;
        if verb_count < contour.verb_start || verb_count >= contour.verb_end {
            continue;
        }
        if !matches!(
            rec.verb,
            Verb::Line | Verb::Quad | Verb::Conic | Verb::Cubic
        ) {
            continue;
        }
        let pts = rec.pts();
        let pt_cnt = verb_pt_count(rec.verb);
        let mut horizontal = true;
        for pt in pts.iter().take(pt_cnt + 1).skip(1) {
            if pts[0].y != pt.y {
                horizontal = false;
                break;
            }
        }
        if horizontal {
            continue;
        }
        let w = if rec.verb == Verb::Conic {
            rec.conic_weight
        } else {
            1.0
        };
        if edge == EdgeMode::Compare {
            winding += contains_edge(pts, rec.verb, w, contour.min_xy);
            continue;
        }
        let min_xy = left_edge(pts, rec.verb, w);
        if min_xy.x > contour.min_xy.x {
            continue;
        }
        if min_xy.x == contour.min_xy.x && min_xy.y != contour.min_xy.y {
            continue;
        }
        contour.min_xy = min_xy;
    }
    winding
}

fn container_contains(
    path: &PathDataBuf,
    contours: &mut [WindingContour],
    contour_idx: usize,
    test_idx: usize,
) -> bool {
    if contours[test_idx].min_xy.x == SK_SCALAR_MAX {
        next_edge(path, &mut contours[test_idx], EdgeMode::Initial);
    }
    let test_min_xy = contours[test_idx].min_xy;
    contours[contour_idx].min_xy = test_min_xy;
    let winding = next_edge(path, &mut contours[contour_idx], EdgeMode::Compare);
    contours[test_idx].contained = winding != 0;
    (-1..=1).contains(&winding)
}

fn in_parent(
    contours: &mut [WindingContour],
    root_children: &mut Vec<usize>,
    contour_idx: usize,
    parent_idx: Option<usize>,
) {
    let c_bounds = contours[contour_idx].bounds;
    let children_copy: Vec<usize> = match parent_idx {
        None => root_children.clone(),
        Some(p) => contours[p].children.clone(),
    };
    for test_idx in children_copy {
        if contours[test_idx].bounds.contains_rect(c_bounds) {
            in_parent(contours, root_children, contour_idx, Some(test_idx));
            return;
        }
    }
    let parent_list = match parent_idx {
        None => root_children,
        Some(p) => &mut contours[p].children,
    };
    let mut moved = Vec::new();
    let mut kept = Vec::new();
    for child in parent_list.drain(..) {
        if c_bounds.contains_rect(contours[child].bounds) {
            moved.push(child);
        } else {
            kept.push(child);
        }
    }
    *parent_list = kept;
    parent_list.push(contour_idx);
    contours[contour_idx].children.extend(moved);
}

fn check_container_children(
    path: &PathDataBuf,
    contours: &mut [WindingContour],
    parent: Option<usize>,
    child: usize,
) -> bool {
    let grand_children = contours[child].children.clone();
    for gc in grand_children {
        if !check_container_children(path, contours, Some(child), gc) {
            return false;
        }
    }
    if let Some(p) = parent {
        if !container_contains(path, contours, p, child) {
            return false;
        }
    }
    true
}

fn mark_reverse(
    path: &PathDataBuf,
    contours: &mut [WindingContour],
    parent: Option<usize>,
    child: usize,
) -> bool {
    let mut reversed = false;
    let grand_children = contours[child].children.clone();
    for gc in grand_children {
        let gc_parent = if contours[gc].contained {
            Some(child)
        } else {
            parent
        };
        reversed |= mark_reverse(path, contours, gc_parent, gc);
    }
    let dir = get_direction(path, &contours[child]);
    contours[child].direction = dir;
    if let Some(p) = parent {
        if contours[p].direction == dir {
            contours[child].reverse = true;
            contours[child].direction = dir.negate();
            return true;
        }
    }
    reversed
}

fn reverse_marked_contours(
    path: &PathDataBuf,
    contours: &[WindingContour],
    fill_type: FillType,
) -> PathDataBuf {
    let mut iter = RangeIter::new(&path.points, &path.verbs, &path.conics);
    let mut verb_count = 0i32;
    let mut result = PathBuilder::new(fill_type);

    for contour in contours {
        let mut reverse_buf = PathBuilder::new(FillType::Winding);
        while verb_count < contour.verb_end {
            let Some((verb, pts, w)) = iter.next() else {
                break;
            };
            let target = if contour.reverse {
                &mut reverse_buf
            } else {
                &mut result
            };
            match verb {
                Verb::Move => {
                    target.move_to(pts[0]);
                }
                Verb::Line => {
                    target.line_to(pts[1]);
                }
                Verb::Quad => {
                    target.quad_to(pts[1], pts[2]);
                }
                Verb::Conic => {
                    target.conic_to(pts[1], pts[2], w);
                }
                Verb::Cubic => {
                    target.cubic_to(pts[1], pts[2], pts[3]);
                }
                Verb::Close => {
                    target.close();
                }
            }
            verb_count += 1;
        }
        if contour.reverse {
            let rev = reverse_buf.into_buf();
            result.reverse_add_path(&rev.points, &rev.verbs, &rev.conics);
        }
    }
    result.into_buf()
}

pub fn as_winding(path: &PathDataBuf) -> Option<PathDataBuf> {
    if !path.is_finite() {
        return None;
    }
    let mut fill_type = path.fill_type;
    if fill_type == FillType::Winding || fill_type == FillType::InverseWinding {
        return Some(path.clone());
    }
    fill_type = if path.fill_type.is_inverse() {
        FillType::InverseWinding
    } else {
        FillType::Winding
    };
    if path.is_empty() || path_is_convex(path) {
        let mut res = path.clone();
        res.fill_type = fill_type;
        return Some(res);
    }

    let mut contours = Vec::new();
    contour_bounds(path, &mut contours);
    if contours.len() <= 1 {
        let mut res = path.clone();
        res.fill_type = fill_type;
        return Some(res);
    }

    let mut root_children = Vec::new();
    for i in 0..contours.len() {
        in_parent(&mut contours, &mut root_children, i, None);
    }

    if root_children.iter().all(|&i| contours[i].children.is_empty()) {
        let mut res = path.clone();
        res.fill_type = fill_type;
        return Some(res);
    }

    for &c_idx in &root_children {
        next_edge(path, &mut contours[c_idx], EdgeMode::Initial);
        contours[c_idx].direction = get_direction(path, &contours[c_idx]);
        if !check_container_children(path, &mut contours, None, c_idx) {
            return None;
        }
    }

    let mut reversed = false;
    for &c_idx in &root_children {
        reversed |= mark_reverse(path, &mut contours, None, c_idx);
    }
    if !reversed {
        let mut res = path.clone();
        res.fill_type = fill_type;
        return Some(res);
    }

    Some(reverse_marked_contours(path, &contours, fill_type))
}
