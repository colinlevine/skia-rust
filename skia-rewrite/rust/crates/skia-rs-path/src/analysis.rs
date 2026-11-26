#![forbid(unsafe_code)]

use crate::builder::PathBuilder;
use crate::iter::{Edge, EdgeIter, PathIter, RangeIter};
use crate::types::{
    all_points_eq, ieee_double_divide, is_axis_aligned, scalar_sign_as_int, Convexity, Direction,
    FirstDirection, PathDataBuf, PathRaw, Verb, K_W0_PLANE_DISTANCE, SK_SCALAR_MAX,
};
use skia_rs_geometry::cubic_clipper::chop_mono_at_y;
use skia_rs_geometry::edge_clipper::{EdgeClipOutput, VERB_CUBIC, VERB_LINE, VERB_QUAD};
use skia_rs_geometry::geometry::{
    chop_cubic_at_y_extrema, chop_quad_at_y_extrema, eval_cubic_at, eval_quad_at,
    eval_quad_tangent_at, find_cubic_extrema, find_quad_extrema, find_unit_quad_roots, Conic,
    MAX_CONIC_TO_QUAD_POW2,
};
use skia_rs_geometry::matrix::{MPERSP_0, MPERSP_1, MPERSP_2, MSCALE_X, MSCALE_Y, MSKEW_X, MSKEW_Y};
use skia_rs_geometry::{
    cpp_max_f32, cpp_min_f32, scalar_nearly_equal, scalar_nearly_zero, Matrix, Point, Rect, Vector,
    SK_SCALAR_NEARLY_ZERO,
};

// ============================================================================
// Convexity (matching SkPathPriv::ComputeConvexity & TransformConvexity)
// ============================================================================

#[inline]
fn sign_neg(x: f32) -> i32 {
    i32::from(x < 0.0)
}

const VALUE_NEVER_RETURNED_BY_SIGN: i32 = 2;

#[derive(Copy, Clone, PartialEq, Eq)]
enum DirChange {
    Unknown,
    Left,
    Right,
    Straight,
    Backwards,
    Invalid,
}

struct Convexicator {
    first_pt: Point,
    first_vec: Vector,
    last_pt: Point,
    last_vec: Vector,
    expected_dir: DirChange,
    first_direction: FirstDirection,
    reversals: i32,
    is_finite: bool,
}

impl Convexicator {
    fn new() -> Self {
        Self {
            first_pt: Point::ZERO,
            first_vec: Point::ZERO,
            last_pt: Point::ZERO,
            last_vec: Point::ZERO,
            expected_dir: DirChange::Invalid,
            first_direction: FirstDirection::Unknown,
            reversals: 0,
            is_finite: true,
        }
    }

    fn set_move_pt(&mut self, pt: Point) {
        self.first_pt = pt;
        self.last_pt = pt;
        self.expected_dir = DirChange::Invalid;
    }

    fn add_pt(&mut self, pt: Point) -> bool {
        if self.last_pt == pt {
            return true;
        }
        if self.first_pt == self.last_pt
            && self.expected_dir == DirChange::Invalid
            && self.last_vec == Point::ZERO
        {
            self.last_vec = pt - self.last_pt;
            self.first_vec = self.last_vec;
        } else if !self.add_vec(pt - self.last_pt) {
            return false;
        }
        self.last_pt = pt;
        true
    }

    fn is_concave_by_sign(points: &[Point]) -> bool {
        if points.len() <= 3 {
            return false;
        }
        let first_pt = points[0];
        let mut curr_pt = first_pt;
        let mut dxes = 0;
        let mut dyes = 0;
        let mut last_sx = VALUE_NEVER_RETURNED_BY_SIGN;
        let mut last_sy = VALUE_NEVER_RETURNED_BY_SIGN;

        let mut step = |pt: Point| -> bool {
            let vec = pt - curr_pt;
            if !vec.is_zero() {
                if !vec.is_finite() {
                    return true;
                }
                let sx = sign_neg(vec.x);
                let sy = sign_neg(vec.y);
                dxes += i32::from(sx != last_sx);
                dyes += i32::from(sy != last_sy);
                if dxes > 3 || dyes > 3 {
                    return true;
                }
                last_sx = sx;
                last_sy = sy;
            }
            curr_pt = pt;
            false
        };

        for &pt in &points[1..] {
            if step(pt) {
                return true;
            }
        }
        step(first_pt)
    }

    fn close(&mut self) -> bool {
        self.add_pt(self.first_pt) && self.add_vec(self.first_vec)
    }

    fn direction_change(&self, cur_vec: Vector) -> DirChange {
        let cross = self.last_vec.cross(cur_vec);
        if !cross.is_finite() {
            return DirChange::Unknown;
        }
        if cross == 0.0 {
            if self.last_vec.dot(cur_vec) < 0.0 {
                DirChange::Backwards
            } else {
                DirChange::Straight
            }
        } else if scalar_sign_as_int(cross) == 1 {
            DirChange::Right
        } else {
            DirChange::Left
        }
    }

    fn add_vec(&mut self, cur_vec: Vector) -> bool {
        let dir = self.direction_change(cur_vec);
        match dir {
            DirChange::Left | DirChange::Right => {
                if self.expected_dir == DirChange::Invalid {
                    self.expected_dir = dir;
                    self.first_direction = if dir == DirChange::Right {
                        FirstDirection::Cw
                    } else {
                        FirstDirection::Ccw
                    };
                } else if dir != self.expected_dir {
                    self.first_direction = FirstDirection::Unknown;
                    return false;
                }
                self.last_vec = cur_vec;
            }
            DirChange::Straight => {}
            DirChange::Backwards => {
                self.last_vec = cur_vec;
                self.reversals += 1;
                return self.reversals < 3;
            }
            DirChange::Unknown => {
                self.is_finite = false;
                return false;
            }
            DirChange::Invalid => unreachable!(),
        }
        true
    }
}

fn trim_trailing_moves<'a>(
    mut pts: &'a [Point],
    mut vbs: &'a [Verb],
) -> (&'a [Point], &'a [Verb]) {
    let mut vb_count = vbs.len();
    while vb_count > 0 && vbs[vb_count - 1] == Verb::Move {
        vb_count -= 1;
    }
    let delta = vbs.len() - vb_count;
    if delta > 0 {
        pts = &pts[..pts.len() - delta];
        vbs = &vbs[..vb_count];
    }
    (pts, vbs)
}

/// Matching `SkPathPriv::ComputeConvexity`.
pub fn compute_convexity(points: &[Point], vbs: &[Verb], conic_weights: &[f32]) -> Convexity {
    let (points, vbs) = trim_trailing_moves(points, vbs);
    if vbs.is_empty() {
        return Convexity::ConvexDegenerate;
    }
    if Convexicator::is_concave_by_sign(points) {
        return Convexity::Concave;
    }

    let mut contour_count = 0;
    let mut needs_close = false;
    let mut state = Convexicator::new();
    let mut iter = PathIter::new(points, vbs, conic_weights);

    while let Some(rec) = iter.next_rec() {
        let verb = rec.verb;
        let pts = rec.pts();

        if contour_count == 0 {
            if verb == Verb::Move {
                state.set_move_pt(pts[0]);
            } else {
                contour_count += 1;
                needs_close = true;
            }
        }
        if contour_count == 1 {
            if verb == Verb::Close || verb == Verb::Move {
                if !state.close() {
                    return Convexity::Concave;
                }
                needs_close = false;
                contour_count += 1;
            } else {
                let count = verb.pts_in_verb();
                for i in 1..=count {
                    if !state.add_pt(pts[i]) {
                        return Convexity::Concave;
                    }
                }
            }
        } else if verb != Verb::Move {
            return Convexity::Concave;
        }
    }

    if needs_close && !state.close() {
        return Convexity::Concave;
    }

    let first_dir = state.first_direction;
    if first_dir == FirstDirection::Unknown && state.reversals >= 3 {
        return Convexity::Concave;
    }
    first_dir.to_convexity()
}

/// Matching `SkPathPriv::TransformConvexity`.
pub fn transform_convexity(matrix: &Matrix, pts: &[Point], mut convexity: Convexity) -> Convexity {
    if matrix.is_identity() || pts.is_empty() {
        return convexity;
    }
    if convexity.is_convex() {
        if !matrix.is_scale_translate() || !is_axis_aligned(pts) {
            convexity = Convexity::Unknown;
        } else {
            let det2x2 = matrix.mat[MSCALE_X] * matrix.mat[MSCALE_Y]
                - matrix.mat[MSKEW_X] * matrix.mat[MSKEW_Y];
            if det2x2 < 0.0 {
                convexity = convexity.opposite_convex_direction();
            } else if det2x2 == 0.0 {
                convexity = Convexity::ConvexDegenerate;
            }
        }
    }
    convexity
}

// ============================================================================
// First Direction (matching SkPathPriv::ComputeFirstDirection)
// ============================================================================

fn cross_prod(p0: Point, p1: Point, p2: Point) -> f32 {
    let mut cross = (p1 - p0).cross(p2 - p0);
    if cross == 0.0 {
        let p0x = p0.x as f64;
        let p0y = p0.y as f64;
        let p1x = p1.x as f64;
        let p1y = p1.y as f64;
        let p2x = p2.x as f64;
        let p2y = p2.y as f64;
        cross = ((p1x - p0x) * (p2y - p0y) - (p1y - p0y) * (p2x - p0x)) as f32;
    }
    cross
}

fn find_max_y(pts: &[Point]) -> usize {
    let mut max_y = pts[0].y;
    let mut first_index = 0usize;
    for (i, p) in pts.iter().enumerate().skip(1) {
        if p.y > max_y {
            max_y = p.y;
            first_index = i;
        }
    }
    first_index
}

fn find_diff_pt(pts: &[Point], index: usize, inc: usize) -> usize {
    let n = pts.len();
    let mut i = index;
    loop {
        i = (i + inc) % n;
        if i == index || pts[index] != pts[i] {
            break;
        }
    }
    i
}

fn find_min_max_x_at_y(pts: &[Point], index: usize) -> (usize, usize) {
    let y = pts[index].y;
    let mut min_x = pts[index].x;
    let mut max_x = min_x;
    let mut min_idx = index;
    let mut max_idx = index;
    for (i, p) in pts.iter().enumerate().skip(index + 1) {
        if p.y != y {
            break;
        }
        if p.x < min_x {
            min_x = p.x;
            min_idx = i;
        } else if p.x > max_x {
            max_x = p.x;
            max_idx = i;
        }
    }
    (min_idx, max_idx)
}

/// Iterates contour point slices matching `ContourIter` in `SkPathPriv.cpp`.
struct FirstDirContourIter<'a> {
    pts: &'a [Point],
    verbs: &'a [Verb],
    pt_pos: usize,
    v_pos: usize,
}

impl<'a> FirstDirContourIter<'a> {
    fn new(pts: &'a [Point], verbs: &'a [Verb]) -> Self {
        Self {
            pts,
            verbs,
            pt_pos: 0,
            v_pos: 0,
        }
    }

    fn next_pts(&mut self) -> Option<&'a [Point]> {
        if self.v_pos >= self.verbs.len() {
            return None;
        }
        debug_assert!(self.verbs[self.v_pos] == Verb::Move);
        let start_pt = self.pt_pos;
        let mut pt_count = 1usize;
        self.v_pos += 1;
        while self.v_pos < self.verbs.len() {
            match self.verbs[self.v_pos] {
                Verb::Move => break,
                Verb::Line => pt_count += 1,
                Verb::Quad | Verb::Conic => pt_count += 2,
                Verb::Cubic => pt_count += 3,
                Verb::Close => {}
            }
            self.v_pos += 1;
        }
        self.pt_pos += pt_count;
        Some(&self.pts[start_pt..start_pt + pt_count])
    }
}

/// Matching `SkPathPriv::ComputeFirstDirection(const SkPathRaw&)`.
pub fn compute_first_direction(raw: &PathRaw<'_>) -> FirstDirection {
    let mut iter = FirstDirContourIter::new(raw.points, raw.verbs);
    let mut ymax = raw.bounds.top;
    let mut ymax_cross = 0.0_f32;

    while let Some(pts) = iter.next_pts() {
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let index = find_max_y(pts);
        if pts[index].y < ymax {
            continue;
        }

        let mut cross: Option<f32> = None;
        if pts[(index + 1) % n].y == pts[index].y {
            let (min_idx, max_idx) = find_min_max_x_at_y(pts, index);
            if min_idx != max_idx {
                cross = Some((min_idx as isize - max_idx as isize) as f32);
            }
        }
        let cross = match cross {
            Some(c) => c,
            None => {
                let prev = find_diff_pt(pts, index, n - 1);
                if prev == index {
                    continue;
                }
                let next = find_diff_pt(pts, index, 1);
                let mut c = cross_prod(pts[prev], pts[index], pts[next]);
                if c == 0.0 && pts[prev].y == pts[index].y && pts[next].y == pts[index].y {
                    c = pts[index].x - pts[next].x;
                }
                c
            }
        };

        if cross != 0.0 {
            ymax = pts[index].y;
            ymax_cross = cross;
        }
    }

    if ymax_cross != 0.0 {
        if ymax_cross > 0.0 {
            FirstDirection::Cw
        } else {
            FirstDirection::Ccw
        }
    } else {
        FirstDirection::Unknown
    }
}

// ============================================================================
// Tight Bounds (matching SkPathPriv::ComputeTightBounds)
// ============================================================================

fn compute_quad_extremas(src: &[Point; 3], extremas: &mut [Point; 5]) -> usize {
    let mut ts = [0.0_f32; 2];
    let mut n = 0usize;
    let mut tx = 0.0_f32;
    if find_quad_extrema(src[0].x, src[1].x, src[2].x, &mut tx) != 0 {
        ts[n] = tx;
        n += 1;
    }
    let mut ty = 0.0_f32;
    if find_quad_extrema(src[0].y, src[1].y, src[2].y, &mut ty) != 0 {
        ts[n] = ty;
        n += 1;
    }
    for i in 0..n {
        extremas[i] = eval_quad_at(src, ts[i]);
    }
    extremas[n] = src[2];
    n + 1
}

fn compute_conic_extremas(src: &[Point; 3], w: f32, extremas: &mut [Point; 5]) -> usize {
    let mut conic = Conic::default();
    conic.set(*src, w);
    let mut ts = [0.0_f32; 2];
    let mut n = 0usize;
    if let Some(t) = conic.find_x_extrema() {
        ts[n] = t;
        n += 1;
    }
    if let Some(t) = conic.find_y_extrema() {
        ts[n] = t;
        n += 1;
    }
    for i in 0..n {
        extremas[i] = conic.eval_at(ts[i]);
    }
    extremas[n] = src[2];
    n + 1
}

fn compute_cubic_extremas(src: &[Point; 4], extremas: &mut [Point; 5]) -> usize {
    let mut ts = [0.0_f32; 4];
    let mut tx = [0.0_f32; 2];
    let nx = find_cubic_extrema(src[0].x, src[1].x, src[2].x, src[3].x, &mut tx) as usize;
    ts[..nx].copy_from_slice(&tx[..nx]);
    let mut ty = [0.0_f32; 2];
    let ny = find_cubic_extrema(src[0].y, src[1].y, src[2].y, src[3].y, &mut ty) as usize;
    ts[nx..nx + ny].copy_from_slice(&ty[..ny]);
    let n = nx + ny;
    for i in 0..n {
        let mut p = Point::ZERO;
        eval_cubic_at(src, ts[i], Some(&mut p), None, None);
        extremas[i] = p;
    }
    extremas[n] = src[3];
    n + 1
}

/// Matching `SkPathPriv::ComputeTightBounds`.
pub fn compute_tight_bounds(points: &[Point], verbs: &[Verb], conic_weights: &[f32]) -> Rect {
    if verbs.is_empty() {
        return Rect::EMPTY;
    }
    let mut l = points[0].x;
    let mut t = points[0].y;
    let mut r = points[0].x;
    let mut b = points[0].y;

    for (verb, pts, w) in RangeIter::new(points, verbs, conic_weights) {
        let mut extremas = [Point::ZERO; 5];
        let count = match verb {
            Verb::Move => {
                extremas[0] = pts[0];
                1
            }
            Verb::Line => {
                extremas[0] = pts[1];
                1
            }
            Verb::Quad => compute_quad_extremas(&[pts[0], pts[1], pts[2]], &mut extremas),
            Verb::Conic => compute_conic_extremas(&[pts[0], pts[1], pts[2]], w, &mut extremas),
            Verb::Cubic => {
                compute_cubic_extremas(&[pts[0], pts[1], pts[2], pts[3]], &mut extremas)
            }
            Verb::Close => 0,
        };
        for p in &extremas[..count] {
            // std::fminf / std::fmaxf semantics: ignore NaN
            l = p.x.min(l);
            t = p.y.min(t);
            r = p.x.max(r);
            b = p.y.max(b);
        }
    }
    Rect::new(l, t, r, b)
}

// ============================================================================
// Hit Testing / Contains (matching SkPathPriv::Contains)
// ============================================================================

#[inline]
fn poly_eval_quad(a: f32, b: f32, c: f32, t: f32) -> f32 {
    (a * t + b) * t + c
}

#[inline]
fn poly_eval_cubic(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    ((a * t + b) * t + c) * t + d
}

#[inline]
fn between(a: f32, b: f32, c: f32) -> bool {
    (a - b) * (c - b) <= 0.0
}

#[inline]
fn eval_cubic_pts(c0: f32, c1: f32, c2: f32, c3: f32, t: f32) -> f32 {
    let a = c3 + 3.0 * (c1 - c2) - c0;
    let b = 3.0 * (c2 - c1 - c1 + c0);
    let c = 3.0 * (c1 - c0);
    let d = c0;
    poly_eval_cubic(a, b, c, d, t)
}

fn find_minmax_x(pts: &[Point; 4]) -> (f32, f32) {
    let mut min_x = pts[0].x;
    let mut max_x = pts[0].x;
    for p in &pts[1..] {
        min_x = cpp_min_f32(min_x, p.x);
        max_x = cpp_max_f32(max_x, p.x);
    }
    (min_x, max_x)
}

#[inline]
fn check_on_curve(x: f32, y: f32, start: Point, end: Point) -> bool {
    if start.y == end.y {
        between(start.x, x, end.x) && x != end.x
    } else {
        x == start.x && y == start.y
    }
}

fn winding_mono_cubic(pts: &[Point; 4], x: f32, y: f32, on_curve_count: &mut i32) -> i32 {
    let mut y0 = pts[0].y;
    let mut y3 = pts[3].y;
    let mut dir = 1;
    if y0 > y3 {
        std::mem::swap(&mut y0, &mut y3);
        dir = -1;
    }
    if y < y0 || y > y3 {
        return 0;
    }
    if check_on_curve(x, y, pts[0], pts[3]) {
        *on_curve_count += 1;
        return 0;
    }
    if y == y3 {
        return 0;
    }
    let (min_x, max_x) = find_minmax_x(pts);
    if x < min_x {
        return 0;
    }
    if x > max_x {
        return dir;
    }
    let t = match chop_mono_at_y(pts, y) {
        Some(t) => t,
        None => return 0,
    };
    let xt = eval_cubic_pts(pts[0].x, pts[1].x, pts[2].x, pts[3].x, t);
    if scalar_nearly_equal(xt, x, SK_SCALAR_NEARLY_ZERO) {
        if x != pts[3].x || y != pts[3].y {
            *on_curve_count += 1;
            return 0;
        }
    }
    if xt < x { dir } else { 0 }
}

fn winding_cubic(pts: &[Point; 4], x: f32, y: f32, on_curve_count: &mut i32) -> i32 {
    let mut dst = [Point::ZERO; 10];
    let n = chop_cubic_at_y_extrema(pts, Some(&mut dst)) as usize;
    let mut w = 0;
    for i in 0..=n {
        let c = [dst[i * 3], dst[i * 3 + 1], dst[i * 3 + 2], dst[i * 3 + 3]];
        w += winding_mono_cubic(&c, x, y, on_curve_count);
    }
    w
}

fn conic_eval_numerator_x(pts: &[Point; 3], w: f32, t: f32) -> f64 {
    let src2w = pts[1].x * w;
    let c = pts[0].x;
    let a = pts[2].x - 2.0 * src2w + c;
    let b = 2.0 * (src2w - c);
    poly_eval_quad(a, b, c, t) as f64
}

fn conic_eval_denominator(w: f32, t: f32) -> f64 {
    let b = 2.0 * (w - 1.0);
    let c = 1.0_f32;
    let a = -b;
    poly_eval_quad(a, b, c, t) as f64
}

fn winding_mono_conic(conic: &Conic, x: f32, y: f32, on_curve_count: &mut i32) -> i32 {
    let pts = &conic.pts;
    let mut y0 = pts[0].y;
    let mut y2 = pts[2].y;
    let mut dir = 1;
    if y0 > y2 {
        std::mem::swap(&mut y0, &mut y2);
        dir = -1;
    }
    if y < y0 || y > y2 {
        return 0;
    }
    if check_on_curve(x, y, pts[0], pts[2]) {
        *on_curve_count += 1;
        return 0;
    }
    if y == y2 {
        return 0;
    }

    let mut roots = [0.0_f32; 2];
    let mut a = pts[2].y;
    let mut b = pts[1].y * conic.w - y * conic.w + y;
    let mut c = pts[0].y;
    a += c - 2.0 * b;
    b -= c;
    c -= y;
    let n = find_unit_quad_roots(a, 2.0 * b, c, &mut roots);
    let xt = if n == 0 {
        pts[(1 - dir) as usize].x
    } else {
        let t = roots[0];
        (conic_eval_numerator_x(pts, conic.w, t) / conic_eval_denominator(conic.w, t)) as f32
    };
    if scalar_nearly_equal(xt, x, SK_SCALAR_NEARLY_ZERO) {
        if x != pts[2].x || y != pts[2].y {
            *on_curve_count += 1;
            return 0;
        }
    }
    if xt < x { dir } else { 0 }
}

#[inline]
fn is_mono_quad(y0: f32, y1: f32, y2: f32) -> bool {
    if y0 == y1 {
        true
    } else if y0 < y1 {
        y1 <= y2
    } else {
        y1 >= y2
    }
}

fn winding_conic(pts: &[Point; 3], x: f32, y: f32, weight: f32, on_curve_count: &mut i32) -> i32 {
    let mut conic = Conic::default();
    conic.set(*pts, weight);
    let mut chopped = [Conic::default(); 2];
    let is_mono = is_mono_quad(pts[0].y, pts[1].y, pts[2].y) || !conic.chop_at_y_extrema(&mut chopped);
    let mut w = winding_mono_conic(if is_mono { &conic } else { &chopped[0] }, x, y, on_curve_count);
    if !is_mono {
        w += winding_mono_conic(&chopped[1], x, y, on_curve_count);
    }
    w
}

fn winding_mono_quad(pts: &[Point; 3], x: f32, y: f32, on_curve_count: &mut i32) -> i32 {
    let mut y0 = pts[0].y;
    let mut y2 = pts[2].y;
    let mut dir = 1;
    if y0 > y2 {
        std::mem::swap(&mut y0, &mut y2);
        dir = -1;
    }
    if y < y0 || y > y2 {
        return 0;
    }
    if check_on_curve(x, y, pts[0], pts[2]) {
        *on_curve_count += 1;
        return 0;
    }
    if y == y2 {
        return 0;
    }
    let mut roots = [0.0_f32; 2];
    let n = find_unit_quad_roots(
        pts[0].y - 2.0 * pts[1].y + pts[2].y,
        2.0 * (pts[1].y - pts[0].y),
        pts[0].y - y,
        &mut roots,
    );
    let xt = if n == 0 {
        pts[(1 - dir) as usize].x
    } else {
        let t = roots[0];
        let c = pts[0].x;
        let a = pts[2].x - 2.0 * pts[1].x + c;
        let b = 2.0 * (pts[1].x - c);
        poly_eval_quad(a, b, c, t)
    };
    if scalar_nearly_equal(xt, x, SK_SCALAR_NEARLY_ZERO) {
        if x != pts[2].x || y != pts[2].y {
            *on_curve_count += 1;
            return 0;
        }
    }
    if xt < x { dir } else { 0 }
}

fn winding_quad(pts: &[Point; 3], x: f32, y: f32, on_curve_count: &mut i32) -> i32 {
    let mut storage = [Point::ZERO; 5];
    let mut n = 0;
    if !is_mono_quad(pts[0].y, pts[1].y, pts[2].y) {
        n = chop_quad_at_y_extrema(pts, &mut storage);
    }
    let first = if n > 0 {
        [storage[0], storage[1], storage[2]]
    } else {
        *pts
    };
    let mut w = winding_mono_quad(&first, x, y, on_curve_count);
    if n > 0 {
        let second = [storage[2], storage[3], storage[4]];
        w += winding_mono_quad(&second, x, y, on_curve_count);
    }
    w
}

fn winding_line(pts: &[Point; 2], x: f32, y: f32, on_curve_count: &mut i32) -> i32 {
    let x0 = pts[0].x;
    let mut y0 = pts[0].y;
    let x1 = pts[1].x;
    let mut y1 = pts[1].y;
    let dy = y1 - y0;
    let mut dir = 1;
    if y0 > y1 {
        std::mem::swap(&mut y0, &mut y1);
        dir = -1;
    }
    if y < y0 || y > y1 {
        return 0;
    }
    if check_on_curve(x, y, pts[0], pts[1]) {
        *on_curve_count += 1;
        return 0;
    }
    if y == y1 {
        return 0;
    }
    let cross = (x1 - x0) * (y - pts[0].y) - dy * (x - x0);
    if cross == 0.0 {
        if x != x1 || y != pts[1].y {
            *on_curve_count += 1;
        }
        dir = 0;
    } else if scalar_sign_as_int(cross) == dir {
        dir = 0;
    }
    dir
}

fn tangent_cubic(pts: &[Point; 4], x: f32, y: f32, tangents: &mut Vec<Vector>) {
    if !between(pts[0].y, y, pts[1].y)
        && !between(pts[1].y, y, pts[2].y)
        && !between(pts[2].y, y, pts[3].y)
    {
        return;
    }
    if !between(pts[0].x, x, pts[1].x)
        && !between(pts[1].x, x, pts[2].x)
        && !between(pts[2].x, x, pts[3].x)
    {
        return;
    }
    let mut dst = [Point::ZERO; 10];
    let n = chop_cubic_at_y_extrema(pts, Some(&mut dst)) as usize;
    for i in 0..=n {
        let c = [dst[i * 3], dst[i * 3 + 1], dst[i * 3 + 2], dst[i * 3 + 3]];
        let t = match chop_mono_at_y(&c, y) {
            Some(t) => t,
            None => continue,
        };
        let xt = eval_cubic_pts(c[0].x, c[1].x, c[2].x, c[3].x, t);
        if !scalar_nearly_equal(x, xt, SK_SCALAR_NEARLY_ZERO) {
            continue;
        }
        let mut tangent = Vector::ZERO;
        eval_cubic_at(&c, t, None, Some(&mut tangent), None);
        tangents.push(tangent);
    }
}

fn tangent_conic(pts: &[Point; 3], x: f32, y: f32, w: f32, tangents: &mut Vec<Vector>) {
    if !between(pts[0].y, y, pts[1].y) && !between(pts[1].y, y, pts[2].y) {
        return;
    }
    if !between(pts[0].x, x, pts[1].x) && !between(pts[1].x, x, pts[2].x) {
        return;
    }
    let mut roots = [0.0_f32; 2];
    let mut a = pts[2].y;
    let mut b = pts[1].y * w - y * w + y;
    let mut c = pts[0].y;
    a += c - 2.0 * b;
    b -= c;
    c -= y;
    let n = find_unit_quad_roots(a, 2.0 * b, c, &mut roots) as usize;
    for &t in &roots[..n] {
        let xt = (conic_eval_numerator_x(pts, w, t) / conic_eval_denominator(w, t)) as f32;
        if !scalar_nearly_equal(x, xt, SK_SCALAR_NEARLY_ZERO) {
            continue;
        }
        let mut conic = Conic::default();
        conic.set(*pts, w);
        tangents.push(conic.eval_tangent_at(t));
    }
}

fn tangent_quad(pts: &[Point; 3], x: f32, y: f32, tangents: &mut Vec<Vector>) {
    if !between(pts[0].y, y, pts[1].y) && !between(pts[1].y, y, pts[2].y) {
        return;
    }
    if !between(pts[0].x, x, pts[1].x) && !between(pts[1].x, x, pts[2].x) {
        return;
    }
    let mut roots = [0.0_f32; 2];
    let n = find_unit_quad_roots(
        pts[0].y - 2.0 * pts[1].y + pts[2].y,
        2.0 * (pts[1].y - pts[0].y),
        pts[0].y - y,
        &mut roots,
    ) as usize;
    for &t in &roots[..n] {
        let c = pts[0].x;
        let a = pts[2].x - 2.0 * pts[1].x + c;
        let b = 2.0 * (pts[1].x - c);
        let xt = poly_eval_quad(a, b, c, t);
        if !scalar_nearly_equal(x, xt, SK_SCALAR_NEARLY_ZERO) {
            continue;
        }
        tangents.push(eval_quad_tangent_at(pts, t));
    }
}

fn tangent_line(pts: &[Point; 2], x: f32, y: f32, tangents: &mut Vec<Vector>) {
    let y0 = pts[0].y;
    let y1 = pts[1].y;
    if !between(y0, y, y1) {
        return;
    }
    let x0 = pts[0].x;
    let x1 = pts[1].x;
    if !between(x0, x, x1) {
        return;
    }
    let dx = x1 - x0;
    let dy = y1 - y0;
    if !scalar_nearly_equal((x - x0) * dy, dx * (y - y0), SK_SCALAR_NEARLY_ZERO) {
        return;
    }
    tangents.push(Vector::new(dx, dy));
}

#[inline]
fn contains_inclusive(r: Rect, p: Point) -> bool {
    r.left <= p.x && p.x <= r.right && r.top <= p.y && p.y <= r.bottom
}

/// Matching `SkPathPriv::Contains`.
pub fn contains(raw: &PathRaw<'_>, p: Point) -> bool {
    let ft = raw.fill_type;
    let is_inverse = ft.is_inverse();
    if raw.is_empty() {
        return is_inverse;
    }
    if !contains_inclusive(raw.bounds, p) {
        return is_inverse;
    }

    let mut w = 0i32;
    let mut on_curve_count = 0i32;

    let mut iter = EdgeIter::new(raw);
    while let Some(rec) = iter.next_edge() {
        match rec.edge {
            Edge::Line => {
                w += winding_line(&[rec.points[0], rec.points[1]], p.x, p.y, &mut on_curve_count)
            }
            Edge::Quad => {
                w += winding_quad(
                    &[rec.points[0], rec.points[1], rec.points[2]],
                    p.x,
                    p.y,
                    &mut on_curve_count,
                )
            }
            Edge::Conic => {
                w += winding_conic(
                    &[rec.points[0], rec.points[1], rec.points[2]],
                    p.x,
                    p.y,
                    rec.conic_weight,
                    &mut on_curve_count,
                )
            }
            Edge::Cubic => {
                w += winding_cubic(&rec.points, p.x, p.y, &mut on_curve_count)
            }
        }
    }

    let even_odd = ft.is_even_odd();
    if even_odd {
        w &= 1;
    }
    if w != 0 {
        return !is_inverse;
    }
    if on_curve_count <= 1 {
        return (on_curve_count != 0) ^ is_inverse;
    }
    if (on_curve_count & 1) != 0 || even_odd {
        return ((on_curve_count & 1) != 0) ^ is_inverse;
    }

    let mut tangents: Vec<Vector> = Vec::new();
    let mut iter = EdgeIter::new(raw);
    while let Some(rec) = iter.next_edge() {
        let old_count = tangents.len();
        match rec.edge {
            Edge::Line => tangent_line(&[rec.points[0], rec.points[1]], p.x, p.y, &mut tangents),
            Edge::Quad => tangent_quad(
                &[rec.points[0], rec.points[1], rec.points[2]],
                p.x,
                p.y,
                &mut tangents,
            ),
            Edge::Conic => tangent_conic(
                &[rec.points[0], rec.points[1], rec.points[2]],
                p.x,
                p.y,
                rec.conic_weight,
                &mut tangents,
            ),
            Edge::Cubic => tangent_cubic(&rec.points, p.x, p.y, &mut tangents),
        }
        if tangents.len() > old_count {
            let last = tangents.len() - 1;
            let tangent = tangents[last];
            if scalar_nearly_zero(tangent.length_sqd(), SK_SCALAR_NEARLY_ZERO) {
                tangents.remove(last);
            } else {
                for index in 0..last {
                    let test = tangents[index];
                    if scalar_nearly_zero(test.cross(tangent), SK_SCALAR_NEARLY_ZERO)
                        && scalar_sign_as_int(tangent.x * test.x) <= 0
                        && scalar_sign_as_int(tangent.y * test.y) <= 0
                    {
                        tangents.remove(last);
                        tangents.swap_remove(index);
                        break;
                    }
                }
            }
        }
    }
    (!tangents.is_empty()) ^ is_inverse
}

// ============================================================================
// Conservatively Contains Rect (matching SkPath::conservativelyContainsRect)
// ============================================================================

#[inline]
fn check_edge_against_rect(p0: Point, p1: Point, rect: Rect, dir: Direction) -> bool {
    let (v, edge_begin) = if dir == Direction::Cw {
        (p1 - p0, p0)
    } else {
        (p0 - p1, p1)
    };
    if v.x != 0.0 || v.y != 0.0 {
        let y_l = v.y * (rect.left - edge_begin.x);
        let x_t = v.x * (rect.top - edge_begin.y);
        let y_r = v.y * (rect.right - edge_begin.x);
        let x_b = v.x * (rect.bottom - edge_begin.y);
        if !(y_l.is_finite() && x_t.is_finite() && y_r.is_finite() && x_b.is_finite()) {
            return false;
        }
        if (x_t < y_l) || (x_t < y_r) || (x_b < y_l) || (x_b < y_r) {
            return false;
        }
    }
    true
}

/// Matching `SkPath::conservativelyContainsRect`.
pub fn conservatively_contains_rect(
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    convexity: Convexity,
    is_finite: bool,
    rect: Rect,
) -> bool {
    if !convexity.is_convex() {
        return false;
    }
    let direction = match convexity.to_direction() {
        Some(d) => d,
        None => return false,
    };

    let mut first_pt = Point::ZERO;
    let mut prev_pt = Point::ZERO;
    let mut segment_count = 0;

    let iter_verbs = if is_finite { verbs } else { &[] };
    for (verb, pts, weight) in RangeIter::new(points, iter_verbs, conics) {
        if verb == Verb::Close || (segment_count > 0 && verb == Verb::Move) {
            segment_count += 1;
            break;
        } else if verb == Verb::Move {
            first_pt = pts[0];
            prev_pt = pts[0];
        } else {
            let point_count = verb.pts_in_verb();
            if !all_points_eq(&pts[..point_count + 1]) {
                let next_pt = point_count;
                segment_count += 1;

                if prev_pt == pts[next_pt] {
                    return false;
                } else if verb == Verb::Conic {
                    let mut orig = Conic::default();
                    orig.set([pts[0], pts[1], pts[2]], weight);
                    let mut quad_pts = [Point::ZERO; 5];
                    let count = orig.chop_into_quads_pow2(&mut quad_pts, 1);
                    debug_assert_eq!(count, 2);
                    if !check_edge_against_rect(quad_pts[0], quad_pts[2], rect, direction) {
                        return false;
                    }
                    if !check_edge_against_rect(quad_pts[2], quad_pts[4], rect, direction) {
                        return false;
                    }
                } else if !check_edge_against_rect(prev_pt, pts[next_pt], rect, direction) {
                    return false;
                }
                prev_pt = pts[next_pt];
            }
        }
    }

    if segment_count > 0 {
        check_edge_against_rect(prev_pt, first_pt, rect, direction)
    } else {
        false
    }
}

// ============================================================================
// Perspective Clip (matching SkPathPriv::PerspectiveClip)
// ============================================================================

struct HalfPlane {
    a: f32,
    b: f32,
    c: f32,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum HalfPlaneResult {
    AllNegative,
    AllPositive,
    Mixed,
}

impl HalfPlane {
    #[inline]
    fn eval(&self, x: f32, y: f32) -> f32 {
        self.a * x + self.b * y + self.c
    }

    fn normalize(&mut self) -> bool {
        let mut a = self.a as f64;
        let mut b = self.b as f64;
        let mut c = self.c as f64;
        let dmag = (a * a + b * b).sqrt();
        if dmag == 0.0 {
            self.a = 0.0;
            self.b = 0.0;
            self.c = 1.0;
            return true;
        }
        let dscale = ieee_double_divide(1.0, dmag);
        a *= dscale;
        b *= dscale;
        c *= dscale;
        if !(a.is_finite() && b.is_finite() && c.is_finite()) || (a == 0.0 && b == 0.0) {
            self.a = 0.0;
            self.b = 0.0;
            self.c = 1.0;
            return false;
        }
        self.a = a as f32;
        self.b = b as f32;
        self.c = c as f32;
        true
    }

    fn test(&self, bounds: Rect) -> HalfPlaneResult {
        let (min_x, max_x) = if self.a >= 0.0 {
            (bounds.left, bounds.right)
        } else {
            (bounds.right, bounds.left)
        };
        let (min_y, max_y) = if self.b >= 0.0 {
            (bounds.top, bounds.bottom)
        } else {
            (bounds.bottom, bounds.top)
        };
        let test = self.eval(min_x, min_y);
        let sign = test * self.eval(max_x, max_y);
        if sign > 0.0 {
            if test < 0.0 {
                HalfPlaneResult::AllNegative
            } else {
                HalfPlaneResult::AllPositive
            }
        } else {
            HalfPlaneResult::Mixed
        }
    }
}

fn clip_path_to_half_plane(raw: &PathRaw<'_>, plane: &HalfPlane) -> Option<PathDataBuf> {
    let p0 = Point::new(-plane.a * plane.c, -plane.b * plane.c);
    let mx = Matrix::make_all(plane.b, plane.a, p0.x, -plane.a, plane.b, p0.y, 0.0, 0.0, 1.0);
    let inv = mx.invert()?;

    // Transform points by inv (non-perspective).
    let mut rot_pts = vec![Point::ZERO; raw.points.len()];
    inv.map_points(&mut rot_pts, raw.points);
    let rot_bounds = crate::types::trimmed_bounds(&rot_pts, raw.verbs)?;
    let rot_raw = PathRaw {
        points: &rot_pts,
        verbs: raw.verbs,
        conics: raw.conics,
        bounds: rot_bounds,
        fill_type: raw.fill_type,
        convexity: Convexity::Unknown,
        segment_mask: raw.segment_mask,
    };

    let big = SK_SCALAR_MAX;
    let clip = Rect::new(-big, 0.0, big, big);

    let mut result = PathBuilder::new(raw.fill_type);
    let mut prev = Point::ZERO;

    let mut consume = |clipper: &EdgeClipOutput, mut new_ctr: bool| {
        let mut add_line_to = false;
        let mut p_idx = 0usize;
        for v_i in 0..clipper.verb_count {
            let verb = clipper.verbs[v_i];
            let pts = &clipper.points[p_idx..];
            if new_ctr {
                result.move_to(pts[0]);
                prev = pts[0];
                new_ctr = false;
            }
            if add_line_to || pts[0] != prev {
                result.line_to(pts[0]);
            }
            match verb {
                VERB_LINE => {
                    result.line_to(pts[1]);
                    prev = pts[1];
                    p_idx += 2;
                }
                VERB_QUAD => {
                    result.quad_to(pts[1], pts[2]);
                    prev = pts[2];
                    p_idx += 3;
                }
                VERB_CUBIC => {
                    result.cubic_to(pts[1], pts[2], pts[3]);
                    prev = pts[3];
                    p_idx += 4;
                }
                _ => {}
            }
            add_line_to = true;
        }
    };

    const CONIC_TOL: f32 = 0.25;
    let mut edge_iter = EdgeIter::new(&rot_raw);
    let mut clipper = EdgeClipOutput::new(false);
    let mut conic_quad_buf = vec![Point::ZERO; (1 << (MAX_CONIC_TO_QUAD_POW2 + 1)) + 1];

    while let Some(e) = edge_iter.next_edge() {
        match e.edge {
            Edge::Line => {
                if clipper.clip_line(e.points[0], e.points[1], clip) {
                    consume(&clipper, e.is_new_contour);
                }
            }
            Edge::Quad => {
                if clipper.clip_quad(&[e.points[0], e.points[1], e.points[2]], clip) {
                    consume(&clipper, e.is_new_contour);
                }
            }
            Edge::Conic => {
                let mut conic = Conic::default();
                conic.set([e.points[0], e.points[1], e.points[2]], e.conic_weight);
                let pow2 = conic.compute_quad_pow2(CONIC_TOL);
                let qcount = conic.chop_into_quads_pow2(&mut conic_quad_buf, pow2) as usize;
                for i in 0..qcount {
                    let q = [
                        conic_quad_buf[i * 2],
                        conic_quad_buf[i * 2 + 1],
                        conic_quad_buf[i * 2 + 2],
                    ];
                    if clipper.clip_quad(&q, clip) {
                        consume(&clipper, e.is_new_contour);
                    }
                }
            }
            Edge::Cubic => {
                if clipper.clip_cubic(&e.points, clip) {
                    consume(&clipper, e.is_new_contour);
                }
            }
        }
    }

    result.transform(&mx);
    let buf = result.into_buf();
    if !buf.is_finite() {
        return None;
    }
    Some(buf)
}

/// Matching `SkPathPriv::PerspectiveClip`.
/// Returns `None` if no clipping was needed (caller keeps original path),
/// or `Some(clipped_buf)` if clipping occurred (which may be empty).
pub fn perspective_clip(raw: &PathRaw<'_>, matrix: &Matrix) -> Option<PathDataBuf> {
    if !matrix.has_perspective() {
        return None;
    }
    let mut plane = HalfPlane {
        a: matrix.mat[MPERSP_0],
        b: matrix.mat[MPERSP_1],
        c: matrix.mat[MPERSP_2] - K_W0_PLANE_DISTANCE,
    };
    if plane.normalize() {
        match plane.test(raw.bounds) {
            HalfPlaneResult::AllPositive => return None,
            HalfPlaneResult::Mixed => {
                return Some(
                    clip_path_to_half_plane(raw, &plane)
                        .unwrap_or_else(|| PathDataBuf::new(crate::types::FillType::Winding)),
                );
            }
            HalfPlaneResult::AllNegative => {}
        }
    }
    Some(PathDataBuf::new(crate::types::FillType::Winding))
}
