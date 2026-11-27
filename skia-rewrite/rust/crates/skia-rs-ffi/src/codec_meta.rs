//! C++/Rust FFI bindings for `skia-rs-codec-meta` (`P2.2`).

use std::ffi::{c_char, c_void, CStr};
use std::slice;

use skia_rs_codec_meta::{
    color_profile,
    exif::{self, ExifMetadata},
    gainmap::{self, BaseImageType, GainmapInfo, GainmapType},
    hdr_agtm::{
        self, AdaptiveGlobalToneMap, AlternateImage, ColorFilterUniforms, ColorGainFunction,
        ComponentMixingFunction, ControlPoint, GainCurve, HeadroomAdaptiveToneMap, Weighting,
        MAX_NUM_ALTERNATE_IMAGES, MAX_NUM_CONTROL_POINTS,
    },
    hdr_metadata::{ContentLightLevelInformation, HdrMetadata, MasteringDisplayColorVolume},
    icc::{self, IccCicp, IccCurve, IccLutStage, IccMatrix3x4, IccProfileDesc},
    tiff::{self, ImageFileDirectory},
    xmp::XmpDocument,
};
use skia_rs_color::color_space::{ColorSpacePrimaries, Matrix3x3, TransferFunction};

// ============================================================================
// Owned byte buffer helper
// ============================================================================

#[repr(C)]
pub struct SkiaRsMetaBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl SkiaRsMetaBytes {
    fn from_vec(mut v: Vec<u8>) -> Self {
        let out = Self {
            ptr: v.as_mut_ptr(),
            len: v.len(),
            cap: v.capacity(),
        };
        std::mem::forget(v);
        out
    }

    fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_codec_meta_free_bytes(buf: SkiaRsMetaBytes) {
    if !buf.ptr.is_null() && buf.cap > 0 {
        drop(Vec::from_raw_parts(buf.ptr, buf.len, buf.cap));
    }
}

#[inline]
unsafe fn slice_from_raw<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        slice::from_raw_parts(ptr, len)
    }
}

// ============================================================================
// TIFF (`SkTiffUtility.cpp`)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_tiff_is_valid_type(ty: u16) -> bool {
    tiff::is_valid_type(ty)
}

#[no_mangle]
pub extern "C" fn skia_rs_tiff_bytes_for_type(ty: u16) -> usize {
    tiff::bytes_for_type(ty)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_tiff_parse_header(
    data: *const u8,
    len: usize,
    out_little_endian: *mut bool,
    out_ifd_offset: *mut u32,
) -> bool {
    let bytes = slice_from_raw(data, len);
    match tiff::parse_header(bytes) {
        Some((le, off)) => {
            if !out_little_endian.is_null() {
                *out_little_endian = le;
            }
            if !out_ifd_offset.is_null() {
                *out_ifd_offset = off;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_tiff_validate_ifd(
    data: *const u8,
    len: usize,
    little_endian: bool,
    ifd_offset: u32,
    allow_truncated: bool,
    out_num_entries: *mut u16,
    out_next_ifd_offset: *mut u32,
) -> bool {
    let bytes = slice_from_raw(data, len);
    match tiff::validate_ifd(bytes, little_endian, ifd_offset, allow_truncated) {
        Some((num, next)) => {
            if !out_num_entries.is_null() {
                *out_num_entries = num;
            }
            if !out_next_ifd_offset.is_null() {
                *out_next_ifd_offset = next;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_tiff_get_entry_tag(
    data: *const u8,
    len: usize,
    little_endian: bool,
    ifd_offset: u32,
    num_entries: u16,
    next_ifd_offset: u32,
    entry_index: u16,
) -> u16 {
    let bytes = slice_from_raw(data, len);
    let ifd = ImageFileDirectory::from_parts(
        bytes,
        little_endian,
        ifd_offset,
        num_entries,
        next_ifd_offset,
    );
    ifd.entry_tag(entry_index)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_tiff_get_entry_raw_data(
    data: *const u8,
    len: usize,
    little_endian: bool,
    ifd_offset: u32,
    num_entries: u16,
    next_ifd_offset: u32,
    entry_index: u16,
    out_tag: *mut u16,
    out_type: *mut u16,
    out_count: *mut u32,
    out_data_offset: *mut usize,
    out_data_size: *mut usize,
) -> bool {
    let bytes = slice_from_raw(data, len);
    let ifd = ImageFileDirectory::from_parts(
        bytes,
        little_endian,
        ifd_offset,
        num_entries,
        next_ifd_offset,
    );
    match ifd.entry_raw_data(entry_index) {
        Some(raw) => {
            if !out_tag.is_null() {
                *out_tag = raw.tag;
            }
            if !out_type.is_null() {
                *out_type = raw.ty;
            }
            if !out_count.is_null() {
                *out_count = raw.count;
            }
            if !out_data_offset.is_null() {
                *out_data_offset = raw.data_offset;
            }
            if !out_data_size.is_null() {
                *out_data_size = raw.data_size;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_tiff_get_entry_values(
    data: *const u8,
    len: usize,
    little_endian: bool,
    ifd_offset: u32,
    num_entries: u16,
    next_ifd_offset: u32,
    entry_index: u16,
    expected_type: u16,
    count: u32,
    out_values: *mut c_void,
) -> bool {
    if out_values.is_null() && count > 0 {
        return false;
    }
    let bytes = slice_from_raw(data, len);
    let ifd = ImageFileDirectory::from_parts(
        bytes,
        little_endian,
        ifd_offset,
        num_entries,
        next_ifd_offset,
    );
    let n = count as usize;
    match expected_type {
        tiff::TYPE_UNSIGNED_SHORT => {
            let out = if n == 0 {
                &mut []
            } else {
                slice::from_raw_parts_mut(out_values as *mut u16, n)
            };
            ifd.entry_unsigned_short(entry_index, out)
        }
        tiff::TYPE_UNSIGNED_LONG => {
            let out = if n == 0 {
                &mut []
            } else {
                slice::from_raw_parts_mut(out_values as *mut u32, n)
            };
            ifd.entry_unsigned_long(entry_index, out)
        }
        tiff::TYPE_SIGNED_RATIONAL => {
            let out = if n == 0 {
                &mut []
            } else {
                slice::from_raw_parts_mut(out_values as *mut f32, n)
            };
            ifd.entry_signed_rational(entry_index, out)
        }
        tiff::TYPE_UNSIGNED_RATIONAL => {
            let out = if n == 0 {
                &mut []
            } else {
                slice::from_raw_parts_mut(out_values as *mut f32, n)
            };
            ifd.entry_unsigned_rational(entry_index, out)
        }
        _ => false,
    }
}

// ============================================================================
// EXIF & EncodedOrigin (`SkExif.cpp`, `SkParseEncodedOrigin.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SkiaRsExifMetadata {
    pub has_origin: bool,
    pub origin: u32,
    pub has_hdr_headroom: bool,
    pub hdr_headroom: f32,
    pub has_resolution_unit: bool,
    pub resolution_unit: u16,
    pub has_x_resolution: bool,
    pub x_resolution: f32,
    pub has_y_resolution: bool,
    pub y_resolution: f32,
    pub has_pixel_x_dimension: bool,
    pub pixel_x_dimension: u32,
    pub has_pixel_y_dimension: bool,
    pub pixel_y_dimension: u32,
}

impl SkiaRsExifMetadata {
    fn to_rust(&self) -> ExifMetadata {
        ExifMetadata {
            origin: self.has_origin.then_some(self.origin),
            hdr_headroom: self.has_hdr_headroom.then_some(self.hdr_headroom),
            resolution_unit: self.has_resolution_unit.then_some(self.resolution_unit),
            x_resolution: self.has_x_resolution.then_some(self.x_resolution),
            y_resolution: self.has_y_resolution.then_some(self.y_resolution),
            pixel_x_dimension: self.has_pixel_x_dimension.then_some(self.pixel_x_dimension),
            pixel_y_dimension: self.has_pixel_y_dimension.then_some(self.pixel_y_dimension),
        }
    }

    fn from_rust(m: &ExifMetadata) -> Self {
        Self {
            has_origin: m.origin.is_some(),
            origin: m.origin.unwrap_or(0),
            has_hdr_headroom: m.hdr_headroom.is_some(),
            hdr_headroom: m.hdr_headroom.unwrap_or(0.0),
            has_resolution_unit: m.resolution_unit.is_some(),
            resolution_unit: m.resolution_unit.unwrap_or(0),
            has_x_resolution: m.x_resolution.is_some(),
            x_resolution: m.x_resolution.unwrap_or(0.0),
            has_y_resolution: m.y_resolution.is_some(),
            y_resolution: m.y_resolution.unwrap_or(0.0),
            has_pixel_x_dimension: m.pixel_x_dimension.is_some(),
            pixel_x_dimension: m.pixel_x_dimension.unwrap_or(0),
            has_pixel_y_dimension: m.pixel_y_dimension.is_some(),
            pixel_y_dimension: m.pixel_y_dimension.unwrap_or(0),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_exif_parse(
    data: *const u8,
    len: usize,
    inout_meta: *mut SkiaRsExifMetadata,
) {
    if inout_meta.is_null() {
        return;
    }
    let bytes = slice_from_raw(data, len);
    let mut meta = (*inout_meta).to_rust();
    exif::parse_exif_into(&mut meta, bytes);
    *inout_meta = SkiaRsExifMetadata::from_rust(&meta);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_exif_write(
    meta: *const SkiaRsExifMetadata,
    out_bytes: *mut SkiaRsMetaBytes,
) -> bool {
    if meta.is_null() || out_bytes.is_null() {
        return false;
    }
    let rmeta = (*meta).to_rust();
    match exif::write_exif(&rmeta) {
        Some(v) => {
            *out_bytes = SkiaRsMetaBytes::from_vec(v);
            true
        }
        None => {
            *out_bytes = SkiaRsMetaBytes::empty();
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_parse_encoded_origin(
    data: *const u8,
    len: usize,
    out_origin: *mut u32,
) -> bool {
    let bytes = slice_from_raw(data, len);
    match exif::parse_encoded_origin(bytes) {
        Some(o) => {
            if !out_origin.is_null() {
                *out_origin = o;
            }
            true
        }
        None => false,
    }
}

// ============================================================================
// ColorProfile (`SkCodecColorProfile.cpp`)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_color_profile_data_space(data_color_space: u32) -> i32 {
    color_profile::classify_data_space(data_color_space) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_color_profile_cicp_android(
    color_primaries: u8,
    transfer_characteristics: u8,
    matrix_coefficients: u8,
    full_range_flag: u8,
    out_trfn: *mut TransferFunction,
    out_matrix: *mut Matrix3x3,
) -> bool {
    match color_profile::cicp_get_android_color_space(
        color_primaries,
        transfer_characteristics,
        matrix_coefficients,
        full_range_flag,
    ) {
        Some((trfn, mat)) => {
            if !out_trfn.is_null() {
                *out_trfn = trfn;
            }
            if !out_matrix.is_null() {
                *out_matrix = mat;
            }
            true
        }
        None => false,
    }
}

// ============================================================================
// GainmapInfo (`SkGainmapInfo.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SkiaRsGainmapInfo {
    pub gainmap_ratio_min: [f32; 4],
    pub gainmap_ratio_max: [f32; 4],
    pub gainmap_gamma: [f32; 4],
    pub epsilon_sdr: [f32; 4],
    pub epsilon_hdr: [f32; 4],
    pub display_ratio_sdr: f32,
    pub display_ratio_hdr: f32,
    pub base_image_type: i32,
    pub gainmap_type: i32,
    pub has_gainmap_math_color_space: bool,
}

impl SkiaRsGainmapInfo {
    fn from_rust(g: &GainmapInfo) -> Self {
        Self {
            gainmap_ratio_min: g.gainmap_ratio_min,
            gainmap_ratio_max: g.gainmap_ratio_max,
            gainmap_gamma: g.gainmap_gamma,
            epsilon_sdr: g.epsilon_sdr,
            epsilon_hdr: g.epsilon_hdr,
            display_ratio_sdr: g.display_ratio_sdr,
            display_ratio_hdr: g.display_ratio_hdr,
            base_image_type: g.base_image_type as i32,
            gainmap_type: g.gainmap_type as i32,
            has_gainmap_math_color_space: g.has_gainmap_math_color_space,
        }
    }

    fn to_rust(&self) -> GainmapInfo {
        GainmapInfo {
            gainmap_ratio_min: self.gainmap_ratio_min,
            gainmap_ratio_max: self.gainmap_ratio_max,
            gainmap_gamma: self.gainmap_gamma,
            epsilon_sdr: self.epsilon_sdr,
            epsilon_hdr: self.epsilon_hdr,
            display_ratio_sdr: self.display_ratio_sdr,
            display_ratio_hdr: self.display_ratio_hdr,
            base_image_type: if self.base_image_type == 1 {
                BaseImageType::Hdr
            } else {
                BaseImageType::Sdr
            },
            gainmap_type: if self.gainmap_type == 1 {
                GainmapType::Apple
            } else {
                GainmapType::Default
            },
            has_gainmap_math_color_space: self.has_gainmap_math_color_space,
        }
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_gainmap_is_ultra_hdr_v1_compatible(
    base_image_type: i32,
    has_math_color_space: bool,
) -> bool {
    let ty = if base_image_type == 1 {
        BaseImageType::Hdr
    } else {
        BaseImageType::Sdr
    };
    gainmap::is_ultra_hdr_v1_compatible(ty, has_math_color_space)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gainmap_parse_version(data: *const u8, len: usize) -> bool {
    if data.is_null() {
        return false;
    }
    gainmap::parse_version(slice_from_raw(data, len))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gainmap_serialize_version(out4: *mut u8) {
    if out4.is_null() {
        return;
    }
    let v = gainmap::serialize_version();
    slice::from_raw_parts_mut(out4, 4).copy_from_slice(&v);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gainmap_parse(
    data: *const u8,
    len: usize,
    out_info: *mut SkiaRsGainmapInfo,
) -> bool {
    if data.is_null() || out_info.is_null() {
        return false;
    }
    match gainmap::parse_gainmap_info(slice_from_raw(data, len)) {
        Some(info) => {
            *out_info = SkiaRsGainmapInfo::from_rust(&info);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gainmap_serialize(
    info: *const SkiaRsGainmapInfo,
    out_bytes: *mut SkiaRsMetaBytes,
) {
    if info.is_null() || out_bytes.is_null() {
        return;
    }
    let bytes = gainmap::serialize_gainmap_info(&(*info).to_rust());
    *out_bytes = SkiaRsMetaBytes::from_vec(bytes);
}

// ============================================================================
// HDR Metadata (`SkHdrMetadata.cpp`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_clli_parse(
    data: *const u8,
    len: usize,
    out: *mut ContentLightLevelInformation,
) -> bool {
    if data.is_null() || out.is_null() {
        return false;
    }
    match ContentLightLevelInformation::parse(slice_from_raw(data, len)) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_clli_serialize(
    clli: *const ContentLightLevelInformation,
    out4: *mut u8,
) {
    if clli.is_null() || out4.is_null() {
        return;
    }
    let b = (*clli).serialize();
    slice::from_raw_parts_mut(out4, 4).copy_from_slice(&b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_clli_parse_png(
    data: *const u8,
    len: usize,
    out: *mut ContentLightLevelInformation,
) -> bool {
    if data.is_null() || out.is_null() {
        return false;
    }
    match ContentLightLevelInformation::parse_png_chunk(slice_from_raw(data, len)) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_clli_serialize_png(
    clli: *const ContentLightLevelInformation,
    out8: *mut u8,
) {
    if clli.is_null() || out8.is_null() {
        return;
    }
    let b = (*clli).serialize_png_chunk();
    slice::from_raw_parts_mut(out8, 8).copy_from_slice(&b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_clli_to_string(
    clli: *const ContentLightLevelInformation,
    out: *mut SkiaRsMetaBytes,
) {
    if clli.is_null() || out.is_null() {
        return;
    }
    *out = SkiaRsMetaBytes::from_vec((*clli).format_string().into_bytes());
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_mdcv_parse(
    data: *const u8,
    len: usize,
    out: *mut MasteringDisplayColorVolume,
) -> bool {
    if data.is_null() || out.is_null() {
        return false;
    }
    match MasteringDisplayColorVolume::parse(slice_from_raw(data, len)) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_mdcv_serialize(
    mdcv: *const MasteringDisplayColorVolume,
    out24: *mut u8,
) {
    if mdcv.is_null() || out24.is_null() {
        return;
    }
    let b = (*mdcv).serialize();
    slice::from_raw_parts_mut(out24, 24).copy_from_slice(&b);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_mdcv_to_string(
    mdcv: *const MasteringDisplayColorVolume,
    out: *mut SkiaRsMetaBytes,
) {
    if mdcv.is_null() || out.is_null() {
        return;
    }
    *out = SkiaRsMetaBytes::from_vec((*mdcv).format_string().into_bytes());
}

// ============================================================================
// HDR AGTM (`SkHdrAgtm.cpp`, `SkHdrAgtmParse.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsAgtmAlternateImage {
    pub hdr_headroom: f32,
    pub mix: ComponentMixingFunction,
    /// Actual count in C++ vector (may exceed 32 on invalid inputs passed to Validate).
    pub raw_num_control_points: u32,
    pub control_points: [ControlPoint; MAX_NUM_CONTROL_POINTS],
}

impl Default for SkiaRsAgtmAlternateImage {
    fn default() -> Self {
        Self {
            hdr_headroom: 0.0,
            mix: ComponentMixingFunction::default(),
            raw_num_control_points: 0,
            control_points: [ControlPoint::default(); MAX_NUM_CONTROL_POINTS],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsAgtmHatm {
    pub baseline_hdr_headroom: f32,
    pub primaries: ColorSpacePrimaries,
    /// Actual count in C++ vector (may exceed 4 on invalid inputs passed to Validate).
    pub raw_num_alternate_images: u32,
    pub alternate_images: [SkiaRsAgtmAlternateImage; MAX_NUM_ALTERNATE_IMAGES],
}

impl Default for SkiaRsAgtmHatm {
    fn default() -> Self {
        Self {
            baseline_hdr_headroom: 0.0,
            primaries: ColorSpacePrimaries::default(),
            raw_num_alternate_images: 0,
            alternate_images: [SkiaRsAgtmAlternateImage::default(); MAX_NUM_ALTERNATE_IMAGES],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct SkiaRsAgtm {
    pub hdr_reference_white: f32,
    pub has_hatm: bool,
    pub hatm: SkiaRsAgtmHatm,
}

impl SkiaRsAgtmHatm {
    fn to_rust(&self) -> HeadroomAdaptiveToneMap {
        let alt_count = (self.raw_num_alternate_images as usize).min(MAX_NUM_ALTERNATE_IMAGES);
        let mut alternate_images = Vec::with_capacity(alt_count);
        for a in 0..alt_count {
            let src = &self.alternate_images[a];
            let cp_count = (src.raw_num_control_points as usize).min(MAX_NUM_CONTROL_POINTS);
            alternate_images.push(AlternateImage {
                hdr_headroom: src.hdr_headroom,
                color_gain_function: ColorGainFunction {
                    component_mixing: src.mix,
                    gain_curve: GainCurve {
                        control_points: src.control_points[..cp_count].to_vec(),
                    },
                },
            });
        }
        HeadroomAdaptiveToneMap {
            baseline_hdr_headroom: self.baseline_hdr_headroom,
            gain_application_space_primaries: self.primaries,
            alternate_images,
        }
    }

    fn has_overflowed_counts(&self) -> bool {
        if (self.raw_num_alternate_images as usize) > MAX_NUM_ALTERNATE_IMAGES {
            return true;
        }
        for a in 0..(self.raw_num_alternate_images as usize) {
            if (self.alternate_images[a].raw_num_control_points as usize) > MAX_NUM_CONTROL_POINTS {
                return true;
            }
        }
        false
    }

    fn from_rust(hatm: &HeadroomAdaptiveToneMap) -> Self {
        let mut out = Self {
            baseline_hdr_headroom: hatm.baseline_hdr_headroom,
            primaries: hatm.gain_application_space_primaries,
            raw_num_alternate_images: hatm.alternate_images.len() as u32,
            alternate_images: [SkiaRsAgtmAlternateImage::default(); MAX_NUM_ALTERNATE_IMAGES],
        };
        for (a, alt) in hatm
            .alternate_images
            .iter()
            .take(MAX_NUM_ALTERNATE_IMAGES)
            .enumerate()
        {
            let cps = &alt.color_gain_function.gain_curve.control_points;
            let n = cps.len().min(MAX_NUM_CONTROL_POINTS);
            out.alternate_images[a].hdr_headroom = alt.hdr_headroom;
            out.alternate_images[a].mix = alt.color_gain_function.component_mixing;
            out.alternate_images[a].raw_num_control_points = cps.len() as u32;
            out.alternate_images[a].control_points[..n].copy_from_slice(&cps[..n]);
        }
        out
    }
}

impl SkiaRsAgtm {
    fn to_rust(&self) -> AdaptiveGlobalToneMap {
        AdaptiveGlobalToneMap {
            hdr_reference_white: self.hdr_reference_white,
            headroom_adaptive_tone_map: self.has_hatm.then(|| self.hatm.to_rust()),
        }
    }

    fn has_overflowed_counts(&self) -> bool {
        self.has_hatm && self.hatm.has_overflowed_counts()
    }

    fn from_rust(agtm: &AdaptiveGlobalToneMap) -> Self {
        Self {
            hdr_reference_white: agtm.hdr_reference_white,
            has_hatm: agtm.headroom_adaptive_tone_map.is_some(),
            hatm: agtm
                .headroom_adaptive_tone_map
                .as_ref()
                .map(SkiaRsAgtmHatm::from_rust)
                .unwrap_or_default(),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_eval_component_mixing(
    mix: *const ComponentMixingFunction,
    c: *const f32,
    out_c: *mut f32,
) {
    if mix.is_null() || c.is_null() || out_c.is_null() {
        return;
    }
    let cin = slice::from_raw_parts(c, 4);
    let res = hdr_agtm::evaluate_component_mixing_function(*mix, [cin[0], cin[1], cin[2], cin[3]]);
    slice::from_raw_parts_mut(out_c, 4).copy_from_slice(&res);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_eval_gain_curve(
    cps: *const ControlPoint,
    count: usize,
    x: f32,
) -> f32 {
    if cps.is_null() || count == 0 {
        return 0.0;
    }
    let cp_slice = slice::from_raw_parts(cps, count);
    hdr_agtm::evaluate_gain_curve(cp_slice, x)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_eval_color_gain(
    mix: *const ComponentMixingFunction,
    cps: *const ControlPoint,
    count: usize,
    c: *const f32,
    out_c: *mut f32,
) {
    if mix.is_null() || c.is_null() || out_c.is_null() {
        return;
    }
    let cp_slice = if cps.is_null() || count == 0 {
        &[]
    } else {
        slice::from_raw_parts(cps, count)
    };
    let gain = ColorGainFunction {
        component_mixing: *mix,
        gain_curve: GainCurve {
            control_points: cp_slice.to_vec(),
        },
    };
    let cin = slice::from_raw_parts(c, 4);
    let res = hdr_agtm::evaluate_color_gain_function(&gain, [cin[0], cin[1], cin[2], cin[3]]);
    slice::from_raw_parts_mut(out_c, 4).copy_from_slice(&res);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_populate_slope_pchip(
    cps: *mut ControlPoint,
    count: usize,
) {
    if cps.is_null() || count == 0 {
        return;
    }
    let cp_slice = slice::from_raw_parts_mut(cps, count);
    hdr_agtm::populate_slope_from_pchip(cp_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_populate_rwtmo(inout_hatm: *mut SkiaRsAgtmHatm) {
    if inout_hatm.is_null() {
        return;
    }
    let mut hatm = (*inout_hatm).to_rust();
    hdr_agtm::populate_using_rwtmo(&mut hatm);
    *inout_hatm = SkiaRsAgtmHatm::from_rust(&hatm);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_compute_weighting(
    hatm: *const SkiaRsAgtmHatm,
    targeted_hdr_headroom: f32,
    out_weighting: *mut Weighting,
) {
    if hatm.is_null() || out_weighting.is_null() {
        return;
    }
    let r = (*hatm).to_rust();
    *out_weighting = hdr_agtm::compute_weighting(&r, targeted_hdr_headroom);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_apply_gain(
    hatm: *const SkiaRsAgtmHatm,
    colors: *mut f32,
    count: usize,
    targeted_hdr_headroom: f32,
) {
    if hatm.is_null() || colors.is_null() || count == 0 {
        return;
    }
    let r = (*hatm).to_rust();
    let floats = slice::from_raw_parts_mut(colors, count * 4);
    let mut vec_colors: Vec<[f32; 4]> = floats
        .chunks_exact(4)
        .map(|ch| [ch[0], ch[1], ch[2], ch[3]])
        .collect();
    hdr_agtm::apply_gain(&r, &mut vec_colors, targeted_hdr_headroom);
    for (dst, src) in floats.chunks_exact_mut(4).zip(vec_colors.iter()) {
        dst.copy_from_slice(src);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_build_xym_pixels(
    hatm: *const SkiaRsAgtmHatm,
    out_width: *mut usize,
    out_height: *mut usize,
    out_pixels: *mut SkiaRsMetaBytes,
) -> bool {
    if hatm.is_null() || out_width.is_null() || out_height.is_null() || out_pixels.is_null() {
        return false;
    }
    let r = (*hatm).to_rust();
    match hdr_agtm::build_gain_curve_xym_pixels(&r) {
        Some((w, h, pixels)) => {
            *out_width = w;
            *out_height = h;
            let mut bytes = Vec::with_capacity(pixels.len() * 16);
            for px in pixels {
                for f in px {
                    bytes.extend_from_slice(&f.to_ne_bytes());
                }
            }
            *out_pixels = SkiaRsMetaBytes::from_vec(bytes);
            true
        }
        None => {
            *out_width = 0;
            *out_height = 0;
            *out_pixels = SkiaRsMetaBytes::empty();
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_compute_color_filter_uniforms(
    hatm: *const SkiaRsAgtmHatm,
    targeted_hdr_headroom: f32,
    scale_factor: f32,
    out_uniforms: *mut ColorFilterUniforms,
) {
    if hatm.is_null() || out_uniforms.is_null() {
        return;
    }
    let r = (*hatm).to_rust();
    *out_uniforms =
        hdr_agtm::compute_color_filter_uniforms(&r, targeted_hdr_headroom, scale_factor);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_populate_tone_map_params(
    has_clli: bool,
    clli: *const ContentLightLevelInformation,
    has_mdcv: bool,
    mdcv: *const MasteringDisplayColorVolume,
    has_agtm: bool,
    agtm: *const SkiaRsAgtm,
    input_is_pq_or_hlg: bool,
    input_pq_or_hlg_white: f32,
    out_agtm: *mut SkiaRsAgtm,
    out_scale_factor: *mut f32,
) -> bool {
    let meta = HdrMetadata {
        clli: if has_clli && !clli.is_null() {
            Some(*clli)
        } else {
            None
        },
        mdcv: if has_mdcv && !mdcv.is_null() {
            Some(*mdcv)
        } else {
            None
        },
        agtm: if has_agtm && !agtm.is_null() {
            Some((*agtm).to_rust())
        } else {
            None
        },
    };
    match hdr_agtm::populate_tone_map_agtm_params(&meta, input_is_pq_or_hlg, input_pq_or_hlg_white)
    {
        Some((res_agtm, scale)) => {
            if !out_agtm.is_null() {
                *out_agtm = SkiaRsAgtm::from_rust(&res_agtm);
            }
            if !out_scale_factor.is_null() {
                *out_scale_factor = scale;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_validate_agtm(agtm: *const SkiaRsAgtm) -> bool {
    if agtm.is_null() {
        return false;
    }
    if (*agtm).has_overflowed_counts() {
        return false;
    }
    hdr_agtm::validate_agtm(&(*agtm).to_rust())
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_validate_hatm(hatm: *const SkiaRsAgtmHatm) -> bool {
    if hatm.is_null() {
        return false;
    }
    if (*hatm).has_overflowed_counts() {
        return false;
    }
    hdr_agtm::validate_hatm(&(*hatm).to_rust())
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_parse(
    data: *const u8,
    len: usize,
    out_agtm: *mut SkiaRsAgtm,
) -> bool {
    if data.is_null() || out_agtm.is_null() {
        return false;
    }
    match hdr_agtm::parse_agtm(slice_from_raw(data, len)) {
        Some(parsed) => {
            *out_agtm = SkiaRsAgtm::from_rust(&parsed);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_serialize(
    agtm: *const SkiaRsAgtm,
    out_bytes: *mut SkiaRsMetaBytes,
) -> bool {
    if agtm.is_null() || out_bytes.is_null() {
        return false;
    }
    if (*agtm).has_overflowed_counts() {
        *out_bytes = SkiaRsMetaBytes::empty();
        return false;
    }
    match hdr_agtm::serialize_agtm(&(*agtm).to_rust()) {
        Some(v) => {
            *out_bytes = SkiaRsMetaBytes::from_vec(v);
            true
        }
        None => {
            *out_bytes = SkiaRsMetaBytes::empty();
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_agtm_to_string(
    agtm: *const SkiaRsAgtm,
    out_bytes: *mut SkiaRsMetaBytes,
) {
    if agtm.is_null() || out_bytes.is_null() {
        return;
    }
    let s = (*agtm).to_rust().format_string();
    *out_bytes = SkiaRsMetaBytes::from_vec(s.into_bytes());
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_hdr_metadata_to_string(
    has_clli: bool,
    clli: *const ContentLightLevelInformation,
    has_mdcv: bool,
    mdcv: *const MasteringDisplayColorVolume,
    has_agtm: bool,
    agtm: *const SkiaRsAgtm,
    out_bytes: *mut SkiaRsMetaBytes,
) {
    if out_bytes.is_null() {
        return;
    }
    let meta = HdrMetadata {
        clli: if has_clli && !clli.is_null() {
            Some(*clli)
        } else {
            None
        },
        mdcv: if has_mdcv && !mdcv.is_null() {
            Some(*mdcv)
        } else {
            None
        },
        agtm: if has_agtm && !agtm.is_null() {
            Some((*agtm).to_rust())
        } else {
            None
        },
    };
    *out_bytes = SkiaRsMetaBytes::from_vec(meta.format_string().into_bytes());
}

// ============================================================================
// XMP (`SkXmp.cpp`)
// ============================================================================

pub struct SkiaRsXmp {
    doc: XmpDocument,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xmp_make(
    std_data: *const u8,
    std_len: usize,
    ext_data: *const u8,
    ext_len: usize,
) -> *mut SkiaRsXmp {
    if std_data.is_null() {
        return std::ptr::null_mut();
    }
    let std_slice = slice_from_raw(std_data, std_len);
    let ext_slice = if !ext_data.is_null() {
        Some(slice_from_raw(ext_data, ext_len))
    } else {
        None
    };
    match XmpDocument::from_bytes(std_slice, ext_slice) {
        Some(doc) => Box::into_raw(Box::new(SkiaRsXmp { doc })),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xmp_destroy(xmp: *mut SkiaRsXmp) {
    if !xmp.is_null() {
        drop(Box::from_raw(xmp));
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xmp_get_extended_guid(xmp: *const SkiaRsXmp) -> *const c_char {
    if xmp.is_null() {
        return std::ptr::null();
    }
    match (*xmp).doc.extended_xmp_guid_cstr() {
        Some(bytes) => bytes.as_ptr() as *const c_char,
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xmp_get_container_gainmap_location(
    xmp: *const SkiaRsXmp,
    out_offset: *mut usize,
    out_size: *mut usize,
) -> bool {
    if xmp.is_null() {
        return false;
    }
    match (*xmp).doc.get_container_gainmap_location() {
        Some((off, sz)) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            if !out_size.is_null() {
                *out_size = sz;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xmp_get_gainmap_info_apple(
    xmp: *const SkiaRsXmp,
    exif_hdr_headroom: f32,
    out_info: *mut SkiaRsGainmapInfo,
) -> bool {
    if xmp.is_null() || out_info.is_null() {
        return false;
    }
    match (*xmp).doc.get_gainmap_info_apple(exif_hdr_headroom) {
        Some(info) => {
            *out_info = SkiaRsGainmapInfo::from_rust(&info);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_xmp_get_gainmap_info_adobe(
    xmp: *const SkiaRsXmp,
    out_info: *mut SkiaRsGainmapInfo,
) -> bool {
    if xmp.is_null() {
        return false;
    }
    match (*xmp).doc.get_gainmap_info_adobe() {
        Some(info) => {
            if !out_info.is_null() {
                *out_info = SkiaRsGainmapInfo::from_rust(&info);
            }
            true
        }
        None => false,
    }
}

// ============================================================================
// ICC Profile Encoder (`SkICC.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsIccCurve {
    pub table_entries: u32,
    pub parametric: TransferFunction,
    pub table_16: *const u8,
}

impl SkiaRsIccCurve {
    unsafe fn to_rust<'a>(&self) -> IccCurve<'a> {
        if self.table_entries > 0 {
            let byte_len = (self.table_entries as usize) * 2;
            let bytes = slice_from_raw(self.table_16, byte_len);
            IccCurve::Table16 {
                table_entries: self.table_entries,
                table_16_be: bytes,
            }
        } else {
            IccCurve::Parametric(self.parametric)
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsIccLutStage {
    pub b_curves: [SkiaRsIccCurve; 3],
    pub has_a_curves: bool,
    pub a_curves: [SkiaRsIccCurve; 3],
    pub has_grid: bool,
    pub grid_points: [u8; 3],
    pub grid_16: *const u8,
    pub has_matrix: bool,
    pub m_curves: [SkiaRsIccCurve; 3],
    pub matrix: [[f32; 4]; 3],
}

impl SkiaRsIccLutStage {
    unsafe fn to_rust<'a>(&self) -> IccLutStage<'a> {
        let b_curves = [
            self.b_curves[0].to_rust(),
            self.b_curves[1].to_rust(),
            self.b_curves[2].to_rust(),
        ];
        let a_curves = self.has_a_curves.then(|| {
            [
                self.a_curves[0].to_rust(),
                self.a_curves[1].to_rust(),
                self.a_curves[2].to_rust(),
            ]
        });
        let (grid_points, grid_16_be) = if self.has_grid {
            let gp = self.grid_points;
            let byte_len =
                3usize * (gp[0] as usize) * (gp[1] as usize) * (gp[2] as usize) * 2usize;
            (Some(gp), Some(slice_from_raw(self.grid_16, byte_len)))
        } else {
            (None, None)
        };
        let (m_curves, matrix) = if self.has_matrix {
            (
                Some([
                    self.m_curves[0].to_rust(),
                    self.m_curves[1].to_rust(),
                    self.m_curves[2].to_rust(),
                ]),
                Some(IccMatrix3x4 { vals: self.matrix }),
            )
        } else {
            (None, None)
        };
        IccLutStage {
            b_curves,
            a_curves,
            grid_points,
            grid_16_be,
            m_curves,
            matrix,
        }
    }
}

#[repr(C)]
pub struct SkiaRsIccProfileDesc {
    pub data_color_space: u32,
    pub pcs: u32,
    pub has_to_xyzd50: bool,
    pub to_xyzd50: Matrix3x3,
    pub has_trc: bool,
    pub trc: [SkiaRsIccCurve; 3],
    pub trc1_same_as_0: bool,
    pub trc2_same_as_1: bool,
    pub has_cicp: bool,
    pub cicp: [u8; 4],
    pub has_hagc: bool,
    pub hagc_ptr: *const u8,
    pub hagc_len: usize,
    pub has_a2b: bool,
    pub a2b: SkiaRsIccLutStage,
    pub has_b2a: bool,
    pub b2a: SkiaRsIccLutStage,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_icc_write_profile(
    profile: *const SkiaRsIccProfileDesc,
    desc_cstr: *const c_char,
    out_bytes: *mut SkiaRsMetaBytes,
) {
    if profile.is_null() || out_bytes.is_null() {
        return;
    }
    let p = &*profile;
    let desc_opt = if desc_cstr.is_null() {
        None
    } else {
        CStr::from_ptr(desc_cstr).to_str().ok()
    };
    let rust_desc = IccProfileDesc {
        data_color_space: p.data_color_space,
        pcs: p.pcs,
        to_xyzd50: p.has_to_xyzd50.then_some(p.to_xyzd50),
        trc: p
            .has_trc
            .then(|| [p.trc[0].to_rust(), p.trc[1].to_rust(), p.trc[2].to_rust()]),
        trc1_same_as_0: p.trc1_same_as_0,
        trc2_same_as_1: p.trc2_same_as_1,
        cicp: p.has_cicp.then_some(IccCicp {
            color_primaries: p.cicp[0],
            transfer_characteristics: p.cicp[1],
            matrix_coefficients: p.cicp[2],
            video_full_range_flag: p.cicp[3],
        }),
        hagc: p
            .has_hagc
            .then(|| slice_from_raw(p.hagc_ptr, p.hagc_len)),
        a2b: p.has_a2b.then(|| p.a2b.to_rust()),
        b2a: p.has_b2a.then(|| p.b2a.to_rust()),
    };
    let bytes = icc::write_icc_profile(&rust_desc, desc_opt);
    *out_bytes = SkiaRsMetaBytes::from_vec(bytes);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_icc_write_from_color_space(
    fn_tf: *const TransferFunction,
    to_xyzd50: *const Matrix3x3,
    agtm_ptr: *const u8,
    agtm_len: usize,
    out_bytes: *mut SkiaRsMetaBytes,
) {
    if fn_tf.is_null() || to_xyzd50.is_null() || out_bytes.is_null() {
        return;
    }
    let agtm = if !agtm_ptr.is_null() && agtm_len > 0 {
        Some(slice_from_raw(agtm_ptr, agtm_len))
    } else {
        None
    };
    let bytes = icc::write_icc_profile_from_color_space(&*fn_tf, &*to_xyzd50, agtm);
    *out_bytes = SkiaRsMetaBytes::from_vec(bytes);
}
