//! Safe Rust port of `SkFont.cpp`, `SkFont_serial.cpp`, and `SkFontMetricsPriv.cpp`.

use crate::{FontEdging, FontHinting, FontMetrics, TextEncoding};
use skia_rs_geometry::{
    cpp_max_f32, scalar_nearly_zero, Matrix, Point, Rect, SK_SCALAR_NEARLY_ZERO,
};
use skia_rs_math::utf;

/// Mirrors `SkFont::PrivFlags` in `include/core/SkFont.h`.
pub const FLAG_FORCE_AUTO_HINTING: u8 = 1 << 0;
pub const FLAG_EMBEDDED_BITMAPS: u8 = 1 << 1;
pub const FLAG_SUBPIXEL: u8 = 1 << 2;
pub const FLAG_LINEAR_METRICS: u8 = 1 << 3;
pub const FLAG_EMBOLDEN: u8 = 1 << 4;
pub const FLAG_BASELINE_SNAP: u8 = 1 << 5;
pub const ALL_FLAGS: u8 = FLAG_FORCE_AUTO_HINTING
    | FLAG_EMBEDDED_BITMAPS
    | FLAG_SUBPIXEL
    | FLAG_LINEAR_METRICS
    | FLAG_EMBOLDEN
    | FLAG_BASELINE_SNAP;

/// Mirrors `SkFontPriv::kCanonicalTextSizeForPaths`.
pub const CANONICAL_TEXT_SIZE_FOR_PATHS: i32 = 64;

/// Mirrors `SkPaintDefaults_TextSize`.
pub const DEFAULT_TEXT_SIZE: f32 = 12.0;

/// Mirrors `valid_size` in `SkFont.cpp`: `std::max<SkScalar>(0, size)`.
#[inline]
pub fn valid_size(size: f32) -> f32 {
    cpp_max_f32(0.0, size)
}

/// Mirrors `set_clear_mask` in `SkFont.cpp`.
#[inline]
pub const fn set_clear_mask(bits: u8, cond: bool, mask: u8) -> u8 {
    if cond {
        bits | mask
    } else {
        bits & !mask
    }
}

/// Mirrors `SkFont::hasSomeAntiAliasing`.
#[inline]
pub const fn has_some_anti_aliasing(edging: u8) -> bool {
    edging == FontEdging::AntiAlias as u8 || edging == FontEdging::SubpixelAntiAlias as u8
}

/// Result of `SkFont::setupForAsPaths`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SetupForAsPathsResult {
    pub flags: u8,
    pub edging: u8,
    pub hinting: u8,
    pub size: f32,
    pub scale: f32,
}

/// Mirrors `SkFont::setupForAsPaths` field updates and returned scale.
pub fn setup_for_as_paths(flags: u8, edging: u8, size: f32) -> SetupForAsPathsResult {
    const FLAGS_TO_IGNORE: u8 = FLAG_EMBEDDED_BITMAPS | FLAG_FORCE_AUTO_HINTING;
    let new_flags = (flags & !FLAGS_TO_IGNORE) | FLAG_SUBPIXEL;
    let new_hinting = FontHinting::None as u8;
    let new_edging = if edging == FontEdging::SubpixelAntiAlias as u8 {
        FontEdging::AntiAlias as u8
    } else {
        edging
    };
    let canonical = CANONICAL_TEXT_SIZE_FOR_PATHS as f32;
    let new_size = valid_size(canonical);
    let scale = size / canonical;
    SetupForAsPathsResult {
        flags: new_flags,
        edging: new_edging,
        hinting: new_hinting,
        size: new_size,
        scale,
    }
}

/// Mirrors `SkFontPriv::MakeTextMatrix(size, scaleX, skewX)`.
pub fn make_text_matrix(size: f32, scale_x: f32, skew_x: f32) -> Matrix {
    let mut m = Matrix::identity();
    m.set_scale(size * scale_x, size);
    if skew_x != 0.0 {
        m.post_skew(skew_x, 0.0);
    }
    m
}

/// Mirrors `SkFontPriv::GetFontBounds`.
pub fn get_font_bounds(size: f32, scale_x: f32, skew_x: f32, typeface_bounds: Rect) -> Rect {
    let mut m = Matrix::identity();
    m.set_scale(size * scale_x, size);
    m.post_skew(skew_x, 0.0);
    match m.map_rect_non_perspective(typeface_bounds) {
        Some((r, _)) => r,
        None => Rect::make_empty(),
    }
}

/// Mirrors `SkFontPriv::ApproximateTransformedTextSize`.
pub fn approximate_transformed_text_size(
    font_size: f32,
    matrix: &Matrix,
    text_location: Point,
) -> f32 {
    if !matrix.has_perspective() {
        font_size * matrix.get_max_scale()
    } else {
        let max_scale_sq = matrix.differential_area_scale(text_location);
        if max_scale_sq.is_finite() && !scalar_nearly_zero(max_scale_sq, SK_SCALAR_NEARLY_ZERO) {
            font_size * max_scale_sq.sqrt()
        } else {
            -font_size
        }
    }
}

/// Mirrors `SkFontPriv::CountTextElements`.
pub fn count_text_elements(bytes: &[u8], encoding: TextEncoding, ptr_aligned2: bool) -> usize {
    match encoding {
        TextEncoding::Utf8 => utf::count_utf8(bytes) as isize as usize,
        TextEncoding::Utf16 => {
            if !ptr_aligned2 || (bytes.len() & 1) != 0 {
                return (-1isize) as usize;
            }
            let mut units = Vec::with_capacity(bytes.len() >> 1);
            for chunk in bytes.chunks_exact(2) {
                units.push(u16::from_ne_bytes([chunk[0], chunk[1]]));
            }
            utf::count_utf16(&units) as isize as usize
        }
        TextEncoding::Utf32 => bytes.len() >> 2,
        TextEncoding::GlyphId => bytes.len() >> 1,
    }
}

/// Decodes UTF-8 or UTF-16 bytes into UTF-32 codepoints (`SkUnichar`), matching `SkConvertToUTF32`.
/// Returns number of codepoints written to `out`.
pub fn decode_text_to_utf32(bytes: &[u8], encoding: TextEncoding, out: &mut [i32]) -> usize {
    match encoding {
        TextEncoding::Utf8 => {
            let mut pos = 0usize;
            let mut idx = 0usize;
            while pos < bytes.len() && idx < out.len() {
                let (cp, consumed) = utf::next_utf8(&bytes[pos..]);
                out[idx] = cp;
                idx += 1;
                pos += consumed;
            }
            idx
        }
        TextEncoding::Utf16 => {
            let unit_count = bytes.len() >> 1;
            let mut units = Vec::with_capacity(unit_count);
            for chunk in bytes.chunks_exact(2) {
                units.push(u16::from_ne_bytes([chunk[0], chunk[1]]));
            }
            let mut pos = 0usize;
            let mut idx = 0usize;
            while pos < units.len() && idx < out.len() {
                let (cp, consumed) = utf::next_utf16(&units[pos..]);
                out[idx] = cp;
                idx += 1;
                pos += consumed;
            }
            idx
        }
        TextEncoding::Utf32 => {
            let n = (bytes.len() >> 2).min(out.len());
            for (dst, chunk) in out.iter_mut().take(n).zip(bytes.chunks_exact(4)) {
                *dst = i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
            n
        }
        TextEncoding::GlyphId => 0,
    }
}

/// Mirrors `SkFont::measureText` accumulation over pre-fetched glyph advances & rects.
pub fn measure_glyphs(
    advances_x: &[f32],
    glyph_rects: Option<&[Rect]>,
    strike_to_source_scale: f32,
    out_bounds: Option<&mut Rect>,
) -> f32 {
    if advances_x.is_empty() {
        if let Some(b) = out_bounds {
            *b = Rect::make_empty();
        }
        return 0.0;
    }

    let mut width = 0.0f32;
    if let (Some(rects), Some(b)) = (glyph_rects, out_bounds) {
        let mut bounds = rects[0];
        width = advances_x[0];
        for i in 1..advances_x.len() {
            let r = rects[i].make_offset(width, 0.0);
            bounds.join(r);
            width += advances_x[i];
        }
        if strike_to_source_scale != 1.0 {
            width *= strike_to_source_scale;
            bounds.left *= strike_to_source_scale;
            bounds.top *= strike_to_source_scale;
            bounds.right *= strike_to_source_scale;
            bounds.bottom *= strike_to_source_scale;
        }
        *b = bounds;
    } else {
        for &adv in advances_x {
            width += adv;
        }
        if strike_to_source_scale != 1.0 {
            width *= strike_to_source_scale;
        }
    }

    width
}

/// Mirrors `scale_pos` in `SkFont.cpp` and `scale_rect` in `SkStrikeRef.cpp`.
#[inline]
pub fn scale_rect(r: Rect, s: f32) -> Rect {
    Rect::make_ltrb(r.left * s, r.top * s, r.right * s, r.bottom * s)
}

/// Mirrors `SkFont::getPos` position accumulation.
pub fn accumulate_positions(
    advances: &[Point],
    strike_to_source_scale: f32,
    origin: Point,
    out_pos: &mut [Point],
) {
    let n = out_pos.len().min(advances.len());
    let mut sum = origin;
    for i in 0..n {
        out_pos[i] = sum;
        sum.x += advances[i].x * strike_to_source_scale;
        sum.y += advances[i].y * strike_to_source_scale;
    }
}

/// Mirrors `SkFont::getXPos` x-position accumulation.
pub fn accumulate_x_positions(
    advances_x: &[f32],
    strike_to_source_scale: f32,
    origin: f32,
    out_xpos: &mut [f32],
) {
    let n = out_xpos.len().min(advances_x.len());
    let mut loc = origin;
    for i in 0..n {
        out_xpos[i] = loc;
        loc += advances_x[i] * strike_to_source_scale;
    }
}

/// Mirrors `SkFontPriv::GlyphsToUnichars` lookup loop.
pub fn glyphs_to_unichars(glyphs: &[u16], glyph_to_unicode: &[i32], out_unichars: &mut [i32]) {
    let num_glyphs = glyph_to_unicode.len();
    let n = glyphs.len().min(out_unichars.len());
    for i in 0..n {
        let id = glyphs[i] as usize;
        out_unichars[i] = if id < num_glyphs {
            glyph_to_unicode[id]
        } else {
            0xFFFD
        };
    }
}

/// Mirrors `SkFontPriv::ScaleFontMetrics`.
#[inline]
pub fn scale_font_metrics(metrics: &mut FontMetrics, scale: f32) {
    metrics.scale(scale);
}

// ============================================================================
// SkFont_serial.cpp bit packing / unpacking
// ============================================================================

pub const SERIAL_SIZE_IS_BYTE_BIT: u32 = 1 << 31;
pub const SERIAL_HAS_SCALEX_BIT: u32 = 1 << 30;
pub const SERIAL_HAS_SKEWX_BIT: u32 = 1 << 29;
pub const SERIAL_HAS_TYPEFACE_BIT: u32 = 1 << 28;

pub const SERIAL_SHIFT_FOR_SIZE: u32 = 16;
pub const SERIAL_MASK_FOR_SIZE: u32 = 0xFF;

pub const SERIAL_SHIFT_FOR_FLAGS: u32 = 4;
pub const SERIAL_MASK_FOR_FLAGS: u32 = 0xFFF;

pub const SERIAL_SHIFT_FOR_EDGING: u32 = 2;
pub const SERIAL_MASK_FOR_EDGING: u32 = 0x3;

pub const SERIAL_SHIFT_FOR_HINTING: u32 = 0;
pub const SERIAL_MASK_FOR_HINTING: u32 = 0x3;

#[inline]
fn scalar_is_byte(x: f32) -> bool {
    let ix = x as i32;
    (ix as f32) == x && ix >= 0 && (ix as u32) <= SERIAL_MASK_FOR_SIZE
}

/// Result of `pack_font_serial` matching `SkFontPriv::Flatten`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FontSerialPackResult {
    pub packed: u32,
    pub write_size: bool,
    pub write_scale_x: bool,
    pub write_skew_x: bool,
    pub write_typeface: bool,
}

/// Packs `SkFont` fields into the 32-bit header matching `SkFontPriv::Flatten`.
pub fn pack_font_serial(
    size: f32,
    scale_x: f32,
    skew_x: f32,
    flags: u8,
    edging: u8,
    hinting: u8,
    has_typeface: bool,
) -> FontSerialPackResult {
    let mut packed: u32 = 0;
    packed |= (flags as u32) << SERIAL_SHIFT_FOR_FLAGS;
    packed |= (edging as u32) << SERIAL_SHIFT_FOR_EDGING;
    packed |= (hinting as u32) << SERIAL_SHIFT_FOR_HINTING;

    let size_is_byte = scalar_is_byte(size);
    if size_is_byte {
        packed |= SERIAL_SIZE_IS_BYTE_BIT;
        packed |= ((size as i32) as u32) << SERIAL_SHIFT_FOR_SIZE;
    }
    let has_scale_x = scale_x != 1.0;
    if has_scale_x {
        packed |= SERIAL_HAS_SCALEX_BIT;
    }
    let has_skew_x = skew_x != 0.0;
    if has_skew_x {
        packed |= SERIAL_HAS_SKEWX_BIT;
    }
    if has_typeface {
        packed |= SERIAL_HAS_TYPEFACE_BIT;
    }

    FontSerialPackResult {
        packed,
        write_size: !size_is_byte,
        write_scale_x: has_scale_x,
        write_skew_x: has_skew_x,
        write_typeface: has_typeface,
    }
}

/// Result of `unpack_font_serial` matching `SkFontPriv::Unflatten`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontSerialUnpackResult {
    pub has_byte_size: bool,
    pub byte_size: f32,
    pub has_scale_x: bool,
    pub has_skew_x: bool,
    pub has_typeface: bool,
    pub flags: u8,
    pub edging: u8,
    pub hinting: u8,
}

/// Unpacks the 32-bit header matching `SkFontPriv::Unflatten`.
pub fn unpack_font_serial(packed: u32) -> FontSerialUnpackResult {
    let has_byte_size = (packed & SERIAL_SIZE_IS_BYTE_BIT) != 0;
    let byte_size = if has_byte_size {
        ((packed >> SERIAL_SHIFT_FOR_SIZE) & SERIAL_MASK_FOR_SIZE) as f32
    } else {
        0.0
    };
    let flags = (((packed >> SERIAL_SHIFT_FOR_FLAGS) & (ALL_FLAGS as u32)) as u8) & ALL_FLAGS;
    let mut edging = (packed >> SERIAL_SHIFT_FOR_EDGING) & SERIAL_MASK_FOR_EDGING;
    if edging > FontEdging::SubpixelAntiAlias as u32 {
        edging = 0;
    }
    let mut hinting = (packed >> SERIAL_SHIFT_FOR_HINTING) & SERIAL_MASK_FOR_HINTING;
    if hinting > FontHinting::Full as u32 {
        hinting = 0;
    }

    FontSerialUnpackResult {
        has_byte_size,
        byte_size,
        has_scale_x: (packed & SERIAL_HAS_SCALEX_BIT) != 0,
        has_skew_x: (packed & SERIAL_HAS_SKEWX_BIT) != 0,
        has_typeface: (packed & SERIAL_HAS_TYPEFACE_BIT) != 0,
        flags,
        edging: edging as u8,
        hinting: hinting as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_size_nan_and_neg() {
        assert_eq!(valid_size(-5.0), 0.0);
        assert_eq!(valid_size(0.0), 0.0);
        assert_eq!(valid_size(14.5), 14.5);
        assert_eq!(valid_size(f32::NAN), 0.0);
    }

    #[test]
    fn test_setup_for_as_paths() {
        let r = setup_for_as_paths(
            FLAG_EMBEDDED_BITMAPS | FLAG_FORCE_AUTO_HINTING | FLAG_BASELINE_SNAP,
            FontEdging::SubpixelAntiAlias as u8,
            128.0,
        );
        assert_eq!(r.flags, FLAG_BASELINE_SNAP | FLAG_SUBPIXEL);
        assert_eq!(r.edging, FontEdging::AntiAlias as u8);
        assert_eq!(r.hinting, FontHinting::None as u8);
        assert_eq!(r.size, 64.0);
        assert_eq!(r.scale, 2.0);
    }

    #[test]
    fn test_font_serial_roundtrip() {
        let p = pack_font_serial(
            16.0,
            1.25,
            -0.25,
            FLAG_BASELINE_SNAP | FLAG_SUBPIXEL,
            FontEdging::AntiAlias as u8,
            FontHinting::Normal as u8,
            true,
        );
        assert!(!p.write_size);
        assert!(p.write_scale_x);
        assert!(p.write_skew_x);
        assert!(p.write_typeface);

        let u = unpack_font_serial(p.packed);
        assert!(u.has_byte_size);
        assert_eq!(u.byte_size, 16.0);
        assert!(u.has_scale_x);
        assert!(u.has_skew_x);
        assert!(u.has_typeface);
        assert_eq!(u.flags, FLAG_BASELINE_SNAP | FLAG_SUBPIXEL);
        assert_eq!(u.edging, FontEdging::AntiAlias as u8);
        assert_eq!(u.hinting, FontHinting::Normal as u8);
    }

    #[test]
    fn test_font_serial_non_byte_size_and_invalid_edging() {
        let p = pack_font_serial(12.5, 1.0, 0.0, 0, 0, 0, false);
        assert!(p.write_size);
        assert!(!p.write_scale_x);
        assert!(!p.write_skew_x);
        assert!(!p.write_typeface);

        // Edging value 3 is out of range -> clamped to 0
        let u = unpack_font_serial(3 << SERIAL_SHIFT_FOR_EDGING);
        assert_eq!(u.edging, 0);
    }

    #[test]
    fn test_count_and_decode_text() {
        let s = "A\u{00E9}\u{1F600}";
        assert_eq!(count_text_elements(s.as_bytes(), TextEncoding::Utf8, true), 3);
        let mut cps = [0i32; 4];
        let n = decode_text_to_utf32(s.as_bytes(), TextEncoding::Utf8, &mut cps);
        assert_eq!(n, 3);
        assert_eq!(&cps[..3], &[0x41, 0xE9, 0x1F600]);
    }

    #[test]
    fn test_measure_glyphs() {
        let adv = [10.0, 20.0];
        let rects = [
            Rect::make_ltrb(0.0, -8.0, 8.0, 2.0),
            Rect::make_ltrb(1.0, -10.0, 15.0, 0.0),
        ];
        let mut b = Rect::make_empty();
        let w = measure_glyphs(&adv, Some(&rects), 2.0, Some(&mut b));
        assert_eq!(w, 60.0);
        assert_eq!(b, Rect::make_ltrb(0.0, -20.0, 50.0, 4.0));
    }
}
