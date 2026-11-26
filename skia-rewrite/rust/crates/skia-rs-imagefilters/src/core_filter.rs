#![forbid(unsafe_code)]

//! Safe Rust logic for `SkImageFilter`, `SkImageFilter_Base`, and `SkLocalMatrixImageFilter`
//! (ported from `src/core/SkImageFilter.cpp` and `src/core/SkLocalMatrixImageFilter.cpp`).

use std::sync::atomic::{AtomicU64, Ordering};

use skia_rs_geometry::{IRect, Matrix, Rect};

use crate::types::{
    irect_intersect, is_valid_rect, matrix_map_rect, MatrixCapability,
};

/// Exact port of `next_image_filter_unique_id()` in `SkImageFilter.cpp`.
pub fn next_image_filter_unique_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// Exact port of `SkImageFilter_Base::Common::unflatten` count validation.
#[inline]
pub fn validate_common_input_count(count: i32, expected_count: i32) -> bool {
    count >= 0 && (expected_count < 0 || count == expected_count)
}

/// Result of validating deprecated crop rect flags in `SkImageFilter_Base::Common::unflatten`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LegacyCropResult {
    Invalid = 0,
    NoCrop = 1,
    HasCrop = 2,
}

/// Exact port of legacy crop rect validation in `SkImageFilter_Base::Common::unflatten`.
#[inline]
pub fn validate_legacy_crop(rect: Rect, flags: u32) -> LegacyCropResult {
    const K_HAS_ALL_CROP_EDGE: u32 = 0x0F;
    if !is_valid_rect(rect) {
        return LegacyCropResult::Invalid;
    }
    if flags == 0x0 {
        LegacyCropResult::NoCrop
    } else if flags == K_HAS_ALL_CROP_EDGE {
        LegacyCropResult::HasCrop
    } else {
        LegacyCropResult::Invalid
    }
}

/// Exact port of `SkImageFilter_Base::getChildInputLayerBounds` when the child filter is null.
#[inline]
pub fn child_input_layer_bounds_null(
    desired_output: IRect,
    content_bounds: Option<IRect>,
) -> IRect {
    match content_bounds {
        Some(cb) => irect_intersect(desired_output, cb).unwrap_or_else(IRect::make_empty),
        None => desired_output,
    }
}

/// Exact port of `std::min(result, input->getCTMCapability())`.
#[inline]
pub fn combine_ctm_capability(a: MatrixCapability, b: MatrixCapability) -> MatrixCapability {
    if a < b {
        a
    } else {
        b
    }
}

/// Decision for `SkLocalMatrixImageFilter::Make`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LocalMatrixAction {
    Reject = 0,
    ReturnInput = 1,
    Create = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct LocalMatrixDecision {
    pub action: LocalMatrixAction,
    pub inv_local: Matrix,
}

/// Exact port of `SkLocalMatrixImageFilter::Make` validation & matrix inversion.
pub fn validate_local_matrix(
    local_matrix: &Matrix,
    input_capability: MatrixCapability,
) -> LocalMatrixDecision {
    if local_matrix.is_identity() {
        return LocalMatrixDecision {
            action: LocalMatrixAction::ReturnInput,
            inv_local: Matrix::identity(),
        };
    }
    let is_translate = (local_matrix.get_type_cached()
        & !skia_rs_geometry::matrix::TRANSLATE_MASK)
        == 0;
    if (input_capability == MatrixCapability::Translate && !is_translate)
        || (input_capability == MatrixCapability::ScaleTranslate
            && !local_matrix.is_scale_translate())
    {
        return LocalMatrixDecision {
            action: LocalMatrixAction::Reject,
            inv_local: Matrix::identity(),
        };
    }
    if let Some(inv) = local_matrix.invert() {
        LocalMatrixDecision {
            action: LocalMatrixAction::Create,
            inv_local: inv,
        }
    } else {
        LocalMatrixDecision {
            action: LocalMatrixAction::Reject,
            inv_local: Matrix::identity(),
        }
    }
}

/// Map rect for `SkLocalMatrixImageFilter::computeFastBounds`.
#[inline]
pub fn local_matrix_map_rect(m: &Matrix, r: Rect) -> Rect {
    matrix_map_rect(m, r)
}
