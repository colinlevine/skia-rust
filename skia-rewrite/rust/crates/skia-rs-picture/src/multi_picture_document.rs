//! Safe Rust implementation of `SkMultiPictureDocument` binary format and layout helpers
//! (`src/utils/SkMultiPictureDocument.cpp`).

use skia_rs_geometry::{cpp_max_f32, Size};

/// Exact file magic (`sizeof(kMagic) - 1 == 24`).
pub const MPD_MAGIC: &[u8; 24] = b"Skia Multi-Picture Doc\n\n";

/// Exact end-of-page annotation key (NUL-terminated for C FFI).
pub const MPD_END_PAGE_CSTR: &[u8] = b"SkMultiPictureEndPage\0";
pub const MPD_END_PAGE: &str = "SkMultiPictureEndPage";

/// Format version (`kVersion == 2`).
pub const MPD_VERSION: u32 = 2;

/// Maximum valid page width or height (`1 << 16 == 65536`).
pub const MPD_MAX_DIMENSION: f32 = (1u32 << 16) as f32;

/// Size in bytes of the fixed header prefix: magic (24) + version (4) + page_count (4) = 32.
pub const MPD_HEADER_PREFIX_SIZE: usize = 24 + 4 + 4;

/// Returns the componentwise max of `sizes`, matching `join(const TArray<SkSize>&)`.
pub fn join_page_sizes(sizes: &[Size]) -> Size {
    let mut joined = Size {
        width: 0.0,
        height: 0.0,
    };
    for &s in sizes {
        joined = Size {
            width: cpp_max_f32(joined.width, s.width),
            height: cpp_max_f32(joined.height, s.height),
        };
    }
    joined
}

/// Encodes the Multi-Picture Document header (magic + version + page_count + page sizes).
pub fn encode_mpd_header(sizes: &[Size]) -> Vec<u8> {
    let mut out = Vec::with_capacity(MPD_HEADER_PREFIX_SIZE + sizes.len() * 8);
    out.extend_from_slice(MPD_MAGIC);
    out.extend_from_slice(&MPD_VERSION.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u32).to_le_bytes());
    for &s in sizes {
        out.extend_from_slice(&s.width.to_le_bytes());
        out.extend_from_slice(&s.height.to_le_bytes());
    }
    out
}

/// Validates a 32-byte header prefix (`magic`, `version == 2`, `page_count <= INT_MAX / 8`).
/// Returns `Some(page_count)` on success or `None` on failure.
pub fn parse_mpd_header_prefix(header: &[u8]) -> Option<i32> {
    if header.len() < MPD_HEADER_PREFIX_SIZE {
        return None;
    }
    if &header[0..24] != MPD_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    if version != MPD_VERSION {
        return None;
    }
    let page_count = u32::from_le_bytes([header[28], header[29], header[30], header[31]]);
    let max_pages = (i32::MAX as usize) / core::mem::size_of::<Size>();
    if (page_count as usize) > max_pages {
        return None;
    }
    Some(page_count as i32)
}

/// Validates a single decoded page size (`!s.isEmpty() && w < kMaxDimension && h < kMaxDimension`).
#[inline]
pub fn is_valid_mpd_page_size(s: Size) -> bool {
    !(s.width <= 0.0 || s.height <= 0.0)
        && s.width < MPD_MAX_DIMENSION
        && s.height < MPD_MAX_DIMENSION
}

/// Checks whether the stream length is sufficient for `page_count` `SkSize` entries.
#[inline]
pub fn validate_mpd_page_count_and_length(
    page_count: i32,
    dst_array_count: i32,
    has_length: bool,
    stream_length: usize,
) -> bool {
    if page_count < 1 || page_count != dst_array_count {
        return false;
    }
    if has_length && stream_length < (page_count as usize) * core::mem::size_of::<Size>() {
        return false;
    }
    true
}

/// Returns `true` if `key` matches the MultiPictureDocument end-of-page annotation key.
#[inline]
pub fn is_end_page_annotation(key: &[u8]) -> bool {
    key == MPD_END_PAGE.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpd_header_roundtrip() {
        let sizes = [
            Size { width: 612.0, height: 792.0 },
            Size { width: 800.0, height: 600.0 },
        ];
        let bytes = encode_mpd_header(&sizes);
        assert_eq!(bytes.len(), 32 + 16);
        assert_eq!(parse_mpd_header_prefix(&bytes), Some(2));
        assert_eq!(join_page_sizes(&sizes), Size { width: 800.0, height: 792.0 });

        // Corrupt magic
        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        assert_eq!(parse_mpd_header_prefix(&bad_magic), None);

        // Wrong version
        let mut bad_ver = bytes.clone();
        bad_ver[24] = 3;
        assert_eq!(parse_mpd_header_prefix(&bad_ver), None);

        // Overflow page count
        let mut bad_cnt = bytes.clone();
        bad_cnt[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_mpd_header_prefix(&bad_cnt), None);
    }

    #[test]
    fn test_mpd_page_size_validation() {
        assert!(is_valid_mpd_page_size(Size { width: 100.0, height: 200.0 }));
        assert!(!is_valid_mpd_page_size(Size { width: 0.0, height: 200.0 }));
        assert!(!is_valid_mpd_page_size(Size { width: -1.0, height: 200.0 }));
        assert!(!is_valid_mpd_page_size(Size { width: 65536.0, height: 100.0 }));
        assert!(!is_valid_mpd_page_size(Size { width: 100.0, height: 65536.0 }));
        assert!(is_end_page_annotation(b"SkMultiPictureEndPage"));
        assert!(!is_end_page_annotation(b"OtherKey"));
    }
}
