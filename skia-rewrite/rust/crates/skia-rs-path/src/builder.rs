#![forbid(unsafe_code)]

use crate::analysis::{compute_tight_bounds, contains, perspective_clip};
use crate::iter::{PathIter, RangeIter};
use crate::shapes::{
    build_rrect_shape, fill_oval_points, fill_rect_points, simplify_rrect, transform_dir_and_start,
    RRectAsEnum, OVAL_VERBS, RECT_VERBS,
};
use crate::types::{
    degrees_to_radians, find_last_move_to_index, is_axis_aligned, is_zero_length_since_point,
    scalar_cos_snap_to_zero, scalar_sin_snap_to_zero, sk_float_ceil2int, sk_float_round,
    trimmed_bounds, AddPathMode, ArcSize, ArcType, Convexity, Direction, FillType, FirstDirection,
    PathDataBuf, PathIsAType, PathRaw, Verb, FOUR_QUARTER_CIRCLE_CONICS, SEG_CONIC, SEG_CUBIC,
    SEG_LINE, SEG_QUAD,
};
use skia_rs_geometry::geometry::{chop_cubic_at_half, Conic, MAX_CONICS_FOR_ARC};
use skia_rs_geometry::{
    copysign_f32, cpp_max_f32, scalar_nearly_equal, scalar_nearly_zero, Matrix, Point, RRect, Rect,
    Vector, SK_SCALAR_NEARLY_ZERO, SK_SCALAR_PI,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathBuilder {
    pub buf: PathDataBuf,
}

fn is_empty_or_moves(verbs: &[Verb]) -> bool {
    verbs.is_empty() || (verbs.len() == 1 && verbs[0] == Verb::Move)
}

fn subdivide_cubic_to(builder: &mut PathBuilder, pts: &[Point; 4], level: i32) {
    let next_level = level - 1;
    if next_level >= 0 {
        let mut tmp = [Point::ZERO; 7];
        chop_cubic_at_half(pts, &mut tmp);
        subdivide_cubic_to(builder, &[tmp[0], tmp[1], tmp[2], tmp[3]], next_level);
        subdivide_cubic_to(builder, &[tmp[3], tmp[4], tmp[5], tmp[6]], next_level);
    } else {
        builder.cubic_to(pts[1], pts[2], pts[3]);
    }
}

impl PathBuilder {
    pub fn new(fill_type: FillType) -> Self {
        Self {
            buf: PathDataBuf::new(fill_type),
        }
    }

    pub fn from_buf(buf: PathDataBuf) -> Self {
        Self { buf }
    }

    pub fn into_buf(self) -> PathDataBuf {
        self.buf
    }

    pub fn reset(&mut self) -> &mut Self {
        self.buf.points.clear();
        self.buf.verbs.clear();
        self.buf.conics.clear();
        self.buf.fill_type = FillType::Winding;
        self.buf.is_volatile = false;
        self.buf.segment_mask = 0;
        self.buf.last_move_index = -1;
        self.buf.is_a_type = PathIsAType::General;
        self.buf.convexity = Convexity::Unknown;
        self
    }

    pub fn set_fill_type(&mut self, ft: FillType) -> &mut Self {
        self.buf.fill_type = ft;
        self
    }

    pub fn fill_type(&self) -> FillType {
        self.buf.fill_type
    }

    pub fn set_is_volatile(&mut self, v: bool) -> &mut Self {
        self.buf.is_volatile = v;
        self
    }

    pub fn toggle_inverse_fill_type(&mut self) -> &mut Self {
        self.buf.fill_type = self.buf.fill_type.toggle_inverse();
        self
    }

    pub fn is_empty(&self) -> bool {
        self.buf.verbs.is_empty()
    }

    pub fn points(&self) -> &[Point] {
        &self.buf.points
    }

    pub fn verbs(&self) -> &[Verb] {
        &self.buf.verbs
    }

    pub fn conics(&self) -> &[f32] {
        &self.buf.conics
    }

    pub fn count_points(&self) -> usize {
        self.buf.points.len()
    }

    pub fn get_last_pt(&self) -> Option<Point> {
        self.buf.points.last().copied()
    }

    pub fn set_point(&mut self, index: usize, p: Point) {
        if index < self.buf.points.len() {
            self.buf.points[index] = p;
            self.buf.is_a_type = PathIsAType::General;
        }
    }

    pub fn set_last_point(&mut self, p: Point) {
        if let Some(last) = self.buf.points.last_mut() {
            *last = p;
            self.buf.is_a_type = PathIsAType::General;
        }
    }

    pub fn inc_reserve(&mut self, extra_pts: usize, extra_vbs: usize, extra_cns: usize) {
        self.buf.points.reserve(extra_pts);
        self.buf.verbs.reserve(extra_vbs);
        self.buf.conics.reserve(extra_cns);
    }

    fn ensure_move(&mut self) {
        self.buf.is_a_type = PathIsAType::General;
        if self.buf.verbs.is_empty() {
            self.move_to(Point::ZERO);
        } else if *self.buf.verbs.last().unwrap() == Verb::Close {
            let pt = self.buf.points[self.buf.last_move_index as usize];
            self.move_to(pt);
        }
    }

    pub fn move_to(&mut self, pt: Point) -> &mut Self {
        if self.buf.verbs.last() == Some(&Verb::Move) {
            *self.buf.points.last_mut().unwrap() = pt;
        } else {
            self.buf.last_move_index = self.buf.points.len() as i32;
            self.buf.points.push(pt);
            self.buf.verbs.push(Verb::Move);
            if self.buf.is_a_type == PathIsAType::Oval || self.buf.is_a_type == PathIsAType::RRect {
                self.buf.is_a_type = PathIsAType::General;
            }
            self.buf.convexity = Convexity::Unknown;
        }
        self
    }

    pub fn line_to(&mut self, pt: Point) -> &mut Self {
        self.ensure_move();
        self.buf.points.push(pt);
        self.buf.verbs.push(Verb::Line);
        self.buf.segment_mask |= SEG_LINE;
        self
    }

    pub fn quad_to(&mut self, pt1: Point, pt2: Point) -> &mut Self {
        self.ensure_move();
        self.buf.points.push(pt1);
        self.buf.points.push(pt2);
        self.buf.verbs.push(Verb::Quad);
        self.buf.segment_mask |= SEG_QUAD;
        self
    }

    pub fn conic_to(&mut self, pt1: Point, pt2: Point, w: f32) -> &mut Self {
        self.ensure_move();
        if w <= 0.0 {
            return self.line_to(pt2);
        }
        self.buf.points.push(pt1);
        self.buf.points.push(pt2);
        if w == 1.0 {
            self.buf.verbs.push(Verb::Quad);
            self.buf.segment_mask |= SEG_QUAD;
        } else if w.is_finite() {
            self.buf.verbs.push(Verb::Conic);
            self.buf.conics.push(w);
            self.buf.segment_mask |= SEG_CONIC;
        } else {
            self.buf.verbs.push(Verb::Line);
            self.buf.verbs.push(Verb::Line);
            self.buf.segment_mask |= SEG_LINE;
        }
        self
    }

    pub fn cubic_to(&mut self, pt1: Point, pt2: Point, pt3: Point) -> &mut Self {
        self.ensure_move();
        self.buf.points.push(pt1);
        self.buf.points.push(pt2);
        self.buf.points.push(pt3);
        self.buf.verbs.push(Verb::Cubic);
        self.buf.segment_mask |= SEG_CUBIC;
        self
    }

    pub fn close(&mut self) -> &mut Self {
        if !self.buf.verbs.is_empty() && *self.buf.verbs.last().unwrap() != Verb::Close {
            self.ensure_move();
            self.buf.verbs.push(Verb::Close);
        }
        self
    }

    pub fn r_move_to(&mut self, pt: Vector) -> &mut Self {
        let last_pt = if !self.buf.points.is_empty() {
            if *self.buf.verbs.last().unwrap() == Verb::Close {
                self.buf.points[self.buf.last_move_index as usize]
            } else {
                *self.buf.points.last().unwrap()
            }
        } else {
            Point::ZERO
        };
        self.move_to(last_pt + pt)
    }

    pub fn r_line_to(&mut self, p1: Vector) -> &mut Self {
        self.ensure_move();
        let base = *self.buf.points.last().unwrap();
        self.line_to(base + p1)
    }

    pub fn r_quad_to(&mut self, p1: Vector, p2: Vector) -> &mut Self {
        self.ensure_move();
        let base = *self.buf.points.last().unwrap();
        self.quad_to(base + p1, base + p2)
    }

    pub fn r_conic_to(&mut self, p1: Vector, p2: Vector, w: f32) -> &mut Self {
        self.ensure_move();
        let base = *self.buf.points.last().unwrap();
        self.conic_to(base + p1, base + p2, w)
    }

    pub fn r_cubic_to(&mut self, p1: Vector, p2: Vector, p3: Vector) -> &mut Self {
        self.ensure_move();
        let base = *self.buf.points.last().unwrap();
        self.cubic_to(base + p1, base + p2, base + p3)
    }

    pub fn add_raw(&mut self, raw: &PathRaw<'_>) -> &mut Self {
        self.inc_reserve(raw.points.len(), raw.verbs.len(), raw.conics.len());
        let mut iter = PathIter::new(raw.points, raw.verbs, raw.conics);
        while let Some(rec) = iter.next_rec() {
            let pts = rec.pts();
            match rec.verb {
                Verb::Move => {
                    self.move_to(pts[0]);
                }
                Verb::Line => {
                    self.line_to(pts[1]);
                }
                Verb::Quad => {
                    self.quad_to(pts[1], pts[2]);
                }
                Verb::Conic => {
                    self.conic_to(pts[1], pts[2], rec.conic_weight);
                }
                Verb::Cubic => {
                    self.cubic_to(pts[1], pts[2], pts[3]);
                }
                Verb::Close => {
                    self.close();
                }
            }
        }
        let has_trailing_move = |vbs: &[Verb]| vbs.last() == Some(&Verb::Move);
        if has_trailing_move(raw.verbs) && !has_trailing_move(&self.buf.verbs) {
            if let Some(&last_pt) = raw.points.last() {
                self.move_to(last_pt);
            }
        }
        self
    }

    pub fn add_rect(&mut self, rect: Rect, dir: Direction, index: u32) -> &mut Self {
        let was_empty = is_empty_or_moves(&self.buf.verbs);
        let pts = fill_rect_points(rect, dir, index);
        let raw = PathRaw {
            points: &pts,
            verbs: &RECT_VERBS,
            conics: &[],
            bounds: rect,
            fill_type: FillType::Winding,
            convexity: dir.to_convexity(),
            segment_mask: SEG_LINE,
        };
        self.add_raw(&raw);
        if was_empty {
            self.buf.convexity = dir.to_convexity();
        }
        self
    }

    pub fn add_oval(&mut self, oval: Rect, dir: Direction, index: u32) -> &mut Self {
        let was_empty = is_empty_or_moves(&self.buf.verbs);
        let pts = fill_oval_points(oval, dir, index);
        let raw = PathRaw {
            points: &pts,
            verbs: &OVAL_VERBS,
            conics: &FOUR_QUARTER_CIRCLE_CONICS,
            bounds: oval,
            fill_type: FillType::Winding,
            convexity: dir.to_convexity(),
            segment_mask: SEG_CONIC,
        };
        self.add_raw(&raw);
        if was_empty {
            self.buf.is_a_type = PathIsAType::Oval;
            self.buf.is_a_dir = dir;
            self.buf.is_a_start_index = (index % 4) as u8;
            self.buf.convexity = dir.to_convexity();
        }
        self
    }

    pub fn add_rrect(&mut self, rrect: &RRect, dir: Direction, index: u32) -> &mut Self {
        let bounds = rrect.rect;
        let (as_type, new_index) = simplify_rrect(rrect, index);
        match as_type {
            RRectAsEnum::Rect => return self.add_rect(bounds, dir, new_index),
            RRectAsEnum::Oval => return self.add_oval(bounds, dir, new_index),
            RRectAsEnum::RRect => {}
        }
        let was_empty = is_empty_or_moves(&self.buf.verbs);
        let shape = build_rrect_shape(rrect, dir, index);
        let raw = shape.as_raw();
        self.add_raw(&raw);
        if was_empty {
            self.buf.is_a_type = PathIsAType::RRect;
            self.buf.is_a_dir = dir;
            self.buf.is_a_start_index = (index % 8) as u8;
            self.buf.convexity = dir.to_convexity();
        }
        self
    }

    pub fn add_circle(&mut self, center: Point, r: f32, dir: Direction) -> &mut Self {
        if r >= 0.0 {
            self.add_oval(
                Rect::new(center.x - r, center.y - r, center.x + r, center.y + r),
                dir,
                1,
            );
        }
        self
    }

    pub fn add_polygon(&mut self, pts: &[Point], is_closed: bool) -> &mut Self {
        if pts.is_empty() {
            return self;
        }
        self.move_to(pts[0]);
        self.polyline_to(&pts[1..]);
        if is_closed {
            self.close();
        }
        self
    }

    pub fn polyline_to(&mut self, pts: &[Point]) -> &mut Self {
        if !pts.is_empty() {
            self.ensure_move();
            let count = pts.len();
            self.inc_reserve(count, count, 0);
            self.buf.points.extend_from_slice(pts);
            self.buf.verbs.resize(self.buf.verbs.len() + count, Verb::Line);
            self.buf.segment_mask |= SEG_LINE;
        }
        self
    }

    pub fn offset(&mut self, dx: f32, dy: f32) -> &mut Self {
        let delta = Vector::new(dx, dy);
        for p in &mut self.buf.points {
            *p += delta;
        }
        self
    }

    pub fn arc_to_oval(
        &mut self,
        oval: Rect,
        mut start_angle: f32,
        sweep_angle: f32,
        mut force_move_to: bool,
    ) -> &mut Self {
        if oval.width() < 0.0 || oval.height() < 0.0 {
            return self;
        }
        start_angle %= 360.0_f32;
        if self.buf.verbs.is_empty() {
            force_move_to = true;
        }

        if let Some(lone_pt) = arc_is_lone_point(oval, start_angle, sweep_angle) {
            return if force_move_to {
                self.move_to(lone_pt)
            } else {
                self.line_to(lone_pt)
            };
        }

        let (start_v, stop_v, dir) = angles_to_unit_vectors(start_angle, sweep_angle);

        let add_pt = |builder: &mut PathBuilder, pt: Point| {
            if force_move_to {
                builder.move_to(pt);
            } else if !nearly_equal_pt(*builder.buf.points.last().unwrap(), pt) {
                builder.line_to(pt);
            }
        };

        if start_v == stop_v {
            let end_angle = degrees_to_radians(start_angle + sweep_angle);
            let radius_x = oval.width() / 2.0;
            let radius_y = oval.height() / 2.0;
            let single_pt = Point::new(
                oval.center_x() + radius_x * end_angle.cos(),
                oval.center_y() + radius_y * end_angle.sin(),
            );
            add_pt(self, single_pt);
            return self;
        }

        let mut conics = [Conic::default(); MAX_CONICS_FOR_ARC];
        let mut single_pt = Point::ZERO;
        let count = build_arc_conics(oval, start_v, stop_v, dir, &mut conics, &mut single_pt);
        if count > 0 {
            self.inc_reserve((count as usize) * 2 + 1, 0, 0);
            let pt = conics[0].pts[0];
            add_pt(self, pt);
            for conic in &conics[..count as usize] {
                self.conic_to(conic.pts[1], conic.pts[2], conic.w);
            }
        } else {
            add_pt(self, single_pt);
        }
        self
    }

    pub fn add_arc(&mut self, oval: Rect, start_angle: f32, sweep_angle: f32) -> &mut Self {
        if oval.is_empty() || sweep_angle == 0.0 {
            return self;
        }
        const FULL_CIRCLE: f32 = 360.0;
        if sweep_angle >= FULL_CIRCLE || sweep_angle <= -FULL_CIRCLE {
            let start_over_90 = start_angle / 90.0;
            let start_over_90_i = sk_float_round(start_over_90);
            let error = start_over_90 - start_over_90_i;
            if scalar_nearly_equal(error, 0.0, SK_SCALAR_NEARLY_ZERO) {
                let mut start_index = (start_over_90_i + 1.0) % 4.0;
                if start_index < 0.0 {
                    start_index += 4.0;
                }
                let dir = if sweep_angle > 0.0 {
                    Direction::Cw
                } else {
                    Direction::Ccw
                };
                return self.add_oval(oval, dir, start_index as u32);
            }
        }
        self.arc_to_oval(oval, start_angle, sweep_angle, true)
    }

    pub fn arc_to_tangent(&mut self, p1: Point, p2: Point, radius: f32) -> &mut Self {
        self.ensure_move();
        if radius == 0.0 {
            return self.line_to(p1);
        }
        let start = *self.buf.points.last().unwrap();

        let bx = (p1.x - start.x) as f64;
        let by = (p1.y - start.y) as f64;
        let blen = (bx * bx + by * by).sqrt();
        let befored = (bx / blen, by / blen);

        let ax = (p2.x - p1.x) as f64;
        let ay = (p2.y - p1.y) as f64;
        let alen = (ax * ax + ay * ay).sqrt();
        let afterd = (ax / alen, ay / alen);

        let cosh = befored.0 * afterd.0 + befored.1 * afterd.1;
        let sinh = befored.0 * afterd.1 - befored.1 * afterd.0;

        if !(befored.0.is_finite()
            && befored.1.is_finite()
            && afterd.0.is_finite()
            && afterd.1.is_finite())
            || scalar_nearly_zero(sinh as f32, SK_SCALAR_NEARLY_ZERO)
        {
            return self.line_to(p1);
        }

        let dist = (((radius as f64) * (1.0 - cosh) / sinh) as f32).abs();
        let xx = (p1.x as f64 - (dist as f64) * befored.0) as f32;
        let yy = (p1.y as f64 - (dist as f64) * befored.1) as f32;

        let mut after = Vector::new(afterd.0 as f32, afterd.1 as f32);
        after.set_length_in_place(dist);
        self.line_to(Point::new(xx, yy));
        let weight = ((0.5_f64 + cosh * 0.5) as f32).sqrt();
        self.conic_to(p1, p1 + after, weight)
    }

    pub fn arc_to_svg(
        &mut self,
        rad: Point,
        angle: f32,
        arc_large: ArcSize,
        arc_sweep: Direction,
        end_pt: Point,
    ) -> &mut Self {
        self.ensure_move();
        let src_pts = [*self.buf.points.last().unwrap(), end_pt];

        if rad.x == 0.0 || rad.y == 0.0 {
            return self.line_to(end_pt);
        }
        if src_pts[0] == src_pts[1] {
            return self.line_to(end_pt);
        }

        let mut rx = rad.x.abs();
        let mut ry = rad.y.abs();
        let mid_point_distance = (src_pts[0] - src_pts[1]) * 0.5;

        let mut point_transform = Matrix::identity();
        point_transform.set_rotate(-angle);

        let mut transformed_mid = [Point::ZERO; 1];
        point_transform.map_points(&mut transformed_mid, &[mid_point_distance]);
        let transformed_mid = transformed_mid[0];

        let square_rx = rx * rx;
        let square_ry = ry * ry;
        let square_x = transformed_mid.x * transformed_mid.x;
        let square_y = transformed_mid.y * transformed_mid.y;

        let mut radii_scale = square_x / square_rx + square_y / square_ry;
        if radii_scale > 1.0 {
            radii_scale = radii_scale.sqrt();
            rx *= radii_scale;
            ry *= radii_scale;
        }

        point_transform.set_scale(1.0 / rx, 1.0 / ry);
        point_transform.pre_rotate(-angle);

        let mut unit_pts = [Point::ZERO; 2];
        point_transform.map_points(&mut unit_pts, &src_pts);
        let mut delta = unit_pts[1] - unit_pts[0];

        let d = delta.x * delta.x + delta.y * delta.y;
        let scale_factor_squared = cpp_max_f32(1.0 / d - 0.25, 0.0);

        let mut scale_factor = scale_factor_squared.sqrt();
        if (arc_sweep == Direction::Ccw) != (arc_large == ArcSize::Large) {
            scale_factor = -scale_factor;
        }
        delta = delta.scale(scale_factor);
        let mut center_point = (unit_pts[0] + unit_pts[1]) * 0.5;
        center_point.x += -delta.y;
        center_point.y += delta.x;
        unit_pts[0] -= center_point;
        unit_pts[1] -= center_point;

        let theta1 = unit_pts[0].y.atan2(unit_pts[0].x);
        let theta2 = unit_pts[1].y.atan2(unit_pts[1].x);
        let mut theta_arc = theta2 - theta1;
        if theta_arc < 0.0 && arc_sweep == Direction::Cw {
            theta_arc += SK_SCALAR_PI * 2.0;
        } else if theta_arc > 0.0 && arc_sweep != Direction::Cw {
            theta_arc -= SK_SCALAR_PI * 2.0;
        }

        if theta_arc.abs() < (SK_SCALAR_PI / (1000.0 * 1000.0)) {
            return self.line_to(end_pt);
        }

        point_transform.set_rotate(angle);
        point_transform.pre_scale(rx, ry);

        let segments = sk_float_ceil2int((theta_arc / (2.0 * SK_SCALAR_PI / 3.0)).abs());
        let theta_width = theta_arc / (segments as f32);
        let t = (0.5 * theta_width).tan();
        if !t.is_finite() {
            return self;
        }
        let mut start_theta = theta1;
        let w = (0.5 + theta_width.cos() * 0.5).sqrt();
        let scalar_is_int = |s: f32| s == s.floor();
        let expect_integers =
            scalar_nearly_zero(SK_SCALAR_PI / 2.0 - theta_width.abs(), SK_SCALAR_NEARLY_ZERO)
                && scalar_is_int(rx)
                && scalar_is_int(ry)
                && scalar_is_int(end_pt.x)
                && scalar_is_int(end_pt.y);

        for _ in 0..segments {
            let end_theta = start_theta + theta_width;
            let sin_end = scalar_sin_snap_to_zero(end_theta);
            let cos_end = scalar_cos_snap_to_zero(end_theta);

            unit_pts[1] = Point::new(cos_end, sin_end) + center_point;
            unit_pts[0] = Point::new(
                unit_pts[1].x + t * sin_end,
                unit_pts[1].y - t * cos_end,
            );
            let mut mapped = [Point::ZERO; 2];
            point_transform.map_points(&mut mapped, &unit_pts);
            if expect_integers {
                for pt in &mut mapped {
                    pt.x = sk_float_round(pt.x);
                    pt.y = sk_float_round(pt.y);
                }
            }
            self.conic_to(mapped[0], mapped[1], w);
            start_theta = end_theta;
        }

        if let Some(last) = self.buf.points.last_mut() {
            *last = end_pt;
        }
        self
    }

    pub fn r_arc_to(
        &mut self,
        r: Point,
        x_axis_rotate: f32,
        large_arc: ArcSize,
        sweep: Direction,
        dxdy: Vector,
    ) -> &mut Self {
        let current = self.get_last_pt().unwrap_or(Point::ZERO);
        self.arc_to_svg(r, x_axis_rotate, large_arc, sweep, current + dxdy)
    }

    pub fn add_path(
        &mut self,
        src_pts: &[Point],
        src_verbs: &[Verb],
        src_conics: &[f32],
        src_seg_mask: u8,
        src_is_finite: bool,
        src_is_a: (PathIsAType, Direction, u8),
        src_convexity: Convexity,
        src_is_volatile: bool,
        matrix: &Matrix,
        mode: AddPathMode,
    ) -> &mut Self {
        if src_verbs.is_empty() {
            return self;
        }

        let can_replace_this =
            (mode == AddPathMode::Append && self.buf.verbs.len() <= 1) || self.buf.verbs.is_empty();
        if can_replace_this && matrix.is_identity() {
            let fill_type = self.buf.fill_type;
            self.reset();
            self.buf.fill_type = fill_type;
            self.buf.is_volatile = src_is_volatile;
            let raw = PathRaw {
                points: src_pts,
                verbs: src_verbs,
                conics: src_conics,
                bounds: Rect::EMPTY,
                fill_type,
                convexity: src_convexity,
                segment_mask: src_seg_mask,
            };
            self.add_raw(&raw);
            self.buf.last_move_index = find_last_move_to_index(&self.buf.verbs, self.buf.points.len());
            self.buf.is_a_type = src_is_a.0;
            self.buf.is_a_dir = src_is_a.1;
            self.buf.is_a_start_index = src_is_a.2;
            self.buf.convexity = src_convexity;
            return self;
        }

        self.buf.convexity = Convexity::Unknown;

        if mode == AddPathMode::Append && !matrix.has_perspective() {
            if self.buf.verbs.last() == Some(&Verb::Move) {
                self.buf.verbs.pop();
                self.buf.points.pop();
            }
            let last_move_to_index = find_last_move_to_index(src_verbs, src_pts.len());
            self.buf.last_move_index = last_move_to_index + self.buf.points.len() as i32;

            self.buf.segment_mask |= src_seg_mask;
            self.buf.is_a_type = PathIsAType::General;
            self.buf.verbs.extend_from_slice(src_verbs);
            let old_len = self.buf.points.len();
            self.buf.points.resize(old_len + src_pts.len(), Point::ZERO);
            matrix.map_points(&mut self.buf.points[old_len..], src_pts);
            self.buf.conics.extend_from_slice(src_conics);
            return self;
        }

        let iter_verbs = if src_is_finite { src_verbs } else { &[] };
        let mut first_verb = true;
        for (verb, pts, w) in RangeIter::new(src_pts, iter_verbs, src_conics) {
            let mut mapped = [Point::ZERO; 3];
            match verb {
                Verb::Move => {
                    matrix.map_points(&mut mapped[..1], &pts[..1]);
                    if first_verb && mode == AddPathMode::Extend && !self.is_empty() {
                        self.ensure_move();
                        let last_pt = self.get_last_pt();
                        if last_pt != Some(mapped[0]) {
                            self.line_to(mapped[0]);
                        }
                    } else {
                        self.move_to(mapped[0]);
                    }
                }
                Verb::Line => {
                    matrix.map_points(&mut mapped[..1], &pts[1..2]);
                    self.line_to(mapped[0]);
                }
                Verb::Quad => {
                    matrix.map_points(&mut mapped[..2], &pts[1..3]);
                    self.quad_to(mapped[0], mapped[1]);
                }
                Verb::Conic => {
                    matrix.map_points(&mut mapped[..2], &pts[1..3]);
                    self.conic_to(mapped[0], mapped[1], w);
                }
                Verb::Cubic => {
                    matrix.map_points(&mut mapped[..3], &pts[1..4]);
                    self.cubic_to(mapped[0], mapped[1], mapped[2]);
                }
                Verb::Close => {
                    self.close();
                }
            }
            first_verb = false;
        }
        self
    }

    pub fn reverse_path_to(
        &mut self,
        src_pts: &[Point],
        src_verbs: &[Verb],
        src_conics: &[f32],
    ) -> &mut Self {
        if src_verbs.is_empty() {
            return self;
        }
        let mut p_idx = src_pts.len() - 1;
        let mut c_idx = src_conics.len();
        for &v in src_verbs.iter().rev() {
            let n = v.pts_in_verb();
            p_idx = p_idx.wrapping_sub(n);
            match v {
                Verb::Move => return self,
                Verb::Line => {
                    self.line_to(src_pts[p_idx]);
                }
                Verb::Quad => {
                    self.quad_to(src_pts[p_idx + 1], src_pts[p_idx]);
                }
                Verb::Conic => {
                    c_idx -= 1;
                    self.conic_to(src_pts[p_idx + 1], src_pts[p_idx], src_conics[c_idx]);
                }
                Verb::Cubic => {
                    self.cubic_to(src_pts[p_idx + 2], src_pts[p_idx + 1], src_pts[p_idx]);
                }
                Verb::Close => {}
            }
        }
        self
    }

    pub fn reverse_add_path(
        &mut self,
        src_pts: &[Point],
        src_verbs: &[Verb],
        src_conics: &[f32],
    ) -> &mut Self {
        if src_verbs.is_empty() {
            return self;
        }
        let mut p_idx = src_pts.len();
        let mut c_idx = src_conics.len();
        let mut need_move = true;
        let mut need_close = false;

        for &v in src_verbs.iter().rev() {
            let n = v.pts_in_verb();
            if need_move {
                p_idx -= 1;
                self.move_to(src_pts[p_idx]);
                need_move = false;
            }
            p_idx = p_idx.wrapping_sub(n);
            match v {
                Verb::Move => {
                    if need_close {
                        self.close();
                        need_close = false;
                    }
                    need_move = true;
                    p_idx = p_idx.wrapping_add(1);
                }
                Verb::Line => {
                    self.line_to(src_pts[p_idx]);
                }
                Verb::Quad => {
                    self.quad_to(src_pts[p_idx + 1], src_pts[p_idx]);
                }
                Verb::Conic => {
                    c_idx -= 1;
                    self.conic_to(src_pts[p_idx + 1], src_pts[p_idx], src_conics[c_idx]);
                }
                Verb::Cubic => {
                    self.cubic_to(src_pts[p_idx + 2], src_pts[p_idx + 1], src_pts[p_idx]);
                }
                Verb::Close => {
                    need_close = true;
                }
            }
        }
        self
    }

    pub fn transform(&mut self, matrix: &Matrix) -> &mut Self {
        if matrix.is_identity() || self.is_empty() {
            return self;
        }

        if matrix.has_perspective() {
            let fill_type = self.buf.fill_type;
            let src_buf = std::mem::replace(&mut self.buf, PathDataBuf::new(fill_type));
            self.buf.fill_type = fill_type;

            let src_finite = src_buf.is_finite();
            let active_buf = if let Some(raw) = src_buf.as_raw(false) {
                if let Some(clipped) = perspective_clip(&raw, matrix) {
                    clipped
                } else {
                    src_buf
                }
            } else {
                // Non-finite source -> empty iteration
                PathDataBuf::new(fill_type)
            };

            let iter_verbs = if src_finite && active_buf.is_finite() {
                &active_buf.verbs[..]
            } else {
                &[]
            };
            for (verb, pts, wt) in
                RangeIter::new(&active_buf.points, iter_verbs, &active_buf.conics)
            {
                match verb {
                    Verb::Move => {
                        self.move_to(pts[0]);
                    }
                    Verb::Line => {
                        self.line_to(pts[1]);
                    }
                    Verb::Quad => {
                        let w = Conic::transform_w(&[pts[0], pts[1], pts[2]], 1.0, matrix);
                        self.conic_to(pts[1], pts[2], w);
                    }
                    Verb::Conic => {
                        let w = Conic::transform_w(&[pts[0], pts[1], pts[2]], wt, matrix);
                        self.conic_to(pts[1], pts[2], w);
                    }
                    Verb::Cubic => {
                        subdivide_cubic_to(self, &[pts[0], pts[1], pts[2], pts[3]], 2);
                    }
                    Verb::Close => {
                        self.close();
                    }
                }
            }
        } else {
            if !matrix.rect_stays_rect() || !is_axis_aligned(&self.buf.points) {
                self.buf.is_a_type = PathIsAType::General;
                if self.buf.convexity.is_convex() {
                    self.buf.convexity = Convexity::Unknown;
                }
            }
            if self.buf.is_a_type == PathIsAType::Oval || self.buf.is_a_type == PathIsAType::RRect {
                let (dir, start) = transform_dir_and_start(
                    matrix,
                    self.buf.is_a_type == PathIsAType::RRect,
                    self.buf.is_a_dir,
                    self.buf.is_a_start_index as u32,
                );
                self.buf.is_a_dir = dir;
                self.buf.is_a_start_index = start as u8;
            }
        }

        matrix.map_points_in_place(&mut self.buf.points);
        self
    }

    pub fn compute_finite_bounds(&self) -> Option<Rect> {
        trimmed_bounds(&self.buf.points, &self.buf.verbs)
    }

    pub fn compute_bounds(&self) -> Rect {
        self.compute_finite_bounds().unwrap_or(Rect::EMPTY)
    }

    pub fn compute_tight_bounds(&self) -> Option<Rect> {
        if !self.buf.is_finite() {
            return None;
        }
        Some(compute_tight_bounds(
            &self.buf.points,
            &self.buf.verbs,
            &self.buf.conics,
        ))
    }

    pub fn is_zero_length_since_point(&self, start_pt_index: usize) -> bool {
        is_zero_length_since_point(&self.buf.points, start_pt_index)
    }

    pub fn contains(&self, p: Point) -> bool {
        if let Some(raw) = self.buf.as_raw(false) {
            contains(&raw, p)
        } else {
            false
        }
    }
}

fn arc_is_lone_point(oval: Rect, start_angle: f32, sweep_angle: f32) -> Option<Point> {
    if sweep_angle == 0.0 && (start_angle == 0.0 || start_angle == 360.0) {
        Some(Point::new(oval.right, oval.center_y()))
    } else if oval.width() == 0.0 && oval.height() == 0.0 {
        Some(Point::new(oval.right, oval.top))
    } else {
        None
    }
}

fn angles_to_unit_vectors(start_angle: f32, sweep_angle: f32) -> (Vector, Vector, Direction) {
    let start_rad = degrees_to_radians(start_angle);
    let mut stop_rad = degrees_to_radians(start_angle + sweep_angle);

    let start_v = Vector::new(
        scalar_cos_snap_to_zero(start_rad),
        scalar_sin_snap_to_zero(start_rad),
    );
    let mut stop_v = Vector::new(
        scalar_cos_snap_to_zero(stop_rad),
        scalar_sin_snap_to_zero(stop_rad),
    );

    if start_v == stop_v {
        let sw = sweep_angle.abs();
        if sw < 360.0 && sw > 359.0 {
            let delta_rad = copysign_f32(1.0 / 512.0, sweep_angle);
            loop {
                stop_rad -= delta_rad;
                stop_v.y = scalar_sin_snap_to_zero(stop_rad);
                stop_v.x = scalar_cos_snap_to_zero(stop_rad);
                if start_v != stop_v {
                    break;
                }
            }
        }
    }
    let dir = if sweep_angle > 0.0 {
        Direction::Cw
    } else {
        Direction::Ccw
    };
    (start_v, stop_v, dir)
}

fn build_arc_conics(
    oval: Rect,
    start: Vector,
    stop: Vector,
    dir: Direction,
    conics: &mut [Conic; MAX_CONICS_FOR_ARC],
    single_pt: &mut Point,
) -> i32 {
    let mut matrix = Matrix::identity();
    matrix.set_scale(oval.width() / 2.0, oval.height() / 2.0);
    matrix.post_translate(oval.center_x(), oval.center_y());

    let count = Conic::build_unit_arc(start, stop, dir == Direction::Ccw, Some(&matrix), conics);
    if count == 0 {
        let mut out = [Point::ZERO; 1];
        matrix.map_points(&mut out, &[stop]);
        *single_pt = out[0];
    }
    count
}

#[inline]
fn nearly_equal_pt(a: Point, b: Point) -> bool {
    scalar_nearly_equal(a.x, b.x, SK_SCALAR_NEARLY_ZERO)
        && scalar_nearly_equal(a.y, b.y, SK_SCALAR_NEARLY_ZERO)
}

/// Matching `SkPathPriv::DrawArcIsConvex`.
pub fn draw_arc_is_convex(sweep_angle: f32, arc_type: ArcType, is_fill_no_path_effect: bool) -> bool {
    if is_fill_no_path_effect && sweep_angle.abs() >= 360.0 {
        return true;
    }
    if arc_type == ArcType::Wedge {
        return sweep_angle.abs() <= 180.0;
    }
    sweep_angle.abs() <= 360.0
}

/// Matching `SkPathPriv::CreateDrawArcPath`.
pub fn create_draw_arc_path(
    oval: Rect,
    mut start_angle: f32,
    mut sweep_angle: f32,
    arc_type: ArcType,
    is_fill_no_path_effect: bool,
) -> PathDataBuf {
    if sweep_angle.abs() > 3600.0 {
        sweep_angle = copysign_f32(3600.0, sweep_angle) + (sweep_angle % 360.0);
    }

    let mut builder = PathBuilder::new(FillType::Winding);
    builder.set_is_volatile(true);

    if is_fill_no_path_effect && sweep_angle.abs() >= 360.0 {
        builder.add_oval(oval, Direction::Cw, 1);
        return builder.into_buf();
    }

    let is_wedge = arc_type == ArcType::Wedge;
    if is_wedge {
        builder.move_to(Point::new(oval.center_x(), oval.center_y()));
    }
    let first_dir = if sweep_angle > 0.0 {
        FirstDirection::Cw
    } else {
        FirstDirection::Ccw
    };
    let convex = draw_arc_is_convex(sweep_angle, arc_type, is_fill_no_path_effect);
    let mut force_move_to = !is_wedge;
    while sweep_angle <= -360.0 {
        builder.arc_to_oval(oval, start_angle, -180.0, force_move_to);
        start_angle -= 180.0;
        builder.arc_to_oval(oval, start_angle, -180.0, false);
        start_angle -= 180.0;
        force_move_to = false;
        sweep_angle += 360.0;
    }
    while sweep_angle >= 360.0 {
        builder.arc_to_oval(oval, start_angle, 180.0, force_move_to);
        start_angle += 180.0;
        builder.arc_to_oval(oval, start_angle, 180.0, false);
        start_angle += 180.0;
        force_move_to = false;
        sweep_angle -= 360.0;
    }
    builder.arc_to_oval(oval, start_angle, sweep_angle, force_move_to);
    if is_wedge {
        builder.close();
    }

    let mut buf = builder.into_buf();
    buf.convexity = if convex {
        first_dir.to_convexity()
    } else {
        Convexity::Concave
    };
    buf
}

// ============================================================================
// Interpolation (matching SkPath_interpolate.cpp)
// ============================================================================

pub fn can_interpolate(
    from_pts_len: usize,
    from_verbs: &[Verb],
    from_conics: &[f32],
    to_pts_len: usize,
    to_verbs: &[Verb],
    to_conics: &[f32],
) -> bool {
    from_pts_len == to_pts_len && from_verbs == to_verbs && from_conics == to_conics
}

pub fn interpolate_points(from_pts: &[Point], to_pts: &[Point], t: f32) -> Vec<Point> {
    from_pts
        .iter()
        .zip(to_pts.iter())
        .map(|(&from, &to)| {
            let diff = to - from;
            Point::new(from.x + diff.x * t, from.y + diff.y * t)
        })
        .collect()
}
