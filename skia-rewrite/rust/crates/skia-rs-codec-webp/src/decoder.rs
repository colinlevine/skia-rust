//! Safe Rust WebP decoder planning: demux/feature classification, colorspace mode
//! selection, subset clamping, scaling, crop/scale rect computation, and incremental
//! decode status interpretation matching `src/codec/SkWebpCodec.cpp`.

use skia_rs_codec_core::types::{
    irect_contains, irect_intersect, irect_make_size, irect_size, CodecResult, EncodedAlpha,
    EncodedColor,
};
use skia_rs_codec_meta::color_profile::DataSpace;
use skia_rs_color::image_info::{color_type_bytes_per_pixel, AlphaType, ColorType};
use skia_rs_geometry::{IRect, ISize};

use crate::animation::{plan_blend_pipeline, WebpBlendPlan};
use crate::types::{
    Vp8StatusCode, WebpCspMode, WebpDemuxState, WebpMuxAnimBlend, MAX_CANVAS_PIXELS,
};

/// Maps `WebPDemuxState` to an early-return `CodecResult` if demuxing failed or
/// did not finish parsing the header. Returns `None` on `ParsedHeader` / `Done`.
#[inline]
pub const fn map_demux_state(state: WebpDemuxState) -> Option<CodecResult> {
    match state {
        WebpDemuxState::ParseError => Some(CodecResult::InvalidInput),
        WebpDemuxState::ParsingHeader => Some(CodecResult::IncompleteInput),
        WebpDemuxState::ParsedHeader | WebpDemuxState::Done => None,
    }
}

/// Validates canvas dimensions to prevent 4-bytes-per-pixel `int32_t` overflow,
/// matching `SkWebpCodec::MakeFromStream`.
#[inline]
pub const fn validate_canvas_dimensions(width: i32, height: i32) -> bool {
    let size = (width as i64).wrapping_mul(height as i64);
    size >= (i32::MIN as i64) && size <= MAX_CANVAS_PIXELS
}

/// Returns true if an embedded ICCP color profile should be kept (RGB only).
#[inline]
pub const fn should_keep_color_profile(data_space: DataSpace) -> bool {
    matches!(data_space, DataSpace::Rgb)
}

/// Maps `WebPGetFeatures` return status to `SkCodec::Result`.
#[inline]
pub const fn map_features_status(status: Vp8StatusCode) -> CodecResult {
    match status {
        Vp8StatusCode::Ok => CodecResult::Success,
        Vp8StatusCode::Suspended | Vp8StatusCode::NotEnoughData => CodecResult::IncompleteInput,
        Vp8StatusCode::OutOfMemory
        | Vp8StatusCode::InvalidParam
        | Vp8StatusCode::BitstreamError
        | Vp8StatusCode::UnsupportedFeature
        | Vp8StatusCode::UserAbort => CodecResult::InvalidInput,
    }
}

/// Classifies encoded color and alpha from canvas size, first frame geometry/alpha,
/// and `WebPBitstreamFeatures::format` (`0 = mixed`, `1 = lossy`, `2 = lossless`).
#[inline]
pub const fn classify_encoded_info(
    canvas_width: i32,
    canvas_height: i32,
    frame_width: i32,
    frame_height: i32,
    frame_has_alpha: bool,
    bitstream_format: i32,
) -> Option<(EncodedColor, EncodedAlpha)> {
    let has_alpha =
        frame_has_alpha || frame_width != canvas_width || frame_height != canvas_height;
    match bitstream_format {
        // 0 = mixed (animated webps), 2 = lossless (BGRA)
        0 | 2 => {
            if has_alpha {
                Some((EncodedColor::Bgra, EncodedAlpha::Unpremul))
            } else {
                Some((EncodedColor::Bgrx, EncodedAlpha::Opaque))
            }
        }
        // 1 = lossy (YUV)
        1 => {
            if has_alpha {
                Some((EncodedColor::Yuva, EncodedAlpha::Unpremul))
            } else {
                Some((EncodedColor::Yuv, EncodedAlpha::Opaque))
            }
        }
        _ => None,
    }
}

/// Exact port of `webp_decode_mode` in `src/codec/SkWebpCodec.cpp`.
#[inline]
pub const fn webp_decode_mode(dst_ct: ColorType, premultiply: bool) -> WebpCspMode {
    match dst_ct {
        ColorType::Bgra8888 => {
            if premultiply {
                WebpCspMode::BgrAPremul
            } else {
                WebpCspMode::Bgra
            }
        }
        ColorType::Rgba8888 => {
            if premultiply {
                WebpCspMode::RgbAPremul
            } else {
                WebpCspMode::Rgba
            }
        }
        ColorType::Rgb565 => WebpCspMode::Rgb565,
        _ => WebpCspMode::Last,
    }
}

/// Exact port of `is_8888` in `src/codec/SkWebpCodec.cpp`.
#[inline]
pub const fn is_8888(ct: ColorType) -> bool {
    matches!(ct, ColorType::Rgba8888 | ColorType::Bgra8888)
}

/// Exact port of `SkWebpCodec::onGetValidSubset`.
///
/// Validates that `desired` is contained within `bounds` and snaps `left` and
/// `top` down to even coordinates (matching libwebp's crop snapping).
#[inline]
pub fn get_valid_subset(bounds: IRect, desired: &mut IRect) -> bool {
    if !irect_contains(bounds, *desired) {
        return false;
    }
    desired.left = (desired.left >> 1) << 1;
    desired.top = (desired.top >> 1) << 1;
    true
}

/// Exact port of `SkScalingCodec::onGetScaledDimensions` (inherited by `SkWebpCodec`).
#[inline]
pub fn get_scaled_dimensions(dims: ISize, desired_scale: f32) -> ISize {
    // SkScalarRoundToInt(x) = floor(x + 0.5)
    let w = (desired_scale * (dims.width as f32) + 0.5).floor() as i32;
    let h = (desired_scale * (dims.height as f32) + 0.5).floor() as i32;
    ISize {
        width: w.max(1),
        height: h.max(1),
    }
}

/// Exact port of `SkScalingCodec::onDimensionsSupported` (inherited by `SkWebpCodec`).
#[inline]
pub const fn dimensions_supported(dims: ISize, requested: ISize) -> bool {
    1 <= requested.width
        && requested.width <= dims.width
        && 1 <= requested.height
        && requested.height <= dims.height
}

#[inline]
fn irect_intersects(a: IRect, b: IRect) -> bool {
    irect_intersect(a, b).is_some()
}

/// Complete decode geometry, buffer, colorspace, and blend plan for `SkWebpCodec::onGetPixels`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpGetPixelsPlan {
    /// True if `SkSampler::Fill` must be called before decoding (even on early return).
    pub should_fill_bg: bool,
    /// True if decoding should return `kSuccess` immediately after optional fill
    /// (subset disjoint from frame or scaled frame collapsed to 0 width/height).
    pub early_return_success: bool,
    pub dst_x: i32,
    pub dst_y: i32,
    pub use_cropping: bool,
    pub crop_left: i32,
    pub crop_top: i32,
    pub crop_width: i32,
    pub crop_height: i32,
    pub use_scaling: bool,
    pub scaled_width: i32,
    pub scaled_height: i32,
    pub blend_with_prev_frame: bool,
    /// `SkColorType` for `webpInfo`.
    pub webp_ct: i32,
    /// `SkAlphaType` for `webpInfo`.
    pub webp_at: i32,
    /// True when libwebp must decode into an intermediate `SkBitmap` rather than `dst`.
    pub needs_temp_decode_buffer: bool,
    /// `WEBP_CSP_MODE` passed to `config.output.colorspace`.
    pub csp_mode: i32,
    /// Byte offset into `dst` (`dstBpp * dstX + rowBytes * dstY`).
    pub dst_byte_offset: usize,
    /// Post-decode blend pipeline plan.
    pub blend_plan: WebpBlendPlan,
}

/// Exact port of `SkWebpCodec::onGetPixels` planning logic (crop/scale geometry,
/// fill policy, `webpInfo` construction, intermediate buffer decision, CSP mode,
/// and blend pipeline setup).
#[allow(clippy::too_many_arguments)]
pub fn plan_get_pixels(
    canvas_dims: ISize,
    frame_rect: IRect,
    frame_has_alpha: bool,
    frame_blend_method: WebpMuxAnimBlend,
    independent: bool,
    subset: Option<IRect>,
    dst_dims: ISize,
    dst_ct: ColorType,
    dst_at: AlphaType,
    dst_row_bytes: usize,
    has_color_xform: bool,
) -> WebpGetPixelsPlan {
    let bounds = irect_make_size(canvas_dims);
    let frame_is_subset = frame_rect != bounds;
    let should_fill_bg = independent && frame_is_subset;

    let mut dst_x = frame_rect.left;
    let mut dst_y = frame_rect.top;
    let mut subset_w = frame_rect.width();
    let mut subset_h = frame_rect.height();

    let mut use_cropping = false;
    let mut crop_left = 0;
    let mut crop_top = 0;
    let mut crop_width = 0;
    let mut crop_height = 0;

    let empty_blend = WebpBlendPlan {
        active: false,
        use_tmp_xform_row: false,
        blend_src_ct: dst_ct as i32,
        premul_dst: false,
        premul_src: false,
        unpremul_dst: false,
    };

    if let Some(mut sub) = subset {
        if !irect_intersects(sub, frame_rect) {
            return WebpGetPixelsPlan {
                should_fill_bg,
                early_return_success: true,
                dst_x,
                dst_y,
                use_cropping: false,
                crop_left: 0,
                crop_top: 0,
                crop_width: 0,
                crop_height: 0,
                use_scaling: false,
                scaled_width: 0,
                scaled_height: 0,
                blend_with_prev_frame: false,
                webp_ct: dst_ct as i32,
                webp_at: dst_at as i32,
                needs_temp_decode_buffer: false,
                csp_mode: WebpCspMode::Last as i32,
                dst_byte_offset: 0,
                blend_plan: empty_blend,
            };
        }

        let min_x = dst_x.min(sub.left);
        let min_y = dst_y.min(sub.top);
        dst_x -= min_x;
        dst_y -= min_y;
        let shifted_frame = frame_rect.make_offset(-min_x, -min_y);
        sub = sub.make_offset(-min_x, -min_y);

        let inter = irect_intersect(shifted_frame, sub).unwrap_or_else(IRect::make_empty);
        subset_w = inter.width();
        subset_h = inter.height();

        use_cropping = true;
        crop_left = sub.left;
        crop_top = sub.top;
        crop_width = subset_w;
        crop_height = subset_h;
    }

    let mut scaled_width = subset_w;
    let mut scaled_height = subset_h;
    let mut use_scaling = false;
    let src_size = subset.map(irect_size).unwrap_or(canvas_dims);
    if src_size != dst_dims {
        use_scaling = true;
        if frame_is_subset {
            let scale_x = (dst_dims.width as f32) / (src_size.width as f32);
            let scale_y = (dst_dims.height as f32) / (src_size.height as f32);

            // Conservative float-to-int truncation (matches C++ `dstX = scaleX * dstX`).
            dst_x = (scale_x * (dst_x as f32)) as i32;
            scaled_width = (scale_x * (scaled_width as f32)) as i32;
            dst_y = (scale_y * (dst_y as f32)) as i32;
            scaled_height = (scale_y * (scaled_height as f32)) as i32;
            if scaled_width == 0 || scaled_height == 0 {
                return WebpGetPixelsPlan {
                    should_fill_bg,
                    early_return_success: true,
                    dst_x,
                    dst_y,
                    use_cropping,
                    crop_left,
                    crop_top,
                    crop_width,
                    crop_height,
                    use_scaling,
                    scaled_width,
                    scaled_height,
                    blend_with_prev_frame: false,
                    webp_ct: dst_ct as i32,
                    webp_at: dst_at as i32,
                    needs_temp_decode_buffer: false,
                    csp_mode: WebpCspMode::Last as i32,
                    dst_byte_offset: 0,
                    blend_plan: empty_blend,
                };
            }
        } else {
            scaled_width = dst_dims.width;
            scaled_height = dst_dims.height;
        }
    }

    let blend_with_prev_frame =
        !independent && matches!(frame_blend_method, WebpMuxAnimBlend::Blend) && frame_has_alpha;

    let webp_at = if !frame_has_alpha {
        AlphaType::Opaque
    } else if has_color_xform || blend_with_prev_frame {
        // Both colorXform and blendLine expect unpremultiplied source pixels.
        AlphaType::Unpremul
    } else {
        dst_at
    };

    let webp_ct = if has_color_xform {
        // Lossless WebP is native BGRA; lossy YUV has equal cost for RGBA/BGRA;
        // colorXform swizzles BGRA->dstCT for free.
        ColorType::Bgra8888
    } else {
        dst_ct
    };

    let needs_temp_decode_buffer = (has_color_xform && !is_8888(dst_ct)) || blend_with_prev_frame;
    let csp_mode = webp_decode_mode(webp_ct, matches!(webp_at, AlphaType::Premul));
    let dst_bpp = color_type_bytes_per_pixel(dst_ct) as usize;
    let dst_byte_offset = dst_bpp
        .saturating_mul(dst_x.max(0) as usize)
        .saturating_add(dst_row_bytes.saturating_mul(dst_y.max(0) as usize));

    let blend_plan = plan_blend_pipeline(
        dst_ct,
        webp_ct,
        dst_at,
        frame_has_alpha,
        has_color_xform,
        blend_with_prev_frame,
    );

    WebpGetPixelsPlan {
        should_fill_bg,
        early_return_success: false,
        dst_x,
        dst_y,
        use_cropping,
        crop_left,
        crop_top,
        crop_width,
        crop_height,
        use_scaling,
        scaled_width,
        scaled_height,
        blend_with_prev_frame,
        webp_ct: webp_ct as i32,
        webp_at: webp_at as i32,
        needs_temp_decode_buffer,
        csp_mode: csp_mode as i32,
        dst_byte_offset,
        blend_plan,
    }
}

/// Interpreted result of `WebPIUpdate` (+ optional `WebPIDecGetRGB`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpIdecUpdateOutcome {
    /// Matches `SkCodec::Result`.
    pub result: i32,
    /// Number of decoded rows within the frame rect to post-process (xform/blend).
    pub rows_decoded: i32,
    /// True if `*rowsDecodedPtr` should be updated with `rows_decoded_out`.
    pub write_rows_decoded_ptr: bool,
    /// Value to write to `*rowsDecodedPtr` (`rowsDecoded + dstY`).
    pub rows_decoded_out: i32,
}

/// Exact port of the `WebPIUpdate` status switch in `SkWebpCodec::onGetPixels`.
#[inline]
pub const fn interpret_idec_update(
    status: Vp8StatusCode,
    scaled_height: i32,
    dst_y: i32,
    got_rgb: bool,
    idec_rows_decoded: i32,
) -> WebpIdecUpdateOutcome {
    match status {
        Vp8StatusCode::Ok => WebpIdecUpdateOutcome {
            result: CodecResult::Success as i32,
            rows_decoded: scaled_height,
            write_rows_decoded_ptr: false,
            rows_decoded_out: 0,
        },
        Vp8StatusCode::Suspended => {
            if !got_rgb || idec_rows_decoded <= 0 {
                WebpIdecUpdateOutcome {
                    result: CodecResult::InvalidInput as i32,
                    rows_decoded: 0,
                    write_rows_decoded_ptr: false,
                    rows_decoded_out: 0,
                }
            } else {
                WebpIdecUpdateOutcome {
                    result: CodecResult::IncompleteInput as i32,
                    rows_decoded: idec_rows_decoded,
                    write_rows_decoded_ptr: true,
                    rows_decoded_out: idec_rows_decoded.saturating_add(dst_y),
                }
            }
        }
        Vp8StatusCode::OutOfMemory
        | Vp8StatusCode::InvalidParam
        | Vp8StatusCode::BitstreamError
        | Vp8StatusCode::UnsupportedFeature
        | Vp8StatusCode::UserAbort
        | Vp8StatusCode::NotEnoughData => WebpIdecUpdateOutcome {
            result: CodecResult::InvalidInput as i32,
            rows_decoded: 0,
            write_rows_decoded_ptr: false,
            rows_decoded_out: 0,
        },
    }
}
