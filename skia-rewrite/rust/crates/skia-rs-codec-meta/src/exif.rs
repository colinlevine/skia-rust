//! Safe Rust port of `src/codec/SkExif.cpp` and `src/codec/SkParseEncodedOrigin.cpp`.

use crate::tiff::{
    self, ImageFileDirectory, ENDIAN_BIG, SIZE_ENTRY, SIZE_LONG, SIZE_SHORT, TYPE_UNSIGNED_LONG,
    TYPE_UNSIGNED_RATIONAL, TYPE_UNSIGNED_SHORT,
};

pub const ORIGIN_TAG: u16 = 0x0112;
pub const RESOLUTION_UNIT_TAG: u16 = 0x0128;
pub const X_RESOLUTION_TAG: u16 = 0x011a;
pub const Y_RESOLUTION_TAG: u16 = 0x011b;
pub const PIXEL_X_DIMENSION_TAG: u16 = 0xa002;
pub const PIXEL_Y_DIMENSION_TAG: u16 = 0xa003;
pub const SUB_IFD_OFFSET_TAG: u16 = 0x8769;
pub const MAKER_NOTE_TAG: u16 = 0x927c;

/// Matches `kLast_SkEncodedOrigin` (8 = `kLeftBottom_SkEncodedOrigin`).
pub const LAST_ENCODED_ORIGIN: u16 = 8;

/// Mirrors `SkExif::Metadata`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExifMetadata {
    pub origin: Option<u32>,
    pub hdr_headroom: Option<f32>,
    pub resolution_unit: Option<u16>,
    pub x_resolution: Option<f32>,
    pub y_resolution: Option<f32>,
    pub pixel_x_dimension: Option<u32>,
    pub pixel_y_dimension: Option<u32>,
}

fn get_maker_note_hdr_headroom(data: &[u8]) -> Option<f32> {
    const SIG: [u8; 14] = [
        b'A', b'p', b'p', b'l', b'e', b' ', b'i', b'O', b'S', 0, 0, 1, b'M', b'M',
    ];
    if data.len() < SIG.len() || data[..SIG.len()] != SIG {
        return None;
    }
    let ifd = ImageFileDirectory::from_offset(data, false, SIG.len() as u32, false)?;

    let mut has_maker33 = false;
    let mut has_maker48 = false;
    let mut maker33 = 0.0_f32;
    let mut maker48 = 0.0_f32;
    for i in 0..ifd.num_entries() {
        match ifd.entry_tag(i) {
            33 => {
                if !has_maker33 {
                    let mut v = [0.0_f32; 1];
                    if ifd.entry_signed_rational(i, &mut v) {
                        has_maker33 = true;
                        maker33 = v[0];
                    }
                }
            }
            48 => {
                if !has_maker48 {
                    let mut v = [0.0_f32; 1];
                    if ifd.entry_signed_rational(i, &mut v) {
                        has_maker48 = true;
                        maker48 = v[0];
                    }
                }
            }
            _ => {}
        }
    }
    if !has_maker33 {
        return None;
    }
    let stops = if maker33 < 1.0_f32 {
        if maker48 <= 0.01_f32 {
            -20.0_f32 * maker48 + 1.8_f32
        } else {
            -0.101_f32 * maker48 + 1.601_f32
        }
    } else if maker48 <= 0.01_f32 {
        -70.0_f32 * maker48 + 3.0_f32
    } else {
        -0.303_f32 * maker48 + 2.303_f32
    };
    Some(2.0_f32.powf(stops.max(0.0_f32)))
}

fn parse_ifd(
    exif: &mut ExifMetadata,
    data: &[u8],
    ifd: ImageFileDirectory<'_>,
    little_endian: bool,
    is_root: bool,
) {
    for i in 0..ifd.num_entries() {
        match ifd.entry_tag(i) {
            ORIGIN_TAG => {
                let mut v = [0u16; 1];
                if exif.origin.is_none() && ifd.entry_unsigned_short(i, &mut v) {
                    if v[0] > 0 && v[0] <= LAST_ENCODED_ORIGIN {
                        exif.origin = Some(v[0] as u32);
                    }
                }
            }
            MAKER_NOTE_TAG => {
                if exif.hdr_headroom.is_none() {
                    if let Some(maker_data) = ifd.entry_undefined_data(i) {
                        exif.hdr_headroom = get_maker_note_hdr_headroom(maker_data);
                    }
                }
            }
            SUB_IFD_OFFSET_TAG => {
                let mut sub_offset = [0u32; 1];
                if is_root && ifd.entry_unsigned_long(i, &mut sub_offset) {
                    if let Some(sub_ifd) =
                        ImageFileDirectory::from_offset(data, little_endian, sub_offset[0], true)
                    {
                        parse_ifd(exif, data, sub_ifd, little_endian, false);
                    }
                }
            }
            X_RESOLUTION_TAG => {
                let mut v = [0.0_f32; 1];
                if exif.x_resolution.is_none() && ifd.entry_unsigned_rational(i, &mut v) {
                    exif.x_resolution = Some(v[0]);
                }
            }
            Y_RESOLUTION_TAG => {
                let mut v = [0.0_f32; 1];
                if exif.y_resolution.is_none() && ifd.entry_unsigned_rational(i, &mut v) {
                    exif.y_resolution = Some(v[0]);
                }
            }
            RESOLUTION_UNIT_TAG => {
                let mut v = [0u16; 1];
                if exif.resolution_unit.is_none() && ifd.entry_unsigned_short(i, &mut v) {
                    exif.resolution_unit = Some(v[0]);
                }
            }
            PIXEL_X_DIMENSION_TAG => {
                let mut v16 = [0u16; 1];
                if exif.pixel_x_dimension.is_none() && ifd.entry_unsigned_short(i, &mut v16) {
                    exif.pixel_x_dimension = Some(v16[0] as u32);
                }
                let mut v32 = [0u32; 1];
                if exif.pixel_x_dimension.is_none() && ifd.entry_unsigned_long(i, &mut v32) {
                    exif.pixel_x_dimension = Some(v32[0]);
                }
            }
            PIXEL_Y_DIMENSION_TAG => {
                let mut v16 = [0u16; 1];
                if exif.pixel_y_dimension.is_none() && ifd.entry_unsigned_short(i, &mut v16) {
                    exif.pixel_y_dimension = Some(v16[0] as u32);
                }
                let mut v32 = [0u32; 1];
                if exif.pixel_y_dimension.is_none() && ifd.entry_unsigned_long(i, &mut v32) {
                    exif.pixel_y_dimension = Some(v32[0]);
                }
            }
            _ => {}
        }
    }
}

/// Parse EXIF data into `metadata` (preserving any already-set fields, matching `SkExif::Parse`).
pub fn parse_exif_into(metadata: &mut ExifMetadata, data: &[u8]) {
    if let Some((little_endian, ifd_offset)) = tiff::parse_header(data) {
        if let Some(ifd) = ImageFileDirectory::from_offset(data, little_endian, ifd_offset, true) {
            parse_ifd(metadata, data, ifd, little_endian, true);
        }
    }
}

/// Parse EXIF data into a fresh `ExifMetadata`.
pub fn parse_exif(data: &[u8]) -> ExifMetadata {
    let mut metadata = ExifMetadata::default();
    parse_exif_into(&mut metadata, data);
    metadata
}

/// Matches `SkParseEncodedOrigin`.
pub fn parse_encoded_origin(data: &[u8]) -> Option<u32> {
    parse_exif(data).origin
}

fn write_entry(
    tag: u16,
    ty: u16,
    count: u32,
    value: u32,
    end_of_data: &mut u32,
    stream: &mut Vec<u8>,
    buffer: &mut Vec<u8>,
) -> bool {
    stream.extend_from_slice(&tag.to_be_bytes());
    stream.extend_from_slice(&ty.to_be_bytes());
    stream.extend_from_slice(&count.to_be_bytes());
    match tag {
        ORIGIN_TAG | RESOLUTION_UNIT_TAG => {
            stream.extend_from_slice(&(value as u16).to_be_bytes());
            stream.extend_from_slice(&0u16.to_be_bytes());
            true
        }
        PIXEL_X_DIMENSION_TAG | PIXEL_Y_DIMENSION_TAG | SUB_IFD_OFFSET_TAG => {
            stream.extend_from_slice(&value.to_be_bytes());
            true
        }
        X_RESOLUTION_TAG | Y_RESOLUTION_TAG => {
            stream.extend_from_slice(&end_of_data.to_be_bytes());
            *end_of_data += 8;
            buffer.extend_from_slice(&value.to_be_bytes()); // Numerator
            buffer.extend_from_slice(&1u32.to_be_bytes()); // Denominator
            true
        }
        _ => false,
    }
}

/// Serialize `ExifMetadata` to untruncated big-endian EXIF bytes.
/// Matches `SkExif::WriteExif` byte-for-byte.
pub fn write_exif(metadata: &ExifMetadata) -> Option<Vec<u8>> {
    if metadata.hdr_headroom.is_some() {
        return None;
    }

    const OFFSET: u32 = 8;
    let mut stream = Vec::new();
    let mut buffer_for_larger_values = Vec::new();

    stream.extend_from_slice(&ENDIAN_BIG);
    stream.extend_from_slice(&OFFSET.to_be_bytes());

    let mut num_tags: u16 = 0;
    let mut num_sub_ifd_tags: u16 = 0;
    if metadata.origin.is_some() {
        num_tags += 1;
    }
    if metadata.resolution_unit.is_some() {
        num_tags += 1;
    }
    if metadata.x_resolution.is_some() {
        num_tags += 1;
    }
    if metadata.y_resolution.is_some() {
        num_tags += 1;
    }
    if metadata.pixel_x_dimension.is_some() {
        num_sub_ifd_tags += 1;
    }
    if metadata.pixel_y_dimension.is_some() {
        num_sub_ifd_tags += 1;
    }
    let sub_ifd_exists = num_sub_ifd_tags > 0;
    if sub_ifd_exists {
        num_tags += 1;
    }

    let mut end_of_data = OFFSET
        + (SIZE_SHORT as u32)
        + (SIZE_ENTRY as u32) * (num_tags as u32)
        + (SIZE_LONG as u32);
    let sub_ifd_offset = end_of_data;
    if sub_ifd_exists {
        end_of_data += (SIZE_SHORT as u32)
            + (SIZE_ENTRY as u32) * (num_sub_ifd_tags as u32)
            + (SIZE_LONG as u32);
    }

    stream.extend_from_slice(&num_tags.to_be_bytes());

    if let Some(origin) = metadata.origin {
        if !write_entry(
            ORIGIN_TAG,
            TYPE_UNSIGNED_SHORT,
            1,
            origin,
            &mut end_of_data,
            &mut stream,
            &mut buffer_for_larger_values,
        ) {
            return None;
        }
    }
    if let Some(unit) = metadata.resolution_unit {
        if !write_entry(
            RESOLUTION_UNIT_TAG,
            TYPE_UNSIGNED_SHORT,
            1,
            unit as u32,
            &mut end_of_data,
            &mut stream,
            &mut buffer_for_larger_values,
        ) {
            return None;
        }
    }
    if let Some(x_res) = metadata.x_resolution {
        if !write_entry(
            X_RESOLUTION_TAG,
            TYPE_UNSIGNED_RATIONAL,
            1,
            x_res as u32,
            &mut end_of_data,
            &mut stream,
            &mut buffer_for_larger_values,
        ) {
            return None;
        }
    }
    if let Some(y_res) = metadata.y_resolution {
        if !write_entry(
            Y_RESOLUTION_TAG,
            TYPE_UNSIGNED_RATIONAL,
            1,
            y_res as u32,
            &mut end_of_data,
            &mut stream,
            &mut buffer_for_larger_values,
        ) {
            return None;
        }
    }
    if sub_ifd_exists
        && !write_entry(
            SUB_IFD_OFFSET_TAG,
            TYPE_UNSIGNED_LONG,
            1,
            sub_ifd_offset,
            &mut end_of_data,
            &mut stream,
            &mut buffer_for_larger_values,
        )
    {
        return None;
    }

    // Next IFD offset = 0
    stream.extend_from_slice(&0u32.to_be_bytes());

    if sub_ifd_exists {
        stream.extend_from_slice(&num_sub_ifd_tags.to_be_bytes());
        if let Some(px) = metadata.pixel_x_dimension {
            if !write_entry(
                PIXEL_X_DIMENSION_TAG,
                TYPE_UNSIGNED_LONG,
                1,
                px,
                &mut end_of_data,
                &mut stream,
                &mut buffer_for_larger_values,
            ) {
                return None;
            }
        }
        if let Some(py) = metadata.pixel_y_dimension {
            if !write_entry(
                PIXEL_Y_DIMENSION_TAG,
                TYPE_UNSIGNED_LONG,
                1,
                py,
                &mut end_of_data,
                &mut stream,
                &mut buffer_for_larger_values,
            ) {
                return None;
            }
        }
        // SubIFD next offset = 0
        stream.extend_from_slice(&0u32.to_be_bytes());
    }

    stream.extend_from_slice(&buffer_for_larger_values);
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_parse_roundtrip() {
        let meta = ExifMetadata {
            origin: Some(6),
            hdr_headroom: None,
            resolution_unit: Some(2),
            x_resolution: Some(144.0),
            y_resolution: Some(36.0),
            pixel_x_dimension: Some(50),
            pixel_y_dimension: Some(100),
        };
        let bytes = write_exif(&meta).expect("write_exif should succeed");
        let parsed = parse_exif(&bytes);
        assert_eq!(parsed, meta);
        assert_eq!(parse_encoded_origin(&bytes), Some(6));
    }

    #[test]
    fn write_fails_with_hdr_headroom() {
        let meta = ExifMetadata {
            hdr_headroom: Some(2.0),
            ..Default::default()
        };
        assert!(write_exif(&meta).is_none());
    }

    #[test]
    fn apple_maker_note_headroom() {
        // Construct Apple MakerNote with maker33 = 0/1, maker48 = 0/1 -> stops = 1.8 -> 2^1.8 = 3.482202
        let mut maker = Vec::new();
        maker.extend_from_slice(b"Apple iOS\0\0\x01MM");
        maker.extend_from_slice(&2u16.to_be_bytes());
        // tag 33, signed rational (10), count 1, offset 14 + 2 + 24 + 4 = 44
        maker.extend_from_slice(&33u16.to_be_bytes());
        maker.extend_from_slice(&tiff::TYPE_SIGNED_RATIONAL.to_be_bytes());
        maker.extend_from_slice(&1u32.to_be_bytes());
        maker.extend_from_slice(&44u32.to_be_bytes());
        // tag 48, signed rational (10), count 1, offset 52
        maker.extend_from_slice(&48u16.to_be_bytes());
        maker.extend_from_slice(&tiff::TYPE_SIGNED_RATIONAL.to_be_bytes());
        maker.extend_from_slice(&1u32.to_be_bytes());
        maker.extend_from_slice(&52u32.to_be_bytes());
        maker.extend_from_slice(&0u32.to_be_bytes());
        // maker33 = 0/1, maker48 = 0/1
        maker.extend_from_slice(&0u32.to_be_bytes());
        maker.extend_from_slice(&1u32.to_be_bytes());
        maker.extend_from_slice(&0u32.to_be_bytes());
        maker.extend_from_slice(&1u32.to_be_bytes());

        let hr = get_maker_note_hdr_headroom(&maker).unwrap();
        assert!((hr - 3.482202_f32).abs() < 1e-4);
    }
}
