//! Stream abstraction for safe Rust BMP, WBMP, and ICO decoding.

/// Minimal byte-stream reader interface matching `SkStream::read` and `SkStream::skip`.
pub trait ByteReader {
    /// Reads up to `buf.len()` bytes into `buf`, returning the number of bytes actually read.
    fn read(&mut self, buf: &mut [u8]) -> usize;

    /// Skips up to `count` bytes, returning the number of bytes actually skipped.
    fn skip(&mut self, count: usize) -> usize;
}

/// In-memory byte-slice implementation of [`ByteReader`].
pub struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    #[inline]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    #[inline]
    pub const fn position(&self) -> usize {
        self.pos
    }

    #[inline]
    pub const fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
}

impl ByteReader for SliceReader<'_> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> usize {
        let avail = self.data.len() - self.pos;
        let to_read = buf.len().min(avail);
        buf[..to_read].copy_from_slice(&self.data[self.pos..self.pos + to_read]);
        self.pos += to_read;
        to_read
    }

    #[inline]
    fn skip(&mut self, count: usize) -> usize {
        let avail = self.data.len() - self.pos;
        let to_skip = count.min(avail);
        self.pos += to_skip;
        to_skip
    }
}

#[inline]
pub(crate) fn read_u16_le(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

#[inline]
pub(crate) fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

#[inline]
pub(crate) fn read_i32_le(buf: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}
