//! Safe Rust port of `src/effects/colorfilters/SkMatrixColorFilter.cpp`.

use skia_rs_geometry::{
    all_finite, scalar_nearly_equal, scalar_nearly_zero, SK_SCALAR_NEARLY_ZERO,
};

/// Picture version where `SkMatrixColorFilter` began serializing the `clamp` boolean.
pub const UNCLAMPED_MATRIX_COLOR_FILTER_VERSION: u32 = 105;

/// Matches `SkMatrixColorFilter::Domain` (`enum class Domain : uint8_t`).
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatrixDomain {
    Rgba = 0,
    Hsla = 1,
}

impl MatrixDomain {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Rgba),
            1 => Some(Self::Hsla),
            _ => None,
        }
    }
}

/// Matches `SkColorFilters::Clamp` (`enum class Clamp : bool`).
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatrixClamp {
    No = 0,
    Yes = 1,
}

impl MatrixClamp {
    #[inline]
    pub const fn from_bool(b: bool) -> Self {
        if b {
            Self::Yes
        } else {
            Self::No
        }
    }

    #[inline]
    pub const fn as_bool(self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// Exact port of `is_alpha_unchanged(const float matrix[20])` in `SkMatrixColorFilter.cpp`.
#[inline]
pub fn is_alpha_unchanged(matrix: &[f32; 20]) -> bool {
    scalar_nearly_zero(matrix[15], SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_zero(matrix[16], SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_zero(matrix[17], SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_equal(matrix[18], 1.0_f32, SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_zero(matrix[19], SK_SCALAR_NEARLY_ZERO)
}

/// Exact port of `SkIsFinite(array, 20)` used by `MakeMatrix`.
#[inline]
pub fn is_valid_matrix(matrix: &[f32; 20]) -> bool {
    all_finite(matrix)
}

/// Stage plan computed by `SkMatrixColorFilter::appendStages`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MatrixStagePlan {
    pub unpremul: bool,
    pub hsla: bool,
    /// `true` => append `clamp_01`; `false` => append `clamp_a_01`.
    pub clamp_all: bool,
    pub premul: bool,
}

/// Exact port of the stage selection logic in `SkMatrixColorFilter::appendStages`.
#[inline]
pub const fn plan_matrix_stages(
    shader_is_opaque: bool,
    alpha_is_unchanged: bool,
    domain: MatrixDomain,
    clamp: MatrixClamp,
) -> MatrixStagePlan {
    let will_stay_opaque = shader_is_opaque && alpha_is_unchanged;
    let hsla = matches!(domain, MatrixDomain::Hsla);
    let clamp_all = matches!(clamp, MatrixClamp::Yes);
    MatrixStagePlan {
        unpremul: !shader_is_opaque,
        hsla,
        clamp_all,
        premul: !will_stay_opaque,
    }
}

/// Pure-Rust representation of `SkMatrixColorFilter`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MatrixColorFilter {
    pub matrix: [f32; 20],
    pub alpha_is_unchanged: bool,
    pub domain: MatrixDomain,
    pub clamp: MatrixClamp,
}

impl MatrixColorFilter {
    /// Validate finiteness and construct a [`MatrixColorFilter`].
    pub fn make(matrix: [f32; 20], domain: MatrixDomain, clamp: MatrixClamp) -> Option<Self> {
        if !is_valid_matrix(&matrix) {
            return None;
        }
        Some(Self {
            alpha_is_unchanged: is_alpha_unchanged(&matrix),
            matrix,
            domain,
            clamp,
        })
    }

    pub fn make_rgba(matrix: [f32; 20], clamp: MatrixClamp) -> Option<Self> {
        Self::make(matrix, MatrixDomain::Rgba, clamp)
    }

    pub fn make_hsla(matrix: [f32; 20]) -> Option<Self> {
        Self::make(matrix, MatrixDomain::Hsla, MatrixClamp::Yes)
    }

    #[inline]
    pub const fn stage_plan(&self, shader_is_opaque: bool) -> MatrixStagePlan {
        plan_matrix_stages(
            shader_is_opaque,
            self.alpha_is_unchanged,
            self.domain,
            self.clamp,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color_matrix::ColorMatrix;

    #[test]
    fn alpha_unchanged_detection() {
        let id = ColorMatrix::identity().mat;
        assert!(is_alpha_unchanged(&id));

        let mut mod_a_scale = id;
        mod_a_scale[18] = 0.5;
        assert!(!is_alpha_unchanged(&mod_a_scale));

        let mut mod_a_trans = id;
        mod_a_trans[19] = 0.1;
        assert!(!is_alpha_unchanged(&mod_a_trans));

        let mut mod_a_from_r = id;
        mod_a_from_r[15] = 0.1;
        assert!(!is_alpha_unchanged(&mod_a_from_r));

        let mut nearly_id = id;
        nearly_id[18] = 1.0 - 1e-5;
        nearly_id[19] = 1e-5;
        assert!(is_alpha_unchanged(&nearly_id));
    }

    #[test]
    fn non_finite_matrix_rejected() {
        let mut m = ColorMatrix::identity().mat;
        assert!(MatrixColorFilter::make_rgba(m, MatrixClamp::Yes).is_some());

        m[7] = f32::NAN;
        assert!(MatrixColorFilter::make_rgba(m, MatrixClamp::Yes).is_none());

        m[7] = f32::INFINITY;
        assert!(MatrixColorFilter::make_hsla(m).is_none());
    }

    #[test]
    fn stage_planning_combinations() {
        // Opaque + alpha unchanged + RGBA + Clamp::Yes => no unpremul, no premul, clamp_01.
        let p = plan_matrix_stages(true, true, MatrixDomain::Rgba, MatrixClamp::Yes);
        assert_eq!(
            p,
            MatrixStagePlan {
                unpremul: false,
                hsla: false,
                clamp_all: true,
                premul: false,
            }
        );

        // Opaque + alpha changed + RGBA + Clamp::No => no unpremul, premul needed, clamp_a_01.
        let p2 = plan_matrix_stages(true, false, MatrixDomain::Rgba, MatrixClamp::No);
        assert_eq!(
            p2,
            MatrixStagePlan {
                unpremul: false,
                hsla: false,
                clamp_all: false,
                premul: true,
            }
        );

        // Non-opaque + alpha unchanged + HSLA + Clamp::Yes => unpremul, hsla, clamp_01, premul.
        let p3 = plan_matrix_stages(false, true, MatrixDomain::Hsla, MatrixClamp::Yes);
        assert_eq!(
            p3,
            MatrixStagePlan {
                unpremul: true,
                hsla: true,
                clamp_all: true,
                premul: true,
            }
        );
    }
}
