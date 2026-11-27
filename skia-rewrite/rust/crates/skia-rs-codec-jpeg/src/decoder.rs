//! Safe Rust port of Skia's JPEG decoder policy, scaling, color conversion,
//! storage planning, subset crop alignment, and YUVA plane calculation logic
//! (`src/codec/SkJpegCodec.cpp` and `src/codec/SkJpegDecoderMgr.cpp`).

use skia_rs_codec_core::types::EncodedColor;
use skia_rs_codec_meta::color_profile::DataSpace;
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_color::yuva_info::Subsampling;

use crate::constants::{DCTSIZE, JColorSpace, JPEG_SIG, MAX_PROGRESSIVE_SCANS};

/// Exact port of `SkJpegCodec::IsJpeg`.
#[inline]
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= JPEG_SIG.len() && bytes[..JPEG_SIG.len()] == JPEG_SIG
}

/// Exact port of `progress_monitor` abort condition in `SkJpegDecoderMgr.cpp`.
#[inline]
pub const fn should_abort_progress(input_scan_number: i32) -> bool {
    input_scan_number >= MAX_PROGRESSIVE_SCANS
}

/// Result of `JpegDecoderMgr::getEncodedColor`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EncodedColorResult {
    pub valid: bool,
    pub encoded_color: i32,
    pub android_rgb_safety_log: bool,
}

/// Exact port of `JpegDecoderMgr::getEncodedColor`.
pub const fn encoded_color_from_jpeg_color_space(jcs: JColorSpace) -> EncodedColorResult {
    match jcs {
        JColorSpace::Grayscale => EncodedColorResult {
            valid: true,
            encoded_color: EncodedColor::Gray as i32,
            android_rgb_safety_log: false,
        },
        JColorSpace::YCbCr => EncodedColorResult {
            valid: true,
            encoded_color: EncodedColor::Yuv as i32,
            android_rgb_safety_log: false,
        },
        JColorSpace::Rgb => EncodedColorResult {
            valid: true,
            encoded_color: EncodedColor::Rgb as i32,
            android_rgb_safety_log: true,
        },
        JColorSpace::Ycck => EncodedColorResult {
            valid: true,
            encoded_color: EncodedColor::Ycck as i32,
            android_rgb_safety_log: false,
        },
        JColorSpace::Cmyk => EncodedColorResult {
            valid: true,
            encoded_color: EncodedColor::InvertedCmyk as i32,
            android_rgb_safety_log: false,
        },
        _ => EncodedColorResult {
            valid: false,
            encoded_color: 0,
            android_rgb_safety_log: false,
        },
    }
}

/// Exact port of the ICC profile `DataSpace` compatibility switch in `SkJpegCodec::MakeFromStream`.
pub const fn is_icc_profile_compatible(jcs: JColorSpace, data_space: DataSpace) -> bool {
    match jcs {
        JColorSpace::Cmyk | JColorSpace::Ycck => matches!(data_space, DataSpace::Cmyk),
        JColorSpace::Grayscale => matches!(data_space, DataSpace::Gray | DataSpace::Rgb),
        _ => matches!(data_space, DataSpace::Rgb),
    }
}

/// Exact port of `get_row_bytes` in `SkJpegCodec.cpp`.
#[inline]
pub const fn get_row_bytes(
    out_color_space: JColorSpace,
    output_width: u32,
    out_color_components: i32,
) -> usize {
    let color_bytes = if matches!(out_color_space, JColorSpace::Rgb565) {
        2usize
    } else if out_color_components > 0 {
        out_color_components as usize
    } else {
        0usize
    };
    (output_width as usize).wrapping_mul(color_bytes)
}

/// Exact port of scale numerator selection in `SkJpegCodec::onGetScaledDimensions`.
/// Returns `num` in `1..=8` (denominator is always 8).
#[inline]
pub fn scale_numerator_for_desired_scale(desired_scale: f32) -> u32 {
    if desired_scale >= 0.9375_f32 {
        8
    } else if desired_scale >= 0.8125_f32 {
        7
    } else if desired_scale >= 0.6875_f32 {
        6
    } else if desired_scale >= 0.5625_f32 {
        5
    } else if desired_scale >= 0.4375_f32 {
        4
    } else if desired_scale >= 0.3125_f32 {
        3
    } else if desired_scale >= 0.1875_f32 {
        2
    } else {
        1
    }
}

/// Matches libjpeg-turbo's `jdiv_round_up(dim * num, 8)` output dimension calculation
/// for scale factors `num / 8` where `num in 1..=8`.
#[inline]
pub const fn scaled_dimension_eighths(dim: u32, num: u32) -> u32 {
    if num >= 8 {
        return dim;
    }
    (((dim as u64) * (num as u64) + 7) / 8) as u32
}

/// Find the scale numerator `num` in `1..=8` (with denom 8) matching `(dst_w, dst_h)`,
/// matching the descending search loop in `SkJpegCodec::onDimensionsSupported`.
pub const fn find_supported_scale_numerator(
    image_w: u32,
    image_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Option<u32> {
    let mut num: u32 = 8;
    loop {
        let out_w = scaled_dimension_eighths(image_w, num);
        let out_h = scaled_dimension_eighths(image_h, num);
        if out_w == dst_w && out_h == dst_h {
            return Some(num);
        }
        if num == 1 || dst_w > out_w || dst_h > out_h {
            return None;
        }
        num -= 1;
    }
}

/// Plan returned for `SkJpegCodec::conversionSupported`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConversionPlan {
    pub supported: bool,
    pub warn_non_opaque: bool,
    pub out_color_space: i32,
    pub set_dither_none: bool,
}

/// Exact port of `SkJpegCodec::conversionSupported`.
pub const fn plan_conversion(
    encoded_jcs: JColorSpace,
    dst_ct: ColorType,
    dst_at: AlphaType,
    needs_color_xform: bool,
) -> ConversionPlan {
    if matches!(dst_at, AlphaType::Unknown) {
        return ConversionPlan {
            supported: false,
            warn_non_opaque: false,
            out_color_space: JColorSpace::Unknown as i32,
            set_dither_none: false,
        };
    }

    let warn_non_opaque = !matches!(dst_at, AlphaType::Opaque);
    let mut set_dither_none = false;

    let mut out_cs = match dst_ct {
        ColorType::Rgba8888 => JColorSpace::ExtRgba,
        ColorType::Bgra8888 => {
            if needs_color_xform {
                JColorSpace::ExtRgba
            } else {
                JColorSpace::ExtBgra
            }
        }
        ColorType::Rgb565 => {
            if needs_color_xform {
                JColorSpace::ExtRgba
            } else {
                set_dither_none = true;
                JColorSpace::Rgb565
            }
        }
        ColorType::Gray8 => {
            if !matches!(encoded_jcs, JColorSpace::Grayscale) {
                return ConversionPlan {
                    supported: false,
                    warn_non_opaque,
                    out_color_space: JColorSpace::Unknown as i32,
                    set_dither_none: false,
                };
            }
            if needs_color_xform {
                JColorSpace::ExtRgba
            } else {
                JColorSpace::Grayscale
            }
        }
        ColorType::Bgr101010xXr | ColorType::RgbaF16 => JColorSpace::ExtRgba,
        _ => {
            return ConversionPlan {
                supported: false,
                warn_non_opaque,
                out_color_space: JColorSpace::Unknown as i32,
                set_dither_none: false,
            };
        }
    };

    // libjpeg-turbo does not convert CMYK/YCCK directly to RGBA.
    if matches!(encoded_jcs, JColorSpace::Cmyk | JColorSpace::Ycck) {
        out_cs = JColorSpace::Cmyk;
    }

    ConversionPlan {
        supported: true,
        warn_non_opaque,
        out_color_space: out_cs as i32,
        set_dither_none,
    }
}

/// Exact port of `needs_swizzler_to_convert_from_cmyk` in `SkJpegCodec.cpp`.
#[inline]
pub const fn needs_swizzler_to_convert_from_cmyk(
    out_jcs: JColorSpace,
    has_src_profile: bool,
    src_profile_data_space: DataSpace,
    has_color_space_xform: bool,
) -> bool {
    if !matches!(out_jcs, JColorSpace::Cmyk) {
        return false;
    }
    let has_cmyk_cs = has_src_profile && matches!(src_profile_data_space, DataSpace::Cmyk);
    !has_cmyk_cs || !has_color_space_xform
}

/// Source bytes-per-pixel for `SkSwizzler::MakeSimple` in `SkJpegCodec::initializeSwizzler`.
#[inline]
pub const fn src_bpp_for_out_color_space(out_jcs: JColorSpace) -> i32 {
    match out_jcs {
        JColorSpace::ExtRgba | JColorSpace::ExtBgra | JColorSpace::Cmyk => 4,
        JColorSpace::Rgb565 => 2,
        JColorSpace::Grayscale => 1,
        _ => 0,
    }
}

/// Plan returned for `SkJpegCodec::allocateStorage`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StoragePlan {
    pub swizzle_bytes: usize,
    pub xform_bytes: usize,
    pub total_bytes: usize,
}

/// Exact port of `SkJpegCodec::allocateStorage` buffer size math.
#[inline]
pub const fn plan_storage(
    dst_width: i32,
    dst_bpp: usize,
    has_swizzler: bool,
    swizzler_width: i32,
    dinfo_row_bytes: usize,
    has_color_xform: bool,
) -> StoragePlan {
    let mut w = dst_width;
    let swizzle_bytes = if has_swizzler {
        w = swizzler_width;
        dinfo_row_bytes
    } else {
        0
    };
    let xform_bytes = if has_color_xform && dst_bpp != 4 && w > 0 {
        (w as usize).wrapping_mul(4)
    } else {
        0
    };
    StoragePlan {
        swizzle_bytes,
        xform_bytes,
        total_bytes: swizzle_bytes.wrapping_add(xform_bytes),
    }
}

/// Plan returned after `jpeg_crop_scanline` in `SkJpegCodec::onStartScanlineDecode`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SubsetCropPlan {
    pub swizzler_subset_x: i32,
    pub swizzler_subset_y: i32,
    pub swizzler_subset_w: i32,
    pub swizzler_subset_h: i32,
    pub needs_swizzler_for_subset: bool,
}

/// Exact port of lines 733-741 in `SkJpegCodec.cpp`.
#[inline]
pub const fn plan_subset_after_crop(
    req_x: i32,
    req_w: i32,
    req_h: i32,
    cropped_start_x: u32,
    cropped_width: u32,
) -> SubsetCropPlan {
    SubsetCropPlan {
        swizzler_subset_x: req_x.wrapping_sub(cropped_start_x as i32),
        swizzler_subset_y: 0,
        swizzler_subset_w: req_w,
        swizzler_subset_h: req_h,
        needs_swizzler_for_subset: cropped_start_x != (req_x as u32)
            || cropped_width != (req_w as u32),
    }
}

/// Exact port of `estimatedLibJpegMemory` calculations in `SkJpegCodec::onGetPixels`.
#[inline]
pub const fn estimate_libjpeg_memory(is_progressive: bool, width: i32, height: i32) -> usize {
    if width <= 0 || height <= 0 {
        return 0;
    }
    if is_progressive {
        (6usize)
            .wrapping_mul(width as usize)
            .wrapping_mul(height as usize)
    } else {
        (34usize).wrapping_mul(width as usize)
    }
}

/// Plan returned by `query_yuva_info` (port of `is_yuv_supported` in `SkJpegCodec.cpp`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct YuvaQueryPlan {
    pub supported: bool,
    pub subsampling: i32,
    pub row_bytes: [usize; 4],
}

/// Exact port of `is_yuv_supported` in `SkJpegCodec.cpp`.
pub const fn query_yuva_info(
    jcs: JColorSpace,
    h_samp: [i32; 3],
    v_samp: [i32; 3],
    width_in_blocks: [u32; 3],
    check_supported_data_types: bool,
    supports_yuv_unorm8: bool,
) -> YuvaQueryPlan {
    let unsupported = YuvaQueryPlan {
        supported: false,
        subsampling: Subsampling::Unknown as i32,
        row_bytes: [0; 4],
    };

    if !matches!(jcs, JColorSpace::YCbCr) {
        return unsupported;
    }

    if h_samp[1] != 1 || v_samp[1] != 1 || h_samp[2] != 1 || v_samp[2] != 1 {
        return unsupported;
    }

    let subsampling = match (h_samp[0], v_samp[0]) {
        (1, 1) => Subsampling::S444,
        (2, 1) => Subsampling::S422,
        (2, 2) => Subsampling::S420,
        (1, 2) => Subsampling::S440,
        (4, 1) => Subsampling::S411,
        (4, 2) => Subsampling::S410,
        _ => return unsupported,
    };

    if check_supported_data_types && !supports_yuv_unorm8 {
        return unsupported;
    }

    YuvaQueryPlan {
        supported: true,
        subsampling: subsampling as i32,
        row_bytes: [
            (width_in_blocks[0] as usize) * DCTSIZE,
            (width_in_blocks[1] as usize) * DCTSIZE,
            (width_in_blocks[2] as usize) * DCTSIZE,
            0,
        ],
    }
}

/// Loop geometry for `SkJpegCodec::onGetYUVAPlanes`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct YuvaPlanesPlan {
    pub num_y_rows_per_block: i32,
    pub num_iters: i32,
    pub remaining_rows: u32,
    pub remaining_uv_rows: i32,
}

/// Exact port of block iteration math in `SkJpegCodec::onGetYUVAPlanes`.
#[inline]
pub const fn plan_yuva_planes(
    output_height: u32,
    y_v_samp_factor: i32,
    u_downsampled_height: u32,
) -> YuvaPlanesPlan {
    let num_y_rows_per_block = (DCTSIZE as i32) * y_v_samp_factor;
    if num_y_rows_per_block <= 0 {
        return YuvaPlanesPlan {
            num_y_rows_per_block: 0,
            num_iters: 0,
            remaining_rows: 0,
            remaining_uv_rows: 0,
        };
    }
    let rows_per_block_u32 = num_y_rows_per_block as u32;
    let num_iters = (output_height / rows_per_block_u32) as i32;
    let remaining_rows = output_height % rows_per_block_u32;
    let remaining_uv_rows = (u_downsampled_height as i32) - (DCTSIZE as i32) * num_iters;
    YuvaPlanesPlan {
        num_y_rows_per_block,
        num_iters,
        remaining_rows,
        remaining_uv_rows,
    }
}
