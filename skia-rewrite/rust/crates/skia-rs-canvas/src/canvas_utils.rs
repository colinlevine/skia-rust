//! Safe Rust canvas utility algorithms matching:
//! - `src/utils/SkCanvasStack.cpp`
//! - `src/utils/SkCanvasStateUtils.cpp`
//! - `src/utils/SkPaintFilterCanvas.cpp`
//! - `src/utils/SkNWayCanvas.cpp`
//! - `src/utils/SkNullCanvas.cpp`

use skia_rs_geometry::{IPoint, IRect, M44};

use crate::{BLEND_MODE_SRC_OVER, COLOR_TYPE_RGB_565, COLOR_TYPE_UNKNOWN};

/// Stable C ABI `RasterConfigs` constants matching `SkCanvasStateUtils.cpp`.
pub const RASTER_CONFIG_UNKNOWN: i32 = 0;
pub const RASTER_CONFIG_RGB_565: i32 = 1;
pub const RASTER_CONFIG_ARGB_8888: i32 = 2;

/// Stable C ABI `ClipRect` matching `SkCanvasStateUtils.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StateClipRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Matches `SkCanvasStack::pushCanvas` local bounds offset calculation:
/// `localBounds = canvasBounds.makeOffset(origin - existingOrigin)`.
#[inline]
pub fn canvas_stack_local_subtract_bounds(
    canvas_bounds: IRect,
    new_origin: IPoint,
    existing_origin: IPoint,
) -> IRect {
    let dx = new_origin.x.wrapping_sub(existing_origin.x);
    let dy = new_origin.y.wrapping_sub(existing_origin.y);
    IRect::from_ltrb(
        canvas_bounds.left.wrapping_add(dx),
        canvas_bounds.top.wrapping_add(dy),
        canvas_bounds.right.wrapping_add(dx),
        canvas_bounds.bottom.wrapping_add(dy),
    )
}

/// Matches `SkCanvasStack::didSetM44` per-layer matrix calculation:
/// `SkM44::Translate(-origin.x, -origin.y) * mx`.
#[inline]
pub fn canvas_stack_layer_matrix(origin: IPoint, mx: &M44) -> M44 {
    let mut trans = M44::identity();
    trans.set_rc(0, 3, -(origin.x as f32));
    trans.set_rc(1, 3, -(origin.y as f32));
    let mut out = M44::identity();
    out.set_concat(&trans, mx);
    out
}

/// Matches `SkCanvasStateUtils::CaptureCanvasState` colorType -> RasterConfig mapping.
#[inline]
pub fn canvas_state_color_type_to_raster_config(color_type: i32, n32_color_type: i32) -> i32 {
    if color_type == n32_color_type {
        RASTER_CONFIG_ARGB_8888
    } else if color_type == COLOR_TYPE_RGB_565 {
        RASTER_CONFIG_RGB_565
    } else {
        RASTER_CONFIG_UNKNOWN
    }
}

/// Matches `make_canvas_from_canvas_layer` RasterConfig -> colorType mapping.
#[inline]
pub fn canvas_state_raster_config_to_color_type(raster_config: i32, n32_color_type: i32) -> i32 {
    match raster_config {
        RASTER_CONFIG_ARGB_8888 => n32_color_type,
        RASTER_CONFIG_RGB_565 => COLOR_TYPE_RGB_565,
        _ => COLOR_TYPE_UNKNOWN,
    }
}

/// Matches `setup_canvas_from_MC_state` clip-rect union loop in `SkCanvasStateUtils.cpp`.
pub fn canvas_state_union_clip_rects(rects: &[StateClipRect]) -> IRect {
    if rects.is_empty() {
        return IRect::default();
    }
    let mut bounds = IRect::from_ltrb(rects[0].left, rects[0].top, rects[0].right, rects[0].bottom);
    for r in &rects[1..] {
        bounds.join(IRect::from_ltrb(r.left, r.top, r.right, r.bottom));
    }
    bounds
}

/// Matches `SkPaintFilterCanvas::onDrawPicture` null-paint restoration check.
#[inline]
pub fn paint_filter_can_restore_null_paint(
    alphaf: f32,
    has_color_filter: bool,
    has_image_filter: bool,
    has_blend_mode: bool,
    blend_mode: i32,
) -> bool {
    alphaf == 1.0
        && !has_color_filter
        && !has_image_filter
        && has_blend_mode
        && blend_mode == BLEND_MODE_SRC_OVER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_stack_helpers() {
        let b = canvas_stack_local_subtract_bounds(
            IRect::from_ltrb(0, 0, 50, 40),
            IPoint { x: 100, y: 200 },
            IPoint { x: 20, y: 50 },
        );
        assert_eq!(b, IRect::from_ltrb(80, 150, 130, 190));

        let m = canvas_stack_layer_matrix(IPoint { x: 10, y: 25 }, &M44::identity());
        assert_eq!(m.rc(0, 3), -10.0);
        assert_eq!(m.rc(1, 3), -25.0);
    }

    #[test]
    fn test_canvas_state_utils_helpers() {
        assert_eq!(canvas_state_color_type_to_raster_config(6, 6), RASTER_CONFIG_ARGB_8888);
        assert_eq!(canvas_state_color_type_to_raster_config(2, 6), RASTER_CONFIG_RGB_565);
        assert_eq!(canvas_state_color_type_to_raster_config(1, 6), RASTER_CONFIG_UNKNOWN);

        assert_eq!(canvas_state_raster_config_to_color_type(RASTER_CONFIG_ARGB_8888, 6), 6);
        assert_eq!(canvas_state_raster_config_to_color_type(RASTER_CONFIG_RGB_565, 6), 2);
        assert_eq!(canvas_state_raster_config_to_color_type(99, 6), COLOR_TYPE_UNKNOWN);

        let u = canvas_state_union_clip_rects(&[
            StateClipRect { left: 10, top: 20, right: 30, bottom: 40 },
            StateClipRect { left: 5, top: 25, right: 50, bottom: 35 },
        ]);
        assert_eq!(u, IRect::from_ltrb(5, 20, 50, 40));
        assert!(canvas_state_union_clip_rects(&[]).is_empty());
    }

    #[test]
    fn test_paint_filter_null_restore() {
        assert!(paint_filter_can_restore_null_paint(1.0, false, false, true, BLEND_MODE_SRC_OVER));
        assert!(!paint_filter_can_restore_null_paint(0.5, false, false, true, BLEND_MODE_SRC_OVER));
        assert!(!paint_filter_can_restore_null_paint(1.0, true, false, true, BLEND_MODE_SRC_OVER));
        assert!(!paint_filter_can_restore_null_paint(1.0, false, false, false, BLEND_MODE_SRC_OVER));
    }
}
