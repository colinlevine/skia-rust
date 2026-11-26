//! Safe Rust device algorithms matching:
//! - `src/core/SkDevice.cpp`
//! - `src/core/SkBitmapDevice.cpp`
//! - `src/core/SkClipStackDevice.cpp`
//! - `src/image/SkTiledImageUtils.cpp`

use skia_rs_core_types::{alpha_255_to_256, alpha_mul, scalar_floor_to_int};
use skia_rs_geometry::{
    matrix::TRANSLATE_MASK, IPoint, IRect, Matrix, Point, Rect, M44,
};

use crate::{
    irect_from_xywh, is_int_f32, is_scale_translate_as_m33, m44_as_m33, matrix_map_rect,
    rect_from_irect, rect_from_wh, scalar_is_int, ALPHA_TYPE_OPAQUE, CLIP_OP_INTERSECT,
    COLOR_TYPE_UNKNOWN, PAINT_STYLE_FILL,
};

/// `SkDrawTiler::kMaxDim = 8192 - 1`.
pub const DRAW_TILER_MAX_DIM: i32 = 8191;

/// Result of `SkDevice::setDeviceCoordinateSystem`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DeviceCoordSystem {
    pub device_to_global: M44,
    pub global_to_device: M44,
    pub local_to_device: M44,
    pub local_to_device_33: Matrix,
}

/// Result of `SkDevice::setGlobalCTM`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DeviceGlobalCtmResult {
    pub local_to_device: M44,
    pub local_to_device_33: Matrix,
}

/// State of `SkNoPixelsDevice::ClipState`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NoPixelsClipState {
    pub clip_bounds: IRect,
    pub is_aa: bool,
    pub is_rect: bool,
}

/// Result of `SkDrawTiler` initialization.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DrawTilerInit {
    pub needs_tiling: bool,
    pub done: bool,
    pub src_bounds: IRect,
    pub origin: IPoint,
}

/// Result of `SkDrawTiler::stepAndSetupTileDraw`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DrawTilerStep {
    pub origin: IPoint,
    pub done: bool,
    pub tile_bounds: IRect,
    pub tile_matrix: Matrix,
}

/// Result of `valid_for_bitmap_device`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BitmapDeviceValidation {
    pub valid: bool,
    pub new_alpha_type: i32,
}

/// Action plan for `SkBitmapDevice::drawImageRect`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BitmapImageRectPlan {
    /// `0` = skip (clipped out / non-finite), `1` = use shader directly,
    /// `2` = extract subset then check/draw, `3` = use drawBitmap if canApply else shader.
    pub action: i32,
    pub matrix: Matrix,
    pub tmp_src: Rect,
    pub dst_rect: Rect,
    pub use_tmp_dst: bool,
    pub src_ir: IRect,
}

/// Matches `SkDevice::setDeviceCoordinateSystem`.
pub fn set_device_coordinate_system(
    mut device_to_global: M44,
    mut global_to_device: M44,
    mut local_to_device: M44,
    buffer_origin_x: i32,
    buffer_origin_y: i32,
) -> DeviceCoordSystem {
    device_to_global.normalize_perspective();
    global_to_device.normalize_perspective();
    local_to_device.normalize_perspective();

    if (buffer_origin_x | buffer_origin_y) != 0 {
        let bx = buffer_origin_x as f32;
        let by = buffer_origin_y as f32;
        device_to_global.pre_translate(bx, by, 0.0);
        global_to_device.post_translate(-bx, -by, 0.0);
        local_to_device.post_translate(-bx, -by, 0.0);
    }

    let local_to_device_33 = m44_as_m33(&local_to_device);
    DeviceCoordSystem {
        device_to_global,
        global_to_device,
        local_to_device,
        local_to_device_33,
    }
}

/// Matches `SkDevice::setGlobalCTM`.
pub fn set_global_ctm(mut ctm: M44, global_to_device: &M44) -> DeviceGlobalCtmResult {
    ctm.normalize_perspective();
    let mut local_to_device = M44::identity();
    local_to_device.set_concat(global_to_device, &ctm);
    let local_to_device_33 = m44_as_m33(&local_to_device);
    DeviceGlobalCtmResult {
        local_to_device,
        local_to_device_33,
    }
}

/// Matches `SkDevice::isPixelAlignedToGlobal`.
pub fn is_pixel_aligned_to_global(device_to_global: &M44) -> bool {
    let mut pixel_aligned = M44::identity();
    pixel_aligned.set_rc(0, 3, device_to_global.rc(0, 3).floor());
    pixel_aligned.set_rc(1, 3, device_to_global.rc(1, 3).floor());
    pixel_aligned == *device_to_global
}

/// Matches `SkDevice::getOrigin`.
#[inline]
pub fn get_origin(device_to_global: &M44) -> IPoint {
    IPoint {
        x: scalar_floor_to_int(device_to_global.rc(0, 3)),
        y: scalar_floor_to_int(device_to_global.rc(1, 3)),
    }
}

/// Matches `SkDevice::getRelativeTransform`.
#[inline]
pub fn get_relative_transform(src_device_to_global: &M44, dst_global_to_device: &M44) -> M44 {
    let mut out = M44::identity();
    out.set_concat(dst_global_to_device, src_device_to_global);
    out
}

/// Matches `SkDevice::drawRegion` path-fallback condition.
pub fn device_draw_region_needs_path(
    local_to_device: &Matrix,
    paint_style: i32,
    has_mask_filter: bool,
    has_path_effect: bool,
    is_aa: bool,
) -> bool {
    let is_non_translate = (local_to_device.compute_type_mask() as i32 & !TRANSLATE_MASK) != 0;
    let complex_paint = paint_style != PAINT_STYLE_FILL || has_mask_filter || has_path_effect;
    let anti_alias = is_aa
        && (!is_int_f32(local_to_device.mat[2]) || !is_int_f32(local_to_device.mat[5]));
    is_non_translate || complex_paint || anti_alias
}

/// Matches `SkDevice::drawDevice` strict constraint check.
pub fn device_draw_device_is_strict(is_nearest: bool, relative_transform: &Matrix) -> bool {
    if !is_nearest {
        return true;
    }
    let mask = relative_transform.compute_type_mask() as i32;
    let is_translate_only = (mask & !TRANSLATE_MASK) == 0;
    !is_translate_only
        || !scalar_is_int(relative_transform.mat[2])
        || !scalar_is_int(relative_transform.mat[5])
}

/// Matches `SkDevice::drawImageLattice` single-pixel alpha modulation.
#[inline]
pub fn device_lattice_modulate_color(color: u32, paint_alpha: u8) -> u32 {
    let c_alpha = ((color >> 24) & 0xFF) as u8;
    let alpha = alpha_mul(c_alpha, alpha_255_to_256(paint_alpha)) as u32;
    (color & 0x00FF_FFFF) | (alpha << 24)
}

/// Matches `quad_to_tris` in `SkDevice.cpp`.
#[inline]
pub fn quad_to_tris(quad: [Point; 4]) -> [Point; 6] {
    [quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]
}

/// Matches `SkDevice::scalerContextFlags`:
/// `1` = `kBoostContrast`, `3` = `kFakeGammaAndBoostContrast`.
#[inline]
pub fn scaler_context_flags(has_cs: bool, gamma_is_linear: bool) -> u32 {
    if has_cs && gamma_is_linear {
        1 // SkScalerContextFlags::kBoostContrast
    } else {
        3 // SkScalerContextFlags::kFakeGammaAndBoostContrast
    }
}

/// Matches `SkNoPixelsDevice::ClipState::op`.
pub fn no_pixels_clip_state_op(
    mut state: NoPixelsClipState,
    op: i32,
    transform: &M44,
    bounds: Rect,
    is_aa: bool,
    fills_bounds: bool,
) -> NoPixelsClipState {
    let is_rect = fills_bounds && is_scale_translate_as_m33(transform);
    state.is_aa |= is_aa;

    let dev_bounds = if bounds.is_empty() {
        Rect::default()
    } else {
        transform.map_rect(bounds)
    };

    if op == CLIP_OP_INTERSECT {
        let clip_r = if is_aa {
            dev_bounds.round_out()
        } else {
            dev_bounds.round()
        };
        let mut inter = IRect::default();
        if inter.intersect_pair(state.clip_bounds, clip_r) {
            state.clip_bounds = inter;
        } else {
            state.clip_bounds = IRect::default();
        }
        state.is_rect &= is_rect;
    } else if is_rect {
        let sub_r = if is_aa {
            dev_bounds.round_in()
        } else {
            dev_bounds.round()
        };
        let (diff, ok) = IRect::subtract(state.clip_bounds, sub_r);
        if ok {
            state.clip_bounds = diff;
        } else {
            state.is_rect = false;
        }
    } else {
        state.is_rect = false;
    }

    state
}

/// Matches `SkNoPixelsDevice::replaceClip` bounds calculation.
pub fn no_pixels_replace_clip_bounds(
    global_to_device: &M44,
    rect: IRect,
    device_bounds: IRect,
) -> IRect {
    let device_rect = global_to_device.map_rect(rect_from_irect(rect)).round();
    let mut out = IRect::default();
    if out.intersect_pair(device_rect, device_bounds) {
        out
    } else {
        IRect::default()
    }
}

/// Matches `SkDrawTiler::NeedsTiling`.
#[inline]
pub fn draw_tiler_needs_tiling(width: i32, height: i32) -> bool {
    width > DRAW_TILER_MAX_DIM || height > DRAW_TILER_MAX_DIM
}

/// Matches `SkDrawTiler` constructor logic in `SkBitmapDevice.cpp`.
pub fn draw_tiler_init(
    clip_r: IRect,
    local_to_device: &Matrix,
    bounds: Option<Rect>,
) -> DrawTilerInit {
    let mut needs_tiling = clip_r.right > DRAW_TILER_MAX_DIM || clip_r.bottom > DRAW_TILER_MAX_DIM;
    let mut done = false;
    let mut src_bounds = IRect::default();

    if needs_tiling {
        if let Some(b) = bounds {
            let mapped = matrix_map_rect(local_to_device, b).round_out();
            let mut inter = IRect::default();
            if inter.intersect_pair(mapped, clip_r) {
                src_bounds = inter;
                needs_tiling =
                    src_bounds.right > DRAW_TILER_MAX_DIM || src_bounds.bottom > DRAW_TILER_MAX_DIM;
            } else {
                needs_tiling = false;
                done = true;
            }
        } else {
            src_bounds = clip_r;
        }
    }

    let origin = if needs_tiling {
        IPoint {
            x: src_bounds.left - DRAW_TILER_MAX_DIM,
            y: src_bounds.top,
        }
    } else {
        IPoint { x: 0, y: 0 }
    };

    DrawTilerInit {
        needs_tiling,
        done,
        src_bounds,
        origin,
    }
}

/// Matches `SkDrawTiler::stepAndSetupTileDraw` in `SkBitmapDevice.cpp`.
pub fn draw_tiler_step(
    mut origin: IPoint,
    src_bounds: IRect,
    local_to_device: &Matrix,
) -> DrawTilerStep {
    if origin.x >= src_bounds.right - DRAW_TILER_MAX_DIM {
        origin.x = src_bounds.left;
        origin.y += DRAW_TILER_MAX_DIM;
    } else {
        origin.x += DRAW_TILER_MAX_DIM;
    }

    let done = origin.x >= src_bounds.right - DRAW_TILER_MAX_DIM
        && origin.y >= src_bounds.bottom - DRAW_TILER_MAX_DIM;

    let tile_bounds = irect_from_xywh(origin.x, origin.y, DRAW_TILER_MAX_DIM, DRAW_TILER_MAX_DIM);
    let mut tile_matrix = *local_to_device;
    tile_matrix.post_translate(-(origin.x as f32), -(origin.y as f32));

    DrawTilerStep {
        origin,
        done,
        tile_bounds,
        tile_matrix,
    }
}

/// Matches `valid_for_bitmap_device` in `SkBitmapDevice.cpp`.
#[inline]
pub fn valid_for_bitmap_device(
    width: i32,
    height: i32,
    color_type: i32,
    alpha_type: i32,
    color_type_always_opaque: bool,
) -> BitmapDeviceValidation {
    if width < 0 || height < 0 || color_type == COLOR_TYPE_UNKNOWN {
        return BitmapDeviceValidation {
            valid: false,
            new_alpha_type: alpha_type,
        };
    }
    let new_alpha_type = if color_type_always_opaque {
        ALPHA_TYPE_OPAQUE
    } else {
        alpha_type
    };
    BitmapDeviceValidation {
        valid: true,
        new_alpha_type,
    }
}

/// Matches `CanApplyDstMatrixAsCTM` in `SkBitmapDevice.cpp`.
#[inline]
pub fn can_apply_dst_matrix_as_ctm(m: &Matrix, has_mask_filter: bool) -> bool {
    if !has_mask_filter {
        return true;
    }
    (m.compute_type_mask() as i32) <= TRANSLATE_MASK
}

/// Matches `SkBitmapDevice::drawImageRect` geometric planning.
pub fn bitmap_device_image_rect_plan(
    bm_width: i32,
    bm_height: i32,
    src: Option<Rect>,
    dst: Rect,
    is_fast_constraint: bool,
    is_default_sampling: bool,
) -> BitmapImageRectPlan {
    let bitmap_bounds = rect_from_wh(bm_width as f32, bm_height as f32);
    let mut tmp_src = src.unwrap_or(bitmap_bounds);

    let mut matrix = Matrix::rect_to_rect(tmp_src, dst, skia_rs_geometry::matrix::ScaleToFit::Fill)
        .unwrap_or_else(Matrix::identity);

    let mut dst_rect = dst;
    let mut use_tmp_dst = false;
    let mut src_is_subset = false;

    if let Some(s) = src {
        if !bitmap_bounds.contains_rect(s) {
            if !tmp_src.intersect(bitmap_bounds) {
                return BitmapImageRectPlan {
                    action: 0,
                    matrix,
                    tmp_src,
                    dst_rect,
                    use_tmp_dst: false,
                    src_ir: IRect::default(),
                };
            }
            let mapped = matrix_map_rect(&matrix, tmp_src);
            if !mapped.is_finite() {
                return BitmapImageRectPlan {
                    action: 0,
                    matrix,
                    tmp_src,
                    dst_rect: mapped,
                    use_tmp_dst: true,
                    src_ir: IRect::default(),
                };
            }
            dst_rect = mapped;
            use_tmp_dst = true;
        }
        src_is_subset = !tmp_src.contains_rect(bitmap_bounds);
    }

    if src_is_subset && is_fast_constraint && !is_default_sampling {
        return BitmapImageRectPlan {
            action: 1, // USE_SHADER
            matrix,
            tmp_src,
            dst_rect,
            use_tmp_dst,
            src_ir: IRect::default(),
        };
    }

    if src_is_subset {
        let src_ir = tmp_src.round_out();
        let dx = if src_ir.left > 0 { src_ir.left as f32 } else { 0.0 };
        let dy = if src_ir.top > 0 { src_ir.top as f32 } else { 0.0 };
        if dx != 0.0 || dy != 0.0 {
            matrix.pre_translate(dx, dy);
        }
        return BitmapImageRectPlan {
            action: 2, // EXTRACT_SUBSET
            matrix,
            tmp_src,
            dst_rect,
            use_tmp_dst,
            src_ir,
        };
    }

    BitmapImageRectPlan {
        action: 3, // USE_DRAWBITMAP_OR_SHADER
        matrix,
        tmp_src,
        dst_rect,
        use_tmp_dst,
        src_ir: IRect::default(),
    }
}

/// Matches `SkBitmapDevice::isClipWideOpen`.
#[inline]
pub fn bitmap_device_is_clip_wide_open(
    is_bw: bool,
    bw_is_rect: bool,
    bw_bounds: IRect,
    width: i32,
    height: i32,
) -> bool {
    is_bw
        && bw_is_rect
        && bw_bounds == IRect::from_ltrb(0, 0, width, height)
}

/// Matches `SkBitmapDevice::isClipRect`.
#[inline]
pub fn bitmap_device_is_clip_rect(rc_empty: bool, rc_is_rect: bool, has_clip_shader: bool) -> bool {
    !rc_empty && rc_is_rect && !has_clip_shader
}

/// Matches `SkTiledImageUtils::GetImageKeyValues` for bitmap-backed images.
pub fn tiled_image_bitmap_key_values(
    gen_id: u32,
    image_bounds: IRect,
    pixel_ref_origin: IPoint,
) -> [u32; 6] {
    let left = image_bounds.left.wrapping_add(pixel_ref_origin.x);
    let top = image_bounds.top.wrapping_add(pixel_ref_origin.y);
    let right = image_bounds.right.wrapping_add(pixel_ref_origin.x);
    let bottom = image_bounds.bottom.wrapping_add(pixel_ref_origin.y);
    [gen_id, 0, left as u32, top as u32, right as u32, bottom as u32]
}

/// Matches `SkTiledImageUtils::GetImageKeyValues` fallback (uniqueID).
#[inline]
pub fn tiled_image_unique_id_key_values(unique_id: u32) -> [u32; 6] {
    [unique_id, 0, 0, 0, 0, 0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CLIP_OP_DIFFERENCE;

    #[test]
    fn test_device_coord_system_and_origin() {
        let cs = set_device_coordinate_system(
            M44::identity(),
            M44::identity(),
            M44::identity(),
            10,
            20,
        );
        assert!(is_pixel_aligned_to_global(&cs.device_to_global));
        assert_eq!(get_origin(&cs.device_to_global), IPoint { x: 10, y: 20 });
        assert_eq!(cs.global_to_device.rc(0, 3), -10.0);
        assert_eq!(cs.local_to_device_33.mat[2], -10.0);
        assert_eq!(cs.local_to_device_33.mat[5], -20.0);
    }

    #[test]
    fn test_no_pixels_clip_state_op() {
        let init = NoPixelsClipState {
            clip_bounds: IRect::from_ltrb(0, 0, 100, 100),
            is_aa: false,
            is_rect: true,
        };
        let after_i = no_pixels_clip_state_op(
            init,
            CLIP_OP_INTERSECT,
            &M44::identity(),
            Rect::from_ltrb(10.0, 20.0, 80.0, 90.0),
            false,
            true,
        );
        assert_eq!(after_i.clip_bounds, IRect::from_ltrb(10, 20, 80, 90));
        assert!(after_i.is_rect);
        assert!(!after_i.is_aa);

        let after_d = no_pixels_clip_state_op(
            after_i,
            CLIP_OP_DIFFERENCE,
            &M44::identity(),
            Rect::from_ltrb(0.0, 20.0, 100.0, 50.0),
            false,
            true,
        );
        assert_eq!(after_d.clip_bounds, IRect::from_ltrb(10, 50, 80, 90));
        assert!(after_d.is_rect);
    }

    #[test]
    fn test_draw_tiler_step() {
        assert!(!draw_tiler_needs_tiling(8191, 8191));
        assert!(draw_tiler_needs_tiling(8192, 100));

        let clip = IRect::from_ltrb(0, 0, 10000, 10000);
        let init = draw_tiler_init(clip, &Matrix::identity(), None);
        assert!(init.needs_tiling);
        assert!(!init.done);
        assert_eq!(init.origin, IPoint { x: -8191, y: 0 });

        let s1 = draw_tiler_step(init.origin, init.src_bounds, &Matrix::identity());
        assert_eq!(s1.origin, IPoint { x: 0, y: 0 });
        assert!(!s1.done);

        let s2 = draw_tiler_step(s1.origin, init.src_bounds, &Matrix::identity());
        assert_eq!(s2.origin, IPoint { x: 8191, y: 0 });
        assert!(!s2.done);

        let s3 = draw_tiler_step(s2.origin, init.src_bounds, &Matrix::identity());
        assert_eq!(s3.origin, IPoint { x: 0, y: 8191 });
        assert!(!s3.done);

        let s4 = draw_tiler_step(s3.origin, init.src_bounds, &Matrix::identity());
        assert_eq!(s4.origin, IPoint { x: 8191, y: 8191 });
        assert!(s4.done);
    }

    #[test]
    fn test_tiled_image_key_values() {
        let keys = tiled_image_bitmap_key_values(
            42,
            IRect::from_ltrb(0, 0, 64, 32),
            IPoint { x: 5, y: 7 },
        );
        assert_eq!(keys, [42, 0, 5, 7, 69, 39]);
        assert_eq!(tiled_image_unique_id_key_values(99), [99, 0, 0, 0, 0, 0]);
    }
}
