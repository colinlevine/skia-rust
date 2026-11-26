//! Safe Rust port of `src/core/SkYUVAInfo.cpp`.

use crate::image_info::{
    COLOR_CHANNEL_FLAGS_GRAY_ALPHA, COLOR_CHANNEL_FLAGS_RG, COLOR_CHANNEL_FLAGS_RGB,
    COLOR_CHANNEL_FLAGS_RGBA, COLOR_CHANNEL_FLAG_ALPHA, COLOR_CHANNEL_FLAG_GRAY,
    COLOR_CHANNEL_FLAG_RED,
};
use crate::yuv_math::YUV_CS_IDENTITY;

pub const MAX_PLANES: usize = 4;
pub const YUVA_CHANNEL_COUNT: usize = 4;

/// Matches `enum class SkColorChannel`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ColorChannel {
    #[default]
    R = 0,
    G = 1,
    B = 2,
    A = 3,
}

impl ColorChannel {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::R),
            1 => Some(Self::G),
            2 => Some(Self::B),
            3 => Some(Self::A),
            _ => None,
        }
    }
}

/// Matches `enum class SkYUVAInfo::PlaneConfig`.
#[repr(i32)]
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PlaneConfig {
    #[default]
    Unknown = 0,
    Y_U_V = 1,
    Y_V_U = 2,
    Y_UV = 3,
    Y_VU = 4,
    YUV = 5,
    UYV = 6,
    Y_U_V_A = 7,
    Y_V_U_A = 8,
    Y_UV_A = 9,
    Y_VU_A = 10,
    YUVA = 11,
    UYVA = 12,
    YUYV = 13,
}

impl PlaneConfig {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::Y_U_V),
            2 => Some(Self::Y_V_U),
            3 => Some(Self::Y_UV),
            4 => Some(Self::Y_VU),
            5 => Some(Self::YUV),
            6 => Some(Self::UYV),
            7 => Some(Self::Y_U_V_A),
            8 => Some(Self::Y_V_U_A),
            9 => Some(Self::Y_UV_A),
            10 => Some(Self::Y_VU_A),
            11 => Some(Self::YUVA),
            12 => Some(Self::UYVA),
            13 => Some(Self::YUYV),
            _ => None,
        }
    }

    /// Exact port of `SkYUVAInfo::NumPlanes`.
    #[inline]
    pub const fn num_planes(self) -> i32 {
        match self {
            Self::Unknown => 0,
            Self::Y_U_V | Self::Y_V_U => 3,
            Self::Y_UV | Self::Y_VU => 2,
            Self::YUV | Self::UYV => 1,
            Self::Y_U_V_A | Self::Y_V_U_A => 4,
            Self::Y_UV_A | Self::Y_VU_A => 3,
            Self::YUVA | Self::UYVA => 1,
            Self::YUYV => 1,
        }
    }

    /// Exact port of `SkYUVAInfo::NumChannelsInPlane`.
    #[inline]
    pub const fn num_channels_in_plane(self, i: i32) -> i32 {
        match self {
            Self::Unknown => 0,
            Self::Y_U_V | Self::Y_V_U => {
                if i >= 0 && i < 3 {
                    1
                } else {
                    0
                }
            }
            Self::Y_UV | Self::Y_VU => match i {
                0 => 1,
                1 => 2,
                _ => 0,
            },
            Self::YUV | Self::UYV => {
                if i == 0 {
                    3
                } else {
                    0
                }
            }
            Self::Y_U_V_A | Self::Y_V_U_A => {
                if i >= 0 && i < 4 {
                    1
                } else {
                    0
                }
            }
            Self::Y_UV_A | Self::Y_VU_A => match i {
                0 => 1,
                1 => 2,
                2 => 1,
                _ => 0,
            },
            Self::YUVA | Self::UYVA | Self::YUYV => {
                if i == 0 {
                    4
                } else {
                    0
                }
            }
        }
    }

    /// Exact port of `SkYUVAInfo::HasAlpha`.
    #[inline]
    pub const fn has_alpha(self) -> bool {
        matches!(
            self,
            Self::Y_U_V_A
                | Self::Y_V_U_A
                | Self::Y_UV_A
                | Self::Y_VU_A
                | Self::YUVA
                | Self::UYVA
        )
    }
}

/// Matches `enum class SkYUVAInfo::Subsampling`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Subsampling {
    #[default]
    Unknown = 0,
    S444 = 1,
    S422 = 2,
    S420 = 3,
    S440 = 4,
    S411 = 5,
    S410 = 6,
}

impl Subsampling {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::S444),
            2 => Some(Self::S422),
            3 => Some(Self::S420),
            4 => Some(Self::S440),
            5 => Some(Self::S411),
            6 => Some(Self::S410),
            _ => None,
        }
    }

    /// Exact port of `SkYUVAInfo::SubsamplingFactors`.
    #[inline]
    pub const fn factors(self) -> (i32, i32) {
        match self {
            Self::Unknown => (0, 0),
            Self::S444 => (1, 1),
            Self::S422 => (2, 1),
            Self::S420 => (2, 2),
            Self::S440 => (1, 2),
            Self::S411 => (4, 1),
            Self::S410 => (4, 2),
        }
    }
}

/// Matches `enum class SkYUVAInfo::Siting`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Siting {
    #[default]
    Centered = 0,
}

/// Matches `SkISize`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ISize {
    pub width: i32,
    pub height: i32,
}

impl ISize {
    #[inline]
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

/// Matches `SkYUVAInfo::YUVALocation`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct YuvaLocation {
    pub plane: i32,
    pub channel: ColorChannel,
}

impl Default for YuvaLocation {
    #[inline]
    fn default() -> Self {
        Self {
            plane: -1,
            channel: ColorChannel::A,
        }
    }
}

pub type YuvaLocations = [YuvaLocation; YUVA_CHANNEL_COUNT];

/// Matches `SkYUVAInfo` layout exactly (`#[repr(C)]`, 32 bytes).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct YuvaInfo {
    pub dimensions: ISize,
    pub plane_config: PlaneConfig,
    pub subsampling: Subsampling,
    pub yuv_color_space: i32,
    pub origin: i32,
    pub siting_x: Siting,
    pub siting_y: Siting,
}

pub const ENCODED_ORIGIN_TOP_LEFT: i32 = 1;
pub const ENCODED_ORIGIN_LEFT_TOP: i32 = 5;

impl Default for YuvaInfo {
    #[inline]
    fn default() -> Self {
        Self {
            dimensions: ISize::new(0, 0),
            plane_config: PlaneConfig::Unknown,
            subsampling: Subsampling::Unknown,
            yuv_color_space: YUV_CS_IDENTITY,
            origin: ENCODED_ORIGIN_TOP_LEFT,
            siting_x: Siting::Centered,
            siting_y: Siting::Centered,
        }
    }
}

#[inline]
pub const fn is_plane_config_compatible_with_subsampling(
    config: PlaneConfig,
    subsampling: Subsampling,
) -> bool {
    if matches!(config, PlaneConfig::Unknown) || matches!(subsampling, Subsampling::Unknown) {
        return false;
    }
    if matches!(config, PlaneConfig::YUYV) {
        return matches!(subsampling, Subsampling::S422);
    }
    matches!(subsampling, Subsampling::S444)
        || !matches!(
            config,
            PlaneConfig::YUV | PlaneConfig::YUVA | PlaneConfig::UYV | PlaneConfig::UYVA
        )
}

/// Exact port of `SkYUVAInfo::PlaneSubsamplingFactors`.
pub const fn plane_subsampling_factors(
    plane_config: PlaneConfig,
    subsampling: Subsampling,
    plane_idx: i32,
) -> (i32, i32) {
    if !is_plane_config_compatible_with_subsampling(plane_config, subsampling)
        || plane_idx < 0
        || plane_idx >= plane_config.num_planes()
    {
        return (0, 0);
    }
    let is_subsampled_plane = match plane_config {
        PlaneConfig::Unknown => return (0, 0),
        PlaneConfig::Y_U_V | PlaneConfig::Y_V_U | PlaneConfig::Y_U_V_A | PlaneConfig::Y_V_U_A => {
            plane_idx == 1 || plane_idx == 2
        }
        PlaneConfig::Y_UV | PlaneConfig::Y_VU | PlaneConfig::Y_UV_A | PlaneConfig::Y_VU_A => {
            plane_idx == 1
        }
        PlaneConfig::YUV | PlaneConfig::UYV | PlaneConfig::YUVA | PlaneConfig::UYVA => false,
        PlaneConfig::YUYV => true,
    };
    if is_subsampled_plane {
        subsampling.factors()
    } else {
        (1, 1)
    }
}

/// Exact port of `SkYUVAInfo::PlaneDimensions`.
pub fn plane_dimensions(
    image_dimensions: ISize,
    plane_config: PlaneConfig,
    subsampling: Subsampling,
    origin: i32,
    out_dims: &mut [ISize; MAX_PLANES],
) -> i32 {
    out_dims.fill(ISize::new(0, 0));
    if !is_plane_config_compatible_with_subsampling(plane_config, subsampling) {
        return 0;
    }

    let (w, h) = if origin >= ENCODED_ORIGIN_LEFT_TOP {
        (image_dimensions.height, image_dimensions.width)
    } else {
        (image_dimensions.width, image_dimensions.height)
    };
    let down2 = |x: i32| (x + 1) / 2;
    let down4 = |x: i32| (x + 3) / 4;
    let uv_size = match subsampling {
        Subsampling::Unknown => return 0,
        Subsampling::S444 => ISize::new(w, h),
        Subsampling::S422 => ISize::new(down2(w), h),
        Subsampling::S420 => ISize::new(down2(w), down2(h)),
        Subsampling::S440 => ISize::new(w, down2(h)),
        Subsampling::S411 => ISize::new(down4(w), h),
        Subsampling::S410 => ISize::new(down4(w), down2(h)),
    };
    match plane_config {
        PlaneConfig::Unknown => 0,
        PlaneConfig::Y_U_V | PlaneConfig::Y_V_U => {
            out_dims[0] = ISize::new(w, h);
            out_dims[1] = uv_size;
            out_dims[2] = uv_size;
            3
        }
        PlaneConfig::Y_UV | PlaneConfig::Y_VU => {
            out_dims[0] = ISize::new(w, h);
            out_dims[1] = uv_size;
            2
        }
        PlaneConfig::Y_U_V_A | PlaneConfig::Y_V_U_A => {
            out_dims[0] = ISize::new(w, h);
            out_dims[1] = uv_size;
            out_dims[2] = uv_size;
            out_dims[3] = ISize::new(w, h);
            4
        }
        PlaneConfig::Y_UV_A | PlaneConfig::Y_VU_A => {
            out_dims[0] = ISize::new(w, h);
            out_dims[1] = uv_size;
            out_dims[2] = ISize::new(w, h);
            3
        }
        PlaneConfig::YUV | PlaneConfig::UYV | PlaneConfig::YUVA | PlaneConfig::UYVA => {
            out_dims[0] = ISize::new(w, h);
            debug_assert_eq!(out_dims[0], uv_size);
            1
        }
        PlaneConfig::YUYV => {
            out_dims[0] = ISize::new(down2(w), h);
            debug_assert_eq!(w % 2, 0);
            1
        }
    }
}

fn channel_index_to_channel(channel_flags: u32, channel_idx: i32) -> Option<ColorChannel> {
    match channel_flags {
        COLOR_CHANNEL_FLAG_GRAY | COLOR_CHANNEL_FLAG_RED => {
            if channel_idx == 0 {
                Some(ColorChannel::R)
            } else {
                None
            }
        }
        COLOR_CHANNEL_FLAGS_GRAY_ALPHA => match channel_idx {
            0 => Some(ColorChannel::R),
            1 => Some(ColorChannel::A),
            _ => None,
        },
        COLOR_CHANNEL_FLAG_ALPHA => {
            if channel_idx == 0 {
                Some(ColorChannel::A)
            } else {
                None
            }
        }
        COLOR_CHANNEL_FLAGS_RG => {
            if channel_idx == 0 || channel_idx == 1 {
                ColorChannel::from_i32(channel_idx)
            } else {
                None
            }
        }
        COLOR_CHANNEL_FLAGS_RGB => {
            if (0..=2).contains(&channel_idx) {
                ColorChannel::from_i32(channel_idx)
            } else {
                None
            }
        }
        COLOR_CHANNEL_FLAGS_RGBA => {
            if (0..=3).contains(&channel_idx) {
                ColorChannel::from_i32(channel_idx)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Exact port of `SkYUVAInfo::GetYUVALocations`.
pub fn get_yuva_locations(config: PlaneConfig, plane_channel_flags: &[u32]) -> YuvaLocations {
    let planes_and_indices: [(i32, i32); YUVA_CHANNEL_COUNT] = match config {
        PlaneConfig::Unknown => return [YuvaLocation::default(); YUVA_CHANNEL_COUNT],
        PlaneConfig::Y_U_V => [(0, 0), (1, 0), (2, 0), (-1, -1)],
        PlaneConfig::Y_V_U => [(0, 0), (2, 0), (1, 0), (-1, -1)],
        PlaneConfig::Y_UV => [(0, 0), (1, 0), (1, 1), (-1, -1)],
        PlaneConfig::Y_VU => [(0, 0), (1, 1), (1, 0), (-1, -1)],
        PlaneConfig::YUV => [(0, 0), (0, 1), (0, 2), (-1, -1)],
        PlaneConfig::UYV => [(0, 1), (0, 0), (0, 2), (-1, -1)],
        PlaneConfig::Y_U_V_A => [(0, 0), (1, 0), (2, 0), (3, 0)],
        PlaneConfig::Y_V_U_A => [(0, 0), (2, 0), (1, 0), (3, 0)],
        PlaneConfig::Y_UV_A => [(0, 0), (1, 0), (1, 1), (2, 0)],
        PlaneConfig::Y_VU_A => [(0, 0), (1, 1), (1, 0), (2, 0)],
        PlaneConfig::YUVA => [(0, 0), (0, 1), (0, 2), (0, 3)],
        PlaneConfig::UYVA => [(0, 1), (0, 0), (0, 2), (0, 3)],
        PlaneConfig::YUYV => [(0, 0), (0, 1), (0, 3), (-1, -1)],
    };

    let mut locations = [YuvaLocation::default(); YUVA_CHANNEL_COUNT];
    for i in 0..YUVA_CHANNEL_COUNT {
        let (plane, chan_idx) = planes_and_indices[i];
        if plane >= 0 {
            let p = plane as usize;
            if p >= plane_channel_flags.len() {
                return [YuvaLocation::default(); YUVA_CHANNEL_COUNT];
            }
            let Some(channel) = channel_index_to_channel(plane_channel_flags[p], chan_idx) else {
                return [YuvaLocation::default(); YUVA_CHANNEL_COUNT];
            };
            locations[i] = YuvaLocation { plane, channel };
        } else {
            debug_assert_eq!(i, 3);
            locations[i] = YuvaLocation {
                plane: -1,
                channel: ColorChannel::R,
            };
        }
    }
    locations
}

impl YuvaInfo {
    /// Exact port of `SkYUVAInfo::SkYUVAInfo(...)`.
    pub fn new(
        dimensions: ISize,
        plane_config: PlaneConfig,
        subsampling: Subsampling,
        yuv_color_space: i32,
        origin: i32,
        siting_x: Siting,
        siting_y: Siting,
    ) -> Self {
        if dimensions.is_empty()
            || !is_plane_config_compatible_with_subsampling(plane_config, subsampling)
        {
            return Self::default();
        }
        if matches!(plane_config, PlaneConfig::YUYV) {
            let memory_width = if origin >= ENCODED_ORIGIN_LEFT_TOP {
                dimensions.height
            } else {
                dimensions.width
            };
            if memory_width % 2 != 0 {
                return Self::default();
            }
        }
        Self {
            dimensions,
            plane_config,
            subsampling,
            yuv_color_space,
            origin,
            siting_x,
            siting_y,
        }
    }

    #[inline]
    pub const fn is_valid(&self) -> bool {
        !matches!(self.plane_config, PlaneConfig::Unknown)
    }

    #[inline]
    pub const fn num_planes(&self) -> i32 {
        self.plane_config.num_planes()
    }

    #[inline]
    pub const fn num_channels_in_plane(&self, i: i32) -> i32 {
        self.plane_config.num_channels_in_plane(i)
    }

    #[inline]
    pub fn plane_dimensions(&self, out_dims: &mut [ISize; MAX_PLANES]) -> i32 {
        plane_dimensions(
            self.dimensions,
            self.plane_config,
            self.subsampling,
            self.origin,
            out_dims,
        )
    }

    /// Exact port of `SkYUVAInfo::computeTotalBytes`.
    pub fn compute_total_bytes(
        &self,
        row_bytes: &[usize; MAX_PLANES],
        mut plane_sizes: Option<&mut [usize; MAX_PLANES]>,
    ) -> usize {
        if !self.is_valid() {
            return 0;
        }
        let mut ok = true;
        let mut total_bytes: usize = 0;
        let mut dims = [ISize::default(); MAX_PLANES];
        let n = self.plane_dimensions(&mut dims) as usize;
        for i in 0..n {
            debug_assert!(!dims[i].is_empty());
            debug_assert!(row_bytes[i] != 0);
            let size = match row_bytes[i].checked_mul(dims[i].height as usize) {
                Some(v) => v,
                None => {
                    ok = false;
                    0
                }
            };
            if let Some(ps) = plane_sizes.as_deref_mut() {
                ps[i] = size;
            }
            total_bytes = match total_bytes.checked_add(size) {
                Some(v) => v,
                None => {
                    ok = false;
                    0
                }
            };
        }
        if let Some(ps) = plane_sizes {
            if ok {
                for slot in ps.iter_mut().take(MAX_PLANES).skip(n) {
                    *slot = 0;
                }
            } else {
                ps.fill(usize::MAX);
            }
        }
        if ok {
            total_bytes
        } else {
            usize::MAX
        }
    }

    #[inline]
    pub fn to_yuva_locations(&self, channel_flags: &[u32]) -> YuvaLocations {
        get_yuva_locations(self.plane_config, channel_flags)
    }

    #[inline]
    pub fn make_subsampling(&self, subsampling: Subsampling) -> Self {
        Self::new(
            self.dimensions,
            self.plane_config,
            subsampling,
            self.yuv_color_space,
            self.origin,
            self.siting_x,
            self.siting_y,
        )
    }

    #[inline]
    pub fn make_dimensions(&self, dimensions: ISize) -> Self {
        Self::new(
            dimensions,
            self.plane_config,
            self.subsampling,
            self.yuv_color_space,
            self.origin,
            self.siting_x,
            self.siting_y,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yuv_math::YUV_CS_REC601_8BIT_FULL;

    #[test]
    fn plane_dimensions_and_subsampling() {
        let info = YuvaInfo::new(
            ISize::new(15, 9),
            PlaneConfig::Y_U_V,
            Subsampling::S420,
            YUV_CS_REC601_8BIT_FULL,
            ENCODED_ORIGIN_TOP_LEFT,
            Siting::Centered,
            Siting::Centered,
        );
        assert!(info.is_valid());
        assert_eq!(info.num_planes(), 3);
        let mut dims = [ISize::default(); MAX_PLANES];
        assert_eq!(info.plane_dimensions(&mut dims), 3);
        assert_eq!(dims[0], ISize::new(15, 9));
        assert_eq!(dims[1], ISize::new(8, 5));
        assert_eq!(dims[2], ISize::new(8, 5));
        assert_eq!(dims[3], ISize::new(0, 0));

        assert_eq!(plane_subsampling_factors(PlaneConfig::Y_U_V, Subsampling::S420, 0), (1, 1));
        assert_eq!(plane_subsampling_factors(PlaneConfig::Y_U_V, Subsampling::S420, 1), (2, 2));
        assert_eq!(plane_subsampling_factors(PlaneConfig::Y_U_V, Subsampling::S420, 3), (0, 0));

        // Origin swapping width and height
        let rot = YuvaInfo::new(
            ISize::new(15, 9),
            PlaneConfig::Y_UV,
            Subsampling::S422,
            YUV_CS_REC601_8BIT_FULL,
            ENCODED_ORIGIN_LEFT_TOP,
            Siting::Centered,
            Siting::Centered,
        );
        assert_eq!(rot.plane_dimensions(&mut dims), 2);
        assert_eq!(dims[0], ISize::new(9, 15));
        assert_eq!(dims[1], ISize::new(5, 15));
    }

    #[test]
    fn incompatible_and_locations() {
        let bad = YuvaInfo::new(
            ISize::new(10, 10),
            PlaneConfig::YUV,
            Subsampling::S420,
            YUV_CS_REC601_8BIT_FULL,
            ENCODED_ORIGIN_TOP_LEFT,
            Siting::Centered,
            Siting::Centered,
        );
        assert!(!bad.is_valid());

        let flags = [COLOR_CHANNEL_FLAG_GRAY, COLOR_CHANNEL_FLAGS_RG, 0, 0];
        let locs = get_yuva_locations(PlaneConfig::Y_UV, &flags);
        assert_eq!(locs[0], YuvaLocation { plane: 0, channel: ColorChannel::R });
        assert_eq!(locs[1], YuvaLocation { plane: 1, channel: ColorChannel::R });
        assert_eq!(locs[2], YuvaLocation { plane: 1, channel: ColorChannel::G });
        assert_eq!(locs[3], YuvaLocation { plane: -1, channel: ColorChannel::R });

        // Compute total bytes & overflow
        let info = YuvaInfo::new(
            ISize::new(16, 8),
            PlaneConfig::Y_UV,
            Subsampling::S420,
            YUV_CS_REC601_8BIT_FULL,
            ENCODED_ORIGIN_TOP_LEFT,
            Siting::Centered,
            Siting::Centered,
        );
        let mut sizes = [0usize; MAX_PLANES];
        let total = info.compute_total_bytes(&[16, 16, 0, 0], Some(&mut sizes));
        assert_eq!(total, 16 * 8 + 16 * 4);
        assert_eq!(sizes, [128, 64, 0, 0]);

        let overflow = info.compute_total_bytes(&[usize::MAX / 2, 16, 0, 0], Some(&mut sizes));
        assert_eq!(overflow, usize::MAX);
        assert_eq!(sizes, [usize::MAX; MAX_PLANES]);
    }
}
