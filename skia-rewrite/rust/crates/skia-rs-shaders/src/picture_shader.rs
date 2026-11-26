//! Safe Rust port of `SkPictureShader.cpp` tile sizing, caching math, and validation.

use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::{
    cpp_max_f32, scalar_nearly_zero, ISize, Matrix, Point, Rect, Size, SK_SCALAR_NEARLY_ZERO,
};

use crate::{float_ceil2int, rect_to_rect_or_identity};

/// Matches `SkColorTypeMaxBitsPerChannel(SkColorType)` in `src/core/SkImageInfoPriv.h`.
#[inline]
pub const fn color_type_max_bits_per_channel(ct: ColorType) -> i32 {
    match ct {
        ColorType::Unknown => 0,
        ColorType::Argb4444 => 4,
        ColorType::Rgb565 => 6,
        ColorType::Alpha8
        | ColorType::Rgba8888
        | ColorType::Rgb888x
        | ColorType::Bgra8888
        | ColorType::Bgr888x
        | ColorType::Gray8
        | ColorType::R8G8Unorm
        | ColorType::Srgba8888
        | ColorType::R8Unorm => 8,
        ColorType::Rgba1010102
        | ColorType::Rgb101010x
        | ColorType::Bgra1010102
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6 => 10,
        ColorType::RgbaF16Norm
        | ColorType::RgbaF16
        | ColorType::RgbF16F16F16x
        | ColorType::A16Unorm
        | ColorType::R16Unorm
        | ColorType::R16G16Unorm
        | ColorType::A16Float
        | ColorType::R16Float
        | ColorType::R16G16Float
        | ColorType::R16G16B16A16Unorm => 16,
        ColorType::RgbaF32 => 32,
    }
}

/// Validates `SkPictureShader::Make` inputs.
#[inline]
pub fn picture_shader_should_be_empty(
    has_picture: bool,
    cull_rect: Rect,
    has_tile: bool,
    tile: Rect,
) -> bool {
    !has_picture || cull_rect.is_empty() || (has_tile && tile.is_empty())
}

/// Computed parameters matching `SkPictureShader::CachedImageInfo::Make`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PictureCachedImageParams {
    pub success: bool,
    pub tile_size: ISize,
    pub tile_scale: Size,
    pub matrix_for_draw: Matrix,
    pub img_color_type: ColorType,
}

/// Exact port of `SkPictureShader::CachedImageInfo::Make` geometry & tile sizing math.
pub fn compute_picture_cached_image_info(
    bounds: Rect,
    total_m: &Matrix,
    dst_color_type: ColorType,
    max_texture_size: i32,
) -> PictureCachedImageParams {
    let mut size = match total_m.decompose_scale() {
        Some((s, _)) => s,
        None => {
            let center = Point::new(bounds.center_x(), bounds.center_y());
            let area = total_m.differential_area_scale(center);
            if !area.is_finite() || scalar_nearly_zero(area, SK_SCALAR_NEARLY_ZERO) {
                Size { width: 1.0, height: 1.0 }
            } else {
                let s = area.sqrt();
                Size { width: s, height: s }
            }
        }
    };

    size.width *= bounds.width();
    size.height *= bounds.height();

    // Clamp the tile size to about 4M pixels
    const MAX_TILE_AREA: f32 = (2048 * 2048) as f32;
    let tile_area = size.width * size.height;
    if tile_area > MAX_TILE_AREA {
        let clamp_scale = (MAX_TILE_AREA / tile_area).sqrt();
        size.width *= clamp_scale;
        size.height *= clamp_scale;
    }

    // Scale down the tile size if larger than maxTextureSize for GPU path
    if max_texture_size != 0 {
        let max_tex = max_texture_size as f32;
        if size.width > max_tex || size.height > max_tex {
            let down_scale = max_tex / cpp_max_f32(size.width, size.height);
            size.width = (size.width * down_scale).floor();
            size.height = (size.height * down_scale).floor();
        }
    }

    let tile_size = ISize {
        width: float_ceil2int(size.width),
        height: float_ceil2int(size.height),
    };
    if tile_size.width <= 0 || tile_size.height <= 0 {
        return PictureCachedImageParams {
            success: false,
            tile_size: ISize { width: 0, height: 0 },
            tile_scale: Size { width: 0.0, height: 0.0 },
            matrix_for_draw: Matrix::identity(),
            img_color_type: ColorType::Unknown,
        };
    }

    let tile_scale = Size {
        width: (tile_size.width as f32) / bounds.width(),
        height: (tile_size.height as f32) / bounds.height(),
    };
    let dst_rect = Rect::from_ltrb(0.0, 0.0, tile_size.width as f32, tile_size.height as f32);
    let matrix_for_draw = rect_to_rect_or_identity(bounds, dst_rect);
    let img_color_type = if color_type_max_bits_per_channel(dst_color_type) <= 8 {
        ColorType::Rgba8888
    } else {
        ColorType::RgbaF16Norm
    };

    PictureCachedImageParams {
        success: true,
        tile_size,
        tile_scale,
        matrix_for_draw,
        img_color_type,
    }
}

/// Computes the local matrix used in `SkPictureShader::rasterShader`:
/// `SkMatrix::Scale(1.f / tileScale.width(), 1.f / tileScale.height())`.
#[inline]
pub fn picture_tile_local_matrix(tile_scale: Size) -> Matrix {
    Matrix::scale_translate(1.0 / tile_scale.width, 1.0 / tile_scale.height, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_image_info_basic_and_clamping() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 50.0);
        let m = Matrix::scale_translate(2.0, 2.0, 0.0, 0.0);
        let info = compute_picture_cached_image_info(bounds, &m, ColorType::Rgba8888, 0);
        assert!(info.success);
        assert_eq!(info.tile_size, ISize { width: 200, height: 100 });
        assert!((info.tile_scale.width - 2.0).abs() < 1e-6);
        assert!((info.tile_scale.height - 2.0).abs() < 1e-6);
        assert_eq!(info.img_color_type, ColorType::Rgba8888);

        // High-bit-depth dst chooses RGBA_F16Norm.
        let info_hdr = compute_picture_cached_image_info(bounds, &m, ColorType::RgbaF16, 0);
        assert_eq!(info_hdr.img_color_type, ColorType::RgbaF16Norm);

        // Large area clamping to ~2048x2048.
        let huge_bounds = Rect::from_ltrb(0.0, 0.0, 4096.0, 4096.0);
        let clamped =
            compute_picture_cached_image_info(huge_bounds, &Matrix::identity(), ColorType::Rgba8888, 0);
        assert!(clamped.success);
        assert_eq!(clamped.tile_size, ISize { width: 2048, height: 2048 });

        // Max texture size clamping.
        let capped =
            compute_picture_cached_image_info(bounds, &m, ColorType::Rgba8888, 64);
        assert!(capped.success);
        assert_eq!(capped.tile_size, ISize { width: 64, height: 32 });
    }

    #[test]
    fn cached_image_info_empty_bounds() {
        let empty = Rect::from_ltrb(0.0, 0.0, 0.0, 0.0);
        let info = compute_picture_cached_image_info(empty, &Matrix::identity(), ColorType::Rgba8888, 0);
        assert!(!info.success);
    }
}
