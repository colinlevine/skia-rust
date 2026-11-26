#![forbid(unsafe_code)]

use crate::geometry::{chop_quad_at, find_unit_quad_roots};
use crate::{Point, Rect};

fn chop_mono_quad_at(c0: f32, c1: f32, c2: f32, target: f32) -> Option<f32> {
    let a = c0 - c1 - c1 + c2;
    let b = 2.0 * (c1 - c0);
    let c = c0 - target;
    let mut roots = [0.0_f32; 2];
    if find_unit_quad_roots(a, b, c, &mut roots) != 0 {
        Some(roots[0])
    } else {
        None
    }
}

pub fn clip_quad(clip: Rect, src_pts: &[Point; 3], dst: &mut [Point; 3]) -> bool {
    let reverse = if src_pts[0].y > src_pts[2].y {
        dst[0] = src_pts[2];
        dst[1] = src_pts[1];
        dst[2] = src_pts[0];
        true
    } else {
        *dst = *src_pts;
        false
    };

    let ctop = clip.top;
    let cbot = clip.bottom;
    if dst[2].y <= ctop || dst[0].y >= cbot {
        return false;
    }

    let mut tmp = [Point::ZERO; 5];
    if dst[0].y < ctop {
        if let Some(t) = chop_mono_quad_at(dst[0].y, dst[1].y, dst[2].y, ctop) {
            chop_quad_at(dst, &mut tmp, t);
            dst[0] = tmp[2];
            dst[1] = tmp[3];
        } else {
            for p in dst.iter_mut() {
                if p.y < ctop {
                    p.y = ctop;
                }
            }
        }
    }

    if dst[2].y > cbot {
        if let Some(t) = chop_mono_quad_at(dst[0].y, dst[1].y, dst[2].y, cbot) {
            chop_quad_at(dst, &mut tmp, t);
            dst[1] = tmp[1];
            dst[2] = tmp[2];
        } else {
            for p in dst.iter_mut() {
                if p.y > cbot {
                    p.y = cbot;
                }
            }
        }
    }

    if reverse {
        dst.swap(0, 2);
    }
    true
}
