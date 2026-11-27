//! Safe Rust port of `src/codec/SkPngCodecBase.cpp`:
//! skcms pixel format selection, xform/swizzler planning, row-size & storage-budget
//! calculations, and PLTE/tRNS color table generation & padding.

use skia_rs_codec_core::types::{
    skcms_pixel_format, CodecResult, EncodedAlpha, EncodedColor, ZeroInitialized,
};
use skia_rs_color::color::{pmcolor_set_argb, premultiply_argb};
use skia_rs_color::image_info::{AlphaType, ColorType};

/// `skcms_PixelFormat_RGB_161616BE` discriminant from `modules/skcms/src/skcms_public.h`.
pub const SKCMS_PIXEL_FORMAT_RGB_161616BE: i32 = 22;

/// `skcms_PixelFormat_RGBA_16161616BE` discriminant from `modules/skcms/src/skcms_public.h`.
pub const SKCMS_PIXEL_FORMAT_RGBA_16161616BE: i32 = 24;

/// Maximum PNG palette entries (`PLTE` chunk allows at most 256 RGB entries).
pub const MAX_PALETTE_ENTRIES: usize = 256;

/// Opaque black (`SK_ColorBLACK = 0xFF000000`).
pub const COLOR_BLACK: u32 = 0xFF00_0000;

/// Exact port of `SkPngCodecBase::XformMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum XformMode {
    /// Requires only a swizzle pass.
    SwizzleOnly = 0,
    /// Requires only a color xform pass.
    ColorOnly = 1,
    /// Requires a swizzle and a color xform.
    SwizzleColor = 2,
}

impl XformMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::SwizzleOnly),
            1 => Some(Self::ColorOnly),
            2 => Some(Self::SwizzleColor),
            _ => None,
        }
    }
}

/// Exact port of `ToPixelFormat(const SkEncodedInfo&)` in `SkPngCodecBase.cpp`.
#[inline]
pub const fn encoded_info_to_skcms_pixel_format(
    color: EncodedColor,
    bits_per_component: u8,
) -> i32 {
    if bits_per_component == 16 {
        if matches!(color, EncodedColor::Rgba) {
            return SKCMS_PIXEL_FORMAT_RGBA_16161616BE;
        } else if matches!(color, EncodedColor::Rgb) {
            return SKCMS_PIXEL_FORMAT_RGB_161616BE;
        }
    } else if matches!(color, EncodedColor::Gray) {
        return skcms_pixel_format::G_8;
    }
    skcms_pixel_format::RGBA_8888
}

/// Exact port of `needs_premul` in `SkPngCodecBase.cpp`.
#[inline]
pub const fn needs_premul(dst_at: AlphaType, encoded_alpha: EncodedAlpha) -> bool {
    matches!(dst_at, AlphaType::Premul) && matches!(encoded_alpha, EncodedAlpha::Unpremul)
}

/// Exact port of `SkCodecPriv::ComputeRowBytes`.
#[inline]
pub const fn compute_encoded_row_bytes(frame_width: i32, encoded_bpp: u32) -> Option<usize> {
    if frame_width <= 0 || frame_width >= 0x00FF_FFFF || encoded_bpp == 0 || encoded_bpp >= 128 {
        return None;
    }
    let w = frame_width as usize;
    if encoded_bpp < 16 {
        if 8 % encoded_bpp != 0 {
            return None;
        }
        let pixels_per_byte = (8 / encoded_bpp) as usize;
        Some((w + pixels_per_byte - 1) / pixels_per_byte)
    } else {
        if encoded_bpp % 8 != 0 {
            return None;
        }
        let bytes_per_pixel = (encoded_bpp / 8) as usize;
        Some(w * bytes_per_pixel)
    }
}

/// Exact port of `SkPngCodecBase::initializeXformParams` destination row size calculation.
#[inline]
pub const fn compute_dst_row_size(xform_width: i32, dst_bytes_per_pixel: u32) -> Option<usize> {
    if xform_width <= 0 || xform_width >= 0x00FF_FFFF {
        return None;
    }
    Some((xform_width as usize) * (dst_bytes_per_pixel as usize))
}

/// Plan returned by `plan_xform_init`, corresponding to `SkPngCodecBase::initializeXforms`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct XformInitPlan {
    pub result: CodecResult,
    pub encoded_row_bytes: usize,
    pub skip_format_conversion: bool,
    pub color_only_mode: bool,
    pub need_color_table: bool,
}

/// Computes the initialization plan for `SkPngCodecBase::initializeXforms`.
pub const fn plan_xform_init(
    encoded_color: EncodedColor,
    encoded_bpc: u8,
    encoded_bpp: u32,
    dst_width: i32,
    frame_width: i32,
    has_subset: bool,
    has_color_xform: bool,
) -> XformInitPlan {
    if frame_width != dst_width && has_subset {
        return XformInitPlan {
            result: CodecResult::InvalidParameters,
            encoded_row_bytes: 0,
            skip_format_conversion: false,
            color_only_mode: false,
            need_color_table: false,
        };
    }

    let Some(encoded_row_bytes) = compute_encoded_row_bytes(frame_width, encoded_bpp) else {
        return XformInitPlan {
            result: CodecResult::InvalidParameters,
            encoded_row_bytes: 0,
            skip_format_conversion: false,
            color_only_mode: false,
            need_color_table: false,
        };
    };

    let skip_format_conversion = match encoded_color {
        EncodedColor::Rgb => {
            if encoded_bpc == 16 {
                has_color_xform
            } else {
                false
            }
        }
        EncodedColor::Rgba | EncodedColor::Gray => has_color_xform,
        _ => false,
    };

    let color_only_mode = skip_format_conversion && !has_subset;
    let need_color_table = !color_only_mode && matches!(encoded_color, EncodedColor::Palette);

    XformInitPlan {
        result: CodecResult::Success,
        encoded_row_bytes,
        skip_format_conversion,
        color_only_mode,
        need_color_table,
    }
}

/// Plan returned by `plan_swizzler`, corresponding to `SkPngCodecBase::initializeSwizzler`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SwizzlerPlan {
    pub xform_mode: XformMode,
    pub swizzler_ct: ColorType,
    pub swizzler_at: AlphaType,
    pub swizzler_zero_init: ZeroInitialized,
    pub use_frame_rect: bool,
    pub use_simple_swizzler: bool,
    pub simple_src_bpp: i32,
}

/// Exact port of `SkPngCodecBase::initializeSwizzler` configuration logic.
pub const fn plan_swizzler(
    encoded_color: EncodedColor,
    encoded_bpc: u8,
    dst_ct: ColorType,
    dst_at: AlphaType,
    zero_init: ZeroInitialized,
    has_color_xform: bool,
    xform_on_decode: bool,
    skip_format_conversion: bool,
    has_subset: bool,
) -> SwizzlerPlan {
    let mut swizzler_ct = dst_ct;
    let mut swizzler_at = dst_at;
    let mut swizzler_zero_init = zero_init;
    let mut xform_mode = XformMode::SwizzleOnly;

    if has_color_xform && xform_on_decode {
        swizzler_ct = if matches!(encoded_color, EncodedColor::Gray) {
            ColorType::Gray8
        } else {
            ColorType::Rgba8888
        };
        if matches!(dst_at, AlphaType::Premul) {
            swizzler_at = AlphaType::Unpremul;
        }
        xform_mode = XformMode::SwizzleColor;
        swizzler_zero_init = ZeroInitialized::No;
    }

    let use_frame_rect = !has_subset;
    let simple_src_bpp = if skip_format_conversion {
        match encoded_color {
            EncodedColor::Rgb => 6, // 16-bit RGB -> 6 bytes per pixel
            EncodedColor::Rgba => (encoded_bpc / 2) as i32,
            EncodedColor::Gray => 1,
            _ => 0,
        }
    } else {
        0
    };

    SwizzlerPlan {
        xform_mode,
        swizzler_ct,
        swizzler_at,
        swizzler_zero_init,
        use_frame_rect,
        use_simple_swizzler: skip_format_conversion,
        simple_src_bpp,
    }
}

/// Exact port of `SkPngCodecBase::allocateStorage` byte-count calculation.
#[inline]
pub const fn compute_storage_bytes(
    xform_mode: XformMode,
    encoded_bpp: u32,
    dst_width: i32,
) -> usize {
    match xform_mode {
        XformMode::SwizzleOnly => 0,
        XformMode::ColorOnly | XformMode::SwizzleColor => {
            if dst_width <= 0 {
                return 0;
            }
            let bytes_per_pixel: usize = if encoded_bpp > 32 {
                (encoded_bpp / 8) as usize
            } else {
                4
            };
            (dst_width as usize) * bytes_per_pixel
        }
    }
}

/// Exact port of `SkCodecPriv::IsRGBA`.
#[inline]
pub const fn is_rgba_order(ct: ColorType, pmcolor_is_rgba: bool) -> bool {
    if pmcolor_is_rgba {
        !matches!(ct, ColorType::Bgra8888)
    } else {
        matches!(ct, ColorType::Rgba8888)
    }
}

/// Result of populating palette entries before optional skcms color xform and padding.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ColorTablePopulateResult {
    pub num_colors: usize,
    pub max_colors: usize,
    pub should_xform_table: bool,
}

/// Exact port of `SkPngCodecBase::createColorTable` entry packing stage.
///
/// Populates `out_table[0..num_colors]` from `plte_rgb` (3 bytes per entry) and
/// optional `trns_alphas`. The caller should then:
/// 1. If `result.should_xform_table`, run `applyColorXform(out_table, out_table, result.num_colors)`.
/// 2. Call `pad_color_table(out_table, result.num_colors, result.max_colors)`.
pub fn populate_color_table_entries(
    plte_rgb: &[u8],
    trns_alphas: Option<&[u8]>,
    bits_per_component: u8,
    encoded_alpha: EncodedAlpha,
    dst_ct: ColorType,
    dst_at: AlphaType,
    has_color_xform: bool,
    xform_on_decode: bool,
    pmcolor_is_rgba: bool,
    out_table: &mut [u32; MAX_PALETTE_ENTRIES],
) -> ColorTablePopulateResult {
    let raw_count = plte_rgb.len() / 3;
    let num_colors = raw_count.min(MAX_PALETTE_ENTRIES);

    let table_ct = if has_color_xform {
        ColorType::Rgba8888
    } else {
        dst_ct
    };
    let is_rgba = is_rgba_order(table_ct, pmcolor_is_rgba);
    let is_bgra = !is_rgba;

    let num_colors_with_alpha = match trns_alphas {
        Some(alphas) => alphas.len().min(num_colors),
        None => 0,
    };

    if let Some(alphas) = trns_alphas {
        let premultiply = !has_color_xform && needs_premul(dst_at, encoded_alpha);
        for i in 0..num_colors_with_alpha {
            let r = plte_rgb[i * 3] as u32;
            let g = plte_rgb[i * 3 + 1] as u32;
            let b = plte_rgb[i * 3 + 2] as u32;
            let a = alphas[i] as u32;
            out_table[i] = if premultiply {
                premultiply_argb(a, r, g, b, is_bgra)
            } else {
                pmcolor_set_argb(a, r, g, b, is_bgra)
            };
        }
    }

    for i in num_colors_with_alpha..num_colors {
        let r = plte_rgb[i * 3] as u32;
        let g = plte_rgb[i * 3 + 1] as u32;
        let b = plte_rgb[i * 3 + 2] as u32;
        out_table[i] = pmcolor_set_argb(0xFF, r, g, b, is_bgra);
    }

    let max_colors = if bits_per_component <= 8 {
        1usize << bits_per_component
    } else {
        MAX_PALETTE_ENTRIES
    };
    let max_colors = max_colors.min(MAX_PALETTE_ENTRIES);

    ColorTablePopulateResult {
        num_colors,
        max_colors,
        should_xform_table: has_color_xform && !xform_on_decode,
    }
}

/// Exact port of the padding step in `SkPngCodecBase::createColorTable`.
///
/// Pads `table[num_colors..max_colors]` with the last color in `table` (or `SK_ColorBLACK`
/// if `num_colors == 0`).
pub fn pad_color_table(
    table: &mut [u32; MAX_PALETTE_ENTRIES],
    num_colors: usize,
    max_colors: usize,
) {
    let clamped_max = max_colors.min(MAX_PALETTE_ENTRIES);
    if num_colors < clamped_max {
        let last_color = if num_colors > 0 {
            table[num_colors - 1]
        } else {
            COLOR_BLACK
        };
        for slot in &mut table[num_colors..clamped_max] {
            *slot = last_color;
        }
    }
}
