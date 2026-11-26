//! Safe Rust algorithms for `SkMask`, `SkMaskBuilder`, and `SkMaskCache` key computation.

use crate::align4;
use skia_rs_geometry::{IRect, Rect, Size};

pub const MASK_FORMAT_BW: u8 = 0;
pub const MASK_FORMAT_A8: u8 = 1;
pub const MASK_FORMAT_3D: u8 = 2;
pub const MASK_FORMAT_ARGB32: u8 = 3;
pub const MASK_FORMAT_LCD16: u8 = 4;
pub const MASK_FORMAT_SDF: u8 = 5;
pub const MASK_FORMAT_COUNT: u8 = 6;

/// Returns the product if it is positive and fits in 31 bits (`1..=i32::MAX`), otherwise 0.
/// Matches `safeMul32` in `SkMask.cpp`.
#[inline]
pub const fn safe_mul32(a: i32, b: i32) -> i32 {
    let size = (a as i64) * (b as i64);
    if size > 0 && size <= (i32::MAX as i64) {
        size as i32
    } else {
        0
    }
}

/// Matches `SkMask::computeImageSize()`.
#[inline]
pub const fn compute_image_size(bounds: IRect, row_bytes: u32) -> usize {
    let h = bounds.bottom.wrapping_sub(bounds.top);
    safe_mul32(h, row_bytes as i32) as usize
}

/// Matches `SkMask::computeTotalImageSize()`.
#[inline]
pub const fn compute_total_image_size(bounds: IRect, row_bytes: u32, format: u8) -> usize {
    let size = compute_image_size(bounds, row_bytes);
    if format == MASK_FORMAT_3D {
        safe_mul32(size as i32, 3) as usize
    } else {
        size
    }
}

/// Matches `SkSafeMath::Align4(size)` used by `SkMaskBuilder::AllocImage`.
/// Returns `align4(size)` or `usize::MAX` on overflow.
#[inline]
pub const fn alloc_image_aligned_size(size: usize) -> usize {
    if size > usize::MAX - 3 {
        usize::MAX
    } else {
        align4(size)
    }
}

/// Matches `gMaskFormatToShift` / `maskFormatToShift` in `SkMask.cpp`.
/// Returns `None` for `kBW_Format` or invalid formats.
#[inline]
pub const fn mask_format_to_shift(format: u8) -> Option<u32> {
    match format {
        MASK_FORMAT_A8 => Some(0),
        MASK_FORMAT_3D => Some(0),
        MASK_FORMAT_ARGB32 => Some(2),
        MASK_FORMAT_LCD16 => Some(1),
        MASK_FORMAT_SDF => Some(0),
        _ => None,
    }
}

/// Computes the byte offset from `fImage` for `SkMask::getAddr(int x, int y)`.
#[inline]
pub const fn get_addr_offset(
    bounds: IRect,
    row_bytes: u32,
    format: u8,
    x: i32,
    y: i32,
) -> Option<isize> {
    let Some(shift) = mask_format_to_shift(format) else {
        return None;
    };
    let dy = y.wrapping_sub(bounds.top) as isize;
    let dx = x.wrapping_sub(bounds.left) as isize;
    Some(dy.wrapping_mul(row_bytes as isize).wrapping_add(dx.wrapping_shl(shift)))
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedMaskDest {
    pub bounds: IRect,
    pub row_bytes: u32,
    pub to_alloc: usize,
    pub valid: bool,
}

/// Matches `SkMaskBuilder::PrepareDestination(int radiusX, int radiusY, const SkMask& src)`.
pub fn prepare_destination(radius_x: i32, radius_y: i32, src_bounds: IRect) -> PreparedMaskDest {
    let invalid = PreparedMaskDest {
        bounds: IRect::make_empty(),
        row_bytes: 0,
        to_alloc: 0,
        valid: false,
    };

    // Match C++ SkSafeMath implicit conversion of int -> size_t: negative values fail overflow check.
    let src_w = src_bounds.bottom.wrapping_sub(src_bounds.top);
    let _ = src_w;
    let w_i32 = src_bounds.right.wrapping_sub(src_bounds.left);
    let h_i32 = src_bounds.bottom.wrapping_sub(src_bounds.top);
    if w_i32 < 0 || h_i32 < 0 || radius_x < 0 || radius_y < 0 {
        return invalid;
    }

    let rx = radius_x as usize;
    let ry = radius_y as usize;
    let Some(two_rx) = rx.checked_add(rx) else {
        return invalid;
    };
    let Some(two_ry) = ry.checked_add(ry) else {
        return invalid;
    };
    let Some(dst_w) = (w_i32 as usize).checked_add(two_rx) else {
        return invalid;
    };
    let Some(dst_h) = (h_i32 as usize).checked_add(two_ry) else {
        return invalid;
    };
    let Some(to_alloc) = dst_w.checked_mul(dst_h) else {
        return invalid;
    };

    if dst_w > (i32::MAX as usize)
        || dst_h > (i32::MAX as usize)
        || to_alloc > (i32::MAX as usize)
    {
        return invalid;
    }

    let mut bounds = IRect::from_wh(dst_w as i32, dst_h as i32);
    bounds = bounds.make_offset(src_bounds.left, src_bounds.top);
    bounds = bounds.make_offset(-radius_x, -radius_y);

    PreparedMaskDest {
        bounds,
        row_bytes: dst_w as u32,
        to_alloc,
        valid: true,
    }
}

/// Computes the 4 `SkSize` fields for `SkMaskCache`'s `RectsBlurKey`.
/// `rects` must have length 1 or 2.
pub fn rects_blur_key_sizes(rects: &[Rect]) -> [Size; 4] {
    debug_assert!(rects.len() == 1 || rects.len() == 2);
    let r0 = rects[0];
    let ir = r0.round_out();
    let s0 = Size {
        width: r0.width(),
        height: r0.height(),
    };
    let (s1, s2) = if rects.len() == 2 {
        let r1 = rects[1];
        (
            Size {
                width: r1.width(),
                height: r1.height(),
            },
            Size {
                width: r0.left - r1.left,
                height: r0.top - r1.top,
            },
        )
    } else {
        (
            Size {
                width: 0.0,
                height: 0.0,
            },
            Size {
                width: 0.0,
                height: 0.0,
            },
        )
    };
    let s3 = Size {
        width: r0.left - ir.left as f32,
        height: r0.top - ir.top as f32,
    };
    [s0, s1, s2, s3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_image_sizes() {
        let b = IRect::from_ltrb(10, 20, 30, 50); // 20x30
        assert_eq!(compute_image_size(b, 20), 600);
        assert_eq!(compute_total_image_size(b, 20, MASK_FORMAT_A8), 600);
        assert_eq!(compute_total_image_size(b, 20, MASK_FORMAT_3D), 1800);

        // Overflow returns 0
        let huge = IRect::from_ltrb(0, 0, 100_000, 100_000);
        assert_eq!(compute_image_size(huge, 100_000), 0);
        assert_eq!(compute_total_image_size(IRect::from_ltrb(0, 0, 1, 1_000_000_000), 1, MASK_FORMAT_3D), 0);
    }

    #[test]
    fn test_prepare_destination() {
        let src = IRect::from_ltrb(5, 10, 15, 25); // 10x15
        let res = prepare_destination(3, 4, src);
        assert!(res.valid);
        assert_eq!(res.bounds, IRect::from_ltrb(2, 6, 18, 29));
        assert_eq!(res.row_bytes, 16);
        assert_eq!(res.to_alloc, 16 * 23);

        // Overflow -> invalid
        let bad = prepare_destination(i32::MAX, 1, src);
        assert!(!bad.valid);
        assert!(bad.bounds.is_empty());
        assert_eq!(bad.row_bytes, 0);
    }

    #[test]
    fn test_get_addr_offset() {
        let b = IRect::from_ltrb(10, 20, 30, 40);
        assert_eq!(get_addr_offset(b, 20, MASK_FORMAT_A8, 12, 23), Some(3 * 20 + 2));
        assert_eq!(get_addr_offset(b, 80, MASK_FORMAT_ARGB32, 12, 23), Some(3 * 80 + 8));
        assert_eq!(get_addr_offset(b, 40, MASK_FORMAT_LCD16, 12, 23), Some(3 * 40 + 4));
        assert_eq!(get_addr_offset(b, 4, MASK_FORMAT_BW, 12, 23), None);
    }

    #[test]
    fn test_rects_blur_key_sizes() {
        let r0 = Rect::from_ltrb(1.25, 2.5, 11.25, 22.5);
        let r1 = Rect::from_ltrb(0.5, 1.0, 5.5, 9.0);
        let sizes1 = rects_blur_key_sizes(&[r0]);
        assert_eq!(sizes1[0], Size { width: 10.0, height: 20.0 });
        assert_eq!(sizes1[1], Size { width: 0.0, height: 0.0 });
        assert_eq!(sizes1[2], Size { width: 0.0, height: 0.0 });
        assert_eq!(sizes1[3], Size { width: 0.25, height: 0.5 });

        let sizes2 = rects_blur_key_sizes(&[r0, r1]);
        assert_eq!(sizes2[1], Size { width: 5.0, height: 8.0 });
        assert_eq!(sizes2[2], Size { width: 0.75, height: 1.5 });
    }
}
