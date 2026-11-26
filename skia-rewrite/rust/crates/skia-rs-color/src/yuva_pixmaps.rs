//! Safe Rust port of `src/core/SkYUVAPixmaps.cpp`.

use crate::image_info::{
    color_type_bytes_per_pixel, image_info_valid_row_bytes, ColorType,
};
use crate::yuva_info::{ISize, PlaneConfig, YuvaInfo, MAX_PLANES};

/// Matches `enum class SkYUVAPixmapInfo::DataType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum YuvaDataType {
    #[default]
    Unorm8 = 0,
    Unorm16 = 1,
    Float16 = 2,
    Unorm10Unorm2 = 3,
}

pub const DATA_TYPE_CNT: usize = 4;

impl YuvaDataType {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unorm8),
            1 => Some(Self::Unorm16),
            2 => Some(Self::Float16),
            3 => Some(Self::Unorm10Unorm2),
            _ => None,
        }
    }
}

/// Exact port of `SkYUVAPixmapInfo::DefaultColorTypeForDataType`.
pub const fn default_color_type_for_data_type(
    data_type: YuvaDataType,
    num_channels: i32,
    default_red: bool,
) -> ColorType {
    match num_channels {
        1 => {
            if default_red {
                match data_type {
                    YuvaDataType::Unorm8 => ColorType::R8Unorm,
                    YuvaDataType::Unorm16 => ColorType::R16Unorm,
                    YuvaDataType::Float16 => ColorType::R16Float,
                    YuvaDataType::Unorm10Unorm2 => ColorType::Unknown,
                }
            } else {
                match data_type {
                    YuvaDataType::Unorm8 => ColorType::Gray8,
                    YuvaDataType::Unorm16 => ColorType::A16Unorm,
                    YuvaDataType::Float16 => ColorType::A16Float,
                    YuvaDataType::Unorm10Unorm2 => ColorType::Unknown,
                }
            }
        }
        2 => match data_type {
            YuvaDataType::Unorm8 => ColorType::R8G8Unorm,
            YuvaDataType::Unorm16 => ColorType::R16G16Unorm,
            YuvaDataType::Float16 => ColorType::R16G16Float,
            YuvaDataType::Unorm10Unorm2 => ColorType::Unknown,
        },
        3 | 4 => match data_type {
            YuvaDataType::Unorm8 => ColorType::Rgba8888,
            YuvaDataType::Unorm16 => ColorType::R16G16B16A16Unorm,
            YuvaDataType::Float16 => ColorType::RgbaF16,
            YuvaDataType::Unorm10Unorm2 => ColorType::Rgba1010102,
        },
        _ => ColorType::Unknown,
    }
}

/// Exact port of `SkYUVAPixmapInfo::NumChannelsAndDataType`.
pub const fn num_channels_and_data_type(ct: ColorType) -> (i32, YuvaDataType) {
    match ct {
        ColorType::R8Unorm | ColorType::Alpha8 | ColorType::Gray8 => (1, YuvaDataType::Unorm8),
        ColorType::R16Unorm | ColorType::A16Unorm => (1, YuvaDataType::Unorm16),
        ColorType::R16Float | ColorType::A16Float => (1, YuvaDataType::Float16),

        ColorType::R8G8Unorm => (2, YuvaDataType::Unorm8),
        ColorType::R16G16Unorm => (2, YuvaDataType::Unorm16),
        ColorType::R16G16Float => (2, YuvaDataType::Float16),

        ColorType::Rgb888x => (3, YuvaDataType::Unorm8),
        ColorType::Rgb101010x => (3, YuvaDataType::Unorm10Unorm2),
        ColorType::RgbF16F16F16x => (3, YuvaDataType::Float16),

        ColorType::Rgba8888 => (4, YuvaDataType::Unorm8),
        ColorType::R16G16B16A16Unorm => (4, YuvaDataType::Unorm16),
        ColorType::RgbaF16 | ColorType::RgbaF16Norm => (4, YuvaDataType::Float16),
        ColorType::Rgba1010102 => (4, YuvaDataType::Unorm10Unorm2),

        _ => (0, YuvaDataType::Unorm8),
    }
}

/// Exact port of `SkYUVAPixmaps::RecommendedRGBAColorType`.
pub const fn recommended_rgba_color_type(data_type: YuvaDataType) -> ColorType {
    match data_type {
        YuvaDataType::Unorm8 => ColorType::Rgba8888,
        YuvaDataType::Unorm16 | YuvaDataType::Float16 => ColorType::RgbaF16,
        YuvaDataType::Unorm10Unorm2 => ColorType::Rgba1010102,
    }
}

/// Bitmask helper for `SkYUVAPixmapInfo::SupportedDataTypes::enableDataType`.
#[inline]
pub const fn supported_data_type_bit_index(data_type: YuvaDataType, num_channels: i32) -> Option<usize> {
    if num_channels < 1 || num_channels > 4 {
        return None;
    }
    Some((data_type as usize) + ((num_channels - 1) as usize) * DATA_TYPE_CNT)
}

/// Check if `bits` (same bit layout as `std::bitset<kDataTypeCnt*4>`) supports `(config, dt)`.
pub const fn supported_data_types_is_supported(
    bits: u32,
    config: PlaneConfig,
    data_type: YuvaDataType,
) -> bool {
    let n = config.num_planes();
    let mut i = 0;
    while i < n {
        let c = config.num_channels_in_plane(i);
        if c < 1 || c > 4 {
            return false;
        }
        let bit = (data_type as usize) + ((c - 1) as usize) * DATA_TYPE_CNT;
        if ((bits >> bit) & 1) == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// Result of validating and resolving `SkYUVAPixmapInfo` plane parameters.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidatedYuvaPixmapInfo {
    pub num_planes: i32,
    pub data_type: YuvaDataType,
    pub plane_dims: [ISize; MAX_PLANES],
    pub color_types: [ColorType; MAX_PLANES],
    pub row_bytes: [usize; MAX_PLANES],
}

/// Exact port of `SkYUVAPixmapInfo::SkYUVAPixmapInfo(yuvaInfo, colorTypes, rowBytes)` validation.
pub fn validate_yuva_pixmap_info(
    yuva_info: &YuvaInfo,
    color_types: &[ColorType; MAX_PLANES],
    row_bytes_opt: Option<&[usize; MAX_PLANES]>,
) -> Option<ValidatedYuvaPixmapInfo> {
    if !yuva_info.is_valid() {
        return None;
    }
    let mut plane_dims = [ISize::default(); MAX_PLANES];
    let n = yuva_info.plane_dimensions(&mut plane_dims) as usize;

    let mut resolved_row_bytes = [0usize; MAX_PLANES];
    match row_bytes_opt {
        Some(rb) => {
            resolved_row_bytes[..n].copy_from_slice(&rb[..n]);
        }
        None => {
            for i in 0..n {
                resolved_row_bytes[i] = (color_type_bytes_per_pixel(color_types[i]) as usize)
                    .wrapping_mul(plane_dims[i].width as usize);
            }
        }
    }

    let mut ok = true;
    let mut data_type = YuvaDataType::Unorm8;
    let mut out_cts = [ColorType::Unknown; MAX_PLANES];
    for i in 0..n {
        out_cts[i] = color_types[i];
        let num_required = yuva_info.num_channels_in_plane(i as i32);
        debug_assert!(num_required > 0);
        let (num_ct_channels, ct_dt) = num_channels_and_data_type(color_types[i]);
        ok &= i == 0 || ct_dt == data_type;
        ok &= num_ct_channels >= num_required;
        ok &= image_info_valid_row_bytes(plane_dims[i].width, color_types[i], resolved_row_bytes[i]);
        data_type = ct_dt;
    }

    if !ok {
        return None;
    }

    Some(ValidatedYuvaPixmapInfo {
        num_planes: n as i32,
        data_type,
        plane_dims,
        color_types: out_cts,
        row_bytes: resolved_row_bytes,
    })
}

/// Exact port of `SkYUVAPixmapInfo::SkYUVAPixmapInfo(yuvaInfo, dataType, rowBytes)`.
pub fn validate_yuva_pixmap_info_from_data_type(
    yuva_info: &YuvaInfo,
    data_type: YuvaDataType,
    row_bytes_opt: Option<&[usize; MAX_PLANES]>,
    default_red: bool,
) -> Option<ValidatedYuvaPixmapInfo> {
    let mut color_types = [ColorType::Unknown; MAX_PLANES];
    let num_planes = yuva_info.num_planes() as usize;
    for (i, slot) in color_types.iter_mut().enumerate().take(num_planes) {
        let num_channels = yuva_info.num_channels_in_plane(i as i32);
        *slot = default_color_type_for_data_type(data_type, num_channels, default_red);
    }
    validate_yuva_pixmap_info(yuva_info, &color_types, row_bytes_opt)
}

/// Compute per-plane byte offsets inside a single allocation for `initPixmapsFromSingleAllocation`.
pub fn compute_plane_offsets(
    yuva_info: &YuvaInfo,
    row_bytes: &[usize; MAX_PLANES],
) -> Option<([usize; MAX_PLANES], usize)> {
    if !yuva_info.is_valid() {
        return None;
    }
    let mut dims = [ISize::default(); MAX_PLANES];
    let n = yuva_info.plane_dimensions(&mut dims) as usize;
    let mut offsets = [0usize; MAX_PLANES];
    let mut addr = 0usize;
    for i in 0..n {
        offsets[i] = addr;
        let plane_size = row_bytes[i].checked_mul(dims[i].height as usize)?;
        addr = addr.checked_add(plane_size)?;
    }
    Some((offsets, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yuv_math::YUV_CS_REC601_8BIT_FULL;
    use crate::yuva_info::{Siting, Subsampling, ENCODED_ORIGIN_TOP_LEFT};

    #[test]
    fn channels_and_data_types() {
        assert_eq!(num_channels_and_data_type(ColorType::Gray8), (1, YuvaDataType::Unorm8));
        assert_eq!(num_channels_and_data_type(ColorType::R8G8Unorm), (2, YuvaDataType::Unorm8));
        assert_eq!(num_channels_and_data_type(ColorType::RgbaF16), (4, YuvaDataType::Float16));
        assert_eq!(num_channels_and_data_type(ColorType::Bgra8888), (0, YuvaDataType::Unorm8));

        assert_eq!(recommended_rgba_color_type(YuvaDataType::Unorm8), ColorType::Rgba8888);
        assert_eq!(recommended_rgba_color_type(YuvaDataType::Unorm16), ColorType::RgbaF16);
        assert_eq!(recommended_rgba_color_type(YuvaDataType::Float16), ColorType::RgbaF16);
        assert_eq!(recommended_rgba_color_type(YuvaDataType::Unorm10Unorm2), ColorType::Rgba1010102);
    }

    #[test]
    fn validate_pixmap_info_and_offsets() {
        let info = YuvaInfo::new(
            ISize::new(16, 12),
            PlaneConfig::Y_UV,
            Subsampling::S420,
            YUV_CS_REC601_8BIT_FULL,
            ENCODED_ORIGIN_TOP_LEFT,
            Siting::Centered,
            Siting::Centered,
        );
        let validated =
            validate_yuva_pixmap_info_from_data_type(&info, YuvaDataType::Unorm8, None, false)
                .unwrap();
        assert_eq!(validated.num_planes, 2);
        assert_eq!(validated.color_types[0], ColorType::Gray8);
        assert_eq!(validated.color_types[1], ColorType::R8G8Unorm);
        assert_eq!(validated.row_bytes[0], 16);
        assert_eq!(validated.row_bytes[1], 16); // 8 * 2

        let (offsets, n) = compute_plane_offsets(&info, &validated.row_bytes).unwrap();
        assert_eq!(n, 2);
        assert_eq!(offsets[0], 0);
        assert_eq!(offsets[1], 16 * 12);

        // Mismatched data types rejected
        let bad = validate_yuva_pixmap_info(
            &info,
            &[
                ColorType::Gray8,
                ColorType::R16G16Unorm,
                ColorType::Unknown,
                ColorType::Unknown,
            ],
            None,
        );
        assert!(bad.is_none());
    }
}
