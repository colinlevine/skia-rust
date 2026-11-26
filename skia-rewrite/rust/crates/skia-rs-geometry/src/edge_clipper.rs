#![forbid(unsafe_code)]

use crate::geometry::{
    chop_cubic_at_1, chop_cubic_at_x_extrema, chop_cubic_at_y_extrema, chop_mono_cubic_at_x,
    chop_mono_cubic_at_y, chop_quad_at, chop_quad_at_x_extrema, chop_quad_at_y_extrema,
    find_unit_quad_roots,
};
use crate::line_clipper::{clip_line, LINE_CLIPPER_MAX_POINTS};
use crate::{cpp_min_f32, Point, Rect};

pub const EDGE_CLIPPER_MAX_VERBS: usize = 18;
pub const EDGE_CLIPPER_MAX_POINTS: usize = 54;

pub const VERB_LINE: u8 = 1;
pub const VERB_QUAD: u8 = 2;
pub const VERB_CUBIC: u8 = 4;

pub struct EdgeClipOutput {
    pub verbs: [u8; EDGE_CLIPPER_MAX_VERBS],
    pub verb_count: usize,
    pub points: [Point; EDGE_CLIPPER_MAX_POINTS],
    pub point_count: usize,
    pub can_cull_to_the_right: bool,
}

impl EdgeClipOutput {
    pub fn new(can_cull_to_the_right: bool) -> Self {
        Self {
            verbs: [0; EDGE_CLIPPER_MAX_VERBS],
            verb_count: 0,
            points: [Point::ZERO; EDGE_CLIPPER_MAX_POINTS],
            point_count: 0,
            can_cull_to_the_right,
        }
    }

    fn append_line(&mut self, p0: Point, p1: Point) {
        self.verbs[self.verb_count] = VERB_LINE;
        self.verb_count += 1;
        self.points[self.point_count] = p0;
        self.points[self.point_count + 1] = p1;
        self.point_count += 2;
    }

    fn append_v_line(&mut self, x: f32, mut y0: f32, mut y1: f32, reverse: bool) {
        self.verbs[self.verb_count] = VERB_LINE;
        self.verb_count += 1;
        if reverse {
            std::mem::swap(&mut y0, &mut y1);
        }
        self.points[self.point_count] = Point::new(x, y0);
        self.points[self.point_count + 1] = Point::new(x, y1);
        self.point_count += 2;
    }

    fn append_quad(&mut self, pts: &[Point], reverse: bool) {
        self.verbs[self.verb_count] = VERB_QUAD;
        self.verb_count += 1;
        if reverse {
            self.points[self.point_count] = pts[2];
            self.points[self.point_count + 2] = pts[0];
        } else {
            self.points[self.point_count] = pts[0];
            self.points[self.point_count + 2] = pts[2];
        }
        self.points[self.point_count + 1] = pts[1];
        self.point_count += 3;
    }

    fn append_cubic(&mut self, pts: &[Point], reverse: bool) {
        self.verbs[self.verb_count] = VERB_CUBIC;
        self.verb_count += 1;
        if reverse {
            for i in 0..4 {
                self.points[self.point_count + i] = pts[3 - i];
            }
        } else {
            self.points[self.point_count..self.point_count + 4].copy_from_slice(&pts[..4]);
        }
        self.point_count += 4;
    }

    pub fn clip_line(&mut self, p0: Point, p1: Point, clip: Rect) -> bool {
        self.verb_count = 0;
        self.point_count = 0;
        let mut lines = [Point::ZERO; LINE_CLIPPER_MAX_POINTS];
        let count = clip_line(&[p0, p1], clip, &mut lines, self.can_cull_to_the_right);
        for i in 0..(count as usize) {
            self.append_line(lines[i], lines[i + 1]);
        }
        self.verb_count != 0
    }

    fn clip_mono_quad(&mut self, src_pts: &[Point; 3], clip: Rect) {
        let mut pts = [Point::ZERO; 3];
        let mut reverse = sort_increasing_y(&mut pts, src_pts);

        if pts[2].y <= clip.top || pts[0].y >= clip.bottom {
            return;
        }
        chop_quad_in_y(&mut pts, clip);

        if pts[0].x > pts[2].x {
            pts.swap(0, 2);
            reverse = !reverse;
        }

        if pts[2].x <= clip.left {
            self.append_v_line(clip.left, pts[0].y, pts[2].y, reverse);
            return;
        }
        if pts[0].x >= clip.right {
            if !self.can_cull_to_the_right {
                self.append_v_line(clip.right, pts[0].y, pts[2].y, reverse);
            }
            return;
        }

        let mut tmp = [Point::ZERO; 5];
        if pts[0].x < clip.left {
            if let Some(t) = chop_mono_quad_at(pts[0].x, pts[1].x, pts[2].x, clip.left) {
                chop_quad_at(&pts, &mut tmp, t);
                self.append_v_line(clip.left, tmp[0].y, tmp[2].y, reverse);
                tmp[2].x = clip.left;
                if tmp[3].x < clip.left {
                    tmp[3].x = clip.left;
                }
                pts[0] = tmp[2];
                pts[1] = tmp[3];
            } else {
                self.append_v_line(clip.left, pts[0].y, pts[2].y, reverse);
                return;
            }
        }

        if pts[2].x > clip.right {
            if let Some(t) = chop_mono_quad_at(pts[0].x, pts[1].x, pts[2].x, clip.right) {
                chop_quad_at(&pts, &mut tmp, t);
                if tmp[1].x > clip.right {
                    tmp[1].x = clip.right;
                }
                tmp[2].x = clip.right;
                self.append_quad(&tmp[0..3], reverse);
                self.append_v_line(clip.right, tmp[2].y, tmp[4].y, reverse);
            } else {
                pts[1].x = cpp_min_f32(pts[1].x, clip.right);
                pts[2].x = cpp_min_f32(pts[2].x, clip.right);
                self.append_quad(&pts, reverse);
            }
        } else {
            self.append_quad(&pts, reverse);
        }
    }

    pub fn clip_quad(&mut self, src_pts: &[Point; 3], clip: Rect) -> bool {
        self.verb_count = 0;
        self.point_count = 0;
        let bounds = Rect::bounds_or_empty(src_pts);
        if !(bounds.top >= clip.bottom || bounds.bottom <= clip.top) {
            let mut mono_y = [Point::ZERO; 5];
            let count_y = chop_quad_at_y_extrema(src_pts, &mut mono_y);
            for y in 0..=(count_y as usize) {
                let mut mono_x = [Point::ZERO; 5];
                let seg_y = [mono_y[y * 2], mono_y[y * 2 + 1], mono_y[y * 2 + 2]];
                let count_x = chop_quad_at_x_extrema(&seg_y, &mut mono_x);
                for x in 0..=(count_x as usize) {
                    let seg_x = [mono_x[x * 2], mono_x[x * 2 + 1], mono_x[x * 2 + 2]];
                    self.clip_mono_quad(&seg_x, clip);
                }
            }
        }
        self.verb_count != 0
    }

    fn clip_mono_cubic(&mut self, src: &[Point; 4], clip: Rect) {
        let mut pts = [Point::ZERO; 4];
        let mut reverse = sort_increasing_y(&mut pts, src);

        if pts[3].y <= clip.top || pts[0].y >= clip.bottom {
            return;
        }
        chop_cubic_in_y(&mut pts, clip);

        if pts[0].x > pts[3].x {
            pts.swap(0, 3);
            pts.swap(1, 2);
            reverse = !reverse;
        }

        if pts[3].x <= clip.left {
            self.append_v_line(clip.left, pts[0].y, pts[3].y, reverse);
            return;
        }
        if pts[0].x >= clip.right {
            if !self.can_cull_to_the_right {
                self.append_v_line(clip.right, pts[0].y, pts[3].y, reverse);
            }
            return;
        }

        if pts[0].x < clip.left {
            let mut tmp = [Point::ZERO; 7];
            chop_mono_cubic_at_x_or_closest(&pts, clip.left, &mut tmp);
            self.append_v_line(clip.left, tmp[0].y, tmp[3].y, reverse);
            tmp[3].x = clip.left;
            if tmp[4].x < clip.left {
                tmp[4].x = clip.left;
            }
            pts[0] = tmp[3];
            pts[1] = tmp[4];
            pts[2] = tmp[5];
        }

        if pts[3].x > clip.right {
            let mut tmp = [Point::ZERO; 7];
            chop_mono_cubic_at_x_or_closest(&pts, clip.right, &mut tmp);
            tmp[3].x = clip.right;
            if tmp[2].x > clip.right {
                tmp[2].x = clip.right;
            }
            self.append_cubic(&tmp[0..4], reverse);
            self.append_v_line(clip.right, tmp[3].y, tmp[6].y, reverse);
        } else {
            self.append_cubic(&pts, reverse);
        }
    }

    pub fn clip_cubic(&mut self, src_pts: &[Point; 4], clip: Rect) -> bool {
        self.verb_count = 0;
        self.point_count = 0;
        let bounds = Rect::bounds_or_empty(src_pts);
        if bounds.bottom > clip.top && bounds.top < clip.bottom {
            let limit = (1_i32 << 22) as f32;
            if bounds.left < -limit
                || bounds.top < -limit
                || bounds.right > limit
                || bounds.bottom > limit
            {
                return self.clip_line(src_pts[0], src_pts[3], clip);
            }
            let mut mono_y = [Point::ZERO; 10];
            let count_y = chop_cubic_at_y_extrema(src_pts, Some(&mut mono_y));
            for y in 0..=(count_y as usize) {
                let mut mono_x = [Point::ZERO; 10];
                let seg_y = [
                    mono_y[y * 3],
                    mono_y[y * 3 + 1],
                    mono_y[y * 3 + 2],
                    mono_y[y * 3 + 3],
                ];
                let count_x = chop_cubic_at_x_extrema(&seg_y, Some(&mut mono_x));
                for x in 0..=(count_x as usize) {
                    let seg_x = [
                        mono_x[x * 3],
                        mono_x[x * 3 + 1],
                        mono_x[x * 3 + 2],
                        mono_x[x * 3 + 3],
                    ];
                    self.clip_mono_cubic(&seg_x, clip);
                }
            }
        }
        self.verb_count != 0
    }
}

fn sort_increasing_y(dst: &mut [Point], src: &[Point]) -> bool {
    let count = src.len();
    if src[0].y > src[count - 1].y {
        for i in 0..count {
            dst[i] = src[count - i - 1];
        }
        true
    } else {
        dst.copy_from_slice(src);
        false
    }
}

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

fn chop_quad_in_y(pts: &mut [Point; 3], clip: Rect) {
    let mut tmp = [Point::ZERO; 5];
    if pts[0].y < clip.top {
        if let Some(t) = chop_mono_quad_at(pts[0].y, pts[1].y, pts[2].y, clip.top) {
            chop_quad_at(pts, &mut tmp, t);
            tmp[2].y = clip.top;
            if tmp[3].y < clip.top {
                tmp[3].y = clip.top;
            }
            pts[0] = tmp[2];
            pts[1] = tmp[3];
        } else {
            for p in pts.iter_mut() {
                if p.y < clip.top {
                    p.y = clip.top;
                }
            }
        }
    }
    if pts[2].y > clip.bottom {
        if let Some(t) = chop_mono_quad_at(pts[0].y, pts[1].y, pts[2].y, clip.bottom) {
            chop_quad_at(pts, &mut tmp, t);
            if tmp[1].y > clip.bottom {
                tmp[1].y = clip.bottom;
            }
            tmp[2].y = clip.bottom;
            pts[1] = tmp[1];
            pts[2] = tmp[2];
        } else {
            for p in pts.iter_mut() {
                if p.y > clip.bottom {
                    p.y = clip.bottom;
                }
            }
        }
    }
}

fn mono_cubic_closest_t(c0: f32, c1: f32, c2: f32, c3: f32, mut x: f32) -> f32 {
    let mut t = 0.5_f32;
    let mut best_t = 0.0_f32;
    let mut step = 0.25_f32;
    let d = c0;
    let a = c3 + 3.0 * (c1 - c2) - d;
    let b = 3.0 * (c2 - c1 - c1 + d);
    let c = 3.0 * (c1 - d);
    x -= d;
    let mut closest = f32::MAX;
    loop {
        let loc = ((a * t + b) * t + c) * t;
        let dist = (loc - x).abs();
        if closest > dist {
            closest = dist;
            best_t = t;
        }
        let last_t = t;
        t += if loc < x { step } else { -step };
        step *= 0.5;
        if !(closest > 0.25 && last_t != t) {
            break;
        }
    }
    best_t
}

fn chop_mono_cubic_at_y_or_closest(src: &[Point; 4], y: f32, dst: &mut [Point; 7]) {
    if chop_mono_cubic_at_y(src, y, dst) {
        return;
    }
    let t = mono_cubic_closest_t(src[0].y, src[1].y, src[2].y, src[3].y, y);
    chop_cubic_at_1(src, dst, t);
}

fn chop_mono_cubic_at_x_or_closest(src: &[Point; 4], x: f32, dst: &mut [Point; 7]) {
    if chop_mono_cubic_at_x(src, x, dst) {
        return;
    }
    let t = mono_cubic_closest_t(src[0].x, src[1].x, src[2].x, src[3].x, x);
    chop_cubic_at_1(src, dst, t);
}

fn chop_cubic_in_y(pts: &mut [Point; 4], clip: Rect) {
    if pts[0].y < clip.top {
        let mut tmp = [Point::ZERO; 7];
        chop_mono_cubic_at_y_or_closest(pts, clip.top, &mut tmp);
        if tmp[3].y < clip.top && tmp[4].y < clip.top && tmp[5].y < clip.top {
            let tmp2 = [tmp[3], tmp[4], tmp[5], tmp[6]];
            chop_mono_cubic_at_y_or_closest(&tmp2, clip.top, &mut tmp);
        }
        tmp[3].y = clip.top;
        if tmp[4].y < clip.top {
            tmp[4].y = clip.top;
        }
        pts[0] = tmp[3];
        pts[1] = tmp[4];
        pts[2] = tmp[5];
    }
    if pts[3].y > clip.bottom {
        let mut tmp = [Point::ZERO; 7];
        chop_mono_cubic_at_y_or_closest(pts, clip.bottom, &mut tmp);
        tmp[3].y = clip.bottom;
        if tmp[2].y > clip.bottom {
            tmp[2].y = clip.bottom;
        }
        pts[1] = tmp[1];
        pts[2] = tmp[2];
        pts[3] = tmp[3];
    }
}
