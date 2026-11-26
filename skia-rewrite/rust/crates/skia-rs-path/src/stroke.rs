#![forbid(unsafe_code)]

use crate::analysis::{compute_convexity, compute_first_direction};
use crate::builder::PathBuilder;
use crate::iter::LegacyPathIter;
use crate::shapes::is_rect_contour;
use crate::types::{
    can_normalize, equals_within_tolerance, ieee_float_divide, is_zero_length_since_point,
    rotate_ccw, rotate_cw, AddPathMode, Cap, Convexity, Direction, FillType, FirstDirection, Join,
    PathIsAType, PathRaw, StrokeStyle, Verb, SEG_LINE, SK_PAINT_DEFAULTS_MITER_LIMIT,
    SK_SCALAR_SQRT_2,
};
use skia_rs_geometry::geometry::{
    chop_cubic_at_1, eval_cubic_at, eval_quad_at, eval_quad_tangent_at, find_cubic_cusp,
    find_cubic_inflections, find_cubic_max_curvature, find_quad_max_curvature,
    find_unit_quad_roots, Conic, MAX_CONICS_FOR_ARC,
};
use skia_rs_geometry::{
    scalar_nearly_zero, Matrix, Point, RRect, Rect, Vector, SK_SCALAR_NEARLY_ZERO,
    SK_SCALAR_ROOT_2_OVER_2,
};

const K_STROKE_REC_FILL_STYLE_WIDTH: f32 = -1.0;
const K_ONE_OVER_SQRT2: f32 = 0.707_106_78_f32;

// ============================================================================
// SkStrokeRec
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StrokeRec {
    pub res_scale: f32,
    pub width: f32,
    pub miter_limit: f32,
    pub cap: Cap,
    pub join: Join,
    pub stroke_and_fill: bool,
}

impl StrokeRec {
    pub fn new_fill() -> Self {
        Self {
            res_scale: 1.0,
            width: K_STROKE_REC_FILL_STYLE_WIDTH,
            miter_limit: SK_PAINT_DEFAULTS_MITER_LIMIT,
            cap: Cap::Butt,
            join: Join::Miter,
            stroke_and_fill: false,
        }
    }

    pub fn new_hairline() -> Self {
        Self {
            res_scale: 1.0,
            width: 0.0,
            miter_limit: SK_PAINT_DEFAULTS_MITER_LIMIT,
            cap: Cap::Butt,
            join: Join::Miter,
            stroke_and_fill: false,
        }
    }

    pub fn from_paint_params(
        paint_style: u8,
        stroke_width: f32,
        stroke_miter: f32,
        cap: Cap,
        join: Join,
        res_scale: f32,
    ) -> Self {
        // SkPaint::Style: 0 = Fill, 1 = Stroke, 2 = StrokeAndFill
        let (width, stroke_and_fill) = match paint_style {
            0 => (K_STROKE_REC_FILL_STYLE_WIDTH, false),
            1 => (stroke_width, false),
            2 => {
                if stroke_width == 0.0 {
                    (K_STROKE_REC_FILL_STYLE_WIDTH, false)
                } else {
                    (stroke_width, true)
                }
            }
            _ => (K_STROKE_REC_FILL_STYLE_WIDTH, false),
        };
        Self {
            res_scale,
            width,
            miter_limit: stroke_miter,
            cap,
            join,
            stroke_and_fill,
        }
    }

    #[inline]
    pub fn get_style(&self) -> StrokeStyle {
        if self.width < 0.0 {
            StrokeStyle::Fill
        } else if self.width == 0.0 {
            StrokeStyle::Hairline
        } else if self.stroke_and_fill {
            StrokeStyle::StrokeAndFill
        } else {
            StrokeStyle::Stroke
        }
    }

    #[inline]
    pub fn is_hairline_style(&self) -> bool {
        self.get_style() == StrokeStyle::Hairline
    }

    #[inline]
    pub fn is_fill_style(&self) -> bool {
        self.get_style() == StrokeStyle::Fill
    }

    #[inline]
    pub fn set_fill_style(&mut self) {
        self.width = K_STROKE_REC_FILL_STYLE_WIDTH;
        self.stroke_and_fill = false;
    }

    #[inline]
    pub fn set_hairline_style(&mut self) {
        self.width = 0.0;
        self.stroke_and_fill = false;
    }

    #[inline]
    pub fn set_stroke_style(&mut self, width: f32, stroke_and_fill: bool) {
        if stroke_and_fill && width == 0.0 {
            self.set_fill_style();
        } else {
            self.width = width;
            self.stroke_and_fill = stroke_and_fill;
        }
    }

    #[inline]
    pub fn set_stroke_params(&mut self, cap: Cap, join: Join, miter_limit: f32) {
        self.cap = cap;
        self.join = join;
        self.miter_limit = miter_limit;
    }

    pub fn apply_to_path(&self, dst: &mut PathBuilder, src: &PathRaw<'_>) -> bool {
        if self.width <= 0.0 {
            return false;
        }
        let stroker = Stroke {
            width: self.width,
            miter_limit: self.miter_limit,
            res_scale: self.res_scale,
            cap: self.cap,
            join: self.join,
            do_fill: self.stroke_and_fill,
        };
        stroker.stroke_path(src, dst);
        true
    }

    #[inline]
    pub fn get_inflation_radius(&self) -> f32 {
        Self::compute_inflation_radius(self.join, self.miter_limit, self.cap, self.width)
    }

    /// Matches `SkStrokeRec::GetInflationRadius(join, miterLimit, cap, strokeWidth)`.
    pub fn compute_inflation_radius(
        join: Join,
        miter_limit: f32,
        cap: Cap,
        stroke_width: f32,
    ) -> f32 {
        if stroke_width < 0.0 {
            return 0.0;
        } else if stroke_width == 0.0 {
            return 1.0;
        }

        let mut multiplier = 1.0_f32;
        if join == Join::Miter {
            multiplier = multiplier.max(miter_limit);
        }
        if cap == Cap::Square {
            multiplier = multiplier.max(SK_SCALAR_SQRT_2);
        }
        (stroke_width / 2.0) * multiplier
    }
}

// ============================================================================
// SkStrokerPriv: Cappers and Joiners
// ============================================================================

pub fn butt_capper(
    sink: &mut PathBuilder,
    _pivot: Point,
    _normal: Vector,
    stop: Point,
    _extend_last_pt: bool,
) {
    sink.line_to(stop);
}

pub fn round_capper(
    sink: &mut PathBuilder,
    pivot: Point,
    normal: Vector,
    stop: Point,
    _extend_last_pt: bool,
) {
    let parallel = rotate_cw(normal);
    let projected_center = pivot + parallel;
    sink.conic_to(
        projected_center + normal,
        projected_center,
        SK_SCALAR_ROOT_2_OVER_2,
    );
    sink.conic_to(projected_center - normal, stop, SK_SCALAR_ROOT_2_OVER_2);
}

pub fn square_capper(
    sink: &mut PathBuilder,
    pivot: Point,
    normal: Vector,
    stop: Point,
    extend_last_pt: bool,
) {
    let parallel = rotate_cw(normal);
    if extend_last_pt {
        sink.set_last_point(pivot + normal + parallel);
        sink.line_to(pivot - normal + parallel);
    } else {
        sink.line_to(pivot + normal + parallel);
        sink.line_to(pivot - normal + parallel);
        sink.line_to(stop);
    }
}

pub fn apply_cap(
    cap: Cap,
    sink: &mut PathBuilder,
    pivot: Point,
    normal: Vector,
    stop: Point,
    extend_last_pt: bool,
) {
    match cap {
        Cap::Butt => butt_capper(sink, pivot, normal, stop, extend_last_pt),
        Cap::Round => round_capper(sink, pivot, normal, stop, extend_last_pt),
        Cap::Square => square_capper(sink, pivot, normal, stop, extend_last_pt),
    }
}

#[inline]
fn is_clockwise(before: Vector, after: Vector) -> bool {
    before.x * after.y > before.y * after.x
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum AngleType {
    Nearly180,
    Sharp,
    Shallow,
    NearlyLine,
}

#[inline]
fn dot_to_angle_type(dot: f32) -> AngleType {
    if dot >= 0.0 {
        if scalar_nearly_zero(1.0 - dot, SK_SCALAR_NEARLY_ZERO) {
            AngleType::NearlyLine
        } else {
            AngleType::Shallow
        }
    } else if scalar_nearly_zero(1.0 + dot, SK_SCALAR_NEARLY_ZERO) {
        AngleType::Nearly180
    } else {
        AngleType::Sharp
    }
}

#[inline]
fn handle_inner_join(inner: &mut PathBuilder, pivot: Point, after: Vector) {
    inner.line_to(pivot);
    inner.line_to(Point::new(pivot.x - after.x, pivot.y - after.y));
}

pub fn blunt_joiner<'a>(
    mut outer: &'a mut PathBuilder,
    mut inner: &'a mut PathBuilder,
    before_unit_normal: Vector,
    pivot: Point,
    after_unit_normal: Vector,
    radius: f32,
    _inv_miter_limit: f32,
    _prev_is_line: bool,
    _curr_is_line: bool,
) {
    let mut after = after_unit_normal * radius;
    if !is_clockwise(before_unit_normal, after_unit_normal) {
        core::mem::swap(&mut outer, &mut inner);
        after = -after;
    }
    outer.line_to(Point::new(pivot.x + after.x, pivot.y + after.y));
    handle_inner_join(inner, pivot, after);
}

#[allow(clippy::too_many_arguments)]
pub fn round_joiner<'a>(
    mut outer: &'a mut PathBuilder,
    mut inner: &'a mut PathBuilder,
    before_unit_normal: Vector,
    pivot: Point,
    after_unit_normal: Vector,
    radius: f32,
    _inv_miter_limit: f32,
    _prev_is_line: bool,
    _curr_is_line: bool,
) {
    let dot_prod = before_unit_normal.dot(after_unit_normal);
    let angle_type = dot_to_angle_type(dot_prod);
    if angle_type == AngleType::NearlyLine {
        return;
    }

    let mut before = before_unit_normal;
    let mut after = after_unit_normal;
    let mut dir_is_ccw = false; // CW

    if !is_clockwise(before, after) {
        core::mem::swap(&mut outer, &mut inner);
        before = -before;
        after = -after;
        dir_is_ccw = true; // CCW
    }

    let mut matrix = Matrix::identity();
    matrix.set_scale(radius, radius);
    matrix.post_translate(pivot.x, pivot.y);
    let mut conics = [Conic::default(); MAX_CONICS_FOR_ARC];
    let count = Conic::build_unit_arc(before, after, dir_is_ccw, Some(&matrix), &mut conics);
    if count > 0 {
        for conic in &conics[..count as usize] {
            outer.conic_to(conic.pts[1], conic.pts[2], conic.w);
        }
        after = after * radius;
        handle_inner_join(inner, pivot, after);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn miter_joiner<'a>(
    mut outer: &'a mut PathBuilder,
    mut inner: &'a mut PathBuilder,
    before_unit_normal: Vector,
    pivot: Point,
    after_unit_normal: Vector,
    radius: f32,
    inv_miter_limit: f32,
    prev_is_line: bool,
    mut curr_is_line: bool,
) {
    let dot_prod = before_unit_normal.dot(after_unit_normal);
    let angle_type = dot_to_angle_type(dot_prod);
    let mut before = before_unit_normal;
    let mut after = after_unit_normal;

    if angle_type == AngleType::NearlyLine {
        return;
    }

    let mut do_blunt = false;
    let mut mid = Vector::ZERO;

    if angle_type == AngleType::Nearly180 {
        curr_is_line = false;
        do_blunt = true;
    } else {
        let ccw = !is_clockwise(before, after);
        if ccw {
            core::mem::swap(&mut outer, &mut inner);
            before = -before;
            after = -after;
        }

        if dot_prod == 0.0 && inv_miter_limit <= K_ONE_OVER_SQRT2 {
            mid = (before + after) * radius;
        } else {
            let sin_half_angle = ((1.0_f32 + dot_prod) / 2.0_f32).sqrt();
            if sin_half_angle < inv_miter_limit {
                curr_is_line = false;
                do_blunt = true;
            } else {
                if angle_type == AngleType::Sharp {
                    mid = Vector::new(after.y - before.y, before.x - after.x);
                    if ccw {
                        mid = -mid;
                    }
                } else {
                    mid = before + after;
                }
                mid.set_length_in_place(radius / sin_half_angle);
            }
        }
    }

    if !do_blunt {
        if prev_is_line {
            outer.set_last_point(pivot + mid);
        } else {
            outer.line_to(pivot + mid);
        }
    }

    after = after * radius;
    if !curr_is_line {
        outer.line_to(Point::new(pivot.x + after.x, pivot.y + after.y));
    }
    handle_inner_join(inner, pivot, after);
}

#[allow(clippy::too_many_arguments)]
pub fn apply_join(
    join: Join,
    outer: &mut PathBuilder,
    inner: &mut PathBuilder,
    before_unit_normal: Vector,
    pivot: Point,
    after_unit_normal: Vector,
    radius: f32,
    inv_miter_limit: f32,
    prev_is_line: bool,
    curr_is_line: bool,
) {
    match join {
        Join::Miter => miter_joiner(
            outer,
            inner,
            before_unit_normal,
            pivot,
            after_unit_normal,
            radius,
            inv_miter_limit,
            prev_is_line,
            curr_is_line,
        ),
        Join::Round => round_joiner(
            outer,
            inner,
            before_unit_normal,
            pivot,
            after_unit_normal,
            radius,
            inv_miter_limit,
            prev_is_line,
            curr_is_line,
        ),
        Join::Bevel => blunt_joiner(
            outer,
            inner,
            before_unit_normal,
            pivot,
            after_unit_normal,
            radius,
            inv_miter_limit,
            prev_is_line,
            curr_is_line,
        ),
    }
}

// ============================================================================
// SkStroke / SkPathStroker
// ============================================================================

const K_TANGENT_RECURSIVE_LIMIT: usize = 0;
const K_CUBIC_RECURSIVE_LIMIT: usize = 1;
const K_CONIC_RECURSIVE_LIMIT: usize = 2;
const K_QUAD_RECURSIVE_LIMIT: usize = 3;
const K_RECURSIVE_LIMITS: [i32; 4] = [5 * 3, 24, 16, 16];

#[inline]
fn degenerate_vector(v: Vector) -> bool {
    !can_normalize(v.x, v.y)
}

fn set_normal_unitnormal_pts(
    before: Point,
    after: Point,
    scale: f32,
    radius: f32,
) -> Option<(Vector, Vector)> {
    let mut unit_normal = Vector::ZERO;
    if !unit_normal.set_normalize((after.x - before.x) * scale, (after.y - before.y) * scale) {
        return None;
    }
    unit_normal = rotate_ccw(unit_normal);
    let normal = unit_normal * radius;
    Some((normal, unit_normal))
}

fn set_normal_unitnormal_vec(vec: Vector, radius: f32) -> Option<(Vector, Vector)> {
    let mut unit_normal = Vector::ZERO;
    if !unit_normal.set_normalize(vec.x, vec.y) {
        return None;
    }
    unit_normal = rotate_ccw(unit_normal);
    let normal = unit_normal * radius;
    Some((normal, unit_normal))
}

#[derive(Copy, Clone, Debug, Default)]
struct QuadConstruct {
    quad: [Point; 3],
    tangent_start: Vector,
    tangent_end: Vector,
    start_t: f32,
    mid_t: f32,
    end_t: f32,
    start_set: bool,
    end_set: bool,
    opposite_tangents: bool,
}

impl QuadConstruct {
    fn init(&mut self, start: f32, end: f32) -> bool {
        self.start_t = start;
        self.mid_t = (start + end) * 0.5_f32;
        self.end_t = end;
        self.start_set = false;
        self.end_set = false;
        self.start_t < self.mid_t && self.mid_t < self.end_t
    }

    fn init_with_start(&mut self, parent: &QuadConstruct) -> bool {
        if !self.init(parent.start_t, parent.mid_t) {
            return false;
        }
        self.quad[0] = parent.quad[0];
        self.tangent_start = parent.tangent_start;
        self.start_set = true;
        true
    }

    fn init_with_end(&mut self, parent: &QuadConstruct) -> bool {
        if !self.init(parent.mid_t, parent.end_t) {
            return false;
        }
        self.quad[2] = parent.quad[2];
        self.tangent_end = parent.tangent_end;
        self.end_set = true;
        true
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum StrokeSide {
    Outer = 1,
    Inner = -1,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ResultType {
    Split,
    Degenerate,
    Quad,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ReductionType {
    Point,
    Line,
    Quad,
    Degenerate,
    Degenerate2,
    Degenerate3,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum IntersectRayType {
    CtrlPt,
    ResultType,
}

fn pt_to_line(pt: Point, line_start: Point, line_end: Point) -> f32 {
    let dxy = line_end - line_start;
    let ab0 = pt - line_start;
    let numer = dxy.dot(ab0);
    let denom = dxy.dot(dxy);
    let t = ieee_float_divide(numer, denom);
    if (0.0..=1.0).contains(&t) {
        let hit = line_start * (1.0 - t) + line_end * t;
        Point::distance_to_sqd(hit, pt)
    } else {
        Point::distance_to_sqd(pt, line_start)
    }
}

fn pt_to_tangent_line(pt: Point, line_start: Point, tangent: Vector) -> f32 {
    let dxy = tangent;
    let ab0 = pt - line_start;
    let numer = dxy.dot(ab0);
    let denom = dxy.dot(dxy);
    let t = ieee_float_divide(numer, denom);
    if (0.0..=1.0).contains(&t) {
        let hit = line_start + tangent * t;
        Point::distance_to_sqd(hit, pt)
    } else {
        Point::distance_to_sqd(pt, line_start)
    }
}

fn cubic_in_line(cubic: &[Point; 4]) -> bool {
    let mut pt_max = -1.0_f32;
    let mut outer1 = 0usize;
    let mut outer2 = 1usize;
    for index in 0..3 {
        for inner in (index + 1)..4 {
            let test_diff = cubic[inner] - cubic[index];
            let test_max = test_diff.x.abs().max(test_diff.y.abs());
            if pt_max < test_max {
                outer1 = index;
                outer2 = inner;
                pt_max = test_max;
            }
        }
    }
    let mid1 = (1 + (2 >> outer2)) >> outer1;
    let mid2 = outer1 ^ outer2 ^ mid1;
    let line_slop = pt_max * pt_max * 0.00001_f32;
    pt_to_line(cubic[mid1], cubic[outer1], cubic[outer2]) <= line_slop
        && pt_to_line(cubic[mid2], cubic[outer1], cubic[outer2]) <= line_slop
}

fn quad_in_line(quad: &[Point; 3]) -> bool {
    let mut pt_max = -1.0_f32;
    let mut outer1 = 0usize;
    let mut outer2 = 1usize;
    for index in 0..2 {
        for inner in (index + 1)..3 {
            let test_diff = quad[inner] - quad[index];
            let test_max = test_diff.x.abs().max(test_diff.y.abs());
            if pt_max < test_max {
                outer1 = index;
                outer2 = inner;
                pt_max = test_max;
            }
        }
    }
    let mid = outer1 ^ outer2 ^ 3;
    const K_CURVATURE_SLOP: f32 = 0.000005_f32;
    let line_slop = pt_max * pt_max * K_CURVATURE_SLOP;
    pt_to_line(quad[mid], quad[outer1], quad[outer2]) <= line_slop
}

fn conic_in_line(conic: &Conic) -> bool {
    quad_in_line(&conic.pts)
}

fn check_cubic_linear(cubic: &[Point; 4], reduction: &mut [Point; 3]) -> (ReductionType, Point) {
    let degenerate_ab = degenerate_vector(cubic[1] - cubic[0]);
    let degenerate_bc = degenerate_vector(cubic[2] - cubic[1]);
    let degenerate_cd = degenerate_vector(cubic[3] - cubic[2]);
    if degenerate_ab && degenerate_bc && degenerate_cd {
        return (ReductionType::Point, Point::ZERO);
    }
    if usize::from(degenerate_ab) + usize::from(degenerate_bc) + usize::from(degenerate_cd) == 2 {
        return (ReductionType::Line, Point::ZERO);
    }
    if !cubic_in_line(cubic) {
        let tangent_pt = if degenerate_ab { cubic[2] } else { cubic[1] };
        return (ReductionType::Quad, tangent_pt);
    }
    let mut t_values = [0.0_f32; 3];
    let count = find_cubic_max_curvature(cubic, &mut t_values);
    let mut r_count = 0usize;
    for &t in &t_values[..count as usize] {
        if 0.0 >= t || t >= 1.0 {
            continue;
        }
        let mut pt = Point::ZERO;
        eval_cubic_at(cubic, t, Some(&mut pt), None, None);
        if pt != cubic[0] && pt != cubic[3] {
            reduction[r_count] = pt;
            r_count += 1;
        }
    }
    let rt = match r_count {
        0 => ReductionType::Line,
        1 => ReductionType::Degenerate,
        2 => ReductionType::Degenerate2,
        _ => ReductionType::Degenerate3,
    };
    (rt, Point::ZERO)
}

fn check_conic_linear(conic: &Conic, reduction: &mut Point) -> ReductionType {
    let degenerate_ab = degenerate_vector(conic.pts[1] - conic.pts[0]);
    let degenerate_bc = degenerate_vector(conic.pts[2] - conic.pts[1]);
    if degenerate_ab && degenerate_bc {
        return ReductionType::Point;
    }
    if degenerate_ab || degenerate_bc {
        return ReductionType::Line;
    }
    if !conic_in_line(conic) {
        return ReductionType::Quad;
    }
    let t = find_quad_max_curvature(&conic.pts);
    if t == 0.0 || t.is_nan() {
        return ReductionType::Line;
    }
    *reduction = conic.eval_at(t);
    ReductionType::Degenerate
}

fn check_quad_linear(quad: &[Point; 3], reduction: &mut Point) -> ReductionType {
    let degenerate_ab = degenerate_vector(quad[1] - quad[0]);
    let degenerate_bc = degenerate_vector(quad[2] - quad[1]);
    if degenerate_ab && degenerate_bc {
        return ReductionType::Point;
    }
    if degenerate_ab || degenerate_bc {
        return ReductionType::Line;
    }
    if !quad_in_line(quad) {
        return ReductionType::Quad;
    }
    let t = find_quad_max_curvature(quad);
    if t == 0.0 || t == 1.0 {
        return ReductionType::Line;
    }
    *reduction = eval_quad_at(quad, t);
    ReductionType::Degenerate
}

fn intersect_quad_ray(line: &[Point; 2], quad: &[Point; 3], roots: &mut [f32; 2]) -> i32 {
    let vec = line[1] - line[0];
    let r0 = vec.cross(quad[0] - line[0]);
    let r1 = vec.cross(quad[1] - line[0]);
    let r2 = vec.cross(quad[2] - line[0]);
    let a = r2 + r0 - 2.0 * r1;
    let b = r1 - r0;
    let c = r0;
    find_unit_quad_roots(a, 2.0 * b, c, roots)
}

#[inline]
fn points_within_dist(near_pt: Point, far_pt: Point, limit: f32) -> bool {
    Point::distance_to_sqd(near_pt, far_pt) <= limit * limit
}

fn sharp_angle(quad: &[Point; 3]) -> bool {
    let mut smaller = quad[1] - quad[0];
    let mut larger = quad[1] - quad[2];
    let smaller_len = smaller.length_sqd();
    let mut larger_len = larger.length_sqd();
    if smaller_len > larger_len {
        core::mem::swap(&mut smaller, &mut larger);
        larger_len = smaller_len;
    }
    if !smaller.set_length_in_place(larger_len) {
        return false;
    }
    smaller.dot(larger) > 0.0
}

fn has_valid_tangent(iter: &LegacyPathIter<'_>) -> bool {
    let mut copy = iter.clone();
    while let Some(rec) = copy.next_rec() {
        let pts = &rec.points;
        match rec.verb {
            Verb::Move => return false,
            Verb::Line => {
                if pts[0] == pts[1] {
                    continue;
                }
                return true;
            }
            Verb::Quad | Verb::Conic => {
                if pts[0] == pts[1] && pts[0] == pts[2] {
                    continue;
                }
                return true;
            }
            Verb::Cubic => {
                if pts[0] == pts[1] && pts[0] == pts[2] && pts[0] == pts[3] {
                    continue;
                }
                return true;
            }
            Verb::Close => return false,
        }
    }
    false
}

struct PathStroker {
    radius: f32,
    inv_miter_limit: f32,
    res_scale: f32,
    inv_res_scale: f32,
    inv_res_scale_squared: f32,

    first_normal: Vector,
    prev_normal: Vector,
    first_unit_normal: Vector,
    prev_unit_normal: Vector,
    first_pt: Point,
    prev_pt: Point,
    first_outer_pt: Point,
    first_outer_pt_index_in_contour: usize,
    segment_count: i32,
    prev_is_line: bool,
    can_ignore_center: bool,

    cap: Cap,
    join: Join,

    inner: PathBuilder,
    outer: PathBuilder,
    cusper: PathBuilder,

    stroke_type: StrokeSide,
    recursion_depth: i32,
    found_tangents: bool,
    join_completed: bool,
}

impl PathStroker {
    fn new(
        src_pt_count: usize,
        radius: f32,
        miter_limit: f32,
        cap: Cap,
        mut join: Join,
        res_scale: f32,
        can_ignore_center: bool,
    ) -> Self {
        let mut inv_miter_limit = 0.0_f32;
        if join == Join::Miter {
            if miter_limit <= 1.0 {
                join = Join::Bevel;
            } else {
                inv_miter_limit = 1.0 / miter_limit;
            }
        }

        let mut outer = PathBuilder::new(FillType::Winding);
        outer.inc_reserve(src_pt_count * 3, 0, 0);
        outer.set_is_volatile(true);
        let mut inner = PathBuilder::new(FillType::Winding);
        inner.inc_reserve(src_pt_count, 0, 0);
        inner.set_is_volatile(true);

        let inv_res_scale = 1.0 / (res_scale * 4.0);
        let inv_res_scale_squared = inv_res_scale * inv_res_scale;

        Self {
            radius,
            inv_miter_limit,
            res_scale,
            inv_res_scale,
            inv_res_scale_squared,
            first_normal: Vector::ZERO,
            prev_normal: Vector::ZERO,
            first_unit_normal: Vector::ZERO,
            prev_unit_normal: Vector::ZERO,
            first_pt: Point::ZERO,
            prev_pt: Point::ZERO,
            first_outer_pt: Point::ZERO,
            first_outer_pt_index_in_contour: 0,
            segment_count: -1,
            prev_is_line: false,
            can_ignore_center,
            cap,
            join,
            inner,
            outer,
            cusper: PathBuilder::new(FillType::Winding),
            stroke_type: StrokeSide::Outer,
            recursion_depth: 0,
            found_tangents: false,
            join_completed: false,
        }
    }

    #[inline]
    fn has_only_move_to(&self) -> bool {
        self.segment_count == 0
    }

    #[inline]
    fn move_to_pt(&self) -> Point {
        self.first_pt
    }

    #[inline]
    fn is_current_contour_empty(&self) -> bool {
        is_zero_length_since_point(self.inner.points(), 0)
            && is_zero_length_since_point(self.outer.points(), self.first_outer_pt_index_in_contour)
    }

    fn pre_join_to(
        &mut self,
        curr_pt: Point,
        curr_is_line: bool,
    ) -> Option<(Vector, Vector)> {
        let (normal, unit_normal) =
            match set_normal_unitnormal_pts(self.prev_pt, curr_pt, self.res_scale, self.radius) {
                Some(pair) => pair,
                None => {
                    if self.cap == Cap::Butt {
                        return None;
                    }
                    (Vector::new(self.radius, 0.0), Vector::new(1.0, 0.0))
                }
            };

        if self.segment_count == 0 {
            self.first_normal = normal;
            self.first_unit_normal = unit_normal;
            self.first_outer_pt = self.prev_pt + normal;
            self.outer.move_to(self.first_outer_pt);
            self.inner.move_to(self.prev_pt - normal);
        } else {
            apply_join(
                self.join,
                &mut self.outer,
                &mut self.inner,
                self.prev_unit_normal,
                self.prev_pt,
                unit_normal,
                self.radius,
                self.inv_miter_limit,
                self.prev_is_line,
                curr_is_line,
            );
        }
        self.prev_is_line = curr_is_line;
        Some((normal, unit_normal))
    }

    fn post_join_to(&mut self, curr_pt: Point, normal: Vector, unit_normal: Vector) {
        self.join_completed = true;
        self.prev_pt = curr_pt;
        self.prev_unit_normal = unit_normal;
        self.prev_normal = normal;
        self.segment_count += 1;
    }

    fn finish_contour(&mut self, close: bool, curr_is_line: bool) {
        if self.segment_count > 0 {
            if close {
                apply_join(
                    self.join,
                    &mut self.outer,
                    &mut self.inner,
                    self.prev_unit_normal,
                    self.prev_pt,
                    self.first_unit_normal,
                    self.radius,
                    self.inv_miter_limit,
                    self.prev_is_line,
                    curr_is_line,
                );
                self.outer.close();

                if self.can_ignore_center {
                    if self.inner.compute_bounds().contains_rect(self.outer.compute_bounds()) {
                        self.outer = self.inner.clone();
                    }
                } else if let Some(pt) = self.inner.get_last_pt() {
                    self.outer.move_to(pt);
                    let inner_buf = core::mem::take(&mut self.inner).into_buf();
                    self.outer
                        .reverse_path_to(&inner_buf.points, &inner_buf.verbs, &inner_buf.conics);
                    self.outer.close();
                }
            } else if let Some(pt) = self.inner.get_last_pt() {
                apply_cap(
                    self.cap,
                    &mut self.outer,
                    self.prev_pt,
                    self.prev_normal,
                    pt,
                    curr_is_line,
                );
                let inner_buf = core::mem::take(&mut self.inner).into_buf();
                self.outer
                    .reverse_path_to(&inner_buf.points, &inner_buf.verbs, &inner_buf.conics);
                apply_cap(
                    self.cap,
                    &mut self.outer,
                    self.first_pt,
                    -self.first_normal,
                    self.first_outer_pt,
                    self.prev_is_line,
                );
                self.outer.close();
            }

            if !self.cusper.is_empty() {
                let cusper_buf = core::mem::take(&mut self.cusper).into_buf();
                let is_fin = cusper_buf.is_finite();
                self.outer.add_path(
                    &cusper_buf.points,
                    &cusper_buf.verbs,
                    &cusper_buf.conics,
                    cusper_buf.segment_mask,
                    is_fin,
                    (
                        cusper_buf.is_a_type,
                        cusper_buf.is_a_dir,
                        cusper_buf.is_a_start_index,
                    ),
                    cusper_buf.convexity,
                    cusper_buf.is_volatile,
                    &Matrix::identity(),
                    AddPathMode::Append,
                );
            }
        }
        self.inner.reset();
        self.segment_count = -1;
        self.first_outer_pt_index_in_contour = self.outer.count_points();
    }

    fn move_to(&mut self, pt: Point) {
        if self.segment_count > 0 {
            self.finish_contour(false, false);
        }
        self.segment_count = 0;
        self.first_pt = pt;
        self.prev_pt = pt;
        self.join_completed = false;
    }

    fn line_to_inner(&mut self, curr_pt: Point, normal: Vector) {
        self.outer.line_to(curr_pt + normal);
        self.inner.line_to(curr_pt - normal);
    }

    fn line_to(&mut self, curr_pt: Point, iter: Option<&LegacyPathIter<'_>>) {
        let teeny_line = equals_within_tolerance(
            self.prev_pt,
            curr_pt,
            SK_SCALAR_NEARLY_ZERO * self.inv_res_scale,
        );
        if self.cap == Cap::Butt && teeny_line {
            return;
        }
        if teeny_line && (self.join_completed || iter.is_some_and(has_valid_tangent)) {
            return;
        }
        let Some((normal, unit_normal)) = self.pre_join_to(curr_pt, true) else {
            return;
        };
        self.line_to_inner(curr_pt, normal);
        self.post_join_to(curr_pt, normal, unit_normal);
    }

    fn set_quad_end_normal(
        &self,
        quad: &[Point; 3],
        normal_ab: Vector,
        unit_normal_ab: Vector,
    ) -> (Vector, Vector) {
        set_normal_unitnormal_pts(quad[1], quad[2], self.res_scale, self.radius)
            .unwrap_or((normal_ab, unit_normal_ab))
    }

    fn set_cubic_end_normal(
        &self,
        cubic: &[Point; 4],
        normal_ab: Vector,
        unit_normal_ab: Vector,
    ) -> (Vector, Vector) {
        let mut ab = cubic[1] - cubic[0];
        let mut cd = cubic[3] - cubic[2];
        let mut degenerate_ab = degenerate_vector(ab);
        let mut degenerate_cd = degenerate_vector(cd);

        if degenerate_ab && degenerate_cd {
            return (normal_ab, unit_normal_ab);
        }
        if degenerate_ab {
            ab = cubic[2] - cubic[0];
            degenerate_ab = degenerate_vector(ab);
        }
        if degenerate_cd {
            cd = cubic[3] - cubic[1];
            degenerate_cd = degenerate_vector(cd);
        }
        if degenerate_ab || degenerate_cd {
            return (normal_ab, unit_normal_ab);
        }
        set_normal_unitnormal_vec(cd, self.radius).unwrap_or((normal_ab, unit_normal_ab))
    }

    fn init_stroke_pass(
        &mut self,
        stroke_type: StrokeSide,
        quad_pts: &mut QuadConstruct,
        t_start: f32,
        t_end: f32,
    ) {
        self.stroke_type = stroke_type;
        self.found_tangents = false;
        self.recursion_depth = 0;
        quad_pts.init(t_start, t_end);
    }

    fn set_ray_pts(&self, t_pt: Point, mut dxy: Vector) -> (Point, Vector) {
        if !dxy.set_length_in_place(self.radius) {
            dxy = Vector::new(self.radius, 0.0);
        }
        let axis_flip = (self.stroke_type as i32) as f32;
        let on_pt = Point::new(t_pt.x + axis_flip * dxy.y, t_pt.y - axis_flip * dxy.x);
        (on_pt, dxy)
    }

    fn conic_perp_ray(&self, conic: &Conic, t: f32) -> (Point, Point, Vector) {
        let t_pt = conic.eval_at(t);
        let mut dxy = conic.eval_tangent_at(t);
        if dxy.x == 0.0 && dxy.y == 0.0 {
            dxy = conic.pts[2] - conic.pts[0];
        }
        let (on_pt, tangent) = self.set_ray_pts(t_pt, dxy);
        (t_pt, on_pt, tangent)
    }

    fn conic_quad_ends(&self, conic: &Conic, quad_pts: &mut QuadConstruct) {
        if !quad_pts.start_set {
            let (_, on_pt, tangent) = self.conic_perp_ray(conic, quad_pts.start_t);
            quad_pts.quad[0] = on_pt;
            quad_pts.tangent_start = tangent;
            quad_pts.start_set = true;
        }
        if !quad_pts.end_set {
            let (_, on_pt, tangent) = self.conic_perp_ray(conic, quad_pts.end_t);
            quad_pts.quad[2] = on_pt;
            quad_pts.tangent_end = tangent;
            quad_pts.end_set = true;
        }
    }

    fn cubic_perp_ray(&self, cubic: &[Point; 4], t: f32) -> (Point, Point, Vector) {
        let mut t_pt = Point::ZERO;
        let mut dxy = Vector::ZERO;
        eval_cubic_at(cubic, t, Some(&mut t_pt), Some(&mut dxy), None);
        if dxy.x == 0.0 && dxy.y == 0.0 {
            let mut chopped = [Point::ZERO; 7];
            let mut use_chopped = false;
            if scalar_nearly_zero(t, SK_SCALAR_NEARLY_ZERO) {
                dxy = cubic[2] - cubic[0];
            } else if scalar_nearly_zero(1.0 - t, SK_SCALAR_NEARLY_ZERO) {
                dxy = cubic[3] - cubic[1];
            } else {
                chop_cubic_at_1(cubic, &mut chopped, t);
                dxy = chopped[3] - chopped[2];
                if dxy.x == 0.0 && dxy.y == 0.0 {
                    dxy = chopped[3] - chopped[1];
                    use_chopped = true;
                }
            }
            if dxy.x == 0.0 && dxy.y == 0.0 {
                dxy = if use_chopped {
                    chopped[3] - chopped[0]
                } else {
                    cubic[3] - cubic[0]
                };
            }
        }
        let (on_pt, tangent) = self.set_ray_pts(t_pt, dxy);
        (t_pt, on_pt, tangent)
    }

    fn cubic_quad_ends(&self, cubic: &[Point; 4], quad_pts: &mut QuadConstruct) {
        if !quad_pts.start_set {
            let (_, on_pt, tangent) = self.cubic_perp_ray(cubic, quad_pts.start_t);
            quad_pts.quad[0] = on_pt;
            quad_pts.tangent_start = tangent;
            quad_pts.start_set = true;
        }
        if !quad_pts.end_set {
            let (_, on_pt, tangent) = self.cubic_perp_ray(cubic, quad_pts.end_t);
            quad_pts.quad[2] = on_pt;
            quad_pts.tangent_end = tangent;
            quad_pts.end_set = true;
        }
    }

    fn quad_perp_ray(&self, quad: &[Point; 3], t: f32) -> (Point, Point, Vector) {
        let t_pt = eval_quad_at(quad, t);
        let mut dxy = eval_quad_tangent_at(quad, t);
        if dxy.x == 0.0 && dxy.y == 0.0 {
            dxy = quad[2] - quad[0];
        }
        let (on_pt, tangent) = self.set_ray_pts(t_pt, dxy);
        (t_pt, on_pt, tangent)
    }

    fn intersect_ray(
        &self,
        quad_pts: &mut QuadConstruct,
        intersect_ray_type: IntersectRayType,
    ) -> ResultType {
        let start = quad_pts.quad[0];
        let end = quad_pts.quad[2];
        let a_len = quad_pts.tangent_start;
        let b_len = quad_pts.tangent_end;
        let denom = a_len.cross(b_len);
        if denom == 0.0 || !denom.is_finite() {
            quad_pts.opposite_tangents = a_len.dot(b_len) < 0.0;
            return ResultType::Degenerate;
        }
        quad_pts.opposite_tangents = false;
        let ab0 = start - end;
        let mut numer_a = b_len.cross(ab0);
        let numer_b = a_len.cross(ab0);
        if (numer_a >= 0.0) == (numer_b >= 0.0) {
            let dist1 = pt_to_tangent_line(start, end, quad_pts.tangent_end);
            let dist2 = pt_to_tangent_line(end, start, quad_pts.tangent_start);
            if dist1.max(dist2) <= self.inv_res_scale_squared {
                return ResultType::Degenerate;
            }
            return ResultType::Split;
        }
        numer_a /= denom;
        let valid_divide = numer_a > numer_a - 1.0;
        if valid_divide {
            if intersect_ray_type == IntersectRayType::CtrlPt {
                quad_pts.quad[1] = start + quad_pts.tangent_start * numer_a;
            }
            return ResultType::Quad;
        }
        quad_pts.opposite_tangents = a_len.dot(b_len) < 0.0;
        ResultType::Degenerate
    }

    fn tangents_meet(&self, cubic: &[Point; 4], quad_pts: &mut QuadConstruct) -> ResultType {
        self.cubic_quad_ends(cubic, quad_pts);
        self.intersect_ray(quad_pts, IntersectRayType::ResultType)
    }

    fn pt_in_quad_bounds(&self, quad: &[Point; 3], pt: Point) -> bool {
        let x_min = quad[0].x.min(quad[1].x).min(quad[2].x);
        if pt.x + self.inv_res_scale < x_min {
            return false;
        }
        let x_max = quad[0].x.max(quad[1].x).max(quad[2].x);
        if pt.x - self.inv_res_scale > x_max {
            return false;
        }
        let y_min = quad[0].y.min(quad[1].y).min(quad[2].y);
        if pt.y + self.inv_res_scale < y_min {
            return false;
        }
        let y_max = quad[0].y.max(quad[1].y).max(quad[2].y);
        if pt.y - self.inv_res_scale > y_max {
            return false;
        }
        true
    }

    fn stroke_close_enough(
        &self,
        stroke: &[Point; 3],
        ray: &[Point; 2],
        quad_pts: &QuadConstruct,
    ) -> ResultType {
        let stroke_mid = eval_quad_at(stroke, 0.5_f32);
        if points_within_dist(ray[0], stroke_mid, self.inv_res_scale) {
            if sharp_angle(&quad_pts.quad) {
                return ResultType::Split;
            }
            return ResultType::Quad;
        }
        if !self.pt_in_quad_bounds(stroke, ray[0]) {
            return ResultType::Split;
        }
        let mut roots = [0.0_f32; 2];
        let root_count = intersect_quad_ray(ray, stroke, &mut roots);
        if root_count != 1 {
            return ResultType::Split;
        }
        let quad_pt = eval_quad_at(stroke, roots[0]);
        let error = self.inv_res_scale * (1.0 - (roots[0] - 0.5_f32).abs() * 2.0);
        if points_within_dist(ray[0], quad_pt, error) {
            if sharp_angle(&quad_pts.quad) {
                return ResultType::Split;
            }
            return ResultType::Quad;
        }
        ResultType::Split
    }

    fn compare_quad_cubic(&self, cubic: &[Point; 4], quad_pts: &mut QuadConstruct) -> ResultType {
        self.cubic_quad_ends(cubic, quad_pts);
        let result_type = self.intersect_ray(quad_pts, IntersectRayType::CtrlPt);
        if result_type != ResultType::Quad {
            return result_type;
        }
        let (t_pt, on_pt, _) = self.cubic_perp_ray(cubic, quad_pts.mid_t);
        let ray = [on_pt, t_pt];
        self.stroke_close_enough(&quad_pts.quad, &ray, quad_pts)
    }

    fn compare_quad_conic(&self, conic: &Conic, quad_pts: &mut QuadConstruct) -> ResultType {
        self.conic_quad_ends(conic, quad_pts);
        let result_type = self.intersect_ray(quad_pts, IntersectRayType::CtrlPt);
        if result_type != ResultType::Quad {
            return result_type;
        }
        let (t_pt, on_pt, _) = self.conic_perp_ray(conic, quad_pts.mid_t);
        let ray = [on_pt, t_pt];
        self.stroke_close_enough(&quad_pts.quad, &ray, quad_pts)
    }

    fn compare_quad_quad(&self, quad: &[Point; 3], quad_pts: &mut QuadConstruct) -> ResultType {
        if !quad_pts.start_set {
            let (_, on_pt, tangent) = self.quad_perp_ray(quad, quad_pts.start_t);
            quad_pts.quad[0] = on_pt;
            quad_pts.tangent_start = tangent;
            quad_pts.start_set = true;
        }
        if !quad_pts.end_set {
            let (_, on_pt, tangent) = self.quad_perp_ray(quad, quad_pts.end_t);
            quad_pts.quad[2] = on_pt;
            quad_pts.tangent_end = tangent;
            quad_pts.end_set = true;
        }
        let result_type = self.intersect_ray(quad_pts, IntersectRayType::CtrlPt);
        if result_type != ResultType::Quad {
            return result_type;
        }
        let (t_pt, on_pt, _) = self.quad_perp_ray(quad, quad_pts.mid_t);
        let ray = [on_pt, t_pt];
        self.stroke_close_enough(&quad_pts.quad, &ray, quad_pts)
    }

    fn add_degenerate_line(&mut self, quad_pts: &QuadConstruct) {
        let sink = match self.stroke_type {
            StrokeSide::Outer => &mut self.outer,
            StrokeSide::Inner => &mut self.inner,
        };
        sink.line_to(quad_pts.quad[2]);
    }

    fn cubic_mid_on_line(&self, cubic: &[Point; 4], quad_pts: &QuadConstruct) -> bool {
        let (_, stroke_mid, _) = self.cubic_perp_ray(cubic, quad_pts.mid_t);
        let dist = pt_to_line(stroke_mid, quad_pts.quad[0], quad_pts.quad[2]);
        dist < self.inv_res_scale_squared
    }

    fn cubic_stroke(&mut self, cubic: &[Point; 4], quad_pts: &mut QuadConstruct) -> bool {
        if !self.found_tangents {
            let result_type = self.tangents_meet(cubic, quad_pts);
            if result_type != ResultType::Quad {
                if (result_type == ResultType::Degenerate
                    || points_within_dist(quad_pts.quad[0], quad_pts.quad[2], self.inv_res_scale))
                    && self.cubic_mid_on_line(cubic, quad_pts)
                {
                    self.add_degenerate_line(quad_pts);
                    return true;
                }
            } else {
                self.found_tangents = true;
            }
        }
        if self.found_tangents {
            let result_type = self.compare_quad_cubic(cubic, quad_pts);
            if result_type == ResultType::Quad {
                let sink = match self.stroke_type {
                    StrokeSide::Outer => &mut self.outer,
                    StrokeSide::Inner => &mut self.inner,
                };
                sink.quad_to(quad_pts.quad[1], quad_pts.quad[2]);
                return true;
            }
            if result_type == ResultType::Degenerate && !quad_pts.opposite_tangents {
                self.add_degenerate_line(quad_pts);
                return true;
            }
        }
        if !quad_pts.quad[2].is_finite() {
            return false;
        }
        let limit_idx = if self.found_tangents {
            K_CUBIC_RECURSIVE_LIMIT
        } else {
            K_TANGENT_RECURSIVE_LIMIT
        };
        self.recursion_depth += 1;
        if self.recursion_depth > K_RECURSIVE_LIMITS[limit_idx] {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        let mut half = QuadConstruct::default();
        if !half.init_with_start(quad_pts) {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        if !self.cubic_stroke(cubic, &mut half) {
            return false;
        }
        if !half.init_with_end(quad_pts) {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        if !self.cubic_stroke(cubic, &mut half) {
            return false;
        }
        self.recursion_depth -= 1;
        true
    }

    fn conic_stroke(&mut self, conic: &Conic, quad_pts: &mut QuadConstruct) -> bool {
        let result_type = self.compare_quad_conic(conic, quad_pts);
        if result_type == ResultType::Quad {
            let sink = match self.stroke_type {
                StrokeSide::Outer => &mut self.outer,
                StrokeSide::Inner => &mut self.inner,
            };
            sink.quad_to(quad_pts.quad[1], quad_pts.quad[2]);
            return true;
        }
        if result_type == ResultType::Degenerate {
            self.add_degenerate_line(quad_pts);
            return true;
        }
        if !quad_pts.quad[2].is_finite() {
            return false;
        }
        self.recursion_depth += 1;
        if self.recursion_depth > K_RECURSIVE_LIMITS[K_CONIC_RECURSIVE_LIMIT] {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        let mut half = QuadConstruct::default();
        if !half.init_with_start(quad_pts) {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        if !self.conic_stroke(conic, &mut half) {
            return false;
        }
        if !half.init_with_end(quad_pts) {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        if !self.conic_stroke(conic, &mut half) {
            return false;
        }
        self.recursion_depth -= 1;
        true
    }

    fn quad_stroke(&mut self, quad: &[Point; 3], quad_pts: &mut QuadConstruct) -> bool {
        let result_type = self.compare_quad_quad(quad, quad_pts);
        if result_type == ResultType::Quad {
            let sink = match self.stroke_type {
                StrokeSide::Outer => &mut self.outer,
                StrokeSide::Inner => &mut self.inner,
            };
            sink.quad_to(quad_pts.quad[1], quad_pts.quad[2]);
            return true;
        }
        if result_type == ResultType::Degenerate {
            self.add_degenerate_line(quad_pts);
            return true;
        }
        if !quad_pts.quad[2].is_finite() {
            return false;
        }
        self.recursion_depth += 1;
        if self.recursion_depth > K_RECURSIVE_LIMITS[K_QUAD_RECURSIVE_LIMIT] {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        let mut half = QuadConstruct::default();
        if !half.init_with_start(quad_pts) {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        if !self.quad_stroke(quad, &mut half) {
            return false;
        }
        if !half.init_with_end(quad_pts) {
            self.add_degenerate_line(quad_pts);
            self.recursion_depth -= 1;
            return true;
        }
        if !self.quad_stroke(quad, &mut half) {
            return false;
        }
        self.recursion_depth -= 1;
        true
    }

    fn conic_to(&mut self, pt1: Point, pt2: Point, weight: f32) {
        let conic = Conic {
            pts: [self.prev_pt, pt1, pt2],
            w: weight,
        };
        let mut reduction = Point::ZERO;
        let reduction_type = check_conic_linear(&conic, &mut reduction);
        if reduction_type == ReductionType::Point || reduction_type == ReductionType::Line {
            self.line_to(pt2, None);
            return;
        }
        if reduction_type == ReductionType::Degenerate {
            self.line_to(reduction, None);
            let save_join = self.join;
            self.join = Join::Round;
            self.line_to(pt2, None);
            self.join = save_join;
            return;
        }
        let Some((normal_ab, unit_ab)) = self.pre_join_to(pt1, false) else {
            self.line_to(pt2, None);
            return;
        };
        let mut quad_pts = QuadConstruct::default();
        self.init_stroke_pass(StrokeSide::Outer, &mut quad_pts, 0.0, 1.0);
        let _ = self.conic_stroke(&conic, &mut quad_pts);
        self.init_stroke_pass(StrokeSide::Inner, &mut quad_pts, 0.0, 1.0);
        let _ = self.conic_stroke(&conic, &mut quad_pts);
        let (normal_bc, unit_bc) = self.set_quad_end_normal(&conic.pts, normal_ab, unit_ab);
        self.post_join_to(pt2, normal_bc, unit_bc);
    }

    fn quad_to(&mut self, pt1: Point, pt2: Point) {
        let quad = [self.prev_pt, pt1, pt2];
        let mut reduction = Point::ZERO;
        let reduction_type = check_quad_linear(&quad, &mut reduction);
        if reduction_type == ReductionType::Point || reduction_type == ReductionType::Line {
            self.line_to(pt2, None);
            return;
        }
        if reduction_type == ReductionType::Degenerate {
            self.line_to(reduction, None);
            let save_join = self.join;
            self.join = Join::Round;
            self.line_to(pt2, None);
            self.join = save_join;
            return;
        }
        let Some((normal_ab, unit_ab)) = self.pre_join_to(pt1, false) else {
            self.line_to(pt2, None);
            return;
        };
        let mut quad_pts = QuadConstruct::default();
        self.init_stroke_pass(StrokeSide::Outer, &mut quad_pts, 0.0, 1.0);
        let _ = self.quad_stroke(&quad, &mut quad_pts);
        self.init_stroke_pass(StrokeSide::Inner, &mut quad_pts, 0.0, 1.0);
        let _ = self.quad_stroke(&quad, &mut quad_pts);
        let (normal_bc, unit_bc) = self.set_quad_end_normal(&quad, normal_ab, unit_ab);
        self.post_join_to(pt2, normal_bc, unit_bc);
    }

    fn cubic_to(&mut self, pt1: Point, pt2: Point, pt3: Point) {
        let cubic = [self.prev_pt, pt1, pt2, pt3];
        let mut reduction = [Point::ZERO; 3];
        let (reduction_type, tangent_pt) = check_cubic_linear(&cubic, &mut reduction);
        if reduction_type == ReductionType::Point || reduction_type == ReductionType::Line {
            self.line_to(pt3, None);
            return;
        }
        if matches!(
            reduction_type,
            ReductionType::Degenerate | ReductionType::Degenerate2 | ReductionType::Degenerate3
        ) {
            self.line_to(reduction[0], None);
            let save_join = self.join;
            self.join = Join::Round;
            if matches!(
                reduction_type,
                ReductionType::Degenerate2 | ReductionType::Degenerate3
            ) {
                self.line_to(reduction[1], None);
            }
            if reduction_type == ReductionType::Degenerate3 {
                self.line_to(reduction[2], None);
            }
            self.line_to(pt3, None);
            self.join = save_join;
            return;
        }
        let Some((normal_ab, unit_ab)) = self.pre_join_to(tangent_pt, false) else {
            self.line_to(pt3, None);
            return;
        };
        let mut t_values = [0.0_f32; 2];
        let count = find_cubic_inflections(&cubic, &mut t_values);
        let mut last_t = 0.0_f32;
        for index in 0..=(count as usize) {
            let next_t = if index < count as usize {
                t_values[index]
            } else {
                1.0
            };
            let mut quad_pts = QuadConstruct::default();
            self.init_stroke_pass(StrokeSide::Outer, &mut quad_pts, last_t, next_t);
            let _ = self.cubic_stroke(&cubic, &mut quad_pts);
            self.init_stroke_pass(StrokeSide::Inner, &mut quad_pts, last_t, next_t);
            let _ = self.cubic_stroke(&cubic, &mut quad_pts);
            last_t = next_t;
        }
        let cusp = find_cubic_cusp(&cubic);
        if cusp > 0.0 {
            let mut cusp_loc = Point::ZERO;
            eval_cubic_at(&cubic, cusp, Some(&mut cusp_loc), None, None);
            self.cusper.add_circle(cusp_loc, self.radius, Direction::Cw);
        }
        let (normal_cd, unit_cd) = self.set_cubic_end_normal(&cubic, normal_ab, unit_ab);
        self.post_join_to(pt3, normal_cd, unit_cd);
    }

    fn close(&mut self, is_line: bool) {
        self.finish_contour(true, is_line);
    }

    fn done(&mut self, dst: &mut PathBuilder, is_line: bool) {
        self.finish_contour(false, is_line);
        *dst = core::mem::take(&mut self.outer);
    }
}

/// Public stroker matching `SkStroke`.
#[derive(Copy, Clone, Debug)]
pub struct Stroke {
    pub width: f32,
    pub miter_limit: f32,
    pub res_scale: f32,
    pub cap: Cap,
    pub join: Join,
    pub do_fill: bool,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            width: 1.0,
            miter_limit: SK_PAINT_DEFAULTS_MITER_LIMIT,
            res_scale: 1.0,
            cap: Cap::Butt,
            join: Join::Miter,
            do_fill: false,
        }
    }
}

fn add_bevel(path: &mut PathBuilder, r: Rect, outer: Rect, dir: Direction) {
    let pts = if dir == Direction::Cw {
        [
            Point::new(r.left, outer.top),
            Point::new(r.right, outer.top),
            Point::new(outer.right, r.top),
            Point::new(outer.right, r.bottom),
            Point::new(r.right, outer.bottom),
            Point::new(r.left, outer.bottom),
            Point::new(outer.left, r.bottom),
            Point::new(outer.left, r.top),
        ]
    } else {
        [
            Point::new(outer.left, r.top),
            Point::new(outer.left, r.bottom),
            Point::new(r.left, outer.bottom),
            Point::new(r.right, outer.bottom),
            Point::new(outer.right, r.bottom),
            Point::new(outer.right, r.top),
            Point::new(r.right, outer.top),
            Point::new(r.left, outer.top),
        ]
    };
    path.add_polygon(&pts, true);
}

impl Stroke {
    pub fn stroke_rect(&self, orig_rect: Rect, dst: &mut PathBuilder, mut dir: Direction) {
        dst.reset();

        let radius = self.width / 2.0;
        if radius <= 0.0 {
            return;
        }

        let mut rw = orig_rect.width();
        let mut rh = orig_rect.height();
        if (rw < 0.0) ^ (rh < 0.0) {
            dir = dir.reverse();
        }
        let rect = orig_rect.make_sorted();
        rw = rect.width();
        rh = rect.height();

        let r = rect.make_inset(-radius, -radius);

        let mut join = self.join;
        if join == Join::Miter && self.miter_limit < SK_SCALAR_SQRT_2 {
            join = Join::Bevel;
        }

        match join {
            Join::Miter => {
                dst.add_rect(r, dir, 0);
            }
            Join::Bevel => {
                add_bevel(dst, rect, r, dir);
            }
            Join::Round => {
                let mut rr = RRect::make_empty();
                rr.set_rect_xy(r, radius, radius);
                let start = if dir == Direction::Cw { 6 } else { 7 };
                dst.add_rrect(&rr, dir, start);
            }
        }

        if self.width < rw.min(rh) && !self.do_fill {
            let inner_r = rect.make_inset(radius, radius);
            dst.add_rect(inner_r, dir.reverse(), 0);
        }
    }

    pub fn stroke_path(&self, src: &PathRaw<'_>, dst: &mut PathBuilder) {
        let radius = self.width / 2.0;
        if radius <= 0.0 {
            return;
        }

        // Check if src is a closed rect.
        if let Some(info) = is_rect_contour(src.points, src.verbs, src.segment_mask as u32, false) {
            if info.is_closed {
                self.stroke_rect(info.rect, dst, info.direction);
                if src.fill_type.is_inverse() {
                    dst.toggle_inverse_fill_type();
                }
                return;
            }
        }

        let is_last_contour_closed = src.verbs.last() == Some(&Verb::Close);
        let is_convex = if src.convexity == Convexity::Unknown {
            compute_convexity(src.points, src.verbs, src.conics).is_convex()
        } else {
            src.convexity.is_convex()
        };
        let ignore_center = self.do_fill
            && src.segment_mask == SEG_LINE
            && is_last_contour_closed
            && is_convex;

        let mut stroker = PathStroker::new(
            src.points.len(),
            radius,
            self.miter_limit,
            self.cap,
            self.join,
            self.res_scale,
            ignore_center,
        );

        let mut iter = LegacyPathIter::new(src.points, src.verbs, src.conics, false);
        let mut last_segment = Verb::Move;
        while let Some(rec) = iter.next_rec() {
            let pts = &rec.points;
            match rec.verb {
                Verb::Move => {
                    stroker.move_to(pts[0]);
                }
                Verb::Line => {
                    stroker.line_to(pts[1], Some(&iter));
                    last_segment = Verb::Line;
                }
                Verb::Quad => {
                    stroker.quad_to(pts[1], pts[2]);
                    last_segment = Verb::Quad;
                }
                Verb::Conic => {
                    stroker.conic_to(pts[1], pts[2], rec.conic_weight);
                    last_segment = Verb::Conic;
                }
                Verb::Cubic => {
                    stroker.cubic_to(pts[1], pts[2], pts[3]);
                    last_segment = Verb::Cubic;
                }
                Verb::Close => {
                    if self.cap != Cap::Butt {
                        if stroker.has_only_move_to() {
                            let pt = stroker.move_to_pt();
                            stroker.line_to(pt, None);
                            last_segment = Verb::Line;
                            continue;
                        }
                        if stroker.is_current_contour_empty() {
                            last_segment = Verb::Line;
                            continue;
                        }
                    }
                    stroker.close(last_segment == Verb::Line);
                }
            }
        }
        stroker.done(dst, last_segment == Verb::Line);

        if self.do_fill && !ignore_center {
            let d = compute_first_direction(src);
            if d == FirstDirection::Ccw {
                dst.reverse_add_path(src.points, src.verbs, src.conics);
            } else {
                dst.add_path(
                    src.points,
                    src.verbs,
                    src.conics,
                    src.segment_mask,
                    src.bounds.is_finite(),
                    (PathIsAType::General, Direction::Cw, 0),
                    src.convexity,
                    false,
                    &Matrix::identity(),
                    AddPathMode::Append,
                );
            }
        }

        if src.fill_type.is_inverse() {
            dst.toggle_inverse_fill_type();
        }
    }
}
