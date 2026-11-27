//! Safe Rust port of `src/codec/SkHdrMetadata.cpp`.

use skia_rs_color::color_space::ColorSpacePrimaries;

use crate::hdr_agtm::AdaptiveGlobalToneMap;

const CLLI_PNG_LUMINANCE_DIVISOR: f32 = 10000.0_f32;
const MDCV_CHROMINANCE_DIVISOR: f32 = 50000.0_f32;
const MDCV_LUMINANCE_DIVISOR: f32 = 10000.0_f32;

/// Mirrors `skhdr::ContentLightLevelInformation`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ContentLightLevelInformation {
    pub max_cll: f32,
    pub max_fall: f32,
}

impl ContentLightLevelInformation {
    #[inline]
    pub fn uint16_max_cll(&self) -> u16 {
        self.max_cll.round().clamp(0.0_f32, 65535.0_f32) as u16
    }

    #[inline]
    pub fn uint16_max_fall(&self) -> u16 {
        self.max_fall.round().clamp(0.0_f32, 65535.0_f32) as u16
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != 4 {
            return None;
        }
        let max_cll = u16::from_be_bytes([data[0], data[1]]) as f32;
        let max_fall = u16::from_be_bytes([data[2], data[3]]) as f32;
        Some(Self { max_cll, max_fall })
    }

    pub fn serialize(&self) -> [u8; 4] {
        let cll = self.uint16_max_cll().to_be_bytes();
        let fall = self.uint16_max_fall().to_be_bytes();
        [cll[0], cll[1], fall[0], fall[1]]
    }

    pub fn parse_png_chunk(data: &[u8]) -> Option<Self> {
        if data.len() != 8 {
            return None;
        }
        let max_cll_scaled = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let max_fall_scaled = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        Some(Self {
            max_cll: (max_cll_scaled as f32) / CLLI_PNG_LUMINANCE_DIVISOR,
            max_fall: (max_fall_scaled as f32) / CLLI_PNG_LUMINANCE_DIVISOR,
        })
    }

    pub fn serialize_png_chunk(&self) -> [u8; 8] {
        let cll = ((self.max_cll * CLLI_PNG_LUMINANCE_DIVISOR).round() as i64 as u32).to_be_bytes();
        let fall =
            ((self.max_fall * CLLI_PNG_LUMINANCE_DIVISOR).round() as i64 as u32).to_be_bytes();
        [
            cll[0], cll[1], cll[2], cll[3], fall[0], fall[1], fall[2], fall[3],
        ]
    }

    pub fn format_string(&self) -> String {
        format!("{{maxCLL:{:.6}, maxFALL:{:.6}}}", self.max_cll, self.max_fall)
    }
}

/// Mirrors `skhdr::MasteringDisplayColorVolume`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct MasteringDisplayColorVolume {
    pub display_primaries: ColorSpacePrimaries,
    pub maximum_display_mastering_luminance: f32,
    pub minimum_display_mastering_luminance: f32,
}

impl MasteringDisplayColorVolume {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != 24 {
            return None;
        }
        let mut c = [0u16; 8];
        for i in 0..8 {
            c[i] = u16::from_be_bytes([data[2 * i], data[2 * i + 1]]);
        }
        let max_lum = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let min_lum = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        Some(Self {
            display_primaries: ColorSpacePrimaries {
                rx: (c[0] as f32) / MDCV_CHROMINANCE_DIVISOR,
                ry: (c[1] as f32) / MDCV_CHROMINANCE_DIVISOR,
                gx: (c[2] as f32) / MDCV_CHROMINANCE_DIVISOR,
                gy: (c[3] as f32) / MDCV_CHROMINANCE_DIVISOR,
                bx: (c[4] as f32) / MDCV_CHROMINANCE_DIVISOR,
                by: (c[5] as f32) / MDCV_CHROMINANCE_DIVISOR,
                wx: (c[6] as f32) / MDCV_CHROMINANCE_DIVISOR,
                wy: (c[7] as f32) / MDCV_CHROMINANCE_DIVISOR,
            },
            maximum_display_mastering_luminance: (max_lum as f32) / MDCV_LUMINANCE_DIVISOR,
            minimum_display_mastering_luminance: (min_lum as f32) / MDCV_LUMINANCE_DIVISOR,
        })
    }

    pub fn serialize(&self) -> [u8; 24] {
        let vals_16 = [
            self.display_primaries.rx,
            self.display_primaries.ry,
            self.display_primaries.gx,
            self.display_primaries.gy,
            self.display_primaries.bx,
            self.display_primaries.by,
            self.display_primaries.wx,
            self.display_primaries.wy,
        ];
        let mut out = [0u8; 24];
        for (i, &v) in vals_16.iter().enumerate() {
            let encoded = ((v * MDCV_CHROMINANCE_DIVISOR).round() as i64 as u16).to_be_bytes();
            out[2 * i] = encoded[0];
            out[2 * i + 1] = encoded[1];
        }
        let max_lum =
            ((self.maximum_display_mastering_luminance * MDCV_LUMINANCE_DIVISOR).round() as i64
                as u32)
                .to_be_bytes();
        let min_lum =
            ((self.minimum_display_mastering_luminance * MDCV_LUMINANCE_DIVISOR).round() as i64
                as u32)
                .to_be_bytes();
        out[16..20].copy_from_slice(&max_lum);
        out[20..24].copy_from_slice(&min_lum);
        out
    }

    pub fn format_string(&self) -> String {
        let p = &self.display_primaries;
        format!(
            "{{red:[{:.8},{:.8}], green:[{:.8},{:.8}], blue:[{:.8},{:.8}], white:[{:.8},{:.8}], maxLum:{:.6}, minLum:{:.6}}}",
            p.rx,
            p.ry,
            p.gx,
            p.gy,
            p.bx,
            p.by,
            p.wx,
            p.wy,
            self.maximum_display_mastering_luminance,
            self.minimum_display_mastering_luminance
        )
    }
}

/// Mirrors `skhdr::Metadata`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HdrMetadata {
    pub clli: Option<ContentLightLevelInformation>,
    pub mdcv: Option<MasteringDisplayColorVolume>,
    pub agtm: Option<AdaptiveGlobalToneMap>,
}

impl HdrMetadata {
    pub fn format_string(&self) -> String {
        let clli_s = self
            .clli
            .as_ref()
            .map(|c| c.format_string())
            .unwrap_or_else(|| "None".to_string());
        let mdcv_s = self
            .mdcv
            .as_ref()
            .map(|m| m.format_string())
            .unwrap_or_else(|| "None".to_string());
        let agtm_s = self
            .agtm
            .as_ref()
            .map(|a| a.format_string())
            .unwrap_or_else(|| "None".to_string());
        format!("{{clli:{}, mdcv:{}, agtm:{}}}", clli_s, mdcv_s, agtm_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clli_parse_serialize() {
        let raw = [0x03, 0xE8, 0x00, 0xFA];
        let clli = ContentLightLevelInformation::parse(&raw).unwrap();
        assert_eq!(clli.max_cll, 1000.0);
        assert_eq!(clli.max_fall, 250.0);
        assert_eq!(clli.serialize(), raw);

        let raw_png = [0x00, 0x98, 0x96, 0x80, 0x00, 0x26, 0x25, 0xA0];
        let clli_png = ContentLightLevelInformation::parse_png_chunk(&raw_png).unwrap();
        assert_eq!(clli_png, clli);
        assert_eq!(clli.serialize_png_chunk(), raw_png);
    }

    #[test]
    fn mdcv_parse_serialize() {
        let raw = [
            0x8A, 0x48, 0x39, 0x08, 0x21, 0x34, 0x9B, 0xAA, 0x19, 0x96, 0x08, 0xFC, 0x3D, 0x13,
            0x40, 0x42, 0x02, 0x62, 0x5A, 0x00, 0x00, 0x00, 0x00, 0x05,
        ];
        let mdcv = MasteringDisplayColorVolume::parse(&raw).unwrap();
        assert_eq!(mdcv.display_primaries.rx, 0.708);
        assert_eq!(mdcv.display_primaries.ry, 0.292);
        assert_eq!(mdcv.maximum_display_mastering_luminance, 4000.0);
        assert_eq!(mdcv.minimum_display_mastering_luminance, 0.0005);
        assert_eq!(mdcv.serialize(), raw);
    }
}
