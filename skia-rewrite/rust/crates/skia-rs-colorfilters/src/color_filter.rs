//! Safe Rust port of `src/core/SkColorFilter.cpp` and `src/effects/colorfilters/SkColorFilterBase.cpp`.

use skia_rs_color::color::Color4f;
use skia_rs_geometry::pin_f32;

/// Matches `SkColorFilterBase::Type`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColorFilterType {
    Noop = 0,
    BlendMode = 1,
    ColorSpaceXform = 2,
    Compose = 3,
    Gaussian = 4,
    Matrix = 5,
    Runtime = 6,
    Table = 7,
    WorkingFormat = 8,
}

/// Arena stack budget used by `SkColorFilterBase::onFilterColor4f`.
pub const ON_FILTER_COLOR4F_ARENA_BYTES: usize = 2048;

/// Pin the alpha channel of an `SkRGBA4f` to `[0, 1]`, matching `SkRGBA4f::pinAlpha()`.
#[inline]
pub fn pin_alpha(color: Color4f) -> Color4f {
    Color4f::new(color.r, color.g, color.b, pin_f32(color.a, 0.0_f32, 1.0_f32))
}

/// Premultiply an unpremultiplied `Color4f`, matching `SkColor4f::premul()`.
#[inline]
pub fn premul_color4f(color: Color4f) -> Color4f {
    Color4f::new(
        color.r * color.a,
        color.g * color.a,
        color.b * color.a,
        color.a,
    )
}

/// Unpremultiply a premultiplied `Color4f`, matching `SkPMColor4f::unpremul()`.
#[inline]
pub fn unpremul_color4f(pm: Color4f) -> Color4f {
    if pm.a == 0.0_f32 {
        Color4f::new(0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32)
    } else {
        let inv_alpha = 1.0_f32 / pm.a;
        Color4f::new(pm.r * inv_alpha, pm.g * inv_alpha, pm.b * inv_alpha, pm.a)
    }
}

/// Exact post-processing step in `SkColorFilter::filterColor4f`:
/// `filteredPMColor.pinAlpha().unpremul()`.
#[inline]
pub fn finalize_filtered_color4f(pm: Color4f) -> Color4f {
    unpremul_color4f(pin_alpha(pm))
}

/// Exact `shaderIsOpaque` flag for `SkColorFilterBase::onFilterColor4f`: `color.fA == 1`.
#[inline]
pub fn pm_color_is_opaque(pm: Color4f) -> bool {
    pm.a == 1.0_f32
}

/// Default `SkColorFilterBase::onAsAColorMode` return value.
#[inline]
pub const fn base_on_as_a_color_mode() -> bool {
    false
}

/// Default `SkColorFilterBase::onAsAColorMatrix` return value.
#[inline]
pub const fn base_on_as_a_color_matrix() -> bool {
    false
}

/// Default fallback color returned by `SkColorFilterBase::onFilterColor4f` when
/// `appendStages` returns `false`.
#[inline]
pub const fn base_filter_fallback_color() -> Color4f {
    Color4f::new(0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_alpha_and_unpremul_behavior() {
        // Alpha > 1 is pinned to 1 before unpremul.
        let c = finalize_filtered_color4f(Color4f::new(0.5, 0.25, 0.75, 1.5));
        assert_eq!(c, Color4f::new(0.5, 0.25, 0.75, 1.0));

        // Alpha <= 0 or NaN is pinned to 0 -> unpremul returns all zeros.
        assert_eq!(
            finalize_filtered_color4f(Color4f::new(0.5, 0.25, 0.75, -0.2)),
            Color4f::new(0.0, 0.0, 0.0, 0.0)
        );
        assert_eq!(
            finalize_filtered_color4f(Color4f::new(0.5, 0.25, 0.75, f32::NAN)),
            Color4f::new(0.0, 0.0, 0.0, 0.0)
        );

        // Half alpha unpremultiplies cleanly.
        let half = finalize_filtered_color4f(Color4f::new(0.25, 0.5, 0.125, 0.5));
        assert_eq!(half, Color4f::new(0.5, 1.0, 0.25, 0.5));
    }

    #[test]
    fn premul_unpremul_roundtrip() {
        let orig = Color4f::new(0.2, 0.6, 0.8, 0.5);
        let pm = premul_color4f(orig);
        assert!(!pm_color_is_opaque(pm));
        let back = unpremul_color4f(pm);
        assert!((back.r - orig.r).abs() < 1e-6);
        assert!((back.g - orig.g).abs() < 1e-6);
        assert!((back.b - orig.b).abs() < 1e-6);
        assert_eq!(back.a, orig.a);
        assert!(pm_color_is_opaque(Color4f::new(1.0, 0.0, 0.0, 1.0)));
    }
}
