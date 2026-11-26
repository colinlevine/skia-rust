#![forbid(unsafe_code)]

use crate::builder::PathBuilder;
use crate::iter::LegacyPathIter;
use crate::types::{ArcSize, Direction, FillType, PathDataBuf, Verb};
use skia_rs_geometry::geometry::Conic;
use skia_rs_geometry::Point;
use skia_rs_math::float_to_decimal::{float_to_decimal, MAX_FLOAT_TO_DECIMAL_LEN};

#[inline]
fn is_ws(b: u8) -> bool {
    (1..=32).contains(&b)
}

#[inline]
fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

#[inline]
fn is_sep(b: u8) -> bool {
    is_ws(b) || b == b','
}

#[inline]
fn is_lower(b: u8) -> bool {
    b.is_ascii_lowercase()
}

#[inline]
fn to_upper(b: u8) -> u8 {
    b - b'a' + b'A'
}

fn skip_ws(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if is_ws(b) {
            s = &s[1..];
        } else {
            break;
        }
    }
    s
}

fn skip_sep(mut s: &[u8]) -> &[u8] {
    while let Some(&b) = s.first() {
        if is_sep(b) {
            s = &s[1..];
        } else {
            break;
        }
    }
    s
}

/// Parses a single `f32` matching `(float)strtod(str, &stop)` after `skip_ws`.
/// Returns `Some((value, rest))` or `None` if no valid floating-point number starts at `s`.
fn parse_strtod_f32(s: &[u8]) -> Option<(f32, &[u8])> {
    let s = skip_ws(s);
    if s.is_empty() {
        return None;
    }

    let mut idx = 0usize;
    let sign_neg = match s[0] {
        b'+' => {
            idx = 1;
            false
        }
        b'-' => {
            idx = 1;
            true
        }
        _ => false,
    };

    if idx >= s.len() {
        return None;
    }

    // Check inf / infinity (case-insensitive)
    if s[idx..].len() >= 3 && s[idx..idx + 3].eq_ignore_ascii_case(b"inf") {
        idx += 3;
        if s[idx..].len() >= 5 && s[idx..idx + 5].eq_ignore_ascii_case(b"inity") {
            idx += 5;
        }
        let v = if sign_neg {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        };
        return Some((v, &s[idx..]));
    }

    // Check nan / nan(...) (case-insensitive)
    if s[idx..].len() >= 3 && s[idx..idx + 3].eq_ignore_ascii_case(b"nan") {
        idx += 3;
        if idx < s.len() && s[idx] == b'(' {
            let mut j = idx + 1;
            while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_') {
                j += 1;
            }
            if j < s.len() && s[j] == b')' {
                idx = j + 1;
            }
        }
        return Some((f32::NAN, &s[idx..]));
    }

    // Check hex float: 0x / 0X
    if s[idx..].len() >= 2 && s[idx] == b'0' && (s[idx + 1] == b'x' || s[idx + 1] == b'X') {
        if let Some((val, consumed)) = parse_hex_float(s, idx + 2, sign_neg) {
            return Some((val, &s[consumed..]));
        }
        // If `0x` is not followed by valid hex significand, C strtod parses just `0`!
        let v = if sign_neg { -0.0_f32 } else { 0.0_f32 };
        return Some((v, &s[idx + 1..]));
    }

    // Decimal float: (\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?
    let mut has_digits = false;
    while idx < s.len() && s[idx].is_ascii_digit() {
        has_digits = true;
        idx += 1;
    }
    if idx < s.len() && s[idx] == b'.' {
        idx += 1;
        while idx < s.len() && s[idx].is_ascii_digit() {
            has_digits = true;
            idx += 1;
        }
    }
    if !has_digits {
        return None;
    }

    // Optional exponent
    if idx < s.len() && (s[idx] == b'e' || s[idx] == b'E') {
        let mut e_idx = idx + 1;
        if e_idx < s.len() && (s[e_idx] == b'+' || s[e_idx] == b'-') {
            e_idx += 1;
        }
        let exp_digit_start = e_idx;
        while e_idx < s.len() && s[e_idx].is_ascii_digit() {
            e_idx += 1;
        }
        if e_idx > exp_digit_start {
            idx = e_idx;
        }
    }

    let token = core::str::from_utf8(&s[..idx]).ok()?;
    let v_f64: f64 = token.parse().ok()?;
    Some((v_f64 as f32, &s[idx..]))
}

fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        b'A'..=b'F' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

fn parse_hex_float(s: &[u8], start: usize, sign_neg: bool) -> Option<(f32, usize)> {
    let mut idx = start;
    let mut mantissa = 0.0_f64;
    let mut has_digits = false;
    while idx < s.len() {
        if let Some(d) = hex_val(s[idx]) {
            mantissa = mantissa * 16.0 + (d as f64);
            has_digits = true;
            idx += 1;
        } else {
            break;
        }
    }
    let mut frac_exp: i32 = 0;
    if idx < s.len() && s[idx] == b'.' {
        idx += 1;
        while idx < s.len() {
            if let Some(d) = hex_val(s[idx]) {
                mantissa = mantissa * 16.0 + (d as f64);
                frac_exp -= 4;
                has_digits = true;
                idx += 1;
            } else {
                break;
            }
        }
    }
    if !has_digits {
        return None;
    }
    let mut bin_exp: i32 = 0;
    if idx < s.len() && (s[idx] == b'p' || s[idx] == b'P') {
        let mut p_idx = idx + 1;
        let exp_neg = if p_idx < s.len() && s[p_idx] == b'-' {
            p_idx += 1;
            true
        } else if p_idx < s.len() && s[p_idx] == b'+' {
            p_idx += 1;
            false
        } else {
            false
        };
        let digit_start = p_idx;
        let mut exp_val: i32 = 0;
        while p_idx < s.len() && s[p_idx].is_ascii_digit() {
            exp_val = exp_val
                .saturating_mul(10)
                .saturating_add((s[p_idx] - b'0') as i32);
            p_idx += 1;
        }
        if p_idx > digit_start {
            idx = p_idx;
            bin_exp = if exp_neg { -exp_val } else { exp_val };
        }
    }
    let total_exp = bin_exp.saturating_add(frac_exp);
    let mut v = mantissa * (2.0_f64).powi(total_exp);
    if sign_neg {
        v = -v;
    }
    Some((v as f32, idx))
}

fn find_scalars<'a>(mut s: &'a [u8], out: &mut [f32]) -> Option<&'a [u8]> {
    let count = out.len();
    for (i, slot) in out.iter_mut().enumerate() {
        let (v, rest) = parse_strtod_f32(s)?;
        *slot = v;
        s = rest;
        if i + 1 < count {
            s = skip_sep(s);
        }
    }
    Some(s)
}

fn find_points<'a>(
    s: &'a [u8],
    out: &mut [Point],
    is_relative: bool,
    relative: Point,
) -> Option<&'a [u8]> {
    let mut coords = [0.0_f32; 6];
    let n = out.len();
    let rest = find_scalars(s, &mut coords[..n * 2])?;
    for i in 0..n {
        let mut p = Point::new(coords[i * 2], coords[i * 2 + 1]);
        if is_relative {
            p.x += relative.x;
            p.y += relative.y;
        }
        out[i] = p;
    }
    Some(rest)
}

fn find_scalar<'a>(
    s: &'a [u8],
    is_relative: bool,
    relative: f32,
) -> Option<(f32, &'a [u8])> {
    let (mut v, rest) = parse_strtod_f32(s)?;
    if is_relative {
        v += relative;
    }
    Some((v, skip_sep(rest)))
}

fn find_flag(s: &[u8]) -> Option<(bool, &[u8])> {
    let &first = s.first()?;
    if first != b'0' && first != b'1' {
        return None;
    }
    Some((first != b'0', skip_sep(&s[1..])))
}

/// Parses an SVG path data string into a `PathDataBuf`, matching `SkParsePath::FromSVGString`.
pub fn from_svg_string(data: &[u8]) -> Option<PathDataBuf> {
    let mut builder = PathBuilder::new(FillType::Winding);
    let mut first = Point::ZERO;
    let mut c = Point::ZERO;
    let mut lastc = Point::ZERO;
    let mut points = [Point::ZERO; 3];
    let mut op: u8 = 0;
    let mut previous_op: u8 = 0;
    let mut relative = false;

    let mut cur: Option<&[u8]> = Some(data);

    loop {
        let mut s = cur?;
        s = skip_ws(s);
        if s.is_empty() {
            break;
        }
        let ch = s[0];
        if is_digit(ch) || ch == b'-' || ch == b'+' || ch == b'.' {
            if op == 0 || op == b'Z' {
                return None;
            }
        } else if is_sep(ch) {
            s = skip_sep(s);
        } else {
            op = ch;
            relative = false;
            if is_lower(op) {
                op = to_upper(op);
                relative = true;
            }
            s = skip_sep(&s[1..]);
        }

        match op {
            b'M' => {
                cur = find_points(s, &mut points[..1], relative, c);
                builder.move_to(points[0]);
                previous_op = 0;
                op = b'L';
                c = points[0];
            }
            b'L' => {
                cur = find_points(s, &mut points[..1], relative, c);
                builder.line_to(points[0]);
                c = points[0];
            }
            b'H' => {
                if let Some((scratch, rest)) = find_scalar(s, relative, c.x) {
                    cur = Some(rest);
                    builder.line_to(Point::new(scratch, c.y));
                    c.x = scratch;
                } else {
                    cur = None;
                }
            }
            b'V' => {
                if let Some((scratch, rest)) = find_scalar(s, relative, c.y) {
                    cur = Some(rest);
                    builder.line_to(Point::new(c.x, scratch));
                    c.y = scratch;
                } else {
                    cur = None;
                }
            }
            b'C' => {
                cur = find_points(s, &mut points[..3], relative, c);
                builder.cubic_to(points[0], points[1], points[2]);
                lastc = points[1];
                c = points[2];
            }
            b'S' => {
                cur = find_points(s, &mut points[1..3], relative, c);
                points[0] = c;
                if previous_op == b'C' || previous_op == b'S' {
                    points[0].x -= lastc.x - c.x;
                    points[0].y -= lastc.y - c.y;
                }
                builder.cubic_to(points[0], points[1], points[2]);
                lastc = points[1];
                c = points[2];
            }
            b'Q' => {
                cur = find_points(s, &mut points[..2], relative, c);
                builder.quad_to(points[0], points[1]);
                lastc = points[0];
                c = points[1];
            }
            b'T' => {
                cur = find_points(s, &mut points[1..2], relative, c);
                points[0] = c;
                if previous_op == b'Q' || previous_op == b'T' {
                    points[0].x -= lastc.x - c.x;
                    points[0].y -= lastc.y - c.y;
                }
                builder.quad_to(points[0], points[1]);
                lastc = points[0];
                c = points[1];
            }
            b'A' => {
                let mut radii = [Point::ZERO; 1];
                let parsed = find_points(s, &mut radii, false, Point::ZERO)
                    .map(skip_sep)
                    .and_then(|r| find_scalar(r, false, 0.0))
                    .and_then(|(angle, r)| {
                        let r = skip_sep(r);
                        let (large_arc, r) = find_flag(r)?;
                        let r = skip_sep(r);
                        let (sweep, r) = find_flag(r)?;
                        let r = skip_sep(r);
                        let r = find_points(r, &mut points[..1], relative, c)?;
                        Some((angle, large_arc, sweep, r))
                    });
                if let Some((angle, large_arc, sweep, rest)) = parsed {
                    let arc_size = if large_arc {
                        ArcSize::Large
                    } else {
                        ArcSize::Small
                    };
                    let dir = if !sweep { Direction::Ccw } else { Direction::Cw };
                    builder.arc_to_svg(radii[0], angle, arc_size, dir, points[0]);
                    if let Some(&last) = builder.points().last() {
                        c = last;
                    }
                    cur = Some(rest);
                } else {
                    cur = None;
                }
            }
            b'Z' => {
                builder.close();
                c = first;
                cur = Some(s);
            }
            _ => return None,
        }

        if previous_op == 0 {
            first = c;
        }
        previous_op = op;
    }

    Some(builder.into_buf())
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathEncoding {
    Absolute = 0,
    Relative = 1,
}

fn write_scalar_as_text(out: &mut String, v: f32) {
    let mut buf = [0u8; MAX_FLOAT_TO_DECIMAL_LEN];
    let len = float_to_decimal(v, &mut buf) as usize;
    if let Ok(s) = core::str::from_utf8(&buf[..len]) {
        out.push_str(s);
    }
}

/// Converts a path to an SVG path data string, matching `SkParsePath::ToSVGString`.
pub fn to_svg_string(
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    encoding: PathEncoding,
) -> String {
    let mut out = String::new();
    let mut current_point = Point::ZERO;
    let rel_selector = encoding == PathEncoding::Relative;

    let mut append_command = |out: &mut String, mut cmd: u8, pts: &[Point]| {
        if rel_selector {
            cmd += 32;
        }
        out.push(cmd as char);
        for (i, &p) in pts.iter().enumerate() {
            let pt = p - current_point;
            if i > 0 {
                out.push(' ');
            }
            write_scalar_as_text(out, pt.x);
            out.push(' ');
            write_scalar_as_text(out, pt.y);
        }
        debug_assert!(!pts.is_empty());
        current_point = if rel_selector {
            pts[pts.len() - 1]
        } else {
            Point::ZERO
        };
    };

    let mut iter = LegacyPathIter::new(points, verbs, conics, false);
    while let Some(rec) = iter.next_rec() {
        let pts = &rec.points;
        match rec.verb {
            Verb::Conic => {
                const TOL: f32 = 1.0_f32 / 1024.0_f32;
                let conic = Conic {
                    pts: [pts[0], pts[1], pts[2]],
                    w: rec.conic_weight,
                };
                let pow2 = conic.compute_quad_pow2(TOL);
                let max_pts = 1 + 2 * (1usize << pow2.max(0));
                let mut quad_pts = vec![Point::ZERO; max_pts];
                let quad_count = conic.chop_into_quads_pow2(&mut quad_pts, pow2);
                for i in 0..(quad_count as usize) {
                    append_command(&mut out, b'Q', &quad_pts[i * 2 + 1..i * 2 + 3]);
                }
            }
            Verb::Move => append_command(&mut out, b'M', &pts[0..1]),
            Verb::Line => append_command(&mut out, b'L', &pts[1..2]),
            Verb::Quad => append_command(&mut out, b'Q', &pts[1..3]),
            Verb::Cubic => append_command(&mut out, b'C', &pts[1..4]),
            Verb::Close => out.push('Z'),
        }
    }

    out
}
