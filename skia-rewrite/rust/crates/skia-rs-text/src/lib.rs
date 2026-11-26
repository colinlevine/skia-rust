#![forbid(unsafe_code)]

//! Safe Rust port of Skia Phase 1 Layer 5.1 text subsystem (`skia-rs-text`).
//!
//! Covers 21 C++ files:
//! - `:core` (20 files):
//!   `SkFont.cpp`, `SkFontDescriptor.cpp`, `SkFontMetricsPriv.cpp`, `SkFontMgr.cpp`,
//!   `SkFontStream.cpp`, `SkFont_serial.cpp`, `SkGlyph.cpp`, `SkGlyphRunPainter.cpp`,
//!   `SkScalerContext.cpp`, `SkStrike.cpp`, `SkStrikeCache.cpp`, `SkStrikeRef.cpp`,
//!   `SkStrikeSpec.cpp`, `SkTextBlob.cpp`, `SkTypeface.cpp`, `SkTypefaceCache.cpp`,
//!   `SkTypeface_remote.cpp`, `src/text/GlyphRun.cpp`, `src/text/SlugFromBuffer.cpp`,
//!   `src/text/StrikeForGPU.cpp`
//! - `:skia` (1 file):
//!   `src/utils/SkTextUtils.cpp`

pub mod font;
pub mod font_descriptor;
pub mod glyph;
pub mod scaler_context;
pub mod sfnt;
pub mod strike;
pub mod text_blob;

/// Mirrors `SkTextEncoding` in `include/core/SkFontTypes.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8 = 0,
    Utf16 = 1,
    Utf32 = 2,
    GlyphId = 3,
}

impl TextEncoding {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Utf8),
            1 => Some(Self::Utf16),
            2 => Some(Self::Utf32),
            3 => Some(Self::GlyphId),
            _ => None,
        }
    }
}

/// Mirrors `SkFontHinting` in `include/core/SkFontTypes.h`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontHinting {
    None = 0,
    Slight = 1,
    Normal = 2,
    Full = 3,
}

impl FontHinting {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Slight),
            2 => Some(Self::Normal),
            3 => Some(Self::Full),
            _ => None,
        }
    }
}

/// Mirrors `SkFont::Edging` in `include/core/SkFont.h`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontEdging {
    Alias = 0,
    AntiAlias = 1,
    SubpixelAntiAlias = 2,
}

impl FontEdging {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Alias),
            1 => Some(Self::AntiAlias),
            2 => Some(Self::SubpixelAntiAlias),
            _ => None,
        }
    }
}

/// Mirrors `SkMask::Format` in `src/core/SkMask.h`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MaskFormat {
    BW = 0,
    A8 = 1,
    Format3D = 2,
    ARGB32 = 3,
    LCD16 = 4,
    SDF = 5,
}

pub const COUNT_MASK_FORMATS: u8 = 6;

impl MaskFormat {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::BW),
            1 => Some(Self::A8),
            2 => Some(Self::Format3D),
            3 => Some(Self::ARGB32),
            4 => Some(Self::LCD16),
            5 => Some(Self::SDF),
            _ => None,
        }
    }

    #[inline]
    pub const fn is_valid_u8(v: u8) -> bool {
        v < COUNT_MASK_FORMATS
    }
}

/// Mirrors `SkAxisAlignment` in `src/core/SkGlyph.h`.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AxisAlignment {
    None = 0,
    X = 1,
    Y = 2,
}

impl AxisAlignment {
    #[inline]
    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::X),
            2 => Some(Self::Y),
            _ => None,
        }
    }
}

/// Mirrors `SkPixelGeometry` in `include/core/SkSurfaceProps.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PixelGeometry {
    Unknown = 0,
    RgbH = 1,
    BgrH = 2,
    RgbV = 3,
    BgrV = 4,
}

impl PixelGeometry {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::RgbH),
            2 => Some(Self::BgrH),
            3 => Some(Self::RgbV),
            4 => Some(Self::BgrV),
            _ => None,
        }
    }
}

/// Mirrors `SkFontStyle::Slant` in `include/core/SkFontStyle.h`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontStyleSlant {
    Upright = 0,
    Italic = 1,
    Oblique = 2,
}

impl FontStyleSlant {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Upright),
            1 => Some(Self::Italic),
            2 => Some(Self::Oblique),
            _ => None,
        }
    }
}

/// Mirrors `SkFontStyle` packed representation (`int32_t fValue`).
///
/// Layout in `include/core/SkFontStyle.h`:
/// - low 16 bits: `uint16_t weight`
/// - next 8 bits: `uint8_t width`
/// - high 8 bits: `uint8_t slant`
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FontStyle {
    pub weight: i32,
    pub width: i32,
    pub slant: u8,
}

impl FontStyle {
    pub const NORMAL_WIDTH: i32 = 5;
    pub const NORMAL_WEIGHT: i32 = 400;
    pub const SEMIBOLD_WEIGHT: i32 = 600;

    #[inline]
    pub fn new(weight: i32, width: i32, slant: FontStyleSlant) -> Self {
        let w = weight.clamp(0, 1000);
        let wd = width.clamp(1, 9);
        Self {
            weight: w,
            width: wd,
            slant: slant as u8,
        }
    }

    #[inline]
    pub fn to_packed_i32(self) -> i32 {
        let w = (self.weight.clamp(0, 1000) as u32) & 0xFFFF;
        let wd = (self.width.clamp(1, 9) as u32) & 0xFF;
        let sl = (self.slant as u32) & 0xFF;
        (w | (wd << 16) | (sl << 24)) as i32
    }

    #[inline]
    pub fn from_packed_i32(v: i32) -> Self {
        let u = v as u32;
        Self {
            weight: (u & 0xFFFF) as i32,
            width: ((u >> 16) & 0xFF) as i32,
            slant: ((u >> 24) & 0xFF) as u8,
        }
    }
}

/// Mirrors `SkTextBlob::GlyphPositioning`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GlyphPositioning {
    Default = 0,
    Horizontal = 1,
    Full = 2,
    RSXform = 3,
}

impl GlyphPositioning {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Default),
            1 => Some(Self::Horizontal),
            2 => Some(Self::Full),
            3 => Some(Self::RSXform),
            _ => None,
        }
    }
}

/// Mirrors `SkTextUtils::Align`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left = 0,
    Center = 1,
    Right = 2,
}

impl TextAlign {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Left),
            1 => Some(Self::Center),
            2 => Some(Self::Right),
            _ => None,
        }
    }
}

/// Mirrors `SkFontMetrics` in `include/core/SkFontMetrics.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct FontMetrics {
    pub flags: u32,
    pub top: f32,
    pub ascent: f32,
    pub descent: f32,
    pub bottom: f32,
    pub leading: f32,
    pub avg_char_width: f32,
    pub max_char_width: f32,
    pub x_min: f32,
    pub x_max: f32,
    pub x_height: f32,
    pub cap_height: f32,
    pub underline_thickness: f32,
    pub underline_position: f32,
    pub strikeout_thickness: f32,
    pub strikeout_position: f32,
}

impl FontMetrics {
    pub const UNDERLINE_THICKNESS_IS_VALID_FLAG: u32 = 1 << 0;
    pub const UNDERLINE_POSITION_IS_VALID_FLAG: u32 = 1 << 1;
    pub const STRIKEOUT_THICKNESS_IS_VALID_FLAG: u32 = 1 << 2;
    pub const STRIKEOUT_POSITION_IS_VALID_FLAG: u32 = 1 << 3;
    pub const BOUNDS_INVALID_FLAG: u32 = 1 << 4;

    #[inline]
    pub const fn has_bounds(&self) -> bool {
        (self.flags & Self::BOUNDS_INVALID_FLAG) == 0
    }

    #[inline]
    pub fn spacing(&self) -> f32 {
        self.descent - self.ascent + self.leading
    }

    #[inline]
    pub fn scale(&mut self, scale: f32) {
        self.top *= scale;
        self.ascent *= scale;
        self.descent *= scale;
        self.bottom *= scale;
        self.leading *= scale;
        self.avg_char_width *= scale;
        self.max_char_width *= scale;
        self.x_min *= scale;
        self.x_max *= scale;
        self.x_height *= scale;
        self.cap_height *= scale;
        self.underline_thickness *= scale;
        self.underline_position *= scale;
        self.strikeout_thickness *= scale;
        self.strikeout_position *= scale;
    }
}

/// Helper matching `SkSetFourByteTag(a, b, c, d)`.
#[inline]
pub const fn four_byte_tag(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}
