//! Safe Rust port of `src/image/SkImage_Raster.cpp` and `src/image/SkImage_RasterFactories.cpp`
//! algorithms, validation, and pixel-rect copy logic.

use skia_rs_bitmap::bitmap::bitmap_validate_set_info;
use skia_rs_color::image_info::{
    compute_byte_size, image_info_valid_row_bytes, AlphaType, ColorType,
};
use skia_rs_geometry::IRect;

use crate::{
    generator::{resolve_unique_id, NEED_NEW_IMAGE_UNIQUE_ID},
    image::{
        color_type_is_alpha_only, image_info_is_valid, image_info_min_row_bytes,
        MAX_IMAGE_DIMENSION,
    },
};

/// Matches `enum class SkCopyPixelsMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CopyPixelsMode {
    IfMutable = 0,
    Always = 1,
    Never = 2,
}

impl CopyPixelsMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::IfMutable),
            1 => Some(Self::Always),
            2 => Some(Self::Never),
            _ => None,
        }
    }
}

/// Exact port of `is_not_subset(const SkBitmap&)` in `SkImage_Raster.cpp`.
#[inline]
pub const fn bitmap_is_not_subset(pr_w: i32, pr_h: i32, bm_w: i32, bm_h: i32) -> bool {
    pr_w == bm_w && pr_h == bm_h
}

/// Computes the `SkImage_Raster` unique ID from a bitmap:
/// shares `bm_gen_id` when the bitmap covers the entire `SkPixelRef`, otherwise allocates a fresh ID.
#[inline]
pub fn raster_image_unique_id_from_bitmap(
    pr_w: i32,
    pr_h: i32,
    bm_w: i32,
    bm_h: i32,
    bm_gen_id: u32,
) -> u32 {
    let raw_id = if bitmap_is_not_subset(pr_w, pr_h, bm_w, bm_h) {
        bm_gen_id
    } else {
        NEED_NEW_IMAGE_UNIQUE_ID
    };
    resolve_unique_id(raw_id)
}

/// Action returned by `plan_raster_from_bitmap`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RasterFromBitmapAction {
    Invalid = 0,
    CopyPixels = 1,
    SharePixels = 2,
}

/// Result of `SkImage_Raster::MakeFromBitmap` validation and copy decision.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RasterFromBitmapPlan {
    pub action: RasterFromBitmapAction,
    pub byte_size: usize,
    pub bitmap_may_be_mutable: bool,
}

/// Exact port of `SkImage_Raster::MakeFromBitmap` decision logic.
pub fn plan_raster_from_bitmap(
    width: i32,
    height: i32,
    ct: ColorType,
    at: AlphaType,
    row_bytes: usize,
    has_pixels: bool,
    is_immutable: bool,
    cpm: CopyPixelsMode,
) -> RasterFromBitmapPlan {
    if !image_info_is_valid(width, height, ct, at)
        || row_bytes < image_info_min_row_bytes(width, ct)
        || !has_pixels
    {
        return RasterFromBitmapPlan {
            action: RasterFromBitmapAction::Invalid,
            byte_size: 0,
            bitmap_may_be_mutable: false,
        };
    }

    if cpm == CopyPixelsMode::Always || (!is_immutable && cpm != CopyPixelsMode::Never) {
        let size = compute_byte_size(width, height, ct, row_bytes);
        if size == usize::MAX {
            return RasterFromBitmapPlan {
                action: RasterFromBitmapAction::Invalid,
                byte_size: 0,
                bitmap_may_be_mutable: false,
            };
        }
        return RasterFromBitmapPlan {
            action: RasterFromBitmapAction::CopyPixels,
            byte_size: size,
            bitmap_may_be_mutable: false,
        };
    }

    RasterFromBitmapPlan {
        action: RasterFromBitmapAction::SharePixels,
        byte_size: 0,
        bitmap_may_be_mutable: cpm == CopyPixelsMode::Never,
    }
}

/// Safe Rust port of `SkRectMemcpy` used by `SkImage_Raster::onMakeSubset` (`copy_bitmap_subset`).
pub fn copy_rect_bytes(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    copy_bytes_per_row: usize,
    height: usize,
) {
    if height == 0 || copy_bytes_per_row == 0 {
        return;
    }
    if dst_rb == src_rb && dst_rb == copy_bytes_per_row {
        let total = copy_bytes_per_row.saturating_mul(height);
        if total <= dst.len() && total <= src.len() {
            dst[..total].copy_from_slice(&src[..total]);
            return;
        }
    }
    for y in 0..height {
        let d0 = y.saturating_mul(dst_rb);
        let s0 = y.saturating_mul(src_rb);
        let d1 = d0.saturating_add(copy_bytes_per_row);
        let s1 = s0.saturating_add(copy_bytes_per_row);
        if d1 <= dst.len() && s1 <= src.len() {
            dst[d0..d1].copy_from_slice(&src[s0..s1]);
        }
    }
}

/// Exact port of `SkImage_Raster::onAsLegacyBitmap` pixelRef sharing check.
#[inline]
pub const fn can_share_pixel_ref_for_legacy_bitmap(
    is_immutable: bool,
    is_temporarily_immutable: bool,
) -> bool {
    is_immutable && !is_temporarily_immutable
}

/// Exact port of `SkImage_Raster::makeShaderForPaint` alpha-image shader composition check.
#[inline]
pub const fn should_compose_alpha_shader(ct: ColorType, paint_has_shader: bool) -> bool {
    color_type_is_alpha_only(ct) && paint_has_shader
}

/// Whether `subset` covers the full bitmap dimensions in `SkImage_Raster::onMakeSubset`.
#[inline]
pub fn is_full_bitmap_subset(bm_w: i32, bm_h: i32, subset: IRect) -> bool {
    subset == IRect::from_wh(bm_w, bm_h)
}

/// Exact port of `valid_args` in `src/image/SkImage_RasterFactories.cpp`.
/// Returns `Some(min_byte_size)` when valid, or `None` when invalid.
pub fn validate_raster_factory_args(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
) -> Option<usize> {
    let ct = ColorType::from_i32(ct_raw)?;
    let at = AlphaType::from_i32(at_raw)?;

    // Mirrors `SkBitmap b; if (!b.setInfo(info, rowBytes)) return false;`
    bitmap_validate_set_info(width, height, ct, at, row_bytes)?;

    if width <= 0 || height <= 0 {
        return None;
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return None;
    }
    if ct == ColorType::Unknown {
        return None;
    }
    if !image_info_valid_row_bytes(width, ct, row_bytes) {
        return None;
    }

    let size = compute_byte_size(width, height, ct, row_bytes);
    if size == usize::MAX {
        return None;
    }

    Some(size)
}

/// Exact validation for `SkImages::RasterFromData`.
pub fn validate_raster_from_data(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    has_data: bool,
    data_size: usize,
) -> bool {
    if !has_data {
        return false;
    }
    match validate_raster_factory_args(width, height, ct_raw, at_raw, row_bytes) {
        Some(min_size) => data_size >= min_size,
        None => false,
    }
}

/// AlphaType selection for `SkImages::RasterFromCompressedTextureData`.
#[inline]
pub const fn compressed_texture_alpha_type(is_opaque: bool) -> AlphaType {
    if is_opaque {
        AlphaType::Opaque
    } else {
        AlphaType::Premul
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_from_bitmap_modes() {
        // Mutable bitmap + kIfMutable -> CopyPixels
        let p = plan_raster_from_bitmap(
            10,
            10,
            ColorType::Rgba8888,
            AlphaType::Premul,
            40,
            true,
            false,
            CopyPixelsMode::IfMutable,
        );
        assert_eq!(p.action, RasterFromBitmapAction::CopyPixels);
        assert_eq!(p.byte_size, 400);
        assert!(!p.bitmap_may_be_mutable);

        // Immutable bitmap + kIfMutable -> SharePixels
        let p = plan_raster_from_bitmap(
            10,
            10,
            ColorType::Rgba8888,
            AlphaType::Premul,
            40,
            true,
            true,
            CopyPixelsMode::IfMutable,
        );
        assert_eq!(p.action, RasterFromBitmapAction::SharePixels);
        assert!(!p.bitmap_may_be_mutable);

        // Immutable bitmap + kAlways -> CopyPixels
        let p = plan_raster_from_bitmap(
            10,
            10,
            ColorType::Rgba8888,
            AlphaType::Premul,
            40,
            true,
            true,
            CopyPixelsMode::Always,
        );
        assert_eq!(p.action, RasterFromBitmapAction::CopyPixels);
        assert_eq!(p.byte_size, 400);

        // Mutable bitmap + kNever -> SharePixels with bitmap_may_be_mutable = true
        let p = plan_raster_from_bitmap(
            10,
            10,
            ColorType::Rgba8888,
            AlphaType::Premul,
            40,
            true,
            false,
            CopyPixelsMode::Never,
        );
        assert_eq!(p.action, RasterFromBitmapAction::SharePixels);
        assert!(p.bitmap_may_be_mutable);

        // Null pixels -> Invalid
        let p = plan_raster_from_bitmap(
            10,
            10,
            ColorType::Rgba8888,
            AlphaType::Premul,
            40,
            false,
            true,
            CopyPixelsMode::IfMutable,
        );
        assert_eq!(p.action, RasterFromBitmapAction::Invalid);
    }

    #[test]
    fn valid_args_checks_all_constraints() {
        assert_eq!(
            validate_raster_factory_args(
                10,
                5,
                ColorType::Rgba8888 as i32,
                AlphaType::Premul as i32,
                64
            ),
            Some(4 * 64 + 40)
        );
        // Zero rowBytes must fail (even though SkBitmap::setInfo allows 0)
        assert!(validate_raster_factory_args(
            10,
            5,
            ColorType::Rgba8888 as i32,
            AlphaType::Premul as i32,
            0
        )
        .is_none());
        // Unaligned rowBytes fails
        assert!(validate_raster_factory_args(
            10,
            5,
            ColorType::Rgba8888 as i32,
            AlphaType::Premul as i32,
            42
        )
        .is_none());
        // Out of range enum fails
        assert!(validate_raster_factory_args(10, 5, 999, AlphaType::Premul as i32, 40).is_none());
        assert!(validate_raster_factory_args(10, 5, ColorType::Rgba8888 as i32, 999, 40).is_none());
        // Unknown color type fails
        assert!(validate_raster_factory_args(
            10,
            5,
            ColorType::Unknown as i32,
            AlphaType::Unknown as i32,
            0
        )
        .is_none());
        // Oversized dimension fails
        assert!(validate_raster_factory_args(
            MAX_IMAGE_DIMENSION + 1,
            1,
            ColorType::Alpha8 as i32,
            AlphaType::Opaque as i32,
            (MAX_IMAGE_DIMENSION + 1) as usize
        )
        .is_none());
    }

    #[test]
    fn copy_rect_bytes_strided_and_contiguous() {
        let src = [
            1u8, 2, 3, 4, 99, 99, // row 0 (stride 6)
            5, 6, 7, 8, 99, 99, // row 1
            9, 10, 11, 12, 99, 99, // row 2
        ];
        let mut dst = [0u8; 12];
        copy_rect_bytes(&mut dst, 4, &src, 6, 4, 3);
        assert_eq!(dst, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        let mut dst2 = [0u8; 12];
        copy_rect_bytes(&mut dst2, 4, &dst, 4, 4, 3);
        assert_eq!(dst2, dst);
    }

    #[test]
    fn legacy_bitmap_share_rules() {
        assert!(can_share_pixel_ref_for_legacy_bitmap(true, false));
        assert!(!can_share_pixel_ref_for_legacy_bitmap(true, true));
        assert!(!can_share_pixel_ref_for_legacy_bitmap(false, false));
    }
}
