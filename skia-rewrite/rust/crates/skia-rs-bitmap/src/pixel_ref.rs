//! Safe Rust port of `SkPixelRef`, `SkMallocPixelRef`, and `SkBitmapCache` helpers.

use std::sync::atomic::{AtomicU32, Ordering};

use skia_rs_color::image_info::{
    color_type_bytes_per_pixel, compute_byte_size, image_info_valid_row_bytes, AlphaType, ColorType,
};

/// Atomic generator for `SkNextID::ImageID()`.
/// Never returns 0 and never sets the low bit (reserved for `SkPixelRef::genIDIsUnique()`).
static NEXT_IMAGE_ID: AtomicU32 = AtomicU32::new(2);

/// Exact port of `SkNextID::ImageID()`.
pub fn next_image_id() -> u32 {
    loop {
        let id = NEXT_IMAGE_ID.fetch_add(2, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// Exact port of `SkMakeResourceCacheSharedIDForBitmap(uint32_t bitmapGenID)`.
#[inline]
pub const fn make_resource_cache_shared_id_for_bitmap(bitmap_gen_id: u32) -> u64 {
    let shared_id: u64 =
        ((b'b' as u64) << 24) | ((b'm' as u64) << 16) | ((b'a' as u64) << 8) | (b'p' as u64);
    (shared_id << 32) | (bitmap_gen_id as u64)
}

/// Validates basic `SkImageInfo` invariants used by `SkMallocPixelRef`.
#[inline]
pub const fn is_valid_image_info(width: i32, height: i32, ct: i32, at: i32) -> bool {
    if width < 0 || height < 0 {
        return false;
    }
    ColorType::from_i32(ct).is_some() && AlphaType::from_i32(at).is_some()
}

/// Computes `info.minRowBytes()` matching Skia's `SkImageInfo::minRowBytes()`.
#[inline]
pub fn min_row_bytes(width: i32, ct: ColorType) -> usize {
    if width < 0 {
        return 0;
    }
    let mrb64 = (width as u64).wrapping_mul(color_type_bytes_per_pixel(ct) as u64);
    if mrb64 <= usize::MAX as u64 {
        mrb64 as usize
    } else {
        0
    }
}

/// Result of validating a `SkMallocPixelRef::MakeAllocate` request.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MallocAllocPlan {
    pub row_bytes: usize,
    pub byte_size: usize,
}

/// Exact port of the validation and size calculation in `SkMallocPixelRef::MakeAllocate`.
pub fn validate_malloc_allocate(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    mut row_bytes: usize,
    fuzzer_limit: bool,
) -> Option<MallocAllocPlan> {
    let ct = ColorType::from_i32(ct_raw)?;
    let _at = AlphaType::from_i32(at_raw)?;
    if width < 0 || height < 0 {
        return None;
    }
    if row_bytes == 0 {
        row_bytes = min_row_bytes(width, ct);
    }
    if !image_info_valid_row_bytes(width, ct, row_bytes) {
        return None;
    }
    let size = compute_byte_size(width, height, ct, row_bytes);
    if size == usize::MAX {
        return None;
    }
    if fuzzer_limit && size > 10_000_000 {
        return None;
    }
    Some(MallocAllocPlan {
        row_bytes,
        byte_size: size,
    })
}

/// Exact port of the validation in `SkMallocPixelRef::MakeWithData`.
pub fn validate_make_with_data(
    width: i32,
    height: i32,
    ct_raw: i32,
    at_raw: i32,
    row_bytes: usize,
    data_size: usize,
) -> bool {
    let Some(ct) = ColorType::from_i32(ct_raw) else {
        return false;
    };
    if AlphaType::from_i32(at_raw).is_none() || width < 0 || height < 0 {
        return false;
    }
    let mrb = min_row_bytes(width, ct);
    if row_bytes < mrb {
        return false;
    }
    let byte_size = compute_byte_size(width, height, ct, row_bytes);
    data_size >= byte_size
}

/// Exact port of the `SkBitmapCache::Alloc` rowBytes & byteSize validation.
pub fn validate_bitmap_cache_alloc(width: i32, height: i32, ct: ColorType) -> Option<MallocAllocPlan> {
    if width < 0 || height < 0 {
        return None;
    }
    let rb = min_row_bytes(width, ct);
    let size = compute_byte_size(width, height, ct, rb);
    if size == usize::MAX {
        return None;
    }
    Some(MallocAllocPlan {
        row_bytes: rb,
        byte_size: size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_image_id_is_even_and_nonzero() {
        let a = next_image_id();
        let b = next_image_id();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_eq!(a & 1, 0);
        assert_eq!(b & 1, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn resource_cache_shared_id_tag() {
        let id = make_resource_cache_shared_id_for_bitmap(0x12345678);
        assert_eq!(id >> 32, 0x626D_6170); // 'bmap'
        assert_eq!(id & 0xFFFF_FFFF, 0x12345678);
    }

    #[test]
    fn malloc_allocate_validation() {
        let plan = validate_malloc_allocate(10, 20, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 0, false)
            .unwrap();
        assert_eq!(plan.row_bytes, 40);
        assert_eq!(plan.byte_size, 800);

        // Unaligned rowBytes for RGBA_8888 must fail
        assert!(validate_malloc_allocate(10, 20, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 41, false).is_none());
        // Negative dimension must fail
        assert!(validate_malloc_allocate(-1, 20, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 0, false).is_none());
        // Overflow must fail
        assert!(validate_malloc_allocate(100_000, 100_000, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 0, false).is_none());
        // Fuzzer limit
        assert!(validate_malloc_allocate(2000, 2000, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 0, true).is_none());
    }

    #[test]
    fn make_with_data_validation() {
        assert!(validate_make_with_data(10, 10, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 40, 400));
        assert!(!validate_make_with_data(10, 10, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 40, 399));
        assert!(!validate_make_with_data(10, 10, ColorType::Rgba8888 as i32, AlphaType::Premul as i32, 36, 400));
    }
}
