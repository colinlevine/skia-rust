//! Safe Rust port of `src/core/SkScalerContext.cpp` (`SkScalerContextRec`, matrix decomposition,
//! `MakeRecAndEffects`, fake bold, path metrics/mask raster helpers, LCD FIR filters,
//! A8->A1 packing, LUT application, and filtered mask clipping).

use crate::font::{
    make_text_matrix, FLAG_BASELINE_SNAP, FLAG_EMBEDDED_BITMAPS, FLAG_EMBOLDEN,
    FLAG_FORCE_AUTO_HINTING, FLAG_LINEAR_METRICS, FLAG_SUBPIXEL,
};
use crate::{AxisAlignment, FontEdging, MaskFormat, PixelGeometry};
use skia_rs_geometry::matrix::{
    AFFINE_MASK, MPERSP_0, MPERSP_1, MPERSP_2, MSCALE_X, MSCALE_Y, MSKEW_X, MSKEW_Y, MTRANS_X,
    MTRANS_Y, SCALE_MASK,
};
use skia_rs_geometry::matrix22::compute_givens_rotation_sin_cos;
use skia_rs_geometry::{IRect, Matrix, Point, Rect, Vector, SK_SCALAR_NEARLY_ZERO};
use skia_rs_math::scalar::float_interp_func;

/// Mirrors `SkScalerContext::Flags`.
pub const FLAG_FRAME_AND_FILL: u16 = 0x0001;
pub const FLAG_EMBEDDED_BITMAP_TEXT: u16 = 0x0004;
pub const FLAG_EMBOLDEN_SCALER: u16 = 0x0008;
pub const FLAG_SUBPIXEL_POSITIONING: u16 = 0x0010;
pub const FLAG_FORCE_AUTOHINTING: u16 = 0x0020;
pub const FLAG_HINTING_SHIFT: u16 = 7;
pub const FLAG_HINTING_BIT1: u16 = 0x0080;
pub const FLAG_HINTING_BIT2: u16 = 0x0100;
pub const FLAG_HINTING_MASK: u16 = FLAG_HINTING_BIT1 | FLAG_HINTING_BIT2;
pub const FLAG_LCD_VERTICAL: u16 = 0x0200;
pub const FLAG_LCD_BGR_ORDER: u16 = 0x0400;
pub const FLAG_GEN_A8_FROM_LCD: u16 = 0x0800;
pub const FLAG_LINEAR_METRICS_SCALER: u16 = 0x1000;
pub const FLAG_BASELINE_SNAP_SCALER: u16 = 0x2000;
pub const FLAG_NEEDS_FOREGROUND_COLOR: u16 = 0x4000;

/// Mirrors `SkScalerContextFlags`.
pub const SCALER_CONTEXT_FLAG_NONE: u32 = 0;
pub const SCALER_CONTEXT_FLAG_FAKE_GAMMA: u32 = 1 << 0;
pub const SCALER_CONTEXT_FLAG_BOOST_CONTRAST: u32 = 1 << 1;

/// Mirrors `SK_MAX_SIZE_FOR_LCDTEXT`.
pub const MAX_SIZE_FOR_LCD_TEXT: f32 = 48.0;
pub const MAX_SIZE2_FOR_LCD_TEXT: f32 = MAX_SIZE_FOR_LCD_TEXT * MAX_SIZE_FOR_LCD_TEXT;

/// Mirrors `kStdFakeBoldInterpKeys` / `kStdFakeBoldInterpValues`.
const STD_FAKE_BOLD_INTERP_KEYS: [f32; 2] = [9.0, 36.0];
const STD_FAKE_BOLD_INTERP_VALUES: [f32; 2] = [1.0 / 24.0, 1.0 / 32.0];

/// Mirrors `SkScalerContextRec::PreMatrixScale`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PreMatrixScale {
    Full = 0,
    Vertical = 1,
    VerticalInteger = 2,
}

impl PreMatrixScale {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Full),
            1 => Some(Self::Vertical),
            2 => Some(Self::VerticalInteger),
            _ => None,
        }
    }
}

/// Exact `#[repr(C)]` layout mirror of `SkScalerContextRec` (56 bytes, dense).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ScalerContextRec {
    pub typeface_id: u32,
    pub text_size: f32,
    pub pre_scale_x: f32,
    pub pre_skew_x: f32,
    pub post_2x2: [[f32; 2]; 2],
    pub frame_width: f32,
    pub miter_limit: f32,
    pub foreground_color: u32,
    pub lum_bits: u32,
    pub device_gamma: u8,
    pub reserved_align2: u8,
    pub contrast: u8,
    pub reserved_align: u8,
    pub mask_format: u8,
    /// Low 4 bits = `fStrokeJoin`, high 4 bits = `fStrokeCap` (matches C++ little-endian bitfields).
    pub stroke_join_cap: u8,
    pub flags: u16,
}

#[inline]
pub fn scalar_round_to_scalar(x: f32) -> f32 {
    ((x as f64) + 0.5_f64).floor() as f32
}

#[inline]
pub const fn external_gamma_from_internal(g: u8) -> f32 {
    (g as f32) / 64.0_f32
}

#[inline]
pub fn internal_gamma_from_external(g: f32) -> u8 {
    (g * 64.0_f32) as u8
}

#[inline]
pub const fn external_contrast_from_internal(c: u8) -> f32 {
    (c as f32) / 255.0_f32
}

#[inline]
pub fn internal_contrast_from_external(c: f32) -> u8 {
    ((c * 255.0_f32) + 0.5_f32) as u8
}

/// Mirrors `sk_t_scale255<3>(base)` in `SkMaskGamma.h`.
#[inline]
const fn scale255_3bits(base: u32) -> u32 {
    let shifted = base << 5;
    shifted | (shifted >> 3) | (shifted >> 6)
}

/// Mirrors `SkMaskGamma::CanonicalColor` (`SkTMaskGamma<3, 3, 3>::CanonicalColor`).
#[inline]
pub const fn canonical_color(color: u32) -> u32 {
    let r = scale255_3bits(((color >> 16) & 0xFF) >> 5);
    let g = scale255_3bits(((color >> 8) & 0xFF) >> 5);
    let b = scale255_3bits((color & 0xFF) >> 5);
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

/// Mirrors `SkComputeLuminance(r, g, b)` in `SkColorData.h`.
#[inline]
pub const fn compute_luminance(r: u32, g: u32, b: u32) -> u32 {
    (r * 54 + g * 183 + b * 19) >> 8
}

/// Mirrors `SkPack888ToRGB16(r, g, b)` in `SkColorData.h`.
#[inline]
pub const fn pack_888_to_rgb16(r: u32, g: u32, b: u32) -> u16 {
    (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16
}

/// Mirrors `sk_relax` in `SkScalerContext.cpp`.
#[inline]
pub fn sk_relax(x: f32) -> f32 {
    let n = scalar_round_to_scalar(x * 1024.0_f32);
    n / 1024.0_f32
}

/// Mirrors `compute_mask_format` in `SkScalerContext.cpp`.
#[inline]
pub const fn compute_mask_format(edging: u8) -> MaskFormat {
    if edging == FontEdging::Alias as u8 {
        MaskFormat::BW
    } else if edging == FontEdging::AntiAlias as u8 {
        MaskFormat::A8
    } else if edging == FontEdging::SubpixelAntiAlias as u8 {
        MaskFormat::LCD16
    } else {
        MaskFormat::A8
    }
}

/// Mirrors `too_big_for_lcd` in `SkScalerContext.cpp`.
#[inline]
pub fn too_big_for_lcd(rec: &ScalerContextRec, check_post_2x2: bool) -> bool {
    if check_post_2x2 {
        let mut area =
            rec.post_2x2[0][0] * rec.post_2x2[1][1] - rec.post_2x2[1][0] * rec.post_2x2[0][1];
        area *= rec.text_size * rec.text_size;
        area > MAX_SIZE2_FOR_LCD_TEXT
    } else {
        rec.text_size > MAX_SIZE_FOR_LCD_TEXT
    }
}

impl ScalerContextRec {
    #[inline]
    pub fn set_luminance_color(&mut self, c: u32) {
        let rgb = 0xFF00_0000 | (c & 0x00FF_FFFF);
        self.lum_bits = canonical_color(rgb);
    }

    #[inline]
    pub fn set_device_gamma(&mut self, g: f32) {
        self.device_gamma = internal_gamma_from_external(g);
    }

    #[inline]
    pub fn set_contrast(&mut self, c: f32) {
        self.contrast = internal_contrast_from_external(c);
    }

    #[inline]
    pub fn ignore_gamma(&mut self) {
        self.set_luminance_color(0);
        self.set_device_gamma(1.0);
    }

    #[inline]
    pub fn ignore_pre_blend(&mut self) {
        self.ignore_gamma();
        self.set_contrast(0.0);
    }

    #[inline]
    pub fn set_stroke_join_cap(&mut self, join: u8, cap: u8) {
        self.stroke_join_cap = (join & 0x0F) | ((cap & 0x0F) << 4);
    }

    #[inline]
    pub fn set_hinting(&mut self, hinting: u8) {
        self.flags =
            (self.flags & !FLAG_HINTING_MASK) | (((hinting as u16) & 0x3) << FLAG_HINTING_SHIFT);
    }

    /// Mirrors `SkScalerContextRec::getMatrixFrom2x2`.
    pub fn get_matrix_from_2x2(&self) -> Matrix {
        let mut m = Matrix::identity();
        m.set9(&[
            self.post_2x2[0][0],
            self.post_2x2[0][1],
            0.0,
            self.post_2x2[1][0],
            self.post_2x2[1][1],
            0.0,
            0.0,
            0.0,
            1.0,
        ]);
        m
    }

    /// Mirrors `SkScalerContextRec::getLocalMatrix`.
    pub fn get_local_matrix(&self) -> Matrix {
        make_text_matrix(self.text_size, self.pre_scale_x, self.pre_skew_x)
    }

    /// Mirrors `SkScalerContextRec::getSingleMatrix`.
    pub fn get_single_matrix(&self) -> Matrix {
        let mut m = self.get_local_matrix();
        let post = self.get_matrix_from_2x2();
        m.post_concat(&post);
        m
    }

    /// Mirrors `SkScalerContextRec::computeAxisAlignmentForHText`.
    pub fn compute_axis_alignment_for_h_text(&self) -> AxisAlignment {
        if (self.flags & FLAG_BASELINE_SNAP_SCALER) == 0 {
            return AxisAlignment::None;
        }
        if self.post_2x2[1][0] == 0.0 {
            return AxisAlignment::X;
        }
        if self.post_2x2[0][0] == 0.0 {
            return AxisAlignment::Y;
        }
        AxisAlignment::None
    }

    /// Mirrors `SkScalerContextRec::useStrokeForFakeBold`.
    pub fn use_stroke_for_fake_bold(&mut self) {
        if (self.flags & FLAG_EMBOLDEN_SCALER) == 0 {
            return;
        }
        self.flags &= !FLAG_EMBOLDEN_SCALER;

        let fake_bold_scale = float_interp_func(
            self.text_size,
            &STD_FAKE_BOLD_INTERP_KEYS,
            &STD_FAKE_BOLD_INTERP_VALUES,
        );
        let extra = self.text_size * fake_bold_scale;

        if self.frame_width >= 0.0 {
            self.frame_width += extra;
        } else {
            self.flags |= FLAG_FRAME_AND_FILL;
            self.frame_width = extra;
            // Default SkPaint values: miter=4.0, join=kMiter_Join(0), cap=kButt_Cap(0)
            self.miter_limit = 4.0;
            self.set_stroke_join_cap(0, 0);
        }
    }

    /// Post-`onFilterRec` portion of `SkScalerContext::PreprocessRec`.
    pub fn preprocess_post_filter(&mut self, has_mask_filter: bool) {
        if has_mask_filter {
            self.ignore_pre_blend();
        }
        let mut lum_color = self.lum_bits;
        if self.mask_format == MaskFormat::A8 as u8 {
            let lum = compute_luminance(
                (lum_color >> 16) & 0xFF,
                (lum_color >> 8) & 0xFF,
                lum_color & 0xFF,
            );
            lum_color = 0xFF00_0000 | (lum << 16) | (lum << 8) | lum;
        }
        self.set_luminance_color(lum_color);
    }

    /// Mirrors `SkScalerContextRec::computeMatrices`.
    pub fn compute_matrices(
        &self,
        pre_matrix_scale: PreMatrixScale,
        out_s: &mut Vector,
        out_s_a: &mut Matrix,
        mut out_gs_a: Option<&mut Matrix>,
        mut out_g_inv: Option<&mut Matrix>,
        out_a: Option<&mut Matrix>,
    ) -> bool {
        let a = self.get_single_matrix();
        if let Some(a_out) = out_a {
            *a_out = a;
        }

        let skewed_or_flipped = a.mat[MSKEW_X] != 0.0
            || a.mat[MSKEW_Y] != 0.0
            || a.mat[MSCALE_X] < 0.0
            || a.mat[MSCALE_Y] < 0.0;

        let ga = if skewed_or_flipped {
            let mut h_arr = [Point::new(1.0, 0.0)];
            a.map_points_in_place(&mut h_arr);
            let h = h_arr[0];

            let (sin_v, cos_v) = compute_givens_rotation_sin_cos(h);
            let mut g = Matrix::identity();
            g.set_sin_cos(sin_v, cos_v);

            let mut ga = g;
            ga.pre_concat(&a);

            if let Some(ref mut g_inv) = out_g_inv {
                g_inv.set9(&[
                    g.mat[MSCALE_X],
                    -g.mat[MSKEW_X],
                    g.mat[MTRANS_X],
                    -g.mat[MSKEW_Y],
                    g.mat[MSCALE_Y],
                    g.mat[MTRANS_Y],
                    g.mat[MPERSP_0],
                    g.mat[MPERSP_1],
                    g.mat[MPERSP_2],
                ]);
            }
            ga
        } else {
            if let Some(ref mut g_inv) = out_g_inv {
                g_inv.reset();
            }
            a
        };

        if ga.mat[MSCALE_X].abs() <= SK_SCALAR_NEARLY_ZERO
            || ga.mat[MSCALE_Y].abs() <= SK_SCALAR_NEARLY_ZERO
            || !ga.is_finite()
        {
            out_s.x = 1.0;
            out_s.y = 1.0;
            out_s_a.set_scale(0.0, 0.0);
            if let Some(ref mut gs_a) = out_gs_a {
                gs_a.set_scale(0.0, 0.0);
            }
            if let Some(ref mut g_inv) = out_g_inv {
                g_inv.reset();
            }
            return false;
        }

        match pre_matrix_scale {
            PreMatrixScale::Full => {
                out_s.x = ga.mat[MSCALE_X].abs();
                out_s.y = ga.mat[MSCALE_Y].abs();
            }
            PreMatrixScale::Vertical => {
                let y_scale = ga.mat[MSCALE_Y].abs();
                out_s.x = y_scale;
                out_s.y = y_scale;
            }
            PreMatrixScale::VerticalInteger => {
                let real_y_scale = ga.mat[MSCALE_Y].abs();
                let mut int_y_scale = scalar_round_to_scalar(real_y_scale);
                if int_y_scale == 0.0 {
                    int_y_scale = 1.0;
                }
                out_s.x = int_y_scale;
                out_s.y = int_y_scale;
            }
        }

        if !skewed_or_flipped
            && (matches!(pre_matrix_scale, PreMatrixScale::Full)
                || (matches!(pre_matrix_scale, PreMatrixScale::Vertical)
                    && a.mat[MSCALE_X] == a.mat[MSCALE_Y]))
        {
            out_s_a.reset();
        } else if !skewed_or_flipped && matches!(pre_matrix_scale, PreMatrixScale::Vertical) {
            out_s_a.reset();
            out_s_a.set_scale(a.mat[MSCALE_X] / out_s.y, 1.0);
        } else {
            *out_s_a = a;
            out_s_a.pre_scale(1.0 / out_s.x, 1.0 / out_s.y);
        }

        if let Some(ref mut gs_a) = out_gs_a {
            **gs_a = ga;
            gs_a.pre_scale(1.0 / out_s.x, 1.0 / out_s.y);
        }

        true
    }
}

/// Input parameters for `make_scaler_context_rec` (mirrors `SkScalerContext::MakeRecAndEffects`).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MakeRecParams {
    pub typeface_id: u32,
    pub font_size: f32,
    pub font_scale_x: f32,
    pub font_skew_x: f32,
    pub font_flags: u8,
    pub font_edging: u8,
    pub font_hinting: u8,
    pub glyph_mask_needs_current_color: bool,

    pub paint_style: u8, // 0=Fill, 1=Stroke, 2=StrokeAndFill
    pub stroke_join: u8,
    pub stroke_cap: u8,
    pub _pad0: u8,
    pub stroke_width: f32,
    pub stroke_miter: f32,
    pub paint_color: u32,
    pub luminance_color: u32,

    pub pixel_geometry: i32,
    pub text_gamma: f32,
    pub text_contrast: f32,
    pub scaler_context_flags: u32,
}

/// Mirrors `SkScalerContext::MakeRecAndEffects` rec construction.
pub fn make_scaler_context_rec(params: &MakeRecParams, device_matrix: &Matrix) -> ScalerContextRec {
    let mut rec = ScalerContextRec::default();
    rec.typeface_id = params.typeface_id;
    rec.text_size = params.font_size;
    rec.pre_scale_x = params.font_scale_x;
    rec.pre_skew_x = params.font_skew_x;

    let mut check_post_2x2 = false;
    let mask = device_matrix.get_type_cached();
    if (mask & SCALE_MASK) != 0 {
        rec.post_2x2[0][0] = sk_relax(device_matrix.mat[MSCALE_X]);
        rec.post_2x2[1][1] = sk_relax(device_matrix.mat[MSCALE_Y]);
        check_post_2x2 = true;
    } else {
        rec.post_2x2[0][0] = 1.0;
        rec.post_2x2[1][1] = 1.0;
    }
    if (mask & AFFINE_MASK) != 0 {
        rec.post_2x2[0][1] = sk_relax(device_matrix.mat[MSKEW_X]);
        rec.post_2x2[1][0] = sk_relax(device_matrix.mat[MSKEW_Y]);
        check_post_2x2 = true;
    } else {
        rec.post_2x2[0][1] = 0.0;
        rec.post_2x2[1][0] = 0.0;
    }

    let mut flags: u16 = 0;
    if (params.font_flags & FLAG_EMBOLDEN) != 0 {
        flags |= FLAG_EMBOLDEN_SCALER;
    }

    // SkPaint::kFill_Style == 0, kStroke_Style == 1, kStrokeAndFill_Style == 2
    if params.paint_style != 0 && params.stroke_width >= 0.0 {
        rec.frame_width = params.stroke_width;
        rec.miter_limit = params.stroke_miter;
        rec.set_stroke_join_cap(params.stroke_join, params.stroke_cap);
        if params.paint_style == 2 {
            flags |= FLAG_FRAME_AND_FILL;
        }
    } else {
        rec.frame_width = -1.0;
        rec.miter_limit = 0.0;
        rec.set_stroke_join_cap(0, 0);
    }

    rec.mask_format = compute_mask_format(params.font_edging) as u8;

    if rec.mask_format == MaskFormat::LCD16 as u8 {
        if too_big_for_lcd(&rec, check_post_2x2) {
            rec.mask_format = MaskFormat::A8 as u8;
            flags |= FLAG_GEN_A8_FROM_LCD;
        } else {
            match PixelGeometry::from_i32(params.pixel_geometry).unwrap_or(PixelGeometry::Unknown) {
                PixelGeometry::Unknown => {
                    rec.mask_format = MaskFormat::A8 as u8;
                    flags |= FLAG_GEN_A8_FROM_LCD;
                }
                PixelGeometry::RgbH => {}
                PixelGeometry::BgrH => {
                    flags |= FLAG_LCD_BGR_ORDER;
                }
                PixelGeometry::RgbV => {
                    flags |= FLAG_LCD_VERTICAL;
                }
                PixelGeometry::BgrV => {
                    flags |= FLAG_LCD_VERTICAL | FLAG_LCD_BGR_ORDER;
                }
            }
        }
    }

    if (params.font_flags & FLAG_EMBEDDED_BITMAPS) != 0 {
        flags |= FLAG_EMBEDDED_BITMAP_TEXT;
    }
    if (params.font_flags & FLAG_SUBPIXEL) != 0 {
        flags |= FLAG_SUBPIXEL_POSITIONING;
    }
    if (params.font_flags & FLAG_FORCE_AUTO_HINTING) != 0 {
        flags |= FLAG_FORCE_AUTOHINTING;
    }
    if (params.font_flags & FLAG_LINEAR_METRICS) != 0 {
        flags |= FLAG_LINEAR_METRICS_SCALER;
    }
    if (params.font_flags & FLAG_BASELINE_SNAP) != 0 {
        flags |= FLAG_BASELINE_SNAP_SCALER;
    }
    if params.glyph_mask_needs_current_color {
        flags |= FLAG_NEEDS_FOREGROUND_COLOR;
        rec.foreground_color = params.paint_color;
    }
    rec.flags = flags;

    rec.set_hinting(params.font_hinting);
    rec.set_luminance_color(params.luminance_color);
    rec.set_device_gamma(params.text_gamma);
    rec.set_contrast(params.text_contrast);

    if (params.scaler_context_flags & SCALER_CONTEXT_FLAG_FAKE_GAMMA) == 0 {
        rec.ignore_gamma();
    }
    if (params.scaler_context_flags & SCALER_CONTEXT_FLAG_BOOST_CONTRAST) == 0 {
        rec.set_contrast(0.0);
    }

    rec
}

/// Exact `sk_saturate_cast<int16_t, float>`: returns `i16::MAX` on NaN.
#[inline]
pub fn saturate_cast_f32_to_i16(mut s: f32) -> i16 {
    let max_f = i16::MAX as f32;
    let min_f = i16::MIN as f32;
    s = if s < max_f { s } else { max_f };
    s = if s > min_f { s } else { min_f };
    s as i16
}

/// Exact `sk_saturate_cast<uint16_t, float>`: returns `u16::MAX` on NaN.
#[inline]
pub fn saturate_cast_f32_to_u16(mut s: f32) -> u16 {
    let max_f = u16::MAX as f32;
    let min_f = u16::MIN as f32;
    s = if s < max_f { s } else { max_f };
    s = if s > min_f { s } else { min_f };
    s as u16
}

/// Exact `sk_saturate_cast<int16_t, int32_t>`.
#[inline]
pub const fn saturate_cast_i32_to_i16(s: i32) -> i16 {
    if s >= i16::MAX as i32 {
        i16::MAX
    } else if s <= i16::MIN as i32 {
        i16::MIN
    } else {
        s as i16
    }
}

/// Exact `sk_saturate_cast<uint16_t, int64_t>`.
#[inline]
pub const fn saturate_cast_i64_to_u16(s: i64) -> u16 {
    if s >= u16::MAX as i64 {
        u16::MAX
    } else if s <= 0 {
        0
    } else {
        s as u16
    }
}

/// Saturated glyph bounds output.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SaturatedGlyphBounds {
    pub left: i16,
    pub top: i16,
    pub width: u16,
    pub height: u16,
    pub mask_format: u8,
}

/// Mirrors `SkScalerContext::SaturateGlyphBounds(SkGlyph*, SkRect&&)`.
pub fn saturate_glyph_bounds_rect(r: Rect) -> SaturatedGlyphBounds {
    let left = r.left.floor();
    let top = r.top.floor();
    let right = r.right.ceil();
    let bottom = r.bottom.ceil();
    SaturatedGlyphBounds {
        left: saturate_cast_f32_to_i16(left),
        top: saturate_cast_f32_to_i16(top),
        width: saturate_cast_f32_to_u16(right - left),
        height: saturate_cast_f32_to_u16(bottom - top),
        mask_format: 0,
    }
}

/// Mirrors `SkScalerContext::SaturateGlyphBounds(SkGlyph*, const SkIRect&)`.
pub fn saturate_glyph_bounds_irect(r: IRect) -> SaturatedGlyphBounds {
    let w64 = (r.right as i64) - (r.left as i64);
    let h64 = (r.bottom as i64) - (r.top as i64);
    SaturatedGlyphBounds {
        left: saturate_cast_i32_to_i16(r.left),
        top: saturate_cast_i32_to_i16(r.top),
        width: saturate_cast_i64_to_u16(w64),
        height: saturate_cast_i64_to_u16(h64),
        mask_format: 0,
    }
}

/// Mirrors `SkScalerContext::GenerateMetricsFromPath`.
pub fn generate_metrics_from_path_bounds(
    mut mask_format: u8,
    mut bounds: Rect,
    vertical_lcd: bool,
    a8_from_lcd: bool,
    hairline: bool,
) -> SaturatedGlyphBounds {
    if mask_format != MaskFormat::BW as u8
        && mask_format != MaskFormat::A8 as u8
        && mask_format != MaskFormat::LCD16 as u8
    {
        mask_format = MaskFormat::A8 as u8;
    }

    if !bounds.is_empty() {
        let from_lcd = (mask_format == MaskFormat::LCD16 as u8)
            || (mask_format == MaskFormat::A8 as u8 && a8_from_lcd);
        let need_extra_width = (from_lcd && !vertical_lcd) || hairline;
        let need_extra_height = (from_lcd && vertical_lcd) || hairline;
        if need_extra_width {
            bounds = Rect::make_ltrb(
                bounds.left.floor() - 1.0,
                bounds.top.floor(),
                bounds.right.ceil() + 1.0,
                bounds.bottom.ceil(),
            );
        }
        if need_extra_height {
            bounds = Rect::make_ltrb(
                bounds.left.floor(),
                bounds.top.floor() - 1.0,
                bounds.right.ceil(),
                bounds.bottom.ceil() + 1.0,
            );
        }
    }

    let mut out = saturate_glyph_bounds_rect(bounds);
    out.mask_format = mask_format;
    out
}

/// Setup parameters computed for `SkScalerContext::GenerateImageFromPath`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PathMaskRasterPlan {
    pub dst_w: i32,
    pub dst_h: i32,
    pub matrix: Matrix,
    pub from_lcd: bool,
    pub intermediate_dst: bool,
}

/// Computes `dstW`, `dstH`, `matrix`, `fromLCD`, and `intermediateDst` for `GenerateImageFromPath`.
pub fn plan_generate_image_from_path(
    bounds: IRect,
    format: u8,
    vertical_lcd: bool,
    a8_from_lcd: bool,
    samples_per_pixel: i32,
) -> PathMaskRasterPlan {
    let src_w = bounds.width();
    let src_h = bounds.height();
    let mut dst_w = src_w;
    let mut dst_h = src_h;

    let mut matrix = Matrix::translate(-(bounds.left as f32), -(bounds.top as f32));

    let from_lcd = (format == MaskFormat::LCD16 as u8)
        || (format == MaskFormat::A8 as u8 && a8_from_lcd);
    let intermediate_dst = from_lcd || format == MaskFormat::BW as u8;

    if from_lcd {
        let spp = samples_per_pixel;
        let spp_f = spp as f32;
        if vertical_lcd {
            dst_w = spp * dst_h - 2 * spp;
            dst_h = src_w;
            matrix.set9(&[
                0.0,
                spp_f,
                -((bounds.top + 1) as f32) * spp_f,
                1.0,
                0.0,
                -(bounds.left as f32),
                0.0,
                0.0,
                1.0,
            ]);
        } else {
            dst_w = spp * dst_w - 2 * spp;
            matrix.set9(&[
                spp_f,
                0.0,
                -((bounds.left + 1) as f32) * spp_f,
                0.0,
                1.0,
                -(bounds.top as f32),
                0.0,
                0.0,
                1.0,
            ]);
        }
    }

    PathMaskRasterPlan {
        dst_w,
        dst_h,
        matrix,
        from_lcd,
        intermediate_dst,
    }
}

/// Mirrors `applyLUTToA8Mask` in `SkScalerContext.cpp`.
pub fn apply_lut_to_a8_mask(
    dst: &mut [u8],
    row_bytes: usize,
    width: usize,
    height: usize,
    lut: &[u8; 256],
) {
    for y in 0..height {
        let row_start = y * row_bytes;
        if row_start + width > dst.len() {
            break;
        }
        for pixel in &mut dst[row_start..row_start + width] {
            *pixel = lut[*pixel as usize];
        }
    }
}

/// Mirrors `packA8ToA1` in `SkScalerContext.cpp`.
pub fn pack_a8_to_a1(
    dst: &mut [u8],
    dst_rb: usize,
    src: &[u8],
    src_rb: usize,
    width: usize,
    height: usize,
) {
    let octs = width >> 3;
    let leftover_bits = width & 7;
    for y in 0..height {
        let src_row = &src[y * src_rb..y * src_rb + width];
        let dst_row_start = y * dst_rb;
        let mut dst_idx = dst_row_start;
        for i in 0..octs {
            let base = i * 8;
            let mut bits: u8 = 0;
            for j in 0..8 {
                bits = (bits << 1) | (src_row[base + j] >> 7);
            }
            dst[dst_idx] = bits;
            dst_idx += 1;
        }
        if leftover_bits > 0 {
            let base = octs * 8;
            let mut bits: u8 = 0;
            let mut shift = 7u32;
            for j in 0..leftover_bits {
                bits |= (src_row[base + j] >> 7) << shift;
                shift = shift.wrapping_sub(1);
            }
            dst[dst_idx] = bits;
        }
    }
}

/// Mirrors `pack4xHToMask` in `SkScalerContext.cpp` (12-tap RGB Gaussian FIR filter).
#[allow(clippy::too_many_arguments)]
pub fn pack_4xh_to_mask(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    src_rb: usize,
    dst: &mut [u8],
    dst_rb: usize,
    to_a8: bool,
    preblend_r: Option<&[u8; 256]>,
    preblend_g: Option<&[u8; 256]>,
    preblend_b: Option<&[u8; 256]>,
    do_bgr: bool,
    do_vert: bool,
) {
    const COEFFICIENTS: [[i32; 12]; 3] = [
        [0x03, 0x0b, 0x1c, 0x33, 0x40, 0x39, 0x24, 0x10, 0x05, 0x01, 0x00, 0x00],
        [0x00, 0x02, 0x08, 0x16, 0x2b, 0x3d, 0x3d, 0x2b, 0x16, 0x08, 0x02, 0x00],
        [0x00, 0x00, 0x01, 0x05, 0x10, 0x24, 0x39, 0x40, 0x33, 0x1c, 0x0b, 0x03],
    ];

    let sample_width = src_width as i32;
    let dst_pb = if to_a8 { 1usize } else { 2usize };

    for y in 0..src_height {
        let mut dst_off = if do_vert { y * dst_pb } else { y * dst_rb };
        let dst_delta = if do_vert { dst_rb } else { dst_pb };
        let src_row = &src[y * src_rb..y * src_rb + src_width];

        let mut sample_x: i32 = -4;
        while sample_x < sample_width + 4 {
            let mut fir = [0i32; 3];
            let start = (sample_x - 4).max(0);
            let end = (sample_x + 8).min(sample_width);
            let mut coeff_index = (start - (sample_x - 4)) as usize;
            let mut sample_index = start;
            while sample_index < end {
                let sample_value = src_row[sample_index as usize] as i32;
                fir[0] += COEFFICIENTS[0][coeff_index] * sample_value;
                fir[1] += COEFFICIENTS[1][coeff_index] * sample_value;
                fir[2] += COEFFICIENTS[2][coeff_index] * sample_value;
                sample_index += 1;
                coeff_index += 1;
            }
            for v in &mut fir {
                *v = (*v / 0x100).min(255);
            }

            let (mut r, mut g, mut b) = if do_bgr {
                (fir[2] as u32, fir[1] as u32, fir[0] as u32)
            } else {
                (fir[0] as u32, fir[1] as u32, fir[2] as u32)
            };

            if to_a8 {
                let mut a = (r + g + b) / 3;
                if let Some(lut_g) = preblend_g {
                    a = lut_g[a as usize] as u32;
                }
                dst[dst_off] = a as u8;
            } else {
                if let (Some(lr), Some(lg), Some(lb)) = (preblend_r, preblend_g, preblend_b) {
                    r = lr[r as usize] as u32;
                    g = lg[g as usize] as u32;
                    b = lb[b as usize] as u32;
                }
                let packed = pack_888_to_rgb16(r, g, b).to_ne_bytes();
                dst[dst_off] = packed[0];
                dst[dst_off + 1] = packed[1];
            }

            dst_off += dst_delta;
            sample_x += 4;
        }
    }
}

/// Mirrors `pack3xHToMask` in `SkScalerContext.cpp` (FreeType 5-tap LCD filter).
#[allow(clippy::too_many_arguments)]
pub fn pack_3xh_to_mask(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    src_rb: usize,
    dst: &mut [u8],
    dst_rb: usize,
    preblend_r: Option<&[u8; 256]>,
    preblend_g: Option<&[u8; 256]>,
    preblend_b: Option<&[u8; 256]>,
    do_bgr: bool,
    do_vert: bool,
) {
    const FILTER: [i32; 5] = [0x08, 0x4D, 0x56, 0x4D, 0x08];
    const SAMPLES_PER_PIXEL: i32 = 3;

    let sample_width = src_width as i32;
    let dst_pb = 2usize;
    let dst_p_delta = if do_vert { dst_rb } else { dst_pb };
    let dst_y_delta = if do_vert { dst_pb } else { dst_rb };

    for y in 0..src_height {
        let mut dst_off = y * dst_y_delta;
        let src_row = &src[y * src_rb..y * src_rb + src_width];

        let mut sample_x: i32 = -SAMPLES_PER_PIXEL;
        while sample_x < sample_width + SAMPLES_PER_PIXEL {
            let mut cov = [0u32; 3];
            for ch in 0..3 {
                let center = sample_x + ch as i32;
                let mut sum = 0i32;
                for tap in 0..5 {
                    let idx = center + tap as i32 - 2;
                    if idx >= 0 && idx < sample_width {
                        sum += FILTER[tap] * (src_row[idx as usize] as i32);
                    }
                }
                cov[ch] = (sum >> 8) as u32;
            }

            let (mut r, mut g, mut b) = if do_bgr {
                (cov[2], cov[1], cov[0])
            } else {
                (cov[0], cov[1], cov[2])
            };

            if let (Some(lr), Some(lg), Some(lb)) = (preblend_r, preblend_g, preblend_b) {
                r = lr[r as usize] as u32;
                g = lg[g as usize] as u32;
                b = lb[b as usize] as u32;
            }
            let packed = pack_888_to_rgb16(r, g, b).to_ne_bytes();
            dst[dst_off] = packed[0];
            dst[dst_off + 1] = packed[1];

            dst_off += dst_p_delta;
            sample_x += SAMPLES_PER_PIXEL;
        }
    }
}

/// Mirrors the filtered mask intersection clipping and 1- or 3-plane copy in `SkScalerContext::getImage`.
#[allow(clippy::too_many_arguments)]
pub fn clip_and_copy_filtered_mask(
    src: &[u8],
    src_bounds: IRect,
    src_rb: usize,
    src_format: u8,
    src_plane_bytes: usize,
    dst: &mut [u8],
    dst_bounds: IRect,
    dst_rb: usize,
    dst_format: u8,
    dst_plane_bytes: usize,
) {
    let is_byte_per_pixel = src_format == MaskFormat::A8 as u8
        || src_format == MaskFormat::Format3D as u8
        || src_format == MaskFormat::SDF as u8;

    if src_format != dst_format || !is_byte_per_pixel || !IRect::intersects(src_bounds, dst_bounds)
    {
        dst.fill(0);
        return;
    }

    let orig_bounds = dst_bounds;
    let mut sb = src_bounds;
    let mut db = dst_bounds;
    let mut src_off: usize = 0;
    let mut dst_off: usize = 0;

    if sb.top < db.top {
        let top_diff = (db.top - sb.top) as usize;
        src_off += src_rb * top_diff;
        sb.top = db.top;
    }
    if db.top < sb.top {
        let top_diff = (sb.top - db.top) as usize;
        dst_off += dst_rb * top_diff;
        db.top = sb.top;
    }

    if sb.left < db.left {
        let left_diff = (db.left - sb.left) as usize;
        src_off += left_diff;
        sb.left = db.left;
    }
    if db.left < sb.left {
        let left_diff = (sb.left - db.left) as usize;
        dst_off += left_diff;
        db.left = sb.left;
    }

    if sb.bottom < db.bottom {
        db.bottom = sb.bottom;
    }
    if db.bottom < sb.bottom {
        sb.bottom = db.bottom;
    }

    if sb.right < db.right {
        db.right = sb.right;
    }
    if db.right < sb.right {
        sb.right = db.right;
    }

    let width = sb.width() as usize;
    let height = sb.height() as usize;

    if db != orig_bounds {
        dst.fill(0);
    }

    let planes = if src_format == MaskFormat::Format3D as u8 {
        3
    } else {
        1
    };

    for plane in 0..planes {
        let mut p_src = src_off + plane * src_plane_bytes;
        let mut p_dst = dst_off + plane * dst_plane_bytes;
        for _ in 0..height {
            dst[p_dst..p_dst + width].copy_from_slice(&src[p_src..p_src + width]);
            p_src += src_rb;
            p_dst += dst_rb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rec_size_is_56() {
        assert_eq!(std::mem::size_of::<ScalerContextRec>(), 56);
    }

    #[test]
    fn test_compute_matrices_identity_and_rotated() {
        let mut rec = ScalerContextRec::default();
        rec.text_size = 16.0;
        rec.pre_scale_x = 1.0;
        rec.pre_skew_x = 0.0;
        rec.post_2x2 = [[1.0, 0.0], [0.0, 1.0]];

        let mut s = Vector::new(0.0, 0.0);
        let mut s_a = Matrix::identity();
        let mut gs_a = Matrix::identity();
        let mut g_inv = Matrix::identity();
        let mut a = Matrix::identity();

        assert!(rec.compute_matrices(
            PreMatrixScale::Full,
            &mut s,
            &mut s_a,
            Some(&mut gs_a),
            Some(&mut g_inv),
            Some(&mut a)
        ));
        assert_eq!(s, Vector::new(16.0, 16.0));
        assert!(s_a.is_identity());
        assert!(g_inv.is_identity());

        // 90-degree rotation
        rec.post_2x2 = [[0.0, -1.0], [1.0, 0.0]];
        assert!(rec.compute_matrices(
            PreMatrixScale::Full,
            &mut s,
            &mut s_a,
            Some(&mut gs_a),
            Some(&mut g_inv),
            Some(&mut a)
        ));
        assert!((s.x - 16.0).abs() < 1e-5);
        assert!((s.y - 16.0).abs() < 1e-5);
        // Verify g_inv * gs_a * scale(s) == a
        let mut recon = g_inv;
        recon.pre_concat(&gs_a);
        recon.pre_scale(s.x, s.y);
        for i in 0..9 {
            assert!((recon.mat[i] - a.mat[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn test_compute_matrices_singular() {
        let mut rec = ScalerContextRec::default();
        rec.text_size = 0.0;
        rec.pre_scale_x = 1.0;
        rec.post_2x2 = [[1.0, 0.0], [0.0, 1.0]];

        let mut s = Vector::new(0.0, 0.0);
        let mut s_a = Matrix::identity();
        assert!(!rec.compute_matrices(PreMatrixScale::Full, &mut s, &mut s_a, None, None, None));
        assert_eq!(s, Vector::new(1.0, 1.0));
        assert_eq!(s_a.mat[MSCALE_X], 0.0);
    }

    #[test]
    fn test_saturate_nan_is_max() {
        assert_eq!(saturate_cast_f32_to_i16(f32::NAN), i16::MAX);
        assert_eq!(saturate_cast_f32_to_u16(f32::NAN), u16::MAX);
        assert_eq!(saturate_cast_f32_to_i16(-1e9), i16::MIN);
        assert_eq!(saturate_cast_f32_to_u16(-5.0), 0);
    }

    #[test]
    fn test_pack_a8_to_a1() {
        let src = [0u8, 255, 128, 127, 255, 0, 0, 255, 255, 0];
        let mut dst = [0u8; 2];
        pack_a8_to_a1(&mut dst, 2, &src, 10, 10, 1);
        assert_eq!(dst[0], 0b0110_1001);
        assert_eq!(dst[1], 0b1000_0000);
    }

    #[test]
    fn test_pack_4xh_solid_a8_and_lcd() {
        // 3 pixels wide src = 12 samples -> dst width = 5 pixels (2 border + 3)
        let src = [255u8; 12];
        let mut dst_a8 = [0u8; 5];
        pack_4xh_to_mask(
            &src, 12, 1, 12, &mut dst_a8, 5, true, None, None, None, false, false,
        );
        // Center pixel gets full 12-tap coverage (~255)
        assert_eq!(dst_a8[2], 255);
        assert!(dst_a8[0] > 0 && dst_a8[4] > 0);
        // Symmetric filter produces symmetric output
        assert_eq!(dst_a8[0], dst_a8[4]);
        assert_eq!(dst_a8[1], dst_a8[3]);

        // Also test LCD16 output (RGB565 white = 0xFFFF at center)
        let mut dst_lcd = [0u8; 10];
        pack_4xh_to_mask(
            &src, 12, 1, 12, &mut dst_lcd, 10, false, None, None, None, false, false,
        );
        let center_px = u16::from_ne_bytes([dst_lcd[4], dst_lcd[5]]);
        assert_eq!(center_px, 0xFFFF);
    }

    #[test]
    fn test_clip_and_copy_filtered_mask_3d() {
        // 3 planes of 4x4 bytes = 48 bytes
        let mut src = vec![0u8; 48];
        for p in 0..3 {
            for i in 0..16 {
                src[p * 16 + i] = ((p + 1) * 16 + i) as u8;
            }
        }
        let mut dst = vec![0xAAu8; 48];
        clip_and_copy_filtered_mask(
            &src,
            IRect::make_ltrb(0, 0, 4, 4),
            4,
            MaskFormat::Format3D as u8,
            16,
            &mut dst,
            IRect::make_ltrb(2, 2, 6, 6),
            4,
            MaskFormat::Format3D as u8,
            16,
        );
        // Intersection is (2,2)-(4,4) -> top-left 2x2 of dst gets bottom-right 2x2 of src
        assert_eq!(dst[0], src[2 * 4 + 2]);
        assert_eq!(dst[1], src[2 * 4 + 3]);
        assert_eq!(dst[2], 0); // cleared outside intersection
        assert_eq!(dst[16], src[16 + 2 * 4 + 2]); // plane 1
        assert_eq!(dst[32], src[32 + 2 * 4 + 2]); // plane 2
    }

    #[test]
    fn test_use_stroke_for_fake_bold() {
        let mut rec = ScalerContextRec::default();
        rec.text_size = 9.0;
        rec.frame_width = -1.0;
        rec.flags = FLAG_EMBOLDEN_SCALER;
        rec.use_stroke_for_fake_bold();
        assert_eq!(rec.flags & FLAG_EMBOLDEN_SCALER, 0);
        assert_ne!(rec.flags & FLAG_FRAME_AND_FILL, 0);
        assert!((rec.frame_width - (9.0 / 24.0)).abs() < 1e-6);
        assert_eq!(rec.miter_limit, 4.0);
    }
}
