//! Safe Rust port of `src/codec/SkJpegMetadataDecoderImpl.cpp`.
//!
//! Handles APP1/APP2/APP11 marker matching, multi-part ICC profile reassembly,
//! EXIF/ISO 21496-1/JUMBF extraction, MPF parameter search, and gainmap offset
//! calculation.

use crate::constants::{
    EXIF_MARKER, EXIF_SIG, ICC_MARKER, ICC_MARKER_INDEX_SIZE, ICC_SIG, ISO_GAINMAP_MARKER,
    ISO_GAINMAP_SIG, JUMBF_MARKER, JUMBF_SIG, MARKER_APP0, MARKER_CODE_SIZE, MARKER_END_OF_IMAGE,
    MARKER_START_OF_SCAN, MPF_MARKER,
};
use crate::multi_picture::MultiPictureParameters;
use crate::segment_scan::{JpegSegment, SegmentScanner};

/// Borrowed reference to a JPEG marker and its parameter payload (without 0xFF/marker/length).
#[derive(Copy, Clone, Debug)]
pub struct JpegMarkerRef<'a> {
    pub marker: u8,
    pub data: &'a [u8],
}

/// Result of reading single- or multi-segment JPEG metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataReadResult {
    /// Metadata was not present or was malformed.
    NotFoundOrInvalid,
    /// Single segment matched and `!always_copy_data`: caller may borrow
    /// `markers[marker_index].data[offset..offset + len]` without copying.
    SingleRef {
        marker_index: usize,
        offset: usize,
        len: usize,
    },
    /// Multi-segment reassembled bytes (or copied single segment when `always_copy_data` is true).
    Owned(Vec<u8>),
}

#[inline]
fn marker_has_signature(marker: &JpegMarkerRef<'_>, target_marker: u8, signature: &[u8]) -> bool {
    marker.marker == target_marker
        && marker.data.len() > signature.len()
        && &marker.data[..signature.len()] == signature
}

/// Exact port of `read_metadata` in `SkJpegMetadataDecoderImpl.cpp`.
pub fn read_metadata(
    markers: &[JpegMarkerRef<'_>],
    target_marker: u8,
    signature: &[u8],
    signature_padding: usize,
    bytes_in_index: usize,
    always_copy_data: bool,
) -> MetadataReadResult {
    let header_size = signature.len() + signature_padding + 2 * bytes_in_index;
    let mut parts: Vec<Option<(usize, &[u8])>> = Vec::new();
    let mut parts_total_size: usize = 0;
    let mut found_part_count: u32 = 0;
    let mut expected_part_count: u32 = 0;

    for (marker_idx, marker) in markers.iter().enumerate() {
        if !marker_has_signature(marker, target_marker, signature) {
            continue;
        }
        let data = marker.data;
        if data.len() <= header_size {
            continue;
        }

        let (part_index, part_count) = if bytes_in_index == 0 {
            (1u32, 1u32)
        } else {
            let offset = signature.len() + signature_padding;
            let mut idx: u32 = 0;
            let mut cnt: u32 = 0;
            for i in 0..bytes_in_index {
                idx = (idx << 8) + (data[offset + i] as u32);
                cnt = (cnt << 8) + (data[offset + bytes_in_index + i] as u32);
            }
            (idx, cnt)
        };

        if part_count == 0 {
            return MetadataReadResult::NotFoundOrInvalid;
        }
        if part_index == 0 || part_index > part_count {
            return MetadataReadResult::NotFoundOrInvalid;
        }

        if expected_part_count == 0 {
            expected_part_count = part_count;
            parts.resize(expected_part_count as usize, None);
        }
        if part_count != expected_part_count {
            return MetadataReadResult::NotFoundOrInvalid;
        }

        let slot = &mut parts[(part_index - 1) as usize];
        if slot.is_some() {
            return MetadataReadResult::NotFoundOrInvalid;
        }

        let payload = &data[header_size..];
        parts_total_size += payload.len();
        *slot = Some((marker_idx, payload));
        found_part_count += 1;

        if found_part_count == expected_part_count {
            break;
        }
    }

    if expected_part_count == 0 || found_part_count != expected_part_count {
        return MetadataReadResult::NotFoundOrInvalid;
    }

    if !always_copy_data && expected_part_count == 1 {
        if let Some((marker_index, payload)) = parts[0] {
            return MetadataReadResult::SingleRef {
                marker_index,
                offset: header_size,
                len: payload.len(),
            };
        }
    }

    let mut out = Vec::with_capacity(parts_total_size);
    for slot in parts {
        if let Some((_, payload)) = slot {
            out.extend_from_slice(payload);
        }
    }
    MetadataReadResult::Owned(out)
}

/// Extract EXIF metadata (`APP1`, `"Exif\0"` + 1 byte padding).
pub fn read_exif_metadata(markers: &[JpegMarkerRef<'_>], copy_data: bool) -> MetadataReadResult {
    read_metadata(markers, EXIF_MARKER, EXIF_SIG, 1, 0, copy_data)
}

/// Extract and stitch ICC profile metadata (`APP2`, `"ICC_PROFILE\0"`, 1-byte index/count).
pub fn read_icc_profile_data(
    markers: &[JpegMarkerRef<'_>],
    copy_data: bool,
) -> MetadataReadResult {
    read_metadata(
        markers,
        ICC_MARKER,
        ICC_SIG,
        0,
        ICC_MARKER_INDEX_SIZE,
        copy_data,
    )
}

/// Extract ISO 21496-1 gainmap metadata (`APP2`, `"urn:iso:std:iso:ts:21496:-1\0"`).
pub fn read_iso_gainmap_metadata(
    markers: &[JpegMarkerRef<'_>],
    copy_data: bool,
) -> MetadataReadResult {
    read_metadata(markers, ISO_GAINMAP_MARKER, ISO_GAINMAP_SIG, 0, 0, copy_data)
}

/// Extract JUMBF superbox metadata (`APP11`, `"JP"`).
pub fn read_jumbf_metadata(markers: &[JpegMarkerRef<'_>], copy_data: bool) -> MetadataReadResult {
    read_metadata(markers, JUMBF_MARKER, JUMBF_SIG, 0, 0, copy_data)
}

/// Scan JPEG bytes up to `StartOfScan` and return all APP1 and APP2 segments.
/// Exact port of `SkJpegMetadataDecoderImpl::SkJpegMetadataDecoderImpl(sk_sp<const SkData>)`.
pub fn scan_header_app1_app2_segments(jpeg_data: &[u8]) -> Vec<JpegSegment> {
    let mut scan = SegmentScanner::new(MARKER_START_OF_SCAN);
    scan.on_bytes(jpeg_data);
    if scan.had_error() || !scan.is_done() {
        return Vec::new();
    }
    scan.segments()
        .iter()
        .copied()
        .filter(|seg| seg.marker == MARKER_APP0 + 1 || seg.marker == MARKER_APP0 + 2)
        .collect()
}

/// Search `markers` for the first APP2 segment that parses as `MultiPictureParameters`.
/// Returns `(params, skipped_mpf_marker_count)` matching `find_mp_params` in
/// `SkJpegMetadataDecoderImpl.cpp`.
pub fn find_mp_params_in_markers(
    markers: &[JpegMarkerRef<'_>],
) -> Option<(MultiPictureParameters, usize)> {
    let mut skipped_count: usize = 0;
    for marker in markers {
        if marker.marker != MPF_MARKER {
            continue;
        }
        if let Some(params) = MultiPictureParameters::parse(marker.data) {
            return Some((params, skipped_count));
        }
        skipped_count += 1;
    }
    None
}

/// Locate the `JpegSegment` corresponding to the `(skipped_count)`-th APP2 segment.
pub fn find_mp_segment_by_skip_count(
    segments: &[JpegSegment],
    mut skipped_count: usize,
) -> Option<JpegSegment> {
    for &seg in segments {
        if seg.marker != MPF_MARKER {
            continue;
        }
        if skipped_count == 0 {
            return Some(seg);
        }
        skipped_count -= 1;
    }
    None
}

/// Exact port of `SkJpegMetadataDecoderImpl::mightHaveGainmapImage`.
pub fn might_have_gainmap_image(markers: &[JpegMarkerRef<'_>]) -> bool {
    find_mp_params_in_markers(markers).is_some()
}

/// Adjust a container XMP gainmap offset relative to EndOfImage.
/// Exact port of lines 194-200 in `SkJpegMetadataDecoderImpl.cpp`.
pub fn compute_container_gainmap_offset(
    segments: &[JpegSegment],
    relative_offset: usize,
) -> usize {
    if let Some(last) = segments.last() {
        if last.marker == MARKER_END_OF_IMAGE {
            return relative_offset.wrapping_add(last.offset + MARKER_CODE_SIZE);
        }
    }
    relative_offset
}
