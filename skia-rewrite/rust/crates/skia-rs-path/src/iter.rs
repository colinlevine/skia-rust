#![forbid(unsafe_code)]

use crate::types::{PathRaw, Verb};
use skia_rs_geometry::Point;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PathIterRec {
    pub points: [Point; 4],
    pub point_count: usize,
    pub conic_weight: f32,
    pub verb: Verb,
}

impl PathIterRec {
    #[inline]
    pub fn pts(&self) -> &[Point] {
        &self.points[..self.point_count]
    }
}

/// Safe Rust equivalent of `SkPathIter`.
#[derive(Clone, Debug)]
pub struct PathIter<'a> {
    p_index: usize,
    v_index: usize,
    c_index: usize,
    points: &'a [Point],
    verbs: &'a [Verb],
    conics: &'a [f32],
    close_storage: [Point; 2],
}

impl<'a> PathIter<'a> {
    pub fn new(points: &'a [Point], mut verbs: &'a [Verb], conics: &'a [f32]) -> Self {
        if verbs.last() == Some(&Verb::Move) {
            verbs = &verbs[..verbs.len() - 1];
        }
        Self {
            p_index: 0,
            v_index: 0,
            c_index: 0,
            points,
            verbs,
            conics,
            close_storage: [Point::ZERO; 2],
        }
    }

    pub fn peek_next_verb(&self) -> Option<Verb> {
        self.verbs.get(self.v_index).copied()
    }

    pub fn next_rec(&mut self) -> Option<PathIterRec> {
        if self.v_index >= self.verbs.len() {
            return None;
        }
        let v = self.verbs[self.v_index];
        self.v_index += 1;

        let mut w = -1.0_f32;
        let n = match v {
            Verb::Move => {
                self.close_storage[1] = self.points[self.p_index];
                self.p_index += 1;
                return Some(PathIterRec {
                    points: [
                        self.close_storage[1],
                        Point::ZERO,
                        Point::ZERO,
                        Point::ZERO,
                    ],
                    point_count: 1,
                    conic_weight: w,
                    verb: v,
                });
            }
            Verb::Line => 1,
            Verb::Quad => 2,
            Verb::Conic => {
                w = self.conics[self.c_index];
                self.c_index += 1;
                2
            }
            Verb::Cubic => 3,
            Verb::Close => {
                debug_assert!(self.p_index > 0);
                self.close_storage[0] = self.points[self.p_index - 1];
                return Some(PathIterRec {
                    points: [
                        self.close_storage[0],
                        self.close_storage[1],
                        Point::ZERO,
                        Point::ZERO,
                    ],
                    point_count: 2,
                    conic_weight: w,
                    verb: v,
                });
            }
        };

        debug_assert!(self.p_index > 0);
        let start = self.p_index - 1;
        self.p_index += n;
        let mut pts = [Point::ZERO; 4];
        pts[..n + 1].copy_from_slice(&self.points[start..start + n + 1]);
        Some(PathIterRec {
            points: pts,
            point_count: n + 1,
            conic_weight: w,
            verb: v,
        })
    }
}

/// Stateless step function for C++ `SkPathIter::next()` FFI bridge.
/// Returns `Some((verb, pts_start_or_sentinel, pts_len, conic_weight))`.
/// When `pts_start_or_sentinel == usize::MAX`, points live in `close_storage`:
/// - For `Move`: `&close_storage[1..2]`
/// - For `Close`: `&close_storage[0..2]`
pub fn path_iter_step(
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    p_index: &mut usize,
    v_index: &mut usize,
    c_index: &mut usize,
    close_storage: &mut [Point; 2],
) -> Option<(Verb, usize, usize, f32)> {
    if *v_index >= verbs.len() {
        return None;
    }
    let v = verbs[*v_index];
    *v_index += 1;
    let mut w = -1.0_f32;
    let n = match v {
        Verb::Move => {
            close_storage[1] = points[*p_index];
            *p_index += 1;
            return Some((v, usize::MAX, 1, w));
        }
        Verb::Line => 1,
        Verb::Quad => 2,
        Verb::Conic => {
            w = conics[*c_index];
            *c_index += 1;
            2
        }
        Verb::Cubic => 3,
        Verb::Close => {
            close_storage[0] = points[*p_index - 1];
            return Some((v, usize::MAX, 2, w));
        }
    };
    let start = *p_index - 1;
    *p_index += n;
    Some((v, start, n + 1, w))
}

/// O(1) raw-byte variant of `path_iter_step` for FFI (avoids O(N) full-buffer validation per step).
pub fn path_iter_step_u8(
    points: &[Point],
    verbs: &[u8],
    conics: &[f32],
    p_index: &mut usize,
    v_index: &mut usize,
    c_index: &mut usize,
    close_storage: &mut [Point; 2],
) -> Option<(Verb, usize, usize, f32)> {
    if *v_index >= verbs.len() {
        return None;
    }
    let v = Verb::from_u8(verbs[*v_index])?;
    *v_index += 1;
    let mut w = -1.0_f32;
    let n = match v {
        Verb::Move => {
            close_storage[1] = points[*p_index];
            *p_index += 1;
            return Some((v, usize::MAX, 1, w));
        }
        Verb::Line => 1,
        Verb::Quad => 2,
        Verb::Conic => {
            w = conics[*c_index];
            *c_index += 1;
            2
        }
        Verb::Cubic => 3,
        Verb::Close => {
            close_storage[0] = points[*p_index - 1];
            return Some((v, usize::MAX, 2, w));
        }
    };
    let start = *p_index - 1;
    *p_index += n;
    Some((v, start, n + 1, w))
}

#[derive(Copy, Clone, Debug)]
pub struct ContourRec<'a> {
    pub points: &'a [Point],
    pub verbs: &'a [Verb],
    pub conics: &'a [f32],
}

/// Safe Rust equivalent of `SkPathContourIter`.
#[derive(Clone, Debug)]
pub struct ContourIter<'a> {
    points: &'a [Point],
    verbs: &'a [Verb],
    conics: &'a [f32],
}

impl<'a> ContourIter<'a> {
    pub fn new(points: &'a [Point], verbs: &'a [Verb], conics: &'a [f32]) -> Self {
        Self {
            points,
            verbs,
            conics,
        }
    }

    pub fn next_contour(&mut self) -> Option<ContourRec<'a>> {
        let (npts, nvbs, nws) = next_contour_counts(self.verbs)?;
        let rec = ContourRec {
            points: &self.points[..npts],
            verbs: &self.verbs[..nvbs],
            conics: &self.conics[..nws],
        };
        self.points = &self.points[npts..];
        self.verbs = &self.verbs[nvbs..];
        self.conics = &self.conics[nws..];
        Some(rec)
    }
}

/// Computes `(npts, nvbs, nws)` for the first contour in `verbs`, matching `SkPathContourIter::next()`.
pub fn next_contour_counts(verbs: &[Verb]) -> Option<(usize, usize, usize)> {
    if verbs.is_empty() {
        return None;
    }
    debug_assert!(verbs[0] == Verb::Move);
    let mut npts = 1usize;
    let mut nvbs = 1usize;
    let mut nws = 0usize;

    for &v in &verbs[1..] {
        match v {
            Verb::Move => break,
            Verb::Line => npts += 1,
            Verb::Quad => npts += 2,
            Verb::Conic => {
                npts += 2;
                nws += 1;
            }
            Verb::Cubic => npts += 3,
            Verb::Close => {
                nvbs += 1;
                break;
            }
        }
        nvbs += 1;
    }
    Some((npts, nvbs, nws))
}

/// O(contour_len) raw-byte variant of `next_contour_counts` for FFI.
pub fn next_contour_counts_u8(verbs: &[u8]) -> Option<(usize, usize, usize)> {
    if verbs.is_empty() {
        return None;
    }
    let first = Verb::from_u8(verbs[0])?;
    debug_assert!(first == Verb::Move);
    let mut npts = 1usize;
    let mut nvbs = 1usize;
    let mut nws = 0usize;

    for &raw_v in &verbs[1..] {
        let v = Verb::from_u8(raw_v)?;
        match v {
            Verb::Move => break,
            Verb::Line => npts += 1,
            Verb::Quad => npts += 2,
            Verb::Conic => {
                npts += 2;
                nws += 1;
            }
            Verb::Cubic => npts += 3,
            Verb::Close => {
                nvbs += 1;
                break;
            }
        }
        nvbs += 1;
    }
    Some((npts, nvbs, nws))
}

/// Safe Rust equivalent of `SkPathPriv::Iterate` / `SkPath::RangeIter`.
#[derive(Clone, Debug)]
pub struct RangeIter<'a> {
    points: &'a [Point],
    verbs: &'a [Verb],
    conics: &'a [f32],
    p_index: usize,
    v_index: usize,
    c_index: usize,
}

impl<'a> RangeIter<'a> {
    pub fn new(points: &'a [Point], verbs: &'a [Verb], conics: &'a [f32]) -> Self {
        Self::new_at(points, verbs, conics, 0, 0, 0)
    }

    pub fn new_at(
        points: &'a [Point],
        verbs: &'a [Verb],
        conics: &'a [f32],
        v_index: usize,
        p_index: usize,
        c_index: usize,
    ) -> Self {
        Self {
            points,
            verbs,
            conics,
            p_index,
            v_index,
            c_index,
        }
    }

    pub fn position(&self) -> (usize, usize, usize) {
        (self.v_index, self.p_index, self.c_index)
    }

    pub fn peek(&self) -> Option<(Verb, &'a [Point], f32)> {
        let mut copy = self.clone();
        copy.next()
    }

    pub fn advance(&mut self) {
        let _ = self.next();
    }

    pub fn is_done(&self) -> bool {
        self.v_index >= self.verbs.len()
    }

    pub fn peek_verb(&self) -> Option<Verb> {
        self.verbs.get(self.v_index).copied()
    }
}

impl<'a> Iterator for RangeIter<'a> {
    type Item = (Verb, &'a [Point], f32);

    fn next(&mut self) -> Option<Self::Item> {
        if self.v_index >= self.verbs.len() {
            return None;
        }
        let verb = self.verbs[self.v_index];
        self.v_index += 1;
        let (pts, w) = match verb {
            Verb::Move => {
                let slice = &self.points[self.p_index..self.p_index + 1];
                self.p_index += 1;
                (slice, 1.0)
            }
            Verb::Line => {
                let slice = &self.points[self.p_index - 1..self.p_index + 1];
                self.p_index += 1;
                (slice, 1.0)
            }
            Verb::Quad => {
                let slice = &self.points[self.p_index - 1..self.p_index + 2];
                self.p_index += 2;
                (slice, 1.0)
            }
            Verb::Conic => {
                let slice = &self.points[self.p_index - 1..self.p_index + 2];
                let w = self.conics[self.c_index];
                self.p_index += 2;
                self.c_index += 1;
                (slice, w)
            }
            Verb::Cubic => {
                let slice = &self.points[self.p_index - 1..self.p_index + 3];
                self.p_index += 3;
                (slice, 1.0)
            }
            Verb::Close => (&self.points[self.p_index - 1..self.p_index], 1.0),
        };
        Some((verb, pts, w))
    }
}

/// Safe Rust equivalent of `SkPath::Iter` (with forceClose and autoClose line emission).
#[derive(Clone, Debug)]
pub struct LegacyPathIter<'a> {
    points: &'a [Point],
    verbs: &'a [Verb],
    conics: &'a [f32],
    p_index: usize,
    v_index: usize,
    c_index: usize, // points to the next unread conic (0 initially)
    last_conic_weight: f32,
    pub move_to: Point,
    pub last_pt: Point,
    pub force_close: bool,
    pub need_close: bool,
    pub close_line: bool,
}

impl<'a> LegacyPathIter<'a> {
    pub fn new(points: &'a [Point], verbs: &'a [Verb], conics: &'a [f32], force_close: bool) -> Self {
        Self {
            points,
            verbs,
            conics,
            p_index: 0,
            v_index: 0,
            c_index: 0,
            last_conic_weight: 1.0,
            move_to: Point::ZERO,
            last_pt: Point::ZERO,
            force_close,
            need_close: false,
            close_line: false,
        }
    }

    pub fn conic_weight(&self) -> f32 {
        self.last_conic_weight
    }

    pub fn is_close_line(&self) -> bool {
        self.close_line
    }

    pub fn is_closed_contour(&self) -> bool {
        legacy_iter_is_closed_contour(&self.verbs[self.v_index..], self.force_close)
    }

    fn auto_close(&mut self, pts: &mut [Point; 4]) -> Verb {
        if self.last_pt != self.move_to {
            if self.last_pt.x.is_nan()
                || self.last_pt.y.is_nan()
                || self.move_to.x.is_nan()
                || self.move_to.y.is_nan()
            {
                return Verb::Close;
            }
            pts[0] = self.last_pt;
            pts[1] = self.move_to;
            self.last_pt = self.move_to;
            self.close_line = true;
            return Verb::Line;
        }
        pts[0] = self.move_to;
        Verb::Close
    }

    pub fn next_verb(&mut self, pts: &mut [Point; 4]) -> Option<Verb> {
        if self.v_index >= self.verbs.len() {
            if self.need_close {
                if self.auto_close(pts) == Verb::Line {
                    return Some(Verb::Line);
                }
                self.need_close = false;
                return Some(Verb::Close);
            }
            return None;
        }

        let mut verb = self.verbs[self.v_index];
        self.v_index += 1;

        match verb {
            Verb::Move => {
                if self.need_close {
                    self.v_index -= 1;
                    verb = self.auto_close(pts);
                    if verb == Verb::Close {
                        self.need_close = false;
                    }
                    return Some(verb);
                }
                if self.v_index >= self.verbs.len() {
                    return None;
                }
                self.move_to = self.points[self.p_index];
                pts[0] = self.move_to;
                self.p_index += 1;
                self.last_pt = self.move_to;
                self.need_close = self.force_close;
            }
            Verb::Line => {
                pts[0] = self.last_pt;
                pts[1] = self.points[self.p_index];
                self.last_pt = self.points[self.p_index];
                self.close_line = false;
                self.p_index += 1;
            }
            Verb::Conic => {
                self.last_conic_weight = self.conics[self.c_index];
                self.c_index += 1;
                pts[0] = self.last_pt;
                pts[1] = self.points[self.p_index];
                pts[2] = self.points[self.p_index + 1];
                self.last_pt = self.points[self.p_index + 1];
                self.p_index += 2;
            }
            Verb::Quad => {
                pts[0] = self.last_pt;
                pts[1] = self.points[self.p_index];
                pts[2] = self.points[self.p_index + 1];
                self.last_pt = self.points[self.p_index + 1];
                self.p_index += 2;
            }
            Verb::Cubic => {
                pts[0] = self.last_pt;
                pts[1] = self.points[self.p_index];
                pts[2] = self.points[self.p_index + 1];
                pts[3] = self.points[self.p_index + 2];
                self.last_pt = self.points[self.p_index + 2];
                self.p_index += 3;
            }
            Verb::Close => {
                verb = self.auto_close(pts);
                if verb == Verb::Line {
                    self.v_index -= 1;
                } else {
                    self.need_close = false;
                }
                self.last_pt = self.move_to;
            }
        }
        Some(verb)
    }

    pub fn next_rec(&mut self) -> Option<PathIterRec> {
        let mut pts = [Point::ZERO; 4];
        let verb = self.next_verb(&mut pts)?;
        let point_count = verb.pts_in_iter();
        let conic_weight = if verb == Verb::Conic {
            self.last_conic_weight
        } else {
            1.0
        };
        Some(PathIterRec {
            points: pts,
            point_count,
            conic_weight,
            verb,
        })
    }
}

pub fn legacy_iter_is_closed_contour(remaining_verbs: &[Verb], force_close: bool) -> bool {
    if remaining_verbs.is_empty() {
        return false;
    }
    if force_close {
        return true;
    }
    let start = if remaining_verbs[0] == Verb::Move {
        1
    } else {
        0
    };
    for &v in &remaining_verbs[start..] {
        if v == Verb::Move {
            break;
        }
        if v == Verb::Close {
            return true;
        }
    }
    false
}

/// O(contour_len) raw-byte variant of `legacy_iter_is_closed_contour` for FFI.
pub fn legacy_iter_is_closed_contour_u8(remaining_verbs: &[u8], force_close: bool) -> bool {
    if remaining_verbs.is_empty() {
        return false;
    }
    if force_close {
        return true;
    }
    let Some(first) = Verb::from_u8(remaining_verbs[0]) else {
        return false;
    };
    let start = if first == Verb::Move { 1 } else { 0 };
    for &raw_v in &remaining_verbs[start..] {
        let Some(v) = Verb::from_u8(raw_v) else {
            return false;
        };
        if v == Verb::Move {
            break;
        }
        if v == Verb::Close {
            return true;
        }
    }
    false
}

/// Edge type matching `SkPathEdgeIter::Edge`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Edge {
    Line = 1,
    Quad = 2,
    Conic = 3,
    Cubic = 4,
}

#[derive(Copy, Clone, Debug)]
pub struct EdgeResult {
    pub points: [Point; 4],
    pub point_count: usize,
    pub edge: Edge,
    pub conic_weight: f32,
    pub is_new_contour: bool,
}

/// Safe Rust equivalent of `SkPathEdgeIter`.
#[derive(Clone, Debug)]
pub struct EdgeIter<'a> {
    points: &'a [Point],
    verbs: &'a [Verb],
    conics: &'a [f32],
    p_index: usize,
    v_index: usize,
    c_index: usize,
    move_to_index: usize,
    needs_close_line: bool,
    next_is_new_contour: bool,
}

impl<'a> EdgeIter<'a> {
    pub fn new(raw: &PathRaw<'a>) -> Self {
        Self::from_slices(raw.points, raw.verbs, raw.conics)
    }

    pub fn from_slices(points: &'a [Point], verbs: &'a [Verb], conics: &'a [f32]) -> Self {
        Self {
            points,
            verbs,
            conics,
            p_index: 0,
            v_index: 0,
            c_index: 0,
            move_to_index: 0,
            needs_close_line: false,
            next_is_new_contour: false,
        }
    }

    fn close_line(&mut self) -> EdgeResult {
        let p0 = self.points[self.p_index - 1];
        let p1 = self.points[self.move_to_index];
        self.needs_close_line = false;
        self.next_is_new_contour = true;
        EdgeResult {
            points: [p0, p1, Point::ZERO, Point::ZERO],
            point_count: 2,
            edge: Edge::Line,
            conic_weight: 1.0,
            is_new_contour: false,
        }
    }

    pub fn next_edge(&mut self) -> Option<EdgeResult> {
        loop {
            if self.v_index >= self.verbs.len() {
                if self.needs_close_line {
                    return Some(self.close_line());
                }
                return None;
            }
            let verb = self.verbs[self.v_index];
            self.v_index += 1;
            match verb {
                Verb::Move => {
                    if self.needs_close_line {
                        let res = self.close_line();
                        self.move_to_index = self.p_index;
                        self.p_index += 1;
                        return Some(res);
                    }
                    self.move_to_index = self.p_index;
                    self.p_index += 1;
                    self.next_is_new_contour = true;
                }
                Verb::Close => {
                    if self.needs_close_line {
                        return Some(self.close_line());
                    }
                }
                Verb::Line | Verb::Quad | Verb::Conic | Verb::Cubic => {
                    let pts_count = verb.pts_in_verb();
                    let w = if verb == Verb::Conic {
                        let w = self.conics[self.c_index];
                        self.c_index += 1;
                        w
                    } else {
                        1.0
                    };
                    self.needs_close_line = true;
                    let start = self.p_index - 1;
                    self.p_index += pts_count;
                    let mut pts = [Point::ZERO; 4];
                    pts[..pts_count + 1].copy_from_slice(&self.points[start..start + pts_count + 1]);
                    let is_new_contour = self.next_is_new_contour;
                    self.next_is_new_contour = false;
                    let edge = match verb {
                        Verb::Line => Edge::Line,
                        Verb::Quad => Edge::Quad,
                        Verb::Conic => Edge::Conic,
                        Verb::Cubic => Edge::Cubic,
                        _ => unreachable!(),
                    };
                    return Some(EdgeResult {
                        points: pts,
                        point_count: pts_count + 1,
                        edge,
                        conic_weight: w,
                        is_new_contour,
                    });
                }
            }
        }
    }
}
