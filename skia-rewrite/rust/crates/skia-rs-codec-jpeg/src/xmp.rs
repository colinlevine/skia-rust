//! Safe Rust port of `src/codec/SkJpegXmp.cpp`.
//!
//! Standard APP1 XMP extraction and Extended XMP multi-segment reassembly with
//! 128-bit ASCII uppercase-hex GUID parsing and RFC 1321 MD5 verification.

use skia_rs_math::md5::Md5State;

use crate::constants::{XMP_EXTENDED_SIG, XMP_STANDARD_SIG};

pub const GUID_ASCII_SIZE: usize = 32;

/// Locate the first APP1 segment containing Standard XMP metadata.
/// Returns `(segment_index, payload_offset)` where `payload_offset == XMP_STANDARD_SIG.len()`.
pub fn find_xmp_standard(app1_params: &[&[u8]]) -> Option<(usize, usize)> {
    let sig_size = XMP_STANDARD_SIG.len();
    for (idx, params) in app1_params.iter().enumerate() {
        if params.len() <= sig_size {
            continue;
        }
        if &params[..sig_size] != XMP_STANDARD_SIG {
            continue;
        }
        return Some((idx, sig_size));
    }
    None
}

/// Parse a 32-byte ASCII uppercase-hex GUID into a 16-byte MD5 digest.
/// Matches `read_xmp_extended` validation in `SkJpegXmp.cpp`.
pub fn parse_guid_ascii(guid_ascii: &[u8]) -> Option<[u8; 16]> {
    if guid_ascii.len() != GUID_ASCII_SIZE {
        return None;
    }
    let mut digest = [0u8; 16];
    for (i, &ch) in guid_ascii.iter().enumerate() {
        let digit = match ch {
            b'0'..=b'9' => ch - b'0',
            b'A'..=b'F' => ch - b'A' + 10,
            _ => return None,
        };
        if i % 2 == 0 {
            digest[i / 2] = digit << 4;
        } else {
            digest[i / 2] |= digit;
        }
    }
    Some(digest)
}

/// Extract, reassemble, and MD5-verify Extended XMP metadata from APP1 segments.
/// Exact port of `read_xmp_extended` in `SkJpegXmp.cpp`.
pub fn read_xmp_extended(app1_params: &[&[u8]], guid_ascii: &[u8]) -> Option<Vec<u8>> {
    const FULL_LENGTH_SIZE: usize = 4;
    const OFFSET_SIZE: usize = 4;
    let sig_size = XMP_EXTENDED_SIG.len();
    let header_size = sig_size + GUID_ASCII_SIZE + FULL_LENGTH_SIZE + OFFSET_SIZE;

    let guid_as_digest = parse_guid_ascii(guid_ascii)?;

    let mut full_length: u32 = 0;
    let mut parts: Vec<(u32, &[u8])> = Vec::new();

    for &params in app1_params {
        if params.len() <= header_size {
            continue;
        }
        if &params[..sig_size] != XMP_EXTENDED_SIG {
            continue;
        }
        let part_guid = &params[sig_size..sig_size + GUID_ASCII_SIZE];
        if part_guid != guid_ascii {
            continue;
        }

        let len_off = sig_size + GUID_ASCII_SIZE;
        let off_off = len_off + FULL_LENGTH_SIZE;
        let part_full_length = u32::from_be_bytes([
            params[len_off],
            params[len_off + 1],
            params[len_off + 2],
            params[len_off + 3],
        ]);
        let part_offset = u32::from_be_bytes([
            params[off_off],
            params[off_off + 1],
            params[off_off + 2],
            params[off_off + 3],
        ]);

        if parts.is_empty() {
            full_length = part_full_length;
        }
        if part_full_length != full_length {
            return None;
        }

        parts.push((part_offset, &params[header_size..]));
    }

    if parts.is_empty() || full_length == 0 {
        return None;
    }

    parts.sort_by_key(|&(offset, _)| offset);

    // Verify contiguity & total length before allocating the full buffer (protects against OOM on corrupt headers).
    let mut expected_offset: usize = 0;
    let full_len_usize = full_length as usize;
    for &(part_offset, part_data) in &parts {
        if (part_offset as usize) != expected_offset {
            return None;
        }
        if part_data.len() > full_len_usize - expected_offset {
            return None;
        }
        expected_offset += part_data.len();
    }
    if expected_offset != full_len_usize {
        return None;
    }

    let mut assembled = Vec::with_capacity(full_len_usize);
    for &(_, part_data) in &parts {
        assembled.extend_from_slice(part_data);
    }

    let mut md5 = Md5State::new();
    md5.update(&assembled);
    if md5.finish() != guid_as_digest {
        return None;
    }

    Some(assembled)
}
