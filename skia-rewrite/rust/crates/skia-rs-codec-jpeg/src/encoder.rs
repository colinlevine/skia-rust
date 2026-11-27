//! Safe Rust port of Skia's JPEG encoder planning, YUV row packing,
//! metadata segment packetization, and UltraHDR/HDRGM gainmap + MPF encoder
//! (`src/encode/SkJpegEncoderImpl.cpp`, `src/encode/SkJPEGWriteUtility.cpp`,
//! `src/encode/SkJpegGainmapEncoder.cpp`).

use skia_rs_codec_meta::exif::{self, ExifMetadata, LAST_ENCODED_ORIGIN};
use skia_rs_codec_meta::gainmap::{BaseImageType, GainmapInfo};
use skia_rs_codec_meta::tiff::{ENDIAN_BIG, TYPE_UNDEFINED, TYPE_UNSIGNED_LONG};
use skia_rs_color::image_info::{
    color_type_channel_flags, AlphaType, ColorType, COLOR_CHANNEL_FLAG_ALPHA,
    COLOR_CHANNEL_FLAG_GRAY, COLOR_CHANNEL_FLAG_RED, COLOR_CHANNEL_FLAGS_GRAY_ALPHA,
    COLOR_CHANNEL_FLAGS_RG, COLOR_CHANNEL_FLAGS_RGB, COLOR_CHANNEL_FLAGS_RGBA,
};
use skia_rs_color::yuva_info::{PlaneConfig, Subsampling};

use crate::constants::{
    EXIF_SIG, ICC_SIG, ISO_GAINMAP_SIG, JColorSpace, MARKER_START_OF_SCAN, MPF_MARKER,
    SEGMENT_PARAMETER_LENGTH_SIZE, XMP_STANDARD_SIG,
};
use crate::multi_picture::{get_image_data_offset, MultiPictureParameters};
use crate::segment_scan::SegmentScanner;

/// Matches `kJPEG_Full_SkYUVColorSpace` in `include/core/SkImageInfo.h`.
pub const YUV_COLOR_SPACE_JPEG_FULL: i32 = 0;

/// Matches `SkYUVAPixmapInfo::DataType::kUnorm8`.
pub const YUVA_DATA_TYPE_UNORM8: i32 = 0;

/// Mirrors `SkJpegEncoder::AlphaOption`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AlphaOption {
    #[default]
    Ignore = 0,
    BlendOnBlack = 1,
}

impl AlphaOption {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Ignore),
            1 => Some(Self::BlendOnBlack),
            _ => None,
        }
    }
}

/// Mirrors `SkJpegEncoder::Downsample`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Downsample {
    #[default]
    S420 = 0,
    S422 = 1,
    S444 = 2,
}

impl Downsample {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::S420),
            1 => Some(Self::S422),
            2 => Some(Self::S444),
            _ => None,
        }
    }
}

#[inline]
const fn color_type_num_channels(ct: ColorType) -> i32 {
    match color_type_channel_flags(ct) {
        COLOR_CHANNEL_FLAG_RED | COLOR_CHANNEL_FLAG_ALPHA | COLOR_CHANNEL_FLAG_GRAY => 1,
        COLOR_CHANNEL_FLAGS_GRAY_ALPHA | COLOR_CHANNEL_FLAGS_RG => 2,
        COLOR_CHANNEL_FLAGS_RGB => 3,
        COLOR_CHANNEL_FLAGS_RGBA => 4,
        _ => 0,
    }
}

#[inline]
const fn color_type_is_alpha_only(ct: ColorType) -> bool {
    color_type_channel_flags(ct) == COLOR_CHANNEL_FLAG_ALPHA
}

/// Plan returned for `SkJpegEncoderMgr::initializeRGB`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RgbEncodePlan {
    pub valid: bool,
    pub jpeg_color_space: i32,
    pub num_components: i32,
    pub use_color_xform: bool,
    pub xform_dst_ct: i32,
    pub xform_dst_at: i32,
    pub y_h_samp: i32,
    pub y_v_samp: i32,
    pub storage_bytes: usize,
}

/// Exact port of `SkJpegEncoderMgr::initializeRGB` decision logic.
pub const fn plan_rgb_encode(
    src_width: i32,
    src_ct: ColorType,
    src_at: AlphaType,
    alpha_option: AlphaOption,
    downsample: Downsample,
) -> RgbEncodePlan {
    let invalid = RgbEncodePlan {
        valid: false,
        jpeg_color_space: JColorSpace::Unknown as i32,
        num_components: 0,
        use_color_xform: false,
        xform_dst_ct: ColorType::Unknown as i32,
        xform_dst_at: AlphaType::Unknown as i32,
        y_h_samp: 1,
        y_v_samp: 1,
        storage_bytes: 0,
    };

    let apply_premul =
        matches!(alpha_option, AlphaOption::BlendOnBlack) && matches!(src_at, AlphaType::Unpremul);

    let (jpeg_cs, num_components, use_xform, xform_ct, xform_at) = match src_ct {
        ColorType::Rgb888x => (
            JColorSpace::ExtRgbx,
            4,
            false,
            ColorType::Unknown,
            AlphaType::Unknown,
        ),
        ColorType::Rgba8888 if !apply_premul => (
            JColorSpace::ExtRgba,
            4,
            false,
            ColorType::Unknown,
            AlphaType::Unknown,
        ),
        ColorType::Bgr888x => (
            JColorSpace::ExtBgrx,
            4,
            false,
            ColorType::Unknown,
            AlphaType::Unknown,
        ),
        ColorType::Bgra8888 if !apply_premul => (
            JColorSpace::ExtBgra,
            4,
            false,
            ColorType::Unknown,
            AlphaType::Unknown,
        ),
        _ => match color_type_num_channels(src_ct) {
            1 => {
                if color_type_is_alpha_only(src_ct) && !matches!(src_ct, ColorType::Alpha8) {
                    return invalid;
                }
                (
                    JColorSpace::Grayscale,
                    1,
                    false,
                    ColorType::Unknown,
                    AlphaType::Unknown,
                )
            }
            3 => (
                JColorSpace::ExtRgbx,
                4,
                true,
                ColorType::Rgb888x,
                AlphaType::Unpremul,
            ),
            4 => {
                let dst_at = if apply_premul {
                    AlphaType::Premul
                } else {
                    src_at
                };
                (JColorSpace::ExtRgba, 4, true, ColorType::Rgba8888, dst_at)
            }
            _ => return invalid,
        },
    };

    let (y_h_samp, y_v_samp) = if num_components != 1 {
        match downsample {
            Downsample::S420 => (2, 2),
            Downsample::S422 => (2, 1),
            Downsample::S444 => (1, 1),
        }
    } else {
        (1, 1)
    };

    let storage_bytes = if use_xform && src_width > 0 {
        (num_components as usize).wrapping_mul(src_width as usize)
    } else {
        0
    };

    RgbEncodePlan {
        valid: true,
        jpeg_color_space: jpeg_cs as i32,
        num_components,
        use_color_xform: use_xform,
        xform_dst_ct: xform_ct as i32,
        xform_dst_at: xform_at as i32,
        y_h_samp,
        y_v_samp,
        storage_bytes,
    }
}

/// Plan returned for `SkJpegEncoderMgr::initializeYUV`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct YuvEncodePlan {
    pub valid: bool,
    pub y_h_samp: i32,
    pub y_v_samp: i32,
    pub storage_bytes: usize,
}

/// Exact port of `SkJpegEncoderMgr::initializeYUV` validation and sampling logic.
pub const fn plan_yuv_encode(
    width: i32,
    yuv_color_space: i32,
    data_type: i32,
    plane_config: PlaneConfig,
    subsampling: Subsampling,
) -> YuvEncodePlan {
    let invalid = YuvEncodePlan {
        valid: false,
        y_h_samp: 0,
        y_v_samp: 0,
        storage_bytes: 0,
    };

    if yuv_color_space != YUV_COLOR_SPACE_JPEG_FULL {
        return invalid;
    }
    if data_type != YUVA_DATA_TYPE_UNORM8 {
        return invalid;
    }
    if !matches!(plane_config, PlaneConfig::Y_U_V | PlaneConfig::Y_UV) {
        return invalid;
    }

    let (ss_horiz, ss_vert) = subsampling.factors();
    let storage_bytes = if width > 0 {
        3usize.wrapping_mul(width as usize)
    } else {
        0
    };

    YuvEncodePlan {
        valid: true,
        y_h_samp: ss_horiz,
        y_v_samp: ss_vert,
        storage_bytes,
    }
}

/// Pack a row of planar `Y_U_V` data into interleaved `[Y, U, V]` triples.
/// Exact port of `yuva_copy_row` (`PlaneConfig::kY_U_V` branch) in `SkJpegEncoderImpl.cpp`.
pub fn yuva_copy_row_planar(
    y_row: &[u8],
    u_row: &[u8],
    v_row: &[u8],
    ss_width_u: usize,
    ss_width_v: usize,
    dst: &mut [u8],
) {
    if ss_width_u == 0 || ss_width_v == 0 {
        return;
    }
    let width = y_row.len().min(dst.len() / 3);
    for col in 0..width {
        let ui = col / ss_width_u;
        let vi = col / ss_width_v;
        if ui < u_row.len() && vi < v_row.len() {
            dst[3 * col] = y_row[col];
            dst[3 * col + 1] = u_row[ui];
            dst[3 * col + 2] = v_row[vi];
        }
    }
}

/// Pack a row of semi-planar `Y_UV` data into interleaved `[Y, U, V]` triples.
/// Exact port of `yuva_copy_row` (`PlaneConfig::kY_UV` branch) in `SkJpegEncoderImpl.cpp`.
pub fn yuva_copy_row_semi_planar(y_row: &[u8], uv_row: &[u8], ss_width_uv: usize, dst: &mut [u8]) {
    if ss_width_uv == 0 {
        return;
    }
    let width = y_row.len().min(dst.len() / 3);
    for col in 0..width {
        let uvi = 2 * (col / ss_width_uv);
        if uvi + 1 < uv_row.len() {
            dst[3 * col] = y_row[col];
            dst[3 * col + 1] = uv_row[uvi];
            dst[3 * col + 2] = uv_row[uvi + 1];
        }
    }
}

// ============================================================================
// Metadata segment packetization (SkJpegMetadataEncoder + SkJpegGainmapEncoder)
// ============================================================================

/// Exact port of `SkJpegMetadataEncoder::AppendICC` segment parameter encoding.
pub fn build_icc_segment_params(icc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ICC_SIG.len() + 2 + icc.len());
    out.extend_from_slice(ICC_SIG);
    out.push(1); // marker index 1
    out.push(1); // total markers 1
    out.extend_from_slice(icc);
    out
}

/// Exact port of `SkJpegMetadataEncoder::AppendXMPStandard` segment parameter encoding.
pub fn build_xmp_standard_segment_params(xmp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(XMP_STANDARD_SIG.len() + xmp.len());
    out.extend_from_slice(XMP_STANDARD_SIG);
    out.extend_from_slice(xmp);
    out
}

/// Exact port of `SkJpegMetadataEncoder::AppendOrigin` segment parameter encoding.
pub fn build_origin_exif_segment_params(origin: u32) -> Option<Vec<u8>> {
    if origin < 1 || origin > (LAST_ENCODED_ORIGIN as u32) {
        return None;
    }
    let metadata = ExifMetadata {
        origin: Some(origin),
        ..ExifMetadata::default()
    };
    let exif_bytes = exif::write_exif(&metadata)?;
    let mut out = Vec::with_capacity(EXIF_SIG.len() + 1 + exif_bytes.len());
    out.extend_from_slice(EXIF_SIG);
    out.push(0);
    out.extend_from_slice(&exif_bytes);
    Some(out)
}

/// Exact port of `get_iso_gainmap_segment_params` in `SkJpegGainmapEncoder.cpp`.
pub fn build_iso_gainmap_segment_params(iso_payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ISO_GAINMAP_SIG.len() + iso_payload.len());
    out.extend_from_slice(ISO_GAINMAP_SIG);
    out.extend_from_slice(iso_payload);
    out
}

/// Exact byte-for-byte port of `get_exif_params()` in `SkJpegGainmapEncoder.cpp`.
pub fn build_minimal_exif_params() -> Vec<u8> {
    let mut s = Vec::with_capacity(46);
    s.extend_from_slice(EXIF_SIG);
    s.push(0);

    s.extend_from_slice(&ENDIAN_BIG);
    s.extend_from_slice(&8u32.to_be_bytes()); // Offset of index IFD

    // Index IFD: 1 tag (SubIFD offset 0x8769 -> 26).
    s.extend_from_slice(&1u16.to_be_bytes());
    s.extend_from_slice(&0x8769u16.to_be_bytes());
    s.extend_from_slice(&TYPE_UNSIGNED_LONG.to_be_bytes());
    s.extend_from_slice(&1u32.to_be_bytes());
    s.extend_from_slice(&26u32.to_be_bytes());
    s.extend_from_slice(&0u32.to_be_bytes()); // Next IFD offset

    // Sub-IFD: 1 tag (ExifVersion 0x9000 -> "0232").
    s.extend_from_slice(&1u16.to_be_bytes());
    s.extend_from_slice(&0x9000u16.to_be_bytes());
    s.extend_from_slice(&TYPE_UNDEFINED.to_be_bytes());
    s.extend_from_slice(&4u32.to_be_bytes());
    s.extend_from_slice(b"0232");
    s.extend_from_slice(&0u32.to_be_bytes()); // Next IFD offset

    s
}

/// Exact byte-for-byte port of `get_mpf_segment()` in `SkJpegGainmapEncoder.cpp`.
pub fn build_mpf_segment(mp_params: &MultiPictureParameters, image_number: u32) -> Vec<u8> {
    let segment_params = mp_params.serialize(image_number);
    let mp_parameter_length = SEGMENT_PARAMETER_LENGTH_SIZE + segment_params.len();
    let mut s = Vec::with_capacity(2 + mp_parameter_length);
    s.push(0xFF);
    s.push(MPF_MARKER);
    s.push((mp_parameter_length / 256) as u8);
    s.push((mp_parameter_length % 256) as u8);
    s.extend_from_slice(&segment_params);
    s
}

// ============================================================================
// Gainmap XMP & MPF assembly (SkJpegGainmapEncoder.cpp)
// ============================================================================

/// Formats an `f32` with C99 `%.8g` semantics matching `SkWStream::writeScalarAsText`
/// (`SkStrAppendScalar`).
fn append_scalar_text(out: &mut Vec<u8>, value: f32) {
    if value.is_nan() {
        out.extend_from_slice(b"nan");
        return;
    }
    if value.is_infinite() {
        if value > 0.0 {
            out.extend_from_slice(b"inf");
        } else {
            out.extend_from_slice(b"-inf");
        }
        return;
    }

    let val = value as f64;
    if val.is_sign_negative() {
        out.push(b'-');
    }
    let abs_val = val.abs();
    if abs_val == 0.0 {
        out.push(b'0');
        return;
    }

    const SIG_DIGITS: usize = 8;
    let sci = format!("{:.*e}", SIG_DIGITS - 1, abs_val);
    let (mantissa, exp_str) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp_str.parse().unwrap_or(0);

    let mut digits = [0u8; SIG_DIGITS];
    let mut dlen = 0usize;
    for b in mantissa.bytes() {
        if b.is_ascii_digit() && dlen < SIG_DIGITS {
            digits[dlen] = b;
            dlen += 1;
        }
    }

    if exp < -4 || exp >= SIG_DIGITS as i32 {
        out.push(digits[0]);
        let mut last = dlen;
        while last > 1 && digits[last - 1] == b'0' {
            last -= 1;
        }
        if last > 1 {
            out.push(b'.');
            out.extend_from_slice(&digits[1..last]);
        }
        out.push(b'e');
        if exp >= 0 {
            out.push(b'+');
        } else {
            out.push(b'-');
        }
        let abs_exp = exp.unsigned_abs();
        if abs_exp < 10 {
            out.push(b'0');
            out.push(b'0' + abs_exp as u8);
        } else {
            out.extend_from_slice(abs_exp.to_string().as_bytes());
        }
    } else if exp < 0 {
        out.extend_from_slice(b"0.");
        let leading_zeros = (-exp - 1) as usize;
        for _ in 0..leading_zeros {
            out.push(b'0');
        }
        let mut last = dlen;
        while last > 0 && digits[last - 1] == b'0' {
            last -= 1;
        }
        out.extend_from_slice(&digits[..last]);
    } else {
        let int_len = (exp as usize) + 1;
        if int_len <= dlen {
            out.extend_from_slice(&digits[..int_len]);
            let frac = &digits[int_len..dlen];
            let mut last = frac.len();
            while last > 0 && frac[last - 1] == b'0' {
                last -= 1;
            }
            if last > 0 {
                out.push(b'.');
                out.extend_from_slice(&frac[..last]);
            }
        } else {
            out.extend_from_slice(&digits[..dlen]);
            for _ in 0..(int_len - dlen) {
                out.push(b'0');
            }
        }
    }
}

#[inline]
fn is_single_channel(c: [f32; 4]) -> bool {
    c[0] == c[1] && c[1] == c[2]
}

fn write_scalar_attr(out: &mut Vec<u8>, attrib: &[u8], value: f32) {
    out.extend_from_slice(b"        ");
    out.extend_from_slice(attrib);
    out.extend_from_slice(b"=\"");
    append_scalar_text(out, value);
    out.extend_from_slice(b"\"\n");
}

fn maybe_write_scalar_attr(out: &mut Vec<u8>, attrib: &[u8], value: [f32; 4]) {
    if is_single_channel(value) {
        write_scalar_attr(out, attrib, value[0]);
    }
}

fn maybe_write_float3_attr(out: &mut Vec<u8>, attrib: &[u8], value: [f32; 4]) {
    if is_single_channel(value) {
        return;
    }
    out.extend_from_slice(b"      <");
    out.extend_from_slice(attrib);
    out.extend_from_slice(b">\n");
    out.extend_from_slice(b"        <rdf:Seq>\n");
    out.extend_from_slice(b"          <rdf:li>");
    append_scalar_text(out, value[0]);
    out.extend_from_slice(b"</rdf:li>\n");
    out.extend_from_slice(b"          <rdf:li>");
    append_scalar_text(out, value[1]);
    out.extend_from_slice(b"</rdf:li>\n");
    out.extend_from_slice(b"          <rdf:li>");
    append_scalar_text(out, value[2]);
    out.extend_from_slice(b"</rdf:li>\n");
    out.extend_from_slice(b"        </rdf:Seq>\n");
    out.extend_from_slice(b"      </");
    out.extend_from_slice(attrib);
    out.extend_from_slice(b">\n");
}

/// Exact byte-for-byte port of `get_gainmap_image_xmp_metadata` in `SkJpegGainmapEncoder.cpp`.
pub fn build_gainmap_image_xmp_metadata(info: &GainmapInfo) -> Vec<u8> {
    let k_log2 = 2.0_f32.ln();
    let gain_map_min = [
        info.gainmap_ratio_min[0].ln() / k_log2,
        info.gainmap_ratio_min[1].ln() / k_log2,
        info.gainmap_ratio_min[2].ln() / k_log2,
        1.0_f32,
    ];
    let gain_map_max = [
        info.gainmap_ratio_max[0].ln() / k_log2,
        info.gainmap_ratio_max[1].ln() / k_log2,
        info.gainmap_ratio_max[2].ln() / k_log2,
        1.0_f32,
    ];
    let gamma = [
        1.0_f32 / info.gainmap_gamma[0],
        1.0_f32 / info.gainmap_gamma[1],
        1.0_f32 / info.gainmap_gamma[2],
        1.0_f32,
    ];

    let mut s = Vec::with_capacity(512);
    s.extend_from_slice(
        b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"XMP Core 5.5.0\">\n\
  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
    <rdf:Description rdf:about=\"\"\n\
        xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\"\n\
        hdrgm:Version=\"1.0\"\n",
    );

    maybe_write_scalar_attr(&mut s, b"hdrgm:GainMapMin", gain_map_min);
    maybe_write_scalar_attr(&mut s, b"hdrgm:GainMapMax", gain_map_max);
    maybe_write_scalar_attr(&mut s, b"hdrgm:Gamma", gamma);
    maybe_write_scalar_attr(&mut s, b"hdrgm:OffsetSDR", info.epsilon_sdr);
    maybe_write_scalar_attr(&mut s, b"hdrgm:OffsetHDR", info.epsilon_hdr);
    write_scalar_attr(
        &mut s,
        b"hdrgm:HDRCapacityMin",
        info.display_ratio_sdr.ln() / k_log2,
    );
    write_scalar_attr(
        &mut s,
        b"hdrgm:HDRCapacityMax",
        info.display_ratio_hdr.ln() / k_log2,
    );
    match info.base_image_type {
        BaseImageType::Sdr => s.extend_from_slice(b"        hdrgm:BaseRenditionIsHDR=\"False\">\n"),
        BaseImageType::Hdr => s.extend_from_slice(b"        hdrgm:BaseRenditionIsHDR=\"True\">\n"),
    }

    maybe_write_float3_attr(&mut s, b"hdrgm:GainMapMin", gain_map_min);
    maybe_write_float3_attr(&mut s, b"hdrgm:GainMapMax", gain_map_max);
    maybe_write_float3_attr(&mut s, b"hdrgm:Gamma", gamma);
    maybe_write_float3_attr(&mut s, b"hdrgm:OffsetSDR", info.epsilon_sdr);
    maybe_write_float3_attr(&mut s, b"hdrgm:OffsetHDR", info.epsilon_hdr);
    s.extend_from_slice(
        b"    </rdf:Description>\n\
  </rdf:RDF>\n\
</x:xmpmeta>",
    );
    s
}

/// Exact byte-for-byte port of `get_base_image_xmp_metadata` in `SkJpegGainmapEncoder.cpp`.
pub fn build_base_image_xmp_metadata(gainmap_item_length: usize) -> Vec<u8> {
    let mut s = Vec::with_capacity(640);
    s.extend_from_slice(
        b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 5.1.2\">\n\
  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
    <rdf:Description\n\
        xmlns:Container=\"http://ns.google.com/photos/1.0/container/\"\n\
        xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\"\n\
        xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\"\n\
        hdrgm:Version=\"1.0\">\n\
      <Container:Directory>\n\
        <rdf:Seq>\n\
          <rdf:li rdf:parseType=\"Resource\">\n\
            <Container:Item\n\
             Item:Semantic=\"Primary\"\n\
             Item:Mime=\"image/jpeg\"/>\n\
          </rdf:li>\n\
          <rdf:li rdf:parseType=\"Resource\">\n\
            <Container:Item\n\
             Item:Semantic=\"GainMap\"\n\
             Item:Mime=\"image/jpeg\"\n\
             Item:Length=\"",
    );
    s.extend_from_slice(gainmap_item_length.to_string().as_bytes());
    s.extend_from_slice(
        b"\"/>\n\
          </rdf:li>\n\
        </rdf:Seq>\n\
      </Container:Directory>\n\
    </rdf:Description>\n\
  </rdf:RDF>\n\
</x:xmpmeta>\n",
    );
    s
}

/// Compute the byte size of the MPF APP2 segment for `image_number` in an `image_count`-image MPF.
pub fn mpf_segment_size(image_count: usize, image_number: u32) -> usize {
    let mp_params = MultiPictureParameters::with_image_count(image_count);
    build_mpf_segment(&mp_params, image_number).len()
}

/// Exact port of `mp_segment_offset` in `SkJpegGainmapEncoder.cpp`.
/// Scans until StartOfScan and returns the offset of the StartOfScan marker, or `None` on failure.
pub fn find_mp_segment_insertion_offset(image: &[u8]) -> Option<usize> {
    let mut scan = SegmentScanner::new(MARKER_START_OF_SCAN);
    scan.on_bytes(image);
    if !scan.is_done() {
        return None;
    }
    let off = scan.segments().last()?.offset;
    if off == 0 {
        return None;
    }
    Some(off)
}

/// Exact port of `SkJpegGainmapEncoder::MakeMPF`.
/// Computes insertion offsets and MPF parameters for all images and returns the concatenated
/// Multi-Picture JPEG stream.
pub fn assemble_mpf(images: &[&[u8]]) -> Option<Vec<u8>> {
    let image_count = images.len();
    if image_count < 1 {
        return Some(Vec::new());
    }

    let mut mp_segment_offsets = Vec::with_capacity(image_count);
    let mut mp_params = MultiPictureParameters::with_image_count(image_count);
    let mut cumulative_size: usize = 0;

    for (i, &img) in images.iter().enumerate() {
        let offset = find_mp_segment_insertion_offset(img)?;
        mp_segment_offsets.push(offset);

        let mpf_len = build_mpf_segment(&mp_params, i as u32).len();
        let image_size = img.len().checked_add(mpf_len)?;
        mp_params.images[i].data_offset =
            get_image_data_offset(cumulative_size, mp_segment_offsets[0]);
        mp_params.images[i].size = image_size as u32;
        cumulative_size = cumulative_size.checked_add(image_size)?;
    }

    let mut out = Vec::with_capacity(cumulative_size);
    for (i, &img) in images.iter().enumerate() {
        let split = mp_segment_offsets[i];
        let mpf_segment = build_mpf_segment(&mp_params, i as u32);
        out.extend_from_slice(&img[..split]);
        out.extend_from_slice(&mpf_segment);
        out.extend_from_slice(&img[split..]);
    }
    Some(out)
}
