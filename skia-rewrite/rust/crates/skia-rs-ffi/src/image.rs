//! FFI exports for `skia-rs-image` (Layer 3.4b).
//!
//! Covers:
//! - `SkImageGenerator.cpp`
//! - `SkImage.cpp`, `SkImage_Base.cpp`
//! - `SkImage_Lazy.cpp`, `SkImage_LazyFactories.cpp`, `SkImage_Picture.cpp`
//! - `SkImage_Raster.cpp`, `SkImage_RasterFactories.cpp`
//! - `SkRescaleAndReadPixels.cpp`
//! - `SkSurface.cpp`, `SkSurface_Base.cpp`, `SkSurface_Null.cpp`, `SkSurface_Raster.cpp`

use skia_rs_color::image_info::{AlphaType, ColorType};
use skia_rs_geometry::{IRect, ISize, Matrix, Rect};
use skia_rs_image::{
    generator,
    image::{self, MakeRasterImagePlan, MakeScaledAction},
    lazy_picture::{self, LazyValidatorPlan, PictureSubsetPlan, NUM_IMAGE_KEY_VALUES},
    raster::{self, CopyPixelsMode, RasterFromBitmapAction, RasterFromBitmapPlan},
    rescale::{self, RescaleGamma, RescaleMode, RescalePlan},
    surface::{
        self, AboutToDrawAction, ContentChangeMode, RasterSnapshotPlan, SnapshotBoundsAction,
        SnapshotBoundsPlan, SurfacePropsData, WritePixelsAction,
    },
};

#[inline]
fn rect_byte_len(height: usize, row_bytes: usize, copy_bytes_per_row: usize) -> usize {
    if height == 0 || copy_bytes_per_row == 0 {
        return 0;
    }
    (height - 1)
        .saturating_mul(row_bytes)
        .saturating_add(copy_bytes_per_row)
}

#[inline]
unsafe fn slice_from_raw<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}

#[inline]
unsafe fn slice_from_raw_mut<'a, T>(ptr: *mut T, len: usize) -> &'a mut [T] {
    if ptr.is_null() || len == 0 {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(ptr, len)
    }
}

// ============================================================================
// SkImageGenerator
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_image_resolve_unique_id(unique_id: u32) -> u32 {
    generator::resolve_unique_id(unique_id)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_generator_validate_get_pixels(
    width: i32,
    ct_raw: i32,
    has_pixels: bool,
    row_bytes: usize,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    generator::validate_get_pixels_args(width, ct, has_pixels, row_bytes)
}

// ============================================================================
// SkImage & SkImage_Base
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_image_plan_make_scaled(
    src_w: i32,
    src_h: i32,
    src_ct_raw: i32,
    src_at_raw: i32,
    same_color_space: bool,
    new_w: i32,
    new_h: i32,
    new_ct_raw: i32,
    new_at_raw: i32,
) -> i32 {
    let (Some(src_ct), Some(src_at), Some(new_ct), Some(new_at)) = (
        ColorType::from_i32(src_ct_raw),
        AlphaType::from_i32(src_at_raw),
        ColorType::from_i32(new_ct_raw),
        AlphaType::from_i32(new_at_raw),
    ) else {
        return MakeScaledAction::Invalid as i32;
    };
    image::plan_make_scaled(
        src_w,
        src_h,
        src_ct,
        src_at,
        same_color_space,
        new_w,
        new_h,
        new_ct,
        new_at,
    ) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_image_make_scaled_dst_rect(new_w: i32, new_h: i32) -> Rect {
    image::make_scaled_dst_rect(new_w, new_h)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_validate_async_rescale_rgba(
    src_w: i32,
    src_h: i32,
    src_rect: *const IRect,
    dst_w: i32,
    dst_h: i32,
    dst_ct_raw: i32,
    dst_at_raw: i32,
) -> bool {
    if src_rect.is_null() {
        return false;
    }
    let (Some(dst_ct), Some(dst_at)) = (
        ColorType::from_i32(dst_ct_raw),
        AlphaType::from_i32(dst_at_raw),
    ) else {
        return false;
    };
    image::validate_async_rescale_rgba(src_w, src_h, *src_rect, dst_w, dst_h, dst_ct, dst_at)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_validate_async_rescale_yuv420(
    src_w: i32,
    src_h: i32,
    src_rect: *const IRect,
    dst_size: *const ISize,
) -> bool {
    if src_rect.is_null() || dst_size.is_null() {
        return false;
    }
    image::validate_async_rescale_yuv420(src_w, src_h, *src_rect, *dst_size)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_scale_pixels_is_identity_size(
    src_w: i32,
    src_h: i32,
    dst_w: i32,
    dst_h: i32,
) -> bool {
    image::scale_pixels_is_identity_size(src_w, src_h, dst_w, dst_h)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_is_alpha_only(ct_raw: i32) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    image::color_type_is_alpha_only(ct)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_should_reinterpret_return_self(
    same_color_space: bool,
    ct_raw: i32,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    image::should_reinterpret_return_self(same_color_space, ct)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_plan_make_raster_image(
    width: i32,
    height: i32,
    ct_raw: i32,
    out: *mut MakeRasterImagePlan,
) -> bool {
    if out.is_null() {
        return false;
    }
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    match image::plan_make_raster_image(width, height, ct) {
        Some(plan) => {
            *out = plan;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_validate_subset(
    width: i32,
    height: i32,
    subset: *const IRect,
) -> bool {
    if subset.is_null() {
        return false;
    }
    image::validate_subset_bounds(width, height, *subset)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_should_invoke_on_make_with_mipmaps(
    has_mips: bool,
    mips_valid_for_root: bool,
) -> bool {
    image::should_invoke_on_make_with_mipmaps(has_mips, mips_valid_for_root)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_async_read_effective_src_rect(
    orig_src_rect: *const IRect,
    peeked: bool,
) -> IRect {
    if orig_src_rect.is_null() {
        return IRect::make_empty();
    }
    image::async_read_effective_src_rect(*orig_src_rect, peeked)
}

// ============================================================================
// SkImage_Lazy & SkImage_Picture
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_lazy_plan_validator(
    has_generator: bool,
    gen_width: i32,
    gen_height: i32,
    gen_ct_raw: i32,
    gen_unique_id: u32,
    has_color_type: bool,
    requested_ct_raw: i32,
    has_color_space: bool,
    out: *mut LazyValidatorPlan,
) {
    if out.is_null() {
        return;
    }
    let gen_ct = ColorType::from_i32(gen_ct_raw).unwrap_or(ColorType::Unknown);
    let req_ct = ColorType::from_i32(requested_ct_raw).unwrap_or(ColorType::Unknown);
    *out = lazy_picture::plan_lazy_validator(
        has_generator,
        gen_width,
        gen_height,
        gen_ct,
        gen_unique_id,
        has_color_type,
        req_ct,
        has_color_space,
    );
}

#[no_mangle]
pub extern "C" fn skia_rs_image_lazy_can_ref_encoded(gen_unique_id: u32, img_unique_id: u32) -> bool {
    lazy_picture::lazy_can_ref_encoded(gen_unique_id, img_unique_id)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_lazy_color_type_and_space_cache_hit(
    has_cached: bool,
    target_ct_raw: i32,
    cached_ct_raw: i32,
    same_color_space: bool,
) -> bool {
    let (Some(target_ct), Some(cached_ct)) = (
        ColorType::from_i32(target_ct_raw),
        ColorType::from_i32(cached_ct_raw),
    ) else {
        return false;
    };
    lazy_picture::lazy_color_type_and_space_cache_hit(
        has_cached,
        target_ct,
        cached_ct,
        same_color_space,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_image_lazy_yuva_dimensions_match(
    yuva_w: i32,
    yuva_h: i32,
    img_w: i32,
    img_h: i32,
) -> bool {
    lazy_picture::lazy_yuva_dimensions_match(yuva_w, yuva_h, img_w, img_h)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_picture_plan_subset(
    orig_matrix: *const Matrix,
    subset: *const IRect,
    ct_raw: i32,
    out: *mut PictureSubsetPlan,
) -> bool {
    if orig_matrix.is_null() || subset.is_null() || out.is_null() {
        return false;
    }
    let ct = ColorType::from_i32(ct_raw).unwrap_or(ColorType::Unknown);
    *out = lazy_picture::plan_picture_subset(*orig_matrix, *subset, ct);
    true
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_picture_key_values(
    has_paint: bool,
    is_srgb: bool,
    matrix: *const Matrix,
    ct_raw: i32,
    pixel_geometry: u32,
    surface_prop_flags: u32,
    width: i32,
    height: i32,
    picture_unique_id: u32,
    out_keys: *mut u32,
) -> bool {
    if matrix.is_null() || out_keys.is_null() {
        return false;
    }
    let ct = ColorType::from_i32(ct_raw).unwrap_or(ColorType::Unknown);
    match lazy_picture::compute_picture_image_key_values(
        has_paint,
        is_srgb,
        &*matrix,
        ct,
        pixel_geometry,
        surface_prop_flags,
        width,
        height,
        picture_unique_id,
    ) {
        Some(keys) => {
            let dst = slice_from_raw_mut(out_keys, NUM_IMAGE_KEY_VALUES);
            dst.copy_from_slice(&keys);
            true
        }
        None => false,
    }
}

// ============================================================================
// SkImage_Raster & SkImage_RasterFactories
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_image_raster_unique_id_from_bitmap(
    pr_w: i32,
    pr_h: i32,
    bm_w: i32,
    bm_h: i32,
    bm_gen_id: u32,
) -> u32 {
    raster::raster_image_unique_id_from_bitmap(pr_w, pr_h, bm_w, bm_h, bm_gen_id)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_raster_plan_from_bitmap(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    has_pixels: bool,
    is_immutable: bool,
    cpm_raw: i32,
    out: *mut RasterFromBitmapPlan,
) {
    if out.is_null() {
        return;
    }
    let (Some(ct), Some(at), Some(cpm)) = (
        ColorType::from_i32(ct_raw),
        AlphaType::from_i32(at_raw),
        CopyPixelsMode::from_i32(cpm_raw),
    ) else {
        *out = RasterFromBitmapPlan {
            action: RasterFromBitmapAction::Invalid,
            byte_size: 0,
            bitmap_may_be_mutable: false,
        };
        return;
    };
    *out = raster::plan_raster_from_bitmap(
        width,
        height,
        ct,
        at,
        row_bytes,
        has_pixels,
        is_immutable,
        cpm,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_raster_copy_rect_bytes(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    copy_bytes_per_row: usize,
    height: usize,
) {
    let dst_len = rect_byte_len(height, dst_rb, copy_bytes_per_row);
    let src_len = rect_byte_len(height, src_rb, copy_bytes_per_row);
    let dst_slice = slice_from_raw_mut(dst, dst_len);
    let src_slice = slice_from_raw(src, src_len);
    raster::copy_rect_bytes(dst_slice, dst_rb, src_slice, src_rb, copy_bytes_per_row, height);
}

#[no_mangle]
pub extern "C" fn skia_rs_image_raster_can_share_legacy_pixel_ref(
    is_immutable: bool,
    is_temporarily_immutable: bool,
) -> bool {
    raster::can_share_pixel_ref_for_legacy_bitmap(is_immutable, is_temporarily_immutable)
}

#[no_mangle]
pub extern "C" fn skia_rs_image_raster_should_compose_alpha_shader(
    ct_raw: i32,
    paint_has_shader: bool,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    raster::should_compose_alpha_shader(ct, paint_has_shader)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_raster_is_full_subset(
    bm_w: i32,
    bm_h: i32,
    subset: *const IRect,
) -> bool {
    if subset.is_null() {
        return false;
    }
    raster::is_full_bitmap_subset(bm_w, bm_h, *subset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_image_raster_validate_factory_args(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    out_min_size: *mut usize,
) -> bool {
    match raster::validate_raster_factory_args(width, height, ct_raw, at_raw, row_bytes) {
        Some(size) => {
            if !out_min_size.is_null() {
                *out_min_size = size;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn skia_rs_image_raster_validate_from_data(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    has_data: bool,
    data_size: usize,
) -> bool {
    raster::validate_raster_from_data(
        width, height, ct_raw, at_raw, row_bytes, has_data, data_size,
    )
}

#[no_mangle]
pub extern "C" fn skia_rs_image_compressed_alpha_type(is_opaque: bool) -> i32 {
    raster::compressed_texture_alpha_type(is_opaque) as i32
}

// ============================================================================
// SkRescaleAndReadPixels
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rescale_build_plan(
    src_rect: *const IRect,
    dst_width: i32,
    dst_height: i32,
    dst_ct_raw: i32,
    src_has_cs: bool,
    src_cs_is_linear: bool,
    rescale_gamma_raw: u8,
    rescale_mode_raw: i32,
    out: *mut RescalePlan,
) -> bool {
    if src_rect.is_null() || out.is_null() {
        return false;
    }
    let (Some(dst_ct), Some(gamma), Some(mode)) = (
        ColorType::from_i32(dst_ct_raw),
        RescaleGamma::from_u8(rescale_gamma_raw),
        RescaleMode::from_i32(rescale_mode_raw),
    ) else {
        return false;
    };
    *out = rescale::build_rescale_plan(
        *src_rect,
        dst_width,
        dst_height,
        dst_ct,
        src_has_cs,
        src_cs_is_linear,
        gamma,
        mode,
    );
    true
}

// ============================================================================
// SkSurface, SkSurface_Base, SkSurface_Null, SkSurface_Raster
// ============================================================================

#[no_mangle]
pub extern "C" fn skia_rs_surface_props_make(
    flags: u32,
    pixel_geometry: i32,
    text_contrast: f32,
    text_gamma: f32,
) -> SurfacePropsData {
    surface::make_surface_props(flags, pixel_geometry, text_contrast, text_gamma)
}

#[no_mangle]
pub extern "C" fn skia_rs_surface_next_generation_id() -> u32 {
    surface::next_surface_generation_id()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_surface_plan_snapshot_bounds(
    surf_width: i32,
    surf_height: i32,
    src_bounds: *const IRect,
    out: *mut SnapshotBoundsPlan,
) {
    if out.is_null() {
        return;
    }
    if src_bounds.is_null() {
        *out = SnapshotBoundsPlan {
            action: SnapshotBoundsAction::Empty,
            clipped_bounds: IRect::make_empty(),
        };
        return;
    }
    *out = surface::plan_surface_snapshot_bounds(surf_width, surf_height, *src_bounds);
}

#[no_mangle]
pub extern "C" fn skia_rs_surface_plan_write_pixels(
    surf_width: i32,
    surf_height: i32,
    has_addr: bool,
    pm_width: i32,
    pm_height: i32,
    x: i32,
    y: i32,
) -> i32 {
    surface::plan_surface_write_pixels(
        surf_width,
        surf_height,
        has_addr,
        pm_width,
        pm_height,
        x,
        y,
    ) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_surface_plan_about_to_draw(
    has_cached_image: bool,
    cached_image_unique: bool,
    mode_raw: i32,
) -> i32 {
    let mode = ContentChangeMode::from_i32(mode_raw).unwrap_or(ContentChangeMode::Retain);
    surface::plan_surface_about_to_draw(has_cached_image, cached_image_unique, mode) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_surface_outstanding_image_snapshot(
    has_cached_image: bool,
    cached_image_unique: bool,
) -> bool {
    surface::outstanding_image_snapshot(has_cached_image, cached_image_unique)
}

#[no_mangle]
pub extern "C" fn skia_rs_surface_validate_null_dims(width: i32, height: i32) -> bool {
    surface::validate_null_surface_dims(width, height)
}

#[no_mangle]
pub extern "C" fn skia_rs_surface_validate_raster_info(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
) -> bool {
    let (Some(ct), Some(at)) = (
        ColorType::from_i32(ct_raw),
        AlphaType::from_i32(at_raw),
    ) else {
        return false;
    };
    surface::validate_raster_surface_info(width, height, ct, at, row_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_surface_plan_raster_snapshot(
    we_own_pixels: bool,
    out: *mut RasterSnapshotPlan,
) {
    if out.is_null() {
        return;
    }
    *out = surface::plan_raster_surface_snapshot(we_own_pixels);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_surface_copy_backing_pixels(
    dst: *mut u8,
    src: *const u8,
    byte_size: usize,
) {
    let dst_slice = slice_from_raw_mut(dst, byte_size);
    let src_slice = slice_from_raw(src, byte_size);
    surface::copy_surface_backing_pixels(dst_slice, src_slice);
}

// Keep unused enum variants referenced cleanly via type assertions.
const _: () = {
    let _ = WritePixelsAction::Skip;
    let _ = AboutToDrawAction::NoAction;
};
