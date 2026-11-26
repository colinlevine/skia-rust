//! Safe Rust port of `SkStrikeSpec.cpp`, `SkStrikeCache.cpp`, `SkStrike.cpp`,
//! `SkStrikeRef.cpp`, and `src/text/StrikeForGPU.cpp` algorithms.

use crate::font::make_text_matrix;
use skia_rs_geometry::matrix::{MSCALE_X, MSCALE_Y, MSKEW_X, MSKEW_Y};
use skia_rs_geometry::Matrix;

/// Mirrors `SkStrikeSpec::ShouldDrawAsPath`.
pub fn should_draw_as_path(
    paint_style: u8,
    stroke_width: f32,
    font_size: f32,
    font_scale_x: f32,
    font_skew_x: f32,
    view_matrix: &Matrix,
) -> bool {
    // SkPaint::kStroke_Style == 1
    if paint_style == 1 && stroke_width == 0.0 {
        return true;
    }

    if view_matrix.has_perspective() {
        return true;
    }

    let mut text_matrix = make_text_matrix(font_size, font_scale_x, font_skew_x);
    text_matrix.post_concat(view_matrix);

    const MEMORY_LIMIT: f32 = 256.0;
    const MAX_SIZE_SQUARED: f32 = MEMORY_LIMIT * MEMORY_LIMIT;

    let dist_x = text_matrix.mat[MSCALE_X] * text_matrix.mat[MSCALE_X]
        + text_matrix.mat[MSKEW_Y] * text_matrix.mat[MSKEW_Y];
    let dist_y = text_matrix.mat[MSKEW_X] * text_matrix.mat[MSKEW_X]
        + text_matrix.mat[MSCALE_Y] * text_matrix.mat[MSCALE_Y];

    dist_x > MAX_SIZE_SQUARED || dist_y > MAX_SIZE_SQUARED
}

/// Purge targets computed for `SkStrikeCache::internalPurge`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct StrikePurgeTargets {
    pub bytes_needed: usize,
    pub count_needed: i32,
}

/// Computes `(bytes_needed, count_needed)` for `SkStrikeCache::internalPurge`.
pub fn compute_strike_purge_targets(
    total_memory_used: usize,
    cache_size_limit: usize,
    min_bytes_needed: usize,
    cache_count: i32,
    cache_count_limit: i32,
    pinner_count: i32,
    check_pinners: bool,
) -> StrikePurgeTargets {
    if pinner_count == cache_count && !check_pinners {
        return StrikePurgeTargets::default();
    }

    let mut bytes_needed = if total_memory_used > cache_size_limit {
        total_memory_used - cache_size_limit
    } else {
        0
    };
    bytes_needed = bytes_needed.max(min_bytes_needed);
    if bytes_needed > 0 {
        bytes_needed = bytes_needed.max(total_memory_used >> 2);
    }

    let mut count_needed = 0i32;
    if cache_count > cache_count_limit {
        count_needed = cache_count - cache_count_limit;
        count_needed = count_needed.max(cache_count >> 2);
    }

    StrikePurgeTargets {
        bytes_needed,
        count_needed,
    }
}

/// Mirrors `SkStrike::dumpMemoryStatistics` font name sanitization:
/// replaces all non-ASCII-alphanumeric bytes with `'_'`.
pub fn sanitize_font_name_for_dump(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        if !b.is_ascii_alphanumeric() {
            *b = b'_';
        }
    }
}

/// Mirrors `SkStrikeRef::getWidthsStrided` raw-pointer overload span-length calculation.
#[inline]
pub fn strided_span_len(count: u32, stride_32: u32) -> usize {
    1usize.max((count as usize).saturating_mul(stride_32 as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_draw_as_path() {
        let id = Matrix::identity();
        // Hairline stroke -> true
        assert!(should_draw_as_path(1, 0.0, 12.0, 1.0, 0.0, &id));
        // Normal fill 12pt -> false
        assert!(!should_draw_as_path(0, 0.0, 12.0, 1.0, 0.0, &id));
        // Huge font 300pt -> true
        assert!(should_draw_as_path(0, 0.0, 300.0, 1.0, 0.0, &id));
        // Perspective matrix -> true
        let mut persp = Matrix::identity();
        persp.set9(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.001, 0.0, 1.0]);
        assert!(should_draw_as_path(0, 0.0, 12.0, 1.0, 0.0, &persp));
    }

    #[test]
    fn test_compute_purge_targets() {
        // Under budget -> 0
        let t0 = compute_strike_purge_targets(100, 1000, 0, 5, 100, 0, true);
        assert_eq!(t0, StrikePurgeTargets { bytes_needed: 0, count_needed: 0 });

        // Slightly over byte budget -> at least 25% of total
        let t1 = compute_strike_purge_targets(1024, 1000, 0, 5, 100, 0, true);
        assert_eq!(t1.bytes_needed, 256);
        assert_eq!(t1.count_needed, 0);

        // Slightly over count budget -> at least 25% of count
        let t2 = compute_strike_purge_targets(100, 1000, 0, 104, 100, 0, true);
        assert_eq!(t2.bytes_needed, 0);
        assert_eq!(t2.count_needed, 26);
    }

    #[test]
    fn test_sanitize_font_name() {
        let mut s = *b"DejaVu Sans-Bold!";
        sanitize_font_name_for_dump(&mut s);
        assert_eq!(&s, b"DejaVu_Sans_Bold_");
    }
}
