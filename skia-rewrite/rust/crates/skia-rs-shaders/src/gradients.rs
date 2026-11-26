//! Safe Rust port of `SkLinearGradient.cpp`, `SkRadialGradient.cpp`,
//! `SkSweepGradient.cpp`, and `SkConicalGradient.cpp`.

use skia_rs_geometry::{
    cpp_max_f32, scalar_nearly_equal, scalar_nearly_zero, Matrix, Point, SK_SCALAR_NEARLY_ZERO,
};

use crate::gradient_base::{
    valid_gradient, GradientInterpolation, DEGENERATE_THRESHOLD,
};
use crate::TileMode;

// ============================================================================
// SkLinearGradient
// ============================================================================

/// Exact port of `pts_to_unit_matrix` in `SkLinearGradient.cpp`.
pub fn linear_pts_to_unit_matrix(p0: Point, p1: Point) -> Matrix {
    let mut vec = p1 - p0;
    let mag = vec.mag();
    let inv = if mag != 0.0 { 1.0_f32 / mag } else { 0.0 };
    vec = vec.scale(inv);

    let mut matrix = Matrix::identity();
    matrix.set_sin_cos_pivot(-vec.y, vec.x, p0.x, p0.y);
    matrix.post_translate(-p0.x, -p0.y);
    matrix.post_scale(inv, inv);
    matrix
}

/// Action returned by `classify_linear_gradient`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinearGradientAction {
    ReturnNull = 0,
    ReturnSingleColor = 1,
    ReturnDegenerate = 2,
    Create = 3,
}

/// Matches `SkShaders::LinearGradient` validation and early-exit logic.
pub fn classify_linear_gradient(
    has_pts: bool,
    p0: Point,
    p1: Point,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> LinearGradientAction {
    if !has_pts {
        return LinearGradientAction::ReturnNull;
    }
    let len = (p1 - p0).mag();
    if !len.is_finite() {
        return LinearGradientAction::ReturnNull;
    }
    if !valid_gradient(has_colors_data, color_count, tile_mode, interp) {
        return LinearGradientAction::ReturnNull;
    }
    if color_count == 1 {
        return LinearGradientAction::ReturnSingleColor;
    }
    if has_lm && !lm_invertible {
        return LinearGradientAction::ReturnNull;
    }
    if scalar_nearly_zero(len, DEGENERATE_THRESHOLD) {
        return LinearGradientAction::ReturnDegenerate;
    }
    LinearGradientAction::Create
}

// ============================================================================
// SkRadialGradient
// ============================================================================

/// Exact port of `rad_to_unit_matrix` in `SkRadialGradient.cpp`.
pub fn radial_to_unit_matrix(center: Point, radius: f32) -> Matrix {
    let inv = 1.0_f32 / radius;
    let mut matrix = Matrix::translate(-center.x, -center.y);
    matrix.post_scale(inv, inv);
    matrix
}

/// Action returned by `classify_radial_gradient`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RadialGradientAction {
    ReturnNull = 0,
    ReturnSingleColor = 1,
    ReturnDegenerate = 2,
    Create = 3,
}

/// Matches `SkShaders::RadialGradient` validation and early-exit logic.
pub fn classify_radial_gradient(
    radius: f32,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> RadialGradientAction {
    if radius < 0.0 {
        return RadialGradientAction::ReturnNull;
    }
    if !valid_gradient(has_colors_data, color_count, tile_mode, interp) {
        return RadialGradientAction::ReturnNull;
    }
    if color_count == 1 {
        return RadialGradientAction::ReturnSingleColor;
    }
    if has_lm && !lm_invertible {
        return RadialGradientAction::ReturnNull;
    }
    if scalar_nearly_zero(radius, DEGENERATE_THRESHOLD) {
        return RadialGradientAction::ReturnDegenerate;
    }
    RadialGradientAction::Create
}

// ============================================================================
// SkSweepGradient
// ============================================================================

/// Matches `SkSweepGradient` constructor matrix (`SkMatrix::Translate(-center.x(), -center.y())`).
#[inline]
pub fn sweep_to_unit_matrix(center: Point) -> Matrix {
    Matrix::translate(-center.x, -center.y)
}

/// Matches `angles_from_t_coeff(tBias, tScale)` in `SkSweepGradient.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SweepAngles {
    pub start_angle: f32,
    pub end_angle: f32,
}

#[inline]
pub fn sweep_angles_from_t_coeff(t_bias: f32, t_scale: f32) -> SweepAngles {
    SweepAngles {
        start_angle: -t_bias * 360.0,
        end_angle: ((1.0_f32 / t_scale) - t_bias) * 360.0,
    }
}

/// Computes `fTBias = -t0`, `fTScale = 1 / (t1 - t0)`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SweepTCoeff {
    pub t_bias: f32,
    pub t_scale: f32,
}

#[inline]
pub fn sweep_t_coeff_from_t(t0: f32, t1: f32) -> SweepTCoeff {
    SweepTCoeff {
        t_bias: -t0,
        t_scale: 1.0_f32 / (t1 - t0),
    }
}

/// Computes `SkMatrix::Scale(fTScale, 1) * SkMatrix::Translate(fTBias, 0)`.
#[inline]
pub fn sweep_stage_matrix(t_bias: f32, t_scale: f32) -> Matrix {
    let mut out = Matrix::identity();
    let s = Matrix::scale_translate(t_scale, 1.0, 0.0, 0.0);
    let t = Matrix::translate(t_bias, 0.0);
    out.set_concat(&s, &t);
    out
}

/// Result of `classify_sweep_gradient`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SweepGradientPlan {
    /// 0 = ReturnNull, 1 = ReturnSingleColor, 2 = ReturnDegenerate,
    /// 3 = RecurseClampedHardStop (0..endAngle with 3 stops [front, front, back] @ [0, 1, 1]),
    /// 4 = Create(t0, t1, effective_tile_mode)
    pub action: i32,
    pub t0: f32,
    pub t1: f32,
    pub effective_tile_mode: u32,
}

/// Exact port of `SkShaders::SweepGradient` factory logic.
pub fn classify_sweep_gradient(
    start_angle: f32,
    end_angle: f32,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> SweepGradientPlan {
    let null_plan = SweepGradientPlan {
        action: 0,
        t0: 0.0,
        t1: 0.0,
        effective_tile_mode: tile_mode,
    };
    if !start_angle.is_finite() || !end_angle.is_finite() || start_angle > end_angle {
        return null_plan;
    }
    if !valid_gradient(has_colors_data, color_count, tile_mode, interp) {
        return null_plan;
    }
    if color_count == 1 {
        return SweepGradientPlan { action: 1, ..null_plan };
    }
    if has_lm && !lm_invertible {
        return null_plan;
    }

    let mut mode = tile_mode;
    if scalar_nearly_equal(start_angle, end_angle, DEGENERATE_THRESHOLD) {
        if mode == (TileMode::Clamp as u32) && end_angle > DEGENERATE_THRESHOLD {
            return SweepGradientPlan {
                action: 3,
                t0: 0.0,
                t1: end_angle,
                effective_tile_mode: mode,
            };
        } else {
            return SweepGradientPlan { action: 2, ..null_plan };
        }
    }

    if start_angle <= 0.0 && end_angle >= 360.0 {
        mode = TileMode::Clamp as u32;
    }

    SweepGradientPlan {
        action: 4,
        t0: start_angle / 360.0,
        t1: end_angle / 360.0,
        effective_tile_mode: mode,
    }
}

// ============================================================================
// SkConicalGradient
// ============================================================================

/// Mirrors `SkConicalGradient::Type`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConicalType {
    Radial = 0,
    Strip = 1,
    Focal = 2,
}

/// Mirrors `SkConicalGradient::FocalData` (exact layout: 12 bytes).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct FocalData {
    pub r1: f32,
    pub focal_x: f32,
    pub is_swapped: bool,
}

impl FocalData {
    #[inline]
    pub fn is_focal_on_circle(&self) -> bool {
        scalar_nearly_zero(1.0 - self.r1, SK_SCALAR_NEARLY_ZERO)
    }

    #[inline]
    pub fn is_swapped(&self) -> bool {
        self.is_swapped
    }

    #[inline]
    pub fn is_well_behaved(&self) -> bool {
        !self.is_focal_on_circle() && self.r1 > 1.0
    }

    #[inline]
    pub fn is_natively_focal(&self) -> bool {
        scalar_nearly_zero(self.focal_x, SK_SCALAR_NEARLY_ZERO)
    }

    /// Exact port of `SkConicalGradient::FocalData::set(r0, r1, matrix)`.
    pub fn set(&mut self, mut r0: f32, mut r1: f32, matrix: &mut Matrix) -> bool {
        self.is_swapped = false;
        self.focal_x = r0 / (r0 - r1);
        if scalar_nearly_zero(self.focal_x - 1.0, SK_SCALAR_NEARLY_ZERO) {
            // swap r0, r1
            matrix.post_translate(-1.0, 0.0);
            matrix.post_scale(-1.0, 1.0);
            core::mem::swap(&mut r0, &mut r1);
            self.focal_x = 0.0;
            self.is_swapped = true;
        }

        // Map {focal point, (1, 0)} to {(0, 0), (1, 0)}
        let from = [Point::new(self.focal_x, 0.0), Point::new(1.0, 0.0)];
        let to = [Point::new(0.0, 0.0), Point::new(1.0, 0.0)];
        let focal_matrix = match Matrix::poly_to_poly(&from, &to) {
            Some(m) => m,
            None => return false,
        };
        matrix.post_concat(&focal_matrix);
        self.r1 = r1 / (1.0 - self.focal_x).abs();

        if self.is_focal_on_circle() {
            matrix.post_scale(0.5, 0.5);
        } else {
            let denom = self.r1 * self.r1 - 1.0;
            matrix.post_scale(self.r1 / denom, 1.0 / denom.abs().sqrt());
        }
        let abs_one_minus_f = (1.0 - self.focal_x).abs();
        matrix.post_scale(abs_one_minus_f, abs_one_minus_f);
        true
    }
}

/// Exact port of `SkConicalGradient::MapToUnitX`.
#[inline]
pub fn conical_map_to_unit_x(start_center: Point, end_center: Point) -> Option<Matrix> {
    let centers = [start_center, end_center];
    let unitvec = [Point::new(0.0, 0.0), Point::new(1.0, 0.0)];
    Matrix::poly_to_poly(&centers, &unitvec)
}

/// Result of `conical_create_params`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ConicalCreateResult {
    pub valid: bool,
    pub conical_type: ConicalType,
    pub gradient_matrix: Matrix,
    pub focal_data: FocalData,
}

/// Exact port of `SkConicalGradient::Create` geometry computation.
pub fn conical_create_params(c0: Point, r0: f32, c1: Point, r1: f32) -> ConicalCreateResult {
    let invalid = ConicalCreateResult {
        valid: false,
        conical_type: ConicalType::Radial,
        gradient_matrix: Matrix::identity(),
        focal_data: FocalData::default(),
    };

    let d_center = (c0 - c1).mag();
    let (mut gradient_matrix, gradient_type) =
        if scalar_nearly_zero(d_center, SK_SCALAR_NEARLY_ZERO) {
            let max_r = cpp_max_f32(r0, r1);
            if scalar_nearly_zero(max_r, SK_SCALAR_NEARLY_ZERO)
                || scalar_nearly_equal(r0, r1, SK_SCALAR_NEARLY_ZERO)
            {
                return invalid;
            }
            let scale = 1.0_f32 / max_r;
            let mut m = Matrix::translate(-c1.x, -c1.y);
            m.post_scale(scale, scale);
            (m, ConicalType::Radial)
        } else {
            let mx = match conical_map_to_unit_x(c0, c1) {
                Some(m) => m,
                None => return invalid,
            };
            let ty = if scalar_nearly_zero(r1 - r0, SK_SCALAR_NEARLY_ZERO) {
                ConicalType::Strip
            } else {
                ConicalType::Focal
            };
            (mx, ty)
        };

    let mut focal_data = FocalData::default();
    if gradient_type == ConicalType::Focal
        && !focal_data.set(r0 / d_center, r1 / d_center, &mut gradient_matrix)
    {
        return invalid;
    }

    ConicalCreateResult {
        valid: true,
        conical_type: gradient_type,
        gradient_matrix,
        focal_data,
    }
}

/// Action returned by `classify_two_point_conical`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TwoPointConicalAction {
    ReturnNull = 0,
    /// Clamped degenerate ring: call RadialGradient(start, endRadius, [front, front, back] @ [0, 1, 1])
    DegenerateClampedRing = 1,
    /// Call MakeDegenerateGradient
    ReturnDegenerate = 2,
    /// Call RadialGradient(start, endRadius, grad, lm)
    DelegateToRadial = 3,
    /// Proceed to SkConicalGradient::Create (duplicating 1 color to 2 if color_count == 1)
    CreateConical = 4,
}

/// Exact port of `SkShaders::TwoPointConicalGradient` decision logic.
pub fn classify_two_point_conical(
    start: Point,
    start_radius: f32,
    end: Point,
    end_radius: f32,
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
    has_lm: bool,
    lm_invertible: bool,
) -> TwoPointConicalAction {
    if start_radius < 0.0 || end_radius < 0.0 {
        return TwoPointConicalAction::ReturnNull;
    }
    if !valid_gradient(has_colors_data, color_count, tile_mode, interp) {
        return TwoPointConicalAction::ReturnNull;
    }
    if has_lm && !lm_invertible {
        return TwoPointConicalAction::ReturnNull;
    }

    if scalar_nearly_zero((start - end).mag(), DEGENERATE_THRESHOLD) {
        if scalar_nearly_equal(start_radius, end_radius, DEGENERATE_THRESHOLD) {
            if tile_mode == (TileMode::Clamp as u32) && end_radius > DEGENERATE_THRESHOLD {
                return TwoPointConicalAction::DegenerateClampedRing;
            } else {
                return TwoPointConicalAction::ReturnDegenerate;
            }
        } else if scalar_nearly_zero(start_radius, DEGENERATE_THRESHOLD) {
            return TwoPointConicalAction::DelegateToRadial;
        }
    }

    TwoPointConicalAction::CreateConical
}

/// Computes stage matrix for concentric conical (`fType == Type::kRadial`):
/// `SkMatrix::Translate(-r1 / dRadius, 0) * SkMatrix::Scale(max(r1, r2) / dRadius, 1)`.
#[inline]
pub fn conical_radial_stage_matrix(r1: f32, r2: f32) -> Matrix {
    let d_radius = r2 - r1;
    let scale = cpp_max_f32(r1, r2) / d_radius;
    let bias = -r1 / d_radius;
    let t = Matrix::translate(bias, 0.0);
    let s = Matrix::scale_translate(scale, 1.0, 0.0, 0.0);
    let mut out = Matrix::identity();
    out.set_concat(&t, &s);
    out
}

/// Computes `ctx->fP0` for strip conical (`fType == Type::kStrip`).
#[inline]
pub fn conical_strip_p0(r1: f32, c1: Point, c2: Point) -> f32 {
    let center_x1 = (c1 - c2).mag();
    let scaled_r0 = r1 / center_x1;
    scaled_r0 * scaled_r0
}

/// Primary focal stage op kind for `SkConicalGradient::appendGradientStages`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConicalFocalOp {
    FocalOnCircle = 0,
    WellBehaved = 1,
    Smaller = 2,
    Greater = 3,
}

/// Stage plan for `SkConicalGradient::appendGradientStages` when `fType == Type::kFocal`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ConicalFocalStagePlan {
    pub p0: f32,
    pub p1: f32,
    pub focal_op: ConicalFocalOp,
    pub mask_degenerates: bool,
    pub negate_x: bool,
    pub compensate_focal: bool,
    pub unswap: bool,
    pub apply_post_mask: bool,
}

/// Exact port of the focal stage planning in `SkConicalGradient::appendGradientStages`.
pub fn plan_conical_focal_stages(fd: &FocalData) -> ConicalFocalStagePlan {
    let focal_on_circle = fd.is_focal_on_circle();
    let well_behaved = fd.is_well_behaved();
    let focal_op = if focal_on_circle {
        ConicalFocalOp::FocalOnCircle
    } else if well_behaved {
        ConicalFocalOp::WellBehaved
    } else if fd.is_swapped() || (1.0 - fd.focal_x) < 0.0 {
        ConicalFocalOp::Smaller
    } else {
        ConicalFocalOp::Greater
    };

    ConicalFocalStagePlan {
        p0: 1.0_f32 / fd.r1,
        p1: fd.focal_x,
        focal_op,
        mask_degenerates: !well_behaved,
        negate_x: (1.0 - fd.focal_x) < 0.0,
        compensate_focal: !fd.is_natively_focal(),
        unswap: fd.is_swapped(),
        apply_post_mask: !well_behaved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_unit_matrix_maps_endpoints_to_0_and_1() {
        let p0 = Point::new(10.0, 20.0);
        let p1 = Point::new(40.0, 60.0); // len = 50
        let m = linear_pts_to_unit_matrix(p0, p1);
        let mut pts = [p0, p1];
        m.map_points_in_place(&mut pts);
        assert!((pts[0].x - 0.0).abs() < 1e-5);
        assert!((pts[0].y - 0.0).abs() < 1e-5);
        assert!((pts[1].x - 1.0).abs() < 1e-5);
        assert!((pts[1].y - 0.0).abs() < 1e-5);
    }

    #[test]
    fn radial_unit_matrix() {
        let m = radial_to_unit_matrix(Point::new(10.0, 20.0), 5.0);
        let mut pts = [Point::new(10.0, 20.0), Point::new(15.0, 20.0)];
        m.map_points_in_place(&mut pts);
        assert!((pts[0].x).abs() < 1e-6 && (pts[0].y).abs() < 1e-6);
        assert!((pts[1].x - 1.0).abs() < 1e-6 && (pts[1].y).abs() < 1e-6);
    }

    #[test]
    fn sweep_angles_and_classification() {
        let coeff = sweep_t_coeff_from_t(0.25, 0.75);
        assert_eq!(coeff.t_bias, -0.25);
        assert_eq!(coeff.t_scale, 2.0);
        let angles = sweep_angles_from_t_coeff(coeff.t_bias, coeff.t_scale);
        assert!((angles.start_angle - 90.0).abs() < 1e-5);
        assert!((angles.end_angle - 270.0).abs() < 1e-5);

        let interp = GradientInterpolation::default();
        // Full [0, 360] converts Repeat -> Clamp
        let full = classify_sweep_gradient(0.0, 360.0, true, 2, TileMode::Repeat as u32, interp, false, true);
        assert_eq!(full.action, 4);
        assert_eq!(full.effective_tile_mode, TileMode::Clamp as u32);

        // Degenerate clamped with endAngle > threshold -> hard stop recursion
        let hard = classify_sweep_gradient(45.0, 45.0, true, 2, TileMode::Clamp as u32, interp, false, true);
        assert_eq!(hard.action, 3);
    }

    #[test]
    fn conical_types_and_focal_data() {
        assert_eq!(core::mem::size_of::<FocalData>(), 12);

        // Concentric -> Radial
        let conc = conical_create_params(Point::new(5.0, 5.0), 2.0, Point::new(5.0, 5.0), 10.0);
        assert!(conc.valid);
        assert_eq!(conc.conical_type, ConicalType::Radial);

        // Equal radii, different centers -> Strip
        let strip = conical_create_params(Point::new(0.0, 0.0), 5.0, Point::new(10.0, 0.0), 5.0);
        assert!(strip.valid);
        assert_eq!(strip.conical_type, ConicalType::Strip);

        // Well-behaved focal: small circle strictly inside large circle
        let focal = conical_create_params(Point::new(0.0, 0.0), 1.0, Point::new(2.0, 0.0), 10.0);
        assert!(focal.valid);
        assert_eq!(focal.conical_type, ConicalType::Focal);
        assert!(focal.focal_data.is_well_behaved());
        let plan = plan_conical_focal_stages(&focal.focal_data);
        assert_eq!(plan.focal_op, ConicalFocalOp::WellBehaved);
        assert!(!plan.mask_degenerates);
        assert!(!plan.apply_post_mask);

        // Swapped focal: r1 == 0, r0 > 0 -> focal_x == 1 -> swapped!
        let swapped = conical_create_params(Point::new(0.0, 0.0), 5.0, Point::new(10.0, 0.0), 0.0);
        assert!(swapped.valid);
        assert_eq!(swapped.conical_type, ConicalType::Focal);
        assert!(swapped.focal_data.is_swapped());
        assert_eq!(swapped.focal_data.focal_x, 0.0);
        let swap_plan = plan_conical_focal_stages(&swapped.focal_data);
        assert!(swap_plan.unswap);

        // Focal on circle: circle0 at (0,0) r=0, circle1 at (5,0) r=5 -> focal at (0,0), fR1 = 1
        let on_circle = conical_create_params(Point::new(0.0, 0.0), 0.0, Point::new(5.0, 0.0), 5.0);
        assert!(on_circle.valid);
        assert!(on_circle.focal_data.is_focal_on_circle());
        let oc_plan = plan_conical_focal_stages(&on_circle.focal_data);
        assert_eq!(oc_plan.focal_op, ConicalFocalOp::FocalOnCircle);
        assert!(oc_plan.mask_degenerates);
        assert!(oc_plan.apply_post_mask);

        // Greater case: r0=0, r1=2, dist=10 -> focal_x=0, fR1=0.2 < 1, not swapped, 1-f > 0
        let greater = conical_create_params(Point::new(0.0, 0.0), 0.0, Point::new(10.0, 0.0), 2.0);
        assert!(greater.valid);
        let g_plan = plan_conical_focal_stages(&greater.focal_data);
        assert_eq!(g_plan.focal_op, ConicalFocalOp::Greater);
        assert!(!g_plan.negate_x);
        assert!(!g_plan.compensate_focal);

        // Smaller case (unswapped, 1 - focal_x < 0): r0=4, r1=2, dist=10 -> focal_x=2 > 1
        let smaller = conical_create_params(Point::new(0.0, 0.0), 4.0, Point::new(10.0, 0.0), 2.0);
        assert!(smaller.valid);
        let s_plan = plan_conical_focal_stages(&smaller.focal_data);
        assert_eq!(s_plan.focal_op, ConicalFocalOp::Smaller);
        assert!(s_plan.negate_x);
        assert!(s_plan.compensate_focal);
    }

    #[test]
    fn factory_classifications_edge_cases() {
        let interp = GradientInterpolation::default();

        // Linear: NaN / degenerate / single color
        assert_eq!(
            classify_linear_gradient(true, Point::new(0.0, 0.0), Point::new(f32::NAN, 0.0), true, 2, 0, interp, false, true),
            LinearGradientAction::ReturnNull
        );
        assert_eq!(
            classify_linear_gradient(true, Point::new(1.0, 1.0), Point::new(1.0, 1.0), true, 2, 0, interp, false, true),
            LinearGradientAction::ReturnDegenerate
        );
        assert_eq!(
            classify_linear_gradient(true, Point::new(0.0, 0.0), Point::new(10.0, 0.0), true, 1, 0, interp, false, true),
            LinearGradientAction::ReturnSingleColor
        );

        // Radial: negative / zero / valid
        assert_eq!(
            classify_radial_gradient(-1.0, true, 2, 0, interp, false, true),
            RadialGradientAction::ReturnNull
        );
        assert_eq!(
            classify_radial_gradient(0.0, true, 2, 0, interp, false, true),
            RadialGradientAction::ReturnDegenerate
        );

        // TwoPointConical: concentric cases
        assert_eq!(
            classify_two_point_conical(Point::new(0.0, 0.0), 5.0, Point::new(0.0, 0.0), 5.0, true, 2, TileMode::Clamp as u32, interp, false, true),
            TwoPointConicalAction::DegenerateClampedRing
        );
        assert_eq!(
            classify_two_point_conical(Point::new(0.0, 0.0), 5.0, Point::new(0.0, 0.0), 5.0, true, 2, TileMode::Repeat as u32, interp, false, true),
            TwoPointConicalAction::ReturnDegenerate
        );
        assert_eq!(
            classify_two_point_conical(Point::new(0.0, 0.0), 0.0, Point::new(0.0, 0.0), 10.0, true, 2, TileMode::Clamp as u32, interp, false, true),
            TwoPointConicalAction::DelegateToRadial
        );
        assert_eq!(
            classify_two_point_conical(Point::new(0.0, 0.0), 2.0, Point::new(0.0, 0.0), 10.0, true, 2, TileMode::Clamp as u32, interp, false, true),
            TwoPointConicalAction::CreateConical
        );

        // Strip p0 and radial stage matrix
        assert!((conical_strip_p0(3.0, Point::new(0.0, 0.0), Point::new(6.0, 0.0)) - 0.25).abs() < 1e-6);
        let rm = conical_radial_stage_matrix(2.0, 10.0);
        // scale = 10/8 = 1.25, bias = -2/8 = -0.25
        assert!((rm.mat[0] - 1.25).abs() < 1e-6);
        assert!((rm.mat[2] - (-0.25)).abs() < 1e-6);
    }
}
