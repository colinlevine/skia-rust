//! Safe Rust port of `src/core/SkBitmap.cpp` algorithms.

use skia_rs_color::image_info::{
    color_type_bytes_per_pixel, color_type_shift_per_pixel, image_info_valid_row_bytes,
    validate_alpha_type, AlphaType, ColorType,
};
use skia_rs_core_types::align4;
use skia_rs_geometry::{IPoint, IRect};

/// Validated result of `SkBitmap::setInfo`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BitmapSetInfoResult {
    pub alpha_type: AlphaType,
    pub row_bytes: usize,
}

/// Exact port of the validation logic in `SkBitmap::setInfo(const SkImageInfo&, size_t rowBytes)`.
pub fn bitmap_validate_set_info(
    width: i32,
    height: i32,
    ct: ColorType,
    at: AlphaType,
    mut row_bytes: usize,
) -> Option<BitmapSetInfoResult> {
    let new_at = validate_alpha_type(ct, at)?;

    // Require that minRowBytes64 and rowBytes both fit in signed 32-bit int.
    let mrb: i64 = (width as i64).wrapping_mul(color_type_bytes_per_pixel(ct) as i64);
    if mrb < 0 || mrb > i32::MAX as i64 {
        return None;
    }
    if row_bytes > i32::MAX as usize {
        return None;
    }
    if width < 0 || height < 0 {
        return None;
    }

    if ct == ColorType::Unknown {
        row_bytes = 0;
    } else if row_bytes == 0 {
        row_bytes = mrb as usize;
    } else if !image_info_valid_row_bytes(width, ct, row_bytes) {
        return None;
    }

    Some(BitmapSetInfoResult {
        alpha_type: new_at,
        row_bytes,
    })
}

/// Exact port of `SkBitmap::pixelRefOrigin()`.
/// `byte_offset` is `(const char*)fPixmap.addr() - (const char*)fPixelRef->pixels()`.
#[inline]
pub fn bitmap_pixel_ref_origin(byte_offset: usize, row_bytes: usize, ct: ColorType) -> IPoint {
    if row_bytes == 0 {
        return IPoint { x: 0, y: 0 };
    }
    let shift = color_type_shift_per_pixel(ct) as u32;
    let x = ((byte_offset % row_bytes) >> shift) as i32;
    let y = (byte_offset / row_bytes) as i32;
    IPoint { x, y }
}

/// Exact port of `SkBitmap::getAddr(x, y)` byte offset calculation.
#[inline]
pub const fn bitmap_addr_offset(x: i32, y: i32, row_bytes: usize, ct: ColorType) -> usize {
    let shift = color_type_shift_per_pixel(ct) as u32;
    (y as usize) * row_bytes + ((x as usize) << shift)
}

/// Exact port of `SkBitmap::setPixelRef` offset calculation: `dy * rowBytes + dx * bpp`.
#[inline]
pub const fn bitmap_pixel_ref_byte_offset(dx: i32, dy: i32, row_bytes: usize, ct: ColorType) -> usize {
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    (dy as usize) * row_bytes + (dx as usize) * bpp
}

/// Result of `SkBitmap::extractSubset` geometry calculation.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BitmapExtractSubsetResult {
    pub clipped: IRect,
    pub new_origin: IPoint,
}

/// Exact port of the intersection and origin offset calculation in `SkBitmap::extractSubset`.
pub fn bitmap_extract_subset(
    width: i32,
    height: i32,
    origin: IPoint,
    subset: IRect,
) -> Option<BitmapExtractSubsetResult> {
    let src_rect = IRect::from_wh(width, height);
    let mut r = IRect::make_empty();
    if !r.intersect_pair(src_rect, subset) {
        return None;
    }
    Some(BitmapExtractSubsetResult {
        clipped: r,
        new_origin: IPoint {
            x: origin.x.wrapping_add(r.left),
            y: origin.y.wrapping_add(r.top),
        },
    })
}

/// Result of initial mask setup in `SkBitmap::extractAlpha`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ExtractAlphaMaskSetup {
    pub bounds: IRect,
    pub row_bytes: u32,
}

/// Exact port of `SkBitmap::extractAlpha` initial mask bounds & rowBytes calculation.
pub fn bitmap_extract_alpha_setup(width: i32, height: i32) -> Option<ExtractAlphaMaskSetup> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(ExtractAlphaMaskSetup {
        bounds: IRect::from_wh(width, height),
        row_bytes: align4(width as usize) as u32,
    })
}

/// Zero-fills an alpha buffer when `src.peekPixels` fails inside `GetBitmapAlpha`.
pub fn zero_alpha_buffer(alpha: &mut [u8], width: usize, height: usize, alpha_row_bytes: usize) {
    for y in 0..height {
        let start = y * alpha_row_bytes;
        let end = start + width;
        if end <= alpha.len() {
            alpha[start..end].fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_info_validation() {
        // 565 forces Opaque
        let r = bitmap_validate_set_info(10, 10, ColorType::Rgb565, AlphaType::Premul, 0).unwrap();
        assert_eq!(r.alpha_type, AlphaType::Opaque);
        assert_eq!(r.row_bytes, 20);

        // Unknown forces row_bytes = 0 and alpha_type = Unknown
        let r = bitmap_validate_set_info(10, 10, ColorType::Unknown, AlphaType::Premul, 100).unwrap();
        assert_eq!(r.alpha_type, AlphaType::Unknown);
        assert_eq!(r.row_bytes, 0);

        // Invalid alpha type for RGBA_8888
        assert!(bitmap_validate_set_info(10, 10, ColorType::Rgba8888, AlphaType::Unknown, 0).is_none());

        // Row bytes > i32::MAX fails
        assert!(bitmap_validate_set_info(10, 10, ColorType::Rgba8888, AlphaType::Premul, (i32::MAX as usize) + 4).is_none());
    }

    #[test]
    fn pixel_ref_origin_and_subset() {
        let rb = 100 * 4;
        let off = bitmap_pixel_ref_byte_offset(7, 13, rb, ColorType::Rgba8888);
        assert_eq!(
            bitmap_pixel_ref_origin(off, rb, ColorType::Rgba8888),
            IPoint { x: 7, y: 13 }
        );

        let sub = bitmap_extract_subset(
            50,
            40,
            IPoint { x: 7, y: 13 },
            IRect::make_ltrb(10, 5, 100, 30),
        )
        .unwrap();
        assert_eq!(sub.clipped, IRect::make_ltrb(10, 5, 50, 30));
        assert_eq!(sub.new_origin, IPoint { x: 17, y: 18 });

        // Disjoint subset fails
        assert!(bitmap_extract_subset(
            50,
            40,
            IPoint { x: 0, y: 0 },
            IRect::make_ltrb(60, 0, 70, 10)
        )
        .is_none());
    }
}
