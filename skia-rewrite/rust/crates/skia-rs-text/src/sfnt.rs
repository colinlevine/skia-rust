//! Safe Rust port of `src/core/SkFontStream.cpp` (SFNT / TTC table directory parsing).

use crate::four_byte_tag;

/// `'ttcf'` tag in big-endian host order.
pub const TTCF_TAG: u32 = four_byte_tag(b't', b't', b'c', b'f');

/// Size of `SkSFNTHeader` in bytes (4 + 2 + 2 + 2 + 2 = 12).
pub const SFNT_HEADER_SIZE: usize = 12;

/// Size of `SkTTCFHeader` / `SkSharedTTHeader` in bytes (4 + 4 + 4 + 4 = 16).
pub const SHARED_TT_HEADER_SIZE: usize = 16;

/// Size of `SkSFNTDirEntry` in bytes (4 + 4 + 4 + 4 = 16).
pub const SFNT_DIR_ENTRY_SIZE: usize = 16;

/// Parsed SFNT directory entry in host byte order.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SfntDirEntry {
    pub tag: u32,
    pub checksum: u32,
    pub offset: u32,
    pub length: u32,
}

#[inline]
fn read_be_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let slice = bytes.get(off..off + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

#[inline]
fn read_be_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let slice = bytes.get(off..off + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Mirrors `SkFontStream::CountTTCEntries` given the first `SHARED_TT_HEADER_SIZE` (16) bytes.
/// Returns `0` if `header_bytes` is too short.
pub fn count_ttc_entries(header_bytes: &[u8]) -> i32 {
    if header_bytes.len() < SHARED_TT_HEADER_SIZE {
        return 0;
    }
    let tag = read_be_u32(header_bytes, 0).unwrap_or(0);
    if tag == TTCF_TAG {
        read_be_u32(header_bytes, 8).unwrap_or(0) as i32
    } else {
        1
    }
}

/// Inspects the first 16 bytes (`SkSharedTTHeader`) of a font stream.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedHeaderProbe {
    /// `true` if the first 4 bytes are `'ttcf'`.
    pub is_collection: bool,
    /// For a collection, the number of TTC entries (`fNumOffsets`). For single SFNT, `0`.
    pub ttc_count: u32,
    /// For a collection with `ttc_index == 0`, the offset of entry 0 (`fOffset0`).
    pub ttc_offset0: u32,
    /// For a single SFNT, the number of tables (`fNumTables`).
    pub single_num_tables: u16,
}

/// Parses the 16-byte `SkSharedTTHeader`. Returns `None` if `header_bytes.len() < 16`.
pub fn probe_shared_header(header_bytes: &[u8]) -> Option<SharedHeaderProbe> {
    if header_bytes.len() < SHARED_TT_HEADER_SIZE {
        return None;
    }
    let tag = read_be_u32(header_bytes, 0)?;
    if tag == TTCF_TAG {
        let ttc_count = read_be_u32(header_bytes, 8)?;
        let ttc_offset0 = read_be_u32(header_bytes, 12)?;
        Some(SharedHeaderProbe {
            is_collection: true,
            ttc_count,
            ttc_offset0,
            single_num_tables: 0,
        })
    } else {
        let num_tables = read_be_u16(header_bytes, 4)?;
        Some(SharedHeaderProbe {
            is_collection: false,
            ttc_count: 0,
            ttc_offset0: 0,
            single_num_tables: num_tables,
        })
    }
}

/// Parses `fNumTables` from a 12-byte `SkSFNTHeader`.
pub fn parse_sfnt_num_tables(sfnt_header_bytes: &[u8]) -> Option<u16> {
    if sfnt_header_bytes.len() < SFNT_HEADER_SIZE {
        return None;
    }
    read_be_u16(sfnt_header_bytes, 4)
}

/// Parses the `ttc_index`-th entry offset from a TTC header buffer of length
/// `SHARED_TT_HEADER_SIZE + ttc_index * 4`.
pub fn parse_ttc_entry_offset(ttc_header_bytes: &[u8], ttc_index: usize) -> Option<u32> {
    let off = 12usize.checked_add(ttc_index.checked_mul(4)?)?;
    read_be_u32(ttc_header_bytes, off)
}

/// Parses a single `SkSFNTDirEntry` at `index` from raw big-endian directory bytes.
pub fn parse_dir_entry(dir_bytes: &[u8], index: usize) -> Option<SfntDirEntry> {
    let base = index.checked_mul(SFNT_DIR_ENTRY_SIZE)?;
    if base.checked_add(SFNT_DIR_ENTRY_SIZE)? > dir_bytes.len() {
        return None;
    }
    Some(SfntDirEntry {
        tag: read_be_u32(dir_bytes, base)?,
        checksum: read_be_u32(dir_bytes, base + 4)?,
        offset: read_be_u32(dir_bytes, base + 8)?,
        length: read_be_u32(dir_bytes, base + 12)?,
    })
}

/// Extracts up to `min(count, out_tags.len())` big-endian tags from raw directory bytes.
/// Returns `count` (total tables in directory).
pub fn extract_table_tags(dir_bytes: &[u8], count: usize, out_tags: &mut [u32]) -> usize {
    let n = count.min(out_tags.len());
    for (i, slot) in out_tags.iter_mut().take(n).enumerate() {
        if let Some(entry) = parse_dir_entry(dir_bytes, i) {
            *slot = entry.tag;
        }
    }
    count
}

/// Result of looking up a table and clamping `(offset, length)` against its directory entry.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TableRangeResult {
    pub found: bool,
    /// Total bytes to skip from the start of the stream (`real_offset + offset`).
    pub stream_offset: usize,
    /// Clamped number of bytes to read.
    pub clamped_length: usize,
}

/// Searches `count` directory entries for `tag` and computes the validated `(stream_offset, clamped_length)`.
/// Matches `SkFontStream::GetTableData`'s exact bounds & overflow checks.
pub fn find_table_range(
    dir_bytes: &[u8],
    count: usize,
    tag: u32,
    offset: usize,
    length: usize,
) -> TableRangeResult {
    for i in 0..count {
        let Some(entry) = parse_dir_entry(dir_bytes, i) else {
            break;
        };
        if entry.tag == tag {
            let real_offset = entry.offset as usize;
            let real_length = entry.length as usize;
            if offset >= real_length {
                return TableRangeResult::default();
            }
            // Match C++: if (offset + length < offset) { return 0; }
            if offset.wrapping_add(length) < offset {
                return TableRangeResult::default();
            }
            let clamped = length.min(real_length - offset);
            return TableRangeResult {
                found: true,
                stream_offset: real_offset.wrapping_add(offset),
                clamped_length: clamped,
            };
        }
    }
    TableRangeResult::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_sfnt_probe_and_lookup() {
        let mut hdr = [0u8; 16];
        // version = 0x00010000, numTables = 2
        hdr[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        hdr[4..6].copy_from_slice(&2u16.to_be_bytes());

        assert_eq!(count_ttc_entries(&hdr), 1);
        let probe = probe_shared_header(&hdr).unwrap();
        assert!(!probe.is_collection);
        assert_eq!(probe.single_num_tables, 2);

        let mut dir = [0u8; 32];
        // Entry 0: 'head' at offset 100, length 54
        dir[0..4].copy_from_slice(b"head");
        dir[8..12].copy_from_slice(&100u32.to_be_bytes());
        dir[12..16].copy_from_slice(&54u32.to_be_bytes());
        // Entry 1: 'cmap' at offset 200, length 80
        dir[16..20].copy_from_slice(b"cmap");
        dir[24..28].copy_from_slice(&200u32.to_be_bytes());
        dir[28..32].copy_from_slice(&80u32.to_be_bytes());

        let mut tags = [0u32; 2];
        assert_eq!(extract_table_tags(&dir, 2, &mut tags), 2);
        assert_eq!(tags[0], four_byte_tag(b'h', b'e', b'a', b'd'));
        assert_eq!(tags[1], four_byte_tag(b'c', b'm', b'a', b'p'));

        let r = find_table_range(&dir, 2, four_byte_tag(b'c', b'm', b'a', b'p'), 10, 1000);
        assert!(r.found);
        assert_eq!(r.stream_offset, 210);
        assert_eq!(r.clamped_length, 70);

        // Overflow offset + length must reject
        let r_ov = find_table_range(&dir, 2, four_byte_tag(b'c', b'm', b'a', b'p'), 10, usize::MAX);
        assert!(!r_ov.found);

        // Offset past real_length must reject
        let r_oob = find_table_range(&dir, 2, four_byte_tag(b'h', b'e', b'a', b'd'), 54, 1);
        assert!(!r_oob.found);
    }

    #[test]
    fn test_ttc_header() {
        let mut ttc = [0u8; 20];
        ttc[0..4].copy_from_slice(b"ttcf");
        ttc[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc[8..12].copy_from_slice(&2u32.to_be_bytes());
        ttc[12..16].copy_from_slice(&32u32.to_be_bytes());
        ttc[16..20].copy_from_slice(&128u32.to_be_bytes());

        assert_eq!(count_ttc_entries(&ttc), 2);
        let probe = probe_shared_header(&ttc).unwrap();
        assert!(probe.is_collection);
        assert_eq!(probe.ttc_count, 2);
        assert_eq!(probe.ttc_offset0, 32);
        assert_eq!(parse_ttc_entry_offset(&ttc, 1), Some(128));
    }
}
