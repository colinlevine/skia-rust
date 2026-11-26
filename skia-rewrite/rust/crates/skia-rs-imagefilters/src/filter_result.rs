#![forbid(unsafe_code)]

//! Safe Rust implementation of `skif::FilterResult` and `skif::FilterResult::Builder`
//! computational, geometric, bounds analysis, crop/transform/color-filter/resolve/shader/rescale/blur
//! planning logic (ported from `src/core/SkImageFilterTypes.cpp`).

use skia_rs_geometry::{
    scalar_nearly_equal, IPoint, IRect, ISize, Matrix, Rect, Size, M44,
};

use crate::types::{
    are_axes_nearly_integer_aligned, bounds_analysis, decompose_transform, float_ceil2int,
    ieee_float_divide, inverse_map_irect, inverse_map_rect, irect_contains_rect, irect_inset,
    irect_intersect, irect_is_empty64, irect_make_xywh, irect_outset,
    is_nearly_integer_translation, m44_as_m33, m44_from_m33, map_irect, matrix_map_rect,
    matrix_rect_to_rect_or_identity, next_log2, periodic_axis_transform, rect_contains_rect,
    rect_from_irect, rect_intersect, rect_make_inset, rect_make_outset, relevant_subset,
    round_out, shader_flags, size_ceil, BoundsScope, Mapping, PixelBoundary, SamplingOptions,
    TileMode, K_LAYER_PADDING, K_ROUND_EPSILON,
};

/// Metadata describing a `FilterResult`'s non-pointer state for Rust analysis.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FilterResultMeta {
    pub has_image: bool,
    pub boundary: PixelBoundary,
    pub sampling: SamplingOptions,
    pub tile_mode: TileMode,
    pub transform: Matrix,
    pub has_color_filter: bool,
    pub cf_affects_transparent_black: bool,
    pub layer_bounds: IRect,
    pub image_dims: ISize,
    pub image_subset: IRect,
    pub backing_dims: ISize,
}

impl FilterResultMeta {
    /// Exact match for `FilterResult::canClampToTransparentBoundary(analysis)`.
    #[inline]
    pub fn can_clamp_to_transparent_boundary(&self, analysis: u32) -> bool {
        self.tile_mode == TileMode::Decal
            && self.boundary == PixelBoundary::Transparent
            && (analysis & bounds_analysis::REQUIRES_DECAL_IN_LAYER_SPACE) == 0
    }
}

/// Exact port of `FilterResult::AutoSurface` padding & overflow check.
/// Returns `Some(padded_bounds)` if valid, or `None` if empty or saturated.
pub fn auto_surface_padded_bounds(dst_bounds: IRect, boundary: PixelBoundary) -> Option<IRect> {
    if irect_is_empty64(dst_bounds) {
        return None;
    }
    let padding = if boundary == PixelBoundary::Unknown {
        0
    } else {
        K_LAYER_PADDING
    };
    if padding == 0 {
        return Some(dst_bounds);
    }
    let padded = irect_outset(dst_bounds, padding, padding);
    if padded.left >= dst_bounds.left
        || padded.right <= dst_bounds.right
        || padded.top >= dst_bounds.top
        || padded.bottom <= dst_bounds.bottom
    {
        return None;
    }
    Some(padded)
}

/// Exact port of `compatible_sampling(...)` in `SkImageFilterTypes.cpp`.
pub fn compatible_sampling(
    current_sampling: SamplingOptions,
    current_xform_wont_affect_nearest: bool,
    mut next_sampling: SamplingOptions,
    next_xform_wont_affect_nearest: bool,
) -> (bool, SamplingOptions) {
    const FILTER_NEAREST: i32 = 0;
    const FILTER_LINEAR: i32 = 1;

    if current_sampling.is_aniso() && next_sampling.is_aniso() {
        let max_a = if current_sampling.max_aniso > next_sampling.max_aniso {
            current_sampling.max_aniso
        } else {
            next_sampling.max_aniso
        };
        next_sampling = SamplingOptions::aniso(max_a);
        (true, next_sampling)
    } else if current_sampling.is_aniso() && next_sampling.filter == FILTER_LINEAR {
        (true, current_sampling)
    } else if next_sampling.is_aniso() && current_sampling.filter == FILTER_LINEAR {
        (true, next_sampling)
    } else if current_sampling.use_cubic
        && (next_sampling.filter == FILTER_LINEAR
            || (next_sampling.use_cubic
                && current_sampling.cubic_b == next_sampling.cubic_b
                && current_sampling.cubic_c == next_sampling.cubic_c))
    {
        (true, current_sampling)
    } else if next_sampling.use_cubic && current_sampling.filter == FILTER_LINEAR {
        (true, next_sampling)
    } else if current_sampling.filter == FILTER_LINEAR && next_sampling.filter == FILTER_LINEAR {
        (true, next_sampling)
    } else if next_sampling.filter == FILTER_NEAREST && current_xform_wont_affect_nearest {
        (true, next_sampling)
    } else if current_sampling.filter == FILTER_NEAREST && next_xform_wont_affect_nearest {
        (true, current_sampling)
    } else {
        (false, next_sampling)
    }
}

/// Exact port of `FilterResult::analyzeBounds(xtraTransform, dstBounds, scope)`.
pub fn analyze_bounds(
    meta: &FilterResultMeta,
    xtra_transform: &Matrix,
    dst_bounds: IRect,
    scope: BoundsScope,
) -> u32 {
    const HALF_PIXEL: f32 = 0.5_f32;
    const CUBIC_RADIUS: f32 = 1.5_f32;

    let mut analysis = bounds_analysis::SIMPLE;
    let fills_layer_bounds =
        meta.tile_mode != TileMode::Decal || (meta.has_color_filter && meta.cf_affects_transparent_black);

    // 1. Is the layer geometry visible in dstBounds?
    let mut pixel_center_bounds = rect_from_irect(dst_bounds);
    let xtra_m44 = m44_from_m33(xtra_transform);
    if !Rect::quad_contains_rect(
        &xtra_m44,
        rect_from_irect(meta.layer_bounds),
        rect_from_irect(dst_bounds),
        K_ROUND_EPSILON,
    ) {
        let mut require_layer_crop = fills_layer_bounds;
        if !fills_layer_bounds {
            let img_rect = IRect {
                left: 0,
                top: 0,
                right: meta.image_dims.width,
                bottom: meta.image_dims.height,
            };
            let image_bounds = map_irect(img_rect, &meta.transform);
            require_layer_crop = !irect_contains_rect(meta.layer_bounds, image_bounds);
        }

        if require_layer_crop {
            analysis |= bounds_analysis::REQUIRES_LAYER_CROP;
            let layer_bounds_in_dst = map_irect(meta.layer_bounds, xtra_transform);
            if let Some(inter) =
                rect_intersect(pixel_center_bounds, rect_from_irect(layer_bounds_in_dst))
            {
                pixel_center_bounds = inter;
            }
        }
    }

    // 2. Are tiling and deferred color filter effects visible in sampled bounds?
    let image_bounds = Rect {
        left: 0.0,
        top: 0.0,
        right: meta.image_dims.width as f32,
        bottom: meta.image_dims.height as f32,
    };
    let mut net_transform = meta.transform;
    net_transform.post_concat(xtra_transform);
    let net_m44 = m44_from_m33(&net_transform);

    let (x_axis_aligned, y_axis_aligned, _) = are_axes_nearly_integer_aligned(&net_transform);
    let is_pixel_aligned = x_axis_aligned && y_axis_aligned;

    let decal_leaks = scope != BoundsScope::Rescale
        && meta.tile_mode == TileMode::Decal
        && meta.sampling != SamplingOptions::NEAREST
        && !is_pixel_aligned;

    let sample_radius = if meta.sampling.use_cubic {
        CUBIC_RADIUS
    } else {
        HALF_PIXEL
    };
    let mut safe_image_bounds = rect_make_inset(image_bounds, sample_radius, sample_radius);
    if meta.sampling == SamplingOptions::DEFAULT_LINEAR && !is_pixel_aligned {
        safe_image_bounds = rect_make_inset(
            safe_image_bounds,
            if x_axis_aligned { 0.0 } else { K_ROUND_EPSILON },
            if y_axis_aligned { 0.0 } else { K_ROUND_EPSILON },
        );
    }
    let mut has_pixel_padding = meta.boundary != PixelBoundary::Unknown;

    let mut cached_scales: Option<Option<[f32; 2]>> = None;
    let mut get_scale_factors = || -> Option<[f32; 2]> {
        if let Some(s) = cached_scales {
            s
        } else {
            let s = net_transform.get_min_max_scales();
            cached_scales = Some(s);
            s
        }
    };

    if !Rect::quad_contains_rect(
        &net_m44,
        if decal_leaks {
            safe_image_bounds
        } else {
            image_bounds
        },
        pixel_center_bounds,
        K_ROUND_EPSILON,
    ) {
        analysis |= bounds_analysis::DST_BOUNDS_NOT_COVERED;
        if fills_layer_bounds {
            analysis |= bounds_analysis::HAS_LAYER_FILLING_EFFECT;
        }
        if decal_leaks {
            let near_unit_scale = match get_scale_factors() {
                Some(sf) => {
                    scalar_nearly_equal(sf[0], 1.0_f32, 0.2_f32)
                        && scalar_nearly_equal(sf[1], 1.0_f32, 0.2_f32)
                }
                None => false,
            };
            if !near_unit_scale {
                analysis |= bounds_analysis::REQUIRES_DECAL_IN_LAYER_SPACE;
                if meta.boundary == PixelBoundary::Transparent {
                    has_pixel_padding = false;
                }
            }
        }
    }

    if scope == BoundsScope::Deferred {
        return analysis;
    } else if scope == BoundsScope::CanDrawDirectly
        && (analysis & bounds_analysis::HAS_LAYER_FILLING_EFFECT) == 0
    {
        let can_use_transparent_padding = has_pixel_padding
            && meta.can_clamp_to_transparent_boundary(analysis)
            && meta.sampling == SamplingOptions::DEFAULT_LINEAR;
        let nn_or_bilerp = meta.sampling == SamplingOptions::DEFAULT_LINEAR
            || meta.sampling == SamplingOptions::NEAREST;
        if (nn_or_bilerp && is_pixel_aligned) || can_use_transparent_padding {
            return analysis;
        }
        if has_pixel_padding && nn_or_bilerp {
            if let Some(sf) = get_scale_factors() {
                if sf[0] >= 1.0_f32
                    && (meta.image_dims.width as f32) * sf[0] >= 1.0_f32
                    && (meta.image_dims.height as f32) * sf[0] >= 1.0_f32
                {
                    return analysis;
                }
            }
        }
    }

    // 3. Would image pixels outside of subset be sampled if shader-clamping is skipped?
    if has_pixel_padding {
        safe_image_bounds =
            rect_make_outset(safe_image_bounds, K_LAYER_PADDING as f32, K_LAYER_PADDING as f32);
    }
    pixel_center_bounds = rect_make_inset(pixel_center_bounds, HALF_PIXEL, HALF_PIXEL);

    let edge_mask = Rect::quad_contains_rect_mask(
        &net_m44,
        safe_image_bounds,
        pixel_center_bounds,
        K_ROUND_EPSILON,
    );
    let all_edge = edge_mask[0] != 0 && edge_mask[1] != 0 && edge_mask[2] != 0 && edge_mask[3] != 0;
    if !all_edge {
        let mut hw_edge = [
            if meta.image_subset.top == 0 { -1 } else { 0 },
            if meta.image_subset.right == meta.backing_dims.width {
                -1
            } else {
                0
            },
            if meta.image_subset.bottom == meta.backing_dims.height {
                -1
            } else {
                0
            },
            if meta.image_subset.left == 0 { -1 } else { 0 },
        ];
        if meta.tile_mode == TileMode::Repeat || meta.tile_mode == TileMode::Mirror {
            // TRBL & BLTR
            hw_edge = [
                hw_edge[0] & hw_edge[2],
                hw_edge[1] & hw_edge[3],
                hw_edge[2] & hw_edge[0],
                hw_edge[3] & hw_edge[1],
            ];
        }
        let all_covered = (edge_mask[0] | hw_edge[0]) != 0
            && (edge_mask[1] | hw_edge[1]) != 0
            && (edge_mask[2] | hw_edge[2]) != 0
            && (edge_mask[3] | hw_edge[3]) != 0;
        if !all_covered {
            analysis |= bounds_analysis::REQUIRES_SHADER_TILING;
        }
    }

    analysis
}

/// Action returned by `plan_apply_crop`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CropAction {
    Empty = 0,
    PeriodicTransform = 1,
    Subset = 2,
    DecalAdjust = 3,
    ResolveAndTile = 4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CropPlan {
    pub action: CropAction,
    pub effective_tile_mode: TileMode,
    pub fitted_crop: IRect,
    pub origin: IPoint,
    pub periodic_transform: Matrix,
    pub double_clamp: bool,
}

/// Exact port of `FilterResult::applyCrop(ctx, crop, tileMode)` decision & bounds math.
pub fn plan_apply_crop(
    meta: &FilterResultMeta,
    desired_output: IRect,
    crop: IRect,
    mut tile_mode: TileMode,
) -> CropPlan {
    let empty_plan = CropPlan {
        action: CropAction::Empty,
        effective_tile_mode: tile_mode,
        fitted_crop: IRect::make_empty(),
        origin: IPoint { x: 0, y: 0 },
        periodic_transform: Matrix::identity(),
        double_clamp: false,
    };

    if irect_is_empty64(crop) || irect_is_empty64(desired_output) {
        return empty_plan;
    }

    if !meta.has_image {
        return empty_plan;
    }
    let Some(mut crop_content) = irect_intersect(crop, meta.layer_bounds) else {
        return empty_plan;
    };

    let mut fitted_crop = relevant_subset(crop, desired_output, tile_mode);
    let Some(inter) = irect_intersect(crop_content, fitted_crop) else {
        return empty_plan;
    };
    crop_content = inter;

    if let Some(periodic) = periodic_axis_transform(tile_mode, fitted_crop, desired_output) {
        return CropPlan {
            action: CropAction::PeriodicTransform,
            effective_tile_mode: tile_mode,
            fitted_crop,
            origin: IPoint { x: 0, y: 0 },
            periodic_transform: periodic,
            double_clamp: false,
        };
    }

    let mut preserve_transparency_in_crop = false;
    if tile_mode == TileMode::Decal {
        fitted_crop = crop_content;
    } else if irect_contains_rect(fitted_crop, desired_output) {
        tile_mode = TileMode::Decal;
        fitted_crop = desired_output;
    } else if !irect_contains_rect(crop_content, fitted_crop) {
        preserve_transparency_in_crop = true;
        if meta.tile_mode == TileMode::Decal && tile_mode == TileMode::Clamp {
            crop_content = irect_outset(crop_content, 1, 1);
            if let Some(ci) = irect_intersect(fitted_crop, crop_content) {
                fitted_crop = ci;
            }
        }
    }

    let double_clamp = meta.tile_mode == TileMode::Clamp && tile_mode == TileMode::Clamp;
    if !preserve_transparency_in_crop {
        if let Some(origin) = is_nearly_integer_translation(&meta.transform) {
            let id = Matrix::identity();
            let has_filling = (analyze_bounds(meta, &id, fitted_crop, BoundsScope::Deferred)
                & bounds_analysis::HAS_LAYER_FILLING_EFFECT)
                != 0;
            if double_clamp || !has_filling {
                return CropPlan {
                    action: CropAction::Subset,
                    effective_tile_mode: tile_mode,
                    fitted_crop,
                    origin,
                    periodic_transform: Matrix::identity(),
                    double_clamp,
                };
            }
        }
    }

    if tile_mode == TileMode::Decal {
        CropPlan {
            action: CropAction::DecalAdjust,
            effective_tile_mode: tile_mode,
            fitted_crop,
            origin: IPoint { x: 0, y: 0 },
            periodic_transform: Matrix::identity(),
            double_clamp: false,
        }
    } else {
        CropPlan {
            action: CropAction::ResolveAndTile,
            effective_tile_mode: tile_mode,
            fitted_crop,
            origin: IPoint { x: 0, y: 0 },
            periodic_transform: Matrix::identity(),
            double_clamp: false,
        }
    }
}

/// Action returned by `plan_apply_color_filter`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColorFilterAction {
    Empty = 0,
    SolidColor1x1 = 1,
    ResolveAndClamp = 2,
    ComposeDeferred = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ColorFilterPlan {
    pub action: ColorFilterAction,
    pub bounds: IRect,
}

/// Exact port of `FilterResult::applyColorFilter(ctx, colorFilter)` decision logic.
pub fn plan_apply_color_filter(
    meta: &FilterResultMeta,
    desired_output: IRect,
    new_cf_affects_transparent_black: bool,
) -> ColorFilterPlan {
    if irect_is_empty64(desired_output) {
        return ColorFilterPlan {
            action: ColorFilterAction::Empty,
            bounds: IRect::make_empty(),
        };
    }

    if new_cf_affects_transparent_black {
        if !meta.has_image {
            return ColorFilterPlan {
                action: ColorFilterAction::SolidColor1x1,
                bounds: irect_make_xywh(desired_output.left, desired_output.top, 1, 1),
            };
        }
        let Some(mut new_layer_bounds) = irect_intersect(meta.layer_bounds, desired_output) else {
            return ColorFilterPlan {
                action: ColorFilterAction::SolidColor1x1,
                bounds: irect_make_xywh(desired_output.left, desired_output.top, 1, 1),
            };
        };

        let id = Matrix::identity();
        if (analyze_bounds(meta, &id, desired_output, BoundsScope::Deferred)
            & bounds_analysis::REQUIRES_LAYER_CROP)
            != 0
        {
            new_layer_bounds = irect_outset(new_layer_bounds, K_LAYER_PADDING, K_LAYER_PADDING);
            if let Some(inter) = irect_intersect(new_layer_bounds, desired_output) {
                new_layer_bounds = inter;
            }
            return ColorFilterPlan {
                action: ColorFilterAction::ResolveAndClamp,
                bounds: new_layer_bounds,
            };
        }

        ColorFilterPlan {
            action: ColorFilterAction::ComposeDeferred,
            bounds: desired_output,
        }
    } else {
        if !meta.has_image || !IRect::intersects(meta.layer_bounds, desired_output) {
            return ColorFilterPlan {
                action: ColorFilterAction::Empty,
                bounds: IRect::make_empty(),
            };
        }
        ColorFilterPlan {
            action: ColorFilterAction::ComposeDeferred,
            bounds: meta.layer_bounds,
        }
    }
}

/// Action returned by `plan_apply_transform`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TransformAction {
    Empty = 0,
    MergeDeferred = 1,
    ResolveFirst = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TransformPlan {
    pub action: TransformAction,
    pub next_sampling: SamplingOptions,
    pub resolve_bounds: IRect,
    pub has_resolve_bounds: bool,
}

/// Exact port of `FilterResult::applyTransform(ctx, transform, sampling)` decision logic.
pub fn plan_apply_transform(
    meta: &FilterResultMeta,
    desired_output: IRect,
    transform: &Matrix,
    sampling: SamplingOptions,
) -> TransformPlan {
    let empty_plan = TransformPlan {
        action: TransformAction::Empty,
        next_sampling: sampling,
        resolve_bounds: IRect::make_empty(),
        has_resolve_bounds: false,
    };

    if !meta.has_image || irect_is_empty64(desired_output) {
        return empty_plan;
    }
    if transform.invert().is_none() {
        return empty_plan;
    }

    let current_xform_is_int = is_nearly_integer_translation(&meta.transform).is_some();
    let next_xform_is_int = is_nearly_integer_translation(transform).is_some();

    let next_sampling = if next_xform_is_int {
        SamplingOptions::DEFAULT_LINEAR
    } else {
        sampling
    };

    let is_cropped = !next_xform_is_int
        && (analyze_bounds(meta, transform, desired_output, BoundsScope::Deferred)
            & bounds_analysis::REQUIRES_LAYER_CROP)
            != 0;

    let (compat, merged_sampling) = compatible_sampling(
        meta.sampling,
        current_xform_is_int,
        next_sampling,
        next_xform_is_int,
    );

    if !is_cropped && compat {
        TransformPlan {
            action: TransformAction::MergeDeferred,
            next_sampling: merged_sampling,
            resolve_bounds: IRect::make_empty(),
            has_resolve_bounds: false,
        }
    } else {
        let resolve_bounds = inverse_map_irect(transform, desired_output);
        TransformPlan {
            action: TransformAction::ResolveFirst,
            next_sampling: merged_sampling,
            resolve_bounds: resolve_bounds.unwrap_or_else(IRect::make_empty),
            has_resolve_bounds: resolve_bounds.is_some(),
        }
    }
}

/// Action returned by `plan_resolve`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResolveAction {
    Empty = 0,
    Subset = 1,
    DrawToSurface = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ResolvePlan {
    pub action: ResolveAction,
    pub dst_bounds: IRect,
    pub origin: IPoint,
    pub boundary: PixelBoundary,
}

/// Exact port of `FilterResult::resolve(ctx, dstBounds, preserveDstBounds)`.
pub fn plan_resolve(
    meta: &FilterResultMeta,
    mut dst_bounds: IRect,
    preserve_dst_bounds: bool,
) -> ResolvePlan {
    if !meta.has_image {
        return ResolvePlan {
            action: ResolveAction::Empty,
            dst_bounds: IRect::make_empty(),
            origin: IPoint { x: 0, y: 0 },
            boundary: PixelBoundary::Unknown,
        };
    }
    if !preserve_dst_bounds {
        let Some(inter) = irect_intersect(dst_bounds, meta.layer_bounds) else {
            return ResolvePlan {
                action: ResolveAction::Empty,
                dst_bounds: IRect::make_empty(),
                origin: IPoint { x: 0, y: 0 },
                boundary: PixelBoundary::Unknown,
            };
        };
        dst_bounds = inter;
    }

    let subset_compatible =
        !meta.has_color_filter && meta.tile_mode == TileMode::Decal && !preserve_dst_bounds;
    if subset_compatible {
        if let Some(origin) = is_nearly_integer_translation(&meta.transform) {
            return ResolvePlan {
                action: ResolveAction::Subset,
                dst_bounds,
                origin,
                boundary: PixelBoundary::Unknown,
            };
        }
    }

    let boundary = if preserve_dst_bounds {
        PixelBoundary::Unknown
    } else {
        PixelBoundary::Transparent
    };
    ResolvePlan {
        action: ResolveAction::DrawToSurface,
        dst_bounds,
        origin: IPoint { x: 0, y: 0 },
        boundary,
    }
}

/// Output of `plan_subset`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SubsetPlan {
    pub valid: bool,
    pub image_subset_rel: IRect,
    pub origin: IPoint,
}

/// Exact port of `FilterResult::subset(knownOrigin, subsetBounds, clampSrcIfDisjoint)` bounds math.
pub fn plan_subset(
    image_dims: ISize,
    known_origin: IPoint,
    subset_bounds: IRect,
    clamp_src_if_disjoint: bool,
) -> SubsetPlan {
    let mut image_bounds = irect_make_xywh(
        known_origin.x,
        known_origin.y,
        image_dims.width,
        image_dims.height,
    );
    image_bounds = relevant_subset(
        image_bounds,
        subset_bounds,
        if clamp_src_if_disjoint {
            TileMode::Clamp
        } else {
            TileMode::Decal
        },
    );
    if irect_is_empty64(image_bounds) {
        return SubsetPlan {
            valid: false,
            image_subset_rel: IRect::make_empty(),
            origin: IPoint { x: 0, y: 0 },
        };
    }

    let subset = IRect {
        left: image_bounds.left - known_origin.x,
        top: image_bounds.top - known_origin.y,
        right: image_bounds.right - known_origin.x,
        bottom: image_bounds.bottom - known_origin.y,
    };
    SubsetPlan {
        valid: true,
        image_subset_rel: subset,
        origin: IPoint {
            x: image_bounds.left,
            y: image_bounds.top,
        },
    }
}

/// Exact port of the PixelBoundary update in `FilterResult::subset`.
pub fn subset_pixel_boundary(
    orig_subset: IRect,
    result_subset: IRect,
    orig_boundary: PixelBoundary,
) -> PixelBoundary {
    if orig_subset == result_subset {
        return orig_boundary;
    }
    let mut safe_subset = orig_subset;
    if orig_boundary == PixelBoundary::Unknown {
        safe_subset = irect_inset(safe_subset, K_LAYER_PADDING, K_LAYER_PADDING);
    }
    if irect_contains_rect(safe_subset, result_subset) {
        PixelBoundary::Initialized
    } else {
        PixelBoundary::Unknown
    }
}

/// Output of `plan_as_shader`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AsShaderPlan {
    pub needs_resolve: bool,
    pub analysis: u32,
    pub final_sampling: SamplingOptions,
}

/// Exact port of `FilterResult::asShader` decision logic.
pub fn plan_as_shader(
    meta: &FilterResultMeta,
    xtra_sampling: SamplingOptions,
    flags: u32,
    sample_bounds: IRect,
    cf_is_simple_coeff_mode: bool,
    color_spaces_equal: bool,
) -> AsShaderPlan {
    let current_xform_is_int = is_nearly_integer_translation(&meta.transform).is_some();
    let next_xform_is_int = (flags & shader_flags::NON_TRIVIAL_SAMPLING) == 0;

    let id = Matrix::identity();
    let analysis = analyze_bounds(meta, &id, sample_bounds, BoundsScope::ShaderOnly);

    let (compat, mut sampling) = compatible_sampling(
        meta.sampling,
        current_xform_is_int,
        xtra_sampling,
        next_xform_is_int,
    );

    let repeated_expensive = (flags & shader_flags::SAMPLED_REPEATEDLY) != 0
        && ((meta.has_color_filter && !cf_is_simple_coeff_mode) || !color_spaces_equal);

    let needs_resolve = repeated_expensive
        || !compat
        || (analysis & bounds_analysis::REQUIRES_LAYER_CROP) != 0;

    if sampling == SamplingOptions::DEFAULT_LINEAR
        && next_xform_is_int
        && (needs_resolve || current_xform_is_int)
    {
        sampling = SamplingOptions::NEAREST;
    }

    AsShaderPlan {
        needs_resolve,
        analysis,
        final_sampling: sampling,
    }
}

/// Output of `plan_shader_view` (`FilterResult::getAnalyzedShaderView`).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ShaderViewPlan {
    pub pre_decal: Matrix,
    pub post_decal: Matrix,
    pub effective_tile_mode: TileMode,
    pub use_pixel_outset: bool,
    pub outset: i32,
    pub strict: bool,
    pub requires_decal_in_layer_space: bool,
    pub decal_bounds: Rect,
}

/// Exact port of `FilterResult::getAnalyzedShaderView` transform/tiling planning.
pub fn plan_shader_view(meta: &FilterResultMeta, analysis: u32) -> ShaderViewPlan {
    let local_matrix = meta.transform;
    let image_bounds = Rect {
        left: 0.0,
        top: 0.0,
        right: meta.image_dims.width as f32,
        bottom: meta.image_dims.height as f32,
    };

    let requires_decal = (analysis & bounds_analysis::REQUIRES_DECAL_IN_LAYER_SPACE) != 0;
    let (post_decal, mut pre_decal) = if local_matrix.rect_stays_rect() || !requires_decal {
        (Matrix::identity(), local_matrix)
    } else {
        decompose_transform(
            &local_matrix,
            skia_rs_geometry::Point::new(image_bounds.center_x(), image_bounds.center_y()),
        )
    };

    let mut effective_tile_mode = meta.tile_mode;
    let decal_clamp_to_transparent = meta.can_clamp_to_transparent_boundary(analysis);
    let strict = (analysis & bounds_analysis::REQUIRES_SHADER_TILING) != 0;

    let use_pixel_outset = (K_LAYER_PADDING > 1 || strict) && decal_clamp_to_transparent;
    let outset = if use_pixel_outset {
        let o = if strict { K_LAYER_PADDING } else { 1 };
        pre_decal.pre_translate(-(o as f32), -(o as f32));
        effective_tile_mode = TileMode::Clamp;
        o
    } else {
        if (analysis & bounds_analysis::DST_BOUNDS_NOT_COVERED) == 0 || requires_decal {
            effective_tile_mode = TileMode::Clamp;
        }
        0
    };

    let decal_bounds = if requires_decal {
        matrix_map_rect(&pre_decal, image_bounds)
    } else {
        Rect::make_empty()
    };

    ShaderViewPlan {
        pre_decal,
        post_decal,
        effective_tile_mode,
        use_pixel_outset,
        outset,
        strict,
        requires_decal_in_layer_space: requires_decal,
        decal_bounds,
    }
}

/// Exact port of `downscale_step_count(float netScaleFactor)`.
pub fn downscale_step_count(net_scale_factor: f32) -> i32 {
    let inv_ceil = float_ceil2int(1.0_f32 / net_scale_factor);
    let mut steps = next_log2(inv_ceil as u32);
    if steps > 0 {
        const MULTI_PASS_LIMIT: f32 = 0.9_f32;
        const NEAR_IDENTITY_LIMIT: f32 = 1.0_f32 - K_ROUND_EPSILON;

        let final_step_scale = net_scale_factor * ((1_i32 << (steps - 1)) as f32);
        let limit = if steps == 1 {
            NEAR_IDENTITY_LIMIT
        } else {
            MULTI_PASS_LIMIT
        };
        if final_step_scale >= limit {
            steps -= 1;
        }
    }
    steps
}

/// Exact port of `scale_about_center(src, sx, sy)`.
#[inline]
pub fn scale_about_center(src: Rect, sx: f32, sy: f32) -> Rect {
    let cx = if sx == 1.0_f32 {
        0.0_f32
    } else {
        0.5_f32 * src.left + 0.5_f32 * src.right
    };
    let cy = if sy == 1.0_f32 {
        0.0_f32
    } else {
        0.5_f32 * src.top + 0.5_f32 * src.bottom
    };
    Rect {
        left: (src.left - cx) * sx,
        top: (src.top - cy) * sy,
        right: (src.right - cx) * sx,
        bottom: (src.bottom - cy) * sy,
    }
}

/// Exact port of the 4 border rects drawn by `draw_color_filtered_border`.
pub fn compute_cf_border_rects(border: IRect) -> [IRect; 4] {
    [
        // Top (with corners)
        IRect {
            left: border.left,
            top: border.top,
            right: border.right,
            bottom: border.top + K_LAYER_PADDING,
        },
        // Bottom (with corners)
        IRect {
            left: border.left,
            top: border.bottom - K_LAYER_PADDING,
            right: border.right,
            bottom: border.bottom,
        },
        // Left (no corners)
        IRect {
            left: border.left,
            top: border.top + K_LAYER_PADDING,
            right: border.left + K_LAYER_PADDING,
            bottom: border.bottom - K_LAYER_PADDING,
        },
        // Right (no corners)
        IRect {
            left: border.right - K_LAYER_PADDING,
            top: border.top + K_LAYER_PADDING,
            right: border.right,
            bottom: border.bottom - K_LAYER_PADDING,
        },
    ]
}

/// A single edge or corner draw pair `(src, dst)` for `draw_tiled_border`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TiledBorderPair {
    pub src: Rect,
    pub dst: Rect,
    pub matrix: Matrix,
}

/// All 8 edge/corner draws computed by `draw_tiled_border`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TiledBorderDraws {
    pub pairs: [TiledBorderPair; 8],
}

/// Exact port of `draw_tiled_border` geometry calculations.
pub fn compute_tiled_border_draws(
    tile_mode: TileMode,
    src_to_dst: &Matrix,
    mut src_border: Rect,
    mut dst_border: Rect,
) -> TiledBorderDraws {
    let eps: f32 = if K_LAYER_PADDING == 1 {
        0.5_f32
    } else {
        1.0_f32 / 256.0_f32
    };
    let pad = K_LAYER_PADDING as f32;

    let dst_sample_bounds = rect_make_inset(dst_border, pad, pad);
    let src_sample_bounds =
        inverse_map_rect(src_to_dst, dst_sample_bounds).unwrap_or_else(Rect::make_empty);

    if tile_mode == TileMode::Mirror || tile_mode == TileMode::Repeat {
        let inset_dst = rect_make_inset(dst_sample_bounds, 0.5_f32, 0.5_f32);
        let inv = inverse_map_rect(src_to_dst, inset_dst).unwrap_or_else(Rect::make_empty);
        src_border = rect_make_outset(inv, 0.5_f32, 0.5_f32);
    }

    if tile_mode == TileMode::Repeat {
        dst_border = Rect {
            left: dst_border.right - pad,
            top: dst_border.bottom - pad,
            right: dst_border.left + pad,
            bottom: dst_border.top + pad,
        };
    }

    let make_pair = |src: Rect, dst: Rect| TiledBorderPair {
        src,
        dst,
        matrix: matrix_rect_to_rect_or_identity(src, dst),
    };
    let make_corner = |sx: f32, sy: f32, dx: f32, dy: f32| {
        let src = Rect {
            left: sx + 0.5_f32 - eps,
            top: sy + 0.5_f32 - eps,
            right: sx + 0.5_f32 + eps,
            bottom: sy + 0.5_f32 + eps,
        };
        let dst = Rect {
            left: dx,
            top: dy,
            right: dx + pad,
            bottom: dy + pad,
        };
        make_pair(src, dst)
    };

    TiledBorderDraws {
        pairs: [
            // Left edge
            make_pair(
                Rect {
                    left: src_border.left + 0.5_f32 - eps,
                    top: src_sample_bounds.top,
                    right: src_border.left + 0.5_f32 + eps,
                    bottom: src_sample_bounds.bottom,
                },
                Rect {
                    left: dst_border.left,
                    top: dst_sample_bounds.top,
                    right: dst_border.left + pad,
                    bottom: dst_sample_bounds.bottom,
                },
            ),
            // Right edge
            make_pair(
                Rect {
                    left: src_border.right - 0.5_f32 - eps,
                    top: src_sample_bounds.top,
                    right: src_border.right - 0.5_f32 + eps,
                    bottom: src_sample_bounds.bottom,
                },
                Rect {
                    left: dst_border.right - pad,
                    top: dst_sample_bounds.top,
                    right: dst_border.right,
                    bottom: dst_sample_bounds.bottom,
                },
            ),
            // Top edge
            make_pair(
                Rect {
                    left: src_sample_bounds.left,
                    top: src_border.top + 0.5_f32 - eps,
                    right: src_sample_bounds.right,
                    bottom: src_border.top + 0.5_f32 + eps,
                },
                Rect {
                    left: dst_sample_bounds.left,
                    top: dst_border.top,
                    right: dst_sample_bounds.right,
                    bottom: dst_border.top + pad,
                },
            ),
            // Bottom edge
            make_pair(
                Rect {
                    left: src_sample_bounds.left,
                    top: src_border.bottom - 0.5_f32 - eps,
                    right: src_sample_bounds.right,
                    bottom: src_border.bottom - 0.5_f32 + eps,
                },
                Rect {
                    left: dst_sample_bounds.left,
                    top: dst_border.bottom - pad,
                    right: dst_sample_bounds.right,
                    bottom: dst_border.bottom,
                },
            ),
            // TL, TR, BR, BL corners
            make_corner(src_border.left, src_border.top, dst_border.left, dst_border.top),
            make_corner(
                src_border.right - 1.0_f32,
                src_border.top,
                dst_border.right - pad,
                dst_border.top,
            ),
            make_corner(
                src_border.right - 1.0_f32,
                src_border.bottom - 1.0_f32,
                dst_border.right - pad,
                dst_border.bottom - pad,
            ),
            make_corner(
                src_border.left,
                src_border.bottom - 1.0_f32,
                dst_border.left,
                dst_border.bottom - pad,
            ),
        ],
    }
}

/// Setup action for `FilterResult::rescale`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RescaleSetupAction {
    Empty = 0,
    NoopAdjustBounds = 1,
    NoopSubset = 2,
    RunSteps = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct RescaleSetup {
    pub action: RescaleSetupAction,
    pub visible_layer_bounds: IRect,
    pub origin: IPoint,
    pub src_rect: IRect,
    pub tile_mode: TileMode,
    pub cf_border: bool,
    pub defer_periodic_tiling: bool,
    pub pre_resolve_deferred_xform: bool,
    pub x_steps: i32,
    pub y_steps: i32,
    pub final_scale_x: f32,
    pub final_scale_y: f32,
}

/// Exact port of `FilterResult::rescale` setup logic (before the step loop).
pub fn compute_rescale_setup(
    meta: &FilterResultMeta,
    desired_output: IRect,
    scale: Size,
    enforce_decal: bool,
    mut allow_overscaling: bool,
    color_type_matches: bool,
    color_space_matches: bool,
) -> RescaleSetup {
    let empty = RescaleSetup {
        action: RescaleSetupAction::Empty,
        visible_layer_bounds: IRect::make_empty(),
        origin: IPoint { x: 0, y: 0 },
        src_rect: IRect::make_empty(),
        tile_mode: TileMode::Decal,
        cf_border: false,
        defer_periodic_tiling: false,
        pre_resolve_deferred_xform: false,
        x_steps: 0,
        y_steps: 0,
        final_scale_x: 1.0,
        final_scale_y: 1.0,
    };

    if !meta.has_image || scale.width <= 0.0_f32 || scale.height <= 0.0_f32 {
        return empty;
    }
    let Some(visible_layer_bounds) = irect_intersect(meta.layer_bounds, desired_output) else {
        return empty;
    };

    let origin_opt = is_nearly_integer_translation(&meta.transform);
    let pixel_aligned = origin_opt.is_some();
    let origin = origin_opt.unwrap_or(IPoint { x: 0, y: 0 });

    let id = Matrix::identity();
    let analysis = analyze_bounds(meta, &id, desired_output, BoundsScope::Rescale);

    let can_defer_tiling = pixel_aligned
        && (analysis & bounds_analysis::REQUIRES_LAYER_CROP) == 0
        && !(enforce_decal && (analysis & bounds_analysis::HAS_LAYER_FILLING_EFFECT) != 0);

    let has_effects_to_apply =
        !can_defer_tiling || meta.has_color_filter || !color_type_matches || !color_space_matches;

    let x_steps = downscale_step_count(scale.width);
    let y_steps = downscale_step_count(scale.height);

    if x_steps == 0 && y_steps == 0 && !has_effects_to_apply {
        if (analysis & bounds_analysis::HAS_LAYER_FILLING_EFFECT) != 0 {
            return RescaleSetup {
                action: RescaleSetupAction::NoopAdjustBounds,
                visible_layer_bounds,
                origin,
                ..empty
            };
        } else {
            return RescaleSetup {
                action: RescaleSetupAction::NoopSubset,
                visible_layer_bounds,
                origin,
                ..empty
            };
        }
    }

    let mut cf_border = false;
    let mut defer_periodic_tiling = false;
    let (mut src_rect, tile_mode) =
        if can_defer_tiling && (analysis & bounds_analysis::HAS_LAYER_FILLING_EFFECT) != 0 {
            let sr = irect_make_xywh(origin.x, origin.y, meta.image_dims.width, meta.image_dims.height);
            if meta.tile_mode == TileMode::Decal {
                cf_border = true;
                (sr, TileMode::Clamp)
            } else {
                let tm = meta.tile_mode;
                defer_periodic_tiling = tm == TileMode::Repeat || tm == TileMode::Mirror;
                (sr, tm)
            }
        } else {
            (visible_layer_bounds, TileMode::Decal)
        };

    src_rect = relevant_subset(src_rect, desired_output, tile_mode);
    if rect_from_irect(src_rect).is_empty() {
        return empty;
    }

    let mut pre_resolve = false;
    if !pixel_aligned && (x_steps > 0 || y_steps > 0) {
        let net_scale = crate::types::map_size(scale, &meta.transform);
        let next_x_steps = if net_scale.width.is_finite() {
            downscale_step_count(net_scale.width)
        } else {
            i32::MAX
        };
        let next_y_steps = if net_scale.height.is_finite() {
            downscale_step_count(net_scale.height)
        } else {
            i32::MAX
        };
        if (x_steps > 0 && next_x_steps > x_steps) || (y_steps > 0 && next_y_steps > y_steps) {
            pre_resolve = true;
        }
    }

    if !defer_periodic_tiling {
        allow_overscaling = false;
    }

    let final_scale_x = if x_steps > 0 {
        if allow_overscaling {
            1.0_f32 / ((1_i32 << x_steps) as f32)
        } else {
            scale.width
        }
    } else {
        1.0_f32
    };
    let final_scale_y = if y_steps > 0 {
        if allow_overscaling {
            1.0_f32 / ((1_i32 << y_steps) as f32)
        } else {
            scale.height
        }
    } else {
        1.0_f32
    };

    RescaleSetup {
        action: RescaleSetupAction::RunSteps,
        visible_layer_bounds,
        origin,
        src_rect,
        tile_mode,
        cf_border,
        defer_periodic_tiling,
        pre_resolve_deferred_xform: pre_resolve,
        x_steps,
        y_steps,
        final_scale_x,
        final_scale_y,
    }
}

/// Output of a single iteration in `FilterResult::rescale`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct RescaleStep {
    pub next_x_steps: i32,
    pub next_y_steps: i32,
    pub dst_bounds_f: Rect,
    pub dst_pixel_bounds: IRect,
    pub sample_bounds: IRect,
    pub boundary: PixelBoundary,
    pub scale_xform: Matrix,
    pub src_sampled: Rect,
}

/// Exact port of one iteration of the `do { ... } while (xSteps > 0 || ySteps > 0)` loop in `rescale`.
pub fn compute_rescale_step(
    src_rect: IRect,
    step_bounds_f: Rect,
    mut x_steps: i32,
    mut y_steps: i32,
    final_scale_x: f32,
    final_scale_y: f32,
    tile_mode: TileMode,
    defer_periodic_tiling: bool,
) -> RescaleStep {
    let mut sx = 1.0_f32;
    if x_steps > 0 {
        sx = if x_steps > 1 {
            0.5_f32
        } else {
            (src_rect.width() as f32) * final_scale_x / step_bounds_f.width()
        };
        x_steps -= 1;
    }

    let mut sy = 1.0_f32;
    if y_steps > 0 {
        sy = if y_steps > 1 {
            0.5_f32
        } else {
            (src_rect.height() as f32) * final_scale_y / step_bounds_f.height()
        };
        y_steps -= 1;
    }

    let mut dst_bounds_f = scale_about_center(step_bounds_f, sx, sy);
    let final_x_step = x_steps == 0 && sx != 1.0_f32;
    let final_y_step = y_steps == 0 && sy != 1.0_f32;
    if defer_periodic_tiling && (final_x_step || final_y_step) {
        let dst_pixels = round_out(dst_bounds_f);
        dst_bounds_f = Rect {
            left: if final_x_step {
                dst_pixels.left as f32
            } else {
                dst_bounds_f.left
            },
            top: if final_y_step {
                dst_pixels.top as f32
            } else {
                dst_bounds_f.top
            },
            right: if final_x_step {
                dst_pixels.right as f32
            } else {
                dst_bounds_f.right
            },
            bottom: if final_y_step {
                dst_pixels.bottom as f32
            } else {
                dst_bounds_f.bottom
            },
        };
    }

    let mut dst_pixel_bounds = round_out(dst_bounds_f);
    let sample_bounds = dst_pixel_bounds;
    let boundary = if tile_mode == TileMode::Decal {
        PixelBoundary::Transparent
    } else {
        dst_pixel_bounds = irect_outset(dst_pixel_bounds, K_LAYER_PADDING, K_LAYER_PADDING);
        PixelBoundary::Unknown
    };

    let scale_xform = matrix_rect_to_rect_or_identity(step_bounds_f, dst_bounds_f);
    let src_sampled =
        inverse_map_rect(&scale_xform, rect_from_irect(sample_bounds)).unwrap_or_else(Rect::make_empty);

    RescaleStep {
        next_x_steps: x_steps,
        next_y_steps: y_steps,
        dst_bounds_f,
        dst_pixel_bounds,
        sample_bounds,
        boundary,
        scale_xform,
        src_sampled,
    }
}

/// Action returned by `plan_make_from_image`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MakeFromImageAction {
    Empty = 0,
    DirectSpecial = 1,
    DrawToSurface = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MakeFromImagePlan {
    pub action: MakeFromImageAction,
    pub src_rect: Rect,
    pub dst_rect: Rect,
    pub src_rect_int: IRect,
    pub transform: Matrix,
    pub dst_bounds: IRect,
}

/// Exact port of `FilterResult::MakeFromImage` geometry & classification logic.
pub fn plan_make_from_image(
    mapping: &Mapping,
    desired_output: IRect,
    image_dims: ISize,
    mut src_rect: Rect,
    mut dst_rect: Rect,
) -> MakeFromImagePlan {
    let empty = MakeFromImagePlan {
        action: MakeFromImageAction::Empty,
        src_rect,
        dst_rect,
        src_rect_int: IRect::make_empty(),
        transform: Matrix::identity(),
        dst_bounds: IRect::make_empty(),
    };

    let image_bounds = Rect {
        left: 0.0,
        top: 0.0,
        right: image_dims.width as f32,
        bottom: image_dims.height as f32,
    };
    if !rect_contains_rect(image_bounds, src_rect) {
        let src_to_dst = matrix_rect_to_rect_or_identity(src_rect, dst_rect);
        let Some(inter) = rect_intersect(src_rect, image_bounds) else {
            return empty;
        };
        src_rect = inter;
        dst_rect = matrix_map_rect(&src_to_dst, src_rect);
    }

    if dst_rect.is_empty() {
        return empty;
    }

    let mut src_rect_int = round_out(src_rect);
    if rect_from_irect(src_rect_int) == src_rect {
        let img_irect = IRect {
            left: 0,
            top: 0,
            right: image_dims.width,
            bottom: image_dims.height,
        };
        let Some(clamped) = irect_intersect(src_rect_int, img_irect) else {
            return empty;
        };
        src_rect_int = clamped;
        let r2r = M44::rect_to_rect(src_rect, dst_rect);
        let mut transform44 = M44::identity();
        transform44.set_concat(&mapping.param_to_layer, &r2r);
        return MakeFromImagePlan {
            action: MakeFromImageAction::DirectSpecial,
            src_rect,
            dst_rect,
            src_rect_int,
            transform: m44_as_m33(&transform44),
            dst_bounds: IRect::make_empty(),
        };
    }

    let dst_bounds_layer = round_out(mapping.param_to_layer_rect(dst_rect));
    let Some(dst_bounds) = irect_intersect(dst_bounds_layer, desired_output) else {
        return empty;
    };

    MakeFromImagePlan {
        action: MakeFromImageAction::DrawToSurface,
        src_rect,
        dst_rect,
        src_rect_int,
        transform: Matrix::identity(),
        dst_bounds,
    }
}

/// Output of `plan_builder_blur` (first half of `FilterResult::Builder::blur`).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct BlurPlan {
    pub valid: bool,
    pub output_bounds: IRect,
    pub sample_bounds: IRect,
    pub rescale_scale: Size,
}

/// Exact port of `FilterResult::Builder::blur` pre-rescale bounds & scale calculation.
pub fn plan_builder_blur(
    desired_output: IRect,
    input_layer_bounds: IRect,
    sigma: Size,
    max_sigma: f32,
) -> BlurPlan {
    let radii = size_ceil(Size {
        width: 3.0_f32 * sigma.width,
        height: 3.0_f32 * sigma.height,
    });
    let max_output = irect_outset(input_layer_bounds, radii.width, radii.height);
    let Some(output_bounds) = irect_intersect(desired_output, max_output) else {
        return BlurPlan {
            valid: false,
            output_bounds: IRect::make_empty(),
            sample_bounds: IRect::make_empty(),
            rescale_scale: Size {
                width: 1.0,
                height: 1.0,
            },
        };
    };
    if irect_is_empty64(output_bounds) {
        return BlurPlan {
            valid: false,
            output_bounds: IRect::make_empty(),
            sample_bounds: IRect::make_empty(),
            rescale_scale: Size {
                width: 1.0,
                height: 1.0,
            },
        };
    }

    let sample_bounds = irect_outset(output_bounds, radii.width, radii.height);
    let sx = if sigma.width > max_sigma {
        max_sigma / sigma.width
    } else {
        1.0_f32
    };
    let sy = if sigma.height > max_sigma {
        max_sigma / sigma.height
    } else {
        1.0_f32
    };

    BlurPlan {
        valid: true,
        output_bounds,
        sample_bounds,
        rescale_scale: Size {
            width: sx,
            height: sy,
        },
    }
}

/// Output of `plan_low_res_blur` (second half of `FilterResult::Builder::blur`).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct LowResBlurPlan {
    pub valid: bool,
    pub low_res_sigma: Size,
    pub src_relative_output: IRect,
    pub blur_output_bounds: IRect,
    pub tile_mode: TileMode,
    pub outset_source_image: bool,
}

/// Exact port of `FilterResult::Builder::blur` post-rescale low-res blur setup.
pub fn plan_low_res_blur(
    sigma: Size,
    max_sigma: f32,
    supports_only_decal: bool,
    output_bounds: IRect,
    low_res_dims: ISize,
    low_res_transform: &Matrix,
    low_res_tile_mode: TileMode,
    low_res_boundary: PixelBoundary,
) -> LowResBlurPlan {
    let inv_scale_x = ieee_float_divide(1.0_f32, low_res_transform.mat[0]);
    let inv_scale_y = ieee_float_divide(1.0_f32, low_res_transform.mat[4]);
    let sx = sigma.width * inv_scale_x;
    let sy = sigma.height * inv_scale_y;
    let low_res_sigma = Size {
        width: if max_sigma < sx { max_sigma } else { sx },
        height: if max_sigma < sy { max_sigma } else { sy },
    };

    let mut low_res_max_output = IRect {
        left: 0,
        top: 0,
        right: low_res_dims.width,
        bottom: low_res_dims.height,
    };

    let src_relative_output =
        if low_res_tile_mode == TileMode::Repeat || low_res_tile_mode == TileMode::Mirror {
            low_res_max_output
        } else {
            let Some(inv_out) = inverse_map_irect(low_res_transform, output_bounds) else {
                return LowResBlurPlan {
                    valid: false,
                    low_res_sigma,
                    src_relative_output: IRect::make_empty(),
                    blur_output_bounds: IRect::make_empty(),
                    tile_mode: low_res_tile_mode,
                    outset_source_image: false,
                };
            };
            let low_radii = size_ceil(Size {
                width: 3.0_f32 * low_res_sigma.width,
                height: 3.0_f32 * low_res_sigma.height,
            });
            low_res_max_output = irect_outset(low_res_max_output, low_radii.width, low_radii.height);
            let sro = relevant_subset(low_res_max_output, inv_out, low_res_tile_mode);
            if irect_is_empty64(sro) {
                return LowResBlurPlan {
                    valid: false,
                    low_res_sigma,
                    src_relative_output: IRect::make_empty(),
                    blur_output_bounds: IRect::make_empty(),
                    tile_mode: low_res_tile_mode,
                    outset_source_image: false,
                };
            }
            irect_outset(sro, K_LAYER_PADDING, K_LAYER_PADDING)
        };

    let mut blur_output_bounds = src_relative_output;
    let mut tile_mode = low_res_tile_mode;
    let can_clamp =
        low_res_tile_mode == TileMode::Decal && low_res_boundary == PixelBoundary::Transparent;
    let outset_source_image = !supports_only_decal && can_clamp;
    if outset_source_image {
        blur_output_bounds = IRect {
            left: blur_output_bounds.left + K_LAYER_PADDING,
            top: blur_output_bounds.top + K_LAYER_PADDING,
            right: blur_output_bounds.right + K_LAYER_PADDING,
            bottom: blur_output_bounds.bottom + K_LAYER_PADDING,
        };
        tile_mode = TileMode::Clamp;
    }

    LowResBlurPlan {
        valid: true,
        low_res_sigma,
        src_relative_output,
        blur_output_bounds,
        tile_mode,
        outset_source_image,
    }
}
