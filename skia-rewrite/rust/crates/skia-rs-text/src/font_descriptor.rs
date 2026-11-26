//! Safe Rust port of `SkFontDescriptor`, `SkFontMgr` style matching, and `SkTypeface` helpers.

use crate::{four_byte_tag, FontStyle, FontStyleSlant};
use skia_rs_geometry::scalar_round_to_int;
use skia_rs_math::scalar::float_interp_func;

/// Width axis percentages indexed by `usWidth` (0..15).
pub const WIDTH_FOR_US_WIDTH: [f32; 16] = [
    50.0, 50.0, 62.5, 75.0, 87.5, 100.0, 112.5, 125.0, 150.0, 200.0, 200.0, 200.0, 200.0, 200.0,
    200.0, 200.0,
];

const US_WIDTHS: [f32; 9] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];

/// Variation axis four-byte tags (`wght`, `wdth`, `slnt`, `ital`).
pub const AXIS_WGHT: u32 = four_byte_tag(b'w', b'g', b'h', b't');
pub const AXIS_WDTH: u32 = four_byte_tag(b'w', b'd', b't', b'h');
pub const AXIS_SLNT: u32 = four_byte_tag(b's', b'l', b'n', b't');
pub const AXIS_ITAL: u32 = four_byte_tag(b'i', b't', b'a', b'l');

/// Mirrors `SkFontDescriptor::SkFontStyleWidthForWidthAxisValue`.
pub fn font_style_width_for_width_axis_value(width: f32) -> i32 {
    scalar_round_to_int(float_interp_func(width, &WIDTH_FOR_US_WIDTH[1..10], &US_WIDTHS))
}

/// Mirrors `SkFontDescriptor::SkFontWidthAxisValueForStyleWidth`.
#[inline]
pub fn font_width_axis_value_for_style_width(width: i32) -> f32 {
    WIDTH_FOR_US_WIDTH[(width & 0xF) as usize]
}

/// Mirrors the initial style bit unpacking in `SkFontDescriptor::Deserialize`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DescriptorStyleAxes {
    pub weight: f32,
    pub width: f32,
    pub slant: f32,
    pub italic: f32,
}

/// Unpacks `styleBits` into `(weight, width, slant, italic)` matching `SkFontDescriptor::Deserialize`.
pub fn unpack_descriptor_style_bits(style_bits: usize) -> DescriptorStyleAxes {
    let weight = ((style_bits >> 16) & 0xFFFF) as f32;
    let width = WIDTH_FOR_US_WIDTH[(style_bits >> 8) & 0x000F];
    let slant_bits = style_bits & 0x000F;
    let slant = if slant_bits != FontStyleSlant::Upright as usize {
        -20.0
    } else {
        0.0
    };
    let italic = if slant_bits == FontStyleSlant::Italic as usize {
        1.0
    } else {
        0.0
    };
    DescriptorStyleAxes {
        weight,
        width,
        slant,
        italic,
    }
}

/// Packs `FontStyle` into `styleBits` and serialized `(weight, width, slant, italic)` scalars
/// matching `SkFontDescriptor::serialize`.
pub fn pack_descriptor_style(style: FontStyle) -> (u32, DescriptorStyleAxes) {
    let style_bits =
        (((style.weight as u32) << 16) | ((style.width as u32) << 8) | (style.slant as u32)) as u32;
    let width = WIDTH_FOR_US_WIDTH[(style.width & 0xF) as usize];
    let slant = if style.slant == FontStyleSlant::Upright as u8 {
        0.0
    } else {
        -20.0
    };
    let italic = if style.slant == FontStyleSlant::Italic as u8 {
        1.0
    } else {
        0.0
    };
    (
        style_bits,
        DescriptorStyleAxes {
            weight: style.weight as f32,
            width,
            slant,
            italic,
        },
    )
}

/// Converts `(weight, width, slant, italic)` into `FontStyle` matching
/// `SkFontDescriptor::Deserialize` and `SkFontMgr::Request::fontStyleFromModel`.
pub fn font_style_from_axes(axes: DescriptorStyleAxes) -> FontStyle {
    let mut slant_enum = FontStyleSlant::Upright;
    if axes.slant != 0.0 {
        slant_enum = FontStyleSlant::Oblique;
    }
    if 0.0 < axes.italic {
        slant_enum = FontStyleSlant::Italic;
    }
    let width_enum = font_style_width_for_width_axis_value(axes.width);
    FontStyle::new(scalar_round_to_int(axes.weight), width_enum, slant_enum)
}

/// Mirrors `SkFontArguments::VariationPosition::Coordinate`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct VariationCoordinate {
    pub axis: u32,
    pub value: f32,
}

/// Mirrors `SkFontMgr::Request::fontStyleFromModel`.
pub fn font_style_from_model(coords: &[VariationCoordinate]) -> FontStyle {
    let mut axes = DescriptorStyleAxes {
        weight: 400.0,
        width: 100.0,
        slant: 0.0,
        italic: 0.0,
    };
    for c in coords {
        match c.axis {
            AXIS_WGHT => axes.weight = c.value,
            AXIS_WDTH => axes.width = c.value,
            AXIS_SLNT => axes.slant = c.value,
            AXIS_ITAL => axes.italic = c.value,
            _ => {}
        }
    }
    font_style_from_axes(axes)
}

/// Mirrors `SkFontMgr::Request::SetModel`.
pub fn set_model_from_font_style(style: FontStyle) -> [VariationCoordinate; 4] {
    [
        VariationCoordinate {
            axis: AXIS_WGHT,
            value: style.weight as f32,
        },
        VariationCoordinate {
            axis: AXIS_WDTH,
            value: font_width_axis_value_for_style_width(style.width),
        },
        VariationCoordinate {
            axis: AXIS_SLNT,
            value: if style.slant == FontStyleSlant::Oblique as u8 {
                -20.0
            } else {
                0.0
            },
        },
        VariationCoordinate {
            axis: AXIS_ITAL,
            value: if style.slant == FontStyleSlant::Italic as u8 {
                1.0
            } else {
                0.0
            },
        },
    ]
}

/// Computes the CSS3 match score of `current` against `pattern`
/// matching `SkFontStyleSet::matchStyleCSS3`.
pub fn css3_style_match_score(pattern: FontStyle, current: FontStyle) -> i32 {
    let mut score: i32 = 0;

    // 1. CSS stretch / SkFontStyle::Width
    if pattern.width <= FontStyle::NORMAL_WIDTH {
        if current.width <= pattern.width {
            score += 10 - pattern.width + current.width;
        } else {
            score += 10 - current.width;
        }
    } else if current.width > pattern.width {
        score += 10 + pattern.width - current.width;
    } else {
        score += current.width;
    }
    score <<= 8;

    // 2. CSS style / SkFontStyle::Slant
    const SLANT_SCORE: [[i32; 3]; 3] = [
        //          Upright, Italic, Oblique  [current]
        /* Upright */ [3, 1, 2],
        /* Italic  */ [1, 3, 2],
        /* Oblique */ [1, 2, 3],
    ];
    let p_slant = (pattern.slant as usize).min(2);
    let c_slant = (current.slant as usize).min(2);
    score += SLANT_SCORE[p_slant][c_slant];
    score <<= 8;

    // 3. CSS weight / SkFontStyle::Weight
    if pattern.weight == current.weight {
        score += 1000;
    } else if pattern.weight < 400 {
        if current.weight <= pattern.weight {
            score += 1000 - pattern.weight + current.weight;
        } else {
            score += 1000 - current.weight;
        }
    } else if pattern.weight <= 500 {
        if current.weight >= pattern.weight && current.weight <= 500 {
            score += 1000 + pattern.weight - current.weight;
        } else if current.weight <= pattern.weight {
            score += 500 + current.weight;
        } else {
            score += 1000 - current.weight;
        }
    } else if current.weight > pattern.weight {
        score += 1000 + pattern.weight - current.weight;
    } else {
        score += current.weight;
    }

    score
}

/// Returns the best matching index in `candidates` using CSS3 matching rules.
/// Returns `0` for an empty slice (caller checks empty before calling).
pub fn match_style_css3_best_index(pattern: FontStyle, candidates: &[FontStyle]) -> usize {
    let mut best_score: i32 = 0;
    let mut best_index: usize = 0;
    for (i, &cand) in candidates.iter().enumerate() {
        let s = css3_style_match_score(pattern, cand);
        if best_score < s {
            best_score = s;
            best_index = i;
        }
    }
    best_index
}

/// Mirrors `SkTypeface::SerializeBehavior`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SerializeBehavior {
    DoIncludeData = 0,
    DontIncludeData = 1,
    IncludeDataIfLocal = 2,
}

impl SerializeBehavior {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::DoIncludeData),
            1 => Some(Self::DontIncludeData),
            2 => Some(Self::IncludeDataIfLocal),
            _ => None,
        }
    }

    #[inline]
    pub const fn should_serialize_data(self, is_local_data: bool) -> bool {
        match self {
            Self::DoIncludeData => true,
            Self::DontIncludeData => false,
            Self::IncludeDataIfLocal => is_local_data,
        }
    }
}

/// Evaluates OS/2 `fsType` raw 16-bit big-endian field and returns the
/// `SkAdvancedTypefaceMetrics::FontFlags` bits to OR into `fFlags`:
/// - bit 1 (`kNotEmbeddable_FontFlag = 0x2`) if Bitmap or (Restricted && !(PreviewPrint || Editable))
/// - bit 2 (`kNotSubsettable_FontFlag = 0x4`) if NoSubsetting
///
/// Mirrors `SkOTTableOS2::Version::V2::Type::Field` in `src/sfnt/SkOTTable_OS_2_V2.h` and
/// `SkTypeface::getAdvancedMetrics` in `src/core/SkTypeface.cpp`.
pub fn os2_fstype_to_advanced_flags(fs_type_raw_be_bytes: [u8; 2]) -> u8 {
    // In SkOTTable_OS_2_V2.h (little-endian host):
    // byte 0 (big-endian high byte):
    //   bit 0: NoSubsetting (0x0100 in BE u16)
    //   bit 1: Bitmap       (0x0200 in BE u16)
    // byte 1 (big-endian low byte):
    //   bit 1: Restricted   (0x0002 in BE u16)
    //   bit 2: PreviewPrint (0x0004 in BE u16)
    //   bit 3: Editable     (0x0008 in BE u16)
    let val = u16::from_be_bytes(fs_type_raw_be_bytes);
    let restricted = (val & 0x0002) != 0;
    let preview_print = (val & 0x0004) != 0;
    let editable = (val & 0x0008) != 0;
    let no_subsetting = (val & 0x0100) != 0;
    let bitmap = (val & 0x0200) != 0;

    let mut flags: u8 = 0;
    if bitmap || (restricted && !(preview_print || editable)) {
        flags |= 0x2; // kNotEmbeddable_FontFlag
    }
    if no_subsetting {
        flags |= 0x4; // kNotSubsettable_FontFlag
    }
    flags
}

/// Mirrors `SkTypefaceCache::NextTypefaceID` overflow guard.
/// Returns `Some(id as u32)` if `id <= u32::MAX as u64`, otherwise `None`.
#[inline]
pub const fn next_typeface_id_checked(id: u64) -> Option<u32> {
    if id > u32::MAX as u64 {
        None
    } else {
        Some(id as u32)
    }
}

/// Computes how many entries `SkTypefaceCache::add` should purge (`limit >> 2`) if `current_size >= limit`.
#[inline]
pub const fn typeface_cache_purge_count(current_size: i32, limit: i32) -> i32 {
    if current_size >= limit {
        limit >> 2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_width_axis_conversion() {
        assert_eq!(font_width_axis_value_for_style_width(5), 100.0);
        assert_eq!(font_style_width_for_width_axis_value(100.0), 5);
        assert_eq!(font_style_width_for_width_axis_value(50.0), 1);
        assert_eq!(font_style_width_for_width_axis_value(200.0), 9);
        assert_eq!(font_style_width_for_width_axis_value(75.0), 3);
    }

    #[test]
    fn test_css3_match_weights_and_slants() {
        let regular = FontStyle::new(400, 5, FontStyleSlant::Upright);
        let medium = FontStyle::new(500, 5, FontStyleSlant::Upright);
        let bold = FontStyle::new(700, 5, FontStyleSlant::Upright);
        let italic = FontStyle::new(400, 5, FontStyleSlant::Italic);
        let oblique = FontStyle::new(400, 5, FontStyleSlant::Oblique);

        // Requesting 400 prefers 500 over 700
        let cands = [bold, medium];
        assert_eq!(match_style_css3_best_index(regular, &cands), 1);

        // Requesting italic prefers oblique over upright
        let cands2 = [regular, oblique];
        assert_eq!(match_style_css3_best_index(italic, &cands2), 1);

        // Exact weight wins
        let cands3 = [medium, bold, regular];
        assert_eq!(match_style_css3_best_index(regular, &cands3), 2);
    }

    #[test]
    fn test_descriptor_style_roundtrip() {
        let style = FontStyle::new(700, 7, FontStyleSlant::Italic);
        let (bits, axes) = pack_descriptor_style(style);
        assert_eq!(axes.weight, 700.0);
        assert_eq!(axes.width, 125.0);
        assert_eq!(axes.slant, -20.0);
        assert_eq!(axes.italic, 1.0);
        let unpacked = unpack_descriptor_style_bits(bits as usize);
        assert_eq!(unpacked, axes);
        assert_eq!(font_style_from_axes(unpacked), style);
    }

    #[test]
    fn test_os2_fstype_flags() {
        // Restricted only -> not embeddable
        assert_eq!(os2_fstype_to_advanced_flags(0x0002u16.to_be_bytes()), 0x2);
        // Restricted + PreviewPrint -> embeddable
        assert_eq!(os2_fstype_to_advanced_flags(0x0006u16.to_be_bytes()), 0x0);
        // NoSubsetting + Bitmap -> both
        assert_eq!(os2_fstype_to_advanced_flags(0x0300u16.to_be_bytes()), 0x6);
    }
}
