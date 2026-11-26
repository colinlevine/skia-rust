#![forbid(unsafe_code)]

//! Port of `SkPathOpsTightBounds.cpp`.

use skia_rs_geometry::Rect;
use skia_rs_path::iter::RangeIter;
use skia_rs_path::types::{PathDataBuf, Verb};

use crate::arena::OpState;
use crate::edge_builder::OpEdgeBuilder;
use crate::types::{between, cpp_max_f32, cpp_min_f32, SK_SCALAR_MAX};

pub fn compute_tight_bounds(path: &PathDataBuf) -> Option<Rect> {
    let mut move_bounds = Rect::new(SK_SCALAR_MAX, SK_SCALAR_MAX, -SK_SCALAR_MAX, -SK_SCALAR_MAX);
    let mut well_behaved = true;
    for (verb, pts, _w) in RangeIter::new(&path.points, &path.verbs, &path.conics) {
        match verb {
            Verb::Move => {
                move_bounds.left = cpp_min_f32(move_bounds.left, pts[0].x);
                move_bounds.top = cpp_min_f32(move_bounds.top, pts[0].y);
                move_bounds.right = cpp_max_f32(move_bounds.right, pts[0].x);
                move_bounds.bottom = cpp_max_f32(move_bounds.bottom, pts[0].y);
            }
            Verb::Quad | Verb::Conic => {
                if well_behaved {
                    well_behaved &= between(pts[0].x as f64, pts[1].x as f64, pts[2].x as f64);
                    well_behaved &= between(pts[0].y as f64, pts[1].y as f64, pts[2].y as f64);
                }
            }
            Verb::Cubic => {
                if well_behaved {
                    well_behaved &= between(pts[0].x as f64, pts[1].x as f64, pts[3].x as f64);
                    well_behaved &= between(pts[0].y as f64, pts[1].y as f64, pts[3].y as f64);
                    well_behaved &= between(pts[0].x as f64, pts[2].x as f64, pts[3].x as f64);
                    well_behaved &= between(pts[0].y as f64, pts[2].y as f64, pts[3].y as f64);
                }
            }
            _ => {}
        }
    }
    if well_behaved {
        return Some(Rect::bounds_or_empty(&path.points));
    }

    let mut state = OpState::new();
    let mut contour_list = state.contour_head;
    let mut builder = OpEdgeBuilder::new(path, contour_list);
    if !builder.finish(&mut state) {
        return None;
    }
    if !state.sort_contour_list(&mut contour_list, false, false) {
        return Some(move_bounds);
    }

    let mut bounds = state.contours[contour_list].bounds;
    let mut cur_opt = state.contours[contour_list].next;
    while let Some(cur) = cur_opt {
        bounds.add_bounds(&state.contours[cur].bounds);
        cur_opt = state.contours[cur].next;
    }
    let mut result = bounds.as_rect();
    if !move_bounds.is_empty() {
        result.join(move_bounds);
    }
    Some(result)
}
