//! Safe Rust WebP RIFF container signature sniffing and header streaming state machine.
//!
//! Exact port of `SkWebpCodec::IsWebp` and `get_header_from_stream` from
//! `src/codec/SkWebpCodec.cpp`.

use crate::types::{ANIMATION_FLAG, EXIF_FLAG};

/// Maximum read chunk size used while scanning a WebP stream (`kBufferSize`).
pub const RIFF_BUFFER_SIZE: usize = 4096;

/// Size of the initial `"RIFF" + u32(file_size) + "WEBP"` container header.
pub const RIFF_INITIAL_HEADER_SIZE: usize = 12;

/// Size of a RIFF chunk header (`FourCC[4] + u32(payload_size)`).
pub const RIFF_CHUNK_HEADER_SIZE: usize = 8;

/// Minimum bytes needed from a `"VP8 "` payload to extract frame header info.
pub const VP8_UNCOMPRESSED_CHUNK_MIN_SIZE: usize = 3;

/// Maximum bytes read from a `"VP8 "` payload when scanning only the header.
pub const VP8_UNCOMPRESSED_CHUNK_MAX_SIZE: usize = 10;

/// Bytes read from a `"VP8L"` payload to extract lossless header info.
pub const VP8L_IMAGE_HEADER_SIZE: usize = 5;

/// Fixed payload size of a `"VP8X"` extended-header chunk.
pub const VP8X_PAYLOAD_SIZE: u32 = 10;

/// Exact port of `SkWebpCodec::IsWebp`.
///
/// WebP starts with `RIFFXXXXWEBPVP` (14 bytes), where `XXXX` is the file size.
#[inline]
pub fn is_webp(buf: &[u8]) -> bool {
    buf.len() >= 14 && &buf[0..4] == b"RIFF" && &buf[8..14] == b"WEBPVP"
}

/// Action requested by [`RiffHeaderReader`] on each iteration of the stream scan.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RiffStepKind {
    /// Caller should read up to `max_bytes` from the stream, write them to the
    /// output buffer if `bytes_read > 0`, and call `feed`.
    NeedRead = 0,
    /// Header scan succeeded; `only_header_parsed` tells whether trailing bytes
    /// remain unread in the stream.
    Done = 1,
    /// Input is invalid or truncated; caller should discard buffered bytes and
    /// report `SkCodec::kIncompleteInput`.
    Invalid = 2,
}

/// FFI-friendly step descriptor returned by [`RiffHeaderReader`].
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RiffReadStep {
    pub kind: i32,
    pub max_bytes: usize,
    pub only_header_parsed: bool,
}

impl RiffReadStep {
    #[inline]
    const fn need_read(max_bytes: usize) -> Self {
        Self {
            kind: RiffStepKind::NeedRead as i32,
            max_bytes,
            only_header_parsed: false,
        }
    }

    #[inline]
    const fn done(only_header_parsed: bool) -> Self {
        Self {
            kind: RiffStepKind::Done as i32,
            max_bytes: 0,
            only_header_parsed,
        }
    }

    #[inline]
    const fn invalid() -> Self {
        Self {
            kind: RiffStepKind::Invalid as i32,
            max_bytes: 0,
            only_header_parsed: false,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ReaderState {
    InitialHeader,
    ChunkHeader,
    Vp8Prefix,
    Vp8lPrefix,
    Vp8xPayload,
    SkipPayload {
        remaining: u32,
        stop_after_chunk: bool,
    },
    DrainToEnd,
    Finished {
        only_header_parsed: bool,
    },
    Failed,
}

/// Streaming RIFF/WebP header scanner matching `get_header_from_stream`.
///
/// The caller owns the `SkStream` / `SkDynamicMemoryWStream` I/O while this
/// safe-Rust state machine governs every byte count, FourCC validation,
/// VP8X flag decode, RIFF odd-byte padding, and stopping condition.
#[derive(Clone, Debug)]
pub struct RiffHeaderReader {
    state: ReaderState,
    is_animated: bool,
    first_chunk_read: bool,
}

impl Default for RiffHeaderReader {
    fn default() -> Self {
        Self::new()
    }
}

impl RiffHeaderReader {
    #[inline]
    pub const fn new() -> Self {
        Self {
            state: ReaderState::InitialHeader,
            is_animated: false,
            first_chunk_read: false,
        }
    }

    /// Queries the next action given whether the underlying stream is currently at EOF.
    pub fn next_step(&mut self, stream_at_end: bool) -> RiffReadStep {
        match self.state {
            ReaderState::InitialHeader => RiffReadStep::need_read(RIFF_INITIAL_HEADER_SIZE),
            ReaderState::ChunkHeader => {
                if stream_at_end {
                    self.state = ReaderState::Finished {
                        only_header_parsed: true,
                    };
                    RiffReadStep::done(true)
                } else {
                    RiffReadStep::need_read(RIFF_CHUNK_HEADER_SIZE)
                }
            }
            ReaderState::Vp8Prefix => RiffReadStep::need_read(VP8_UNCOMPRESSED_CHUNK_MAX_SIZE),
            ReaderState::Vp8lPrefix => RiffReadStep::need_read(VP8L_IMAGE_HEADER_SIZE),
            ReaderState::Vp8xPayload => RiffReadStep::need_read(VP8X_PAYLOAD_SIZE as usize),
            ReaderState::SkipPayload {
                remaining,
                stop_after_chunk,
            } => {
                if remaining == 0 {
                    if stop_after_chunk {
                        self.state = ReaderState::Finished {
                            only_header_parsed: true,
                        };
                        RiffReadStep::done(true)
                    } else {
                        self.state = ReaderState::ChunkHeader;
                        self.next_step(stream_at_end)
                    }
                } else {
                    RiffReadStep::need_read((remaining as usize).min(RIFF_BUFFER_SIZE))
                }
            }
            ReaderState::DrainToEnd => RiffReadStep::need_read(RIFF_BUFFER_SIZE),
            ReaderState::Finished { only_header_parsed } => RiffReadStep::done(only_header_parsed),
            ReaderState::Failed => RiffReadStep::invalid(),
        }
    }

    /// Feeds the bytes just read from the stream (and the post-read `stream_at_end` flag)
    /// into the state machine and returns the next step.
    pub fn feed(&mut self, bytes: &[u8], stream_at_end: bool) -> RiffReadStep {
        match self.state {
            ReaderState::InitialHeader => {
                if bytes.len() < RIFF_INITIAL_HEADER_SIZE
                    || &bytes[0..4] != b"RIFF"
                    || &bytes[8..12] != b"WEBP"
                {
                    self.state = ReaderState::Failed;
                    return RiffReadStep::invalid();
                }
                self.state = ReaderState::ChunkHeader;
                self.next_step(stream_at_end)
            }
            ReaderState::ChunkHeader => {
                if bytes.len() < RIFF_CHUNK_HEADER_SIZE {
                    self.state = ReaderState::Failed;
                    return RiffReadStep::invalid();
                }
                let fourcc = &bytes[0..4];
                let payload_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

                if fourcc == b"VP8 " {
                    self.state = ReaderState::Vp8Prefix;
                    RiffReadStep::need_read(VP8_UNCOMPRESSED_CHUNK_MAX_SIZE)
                } else if fourcc == b"VP8L" {
                    self.state = ReaderState::Vp8lPrefix;
                    RiffReadStep::need_read(VP8L_IMAGE_HEADER_SIZE)
                } else if fourcc == b"VP8X" {
                    if self.first_chunk_read || payload_size != VP8X_PAYLOAD_SIZE {
                        self.state = ReaderState::Failed;
                        return RiffReadStep::invalid();
                    }
                    self.first_chunk_read = true;
                    self.state = ReaderState::Vp8xPayload;
                    RiffReadStep::need_read(VP8X_PAYLOAD_SIZE as usize)
                } else {
                    if !self.first_chunk_read {
                        // Only VP8, VP8L, or VP8X chunks may appear first.
                        self.state = ReaderState::Failed;
                        return RiffReadStep::invalid();
                    }
                    let is_anmf = fourcc == b"ANMF";
                    // RIFF pads odd payload lengths with a single trailing zero byte.
                    let padded_size = payload_size.wrapping_add(payload_size & 1);
                    let stop_after_chunk = is_anmf && self.is_animated;
                    self.state = ReaderState::SkipPayload {
                        remaining: padded_size,
                        stop_after_chunk,
                    };
                    self.next_step(stream_at_end)
                }
            }
            ReaderState::Vp8Prefix => {
                if bytes.len() < VP8_UNCOMPRESSED_CHUNK_MIN_SIZE {
                    self.state = ReaderState::Failed;
                    RiffReadStep::invalid()
                } else {
                    self.state = ReaderState::Finished {
                        only_header_parsed: true,
                    };
                    RiffReadStep::done(true)
                }
            }
            ReaderState::Vp8lPrefix => {
                if bytes.len() < VP8L_IMAGE_HEADER_SIZE {
                    self.state = ReaderState::Failed;
                    RiffReadStep::invalid()
                } else {
                    self.state = ReaderState::Finished {
                        only_header_parsed: true,
                    };
                    RiffReadStep::done(true)
                }
            }
            ReaderState::Vp8xPayload => {
                if bytes.len() < VP8X_PAYLOAD_SIZE as usize {
                    self.state = ReaderState::Failed;
                    return RiffReadStep::invalid();
                }
                let flags = bytes[0] as u32;
                let has_exif = (flags & EXIF_FLAG) != 0;
                self.is_animated = (flags & ANIMATION_FLAG) != 0;
                if has_exif {
                    // EXIF sits at the end of the file; mirror the C++ `do { read } while (!isAtEnd())`.
                    self.state = ReaderState::DrainToEnd;
                    RiffReadStep::need_read(RIFF_BUFFER_SIZE)
                } else {
                    self.state = ReaderState::ChunkHeader;
                    self.next_step(stream_at_end)
                }
            }
            ReaderState::SkipPayload {
                remaining,
                stop_after_chunk,
            } => {
                if bytes.is_empty() {
                    // Premature EOF before consuming full chunk payload.
                    self.state = ReaderState::Failed;
                    return RiffReadStep::invalid();
                }
                let consumed = bytes.len().min(remaining as usize) as u32;
                let new_remaining = remaining - consumed;
                self.state = ReaderState::SkipPayload {
                    remaining: new_remaining,
                    stop_after_chunk,
                };
                self.next_step(stream_at_end)
            }
            ReaderState::DrainToEnd => {
                if stream_at_end {
                    self.state = ReaderState::Finished {
                        only_header_parsed: false,
                    };
                    RiffReadStep::done(false)
                } else {
                    RiffReadStep::need_read(RIFF_BUFFER_SIZE)
                }
            }
            ReaderState::Finished { only_header_parsed } => RiffReadStep::done(only_header_parsed),
            ReaderState::Failed => RiffReadStep::invalid(),
        }
    }
}

/// Convenience helper for scanning an in-memory byte slice using [`RiffHeaderReader`].
///
/// Returns `Some((only_header_parsed, prefix_len))` on success, or `None` on invalid/truncated data.
pub fn scan_riff_header_slice(data: &[u8]) -> Option<(bool, usize)> {
    let mut reader = RiffHeaderReader::new();
    let mut offset = 0usize;
    let mut step = reader.next_step(offset >= data.len());
    loop {
        match step.kind {
            x if x == RiffStepKind::NeedRead as i32 => {
                let avail = data.len().saturating_sub(offset);
                let to_read = step.max_bytes.min(avail);
                let chunk = &data[offset..offset + to_read];
                offset += to_read;
                step = reader.feed(chunk, offset >= data.len());
            }
            x if x == RiffStepKind::Done as i32 => {
                return Some((step.only_header_parsed, offset));
            }
            _ => return None,
        }
    }
}
