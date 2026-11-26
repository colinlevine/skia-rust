#![forbid(unsafe_code)]

//! Safe Rust implementation of Skia Phase 1 Layer 4.3 (`skia-rs-effects`):
//! - Path effects (1D, 2D, Corner, Dash, Discrete, Trim)
//! - Mask filters (Emboss, Table, BlurMaskFilter, MaskFilterBase nine-patch)
//! - Mask blur filter (`SkMaskBlurFilter`)
//! - CPU blur engine (`SkBlurEngine`, `SkShaderBlurAlgorithm`)
//! - Shadows (`SkDrawShadowInfo`, `SkShadowTessellator`, `SkShadowUtils`)

pub mod blur_engine;
pub mod mask_blur_filter;
pub mod mask_filters;
pub mod path_effects;
pub mod shadow_info;
pub mod shadow_tessellator;

use skia_rs_geometry::{Matrix, Point, Point3, Rect, Vector, W0_PLANE_DISTANCE};

#[inline]
pub(crate) fn matrix_map_point(m: &Matrix, p: Point) -> Point {
    let mut dst = [Point::ZERO; 1];
    m.map_points(&mut dst, &[p]);
    dst[0]
}

#[inline]
pub(crate) fn matrix_map_vector(m: &Matrix, v: Vector) -> Vector {
    let mut dst = [Point::ZERO; 1];
    m.map_vectors(&mut dst, &[v]);
    dst[0]
}

/// Maps `src` through `m`, matching `SkMatrix::mapRect` (both affine and perspective).
pub(crate) fn matrix_map_rect(m: &Matrix, src: Rect) -> Rect {
    if let Some((r, _)) = m.map_rect_non_perspective(src) {
        return r;
    }
    // Perspective case: clip homogeneous quad against w = W0_PLANE_DISTANCE and bound.
    let quad = src.to_quad();
    let mut homo = [Point3::default(); 4];
    m.map_points_to_homogeneous(&mut homo, &quad);

    let mut clipped = Vec::with_capacity(8);
    for i in 0..4 {
        let a = homo[i];
        let b = homo[(i + 1) & 3];
        let a_in = a.z >= W0_PLANE_DISTANCE;
        let b_in = b.z >= W0_PLANE_DISTANCE;
        if a_in {
            clipped.push(Point::new(a.x / a.z, a.y / a.z));
        }
        if a_in != b_in {
            let t = (W0_PLANE_DISTANCE - a.z) / (b.z - a.z);
            let x = a.x + (b.x - a.x) * t;
            let y = a.y + (b.y - a.y) * t;
            clipped.push(Point::new(x / W0_PLANE_DISTANCE, y / W0_PLANE_DISTANCE));
        }
    }
    if clipped.is_empty() {
        return Rect::make_empty();
    }
    let mut dst = Rect::make_empty();
    dst.set_bounds_no_check(&clipped);
    dst
}

#[inline]
pub(crate) fn point3_is_finite(p: Point3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}
