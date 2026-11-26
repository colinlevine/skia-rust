#![forbid(unsafe_code)]

use skia_rs_geometry::{
    float_saturate2int, scalar_nearly_zero, Point, Rect, Vector, SK_SCALAR_NEARLY_ZERO,
    SK_SCALAR_PI, SK_SCALAR_ROOT_2_OVER_2,
};

pub const SK_SCALAR_MAX: f32 = 3.402_823_5e38_f32;
pub const SK_SCALAR_SQRT_2: f32 = 1.414_213_5_f32;
pub const SK_SCALAR_SIN_COS_NEARLY_ZERO: f32 = 1.0 / (1 << 16) as f32;
pub const SK_PAINT_DEFAULTS_MITER_LIMIT: f32 = 4.0;
pub const K_W0_PLANE_DISTANCE: f32 = 1.0 / (1 << 14) as f32;

pub const SEG_LINE: u8 = 1 << 0;
pub const SEG_QUAD: u8 = 1 << 1;
pub const SEG_CONIC: u8 = 1 << 2;
pub const SEG_CUBIC: u8 = 1 << 3;

pub const FOUR_QUARTER_CIRCLE_CONICS: [f32; 4] = [
    SK_SCALAR_ROOT_2_OVER_2,
    SK_SCALAR_ROOT_2_OVER_2,
    SK_SCALAR_ROOT_2_OVER_2,
    SK_SCALAR_ROOT_2_OVER_2,
];

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FillType {
    #[default]
    Winding = 0,
    EvenOdd = 1,
    InverseWinding = 2,
    InverseEvenOdd = 3,
}

impl FillType {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Winding),
            1 => Some(Self::EvenOdd),
            2 => Some(Self::InverseWinding),
            3 => Some(Self::InverseEvenOdd),
            _ => None,
        }
    }

    #[inline]
    pub fn is_even_odd(self) -> bool {
        ((self as u8) & 1) != 0
    }

    #[inline]
    pub fn is_inverse(self) -> bool {
        ((self as u8) & 2) != 0
    }

    #[inline]
    pub fn toggle_inverse(self) -> Self {
        Self::from_u8((self as u8) ^ 2).unwrap()
    }

    #[inline]
    pub fn convert_to_non_inverse(self) -> Self {
        Self::from_u8((self as u8) & 1).unwrap()
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Cw = 0,
    Ccw = 1,
}

impl Direction {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Cw),
            1 => Some(Self::Ccw),
            _ => None,
        }
    }

    #[inline]
    pub fn reverse(self) -> Self {
        match self {
            Self::Cw => Self::Ccw,
            Self::Ccw => Self::Cw,
        }
    }

    #[inline]
    pub fn to_convexity(self) -> Convexity {
        match self {
            Self::Cw => Convexity::ConvexCw,
            Self::Ccw => Convexity::ConvexCcw,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Verb {
    Move = 0,
    Line = 1,
    Quad = 2,
    Conic = 3,
    Cubic = 4,
    Close = 5,
}

impl Verb {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Move),
            1 => Some(Self::Line),
            2 => Some(Self::Quad),
            3 => Some(Self::Conic),
            4 => Some(Self::Cubic),
            5 => Some(Self::Close),
            _ => None,
        }
    }

    /// Number of stored points consumed by this verb (Move=1, Line=1, Quad=2, Conic=2, Cubic=3, Close=0).
    #[inline]
    pub const fn pts_in_verb(self) -> usize {
        match self {
            Self::Move => 1,
            Self::Line => 1,
            Self::Quad => 2,
            Self::Conic => 2,
            Self::Cubic => 3,
            Self::Close => 0,
        }
    }

    /// Number of points returned by legacy iterator (Move=1, Line=2, Quad=3, Conic=3, Cubic=4, Close=0).
    #[inline]
    pub const fn pts_in_iter(self) -> usize {
        match self {
            Self::Move => 1,
            Self::Line => 2,
            Self::Quad => 3,
            Self::Conic => 3,
            Self::Cubic => 4,
            Self::Close => 0,
        }
    }

    #[inline]
    pub const fn segment_mask(self) -> u8 {
        match self {
            Self::Move => 0,
            Self::Line => SEG_LINE,
            Self::Quad => SEG_QUAD,
            Self::Conic => SEG_CONIC,
            Self::Cubic => SEG_CUBIC,
            Self::Close => 0,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Convexity {
    ConvexCw = 0,
    ConvexCcw = 1,
    ConvexDegenerate = 2,
    Concave = 3,
    #[default]
    Unknown = 4,
}

impl Convexity {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::ConvexCw),
            1 => Some(Self::ConvexCcw),
            2 => Some(Self::ConvexDegenerate),
            3 => Some(Self::Concave),
            4 => Some(Self::Unknown),
            _ => None,
        }
    }

    #[inline]
    pub fn is_convex(self) -> bool {
        matches!(
            self,
            Self::ConvexCw | Self::ConvexCcw | Self::ConvexDegenerate
        )
    }

    #[inline]
    pub fn opposite_convex_direction(self) -> Self {
        debug_assert!(self.is_convex());
        match self {
            Self::ConvexCw => Self::ConvexCcw,
            Self::ConvexCcw => Self::ConvexCw,
            other => other,
        }
    }

    #[inline]
    pub fn to_direction(self) -> Option<Direction> {
        match self {
            Self::ConvexCw => Some(Direction::Cw),
            Self::ConvexCcw => Some(Direction::Ccw),
            _ => None,
        }
    }

    #[inline]
    pub fn to_first_direction(self) -> FirstDirection {
        match self {
            Self::ConvexCw => FirstDirection::Cw,
            Self::ConvexCcw => FirstDirection::Ccw,
            _ => FirstDirection::Unknown,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FirstDirection {
    Cw = 0,
    Ccw = 1,
    #[default]
    Unknown = 2,
}

impl FirstDirection {
    #[inline]
    pub fn to_convexity(self) -> Convexity {
        match self {
            Self::Cw => Convexity::ConvexCw,
            Self::Ccw => Convexity::ConvexCcw,
            Self::Unknown => Convexity::ConvexDegenerate,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PathIsAType {
    #[default]
    General = 0,
    Oval = 1,
    RRect = 2,
}

impl PathIsAType {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::General),
            1 => Some(Self::Oval),
            2 => Some(Self::RRect),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArcSize {
    Small = 0,
    Large = 1,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArcType {
    Arc = 0,
    Wedge = 1,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AddPathMode {
    Append = 0,
    Extend = 1,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Cap {
    #[default]
    Butt = 0,
    Round = 1,
    Square = 2,
}

impl Cap {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Butt),
            1 => Some(Self::Round),
            2 => Some(Self::Square),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Join {
    #[default]
    Miter = 0,
    Round = 1,
    Bevel = 2,
}

impl Join {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Miter),
            1 => Some(Self::Round),
            2 => Some(Self::Bevel),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StrokeStyle {
    Hairline = 0,
    Fill = 1,
    Stroke = 2,
    StrokeAndFill = 3,
}

#[derive(Copy, Clone, Debug)]
pub struct PathRaw<'a> {
    pub points: &'a [Point],
    pub verbs: &'a [Verb],
    pub conics: &'a [f32],
    pub bounds: Rect,
    pub fill_type: FillType,
    pub convexity: Convexity,
    pub segment_mask: u8,
}

impl<'a> PathRaw<'a> {
    pub fn empty(fill_type: FillType) -> Self {
        Self {
            points: &[],
            verbs: &[],
            conics: &[],
            bounds: Rect::EMPTY,
            fill_type,
            convexity: Convexity::ConvexDegenerate,
            segment_mask: 0,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }
}

/// Owned path buffer returned by Rust algorithms (builder, stroke, dash, parse, perspective clip).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathDataBuf {
    pub points: Vec<Point>,
    pub verbs: Vec<Verb>,
    pub conics: Vec<f32>,
    pub fill_type: FillType,
    pub convexity: Convexity,
    pub segment_mask: u8,
    pub last_move_index: i32,
    pub is_a_type: PathIsAType,
    pub is_a_dir: Direction,
    pub is_a_start_index: u8,
    pub is_volatile: bool,
}

impl PathDataBuf {
    pub fn new(fill_type: FillType) -> Self {
        Self {
            points: Vec::new(),
            verbs: Vec::new(),
            conics: Vec::new(),
            fill_type,
            convexity: Convexity::Unknown,
            segment_mask: 0,
            last_move_index: -1,
            is_a_type: PathIsAType::General,
            is_a_dir: Direction::Cw,
            is_a_start_index: 0,
            is_volatile: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    pub fn is_finite(&self) -> bool {
        self.points.iter().all(|p| p.is_finite())
    }

    pub fn as_raw(&self, resolve_convexity: bool) -> Option<PathRaw<'_>> {
        let bounds = trimmed_bounds(&self.points, &self.verbs)?;
        let convexity = if self.convexity == Convexity::Unknown && resolve_convexity {
            crate::analysis::compute_convexity(&self.points, &self.verbs, &self.conics)
        } else {
            self.convexity
        };
        Some(PathRaw {
            points: &self.points,
            verbs: &self.verbs,
            conics: &self.conics,
            bounds,
            fill_type: self.fill_type,
            convexity,
            segment_mask: self.segment_mask,
        })
    }
}

#[inline]
pub fn compute_segment_mask(verbs: &[Verb]) -> u8 {
    let mut mask = 0u8;
    for &v in verbs {
        mask |= v.segment_mask();
    }
    mask
}

#[inline]
pub fn valid_conic_weight(w: f32) -> bool {
    w >= 0.0 && w.is_finite()
}

/// Validates verb sequence, point count, conic count, and conic weights (matching `valid_path_data`).
pub fn valid_path_data(pts: &[Point], vbs: &[Verb], conics: &[f32]) -> bool {
    if vbs.is_empty() {
        return pts.is_empty() && conics.is_empty();
    }
    if vbs[0] != Verb::Move {
        return false;
    }
    let mut prev = Verb::Move;
    let mut point_count = 1usize;
    let mut conic_count = 0usize;

    for &curr in &vbs[1..] {
        if prev == Verb::Move && curr == Verb::Move {
            return false;
        }
        if prev == Verb::Close && curr != Verb::Move {
            return false;
        }
        point_count = match point_count.checked_add(curr.pts_in_verb()) {
            Some(c) => c,
            None => return false,
        };
        if curr == Verb::Conic {
            conic_count += 1;
        }
        prev = curr;
    }

    if pts.len() != point_count || conics.len() != conic_count {
        return false;
    }

    for &w in conics {
        if !valid_conic_weight(w) {
            return false;
        }
    }

    true
}

/// Validates raw u8 verb bytes and converts to `Vec<Verb>` if valid.
pub fn valid_raw_verbs(raw_verbs: &[u8]) -> Option<Vec<Verb>> {
    let mut out = Vec::with_capacity(raw_verbs.len());
    for &b in raw_verbs {
        out.push(Verb::from_u8(b)?);
    }
    Some(out)
}

/// Matching `SkPathPriv::TrimmedBounds`.
pub fn trimmed_bounds(pts: &[Point], vbs: &[Verb]) -> Option<Rect> {
    let slice = if vbs.len() > 1 && *vbs.last().unwrap() == Verb::Close {
        pts
    } else if vbs.len() > 1 && *vbs.last().unwrap() == Verb::Move {
        if pts.is_empty() || !pts.last().unwrap().is_finite() {
            return None;
        }
        &pts[..pts.len() - 1]
    } else {
        pts
    };
    Rect::bounds(slice)
}

/// Matching `SkPathPriv::FindLastMoveToIndex`.
pub fn find_last_move_to_index(verbs: &[Verb], pt_count: usize) -> i32 {
    if verbs.is_empty() {
        debug_assert!(pt_count == 0);
        return -1;
    }
    debug_assert!(verbs[0] == Verb::Move);
    debug_assert!(pt_count > 0);

    let mut pt_index = (pt_count as i32) - 1;
    for &verb in verbs.iter().rev() {
        if verb == Verb::Move {
            break;
        }
        pt_index -= verb.pts_in_verb() as i32;
    }
    debug_assert!(pt_index >= 0);
    pt_index
}

/// Matching `SkPathPriv::IsAxisAligned`.
pub fn is_axis_aligned(pts: &[Point]) -> bool {
    for i in 1..pts.len() {
        if pts[i - 1].x != pts[i].x && pts[i - 1].y != pts[i].y {
            return false;
        }
    }
    true
}

/// Matching `SkPathPriv::AllPointsEq`.
pub fn all_points_eq(pts: &[Point]) -> bool {
    if pts.len() <= 1 {
        return true;
    }
    let first = pts[0];
    for &p in &pts[1..] {
        if first != p {
            return false;
        }
    }
    true
}

/// Matching `SkPathPriv::IsClosedSingleContour`.
pub fn is_closed_single_contour(verbs: &[Verb]) -> bool {
    if verbs.is_empty() {
        return false;
    }
    let mut move_count = 0usize;
    for (i, &verb) in verbs.iter().enumerate() {
        match verb {
            Verb::Move => {
                move_count += 1;
                if move_count > 1 {
                    return false;
                }
            }
            Verb::Close => return i + 1 == verbs.len(),
            _ => {}
        }
    }
    false
}

#[inline]
pub fn equals_within_tolerance(p1: Point, p2: Point, tol: f32) -> bool {
    scalar_nearly_zero(p1.x - p2.x, tol) && scalar_nearly_zero(p1.y - p2.y, tol)
}

#[inline]
pub fn is_line_degenerate(p1: Point, p2: Point, exact: bool) -> bool {
    if exact {
        p1 == p2
    } else {
        equals_within_tolerance(p1, p2, SK_SCALAR_NEARLY_ZERO)
    }
}

#[inline]
pub fn is_quad_degenerate(p1: Point, p2: Point, p3: Point, exact: bool) -> bool {
    if exact {
        p1 == p2 && p2 == p3
    } else {
        equals_within_tolerance(p1, p2, SK_SCALAR_NEARLY_ZERO)
            && equals_within_tolerance(p2, p3, SK_SCALAR_NEARLY_ZERO)
    }
}

#[inline]
pub fn is_cubic_degenerate(p1: Point, p2: Point, p3: Point, p4: Point, exact: bool) -> bool {
    if exact {
        p1 == p2 && p2 == p3 && p3 == p4
    } else {
        equals_within_tolerance(p1, p2, SK_SCALAR_NEARLY_ZERO)
            && equals_within_tolerance(p2, p3, SK_SCALAR_NEARLY_ZERO)
            && equals_within_tolerance(p3, p4, SK_SCALAR_NEARLY_ZERO)
    }
}

#[inline]
pub fn is_zero_length_since_point(pts: &[Point], start_pt_index: usize) -> bool {
    if start_pt_index >= pts.len() {
        return true;
    }
    all_points_eq(&pts[start_pt_index..])
}

#[inline]
pub fn degrees_to_radians(degrees: f32) -> f32 {
    degrees * (SK_SCALAR_PI / 180.0)
}

#[inline]
pub fn scalar_sin_snap_to_zero(radians: f32) -> f32 {
    let v = radians.sin();
    if scalar_nearly_zero(v, SK_SCALAR_SIN_COS_NEARLY_ZERO) {
        0.0
    } else {
        v
    }
}

#[inline]
pub fn scalar_cos_snap_to_zero(radians: f32) -> f32 {
    let v = radians.cos();
    if scalar_nearly_zero(v, SK_SCALAR_SIN_COS_NEARLY_ZERO) {
        0.0
    } else {
        v
    }
}

/// Matches `sk_float_round(x) = floor(x + 0.5f)`.
#[inline]
pub fn sk_float_round(x: f32) -> f32 {
    (x + 0.5_f32).floor()
}

/// Matches `sk_float_ceil2int(x) = sk_float_saturate2int(ceil(x))`.
#[inline]
pub fn sk_float_ceil2int(x: f32) -> i32 {
    float_saturate2int(x.ceil())
}

#[inline]
pub fn can_normalize(dx: f32, dy: f32) -> bool {
    dx.is_finite() && dy.is_finite() && (dx != 0.0 || dy != 0.0)
}

#[inline]
pub fn rotate_cw(v: Vector) -> Vector {
    Vector::new(-v.y, v.x)
}

#[inline]
pub fn rotate_ccw(v: Vector) -> Vector {
    Vector::new(v.y, -v.x)
}

#[inline]
pub fn point_distance(a: Point, b: Point) -> f32 {
    Point::length(a.x - b.x, a.y - b.y)
}

#[inline]
pub fn ieee_float_divide(numer: f32, denom: f32) -> f32 {
    numer / denom
}

#[inline]
pub fn ieee_double_divide(numer: f64, denom: f64) -> f64 {
    numer / denom
}

#[inline]
pub fn scalar_sign_as_int(x: f32) -> i32 {
    if x < 0.0 {
        -1
    } else if x > 0.0 {
        1
    } else {
        0
    }
}
