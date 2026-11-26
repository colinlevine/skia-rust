//! Safe Rust pixel record, storage, and context helpers matching:
//! - `src/core/SkReadPixelsRec.cpp`
//! - `src/core/SkWritePixelsRec.cpp`
//! - `src/core/SkAutoPixmapStorage.cpp`
//! - `src/core/SkPixelStorage.cpp`
//! - `src/core/SkContext.cpp`
//! - `src/core/SkRecorder.cpp`
//! - `src/core/RasterContext.cpp`
//! - `src/cpu/Recorder.cpp`

use core::sync::atomic::{AtomicU32, Ordering};
use skia_rs_geometry::IRect;

use crate::irect_from_xywh;

/// Result of `SkReadPixelsRec::trim` / `SkWritePixelsRec::trim`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PixelsRecTrimResult {
    pub valid: bool,
    pub byte_offset: usize,
    pub new_width: i32,
    pub new_height: i32,
    pub new_x: i32,
    pub new_y: i32,
}

/// Dispatch result for `SkContext::makeRecorder`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ContextRecorderPlan {
    Null = 0,
    Cpu = 1,
    Gpu = 2,
}

/// Exact pixel-record trimming matching `SkReadPixelsRec::trim` and `SkWritePixelsRec::trim`.
#[allow(clippy::too_many_arguments)]
pub fn trim_pixels_rec(
    has_pixels: bool,
    info_width: i32,
    info_height: i32,
    min_row_bytes: usize,
    bytes_per_pixel: usize,
    row_bytes: usize,
    x: i32,
    y: i32,
    bound_width: i32,
    bound_height: i32,
) -> PixelsRecTrimResult {
    let invalid = PixelsRecTrimResult {
        valid: false,
        byte_offset: 0,
        new_width: info_width,
        new_height: info_height,
        new_x: x,
        new_y: y,
    };

    if !has_pixels || row_bytes < min_row_bytes || min_row_bytes == 0 {
        return invalid;
    }
    if info_width <= 0 || info_height <= 0 {
        return invalid;
    }
    // Negating INT_MIN is UB in C++ and overflows i32.
    if x == i32::MIN || y == i32::MIN {
        return invalid;
    }

    let req = irect_from_xywh(x, y, info_width, info_height);
    let bounds = IRect::from_ltrb(0, 0, bound_width, bound_height);
    let mut clipped = IRect::default();
    if !clipped.intersect_pair(req, bounds) {
        return invalid;
    }

    let neg_x = if x < 0 { (-x) as usize } else { 0 };
    let neg_y = if y < 0 { (-y) as usize } else { 0 };

    let Some(y_offset) = neg_y.checked_mul(row_bytes) else {
        return invalid;
    };
    let Some(x_offset) = neg_x.checked_mul(bytes_per_pixel) else {
        return invalid;
    };
    let Some(total) = y_offset.checked_add(x_offset) else {
        return invalid;
    };

    PixelsRecTrimResult {
        valid: true,
        byte_offset: total,
        new_width: clipped.width(),
        new_height: clipped.height(),
        new_x: clipped.left,
        new_y: clipped.top,
    }
}

/// Global monotonic non-zero ID generator matching `SkPixelStorage::NextId()`.
static PIXEL_STORAGE_NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub fn next_pixel_storage_id() -> u32 {
    loop {
        let id = PIXEL_STORAGE_NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// Matches `SkContext::makeRecorder` backend selection (`0=Automatic, 1=CPU, 2=GPU`).
#[inline]
pub fn context_select_recorder_backend(backend: i32, has_gpu_context: bool) -> ContextRecorderPlan {
    match backend {
        0 => {
            if has_gpu_context {
                ContextRecorderPlan::Gpu
            } else {
                ContextRecorderPlan::Cpu
            }
        }
        1 => ContextRecorderPlan::Cpu,
        2 => {
            if has_gpu_context {
                ContextRecorderPlan::Gpu
            } else {
                ContextRecorderPlan::Null
            }
        }
        _ => ContextRecorderPlan::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_pixels_rec_negative_origin() {
        let r = trim_pixels_rec(true, 10, 10, 40, 4, 64, -2, -3, 20, 20);
        assert!(r.valid);
        assert_eq!(r.new_x, 0);
        assert_eq!(r.new_y, 0);
        assert_eq!(r.new_width, 8);
        assert_eq!(r.new_height, 7);
        assert_eq!(r.byte_offset, 3 * 64 + 2 * 4);
    }

    #[test]
    fn test_trim_pixels_rec_rejections() {
        // Null pixels
        assert!(!trim_pixels_rec(false, 10, 10, 40, 4, 40, 0, 0, 10, 10).valid);
        // Row bytes too small
        assert!(!trim_pixels_rec(true, 10, 10, 40, 4, 36, 0, 0, 10, 10).valid);
        // INT_MIN origin
        assert!(!trim_pixels_rec(true, 10, 10, 40, 4, 40, i32::MIN, 0, 10, 10).valid);
        // Disjoint
        assert!(!trim_pixels_rec(true, 10, 10, 40, 4, 40, 20, 0, 10, 10).valid);
        // Overflow in y_offset
        assert!(!trim_pixels_rec(true, 10, 10, 40, 4, usize::MAX, 0, -2, 10, 10).valid);
    }

    #[test]
    fn test_pixel_storage_id_monotonic() {
        let a = next_pixel_storage_id();
        let b = next_pixel_storage_id();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn test_context_backend_selection() {
        assert_eq!(context_select_recorder_backend(0, false), ContextRecorderPlan::Cpu);
        assert_eq!(context_select_recorder_backend(0, true), ContextRecorderPlan::Gpu);
        assert_eq!(context_select_recorder_backend(1, true), ContextRecorderPlan::Cpu);
        assert_eq!(context_select_recorder_backend(2, false), ContextRecorderPlan::Null);
        assert_eq!(context_select_recorder_backend(2, true), ContextRecorderPlan::Gpu);
    }
}
