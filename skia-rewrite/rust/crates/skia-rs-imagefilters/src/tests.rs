#![forbid(unsafe_code)]

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_geometry::{IPoint, IRect, ISize, Matrix, Point, Point3, Rect, Size, M44};

use crate::cache::{CacheKey, CacheKeyBytes, LruCacheIndex, CACHE_KEY_BYTES_LEN};
use crate::core_filter::{
    child_input_layer_bounds_null, combine_ctm_capability, next_image_filter_unique_id,
    validate_common_input_count, validate_legacy_crop, validate_local_matrix, LegacyCropResult,
    LocalMatrixAction,
};
use crate::effects::{
    adjust_image_filter_rects, blend_affects_transparent_black, blend_null_shader_action,
    blur_fast_bounds, blur_kernel_bounds, classify_blend_bounds_rule, classify_blend_factory,
    combine_blend_fast_bounds, combine_blend_output_bounds, compute_lighting_uniforms,
    crop_fast_bounds, crop_layer_rect, crop_output_layer_bounds, crop_required_input,
    displacement_channel_selector, displacement_fast_bounds, displacement_outset_bounds,
    encode_matrix_conv_kernel_a8, lighting_clamp_rect, magnifier_fast_bounds,
    magnifier_output_bounds, map_blur_sigma, map_z_to_layer, matrix_conv_bounds_affected,
    matrix_conv_bounds_sampled, matrix_transform_output_bounds, matrix_transform_required_input,
    morphology_fast_bounds, morphology_kernel_output_bounds, morphology_map_radii,
    picture_intersect_cull_rect, plan_magnifier_filter, plan_morphology_passes,
    quantize_matrix_conv_kernel, spot_light_dir_and_cos_cutoff, validate_blur_params,
    validate_displacement_params, validate_lighting_params, validate_magnifier_params,
    validate_matrix_convolution_params, validate_morphology_params, BlendBoundsRule,
    BlendFactoryAction, BlendNullShaderAction, LightType, MagnifierFilterAction,
    MagnifierValidation, MaterialType, MatrixConvKernelTier, MorphDirection, MorphType,
    MorphValidation,
};
use crate::filter_result::{
    analyze_bounds, auto_surface_padded_bounds, compatible_sampling, compute_cf_border_rects,
    compute_rescale_setup, compute_rescale_step, compute_tiled_border_draws, downscale_step_count,
    plan_apply_color_filter, plan_apply_crop, plan_apply_transform, plan_as_shader,
    plan_builder_blur, plan_low_res_blur, plan_make_from_image, plan_resolve, plan_shader_view,
    plan_subset, scale_about_center, subset_pixel_boundary, ColorFilterAction, CropAction,
    FilterResultMeta, MakeFromImageAction, RescaleSetupAction, ResolveAction, TransformAction,
};
use crate::types::{
    are_axes_nearly_integer_aligned, bounds_analysis, double_saturate2int, inverse_map_irect,
    inverse_map_rect, is_nearly_integer_translation, is_valid_rect, map_ipoint, map_irect,
    map_isize, map_ivector, map_matrix, map_point, map_rect, map_size, map_vector,
    periodic_axis_transform, relevant_subset, round_in, round_out, shader_flags, size_ceil,
    size_floor, size_round, BoundsScope, FVector, IVector, Mapping, MatrixCapability,
    PixelBoundary, SamplingOptions, TileMode, SK_MAX_S32, SK_MIN_S32,
};

fn make_meta(w: i32, h: i32, origin_x: i32, origin_y: i32) -> FilterResultMeta {
    let transform = Matrix::translate(origin_x as f32, origin_y as f32);
    FilterResultMeta {
        has_image: true,
        boundary: PixelBoundary::Unknown,
        sampling: SamplingOptions::DEFAULT_LINEAR,
        tile_mode: TileMode::Decal,
        transform,
        has_color_filter: false,
        cf_affects_transparent_black: false,
        layer_bounds: IRect {
            left: origin_x,
            top: origin_y,
            right: origin_x + w,
            bottom: origin_y + h,
        },
        image_dims: ISize {
            width: w,
            height: h,
        },
        image_subset: IRect {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        },
        backing_dims: ISize {
            width: w,
            height: h,
        },
    }
}

#[test]
fn test_rounding_and_saturate() {
    // Near-integer tolerance: 10.0005 rounds out to 10 (inset by 1e-3 => 10.0015? Wait: left+1e-3, right-1e-3)
    let r = Rect {
        left: 9.9995,
        top: 19.9995,
        right: 30.0005,
        bottom: 40.0005,
    };
    assert_eq!(
        round_out(r),
        IRect {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40
        }
    );
    let r2 = Rect {
        left: 10.0005,
        top: 20.0005,
        right: 29.9995,
        bottom: 39.9995,
    };
    assert_eq!(
        round_in(r2),
        IRect {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40
        }
    );

    assert_eq!(
        size_ceil(Size {
            width: 3.0005,
            height: 3.5
        }),
        ISize {
            width: 3,
            height: 4
        }
    );
    assert_eq!(
        size_floor(Size {
            width: 2.9995,
            height: 2.5
        }),
        ISize {
            width: 3,
            height: 2
        }
    );
    assert_eq!(
        size_round(Size {
            width: 2.6,
            height: 2.4
        }),
        ISize {
            width: 3,
            height: 2
        }
    );

    assert_eq!(double_saturate2int(f64::NAN), SK_MAX_S32);
    assert_eq!(double_saturate2int(1e20), SK_MAX_S32);
    assert_eq!(double_saturate2int(-1e20), SK_MIN_S32);
    assert!(is_valid_rect(Rect {
        left: 0.0,
        top: 0.0,
        right: 10.0,
        bottom: 10.0
    }));
    assert!(!is_valid_rect(Rect {
        left: 10.0,
        top: 0.0,
        right: 0.0,
        bottom: 10.0
    }));
    assert!(!is_valid_rect(Rect {
        left: f32::NAN,
        top: 0.0,
        right: 10.0,
        bottom: 10.0
    }));
}

#[test]
fn test_integer_alignment_and_mapping() {
    let m = Matrix::translate(15.0004, -7.0004);
    assert_eq!(
        is_nearly_integer_translation(&m),
        Some(IPoint { x: 15, y: -7 })
    );
    let m_frac = Matrix::translate(15.5, -7.0);
    let (ax, ay, pt) = are_axes_nearly_integer_aligned(&m_frac);
    assert!(!ax && ay && pt.is_none());

    let mut mapping = Mapping::identity();
    let mut rot = Matrix::identity();
    rot.set_rotate(45.0);
    rot.post_scale(2.0, 3.0);
    let ctm = M44::from_rows_scalar(
        rot.mat[0], rot.mat[1], 0.0, rot.mat[2], rot.mat[3], rot.mat[4], 0.0, rot.mat[5], 0.0, 0.0,
        1.0, 0.0, rot.mat[6], rot.mat[7], 0.0, rot.mat[8],
    );
    assert!(mapping.decompose_ctm(&ctm, MatrixCapability::ScaleTranslate, Point::new(0.0, 0.0)));
    // Layer matrix should be pure scale, remainder should be rotation.
    assert!(mapping.layer_matrix_33().is_scale_translate());
    assert!(mapping.adjust_layer_space(&M44::from_rows_scalar(
        1.0, 0.0, 0.0, -5.0, 0.0, 1.0, 0.0, -10.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0
    )));
    assert_eq!(mapping.param_to_layer.rc(0, 3), -5.0);
    assert_eq!(mapping.param_to_layer.rc(1, 3), -10.0);

    // High-precision irect mapping and inverse mapping (including negative scale)
    let st = Matrix::scale_translate(-2.0, 3.0, 100.0, -50.0);
    let src_ir = IRect {
        left: 10,
        top: 20,
        right: 30,
        bottom: 40,
    };
    let mapped = map_irect(src_ir, &st);
    assert_eq!(
        mapped,
        IRect {
            left: 40,
            top: 10,
            right: 80,
            bottom: 70
        }
    );
    assert_eq!(inverse_map_irect(&st, mapped), Some(src_ir));
    assert_eq!(
        inverse_map_rect(&st, Rect::make_empty()),
        Some(Rect::make_empty())
    );

    // Other map_* specializations
    assert_eq!(
        map_rect(Rect::make_empty(), &st),
        Rect::make_empty()
    );
    assert_eq!(
        map_ipoint(IPoint { x: 1, y: 2 }, &Matrix::translate(3.0, 4.0)),
        IPoint { x: 4, y: 6 }
    );
    assert_eq!(
        map_point(Point::new(1.0, 2.0), &Matrix::translate(3.0, 4.0)),
        Point::new(4.0, 6.0)
    );
    assert_eq!(
        map_vector(FVector { x: 1.0, y: 2.0 }, &st),
        FVector { x: -2.0, y: 6.0 }
    );
    assert_eq!(
        map_ivector(IVector { x: 1, y: 2 }, &st),
        IVector { x: -2, y: 6 }
    );
    assert_eq!(
        map_size(Size { width: 3.0, height: 4.0 }, &st),
        Size {
            width: 6.0,
            height: 12.0
        }
    );
    assert_eq!(
        map_isize(ISize { width: 3, height: 4 }, &st),
        ISize {
            width: 6,
            height: 12
        }
    );
    let conjugated = map_matrix(&Matrix::translate(5.0, 0.0), &Matrix::scale_translate(2.0, 2.0, 0.0, 0.0));
    assert!((conjugated.mat[2] - 10.0).abs() < 1e-5);
}

#[test]
fn test_relevant_subset_and_periodic() {
    let src = IRect {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
    };
    let disjoint = IRect {
        left: 20,
        top: 5,
        right: 30,
        bottom: 8,
    };
    assert_eq!(relevant_subset(src, disjoint, TileMode::Decal), IRect::make_empty());
    assert_eq!(
        relevant_subset(src, disjoint, TileMode::Clamp),
        IRect {
            left: 9,
            top: 5,
            right: 10,
            bottom: 8
        }
    );
    assert_eq!(relevant_subset(src, disjoint, TileMode::Repeat), src);

    // Periodic single-period simplification for Repeat and Mirror
    let out_repeat = IRect {
        left: 12,
        top: 2,
        right: 18,
        bottom: 8,
    };
    let m_rep = periodic_axis_transform(TileMode::Repeat, src, out_repeat).unwrap();
    assert_eq!(m_rep.mat[0], 1.0);
    assert_eq!(m_rep.mat[2], 10.0);

    let m_mir = periodic_axis_transform(TileMode::Mirror, src, out_repeat).unwrap();
    assert_eq!(m_mir.mat[0], -1.0);
    assert_eq!(m_mir.mat[2], 20.0);

    // Spanning two periods cannot simplify
    let out_span = IRect {
        left: 8,
        top: 0,
        right: 12,
        bottom: 10,
    };
    assert!(periodic_axis_transform(TileMode::Repeat, src, out_span).is_none());
}

#[test]
fn test_filter_result_analysis_and_plans() {
    let meta = make_meta(100, 100, 0, 0);
    let id = Matrix::identity();

    // Exact coverage => simple
    let a = analyze_bounds(
        &meta,
        &id,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        BoundsScope::CanDrawDirectly,
    );
    assert_eq!(a, bounds_analysis::SIMPLE);

    // Compatible sampling checks
    let (ok, res) = compatible_sampling(
        SamplingOptions::DEFAULT_LINEAR,
        true,
        SamplingOptions::NEAREST,
        false,
    );
    assert!(ok && res == SamplingOptions::NEAREST);
    let (ok2, _) = compatible_sampling(
        SamplingOptions::NEAREST,
        false,
        SamplingOptions::DEFAULT_LINEAR,
        false,
    );
    assert!(!ok2);

    // Crop plan: decal subset inside image
    let crop_plan = plan_apply_crop(
        &meta,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        IRect {
            left: 10,
            top: 20,
            right: 50,
            bottom: 60,
        },
        TileMode::Decal,
    );
    assert_eq!(crop_plan.action, CropAction::Subset);
    assert_eq!(
        crop_plan.fitted_crop,
        IRect {
            left: 10,
            top: 20,
            right: 50,
            bottom: 60
        }
    );

    // ColorFilter plan on empty image with transparent-black-affecting CF => 1x1 solid color
    let mut empty_meta = meta;
    empty_meta.has_image = false;
    let cf_plan = plan_apply_color_filter(
        &empty_meta,
        IRect {
            left: 5,
            top: 7,
            right: 25,
            bottom: 27,
        },
        true,
    );
    assert_eq!(cf_plan.action, ColorFilterAction::SolidColor1x1);
    assert_eq!(
        cf_plan.bounds,
        IRect {
            left: 5,
            top: 7,
            right: 6,
            bottom: 8
        }
    );

    // Transform plan
    let xform_plan = plan_apply_transform(
        &meta,
        IRect {
            left: 0,
            top: 0,
            right: 200,
            bottom: 200,
        },
        &Matrix::translate(10.0, 20.0),
        SamplingOptions::DEFAULT_LINEAR,
    );
    assert_eq!(xform_plan.action, TransformAction::MergeDeferred);

    // Resolve plan
    let res_plan = plan_resolve(
        &meta,
        IRect {
            left: 10,
            top: 10,
            right: 90,
            bottom: 90,
        },
        false,
    );
    assert_eq!(res_plan.action, ResolveAction::Subset);

    // Subset + PixelBoundary upgrade
    let sub_plan = plan_subset(
        ISize {
            width: 100,
            height: 100,
        },
        IPoint { x: 0, y: 0 },
        IRect {
            left: 10,
            top: 10,
            right: 90,
            bottom: 90,
        },
        false,
    );
    assert!(sub_plan.valid);
    assert_eq!(
        subset_pixel_boundary(
            IRect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100
            },
            sub_plan.image_subset_rel,
            PixelBoundary::Unknown
        ),
        PixelBoundary::Initialized
    );

    // AutoSurface overflow protection
    assert!(auto_surface_padded_bounds(
        IRect {
            left: SK_MIN_S32,
            top: 0,
            right: 10,
            bottom: 10
        },
        PixelBoundary::Transparent
    )
    .is_none());
    assert_eq!(
        auto_surface_padded_bounds(
            IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            },
            PixelBoundary::Transparent
        ),
        Some(IRect {
            left: -1,
            top: -1,
            right: 11,
            bottom: 11
        })
    );
}

#[test]
fn test_rescale_shader_and_blur_plans() {
    assert_eq!(downscale_step_count(1.0), 0);
    assert_eq!(downscale_step_count(0.9995), 0);
    assert_eq!(downscale_step_count(0.5), 1);
    assert_eq!(downscale_step_count(0.25), 2);
    assert_eq!(downscale_step_count(0.125), 3);

    let scaled = scale_about_center(
        Rect {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 50.0,
        },
        0.5,
        0.5,
    );
    assert_eq!(
        scaled,
        Rect {
            left: -25.0,
            top: -12.5,
            right: 25.0,
            bottom: 12.5
        }
    );

    let meta = make_meta(100, 100, 0, 0);
    let setup = compute_rescale_setup(
        &meta,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        Size {
            width: 0.25,
            height: 0.25,
        },
        false,
        true,
        true,
        true,
    );
    assert_eq!(setup.action, RescaleSetupAction::RunSteps);
    assert_eq!(setup.x_steps, 2);
    assert_eq!(setup.y_steps, 2);

    let step = compute_rescale_step(
        setup.src_rect,
        Rect {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 100.0,
        },
        setup.x_steps,
        setup.y_steps,
        setup.final_scale_x,
        setup.final_scale_y,
        setup.tile_mode,
        setup.defer_periodic_tiling,
    );
    assert_eq!(step.next_x_steps, 1);
    assert_eq!(step.next_y_steps, 1);

    let borders = compute_cf_border_rects(IRect {
        left: 0,
        top: 0,
        right: 10,
        bottom: 10,
    });
    assert_eq!(borders[0].bottom, 1);
    assert_eq!(borders[1].top, 9);

    let draws = compute_tiled_border_draws(
        TileMode::Clamp,
        &Matrix::identity(),
        Rect {
            left: 0.0,
            top: 0.0,
            right: 10.0,
            bottom: 10.0,
        },
        Rect {
            left: -1.0,
            top: -1.0,
            right: 11.0,
            bottom: 11.0,
        },
    );
    assert_eq!(draws.pairs.len(), 8);

    let as_shader = plan_as_shader(
        &meta,
        SamplingOptions::DEFAULT_LINEAR,
        shader_flags::NONE,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        true,
        true,
    );
    assert!(!as_shader.needs_resolve);
    assert_eq!(as_shader.final_sampling, SamplingOptions::NEAREST);

    let sv = plan_shader_view(&meta, as_shader.analysis);
    assert_eq!(sv.effective_tile_mode, TileMode::Clamp);

    let img_plan = plan_make_from_image(
        &Mapping::identity(),
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        ISize {
            width: 100,
            height: 100,
        },
        Rect {
            left: 10.0,
            top: 10.0,
            right: 50.0,
            bottom: 50.0,
        },
        Rect {
            left: 0.0,
            top: 0.0,
            right: 40.0,
            bottom: 40.0,
        },
    );
    assert_eq!(img_plan.action, MakeFromImageAction::DirectSpecial);

    let bp = plan_builder_blur(
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        IRect {
            left: 10,
            top: 10,
            right: 50,
            bottom: 50,
        },
        Size {
            width: 4.0,
            height: 4.0,
        },
        2.0,
    );
    assert!(bp.valid);
    assert_eq!(bp.rescale_scale.width, 0.5);

    let lbp = plan_low_res_blur(
        Size {
            width: 4.0,
            height: 4.0,
        },
        2.0,
        false,
        bp.output_bounds,
        ISize {
            width: 50,
            height: 50,
        },
        &Matrix::scale_translate(2.0, 2.0, 0.0, 0.0),
        TileMode::Decal,
        PixelBoundary::Transparent,
    );
    assert!(lbp.valid);
    assert!(lbp.outset_source_image);
    assert_eq!(lbp.tile_mode, TileMode::Clamp);
}

#[test]
fn test_lru_cache_and_key_hashing() {
    assert_eq!(std::mem::size_of::<CacheKey>(), CACHE_KEY_BYTES_LEN);
    let make_key = |uid: u64| {
        CacheKeyBytes::from_key(&CacheKey {
            unique_id: uid,
            matrix: Matrix::identity(),
            clip_bounds: IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
            src_gen_id: 1,
            src_subset: IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
            pad: 0,
        })
    };

    let k1 = make_key(1);
    let k2 = make_key(2);
    let k3 = make_key(3);
    assert_ne!(k1.hash32(), k2.hash32());

    let mut cache = LruCacheIndex::new(100);
    let (h1, ev1) = cache.set(k1, 0x1000, 60);
    assert!(ev1.is_empty());
    let (h2, ev2) = cache.set(k2, 0x1000, 40);
    assert!(ev2.is_empty());
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.current_bytes(), 100);

    // Promote k1 so k2 becomes tail
    assert_eq!(cache.get(&k1), Some(h1));
    // Insert k3 (40 bytes) => 140 > 100, evicts k2 (40) => 100 <= 100
    let (h3, ev3) = cache.set(k3, 0x2000, 40);
    assert_eq!(ev3, vec![h2]);
    assert_eq!(cache.get(&k2), None);
    assert_eq!(cache.get(&k3), Some(h3));

    // Purge by filter 0x1000 => evicts k1 only
    let ev_f = cache.purge_by_filter(0x1000);
    assert_eq!(ev_f, vec![h1]);
    assert_eq!(cache.count(), 1);

    // Oversized single entry (200 > 100) evicts k3 but never self-evicts
    let (h4, ev4) = cache.set(k2, 0x3000, 200);
    assert_eq!(ev4, vec![h3]);
    assert_eq!(cache.count(), 1);
    assert_eq!(cache.get(&k2), Some(h4));

    // Purge all remaining
    let ev_p = cache.purge();
    assert_eq!(ev_p, vec![h4]);
    assert_eq!(cache.count(), 0);
    assert_eq!(cache.current_bytes(), 0);
}

#[test]
fn test_core_filter_helpers() {
    let a = next_image_filter_unique_id();
    let b = next_image_filter_unique_id();
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b);

    assert!(validate_common_input_count(2, 2));
    assert!(validate_common_input_count(5, -1));
    assert!(!validate_common_input_count(-1, -1));
    assert!(!validate_common_input_count(1, 2));

    assert_eq!(
        validate_legacy_crop(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0
            },
            0x0F
        ),
        LegacyCropResult::HasCrop
    );
    assert_eq!(
        validate_legacy_crop(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0
            },
            0x03
        ),
        LegacyCropResult::Invalid
    );

    assert_eq!(
        child_input_layer_bounds_null(
            IRect {
                left: 0,
                top: 0,
                right: 20,
                bottom: 20
            },
            Some(IRect {
                left: 5,
                top: 5,
                right: 15,
                bottom: 15
            })
        ),
        IRect {
            left: 5,
            top: 5,
            right: 15,
            bottom: 15
        }
    );

    assert_eq!(
        combine_ctm_capability(MatrixCapability::Complex, MatrixCapability::Translate),
        MatrixCapability::Translate
    );

    assert_eq!(
        validate_local_matrix(&Matrix::identity(), MatrixCapability::Translate).action,
        LocalMatrixAction::ReturnInput
    );
    assert_eq!(
        validate_local_matrix(
            &Matrix::scale_translate(2.0, 2.0, 0.0, 0.0),
            MatrixCapability::Translate
        )
        .action,
        LocalMatrixAction::Reject
    );
    assert_eq!(
        validate_local_matrix(
            &Matrix::scale_translate(2.0, 2.0, 5.0, 6.0),
            MatrixCapability::ScaleTranslate
        )
        .action,
        LocalMatrixAction::Create
    );
}

#[test]
fn test_effects_logic() {
    // Blend
    assert_eq!(
        classify_blend_factory(BlendMode::Clear),
        BlendFactoryAction::ReturnEmpty
    );
    assert!(blend_affects_transparent_black(false, true, 0.5));
    assert!(!blend_affects_transparent_black(false, true, 0.0));
    assert_eq!(
        blend_null_shader_action(true, false, false, Some(BlendMode::SrcOver)),
        BlendNullShaderAction::ReturnBg
    );
    // Modulate: dst == kSC => Intersect in onGetOutputLayerBounds (include_sc=true), BgOnly in fastBounds (include_sc=false)
    assert_eq!(
        classify_blend_bounds_rule(Some(BlendMode::Modulate), None, true),
        BlendBoundsRule::Intersect
    );
    assert_eq!(
        classify_blend_bounds_rule(Some(BlendMode::Modulate), None, false),
        BlendBoundsRule::BgOnly
    );
    assert_eq!(
        combine_blend_output_bounds(
            BlendBoundsRule::Intersect,
            Some(IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            }),
            Some(IRect {
                left: 5,
                top: 5,
                right: 15,
                bottom: 15
            })
        ),
        Some(IRect {
            left: 5,
            top: 5,
            right: 10,
            bottom: 10
        })
    );
    assert_eq!(
        combine_blend_fast_bounds(
            BlendBoundsRule::Union,
            Rect {
                left: 0.0,
                top: 0.0,
                right: 5.0,
                bottom: 5.0
            },
            Rect {
                left: 10.0,
                top: 10.0,
                right: 20.0,
                bottom: 20.0
            }
        ),
        Rect {
            left: 0.0,
            top: 0.0,
            right: 20.0,
            bottom: 20.0
        }
    );

    // Blur
    assert!(validate_blur_params(0.0, 5.0));
    assert!(!validate_blur_params(-1.0, 5.0));
    let id_map = Mapping::identity();
    assert_eq!(
        map_blur_sigma(
            &id_map,
            Size {
                width: 1000.0,
                height: 0.01
            }
        ),
        Size {
            width: 532.0,
            height: 0.0
        }
    );
    assert_eq!(
        blur_kernel_bounds(
            &id_map,
            Size {
                width: 2.0,
                height: 3.0
            },
            IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            }
        ),
        IRect {
            left: -6,
            top: -9,
            right: 16,
            bottom: 19
        }
    );
    assert_eq!(
        blur_fast_bounds(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0
            },
            Size {
                width: 2.0,
                height: 3.0
            }
        ),
        Rect {
            left: -6.0,
            top: -9.0,
            right: 16.0,
            bottom: 19.0
        }
    );

    // Crop
    let crop = Rect {
        left: 10.4,
        top: 10.4,
        right: 20.6,
        bottom: 20.6,
    };
    assert_eq!(
        crop_layer_rect(&id_map, crop, TileMode::Decal),
        IRect {
            left: 10,
            top: 10,
            right: 21,
            bottom: 21
        }
    );
    assert_eq!(
        crop_layer_rect(&id_map, crop, TileMode::Clamp),
        IRect {
            left: 11,
            top: 11,
            right: 20,
            bottom: 20
        }
    );
    assert!(crop_output_layer_bounds(
        &id_map,
        crop,
        TileMode::Clamp,
        Some(IRect {
            left: 0,
            top: 0,
            right: 50,
            bottom: 50
        })
    )
    .is_none());
    assert_eq!(
        crop_required_input(
            &id_map,
            crop,
            TileMode::Decal,
            IRect {
                left: 0,
                top: 0,
                right: 15,
                bottom: 15
            }
        ),
        IRect {
            left: 10,
            top: 10,
            right: 15,
            bottom: 15
        }
    );
    assert_eq!(
        crop_fast_bounds(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 100.0,
                bottom: 100.0
            },
            Rect {
                left: 10.0,
                top: 10.0,
                right: 20.0,
                bottom: 20.0
            },
            TileMode::Decal
        ),
        Rect {
            left: 10.0,
            top: 10.0,
            right: 20.0,
            bottom: 20.0
        }
    );

    // Displacement
    assert!(validate_displacement_params(0, 3, 20.0));
    assert!(!validate_displacement_params(4, 0, 20.0));
    assert_eq!(displacement_channel_selector(1), [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(
        displacement_outset_bounds(
            &id_map,
            20.0,
            IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            }
        ),
        IRect {
            left: -10,
            top: -10,
            right: 20,
            bottom: 20
        }
    );
    assert_eq!(
        displacement_fast_bounds(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0
            },
            -20.0
        ),
        Rect {
            left: -10.0,
            top: -10.0,
            right: 20.0,
            bottom: 20.0
        }
    );

    // Image
    let (asrc, adst) = adjust_image_filter_rects(
        ISize {
            width: 50,
            height: 50,
        },
        Rect {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 100.0,
        },
        Rect {
            left: 0.0,
            top: 0.0,
            right: 200.0,
            bottom: 200.0,
        },
    )
    .unwrap();
    assert_eq!(
        asrc,
        Rect {
            left: 0.0,
            top: 0.0,
            right: 50.0,
            bottom: 50.0
        }
    );
    assert_eq!(
        adst,
        Rect {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 100.0
        }
    );

    // Lighting
    assert!(validate_lighting_params(
        1.0,
        2.0,
        3.0,
        Point::new(0.0, 0.0),
        10.0,
        FVector { x: 0.0, y: 0.0 },
        1.0,
        1.0,
        0.5
    ));
    assert_eq!(map_z_to_layer(&id_map, 5.0), 5.0);
    let (sdir, cos_c) = spot_light_dir_and_cos_cutoff(
        Point3::new(1.0, 2.0, 3.0),
        Point3::new(4.0, 6.0, 3.0),
        60.0,
    );
    assert_eq!(sdir, Point3::new(3.0, 4.0, 0.0));
    assert!((cos_c - 0.5).abs() < 1e-5);
    let lu = compute_lighting_uniforms(
        &id_map,
        IRect {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        },
        LightType::Distant,
        0xFF_FF_80_00,
        Point::new(0.0, 0.0),
        0.0,
        FVector { x: 3.0, y: 4.0 },
        0.0,
        0.0,
        0.0,
        MaterialType::Diffuse,
        2.0,
        1.0,
        0.0,
    );
    assert!((lu.light_dir_and_spot_cutoff[0] - 0.6).abs() < 1e-6);
    assert!((lu.light_dir_and_spot_cutoff[1] - 0.8).abs() < 1e-6);
    assert_eq!(lu.material_and_light_type[3], -1.0);
    assert_eq!(
        lighting_clamp_rect(
            IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            },
            IRect {
                left: -1,
                top: -1,
                right: 11,
                bottom: 11
            },
            IRect {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10
            }
        ),
        IRect {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10
        }
    );

    // Magnifier
    let lens = Rect {
        left: 0.0,
        top: 0.0,
        right: 100.0,
        bottom: 100.0,
    };
    assert_eq!(
        validate_magnifier_params(lens, 2.0, 10.0),
        MagnifierValidation::Valid
    );
    assert_eq!(
        validate_magnifier_params(lens, 1.0, 10.0),
        MagnifierValidation::NoOp
    );
    let mag_plan = plan_magnifier_filter(
        &id_map,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
        lens,
        2.0,
        10.0,
        Some(IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        }),
    );
    assert_eq!(mag_plan.action, MagnifierFilterAction::Shader);
    assert!((mag_plan.inv_inset_uniform[0] - 0.1).abs() < 1e-6);
    assert_eq!(
        magnifier_output_bounds(&id_map, lens, None),
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100
        }
    );
    assert_eq!(magnifier_fast_bounds(lens, lens), lens);

    // MatrixConvolution
    assert!(validate_matrix_convolution_params(
        ISize {
            width: 3,
            height: 3
        },
        true,
        IPoint { x: 1, y: 1 }
    ));
    assert!(!validate_matrix_convolution_params(
        ISize {
            width: 20,
            height: 20
        },
        true,
        IPoint { x: 1, y: 1 }
    ));
    assert_eq!(
        quantize_matrix_conv_kernel(9),
        (28, MatrixConvKernelTier::Uniforms)
    );
    assert_eq!(
        quantize_matrix_conv_kernel(28),
        (64, MatrixConvKernelTier::TexSm)
    );
    assert_eq!(
        quantize_matrix_conv_kernel(65),
        (256, MatrixConvKernelTier::TexLg)
    );
    let mut kbuf = [0u8; 64];
    let (ig, ib) = encode_matrix_conv_kernel_a8(&[0.0, 0.5, 1.0], &mut kbuf);
    assert_eq!(ig, 1.0);
    assert_eq!(ib, 0.0);
    assert_eq!(&kbuf[0..4], &[0, 128, 255, 0]);
    assert_eq!(
        matrix_conv_bounds_sampled(
            IRect {
                left: 10,
                top: 10,
                right: 20,
                bottom: 20
            },
            ISize {
                width: 3,
                height: 3
            },
            IPoint { x: 1, y: 1 }
        ),
        IRect {
            left: 9,
            top: 9,
            right: 21,
            bottom: 21
        }
    );
    assert_eq!(
        matrix_conv_bounds_affected(
            IRect {
                left: 10,
                top: 10,
                right: 20,
                bottom: 20
            },
            ISize {
                width: 3,
                height: 3
            },
            IPoint { x: 1, y: 1 }
        ),
        IRect {
            left: 9,
            top: 9,
            right: 21,
            bottom: 21
        }
    );

    // MatrixTransform
    let xform = Matrix::translate(5.0, -3.0);
    assert_eq!(
        matrix_transform_required_input(
            &id_map,
            &xform,
            IRect {
                left: 10,
                top: 10,
                right: 20,
                bottom: 20
            },
            true
        ),
        IRect {
            left: 5,
            top: 13,
            right: 15,
            bottom: 23
        }
    );
    assert_eq!(
        matrix_transform_output_bounds(
            &id_map,
            &xform,
            Some(IRect {
                left: 5,
                top: 13,
                right: 15,
                bottom: 23
            })
        ),
        Some(IRect {
            left: 10,
            top: 10,
            right: 20,
            bottom: 20
        })
    );

    // Morphology
    assert_eq!(
        validate_morphology_params(Size {
            width: 0.0,
            height: 0.0
        }),
        MorphValidation::Identity
    );
    assert_eq!(
        morphology_map_radii(
            &id_map,
            Size {
                width: 500.0,
                height: 3.4
            }
        ),
        ISize {
            width: 256,
            height: 3
        }
    );
    assert_eq!(
        morphology_kernel_output_bounds(
            MorphType::Erode,
            &id_map,
            Size {
                width: 2.0,
                height: 3.0
            },
            IRect {
                left: 0,
                top: 0,
                right: 20,
                bottom: 20
            }
        ),
        IRect {
            left: 2,
            top: 3,
            right: 18,
            bottom: 17
        }
    );
    assert_eq!(
        morphology_fast_bounds(
            MorphType::Dilate,
            Size {
                width: 2.0,
                height: 3.0
            },
            Rect {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0
            }
        ),
        Rect {
            left: -2.0,
            top: -3.0,
            right: 12.0,
            bottom: 13.0
        }
    );
    let passes = plan_morphology_passes(
        MorphDirection::X,
        256,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        },
    );
    assert_eq!(passes.len(), 6);
    assert!(passes[0].is_linear && passes[0].step_radius == 14);
    assert!(!passes[1].is_linear && passes[1].step_radius == 14);
    assert_eq!(passes.iter().map(|p| p.step_radius).sum::<i32>(), 256);
    assert_eq!(
        passes.last().unwrap().step_desired_output,
        IRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100
        }
    );

    // Picture
    assert_eq!(
        picture_intersect_cull_rect(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 50.0,
                bottom: 50.0
            },
            Rect {
                left: 25.0,
                top: 25.0,
                right: 100.0,
                bottom: 100.0
            }
        ),
        Some(Rect {
            left: 25.0,
            top: 25.0,
            right: 50.0,
            bottom: 50.0
        })
    );
}
