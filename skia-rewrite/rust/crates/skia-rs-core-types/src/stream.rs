//! Safe Rust algorithms for `SkStream`, `SkWStream`, `SkFILEStream`,
//! `SkMemoryStream`, `SkDynamicMemoryWStream`, and `SkStreamPriv`.

use crate::align4;
use skia_rs_math::safe_math;

pub const MAX_BYTE_FOR_U8: usize = 0xFD;
pub const BYTE_SENTINEL_FOR_U16: u8 = 0xFE;
pub const BYTE_SENTINEL_FOR_U32: u8 = 0xFF;

/// Minimum block size for `SkDynamicMemoryWStream`.
pub const DYNAMIC_MEMORY_WSTREAM_MIN_BLOCK_SIZE: usize = 4096;

/// Matches `SkWStream::SizeOfPackedUInt(size_t value)`.
#[inline]
pub const fn size_of_packed_uint(value: usize) -> i32 {
    if value <= MAX_BYTE_FOR_U8 {
        1
    } else if value <= 0xFFFF {
        3
    } else {
        5
    }
}

/// Encodes `value` into `out` (up to 5 bytes), returning the byte length (1, 3, or 5).
/// Matches `SkWStream::writePackedUInt` byte layout on native endianness.
pub fn encode_packed_uint(value: usize, out: &mut [u8; 5]) -> usize {
    if value <= MAX_BYTE_FOR_U8 {
        out[0] = value as u8;
        1
    } else if value <= 0xFFFF {
        let v16 = (value as u16).to_ne_bytes();
        out[0] = BYTE_SENTINEL_FOR_U16;
        out[1] = v16[0];
        out[2] = v16[1];
        3
    } else {
        let v32 = (value as u32).to_ne_bytes();
        out[0] = BYTE_SENTINEL_FOR_U32;
        out[1] = v32[0];
        out[2] = v32[1];
        out[3] = v32[2];
        out[4] = v32[3];
        5
    }
}

/// Decodes a packed unsigned integer from a byte slice, returning `(value, bytes_consumed)`.
pub fn decode_packed_uint(bytes: &[u8]) -> Option<(usize, usize)> {
    let first = *bytes.first()?;
    if first == BYTE_SENTINEL_FOR_U16 {
        if bytes.len() < 3 {
            return None;
        }
        let v = u16::from_ne_bytes([bytes[1], bytes[2]]);
        Some((v as usize, 3))
    } else if first == BYTE_SENTINEL_FOR_U32 {
        if bytes.len() < 5 {
            return None;
        }
        let v = u32::from_ne_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        Some((v as usize, 5))
    } else {
        Some((first as usize, 1))
    }
}

/// Clamps `(start, current)` for `SkFILEStream` constructor.
#[inline]
pub fn file_stream_clamp_init(end: usize, start: usize, current: usize) -> (usize, usize) {
    let clamped_start = start.min(end);
    let clamped_current = current.clamp(clamped_start, end);
    (clamped_start, clamped_current)
}

/// Computes new `fCurrent` for `SkFILEStream::seek(position)`.
#[inline]
pub fn file_stream_seek(start: usize, end: usize, position: usize) -> usize {
    safe_math::add(position, start).min(end)
}

/// Computes new `fCurrent` for `SkFILEStream::move(long offset)`.
/// Matches Skia's exact overflow-safe behavior including `i64::MIN`.
#[inline]
pub fn file_stream_move(start: usize, end: usize, current: usize, offset: i64) -> usize {
    if offset < 0 {
        let pos = current - start;
        if offset == i64::MIN {
            start
        } else {
            let neg = (-offset) as u64;
            if neg > (usize::MAX as u64) || (neg as usize) >= pos {
                start
            } else {
                current - (neg as usize)
            }
        }
    } else {
        let pos_off = offset as u64;
        if pos_off > (usize::MAX as u64) {
            end
        } else {
            safe_math::add(current, pos_off as usize).min(end)
        }
    }
}

/// Computes clamped read length for `SkFILEStream::read` or `SkMemoryStream::read`.
#[inline]
pub const fn clamp_stream_read(remaining: usize, requested: usize) -> usize {
    if requested > remaining {
        remaining
    } else {
        requested
    }
}

/// Computes new offset for `SkMemoryStream::seek(position)`.
#[inline]
pub const fn memory_stream_seek(data_size: usize, position: usize) -> usize {
    if position > data_size {
        data_size
    } else {
        position
    }
}

/// Computes new offset for `SkMemoryStream::move(long offset)`.
/// Matches C++ `this->seek(fOffset + offset)` where `offset` is converted to `size_t` (wrapping add).
#[inline]
pub const fn memory_stream_move(data_size: usize, current_offset: usize, delta: isize) -> usize {
    let target = current_offset.wrapping_add(delta as usize);
    memory_stream_seek(data_size, target)
}

/// Computes the payload size of a new `SkDynamicMemoryWStream::Block`.
/// Always a multiple of 4 (for `padToAlign4()`).
#[inline]
pub fn dynamic_block_payload_size(count: usize, block_header_size: usize) -> usize {
    let min_payload = DYNAMIC_MEMORY_WSTREAM_MIN_BLOCK_SIZE.saturating_sub(block_header_size);
    let size = count.max(min_payload);
    align4(size)
}

/// Computes how many zero padding bytes `SkDynamicMemoryWStream::padToAlign4` needs.
#[inline]
pub const fn dynamic_pad_align4_bytes(tail_written: usize) -> usize {
    tail_written.wrapping_neg() & 0x03
}

/// Matches `SkStreamPriv::RemainingLengthIsBelow`.
#[inline]
pub const fn remaining_length_is_below(
    has_length: bool,
    length: usize,
    has_position: bool,
    position: usize,
    requested: usize,
) -> bool {
    if has_length {
        if has_position {
            let remaining = length.saturating_sub(position);
            remaining < requested
        } else {
            length < requested
        }
    } else {
        false
    }
}

/// Safe Rust in-memory dynamic stream mirror (useful for testing and pure-Rust callers).
#[derive(Debug, Default, Clone)]
pub struct DynamicMemoryWStream {
    blocks: Vec<Vec<u8>>,
    bytes_before_tail: usize,
}

impl DynamicMemoryWStream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bytes_written(&self) -> usize {
        match self.blocks.last() {
            Some(tail) => self.bytes_before_tail + tail.len(),
            None => 0,
        }
    }

    pub fn write(&mut self, mut data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Some(tail) = self.blocks.last_mut() {
            let avail = tail.capacity() - tail.len();
            if avail > 0 {
                let n = avail.min(data.len());
                tail.extend_from_slice(&data[..n]);
                data = &data[n..];
                if data.is_empty() {
                    return;
                }
            }
            self.bytes_before_tail += tail.len();
        }
        let cap = dynamic_block_payload_size(data.len(), 24);
        let mut block = Vec::with_capacity(cap);
        block.extend_from_slice(data);
        self.blocks.push(block);
    }

    pub fn pad_to_align4(&mut self) {
        if let Some(tail) = self.blocks.last_mut() {
            let pad = dynamic_pad_align4_bytes(tail.len());
            for _ in 0..pad {
                tail.push(0);
            }
        }
    }

    pub fn read(&self, mut offset: usize, dst: &mut [u8]) -> bool {
        let mut count = dst.len();
        if offset.checked_add(count).map_or(true, |end| end > self.bytes_written()) {
            return false;
        }
        let mut dst_pos = 0;
        for block in &self.blocks {
            let size = block.len();
            if offset < size {
                let part = (size - offset).min(count);
                dst[dst_pos..dst_pos + part].copy_from_slice(&block[offset..offset + part]);
                if count <= part {
                    return true;
                }
                count -= part;
                dst_pos += part;
            }
            offset = offset.saturating_sub(size);
        }
        count == 0
    }

    pub fn copy_to(&self, dst: &mut [u8]) {
        let mut pos = 0;
        for block in &self.blocks {
            dst[pos..pos + block.len()].copy_from_slice(block);
            pos += block.len();
        }
    }

    pub fn reset(&mut self) {
        self.blocks.clear();
        self.bytes_before_tail = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packed_uint_roundtrip() {
        let cases = [0usize, 1, 0xFC, 0xFD, 0xFE, 0xFF, 0x100, 0xFFFF, 0x10000, 0xDEAD_BEEF];
        for &val in &cases {
            let mut buf = [0u8; 5];
            let len = encode_packed_uint(val, &mut buf);
            assert_eq!(len as i32, size_of_packed_uint(val));
            let (decoded, consumed) = decode_packed_uint(&buf[..len]).unwrap();
            assert_eq!(decoded, val);
            assert_eq!(consumed, len);
        }
        assert_eq!(decode_packed_uint(&[BYTE_SENTINEL_FOR_U16, 1]), None);
        assert_eq!(decode_packed_uint(&[BYTE_SENTINEL_FOR_U32, 1, 2, 3]), None);
    }

    #[test]
    fn test_file_stream_move_and_seek() {
        assert_eq!(file_stream_seek(10, 100, 20), 30);
        assert_eq!(file_stream_seek(10, 100, 200), 100);
        assert_eq!(file_stream_seek(10, 100, usize::MAX), 100);

        assert_eq!(file_stream_move(10, 100, 50, -20), 30);
        assert_eq!(file_stream_move(10, 100, 50, -50), 10);
        assert_eq!(file_stream_move(10, 100, 50, i64::MIN), 10);
        assert_eq!(file_stream_move(10, 100, 50, 30), 80);
        assert_eq!(file_stream_move(10, 100, 50, 200), 100);
        assert_eq!(file_stream_move(10, 100, 50, i64::MAX), 100);
    }

    #[test]
    fn test_memory_stream_move() {
        assert_eq!(memory_stream_move(100, 10, -20), 100); // wraps to huge > 100 -> clamps to 100
        assert_eq!(memory_stream_move(100, 50, -20), 30);
        assert_eq!(memory_stream_move(100, 50, 60), 100);
    }

    #[test]
    fn test_dynamic_stream() {
        let mut s = DynamicMemoryWStream::new();
        s.write(b"abc");
        assert_eq!(s.bytes_written(), 3);
        s.pad_to_align4();
        assert_eq!(s.bytes_written(), 4);
        let big = vec![0xABu8; 5000];
        s.write(&big);
        assert_eq!(s.bytes_written(), 5004);
        let mut out = [0u8; 6];
        assert!(s.read(2, &mut out));
        assert_eq!(&out, &[b'c', 0, 0xAB, 0xAB, 0xAB, 0xAB]);
        assert!(!s.read(5000, &mut out));
    }

    #[test]
    fn test_remaining_length() {
        assert!(remaining_length_is_below(true, 100, true, 90, 11));
        assert!(!remaining_length_is_below(true, 100, true, 90, 10));
        assert!(remaining_length_is_below(true, 10, false, 0, 11));
        assert!(!remaining_length_is_below(false, 0, false, 0, 100));
    }
}
