#![forbid(unsafe_code)]

use crate::geometry::chop_cubic_at_1;
use crate::{scalar_interp, Point, Rect};

pub fn chop_mono_at_y(pts: &[Point; 4], y: f32) -> Option<f32> {
    let ycrv = [
        pts[0].y - y,
        pts[1].y - y,
        pts[2].y - y,
        pts[3].y - y,
    ];

    let (mut t_neg, mut t_pos) = if ycrv[0] < 0.0 {
        if ycrv[3] < 0.0 {
            return None;
        }
        (0.0_f32, 1.0_f32)
    } else if ycrv[0] > 0.0 {
        if ycrv[3] > 0.0 {
            return None;
        }
        (1.0_f32, 0.0_f32)
    } else {
        return Some(0.0);
    };

    const TOL: f32 = 1.0 / 65536.0;
    loop {
        let t_mid = (t_pos + t_neg) / 2.0;
        let y01 = scalar_interp(ycrv[0], ycrv[1], t_mid);
        let y12 = scalar_interp(ycrv[1], ycrv[2], t_mid);
        let y23 = scalar_interp(ycrv[2], ycrv[3], t_mid);
        let y012 = scalar_interp(y01, y12, t_mid);
        let y123 = scalar_interp(y12, y23, t_mid);
        let y0123 = scalar_interp(y012, y123, t_mid);
        if y0123 == 0.0 {
            return Some(t_mid);
        }
        if y0123 < 0.0 {
            t_neg = t_mid;
        } else {
            t_pos = t_mid;
        }
        if (t_pos - t_neg).abs() <= TOL {
            break;
        }
    }
    Some((t_neg + t_pos) / 2.0)
}

pub fn clip_cubic(clip: Rect, src_pts: &[Point; 4], dst: &mut [Point; 4]) -> bool {
    let reverse = if src_pts[0].y > src_pts[3].y {
        dst[0] = src_pts[3];
        dst[1] = src_pts[2];
        dst[2] = src_pts[1];
        dst[3] = src_pts[0];
        true
    } else {
        *dst = *src_pts;
        false
    };

    let ctop = clip.top;
    let cbot = clip.bottom;
    if dst[3].y <= ctop || dst[0].y >= cbot {
        return false;
    }

    let mut tmp = [Point::ZERO; 7];
    if dst[0].y < ctop {
        if let Some(t) = chop_mono_at_y(dst, ctop) {
            chop_cubic_at_1(dst, &mut tmp, t);
            dst[0] = tmp[3];
            dst[1] = tmp[4];
            dst[2] = tmp[5];
        }
    }
    if dst[3].y > cbot {
        if let Some(t) = chop_mono_at_y(dst, cbot) {
            chop_cubic_at_1(dst, &mut tmp, t);
            dst[1] = tmp[1];
            dst[2] = tmp[2];
            dst[3] = tmp[3];
        }
    }

    if reverse {
        dst.swap(0, 3);
        dst.swap(1, 2);
    }
    true
}
