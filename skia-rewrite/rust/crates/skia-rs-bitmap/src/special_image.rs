//! Safe Rust port of `src/core/SkSpecialImage.cpp` algorithms.

use skia_rs_geometry::{IRect, Matrix, Rect};

/// Matches `SkSpecialImage::ganeshSafeSampling`:
/// returns `true` iff sampling must be downgraded to `SkSamplingOptions(SkFilterMode::kLinear)`.
#[inline]
pub const fn ganesh_should_downgrade_sampling(
    is_exact_fit: bool,
    max_aniso: i32,
    use_cubic: bool,
    mipmap_mode: i32,
) -> bool {
    if is_exact_fit {
        return false;
    }
    max_aniso != 0 || use_cubic || mipmap_mode != 0
}

/// Matches `SkSpecialImage::draw` destination rect computation.
#[inline]
pub fn draw_dst_rect(x: f32, y: f32, subset: IRect) -> Rect {
    Rect::from_ltrb(
        x,
        y,
        x + (subset.width() as f32),
        y + (subset.height() as f32),
    )
}

/// Matches `SkSpecialImage::asShader` & `SkSpecialImage_Raster::asShader` local matrix adjustment:
/// `SkMatrix subsetOrigin = SkMatrix::Translate(-subset.topLeft()); subsetOrigin.postConcat(lm);`
#[inline]
pub fn shader_subset_matrix(subset: IRect, lm: &Matrix) -> Matrix {
    let mut subset_origin = Matrix::translate(-(subset.left as f32), -(subset.top as f32));
    subset_origin.post_concat(lm);
    subset_origin
}

/// Matches `SkSpecialImages::CopyFromRaster` output subset rect: `SkIRect::MakeWH(subset.width(), subset.height())`.
#[inline]
pub fn copy_from_raster_subset(subset: IRect) -> IRect {
    IRect::from_wh(subset.width(), subset.height())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ganesh_safe_sampling_checks() {
        // Exact fit never downgrades
        assert!(!ganesh_should_downgrade_sampling(true, 4, true, 2));
        // Non-exact fit downgrades aniso, cubic, or mipmap
        assert!(ganesh_should_downgrade_sampling(false, 4, false, 0));
        assert!(ganesh_should_downgrade_sampling(false, 0, true, 0));
        assert!(ganesh_should_downgrade_sampling(false, 0, false, 1));
        assert!(!ganesh_should_downgrade_sampling(false, 0, false, 0));
    }

    #[test]
    fn shader_matrix_offsets_by_negative_subset_topleft() {
        let sub = IRect::make_ltrb(10, 20, 50, 60);
        let lm = Matrix::scale_translate(2.0, 3.0, 5.0, 7.0);
        let res = shader_subset_matrix(sub, &lm);
        // Translate(-10, -20) then ScaleTranslate(2, 3, 5, 7):
        // x' = 2*(x - 10) + 5 = 2x - 15
        // y' = 3*(y - 20) + 7 = 3y - 53
        assert_eq!(res.mat[0], 2.0);
        assert_eq!(res.mat[4], 3.0);
        assert_eq!(res.mat[2], -15.0);
        assert_eq!(res.mat[5], -53.0);
    }
}
