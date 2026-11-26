//! Safe Rust port of `src/image/SkSurface.cpp`, `src/image/SkSurface_Base.cpp`,
//! `src/image/SkSurface_Null.cpp`, and `src/image/SkSurface_Raster.cpp` logic.

use std::sync::atomic::{AtomicU32, Ordering};

use skia_rs_color::image_info::{image_info_valid_row_bytes, AlphaType, ColorType};
use skia_rs_geometry::IRect;

use crate::{
    image::{image_info_is_valid, irect_contains},
    raster::CopyPixelsMode,
};

/// Sentinel matching `kIgnoreRowBytesValue = static_cast<size_t>(~0)` in `SkSurfacePriv.h`.
pub const IGNORE_ROW_BYTES_VALUE: usize = usize::MAX;

/// Matches `SkSurface::ContentChangeMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ContentChangeMode {
    Discard = 0,
    Retain = 1,
}

impl ContentChangeMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Discard),
            1 => Some(Self::Retain),
            _ => None,
        }
    }
}

/// POD mirror of `SkSurfaceProps` (`uint32_t fFlags; SkPixelGeometry fPixelGeometry; float fTextContrast; float fTextGamma;`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SurfacePropsData {
    pub flags: u32,
    pub pixel_geometry: i32,
    pub text_contrast: f32,
    pub text_gamma: f32,
}

/// Initializes a `SurfacePropsData` with the given parameters.
#[inline]
pub const fn make_surface_props(
    flags: u32,
    pixel_geometry: i32,
    text_contrast: f32,
    text_gamma: f32,
) -> SurfacePropsData {
    SurfacePropsData {
        flags,
        pixel_geometry,
        text_contrast,
        text_gamma,
    }
}

/// Atomic counter for `SkSurface_Base::newGenerationID()`. Starts at 1, increments by 1.
static NEXT_SURFACE_GENERATION_ID: AtomicU32 = AtomicU32::new(1);

/// Exact port of `SkSurface_Base::newGenerationID()`.
#[inline]
pub fn next_surface_generation_id() -> u32 {
    NEXT_SURFACE_GENERATION_ID.fetch_add(1, Ordering::Relaxed)
}

/// Action returned by `plan_surface_snapshot_bounds`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SnapshotBoundsAction {
    Empty = 0,
    FullSurface = 1,
    Subset = 2,
}

/// Result of `SkSurface::makeImageSnapshot(const SkIRect& srcBounds)`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SnapshotBoundsPlan {
    pub action: SnapshotBoundsAction,
    pub clipped_bounds: IRect,
}

/// Exact port of `SkSurface::makeImageSnapshot(const SkIRect& srcBounds)`.
pub fn plan_surface_snapshot_bounds(
    surf_width: i32,
    surf_height: i32,
    src_bounds: IRect,
) -> SnapshotBoundsPlan {
    let surf_bounds = IRect::from_wh(surf_width, surf_height);
    let mut clipped = IRect::make_empty();
    if !clipped.intersect_pair(src_bounds, surf_bounds) {
        return SnapshotBoundsPlan {
            action: SnapshotBoundsAction::Empty,
            clipped_bounds: IRect::make_empty(),
        };
    }
    if clipped == surf_bounds {
        SnapshotBoundsPlan {
            action: SnapshotBoundsAction::FullSurface,
            clipped_bounds: clipped,
        }
    } else {
        SnapshotBoundsPlan {
            action: SnapshotBoundsAction::Subset,
            clipped_bounds: clipped,
        }
    }
}

/// Action returned by `plan_surface_write_pixels`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WritePixelsAction {
    Skip = 0,
    Retain = 1,
    Discard = 2,
}

/// Exact port of `SkSurface::writePixels(const SkPixmap&, int x, int y)` intersection and mode logic.
pub fn plan_surface_write_pixels(
    surf_width: i32,
    surf_height: i32,
    has_addr: bool,
    pm_width: i32,
    pm_height: i32,
    x: i32,
    y: i32,
) -> WritePixelsAction {
    if !has_addr || pm_width <= 0 || pm_height <= 0 {
        return WritePixelsAction::Skip;
    }
    let src_r = IRect::make_ltrb(
        x,
        y,
        x.saturating_add(pm_width),
        y.saturating_add(pm_height),
    );
    let dst_r = IRect::from_wh(surf_width, surf_height);
    if !IRect::intersects(src_r, dst_r) {
        return WritePixelsAction::Skip;
    }
    if irect_contains(src_r, dst_r) {
        WritePixelsAction::Discard
    } else {
        WritePixelsAction::Retain
    }
}

/// Action returned by `plan_surface_about_to_draw`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AboutToDrawAction {
    /// No cached image and `Retain` mode: nothing to do.
    NoAction = 0,
    /// No cached image and `Discard` mode: invoke `onDiscard()`.
    Discard = 1,
    /// Cached image exists and is unique: drop cached image, then invoke `onRestoreBackingMutability()`.
    DropAndRestoreMutability = 2,
    /// Cached image exists and is shared: invoke `onCopyOnWrite(mode)`; if true, drop cached image.
    CopyOnWriteAndDrop = 3,
}

/// Exact port of `SkSurface_Base::aboutToDraw(ContentChangeMode)` state transition.
#[inline]
pub const fn plan_surface_about_to_draw(
    has_cached_image: bool,
    cached_image_unique: bool,
    mode: ContentChangeMode,
) -> AboutToDrawAction {
    if has_cached_image {
        if cached_image_unique {
            AboutToDrawAction::DropAndRestoreMutability
        } else {
            AboutToDrawAction::CopyOnWriteAndDrop
        }
    } else if matches!(mode, ContentChangeMode::Discard) {
        AboutToDrawAction::Discard
    } else {
        AboutToDrawAction::NoAction
    }
}

/// Exact port of `SkSurface_Base::outstandingImageSnapshot()`.
#[inline]
pub const fn outstanding_image_snapshot(
    has_cached_image: bool,
    cached_image_unique: bool,
) -> bool {
    has_cached_image && !cached_image_unique
}

/// Exact port of `SkSurfaces::Null(int width, int height)` dimension validation.
#[inline]
pub const fn validate_null_surface_dims(width: i32, height: i32) -> bool {
    width >= 1 && height >= 1
}

/// Exact port of `SkSurfaceValidateRasterInfo(const SkImageInfo&, size_t rowBytes)`.
pub fn validate_raster_surface_info(
    width: i32,
    height: i32,
    ct: ColorType,
    at: AlphaType,
    row_bytes: usize,
) -> bool {
    if !image_info_is_valid(width, height, ct, at) {
        return false;
    }
    if row_bytes == IGNORE_ROW_BYTES_VALUE {
        return true;
    }
    if !image_info_valid_row_bytes(width, ct, row_bytes) {
        return false;
    }
    let Some(size) = (height as u64).checked_mul(row_bytes as u64) else {
        return false;
    };
    size <= i32::MAX as u64
}

/// Plan for `SkSurface_Raster::onNewImageSnapshot(nullptr)`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RasterSnapshotPlan {
    pub copy_mode: CopyPixelsMode,
    pub set_temporarily_immutable: bool,
}

/// Exact port of `SkSurface_Raster::onNewImageSnapshot(nullptr)` copy-on-write decision.
#[inline]
pub const fn plan_raster_surface_snapshot(we_own_pixels: bool) -> RasterSnapshotPlan {
    if we_own_pixels {
        RasterSnapshotPlan {
            copy_mode: CopyPixelsMode::IfMutable,
            set_temporarily_immutable: true,
        }
    } else {
        RasterSnapshotPlan {
            copy_mode: CopyPixelsMode::Always,
            set_temporarily_immutable: false,
        }
    }
}

/// Safe Rust byte-buffer copy for `SkSurface_Raster::onCopyOnWrite` (`kRetain_ContentChangeMode`).
#[inline]
pub fn copy_surface_backing_pixels(dst: &mut [u8], src: &[u8]) {
    let len = dst.len().min(src.len());
    dst[..len].copy_from_slice(&src[..len]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_generation_id_is_nonzero_and_monotonic() {
        let a = next_surface_generation_id();
        let b = next_surface_generation_id();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn snapshot_bounds_cases() {
        // Non-intersecting -> Empty
        let p = plan_surface_snapshot_bounds(100, 100, IRect::from_ltrb(100, 0, 120, 50));
        assert_eq!(p.action, SnapshotBoundsAction::Empty);

        // Superset -> FullSurface
        let p = plan_surface_snapshot_bounds(100, 100, IRect::from_ltrb(-10, -10, 200, 200));
        assert_eq!(p.action, SnapshotBoundsAction::FullSurface);
        assert_eq!(p.clipped_bounds, IRect::from_wh(100, 100));

        // Partial overlap -> Subset
        let p = plan_surface_snapshot_bounds(100, 100, IRect::from_ltrb(50, 60, 150, 160));
        assert_eq!(p.action, SnapshotBoundsAction::Subset);
        assert_eq!(p.clipped_bounds, IRect::from_ltrb(50, 60, 100, 100));
    }

    #[test]
    fn write_pixels_mode_selection() {
        // Null addr or empty pm -> Skip
        assert_eq!(
            plan_surface_write_pixels(100, 100, false, 10, 10, 0, 0),
            WritePixelsAction::Skip
        );
        assert_eq!(
            plan_surface_write_pixels(100, 100, true, 0, 10, 0, 0),
            WritePixelsAction::Skip
        );
        // Outside surface -> Skip
        assert_eq!(
            plan_surface_write_pixels(100, 100, true, 10, 10, 100, 0),
            WritePixelsAction::Skip
        );
        // Partial overlap -> Retain
        assert_eq!(
            plan_surface_write_pixels(100, 100, true, 50, 50, 0, 0),
            WritePixelsAction::Retain
        );
        // Full cover -> Discard
        assert_eq!(
            plan_surface_write_pixels(100, 100, true, 100, 100, 0, 0),
            WritePixelsAction::Discard
        );
        assert_eq!(
            plan_surface_write_pixels(100, 100, true, 120, 120, -10, -10),
            WritePixelsAction::Discard
        );
    }

    #[test]
    fn about_to_draw_state_machine() {
        assert_eq!(
            plan_surface_about_to_draw(false, false, ContentChangeMode::Retain),
            AboutToDrawAction::NoAction
        );
        assert_eq!(
            plan_surface_about_to_draw(false, false, ContentChangeMode::Discard),
            AboutToDrawAction::Discard
        );
        assert_eq!(
            plan_surface_about_to_draw(true, true, ContentChangeMode::Retain),
            AboutToDrawAction::DropAndRestoreMutability
        );
        assert_eq!(
            plan_surface_about_to_draw(true, false, ContentChangeMode::Retain),
            AboutToDrawAction::CopyOnWriteAndDrop
        );
        assert!(outstanding_image_snapshot(true, false));
        assert!(!outstanding_image_snapshot(true, true));
        assert!(!outstanding_image_snapshot(false, false));
    }

    #[test]
    fn validate_raster_info_checks() {
        assert!(validate_raster_surface_info(
            100,
            100,
            ColorType::Rgba8888,
            AlphaType::Premul,
            IGNORE_ROW_BYTES_VALUE
        ));
        assert!(validate_raster_surface_info(
            100,
            100,
            ColorType::Rgba8888,
            AlphaType::Premul,
            400
        ));
        // Unaligned rowBytes fails
        assert!(!validate_raster_surface_info(
            100,
            100,
            ColorType::Rgba8888,
            AlphaType::Premul,
            401
        ));
        // Total size > SK_MaxS32 fails
        assert!(!validate_raster_surface_info(
            100_000,
            100_000,
            ColorType::Rgba8888,
            AlphaType::Premul,
            400_000
        ));
        // Invalid alpha type fails
        assert!(!validate_raster_surface_info(
            10,
            10,
            ColorType::Rgba8888,
            AlphaType::Unknown,
            IGNORE_ROW_BYTES_VALUE
        ));
        // Null surface dims
        assert!(validate_null_surface_dims(1, 1));
        assert!(!validate_null_surface_dims(0, 1));
        assert!(!validate_null_surface_dims(1, -5));
    }
}
