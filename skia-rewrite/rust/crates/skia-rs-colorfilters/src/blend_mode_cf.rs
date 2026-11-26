//! Safe Rust port of `src/effects/colorfilters/SkBlendModeColorFilter.cpp`.

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_color::color::{Color, Color4f};

use crate::color_filter::pin_alpha;

/// Picture version where `SkBlendModeColorFilter` switched from 8-bit `SkColor` to `SkColor4f`.
pub const BLEND4F_COLOR_FILTER_VERSION: u32 = 93;

/// Exact port of `SkIsValidMode(SkBlendMode)`.
#[inline]
pub const fn is_valid_blend_mode(mode: i32) -> bool {
    BlendMode::from_i32(mode).is_some()
}

/// Exact port of `SkBlendModeColorFilter::onIsAlphaUnchanged()`.
#[inline]
pub const fn is_alpha_unchanged(mode: BlendMode) -> bool {
    matches!(mode, BlendMode::Dst | BlendMode::SrcATop)
}

/// Exact port of `SkBlendModeColorFilter::onAsAColorMode`.
#[inline]
pub fn as_a_color_mode(color: Color4f, mode: BlendMode) -> (Color, BlendMode) {
    (color.to_sk_color(), mode)
}

/// Pure-Rust representation of `SkBlendModeColorFilter`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BlendModeColorFilter {
    /// Always stored in unpremul sRGB.
    pub color: Color4f,
    pub mode: BlendMode,
}

impl BlendModeColorFilter {
    #[inline]
    pub const fn new(color: Color4f, mode: BlendMode) -> Self {
        Self { color, mode }
    }

    #[inline]
    pub const fn is_alpha_unchanged(&self) -> bool {
        is_alpha_unchanged(self.mode)
    }

    #[inline]
    pub fn as_a_color_mode(&self) -> (Color, BlendMode) {
        as_a_color_mode(self.color, self.mode)
    }
}

/// FFI-friendly result of simplifying a blend mode color filter after sRGB conversion.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BlendSimplifyResult {
    /// `true` => allocate `SkBlendModeColorFilter(color, mode)`; `false` => return `nullptr` (noop/invalid).
    pub should_create: bool,
    pub color: Color4f,
    pub mode: i32,
}

/// Step 1 of `SkColorFilters::Blend`: validate mode and pin alpha prior to sRGB xform.
#[inline]
pub fn prepare_blend_input(color: Color4f, mode_raw: i32) -> Option<(Color4f, BlendMode)> {
    let mode = BlendMode::from_i32(mode_raw)?;
    Some((pin_alpha(color), mode))
}

/// Step 2 of `SkColorFilters::Blend`: collapse modes and weed out no-op combinations.
/// `srgb` is the already-alpha-pinned, unpremul sRGB color.
pub fn simplify_blend_filter(mut srgb: Color4f, mode_raw: i32) -> Option<BlendModeColorFilter> {
    let mut mode = BlendMode::from_i32(mode_raw)?;
    let alpha = srgb.a;

    if mode == BlendMode::Clear {
        srgb = Color4f::new(0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        mode = BlendMode::Src;
    } else if mode == BlendMode::SrcOver {
        if alpha == 0.0_f32 {
            mode = BlendMode::Dst;
        } else if alpha == 1.0_f32 {
            mode = BlendMode::Src;
        }
    }

    if mode == BlendMode::Dst
        || (alpha == 0.0_f32
            && matches!(
                mode,
                BlendMode::SrcOver
                    | BlendMode::DstOver
                    | BlendMode::DstOut
                    | BlendMode::SrcATop
                    | BlendMode::Xor
                    | BlendMode::Darken
            ))
        || (alpha == 1.0_f32 && mode == BlendMode::DstIn)
    {
        return None;
    }

    Some(BlendModeColorFilter { color: srgb, mode })
}

/// FFI wrapper for [`simplify_blend_filter`].
pub fn simplify_blend_filter_ffi(srgb: Color4f, mode_raw: i32) -> BlendSimplifyResult {
    match simplify_blend_filter(srgb, mode_raw) {
        Some(cf) => BlendSimplifyResult {
            should_create: true,
            color: cf.color,
            mode: cf.mode as i32,
        },
        None => BlendSimplifyResult {
            should_create: false,
            color: Color4f::default(),
            mode: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_unchanged_modes() {
        assert!(is_alpha_unchanged(BlendMode::Dst));
        assert!(is_alpha_unchanged(BlendMode::SrcATop));
        assert!(!is_alpha_unchanged(BlendMode::SrcOver));
        assert!(!is_alpha_unchanged(BlendMode::Src));
        assert!(!is_alpha_unchanged(BlendMode::Modulate));
    }

    #[test]
    fn clear_collapses_to_transparent_src() {
        let cf = simplify_blend_filter(Color4f::new(1.0, 0.5, 0.25, 0.75), BlendMode::Clear as i32)
            .expect("Clear becomes transparent Src");
        assert_eq!(cf.color, Color4f::new(0.0, 0.0, 0.0, 0.0));
        assert_eq!(cf.mode, BlendMode::Src);
    }

    #[test]
    fn src_over_simplifications() {
        // alpha == 0 -> Dst -> noop
        assert!(
            simplify_blend_filter(Color4f::new(1.0, 0.0, 0.0, 0.0), BlendMode::SrcOver as i32)
                .is_none()
        );
        // alpha == 1 -> Src
        let opaque =
            simplify_blend_filter(Color4f::new(1.0, 0.2, 0.3, 1.0), BlendMode::SrcOver as i32)
                .unwrap();
        assert_eq!(opaque.mode, BlendMode::Src);
        // intermediate alpha -> stays SrcOver
        let semi =
            simplify_blend_filter(Color4f::new(1.0, 0.2, 0.3, 0.5), BlendMode::SrcOver as i32)
                .unwrap();
        assert_eq!(semi.mode, BlendMode::SrcOver);
    }

    #[test]
    fn noop_combinations_return_none() {
        assert!(
            simplify_blend_filter(Color4f::new(0.5, 0.5, 0.5, 0.5), BlendMode::Dst as i32)
                .is_none()
        );
        for m in [
            BlendMode::DstOver,
            BlendMode::DstOut,
            BlendMode::SrcATop,
            BlendMode::Xor,
            BlendMode::Darken,
        ] {
            assert!(simplify_blend_filter(Color4f::new(0.5, 0.5, 0.5, 0.0), m as i32).is_none());
            assert!(simplify_blend_filter(Color4f::new(0.5, 0.5, 0.5, 0.5), m as i32).is_some());
        }
        assert!(
            simplify_blend_filter(Color4f::new(0.5, 0.5, 0.5, 1.0), BlendMode::DstIn as i32)
                .is_none()
        );
        assert!(
            simplify_blend_filter(Color4f::new(0.5, 0.5, 0.5, 0.5), BlendMode::DstIn as i32)
                .is_some()
        );
        // Invalid mode
        assert!(simplify_blend_filter(Color4f::new(1.0, 1.0, 1.0, 1.0), -1).is_none());
        assert!(simplify_blend_filter(Color4f::new(1.0, 1.0, 1.0, 1.0), 99).is_none());
    }
}
