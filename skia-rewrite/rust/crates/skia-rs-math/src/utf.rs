//! UTF-8 / UTF-16 / UTF-32 conversion utilities matching Skia's `SkUTF`.

#[inline]
const fn utf16_is_high_surrogate(c: u16) -> bool {
    (c & 0xFC00) == 0xD800
}

#[inline]
const fn utf16_is_low_surrogate(c: u16) -> bool {
    (c & 0xFC00) == 0xDC00
}

#[inline]
fn utf8_byte_type(c: u8) -> i32 {
    if c < 0x80 {
        1
    } else if c < 0xC0 {
        0
    } else if c >= 0xF5 || (c & 0xFE) == 0xC0 {
        -1
    } else {
        (((0xe5u32 << 24) >> (((c as u32) >> 4) << 1)) & 3) as i32 + 1
    }
}

pub fn count_utf8(bytes: &[u8]) -> i32 {
    let mut i = 0usize;
    let mut count = 0i32;
    while i < bytes.len() {
        let ty = utf8_byte_type(bytes[i]);
        if ty <= 0 || i + (ty as usize) > bytes.len() {
            return -1;
        }
        let mut rem = ty;
        while rem > 1 {
            i += 1;
            if utf8_byte_type(bytes[i]) != 0 {
                return -1;
            }
            rem -= 1;
        }
        i += 1;
        count += 1;
    }
    count
}

pub fn count_utf16(units: &[u16]) -> i32 {
    let mut i = 0usize;
    let mut count = 0i32;
    while i < units.len() {
        let c = units[i];
        i += 1;
        if utf16_is_low_surrogate(c) {
            return -1;
        }
        if utf16_is_high_surrogate(c) {
            if i >= units.len() {
                return -1;
            }
            let low = units[i];
            i += 1;
            if !utf16_is_low_surrogate(low) {
                return -1;
            }
        }
        count += 1;
    }
    count
}

pub fn count_utf32(units: &[i32]) -> i32 {
    let Ok(n) = i32::try_from(units.len()) else {
        return -1;
    };
    const INVALID_MASK: u32 = 0xFF00_0000;
    for &u in units {
        if ((u as u32) & INVALID_MASK) != 0 {
            return -1;
        }
    }
    n
}

/// Returns `(codepoint_or_neg1, bytes_consumed)`. On error, `bytes_consumed == bytes.len()`.
pub fn next_utf8(bytes: &[u8]) -> (i32, usize) {
    if bytes.is_empty() {
        return (-1, 0);
    }
    let first = bytes[0];
    if utf8_byte_type(first) <= 0 {
        return (-1, bytes.len());
    }
    let mut c = first as i32;
    let mut hic = c.wrapping_shl(24);
    let mut idx = 0usize;
    if hic < 0 {
        let mut mask: u32 = !0x3Fu32;
        hic = ((hic as u32) << 1) as i32;
        loop {
            idx += 1;
            if idx >= bytes.len() {
                return (-1, bytes.len());
            }
            let next_byte = bytes[idx];
            if utf8_byte_type(next_byte) != 0 {
                return (-1, bytes.len());
            }
            c = (c << 6) | ((next_byte & 0x3F) as i32);
            mask <<= 5;
            hic = ((hic as u32) << 1) as i32;
            if hic >= 0 {
                break;
            }
        }
        c &= !(mask as i32);
    }
    (c, idx + 1)
}

pub fn next_utf8_with_replacement(bytes: &[u8]) -> (i32, usize) {
    let (val, consumed) = next_utf8(bytes);
    (if val < 0 { 0xFFFD } else { val }, consumed)
}

/// Returns `(codepoint_or_neg1, u16_units_consumed)`. On error, `consumed == units.len()`.
pub fn next_utf16(units: &[u16]) -> (i32, usize) {
    if units.is_empty() {
        return (-1, 0);
    }
    let c = units[0];
    if utf16_is_low_surrogate(c) {
        return (-1, units.len());
    }
    if utf16_is_high_surrogate(c) {
        if units.len() < 2 {
            return (-1, units.len());
        }
        let low = units[1];
        if !utf16_is_low_surrogate(low) {
            return (-1, units.len());
        }
        let result = ((c as i32) << 10) + (low as i32) - ((0xD800 << 10) + 0xDC00 - 0x10000);
        return (result, 2);
    }
    (c as i32, 1)
}

/// Returns `(codepoint_or_neg1, u32_units_consumed)`. On error, `consumed == units.len()`.
pub fn next_utf32(units: &[i32]) -> (i32, usize) {
    if units.is_empty() {
        return (-1, 0);
    }
    let value = units[0];
    if ((value as u32) & 0xFF00_0000) != 0 {
        return (-1, units.len());
    }
    (value, 1)
}

pub fn to_utf8(mut uni: i32, dst: Option<&mut [u8; 4]>) -> usize {
    if (uni as u32) > 0x10FFFF {
        return 0;
    }
    if uni <= 127 {
        if let Some(out) = dst {
            out[0] = uni as u8;
        }
        return 1;
    }
    let mut tmp = [0u8; 4];
    let mut p = 0usize;
    let mut count = 1usize;
    while uni > (0x7F >> count) {
        tmp[p] = (0x80 | (uni & 0x3F)) as u8;
        p += 1;
        uni >>= 6;
        count += 1;
    }
    if let Some(out) = dst {
        out[0] = (!(0xFFu32 >> count) as u8) | (uni as u8);
        for k in 0..(count - 1) {
            out[count - 1 - k] = tmp[k];
        }
    }
    count
}

pub fn to_utf16(uni: i32, dst: Option<&mut [u16; 2]>) -> usize {
    if (uni as u32) > 0x10FFFF {
        return 0;
    }
    let extra = uni > 0xFFFF;
    if let Some(out) = dst {
        if extra {
            out[0] = ((0xD800 - 64) + (uni >> 10)) as u16;
            out[1] = (0xDC00 | (uni & 0x3FF)) as u16;
        } else {
            out[0] = uni as u16;
        }
    }
    1 + usize::from(extra)
}

pub fn utf8_to_utf16(mut dst: Option<&mut [u16]>, src: &[u8]) -> i32 {
    let mut dst_len: i32 = 0;
    let mut dst_pos: usize = 0;
    let mut pos: usize = 0;
    while pos < src.len() {
        let (uni, consumed) = next_utf8(&src[pos..]);
        pos += consumed;
        if uni < 0 {
            return -1;
        }
        let mut buf = [0u16; 2];
        let count = to_utf16(uni, Some(&mut buf));
        if count == 0 {
            return -1;
        }
        dst_len += count as i32;
        if let Some(ref mut out) = dst {
            for &elem in &buf[..count] {
                if dst_pos < out.len() {
                    out[dst_pos] = elem;
                    dst_pos += 1;
                }
            }
        }
    }
    dst_len
}

pub fn utf16_to_utf8(mut dst: Option<&mut [u8]>, src: &[u16]) -> i32 {
    let mut dst_len: i32 = 0;
    let mut dst_pos: usize = 0;
    let mut pos: usize = 0;
    while pos < src.len() {
        let (uni, consumed) = next_utf16(&src[pos..]);
        pos += consumed;
        if uni < 0 {
            return -1;
        }
        let mut buf = [0u8; 4];
        let count = to_utf8(uni, Some(&mut buf));
        if count == 0 {
            return -1;
        }
        dst_len += count as i32;
        if let Some(ref mut out) = dst {
            for &elem in &buf[..count] {
                if dst_pos < out.len() {
                    out[dst_pos] = elem;
                    dst_pos += 1;
                }
            }
        }
    }
    dst_len
}
