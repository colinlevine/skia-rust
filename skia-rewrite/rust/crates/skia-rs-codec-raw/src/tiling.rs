//! Safe Rust port of `SkDngHost` tile/area task partitioning (`compute_task_areas`,
//! `num_tiles_in_area`, `num_tasks_required`, `num_tiles_per_task`, `PerformAreaTaskThreads`).

use crate::types::{DngPoint, DngRect};

/// Exact port of `SkDngHost::PerformAreaTaskThreads`.
/// On Android, limits to 1 thread due to DNG warp memory scaling; elsewhere uses `max_mp_threads`.
#[inline]
pub const fn perform_area_task_threads(is_android: bool, max_mp_threads: u32) -> u32 {
    if is_android {
        1
    } else {
        max_mp_threads
    }
}

/// Calculates the number of tiles of `tile_size` that fit into `area_size` in vertical and
/// horizontal directions. Exact port of `num_tiles_in_area` in `SkRawCodec.cpp`.
#[inline]
pub fn num_tiles_in_area(area_size: DngPoint, tile_size_v: f64, tile_size_h: f64) -> DngPoint {
    DngPoint {
        v: ((area_size.v as f64 + tile_size_v - 1.0) / tile_size_v) as i32,
        h: ((area_size.h as f64 + tile_size_h - 1.0) / tile_size_h) as i32,
    }
}

/// Exact port of `num_tasks_required` in `SkRawCodec.cpp`.
#[inline]
pub const fn num_tasks_required(tiles_in_task: DngPoint, tiles_in_area: DngPoint) -> i32 {
    ((tiles_in_area.v + tiles_in_task.v - 1) / tiles_in_task.v)
        * ((tiles_in_area.h + tiles_in_task.h - 1) / tiles_in_task.h)
}

/// Calculates the number of tiles to process per task, taking into account `max_tasks`.
/// Prefers increasing horizontally for better locality of reference.
/// Returns `None` on invalid configuration (corresponding to `ThrowProgramError`).
pub fn num_tiles_per_task(max_tasks: i32, tiles_in_area: DngPoint) -> Option<DngPoint> {
    let mut tiles_in_task = DngPoint::new(1, 1);
    while num_tasks_required(tiles_in_task, tiles_in_area) > max_tasks {
        if tiles_in_task.h < tiles_in_area.h {
            tiles_in_task.h += 1;
        } else if tiles_in_task.v < tiles_in_area.v {
            tiles_in_task.v += 1;
        } else {
            return None;
        }
    }
    Some(tiles_in_task)
}

/// Partitions `area` into up to `max_tasks` non-overlapping tile-aligned [`DngRect`] task areas.
/// Exact port of `compute_task_areas` in `SkRawCodec.cpp`.
/// Returns `None` if `tile_size` is non-positive or `num_tiles_per_task` fails.
pub fn compute_task_areas(
    max_tasks: i32,
    area: DngRect,
    tile_size: DngPoint,
) -> Option<Vec<DngRect>> {
    if tile_size.v <= 0 || tile_size.h <= 0 {
        return None;
    }
    let area_size = area.size();
    let tiles_in_area = num_tiles_in_area(area_size, tile_size.v as f64, tile_size.h as f64);
    let tiles_per_task = num_tiles_per_task(max_tasks, tiles_in_area)?;
    let task_area_size = DngPoint {
        v: tiles_per_task.v * tile_size.v,
        h: tiles_per_task.h * tile_size.h,
    };

    let mut task_areas = Vec::new();
    let mut v = 0;
    while v < tiles_in_area.v {
        let mut h = 0;
        while h < tiles_in_area.h {
            let t = area.t + v * tile_size.v;
            let l = area.l + h * tile_size.h;
            let b = (t + task_area_size.v).min(area.b);
            let r = (l + task_area_size.h).min(area.r);
            task_areas.push(DngRect::new(t, l, b, r));
            h += tiles_per_task.h;
        }
        v += tiles_per_task.v;
    }
    Some(task_areas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_count_policy() {
        assert_eq!(perform_area_task_threads(true, 8), 1);
        assert_eq!(perform_area_task_threads(false, 8), 8);
        assert_eq!(perform_area_task_threads(false, 32), 32);
    }

    #[test]
    fn tiling_exact_and_clamped_areas() {
        let area = DngRect::new(0, 0, 1000, 1500);
        let tile = DngPoint::new(256, 256);

        let tiles = num_tiles_in_area(area.size(), 256.0, 256.0);
        assert_eq!(tiles, DngPoint::new(4, 6));
        assert_eq!(num_tasks_required(DngPoint::new(1, 1), tiles), 24);

        // With max_tasks = 8: grows horizontally first (h=3 -> 4*2 = 8 tasks).
        let per_task = num_tiles_per_task(8, tiles).unwrap();
        assert_eq!(per_task, DngPoint::new(1, 3));

        let rects = compute_task_areas(8, area, tile).unwrap();
        assert_eq!(rects.len(), 8);
        // Verify complete, non-overlapping coverage of [0,0,1000,1500].
        let total_pixels: i64 = rects
            .iter()
            .map(|r| {
                assert!(r.t >= area.t && r.b <= area.b && r.t < r.b);
                assert!(r.l >= area.l && r.r <= area.r && r.l < r.r);
                (r.b - r.t) as i64 * (r.r - r.l) as i64
            })
            .sum();
        assert_eq!(total_pixels, 1000 * 1500);
        assert_eq!(rects[0], DngRect::new(0, 0, 256, 768));
        assert_eq!(rects[1], DngRect::new(0, 768, 256, 1500));
        assert_eq!(rects[7], DngRect::new(768, 768, 1000, 1500));
    }

    #[test]
    fn tiling_single_task_and_error_cases() {
        let area = DngRect::new(10, 20, 522, 1044);
        let tile = DngPoint::new(256, 256);

        let rects = compute_task_areas(1, area, tile).unwrap();
        assert_eq!(rects, vec![area]);

        // max_tasks == 0 for non-empty area fails (ThrowProgramError).
        assert!(compute_task_areas(0, area, tile).is_none());

        // Non-positive tile size fails safely.
        assert!(compute_task_areas(4, area, DngPoint::new(0, 256)).is_none());
        assert!(compute_task_areas(4, area, DngPoint::new(256, -1)).is_none());

        // Empty area yields 0 tasks without error.
        let empty = DngRect::new(10, 10, 10, 100);
        assert_eq!(compute_task_areas(4, empty, tile).unwrap(), Vec::<DngRect>::new());
    }

    #[test]
    fn tiling_vertical_growth_after_horizontal_saturation() {
        // 8 rows x 2 cols of 256x256 tiles = 16 tiles.
        let area = DngRect::new(0, 0, 2048, 512);
        let tile = DngPoint::new(256, 256);

        // With max_tasks = 4: horizontal saturates at h = 2 (8 tasks > 4),
        // then vertical grows to v = 2 (4 * 1 = 4 tasks <= 4).
        let tiles_in_area = num_tiles_in_area(area.size(), 256.0, 256.0);
        assert_eq!(tiles_in_area, DngPoint::new(8, 2));
        let per_task = num_tiles_per_task(4, tiles_in_area).unwrap();
        assert_eq!(per_task, DngPoint::new(2, 2));

        let rects = compute_task_areas(4, area, tile).unwrap();
        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0], DngRect::new(0, 0, 512, 512));
        assert_eq!(rects[1], DngRect::new(512, 0, 1024, 512));
        assert_eq!(rects[2], DngRect::new(1024, 0, 1536, 512));
        assert_eq!(rects[3], DngRect::new(1536, 0, 2048, 512));
    }
}
