//! Safe Rust buffered/seekable RAW stream state machine, overflow-checked offset arithmetic,
//! 100 MiB size-limited buffer, and preview `transferBuffer` planning.

use crate::types::{RAW_MAX_STREAM_SIZE, RAW_MIN_CHUNK_READ, RAW_READ_TO_END};

/// Exact port of `safe_add_to_size_t<uint64_t>` in `SkRawCodec.cpp`.
#[inline]
pub fn safe_add_to_usize(arg1: u64, arg2: u64) -> Option<usize> {
    let sum = arg1.checked_add(arg2)?;
    usize::try_from(sum).ok()
}

/// Exact port of `safe_add_to_size_t<size_t>` in `SkRawCodec.cpp`.
#[inline]
pub const fn safe_add_usize(arg1: usize, arg2: usize) -> Option<usize> {
    arg1.checked_add(arg2)
}

/// Exact port of `is_asset_stream(const SkStream&)`:
/// a stream is treated as seekable/asset iff it has both known length and position.
#[inline]
pub const fn is_asset_stream(has_length: bool, has_position: bool) -> bool {
    has_length && has_position
}

/// Validates a `dng_stream::DoRead(data, count, offset)` request.
/// Returns `Some((offset_usize, count_usize))` iff `count as u64 + offset` fits in `usize`.
#[inline]
pub fn validate_dng_read(count: u32, offset: u64) -> Option<(usize, usize)> {
    let _sum = safe_add_to_usize(count as u64, offset)?;
    let offset_usize = usize::try_from(offset).ok()?;
    let count_usize = usize::try_from(count).ok()?;
    Some((offset_usize, count_usize))
}

/// Validates a `SkRawAssetStream::read(data, offset, length)` range check.
/// Returns `Some(sum)` on valid range, `None` on overflow.
#[inline]
pub const fn validate_asset_read(offset: usize, length: usize) -> Option<usize> {
    if length == 0 {
        return Some(offset);
    }
    safe_add_usize(offset, length)
}

/// Exact port of `SkRawAssetStream::transferBuffer(offset, size)` byte-count calculation.
///
/// Allows reading fewer than `size` bytes (capped at `stream_length - offset`) because
/// `SkJpegCodec` handles partial JPEG streams. Returns `None` if `offset > stream_length`,
/// `offset + size` overflows, or `bytes_to_read == 0`.
#[inline]
pub fn plan_asset_transfer(stream_length: usize, offset: usize, size: usize) -> Option<usize> {
    if stream_length < offset {
        return None;
    }
    let sum = safe_add_usize(offset, size)?;
    let bytes_to_read = sum.min(stream_length) - offset;
    if bytes_to_read == 0 {
        return None;
    }
    Some(bytes_to_read)
}

/// Action returned by [`RawBufferedStream::plan_buffer_more`].
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BufferMoreActionKind {
    /// Requested bytes are already buffered (or whole stream was already read).
    AlreadySatisfied = 0,
    /// Cannot satisfy request (e.g. EOF already reached before `new_size`).
    Failed = 1,
    /// Read remaining underlying stream until EOF (`kReadToEnd`).
    ReadToEnd = 2,
    /// Read a chunk of `size_to_read` bytes (must obtain at least `size_requested`).
    ReadChunk = 3,
}

/// Step plan for `SkRawBufferedStream::bufferMoreData`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufferMorePlan {
    /// Discriminant of [`BufferMoreActionKind`].
    pub kind: i32,
    pub size_requested: usize,
    pub size_to_read: usize,
}

/// Strategy returned by [`RawBufferedStream::plan_transfer`].
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BufferedTransferKind {
    /// `offset > bytes_written`: skip `skip_length` bytes in underlying stream, then read `remaining`.
    SkipAndRead = 0,
    /// `offset <= bytes_written`: copy `already_buffered` bytes from buffer, then read `remaining`.
    CopyAndRead = 1,
}

/// Plan for `SkRawBufferedStream::transferBuffer`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufferedTransferPlan {
    /// Discriminant of [`BufferedTransferKind`].
    pub kind: i32,
    pub skip_length: usize,
    pub copy_offset: usize,
    pub already_buffered: usize,
    pub remaining: usize,
}

/// Safe Rust replacement for `SkRawLimitedDynamicMemoryWStream` + `SkRawBufferedStream` state.
///
/// Owns the growable byte buffer capped at `max_stream_size` (100 MiB by default) and drives
/// every read, read-to-end, and destructive `transferBuffer` boundary calculation.
#[derive(Debug)]
pub struct RawBufferedStream {
    buffer: Vec<u8>,
    max_stream_size: usize,
    whole_stream_read: bool,
}

impl Default for RawBufferedStream {
    fn default() -> Self {
        Self::new()
    }
}

impl RawBufferedStream {
    pub fn new() -> Self {
        Self::with_max_size(RAW_MAX_STREAM_SIZE)
    }

    pub fn with_max_size(max_stream_size: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_stream_size,
            whole_stream_read: false,
        }
    }

    #[inline]
    pub fn bytes_written(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn max_stream_size(&self) -> usize {
        self.max_stream_size
    }

    #[inline]
    pub fn is_whole_stream_read(&self) -> bool {
        self.whole_stream_read
    }

    #[inline]
    pub fn mark_whole_stream_read(&mut self) {
        self.whole_stream_read = true;
    }

    /// Appends `data` to the size-limited buffer. Returns `false` (leaving buffer unchanged)
    /// if `bytes_written + data.len()` overflows or exceeds `max_stream_size`.
    pub fn write(&mut self, data: &[u8]) -> bool {
        let Some(new_size) = safe_add_usize(self.buffer.len(), data.len()) else {
            return false;
        };
        if new_size > self.max_stream_size {
            return false;
        }
        self.buffer.extend_from_slice(data);
        true
    }

    /// Reads `dst.len()` bytes starting at `offset` from the already-buffered data.
    /// Returns `false` if `offset + dst.len()` overflows or exceeds `bytes_written()`.
    pub fn read_buffered(&self, offset: usize, dst: &mut [u8]) -> bool {
        if dst.is_empty() {
            return true;
        }
        let Some(end) = safe_add_usize(offset, dst.len()) else {
            return false;
        };
        if end > self.buffer.len() {
            return false;
        }
        dst.copy_from_slice(&self.buffer[offset..end]);
        true
    }

    /// Exact port of `SkRawBufferedStream::bufferMoreData(size_t newSize)` decision step.
    pub fn plan_buffer_more(&self, new_size: usize) -> BufferMorePlan {
        if new_size == RAW_READ_TO_END {
            if self.whole_stream_read {
                return BufferMorePlan {
                    kind: BufferMoreActionKind::AlreadySatisfied as i32,
                    size_requested: 0,
                    size_to_read: 0,
                };
            }
            return BufferMorePlan {
                kind: BufferMoreActionKind::ReadToEnd as i32,
                size_requested: 0,
                size_to_read: RAW_MIN_CHUNK_READ,
            };
        }

        if new_size <= self.buffer.len() {
            return BufferMorePlan {
                kind: BufferMoreActionKind::AlreadySatisfied as i32,
                size_requested: 0,
                size_to_read: 0,
            };
        }

        if self.whole_stream_read {
            return BufferMorePlan {
                kind: BufferMoreActionKind::Failed as i32,
                size_requested: 0,
                size_to_read: 0,
            };
        }

        let size_requested = new_size - self.buffer.len();
        let size_to_read = RAW_MIN_CHUNK_READ.max(size_requested);
        BufferMorePlan {
            kind: BufferMoreActionKind::ReadChunk as i32,
            size_requested,
            size_to_read,
        }
    }

    /// Commits bytes read by the host stream for a `ReadChunk` plan.
    /// Matches `SkRawBufferedStream::bufferMoreData`: fails without writing if
    /// `chunk.len() < size_requested`, otherwise enforces the size limit via [`Self::write`].
    pub fn commit_chunk(&mut self, chunk: &[u8], size_requested: usize) -> bool {
        if chunk.len() < size_requested {
            return false;
        }
        self.write(chunk)
    }

    /// Computes the destructive `transferBuffer(offset, size)` plan against currently buffered bytes.
    pub fn plan_transfer(&self, offset: usize, size: usize) -> BufferedTransferPlan {
        let written = self.buffer.len();
        if offset > written {
            BufferedTransferPlan {
                kind: BufferedTransferKind::SkipAndRead as i32,
                skip_length: offset - written,
                copy_offset: 0,
                already_buffered: 0,
                remaining: size,
            }
        } else {
            let already_buffered = (written - offset).min(size);
            let remaining = size - already_buffered;
            BufferedTransferPlan {
                kind: BufferedTransferKind::CopyAndRead as i32,
                skip_length: 0,
                copy_offset: offset,
                already_buffered,
                remaining,
            }
        }
    }

    /// Computes the final transferred byte count after reading `bytes_read` from the underlying
    /// stream for the `remaining` unbuffered tail.
    #[inline]
    pub const fn finalize_transfer_size(
        already_buffered: usize,
        remaining: usize,
        bytes_read: usize,
    ) -> Option<usize> {
        let tail = if bytes_read < remaining {
            bytes_read
        } else {
            remaining
        };
        safe_add_usize(already_buffered, tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_addition_and_dng_read_validation() {
        assert_eq!(safe_add_to_usize(100, 200), Some(300));
        assert_eq!(safe_add_to_usize(u64::MAX, 1), None);
        assert_eq!(safe_add_usize(usize::MAX, 1), None);
        assert_eq!(safe_add_usize(0, 0), Some(0));

        assert_eq!(validate_dng_read(512, 1024), Some((1024, 512)));
        assert_eq!(validate_dng_read(1, u64::MAX), None);

        assert_eq!(validate_asset_read(10, 0), Some(10));
        assert_eq!(validate_asset_read(usize::MAX, 0), Some(usize::MAX));
        assert_eq!(validate_asset_read(usize::MAX, 1), None);
        assert_eq!(validate_asset_read(10, 20), Some(30));

        assert!(is_asset_stream(true, true));
        assert!(!is_asset_stream(true, false));
        assert!(!is_asset_stream(false, true));
        assert!(!is_asset_stream(false, false));
    }

    #[test]
    fn asset_transfer_planning() {
        // Exact full slice.
        assert_eq!(plan_asset_transfer(1000, 200, 300), Some(300));
        // Partial tail allowed (for partial JPEG preview).
        assert_eq!(plan_asset_transfer(1000, 800, 500), Some(200));
        // Offset at or beyond end -> None.
        assert_eq!(plan_asset_transfer(1000, 1000, 10), None);
        assert_eq!(plan_asset_transfer(1000, 1001, 10), None);
        // Zero size -> None.
        assert_eq!(plan_asset_transfer(1000, 100, 0), None);
        // Overflow -> None.
        assert_eq!(plan_asset_transfer(1000, 100, usize::MAX), None);
    }

    #[test]
    fn buffered_stream_limit_and_reads() {
        let mut s = RawBufferedStream::with_max_size(100);
        assert_eq!(s.bytes_written(), 0);
        assert_eq!(s.max_stream_size(), 100);

        // Plan to read 16 bytes -> ReadChunk requesting 16, read size max(8192, 16) = 8192.
        let plan = s.plan_buffer_more(16);
        assert_eq!(plan.kind, BufferMoreActionKind::ReadChunk as i32);
        assert_eq!(plan.size_requested, 16);
        assert_eq!(plan.size_to_read, RAW_MIN_CHUNK_READ);

        // Short read (< size_requested) fails without mutating buffer.
        assert!(!s.commit_chunk(&[1u8; 10], 16));
        assert_eq!(s.bytes_written(), 0);

        // Sufficient chunk succeeds.
        let data: Vec<u8> = (0u8..64).collect();
        assert!(s.commit_chunk(&data, 16));
        assert_eq!(s.bytes_written(), 64);

        // Already satisfied for <= 64.
        let plan2 = s.plan_buffer_more(64);
        assert_eq!(plan2.kind, BufferMoreActionKind::AlreadySatisfied as i32);

        // Read slice from cache.
        let mut out = [0u8; 8];
        assert!(s.read_buffered(10, &mut out));
        assert_eq!(out, [10, 11, 12, 13, 14, 15, 16, 17]);
        assert!(!s.read_buffered(60, &mut out));

        // Exceeding 100-byte limit is rejected cleanly without modifying buffer.
        assert!(!s.write(&[0u8; 37]));
        assert_eq!(s.bytes_written(), 64);
        assert!(s.write(&[9u8; 36]));
        assert_eq!(s.bytes_written(), 100);
    }

    #[test]
    fn buffered_stream_read_to_end_and_eof_state() {
        let mut s = RawBufferedStream::new();
        let plan = s.plan_buffer_more(RAW_READ_TO_END);
        assert_eq!(plan.kind, BufferMoreActionKind::ReadToEnd as i32);

        assert!(s.write(b"hello world"));
        s.mark_whole_stream_read();
        assert!(s.is_whole_stream_read());

        // Second ReadToEnd is already satisfied.
        let plan = s.plan_buffer_more(RAW_READ_TO_END);
        assert_eq!(plan.kind, BufferMoreActionKind::AlreadySatisfied as i32);

        // Requesting beyond EOF fails immediately.
        let plan = s.plan_buffer_more(12);
        assert_eq!(plan.kind, BufferMoreActionKind::Failed as i32);

        // Requesting within buffered bytes still succeeds.
        let plan = s.plan_buffer_more(11);
        assert_eq!(plan.kind, BufferMoreActionKind::AlreadySatisfied as i32);
    }

    #[test]
    fn buffered_stream_transfer_plans() {
        let mut s = RawBufferedStream::new();
        assert!(s.write(&[1u8; 100]));

        // Case 1: completely inside buffered region.
        let p = s.plan_transfer(20, 50);
        assert_eq!(p.kind, BufferedTransferKind::CopyAndRead as i32);
        assert_eq!(p.copy_offset, 20);
        assert_eq!(p.already_buffered, 50);
        assert_eq!(p.remaining, 0);

        // Case 2: straddles buffered + unbuffered stream.
        let p = s.plan_transfer(80, 50);
        assert_eq!(p.kind, BufferedTransferKind::CopyAndRead as i32);
        assert_eq!(p.copy_offset, 80);
        assert_eq!(p.already_buffered, 20);
        assert_eq!(p.remaining, 30);
        assert_eq!(RawBufferedStream::finalize_transfer_size(20, 30, 30), Some(50));
        assert_eq!(RawBufferedStream::finalize_transfer_size(20, 30, 12), Some(32));

        // Case 3: strictly past buffered region -> SkipAndRead.
        let p = s.plan_transfer(150, 40);
        assert_eq!(p.kind, BufferedTransferKind::SkipAndRead as i32);
        assert_eq!(p.skip_length, 50);
        assert_eq!(p.already_buffered, 0);
        assert_eq!(p.remaining, 40);
    }
}
