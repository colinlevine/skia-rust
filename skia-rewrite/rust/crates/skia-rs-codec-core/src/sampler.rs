//! Safe Rust port of `src/codec/SkSampler.cpp` and sampling coordinate math from `SkCodecPriv.h`.

use skia_rs_color::image_info::{color_type_bytes_per_pixel, ColorType};
use skia_rs_geometry::{IRect, ISize};

use crate::types::{irect_contains, irect_make_size, ZeroInitialized};

/// Biased 0.0f (384) representation for `kBGR_101010x_XR_SkColorType`.
pub const BGR_101010X_XR_ZERO: u32 = (384u32 << 0) | (384u32 << 10) | (384u32 << 20);

/// Biased 0.0f (384) representation for `kBGRA_10101010_XR_SkColorType`.
pub const BGRA_10101010_XR_ZERO: u64 =
    (384u64 << 6) | (384u64 << 22) | (384u64 << 38) | (384u64 << 54);

/// Exact port of `SkSampler::FillIsZeroBytes`.
#[inline]
pub const fn fill_is_zero_bytes(ct: ColorType) -> bool {
    !matches!(ct, ColorType::Bgr101010xXr | ColorType::Bgra10101010Xr)
}

/// Returns the number of bytes filled per row for a given color type and pixel width.
#[inline]
pub const fn fill_row_bytes(ct: ColorType, width: usize) -> usize {
    match ct {
        ColorType::Rgba8888
        | ColorType::Bgra8888
        | ColorType::Rgba1010102
        | ColorType::Bgr101010xXr => width * 4,
        ColorType::Rgb565 => width * 2,
        ColorType::Gray8 | ColorType::Alpha8 => width,
        ColorType::RgbaF16 | ColorType::Bgra10101010Xr => width * 8,
        _ => width * (color_type_bytes_per_pixel(ct) as usize),
    }
}

/// Exact port of `SkSampler::Fill`.
///
/// `dst` must have length at least `(num_rows - 1) * row_bytes + fill_row_bytes(ct, width)`
/// when `num_rows > 0 && width > 0`.
pub fn fill(
    ct: ColorType,
    width: i32,
    num_rows: i32,
    dst: &mut [u8],
    row_bytes: usize,
    zero_init: ZeroInitialized,
) {
    if zero_init == ZeroInitialized::Yes && fill_is_zero_bytes(ct) {
        return;
    }
    if width <= 0 || num_rows <= 0 {
        return;
    }

    let w = width as usize;
    let rows = num_rows as usize;

    match ct {
        ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::Rgba1010102 => {
            let bytes = w * 4;
            for row in 0..rows {
                let start = row * row_bytes;
                dst[start..start + bytes].fill(0);
            }
        }
        ColorType::Bgr101010xXr => {
            let pattern = BGR_101010X_XR_ZERO.to_ne_bytes();
            let bytes = w * 4;
            for row in 0..rows {
                let start = row * row_bytes;
                let row_slice = &mut dst[start..start + bytes];
                for chunk in row_slice.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&pattern);
                }
            }
        }
        ColorType::Rgb565 => {
            let bytes = w * 2;
            for row in 0..rows {
                let start = row * row_bytes;
                dst[start..start + bytes].fill(0);
            }
        }
        ColorType::Gray8 | ColorType::Alpha8 => {
            for row in 0..rows {
                let start = row * row_bytes;
                dst[start..start + w].fill(0);
            }
        }
        ColorType::RgbaF16 => {
            let bytes = w * 8;
            for row in 0..rows {
                let start = row * row_bytes;
                dst[start..start + bytes].fill(0);
            }
        }
        ColorType::Bgra10101010Xr => {
            let pattern = BGRA_10101010_XR_ZERO.to_ne_bytes();
            let bytes = w * 8;
            for row in 0..rows {
                let start = row * row_bytes;
                let row_slice = &mut dst[start..start + bytes];
                for chunk in row_slice.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&pattern);
                }
            }
        }
        _ => {
            let bytes = w * (color_type_bytes_per_pixel(ct) as usize);
            for row in 0..rows {
                let start = row * row_bytes;
                dst[start..start + bytes].fill(0);
            }
        }
    }
}

/// Exact port of `SkCodecPriv::GetScaleFromSampleSize`.
#[inline]
pub fn get_scale_from_sample_size(sample_size: i32) -> f32 {
    1.0_f32 / (sample_size as f32)
}

/// Exact port of `SkCodecPriv::IsValidSubset`.
#[inline]
pub fn is_valid_subset(subset: IRect, image_dims: ISize) -> bool {
    irect_contains(irect_make_size(image_dims), subset)
}

/// Exact port of `SkCodecPriv::GetSampledDimension`.
#[inline]
pub const fn get_sampled_dimension(src_dimension: i32, sample_size: i32) -> i32 {
    if sample_size > src_dimension {
        return 1;
    }
    if sample_size == 0 {
        return 0;
    }
    src_dimension / sample_size
}

/// Exact port of `SkCodecPriv::GetStartCoord`.
#[inline]
pub const fn get_start_coord(sample_factor: i32) -> i32 {
    sample_factor / 2
}

/// Exact port of `SkCodecPriv::GetDstCoord`.
#[inline]
pub const fn get_dst_coord(src_coord: i32, sample_factor: i32) -> i32 {
    src_coord / sample_factor
}

/// Exact port of `SkCodecPriv::IsCoordNecessary`.
#[inline]
pub const fn is_coord_necessary(src_coord: i32, sample_factor: i32, scaled_dim: i32) -> bool {
    let start_coord = get_start_coord(sample_factor);
    if src_coord < start_coord || get_dst_coord(src_coord, sample_factor) >= scaled_dim {
        return false;
    }
    ((src_coord - start_coord) % sample_factor) == 0
}

/// Exact port of `SkSampler::rowNeeded`.
#[inline]
pub const fn row_needed(row: i32, sample_y: i32) -> bool {
    (row - get_start_coord(sample_y)) % sample_y == 0
}

/// Exact port of `SkCodecPriv::ComputeRowBytes`.
#[inline]
pub const fn compute_row_bytes(width: i32, bits_per_pixel: u32) -> usize {
    if bits_per_pixel < 16 {
        let pixels_per_byte = 8 / bits_per_pixel;
        ((width as u32) + pixels_per_byte - 1) as usize / (pixels_per_byte as usize)
    } else {
        let bytes_per_pixel = bits_per_pixel / 8;
        (width as usize) * (bytes_per_pixel as usize)
    }
}
