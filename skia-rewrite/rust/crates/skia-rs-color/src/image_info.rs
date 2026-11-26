//! Safe Rust port of `src/core/SkImageInfo.cpp` and `src/core/SkImageInfoPriv.h`.

pub const COLOR_CHANNEL_FLAG_RED: u32 = 1 << 0;
pub const COLOR_CHANNEL_FLAG_GREEN: u32 = 1 << 1;
pub const COLOR_CHANNEL_FLAG_BLUE: u32 = 1 << 2;
pub const COLOR_CHANNEL_FLAG_ALPHA: u32 = 1 << 3;
pub const COLOR_CHANNEL_FLAG_GRAY: u32 = 0x10;
pub const COLOR_CHANNEL_FLAGS_GRAY_ALPHA: u32 = COLOR_CHANNEL_FLAG_GRAY | COLOR_CHANNEL_FLAG_ALPHA;
pub const COLOR_CHANNEL_FLAGS_RG: u32 = COLOR_CHANNEL_FLAG_RED | COLOR_CHANNEL_FLAG_GREEN;
pub const COLOR_CHANNEL_FLAGS_RGB: u32 = COLOR_CHANNEL_FLAGS_RG | COLOR_CHANNEL_FLAG_BLUE;
pub const COLOR_CHANNEL_FLAGS_RGBA: u32 = COLOR_CHANNEL_FLAGS_RGB | COLOR_CHANNEL_FLAG_ALPHA;

/// Matches `enum SkAlphaType : int`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AlphaType {
    #[default]
    Unknown = 0,
    Opaque = 1,
    Premul = 2,
    Unpremul = 3,
}

impl AlphaType {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::Opaque),
            2 => Some(Self::Premul),
            3 => Some(Self::Unpremul),
            _ => None,
        }
    }
}

/// Matches `enum SkColorType : int`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ColorType {
    #[default]
    Unknown = 0,
    Alpha8 = 1,
    Rgb565 = 2,
    Argb4444 = 3,
    Rgba8888 = 4,
    Rgb888x = 5,
    Bgra8888 = 6,
    Rgba1010102 = 7,
    Bgra1010102 = 8,
    Rgb101010x = 9,
    Bgr101010x = 10,
    Bgr101010xXr = 11,
    Bgra10101010Xr = 12,
    Rgba10x6 = 13,
    Gray8 = 14,
    RgbaF16Norm = 15,
    RgbaF16 = 16,
    RgbF16F16F16x = 17,
    RgbaF32 = 18,
    R8G8Unorm = 19,
    A16Float = 20,
    R16Float = 21,
    R16G16Float = 22,
    A16Unorm = 23,
    R16Unorm = 24,
    R16G16Unorm = 25,
    R16G16B16A16Unorm = 26,
    Srgba8888 = 27,
    R8Unorm = 28,
    Bgr888x = 29,
}

impl ColorType {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::Alpha8),
            2 => Some(Self::Rgb565),
            3 => Some(Self::Argb4444),
            4 => Some(Self::Rgba8888),
            5 => Some(Self::Rgb888x),
            6 => Some(Self::Bgra8888),
            7 => Some(Self::Rgba1010102),
            8 => Some(Self::Bgra1010102),
            9 => Some(Self::Rgb101010x),
            10 => Some(Self::Bgr101010x),
            11 => Some(Self::Bgr101010xXr),
            12 => Some(Self::Bgra10101010Xr),
            13 => Some(Self::Rgba10x6),
            14 => Some(Self::Gray8),
            15 => Some(Self::RgbaF16Norm),
            16 => Some(Self::RgbaF16),
            17 => Some(Self::RgbF16F16F16x),
            18 => Some(Self::RgbaF32),
            19 => Some(Self::R8G8Unorm),
            20 => Some(Self::A16Float),
            21 => Some(Self::R16Float),
            22 => Some(Self::R16G16Float),
            23 => Some(Self::A16Unorm),
            24 => Some(Self::R16Unorm),
            25 => Some(Self::R16G16Unorm),
            26 => Some(Self::R16G16B16A16Unorm),
            27 => Some(Self::Srgba8888),
            28 => Some(Self::R8Unorm),
            29 => Some(Self::Bgr888x),
            _ => None,
        }
    }
}

/// Exact port of `SkColorTypeBytesPerPixel`.
pub const fn color_type_bytes_per_pixel(ct: ColorType) -> i32 {
    match ct {
        ColorType::Unknown => 0,
        ColorType::Alpha8 | ColorType::Gray8 | ColorType::R8Unorm => 1,
        ColorType::Rgb565
        | ColorType::Argb4444
        | ColorType::R8G8Unorm
        | ColorType::A16Unorm
        | ColorType::R16Unorm
        | ColorType::A16Float
        | ColorType::R16Float => 2,
        ColorType::Rgba8888
        | ColorType::Bgra8888
        | ColorType::Rgb888x
        | ColorType::Bgr888x
        | ColorType::Rgba1010102
        | ColorType::Rgb101010x
        | ColorType::Bgra1010102
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::R16G16Unorm
        | ColorType::R16G16Float
        | ColorType::Srgba8888 => 4,
        ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6
        | ColorType::RgbaF16Norm
        | ColorType::RgbaF16
        | ColorType::RgbF16F16F16x
        | ColorType::R16G16B16A16Unorm => 8,
        ColorType::RgbaF32 => 16,
    }
}

/// Exact port of `SkColorTypeShiftPerPixel`.
pub const fn color_type_shift_per_pixel(ct: ColorType) -> i32 {
    match ct {
        ColorType::Unknown | ColorType::Alpha8 | ColorType::Gray8 | ColorType::R8Unorm => 0,
        ColorType::Rgb565
        | ColorType::Argb4444
        | ColorType::R8G8Unorm
        | ColorType::A16Unorm
        | ColorType::R16Unorm
        | ColorType::A16Float
        | ColorType::R16Float => 1,
        ColorType::Rgba8888
        | ColorType::Rgb888x
        | ColorType::Bgra8888
        | ColorType::Bgr888x
        | ColorType::Rgba1010102
        | ColorType::Rgb101010x
        | ColorType::Bgra1010102
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::R16G16Unorm
        | ColorType::R16G16Float
        | ColorType::Srgba8888 => 2,
        ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6
        | ColorType::RgbaF16Norm
        | ColorType::RgbaF16
        | ColorType::RgbF16F16F16x
        | ColorType::R16G16B16A16Unorm => 3,
        ColorType::RgbaF32 => 4,
    }
}

/// Exact port of `SkColorTypeChannelFlags`.
pub const fn color_type_channel_flags(ct: ColorType) -> u32 {
    match ct {
        ColorType::Unknown => 0,
        ColorType::Alpha8 | ColorType::A16Unorm | ColorType::A16Float => COLOR_CHANNEL_FLAG_ALPHA,
        ColorType::Gray8 => COLOR_CHANNEL_FLAG_GRAY,
        ColorType::R16Unorm | ColorType::R16Float | ColorType::R8Unorm => COLOR_CHANNEL_FLAG_RED,
        ColorType::R8G8Unorm | ColorType::R16G16Unorm | ColorType::R16G16Float => {
            COLOR_CHANNEL_FLAGS_RG
        }
        ColorType::Rgb565
        | ColorType::Rgb888x
        | ColorType::Bgr888x
        | ColorType::Rgb101010x
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::RgbF16F16F16x => COLOR_CHANNEL_FLAGS_RGB,
        ColorType::Argb4444
        | ColorType::Rgba8888
        | ColorType::Bgra8888
        | ColorType::Rgba1010102
        | ColorType::Bgra1010102
        | ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6
        | ColorType::RgbaF16Norm
        | ColorType::RgbaF16
        | ColorType::RgbaF32
        | ColorType::R16G16B16A16Unorm
        | ColorType::Srgba8888 => COLOR_CHANNEL_FLAGS_RGBA,
    }
}

/// Exact port of `SkColorTypeIsAlwaysOpaque`.
#[inline]
pub const fn color_type_is_always_opaque(ct: ColorType) -> bool {
    (color_type_channel_flags(ct) & COLOR_CHANNEL_FLAG_ALPHA) == 0
}

/// Exact port of `SkColorTypeComputeOffset`.
#[inline]
pub const fn color_type_compute_offset(ct: ColorType, x: i32, y: i32, row_bytes: usize) -> usize {
    if matches!(ct, ColorType::Unknown) {
        return 0;
    }
    (y as usize) * row_bytes + ((x as usize) << (color_type_shift_per_pixel(ct) as u32))
}

/// Exact port of `SkImageInfo::computeByteSize`.
pub fn compute_byte_size(width: i32, height: i32, ct: ColorType, row_bytes: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if width < 0 || height < 0 {
        return usize::MAX;
    }
    let h_minus_1 = match height.checked_sub(1) {
        Some(v) if v >= 0 => v as usize,
        _ => return usize::MAX,
    };
    let Some(rows_bytes) = h_minus_1.checked_mul(row_bytes) else {
        return usize::MAX;
    };
    let Some(last_row) = (width as usize).checked_mul(color_type_bytes_per_pixel(ct) as usize)
    else {
        return usize::MAX;
    };
    let Some(bytes) = rows_bytes.checked_add(last_row) else {
        return usize::MAX;
    };

    const MAX_SIGNED_32BIT_SIZE: usize = i32::MAX as usize;
    if bytes <= MAX_SIGNED_32BIT_SIZE {
        bytes
    } else {
        usize::MAX
    }
}

/// Exact port of `SkColorTypeValidateAlphaType`.
/// Returns `Some(canonical)` if valid, or `None` if invalid.
pub const fn validate_alpha_type(ct: ColorType, mut at: AlphaType) -> Option<AlphaType> {
    match ct {
        ColorType::Unknown => {
            at = AlphaType::Unknown;
        }
        ColorType::Alpha8 | ColorType::A16Unorm | ColorType::A16Float => {
            if matches!(at, AlphaType::Unpremul) {
                at = AlphaType::Premul;
            }
            if matches!(at, AlphaType::Unknown) {
                return None;
            }
        }
        ColorType::Argb4444
        | ColorType::Rgba8888
        | ColorType::Srgba8888
        | ColorType::Bgra8888
        | ColorType::Rgba1010102
        | ColorType::Bgra1010102
        | ColorType::Rgba10x6
        | ColorType::RgbaF16Norm
        | ColorType::RgbaF16
        | ColorType::RgbaF32
        | ColorType::Bgra10101010Xr
        | ColorType::R16G16B16A16Unorm => {
            if matches!(at, AlphaType::Unknown) {
                return None;
            }
        }
        ColorType::Gray8
        | ColorType::R8G8Unorm
        | ColorType::R16Unorm
        | ColorType::R16Float
        | ColorType::R16G16Unorm
        | ColorType::R16G16Float
        | ColorType::Rgb565
        | ColorType::Rgb888x
        | ColorType::Bgr888x
        | ColorType::Rgb101010x
        | ColorType::Bgr101010x
        | ColorType::Bgr101010xXr
        | ColorType::RgbF16F16F16x
        | ColorType::R8Unorm => {
            at = AlphaType::Opaque;
        }
    }
    Some(at)
}

#[inline]
pub const fn image_info_valid_row_bytes(
    width: i32,
    ct: ColorType,
    row_bytes: usize,
) -> bool {
    if row_bytes < (width as usize).saturating_mul(color_type_bytes_per_pixel(ct) as usize) {
        return false;
    }
    let shift = color_type_shift_per_pixel(ct) as u32;
    let aligned_row_bytes = row_bytes >> shift;
    (aligned_row_bytes << shift) == row_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_shifts_consistent() {
        for v in 0..=29 {
            let ct = ColorType::from_i32(v).unwrap();
            let bpp = color_type_bytes_per_pixel(ct);
            let shift = color_type_shift_per_pixel(ct);
            if ct == ColorType::Unknown {
                assert_eq!(bpp, 0);
                assert_eq!(shift, 0);
            } else {
                assert_eq!(bpp, 1 << shift, "mismatch for {ct:?}");
            }
        }
    }

    #[test]
    fn compute_byte_size_checks() {
        assert_eq!(compute_byte_size(10, 0, ColorType::Rgba8888, 40), 0);
        assert_eq!(compute_byte_size(10, 5, ColorType::Rgba8888, 64), 4 * 64 + 40);
        assert_eq!(compute_byte_size(10, 1, ColorType::Rgba8888, 1000), 40);
        // Exceeding SK_MaxS32 returns usize::MAX
        assert_eq!(compute_byte_size(10, 100_000, ColorType::Rgba8888, 100_000), usize::MAX);
    }

    #[test]
    fn validate_alpha_types() {
        assert_eq!(
            validate_alpha_type(ColorType::Unknown, AlphaType::Premul),
            Some(AlphaType::Unknown)
        );
        assert_eq!(
            validate_alpha_type(ColorType::Alpha8, AlphaType::Unpremul),
            Some(AlphaType::Premul)
        );
        assert_eq!(
            validate_alpha_type(ColorType::Alpha8, AlphaType::Unknown),
            None
        );
        assert_eq!(
            validate_alpha_type(ColorType::Rgba8888, AlphaType::Unknown),
            None
        );
        assert_eq!(
            validate_alpha_type(ColorType::Rgb565, AlphaType::Premul),
            Some(AlphaType::Opaque)
        );
        assert!(color_type_is_always_opaque(ColorType::Rgb565));
        assert!(!color_type_is_always_opaque(ColorType::Rgba8888));
    }
}
