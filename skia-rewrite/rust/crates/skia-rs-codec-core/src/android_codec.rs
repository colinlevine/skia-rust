//! Safe Rust port of `src/codec/SkAndroidCodec.cpp`, `src/codec/SkAndroidCodecAdapter.cpp`,
//! and `src/codec/SkSampledCodec.cpp` algorithms and math.

use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::{IRect, ISize};

use crate::sampler::{
    get_sampled_dimension, get_scale_from_sample_size, get_start_coord, is_valid_subset,
};
use crate::types::{irect_make_size, irect_make_xywh, CodecResult, EncodedImageFormat};

/// Which concrete subclass `SkAndroidCodec::MakeFromCodec` instantiates.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AndroidCodecKind {
    Unsupported = 0,
    Sampled = 1,
    Adapter = 2,
}

/// Exact port of `SkAndroidCodec::MakeFromCodec` format dispatch.
#[inline]
pub const fn select_android_codec_kind(format: EncodedImageFormat) -> AndroidCodecKind {
    match format {
        EncodedImageFormat::Png
        | EncodedImageFormat::Ico
        | EncodedImageFormat::Jpeg
        | EncodedImageFormat::Bmp
        | EncodedImageFormat::Wbmp => AndroidCodecKind::Sampled,
        EncodedImageFormat::Gif
        | EncodedImageFormat::Webp
        | EncodedImageFormat::Dng
        | EncodedImageFormat::Avif
        | EncodedImageFormat::Heif
        | EncodedImageFormat::Jpegxl => AndroidCodecKind::Adapter,
        EncodedImageFormat::Pkm | EncodedImageFormat::Ktx | EncodedImageFormat::Astc => {
            AndroidCodecKind::Unsupported
        }
    }
}

/// Exact port of `SkAndroidCodec::computeOutputColorType`.
pub const fn compute_output_color_type(
    requested_ct: ColorType,
    info_ct: ColorType,
    info_at: AlphaType,
    bits_per_component: u8,
    color_depth: u8,
    pmcolor_is_bgra: bool,
) -> ColorType {
    let high_precision = bits_per_component > 8;
    let n32 = if pmcolor_is_bgra {
        ColorType::Bgra8888
    } else {
        ColorType::Rgba8888
    };

    match requested_ct {
        ColorType::Argb4444 => return n32,
        ColorType::Rgba8888 if !pmcolor_is_bgra => {}
        ColorType::Bgra8888 if pmcolor_is_bgra => {}
        ColorType::Alpha8 | ColorType::Gray8 => {
            if matches!(info_ct, ColorType::Gray8) {
                return ColorType::Gray8;
            }
        }
        ColorType::Rgb565 => {
            if matches!(info_at, AlphaType::Opaque) {
                return ColorType::Rgb565;
            }
        }
        ColorType::Rgba1010102 => {
            if color_depth == 10 && matches!(info_at, AlphaType::Opaque) {
                return ColorType::Rgba1010102;
            }
        }
        ColorType::RgbaF16 => return ColorType::RgbaF16,
        _ => {}
    }

    if color_depth == 10 && matches!(info_at, AlphaType::Opaque) {
        return ColorType::Rgba1010102;
    }

    if high_precision {
        ColorType::RgbaF16
    } else {
        n32
    }
}

/// Exact port of `SkAndroidCodec::computeOutputAlphaType`.
#[inline]
pub const fn compute_output_alpha_type(info_at: AlphaType, requested_unpremul: bool) -> AlphaType {
    if matches!(info_at, AlphaType::Opaque) {
        return AlphaType::Opaque;
    }
    if requested_unpremul {
        AlphaType::Unpremul
    } else {
        AlphaType::Premul
    }
}

/// Returns true if `SkAndroidCodec::computeOutputColorSpace` supports color correction for `output_ct`.
#[inline]
pub const fn supports_output_color_space(output_ct: ColorType) -> bool {
    matches!(
        output_ct,
        ColorType::RgbaF16
            | ColorType::Rgb565
            | ColorType::Rgba8888
            | ColorType::Bgra8888
            | ColorType::Rgba1010102
    )
}

#[inline]
pub const fn is_valid_sample_size(sample_size: i32) -> bool {
    sample_size > 0
}

#[inline]
const fn smaller_than(a: ISize, b: ISize) -> bool {
    a.width < b.width || a.height < b.height
}

#[inline]
const fn strictly_bigger_than(a: ISize, b: ISize) -> bool {
    a.width > b.width && a.height > b.height
}

/// Exact port of `SkAndroidCodec::computeSampleSize`.
pub fn compute_sample_size<FAnimated, FSampled>(
    orig_dims: ISize,
    desired_size: &mut ISize,
    is_webp: bool,
    is_animated_webp: FAnimated,
    get_sampled_dimensions: FSampled,
) -> i32
where
    FAnimated: FnOnce() -> bool,
    FSampled: Fn(i32) -> ISize,
{
    if *desired_size == orig_dims {
        return 1;
    }

    if smaller_than(orig_dims, *desired_size) {
        *desired_size = orig_dims;
        return 1;
    }

    // Handle bad input:
    if desired_size.width < 1 || desired_size.height < 1 {
        *desired_size = ISize {
            width: desired_size.width.max(1),
            height: desired_size.height.max(1),
        };
    }

    if is_webp {
        if is_animated_webp() {
            // Cannot downscale animated webp
            *desired_size = orig_dims;
        }
        return 1;
    }

    let sample_x = orig_dims.width / desired_size.width;
    let sample_y = orig_dims.height / desired_size.height;
    let mut sample_size = sample_x.min(sample_y);
    let mut computed_size = get_sampled_dimensions(sample_size);
    if computed_size == *desired_size {
        return sample_size;
    }

    if computed_size == orig_dims || sample_size == 1 {
        // Cannot downscale
        *desired_size = computed_size;
        return 1;
    }

    if strictly_bigger_than(computed_size, *desired_size) {
        // See if there is a tighter fit.
        loop {
            let smaller = get_sampled_dimensions(sample_size + 1);
            if smaller == *desired_size {
                return sample_size + 1;
            }
            if smaller == computed_size || smaller_than(smaller, *desired_size) {
                // Cannot get any smaller without being smaller than desired.
                *desired_size = computed_size;
                return sample_size;
            }

            sample_size += 1;
            computed_size = smaller;
        }
    }

    if !smaller_than(computed_size, *desired_size) {
        // One of the computed dimensions is equal to desired, and the other is bigger.
        *desired_size = computed_size;
        return sample_size;
    }

    // computed_size is too small. Make it larger.
    while sample_size > 2 {
        let bigger = get_sampled_dimensions(sample_size - 1);
        if bigger == *desired_size || !smaller_than(bigger, *desired_size) {
            *desired_size = bigger;
            return sample_size - 1;
        }
        sample_size -= 1;
    }

    *desired_size = orig_dims;
    1
}

/// Validation result for `SkAndroidCodec::getAndroidPixels`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AndroidPixelsValidation {
    pub result: i32,
    /// `true` if `options->fSubset` covers the whole image and should be cleared to `nullptr`.
    pub strip_full_subset: bool,
}

/// Exact port of `SkAndroidCodec::getAndroidPixels` input & subset validation.
pub fn validate_android_pixels(
    has_pixels: bool,
    row_bytes: usize,
    min_row_bytes: usize,
    codec_dims: ISize,
    subset: Option<IRect>,
) -> AndroidPixelsValidation {
    if !has_pixels || row_bytes < min_row_bytes {
        return AndroidPixelsValidation {
            result: CodecResult::InvalidParameters as i32,
            strip_full_subset: false,
        };
    }

    if let Some(sub) = subset {
        if !is_valid_subset(sub, codec_dims) {
            return AndroidPixelsValidation {
                result: CodecResult::InvalidParameters as i32,
                strip_full_subset: false,
            };
        }
        if irect_make_size(codec_dims) == sub {
            return AndroidPixelsValidation {
                result: CodecResult::Success as i32,
                strip_full_subset: true,
            };
        }
    }

    AndroidPixelsValidation {
        result: CodecResult::Success as i32,
        strip_full_subset: false,
    }
}

/// Plan returned by `plan_native_scaling` matching `SkSampledCodec::accountForNativeScaling`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NativeScalingPlan {
    /// Whether `codec()->getScaledDimensions(scale)` should be called.
    pub use_native_scale: bool,
    /// Scale factor to pass to `getScaledDimensions` when `use_native_scale` is true.
    pub scale: f32,
    /// Remaining sample size after native scaling.
    pub remaining_sample_size: i32,
    /// Value to write to `*nativeSampleSize` (if non-null).
    pub native_sample_size: i32,
}

/// Exact port of `SkSampledCodec::accountForNativeScaling`.
pub fn plan_native_scaling(format: EncodedImageFormat, sample_size: i32) -> NativeScalingPlan {
    let mut plan = NativeScalingPlan {
        use_native_scale: false,
        scale: 1.0,
        remaining_sample_size: sample_size,
        native_sample_size: 1,
    };

    if format == EncodedImageFormat::Jpeg {
        match sample_size {
            2 | 4 | 8 => {
                plan.use_native_scale = true;
                plan.scale = get_scale_from_sample_size(sample_size);
                plan.remaining_sample_size = 1;
                // Note: Matches SkSampledCodec.cpp lines 29-43 where *nativeSampleSize stays 1.
                return plan;
            }
            _ => {}
        }

        for supported in [8, 4, 2] {
            let actual = sample_size / supported;
            let remainder = sample_size % supported;
            if remainder == 0 {
                plan.use_native_scale = true;
                plan.scale = get_scale_from_sample_size(supported);
                plan.remaining_sample_size = actual;
                plan.native_sample_size = supported;
                break;
            }
        }
    }

    plan
}

/// Subset rectangles computed by `SkSampledCodec::onGetAndroidPixels` when native codec supports `scaledSize`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ScaledSubsetPlan {
    pub scaled_subset_x: i32,
    pub scaled_subset_y: i32,
    pub incremental_subset: IRect,
    pub scanline_subset: IRect,
}

/// Exact port of `SkSampledCodec::onGetAndroidPixels` scaled subset bounds math.
pub fn compute_scaled_subset(
    subset: IRect,
    sample_size: i32,
    dst_dims: ISize,
    scaled_size: ISize,
) -> ScaledSubsetPlan {
    let scaled_subset_x = subset.left / sample_size;
    let scaled_subset_y = subset.top / sample_size;
    let incremental_subset =
        irect_make_xywh(scaled_subset_x, scaled_subset_y, dst_dims.width, dst_dims.height);
    let scanline_subset =
        irect_make_xywh(scaled_subset_x, 0, dst_dims.width, scaled_size.height);
    ScaledSubsetPlan {
        scaled_subset_x,
        scaled_subset_y,
        incremental_subset,
        scanline_subset,
    }
}

/// Coordinate & subset plan computed by `SkSampledCodec::sampledDecode`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SampledDecodeParams {
    pub subset_y: i32,
    pub subset_width: i32,
    pub subset_height: i32,
    pub scanline_subset: IRect,
    pub incremental_subset: IRect,
    pub sample_x: i32,
    pub sample_y: i32,
    pub start_y: i32,
}

/// Exact port of `SkSampledCodec::sampledDecode` subset & sampling coordinate computation.
pub fn compute_sampled_decode_params(
    native_size: ISize,
    dst_dims: ISize,
    native_sample_size: i32,
    subset: Option<IRect>,
) -> SampledDecodeParams {
    let (subset_x, subset_y, subset_width, subset_height) = if let Some(sub) = subset {
        let sx = sub.left / native_sample_size;
        let sy = sub.top / native_sample_size;
        let sw = get_sampled_dimension(sub.width(), native_sample_size);
        let sh = get_sampled_dimension(sub.height(), native_sample_size);
        (sx, sy, sw, sh)
    } else {
        (0, 0, native_size.width, native_size.height)
    };

    let scanline_subset = irect_make_xywh(subset_x, 0, subset_width, native_size.height);
    let incremental_subset = IRect::make_ltrb(
        subset_x,
        subset_y,
        subset_x.saturating_add(subset_width),
        subset_y.saturating_add(subset_height),
    );

    let sample_x = if dst_dims.width > 0 {
        subset_width / dst_dims.width
    } else {
        0
    };
    let sample_y = if dst_dims.height > 0 {
        subset_height / dst_dims.height
    } else {
        0
    };
    let sampling_offset_y = get_start_coord(sample_y);
    let start_y = sampling_offset_y + subset_y;

    SampledDecodeParams {
        subset_y,
        subset_width,
        subset_height,
        scanline_subset,
        incremental_subset,
        sample_x,
        sample_y,
        start_y,
    }
}
