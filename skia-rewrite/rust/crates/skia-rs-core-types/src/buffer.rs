//! Safe Rust algorithms for `SkRBuffer`, `SkWBuffer`, `SkReadBuffer`,
//! `SkWriteBuffer`, and `SkWriter32`.

use crate::{align4, is_align4};
use skia_rs_math::safe_math;

// ---------------------------------------------------------------------------
// SkRBuffer / SkWBuffer helpers
// ---------------------------------------------------------------------------

/// Computes the state transition for `SkRBuffer::skip(size)`.
/// Returns `(new_valid, advance_bytes)`.
#[inline]
pub const fn rbuffer_skip(valid: bool, available: usize, size: usize) -> (bool, usize) {
    if valid && size <= available {
        (true, size)
    } else {
        (false, 0)
    }
}

/// Computes the state transition for `SkRBuffer::skipToAlign4()`.
/// `pos_addr` is the integer address of `fPos`.
/// Returns `(new_valid, advance_bytes)`.
#[inline]
pub const fn rbuffer_skip_to_align4(pos_addr: usize, valid: bool, available: usize) -> (bool, usize) {
    let n = align4(pos_addr).wrapping_sub(pos_addr);
    if valid && n <= available {
        (true, n)
    } else {
        (false, 0)
    }
}

/// Computes the padding byte count for `SkWBuffer::padToAlign4()`.
#[inline]
pub const fn wbuffer_pad_align4(pos: usize) -> usize {
    align4(pos).wrapping_sub(pos)
}

// ---------------------------------------------------------------------------
// SkReadBuffer helpers
// ---------------------------------------------------------------------------

/// Validates `(data_addr, size)` for `SkReadBuffer::setMemory`.
#[inline]
pub const fn read_buffer_validate_memory(addr: usize, size: usize) -> bool {
    is_align4(addr) && is_align4(size)
}

/// Computes aligned increment for `SkReadBuffer::skip(size)`.
/// Returns `Some(inc)` if `inc >= size` (no overflow), `curr_addr` is 4-byte aligned,
/// and `inc <= available`; otherwise `None`.
#[inline]
pub const fn read_buffer_skip(
    already_error: bool,
    curr_addr: usize,
    available: usize,
    size: usize,
) -> Option<usize> {
    if already_error {
        return None;
    }
    let inc = align4(size);
    if inc < size || !is_align4(curr_addr) || inc > available {
        return None;
    }
    Some(inc)
}

/// Computes aligned increment for `SkReadBuffer::skip(count, size)`.
#[inline]
pub fn read_buffer_skip_mul(
    already_error: bool,
    curr_addr: usize,
    available: usize,
    count: usize,
    size: usize,
) -> Option<usize> {
    let total = safe_math::mul(count, size);
    read_buffer_skip(already_error, curr_addr, available, total)
}

/// Validates a boolean read from `SkReadBuffer::readBool()`:
/// value must be 0 or 1. Returns `(valid, bool_value)`.
#[inline]
pub const fn read_buffer_validate_bool(value: u32) -> (bool, bool) {
    ((value & !1u32) == 0, value != 0)
}

/// Validates and clamps an integer for `SkReadBuffer::checkInt(min, max)`.
/// Returns `(valid, result_value)`.
#[inline]
pub const fn read_buffer_check_int(value: i32, min: i32, max: i32) -> (bool, i32) {
    if value < min || value > max {
        (false, min)
    } else {
        (true, value)
    }
}

/// Matches `sk_negate_to_size_t(int32_t value)` used by `SkReadBuffer::readTypeface`.
#[inline]
pub const fn negate_to_size_t(value: i32) -> usize {
    (value as i64).wrapping_neg() as usize
}

/// Encodes a 1-based flattenable dictionary index into a 32-bit word with
/// zero low byte sentinel (`index << 8`), matching `SkBinaryWriteBuffer::writeFlattenable`.
#[inline]
pub const fn encode_flattenable_dict_index(index: u32) -> u32 {
    debug_assert!((index >> 24) == 0);
    index << 8
}

/// Decodes a flattenable dictionary index from a 32-bit word (`word >> 8`),
/// matching `SkReadBuffer::readRawFlattenable`.
#[inline]
pub const fn decode_flattenable_dict_index(word: u32) -> u32 {
    word >> 8
}

// ---------------------------------------------------------------------------
// SkWriter32 helpers
// ---------------------------------------------------------------------------

/// Matches `SkWriter32::WriteStringSize`: `SkAlign4(4 + len + 1)`.
#[inline]
pub const fn writer32_string_size(len: usize) -> usize {
    align4(4 + len + 1)
}

/// Matches `SkWriter32::growToAtLeast`: `4096 + max(size, capacity + capacity / 2)`.
#[inline]
pub fn writer32_grow_capacity(current_capacity: usize, needed_size: usize) -> usize {
    let grown = current_capacity + (current_capacity / 2);
    4096 + needed_size.max(grown)
}

/// Formats `[u32 len][bytes...][\0][0..3 zero pad]` into `dst`.
/// `dst` must have length `>= writer32_string_size(src.len())`.
/// Returns total bytes written (multiple of 4).
pub fn writer32_format_string(src: &[u8], dst: &mut [u8]) -> usize {
    let total = writer32_string_size(src.len());
    debug_assert!(dst.len() >= total);
    let len_bytes = (src.len() as u32).to_ne_bytes();
    dst[0..4].copy_from_slice(&len_bytes);
    dst[4..4 + src.len()].copy_from_slice(src);
    // Zero-fill terminating NUL and alignment padding bytes.
    for b in &mut dst[4 + src.len()..total] {
        *b = 0;
    }
    total
}

/// Encodes `SkSamplingOptions` into up to 4 `u32` words for `SkWriter32::writeSampling`.
/// Returns the number of `u32` words written (1 or 4).
pub fn writer32_encode_sampling(
    max_aniso: i32,
    use_cubic: bool,
    cubic_b: f32,
    cubic_c: f32,
    filter: u32,
    mipmap: u32,
    out_words: &mut [u32; 4],
) -> usize {
    out_words[0] = max_aniso as u32;
    if max_aniso != 0 {
        return 1;
    }
    out_words[1] = u32::from(use_cubic);
    if use_cubic {
        out_words[2] = cubic_b.to_bits();
        out_words[3] = cubic_c.to_bits();
    } else {
        out_words[2] = filter;
        out_words[3] = mipmap;
    }
    4
}

/// Safe Rust in-memory `Writer32` implementation for pure-Rust use and testing.
#[derive(Debug, Default, Clone)]
pub struct Writer32 {
    buf: Vec<u8>,
}

impl Writer32 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bytes_written(&self) -> usize {
        self.buf.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn write32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_ne_bytes());
    }

    pub fn write_scalar(&mut self, value: f32) {
        self.write32(value.to_bits());
    }

    pub fn write_pad(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        let aligned = align4(data.len());
        self.buf.resize(self.buf.len() + (aligned - data.len()), 0);
    }

    pub fn write_string(&mut self, s: &[u8]) {
        let total = writer32_string_size(s.len());
        let start = self.buf.len();
        self.buf.resize(start + total, 0);
        writer32_format_string(s, &mut self.buf[start..start + total]);
    }
}

/// Safe Rust read buffer mirror for pure-Rust testing of binary read/write roundtrips.
#[derive(Debug)]
pub struct SafeReadBuffer<'a> {
    data: &'a [u8],
    pos: usize,
    error: bool,
}

impl<'a> SafeReadBuffer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let valid = is_align4(data.len());
        Self {
            data: if valid { data } else { &[] },
            pos: 0,
            error: !valid,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.error
    }

    pub fn available(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn skip(&mut self, size: usize) -> Option<&'a [u8]> {
        if let Some(inc) = read_buffer_skip(self.error, self.pos, self.available(), size) {
            let slice = &self.data[self.pos..self.pos + size];
            self.pos += inc;
            Some(slice)
        } else {
            self.pos = self.data.len();
            self.error = true;
            None
        }
    }

    pub fn read_u32(&mut self) -> u32 {
        if let Some(bytes) = self.skip(4) {
            u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            0
        }
    }

    pub fn read_bool(&mut self) -> bool {
        let v = self.read_u32();
        let (ok, b) = read_buffer_validate_bool(v);
        if !ok {
            self.pos = self.data.len();
            self.error = true;
        }
        b
    }

    pub fn read_string(&mut self) -> Option<&'a [u8]> {
        let len = self.read_u32() as usize;
        let len_plus_1 = len.checked_add(1)?;
        let bytes = self.skip(len_plus_1)?;
        if bytes[len] == 0 {
            Some(&bytes[..len])
        } else {
            self.pos = self.data.len();
            self.error = true;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rbuffer_and_wbuffer() {
        assert_eq!(rbuffer_skip(true, 10, 4), (true, 4));
        assert_eq!(rbuffer_skip(true, 10, 11), (false, 0));
        assert_eq!(rbuffer_skip(false, 10, 4), (false, 0));

        assert_eq!(rbuffer_skip_to_align4(0x1001, true, 10), (true, 3));
        assert_eq!(rbuffer_skip_to_align4(0x1004, true, 0), (true, 0));
        assert_eq!(rbuffer_skip_to_align4(0x1001, true, 2), (false, 0));

        assert_eq!(wbuffer_pad_align4(0), 0);
        assert_eq!(wbuffer_pad_align4(1), 3);
        assert_eq!(wbuffer_pad_align4(2), 2);
        assert_eq!(wbuffer_pad_align4(3), 1);
        assert_eq!(wbuffer_pad_align4(4), 0);
    }

    #[test]
    fn test_writer32_and_read_buffer_roundtrip() {
        let mut w = Writer32::new();
        w.write32(42);
        w.write32(1);
        w.write_string(b"hello");
        w.write_string(b"");
        w.write_string(b"abcd");

        let mut r = SafeReadBuffer::new(w.as_bytes());
        assert!(r.is_valid());
        assert_eq!(r.read_u32(), 42);
        assert!(r.read_bool());
        assert_eq!(r.read_string(), Some(&b"hello"[..]));
        assert_eq!(r.read_string(), Some(&b""[..]));
        assert_eq!(r.read_string(), Some(&b"abcd"[..]));
        assert!(r.is_valid());
        assert_eq!(r.available(), 0);
    }

    #[test]
    fn test_writer32_grow_and_sampling() {
        assert_eq!(writer32_grow_capacity(0, 100), 4196);
        assert_eq!(writer32_grow_capacity(4000, 100), 4096 + 6000);
        assert_eq!(writer32_string_size(0), 8);
        assert_eq!(writer32_string_size(3), 8);
        assert_eq!(writer32_string_size(4), 12);

        let mut words = [0u32; 4];
        assert_eq!(writer32_encode_sampling(4, false, 0.0, 0.0, 0, 0, &mut words), 1);
        assert_eq!(words[0], 4);
        assert_eq!(writer32_encode_sampling(0, true, 0.33, 0.33, 0, 0, &mut words), 4);
        assert_eq!(words[1], 1);
        assert_eq!(f32::from_bits(words[2]), 0.33);
        assert_eq!(writer32_encode_sampling(0, false, 0.0, 0.0, 1, 2, &mut words), 4);
        assert_eq!(words, [0, 0, 1, 2]);
    }

    #[test]
    fn test_flattenable_dict_index_and_negate() {
        assert_eq!(encode_flattenable_dict_index(7), 7 << 8);
        assert_eq!(decode_flattenable_dict_index(7 << 8), 7);
        assert_eq!(negate_to_size_t(-100), 100);
        assert_eq!(negate_to_size_t(i32::MIN), 2_147_483_648usize);
    }
}
