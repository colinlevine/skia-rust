//! Safe Rust port of `src/codec/SkWbmpCodec.cpp`.

use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::ISize;

use crate::stream::{ByteReader, SliceReader};

/// Maximum valid WBMP width or height (`0xFFFF`).
pub const WBMP_MAX_DIM: u64 = 0xFFFF;

/// Exact port of `get_src_row_bytes` in `SkWbmpCodec.cpp`: `SkAlign8(width) >> 3`.
#[inline]
pub const fn wbmp_src_row_bytes(width: i32) -> usize {
    (((width as usize) + 7) & !7) >> 3
}

/// Exact port of `valid_color_type` in `SkWbmpCodec.cpp`.
#[inline]
pub const fn wbmp_valid_color_type(color_type: ColorType, has_color_space: bool) -> bool {
    match color_type {
        ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::Gray8 | ColorType::Rgb565 => true,
        ColorType::RgbaF16 => has_color_space,
        _ => false,
    }
}

/// Exact port of `SkCodecPriv::ValidAlpha`.
#[inline]
pub const fn valid_alpha(dst_alpha: AlphaType, src_is_opaque: bool) -> bool {
    if matches!(dst_alpha, AlphaType::Unknown) {
        return false;
    }
    if src_is_opaque {
        true
    } else {
        !matches!(dst_alpha, AlphaType::Opaque)
    }
}

/// Exact port of `SkWbmpCodec::conversionSupported`.
#[inline]
pub const fn wbmp_conversion_supported(
    dst_color_type: ColorType,
    dst_alpha_type: AlphaType,
    has_color_space: bool,
    src_is_opaque: bool,
) -> bool {
    wbmp_valid_color_type(dst_color_type, has_color_space)
        && valid_alpha(dst_alpha_type, src_is_opaque)
}

/// Reads a WBMP multi-byte integer (variable-length quantity).
/// Exact port of `read_mbf` in `SkWbmpCodec.cpp`.
pub fn read_mbf(reader: &mut impl ByteReader) -> Option<u64> {
    const LIMIT: u64 = 0xFE00_0000_0000_0000;
    const _: () = assert!(LIMIT == !((!0u64) >> 7));

    let mut n: u64 = 0;
    let mut byte = [0u8; 1];
    loop {
        if (n & LIMIT) != 0 {
            // Shifting left by 7 would overflow.
            return None;
        }
        if reader.read(&mut byte) != 1 {
            return None;
        }
        let data = byte[0];
        n = (n << 7) | u64::from(data & 0x7F);
        if (data & 0x80) == 0 {
            break;
        }
    }
    Some(n)
}

/// Reads and validates a WBMP header from `reader`, returning its `(width, height)`.
/// Exact port of `read_header` in `SkWbmpCodec.cpp`.
pub fn read_wbmp_header(reader: &mut impl ByteReader) -> Option<ISize> {
    let mut byte = [0u8; 1];

    // Type field must be 0.
    if reader.read(&mut byte) != 1 || byte[0] != 0 {
        return None;
    }
    // Fixed header field: bits 7 and 4..0 must be zero (`(data & 0x9F) == 0`).
    if reader.read(&mut byte) != 1 || (byte[0] & 0x9F) != 0 {
        return None;
    }

    let width = read_mbf(reader)?;
    if width == 0 || width > WBMP_MAX_DIM {
        return None;
    }
    let height = read_mbf(reader)?;
    if height == 0 || height > WBMP_MAX_DIM {
        return None;
    }

    Some(ISize {
        width: width as i32,
        height: height as i32,
    })
}

/// Exact port of `SkWbmpCodec::IsWbmp`.
#[inline]
pub fn is_wbmp(buffer: &[u8]) -> bool {
    let mut reader = SliceReader::new(buffer);
    read_wbmp_header(&mut reader).is_some()
}
