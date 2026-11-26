//! Safe Rust port of logic from `src/core/SkRasterPipeline.cpp` and
//! `src/core/SkRasterPipelineBlitter.cpp`.

use skia_rs_color::blend_mode::{should_pre_scale_coverage, BlendMode};
use skia_rs_color::image_info::{AlphaType, ColorType};

/// Exact port of `SkColorTypeIsNormalized` from `src/core/SkImageInfoPriv.h`.
#[inline]
pub const fn color_type_is_normalized(ct: ColorType) -> bool {
    match ct {
        ColorType::Unknown
        | ColorType::Alpha8
        | ColorType::Rgb565
        | ColorType::Argb4444
        | ColorType::Rgba8888
        | ColorType::Rgb888x
        | ColorType::Bgra8888
        | ColorType::Bgr888x
        | ColorType::Rgba1010102
        | ColorType::Rgb101010x
        | ColorType::Bgra1010102
        | ColorType::Bgr101010x
        | ColorType::Rgba10x6
        | ColorType::Gray8
        | ColorType::RgbaF16Norm
        | ColorType::R8G8Unorm
        | ColorType::A16Unorm
        | ColorType::A16Float // subtle: alpha is always [0, 1]
        | ColorType::R16Unorm
        | ColorType::R16G16Unorm
        | ColorType::R16G16B16A16Unorm
        | ColorType::Srgba8888
        | ColorType::R8Unorm => true,

        ColorType::Bgra10101010Xr
        | ColorType::Bgr101010xXr
        | ColorType::RgbF16F16F16x
        | ColorType::RgbaF16
        | ColorType::RgbaF32
        | ColorType::R16Float
        | ColorType::R16G16Float => false,
    }
}

/// Classification of `appendSetRGB(rgb)`.
#[inline]
pub fn is_bounded_rgb(rgb: [f32; 3]) -> bool {
    0.0 <= rgb[0]
        && rgb[0] <= 1.0
        && 0.0 <= rgb[1]
        && rgb[1] <= 1.0
        && 0.0 <= rgb[2]
        && rgb[2] <= 1.0
}

/// Classification of `appendConstantColor(rgba)`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConstantColorClassification {
    /// 0 = black_color, 1 = white_color, 2 = uniform_color, 3 = unbounded_uniform_color
    pub kind: i32,
    pub rgba16: [u16; 4],
}

/// Exact port of `SkRasterPipeline::appendConstantColor` classification and U16 quantization.
pub fn classify_constant_color(rgba: [f32; 4]) -> ConstantColorClassification {
    if rgba[0] == 0.0 && rgba[1] == 0.0 && rgba[2] == 0.0 && rgba[3] == 1.0 {
        return ConstantColorClassification {
            kind: 0,
            rgba16: [0; 4],
        };
    }
    if rgba[0] == 1.0 && rgba[1] == 1.0 && rgba[2] == 1.0 && rgba[3] == 1.0 {
        return ConstantColorClassification {
            kind: 1,
            rgba16: [0; 4],
        };
    }
    if 0.0 <= rgba[0]
        && rgba[0] <= rgba[3]
        && 0.0 <= rgba[1]
        && rgba[1] <= rgba[3]
        && 0.0 <= rgba[2]
        && rgba[2] <= rgba[3]
    {
        let mut rgba16 = [0u16; 4];
        for i in 0..4 {
            rgba16[i] = (rgba[i] * 255.0f32 + 0.5f32) as u16;
        }
        ConstantColorClassification { kind: 2, rgba16 }
    } else {
        ConstantColorClassification {
            kind: 3,
            rgba16: [0; 4],
        }
    }
}

/// Classification of `appendMatrix(matrix)`.
/// Matches `SkMatrix::TypeMask`: Identity=0, Translate=1, Scale=2, Affine=4, Perspective=8.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatrixStageKind {
    Identity = 0,
    Translate = 1,
    ScaleTranslate = 2,
    Affine2x3 = 3,
    Perspective = 4,
}

/// Exact port of `SkRasterPipeline::appendMatrix` dispatch.
#[inline]
pub const fn classify_matrix_stage(type_mask: u32, has_perspective: bool) -> MatrixStageKind {
    const TRANSLATE_MASK: u32 = 0x01;
    const SCALE_MASK: u32 = 0x02;
    if type_mask == 0 {
        MatrixStageKind::Identity
    } else if type_mask == TRANSLATE_MASK {
        MatrixStageKind::Translate
    } else if (type_mask | (SCALE_MASK | TRANSLATE_MASK)) == (SCALE_MASK | TRANSLATE_MASK) {
        MatrixStageKind::ScaleTranslate
    } else if !has_perspective {
        MatrixStageKind::Affine2x3
    } else {
        MatrixStageKind::Perspective
    }
}

/// Classification of `appendTransferFunction(tf)` given `skcms_TFType` discriminant.
/// `skcms_TFType`: Invalid=0, sRGBish=1, PQish=2, HLGish=3, HLGinvish=4.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TransferFnStageKind {
    Invalid = 0,
    Gamma = 1,
    Parametric = 2,
    PQish = 3,
    HLGish = 4,
    HLGinvish = 5,
}

/// Exact port of `SkRasterPipeline::appendTransferFunction` dispatch.
#[inline]
pub fn classify_transfer_function(
    tf_type: i32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
) -> TransferFnStageKind {
    match tf_type {
        1 => {
            if a == 1.0 && b == 0.0 && c == 0.0 && d == 0.0 && e == 0.0 && f == 0.0 {
                TransferFnStageKind::Gamma
            } else {
                TransferFnStageKind::Parametric
            }
        }
        2 => TransferFnStageKind::PQish,
        3 => TransferFnStageKind::HLGish,
        4 => TransferFnStageKind::HLGinvish,
        _ => TransferFnStageKind::Invalid,
    }
}

/// Exact port of `SkRasterPipeline::stagesNeeded`.
#[inline]
pub const fn stages_needed(num_stages: i32, has_rewind_ctx: bool) -> i32 {
    num_stages + 1 + if has_rewind_ctx { 1 } else { 0 }
}

/// Exact port of `can_direct_blit` from `SkRasterPipelineBlitter.cpp`.
#[inline]
pub fn can_direct_blit(
    has_shader: bool,
    has_color_filter: bool,
    is_dither: bool,
    blend_mode: Option<BlendMode>,
    alphaf: f32,
) -> bool {
    if has_shader || has_color_filter || is_dither {
        return false;
    }
    match blend_mode {
        Some(BlendMode::Clear | BlendMode::Src) => true,
        Some(BlendMode::SrcOver) => alphaf == 1.0f32,
        _ => false,
    }
}

/// Exact port of dither rate table in `SkRasterPipelineBlitter::Create`.
#[inline]
pub const fn dither_rate_for_color_type(ct: ColorType) -> f32 {
    match ct {
        ColorType::Argb4444 => 1.0 / 15.0,
        ColorType::Rgb565 => 1.0 / 63.0,
        ColorType::Gray8
        | ColorType::Rgb888x
        | ColorType::Rgba8888
        | ColorType::Bgra8888
        | ColorType::Bgr888x
        | ColorType::Srgba8888
        | ColorType::R8Unorm => 1.0 / 255.0,
        ColorType::Rgb101010x
        | ColorType::Rgba1010102
        | ColorType::Bgr101010x
        | ColorType::Bgra1010102
        | ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6 => 1.0 / 1023.0,
        _ => 0.0,
    }
}

/// Exact port of the `srcover_rgba_8888` fast-path predicate in `SkRasterPipelineBlitter::blitRect`.
#[inline]
pub fn can_use_srcover_8888_fast_path(
    blend_mode: Option<BlendMode>,
    dst_ct: ColorType,
    has_dst_cs: bool,
    dst_at: AlphaType,
    dither_rate: f32,
) -> bool {
    matches!(blend_mode, Some(BlendMode::SrcOver))
        && matches!(dst_ct, ColorType::Rgba8888 | ColorType::Bgra8888)
        && !has_dst_cs
        && !matches!(dst_at, AlphaType::Unpremul)
        && dither_rate == 0.0f32
}

/// Exact port of `SkRasterPipelineBlitter` coverage pre-scaling check.
#[inline]
pub const fn rp_blitter_should_prescale_coverage(
    blend_mode: Option<BlendMode>,
    rgb_coverage: bool,
) -> bool {
    match blend_mode {
        Some(m) => should_pre_scale_coverage(m, rgb_coverage),
        None => false,
    }
}

/// Exact port of `extract_mask_plane` offset calculation in `SkRasterPipelineBlitter::blitMask`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MaskPlaneAddress {
    pub stride: i32,
    pub byte_offset: isize,
}

#[inline]
pub const fn compute_mask_plane_address(
    bounds_left: i32,
    bounds_top: i32,
    row_bytes: usize,
    plane: i32,
    image_size: usize,
    is_lcd16: bool,
) -> MaskPlaneAddress {
    let bpp: usize = if is_lcd16 { 2 } else { 1 };
    let stride = (row_bytes / bpp) as i32;
    let plane_off = (plane as isize).wrapping_mul(image_size as isize);
    let x_off = (bounds_left as isize).wrapping_mul(bpp as isize);
    let y_off = (bounds_top as isize).wrapping_mul(row_bytes as isize);
    MaskPlaneAddress {
        stride,
        byte_offset: plane_off.wrapping_sub(x_off).wrapping_sub(y_off),
    }
}

/// Exact port of `can_use_memset2d` check in `SkRasterPipelineBlitter::Create`.
#[inline]
pub const fn can_use_memset2d(
    is_constant: bool,
    blend_mode: Option<BlendMode>,
    bytes_per_pixel: i32,
) -> bool {
    is_constant && matches!(blend_mode, Some(BlendMode::Src)) && bytes_per_pixel <= 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_color_classification() {
        assert_eq!(classify_constant_color([0.0, 0.0, 0.0, 1.0]).kind, 0);
        assert_eq!(classify_constant_color([1.0, 1.0, 1.0, 1.0]).kind, 1);

        let c = classify_constant_color([0.25, 0.5, 0.0, 0.5]);
        assert_eq!(c.kind, 2);
        assert_eq!(c.rgba16, [64, 128, 0, 128]);

        // Unbounded (r > a)
        let u = classify_constant_color([0.75, 0.25, 0.0, 0.5]);
        assert_eq!(u.kind, 3);
        // Unbounded (negative)
        let u2 = classify_constant_color([-0.1, 0.0, 0.0, 1.0]);
        assert_eq!(u2.kind, 3);
    }

    #[test]
    fn matrix_and_transfer_fn_classification() {
        assert_eq!(classify_matrix_stage(0, false), MatrixStageKind::Identity);
        assert_eq!(classify_matrix_stage(1, false), MatrixStageKind::Translate);
        assert_eq!(classify_matrix_stage(2, false), MatrixStageKind::ScaleTranslate);
        assert_eq!(classify_matrix_stage(3, false), MatrixStageKind::ScaleTranslate);
        assert_eq!(classify_matrix_stage(4, false), MatrixStageKind::Affine2x3);
        assert_eq!(classify_matrix_stage(8, true), MatrixStageKind::Perspective);

        assert_eq!(
            classify_transfer_function(1, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            TransferFnStageKind::Gamma
        );
        assert_eq!(
            classify_transfer_function(1, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0),
            TransferFnStageKind::Parametric
        );
        assert_eq!(
            classify_transfer_function(2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            TransferFnStageKind::PQish
        );
        assert_eq!(
            classify_transfer_function(3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            TransferFnStageKind::HLGish
        );
        assert_eq!(
            classify_transfer_function(4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            TransferFnStageKind::HLGinvish
        );
    }

    #[test]
    fn dither_rates_and_normalized_types() {
        assert_eq!(dither_rate_for_color_type(ColorType::Argb4444), 1.0 / 15.0);
        assert_eq!(dither_rate_for_color_type(ColorType::Rgb565), 1.0 / 63.0);
        assert_eq!(dither_rate_for_color_type(ColorType::Rgba8888), 1.0 / 255.0);
        assert_eq!(dither_rate_for_color_type(ColorType::Rgba1010102), 1.0 / 1023.0);
        assert_eq!(dither_rate_for_color_type(ColorType::RgbaF16), 0.0);
        assert_eq!(dither_rate_for_color_type(ColorType::Alpha8), 0.0);

        assert!(color_type_is_normalized(ColorType::A16Float));
        assert!(color_type_is_normalized(ColorType::RgbaF16Norm));
        assert!(!color_type_is_normalized(ColorType::RgbaF16));
        assert!(!color_type_is_normalized(ColorType::RgbaF32));
        assert!(!color_type_is_normalized(ColorType::R16Float));
    }

    #[test]
    fn direct_blit_and_fast_paths() {
        assert!(can_direct_blit(false, false, false, Some(BlendMode::Clear), 0.0));
        assert!(can_direct_blit(false, false, false, Some(BlendMode::Src), 0.25));
        assert!(can_direct_blit(false, false, false, Some(BlendMode::SrcOver), 1.0));
        assert!(!can_direct_blit(false, false, false, Some(BlendMode::SrcOver), 0.5));
        assert!(!can_direct_blit(true, false, false, Some(BlendMode::Src), 1.0));

        assert!(can_use_srcover_8888_fast_path(
            Some(BlendMode::SrcOver),
            ColorType::Bgra8888,
            false,
            AlphaType::Premul,
            0.0
        ));
        assert!(!can_use_srcover_8888_fast_path(
            Some(BlendMode::SrcOver),
            ColorType::Bgra8888,
            true,
            AlphaType::Premul,
            0.0
        ));
        assert!(!can_use_srcover_8888_fast_path(
            Some(BlendMode::SrcOver),
            ColorType::Bgra8888,
            false,
            AlphaType::Unpremul,
            0.0
        ));
    }

    #[test]
    fn mask_plane_address_computation() {
        let a = compute_mask_plane_address(10, 20, 64, 1, 1024, false);
        assert_eq!(a.stride, 64);
        assert_eq!(a.byte_offset, 1024 - 10 - 20 * 64);

        let b = compute_mask_plane_address(5, 3, 40, 0, 200, true);
        assert_eq!(b.stride, 20);
        assert_eq!(b.byte_offset, -(5 * 2) - 3 * 40);
    }
}
