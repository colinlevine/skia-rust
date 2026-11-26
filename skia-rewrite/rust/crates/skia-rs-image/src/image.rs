//! Safe Rust port of `src/image/SkImage.cpp` and `src/image/SkImage_Base.cpp` algorithms
//! and validation logic.

use skia_rs_color::image_info::{
    color_type_bytes_per_pixel, color_type_channel_flags, compute_byte_size, AlphaType, ColorType,
    COLOR_CHANNEL_FLAG_ALPHA,
};
use skia_rs_geometry::{IRect, ISize, Rect};

/// Maximum valid dimension for `SkImageInfoIsValid` (`SK_MaxS32 >> 2`).
pub const MAX_IMAGE_DIMENSION: i32 = i32::MAX >> 2;

/// Exact port of `SkImageInfo::minRowBytes64()`.
#[inline]
pub const fn image_info_min_row_bytes64(width: i32, ct: ColorType) -> u64 {
    ((width as i64).wrapping_mul(color_type_bytes_per_pixel(ct) as i64)) as u64
}

/// Exact port of `SkImageInfo::minRowBytes()`.
/// Returns 0 if `minRowBytes64()` does not fit in a signed 32-bit integer (including negative widths).
#[inline]
pub const fn image_info_min_row_bytes(width: i32, ct: ColorType) -> usize {
    let mrb64 = image_info_min_row_bytes64(width, ct);
    if mrb64 > i32::MAX as u64 {
        0
    } else {
        mrb64 as usize
    }
}

/// Exact port of `SkColorTypeIsAlphaOnly`.
#[inline]
pub const fn color_type_is_alpha_only(ct: ColorType) -> bool {
    color_type_channel_flags(ct) == COLOR_CHANNEL_FLAG_ALPHA
}

/// Exact port of `SkColorInfoIsValid`.
#[inline]
pub const fn color_info_is_valid(ct: ColorType, at: AlphaType) -> bool {
    !matches!(ct, ColorType::Unknown) && !matches!(at, AlphaType::Unknown)
}

/// Exact port of `SkImageInfoIsValid`.
#[inline]
pub const fn image_info_is_valid(width: i32, height: i32, ct: ColorType, at: AlphaType) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return false;
    }
    color_info_is_valid(ct, at)
}

/// Exact port of `SkIRect::contains(const SkIRect&)`.
#[inline]
pub fn irect_contains(outer: IRect, inner: IRect) -> bool {
    !inner.is_empty()
        && !outer.is_empty()
        && outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Action returned by `plan_make_scaled`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MakeScaledAction {
    Invalid = 0,
    ReturnSelf = 1,
    DrawScaled = 2,
}

/// Exact decision logic for `SkImage::makeScaled`.
pub fn plan_make_scaled(
    src_w: i32,
    src_h: i32,
    src_ct: ColorType,
    src_at: AlphaType,
    same_color_space: bool,
    new_w: i32,
    new_h: i32,
    new_ct: ColorType,
    new_at: AlphaType,
) -> MakeScaledAction {
    if !image_info_is_valid(new_w, new_h, new_ct, new_at) {
        return MakeScaledAction::Invalid;
    }
    if new_w == src_w && new_h == src_h && new_ct == src_ct && new_at == src_at && same_color_space
    {
        return MakeScaledAction::ReturnSelf;
    }
    MakeScaledAction::DrawScaled
}

/// Destination rectangle for `SkImage::makeScaled` draw call: `SkRect::MakeIWH(w, h)`.
#[inline]
pub const fn make_scaled_dst_rect(new_w: i32, new_h: i32) -> Rect {
    Rect {
        left: 0.0,
        top: 0.0,
        right: new_w as f32,
        bottom: new_h as f32,
    }
}

/// Exact argument validation for `SkImage::asyncRescaleAndReadPixels` and
/// `SkSurface::asyncRescaleAndReadPixels`.
#[inline]
pub fn validate_async_rescale_rgba(
    src_w: i32,
    src_h: i32,
    src_rect: IRect,
    dst_w: i32,
    dst_h: i32,
    dst_ct: ColorType,
    dst_at: AlphaType,
) -> bool {
    irect_contains(IRect::from_wh(src_w, src_h), src_rect)
        && image_info_is_valid(dst_w, dst_h, dst_ct, dst_at)
}

/// Exact argument validation for `SkImage::asyncRescaleAndReadPixelsYUV(A)420` and
/// `SkSurface::asyncRescaleAndReadPixelsYUV(A)420`.
#[inline]
pub fn validate_async_rescale_yuv420(
    src_w: i32,
    src_h: i32,
    src_rect: IRect,
    dst_size: ISize,
) -> bool {
    let is_zero = dst_size.width == 0 && dst_size.height == 0;
    irect_contains(IRect::from_wh(src_w, src_h), src_rect)
        && !is_zero
        && (dst_size.width & 0b1) == 0
        && (dst_size.height & 0b1) == 0
}

/// Whether `SkImage::scalePixels` can delegate directly to `readPixels(0, 0)`.
#[inline]
pub const fn scale_pixels_is_identity_size(src_w: i32, src_h: i32, dst_w: i32, dst_h: i32) -> bool {
    src_w == dst_w && src_h == dst_h
}

/// Whether `SkImage::reinterpretColorSpace` should return `this` without creating a new image.
#[inline]
pub const fn should_reinterpret_return_self(same_color_space: bool, ct: ColorType) -> bool {
    same_color_space || color_type_is_alpha_only(ct)
}

/// Validated byte size and rowBytes for `SkImage::makeRasterImage`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MakeRasterImagePlan {
    pub row_bytes: usize,
    pub byte_size: usize,
}

/// Exact size/rowBytes computation and overflow check for `SkImage::makeRasterImage`.
pub fn plan_make_raster_image(
    width: i32,
    height: i32,
    ct: ColorType,
) -> Option<MakeRasterImagePlan> {
    let row_bytes = image_info_min_row_bytes(width, ct);
    let byte_size = compute_byte_size(width, height, ct, row_bytes);
    if byte_size == usize::MAX {
        return None;
    }
    Some(MakeRasterImagePlan {
        row_bytes,
        byte_size,
    })
}

/// Exact validation for `SkImage_Base::makeSubset`.
#[inline]
pub fn validate_subset_bounds(width: i32, height: i32, subset: IRect) -> bool {
    if subset.is_empty() {
        return false;
    }
    irect_contains(IRect::from_wh(width, height), subset)
}

/// Whether `SkImage::withMipmaps` should invoke `onMakeWithMipmaps`.
#[inline]
pub const fn should_invoke_on_make_with_mipmaps(has_mips: bool, mips_valid_for_root: bool) -> bool {
    !has_mips || mips_valid_for_root
}

/// Computes the source rectangle passed to `SkRescaleAndReadPixels` in
/// `SkImage_Base::onAsyncRescaleAndReadPixels` / `SkSurface_Base::onAsyncRescaleAndReadPixels`.
#[inline]
pub fn async_read_effective_src_rect(orig_src_rect: IRect, peeked: bool) -> IRect {
    if peeked {
        orig_src_rect
    } else {
        IRect::from_wh(orig_src_rect.width(), orig_src_rect.height())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_info_validity_checks() {
        assert!(image_info_is_valid(1, 1, ColorType::Rgba8888, AlphaType::Premul));
        assert!(image_info_is_valid(
            MAX_IMAGE_DIMENSION,
            MAX_IMAGE_DIMENSION,
            ColorType::Alpha8,
            AlphaType::Opaque
        ));
        assert!(!image_info_is_valid(0, 10, ColorType::Rgba8888, AlphaType::Premul));
        assert!(!image_info_is_valid(10, -1, ColorType::Rgba8888, AlphaType::Premul));
        assert!(!image_info_is_valid(
            MAX_IMAGE_DIMENSION + 1,
            10,
            ColorType::Rgba8888,
            AlphaType::Premul
        ));
        assert!(!image_info_is_valid(10, 10, ColorType::Unknown, AlphaType::Premul));
        assert!(!image_info_is_valid(10, 10, ColorType::Rgba8888, AlphaType::Unknown));
    }

    #[test]
    fn min_row_bytes_overflow_returns_zero() {
        assert_eq!(image_info_min_row_bytes(10, ColorType::Rgba8888), 40);
        assert_eq!(image_info_min_row_bytes(-1, ColorType::Rgba8888), 0);
        // (i32::MAX / 4 + 1) * 4 > i32::MAX -> 0
        assert_eq!(
            image_info_min_row_bytes((i32::MAX / 4) + 1, ColorType::Rgba8888),
            0
        );
    }

    #[test]
    fn alpha_only_checks() {
        assert!(color_type_is_alpha_only(ColorType::Alpha8));
        assert!(color_type_is_alpha_only(ColorType::A16Unorm));
        assert!(color_type_is_alpha_only(ColorType::A16Float));
        assert!(!color_type_is_alpha_only(ColorType::Rgba8888));
        assert!(!color_type_is_alpha_only(ColorType::Gray8));
        assert!(!color_type_is_alpha_only(ColorType::Unknown));
    }

    #[test]
    fn make_scaled_decisions() {
        assert_eq!(
            plan_make_scaled(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul,
                true,
                0,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul
            ),
            MakeScaledAction::Invalid
        );
        assert_eq!(
            plan_make_scaled(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul,
                true,
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul
            ),
            MakeScaledAction::ReturnSelf
        );
        assert_eq!(
            plan_make_scaled(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul,
                false,
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul
            ),
            MakeScaledAction::DrawScaled
        );
        assert_eq!(
            plan_make_scaled(
                10,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul,
                true,
                20,
                10,
                ColorType::Rgba8888,
                AlphaType::Premul
            ),
            MakeScaledAction::DrawScaled
        );
    }

    #[test]
    fn async_rescale_validation() {
        let valid_rect = IRect::from_ltrb(2, 3, 10, 12);
        assert!(validate_async_rescale_rgba(
            16,
            16,
            valid_rect,
            8,
            8,
            ColorType::Rgba8888,
            AlphaType::Premul
        ));
        // Empty src_rect fails
        assert!(!validate_async_rescale_rgba(
            16,
            16,
            IRect::from_ltrb(5, 5, 5, 10),
            8,
            8,
            ColorType::Rgba8888,
            AlphaType::Premul
        ));
        // Out-of-bounds src_rect fails
        assert!(!validate_async_rescale_rgba(
            16,
            16,
            IRect::from_ltrb(0, 0, 17, 16),
            8,
            8,
            ColorType::Rgba8888,
            AlphaType::Premul
        ));
        // YUV420 requires even non-zero destination dimensions
        assert!(validate_async_rescale_yuv420(
            16,
            16,
            valid_rect,
            ISize { width: 8, height: 6 }
        ));
        assert!(!validate_async_rescale_yuv420(
            16,
            16,
            valid_rect,
            ISize { width: 7, height: 6 }
        ));
        assert!(!validate_async_rescale_yuv420(
            16,
            16,
            valid_rect,
            ISize { width: 0, height: 0 }
        ));
    }

    #[test]
    fn subset_validation_and_raster_plan() {
        assert!(validate_subset_bounds(10, 10, IRect::from_ltrb(1, 2, 10, 10)));
        assert!(!validate_subset_bounds(10, 10, IRect::from_ltrb(5, 5, 5, 8)));
        assert!(!validate_subset_bounds(10, 10, IRect::from_ltrb(-1, 0, 5, 5)));

        let plan = plan_make_raster_image(32, 16, ColorType::Rgba8888).unwrap();
        assert_eq!(plan.row_bytes, 128);
        assert_eq!(plan.byte_size, 2048);

        assert!(plan_make_raster_image(100_000, 100_000, ColorType::Rgba8888).is_none());
    }
}
