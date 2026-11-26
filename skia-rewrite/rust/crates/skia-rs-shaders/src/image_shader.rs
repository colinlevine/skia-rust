//! Safe Rust port of `SkImageShader.cpp` and `SkBitmapProcShader.cpp`.

use skia_rs_color::image_info::{color_type_channel_flags, AlphaType, ColorType};
use skia_rs_geometry::{Matrix, Rect, M44};

use crate::{rect_to_rect_or_identity, FilterMode, MipmapMode, SamplingOptions, TileMode};

/// Flag matching `SkShaderBase::kOpaqueAlpha_Flag`.
pub const OPAQUE_ALPHA_FLAG: u32 = 1 << 0;

/// Matches `SkImageShader::CubicResamplerMatrix(float B, float C)`.
#[inline]
pub fn cubic_resampler_matrix(b: f32, c: f32) -> M44 {
    M44::from_rows_scalar(
        (1.0_f32 / 6.0) * b,
        -(3.0_f32 / 6.0) * b - c,
        (3.0_f32 / 6.0) * b + 2.0 * c,
        -(1.0_f32 / 6.0) * b - c,
        1.0 - (2.0_f32 / 6.0) * b,
        0.0,
        -3.0 + (12.0_f32 / 6.0) * b + c,
        2.0 - (9.0_f32 / 6.0) * b - c,
        (1.0_f32 / 6.0) * b,
        (3.0_f32 / 6.0) * b + c,
        3.0 - (15.0_f32 / 6.0) * b - 2.0 * c,
        -2.0 + (9.0_f32 / 6.0) * b + c,
        0.0,
        0.0,
        -c,
        (1.0_f32 / 6.0) * b + c,
    )
}

/// Column-major weights for `GatherCtx::weights` / `SamplerCtx::weights`.
#[inline]
pub fn cubic_resampler_weights_col_major(b: f32, c: f32) -> [f32; 16] {
    cubic_resampler_matrix(b, c).mat
}

/// Matches `optimize(SkTileMode tm, int dimension)` in `SkImageShader.cpp`.
#[inline]
pub const fn optimize_tile_mode(
    tm: TileMode,
    dimension: i32,
    android_framework: bool,
) -> TileMode {
    if android_framework {
        tm
    } else if !matches!(tm, TileMode::Decal) && dimension == 1 {
        TileMode::Clamp
    } else {
        tm
    }
}

/// Action returned by `classify_make_subset` / `classify_make_raw`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ImageShaderMakeAction {
    ReturnNull = 0,
    ReturnEmpty = 1,
    Create = 2,
}

/// Matches `SkImageShader::MakeSubset` validation logic.
pub fn classify_make_subset(
    has_image: bool,
    img_w: i32,
    img_h: i32,
    subset: Rect,
    sampling: &SamplingOptions,
) -> ImageShaderMakeAction {
    let is_unit = |x: f32| x >= 0.0 && x <= 1.0;
    if sampling.use_cubic && (!is_unit(sampling.cubic.b) || !is_unit(sampling.cubic.c)) {
        return ImageShaderMakeAction::ReturnNull;
    }
    if !has_image || subset.is_empty() {
        return ImageShaderMakeAction::ReturnEmpty;
    }
    let img_bounds = Rect::from_ltrb(0.0, 0.0, img_w as f32, img_h as f32);
    if !img_bounds.contains_rect(subset) {
        return ImageShaderMakeAction::ReturnNull;
    }
    ImageShaderMakeAction::Create
}

/// Matches `SkImageShader::MakeRaw` validation logic.
#[inline]
pub const fn classify_make_raw(has_image: bool, use_cubic: bool) -> ImageShaderMakeAction {
    if use_cubic {
        ImageShaderMakeAction::ReturnNull
    } else if !has_image {
        ImageShaderMakeAction::ReturnEmpty
    } else {
        ImageShaderMakeAction::Create
    }
}

/// Matches `SkImageShader::isOpaque()`.
#[inline]
pub const fn image_shader_is_opaque(img_is_opaque: bool, tmx: TileMode, tmy: TileMode) -> bool {
    img_is_opaque && !matches!(tmx, TileMode::Decal) && !matches!(tmy, TileMode::Decal)
}

/// Matches `SkSamplingPriv::AnisoFallback(bool imageIsMipped)`.
#[inline]
pub const fn aniso_fallback(image_is_mipped: bool) -> SamplingOptions {
    let mm = if image_is_mipped {
        MipmapMode::Linear
    } else {
        MipmapMode::None
    };
    SamplingOptions::new(FilterMode::Linear, mm)
}

/// Matches `tweak_sampling(SkSamplingOptions sampling, const SkMatrix& matrix)` in `SkImageShader.cpp`.
pub fn tweak_sampling(sampling: SamplingOptions, matrix: &Matrix) -> SamplingOptions {
    let mut filter = sampling.filter;
    let ty = matrix.get_type_cached();
    let tx = matrix.mat[2];
    let ty_val = matrix.mat[5];
    // SkMatrix::kTranslate_Mask == 0x01
    if filter == FilterMode::Linear
        && ty <= 0x01
        && tx == (tx as i32) as f32
        && ty_val == (ty_val as i32) as f32
    {
        filter = FilterMode::Nearest;
    }
    SamplingOptions::new(filter, sampling.mipmap)
}

/// Result of geometry calculations for `SkImageShader::MakeForDrawRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct DrawRectGeometry {
    pub valid: bool,
    pub src: Rect,
    pub dst: Rect,
    pub local_matrix: Matrix,
    pub image_is_alpha_only: bool,
}

/// Matches `SkImageShader::MakeForDrawRect` geometry & rect intersection logic.
pub fn compute_draw_rect_geometry(
    img_w: i32,
    img_h: i32,
    img_ct: ColorType,
    mut src: Rect,
    mut dst: Rect,
) -> DrawRectGeometry {
    let img_bounds = Rect::from_ltrb(0.0, 0.0, img_w as f32, img_h as f32);
    let local_matrix = rect_to_rect_or_identity(src, dst);
    if !img_bounds.contains_rect(src) {
        if !src.intersect(img_bounds) {
            return DrawRectGeometry {
                valid: false,
                src: Rect::from_ltrb(0.0, 0.0, 0.0, 0.0),
                dst: Rect::from_ltrb(0.0, 0.0, 0.0, 0.0),
                local_matrix: Matrix::identity(),
                image_is_alpha_only: false,
            };
        }
        // Update dst to match smaller src (SkMatrix::mapRect)
        dst = map_rect_general(&local_matrix, src);
    }
    // kAlpha_SkColorChannelFlag == 0x08
    let image_is_alpha_only = color_type_channel_flags(img_ct) == 0x08;
    DrawRectGeometry {
        valid: true,
        src,
        dst,
        local_matrix,
        image_is_alpha_only,
    }
}

/// Helper matching `SkMatrix::mapRect(const SkRect&)`.
pub fn map_rect_general(m: &Matrix, src: Rect) -> Rect {
    if m.is_scale_translate() {
        m.map_rect_scale_translate(src)
    } else if !m.has_perspective() {
        m.map_rect_non_perspective(src)
            .map(|(r, _)| r)
            .unwrap_or_else(|| Rect::from_ltrb(0.0, 0.0, 0.0, 0.0))
    } else {
        let mut quad = src.to_quad();
        m.map_points_in_place(&mut quad);
        let mut out = Rect::from_ltrb(0.0, 0.0, 0.0, 0.0);
        out.set_bounds_check(&quad);
        out
    }
}

/// Computed parameters for `MipLevelHelper::allocAndInit`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MipLevelInitParams {
    pub width_f: f32,
    pub height_f: f32,
    pub inv_width: f32,
    pub inv_height: f32,
    pub cubic_weights: [f32; 16],
    pub has_cubic_weights: bool,
    pub round_down_at_integer: bool,
    pub mirror_bias_dir: i32,
    pub has_decal: bool,
    pub decal_limit_x: f32,
    pub decal_limit_y: f32,
    pub decal_inclusive_edge_x: f32,
    pub decal_inclusive_edge_y: f32,
}

/// Matches `MipLevelHelper::allocAndInit` in `SkImageShader.cpp`.
pub fn compute_mip_level_init(
    pm_width: i32,
    pm_height: i32,
    sampling: &SamplingOptions,
    tmx: TileMode,
    tmy: TileMode,
) -> MipLevelInitParams {
    let width_f = pm_width as f32;
    let height_f = pm_height as f32;
    let inv_width = 1.0_f32 / width_f;
    let inv_height = 1.0_f32 / height_f;

    let (has_cubic_weights, cubic_weights) = if sampling.use_cubic {
        (true, cubic_resampler_weights_col_major(sampling.cubic.b, sampling.cubic.c))
    } else {
        (false, [0.0; 16])
    };

    let round_down_at_integer = !sampling.use_cubic && sampling.filter == FilterMode::Nearest;
    let mirror_bias_dir = if round_down_at_integer { 1 } else { -1 };

    let has_decal = matches!(tmx, TileMode::Decal) || matches!(tmy, TileMode::Decal);
    let (decal_inclusive_edge_x, decal_inclusive_edge_y) = if has_decal && round_down_at_integer {
        (width_f, height_f)
    } else {
        (0.0, 0.0)
    };

    MipLevelInitParams {
        width_f,
        height_f,
        inv_width,
        inv_height,
        cubic_weights,
        has_cubic_weights,
        round_down_at_integer,
        mirror_bias_dir,
        has_decal,
        decal_limit_x: width_f,
        decal_limit_y: height_f,
        decal_inclusive_edge_x,
        decal_inclusive_edge_y,
    }
}

/// Fast-path classification for `SkImageShader::appendStages`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ImageShaderFastPath {
    None = 0,
    BilerpClamp8888ForceHighP = 1,
    BilerpClamp8888UniformY = 2,
    BilerpClamp8888 = 3,
    BicubicClamp8888 = 4,
}

/// Matches the fast-path decision in `SkImageShader::appendStages`.
pub fn classify_image_fast_path(
    ct: ColorType,
    sampling: &SamplingOptions,
    tmx: TileMode,
    tmy: TileMode,
    dst_bounds: Rect,
    base_inv: &Matrix,
    coords_already_seeded: bool,
    total_matrix_is_valid: bool,
    total_matrix: &Matrix,
    total_inv: &Matrix,
) -> ImageShaderFastPath {
    let is_8888 = matches!(ct, ColorType::Rgba8888 | ColorType::Bgra8888);
    if !is_8888 || tmx != TileMode::Clamp || tmy != TileMode::Clamp {
        return ImageShaderFastPath::None;
    }

    if !sampling.use_cubic
        && sampling.filter == FilterMode::Linear
        && sampling.mipmap != MipmapMode::Linear
    {
        let should_use_highp = should_use_highp_bilerp(dst_bounds, base_inv);
        let uniform_y = !coords_already_seeded
            && total_matrix_is_valid
            && !total_matrix.has_perspective()
            && !total_inv.has_perspective()
            && total_inv.mat[3] == 0.0;

        return if should_use_highp {
            ImageShaderFastPath::BilerpClamp8888ForceHighP
        } else if uniform_y {
            ImageShaderFastPath::BilerpClamp8888UniformY
        } else {
            ImageShaderFastPath::BilerpClamp8888
        };
    }

    if sampling.use_cubic {
        return ImageShaderFastPath::BicubicClamp8888;
    }

    ImageShaderFastPath::None
}

/// Matches the `shouldUseHighPBilerp` check in `SkImageShader::appendStages`.
pub fn should_use_highp_bilerp(dst_bounds: Rect, base_inv: &Matrix) -> bool {
    if dst_bounds.is_empty() {
        return false;
    }
    let mut quad = dst_bounds.to_quad();
    base_inv.map_points_in_place(&mut quad);
    let mut device_image_space = Rect::from_ltrb(0.0, 0.0, 0.0, 0.0);
    device_image_space.set_bounds_check(&quad);
    let vals = [
        device_image_space.left,
        device_image_space.top,
        device_image_space.right,
        device_image_space.bottom,
    ];
    for val in vals {
        if val > (i16::MAX as f32) || val < (i16::MIN as f32) || !val.is_finite() {
            return true;
        }
    }
    false
}

/// Matches the cubic clamp op selection in `SkImageShader::appendStages`:
/// returns `true` for `clamp_01`, `false` for `clamp_gamut`.
#[inline]
pub const fn cubic_uses_clamp_01(at: AlphaType, clamp_as_if_unpremul: bool) -> bool {
    matches!(at, AlphaType::Unpremul) || clamp_as_if_unpremul
}

/// Matches `legacy_shader_can_handle(const SkMatrix& inv)` in `SkImageShader.cpp`.
pub fn legacy_shader_can_handle(inv: &Matrix, has_dxdy_opt: bool) -> bool {
    if !has_dxdy_opt && !inv.is_scale_translate() {
        return false;
    }
    const MAX_DEV_COORD: f32 = 32767.0;
    let src = map_rect_general(inv, Rect::from_ltrb(0.0, 0.0, MAX_DEV_COORD, MAX_DEV_COORD));
    let max_fixed32dot32 = (i32::MAX as f32) * 0.25;
    let limit = Rect::from_ltrb(
        -max_fixed32dot32,
        -max_fixed32dot32,
        max_fixed32dot32,
        max_fixed32dot32,
    );
    limit.contains_rect(src)
}

/// Matches `BitmapProcShaderContext` flag calculation in `SkBitmapProcShader.cpp`.
#[inline]
pub const fn bitmap_proc_shader_flags(pixmap_is_opaque: bool, paint_alpha: u8) -> u32 {
    if pixmap_is_opaque && paint_alpha == 255 {
        OPAQUE_ALPHA_FLAG
    } else {
        0
    }
}

/// Matches `SkImageShader::onMakeContext` eligibility check (under legacy shader context).
pub fn legacy_context_sampling_supported(sampling: &SamplingOptions) -> bool {
    if sampling.use_cubic {
        return false;
    }
    matches!(
        (sampling.filter, sampling.mipmap),
        (FilterMode::Nearest, MipmapMode::None)
            | (FilterMode::Linear, MipmapMode::None)
            | (FilterMode::Linear, MipmapMode::Nearest)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubic_resampler_mitchell_and_catmull() {
        // Mitchell: B = 1/3, C = 1/3
        let m = cubic_resampler_matrix(1.0 / 3.0, 1.0 / 3.0);
        assert!((m.rc(0, 0) - 1.0 / 18.0).abs() < 1e-6);
        assert!((m.rc(0, 1) - (-9.0 / 18.0)).abs() < 1e-6);
        assert!((m.rc(0, 2) - 15.0 / 18.0).abs() < 1e-6);
        assert!((m.rc(0, 3) - (-7.0 / 18.0)).abs() < 1e-6);
        assert!((m.rc(1, 0) - 16.0 / 18.0).abs() < 1e-6);
        assert!((m.rc(3, 3) - 7.0 / 18.0).abs() < 1e-6);

        // Catmull-Rom: B = 0, C = 0.5
        let cr = cubic_resampler_matrix(0.0, 0.5);
        assert_eq!(cr.rc(0, 0), 0.0);
        assert_eq!(cr.rc(0, 1), -0.5);
        assert_eq!(cr.rc(0, 2), 1.0);
        assert_eq!(cr.rc(0, 3), -0.5);
        assert_eq!(cr.rc(1, 0), 1.0);
        assert_eq!(cr.rc(1, 2), -2.5);
        assert_eq!(cr.rc(1, 3), 1.5);
    }

    #[test]
    fn tile_mode_optimize_1px() {
        assert_eq!(optimize_tile_mode(TileMode::Repeat, 1, false), TileMode::Clamp);
        assert_eq!(optimize_tile_mode(TileMode::Mirror, 1, false), TileMode::Clamp);
        assert_eq!(optimize_tile_mode(TileMode::Decal, 1, false), TileMode::Decal);
        assert_eq!(optimize_tile_mode(TileMode::Repeat, 2, false), TileMode::Repeat);
        assert_eq!(optimize_tile_mode(TileMode::Repeat, 1, true), TileMode::Repeat);
    }

    #[test]
    fn tweak_sampling_int_translate() {
        let linear = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        let int_t = Matrix::translate(5.0, -3.0);
        assert_eq!(tweak_sampling(linear, &int_t).filter, FilterMode::Nearest);

        let frac_t = Matrix::translate(5.5, -3.0);
        assert_eq!(tweak_sampling(linear, &frac_t).filter, FilterMode::Linear);

        let scaled = Matrix::scale_translate(2.0, 2.0, 0.0, 0.0);
        assert_eq!(tweak_sampling(linear, &scaled).filter, FilterMode::Linear);
    }

    #[test]
    fn make_subset_and_draw_rect() {
        let linear = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        assert_eq!(
            classify_make_subset(true, 10, 10, Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &linear),
            ImageShaderMakeAction::Create
        );
        assert_eq!(
            classify_make_subset(true, 10, 10, Rect::from_ltrb(-1.0, 0.0, 10.0, 10.0), &linear),
            ImageShaderMakeAction::ReturnNull
        );
        let bad_cubic = SamplingOptions::cubic(1.5, 0.0);
        assert_eq!(
            classify_make_subset(true, 10, 10, Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &bad_cubic),
            ImageShaderMakeAction::ReturnNull
        );

        let geom = compute_draw_rect_geometry(
            10,
            10,
            ColorType::Rgba8888,
            Rect::from_ltrb(-5.0, 0.0, 5.0, 10.0),
            Rect::from_ltrb(0.0, 0.0, 20.0, 20.0),
        );
        assert!(geom.valid);
        assert_eq!(geom.src, Rect::from_ltrb(0.0, 0.0, 5.0, 10.0));
        assert_eq!(geom.dst, Rect::from_ltrb(10.0, 0.0, 20.0, 20.0));
    }

    #[test]
    fn fast_path_highp_and_uniform_y() {
        let linear = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        let id = Matrix::identity();
        let small = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            classify_image_fast_path(
                ColorType::Rgba8888,
                &linear,
                TileMode::Clamp,
                TileMode::Clamp,
                small,
                &id,
                false,
                true,
                &id,
                &id,
            ),
            ImageShaderFastPath::BilerpClamp8888UniformY
        );

        let huge = Rect::from_ltrb(0.0, 0.0, 40000.0, 100.0);
        assert_eq!(
            classify_image_fast_path(
                ColorType::Rgba8888,
                &linear,
                TileMode::Clamp,
                TileMode::Clamp,
                huge,
                &id,
                false,
                true,
                &id,
                &id,
            ),
            ImageShaderFastPath::BilerpClamp8888ForceHighP
        );
    }

    #[test]
    fn mip_level_init_nearest_decal() {
        let nearest = SamplingOptions::new(FilterMode::Nearest, MipmapMode::None);
        let p = compute_mip_level_init(64, 32, &nearest, TileMode::Decal, TileMode::Clamp);
        assert!(p.round_down_at_integer);
        assert_eq!(p.mirror_bias_dir, 1);
        assert!(p.has_decal);
        assert_eq!(p.decal_inclusive_edge_x, 64.0);
        assert_eq!(p.decal_inclusive_edge_y, 32.0);
    }

    #[test]
    fn legacy_helpers_and_flags() {
        assert!(legacy_shader_can_handle(&Matrix::identity(), false));
        assert!(!legacy_shader_can_handle(&Matrix::scale_translate(1e6, 1.0, 0.0, 0.0), true));
        assert!(legacy_context_sampling_supported(&SamplingOptions::new(
            FilterMode::Linear,
            MipmapMode::Nearest,
        )));
        assert!(!legacy_context_sampling_supported(&SamplingOptions::new(
            FilterMode::Linear,
            MipmapMode::Linear,
        )));
        assert!(!legacy_context_sampling_supported(&SamplingOptions::cubic(0.0, 0.5)));
        assert_eq!(bitmap_proc_shader_flags(true, 255), OPAQUE_ALPHA_FLAG);
        assert_eq!(bitmap_proc_shader_flags(true, 128), 0);
        assert!(cubic_uses_clamp_01(AlphaType::Unpremul, false));
        assert!(cubic_uses_clamp_01(AlphaType::Premul, true));
        assert!(!cubic_uses_clamp_01(AlphaType::Premul, false));
        assert_eq!(aniso_fallback(true).mipmap, MipmapMode::Linear);
        assert_eq!(aniso_fallback(false).mipmap, MipmapMode::None);
    }
}
