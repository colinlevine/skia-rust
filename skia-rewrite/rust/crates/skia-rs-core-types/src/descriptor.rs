//! Safe Rust algorithms for `SkDescriptor` and `SkAutoDescriptor`.

use crate::is_align4;
use skia_rs_math::checksum;

/// Matches `SkDescriptor`'s 12-byte header layout (`fChecksum`, `fLength`, `fCount`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptorHeader {
    pub checksum: u32,
    pub length: u32,
    pub count: u32,
}

/// Matches `SkDescriptor::Entry` 8-byte layout (`fTag`, `fLen`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptorEntry {
    pub tag: u32,
    pub len: u32,
}

pub const DESCRIPTOR_HEADER_SIZE: usize = 12;
pub const DESCRIPTOR_ENTRY_SIZE: usize = 8;

#[inline]
fn read_u32_ne(bytes: &[u8], offset: usize) -> u32 {
    u32::from_ne_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[inline]
fn write_u32_ne(bytes: &mut [u8], offset: usize, val: u32) {
    bytes[offset..offset + 4].copy_from_slice(&val.to_ne_bytes());
}

/// Computes `SkDescriptor::ComputeChecksum(desc)`:
/// hashes all bytes after the first 4-byte checksum field using `SkChecksum::Hash32(..., 0)`.
pub fn compute_checksum(desc_bytes: &[u8]) -> u32 {
    debug_assert!(desc_bytes.len() >= DESCRIPTOR_HEADER_SIZE);
    checksum::hash32(&desc_bytes[4..], 0)
}

/// Adds an entry to a descriptor buffer whose current used length is stored in header `fLength`.
/// Returns the byte offset of the entry's payload data within `buf`.
pub fn add_entry(buf: &mut [u8], tag: u32, payload_len: usize, data: Option<&[u8]>) -> usize {
    debug_assert!(tag != 0);
    debug_assert!(is_align4(payload_len));

    let cur_len = read_u32_ne(buf, 4) as usize;
    let cur_count = read_u32_ne(buf, 8);

    write_u32_ne(buf, cur_len, tag);
    write_u32_ne(buf, cur_len + 4, payload_len as u32);

    let data_offset = cur_len + DESCRIPTOR_ENTRY_SIZE;
    if let Some(src) = data {
        buf[data_offset..data_offset + payload_len].copy_from_slice(&src[..payload_len]);
    }

    let new_len = cur_len + DESCRIPTOR_ENTRY_SIZE + payload_len;
    write_u32_ne(buf, 4, new_len as u32);
    write_u32_ne(buf, 8, cur_count + 1);

    data_offset
}

/// Searches a descriptor buffer for `tag`.
/// Returns `Some((data_offset, data_len))` if found, else `None`.
pub fn find_entry(buf: &[u8], tag: u32) -> Option<(usize, u32)> {
    if buf.len() < DESCRIPTOR_HEADER_SIZE {
        return None;
    }
    let count = read_u32_ne(buf, 8);
    let mut offset = DESCRIPTOR_HEADER_SIZE;
    for _ in 0..count {
        if offset + DESCRIPTOR_ENTRY_SIZE > buf.len() {
            return None;
        }
        let entry_tag = read_u32_ne(buf, offset);
        let entry_len = read_u32_ne(buf, offset + 4);
        let data_offset = offset + DESCRIPTOR_ENTRY_SIZE;
        if entry_tag == tag {
            return Some((data_offset, entry_len));
        }
        offset = data_offset.checked_add(entry_len as usize)?;
    }
    None
}

/// Matches `SkDescriptor::isValid()`.
pub fn is_valid(buf: &[u8], rec_tag: u32, rec_size: usize) -> bool {
    if buf.len() < DESCRIPTOR_HEADER_SIZE {
        return false;
    }
    let length = read_u32_ne(buf, 4) as usize;
    let mut count = read_u32_ne(buf, 8);

    if length < DESCRIPTOR_HEADER_SIZE || !is_align4(length) || length > buf.len() {
        return false;
    }

    let mut length_remaining = length - DESCRIPTOR_HEADER_SIZE;
    let mut offset = DESCRIPTOR_HEADER_SIZE;

    while length_remaining > 0 && count > 0 {
        if length_remaining < DESCRIPTOR_ENTRY_SIZE {
            return false;
        }
        length_remaining -= DESCRIPTOR_ENTRY_SIZE;

        let entry_tag = read_u32_ne(buf, offset);
        let entry_len = read_u32_ne(buf, offset + 4) as usize;

        if length_remaining < entry_len || !is_align4(entry_len) {
            return false;
        }
        length_remaining -= entry_len;

        if entry_tag == rec_tag && entry_len != rec_size {
            return false;
        }

        offset += DESCRIPTOR_ENTRY_SIZE + entry_len;
        count -= 1;
    }

    length_remaining == 0 && count == 0
}

/// Matches `SkDescriptor::operator==`: compares `fLength` bytes starting from checksum word.
/// Returns `true` iff identical. Note: `a` and `b` must be at least `a_len` bytes,
/// with `a` starting at `fChecksum` and `fLength` at bytes 4..8.
pub fn descriptors_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() < DESCRIPTOR_HEADER_SIZE || b.len() < DESCRIPTOR_HEADER_SIZE {
        return false;
    }
    // Check checksum first (early exit matching C++ loop).
    if a[0..4] != b[0..4] {
        return false;
    }
    // Check length second.
    if a[4..8] != b[4..8] {
        return false;
    }
    let len = read_u32_ne(a, 4) as usize;
    if len > a.len() || len > b.len() {
        return false;
    }
    a[..len] == b[..len]
}

/// Validates a deserialized descriptor header for `SkAutoDescriptor::MakeFromBuffer`.
/// Returns `Some(body_length)` if valid, else `None`.
#[inline]
pub const fn validate_header_for_read(header_length: u32, available: usize) -> Option<u32> {
    let len = header_length as usize;
    if len < DESCRIPTOR_HEADER_SIZE || !is_align4(len) {
        return None;
    }
    let body_len = len - DESCRIPTOR_HEADER_SIZE;
    if body_len > available {
        return None;
    }
    Some(body_len as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_desc(buf: &mut [u8]) {
        write_u32_ne(buf, 0, 0);
        write_u32_ne(buf, 4, DESCRIPTOR_HEADER_SIZE as u32);
        write_u32_ne(buf, 8, 0);
    }

    #[test]
    fn test_descriptor_pack_find_checksum_validate() {
        let rec_tag = 0x72656320u32;
        let rec_payload = [0x11u8; 16];
        let extra_tag = 0x65787420u32;
        let extra_payload = [0x22u8; 8];

        let total_len = DESCRIPTOR_HEADER_SIZE + (DESCRIPTOR_ENTRY_SIZE + 16) + (DESCRIPTOR_ENTRY_SIZE + 8);
        let mut buf = vec![0u8; total_len];
        init_desc(&mut buf);

        let off1 = add_entry(&mut buf, rec_tag, 16, Some(&rec_payload));
        assert_eq!(off1, DESCRIPTOR_HEADER_SIZE + DESCRIPTOR_ENTRY_SIZE);
        let off2 = add_entry(&mut buf, extra_tag, 8, Some(&extra_payload));
        assert_eq!(off2, DESCRIPTOR_HEADER_SIZE + DESCRIPTOR_ENTRY_SIZE + 16 + DESCRIPTOR_ENTRY_SIZE);

        let cksum = compute_checksum(&buf);
        write_u32_ne(&mut buf, 0, cksum);

        assert!(is_valid(&buf, rec_tag, 16));
        assert!(!is_valid(&buf, rec_tag, 20)); // wrong rec size fails

        assert_eq!(find_entry(&buf, rec_tag), Some((off1, 16)));
        assert_eq!(find_entry(&buf, extra_tag), Some((off2, 8)));
        assert_eq!(find_entry(&buf, 0x9999), None);

        let mut buf2 = buf.clone();
        assert!(descriptors_equal(&buf, &buf2));
        buf2[off2] ^= 0xFF;
        assert!(!descriptors_equal(&buf, &buf2));
    }

    #[test]
    fn test_validate_header_for_read() {
        assert_eq!(validate_header_for_read(8, 100), None);
        assert_eq!(validate_header_for_read(14, 100), None);
        assert_eq!(validate_header_for_read(20, 4), None);
        assert_eq!(validate_header_for_read(20, 8), Some(8));
    }
}
