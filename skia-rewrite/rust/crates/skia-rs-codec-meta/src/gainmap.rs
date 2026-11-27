//! Safe Rust port of `src/codec/SkGainmapInfo.cpp` (ISO 21496-1 gainmap metadata).

const IS_MULTI_CHANNEL_MASK: u8 = 1 << 7;
const USE_BASE_COLOUR_SPACE_MASK: u8 = 1 << 6;

/// Mirrors `SkGainmapInfo::BaseImageType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BaseImageType {
    Sdr = 0,
    Hdr = 1,
}

/// Mirrors `SkGainmapInfo::Type`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GainmapType {
    Default = 0,
    Apple = 1,
}

/// Plain-data mirror of `SkGainmapInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct GainmapInfo {
    pub gainmap_ratio_min: [f32; 4],
    pub gainmap_ratio_max: [f32; 4],
    pub gainmap_gamma: [f32; 4],
    pub epsilon_sdr: [f32; 4],
    pub epsilon_hdr: [f32; 4],
    pub display_ratio_sdr: f32,
    pub display_ratio_hdr: f32,
    pub base_image_type: BaseImageType,
    pub gainmap_type: GainmapType,
    /// True if `fGainmapMathColorSpace` is non-null (i.e. NOT using base colour space).
    pub has_gainmap_math_color_space: bool,
}

impl Default for GainmapInfo {
    fn default() -> Self {
        Self {
            gainmap_ratio_min: [1.0, 1.0, 1.0, 1.0],
            gainmap_ratio_max: [2.0, 2.0, 2.0, 1.0],
            gainmap_gamma: [1.0, 1.0, 1.0, 1.0],
            epsilon_sdr: [0.0, 0.0, 0.0, 1.0],
            epsilon_hdr: [0.0, 0.0, 0.0, 1.0],
            display_ratio_sdr: 1.0,
            display_ratio_hdr: 2.0,
            base_image_type: BaseImageType::Sdr,
            gainmap_type: GainmapType::Default,
            has_gainmap_math_color_space: false,
        }
    }
}

struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn read_u16_be(&mut self) -> Option<u16> {
        let slice = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn read_u32_be(&mut self) -> Option<u32> {
        let slice = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_i32_be(&mut self) -> Option<i32> {
        let slice = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }
}

fn read_rational_be(r: &mut ByteReader<'_>) -> Option<f32> {
    let numerator = r.read_i32_be()?;
    let denominator = r.read_u32_be()?;
    if denominator == 0 {
        return None;
    }
    Some(((numerator as f64) / (denominator as f64)) as f32)
}

fn read_positive_rational_be(r: &mut ByteReader<'_>) -> Option<f32> {
    let numerator = r.read_u32_be()?;
    let denominator = r.read_u32_be()?;
    if denominator == 0 {
        return None;
    }
    Some(((numerator as f64) / (denominator as f64)) as f32)
}

fn write_rational_be(out: &mut Vec<u8>, x: f32) {
    let denominator: u32 = if x.abs() > 1.0_f32 {
        0x1000
    } else {
        0x1000_0000
    };
    let numerator = ((x as f64) * (denominator as f64)).round() as i64 as i32;
    out.extend_from_slice(&numerator.to_be_bytes());
    out.extend_from_slice(&denominator.to_be_bytes());
}

fn write_positive_rational_be(out: &mut Vec<u8>, x: f32) {
    let denominator: u32 = if x > 1.0_f32 { 0x1000 } else { 0x1000_0000 };
    let numerator = ((x as f64) * (denominator as f64)).round() as i64 as u32;
    out.extend_from_slice(&numerator.to_be_bytes());
    out.extend_from_slice(&denominator.to_be_bytes());
}

fn read_iso_gainmap_version(r: &mut ByteReader<'_>) -> bool {
    let Some(minimum_version) = r.read_u16_be() else {
        return false;
    };
    if minimum_version != 0 {
        return false;
    }
    r.read_u16_be().is_some()
}

/// Matches `SkGainmapInfo::isUltraHDRv1Compatible`.
pub const fn is_ultra_hdr_v1_compatible(
    base_image_type: BaseImageType,
    has_gainmap_math_color_space: bool,
) -> bool {
    if matches!(base_image_type, BaseImageType::Hdr) {
        return false;
    }
    if has_gainmap_math_color_space {
        return false;
    }
    true
}

/// Matches `SkGainmapInfo::ParseVersion`.
pub fn parse_version(data: &[u8]) -> bool {
    let mut r = ByteReader::new(data);
    read_iso_gainmap_version(&mut r)
}

/// Matches `SkGainmapInfo::SerializeVersion`.
pub const fn serialize_version() -> [u8; 4] {
    [0, 0, 0, 0]
}

/// Matches `SkGainmapInfo::Parse`.
pub fn parse_gainmap_info(data: &[u8]) -> Option<GainmapInfo> {
    let mut r = ByteReader::new(data);
    if !read_iso_gainmap_version(&mut r) {
        return None;
    }

    let flags = r.read_u8()?;
    let is_multi_channel = (flags & IS_MULTI_CHANNEL_MASK) != 0;
    let use_base_colour_space = (flags & USE_BASE_COLOUR_SPACE_MASK) != 0;

    let base_hdr_headroom = read_positive_rational_be(&mut r)?;
    let altr_hdr_headroom = read_positive_rational_be(&mut r)?;

    let mut gain_map_min = [0.0_f32; 3];
    let mut gain_map_max = [0.0_f32; 3];
    let mut gamma = [0.0_f32; 3];
    let mut base_offset = [0.0_f32; 3];
    let mut altr_offset = [0.0_f32; 3];

    let channel_count = if is_multi_channel { 3 } else { 1 };
    for i in 0..channel_count {
        gain_map_min[i] = read_rational_be(&mut r)?;
        gain_map_max[i] = read_rational_be(&mut r)?;
        gamma[i] = read_positive_rational_be(&mut r)?;
        if gamma[i] == 0.0_f32 {
            return None;
        }
        base_offset[i] = read_rational_be(&mut r)?;
        altr_offset[i] = read_rational_be(&mut r)?;
    }

    let mut info = GainmapInfo::default();
    info.has_gainmap_math_color_space = !use_base_colour_space;
    if base_hdr_headroom < altr_hdr_headroom {
        info.base_image_type = BaseImageType::Sdr;
        info.display_ratio_sdr = base_hdr_headroom.exp2();
        info.display_ratio_hdr = altr_hdr_headroom.exp2();
    } else {
        info.base_image_type = BaseImageType::Hdr;
        info.display_ratio_hdr = base_hdr_headroom.exp2();
        info.display_ratio_sdr = altr_hdr_headroom.exp2();
    }
    for i in 0..3 {
        let j = if i >= channel_count { 0 } else { i };
        info.gainmap_ratio_min[i] = gain_map_min[j].exp2();
        info.gainmap_ratio_max[i] = gain_map_max[j].exp2();
        info.gainmap_gamma[i] = 1.0_f32 / gamma[j];
        match info.base_image_type {
            BaseImageType::Sdr => {
                info.epsilon_sdr[i] = base_offset[j];
                info.epsilon_hdr[i] = altr_offset[j];
            }
            BaseImageType::Hdr => {
                info.epsilon_hdr[i] = base_offset[j];
                info.epsilon_sdr[i] = altr_offset[j];
            }
        }
    }
    Some(info)
}

#[inline]
fn is_single_channel(c: [f32; 4]) -> bool {
    c[0] == c[1] && c[1] == c[2]
}

/// Matches `SkGainmapInfo::serialize`.
pub fn serialize_gainmap_info(info: &GainmapInfo) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    // Version
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    let all_single_channel = is_single_channel(info.gainmap_ratio_min)
        && is_single_channel(info.gainmap_ratio_max)
        && is_single_channel(info.gainmap_gamma)
        && is_single_channel(info.epsilon_sdr)
        && is_single_channel(info.epsilon_hdr);

    let mut flags = 0u8;
    if !info.has_gainmap_math_color_space {
        flags |= USE_BASE_COLOUR_SPACE_MASK;
    }
    if !all_single_channel {
        flags |= IS_MULTI_CHANNEL_MASK;
    }
    out.push(flags);

    match info.base_image_type {
        BaseImageType::Sdr => {
            write_positive_rational_be(&mut out, info.display_ratio_sdr.log2());
            write_positive_rational_be(&mut out, info.display_ratio_hdr.log2());
        }
        BaseImageType::Hdr => {
            write_positive_rational_be(&mut out, info.display_ratio_hdr.log2());
            write_positive_rational_be(&mut out, info.display_ratio_sdr.log2());
        }
    }

    let channels = if all_single_channel { 1 } else { 3 };
    for i in 0..channels {
        write_rational_be(&mut out, info.gainmap_ratio_min[i].log2());
        write_rational_be(&mut out, info.gainmap_ratio_max[i].log2());
        write_positive_rational_be(&mut out, 1.0_f32 / info.gainmap_gamma[i]);
        match info.base_image_type {
            BaseImageType::Sdr => {
                write_rational_be(&mut out, info.epsilon_sdr[i]);
                write_rational_be(&mut out, info.epsilon_hdr[i]);
            }
            BaseImageType::Hdr => {
                write_rational_be(&mut out, info.epsilon_hdr[i]);
                write_rational_be(&mut out, info.epsilon_sdr[i]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_roundtrip() {
        let v = serialize_version();
        assert_eq!(v, [0, 0, 0, 0]);
        assert!(parse_version(&v));
        assert!(!parse_version(&[0, 1, 0, 0]));
        assert!(!parse_version(&[0, 0, 0]));
    }

    #[test]
    fn single_and_multi_channel_roundtrip() {
        let mut info = GainmapInfo::default();
        info.gainmap_ratio_min = [1.0, 1.0, 1.0, 1.0];
        info.gainmap_ratio_max = [8.0, 8.0, 8.0, 1.0];
        info.gainmap_gamma = [0.5, 0.5, 0.5, 1.0];
        info.epsilon_sdr = [0.015625, 0.015625, 0.015625, 1.0];
        info.epsilon_hdr = [0.015625, 0.015625, 0.015625, 1.0];
        info.display_ratio_sdr = 1.0;
        info.display_ratio_hdr = 4.0;

        let encoded = serialize_gainmap_info(&info);
        let decoded = parse_gainmap_info(&encoded).unwrap();
        assert_eq!(decoded.base_image_type, BaseImageType::Sdr);
        assert!((decoded.gainmap_ratio_max[0] - 8.0).abs() < 1e-4);
        assert!((decoded.gainmap_gamma[0] - 0.5).abs() < 1e-4);
        assert!((decoded.display_ratio_hdr - 4.0).abs() < 1e-4);

        // Multi-channel HDR base
        info.base_image_type = BaseImageType::Hdr;
        info.has_gainmap_math_color_space = true;
        info.gainmap_ratio_max = [4.0, 8.0, 16.0, 1.0];
        let encoded_mc = serialize_gainmap_info(&info);
        let decoded_mc = parse_gainmap_info(&encoded_mc).unwrap();
        assert_eq!(decoded_mc.base_image_type, BaseImageType::Hdr);
        assert!(decoded_mc.has_gainmap_math_color_space);
        assert!((decoded_mc.gainmap_ratio_max[0] - 4.0).abs() < 1e-3);
        assert!((decoded_mc.gainmap_ratio_max[1] - 8.0).abs() < 1e-3);
        assert!((decoded_mc.gainmap_ratio_max[2] - 16.0).abs() < 1e-3);
    }
}
