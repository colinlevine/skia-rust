//! Safe Rust I/O buffer metadata and seek/compact arithmetic mirroring
//! `fill_buffer` and `seek_buffer` in `SkWuffsCodec.cpp`.

/// Mirrors `wuffs_base__io_buffer_meta`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IoBufferMeta {
    pub wi: usize,
    pub ri: usize,
    pub pos: u64,
    pub closed: bool,
}

impl IoBufferMeta {
    #[inline]
    pub const fn empty() -> Self {
        Self {
            wi: 0,
            ri: 0,
            pos: 0,
            closed: false,
        }
    }

    /// Exact port of `wuffs_base__io_buffer::reader_io_position()`.
    #[inline]
    pub const fn reader_io_position(self) -> u64 {
        self.pos.wrapping_add(self.ri as u64)
    }

    /// Exact port of `fill_buffer` post-read metadata update:
    /// advances `wi` by `num_read` and hard-codes `closed = false` so Wuffs yields
    /// recoverable `short_read` (`kIncompleteInput`) rather than fatal `kErrorInInput`.
    #[inline]
    pub fn on_bytes_read(&mut self, num_read: usize) -> bool {
        self.wi = self.wi.saturating_add(num_read);
        self.closed = false;
        num_read > 0
    }
}

/// Action kind produced by `plan_seek_buffer`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SeekBufferAction {
    /// Target position was inside the already-buffered window; `meta.ri` was updated in place.
    HitBuffer = 0,
    /// Caller must seek the backing `SkStream` to `stream_offset` and then call
    /// `apply_stream_seek_success`.
    NeedStreamSeek = 1,
    /// Target position exceeds `usize::MAX` and cannot be passed to `SkStream::seek(size_t)`.
    Overflow = 2,
}

/// Result of planning a seek on `IoBufferMeta`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SeekBufferPlan {
    pub action: i32,
    pub stream_offset: usize,
}

/// Exact port of `seek_buffer` in `SkWuffsCodec.cpp` (lines 116-132).
///
/// First attempts to satisfy the seek purely by adjusting `meta.ri` within the
/// buffered range `[meta.pos, meta.pos + meta.wi]`. Otherwise validates `pos <= usize::MAX`
/// and requests a backing stream seek.
#[inline]
pub fn plan_seek_buffer(meta: &mut IoBufferMeta, target_pos: u64) -> SeekBufferPlan {
    if target_pos >= meta.pos && (target_pos - meta.pos) <= meta.wi as u64 {
        meta.ri = (target_pos - meta.pos) as usize;
        return SeekBufferPlan {
            action: SeekBufferAction::HitBuffer as i32,
            stream_offset: 0,
        };
    }
    if target_pos > usize::MAX as u64 {
        return SeekBufferPlan {
            action: SeekBufferAction::Overflow as i32,
            stream_offset: 0,
        };
    }
    SeekBufferPlan {
        action: SeekBufferAction::NeedStreamSeek as i32,
        stream_offset: target_pos as usize,
    }
}

/// Updates `IoBufferMeta` after the backing stream successfully sought to `target_pos`.
#[inline]
pub fn apply_stream_seek_success(meta: &mut IoBufferMeta, target_pos: u64) {
    meta.wi = 0;
    meta.ri = 0;
    meta.pos = target_pos;
    meta.closed = false;
}

/// Plan for compacting unread bytes in an I/O buffer (exact mirror of `io_buffer::compact()`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CompactPlan {
    pub src_offset: usize,
    pub len: usize,
}

/// Computes the byte move required to compact `meta` and updates `meta` in place.
#[inline]
pub fn compact_io_buffer_meta(meta: &mut IoBufferMeta) -> CompactPlan {
    if meta.ri == 0 {
        return CompactPlan {
            src_offset: 0,
            len: 0,
        };
    }
    let src_offset = meta.ri;
    let len = meta.wi.saturating_sub(meta.ri);
    meta.pos = meta.pos.wrapping_add(meta.ri as u64);
    meta.wi = len;
    meta.ri = 0;
    CompactPlan { src_offset, len }
}
