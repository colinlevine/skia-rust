#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkRegion_path.cpp`.
//!
//! Includes:
//! - `RegionBuilder` (exact safe Rust equivalent of `SkRgnBuilder`, building RLE
//!   runs from Y->X scanline `blit_h` calls with row collapsing and empty-span
//!   insertion).
//! - Boundary path extraction (`extract_boundary_path`, converting a region's
//!   rectangles into sorted vertical edges, linking them into closed contours,
//!   and emitting `MoveTo` / `LineTo` / `Close` path commands skipping collinear
//!   segments).
//! - Overflow-safe capacity and tile grid calculations for `SkRegion::setPath`.

use crate::region::{Region, RECT_REGION_RUNS, RUN_TYPE_SENTINEL};
use skia_rs_geometry::IRect;

/// Maximum conic-to-quad subdivision power matching `SkConic::kMaxConicToQuadPOW2`.
pub const MAX_CONIC_TO_QUAD_POW2: u32 = 5;

/// Matches `verb_to_initial_last_index` in `SkRegion_path.cpp`.
/// `verb`: 0=Move, 1=Line, 2=Quad, 3=Conic, 4=Cubic, 5=Close.
#[inline]
pub const fn verb_to_initial_last_index(verb: u8) -> u8 {
    const TABLE: [u8; 6] = [0, 1, 2, 2, 3, 0];
    if (verb as usize) < TABLE.len() {
        TABLE[verb as usize]
    } else {
        0
    }
}

/// Matches `verb_to_max_edges` in `SkRegion_path.cpp`.
#[inline]
pub const fn verb_to_max_edges(verb: u8) -> u32 {
    const TABLE: [u32; 6] = [0, 1, 2, 2 << MAX_CONIC_TO_QUAD_POW2, 3, 0];
    if (verb as usize) < TABLE.len() {
        TABLE[verb as usize]
    } else {
        0
    }
}

/// Computes worst-case `RunType` storage count matching `SkRgnBuilder::init`.
/// Returns `None` on negative inputs or overflow.
pub fn compute_rgn_builder_capacity(
    max_height: i32,
    max_transitions: i32,
    path_is_inverse: bool,
) -> Option<usize> {
    if (max_height | max_transitions) < 0 {
        return None;
    }
    let mt = if path_is_inverse {
        max_transitions.checked_add(2)?
    } else {
        max_transitions
    };
    let h_plus_1 = max_height.checked_add(1)? as usize;
    let w_plus_3 = (3_i32).checked_add(mt)? as usize;
    let mut count = h_plus_1.checked_mul(w_plus_3)?;
    if path_is_inverse {
        count = count.checked_add(10)?;
    }
    if i32::try_from(count).is_err() {
        return None;
    }
    Some(count)
}

/// Tiling parameters computed for oversized clip bounds in `SkRegion::setPath`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PathTileGrid {
    pub intersection: IRect,
    pub tile_size: i32,
    pub tiles_x: i32,
    pub tiles_y: i32,
}

pub const PATH_TILE_SIZE: i32 = 32767 >> 1;
pub const PATH_TILE_LIMIT: i32 = 1000;

/// Computes tile grid parameters matching `SkRegion::setPath` tiling logic.
/// Returns `None` if `path_bounds` and `clip_bounds` don't intersect, overflow, or exceed `PATH_TILE_LIMIT`.
pub fn compute_path_tile_grid(path_bounds: IRect, clip_bounds: IRect) -> Option<PathTileGrid> {
    let mut intersection = IRect::make_empty();
    if !intersection.intersect_pair(path_bounds, clip_bounds) {
        return None;
    }
    let width = intersection.right.checked_sub(intersection.left)?;
    let height = intersection.bottom.checked_sub(intersection.top)?;
    let tiles_x = width.checked_add(PATH_TILE_SIZE - 1)?.checked_div(PATH_TILE_SIZE)?;
    let tiles_y = height.checked_add(PATH_TILE_SIZE - 1)?.checked_div(PATH_TILE_SIZE)?;
    let total_tiles = tiles_x.checked_mul(tiles_y)?;
    if total_tiles > PATH_TILE_LIMIT {
        return None;
    }
    Some(PathTileGrid {
        intersection,
        tile_size: PATH_TILE_SIZE,
        tiles_x,
        tiles_y,
    })
}

// ---------------------------------------------------------------------------
// `RegionBuilder` (safe Rust port of `SkRgnBuilder`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Scanline {
    last_y: i32,
    xs: Vec<i32>,
}

/// Safe Rust port of `SkRgnBuilder`.
///
/// Accumulates horizontal spans (`blit_h`) in ascending Y -> X order, merges
/// adjacent spans on the same scanline, collapses identical consecutive rows,
/// inserts empty gap rows when Y skips, and exports exact SkRegion RLE runs.
#[derive(Clone, Debug, Default)]
pub struct RegionBuilder {
    top: i32,
    scanlines: Vec<Scanline>,
    curr: Option<Scanline>,
    finished: bool,
}

impl RegionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(max_height: i32, max_transitions: i32, path_is_inverse: bool) -> Option<Self> {
        // Perform the exact same overflow / sanity check as SkRgnBuilder::init
        let _cap = compute_rgn_builder_capacity(max_height, max_transitions, path_is_inverse)?;
        Some(Self {
            top: 0,
            scanlines: Vec::new(),
            curr: None,
            finished: false,
        })
    }

    fn collapse_or_push_curr(&mut self) {
        let Some(curr) = self.curr.take() else {
            return;
        };
        if let Some(prev) = self.scanlines.last_mut() {
            if prev.last_y + 1 == curr.last_y && prev.xs == curr.xs {
                prev.last_y = curr.last_y;
                return;
            }
        }
        self.scanlines.push(curr);
    }

    /// Appends a horizontal span `[x, x + width)` at scanline `y`.
    pub fn blit_h(&mut self, x: i32, y: i32, width: i32) {
        debug_assert!(!self.finished);
        match self.curr.as_mut() {
            None => {
                self.top = y;
                self.curr = Some(Scanline {
                    last_y: y,
                    xs: vec![x, x.wrapping_add(width)],
                });
            }
            Some(curr) => {
                debug_assert!(y >= curr.last_y);
                if y > curr.last_y {
                    let prev_last_y = curr.last_y;
                    self.collapse_or_push_curr();
                    if y - 1 > prev_last_y {
                        self.scanlines.push(Scanline {
                            last_y: y - 1,
                            xs: Vec::new(),
                        });
                    }
                    self.curr = Some(Scanline {
                        last_y: y,
                        xs: vec![x, x.wrapping_add(width)],
                    });
                } else if curr.xs.last().copied() == Some(x) {
                    *curr.xs.last_mut().unwrap() = x.wrapping_add(width);
                } else {
                    curr.xs.push(x);
                    curr.xs.push(x.wrapping_add(width));
                }
            }
        }
    }

    /// Flushes the final in-progress scanline.
    pub fn done(&mut self) {
        if !self.finished {
            self.collapse_or_push_curr();
            self.finished = true;
        }
    }

    /// Computes exact SkRegion `RunType` count (`0` if empty, `7` if single rect).
    pub fn compute_run_count(&self) -> usize {
        if self.scanlines.is_empty() {
            return 0;
        }
        // Top-Y (1) + per scanline: [Bottom, IntervalCount, xs..., X-Sentinel] + Y-Sentinel (1)
        let mut total = 2usize;
        for line in &self.scanlines {
            total += 3 + line.xs.len();
        }
        total
    }

    /// If `compute_run_count() == RECT_REGION_RUNS`, returns the single `IRect`.
    pub fn to_rect(&self) -> Option<IRect> {
        if self.compute_run_count() != RECT_REGION_RUNS || self.scanlines.len() != 1 {
            return None;
        }
        let line = &self.scanlines[0];
        if line.xs.len() != 2 {
            return None;
        }
        Some(IRect::make_ltrb(
            line.xs[0],
            self.top,
            line.xs[1],
            line.last_y.wrapping_add(1),
        ))
    }

    /// Writes the SkRegion RLE runs into `dst` (length must equal `compute_run_count()`).
    pub fn copy_to_runs(&self, dst: &mut [i32]) {
        debug_assert_eq!(dst.len(), self.compute_run_count());
        let mut i = 0usize;
        dst[i] = self.top;
        i += 1;
        for line in &self.scanlines {
            dst[i] = line.last_y.wrapping_add(1);
            i += 1;
            dst[i] = (line.xs.len() >> 1) as i32;
            i += 1;
            if !line.xs.is_empty() {
                dst[i..i + line.xs.len()].copy_from_slice(&line.xs);
                i += line.xs.len();
            }
            dst[i] = RUN_TYPE_SENTINEL;
            i += 1;
        }
        dst[i] = RUN_TYPE_SENTINEL;
        i += 1;
        debug_assert_eq!(i, dst.len());
    }

    /// Finishes and returns a `Region`.
    pub fn into_region(mut self) -> Region {
        self.done();
        let count = self.compute_run_count();
        if count == 0 {
            Region::Empty
        } else if let Some(rect) = self.to_rect() {
            Region::from_rect(rect)
        } else {
            let mut runs = vec![0_i32; count];
            self.copy_to_runs(&mut runs);
            let info = crate::region::compute_run_bounds(&runs);
            if info.bounds.is_empty() {
                Region::Empty
            } else {
                Region::Complex {
                    bounds: info.bounds,
                    y_span_count: info.y_span_count,
                    interval_count: info.interval_count,
                    runs,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Boundary path extraction (safe Rust port of `SkRegion::addBoundaryPath`)
// ---------------------------------------------------------------------------

/// Path command emitted by `extract_boundary_path`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PathCmd {
    /// 0 = MoveTo(x, y), 1 = LineTo(x, y), 2 = Close
    pub verb: u8,
    pub x: f32,
    pub y: f32,
}

pub const PATH_CMD_MOVE: u8 = 0;
pub const PATH_CMD_LINE: u8 = 1;
pub const PATH_CMD_CLOSE: u8 = 2;

const Y0_LINK: u8 = 0x01;
const Y1_LINK: u8 = 0x02;
const COMPLETE_LINK: u8 = Y0_LINK | Y1_LINK;

#[derive(Copy, Clone, Debug)]
struct Edge {
    x: i32,
    y0: i32,
    y1: i32,
    flags: u8,
    next: usize,
}

impl Edge {
    #[inline]
    fn new(x: i32, y0: i32, y1: i32) -> Self {
        debug_assert!(y0 != y1);
        Self {
            x,
            y0,
            y1,
            flags: 0,
            next: usize::MAX,
        }
    }

    #[inline]
    fn top(self) -> i32 {
        self.y0.min(self.y1)
    }
}

fn find_link(edges: &mut [Edge], base: usize) {
    if edges[base].flags == COMPLETE_LINK {
        return;
    }

    let y0 = edges[base].y0;
    let y1 = edges[base].y1;

    if (edges[base].flags & Y0_LINK) == 0 {
        for e in (base + 1)..edges.len() {
            if (edges[e].flags & Y1_LINK) == 0 && y0 == edges[e].y1 {
                edges[e].next = base;
                edges[e].flags |= Y1_LINK;
                break;
            }
        }
    }

    if (edges[base].flags & Y1_LINK) == 0 {
        for e in (base + 1)..edges.len() {
            if (edges[e].flags & Y0_LINK) == 0 && y1 == edges[e].y0 {
                edges[base].next = e;
                edges[e].flags |= Y0_LINK;
                break;
            }
        }
    }

    edges[base].flags = COMPLETE_LINK;
}

fn extract_contour(edges: &mut [Edge], cmds: &mut Vec<PathCmd>) -> usize {
    let mut start = 0usize;
    while edges[start].flags == 0 {
        start += 1;
    }

    let base = start;
    let mut prev = base;
    let mut cur = edges[prev].next;
    debug_assert_ne!(cur, base);

    let mut count = 1usize;
    cmds.push(PathCmd {
        verb: PATH_CMD_MOVE,
        x: edges[prev].x as f32,
        y: edges[prev].y0 as f32,
    });
    edges[prev].flags = 0;

    while cur != base {
        let prev_e = edges[prev];
        let cur_e = edges[cur];
        if prev_e.x != cur_e.x || prev_e.y1 != cur_e.y0 {
            cmds.push(PathCmd {
                verb: PATH_CMD_LINE,
                x: prev_e.x as f32,
                y: prev_e.y1 as f32,
            });
            cmds.push(PathCmd {
                verb: PATH_CMD_LINE,
                x: cur_e.x as f32,
                y: cur_e.y0 as f32,
            });
        }
        prev = cur;
        cur = edges[cur].next;
        count += 1;
        edges[prev].flags = 0;
    }

    cmds.push(PathCmd {
        verb: PATH_CMD_LINE,
        x: edges[prev].x as f32,
        y: edges[prev].y1 as f32,
    });
    cmds.push(PathCmd {
        verb: PATH_CMD_CLOSE,
        x: 0.0,
        y: 0.0,
    });
    count
}

/// Extracts boundary path commands for a complex region's RLE `runs` array.
/// Matches `SkRegion::addBoundaryPath` for complex regions.
pub fn extract_boundary_path_from_complex_runs(runs: &[i32]) -> Vec<PathCmd> {
    let mut edges = Vec::new();
    let mut idx = 0usize;
    let mut top = runs[idx];
    idx += 1;
    let mut bot = runs[idx];
    idx += 1;

    loop {
        let intervals = runs[idx] as usize;
        idx += 1;
        for _ in 0..intervals {
            let left = runs[idx];
            let right = runs[idx + 1];
            idx += 2;
            edges.push(Edge::new(left, bot, top));
            edges.push(Edge::new(right, top, bot));
        }
        debug_assert_eq!(runs[idx], RUN_TYPE_SENTINEL);
        idx += 1;
        top = bot;
        bot = runs[idx];
        idx += 1;
        if bot == RUN_TYPE_SENTINEL {
            break;
        }
    }

    extract_boundary_from_edges(&mut edges)
}

/// Extracts boundary path commands for any `Region`.
/// Returns empty vec if the region is empty.
pub fn extract_boundary_path(rgn: &Region) -> Vec<PathCmd> {
    match rgn {
        Region::Empty => Vec::new(),
        Region::Rect(b) => {
            let (l, t, r, bot) = (b.left as f32, b.top as f32, b.right as f32, b.bottom as f32);
            vec![
                PathCmd { verb: PATH_CMD_MOVE, x: l, y: t },
                PathCmd { verb: PATH_CMD_LINE, x: r, y: t },
                PathCmd { verb: PATH_CMD_LINE, x: r, y: bot },
                PathCmd { verb: PATH_CMD_LINE, x: l, y: bot },
                PathCmd { verb: PATH_CMD_CLOSE, x: 0.0, y: 0.0 },
            ]
        }
        Region::Complex { runs, .. } => extract_boundary_path_from_complex_runs(runs),
    }
}

fn extract_boundary_from_edges(edges: &mut [Edge]) -> Vec<PathCmd> {
    edges.sort_unstable_by(|a, b| {
        if a.x == b.x {
            a.top().cmp(&b.top())
        } else {
            a.x.cmp(&b.x)
        }
    });

    for i in 0..edges.len() {
        find_link(edges, i);
    }

    let mut remaining = edges.len();
    let mut cmds = Vec::with_capacity(remaining * 2);
    while remaining > 0 {
        remaining -= extract_contour(edges, &mut cmds);
    }
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::RegionOp;

    #[test]
    fn region_builder_collapses_rows_and_inserts_gaps() {
        let mut b = RegionBuilder::new();
        // Band 1: y=0..2, [0, 10) and [20, 30)
        b.blit_h(0, 0, 5);
        b.blit_h(5, 0, 5); // extends [0, 10)
        b.blit_h(20, 0, 10);
        b.blit_h(0, 1, 10);
        b.blit_h(20, 1, 10);
        // Gap at y=2..4
        // Band 2: y=4..6, [10, 20)
        b.blit_h(10, 4, 10);
        b.blit_h(10, 5, 10);

        let rgn = b.into_region();
        assert!(rgn.is_complex());
        assert!(rgn.is_valid());
        let rects: Vec<IRect> = rgn.iter().collect();
        assert_eq!(
            rects,
            vec![
                IRect::make_ltrb(0, 0, 10, 2),
                IRect::make_ltrb(20, 0, 30, 2),
                IRect::make_ltrb(10, 4, 20, 6),
            ]
        );
    }

    #[test]
    fn region_builder_single_rect() {
        let mut b = RegionBuilder::new();
        for y in 10..20 {
            b.blit_h(5, y, 15);
        }
        let rgn = b.into_region();
        assert!(rgn.is_rect());
        assert_eq!(rgn.bounds(), IRect::make_ltrb(5, 10, 20, 20));
    }

    #[test]
    fn boundary_path_l_shape_and_ring() {
        // L-shape: 1 closed contour with 6 vertices (Move + 5 Line + Close)
        let mut l = Region::Empty;
        l.set_rects(&[
            IRect::make_ltrb(0, 0, 10, 20),
            IRect::make_ltrb(0, 10, 20, 20),
        ]);
        let cmds = extract_boundary_path(&l);
        let close_count = cmds.iter().filter(|c| c.verb == PATH_CMD_CLOSE).count();
        assert_eq!(close_count, 1);
        assert_eq!(cmds[0].verb, PATH_CMD_MOVE);

        // Ring (rect with hole): 2 closed contours (outer + inner hole)
        let big = Region::from_rect(IRect::make_ltrb(0, 0, 30, 30));
        let hole = Region::from_rect(IRect::make_ltrb(10, 10, 20, 20));
        let mut ring = Region::Empty;
        ring.op_regions(&big, &hole, RegionOp::Difference);
        let cmds = extract_boundary_path(&ring);
        let close_count = cmds.iter().filter(|c| c.verb == PATH_CMD_CLOSE).count();
        assert_eq!(close_count, 2);
    }
}
