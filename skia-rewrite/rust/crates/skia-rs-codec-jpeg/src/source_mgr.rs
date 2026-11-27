//! Safe Rust port of `src/codec/SkJpegSourceMgr.cpp` and `src/codec/SkJpegUtility.cpp`
//! buffer management, skip planning, and unseekable subset extraction state machines.

/// Mirrors the concrete `SkJpegSourceMgr` subclass chosen by `SkJpegSourceMgr::Make`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SourceMgrKind {
    Unseekable = 0,
    Memory = 1,
    Buffered = 2,
}

/// Exact port of `SkJpegSourceMgr::Make` selection logic.
#[inline]
pub const fn select_source_mgr_kind(
    decode_gainmaps: bool,
    has_position: bool,
    has_length: bool,
    has_memory_base: bool,
) -> SourceMgrKind {
    if decode_gainmaps && !has_position {
        return SourceMgrKind::Unseekable;
    }
    if has_length && has_memory_base {
        SourceMgrKind::Memory
    } else {
        SourceMgrKind::Buffered
    }
}

/// Plan for skipping `bytes_to_skip` on a memory-mapped source (`SkJpegMemorySourceMgr`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MemorySkipPlan {
    pub ok: bool,
    pub advance: usize,
    pub new_bytes_in_buffer: usize,
}

#[inline]
pub const fn plan_memory_skip(bytes_to_skip: usize, bytes_in_buffer: usize) -> MemorySkipPlan {
    if bytes_to_skip > bytes_in_buffer {
        MemorySkipPlan {
            ok: false,
            advance: 0,
            new_bytes_in_buffer: 0,
        }
    } else {
        MemorySkipPlan {
            ok: true,
            advance: bytes_to_skip,
            new_bytes_in_buffer: bytes_in_buffer - bytes_to_skip,
        }
    }
}

/// Plan for skipping `bytes_to_skip` on a buffered or unseekable source.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BufferedSkipPlan {
    pub advance_in_buffer: usize,
    pub new_bytes_in_buffer: usize,
    pub stream_bytes_to_skip: usize,
    pub reset_buffer_ptr: bool,
}

#[inline]
pub const fn plan_buffered_skip(
    bytes_to_skip: usize,
    bytes_in_buffer: usize,
) -> BufferedSkipPlan {
    if bytes_to_skip <= bytes_in_buffer {
        BufferedSkipPlan {
            advance_in_buffer: bytes_to_skip,
            new_bytes_in_buffer: bytes_in_buffer - bytes_to_skip,
            stream_bytes_to_skip: 0,
            reset_buffer_ptr: false,
        }
    } else {
        BufferedSkipPlan {
            advance_in_buffer: 0,
            new_bytes_in_buffer: 0,
            stream_bytes_to_skip: bytes_to_skip - bytes_in_buffer,
            reset_buffer_ptr: true,
        }
    }
}

/// Bounds check for `SkJpegMemorySourceMgr::getSubsetData`.
#[inline]
pub const fn validate_memory_subset(stream_length: usize, offset: usize, size: usize) -> bool {
    offset <= stream_length && size <= stream_length - offset
}

/// Plan for `SkJpegUnseekableSourceMgr::getSubsetData`.
/// Encodes the exact buffer/skip/read transitions from `SkJpegSourceMgr.cpp` lines 324-385.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnseekableSubsetPlan {
    pub ok: bool,
    /// Number of bytes to skip in the underlying stream before reading subset data.
    pub stream_bytes_to_skip: usize,
    /// Byte offset inside `fBuffer` to copy from (when `copy_from_buffer_len > 0`).
    pub copy_from_buffer_offset: usize,
    /// Number of bytes to copy from `fBuffer` into the output buffer.
    pub copy_from_buffer_len: usize,
    /// Remaining bytes to read from `fStream` after any buffer copy / stream skip.
    pub stream_bytes_to_read: usize,
    /// Updated `fLastReadOffset` after consuming/discarding `fBuffer` (before stream skip/read).
    pub next_last_read_offset: usize,
    /// Updated `fLastReadSize` after consuming/discarding `fBuffer` (if all data was satisfied
    /// from `fBuffer`, keeps `last_read_size` unchanged, matching C++ early return!).
    pub next_last_read_size: usize,
}

pub fn plan_unseekable_subset(
    last_read_offset: usize,
    last_read_size: usize,
    req_offset: usize,
    req_size: usize,
) -> UnseekableSubsetPlan {
    if req_offset < last_read_offset {
        return UnseekableSubsetPlan {
            ok: false,
            stream_bytes_to_skip: 0,
            copy_from_buffer_offset: 0,
            copy_from_buffer_len: 0,
            stream_bytes_to_read: 0,
            next_last_read_offset: last_read_offset,
            next_last_read_size: last_read_size,
        };
    }

    let offset_into_buffer = req_offset - last_read_offset;
    if offset_into_buffer >= last_read_size {
        // Must skip past `fBuffer` to reach `req_offset`.
        let after_buf_offset = last_read_offset + last_read_size;
        let stream_bytes_to_skip = req_offset - after_buf_offset;
        UnseekableSubsetPlan {
            ok: true,
            stream_bytes_to_skip,
            copy_from_buffer_offset: 0,
            copy_from_buffer_len: 0,
            stream_bytes_to_read: req_size,
            next_last_read_offset: after_buf_offset,
            next_last_read_size: 0,
        }
    } else {
        // Part (or all) of the requested subset is already in `fBuffer`.
        let copy_len = (last_read_size - offset_into_buffer).min(req_size);
        let remaining = req_size - copy_len;
        if remaining == 0 {
            // Early return path: `fLastReadOffset` and `fLastReadSize` remain unchanged.
            UnseekableSubsetPlan {
                ok: true,
                stream_bytes_to_skip: 0,
                copy_from_buffer_offset: offset_into_buffer,
                copy_from_buffer_len: copy_len,
                stream_bytes_to_read: 0,
                next_last_read_offset: last_read_offset,
                next_last_read_size: last_read_size,
            }
        } else {
            UnseekableSubsetPlan {
                ok: true,
                stream_bytes_to_skip: 0,
                copy_from_buffer_offset: offset_into_buffer,
                copy_from_buffer_len: copy_len,
                stream_bytes_to_read: remaining,
                next_last_read_offset: last_read_offset + last_read_size,
                next_last_read_size: 0,
            }
        }
    }
}
