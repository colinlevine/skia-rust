//! C++/Rust FFI bindings for `skia-rs-codec-jpeg` (`P2.5`).

use std::ffi::c_void;
use std::slice;

use skia_rs_codec_jpeg::{
    constants::JColorSpace,
    decoder::{
        self, ConversionPlan, EncodedColorResult, StoragePlan, SubsetCropPlan, YuvaPlanesPlan,
        YuvaQueryPlan,
    },
    encoder::{self, AlphaOption, Downsample, RgbEncodePlan, YuvEncodePlan},
    metadata::{self, JpegMarkerRef, MetadataReadResult},
    multi_picture::{self, MpImage, MultiPictureParameters},
    segment_scan::{self, JpegSegment, SegmentScannerState},
    source_mgr::{self, BufferedSkipPlan, MemorySkipPlan, UnseekableSubsetPlan},
    xmp,
};
use skia_rs_codec_meta::{
    color_profile::DataSpace,
    gainmap::{BaseImageType, GainmapInfo, GainmapType},
};
use skia_rs_color::{
    image_info::{AlphaType, ColorType},
    yuva_info::{PlaneConfig, Subsampling},
};

// ============================================================================
// Owned byte buffer & shared view structs
// ============================================================================

#[repr(C)]
pub struct SkiaRsJpegBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl SkiaRsJpegBytes {
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
pub unsafe extern "C" fn skia_rs_jpeg_free_bytes(buf: SkiaRsJpegBytes) {
    if !buf.ptr.is_null() && buf.cap > 0 {
        drop(Vec::from_raw_parts(buf.ptr, buf.len, buf.cap));
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsJpegSlice {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SkiaRsJpegMarkerView {
    pub marker: u8,
    pub ptr: *const u8,
    pub len: usize,
}

/// Result of `skia_rs_jpeg_read_*` metadata queries:
/// - `kind == 0`: not found or invalid
/// - `kind == 1`: single-segment zero-copy reference (`marker_index`, `offset`, `len`)
/// - `kind == 2`: owned assembled buffer in `owned` (free with `skia_rs_jpeg_free_bytes`)
#[repr(C)]
pub struct SkiaRsJpegMetadataResult {
    pub kind: i32,
    pub marker_index: usize,
    pub offset: usize,
    pub len: usize,
    pub owned: SkiaRsJpegBytes,
}

impl SkiaRsJpegMetadataResult {
    fn from_read_result(r: MetadataReadResult) -> Self {
        match r {
            MetadataReadResult::NotFoundOrInvalid => Self {
                kind: 0,
                marker_index: 0,
                offset: 0,
                len: 0,
                owned: SkiaRsJpegBytes::empty(),
            },
            MetadataReadResult::SingleRef {
                marker_index,
                offset,
                len,
            } => Self {
                kind: 1,
                marker_index,
                offset,
                len,
                owned: SkiaRsJpegBytes::empty(),
            },
            MetadataReadResult::Owned(v) => Self {
                kind: 2,
                marker_index: 0,
                offset: 0,
                len: v.len(),
                owned: SkiaRsJpegBytes::from_vec(v),
            },
        }
    }
}

#[repr(C)]
pub struct SkiaRsJpegGainmapInfo {
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

#[inline]
unsafe fn slice_from_raw<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        slice::from_raw_parts(ptr, len)
    }
}

#[inline]
const fn data_space_from_i32(v: i32) -> DataSpace {
    match v {
        0 => DataSpace::Rgb,
        1 => DataSpace::Cmyk,
        2 => DataSpace::Gray,
        _ => DataSpace::Other,
    }
}

unsafe fn collect_marker_views<'a>(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
) -> Vec<JpegMarkerRef<'a>> {
    if markers.is_null() || count == 0 {
        return Vec::new();
    }
    let views = slice::from_raw_parts(markers, count);
    views
        .iter()
        .map(|v| JpegMarkerRef {
            marker: v.marker,
            data: slice_from_raw(v.ptr, v.len),
        })
        .collect()
}

// ============================================================================
// Segment Scanner & Multi-Picture Format (`jpeg_mpf`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_scanner_on_bytes(
    state: *mut SegmentScannerState,
    data: *const u8,
    size: usize,
    emit_cb: Option<extern "C" fn(*mut c_void, JpegSegment)>,
    ctx: *mut c_void,
) {
    if state.is_null() {
        return;
    }
    let bytes = slice_from_raw(data, size);
    (*state).on_bytes(bytes, &mut |seg| {
        if let Some(cb) = emit_cb {
            cb(ctx, seg);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_scanner_on_byte(
    state: *mut SegmentScannerState,
    byte: u8,
    emit_cb: Option<extern "C" fn(*mut c_void, JpegSegment)>,
    ctx: *mut c_void,
) {
    if state.is_null() {
        return;
    }
    (*state).on_byte(byte, &mut |seg| {
        if let Some(cb) = emit_cb {
            cb(ctx, seg);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_scanner_on_marker_second_byte(
    state: *mut SegmentScannerState,
    byte: u8,
    emit_cb: Option<extern "C" fn(*mut c_void, JpegSegment)>,
    ctx: *mut c_void,
) {
    if state.is_null() {
        return;
    }
    (*state).on_marker_second_byte(byte, &mut |seg| {
        if let Some(cb) = emit_cb {
            cb(ctx, seg);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_segment_param_range(
    scanned_len: usize,
    segment: JpegSegment,
    out_offset: *mut usize,
    out_len: *mut usize,
) -> bool {
    match segment_scan::segment_parameter_range(scanned_len, segment) {
        Some((off, len)) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            if !out_len.is_null() {
                *out_len = len;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_verify_memory_segment_params(
    data: *const u8,
    data_len: usize,
    segment: JpegSegment,
    out_offset: *mut usize,
    out_len: *mut usize,
) -> bool {
    let bytes = slice_from_raw(data, data_len);
    match segment_scan::verify_memory_segment_parameters(bytes, segment) {
        Some((off, len)) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            if !out_len.is_null() {
                *out_len = len;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_mpf_parse(
    data: *const u8,
    len: usize,
    emit_cb: Option<extern "C" fn(*mut c_void, MpImage)>,
    ctx: *mut c_void,
) -> bool {
    let bytes = slice_from_raw(data, len);
    let Some(params) = MultiPictureParameters::parse(bytes) else {
        return false;
    };
    if let Some(cb) = emit_cb {
        for img in params.images {
            cb(ctx, img);
        }
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_mpf_serialize(
    images: *const MpImage,
    image_count: usize,
    individual_image_number: u32,
) -> SkiaRsJpegBytes {
    let imgs = if images.is_null() || image_count == 0 {
        &[]
    } else {
        slice::from_raw_parts(images, image_count)
    };
    let params = MultiPictureParameters {
        images: imgs.to_vec(),
    };
    SkiaRsJpegBytes::from_vec(params.serialize(individual_image_number))
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_mpf_get_image_absolute_offset(
    data_offset: u32,
    mp_segment_offset: usize,
) -> usize {
    multi_picture::get_image_absolute_offset(data_offset, mp_segment_offset)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_mpf_get_image_data_offset(
    image_absolute_offset: usize,
    mp_segment_offset: usize,
) -> u32 {
    multi_picture::get_image_data_offset(image_absolute_offset, mp_segment_offset)
}

// ============================================================================
// XMP & Metadata (`jpeg_decode`)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_find_xmp_standard(
    slices: *const SkiaRsJpegSlice,
    count: usize,
    out_index: *mut usize,
    out_offset: *mut usize,
) -> bool {
    if slices.is_null() || count == 0 {
        return false;
    }
    let raw = slice::from_raw_parts(slices, count);
    let refs: Vec<&[u8]> = raw.iter().map(|s| slice_from_raw(s.ptr, s.len)).collect();
    match xmp::find_xmp_standard(&refs) {
        Some((idx, off)) => {
            if !out_index.is_null() {
                *out_index = idx;
            }
            if !out_offset.is_null() {
                *out_offset = off;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_read_xmp_extended(
    slices: *const SkiaRsJpegSlice,
    count: usize,
    guid: *const u8,
    guid_len: usize,
    out_bytes: *mut SkiaRsJpegBytes,
) -> bool {
    if slices.is_null() || count == 0 || guid.is_null() || out_bytes.is_null() {
        return false;
    }
    let raw = slice::from_raw_parts(slices, count);
    let refs: Vec<&[u8]> = raw.iter().map(|s| slice_from_raw(s.ptr, s.len)).collect();
    let guid_bytes = slice_from_raw(guid, guid_len);
    match xmp::read_xmp_extended(&refs, guid_bytes) {
        Some(v) => {
            *out_bytes = SkiaRsJpegBytes::from_vec(v);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_read_exif(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
    copy_data: bool,
) -> SkiaRsJpegMetadataResult {
    let refs = collect_marker_views(markers, count);
    SkiaRsJpegMetadataResult::from_read_result(metadata::read_exif_metadata(&refs, copy_data))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_read_icc(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
    copy_data: bool,
) -> SkiaRsJpegMetadataResult {
    let refs = collect_marker_views(markers, count);
    SkiaRsJpegMetadataResult::from_read_result(metadata::read_icc_profile_data(&refs, copy_data))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_read_iso_gainmap(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
    copy_data: bool,
) -> SkiaRsJpegMetadataResult {
    let refs = collect_marker_views(markers, count);
    SkiaRsJpegMetadataResult::from_read_result(metadata::read_iso_gainmap_metadata(
        &refs, copy_data,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_read_jumbf(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
    copy_data: bool,
) -> SkiaRsJpegMetadataResult {
    let refs = collect_marker_views(markers, count);
    SkiaRsJpegMetadataResult::from_read_result(metadata::read_jumbf_metadata(&refs, copy_data))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_scan_header_app1_app2(
    data: *const u8,
    len: usize,
    emit_cb: Option<extern "C" fn(*mut c_void, JpegSegment)>,
    ctx: *mut c_void,
) {
    let bytes = slice_from_raw(data, len);
    let segs = metadata::scan_header_app1_app2_segments(bytes);
    if let Some(cb) = emit_cb {
        for seg in segs {
            cb(ctx, seg);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_might_have_gainmap(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
) -> bool {
    let refs = collect_marker_views(markers, count);
    metadata::might_have_gainmap_image(&refs)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_find_mp_in_markers(
    markers: *const SkiaRsJpegMarkerView,
    count: usize,
    emit_cb: Option<extern "C" fn(*mut c_void, MpImage)>,
    ctx: *mut c_void,
    out_skipped_count: *mut usize,
) -> bool {
    let refs = collect_marker_views(markers, count);
    let Some((params, skipped)) = metadata::find_mp_params_in_markers(&refs) else {
        return false;
    };
    if let Some(cb) = emit_cb {
        for img in params.images {
            cb(ctx, img);
        }
    }
    if !out_skipped_count.is_null() {
        *out_skipped_count = skipped;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_find_mp_segment_by_skip(
    segments: *const JpegSegment,
    count: usize,
    skipped_count: usize,
    out_segment: *mut JpegSegment,
) -> bool {
    if segments.is_null() || count == 0 {
        return false;
    }
    let segs = slice::from_raw_parts(segments, count);
    match metadata::find_mp_segment_by_skip_count(segs, skipped_count) {
        Some(seg) => {
            if !out_segment.is_null() {
                *out_segment = seg;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_compute_container_gainmap_offset(
    segments: *const JpegSegment,
    count: usize,
    relative_offset: usize,
) -> usize {
    let segs = if segments.is_null() || count == 0 {
        &[]
    } else {
        slice::from_raw_parts(segments, count)
    };
    metadata::compute_container_gainmap_offset(segs, relative_offset)
}

// ============================================================================
// SourceMgr & Decoder (`jpeg_decode`)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_select_source_mgr(
    decode_gainmaps: bool,
    has_position: bool,
    has_length: bool,
    has_memory_base: bool,
) -> i32 {
    source_mgr::select_source_mgr_kind(
        decode_gainmaps,
        has_position,
        has_length,
        has_memory_base,
    ) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_memory_skip(
    bytes_to_skip: usize,
    bytes_in_buffer: usize,
) -> MemorySkipPlan {
    source_mgr::plan_memory_skip(bytes_to_skip, bytes_in_buffer)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_buffered_skip(
    bytes_to_skip: usize,
    bytes_in_buffer: usize,
) -> BufferedSkipPlan {
    source_mgr::plan_buffered_skip(bytes_to_skip, bytes_in_buffer)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_validate_memory_subset(
    stream_length: usize,
    offset: usize,
    size: usize,
) -> bool {
    source_mgr::validate_memory_subset(stream_length, offset, size)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_unseekable_subset(
    last_read_offset: usize,
    last_read_size: usize,
    req_offset: usize,
    req_size: usize,
) -> UnseekableSubsetPlan {
    source_mgr::plan_unseekable_subset(last_read_offset, last_read_size, req_offset, req_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_is_jpeg(buf: *const u8, len: usize) -> bool {
    decoder::is_jpeg(slice_from_raw(buf, len))
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_should_abort_progress(input_scan_number: i32) -> bool {
    decoder::should_abort_progress(input_scan_number)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_encoded_color(jpeg_color_space: i32) -> EncodedColorResult {
    let jcs = JColorSpace::from_i32(jpeg_color_space).unwrap_or(JColorSpace::Unknown);
    decoder::encoded_color_from_jpeg_color_space(jcs)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_is_icc_compatible(jpeg_color_space: i32, data_space: i32) -> bool {
    let jcs = JColorSpace::from_i32(jpeg_color_space).unwrap_or(JColorSpace::Unknown);
    decoder::is_icc_profile_compatible(jcs, data_space_from_i32(data_space))
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_get_row_bytes(
    out_color_space: i32,
    output_width: u32,
    out_color_components: i32,
) -> usize {
    let jcs = JColorSpace::from_i32(out_color_space).unwrap_or(JColorSpace::Unknown);
    decoder::get_row_bytes(jcs, output_width, out_color_components)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_scale_numerator(desired_scale: f32) -> u32 {
    decoder::scale_numerator_for_desired_scale(desired_scale)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_conversion(
    encoded_jcs: i32,
    dst_ct: i32,
    dst_at: i32,
    needs_color_xform: bool,
) -> ConversionPlan {
    let jcs = JColorSpace::from_i32(encoded_jcs).unwrap_or(JColorSpace::Unknown);
    let ct = ColorType::from_i32(dst_ct).unwrap_or(ColorType::Unknown);
    let at = AlphaType::from_i32(dst_at).unwrap_or(AlphaType::Unknown);
    decoder::plan_conversion(jcs, ct, at, needs_color_xform)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_needs_cmyk_swizzler(
    out_jcs: i32,
    has_profile: bool,
    profile_data_space: i32,
    has_color_xform: bool,
) -> bool {
    let jcs = JColorSpace::from_i32(out_jcs).unwrap_or(JColorSpace::Unknown);
    decoder::needs_swizzler_to_convert_from_cmyk(
        jcs,
        has_profile,
        data_space_from_i32(profile_data_space),
        has_color_xform,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_src_bpp_for_out_cs(out_jcs: i32) -> i32 {
    let jcs = JColorSpace::from_i32(out_jcs).unwrap_or(JColorSpace::Unknown);
    decoder::src_bpp_for_out_color_space(jcs)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_storage(
    dst_width: i32,
    dst_bpp: usize,
    has_swizzler: bool,
    swizzler_width: i32,
    dinfo_row_bytes: usize,
    has_color_xform: bool,
) -> StoragePlan {
    decoder::plan_storage(
        dst_width,
        dst_bpp,
        has_swizzler,
        swizzler_width,
        dinfo_row_bytes,
        has_color_xform,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_subset_after_crop(
    req_x: i32,
    req_w: i32,
    req_h: i32,
    cropped_start_x: u32,
    cropped_width: u32,
) -> SubsetCropPlan {
    decoder::plan_subset_after_crop(req_x, req_w, req_h, cropped_start_x, cropped_width)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_estimate_libjpeg_memory(
    is_progressive: bool,
    width: i32,
    height: i32,
) -> usize {
    decoder::estimate_libjpeg_memory(is_progressive, width, height)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_query_yuva_info(
    jcs: i32,
    h_samp: *const i32,
    v_samp: *const i32,
    width_in_blocks: *const u32,
    check_supported: bool,
    supports_yuv_unorm8: bool,
) -> YuvaQueryPlan {
    if h_samp.is_null() || v_samp.is_null() || width_in_blocks.is_null() {
        return YuvaQueryPlan {
            supported: false,
            subsampling: Subsampling::Unknown as i32,
            row_bytes: [0; 4],
        };
    }
    let hs = slice::from_raw_parts(h_samp, 3);
    let vs = slice::from_raw_parts(v_samp, 3);
    let wb = slice::from_raw_parts(width_in_blocks, 3);
    let cs = JColorSpace::from_i32(jcs).unwrap_or(JColorSpace::Unknown);
    decoder::query_yuva_info(
        cs,
        [hs[0], hs[1], hs[2]],
        [vs[0], vs[1], vs[2]],
        [wb[0], wb[1], wb[2]],
        check_supported,
        supports_yuv_unorm8,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_yuva_planes(
    output_height: u32,
    y_v_samp: i32,
    u_downsampled_height: u32,
) -> YuvaPlanesPlan {
    decoder::plan_yuva_planes(output_height, y_v_samp, u_downsampled_height)
}

// ============================================================================
// Encoder & Gainmap Encoder (`jpeg_encode`)
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_rgb_encode(
    src_width: i32,
    src_ct: i32,
    src_at: i32,
    alpha_option: i32,
    downsample: i32,
) -> RgbEncodePlan {
    let ct = ColorType::from_i32(src_ct).unwrap_or(ColorType::Unknown);
    let at = AlphaType::from_i32(src_at).unwrap_or(AlphaType::Unknown);
    let ao = AlphaOption::from_i32(alpha_option).unwrap_or(AlphaOption::Ignore);
    let ds = Downsample::from_i32(downsample).unwrap_or(Downsample::S420);
    encoder::plan_rgb_encode(src_width, ct, at, ao, ds)
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_plan_yuv_encode(
    width: i32,
    yuv_color_space: i32,
    data_type: i32,
    plane_config: i32,
    subsampling: i32,
) -> YuvEncodePlan {
    let pc = PlaneConfig::from_i32(plane_config).unwrap_or(PlaneConfig::Unknown);
    let ss = Subsampling::from_i32(subsampling).unwrap_or(Subsampling::Unknown);
    encoder::plan_yuv_encode(width, yuv_color_space, data_type, pc, ss)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_yuva_copy_row_planar(
    y_row: *const u8,
    y_len: usize,
    u_row: *const u8,
    u_len: usize,
    v_row: *const u8,
    v_len: usize,
    ss_width_u: usize,
    ss_width_v: usize,
    dst: *mut u8,
    dst_len: usize,
) {
    if dst.is_null() || dst_len == 0 {
        return;
    }
    let y = slice_from_raw(y_row, y_len);
    let u = slice_from_raw(u_row, u_len);
    let v = slice_from_raw(v_row, v_len);
    let dst_slice = slice::from_raw_parts_mut(dst, dst_len);
    encoder::yuva_copy_row_planar(y, u, v, ss_width_u, ss_width_v, dst_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_yuva_copy_row_semi_planar(
    y_row: *const u8,
    y_len: usize,
    uv_row: *const u8,
    uv_len: usize,
    ss_width_uv: usize,
    dst: *mut u8,
    dst_len: usize,
) {
    if dst.is_null() || dst_len == 0 {
        return;
    }
    let y = slice_from_raw(y_row, y_len);
    let uv = slice_from_raw(uv_row, uv_len);
    let dst_slice = slice::from_raw_parts_mut(dst, dst_len);
    encoder::yuva_copy_row_semi_planar(y, uv, ss_width_uv, dst_slice);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_build_icc_segment_params(
    icc: *const u8,
    len: usize,
) -> SkiaRsJpegBytes {
    SkiaRsJpegBytes::from_vec(encoder::build_icc_segment_params(slice_from_raw(icc, len)))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_build_xmp_standard_segment_params(
    xmp_bytes: *const u8,
    len: usize,
) -> SkiaRsJpegBytes {
    SkiaRsJpegBytes::from_vec(encoder::build_xmp_standard_segment_params(slice_from_raw(
        xmp_bytes, len,
    )))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_build_origin_exif_segment_params(
    origin: u32,
    out_bytes: *mut SkiaRsJpegBytes,
) -> bool {
    if out_bytes.is_null() {
        return false;
    }
    match encoder::build_origin_exif_segment_params(origin) {
        Some(v) => {
            *out_bytes = SkiaRsJpegBytes::from_vec(v);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_build_iso_gainmap_segment_params(
    data: *const u8,
    len: usize,
) -> SkiaRsJpegBytes {
    SkiaRsJpegBytes::from_vec(encoder::build_iso_gainmap_segment_params(slice_from_raw(
        data, len,
    )))
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_build_minimal_exif_params() -> SkiaRsJpegBytes {
    SkiaRsJpegBytes::from_vec(encoder::build_minimal_exif_params())
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_build_gainmap_xmp(
    info: *const SkiaRsJpegGainmapInfo,
) -> SkiaRsJpegBytes {
    if info.is_null() {
        return SkiaRsJpegBytes::empty();
    }
    let raw = &*info;
    let gm = GainmapInfo {
        gainmap_ratio_min: raw.gainmap_ratio_min,
        gainmap_ratio_max: raw.gainmap_ratio_max,
        gainmap_gamma: raw.gainmap_gamma,
        epsilon_sdr: raw.epsilon_sdr,
        epsilon_hdr: raw.epsilon_hdr,
        display_ratio_sdr: raw.display_ratio_sdr,
        display_ratio_hdr: raw.display_ratio_hdr,
        base_image_type: if raw.base_image_type == 1 {
            BaseImageType::Hdr
        } else {
            BaseImageType::Sdr
        },
        gainmap_type: if raw.gainmap_type == 1 {
            GainmapType::Apple
        } else {
            GainmapType::Default
        },
        has_gainmap_math_color_space: raw.has_gainmap_math_color_space,
    };
    SkiaRsJpegBytes::from_vec(encoder::build_gainmap_image_xmp_metadata(&gm))
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_build_base_xmp(gainmap_item_length: usize) -> SkiaRsJpegBytes {
    SkiaRsJpegBytes::from_vec(encoder::build_base_image_xmp_metadata(gainmap_item_length))
}

#[no_mangle]
pub extern "C" fn skia_rs_jpeg_mpf_segment_size(image_count: usize, image_number: u32) -> usize {
    encoder::mpf_segment_size(image_count, image_number)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_jpeg_assemble_mpf(
    images: *const SkiaRsJpegSlice,
    count: usize,
    out_bytes: *mut SkiaRsJpegBytes,
) -> bool {
    if out_bytes.is_null() {
        return false;
    }
    let refs: Vec<&[u8]> = if images.is_null() || count == 0 {
        Vec::new()
    } else {
        slice::from_raw_parts(images, count)
            .iter()
            .map(|s| slice_from_raw(s.ptr, s.len))
            .collect()
    };
    match encoder::assemble_mpf(&refs) {
        Some(v) => {
            *out_bytes = SkiaRsJpegBytes::from_vec(v);
            true
        }
        None => false,
    }
}
