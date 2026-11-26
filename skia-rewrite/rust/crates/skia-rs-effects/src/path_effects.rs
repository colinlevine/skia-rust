//! Safe Rust implementations for Skia Path Effects:
//! - `Sk1DPathEffect` (phase normalization, point morphing)
//! - `Sk2DPathEffect` (line/path 2D span mapping)
//! - `SkCornerPathEffect` (`ComputeStep`, full corner-rounding verb state machine)
//! - `SkDashPathEffect` (`outset_for_stroke`, `cull_line`, `onAsPoints`)
//! - `SkDiscretePathEffect` (`LCGRandom`, perturbation, contour subdivision planning, fast bounds)
//! - `SkTrimPathEffect` (parameter validation/pinning, span planning)

use crate::{matrix_map_point, matrix_map_rect};
use skia_rs_geometry::{
    float_midpoint, pin_f32, scalar_nearly_equal, Matrix, Point, Rect, Vector,
    SK_SCALAR_NEARLY_ZERO,
};

/// Maximum iterations governor used in `Sk1DPathEffect` and `SkDiscretePathEffect`.
pub const MAX_REASONABLE_ITERATIONS: i32 = 100_000;

/// Maximum dash count for `SkDashImpl::onAsPoints` (matches `SkDashPath::kMaxDashCount`).
pub const MAX_DASH_COUNT: f32 = 1_000_000.0;

/// Matches `sk_float_saturate2int` / `sk_float_floor2int`.
#[inline]
pub(crate) fn float_floor_to_int(x: f32) -> i32 {
    let f = x.floor();
    if f >= (i32::MAX as f32) {
        i32::MAX
    } else if f <= (i32::MIN as f32) {
        i32::MIN
    } else if f.is_nan() {
        i32::MAX
    } else {
        f as i32
    }
}

/// Matches `sk_float_round2int(x) = sk_float_floor2int(x + 0.5f)`.
#[inline]
pub(crate) fn float_round_to_int(x: f32) -> i32 {
    float_floor_to_int(x + 0.5)
}

/// Matches `sk_float_ceil2int(x)`.
#[inline]
pub(crate) fn float_ceil_to_int(x: f32) -> i32 {
    let c = x.ceil();
    if c >= (i32::MAX as f32) {
        i32::MAX
    } else if c <= (i32::MIN as f32) {
        i32::MIN
    } else if c.is_nan() {
        i32::MAX
    } else {
        c as i32
    }
}

// ============================================================================
// Sk1DPathEffect
// ============================================================================

/// Validates `SkPath1DPathEffect::Make` arguments.
#[inline]
pub fn path_1d_is_valid(advance: f32, phase: f32, path_is_empty: bool) -> bool {
    advance > 0.0 && advance.is_finite() && phase.is_finite() && !path_is_empty
}

/// Computes `fInitialOffset` from `advance` and `phase` matching
/// `SkPath1DPathEffectImpl::SkPath1DPathEffectImpl`.
pub fn path_1d_initial_offset(advance: f32, mut phase: f32) -> f32 {
    if phase < 0.0 {
        phase = -phase;
        if phase > advance {
            phase %= advance;
        }
    } else {
        if phase > advance {
            phase %= advance;
        }
        phase = advance - phase;
    }
    if phase >= advance {
        phase = 0.0;
    }
    phase
}

/// Morphs a single point `(sx, sy)` given path position `pos` and unit `tangent`
/// at `dist + sx`, matching `morphpoints` in `Sk1DPathEffect.cpp`.
#[inline]
pub fn path_1d_morph_point(sx: f32, sy: f32, pos: Point, tangent: Vector) -> Point {
    let mut matrix = Matrix::identity();
    matrix.set_sin_cos(tangent.y, tangent.x);
    matrix.pre_translate(-sx, 0.0);
    matrix.post_translate(pos.x, pos.y);
    matrix_map_point(&matrix, Point::new(sx, sy))
}

/// Computes the quadratic control midpoint used when morphing a line segment in `morphpath`.
#[inline]
pub fn path_1d_line_midpoint(p0: Point, p1: Point) -> Point {
    Point::new(float_midpoint(p0.x, p1.x), float_midpoint(p0.y, p1.y))
}

// ============================================================================
// Sk2DPathEffect
// ============================================================================

/// Validates `SkLine2DPathEffect::Make` width (`width >= 0`, false for NaN).
#[inline]
pub fn line_2d_is_valid(width: f32) -> bool {
    width >= 0.0
}

/// Computes the two transformed line endpoints for `SkLine2DPathEffectImpl::nextSpan`.
/// Returns `None` if `ucount <= 1`.
#[inline]
pub fn line_2d_span_endpoints(u: i32, v: i32, ucount: i32, mat: &Matrix) -> Option<[Point; 2]> {
    if ucount <= 1 {
        return None;
    }
    let s0 = Point::new((u as f32) + 0.5, (v as f32) + 0.5);
    let s1 = Point::new(((u + ucount) as f32) + 0.5, (v as f32) + 0.5);
    Some([matrix_map_point(mat, s0), matrix_map_point(mat, s1)])
}

/// Maps a single grid cell `(x, y)` center `(x + 0.5, y + 0.5)` through `mat`
/// for `Sk2DPathEffect::nextSpan`.
#[inline]
pub fn path_2d_cell_location(x: i32, y: i32, mat: &Matrix) -> Point {
    matrix_map_point(mat, Point::new((x as f32) + 0.5, (y as f32) + 0.5))
}

// ============================================================================
// SkCornerPathEffect
// ============================================================================

/// Validates `SkCornerPathEffect::Make` radius.
#[inline]
pub fn corner_is_valid(radius: f32) -> bool {
    radius.is_finite() && radius > 0.0
}

/// Exact port of `ComputeStep` in `SkCornerPathEffect.cpp`.
/// Returns `(step, draw_segment)`.
#[inline]
pub fn corner_compute_step(a: Point, b: Point, radius: f32) -> (Point, bool) {
    let dist = Point::length(a.x - b.x, a.y - b.y);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    if dist <= radius * 2.0 {
        (Point::new(dx * 0.5, dy * 0.5), false)
    } else {
        let scale = radius / dist;
        (Point::new(dx * scale, dy * scale), true)
    }
}

/// Verb enum mirroring `SkPathVerb` for pure-Rust corner filtering.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerVerb {
    Move = 0,
    Line = 1,
    Quad = 2,
    Conic = 3,
    Cubic = 4,
    Close = 5,
}

/// Output command emitted by `CornerPathState`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerOutCmd {
    pub verb: u8,
    pub _pad: [u8; 3],
    pub pts: [Point; 3],
    pub weight: f32,
}

impl CornerOutCmd {
    #[inline]
    fn move_to(p: Point) -> Self {
        Self {
            verb: CornerVerb::Move as u8,
            _pad: [0; 3],
            pts: [p, Point::new(0.0, 0.0), Point::new(0.0, 0.0)],
            weight: 0.0,
        }
    }

    #[inline]
    fn line_to(p: Point) -> Self {
        Self {
            verb: CornerVerb::Line as u8,
            _pad: [0; 3],
            pts: [p, Point::new(0.0, 0.0), Point::new(0.0, 0.0)],
            weight: 0.0,
        }
    }

    #[inline]
    fn quad_to(p1: Point, p2: Point) -> Self {
        Self {
            verb: CornerVerb::Quad as u8,
            _pad: [0; 3],
            pts: [p1, p2, Point::new(0.0, 0.0)],
            weight: 0.0,
        }
    }

    #[inline]
    fn conic_to(p1: Point, p2: Point, w: f32) -> Self {
        Self {
            verb: CornerVerb::Conic as u8,
            _pad: [0; 3],
            pts: [p1, p2, Point::new(0.0, 0.0)],
            weight: w,
        }
    }

    #[inline]
    fn cubic_to(p1: Point, p2: Point, p3: Point) -> Self {
        Self {
            verb: CornerVerb::Cubic as u8,
            _pad: [0; 3],
            pts: [p1, p2, p3],
            weight: 0.0,
        }
    }

    #[inline]
    fn close() -> Self {
        Self {
            verb: CornerVerb::Close as u8,
            _pad: [0; 3],
            pts: [Point::new(0.0, 0.0); 3],
            weight: 0.0,
        }
    }
}

/// Stateful corner-rounding filter matching `SkCornerPathEffectImpl::onFilterPath`.
#[derive(Debug, Clone)]
pub struct CornerPathState {
    radius: f32,
    prev_verb: u8,
    move_to_pt: Point,
    last_corner: Point,
    first_step: Point,
    step: Point,
    prev_is_valid: bool,
}

impl CornerPathState {
    pub fn new(radius: f32) -> Self {
        Self {
            radius,
            prev_verb: 0xFF,
            move_to_pt: Point::new(0.0, 0.0),
            last_corner: Point::new(0.0, 0.0),
            first_step: Point::new(0.0, 0.0),
            step: Point::new(0.0, 0.0),
            prev_is_valid: true,
        }
    }

    /// Processes a single verb from `SkPath::Iter`. Emits 0..=3 commands into `out` and
    /// returns the count emitted.
    pub fn process_verb(
        &mut self,
        verb: CornerVerb,
        pts: &[Point; 4],
        conic_weight: f32,
        is_closed_contour: bool,
        out: &mut [CornerOutCmd; 3],
    ) -> usize {
        let mut count = 0usize;
        match verb {
            CornerVerb::Move => {
                if self.prev_verb == CornerVerb::Line as u8 {
                    out[count] = CornerOutCmd::line_to(self.last_corner);
                    count += 1;
                }
                if is_closed_contour {
                    self.move_to_pt = pts[0];
                    self.prev_is_valid = false;
                } else {
                    out[count] = CornerOutCmd::move_to(pts[0]);
                    count += 1;
                    self.prev_is_valid = true;
                }
            }
            CornerVerb::Line => {
                let (step, draw_segment) = corner_compute_step(pts[0], pts[1], self.radius);
                self.step = step;
                if !self.prev_is_valid {
                    out[count] = CornerOutCmd::move_to(Point::new(
                        self.move_to_pt.x + self.step.x,
                        self.move_to_pt.y + self.step.y,
                    ));
                    count += 1;
                    self.prev_is_valid = true;
                } else {
                    out[count] = CornerOutCmd::quad_to(
                        pts[0],
                        Point::new(pts[0].x + self.step.x, pts[0].y + self.step.y),
                    );
                    count += 1;
                }
                if draw_segment {
                    out[count] = CornerOutCmd::line_to(Point::new(
                        pts[1].x - self.step.x,
                        pts[1].y - self.step.y,
                    ));
                    count += 1;
                }
                self.last_corner = pts[1];
                self.prev_is_valid = true;
            }
            CornerVerb::Quad => {
                if !self.prev_is_valid {
                    out[count] = CornerOutCmd::move_to(pts[0]);
                    count += 1;
                    self.prev_is_valid = true;
                }
                out[count] = CornerOutCmd::quad_to(pts[1], pts[2]);
                count += 1;
                self.last_corner = pts[2];
                self.first_step = Point::new(0.0, 0.0);
            }
            CornerVerb::Conic => {
                if !self.prev_is_valid {
                    out[count] = CornerOutCmd::move_to(pts[0]);
                    count += 1;
                    self.prev_is_valid = true;
                }
                out[count] = CornerOutCmd::conic_to(pts[1], pts[2], conic_weight);
                count += 1;
                self.last_corner = pts[2];
                self.first_step = Point::new(0.0, 0.0);
            }
            CornerVerb::Cubic => {
                if !self.prev_is_valid {
                    out[count] = CornerOutCmd::move_to(pts[0]);
                    count += 1;
                    self.prev_is_valid = true;
                }
                out[count] = CornerOutCmd::cubic_to(pts[1], pts[2], pts[3]);
                count += 1;
                self.last_corner = pts[3];
                self.first_step = Point::new(0.0, 0.0);
            }
            CornerVerb::Close => {
                if self.first_step.x != 0.0 || self.first_step.y != 0.0 {
                    out[count] = CornerOutCmd::quad_to(
                        self.last_corner,
                        Point::new(
                            self.last_corner.x + self.first_step.x,
                            self.last_corner.y + self.first_step.y,
                        ),
                    );
                    count += 1;
                }
                out[count] = CornerOutCmd::close();
                count += 1;
                self.prev_is_valid = false;
            }
        }
        if self.prev_verb == CornerVerb::Move as u8 {
            self.first_step = self.step;
        }
        self.prev_verb = verb as u8;
        count
    }

    /// Finalizes after the path iterator finishes. Returns `Some(last_corner)` if a final
    /// `lineTo(lastCorner)` should be emitted.
    #[inline]
    pub fn finish(&self) -> Option<Point> {
        if self.prev_is_valid {
            Some(self.last_corner)
        } else {
            None
        }
    }
}

// ============================================================================
// SkDashPathEffect (`onAsPoints` / `cull_line`)
// ============================================================================

/// Outsets `rect` for stroke width & miter join (matching `outset_for_stroke` in
/// `SkDashPathEffect.cpp`).
#[inline]
pub fn dash_outset_for_stroke(rect: &mut Rect, stroke_width: f32, is_miter_join: bool, miter: f32) {
    let mut radius = stroke_width / 2.0;
    if radius == 0.0 {
        radius = 1.0; // hairlines
    }
    if is_miter_join {
        radius *= miter;
    }
    rect.left -= radius;
    rect.top -= radius;
    rect.right += radius;
    rect.bottom += radius;
}

/// Attempts to trim an axis-aligned line segment to minimally cover `cull_rect`, keeping phase
/// modulo `interval_length`. Exact port of `cull_line` in `SkDashPathEffect.cpp`.
pub fn dash_cull_line(
    pts: &mut [Point; 2],
    stroke_width: f32,
    is_miter_join: bool,
    miter: f32,
    ctm: &Matrix,
    cull_rect: Option<Rect>,
    interval_length: f32,
) -> bool {
    let Some(mut bounds) = cull_rect else {
        return false;
    };

    let dx = pts[1].x - pts[0].x;
    let dy = pts[1].y - pts[0].y;
    if (dx != 0.0 && dy != 0.0) || (dx == 0.0 && dy == 0.0) {
        return false;
    }

    dash_outset_for_stroke(&mut bounds, stroke_width, is_miter_join, miter);

    let Some(inv) = ctm.invert() else {
        return false;
    };
    bounds = matrix_map_rect(&inv, bounds);

    if dx != 0.0 {
        let mut min_x = pts[0].x;
        let mut max_x = pts[1].x;
        if dx < 0.0 {
            std::mem::swap(&mut min_x, &mut max_x);
        }
        if max_x <= bounds.left || min_x >= bounds.right {
            return false;
        }
        if min_x < bounds.left {
            min_x = bounds.left - ((bounds.left - min_x) % interval_length);
        }
        if max_x > bounds.right {
            max_x = bounds.right + ((max_x - bounds.right) % interval_length);
        }
        if dx < 0.0 {
            std::mem::swap(&mut min_x, &mut max_x);
        }
        pts[0].x = min_x;
        pts[1].x = max_x;
    } else {
        let mut min_y = pts[0].y;
        let mut max_y = pts[1].y;
        if dy < 0.0 {
            std::mem::swap(&mut min_y, &mut max_y);
        }
        if max_y <= bounds.top || min_y >= bounds.bottom {
            return false;
        }
        if min_y < bounds.top {
            min_y = bounds.top - ((bounds.top - min_y) % interval_length);
        }
        if max_y > bounds.bottom {
            max_y = bounds.bottom + ((max_y - bounds.bottom) % interval_length);
        }
        if dy < 0.0 {
            std::mem::swap(&mut min_y, &mut max_y);
        }
        pts[0].y = min_y;
        pts[1].y = max_y;
    }

    true
}

/// Result of `dash_as_points` matching `SkDashImpl::onAsPoints`.
#[derive(Debug, Clone, PartialEq)]
pub struct DashPointsResult {
    pub flags: u32,
    pub size: Point,
    pub points: Vec<Point>,
    pub first_rect: Option<Rect>,
    pub last_rect: Option<Rect>,
}

/// Computes fast dash point-cloud data for an axis-aligned 2-point line segment, matching
/// `SkDashImpl::onAsPoints` in `SkDashPathEffect.cpp`.
///
/// `cap`: 0 = Butt, 1 = Round, 2 = Square (`SkPaint::Cap`).
#[allow(clippy::too_many_arguments)]
pub fn dash_as_points(
    intervals: &[f32],
    initial_dash_length: f32,
    initial_dash_index: usize,
    interval_length: f32,
    line_pts: [Point; 2],
    stroke_width: f32,
    cap: i32,
    is_miter_join: bool,
    miter: f32,
    matrix: &Matrix,
    cull_rect: Option<Rect>,
    populate_results: bool,
) -> Option<DashPointsResult> {
    if stroke_width <= 0.0 {
        return None;
    }
    if intervals.len() != 2
        || !scalar_nearly_equal(intervals[0], intervals[1], SK_SCALAR_NEARLY_ZERO)
        || intervals[0] != intervals[0].floor()
        || intervals[1] != intervals[1].floor()
    {
        return None;
    }
    // kButt_Cap == 0
    if cap != 0 {
        return None;
    }
    if !matrix.rect_stays_rect() {
        return None;
    }

    let mut pts = line_pts;
    if !dash_cull_line(
        &mut pts,
        stroke_width,
        is_miter_join,
        miter,
        matrix,
        cull_rect,
        interval_length,
    ) {
        return None;
    }

    let length = Point::length(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
    let mut tangent = Point::new(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
    if tangent.is_zero() {
        return None;
    }
    let inv_len = 1.0_f32 / length;
    tangent.x *= inv_len;
    tangent.y *= inv_len;

    let mut is_x_axis = true;
    let size;
    if scalar_nearly_equal(1.0, tangent.x, SK_SCALAR_NEARLY_ZERO)
        || scalar_nearly_equal(-1.0, tangent.x, SK_SCALAR_NEARLY_ZERO)
    {
        size = Point::new(intervals[0] / 2.0, stroke_width / 2.0);
    } else if scalar_nearly_equal(1.0, tangent.y, SK_SCALAR_NEARLY_ZERO)
        || scalar_nearly_equal(-1.0, tangent.y, SK_SCALAR_NEARLY_ZERO)
    {
        size = Point::new(stroke_width / 2.0, intervals[0] / 2.0);
        is_x_axis = false;
    } else if cap != 1 {
        return None;
    } else {
        size = Point::new(0.0, 0.0);
    }

    if !populate_results {
        return Some(DashPointsResult {
            flags: 0,
            size,
            points: Vec::new(),
            first_rect: None,
            last_rect: None,
        });
    }

    let mut flags = 0u32;
    let clamped_initial_dash_length = if initial_dash_length < length {
        initial_dash_length
    } else {
        length
    };
    if cap == 1 {
        flags |= 0x1; // PointData::kCircles_PointFlag
    }

    let mut num_points: i32 = 0;
    let mut len2 = length;
    if clamped_initial_dash_length > 0.0 || initial_dash_index == 0 {
        if initial_dash_index == 0 {
            if clamped_initial_dash_length > 0.0 {
                if clamped_initial_dash_length >= intervals[0] {
                    num_points += 1;
                }
                len2 -= clamped_initial_dash_length;
            }
            len2 -= intervals[1];
            if len2 < 0.0 {
                len2 = 0.0;
            }
        } else {
            len2 -= clamped_initial_dash_length;
        }
    }

    let num_intervals = len2 / interval_length;
    if !num_intervals.is_finite() || num_intervals > MAX_DASH_COUNT {
        return None;
    }
    let mut num_mid_points = float_floor_to_int(num_intervals);
    num_points += num_mid_points;
    len2 -= (num_mid_points as f32) * interval_length;
    let mut partial_last = false;
    if len2 > 0.0 {
        if len2 < intervals[0] {
            partial_last = true;
        } else {
            num_mid_points += 1;
            num_points += 1;
        }
    }

    let mut points = Vec::with_capacity(num_points.max(0) as usize);
    let mut first_rect = None;
    let mut last_rect = None;

    let mut distance: f32 = 0.0;
    if clamped_initial_dash_length > 0.0 || initial_dash_index == 0 {
        if initial_dash_index == 0 {
            if clamped_initial_dash_length > 0.0 {
                let x = pts[0].x + tangent.x * (clamped_initial_dash_length / 2.0);
                let y = pts[0].y + tangent.y * (clamped_initial_dash_length / 2.0);
                let (half_w, half_h) = if is_x_axis {
                    (clamped_initial_dash_length / 2.0, stroke_width / 2.0)
                } else {
                    (stroke_width / 2.0, clamped_initial_dash_length / 2.0)
                };
                if clamped_initial_dash_length < intervals[0] {
                    first_rect = Some(Rect::make_ltrb(x - half_w, y - half_h, x + half_w, y + half_h));
                } else {
                    points.push(Point::new(x, y));
                }
                distance += clamped_initial_dash_length;
            }
            distance += intervals[1];
        } else {
            distance += clamped_initial_dash_length;
        }
    }

    if num_mid_points != 0 {
        distance += intervals[0] / 2.0;
        for _ in 0..num_mid_points {
            let x = pts[0].x + tangent.x * distance;
            let y = pts[0].y + tangent.y * distance;
            points.push(Point::new(x, y));
            distance += interval_length;
        }
        distance -= intervals[0] / 2.0;
    }

    if partial_last {
        let temp = length - distance;
        let x = pts[0].x + tangent.x * (distance + (temp / 2.0));
        let y = pts[0].y + tangent.y * (distance + (temp / 2.0));
        let (half_w, half_h) = if is_x_axis {
            (temp / 2.0, stroke_width / 2.0)
        } else {
            (stroke_width / 2.0, temp / 2.0)
        };
        last_rect = Some(Rect::make_ltrb(x - half_w, y - half_h, x + half_w, y + half_h));
    }

    debug_assert_eq!(points.len(), num_points as usize);
    Some(DashPointsResult {
        flags,
        size,
        points,
        first_rect,
        last_rect,
    })
}

// ============================================================================
// SkDiscretePathEffect
// ============================================================================

/// Exact port of `LCGRandom` from `SkDiscretePathEffect.cpp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LcgRandom {
    seed: u32,
}

impl LcgRandom {
    const K_MUL: u32 = 1_664_525;
    const K_ADD: u32 = 1_013_904_223;

    #[inline]
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// Initializes the PRNG from `seed_assist` and the first contour's length,
    /// matching `SkDiscretePathEffectImpl::onFilterPath`.
    #[inline]
    pub fn from_contour(seed_assist: u32, first_contour_length: f32) -> Self {
        let seed = seed_assist ^ (float_round_to_int(first_contour_length) as u32);
        Self::new(seed ^ seed.rotate_left(16))
    }

    #[inline]
    pub fn seed(&self) -> u32 {
        self.seed
    }

    #[inline]
    fn next_u(&mut self) -> u32 {
        let r = self.seed.wrapping_mul(Self::K_MUL).wrapping_add(Self::K_ADD);
        self.seed = r;
        r
    }

    #[inline]
    fn next_s(&mut self) -> i32 {
        self.next_u() as i32
    }

    #[inline]
    fn next_sfixed1(&mut self) -> i32 {
        self.next_s() >> 15
    }

    /// Returns the next pseudo-random `f32` in `[-1.0, 1.0)`.
    #[inline]
    pub fn next_sscalar1(&mut self) -> f32 {
        (self.next_sfixed1() as f32) * (1.0_f32 / 65536.0_f32)
    }
}

/// Validates `SkDiscretePathEffect::Make` parameters.
#[inline]
pub fn discrete_is_valid(seg_length: f32, deviation: f32) -> bool {
    seg_length.is_finite() && deviation.is_finite() && seg_length > SK_SCALAR_NEARLY_ZERO
}

/// Perturbs point `p` along the CCW normal of `tangent` by `scale`,
/// matching `Perterb` in `SkDiscretePathEffect.cpp`.
#[inline]
pub fn discrete_perturb(p: Point, tangent: Vector, scale: f32) -> Point {
    // SkPointPriv::RotateCCW: (tangent.y, -tangent.x)
    let mut normal = Point::new(tangent.y, -tangent.x);
    normal.set_length_in_place(scale);
    Point::new(p.x + normal.x, p.y + normal.y)
}

/// Contour subdivision plan for `SkDiscretePathEffectImpl::onFilterPath`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiscreteContourPlan {
    pub copy_verbatim: bool,
    pub _pad: [u8; 3],
    pub n: i32,
    pub delta: f32,
    pub initial_distance: f32,
}

/// Computes subdivision parameters for a single contour in `SkDiscretePathEffectImpl::onFilterPath`.
pub fn discrete_contour_plan(
    seg_length: f32,
    length: f32,
    do_fill: bool,
    is_closed: bool,
) -> DiscreteContourPlan {
    let min_segs = if do_fill { 3.0_f32 } else { 2.0_f32 };
    if seg_length * min_segs > length {
        return DiscreteContourPlan {
            copy_verbatim: true,
            _pad: [0; 3],
            n: 0,
            delta: 0.0,
            initial_distance: 0.0,
        };
    }

    let mut n = float_round_to_int(length / seg_length).min(MAX_REASONABLE_ITERATIONS);
    let delta = length / (n as f32);
    let mut distance = 0.0_f32;
    if is_closed {
        n -= 1;
        distance += delta / 2.0;
    }
    DiscreteContourPlan {
        copy_verbatim: false,
        _pad: [0; 3],
        n,
        delta,
        initial_distance: distance,
    }
}

/// Outsets `bounds` by `|deviation|` for `SkDiscretePathEffectImpl::computeFastBounds`.
#[inline]
pub fn discrete_compute_fast_bounds(bounds: Rect, deviation: f32) -> Rect {
    let outset = deviation.abs();
    Rect::make_ltrb(
        bounds.left - outset,
        bounds.top - outset,
        bounds.right + outset,
        bounds.bottom + outset,
    )
}

// ============================================================================
// SkTrimPathEffect
// ============================================================================

/// Mode mirroring `SkTrimPathEffect::Mode`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    Normal = 0,
    Inverted = 1,
}

/// Validates and pins `SkTrimPathEffect::Make` arguments.
/// Returns `Some((pinned_start, pinned_stop))` if a `SkTrimPE` should be constructed,
/// or `None` if `Make` should return `nullptr` (invalid or identity).
pub fn trim_validate_and_pin(start_t: f32, stop_t: f32, mode: TrimMode) -> Option<(f32, f32)> {
    if !start_t.is_finite() || !stop_t.is_finite() {
        return None;
    }
    if start_t <= 0.0 && stop_t >= 1.0 && mode == TrimMode::Normal {
        return None;
    }
    let start_t = pin_f32(start_t, 0.0, 1.0);
    let stop_t = pin_f32(stop_t, 0.0, 1.0);
    if start_t >= stop_t && mode == TrimMode::Inverted {
        return None;
    }
    Some((start_t, stop_t))
}

/// Span plan for `SkTrimPE::onFilterPath`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimSpanPlan {
    pub has_first_span: bool,
    pub has_second_span: bool,
    pub is_inverted: bool,
    pub _pad: u8,
    pub first_start: f32,
    pub first_stop: f32,
    pub second_start: f32,
    pub second_stop: f32,
}

/// Computes the 0, 1, or 2 arc-length spans to extract in `SkTrimPE::onFilterPath`.
pub fn trim_compute_spans(total_len: f32, start_t: f32, stop_t: f32, mode: TrimMode) -> TrimSpanPlan {
    let arc_start = total_len * start_t;
    let arc_stop = total_len * stop_t;
    match mode {
        TrimMode::Normal => {
            if arc_start < arc_stop {
                TrimSpanPlan {
                    has_first_span: true,
                    has_second_span: false,
                    is_inverted: false,
                    _pad: 0,
                    first_start: arc_start,
                    first_stop: arc_stop,
                    second_start: 0.0,
                    second_stop: 0.0,
                }
            } else {
                TrimSpanPlan {
                    has_first_span: false,
                    has_second_span: false,
                    is_inverted: false,
                    _pad: 0,
                    first_start: 0.0,
                    first_stop: 0.0,
                    second_start: 0.0,
                    second_stop: 0.0,
                }
            }
        }
        TrimMode::Inverted => TrimSpanPlan {
            has_first_span: arc_stop < total_len,
            has_second_span: 0.0 < arc_start,
            is_inverted: true,
            _pad: 0,
            first_start: arc_stop,
            first_stop: total_len,
            second_start: 0.0,
            second_stop: arc_start,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_1d_initial_offset() {
        assert!(!path_1d_is_valid(0.0, 0.0, false));
        assert!(!path_1d_is_valid(10.0, f32::NAN, false));
        assert!(!path_1d_is_valid(10.0, 0.0, true));
        assert!(path_1d_is_valid(10.0, 3.0, false));

        assert_eq!(path_1d_initial_offset(10.0, 0.0), 0.0);
        assert_eq!(path_1d_initial_offset(10.0, 3.0), 7.0);
        assert_eq!(path_1d_initial_offset(10.0, 13.0), 7.0);
        assert_eq!(path_1d_initial_offset(10.0, -3.0), 3.0);
        assert_eq!(path_1d_initial_offset(10.0, -13.0), 3.0);
        assert_eq!(path_1d_initial_offset(10.0, 10.0), 0.0);
    }

    #[test]
    fn test_path_1d_morph_point() {
        let p = path_1d_morph_point(5.0, 2.0, Point::new(10.0, 20.0), Point::new(1.0, 0.0));
        assert!((p.x - 10.0).abs() < 1e-5);
        assert!((p.y - 22.0).abs() < 1e-5);

        let p90 = path_1d_morph_point(5.0, 2.0, Point::new(10.0, 20.0), Point::new(0.0, 1.0));
        assert!((p90.x - 8.0).abs() < 1e-5);
        assert!((p90.y - 20.0).abs() < 1e-5);
    }

    #[test]
    fn test_line_2d() {
        assert!(line_2d_is_valid(0.0));
        assert!(line_2d_is_valid(2.5));
        assert!(!line_2d_is_valid(-1.0));
        assert!(!line_2d_is_valid(f32::NAN));

        let id = Matrix::identity();
        assert!(line_2d_span_endpoints(0, 0, 1, &id).is_none());
        let pts = line_2d_span_endpoints(2, 3, 5, &id).unwrap();
        assert_eq!(pts[0], Point::new(2.5, 3.5));
        assert_eq!(pts[1], Point::new(7.5, 3.5));
    }

    #[test]
    fn test_corner_compute_step_and_state() {
        assert!(corner_is_valid(5.0));
        assert!(!corner_is_valid(0.0));
        assert!(!corner_is_valid(-1.0));
        assert!(!corner_is_valid(f32::INFINITY));

        let (step, draw_seg) = corner_compute_step(Point::new(0.0, 0.0), Point::new(100.0, 0.0), 10.0);
        assert!(draw_seg);
        assert_eq!(step, Point::new(10.0, 0.0));

        let (step_short, draw_seg_short) =
            corner_compute_step(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 10.0);
        assert!(!draw_seg_short);
        assert_eq!(step_short, Point::new(5.0, 0.0));

        // Open L-shape: move(0,0), line(100,0), line(100,100)
        let mut st = CornerPathState::new(10.0);
        let mut out = [CornerOutCmd::close(); 3];
        let z = Point::new(0.0, 0.0);
        let n0 = st.process_verb(CornerVerb::Move, &[Point::new(0.0, 0.0), z, z, z], 0.0, false, &mut out);
        assert_eq!(n0, 1);
        assert_eq!(out[0].verb, CornerVerb::Move as u8);

        let n1 = st.process_verb(
            CornerVerb::Line,
            &[Point::new(0.0, 0.0), Point::new(100.0, 0.0), z, z],
            0.0,
            false,
            &mut out,
        );
        assert_eq!(n1, 2); // quad + line
        assert_eq!(out[1].pts[0], Point::new(90.0, 0.0));

        let n2 = st.process_verb(
            CornerVerb::Line,
            &[Point::new(100.0, 0.0), Point::new(100.0, 100.0), z, z],
            0.0,
            false,
            &mut out,
        );
        assert_eq!(n2, 2);
        assert_eq!(out[0].verb, CornerVerb::Quad as u8);
        assert_eq!(out[0].pts[0], Point::new(100.0, 0.0));
        assert_eq!(out[0].pts[1], Point::new(100.0, 10.0));
        assert_eq!(st.finish(), Some(Point::new(100.0, 100.0)));
    }

    #[test]
    fn test_dash_as_points_horizontal() {
        let res = dash_as_points(
            &[4.0, 4.0],
            4.0,
            0,
            8.0,
            [Point::new(0.0, 10.0), Point::new(20.0, 10.0)],
            2.0,
            0,
            false,
            4.0,
            &Matrix::identity(),
            Some(Rect::make_ltrb(0.0, 0.0, 100.0, 100.0)),
            true,
        )
        .unwrap();
        assert_eq!(res.size, Point::new(2.0, 1.0));
        assert!(res.first_rect.is_none());
        assert!(res.last_rect.is_none());
        // Dashes at [0,4], [8,12], [16,20] -> midpoints at x=2, 10, 18
        assert_eq!(res.points, vec![
            Point::new(2.0, 10.0),
            Point::new(10.0, 10.0),
            Point::new(18.0, 10.0),
        ]);
    }

    #[test]
    fn test_discrete_lcg_and_plan() {
        let mut rng = LcgRandom::from_contour(0, 100.0);
        let v1 = rng.next_sscalar1();
        let v2 = rng.next_sscalar1();
        assert!((-1.0..1.0).contains(&v1));
        assert!((-1.0..1.0).contains(&v2));
        assert_ne!(v1, v2);

        let p = discrete_perturb(Point::new(10.0, 10.0), Point::new(1.0, 0.0), 5.0);
        // RotateCCW(1, 0) = (0, -1), scaled by 5 = (0, -5)
        assert!((p.x - 10.0).abs() < 1e-5);
        assert!((p.y - 5.0).abs() < 1e-5);

        let short_plan = discrete_contour_plan(10.0, 15.0, false, false);
        assert!(short_plan.copy_verbatim);

        let long_plan = discrete_contour_plan(10.0, 100.0, false, true);
        assert!(!long_plan.copy_verbatim);
        assert_eq!(long_plan.n, 9);
        assert!((long_plan.delta - 10.0).abs() < 1e-5);
        assert!((long_plan.initial_distance - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_trim_validate_and_spans() {
        assert_eq!(trim_validate_and_pin(0.0, 1.0, TrimMode::Normal), None);
        assert_eq!(trim_validate_and_pin(-0.5, 1.5, TrimMode::Normal), None);
        assert_eq!(trim_validate_and_pin(0.5, 0.5, TrimMode::Inverted), None);
        assert_eq!(trim_validate_and_pin(0.2, 0.8, TrimMode::Normal), Some((0.2, 0.8)));
        assert_eq!(trim_validate_and_pin(0.2, 0.8, TrimMode::Inverted), Some((0.2, 0.8)));

        let plan = trim_compute_spans(100.0, 0.25, 0.75, TrimMode::Inverted);
        assert!(plan.has_first_span);
        assert!(plan.has_second_span);
        assert_eq!(plan.first_start, 75.0);
        assert_eq!(plan.first_stop, 100.0);
        assert_eq!(plan.second_start, 0.0);
        assert_eq!(plan.second_stop, 25.0);
    }
}
