//! Safe Rust port of `src/codec/SkBmpMaskCodec.cpp` coordination and validation logic.

use skia_rs_codec_core::types::CodecResult;
use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::ISize;

/// Exact port of `SkBmpMaskCodec::onGetPixels` parameter validation.
#[inline]
pub const fn validate_mask_get_pixels(
    has_subset: bool,
    dst_dims: ISize,
    src_dims: ISize,
) -> CodecResult {
    if has_subset {
        return CodecResult::Unimplemented;
    }
    if dst_dims.width != src_dims.width || dst_dims.height != src_dims.height {
        return CodecResult::InvalidScale;
    }
    CodecResult::Success
}

/// Output swizzler `ColorType` and `AlphaType` computed by [`prepare_mask_swizzler_info`].
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MaskSwizzlerTargetInfo {
    pub color_type: ColorType,
    pub alpha_type: AlphaType,
}

/// Exact port of `SkBmpMaskCodec::onPrepareToDecode` swizzler target info selection.
#[inline]
pub const fn prepare_mask_swizzler_info(
    dst_color_type: ColorType,
    dst_alpha_type: AlphaType,
    has_color_xform: bool,
) -> MaskSwizzlerTargetInfo {
    if has_color_xform {
        let alpha_type = if matches!(dst_alpha_type, AlphaType::Premul) {
            AlphaType::Unpremul
        } else {
            dst_alpha_type
        };
        MaskSwizzlerTargetInfo {
            color_type: ColorType::Bgra8888,
            alpha_type,
        }
    } else {
        MaskSwizzlerTargetInfo {
            color_type: dst_color_type,
            alpha_type: dst_alpha_type,
        }
    }
}
