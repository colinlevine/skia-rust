//! Safe Rust port of `src/core/SkBlendMode.cpp` and `src/core/SkBlendModeBlender.cpp`.

use crate::color::Color4f;

/// Matches `enum class SkBlendMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlendMode {
    Clear = 0,
    Src = 1,
    Dst = 2,
    SrcOver = 3,
    DstOver = 4,
    SrcIn = 5,
    DstIn = 6,
    SrcOut = 7,
    DstOut = 8,
    SrcATop = 9,
    DstATop = 10,
    Xor = 11,
    Plus = 12,
    Modulate = 13,
    Screen = 14,
    Overlay = 15,
    Darken = 16,
    Lighten = 17,
    ColorDodge = 18,
    ColorBurn = 19,
    HardLight = 20,
    SoftLight = 21,
    Difference = 22,
    Exclusion = 23,
    Multiply = 24,
    Hue = 25,
    Saturation = 26,
    Color = 27,
    Luminosity = 28,
}

impl BlendMode {
    pub const COUNT: usize = 29;

    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Clear),
            1 => Some(Self::Src),
            2 => Some(Self::Dst),
            3 => Some(Self::SrcOver),
            4 => Some(Self::DstOver),
            5 => Some(Self::SrcIn),
            6 => Some(Self::DstIn),
            7 => Some(Self::SrcOut),
            8 => Some(Self::DstOut),
            9 => Some(Self::SrcATop),
            10 => Some(Self::DstATop),
            11 => Some(Self::Xor),
            12 => Some(Self::Plus),
            13 => Some(Self::Modulate),
            14 => Some(Self::Screen),
            15 => Some(Self::Overlay),
            16 => Some(Self::Darken),
            17 => Some(Self::Lighten),
            18 => Some(Self::ColorDodge),
            19 => Some(Self::ColorBurn),
            20 => Some(Self::HardLight),
            21 => Some(Self::SoftLight),
            22 => Some(Self::Difference),
            23 => Some(Self::Exclusion),
            24 => Some(Self::Multiply),
            25 => Some(Self::Hue),
            26 => Some(Self::Saturation),
            27 => Some(Self::Color),
            28 => Some(Self::Luminosity),
            _ => None,
        }
    }
}

/// Matches `enum class SkBlendModeCoeff`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendModeCoeff {
    Zero = 0,
    One = 1,
    SC = 2,
    ISC = 3,
    DC = 4,
    IDC = 5,
    SA = 6,
    ISA = 7,
    DA = 8,
    IDA = 9,
}

/// Matches `enum class SkBlendFastPath`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendFastPath {
    Normal = 0,
    SrcOver = 1,
    SkipDrawing = 2,
}

/// Exact port of `SkBlendMode_ShouldPreScaleCoverage`.
pub const fn should_pre_scale_coverage(mode: BlendMode, rgb_coverage: bool) -> bool {
    match mode {
        BlendMode::Dst | BlendMode::DstOver | BlendMode::Plus => true,
        BlendMode::DstOut | BlendMode::SrcATop | BlendMode::SrcOver | BlendMode::Xor => {
            !rgb_coverage
        }
        _ => false,
    }
}

/// Exact port of `SkBlendMode_SupportsCoverageAsAlpha`.
#[inline]
pub const fn supports_coverage_as_alpha(mode: BlendMode) -> bool {
    should_pre_scale_coverage(mode, false)
}

const COEFFS: [(BlendModeCoeff, BlendModeCoeff); 15] = [
    (BlendModeCoeff::Zero, BlendModeCoeff::Zero), // Clear
    (BlendModeCoeff::One, BlendModeCoeff::Zero),  // Src
    (BlendModeCoeff::Zero, BlendModeCoeff::One),  // Dst
    (BlendModeCoeff::One, BlendModeCoeff::ISA),   // SrcOver
    (BlendModeCoeff::IDA, BlendModeCoeff::One),   // DstOver
    (BlendModeCoeff::DA, BlendModeCoeff::Zero),   // SrcIn
    (BlendModeCoeff::Zero, BlendModeCoeff::SA),   // DstIn
    (BlendModeCoeff::IDA, BlendModeCoeff::Zero),  // SrcOut
    (BlendModeCoeff::Zero, BlendModeCoeff::ISA),  // DstOut
    (BlendModeCoeff::DA, BlendModeCoeff::ISA),    // SrcATop
    (BlendModeCoeff::IDA, BlendModeCoeff::SA),    // DstATop
    (BlendModeCoeff::IDA, BlendModeCoeff::ISA),   // Xor
    (BlendModeCoeff::One, BlendModeCoeff::One),   // Plus
    (BlendModeCoeff::Zero, BlendModeCoeff::SC),   // Modulate
    (BlendModeCoeff::One, BlendModeCoeff::ISC),   // Screen
];

/// Exact port of `SkBlendMode_AsCoeff`.
pub const fn as_coeff(mode: BlendMode) -> Option<(BlendModeCoeff, BlendModeCoeff)> {
    let idx = mode as usize;
    if idx >= COEFFS.len() {
        return None;
    }
    Some(COEFFS[idx])
}

/// Exact port of `SkBlendMode_Name` (null-terminated C string bytes for zero-copy FFI).
pub const fn name_cstr(mode: BlendMode) -> &'static [u8] {
    match mode {
        BlendMode::Clear => b"Clear\0",
        BlendMode::Src => b"Src\0",
        BlendMode::Dst => b"Dst\0",
        BlendMode::SrcOver => b"SrcOver\0",
        BlendMode::DstOver => b"DstOver\0",
        BlendMode::SrcIn => b"SrcIn\0",
        BlendMode::DstIn => b"DstIn\0",
        BlendMode::SrcOut => b"SrcOut\0",
        BlendMode::DstOut => b"DstOut\0",
        BlendMode::SrcATop => b"SrcATop\0",
        BlendMode::DstATop => b"DstATop\0",
        BlendMode::Xor => b"Xor\0",
        BlendMode::Plus => b"Plus\0",
        BlendMode::Modulate => b"Modulate\0",
        BlendMode::Screen => b"Screen\0",
        BlendMode::Overlay => b"Overlay\0",
        BlendMode::Darken => b"Darken\0",
        BlendMode::Lighten => b"Lighten\0",
        BlendMode::ColorDodge => b"ColorDodge\0",
        BlendMode::ColorBurn => b"ColorBurn\0",
        BlendMode::HardLight => b"HardLight\0",
        BlendMode::SoftLight => b"SoftLight\0",
        BlendMode::Difference => b"Difference\0",
        BlendMode::Exclusion => b"Exclusion\0",
        BlendMode::Multiply => b"Multiply\0",
        BlendMode::Hue => b"Hue\0",
        BlendMode::Saturation => b"Saturation\0",
        BlendMode::Color => b"Color\0",
        BlendMode::Luminosity => b"Luminosity\0",
    }
}

/// Exact port of `SkBlendMode_Name` as a Rust `&str`.
pub fn name(mode: BlendMode) -> &'static str {
    let bytes = name_cstr(mode);
    core::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap()
}

/// Exact port of `CheckFastPath(paint, dstIsOpaque)`.
pub const fn check_fast_path(
    mode: Option<BlendMode>,
    just_solid_color: bool,
    dst_is_opaque: bool,
) -> BlendFastPath {
    let Some(bm) = mode else {
        return BlendFastPath::Normal;
    };
    match bm {
        BlendMode::SrcOver => BlendFastPath::SrcOver,
        BlendMode::Src => {
            if just_solid_color {
                BlendFastPath::SrcOver
            } else {
                BlendFastPath::Normal
            }
        }
        BlendMode::Dst => BlendFastPath::SkipDrawing,
        BlendMode::DstOver => {
            if dst_is_opaque {
                BlendFastPath::SkipDrawing
            } else {
                BlendFastPath::Normal
            }
        }
        BlendMode::SrcIn => {
            if dst_is_opaque && just_solid_color {
                BlendFastPath::SrcOver
            } else {
                BlendFastPath::Normal
            }
        }
        BlendMode::DstIn => {
            if just_solid_color {
                BlendFastPath::SkipDrawing
            } else {
                BlendFastPath::Normal
            }
        }
        _ => BlendFastPath::Normal,
    }
}

/// Exact port of the fast path in `SkBlendMode_Apply` for Clear/Src/Dst/SrcOver.
/// Returns `None` if the mode should be evaluated via `SkRasterPipeline`.
pub fn apply_fast(mode: BlendMode, src: Color4f, dst: Color4f) -> Option<Color4f> {
    match mode {
        BlendMode::Clear => Some(Color4f::new(0.0, 0.0, 0.0, 0.0)),
        BlendMode::Src => Some(src),
        BlendMode::Dst => Some(dst),
        BlendMode::SrcOver => {
            let inv_sa = 1.0_f32 - src.a;
            Some(Color4f::new(
                src.r + dst.r * inv_sa,
                src.g + dst.g * inv_sa,
                src.b + dst.b * inv_sa,
                src.a + dst.a * inv_sa,
            ))
        }
        _ => None,
    }
}

/// Exact port of `SkBlenderBase::affectsTransparentBlack`.
pub const fn affects_transparent_black(mode: Option<BlendMode>) -> bool {
    let Some(bm) = mode else {
        return true;
    };
    match as_coeff(bm) {
        Some((_src, dst)) => {
            !matches!(dst, BlendModeCoeff::One | BlendModeCoeff::ISA | BlendModeCoeff::ISC)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coeff_and_transparent_black() {
        assert_eq!(
            as_coeff(BlendMode::SrcOver),
            Some((BlendModeCoeff::One, BlendModeCoeff::ISA))
        );
        assert_eq!(as_coeff(BlendMode::Multiply), None);
        assert!(!affects_transparent_black(Some(BlendMode::SrcOver)));
        assert!(affects_transparent_black(Some(BlendMode::Clear)));
        assert!(affects_transparent_black(Some(BlendMode::Src)));
        assert!(!affects_transparent_black(Some(BlendMode::Multiply)));
        assert!(affects_transparent_black(None));
    }

    #[test]
    fn fast_paths_and_coverage() {
        assert!(should_pre_scale_coverage(BlendMode::Plus, true));
        assert!(!should_pre_scale_coverage(BlendMode::SrcOver, true));
        assert!(should_pre_scale_coverage(BlendMode::SrcOver, false));
        assert!(supports_coverage_as_alpha(BlendMode::Xor));

        assert_eq!(
            check_fast_path(Some(BlendMode::Src), true, false),
            BlendFastPath::SrcOver
        );
        assert_eq!(
            check_fast_path(Some(BlendMode::DstOver), false, true),
            BlendFastPath::SkipDrawing
        );
        assert_eq!(
            check_fast_path(Some(BlendMode::Dst), false, false),
            BlendFastPath::SkipDrawing
        );
    }

    #[test]
    fn names_all_modes() {
        assert_eq!(name(BlendMode::Clear), "Clear");
        assert_eq!(name(BlendMode::SrcOver), "SrcOver");
        assert_eq!(name(BlendMode::Luminosity), "Luminosity");
    }
}
