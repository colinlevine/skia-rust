#![forbid(unsafe_code)]

//! Safe Rust implementations of computational, geometric, validation, bounds, kernel encoding,
//! uniform calculation, and multi-pass planning logic for all 16 effect image filters
//! (`src/effects/imagefilters/*.cpp`, excluding `SkRuntimeImageFilter.cpp`).

use skia_rs_color::blend_mode::{as_coeff, BlendMode, BlendModeCoeff};
use skia_rs_geometry::{
    float_midpoint, pin_f32, scalar_nearly_zero, scalar_round_to_int, IPoint, IRect, ISize,
    Matrix, Point, Point3, Rect, Size, SK_SCALAR_NEARLY_ZERO, SK_SCALAR_PI,
};

use crate::types::{
    inverse_map_irect, irect_adjust, irect_contains_rect, irect_inset, irect_intersect,
    irect_outset, is_valid_rect, make_large_s32, map_irect, matrix_map_rect,
    matrix_rect_to_rect_or_identity, rect_contains_rect, rect_from_irect, rect_intersect,
    rect_make_inset, rect_make_outset, relevant_subset, round_in, round_out, size_ceil, size_round,
    FVector, Mapping, TileMode,
};

// ============================================================================
// 1. SkBlendImageFilter (Blend + Arithmetic)
// ============================================================================

/// Factory simplification for `SkImageFilters::Blend`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendFactoryAction {
    Keep = 0,
    ReturnForeground = 1,
    ReturnBackground = 2,
    ReturnEmpty = 3,
}

#[inline]
pub fn classify_blend_factory(mode: BlendMode) -> BlendFactoryAction {
    match mode {
        BlendMode::Src => BlendFactoryAction::ReturnForeground,
        BlendMode::Dst => BlendFactoryAction::ReturnBackground,
        BlendMode::Clear => BlendFactoryAction::ReturnEmpty,
        _ => BlendFactoryAction::Keep,
    }
}

/// Exact port of `SkBlendImageFilter::onAffectsTransparentBlack()`.
#[inline]
pub fn blend_affects_transparent_black(
    has_blend_mode: bool,
    has_arithmetic_coeffs: bool,
    k3: f32,
) -> bool {
    !has_blend_mode && (!has_arithmetic_coeffs || k3 != 0.0_f32)
}

/// Shortcut decision for `SkBlendImageFilter::makeBlendShader` when at least one input is null.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendNullShaderAction {
    ReturnNull = 0,
    ReturnBg = 1,
    ReturnFg = 2,
    UseBlendShader = 3,
}

/// Exact port of `SkBlendImageFilter::makeBlendShader` null-input logic.
pub fn blend_null_shader_action(
    has_bg: bool,
    has_fg: bool,
    affects_transparent_black: bool,
    blend_mode: Option<BlendMode>,
) -> BlendNullShaderAction {
    if has_bg && has_fg {
        return BlendNullShaderAction::UseBlendShader;
    }
    if !affects_transparent_black && !has_bg && !has_fg {
        return BlendNullShaderAction::ReturnNull;
    }
    if let Some(bm) = blend_mode {
        if let Some((src, dst)) = as_coeff(bm) {
            if has_bg
                && (dst == BlendModeCoeff::One
                    || dst == BlendModeCoeff::ISA
                    || dst == BlendModeCoeff::ISC)
            {
                return BlendNullShaderAction::ReturnBg;
            }
            if has_fg && (src == BlendModeCoeff::One || src == BlendModeCoeff::IDA) {
                return BlendNullShaderAction::ReturnFg;
            }
        }
    }
    BlendNullShaderAction::UseBlendShader
}

/// Output bounds combination rule for `SkBlendImageFilter`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendBoundsRule {
    Unbounded = 0,
    Intersect = 1,
    FgOnly = 2,
    BgOnly = 3,
    Union = 4,
}

/// Exact port of `SkBlendImageFilter::onGetOutputLayerBounds` / `computeFastBounds` classification.
///
/// Note: `onGetOutputLayerBounds` checks `dst == kZero || dst == kSA || dst == kSC`, while
/// `computeFastBounds` checks `dst == kZero || dst == kSA`; `include_sc` distinguishes the two.
pub fn classify_blend_bounds_rule(
    blend_mode: Option<BlendMode>,
    arith_k: Option<[f32; 4]>,
    include_sc: bool,
) -> BlendBoundsRule {
    let mut transparent_outside_fg = false;
    let mut transparent_outside_bg = false;

    if let Some(bm) = blend_mode {
        if let Some((src, dst)) = as_coeff(bm) {
            transparent_outside_fg = dst == BlendModeCoeff::Zero
                || dst == BlendModeCoeff::SA
                || (include_sc && dst == BlendModeCoeff::SC);
            transparent_outside_bg = src == BlendModeCoeff::Zero || src == BlendModeCoeff::DA;
        }
    } else if let Some(k) = arith_k {
        if k[3] != 0.0_f32 {
            return BlendBoundsRule::Unbounded;
        }
        transparent_outside_fg = k[2] == 0.0_f32;
        transparent_outside_bg = k[1] == 0.0_f32;
    } else {
        return BlendBoundsRule::Unbounded;
    }

    match (transparent_outside_fg, transparent_outside_bg) {
        (true, true) => BlendBoundsRule::Intersect,
        (true, false) => BlendBoundsRule::FgOnly,
        (false, true) => BlendBoundsRule::BgOnly,
        (false, false) => BlendBoundsRule::Union,
    }
}

/// Combines optional layer-space output bounds according to `BlendBoundsRule`.
pub fn combine_blend_output_bounds(
    rule: BlendBoundsRule,
    fg: Option<IRect>,
    bg: Option<IRect>,
) -> Option<IRect> {
    match rule {
        BlendBoundsRule::Unbounded => None,
        BlendBoundsRule::FgOnly => fg,
        BlendBoundsRule::BgOnly => bg,
        BlendBoundsRule::Intersect => match (fg, bg) {
            (None, Some(b)) => Some(b),
            (Some(f), Some(b)) => Some(irect_intersect(f, b).unwrap_or_else(IRect::make_empty)),
            (Some(f), None) => Some(f),
            (None, None) => None,
        },
        BlendBoundsRule::Union => match (fg, bg) {
            (Some(f), Some(mut b)) => {
                b.join(f);
                Some(b)
            }
            _ => None,
        },
    }
}

/// Combines fast bounds according to `BlendBoundsRule`.
pub fn combine_blend_fast_bounds(rule: BlendBoundsRule, fg: Rect, bg: Rect) -> Rect {
    match rule {
        BlendBoundsRule::Unbounded => make_large_s32(),
        BlendBoundsRule::FgOnly => fg,
        BlendBoundsRule::BgOnly => bg,
        BlendBoundsRule::Intersect => rect_intersect(fg, bg).unwrap_or_else(Rect::make_empty),
        BlendBoundsRule::Union => {
            let mut out = bg;
            out.join(fg);
            out
        }
    }
}

// ============================================================================
// 2. SkBlurImageFilter
// ============================================================================

/// Maximum blur sigma (`SkBlurImageFilter.cpp`: `kMaxSigma = 532.f`).
pub const K_MAX_BLUR_SIGMA: f32 = 532.0_f32;

/// Exact match for `SkBlurEngine::IsEffectivelyIdentity(float sigma)`.
#[inline]
pub fn blur_is_effectively_identity(sigma: f32) -> bool {
    sigma <= 0.03_f32
}

/// Exact port of `SkImageFilters::Blur` parameter validation.
#[inline]
pub fn validate_blur_params(sigma_x: f32, sigma_y: f32) -> bool {
    sigma_x.is_finite() && sigma_y.is_finite() && sigma_x >= 0.0_f32 && sigma_y >= 0.0_f32
}

/// Exact port of `SkBlurImageFilter::mapSigma(mapping)`.
pub fn map_blur_sigma(mapping: &Mapping, param_sigma: Size) -> Size {
    let mapped = mapping.param_to_layer_size(param_sigma);
    let mut sx = if K_MAX_BLUR_SIGMA < mapped.width {
        K_MAX_BLUR_SIGMA
    } else {
        mapped.width
    };
    let mut sy = if K_MAX_BLUR_SIGMA < mapped.height {
        K_MAX_BLUR_SIGMA
    } else {
        mapped.height
    };
    if !sx.is_finite() || blur_is_effectively_identity(sx) {
        sx = 0.0_f32;
    }
    if !sy.is_finite() || blur_is_effectively_identity(sy) {
        sy = 0.0_f32;
    }
    Size {
        width: sx,
        height: sy,
    }
}

/// Exact port of `SkBlurImageFilter::kernelBounds(mapping, bounds)`.
pub fn blur_kernel_bounds(mapping: &Mapping, param_sigma: Size, bounds: IRect) -> IRect {
    let sigma = map_blur_sigma(mapping, param_sigma);
    let radii = size_ceil(Size {
        width: 3.0_f32 * sigma.width,
        height: 3.0_f32 * sigma.height,
    });
    irect_outset(bounds, radii.width, radii.height)
}

/// Exact port of `SkBlurImageFilter::computeFastBounds`.
#[inline]
pub fn blur_fast_bounds(child_bounds: Rect, param_sigma: Size) -> Rect {
    rect_make_outset(
        child_bounds,
        param_sigma.width * 3.0_f32,
        param_sigma.height * 3.0_f32,
    )
}

// ============================================================================
// 3. SkCropImageFilter (Crop / Tile / Empty)
// ============================================================================

/// Exact port of `SkImageFilters::Crop` rect validation.
#[inline]
pub fn validate_crop_rect(rect: Rect) -> bool {
    is_valid_rect(rect)
}

/// Exact port of `SkCropImageFilter::cropRect(mapping)`.
#[inline]
pub fn crop_layer_rect(mapping: &Mapping, param_crop: Rect, tile_mode: TileMode) -> IRect {
    let crop = mapping.param_to_layer_rect(param_crop);
    if tile_mode == TileMode::Decal {
        round_out(crop)
    } else {
        round_in(crop)
    }
}

/// Exact port of `SkCropImageFilter::requiredInput(mapping, outputBounds)`.
#[inline]
pub fn crop_required_input(
    mapping: &Mapping,
    param_crop: Rect,
    tile_mode: TileMode,
    output_bounds: IRect,
) -> IRect {
    relevant_subset(
        crop_layer_rect(mapping, param_crop, tile_mode),
        output_bounds,
        tile_mode,
    )
}

/// Exact port of `SkCropImageFilter::onGetOutputLayerBounds`.
pub fn crop_output_layer_bounds(
    mapping: &Mapping,
    param_crop: Rect,
    tile_mode: TileMode,
    child_output: Option<IRect>,
) -> Option<IRect> {
    let mut crop = crop_layer_rect(mapping, param_crop, tile_mode);
    if let Some(child) = child_output {
        if let Some(inter) = irect_intersect(crop, child) {
            crop = inter;
        } else {
            return Some(IRect::make_empty());
        }
    }
    if tile_mode == TileMode::Decal {
        Some(crop)
    } else {
        None
    }
}

/// Exact port of `SkCropImageFilter::computeFastBounds`.
pub fn crop_fast_bounds(input_bounds: Rect, param_crop: Rect, tile_mode: TileMode) -> Rect {
    let Some(inter) = rect_intersect(input_bounds, param_crop) else {
        return Rect::make_empty();
    };
    if tile_mode == TileMode::Decal {
        inter
    } else {
        make_large_s32()
    }
}

// ============================================================================
// 4. SkDisplacementMapImageFilter
// ============================================================================

/// Exact port of `channel_selector_type_is_valid`.
#[inline]
pub fn displacement_channel_is_valid(channel: i32) -> bool {
    (0..=3).contains(&channel)
}

/// Exact port of `SkImageFilters::DisplacementMap` validation.
#[inline]
pub fn validate_displacement_params(x_channel: i32, y_channel: i32, scale: f32) -> bool {
    displacement_channel_is_valid(x_channel)
        && displacement_channel_is_valid(y_channel)
        && scale.is_finite()
}

/// Exact port of `channelSelector` uniform helper in `make_displacement_shader`.
#[inline]
pub fn displacement_channel_selector(channel: i32) -> [f32; 4] {
    [
        if channel == 0 { 1.0 } else { 0.0 },
        if channel == 1 { 1.0 } else { 0.0 },
        if channel == 2 { 1.0 } else { 0.0 },
        if channel == 3 { 1.0 } else { 0.0 },
    ]
}

/// Exact port of `SkDisplacementMapImageFilter::outsetByMaxDisplacement`.
pub fn displacement_outset_bounds(mapping: &Mapping, scale: f32, bounds: IRect) -> IRect {
    let max_disp = mapping.param_to_layer_size(Size {
        width: 0.5_f32 * scale,
        height: 0.5_f32 * scale,
    });
    let ceil_disp = size_ceil(max_disp);
    irect_outset(bounds, ceil_disp.width, ceil_disp.height)
}

/// Exact port of layer-space displacement vector scale.
#[inline]
pub fn displacement_layer_scale(mapping: &Mapping, scale: f32) -> FVector {
    mapping.param_to_layer_vector(FVector { x: scale, y: scale })
}

/// Exact port of `SkDisplacementMapImageFilter::computeFastBounds`.
#[inline]
pub fn displacement_fast_bounds(color_bounds: Rect, scale: f32) -> Rect {
    let max_disp = 0.5_f32 * scale.abs();
    rect_make_outset(color_bounds, max_disp, max_disp)
}

// ============================================================================
// 5. SkImageImageFilter
// ============================================================================

/// Exact port of `SkImageFilters::Image` src/dst rect clamping & validation.
/// Returns `Some((adjusted_src, adjusted_dst))` or `None` if empty.
pub fn adjust_image_filter_rects(
    image_dims: ISize,
    src_rect: Rect,
    dst_rect: Rect,
) -> Option<(Rect, Rect)> {
    if src_rect.is_empty() || dst_rect.is_empty() {
        return None;
    }
    let image_bounds = Rect {
        left: 0.0,
        top: 0.0,
        right: image_dims.width as f32,
        bottom: image_dims.height as f32,
    };
    if rect_contains_rect(image_bounds, src_rect) {
        Some((src_rect, dst_rect))
    } else {
        let src_to_dst = matrix_rect_to_rect_or_identity(src_rect, dst_rect);
        let inter = rect_intersect(image_bounds, src_rect)?;
        let mapped = matrix_map_rect(&src_to_dst, inter);
        if mapped.is_empty() {
            None
        } else {
            Some((inter, mapped))
        }
    }
}

// ============================================================================
// 6. SkLightingImageFilter
// ============================================================================

/// Matches `Light::Type` in `SkLightingImageFilter.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LightType {
    Distant = 0,
    Point = 1,
    Spot = 2,
}

/// Matches `Material::Type` in `SkLightingImageFilter.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MaterialType {
    Diffuse = 0,
    Specular = 1,
    EmbossSpecular = 2,
}

/// Exact port of `LayerSpace<ZValue>::Map(mapping, z)`.
#[inline]
pub fn map_z_to_layer(mapping: &Mapping, z: f32) -> f32 {
    let z2d = mapping.param_to_layer_vector(FVector { x: z, y: z });
    float_midpoint(z2d.x, z2d.y)
}

/// Exact port of `make_lighting` validation in `SkLightingImageFilter.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn validate_lighting_params(
    k: f32,
    shininess: f32,
    surface_depth: f32,
    loc_xy: Point,
    loc_z: f32,
    dir_xy: FVector,
    dir_z: f32,
    falloff_exponent: f32,
    cos_cutoff_angle: f32,
) -> bool {
    if !(k.is_finite() && shininess.is_finite() && surface_depth.is_finite()) || k < 0.0_f32 {
        return false;
    }
    if !(loc_xy.x.is_finite()
        && loc_xy.y.is_finite()
        && dir_xy.is_finite()
        && falloff_exponent.is_finite()
        && cos_cutoff_angle.is_finite()
        && loc_z.is_finite()
        && dir_z.is_finite())
        || cos_cutoff_angle < -1.0_f32
        || cos_cutoff_angle > 1.0_f32
    {
        return false;
    }
    true
}

/// Exact port of `SpotLitDiffuse` / `SpotLitSpecular` direction & cosine cutoff calculation.
#[inline]
pub fn spot_light_dir_and_cos_cutoff(
    location: Point3,
    target: Point3,
    cutoff_angle_deg: f32,
) -> (Point3, f32) {
    let dir = Point3 {
        x: target.x - location.x,
        y: target.y - location.y,
        z: target.z - location.z,
    };
    let rad = cutoff_angle_deg * (SK_SCALAR_PI / 180.0_f32);
    (dir, rad.cos())
}

/// Uniform payload for `make_normal_shader` and `make_lighting_shader`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct LightingUniforms {
    pub normal_edge_bounds: Rect,
    pub neg_surface_depth: f32,
    pub material_and_light_type: [f32; 4],
    pub light_pos_and_spot_falloff: [f32; 4],
    pub light_dir_and_spot_cutoff: [f32; 4],
    pub light_color: [f32; 3],
}

/// Exact port of `SkLightingImageFilter::onFilterImage` clamp-rect calculation.
pub fn lighting_clamp_rect(
    child_layer_bounds: IRect,
    required_input: IRect,
    desired_output: IRect,
) -> IRect {
    if irect_contains_rect(child_layer_bounds, required_input) {
        return required_input;
    }
    let edge_clamp = |actual: i32, requested: i32, output: i32| {
        if actual == output {
            output
        } else {
            requested
        }
    };
    IRect {
        left: edge_clamp(child_layer_bounds.left, required_input.left, desired_output.left),
        top: edge_clamp(child_layer_bounds.top, required_input.top, desired_output.top),
        right: edge_clamp(
            child_layer_bounds.right,
            required_input.right,
            desired_output.right,
        ),
        bottom: edge_clamp(
            child_layer_bounds.bottom,
            required_input.bottom,
            desired_output.bottom,
        ),
    }
}

/// Exact port of `SkLightingImageFilter` layer-space parameter mapping & uniform assembly.
#[allow(clippy::too_many_arguments)]
pub fn compute_lighting_uniforms(
    mapping: &Mapping,
    clamp_rect: IRect,
    light_type: LightType,
    light_color_argb: u32,
    loc_xy: Point,
    loc_z: f32,
    dir_xy: FVector,
    dir_z: f32,
    falloff_exponent: f32,
    cos_cutoff_angle: f32,
    mat_type: MaterialType,
    surface_depth: f32,
    k: f32,
    shininess: f32,
) -> LightingUniforms {
    let layer_surface_depth = map_z_to_layer(mapping, surface_depth);
    let layer_loc_xy = mapping.param_to_layer_point(loc_xy);
    let layer_loc_z = map_z_to_layer(mapping, loc_z);
    let layer_dir_xy = mapping.param_to_layer_vector(dir_xy);
    let layer_dir_z = map_z_to_layer(mapping, dir_z);

    let light_type_f = match light_type {
        LightType::Point => 0.0_f32,
        LightType::Distant => -1.0_f32,
        LightType::Spot => 1.0_f32,
    };

    let dir_len = (layer_dir_xy.x * layer_dir_xy.x
        + layer_dir_xy.y * layer_dir_xy.y
        + layer_dir_z * layer_dir_z)
        .sqrt();
    let inv_dir_len = if dir_len != 0.0_f32 {
        1.0_f32 / dir_len
    } else {
        0.0_f32
    };

    let color_scale = k / 255.0_f32;
    let r = ((light_color_argb >> 16) & 0xFF) as f32;
    let g = ((light_color_argb >> 8) & 0xFF) as f32;
    let b = (light_color_argb & 0xFF) as f32;

    LightingUniforms {
        normal_edge_bounds: rect_make_inset(rect_from_irect(clamp_rect), 0.5_f32, 0.5_f32),
        neg_surface_depth: -layer_surface_depth,
        material_and_light_type: [
            layer_surface_depth,
            shininess,
            (mat_type as i32) as f32,
            light_type_f,
        ],
        light_pos_and_spot_falloff: [
            layer_loc_xy.x,
            layer_loc_xy.y,
            layer_loc_z,
            falloff_exponent,
        ],
        light_dir_and_spot_cutoff: [
            inv_dir_len * layer_dir_xy.x,
            inv_dir_len * layer_dir_xy.y,
            inv_dir_len * layer_dir_z,
            cos_cutoff_angle,
        ],
        light_color: [r * color_scale, g * color_scale, b * color_scale],
    }
}

/// Exact port of `SkLightingImageFilter::requiredInput`.
#[inline]
pub fn lighting_required_input(desired_output: IRect) -> IRect {
    irect_outset(desired_output, 1, 1)
}

// ============================================================================
// 7. SkMagnifierImageFilter
// ============================================================================

/// Validation outcome for `SkImageFilters::Magnifier`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MagnifierValidation {
    Invalid = 0,
    NoOp = 1,
    Valid = 2,
}

/// Exact port of `SkImageFilters::Magnifier` parameter validation.
pub fn validate_magnifier_params(
    lens_bounds: Rect,
    zoom_amount: f32,
    inset: f32,
) -> MagnifierValidation {
    if lens_bounds.is_empty()
        || !lens_bounds.is_finite()
        || zoom_amount <= 0.0_f32
        || inset < 0.0_f32
        || !(zoom_amount.is_finite() && inset.is_finite())
    {
        return MagnifierValidation::Invalid;
    }
    if zoom_amount > 1.0_f32 {
        MagnifierValidation::Valid
    } else {
        MagnifierValidation::NoOp
    }
}

/// Action returned by `plan_magnifier_filter`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MagnifierFilterAction {
    Empty = 0,
    DirectTransform = 1,
    Shader = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MagnifierFilterPlan {
    pub action: MagnifierFilterAction,
    pub lens_bounds: Rect,
    pub lens_bounds_round_out: IRect,
    pub src_rect_round_out: IRect,
    pub expected_child_round_out: IRect,
    pub inv_zoom_xform: Matrix,
    pub zoom_xform_uniform: [f32; 4], // [Tx, Ty, Sx, Sy]
    pub inv_inset_uniform: [f32; 2],
}

/// Exact port of `SkMagnifierImageFilter::onFilterImage` geometry & uniform calculation.
pub fn plan_magnifier_filter(
    mapping: &Mapping,
    desired_output: IRect,
    param_lens_bounds: Rect,
    zoom_amount: f32,
    param_inset: f32,
    child_output_bounds: Option<IRect>,
) -> MagnifierFilterPlan {
    let empty = MagnifierFilterPlan {
        action: MagnifierFilterAction::Empty,
        lens_bounds: Rect::make_empty(),
        lens_bounds_round_out: IRect::make_empty(),
        src_rect_round_out: IRect::make_empty(),
        expected_child_round_out: IRect::make_empty(),
        inv_zoom_xform: Matrix::identity(),
        zoom_xform_uniform: [0.0; 4],
        inv_inset_uniform: [0.0; 2],
    };

    let lens_bounds = mapping.param_to_layer_rect(param_lens_bounds);
    let mut zoom_center = Point::new(lens_bounds.center_x(), lens_bounds.center_y());

    let Some(visible_lens_bounds) = rect_intersect(lens_bounds, rect_from_irect(desired_output))
    else {
        return empty;
    };

    let expected_child_output = match child_output_bounds {
        Some(out) => rect_from_irect(out),
        None => lens_bounds,
    };

    zoom_center = Point {
        x: pin_f32(
            zoom_center.x,
            expected_child_output.left,
            expected_child_output.right,
        ),
        y: pin_f32(
            zoom_center.y,
            expected_child_output.top,
            expected_child_output.bottom,
        ),
    };

    let max_dim = if lens_bounds.width() < lens_bounds.height() {
        lens_bounds.height()
    } else {
        lens_bounds.width()
    };
    let max_lens_size = if 1.0_f32 < max_dim { max_dim } else { 1.0_f32 };
    let clamped_zoom = if 2.0_f32 * max_lens_size < zoom_amount {
        2.0_f32 * max_lens_size
    } else {
        zoom_amount
    };
    let inv_zoom = 1.0_f32 / clamped_zoom;

    let mut src_rect = Rect {
        left: lens_bounds.left * inv_zoom + zoom_center.x * (1.0_f32 - inv_zoom),
        top: lens_bounds.top * inv_zoom + zoom_center.y * (1.0_f32 - inv_zoom),
        right: lens_bounds.right * inv_zoom + zoom_center.x * (1.0_f32 - inv_zoom),
        bottom: lens_bounds.bottom * inv_zoom + zoom_center.y * (1.0_f32 - inv_zoom),
    };

    let mut zoom_xform = matrix_rect_to_rect_or_identity(lens_bounds, src_rect);
    if !rect_contains_rect(expected_child_output, visible_lens_bounds) {
        src_rect = matrix_map_rect(&zoom_xform, visible_lens_bounds);
        if expected_child_output.width() >= src_rect.width()
            && expected_child_output.height() >= src_rect.height()
        {
            let left = if src_rect.left < expected_child_output.left {
                expected_child_output.left
            } else {
                let min_r = if expected_child_output.right < src_rect.right {
                    expected_child_output.right
                } else {
                    src_rect.right
                };
                min_r - src_rect.width()
            };
            let top = if src_rect.top < expected_child_output.top {
                expected_child_output.top
            } else {
                let min_b = if expected_child_output.bottom < src_rect.bottom {
                    expected_child_output.bottom
                } else {
                    src_rect.bottom
                };
                min_b - src_rect.height()
            };
            src_rect = Rect {
                left,
                top,
                right: left + src_rect.width(),
                bottom: top + src_rect.height(),
            };
            zoom_xform = matrix_rect_to_rect_or_identity(visible_lens_bounds, src_rect);
        }
    }

    let inset = mapping.param_to_layer_size(Size {
        width: param_inset,
        height: param_inset,
    });
    let lens_bounds_round_out = round_out(lens_bounds);
    if inset.width <= 0.0_f32 || inset.height <= 0.0_f32 {
        let Some(inv_zoom_xform) = zoom_xform.invert() else {
            return empty;
        };
        return MagnifierFilterPlan {
            action: MagnifierFilterAction::DirectTransform,
            lens_bounds,
            lens_bounds_round_out,
            src_rect_round_out: round_out(src_rect),
            expected_child_round_out: round_out(expected_child_output),
            inv_zoom_xform,
            zoom_xform_uniform: [0.0; 4],
            inv_inset_uniform: [0.0; 2],
        };
    }

    MagnifierFilterPlan {
        action: MagnifierFilterAction::Shader,
        lens_bounds,
        lens_bounds_round_out,
        src_rect_round_out: round_out(src_rect),
        expected_child_round_out: round_out(expected_child_output),
        inv_zoom_xform: Matrix::identity(),
        zoom_xform_uniform: [
            zoom_xform.mat[2],
            zoom_xform.mat[5],
            zoom_xform.mat[0],
            zoom_xform.mat[4],
        ],
        inv_inset_uniform: [1.0_f32 / inset.width, 1.0_f32 / inset.height],
    }
}

/// Exact port of `SkMagnifierImageFilter::onGetOutputLayerBounds`.
pub fn magnifier_output_bounds(
    mapping: &Mapping,
    param_lens_bounds: Rect,
    child_output: Option<IRect>,
) -> IRect {
    let lens_bounds = round_out(mapping.param_to_layer_rect(param_lens_bounds));
    match child_output {
        None => lens_bounds,
        Some(out) => irect_intersect(lens_bounds, out).unwrap_or_else(IRect::make_empty),
    }
}

/// Exact port of `SkMagnifierImageFilter::computeFastBounds`.
#[inline]
pub fn magnifier_fast_bounds(child_bounds: Rect, param_lens_bounds: Rect) -> Rect {
    rect_intersect(child_bounds, param_lens_bounds).unwrap_or_else(Rect::make_empty)
}

// ============================================================================
// 8. SkMatrixConvolutionImageFilter
// ============================================================================

pub const K_LARGE_KERNEL_SIZE: i32 = 256;
pub const K_SMALL_KERNEL_SIZE: i32 = 64;
pub const K_MAX_UNIFORM_KERNEL_SIZE: i32 = 28;

/// Kernel storage tier matching `quantize_by_kernel_size`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatrixConvKernelTier {
    Uniforms = 0,
    TexSm = 1,
    TexLg = 2,
}

/// Exact port of `quantize_by_kernel_size(int kernelSize)`.
#[inline]
pub fn quantize_matrix_conv_kernel(kernel_length: i32) -> (i32, MatrixConvKernelTier) {
    if kernel_length < K_MAX_UNIFORM_KERNEL_SIZE {
        (K_MAX_UNIFORM_KERNEL_SIZE, MatrixConvKernelTier::Uniforms)
    } else if kernel_length <= K_SMALL_KERNEL_SIZE {
        (K_SMALL_KERNEL_SIZE, MatrixConvKernelTier::TexSm)
    } else {
        (K_LARGE_KERNEL_SIZE, MatrixConvKernelTier::TexLg)
    }
}

/// Exact port of `SkImageFilters::MatrixConvolution` parameter validation.
pub fn validate_matrix_convolution_params(
    kernel_size: ISize,
    has_kernel: bool,
    kernel_offset: IPoint,
) -> bool {
    if kernel_size.width < 1 || kernel_size.height < 1 {
        return false;
    }
    let area = (kernel_size.width as i64).checked_mul(kernel_size.height as i64);
    match area {
        Some(a) if a <= K_LARGE_KERNEL_SIZE as i64 => {}
        _ => return false,
    }
    if !has_kernel {
        return false;
    }
    if kernel_offset.x < 0
        || kernel_offset.x >= kernel_size.width
        || kernel_offset.y < 0
        || kernel_offset.y >= kernel_size.height
    {
        return false;
    }
    true
}

/// Exact port of `create_kernel_bitmap` normalization and A8 encoding.
/// Writes `quantized_size` A8 bytes into `out_pixels`, returning `(inner_gain, inner_bias)`.
pub fn encode_matrix_conv_kernel_a8(kernel: &[f32], out_pixels: &mut [u8]) -> (f32, f32) {
    if kernel.is_empty() {
        return (1.0_f32, 0.0_f32);
    }
    let mut min_v = kernel[0];
    let mut max_v = kernel[0];
    for &v in &kernel[1..] {
        if v < min_v {
            min_v = v;
        }
        if v > max_v {
            max_v = v;
        }
    }
    let mut inner_gain = max_v - min_v;
    let inner_bias = min_v;
    if scalar_nearly_zero(inner_gain, SK_SCALAR_NEARLY_ZERO) {
        inner_gain = 1.0_f32;
    }
    let len = kernel.len().min(out_pixels.len());
    for i in 0..len {
        let q = scalar_round_to_int(255.0_f32 * (kernel[i] - min_v) / inner_gain);
        out_pixels[i] = q as u8;
    }
    for px in &mut out_pixels[len..] {
        *px = 0;
    }
    (inner_gain, inner_bias)
}

/// Exact port of `SkMatrixConvolutionImageFilter::boundsSampledByKernel`.
#[inline]
pub fn matrix_conv_bounds_sampled(bounds: IRect, kernel_size: ISize, kernel_offset: IPoint) -> IRect {
    irect_adjust(
        bounds,
        -kernel_offset.x,
        -kernel_offset.y,
        kernel_size.width - kernel_offset.x - 1,
        kernel_size.height - kernel_offset.y - 1,
    )
}

/// Exact port of `SkMatrixConvolutionImageFilter::boundsAffectedByKernel`.
#[inline]
pub fn matrix_conv_bounds_affected(
    bounds: IRect,
    kernel_size: ISize,
    kernel_offset: IPoint,
) -> IRect {
    irect_adjust(
        bounds,
        kernel_offset.x - kernel_size.width + 1,
        kernel_offset.y - kernel_size.height + 1,
        kernel_offset.x,
        kernel_offset.y,
    )
}

/// Exact port of `gainAndBias` uniform scaling (`{fGain, fBias / 255.f}`).
#[inline]
pub fn matrix_conv_gain_and_bias(gain: f32, bias: f32) -> [f32; 2] {
    [gain, bias / 255.0_f32]
}

// ============================================================================
// 9. SkMatrixTransformImageFilter
// ============================================================================

/// Exact port of `SkMatrixTransformImageFilter::requiredInput`.
pub fn matrix_transform_required_input(
    mapping: &Mapping,
    param_transform: &Matrix,
    desired_output: IRect,
    is_default_nearest_sampling: bool,
) -> IRect {
    let layer_xform = mapping.param_to_layer_matrix(param_transform);
    let Some(mut required) = inverse_map_irect(&layer_xform, desired_output) else {
        return IRect::make_empty();
    };
    if !is_default_nearest_sampling {
        required = irect_outset(required, 1, 1);
    }
    required
}

/// Exact port of `SkMatrixTransformImageFilter::onGetOutputLayerBounds`.
pub fn matrix_transform_output_bounds(
    mapping: &Mapping,
    param_transform: &Matrix,
    child_output: Option<IRect>,
) -> Option<IRect> {
    let child = child_output?;
    let layer_xform = mapping.param_to_layer_matrix(param_transform);
    Some(map_irect(child, &layer_xform))
}

// ============================================================================
// 10. SkMorphologyImageFilter
// ============================================================================

/// Matches `MorphType` in `SkMorphologyImageFilter.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MorphType {
    Erode = 0,
    Dilate = 1,
}

/// Matches `MorphDirection` in `SkMorphologyImageFilter.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MorphDirection {
    X = 0,
    Y = 1,
}

pub const K_MAX_MORPH_RADII: i32 = 256;
pub const K_MAX_LINEAR_MORPH_RADIUS: i32 = 14;

/// Validation outcome for `make_morphology`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MorphValidation {
    Invalid = 0,
    Identity = 1,
    Valid = 2,
}

/// Exact port of `make_morphology` parameter validation.
#[inline]
pub fn validate_morphology_params(radii: Size) -> MorphValidation {
    if radii.width < 0.0_f32 || radii.height < 0.0_f32 {
        MorphValidation::Invalid
    } else if radii.width > 0.0_f32 || radii.height > 0.0_f32 {
        MorphValidation::Valid
    } else {
        MorphValidation::Identity
    }
}

/// Exact port of `SkMorphologyImageFilter::radii(mapping)`.
#[inline]
pub fn morphology_map_radii(mapping: &Mapping, param_radii: Size) -> ISize {
    let r = size_round(mapping.param_to_layer_size(param_radii));
    ISize {
        width: r.width.min(K_MAX_MORPH_RADII),
        height: r.height.min(K_MAX_MORPH_RADII),
    }
}

/// Exact port of `SkMorphologyImageFilter::requiredInput`.
#[inline]
pub fn morphology_required_input(mapping: &Mapping, param_radii: Size, bounds: IRect) -> IRect {
    let r = morphology_map_radii(mapping, param_radii);
    irect_outset(bounds, r.width, r.height)
}

/// Exact port of `SkMorphologyImageFilter::kernelOutputBounds`.
#[inline]
pub fn morphology_kernel_output_bounds(
    morph_type: MorphType,
    mapping: &Mapping,
    param_radii: Size,
    bounds: IRect,
) -> IRect {
    let r = morphology_map_radii(mapping, param_radii);
    if morph_type == MorphType::Dilate {
        irect_outset(bounds, r.width, r.height)
    } else {
        irect_inset(bounds, r.width, r.height)
    }
}

/// Exact port of `SkMorphologyImageFilter::computeFastBounds`.
#[inline]
pub fn morphology_fast_bounds(morph_type: MorphType, param_radii: Size, child_bounds: Rect) -> Rect {
    if morph_type == MorphType::Dilate {
        rect_make_outset(child_bounds, param_radii.width, param_radii.height)
    } else {
        rect_make_inset(child_bounds, param_radii.width, param_radii.height)
    }
}

/// One pass of `morphology_pass` (either linear or sparse doubling).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MorphPassStep {
    pub is_linear: bool,
    pub step_radius: i32,
    pub sample_bounds: IRect,
    pub step_desired_output: IRect,
}

/// Exact port of the multi-pass linear + sparse doubling schedule in `morphology_pass`.
/// Since `K_MAX_MORPH_RADII == 256` and `K_MAX_LINEAR_MORPH_RADIUS == 14`, at most 6 passes
/// are ever needed (`14 -> 28 -> 56 -> 112 -> 224 -> 256`).
pub fn plan_morphology_passes(
    dir: MorphDirection,
    radius: i32,
    desired_output: IRect,
) -> Vec<MorphPassStep> {
    let mut steps = Vec::new();
    if radius <= 0 {
        return steps;
    }

    let axis_delta = |step: i32| -> (i32, i32) {
        if dir == MorphDirection::X {
            (step, 0)
        } else {
            (0, step)
        }
    };

    let (dx0, dy0) = axis_delta(radius);
    let mut sample_bounds = irect_outset(desired_output, dx0, dy0);
    let mut applied_radius = 0;

    while radius > applied_radius {
        let is_linear = applied_radius == 0;
        let step_radius = if is_linear {
            K_MAX_LINEAR_MORPH_RADIUS.min(radius)
        } else {
            (radius - applied_radius).min(applied_radius)
        };

        let step_desired_output = if applied_radius + step_radius < radius {
            let (dx, dy) = axis_delta(step_radius);
            irect_inset(sample_bounds, dx, dy)
        } else {
            desired_output
        };

        steps.push(MorphPassStep {
            is_linear,
            step_radius,
            sample_bounds,
            step_desired_output,
        });

        sample_bounds = step_desired_output;
        applied_radius += step_radius;
    }

    steps
}

// ============================================================================
// 11. SkPictureImageFilter
// ============================================================================

/// Exact port of `SkImageFilters::Picture` cull rect intersection.
#[inline]
pub fn picture_intersect_cull_rect(pic_cull: Rect, target_rect: Rect) -> Option<Rect> {
    rect_intersect(pic_cull, target_rect)
}
