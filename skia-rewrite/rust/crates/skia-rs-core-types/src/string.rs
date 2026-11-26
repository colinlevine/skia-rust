//! Safe Rust algorithms for `SkString` and `SkStringUtils`.

use crate::align4;
use skia_rs_math::utf;

pub const STR_APPEND_U32_MAX_SIZE: usize = 10;
pub const STR_APPEND_S32_MAX_SIZE: usize = 11;
pub const STR_APPEND_U64_MAX_SIZE: usize = 20;
pub const STR_APPEND_S64_MAX_SIZE: usize = 21;
pub const STR_APPEND_SCALAR_MAX_SIZE: usize = 15;

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Matches `SkStrEndsWith(const char string[], const char suffixStr[])`.
#[inline]
pub fn str_ends_with(s: &[u8], suffix: &[u8]) -> bool {
    s.ends_with(suffix)
}

/// Matches `SkStrEndsWith(const char string[], char suffixChar)`.
#[inline]
pub fn str_ends_with_char(s: &[u8], suffix_char: u8) -> bool {
    s.last().copied() == Some(suffix_char)
}

/// Matches `SkStrStartsWithOneOf(const char string[], const char prefixes[])`,
/// where `prefixes` is a sequence of NUL-terminated strings ended by an empty string (`\0\0`).
/// `prefixes_with_terminator` includes the entries up to (and including) the double NUL.
pub fn str_starts_with_one_of(s: &[u8], prefixes: &[u8]) -> i32 {
    let mut idx = 0i32;
    let mut pos = 0usize;
    while pos < prefixes.len() {
        let mut end = pos;
        while end < prefixes.len() && prefixes[end] != 0 {
            end += 1;
        }
        let prefix = &prefixes[pos..end];
        if s.starts_with(prefix) {
            return idx;
        }
        pos = end + 1;
        idx += 1;
        if pos >= prefixes.len() || prefixes[pos] == 0 {
            break;
        }
    }
    -1
}

/// Matches `SkStrAppendU32(char string[], uint32_t dec)`. Returns bytes written.
pub fn str_append_u32(mut dec: u32, out: &mut [u8]) -> usize {
    let mut buf = [0u8; STR_APPEND_U32_MAX_SIZE];
    let mut p = buf.len();
    loop {
        p -= 1;
        buf[p] = b'0' + (dec % 10) as u8;
        dec /= 10;
        if dec == 0 {
            break;
        }
    }
    let len = buf.len() - p;
    let n = len.min(out.len());
    out[..n].copy_from_slice(&buf[p..p + n]);
    n
}

/// Matches `SkStrAppendS32(char string[], int32_t dec)`. Returns bytes written.
pub fn str_append_s32(dec: i32, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if dec < 0 {
        out[0] = b'-';
        let udec = (dec as u32).wrapping_neg();
        1 + str_append_u32(udec, &mut out[1..])
    } else {
        str_append_u32(dec as u32, out)
    }
}

/// Matches `SkStrAppendU64(char string[], uint64_t dec, int minDigits)`. Returns bytes written.
pub fn str_append_u64(mut dec: u64, mut min_digits: i32, out: &mut [u8]) -> usize {
    let mut buf = [0u8; STR_APPEND_U64_MAX_SIZE];
    let mut p = buf.len();
    loop {
        p -= 1;
        buf[p] = b'0' + (dec % 10) as u8;
        dec /= 10;
        min_digits -= 1;
        if dec == 0 {
            break;
        }
    }
    while min_digits > 0 && p > 0 {
        p -= 1;
        buf[p] = b'0';
        min_digits -= 1;
    }
    let len = buf.len() - p;
    let n = len.min(out.len());
    out[..n].copy_from_slice(&buf[p..p + n]);
    n
}

/// Matches `SkStrAppendS64(char string[], int64_t dec, int minDigits)`. Returns bytes written.
pub fn str_append_s64(dec: i64, min_digits: i32, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if dec < 0 {
        out[0] = b'-';
        let udec = (dec as u64).wrapping_neg();
        1 + str_append_u64(udec, min_digits, &mut out[1..])
    } else {
        str_append_u64(dec as u64, min_digits, out)
    }
}

/// Matches `SkString::insertHex` formatting. Returns bytes written (1..=8).
pub fn format_hex_u32(mut hex: u32, min_digits: i32, out: &mut [u8; 8]) -> usize {
    let mut min_d = min_digits.clamp(0, 8);
    let mut buf = [0u8; 8];
    let mut p = 8usize;
    loop {
        p -= 1;
        buf[p] = HEX_UPPER[(hex & 0xF) as usize];
        hex >>= 4;
        min_d -= 1;
        if hex == 0 {
            break;
        }
    }
    while min_d > 0 && p > 0 {
        p -= 1;
        buf[p] = b'0';
        min_d -= 1;
    }
    let len = 8 - p;
    out[..len].copy_from_slice(&buf[p..8]);
    len
}

/// Formats a finite `f64` with C99 `%.{sig_digits}g` semantics into `Vec<u8>`.
pub fn format_c99_g(val: f64, sig_digits: usize) -> Vec<u8> {
    debug_assert!(sig_digits >= 1);
    let mut out = Vec::with_capacity(24);
    if val.is_sign_negative() {
        out.push(b'-');
    }
    let abs_val = val.abs();
    if abs_val == 0.0 {
        out.push(b'0');
        return out;
    }

    let sci = format!("{:.*e}", sig_digits - 1, abs_val);
    let (mantissa, exp_str) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp_str.parse().unwrap_or(0);

    let mut digits = Vec::with_capacity(sig_digits);
    for b in mantissa.bytes() {
        if b.is_ascii_digit() {
            digits.push(b);
        }
    }

    if exp < -4 || exp >= sig_digits as i32 {
        // Scientific notation: d[.ddd]e±dd
        out.push(digits[0]);
        let mut last = digits.len();
        while last > 1 && digits[last - 1] == b'0' {
            last -= 1;
        }
        if last > 1 {
            out.push(b'.');
            out.extend_from_slice(&digits[1..last]);
        }
        out.push(b'e');
        if exp >= 0 {
            out.push(b'+');
        } else {
            out.push(b'-');
        }
        let abs_exp = exp.unsigned_abs();
        if abs_exp < 10 {
            out.push(b'0');
            out.push(b'0' + abs_exp as u8);
        } else {
            let mut ebuf = [0u8; 10];
            let elen = str_append_u32(abs_exp, &mut ebuf);
            out.extend_from_slice(&ebuf[..elen]);
        }
    } else if exp < 0 {
        // Fixed notation: 0.000...digits (trailing zeros stripped)
        out.extend_from_slice(b"0.");
        let leading_zeros = (-exp - 1) as usize;
        for _ in 0..leading_zeros {
            out.push(b'0');
        }
        let mut last = digits.len();
        while last > 0 && digits[last - 1] == b'0' {
            last -= 1;
        }
        out.extend_from_slice(&digits[..last]);
    } else {
        // Fixed notation: integer part + optional trimmed fractional part
        let int_len = (exp as usize) + 1;
        if int_len <= digits.len() {
            out.extend_from_slice(&digits[..int_len]);
            let frac = &digits[int_len..];
            let mut last = frac.len();
            while last > 0 && frac[last - 1] == b'0' {
                last -= 1;
            }
            if last > 0 {
                out.push(b'.');
                out.extend_from_slice(&frac[..last]);
            }
        } else {
            out.extend_from_slice(&digits);
            for _ in 0..(int_len - digits.len()) {
                out.push(b'0');
            }
        }
    }

    out
}

/// Matches `SkStrAppendScalar(char string[], SkScalar value)`. Returns bytes written.
pub fn str_append_scalar(value: f32, out: &mut [u8]) -> usize {
    let bytes: Vec<u8> = if value.is_nan() {
        b"nan".to_vec()
    } else if value.is_infinite() {
        if value > 0.0 {
            b"inf".to_vec()
        } else {
            b"-inf".to_vec()
        }
    } else {
        format_c99_g(value as f64, 8)
    };
    let n = bytes.len().min(out.len());
    out[..n].copy_from_slice(&bytes[..n]);
    n
}

/// Formats a scalar for `SkAppendScalar(SkString*, SkScalar, SkScalarAsStringType)`.
/// `as_hex == true` -> `SkBits2Float(0x%08x)`
/// `as_hex == false` -> `%.9g` with `'f'` appended if it contains `'.'`.
pub fn format_append_scalar(value: f32, as_hex: bool) -> Vec<u8> {
    if as_hex {
        let bits = value.to_bits();
        let mut out = b"SkBits2Float(0x00000000)".to_vec();
        for i in 0..8 {
            let nibble = ((bits >> (28 - i * 4)) & 0xF) as usize;
            out[15 + i] = HEX_LOWER[nibble];
        }
        out
    } else {
        let mut tmp = if value.is_nan() {
            if value.is_sign_negative() {
                b"-nan".to_vec()
            } else {
                b"nan".to_vec()
            }
        } else if value.is_infinite() {
            if value > 0.0 {
                b"inf".to_vec()
            } else {
                b"-inf".to_vec()
            }
        } else {
            format_c99_g(value as f64, 9)
        };
        if tmp.contains(&b'.') {
            tmp.push(b'f');
        }
        tmp
    }
}

/// Matches `trim_size_t_to_u32` in `SkString.cpp`.
#[inline]
pub const fn trim_size_t_to_u32(value: usize) -> u32 {
    if value > (u32::MAX as usize) {
        u32::MAX
    } else {
        value as u32
    }
}

/// Matches `check_add32(size_t base, size_t extra)` in `SkString.cpp`.
#[inline]
pub const fn check_add32(base: usize, extra: usize) -> usize {
    let max_u32 = u32::MAX as usize;
    if base.saturating_add(extra) > max_u32 {
        max_u32.saturating_sub(base)
    } else {
        extra
    }
}

/// Matches `SkString::Rec::Make` allocation size computation with overflow checks.
/// Returns `Some((string_len_u32, allocation_size))` or `None` on overflow.
#[inline]
pub const fn string_rec_alloc_size(rec_header_size: usize, len: usize) -> Option<(u32, usize)> {
    if len > (u32::MAX as usize) {
        return None;
    }
    let Some(overhead) = rec_header_size.checked_add(1) else {
        return None;
    };
    let Some(raw) = len.checked_add(overhead) else {
        return None;
    };
    let Some(plus3) = raw.checked_add(3) else {
        return None;
    };
    let aligned = plus3 & !3usize;
    Some((len as u32, aligned))
}

/// Matches in-place reuse condition in `SkString::resize` and `SkString::set`:
/// `is_unique && ((new_len >> 2) <= (current_len >> 2))`.
#[inline]
pub const fn can_reuse_rec_for_resize(is_unique: bool, current_len: u32, new_len: u32) -> bool {
    is_unique && ((new_len >> 2) <= (current_len >> 2))
}

/// Matches in-place insert condition in `SkString::insert`:
/// `is_unique && (length >> 2) == ((length + insert_len) >> 2)`.
#[inline]
pub const fn can_insert_in_place(is_unique: bool, current_len: usize, insert_len: usize) -> bool {
    is_unique && (current_len >> 2) == ((current_len + insert_len) >> 2)
}

/// Computes clamped `(actual_length, tail_length)` for `SkString::remove(offset, length)`.
#[inline]
pub const fn clamp_remove_range(size: usize, offset: usize, length: usize) -> (usize, usize) {
    if offset >= size {
        return (0, 0);
    }
    let max_len = size - offset;
    let actual_len = if length > max_len { max_len } else { length };
    let tail = size - (offset + actual_len);
    (actual_len, tail)
}

/// Matches `SkTabString(const SkString& string, int tabCnt)`.
pub fn tab_string(input: &[u8], tab_cnt: i32) -> Vec<u8> {
    if tab_cnt <= 0 {
        return input.to_vec();
    }
    let tabs = tab_cnt as usize;
    let mut result = Vec::with_capacity(input.len() + tabs * 4);
    let mut pos = 0usize;
    while pos < input.len() {
        if let Some(rel_nl) = input[pos..].iter().position(|&b| b == b'\n') {
            if rel_nl > 0 {
                for _ in 0..tabs {
                    result.push(b'\t');
                }
            }
            result.extend_from_slice(&input[pos..pos + rel_nl + 1]);
            pos += rel_nl + 1;
        } else {
            for _ in 0..tabs {
                result.push(b'\t');
            }
            result.extend_from_slice(&input[pos..]);
            break;
        }
    }
    result
}

/// Matches `SkStringFromUTF16(const uint16_t* src, size_t count)`.
/// Returns `(utf8_bytes_len, src_units_used)` where `utf8_bytes_len <= u32::MAX`.
pub fn utf16_to_utf8_clamped_len(src: &[u16]) -> (usize, usize) {
    let mut n = 0usize;
    let mut pos = 0usize;
    while pos < src.len() {
        let last = pos;
        let (u, consumed) = utf::next_utf16(&src[pos..]);
        pos += consumed;
        let s = utf::to_utf8(u, None);
        if n > (u32::MAX as usize) - s {
            return (n, last);
        }
        n += s;
    }
    (n, src.len())
}

/// Converts `src[..units_used]` into UTF-8 bytes in `dst`, returning bytes written.
pub fn utf16_to_utf8_write(src: &[u16], dst: &mut [u8]) -> usize {
    let mut pos = 0usize;
    let mut out_pos = 0usize;
    while pos < src.len() {
        let (u, consumed) = utf::next_utf16(&src[pos..]);
        pos += consumed;
        let mut tmp = [0u8; 4];
        let s = utf::to_utf8(u, Some(&mut tmp));
        if out_pos + s <= dst.len() {
            dst[out_pos..out_pos + s].copy_from_slice(&tmp[..s]);
            out_pos += s;
        }
    }
    out_pos
}

/// Matches `SkStrSplit(str, delimiters, splitMode, out)`.
/// Returns `(start, len)` ranges into `input`.
pub fn str_split_ranges(input: &[u8], delimiters: &[u8], strict: bool) -> Vec<(usize, usize)> {
    let is_delim = |b: u8| delimiters.contains(&b);
    let mut pos = 0usize;
    if !strict {
        while pos < input.len() && is_delim(input[pos]) {
            pos += 1;
        }
    }
    if pos >= input.len() {
        return Vec::new();
    }

    let mut out = Vec::new();
    loop {
        let start = pos;
        while pos < input.len() && !is_delim(input[pos]) {
            pos += 1;
        }
        let len = pos - start;
        if strict || len > 0 {
            out.push((start, len));
        }

        if pos >= input.len() {
            break;
        }
        if !strict {
            while pos < input.len() && is_delim(input[pos]) {
                pos += 1;
            }
        } else {
            pos += 1;
        }
    }
    out
}

/// Helper to align a string size to 4 bytes (exposed for tests/consistency).
#[inline]
pub const fn aligned_rec_payload(len: usize) -> usize {
    align4(len + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ends_with_and_starts_with_one_of() {
        assert!(str_ends_with(b"hello", b"llo"));
        assert!(str_ends_with(b"hello", b""));
        assert!(!str_ends_with(b"hello", b"ll"));
        assert!(str_ends_with_char(b"hello", b'o'));
        assert!(!str_ends_with_char(b"", b'o'));

        let prefixes = b"foo\0bar\0baz\0\0";
        assert_eq!(str_starts_with_one_of(b"foobar", prefixes), 0);
        assert_eq!(str_starts_with_one_of(b"barbaz", prefixes), 1);
        assert_eq!(str_starts_with_one_of(b"baz", prefixes), 2);
        assert_eq!(str_starts_with_one_of(b"qux", prefixes), -1);
    }

    #[test]
    fn test_append_ints_and_hex() {
        let mut buf = [0u8; 32];
        let n = str_append_s32(i32::MIN, &mut buf);
        assert_eq!(&buf[..n], b"-2147483648");

        let n = str_append_s32(i32::MAX, &mut buf);
        assert_eq!(&buf[..n], b"2147483647");

        let n = str_append_u32(u32::MAX, &mut buf);
        assert_eq!(&buf[..n], b"4294967295");

        let n = str_append_s64(i64::MIN, 0, &mut buf);
        assert_eq!(&buf[..n], b"-9223372036854775808");

        let n = str_append_s64(-0x0100_0000, 15, &mut buf);
        assert_eq!(&buf[..n], b"-000000016777216");

        let n = str_append_u64(u64::MAX, 0, &mut buf);
        assert_eq!(&buf[..n], b"18446744073709551615");

        let mut hbuf = [0u8; 8];
        let n = format_hex_u32(0x1aB, 6, &mut hbuf);
        assert_eq!(&hbuf[..n], b"0001AB");
        let n = format_hex_u32(0, 0, &mut hbuf);
        assert_eq!(&hbuf[..n], b"0");
    }

    #[test]
    fn test_append_scalar_matches_skia_string_test() {
        let cases: &[(f32, &str)] = &[
            (0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.5, "0.5"),
            (f32::INFINITY, "inf"),
            (f32::NEG_INFINITY, "-inf"),
            (f32::NAN, "nan"),
            (-f32::NAN, "nan"),
            (3.4028234e38_f32, "3.4028235e+38"),
            (-3.4028234e38_f32, "-3.4028235e+38"),
            (1e-5_f32, "9.9999997e-06"),
            (1e-4_f32, "9.9999997e-05"),
            (1e-3_f32, "0.001"),
            (1e8_f32, "1e+08"),
            (1e7_f32, "10000000"),
        ];
        for &(val, expected) in cases {
            let mut buf = [0u8; STR_APPEND_SCALAR_MAX_SIZE];
            let n = str_append_scalar(val, &mut buf);
            assert_eq!(
                std::str::from_utf8(&buf[..n]).unwrap(),
                expected,
                "failed for {val:?}"
            );
        }

        assert_eq!(
            String::from_utf8(format_append_scalar(1.0, true)).unwrap(),
            "SkBits2Float(0x3f800000)"
        );
        assert_eq!(
            String::from_utf8(format_append_scalar(0.5, false)).unwrap(),
            "0.5f"
        );
        assert_eq!(
            String::from_utf8(format_append_scalar(2.0, false)).unwrap(),
            "2"
        );
    }

    #[test]
    fn test_tab_string_and_split() {
        assert_eq!(tab_string(b"a\n\nb\nc", 2), b"\t\ta\n\n\t\tb\n\t\tc");
        assert_eq!(tab_string(b"abc", 0), b"abc");

        let s = b"a-_b_c-dee--f-_-_-g-";
        let coalesced: Vec<&[u8]> = str_split_ranges(s, b"-_", false)
            .into_iter()
            .map(|(st, l)| &s[st..st + l])
            .collect();
        assert_eq!(coalesced, vec![&b"a"[..], b"b", b"c", b"dee", b"f", b"g"]);

        let strict: Vec<&[u8]> = str_split_ranges(s, b"-_", true)
            .into_iter()
            .map(|(st, l)| &s[st..st + l])
            .collect();
        assert_eq!(strict.len(), 13);
        assert_eq!(strict[0], b"a");
        assert_eq!(strict[1], b"");
        assert_eq!(strict[11], b"g");
        assert_eq!(strict[12], b"");

        let nl = b"\n";
        assert_eq!(str_split_ranges(nl, b"\n", false).len(), 0);
        assert_eq!(str_split_ranges(nl, b"\n", true), vec![(0, 0), (1, 0)]);
        assert_eq!(str_split_ranges(b",,", b",", true).len(), 3);
    }

    #[test]
    fn test_utf16_to_utf8() {
        let src: &[u16] = &[0x0041, 0xD83D, 0xDE00, 0x0042]; // "A😀B"
        let (len, used) = utf16_to_utf8_clamped_len(src);
        assert_eq!(len, 6);
        assert_eq!(used, 4);
        let mut out = vec![0u8; len];
        assert_eq!(utf16_to_utf8_write(&src[..used], &mut out), 6);
        assert_eq!(std::str::from_utf8(&out).unwrap(), "A😀B");
    }
}
