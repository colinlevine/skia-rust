//! Safe Rust port of `src/codec/SkIcoCodec.cpp` directory parsing, sorting,
//! validation, and embedded-codec selection logic.

use skia_rs_codec_core::types::CodecResult;
use skia_rs_geometry::ISize;

use crate::stream::{read_u16_le, read_u32_le};

/// Size of the ICO directory header in bytes.
pub const ICO_DIRECTORY_BYTES: usize = 6;
/// Size of each ICO directory entry in bytes.
pub const ICO_DIR_ENTRY_BYTES: usize = 16;

/// Validated embedded image entry (offset and byte length within the ICO stream).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IcoEntry {
    pub offset: u32,
    pub size: u32,
}

/// Parsed and validated ICO directory entries ready for embedded codec instantiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcoDirectory {
    pub entries: Vec<IcoEntry>,
    /// Result code to return if none of the candidate entries yields a valid embedded codec.
    pub default_result: CodecResult,
}

/// Exact port of `SkIcoCodec::IsIco`.
#[inline]
pub fn is_ico(buffer: &[u8]) -> bool {
    const ICO_SIG: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
    const CUR_SIG: [u8; 4] = [0x00, 0x00, 0x02, 0x00];
    buffer.len() >= 4 && (buffer[..4] == ICO_SIG || buffer[..4] == CUR_SIG)
}

/// Checks whether an embedded ICO payload starts with the 8-byte PNG signature.
#[inline]
pub fn is_png_payload(buffer: &[u8]) -> bool {
    const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    buffer.len() >= 8 && buffer[..8] == PNG_SIG
}

/// Parses, sorts (by increasing offset), and bounds-validates an ICO directory.
///
/// Exact port of `SkIcoCodec::MakeFromStream` lines 80-188.
pub fn parse_ico_directory(data: &[u8]) -> Result<IcoDirectory, CodecResult> {
    if data.len() < ICO_DIRECTORY_BYTES {
        return Err(CodecResult::IncompleteInput);
    }

    let num_images = usize::from(read_u16_le(data, 4));
    if num_images == 0 {
        return Err(CodecResult::InvalidInput);
    }

    let dir_end = ICO_DIRECTORY_BYTES
        .checked_add(num_images.checked_mul(ICO_DIR_ENTRY_BYTES).ok_or(CodecResult::IncompleteInput)?)
        .ok_or(CodecResult::IncompleteInput)?;
    if data.len() < dir_end {
        return Err(CodecResult::IncompleteInput);
    }

    let mut raw_entries = Vec::with_capacity(num_images);
    for i in 0..num_images {
        let base = ICO_DIRECTORY_BYTES + i * ICO_DIR_ENTRY_BYTES;
        let size = read_u32_le(data, base + 8);
        let offset = read_u32_le(data, base + 12);
        raw_entries.push(IcoEntry { offset, size });
    }

    // Sort by increasing offset (matching SkTQSort with `a.offset < b.offset`).
    raw_entries.sort_by_key(|e| e.offset);

    let mut default_result = CodecResult::InvalidInput;
    let mut bytes_read = dir_end;
    let mut valid_entries = Vec::with_capacity(num_images);

    for entry in raw_entries {
        let offset = entry.offset as usize;
        let size = entry.size as usize;

        // Ensure that the offset is valid (does not overlap directory or prior image).
        if offset < bytes_read {
            continue;
        }

        // If we cannot skip to offset, assume we have reached the end of the stream.
        if offset >= data.len() {
            break;
        }

        let Some(end) = offset.checked_add(size) else {
            default_result = CodecResult::IncompleteInput;
            break;
        };
        if end > data.len() {
            default_result = CodecResult::IncompleteInput;
            break;
        }

        valid_entries.push(entry);
        bytes_read = end;
    }

    Ok(IcoDirectory {
        entries: valid_entries,
        default_result,
    })
}

/// Selects the index of the embedded codec with the largest `computeMinByteSize()`.
/// Exact port of `SkIcoCodec::MakeFromStream` lines 227-237.
#[inline]
pub fn ico_select_largest_codec(min_byte_sizes: &[usize]) -> usize {
    let mut max_size = 0usize;
    let mut max_index = 0usize;
    for (i, &size) in min_byte_sizes.iter().enumerate() {
        if size > max_size {
            max_size = size;
            max_index = i;
        }
    }
    max_index
}

/// Chooses the embedded codec dimensions with pixel area closest to
/// `desired_scale * orig.width * orig.height`.
/// Exact port of `SkIcoCodec::onGetScaledDimensions`.
pub fn ico_get_scaled_dimensions(
    orig: ISize,
    desired_scale: f32,
    embedded_dims: &[ISize],
) -> ISize {
    if embedded_dims.is_empty() {
        return orig;
    }
    let orig_width = orig.width;
    let orig_height = orig.height;
    let desired_size = (desired_scale * (orig_width as f32)) * (orig_height as f32);
    let mut min_error = ((orig_width.wrapping_mul(orig_height)) as f32) - desired_size + 1.0_f32;
    let mut min_index: i32 = -1;

    for (i, &dim) in embedded_dims.iter().enumerate() {
        let area = (dim.width.wrapping_mul(dim.height)) as f32;
        let error = (area - desired_size).abs();
        if error < min_error {
            min_error = error;
            min_index = i as i32;
        }
    }

    if min_index >= 0 {
        embedded_dims[min_index as usize]
    } else {
        orig
    }
}

/// Searches `embedded_dims` starting at `start_index` for an exact match to `requested`.
/// Returns the matching index or `-1` if none exists.
/// Exact port of `SkIcoCodec::chooseCodec`.
#[inline]
pub fn ico_choose_codec(embedded_dims: &[ISize], requested: ISize, start_index: i32) -> i32 {
    if start_index < 0 {
        return -1;
    }
    let start = start_index as usize;
    for (i, &dim) in embedded_dims.iter().enumerate().skip(start) {
        if dim == requested {
            return i as i32;
        }
    }
    -1
}
