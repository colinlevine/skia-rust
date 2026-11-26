#![forbid(unsafe_code)]

use crate::{
    all_finite, cpp_max_f32, cpp_min_f32, float_midpoint, scalar_nearly_equal, Point, Rect,
    SK_SCALAR_ROOT_2_OVER_2,
};

pub const TYPE_EMPTY: i32 = 0;
pub const TYPE_RECT: i32 = 1;
pub const TYPE_OVAL: i32 = 2;
pub const TYPE_SIMPLE: i32 = 3;
pub const TYPE_NINE_PATCH: i32 = 4;
pub const TYPE_COMPLEX: i32 = 5;
pub const LAST_TYPE: i32 = TYPE_COMPLEX;

pub const CORNER_UL: usize = 0;
pub const CORNER_UR: usize = 1;
pub const CORNER_LR: usize = 2;
pub const CORNER_LL: usize = 3;

pub const SIZE_IN_MEMORY: usize = 48;

/// Matches `SkRRect` (`{ SkRect fRect; SkVector fRadii[4]; int32_t fType; }`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RRect {
    pub rect: Rect,
    pub radii: [Point; 4],
    pub rtype: i32,
}

impl Default for RRect {
    fn default() -> Self {
        Self::make_empty()
    }
}

#[inline]
fn nextafter_toward_zero(x: f32) -> f32 {
    if x.is_nan() || x == 0.0 {
        return x;
    }
    let bits = x.to_bits();
    f32::from_bits(bits - 1)
}

/// Matches `SkScaleToSides::AdjustRadii`.
fn adjust_radii(scale: f64, a: &mut f32, b: &mut f32, min_coord: f32, max_coord: f32) {
    let diff_d = (max_coord as f64) - (min_coord as f64);
    let diff_f = (max_coord - min_coord) as f64;
    let limit = if diff_f < diff_d { diff_f } else { diff_d };

    if scale < 1.0 {
        *a = ((*a as f64) * scale) as f32;
        *b = ((*b as f64) * scale) as f32;
    }

    if max_coord - *b < min_coord {
        *b = nextafter_toward_zero(max_coord - min_coord);
    }
    if min_coord + *a > max_coord {
        *a = nextafter_toward_zero(max_coord - min_coord);
    }

    if ((*a + *b) as f64) > limit {
        let swap = *a > *b;
        let (min_val, max_ref) = if swap { (*b, a) } else { (*a, b) };
        let mut new_max = (limit - (min_val as f64)) as f32;
        while ((new_max + min_val) as f64) > limit && new_max > 0.0 {
            new_max = nextafter_toward_zero(new_max);
        }
        *max_ref = new_max;
    }
}

fn clamp_to_zero(radii: &mut [Point; 4]) -> bool {
    let mut all_square = true;
    for r in radii.iter_mut() {
        if r.x <= 0.0 || r.y <= 0.0 {
            r.x = 0.0;
            r.y = 0.0;
        } else {
            all_square = false;
        }
    }
    all_square
}

fn radii_are_nine_patch(radii: &[Point; 4]) -> bool {
    radii[CORNER_UL].x == radii[CORNER_LL].x
        && radii[CORNER_UL].y == radii[CORNER_UR].y
        && radii[CORNER_UR].x == radii[CORNER_LR].x
        && radii[CORNER_LL].y == radii[CORNER_LR].y
}

#[inline]
fn compute_min_scale(rad1: f64, rad2: f64, limit: f64, cur_min: f64) -> f64 {
    if (rad1 + rad2) > limit {
        let ratio = limit / (rad1 + rad2);
        if ratio < cur_min { ratio } else { cur_min }
    } else {
        cur_min
    }
}

#[inline]
fn flush_to_zero(a: &mut f32, b: &mut f32) {
    if *a + *b == *a {
        *b = 0.0;
    } else if *a + *b == *b {
        *a = 0.0;
    }
}

fn are_radii_predicates_valid(
    radius_from_min: f32,
    radius_from_max: f32,
    min_coord: f32,
    max_coord: f32,
) -> bool {
    if min_coord > max_coord || radius_from_min < 0.0 || radius_from_max < 0.0 {
        return false;
    }
    let limit = max_coord - min_coord;
    let sum = radius_from_min + radius_from_max;
    let pt_from_min = min_coord + radius_from_min;
    let pt_from_max = max_coord - radius_from_max;
    sum <= limit && min_coord <= pt_from_max && pt_from_min <= max_coord
}

impl RRect {
    #[inline]
    pub const fn make_empty() -> Self {
        Self {
            rect: Rect::make_empty(),
            radii: [Point::ZERO; 4],
            rtype: TYPE_EMPTY,
        }
    }

    pub fn make_rect(rect: Rect) -> Self {
        let mut rr = Self::make_empty();
        rr.set_rect(rect);
        rr
    }

    pub fn make_oval(oval: Rect) -> Self {
        let mut rr = Self::make_empty();
        rr.set_oval(oval);
        rr
    }

    pub fn initialize_rect(&mut self, rect: Rect) -> bool {
        if !rect.is_finite() {
            *self = Self::make_empty();
            return false;
        }
        self.rect = rect.make_sorted();
        if self.rect.is_empty() {
            self.radii = [Point::ZERO; 4];
            self.rtype = TYPE_EMPTY;
            return false;
        }
        true
    }

    pub fn set_rect(&mut self, rect: Rect) {
        if !self.initialize_rect(rect) {
            return;
        }
        self.radii = [Point::ZERO; 4];
        self.rtype = TYPE_RECT;
    }

    pub fn set_oval(&mut self, oval: Rect) {
        if !self.initialize_rect(oval) {
            return;
        }
        let x_rad = self.rect.half_width();
        let y_rad = self.rect.half_height();
        if x_rad == 0.0 || y_rad == 0.0 {
            self.radii = [Point::ZERO; 4];
            self.rtype = TYPE_RECT;
        } else {
            for r in &mut self.radii {
                r.set(x_rad, y_rad);
            }
            self.rtype = TYPE_OVAL;
        }
    }

    pub fn set_rect_xy(&mut self, rect: Rect, mut x_rad: f32, mut y_rad: f32) {
        if !self.initialize_rect(rect) {
            return;
        }
        if !x_rad.is_finite() || !y_rad.is_finite() {
            x_rad = 0.0;
            y_rad = 0.0;
        }
        let was_circular = x_rad == y_rad;
        if self.rect.width() < x_rad + x_rad || self.rect.height() < y_rad + y_rad {
            let scale = cpp_min_f32(
                self.rect.width() / (x_rad + x_rad),
                self.rect.height() / (y_rad + y_rad),
            );
            x_rad *= scale;
            y_rad *= scale;
            x_rad = cpp_min_f32(x_rad, self.rect.half_width());
            y_rad = cpp_min_f32(y_rad, self.rect.half_height());
            if was_circular {
                let m = cpp_min_f32(x_rad, y_rad);
                x_rad = m;
                y_rad = m;
            }
        }
        if x_rad <= 0.0 || y_rad <= 0.0 {
            self.set_rect(rect);
            return;
        }
        for r in &mut self.radii {
            r.set(x_rad, y_rad);
        }
        self.rtype = if x_rad >= self.rect.half_width() && y_rad >= self.rect.half_height() {
            TYPE_OVAL
        } else {
            TYPE_SIMPLE
        };
    }

    pub fn set_nine_patch(
        &mut self,
        rect: Rect,
        mut left_rad: f32,
        mut top_rad: f32,
        mut right_rad: f32,
        mut bottom_rad: f32,
    ) {
        if !self.initialize_rect(rect) {
            return;
        }
        if !left_rad.is_finite()
            || !top_rad.is_finite()
            || !right_rad.is_finite()
            || !bottom_rad.is_finite()
        {
            self.set_rect(rect);
            return;
        }
        left_rad = cpp_max_f32(left_rad, 0.0);
        top_rad = cpp_max_f32(top_rad, 0.0);
        right_rad = cpp_max_f32(right_rad, 0.0);
        bottom_rad = cpp_max_f32(bottom_rad, 0.0);

        let mut scale = 1.0_f32;
        if left_rad + right_rad > self.rect.width() {
            scale = self.rect.width() / (left_rad + right_rad);
        }
        if top_rad + bottom_rad > self.rect.height() {
            scale = cpp_min_f32(scale, self.rect.height() / (top_rad + bottom_rad));
        }
        if scale < 1.0 {
            left_rad *= scale;
            top_rad *= scale;
            right_rad *= scale;
            bottom_rad *= scale;
        }

        if left_rad == right_rad && top_rad == bottom_rad {
            if left_rad >= (self.rect.width() / 2.0) && top_rad >= (self.rect.height() / 2.0) {
                self.rtype = TYPE_OVAL;
                left_rad = self.rect.half_width();
                right_rad = left_rad;
                top_rad = self.rect.half_height();
                bottom_rad = top_rad;
            } else if left_rad == 0.0 || top_rad == 0.0 {
                self.rtype = TYPE_RECT;
                left_rad = 0.0;
                top_rad = 0.0;
                right_rad = 0.0;
                bottom_rad = 0.0;
            } else {
                self.rtype = TYPE_SIMPLE;
            }
        } else {
            self.rtype = TYPE_NINE_PATCH;
        }

        self.radii[CORNER_UL].set(left_rad, top_rad);
        self.radii[CORNER_UR].set(right_rad, top_rad);
        self.radii[CORNER_LR].set(right_rad, bottom_rad);
        self.radii[CORNER_LL].set(left_rad, bottom_rad);

        if clamp_to_zero(&mut self.radii) {
            self.set_rect(rect);
            return;
        }
        if self.rtype == TYPE_NINE_PATCH && !radii_are_nine_patch(&self.radii) {
            self.rtype = TYPE_COMPLEX;
        }
    }

    pub fn set_rect_radii(&mut self, rect: Rect, radii: &[Point; 4]) {
        if !self.initialize_rect(rect) {
            return;
        }
        let flat = [
            radii[0].x, radii[0].y, radii[1].x, radii[1].y,
            radii[2].x, radii[2].y, radii[3].x, radii[3].y,
        ];
        if !all_finite(&flat) {
            self.set_rect(rect);
            return;
        }
        self.radii = *radii;
        if clamp_to_zero(&mut self.radii) {
            self.set_rect(rect);
            return;
        }
        self.scale_radii();
    }

    pub fn scale_radii(&mut self) -> bool {
        let was_simple = self.radii[0] == self.radii[1]
            && self.radii[0] == self.radii[2]
            && self.radii[0] == self.radii[3];
        let was_circular = was_simple && self.radii[0].x == self.radii[0].y;

        let mut scale = 1.0_f64;
        let width = (self.rect.right as f64) - (self.rect.left as f64);
        let height = (self.rect.bottom as f64) - (self.rect.top as f64);
        scale = compute_min_scale(self.radii[0].x as f64, self.radii[1].x as f64, width, scale);
        scale = compute_min_scale(self.radii[1].y as f64, self.radii[2].y as f64, height, scale);
        scale = compute_min_scale(self.radii[2].x as f64, self.radii[3].x as f64, width, scale);
        scale = compute_min_scale(self.radii[3].y as f64, self.radii[0].y as f64, height, scale);

        let (mut r0x, mut r1x) = (self.radii[0].x, self.radii[1].x);
        flush_to_zero(&mut r0x, &mut r1x);
        adjust_radii(scale, &mut r0x, &mut r1x, self.rect.left, self.rect.right);
        self.radii[0].x = r0x;
        self.radii[1].x = r1x;

        let (mut r1y, mut r2y) = (self.radii[1].y, self.radii[2].y);
        flush_to_zero(&mut r1y, &mut r2y);
        adjust_radii(scale, &mut r1y, &mut r2y, self.rect.top, self.rect.bottom);
        self.radii[1].y = r1y;
        self.radii[2].y = r2y;

        let (mut r2x, mut r3x) = (self.radii[2].x, self.radii[3].x);
        flush_to_zero(&mut r2x, &mut r3x);
        adjust_radii(scale, &mut r3x, &mut r2x, self.rect.left, self.rect.right);
        self.radii[2].x = r2x;
        self.radii[3].x = r3x;

        let (mut r3y, mut r0y) = (self.radii[3].y, self.radii[0].y);
        flush_to_zero(&mut r3y, &mut r0y);
        adjust_radii(scale, &mut r0y, &mut r3y, self.rect.top, self.rect.bottom);
        self.radii[3].y = r3y;
        self.radii[0].y = r0y;

        if !clamp_to_zero(&mut self.radii) && was_simple {
            let mut min_x = cpp_min_f32(
                cpp_min_f32(self.radii[0].x, self.radii[1].x),
                cpp_min_f32(self.radii[2].x, self.radii[3].x),
            );
            let mut min_y = cpp_min_f32(
                cpp_min_f32(self.radii[0].y, self.radii[1].y),
                cpp_min_f32(self.radii[2].y, self.radii[3].y),
            );
            if was_circular {
                let m = cpp_min_f32(min_x, min_y);
                min_x = m;
                min_y = m;
            }
            for r in &mut self.radii {
                r.set(min_x, min_y);
            }
        }

        self.compute_type();
        scale < 1.0
    }

    pub fn compute_type(&mut self) {
        if self.rect.is_empty() {
            self.rtype = TYPE_EMPTY;
            return;
        }
        let mut all_radii_equal = true;
        let mut all_corners_square = self.radii[0].x == 0.0 || self.radii[0].y == 0.0;

        for i in 1..4 {
            if self.radii[i].x != 0.0 && self.radii[i].y != 0.0 {
                all_corners_square = false;
            }
            if self.radii[i].x != self.radii[i - 1].x || self.radii[i].y != self.radii[i - 1].y {
                all_radii_equal = false;
            }
        }

        if all_corners_square {
            self.rtype = TYPE_RECT;
            return;
        }

        if all_radii_equal {
            let hw = self.rect.half_width();
            let hh = self.rect.half_height();
            if self.radii[0].x >= hw && self.radii[0].y >= hh {
                self.rtype = TYPE_OVAL;
                for r in &mut self.radii {
                    r.set(hw, hh);
                }
            } else {
                self.rtype = TYPE_SIMPLE;
            }
            return;
        }

        self.rtype = if radii_are_nine_patch(&self.radii) {
            TYPE_NINE_PATCH
        } else {
            TYPE_COMPLEX
        };
    }

    pub fn check_corner_containment(&self, x: f32, y: f32) -> bool {
        let (canonical_x, canonical_y, index);
        if self.rtype == TYPE_OVAL {
            canonical_x = x - self.rect.center_x();
            canonical_y = y - self.rect.center_y();
            index = CORNER_UL;
        } else if x < self.rect.left + self.radii[CORNER_UL].x
            && y < self.rect.top + self.radii[CORNER_UL].y
        {
            index = CORNER_UL;
            canonical_x = x - (self.rect.left + self.radii[CORNER_UL].x);
            canonical_y = y - (self.rect.top + self.radii[CORNER_UL].y);
        } else if x < self.rect.left + self.radii[CORNER_LL].x
            && y > self.rect.bottom - self.radii[CORNER_LL].y
        {
            index = CORNER_LL;
            canonical_x = x - (self.rect.left + self.radii[CORNER_LL].x);
            canonical_y = y - (self.rect.bottom - self.radii[CORNER_LL].y);
        } else if x > self.rect.right - self.radii[CORNER_UR].x
            && y < self.rect.top + self.radii[CORNER_UR].y
        {
            index = CORNER_UR;
            canonical_x = x - (self.rect.right - self.radii[CORNER_UR].x);
            canonical_y = y - (self.rect.top + self.radii[CORNER_UR].y);
        } else if x > self.rect.right - self.radii[CORNER_LR].x
            && y > self.rect.bottom - self.radii[CORNER_LR].y
        {
            index = CORNER_LR;
            canonical_x = x - (self.rect.right - self.radii[CORNER_LR].x);
            canonical_y = y - (self.rect.bottom - self.radii[CORNER_LR].y);
        } else {
            return true;
        }

        let rx = self.radii[index].x;
        let ry = self.radii[index].y;
        let dist = (canonical_x * canonical_x) * (ry * ry) + (canonical_y * canonical_y) * (rx * rx);
        let prod = rx * ry;
        dist <= prod * prod
    }

    pub fn contains_point(&self, point: Point) -> bool {
        if !self.rect.contains_xy(point.x, point.y) {
            return false;
        }
        if self.rtype == TYPE_RECT {
            return true;
        }
        self.check_corner_containment(point.x, point.y)
    }

    pub fn contains_rect(&self, rect: Rect) -> bool {
        if !self.rect.contains_rect(rect) {
            return false;
        }
        if self.rtype == TYPE_RECT {
            return true;
        }
        self.check_corner_containment(rect.left, rect.top)
            && self.check_corner_containment(rect.right, rect.top)
            && self.check_corner_containment(rect.right, rect.bottom)
            && self.check_corner_containment(rect.left, rect.bottom)
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        let mut r = self.rect.make_inset(dx, dy);
        let mut degenerate = false;
        if r.right <= r.left {
            degenerate = true;
            let mid = float_midpoint(r.left, r.right);
            r.left = mid;
            r.right = mid;
        }
        if r.bottom <= r.top {
            degenerate = true;
            let mid = float_midpoint(r.top, r.bottom);
            r.top = mid;
            r.bottom = mid;
        }
        if degenerate {
            return Self {
                rect: r,
                radii: [Point::ZERO; 4],
                rtype: TYPE_EMPTY,
            };
        }
        if !r.is_finite() {
            return Self::make_empty();
        }
        let mut radii = self.radii;
        for rad in &mut radii {
            if rad.x != 0.0 {
                rad.x -= dx;
            }
            if rad.y != 0.0 {
                rad.y -= dy;
            }
        }
        let mut dst = Self::make_empty();
        dst.set_rect_radii(r, &radii);
        dst
    }

    pub fn offset(&mut self, dx: f32, dy: f32) {
        if !self.initialize_rect(self.rect.make_offset(dx, dy)) {
            return;
        }
        if self.rtype == TYPE_OVAL {
            let r = self.rect;
            self.set_oval(r);
            return;
        }
        if !Self::are_rect_and_radii_valid(self.rect, &self.radii) {
            let r = self.rect;
            let radii = self.radii;
            self.set_rect_radii(r, &radii);
        }
    }

    pub fn are_rect_and_radii_valid(rect: Rect, radii: &[Point; 4]) -> bool {
        if !rect.is_finite() || !rect.is_sorted() {
            return false;
        }
        are_radii_predicates_valid(radii[CORNER_UL].x, radii[CORNER_UR].x, rect.left, rect.right)
            && are_radii_predicates_valid(
                radii[CORNER_LL].x,
                radii[CORNER_LR].x,
                rect.left,
                rect.right,
            )
            && are_radii_predicates_valid(
                radii[CORNER_UL].y,
                radii[CORNER_LL].y,
                rect.top,
                rect.bottom,
            )
            && are_radii_predicates_valid(
                radii[CORNER_UR].y,
                radii[CORNER_LR].y,
                rect.top,
                rect.bottom,
            )
    }

    pub fn is_valid(&self) -> bool {
        if !Self::are_rect_and_radii_valid(self.rect, &self.radii) {
            return false;
        }
        let mut all_radii_zero = self.radii[0].x == 0.0 && self.radii[0].y == 0.0;
        let mut all_corners_square = self.radii[0].x == 0.0 || self.radii[0].y == 0.0;
        let mut all_radii_same = true;

        for i in 1..4 {
            if self.radii[i].x != 0.0 || self.radii[i].y != 0.0 {
                all_radii_zero = false;
            }
            if self.radii[i].x != self.radii[i - 1].x || self.radii[i].y != self.radii[i - 1].y {
                all_radii_same = false;
            }
            if self.radii[i].x != 0.0 && self.radii[i].y != 0.0 {
                all_corners_square = false;
            }
        }
        let patches_of_nine = radii_are_nine_patch(&self.radii);

        if self.rtype < 0 || self.rtype > LAST_TYPE {
            return false;
        }
        match self.rtype {
            TYPE_EMPTY => {
                self.rect.is_empty() && all_radii_zero && all_radii_same && all_corners_square
            }
            TYPE_RECT => {
                !self.rect.is_empty() && all_radii_zero && all_radii_same && all_corners_square
            }
            TYPE_OVAL => {
                if self.rect.is_empty() || all_radii_zero || !all_radii_same || all_corners_square {
                    return false;
                }
                let hw = self.rect.half_width();
                let hh = self.rect.half_height();
                self.radii.iter().all(|r| r.x == hw && r.y == hh)
            }
            TYPE_SIMPLE => {
                !self.rect.is_empty() && !all_radii_zero && all_radii_same && !all_corners_square
            }
            TYPE_NINE_PATCH => {
                !self.rect.is_empty()
                    && !all_radii_zero
                    && !all_radii_same
                    && !all_corners_square
                    && patches_of_nine
            }
            TYPE_COMPLEX => {
                !self.rect.is_empty()
                    && !all_radii_zero
                    && !all_radii_same
                    && !all_corners_square
                    && !patches_of_nine
            }
            _ => false,
        }
    }

    pub fn is_nearly_simple_circular(&self, tolerance: f32) -> bool {
        let r = self.radii[0].x;
        scalar_nearly_equal(r, self.radii[0].y, tolerance)
            && scalar_nearly_equal(r, self.radii[1].x, tolerance)
            && scalar_nearly_equal(r, self.radii[1].y, tolerance)
            && scalar_nearly_equal(r, self.radii[2].x, tolerance)
            && scalar_nearly_equal(r, self.radii[2].y, tolerance)
            && scalar_nearly_equal(r, self.radii[3].x, tolerance)
            && scalar_nearly_equal(r, self.radii[3].y, tolerance)
    }

    pub fn all_corners_circular(&self, tolerance: f32) -> bool {
        scalar_nearly_equal(self.radii[0].x, self.radii[0].y, tolerance)
            && scalar_nearly_equal(self.radii[1].x, self.radii[1].y, tolerance)
            && scalar_nearly_equal(self.radii[2].x, self.radii[2].y, tolerance)
            && scalar_nearly_equal(self.radii[3].x, self.radii[3].y, tolerance)
    }

    pub fn is_relatively_circular(rx: f32, ry: f32, tolerance: f32) -> bool {
        (rx <= tolerance && ry <= tolerance)
            || (rx - ry).abs() <= tolerance * cpp_max_f32(rx, ry)
    }

    pub fn all_corners_relatively_circular(&self, tolerance: f32) -> bool {
        Self::is_relatively_circular(self.radii[0].x, self.radii[0].y, tolerance)
            && Self::is_relatively_circular(self.radii[1].x, self.radii[1].y, tolerance)
            && Self::is_relatively_circular(self.radii[2].x, self.radii[2].y, tolerance)
            && Self::is_relatively_circular(self.radii[3].x, self.radii[3].y, tolerance)
    }

    pub fn inner_bounds(&self) -> Rect {
        if self.rtype == TYPE_EMPTY || self.rtype == TYPE_RECT {
            return self.rect;
        }
        let mut inner = self.rect;
        let tl = self.radii[CORNER_UL];
        let tr = self.radii[CORNER_UR];
        let bl = self.radii[CORNER_LL];
        let br = self.radii[CORNER_LR];

        let left_shift = cpp_max_f32(tl.x, bl.x);
        let top_shift = cpp_max_f32(tl.y, tr.y);
        let right_shift = cpp_max_f32(tr.x, br.x);
        let bottom_shift = cpp_max_f32(bl.y, br.y);

        let dw = left_shift + right_shift;
        let dh = top_shift + bottom_shift;

        let horiz_area = (inner.width() - dw) * inner.height();
        let vert_area = (inner.height() - dh) * inner.width();
        const SCALE: f32 = (1.0 - SK_SCALAR_ROOT_2_OVER_2) + 1e-5;
        let inner_area = (inner.width() - SCALE * dw) * (inner.height() - SCALE * dh);

        if horiz_area > vert_area && horiz_area > inner_area {
            inner.left += left_shift;
            inner.right -= right_shift;
        } else if vert_area > inner_area {
            inner.top += top_shift;
            inner.bottom -= bottom_shift;
        } else if inner_area > 0.0 {
            inner.left += SCALE * left_shift;
            inner.right -= SCALE * right_shift;
            inner.top += SCALE * top_shift;
            inner.bottom -= SCALE * bottom_shift;
        } else {
            return Rect::make_empty();
        }
        inner
    }

    pub fn conservative_intersect(a: &Self, b: &Self) -> Self {
        let get_corner = |r: Rect, corner: usize| -> Point {
            match corner {
                CORNER_UL => Point::new(r.left, r.top),
                CORNER_UR => Point::new(r.right, r.top),
                CORNER_LL => Point::new(r.left, r.bottom),
                _ => Point::new(r.right, r.bottom),
            }
        };
        let inside_corner = |corner: usize, pa: Point, pb: Point| -> bool {
            match corner {
                CORNER_UL => pa.x >= pb.x && pa.y >= pb.y,
                CORNER_UR => pa.x <= pb.x && pa.y >= pb.y,
                CORNER_LR => pa.x <= pb.x && pa.y <= pb.y,
                _ => pa.x >= pb.x && pa.y <= pb.y,
            }
        };

        let mut intersection = Self::make_empty();
        if !intersection.rect.intersect_pair(a.rect, b.rect) {
            return Self::make_empty();
        }

        for c in [CORNER_UL, CORNER_UR, CORNER_LR, CORNER_LL] {
            let test = get_corner(intersection.rect, c);
            let a_corner = get_corner(a.rect, c);
            let b_corner = get_corner(b.rect, c);

            let ok = if test == a_corner && test == b_corner {
                let ar = a.radii[c];
                let br = b.radii[c];
                if ar.x >= br.x && ar.y >= br.y {
                    intersection.radii[c] = ar;
                    true
                } else if br.x >= ar.x && br.y >= ar.y {
                    intersection.radii[c] = br;
                    true
                } else {
                    false
                }
            } else if test == a_corner {
                intersection.radii[c] = a.radii[c];
                if intersection.radii[c] == b.radii[c] {
                    inside_corner(c, a_corner, b_corner)
                } else {
                    b.check_corner_containment(a_corner.x, a_corner.y)
                }
            } else if test == b_corner {
                intersection.radii[c] = b.radii[c];
                if intersection.radii[c] == a.radii[c] {
                    inside_corner(c, b_corner, a_corner)
                } else {
                    a.check_corner_containment(b_corner.x, b_corner.y)
                }
            } else {
                intersection.radii[c] = Point::ZERO;
                a.check_corner_containment(test.x, test.y)
                    && b.check_corner_containment(test.x, test.y)
            };
            if !ok {
                return Self::make_empty();
            }
        }

        if !Self::are_rect_and_radii_valid(intersection.rect, &intersection.radii)
            || intersection.scale_radii()
        {
            return Self::make_empty();
        }
        intersection.compute_type();
        intersection
    }

    pub fn write_to_bytes(&self, dst: &mut [u8; SIZE_IN_MEMORY]) {
        let floats: [f32; 12] = [
            self.rect.left, self.rect.top, self.rect.right, self.rect.bottom,
            self.radii[0].x, self.radii[0].y,
            self.radii[1].x, self.radii[1].y,
            self.radii[2].x, self.radii[2].y,
            self.radii[3].x, self.radii[3].y,
        ];
        for (i, &f) in floats.iter().enumerate() {
            dst[i * 4..(i + 1) * 4].copy_from_slice(&f.to_ne_bytes());
        }
    }

    pub fn read_from_bytes(&mut self, src: &[u8]) -> usize {
        if src.len() < SIZE_IN_MEMORY {
            return 0;
        }
        let mut floats = [0.0_f32; 12];
        for i in 0..12 {
            let bytes: [u8; 4] = [
                src[i * 4],
                src[i * 4 + 1],
                src[i * 4 + 2],
                src[i * 4 + 3],
            ];
            floats[i] = f32::from_ne_bytes(bytes);
        }
        let rect = Rect::make_ltrb(floats[0], floats[1], floats[2], floats[3]);
        let radii = [
            Point::new(floats[4], floats[5]),
            Point::new(floats[6], floats[7]),
            Point::new(floats[8], floats[9]),
            Point::new(floats[10], floats[11]),
        ];
        self.set_rect_radii(rect, &radii);
        SIZE_IN_MEMORY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrect_types_and_containment() {
        let mut rr = RRect::make_empty();
        rr.set_rect_xy(Rect::make_ltrb(0.0, 0.0, 100.0, 100.0), 10.0, 10.0);
        assert_eq!(rr.rtype, TYPE_SIMPLE);
        assert!(rr.is_valid());
        assert!(rr.contains_point(Point::new(50.0, 50.0)));
        assert!(!rr.contains_point(Point::new(0.5, 0.5)));

        let mut buf = [0u8; SIZE_IN_MEMORY];
        rr.write_to_bytes(&mut buf);
        let mut rr2 = RRect::make_empty();
        assert_eq!(rr2.read_from_bytes(&buf), SIZE_IN_MEMORY);
        assert_eq!(rr, rr2);
    }
}
