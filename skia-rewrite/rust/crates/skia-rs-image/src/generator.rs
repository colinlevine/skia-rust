//! Safe Rust port of `src/core/SkImageGenerator.cpp` helpers and validation.

use skia_rs_bitmap::pixel_ref::next_image_id;
use skia_rs_color::image_info::ColorType;

use crate::image::image_info_min_row_bytes;

/// Sentinel matching `kNeedNewImageUniqueID` (0).
pub const NEED_NEW_IMAGE_UNIQUE_ID: u32 = 0;

/// Resolves an image or generator unique ID: allocates a fresh even non-zero ID from the shared
/// `SkNextID::ImageID()` sequence when `unique_id == kNeedNewImageUniqueID` (0).
#[inline]
pub fn resolve_unique_id(unique_id: u32) -> u32 {
    if unique_id == NEED_NEW_IMAGE_UNIQUE_ID {
        next_image_id()
    } else {
        unique_id
    }
}

/// Exact port of argument validation in `SkImageGenerator::getPixels`.
#[inline]
pub fn validate_get_pixels_args(
    width: i32,
    ct: ColorType,
    has_pixels: bool,
    row_bytes: usize,
) -> bool {
    if ct == ColorType::Unknown {
        return false;
    }
    if !has_pixels {
        return false;
    }
    if row_bytes < image_info_min_row_bytes(width, ct) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_id_preserves_nonzero_and_allocates_zero() {
        assert_eq!(resolve_unique_id(42), 42);
        let a = resolve_unique_id(NEED_NEW_IMAGE_UNIQUE_ID);
        let b = resolve_unique_id(NEED_NEW_IMAGE_UNIQUE_ID);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
        assert_eq!(a & 1, 0);
        assert_eq!(b & 1, 0);
    }

    #[test]
    fn get_pixels_arg_validation() {
        assert!(validate_get_pixels_args(16, ColorType::Rgba8888, true, 64));
        assert!(validate_get_pixels_args(16, ColorType::Rgba8888, true, 128));
        assert!(!validate_get_pixels_args(16, ColorType::Unknown, true, 64));
        assert!(!validate_get_pixels_args(16, ColorType::Rgba8888, false, 64));
        assert!(!validate_get_pixels_args(16, ColorType::Rgba8888, true, 63));
        // Zero-width with non-null pixels and 0 rowBytes is allowed by SkImageGenerator::getPixels
        assert!(validate_get_pixels_args(0, ColorType::Rgba8888, true, 0));
    }
}
