//! Safe Rust port of `SkShader.cpp`, `SkShaderBase.cpp`, `SkColorShader.cpp`,
//! `SkEmptyShader.cpp`, and `SkLocalMatrixShader.cpp`.

use skia_rs_color::color::Color4f;
use skia_rs_geometry::{all_finite, Matrix};

/// Mirrors `SkShaders::MatrixRec` in `src/shaders/SkShaderBase.h` (exact layout: 124 bytes).
///
/// Layout check:
/// - `ctm`: `Matrix` (40 bytes)
/// - `total_local_matrix`: `Matrix` (40 bytes)
/// - `pending_local_matrix`: `Matrix` (40 bytes)
/// - `total_matrix_is_valid`: `bool` (1 byte)
/// - `ctm_applied`: `bool` (1 byte)
/// - 2 bytes trailing padding -> total 124 bytes (align 4).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MatrixRec {
    pub ctm: Matrix,
    pub total_local_matrix: Matrix,
    pub pending_local_matrix: Matrix,
    pub total_matrix_is_valid: bool,
    pub ctm_applied: bool,
}

/// Result of `MatrixRec::apply`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MatrixRecApplyResult {
    pub valid: bool,
    pub seed_shader: bool,
    pub total_matrix: Matrix,
    pub new_rec: MatrixRec,
}

/// Result of `MatrixRec::applyForFragmentProcessor`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MatrixRecFpResult {
    pub total_matrix: Matrix,
    pub valid: bool,
}

/// Matches `SkShaderBase::ConcatLocalMatrices`.
#[inline]
pub fn concat_local_matrices(
    parent_lm: &Matrix,
    child_lm: &Matrix,
    android_framework_order: bool,
) -> Matrix {
    let mut out = Matrix::identity();
    if android_framework_order {
        out.set_concat(child_lm, parent_lm);
    } else {
        out.set_concat(parent_lm, child_lm);
    }
    out
}

impl MatrixRec {
    #[inline]
    pub fn from_ctm(ctm: Matrix) -> Self {
        Self {
            ctm,
            total_local_matrix: Matrix::identity(),
            pending_local_matrix: Matrix::identity(),
            total_matrix_is_valid: true,
            ctm_applied: false,
        }
    }

    /// Matches `SkShaders::MatrixRec::apply(rec, postInv)`.
    pub fn apply(&self, post_inv: &Matrix) -> MatrixRecApplyResult {
        let mut total = self.pending_local_matrix;
        if !self.ctm_applied {
            let mut combined = Matrix::identity();
            combined.set_concat(&self.ctm, &total);
            total = combined;
        }
        let inv = match total.invert() {
            Some(inv) => inv,
            None => {
                return MatrixRecApplyResult {
                    valid: false,
                    seed_shader: false,
                    total_matrix: Matrix::identity(),
                    new_rec: *self,
                };
            }
        };
        let mut final_mat = Matrix::identity();
        final_mat.set_concat(post_inv, &inv);

        let new_rec = MatrixRec {
            ctm: self.ctm,
            total_local_matrix: self.total_local_matrix,
            pending_local_matrix: Matrix::identity(),
            total_matrix_is_valid: self.total_matrix_is_valid,
            ctm_applied: true,
        };
        MatrixRecApplyResult {
            valid: true,
            seed_shader: !self.ctm_applied,
            total_matrix: final_mat,
            new_rec,
        }
    }

    /// Matches `SkShaders::MatrixRec::applyForFragmentProcessor(postInv)`.
    pub fn apply_for_fragment_processor(&self, post_inv: &Matrix) -> MatrixRecFpResult {
        match self.pending_local_matrix.invert() {
            Some(inv) => {
                let mut out = Matrix::identity();
                out.set_concat(post_inv, &inv);
                MatrixRecFpResult {
                    total_matrix: out,
                    valid: true,
                }
            }
            None => MatrixRecFpResult {
                total_matrix: Matrix::identity(),
                valid: false,
            },
        }
    }

    /// Matches `SkShaders::MatrixRec::applied()`.
    #[inline]
    pub fn applied(&self) -> Self {
        Self {
            ctm: self.ctm,
            total_local_matrix: self.total_local_matrix,
            pending_local_matrix: Matrix::identity(),
            total_matrix_is_valid: self.total_matrix_is_valid,
            ctm_applied: false,
        }
    }

    /// Matches `SkShaders::MatrixRec::concat(m)`.
    #[inline]
    pub fn concat(&self, m: &Matrix, android_framework_order: bool) -> Self {
        Self {
            ctm: self.ctm,
            total_local_matrix: concat_local_matrices(
                &self.total_local_matrix,
                m,
                android_framework_order,
            ),
            pending_local_matrix: concat_local_matrices(
                &self.pending_local_matrix,
                m,
                android_framework_order,
            ),
            total_matrix_is_valid: self.total_matrix_is_valid,
            ctm_applied: self.ctm_applied,
        }
    }

    /// Matches `SkShaders::MatrixRec::totalMatrix()`.
    #[inline]
    pub fn total_matrix(&self) -> Matrix {
        let mut out = Matrix::identity();
        out.set_concat(&self.ctm, &self.total_local_matrix);
        out
    }

    /// Matches `SkShaders::MatrixRec::totalInverse()`.
    #[inline]
    pub fn total_inverse(&self) -> Option<Matrix> {
        self.total_matrix().invert()
    }
}

/// Matches `SkShaderBase::asLuminanceColor`: forces alpha to 1.0 when `onAsLuminanceColor` succeeds.
#[inline]
pub fn finalize_luminance_color(mut color: Color4f) -> Color4f {
    color.a = 1.0;
    color
}

/// Matches `SkShaderBase::makeContext` matrix eligibility check (under legacy shader context).
#[inline]
pub fn context_matrix_is_eligible(total_matrix: &Matrix) -> bool {
    !total_matrix.has_perspective() && total_matrix.invert().is_some()
}

/// Validates and pins alpha for `SkShaders::Color(const SkColor4f&, sk_sp<SkColorSpace>)`.
/// Returns `None` if any component is non-finite.
#[inline]
pub fn color_shader_validate_and_pin(color: Color4f) -> Option<Color4f> {
    if !all_finite(&[color.r, color.g, color.b, color.a]) {
        return None;
    }
    Some(Color4f {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a.clamp(0.0, 1.0),
    })
}

/// Matches `SkColor4f::FromColor(SkColor)` for `SkShaders::Color(SkColor)`.
#[inline]
pub fn color_shader_from_sk_color(color: u32) -> Color4f {
    Color4f::from_color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_rec_layout_and_ops() {
        assert_eq!(core::mem::size_of::<MatrixRec>(), 124);

        let ctm = Matrix::scale_translate(2.0, 3.0, 10.0, 20.0);
        let rec = MatrixRec::from_ctm(ctm);
        assert!(!rec.ctm_applied);
        assert!(rec.total_matrix_is_valid);

        let lm = Matrix::translate(5.0, -5.0);
        let rec2 = rec.concat(&lm, false);
        assert_eq!(rec2.total_local_matrix.mat[2], 5.0);
        assert_eq!(rec2.pending_local_matrix.mat[5], -5.0);

        let res = rec2.apply(&Matrix::identity());
        assert!(res.valid);
        assert!(res.seed_shader);
        assert!(res.new_rec.ctm_applied);
        assert!(res.new_rec.pending_local_matrix.is_identity());

        // Second apply should not re-seed shader.
        let res2 = res.new_rec.apply(&Matrix::identity());
        assert!(res2.valid);
        assert!(!res2.seed_shader);
        assert!(res2.total_matrix.is_identity());
    }

    #[test]
    fn matrix_rec_singular_fails() {
        let singular = Matrix::scale_translate(0.0, 0.0, 0.0, 0.0);
        let rec = MatrixRec::from_ctm(singular);
        let res = rec.apply(&Matrix::identity());
        assert!(!res.valid);

        let fp = rec.concat(&singular, false).apply_for_fragment_processor(&Matrix::identity());
        assert!(!fp.valid);
    }

    #[test]
    fn color_shader_pin_and_non_finite() {
        assert!(color_shader_validate_and_pin(Color4f::new(f32::NAN, 0.0, 0.0, 1.0)).is_none());
        assert!(color_shader_validate_and_pin(Color4f::new(0.0, f32::INFINITY, 0.0, 1.0)).is_none());
        let pinned = color_shader_validate_and_pin(Color4f::new(1.5, -0.2, 0.5, 2.0)).unwrap();
        assert_eq!(pinned, Color4f::new(1.5, -0.2, 0.5, 1.0));
        let pinned_neg = color_shader_validate_and_pin(Color4f::new(0.1, 0.2, 0.3, -1.0)).unwrap();
        assert_eq!(pinned_neg.a, 0.0);
    }
}
