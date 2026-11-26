//! Safe Rust algorithms for `SkPaint` and `SkPaintPriv`.

use skia_rs_geometry::{pin_f32, Rect};

// Blend mode constants matching `include/core/SkBlendMode.h`.
pub const BLEND_CLEAR: u8 = 0;
pub const BLEND_SRC: u8 = 1;
pub const BLEND_DST: u8 = 2;
pub const BLEND_SRC_OVER: u8 = 3;
pub const BLEND_DST_OVER: u8 = 4;
pub const BLEND_SRC_IN: u8 = 5;
pub const BLEND_DST_IN: u8 = 6;
pub const BLEND_SRC_OUT: u8 = 7;
pub const BLEND_DST_OUT: u8 = 8;
pub const BLEND_SRC_ATOP: u8 = 9;
pub const BLEND_DST_ATOP: u8 = 10;
pub const BLEND_XOR: u8 = 11;
pub const BLEND_PLUS: u8 = 12;
pub const BLEND_MODULATE: u8 = 13;
pub const BLEND_SCREEN: u8 = 14;
pub const BLEND_LAST_MODE: u8 = 28; // kLuminosity

pub const CUSTOM_BLEND_MODE_SENTINEL: u8 = 0xFF;

// Blend mode coeff constants matching `SkBlendModeCoeff`.
pub const COEFF_ZERO: u8 = 0;
pub const COEFF_ONE: u8 = 1;
pub const COEFF_SC: u8 = 2;
pub const COEFF_ISC: u8 = 3;
pub const COEFF_DC: u8 = 4;
pub const COEFF_IDC: u8 = 5;
pub const COEFF_SA: u8 = 6;
pub const COEFF_ISA: u8 = 7;
pub const COEFF_DA: u8 = 8;
pub const COEFF_IDA: u8 = 9;

// ColorType constants matching `include/core/SkColorType.h`.
pub const COLOR_TYPE_UNKNOWN: i32 = 0;
pub const COLOR_TYPE_RGB_565: i32 = 2;
pub const COLOR_TYPE_ARGB_4444: i32 = 3;

// Paint Style/Cap/Join limits matching `include/core/SkPaint.h`.
pub const PAINT_STYLE_COUNT: u32 = 3;
pub const PAINT_CAP_COUNT: u32 = 3;
pub const PAINT_JOIN_COUNT: u32 = 3;
pub const PAINT_DEFAULT_MITER_LIMIT: f32 = 4.0;

// ShaderOverrideOpacity values matching `SkPaintPriv::ShaderOverrideOpacity`.
pub const SHADER_OVERRIDE_NONE: u8 = 0;
pub const SHADER_OVERRIDE_OPAQUE: u8 = 1;
pub const SHADER_OVERRIDE_NOT_OPAQUE: u8 = 2;

// FlatFlags matching `SkPaintPriv.cpp`.
pub const FLAT_FLAG_HAS_TYPEFACE: u8 = 0x1;
pub const FLAT_FLAG_HAS_EFFECTS: u8 = 0x2;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaintUnpackedV68 {
    pub anti_alias: bool,
    pub dither: bool,
    pub has_blend_mode: bool,
    pub blend_mode: u8,
    pub stroke_cap: u8,
    pub stroke_join: u8,
    pub style: u8,
    pub flat_flags: u8,
    pub safe_ok: bool,
}

/// Pins alpha to `[0.0, 1.0]` matching `SkPaint::setAlphaf`.
#[inline]
pub fn pin_alphaf(a: f32) -> f32 {
    pin_f32(a, 0.0, 1.0)
}

/// Matches `SkPaint::nothingToDraw()`.
/// `blend_mode` is `Some(mode)` when `asBlendMode()` has a value, or `None` for custom blenders.
#[inline]
pub const fn nothing_to_draw(
    blend_mode: Option<u8>,
    alpha: u8,
    color_filter_affects_alpha: bool,
    image_filter_affects_alpha: bool,
) -> bool {
    let Some(bm) = blend_mode else {
        return false;
    };
    match bm {
        BLEND_SRC_OVER | BLEND_SRC_ATOP | BLEND_DST_OUT | BLEND_DST_OVER | BLEND_PLUS => {
            if alpha == 0 {
                !color_filter_affects_alpha && !image_filter_affects_alpha
            } else {
                false
            }
        }
        BLEND_DST => true,
        _ => false,
    }
}

/// Matches `SkBlendMode_AsCoeff` table in `SkBlendMode.cpp`.
#[inline]
pub const fn blend_mode_as_coeff(mode: u8) -> Option<(u8, u8)> {
    const TABLE: [(u8, u8); 15] = [
        (COEFF_ZERO, COEFF_ZERO), // clear
        (COEFF_ONE, COEFF_ZERO),  // src
        (COEFF_ZERO, COEFF_ONE),  // dst
        (COEFF_ONE, COEFF_ISA),   // src-over
        (COEFF_IDA, COEFF_ONE),   // dst-over
        (COEFF_DA, COEFF_ZERO),   // src-in
        (COEFF_ZERO, COEFF_SA),   // dst-in
        (COEFF_IDA, COEFF_ZERO),  // src-out
        (COEFF_ZERO, COEFF_ISA),  // dst-out
        (COEFF_DA, COEFF_ISA),    // src-atop
        (COEFF_IDA, COEFF_SA),    // dst-atop
        (COEFF_IDA, COEFF_ISA),   // xor
        (COEFF_ONE, COEFF_ONE),   // plus
        (COEFF_ZERO, COEFF_SC),   // modulate
        (COEFF_ONE, COEFF_ISC),   // screen
    ];
    if (mode as usize) < TABLE.len() {
        Some(TABLE[mode as usize])
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SrcColorOpacity {
    Opaque,
    TransparentBlack,
    TransparentAlpha,
    Unknown,
}

#[inline]
const fn blend_mode_is_opaque(mode: u8, opacity: SrcColorOpacity) -> bool {
    let Some((src, dst)) = blend_mode_as_coeff(mode) else {
        return false;
    };
    match src {
        COEFF_DA | COEFF_DC | COEFF_IDA | COEFF_IDC => return false,
        _ => {}
    }
    match dst {
        COEFF_ZERO => true,
        COEFF_ISA => matches!(opacity, SrcColorOpacity::Opaque),
        COEFF_SA => matches!(
            opacity,
            SrcColorOpacity::TransparentBlack | SrcColorOpacity::TransparentAlpha
        ),
        COEFF_SC => matches!(opacity, SrcColorOpacity::TransparentBlack),
        _ => false,
    }
}

/// Matches `SkPaintPriv::Overwrites`.
pub const fn paint_overwrites(
    has_paint: bool,
    override_opacity: u8,
    changes_alpha: bool,
    paint_alpha: u8,
    has_shader: bool,
    shader_is_opaque: bool,
    blend_mode: Option<u8>,
) -> bool {
    if !has_paint {
        return override_opacity != SHADER_OVERRIDE_NOT_OPAQUE;
    }

    let mut opacity = SrcColorOpacity::Unknown;
    if !changes_alpha {
        if paint_alpha == 0xFF
            && override_opacity != SHADER_OVERRIDE_NOT_OPAQUE
            && (!has_shader || shader_is_opaque)
        {
            opacity = SrcColorOpacity::Opaque;
        } else if paint_alpha == 0 {
            if override_opacity == SHADER_OVERRIDE_NONE && !has_shader {
                opacity = SrcColorOpacity::TransparentBlack;
            } else {
                opacity = SrcColorOpacity::TransparentAlpha;
            }
        }
    }

    let Some(bm) = blend_mode else {
        return false;
    };
    blend_mode_is_opaque(bm, opacity)
}

/// Matches `SkPaintPriv::ShouldDither`.
#[inline]
pub const fn should_dither(
    is_dither: bool,
    dst_color_type: i32,
    has_image_filter: bool,
    has_mask_filter: bool,
    has_shader: bool,
    shader_is_constant: bool,
) -> bool {
    if !is_dither {
        return false;
    }
    if dst_color_type == COLOR_TYPE_UNKNOWN {
        return false;
    }
    if dst_color_type == COLOR_TYPE_RGB_565 || dst_color_type == COLOR_TYPE_ARGB_4444 {
        return true;
    }
    has_image_filter || has_mask_filter || (has_shader && !shader_is_constant)
}

/// Matches `pack_v68` in `SkPaintPriv.cpp`.
#[inline]
pub const fn pack_v68(
    anti_alias: bool,
    dither: bool,
    blend_mode: Option<u8>,
    stroke_cap: u8,
    stroke_join: u8,
    style: u8,
    flat_flags: u8,
) -> u32 {
    let flags = ((dither as u32) << 1) | (anti_alias as u32);
    let mode = match blend_mode {
        Some(m) => m as u32,
        None => CUSTOM_BLEND_MODE_SENTINEL as u32,
    };
    let mut packed = 0u32;
    packed |= flags & 0xFF;
    packed |= (mode & 0xFF) << 8;
    packed |= ((stroke_cap as u32) & 0x3) << 16;
    packed |= ((stroke_join as u32) & 0x3) << 18;
    packed |= ((style as u32) & 0x3) << 20;
    // bits 22..24 were filterQuality (always 0)
    packed |= ((flat_flags as u32) & 0xFF) << 24;
    packed
}

/// Matches `unpack_v68` in `SkPaintPriv.cpp` including `SkSafeRange::checkLE`.
pub const fn unpack_v68(mut packed: u32) -> PaintUnpackedV68 {
    let mut safe_ok = true;
    let anti_alias = (packed & 1) != 0;
    let dither = (packed & 2) != 0;
    packed >>= 8;

    let raw_mode = (packed & 0xFF) as u8;
    let (has_blend_mode, blend_mode) = if raw_mode != CUSTOM_BLEND_MODE_SENTINEL {
        if raw_mode <= BLEND_LAST_MODE {
            (true, raw_mode)
        } else {
            safe_ok = false;
            (true, BLEND_LAST_MODE)
        }
    } else {
        (false, 0)
    };
    packed >>= 8;

    let raw_cap = (packed & 0x3) as u8;
    let stroke_cap = if raw_cap <= 2 {
        raw_cap
    } else {
        safe_ok = false;
        2
    };
    packed >>= 2;

    let raw_join = (packed & 0x3) as u8;
    let stroke_join = if raw_join <= 2 {
        raw_join
    } else {
        safe_ok = false;
        2
    };
    packed >>= 2;

    let raw_style = (packed & 0x3) as u8;
    let style = if raw_style <= 2 {
        raw_style
    } else {
        safe_ok = false;
        2
    };
    packed >>= 2;

    // skip filterQuality bits
    packed >>= 2;

    let flat_flags = (packed & 0xFF) as u8;

    PaintUnpackedV68 {
        anti_alias,
        dither,
        has_blend_mode,
        blend_mode,
        stroke_cap,
        stroke_join,
        style,
        flat_flags,
        safe_ok,
    }
}

/// Outsets a `Rect` by `(radius, radius)` matching `src->makeOutset(radius, radius)`.
#[inline]
pub fn outset_rect(src: Rect, radius: f32) -> Rect {
    Rect::from_ltrb(
        src.left - radius,
        src.top - radius,
        src.right + radius,
        src.bottom + radius,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nothing_to_draw() {
        assert!(nothing_to_draw(Some(BLEND_DST), 255, true, true));
        assert!(nothing_to_draw(Some(BLEND_SRC_OVER), 0, false, false));
        assert!(!nothing_to_draw(Some(BLEND_SRC_OVER), 0, true, false));
        assert!(!nothing_to_draw(Some(BLEND_SRC_OVER), 0, false, true));
        assert!(!nothing_to_draw(Some(BLEND_SRC_OVER), 1, false, false));
        assert!(!nothing_to_draw(Some(BLEND_SRC), 0, false, false));
        assert!(!nothing_to_draw(None, 0, false, false));
    }

    #[test]
    fn test_overwrites() {
        assert!(paint_overwrites(false, SHADER_OVERRIDE_NONE, false, 255, false, false, None));
        assert!(!paint_overwrites(false, SHADER_OVERRIDE_NOT_OPAQUE, false, 255, false, false, None));

        // SrcOver opaque black overwrites
        assert!(paint_overwrites(
            true,
            SHADER_OVERRIDE_NONE,
            false,
            255,
            false,
            false,
            Some(BLEND_SRC_OVER)
        ));
        // SrcOver translucent does not overwrite
        assert!(!paint_overwrites(
            true,
            SHADER_OVERRIDE_NONE,
            false,
            128,
            false,
            false,
            Some(BLEND_SRC_OVER)
        ));
        // Clear always overwrites
        assert!(paint_overwrites(
            true,
            SHADER_OVERRIDE_NOT_OPAQUE,
            true,
            0,
            true,
            false,
            Some(BLEND_CLEAR)
        ));
        // Custom blender does not overwrite
        assert!(!paint_overwrites(
            true,
            SHADER_OVERRIDE_NONE,
            false,
            255,
            false,
            false,
            None
        ));
    }

    #[test]
    fn test_should_dither() {
        assert!(!should_dither(false, COLOR_TYPE_RGB_565, true, true, true, false));
        assert!(!should_dither(true, COLOR_TYPE_UNKNOWN, true, true, true, false));
        assert!(should_dither(true, COLOR_TYPE_RGB_565, false, false, false, true));
        assert!(should_dither(true, COLOR_TYPE_ARGB_4444, false, false, false, true));
        assert!(!should_dither(true, 4, false, false, true, true));
        assert!(should_dither(true, 4, false, false, true, false));
    }

    #[test]
    fn test_pack_unpack_v68() {
        let packed = pack_v68(true, true, Some(BLEND_SCREEN), 2, 1, 2, FLAT_FLAG_HAS_EFFECTS);
        let u = unpack_v68(packed);
        assert!(u.safe_ok);
        assert!(u.anti_alias);
        assert!(u.dither);
        assert!(u.has_blend_mode);
        assert_eq!(u.blend_mode, BLEND_SCREEN);
        assert_eq!(u.stroke_cap, 2);
        assert_eq!(u.stroke_join, 1);
        assert_eq!(u.style, 2);
        assert_eq!(u.flat_flags, FLAT_FLAG_HAS_EFFECTS);

        // Custom blender sentinel
        let packed_custom = pack_v68(false, false, None, 0, 0, 0, FLAT_FLAG_HAS_EFFECTS);
        let uc = unpack_v68(packed_custom);
        assert!(uc.safe_ok);
        assert!(!uc.has_blend_mode);

        // Out-of-range style (3) fails safe check
        let bad = pack_v68(false, false, Some(BLEND_SRC_OVER), 3, 0, 0, 0);
        let ub = unpack_v68(bad);
        assert!(!ub.safe_ok);
    }
}
