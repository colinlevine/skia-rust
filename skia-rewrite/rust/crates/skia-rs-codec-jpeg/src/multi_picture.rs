//! Safe Rust port of `src/codec/SkJpegMultiPicture.cpp` and `src/codec/SkJpegMultiPicture.h`.
//!
//! CIPA DC-x007-2009 Multi-Picture Format (MPF) APP2 parser, serializer, and
//! image offset calculator.

use skia_rs_codec_meta::tiff::{
    self, ImageFileDirectory, ENDIAN_BIG, TYPE_UNDEFINED, TYPE_UNSIGNED_LONG,
};

use crate::constants::{MARKER_CODE_SIZE, MPF_SIG, SEGMENT_PARAMETER_LENGTH_SIZE};

const VERSION_TAG: u16 = 0xB000;
const VERSION_COUNT: u32 = 4;
const VERSION_SIZE: usize = 4;
const VERSION_EXPECTED: [u8; VERSION_SIZE] = *b"0100";

const NUMBER_OF_IMAGES_TAG: u16 = 0xB001;
const NUMBER_OF_IMAGES_COUNT: u32 = 1;

const MP_ENTRY_TAG: u16 = 0xB002;
const MP_ENTRY_SIZE: usize = 16;

const MP_ENTRY_ATTRIBUTE_FORMAT_MASK: u32 = 0x0700_0000;
const MP_ENTRY_ATTRIBUTE_FORMAT_JPEG: u32 = 0x0000_0000;

const MP_ENTRY_ATTRIBUTE_TYPE_MASK: u32 = 0x00FF_FFFF;
const MP_ENTRY_ATTRIBUTE_TYPE_PRIMARY: u32 = 0x0003_0000;

const INDIVIDUAL_IMAGE_UNIQUE_ID_TAG: u16 = 0xB003;
const INDIVIDUAL_IMAGE_UNIQUE_ID_SIZE: usize = 33;

const TOTAL_NUMBER_CAPTURED_FRAMES_TAG: u16 = 0xB004;

/// Mirrors `SkJpegMultiPictureParameters::Image`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MpImage {
    pub size: u32,
    pub data_offset: u32,
}

/// Mirrors `SkJpegMultiPictureParameters`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiPictureParameters {
    pub images: Vec<MpImage>,
}

impl MultiPictureParameters {
    pub fn with_image_count(number_of_images: usize) -> Self {
        Self {
            images: vec![MpImage::default(); number_of_images],
        }
    }

    /// Exact port of `SkJpegMultiPictureParameters::Make`.
    pub fn parse(segment_parameters: &[u8]) -> Option<Self> {
        if segment_parameters.len() < MPF_SIG.len() {
            return None;
        }
        if &segment_parameters[..MPF_SIG.len()] != MPF_SIG {
            return None;
        }
        let ifd_data = &segment_parameters[MPF_SIG.len()..];

        let (little_endian, ifd_offset) = tiff::parse_header(ifd_data)?;
        let ifd = ImageFileDirectory::from_offset(ifd_data, little_endian, ifd_offset, false)?;

        let tag_count = ifd.num_entries();
        let mut number_of_images: u32 = 0;
        let mut mp_entries_data: Option<&[u8]> = None;

        // Tags in MP Index IFD must be in strictly ascending tag ID order (section 5.2.3).
        let mut previous_tag: u16 = 0;
        for entry_idx in 0..tag_count {
            let tag = ifd.entry_tag(entry_idx);
            if previous_tag >= tag {
                return None;
            }
            previous_tag = tag;

            match tag {
                VERSION_TAG => {
                    let data = ifd.entry_undefined_data(entry_idx)?;
                    if data.len() != VERSION_SIZE || data != VERSION_EXPECTED {
                        return None;
                    }
                }
                NUMBER_OF_IMAGES_TAG => {
                    let mut val = [0u32; 1];
                    if ifd.entry_unsigned_long(entry_idx, &mut val) {
                        number_of_images = val[0];
                    }
                    if number_of_images < 1 {
                        return None;
                    }
                }
                MP_ENTRY_TAG => {
                    let data = ifd.entry_undefined_data(entry_idx)?;
                    let expected_size = MP_ENTRY_SIZE.checked_mul(number_of_images as usize)?;
                    if data.len() != expected_size {
                        return None;
                    }
                    mp_entries_data = Some(data);
                }
                INDIVIDUAL_IMAGE_UNIQUE_ID_TAG => {
                    let data = ifd.entry_undefined_data(entry_idx)?;
                    let expected_size =
                        INDIVIDUAL_IMAGE_UNIQUE_ID_SIZE.checked_mul(number_of_images as usize)?;
                    if data.len() != expected_size {
                        return None;
                    }
                }
                TOTAL_NUMBER_CAPTURED_FRAMES_TAG => {
                    let mut val = [0u32; 1];
                    let _ = ifd.entry_unsigned_long(entry_idx, &mut val);
                }
                _ => {
                    return None;
                }
            }
        }

        if number_of_images == 0 {
            return None;
        }
        let entries_bytes = mp_entries_data?;

        let mut images = Vec::with_capacity(number_of_images as usize);
        for i in 0..(number_of_images as usize) {
            let entry = entries_bytes.get(i * MP_ENTRY_SIZE..(i + 1) * MP_ENTRY_SIZE)?;
            let attribute = tiff::get_endian_u32(&entry[0..4], little_endian);
            let size = tiff::get_endian_u32(&entry[4..8], little_endian);
            let data_offset = tiff::get_endian_u32(&entry[8..12], little_endian);

            let is_primary = (attribute & MP_ENTRY_ATTRIBUTE_TYPE_MASK)
                == MP_ENTRY_ATTRIBUTE_TYPE_PRIMARY;
            let is_jpeg = (attribute & MP_ENTRY_ATTRIBUTE_FORMAT_MASK)
                == MP_ENTRY_ATTRIBUTE_FORMAT_JPEG;

            if is_primary != (i == 0) {
                return None;
            }
            if !is_jpeg {
                return None;
            }
            if i == 0 && data_offset != 0 {
                return None;
            }

            images.push(MpImage { size, data_offset });
        }

        Some(Self { images })
    }

    /// Exact byte-for-byte port of `SkJpegMultiPictureParameters::serialize`.
    pub fn serialize(&self, individual_image_number: u32) -> Vec<u8> {
        let number_of_images = self.images.len() as u32;
        let mut s = Vec::new();

        // MPF signature.
        s.extend_from_slice(MPF_SIG);

        // Always write big-endian.
        s.extend_from_slice(&ENDIAN_BIG);

        // First IFD offset = sizeof(kEndianBig) + sizeof(u32) = 8.
        let first_ifd_offset: u32 = (ENDIAN_BIG.len() + 4) as u32;
        s.extend_from_slice(&first_ifd_offset.to_be_bytes());
        debug_assert_eq!(s.len() - MPF_SIG.len(), first_ifd_offset as usize);

        if individual_image_number == 0 {
            // MP Index IFD: 3 tags (version, number of images, MP entries).
            s.extend_from_slice(&3u16.to_be_bytes());
        } else {
            // MP Attribute IFD: 1 tag (version).
            s.extend_from_slice(&1u16.to_be_bytes());
        }

        // Tag 0xB000: Version "0100".
        s.extend_from_slice(&VERSION_TAG.to_be_bytes());
        s.extend_from_slice(&TYPE_UNDEFINED.to_be_bytes());
        s.extend_from_slice(&VERSION_COUNT.to_be_bytes());
        s.extend_from_slice(&VERSION_EXPECTED);

        if individual_image_number == 0 {
            // Tag 0xB001: Number of images.
            s.extend_from_slice(&NUMBER_OF_IMAGES_TAG.to_be_bytes());
            s.extend_from_slice(&TYPE_UNSIGNED_LONG.to_be_bytes());
            s.extend_from_slice(&NUMBER_OF_IMAGES_COUNT.to_be_bytes());
            s.extend_from_slice(&number_of_images.to_be_bytes());

            // Tag 0xB002: MP Entries.
            s.extend_from_slice(&MP_ENTRY_TAG.to_be_bytes());
            s.extend_from_slice(&TYPE_UNDEFINED.to_be_bytes());
            let mp_entries_size = (MP_ENTRY_SIZE as u32).wrapping_mul(number_of_images);
            s.extend_from_slice(&mp_entries_size.to_be_bytes());
            let mp_entry_offset = (s.len() - MPF_SIG.len() + 4 + 4) as u32;
            s.extend_from_slice(&mp_entry_offset.to_be_bytes());

            // Next (attribute) IFD offset = 0.
            s.extend_from_slice(&0u32.to_be_bytes());

            debug_assert_eq!(s.len() - MPF_SIG.len(), mp_entry_offset as usize);
            for (i, image) in self.images.iter().enumerate() {
                let mut attribute = MP_ENTRY_ATTRIBUTE_FORMAT_JPEG;
                if i == 0 {
                    attribute |= MP_ENTRY_ATTRIBUTE_TYPE_PRIMARY;
                }
                s.extend_from_slice(&attribute.to_be_bytes());
                s.extend_from_slice(&image.size.to_be_bytes());
                s.extend_from_slice(&image.data_offset.to_be_bytes());
                // Dependent image 1 & 2 entries = 0.
                s.extend_from_slice(&0u16.to_be_bytes());
                s.extend_from_slice(&0u16.to_be_bytes());
            }
        } else {
            // Next IFD offset = 0.
            s.extend_from_slice(&0u32.to_be_bytes());
        }

        s
    }
}

#[inline]
const fn mp_header_absolute_offset(mp_segment_offset: usize) -> usize {
    mp_segment_offset + MARKER_CODE_SIZE + SEGMENT_PARAMETER_LENGTH_SIZE + MPF_SIG.len()
}

/// Exact port of `SkJpegMultiPictureParameters::GetImageAbsoluteOffset`.
#[inline]
pub const fn get_image_absolute_offset(data_offset: u32, mp_segment_offset: usize) -> usize {
    if data_offset == 0 {
        return 0;
    }
    mp_header_absolute_offset(mp_segment_offset).wrapping_add(data_offset as usize)
}

/// Exact port of `SkJpegMultiPictureParameters::GetImageDataOffset`.
#[inline]
pub const fn get_image_data_offset(image_absolute_offset: usize, mp_segment_offset: usize) -> u32 {
    if image_absolute_offset == 0 {
        return 0;
    }
    image_absolute_offset.wrapping_sub(mp_header_absolute_offset(mp_segment_offset)) as u32
}
