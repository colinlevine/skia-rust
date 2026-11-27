//! Safe Rust port of `src/codec/SkCodec.cpp` and `src/codec/SkCodecImageGenerator.cpp`
//! core validation, policy, color-xform planning, scanline, and incomplete-fill logic.

use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::matrix::ScaleToFit;
use skia_rs_geometry::{IRect, ISize, Matrix, Rect};

use crate::sampler::fill_is_zero_bytes;
use crate::types::{
    irect_contains, irect_intersect, irect_make_size, skcms_alpha_format, skcms_pixel_format,
    CodecResult, DisposalMethod, EncodedAlpha, EncodedColor, EncodedOrigin, ScanlineOrder,
    SelectionPolicy, XformTime, ZeroInitialized, MIN_BUFFERED_BYTES_NEEDED, NO_FRAME,
};

/// Exact port of `SkCodec::ResultToString` (null-terminated C string bytes).
pub const fn result_to_cstr(result: i32) -> &'static [u8] {
    match CodecResult::from_i32(result) {
        Some(CodecResult::Success) => b"success\0",
        Some(CodecResult::IncompleteInput) => b"incomplete input\0",
        Some(CodecResult::ErrorInInput) => b"error in input\0",
        Some(CodecResult::InvalidConversion) => b"invalid conversion\0",
        Some(CodecResult::InvalidScale) => b"invalid scale\0",
        Some(CodecResult::InvalidParameters) => b"invalid parameters\0",
        Some(CodecResult::InvalidInput) => b"invalid input\0",
        Some(CodecResult::CouldNotRewind) => b"could not rewind\0",
        Some(CodecResult::InternalError) => b"internal error\0",
        Some(CodecResult::Unimplemented) => b"unimplemented\0",
        Some(CodecResult::OutOfMemory) => b"out of memory\0",
        None => b"bogus result value\0",
    }
}

/// Decoder context dispatch kind for `SkCodec::MakeFromStream`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DecoderDispatchKind {
    Default = 0,
    PngChunkReader = 1,
    SelectionPolicy = 2,
    RawFallback = 3,
}

/// Classifies a decoder `id` string for `SkCodec::MakeFromStream`.
pub fn classify_decoder_id(id: &[u8]) -> DecoderDispatchKind {
    if id == b"png" {
        DecoderDispatchKind::PngChunkReader
    } else if id == b"heif" || id == b"gif" {
        DecoderDispatchKind::SelectionPolicy
    } else if id == b"raw" {
        DecoderDispatchKind::RawFallback
    } else {
        DecoderDispatchKind::Default
    }
}

/// Validates `SkCodec::SelectionPolicy` for `SkCodec::MakeFromStream`.
#[inline]
pub const fn is_valid_selection_policy(policy: i32) -> bool {
    matches!(
        SelectionPolicy::from_i32(policy),
        Some(SelectionPolicy::PreferStillImage | SelectionPolicy::PreferAnimation)
    )
}

/// Result returned by `SkCodec::MakeFromStream` when no decoder matched.
#[inline]
pub const fn no_decoder_match_result(bytes_read: usize) -> CodecResult {
    if bytes_read < MIN_BUFFERED_BYTES_NEEDED {
        CodecResult::IncompleteInput
    } else {
        CodecResult::Unimplemented
    }
}

/// Exact port of `SkCodecPriv::ValidAlpha`.
#[inline]
pub const fn valid_alpha(dst_alpha: AlphaType, src_is_opaque: bool) -> bool {
    if matches!(dst_alpha, AlphaType::Unknown) {
        return false;
    }
    if src_is_opaque {
        return true;
    }
    !matches!(dst_alpha, AlphaType::Opaque)
}

/// Exact port of `SkCodec::conversionSupported`.
#[inline]
pub const fn conversion_supported(
    encoded_color: EncodedColor,
    dst_ct: ColorType,
    dst_at: AlphaType,
    src_is_opaque: bool,
) -> bool {
    if !valid_alpha(dst_at, src_is_opaque) {
        return false;
    }
    match dst_ct {
        ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::RgbaF16 => true,
        ColorType::Bgr101010xXr | ColorType::Rgb565 => src_is_opaque,
        ColorType::Gray8 => matches!(encoded_color, EncodedColor::Gray) && src_is_opaque,
        ColorType::Alpha8 => matches!(encoded_color, EncodedColor::XAlpha),
        _ => false,
    }
}

/// Exact port of `SkCodecPriv::SelectXformFormat`.
#[inline]
pub const fn select_xform_format(
    ct: ColorType,
    for_color_table: bool,
    pmcolor_is_rgba: bool,
) -> Option<i32> {
    match ct {
        ColorType::Rgba8888 => Some(skcms_pixel_format::RGBA_8888),
        ColorType::Bgra8888 => Some(skcms_pixel_format::BGRA_8888),
        ColorType::Rgb565 => {
            if for_color_table {
                if pmcolor_is_rgba {
                    Some(skcms_pixel_format::RGBA_8888)
                } else {
                    Some(skcms_pixel_format::BGRA_8888)
                }
            } else {
                Some(skcms_pixel_format::BGR_565)
            }
        }
        ColorType::RgbaF16 => Some(skcms_pixel_format::RGBA_HHHH),
        ColorType::Rgba1010102 => Some(skcms_pixel_format::RGBA_1010102),
        ColorType::Bgr101010xXr => Some(skcms_pixel_format::BGR_101010X_XR),
        ColorType::Gray8 => Some(skcms_pixel_format::G_8),
        _ => None,
    }
}

/// Returns true if `dst_ct` unconditionally requires a color xform when `usesColorXform()` is true.
#[inline]
pub const fn color_type_forces_xform(dst_ct: ColorType) -> bool {
    matches!(
        dst_ct,
        ColorType::RgbaF16 | ColorType::Rgba1010102 | ColorType::Bgr101010xXr
    )
}

/// Output of `finalize_color_xform`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ColorXformPlan {
    pub success: bool,
    pub xform_time: i32,
    pub dst_xform_format: i32,
    pub dst_xform_alpha_format: i32,
}

/// Computes `fXformTime`, `fDstXformFormat`, and `fDstXformAlphaFormat` when `needs_color_xform` is true.
pub const fn finalize_color_xform(
    encoded_color: EncodedColor,
    encoded_alpha: EncodedAlpha,
    dst_ct: ColorType,
    dst_at: AlphaType,
    pmcolor_is_rgba: bool,
) -> ColorXformPlan {
    let xform_time =
        if !matches!(encoded_color, EncodedColor::Palette) || matches!(dst_ct, ColorType::RgbaF16) {
            XformTime::DecodeRow
        } else {
            XformTime::Palette
        };

    let for_color_table = matches!(xform_time, XformTime::Palette);
    let Some(dst_xform_format) = select_xform_format(dst_ct, for_color_table, pmcolor_is_rgba) else {
        return ColorXformPlan {
            success: false,
            xform_time: xform_time as i32,
            dst_xform_format: 0,
            dst_xform_alpha_format: skcms_alpha_format::UNPREMUL,
        };
    };

    let dst_xform_alpha_format = if matches!(encoded_alpha, EncodedAlpha::Unpremul)
        && matches!(dst_at, AlphaType::Premul)
    {
        skcms_alpha_format::PREMUL_AS_ENCODED
    } else {
        skcms_alpha_format::UNPREMUL
    };

    ColorXformPlan {
        success: true,
        xform_time: xform_time as i32,
        dst_xform_format,
        dst_xform_alpha_format,
    }
}

/// Exact port of `SkCodec::allocateFromBudget`.
#[inline]
pub fn allocate_from_budget(budget: &mut usize, num_bytes: usize) -> bool {
    if num_bytes > *budget {
        return false;
    }
    *budget -= num_bytes;
    true
}

/// Result of `compute_zero_rect` for `zero_rect` in `SkCodec.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ZeroRectPlan {
    /// `false` if Rect2Rect / mapRect failed (caller returns `false` / `kInternalError`).
    pub ok: bool,
    /// `true` if there is a non-empty region to zero.
    pub should_fill: bool,
    /// Intersected rectangle in destination coordinates.
    pub rect: IRect,
    /// Byte offset from `pixels` to `(rect.left, rect.top)`.
    pub byte_offset: usize,
}

/// Exact port of `zero_rect` coordinate mapping & clipping in `src/codec/SkCodec.cpp`.
pub fn compute_zero_rect(
    dst_dims: ISize,
    src_dims: ISize,
    mut prev_rect: IRect,
    bpp: usize,
    row_bytes: usize,
) -> ZeroRectPlan {
    if dst_dims != src_dims {
        let src = Rect::make_ltrb(0.0, 0.0, src_dims.width as f32, src_dims.height as f32);
        let dst = Rect::make_ltrb(0.0, 0.0, dst_dims.width as f32, dst_dims.height as f32);
        let Some(map) = Matrix::rect_to_rect(src, dst, ScaleToFit::Fill) else {
            return ZeroRectPlan {
                ok: false,
                should_fill: false,
                rect: IRect::EMPTY,
                byte_offset: 0,
            };
        };
        let as_rect = Rect::make_ltrb(
            prev_rect.left as f32,
            prev_rect.top as f32,
            prev_rect.right as f32,
            prev_rect.bottom as f32,
        );
        let Some((mapped, _)) = map.map_rect_non_perspective(as_rect) else {
            return ZeroRectPlan {
                ok: false,
                should_fill: false,
                rect: IRect::EMPTY,
                byte_offset: 0,
            };
        };
        prev_rect = mapped.round_out();
    }

    let Some(clipped) = irect_intersect(prev_rect, irect_make_size(dst_dims)) else {
        return ZeroRectPlan {
            ok: true,
            should_fill: false,
            rect: IRect::EMPTY,
            byte_offset: 0,
        };
    };

    let byte_offset = (clipped.left as usize) * bpp + (clipped.top as usize) * row_bytes;
    ZeroRectPlan {
        ok: true,
        should_fill: true,
        rect: clipped,
        byte_offset,
    }
}

/// Basic validation for `SkCodec::getPixelsBudgeted`.
pub const fn validate_get_pixels_params(
    dst_ct: ColorType,
    has_pixels: bool,
    row_bytes: usize,
    min_row_bytes: usize,
) -> CodecResult {
    if matches!(dst_ct, ColorType::Unknown) {
        return CodecResult::InvalidConversion;
    }
    if !has_pixels {
        return CodecResult::InvalidParameters;
    }
    if row_bytes < min_row_bytes {
        return CodecResult::InvalidParameters;
    }
    CodecResult::Success
}

/// Validation for `SkCodec::startIncrementalDecode`.
pub fn validate_start_incremental_decode(
    supports_incremental: bool,
    dst_ct: ColorType,
    has_pixels: bool,
    dst_dims: ISize,
    subset: Option<IRect>,
) -> CodecResult {
    if !supports_incremental {
        return CodecResult::Unimplemented;
    }
    if matches!(dst_ct, ColorType::Unknown) {
        return CodecResult::InvalidConversion;
    }
    if !has_pixels {
        return CodecResult::InvalidParameters;
    }
    if let Some(sub) = subset {
        let size = irect_make_size(dst_dims);
        if !irect_contains(size, sub) {
            return CodecResult::InvalidParameters;
        }
        let top = sub.top;
        let bottom = sub.bottom;
        if top < 0 || top >= dst_dims.height || top >= bottom || bottom > dst_dims.height {
            return CodecResult::InvalidParameters;
        }
    }
    CodecResult::Success
}

/// Validation for `SkCodec::startScanlineDecode`.
pub fn validate_start_scanline_decode(
    dst_dims: ISize,
    subset: Option<IRect>,
    frame_index: i32,
) -> CodecResult {
    if let Some(sub) = subset {
        let size = irect_make_size(dst_dims);
        if !irect_contains(size, sub) {
            return CodecResult::InvalidInput;
        }
        if sub.top != 0 || sub.height() != dst_dims.height {
            return CodecResult::InvalidInput;
        }
    }
    if frame_index != 0 {
        return CodecResult::Unimplemented;
    }
    CodecResult::Success
}

/// Validation for `SkCodec::getScanlines`.
#[inline]
pub const fn validate_get_scanlines(
    curr_scanline: i32,
    dst_height: i32,
    count_lines: i32,
) -> bool {
    if curr_scanline < 0 {
        return false;
    }
    if count_lines <= 0 || curr_scanline.saturating_add(count_lines) > dst_height {
        return false;
    }
    true
}

/// Validation for `SkCodec::skipScanlines`.
#[inline]
pub const fn validate_skip_scanlines(
    curr_scanline: i32,
    dst_height: i32,
    count_lines: i32,
) -> bool {
    if curr_scanline < 0 {
        return false;
    }
    if count_lines < 0 || curr_scanline.saturating_add(count_lines) > dst_height {
        return false;
    }
    true
}

/// Exact port of `SkCodec::onOutputScanline`.
#[inline]
pub const fn output_scanline(
    order: ScanlineOrder,
    encoded_height: i32,
    input_scanline: i32,
) -> i32 {
    match order {
        ScanlineOrder::TopDown => input_scanline,
        ScanlineOrder::BottomUp => encoded_height - input_scanline - 1,
    }
}

/// Pre-validation for `SkCodec::handleFrameIndex` when `frame_index != 0`.
pub const fn validate_nonzero_frame_index(
    frame_index: i32,
    has_subset: bool,
    frame_count: i32,
) -> CodecResult {
    if frame_index < 0 {
        return CodecResult::InvalidParameters;
    }
    if has_subset {
        return CodecResult::InvalidParameters;
    }
    if frame_index >= frame_count {
        return CodecResult::IncompleteInput;
    }
    CodecResult::Success
}

/// Validates `options.fPriorFrame` against `[required_frame, frame_index)` and disposal method.
pub const fn validate_prior_frame_range(
    prior_frame: i32,
    required_frame: i32,
    frame_index: i32,
) -> bool {
    prior_frame >= required_frame && prior_frame < frame_index
}

/// Action for a prepared prior frame's disposal method in `SkCodec::handleFrameIndex`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PriorFrameDisposalAction {
    /// Proceed normally (no clear needed).
    Continue = 0,
    /// `kRestorePrevious` cannot be used as a prior frame -> `kInvalidParameters`.
    InvalidParameters = 1,
    /// `kRestoreBGColor` and `prepped_frame_id == required_frame` -> call `zero_rect`.
    ZeroRect = 2,
}

#[inline]
pub const fn classify_prior_frame_disposal(
    disposal: DisposalMethod,
    prepped_frame_id: i32,
    required_frame: i32,
) -> PriorFrameDisposalAction {
    match disposal {
        DisposalMethod::RestorePrevious => PriorFrameDisposalAction::InvalidParameters,
        DisposalMethod::RestoreBgColor => {
            if prepped_frame_id == required_frame {
                PriorFrameDisposalAction::ZeroRect
            } else {
                PriorFrameDisposalAction::Continue
            }
        }
        DisposalMethod::Keep => PriorFrameDisposalAction::Continue,
    }
}

/// Plan computed for `SkCodec::fillIncompleteImage`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FillIncompletePlan {
    pub should_fill: bool,
    pub fill_width: i32,
    pub lines_remaining: i32,
    pub byte_offset: usize,
}

/// Exact port of `SkCodec::fillIncompleteImage` decision & geometry math.
#[allow(clippy::too_many_arguments)]
pub fn plan_fill_incomplete_image(
    dst_ct: ColorType,
    dst_width: i32,
    row_bytes: usize,
    zero_init: ZeroInitialized,
    lines_requested: i32,
    lines_decoded: i32,
    has_frame: bool,
    frame_required: i32,
    frame_rect_matches_bounds: bool,
    sampler_fill_width: i32, // < 0 if no sampler
    subset_width: i32,       // < 0 if no subset
    scanline_order: ScanlineOrder,
) -> FillIncompletePlan {
    if zero_init == ZeroInitialized::Yes && fill_is_zero_bytes(dst_ct) {
        return FillIncompletePlan {
            should_fill: false,
            fill_width: 0,
            lines_remaining: 0,
            byte_offset: 0,
        };
    }

    if has_frame && (frame_required != NO_FRAME || (!frame_rect_matches_bounds && lines_decoded > 0))
    {
        return FillIncompletePlan {
            should_fill: false,
            fill_width: 0,
            lines_remaining: 0,
            byte_offset: 0,
        };
    }

    let lines_remaining = lines_requested - lines_decoded;
    let fill_width = if sampler_fill_width >= 0 {
        sampler_fill_width
    } else if subset_width >= 0 {
        subset_width
    } else {
        dst_width
    };
    let byte_offset = if scanline_order == ScanlineOrder::BottomUp {
        0
    } else {
        (lines_decoded as usize) * row_bytes
    };

    FillIncompletePlan {
        should_fill: true,
        fill_width,
        lines_remaining,
        byte_offset,
    }
}

/// Exact port of `SkCodecImageGenerator` `adjust_info`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GeneratorInfoAdjust {
    pub width: i32,
    pub height: i32,
    pub alpha_type: i32,
}

pub const fn adjust_generator_info(
    width: i32,
    height: i32,
    alpha_type: AlphaType,
    origin: EncodedOrigin,
    has_requested_alpha: bool,
    requested_alpha: AlphaType,
) -> GeneratorInfoAdjust {
    let at = if has_requested_alpha {
        requested_alpha
    } else if matches!(alpha_type, AlphaType::Unpremul) {
        AlphaType::Premul
    } else {
        alpha_type
    };
    let (w, h) = if origin.swaps_width_height() {
        (height, width)
    } else {
        (width, height)
    };
    GeneratorInfoAdjust {
        width: w,
        height: h,
        alpha_type: at as i32,
    }
}

/// Exact port of `SkCodecImageGenerator` / `SkCodec::getImage` decode-result acceptance check.
#[inline]
pub const fn is_partial_or_full_success(result: CodecResult) -> bool {
    matches!(
        result,
        CodecResult::Success | CodecResult::IncompleteInput | CodecResult::ErrorInInput
    )
}
