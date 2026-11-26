//! Safe Rust port of `src/effects/colorfilters/SkGaussianColorFilter.cpp`.

use skia_rs_color::color::Color4f;

/// Quartic approximation used by Skia's `gauss_a_to_rgba` raster pipeline stage
/// (`exp(-(1 - a)^2 * 4) - 0.018`).
#[inline]
pub fn eval_gaussian_alpha(a: f32) -> f32 {
    const C4: f32 = -2.26661229133605957031_f32;
    const C3: f32 = 2.89795351028442382812_f32;
    const C2: f32 = 0.21345567703247070312_f32;
    const C1: f32 = 0.15489584207534790039_f32;
    const C0: f32 = 0.00030726194381713867_f32;

    a * (a * (a * (a * C4 + C3) + C2) + C1) + C0
}

/// Apply the Gaussian color filter to a premultiplied color, outputting premul white
/// with the remapped alpha `(g, g, g, g)`.
#[inline]
pub fn apply_gaussian_pm_color(pm: Color4f) -> Color4f {
    let g = eval_gaussian_alpha(pm.a);
    Color4f::new(g, g, g, g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_ramp_endpoints_and_monotonicity() {
        let g0 = eval_gaussian_alpha(0.0);
        let g_half = eval_gaussian_alpha(0.5);
        let g1 = eval_gaussian_alpha(1.0);

        assert!(g0 >= 0.0 && g0 < 0.001);
        assert!((g_half - 0.35).abs() < 0.05);
        assert!((g1 - 1.0).abs() < 0.01);
        assert!(g0 < g_half && g_half < g1);

        let pm = apply_gaussian_pm_color(Color4f::new(0.2, 0.3, 0.4, 0.5));
        assert_eq!(pm.r, g_half);
        assert_eq!(pm.g, g_half);
        assert_eq!(pm.b, g_half);
        assert_eq!(pm.a, g_half);
    }
}
