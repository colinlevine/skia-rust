//! Safe Rust port of `SkColorFilterShader.cpp`, `SkCoordClampShader.cpp`,
//! `SkWorkingColorSpaceShader.cpp`, `SkTransformShader.cpp`, and `SkTriColorShader.cpp`.

use skia_rs_color::color::Color4f;
use skia_rs_color::image_info::AlphaType;
use skia_rs_geometry::{Matrix, Point, Rect};

// ============================================================================
// SkColorFilterShader
// ============================================================================

/// Action returned by `classify_color_filter_shader`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColorFilterShaderAction {
    ReturnNull = 0,
    ReturnShader = 1,
    Create = 2,
}

#[inline]
pub const fn classify_color_filter_shader(
    has_shader: bool,
    has_filter: bool,
) -> ColorFilterShaderAction {
    if !has_shader {
        ColorFilterShaderAction::ReturnNull
    } else if !has_filter {
        ColorFilterShaderAction::ReturnShader
    } else {
        ColorFilterShaderAction::Create
    }
}

#[inline]
pub fn color_filter_shader_is_opaque(
    shader_is_opaque: bool,
    alpha: f32,
    filter_alpha_unchanged: bool,
) -> bool {
    shader_is_opaque && alpha == 1.0 && filter_alpha_unchanged
}

/// Stage plan for `SkColorFilterShader::appendStages`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ColorFilterShaderStagePlan {
    pub append_scale_alpha: bool,
    pub filter_shader_is_opaque: bool,
}

#[inline]
pub fn plan_color_filter_shader_stages(
    alpha: f32,
    shader_is_opaque: bool,
) -> ColorFilterShaderStagePlan {
    ColorFilterShaderStagePlan {
        append_scale_alpha: alpha != 1.0,
        filter_shader_is_opaque: alpha == 1.0 && shader_is_opaque,
    }
}

// ============================================================================
// SkCoordClampShader
// ============================================================================

/// Validates `SkShaders::CoordClamp(shader, subset)`.
#[inline]
pub fn coord_clamp_is_valid(has_shader: bool, subset: Rect) -> bool {
    has_shader && subset.is_sorted()
}

/// Mirrors `SkRasterPipelineContexts::CoordClampCtx`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CoordClampCtx {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[inline]
pub const fn make_coord_clamp_ctx(subset: Rect) -> CoordClampCtx {
    CoordClampCtx {
        min_x: subset.left,
        min_y: subset.top,
        max_x: subset.right,
        max_y: subset.bottom,
    }
}

// ============================================================================
// SkWorkingColorSpaceShader
// ============================================================================

/// Action returned by `classify_working_color_space_shader`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorkingColorSpaceAction {
    ReturnNull = 0,
    ReturnShader = 1,
    Create = 2,
}

#[inline]
pub const fn classify_working_color_space_shader(
    has_shader: bool,
    has_input_cs: bool,
    has_output_cs: bool,
    work_in_unpremul: bool,
) -> WorkingColorSpaceAction {
    if !has_shader {
        WorkingColorSpaceAction::ReturnNull
    } else if !has_input_cs && !has_output_cs && !work_in_unpremul {
        WorkingColorSpaceAction::ReturnShader
    } else {
        WorkingColorSpaceAction::Create
    }
}

#[inline]
pub const fn resolve_working_alpha_type(work_in_unpremul: bool, dst_at: AlphaType) -> AlphaType {
    if work_in_unpremul {
        AlphaType::Unpremul
    } else {
        dst_at
    }
}

#[inline]
pub const fn make_opaque_color4f(c: Color4f) -> Color4f {
    Color4f {
        r: c.r,
        g: c.g,
        b: c.b,
        a: 1.0,
    }
}

// ============================================================================
// SkTransformShader
// ============================================================================

/// Matches `SkTransformShader::update(const SkMatrix&)`.
/// Returns `Some([f32; 9])` to store into `fMatrixStorage`, or `None` if perspective is disallowed.
#[inline]
pub fn transform_shader_update(allow_perspective: bool, matrix: &Matrix) -> Option<[f32; 9]> {
    if !allow_perspective && matrix.has_perspective() {
        return None;
    }
    Some(matrix.mat)
}

// ============================================================================
// SkTriColorShader
// ============================================================================

/// Column-major 4x3 matrix matching `SkTriColorShader::Matrix43`.
#[inline]
pub fn matrix43_concat(a: [f32; 12], b: &Matrix) -> [f32; 12] {
    let dot = |index: usize, x: f32, y: f32| -> f32 { a[index] * x + a[index + 4] * y };
    let sx = b.mat[0];
    let ky = b.mat[3];
    let kx = b.mat[1];
    let sy = b.mat[4];
    let tx = b.mat[2];
    let ty = b.mat[5];

    [
        dot(0, sx, ky),
        dot(1, sx, ky),
        dot(2, sx, ky),
        dot(3, sx, ky),
        dot(0, kx, sy),
        dot(1, kx, sy),
        dot(2, kx, sy),
        dot(3, kx, sy),
        dot(0, tx, ty) + a[8],
        dot(1, tx, ty) + a[9],
        dot(2, tx, ty) + a[10],
        dot(3, tx, ty) + a[11],
    ]
}

/// Result of `tri_color_update`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TriColorUpdateResult {
    pub m33: Matrix,
    pub m43: [f32; 12],
    pub valid: bool,
}

/// Matches `SkTriColorShader::update`.
pub fn tri_color_update(
    use_persp: bool,
    ctm_inv: &Matrix,
    p0: Point,
    p1: Point,
    p2: Point,
    c0: Color4f,
    c1: Color4f,
    c2: Color4f,
) -> TriColorUpdateResult {
    let mut m = Matrix::identity();
    m.set9(&[
        p1.x - p0.x,
        p2.x - p0.x,
        p0.x,
        p1.y - p0.y,
        p2.y - p0.y,
        p0.y,
        0.0,
        0.0,
        1.0,
    ]);
    let im = match m.invert() {
        Some(im) => im,
        None => {
            return TriColorUpdateResult {
                m33: Matrix::identity(),
                m43: [0.0; 12],
                valid: false,
            };
        }
    };

    let mut m33 = Matrix::identity();
    m33.set_concat(&im, ctm_inv);

    let mut m43 = [
        c1.r - c0.r,
        c1.g - c0.g,
        c1.b - c0.b,
        c1.a - c0.a,
        c2.r - c0.r,
        c2.g - c0.g,
        c2.b - c0.b,
        c2.a - c0.a,
        c0.r,
        c0.g,
        c0.b,
        c0.a,
    ];

    if !use_persp {
        m43 = matrix43_concat(m43, &m33);
    }

    TriColorUpdateResult {
        m33,
        m43,
        valid: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_filter_shader_rules() {
        assert_eq!(
            classify_color_filter_shader(false, true),
            ColorFilterShaderAction::ReturnNull
        );
        assert_eq!(
            classify_color_filter_shader(true, false),
            ColorFilterShaderAction::ReturnShader
        );
        assert_eq!(
            classify_color_filter_shader(true, true),
            ColorFilterShaderAction::Create
        );
        assert!(color_filter_shader_is_opaque(true, 1.0, true));
        assert!(!color_filter_shader_is_opaque(true, 0.5, true));
        let plan = plan_color_filter_shader_stages(0.5, true);
        assert!(plan.append_scale_alpha);
        assert!(!plan.filter_shader_is_opaque);
    }

    #[test]
    fn coord_clamp_and_working_cs() {
        assert!(coord_clamp_is_valid(true, Rect::from_ltrb(0.0, 0.0, 10.0, 10.0)));
        assert!(!coord_clamp_is_valid(true, Rect::from_ltrb(10.0, 0.0, 0.0, 10.0)));
        assert!(!coord_clamp_is_valid(false, Rect::from_ltrb(0.0, 0.0, 10.0, 10.0)));

        assert_eq!(
            classify_working_color_space_shader(true, false, false, false),
            WorkingColorSpaceAction::ReturnShader
        );
        assert_eq!(
            classify_working_color_space_shader(true, false, false, true),
            WorkingColorSpaceAction::Create
        );
    }

    #[test]
    fn tri_color_update_unit_triangle() {
        let res = tri_color_update(
            false,
            &Matrix::identity(),
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(0.0, 1.0),
            Color4f::new(0.0, 0.0, 0.0, 1.0),
            Color4f::new(1.0, 0.0, 0.0, 1.0),
            Color4f::new(0.0, 1.0, 0.0, 1.0),
        );
        assert!(res.valid);
        assert!((res.m43[0] - 1.0).abs() < 1e-6);
        assert!((res.m43[5] - 1.0).abs() < 1e-6);
        assert!((res.m43[11] - 1.0).abs() < 1e-6);

        // Degenerate collinear triangle fails.
        let deg = tri_color_update(
            false,
            &Matrix::identity(),
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Color4f::new(0.0, 0.0, 0.0, 1.0),
            Color4f::new(1.0, 0.0, 0.0, 1.0),
            Color4f::new(0.0, 1.0, 0.0, 1.0),
        );
        assert!(!deg.valid);
    }

    #[test]
    fn transform_shader_perspective_check() {
        let mut persp = Matrix::identity();
        persp.set9(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.01, 0.0, 1.0]);
        assert!(transform_shader_update(false, &persp).is_none());
        assert!(transform_shader_update(true, &persp).is_some());
    }
}
