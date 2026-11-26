#![forbid(unsafe_code)]

use crate::builder::PathBuilder;
use crate::iter::PathIter;
use crate::types::{
    valid_path_data, Direction, FillType, PathDataBuf, PathRaw, Verb,
};
use skia_rs_geometry::rrect::SIZE_IN_MEMORY as RRECT_SIZE_IN_MEMORY;
use skia_rs_geometry::{Point, RRect};

// ============================================================================
// SkPath_serial.cpp
// ============================================================================

const K_TYPE_SERIALIZATION_SHIFT: u32 = 28;
const K_DIRECTION_SERIALIZATION_SHIFT: u32 = 26;
const K_FILL_TYPE_SERIALIZATION_SHIFT: u32 = 8;
const K_VERSION_SERIALIZATION_MASK: u32 = 0xFF;

const K_JUST_PUBLIC_DATA_VERSION: u32 = 4;
const K_VERBS_ARE_STORED_FORWARD_VERSION: u32 = 5;
const K_CURRENT_VERSION: u32 = K_VERBS_ARE_STORED_FORWARD_VERSION;

const K_SER_TYPE_GENERAL: u32 = 0;
const K_SER_TYPE_RRECT: u32 = 1;

#[inline]
fn align_up_4(size: usize) -> Option<usize> {
    size.checked_add(3).map(|v| v & !3usize)
}

#[derive(Copy, Clone, Debug)]
pub struct RRectSerialInfo {
    pub rrect: RRect,
    pub direction: Direction,
    pub start_index: u32,
}

/// Computes serialized size in bytes for an RRect/Oval fast-path, or `0` if `rrect_info` is `None`.
pub fn write_to_memory_as_rrect_size(rrect_info: Option<&RRectSerialInfo>) -> usize {
    if rrect_info.is_some() {
        4 + RRECT_SIZE_IN_MEMORY + 4
    } else {
        0
    }
}

/// Serializes an RRect/Oval path into `dst`. Returns bytes written (or size needed if `dst` is `None`).
pub fn write_to_memory_as_rrect(
    fill_type: FillType,
    rrect_info: Option<&RRectSerialInfo>,
    dst: Option<&mut [u8]>,
) -> usize {
    let Some(info) = rrect_info else {
        return 0;
    };
    let size_needed = 4 + RRECT_SIZE_IN_MEMORY + 4;
    let Some(buf) = dst else {
        return size_needed;
    };
    if buf.len() < size_needed {
        return 0;
    }

    let packed: i32 = (((fill_type as u32) << K_FILL_TYPE_SERIALIZATION_SHIFT)
        | ((info.direction as u32) << K_DIRECTION_SERIALIZATION_SHIFT)
        | (K_SER_TYPE_RRECT << K_TYPE_SERIALIZATION_SHIFT)
        | K_CURRENT_VERSION) as i32;

    buf[0..4].copy_from_slice(&packed.to_ne_bytes());
    let mut rr_bytes = [0u8; RRECT_SIZE_IN_MEMORY];
    info.rrect.write_to_bytes(&mut rr_bytes);
    buf[4..4 + RRECT_SIZE_IN_MEMORY].copy_from_slice(&rr_bytes);
    buf[4 + RRECT_SIZE_IN_MEMORY..size_needed]
        .copy_from_slice(&(info.start_index as i32).to_ne_bytes());
    size_needed
}

/// Computes serialized byte size for a general path (or RRect if `rrect_info` is `Some`).
pub fn write_to_memory_size(
    points_len: usize,
    verbs_len: usize,
    conics_len: usize,
    rrect_info: Option<&RRectSerialInfo>,
) -> usize {
    if rrect_info.is_some() {
        return 4 + RRECT_SIZE_IN_MEMORY + 4;
    }
    let pts_bytes = match points_len.checked_mul(8) {
        Some(v) => v,
        None => return 0,
    };
    let cnx_bytes = match conics_len.checked_mul(4) {
        Some(v) => v,
        None => return 0,
    };
    let mut size: usize = 16; // 4 * sizeof(int32_t)
    size = match size.checked_add(pts_bytes) {
        Some(v) => v,
        None => return 0,
    };
    size = match size.checked_add(cnx_bytes) {
        Some(v) => v,
        None => return 0,
    };
    size = match size.checked_add(verbs_len) {
        Some(v) => v,
        None => return 0,
    };
    align_up_4(size).unwrap_or(0)
}

/// Serializes path matching `SkPath::writeToMemory`.
/// If `dst` is `None`, returns the required byte size without writing.
pub fn write_to_memory(
    fill_type: FillType,
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    rrect_info: Option<&RRectSerialInfo>,
    dst: Option<&mut [u8]>,
) -> usize {
    if rrect_info.is_some() {
        return write_to_memory_as_rrect(fill_type, rrect_info, dst);
    }

    let size = write_to_memory_size(points.len(), verbs.len(), conics.len(), None);
    if size == 0 {
        return 0;
    }
    let Some(buf) = dst else {
        return size;
    };
    if buf.len() < size {
        return 0;
    }

    let packed: i32 = (((fill_type as u32) << K_FILL_TYPE_SERIALIZATION_SHIFT)
        | (K_SER_TYPE_GENERAL << K_TYPE_SERIALIZATION_SHIFT)
        | K_CURRENT_VERSION) as i32;

    let mut pos = 0usize;
    buf[pos..pos + 4].copy_from_slice(&packed.to_ne_bytes());
    pos += 4;
    buf[pos..pos + 4].copy_from_slice(&(points.len() as i32).to_ne_bytes());
    pos += 4;
    buf[pos..pos + 4].copy_from_slice(&(conics.len() as i32).to_ne_bytes());
    pos += 4;
    buf[pos..pos + 4].copy_from_slice(&(verbs.len() as i32).to_ne_bytes());
    pos += 4;

    for p in points {
        buf[pos..pos + 4].copy_from_slice(&p.x.to_ne_bytes());
        buf[pos + 4..pos + 8].copy_from_slice(&p.y.to_ne_bytes());
        pos += 8;
    }
    for &w in conics {
        buf[pos..pos + 4].copy_from_slice(&w.to_ne_bytes());
        pos += 4;
    }
    for &v in verbs {
        buf[pos] = v as u8;
        pos += 1;
    }
    while pos < size {
        buf[pos] = 0;
        pos += 1;
    }
    size
}

/// Deserialized path result from `read_from_memory`.
#[derive(Clone, Debug)]
pub enum ReadPathResult {
    RRect {
        rrect: RRect,
        direction: Direction,
        start_index: u32,
        fill_type: FillType,
        bytes_read: usize,
    },
    General {
        buf: PathDataBuf,
        bytes_read: usize,
    },
}

fn read_u32_ne(src: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > src.len() {
        return None;
    }
    let bytes = [src[*pos], src[*pos + 1], src[*pos + 2], src[*pos + 3]];
    *pos += 4;
    Some(u32::from_ne_bytes(bytes))
}

/// Deserializes a path matching `SkPath::ReadFromMemory`.
pub fn read_from_memory(src: &[u8]) -> Option<ReadPathResult> {
    let mut pos = 0usize;
    let packed = read_u32_ne(src, &mut pos)?;
    let version = packed & K_VERSION_SERIALIZATION_MASK;
    let verbs_are_forward = version == K_VERBS_ARE_STORED_FORWARD_VERSION;
    if !verbs_are_forward && version != K_JUST_PUBLIC_DATA_VERSION {
        return None;
    }

    let fill_type = FillType::from_u8(((packed >> K_FILL_TYPE_SERIALIZATION_SHIFT) & 0x3) as u8)?;
    let ser_type = (packed >> K_TYPE_SERIALIZATION_SHIFT) & 0xF;

    match ser_type {
        K_SER_TYPE_RRECT => {
            let dir_raw = ((packed >> K_DIRECTION_SERIALIZATION_SHIFT) & 0x3) as u8;
            let direction = Direction::from_u8(dir_raw)?;
            if pos + RRECT_SIZE_IN_MEMORY > src.len() {
                return None;
            }
            let mut rrect = RRect::make_empty();
            let read_rr = rrect.read_from_bytes(&src[pos..pos + RRECT_SIZE_IN_MEMORY]);
            if read_rr != RRECT_SIZE_IN_MEMORY {
                return None;
            }
            pos += RRECT_SIZE_IN_MEMORY;
            let start = read_u32_ne(src, &mut pos)? as i32;
            if !(0..=7).contains(&start) {
                return None;
            }
            pos = align_up_4(pos)?;
            if pos > src.len() {
                return None;
            }
            Some(ReadPathResult::RRect {
                rrect,
                direction,
                start_index: start as u32,
                fill_type,
                bytes_read: pos,
            })
        }
        K_SER_TYPE_GENERAL => {
            let pts_count = read_u32_ne(src, &mut pos)? as usize;
            let cnx_count = read_u32_ne(src, &mut pos)? as usize;
            let vbs_count = read_u32_ne(src, &mut pos)? as usize;

            let pts_bytes = pts_count.checked_mul(8)?;
            let cnx_bytes = cnx_count.checked_mul(4)?;
            let pts_start = pos;
            let cnx_start = pts_start.checked_add(pts_bytes)?;
            let vbs_start = cnx_start.checked_add(cnx_bytes)?;
            let vbs_end = vbs_start.checked_add(vbs_count)?;
            let aligned_end = align_up_4(vbs_end)?;
            if aligned_end > src.len() {
                return None;
            }

            if vbs_count == 0 {
                if pts_count == 0 && cnx_count == 0 {
                    return Some(ReadPathResult::General {
                        buf: PathDataBuf::new(fill_type),
                        bytes_read: aligned_end,
                    });
                }
                return None;
            }

            let mut points = Vec::with_capacity(pts_count);
            for i in 0..pts_count {
                let off = pts_start + i * 8;
                let x = f32::from_ne_bytes([src[off], src[off + 1], src[off + 2], src[off + 3]]);
                let y =
                    f32::from_ne_bytes([src[off + 4], src[off + 5], src[off + 6], src[off + 7]]);
                points.push(Point::new(x, y));
            }

            let mut conics = Vec::with_capacity(cnx_count);
            for i in 0..cnx_count {
                let off = cnx_start + i * 4;
                let w = f32::from_ne_bytes([src[off], src[off + 1], src[off + 2], src[off + 3]]);
                conics.push(w);
            }

            let mut verbs = Vec::with_capacity(vbs_count);
            if verbs_are_forward {
                for &b in &src[vbs_start..vbs_end] {
                    verbs.push(Verb::from_u8(b)?);
                }
            } else {
                for &b in src[vbs_start..vbs_end].iter().rev() {
                    verbs.push(Verb::from_u8(b)?);
                }
            }

            if !valid_path_data(&points, &verbs, &conics) {
                return None;
            }

            let mut builder = PathBuilder::new(fill_type);
            let raw = PathRaw {
                points: &points,
                verbs: &verbs,
                conics: &conics,
                bounds: skia_rs_geometry::Rect::EMPTY,
                fill_type,
                convexity: crate::types::Convexity::Unknown,
                segment_mask: crate::types::compute_segment_mask(&verbs),
            };
            builder.add_raw(&raw);
            Some(ReadPathResult::General {
                buf: builder.into_buf(),
                bytes_read: aligned_end,
            })
        }
        _ => None,
    }
}

// ============================================================================
// SkPathDump.cpp
// ============================================================================

const FILL_TYPE_STRS: [&str; 4] = [
    "Winding",
    "EvenOdd",
    "InverseWinding",
    "InverseEvenOdd",
];

/// Formats a scalar matching C `printf("%.9g", value)` followed by `'f'` if it contains `'.'`.
fn format_scalar_dec(value: f32) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    // Replicate C `%.9g` formatting on f64 promotion (since C printf promotes float to double).
    let d = value as f64;
    let abs_d = d.abs();
    let exp10 = abs_d.log10().floor() as i32;
    let mut s = if exp10 < -4 || exp10 >= 9 {
        // Scientific notation with 8 digits after decimal (9 significant digits), strip trailing zeros.
        let raw = format!("{:.8e}", d);
        if let Some((mantissa, exp_part)) = raw.split_once('e') {
            let trimmed_m = if mantissa.contains('.') {
                mantissa.trim_end_matches('0').trim_end_matches('.')
            } else {
                mantissa
            };
            let exp_val: i32 = exp_part.parse().unwrap_or(0);
            if exp_val >= 0 {
                format!("{}e+{:02}", trimmed_m, exp_val)
            } else {
                format!("{}e-{:02}", trimmed_m, -exp_val)
            }
        } else {
            raw
        }
    } else {
        // Fixed notation with `9 - 1 - exp10` decimal places, strip trailing zeros.
        let frac_digits = (8 - exp10).max(0) as usize;
        let raw = format!("{:.*}", frac_digits, d);
        if raw.contains('.') {
            raw.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            raw
        }
    };

    if s.contains('.') {
        s.push('f');
    }
    s
}

fn append_scalar(out: &mut String, value: f32, as_hex: bool) {
    if as_hex {
        use core::fmt::Write;
        let _ = write!(out, "SkBits2Float(0x{:08x})", value.to_bits());
    } else {
        out.push_str(&format_scalar_dec(value));
    }
}

fn append_params(
    out: &mut String,
    label: &str,
    pts: &[Point],
    as_hex: bool,
    use_semicolon: bool,
    conic_weight: Option<f32>,
) {
    out.push_str(label);
    out.push('(');

    let count = pts.len() * 2;
    for (i, p) in pts.iter().enumerate() {
        append_scalar(out, p.x, as_hex);
        out.push_str(", ");
        append_scalar(out, p.y, as_hex);
        if (i + 1) * 2 < count {
            out.push_str(", ");
        }
    }
    if let Some(w) = conic_weight {
        out.push_str(", ");
        append_scalar(out, w, as_hex);
    }
    out.push_str(if use_semicolon { ");" } else { ")" });

    if as_hex {
        out.push_str("  // ");
        for (i, p) in pts.iter().enumerate() {
            append_scalar(out, p.x, false);
            out.push_str(", ");
            append_scalar(out, p.y, false);
            if (i + 1) * 2 < count {
                out.push_str(", ");
            }
        }
        if let Some(w) = conic_weight {
            if w >= 0.0 {
                out.push_str(", ");
                append_scalar(out, w, false);
            }
        }
    }
    out.push('\n');
}

fn dump_iter(
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    out: &mut String,
    cmd_prefix: &str,
    as_hex: bool,
    use_semicolon: bool,
) {
    let mut iter = PathIter::new(points, verbs, conics);
    while let Some(rec) = iter.next_rec() {
        match rec.verb {
            Verb::Move => {
                let cmd = format!("{}.moveTo", cmd_prefix);
                append_params(out, &cmd, &rec.points[0..1], as_hex, use_semicolon, None);
            }
            Verb::Line => {
                let cmd = format!("{}.lineTo", cmd_prefix);
                append_params(out, &cmd, &rec.points[1..2], as_hex, use_semicolon, None);
            }
            Verb::Quad => {
                let cmd = format!("{}.quadTo", cmd_prefix);
                append_params(out, &cmd, &rec.points[1..3], as_hex, use_semicolon, None);
            }
            Verb::Conic => {
                let cmd = format!("{}.conicTo", cmd_prefix);
                append_params(
                    out,
                    &cmd,
                    &rec.points[1..3],
                    as_hex,
                    use_semicolon,
                    Some(rec.conic_weight),
                );
            }
            Verb::Cubic => {
                let cmd = format!("{}.cubicTo", cmd_prefix);
                append_params(out, &cmd, &rec.points[1..4], as_hex, use_semicolon, None);
            }
            Verb::Close => {
                out.push_str(cmd_prefix);
                out.push_str(if use_semicolon { ".close();\n" } else { ".close()\n" });
            }
        }
    }
}

/// Formats a path matching `SkPath::dump`.
pub fn dump_path_to_string(
    fill_type: FillType,
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    dump_as_hex: bool,
) -> String {
    let mut out = format!(
        "path.setFillType(SkPathFillType::k{});\n",
        FILL_TYPE_STRS[fill_type as usize]
    );
    dump_iter(points, verbs, conics, &mut out, "path", dump_as_hex, true);
    out
}

/// Formats a builder matching `SkPathBuilder::dumpToString`.
pub fn dump_builder_to_string(
    fill_type: FillType,
    points: &[Point],
    verbs: &[Verb],
    conics: &[f32],
    dump_as_hex: bool,
) -> String {
    let mut out = format!(
        "SkPathBuilder(SkPathFillType::k{})\n",
        FILL_TYPE_STRS[fill_type as usize]
    );
    dump_iter(points, verbs, conics, &mut out, "", dump_as_hex, false);
    out
}
