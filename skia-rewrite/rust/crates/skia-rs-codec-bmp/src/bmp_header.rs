//! Safe Rust port of `src/codec/SkBmpCodec.cpp` header parsing and core helpers.

use skia_rs_codec_core::{
    sampler::compute_row_bytes,
    types::{CodecResult, EncodedAlpha, EncodedColor, ScanlineOrder},
};
use skia_rs_color::masks::{InputMasks, Masks};

use crate::stream::{read_i32_le, read_u16_le, read_u32_le, ByteReader};

/// Size of the BMP file header (`BITMAPFILEHEADER`).
pub const BMP_HEADER_BYTES: u32 = 14;
/// Size of the BMP file header plus the 4-byte DIB header size field.
pub const BMP_HEADER_BYTES_PLUS_FOUR: usize = (BMP_HEADER_BYTES + 4) as usize;
pub const BMP_OS2_V1_BYTES: u32 = 12;
pub const BMP_OS2_V2_BYTES: u32 = 64;
pub const BMP_INFO_BASE_BYTES: u32 = 16;
pub const BMP_INFO_V1_BYTES: u32 = 40;
pub const BMP_INFO_V2_BYTES: u32 = 52;
pub const BMP_INFO_V3_BYTES: u32 = 56;
pub const BMP_INFO_V4_BYTES: u32 = 108;
pub const BMP_INFO_V5_BYTES: u32 = 124;
pub const BMP_MASK_BYTES: u32 = 12;

/// Arbitrary maximum BMP dimension matching Skia / Chromium (`1 << 16`).
pub const BMP_MAX_DIM: i32 = 1 << 16;

/// Version and type of the second (DIB / info) bitmap header.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BmpHeaderType {
    InfoV1 = 0,
    InfoV2 = 1,
    InfoV3 = 2,
    InfoV4 = 3,
    InfoV5 = 4,
    Os2V1 = 5,
    Os2Vx = 6,
    Unknown = 7,
}

/// Bitmap compression method values (`biCompression`).
pub mod compression {
    pub const NONE: u32 = 0;
    pub const RLE_8BIT: u32 = 1;
    pub const RLE_4BIT: u32 = 2;
    pub const BIT_MASKS: u32 = 3;
    pub const JPEG: u32 = 4;
    pub const PNG: u32 = 5;
    pub const ALPHA_BIT_MASKS: u32 = 6;
    pub const CMYK: u32 = 11;
    pub const CMYK_RLE_8BIT: u32 = 12;
    pub const CMYK_RLE_4BIT: u32 = 13;
}

/// Input format dispatch matching `BmpInputFormat` in `SkBmpCodec.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BmpInputFormat {
    Standard = 0,
    Rle = 1,
    BitMask = 2,
    Unknown = 3,
}

/// Parsed BMP header metadata returned by [`read_bmp_header`].
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BmpHeaderInfo {
    pub input_format: BmpInputFormat,
    pub width: i32,
    pub height: i32,
    pub encoded_color: EncodedColor,
    pub encoded_alpha: EncodedAlpha,
    pub bits_per_component: u8,
    pub bits_per_pixel: u16,
    pub row_order: ScanlineOrder,
    pub num_colors: u32,
    pub bytes_per_color: u32,
    /// Offset from the end of the headers (`offset - bytes_read`), used for standard/RLE.
    pub pixel_data_offset: u32,
    pub is_opaque: bool,
    pub input_masks: InputMasks,
}

impl Default for BmpHeaderInfo {
    fn default() -> Self {
        Self {
            input_format: BmpInputFormat::Unknown,
            width: 0,
            height: 0,
            encoded_color: EncodedColor::Bgr,
            encoded_alpha: EncodedAlpha::Opaque,
            bits_per_component: 8,
            bits_per_pixel: 0,
            row_order: ScanlineOrder::BottomUp,
            num_colors: 0,
            bytes_per_color: 4,
            pixel_data_offset: 0,
            is_opaque: true,
            input_masks: InputMasks::default(),
        }
    }
}

/// Exact port of `SkBmpCodec::IsBmp`.
#[inline]
pub fn is_bmp(buffer: &[u8]) -> bool {
    buffer.len() >= 2 && buffer[0] == b'B' && buffer[1] == b'M'
}

/// Exact port of `get_header_type` in `SkBmpCodec.cpp`.
#[inline]
pub const fn get_header_type(info_bytes: u32) -> BmpHeaderType {
    if info_bytes >= BMP_INFO_BASE_BYTES {
        match info_bytes {
            BMP_INFO_V1_BYTES => BmpHeaderType::InfoV1,
            BMP_INFO_V2_BYTES => BmpHeaderType::InfoV2,
            BMP_INFO_V3_BYTES => BmpHeaderType::InfoV3,
            BMP_INFO_V4_BYTES => BmpHeaderType::InfoV4,
            BMP_INFO_V5_BYTES => BmpHeaderType::InfoV5,
            16 | 20 | 24 | 28 | 32 | 36 | 42 | 46 | 48 | 60 | BMP_OS2_V2_BYTES => {
                BmpHeaderType::Os2Vx
            }
            _ => BmpHeaderType::Unknown,
        }
    } else if info_bytes >= BMP_OS2_V1_BYTES {
        BmpHeaderType::Os2V1
    } else {
        BmpHeaderType::Unknown
    }
}

/// Aligns `x` up to a multiple of 4 (`SkAlign4`).
#[inline]
pub const fn align4(x: usize) -> usize {
    (x + 3) & !3
}

/// Exact port of `SkBmpCodec`'s `fSrcRowBytes`:
/// `SkAlign4(SkCodecPriv::ComputeRowBytes(width, bitsPerPixel))`.
#[inline]
pub const fn bmp_src_row_bytes(width: i32, bits_per_pixel: u16) -> usize {
    align4(compute_row_bytes(width, bits_per_pixel as u32))
}

/// Exact port of `SkBmpStandardCodec`'s `fAndMaskRowBytes`:
/// `SkAlign4(SkCodecPriv::ComputeRowBytes(width, 1))`.
#[inline]
pub const fn bmp_and_mask_row_bytes(width: i32) -> usize {
    align4(compute_row_bytes(width, 1))
}

/// Exact port of `SkBmpCodec::getDstRow`.
#[inline]
pub const fn bmp_dst_row(row_order: ScanlineOrder, y: i32, height: i32) -> i32 {
    match row_order {
        ScanlineOrder::TopDown => y,
        ScanlineOrder::BottomUp => height - y - 1,
    }
}

/// Exact port of `SkBmpCodec::ReadHeader`.
///
/// When `validate_masks` is `true` (corresponding to `codecOut != nullptr` in C++),
/// bitmask headers additionally validate `Masks::create(input_masks, bpp / 8)` and
/// determine whether an alpha mask is present. When `validate_masks` is `false`
/// (used on `onRewind()`), the stream is advanced identically and mask creation is skipped.
pub fn read_bmp_header(
    reader: &mut impl ByteReader,
    in_ico: bool,
    validate_masks: bool,
) -> Result<BmpHeaderInfo, CodecResult> {
    let total_bytes: u32;
    let offset: u32;
    let info_bytes: u32;

    if !in_ico {
        let mut h_buf = [0u8; BMP_HEADER_BYTES_PLUS_FOUR];
        if reader.read(&mut h_buf) != BMP_HEADER_BYTES_PLUS_FOUR {
            return Err(CodecResult::IncompleteInput);
        }

        total_bytes = read_u32_le(&h_buf, 2);
        offset = read_u32_le(&h_buf, 10);
        if offset < BMP_HEADER_BYTES + BMP_OS2_V1_BYTES {
            return Err(CodecResult::InvalidInput);
        }

        info_bytes = read_u32_le(&h_buf, 14);
        if info_bytes < BMP_OS2_V1_BYTES {
            return Err(CodecResult::InvalidInput);
        }
    } else {
        total_bytes = 0;
        offset = 0;

        let mut h_buf = [0u8; 4];
        if reader.read(&mut h_buf) != 4 {
            return Err(CodecResult::IncompleteInput);
        }
        info_bytes = read_u32_le(&h_buf, 0);
        if info_bytes < BMP_OS2_V1_BYTES {
            return Err(CodecResult::InvalidInput);
        }
    }

    let header_type = get_header_type(info_bytes);
    if header_type == BmpHeaderType::Unknown {
        return Err(CodecResult::InvalidInput);
    }

    // All valid header types have info_bytes <= 124, so info_bytes_remaining <= 120.
    let info_bytes_remaining = (info_bytes - 4) as usize;
    let mut i_buf = [0u8; (BMP_INFO_V5_BYTES - 4) as usize];
    let i_slice = &mut i_buf[..info_bytes_remaining];
    if reader.read(i_slice) != info_bytes_remaining {
        return Err(CodecResult::IncompleteInput);
    }

    let mut bits_per_pixel: u16;
    let mut compression: u32 = compression::NONE;
    let mut num_colors: u32 = 0;
    let bytes_per_color: u32;
    let width: i32;
    let mut height: i32;

    match header_type {
        BmpHeaderType::InfoV1
        | BmpHeaderType::InfoV2
        | BmpHeaderType::InfoV3
        | BmpHeaderType::InfoV4
        | BmpHeaderType::InfoV5
        | BmpHeaderType::Os2Vx => {
            debug_assert!(info_bytes_remaining >= 12);
            width = read_i32_le(i_slice, 0);
            height = read_i32_le(i_slice, 4);
            bits_per_pixel = read_u16_le(i_slice, 10);

            if info_bytes_remaining >= 16 {
                compression = read_u32_le(i_slice, 12);
                if info_bytes_remaining >= 32 {
                    num_colors = read_u32_le(i_slice, 28);
                }
            }
            bytes_per_color = 4;
        }
        BmpHeaderType::Os2V1 => {
            width = i32::from(read_u16_le(i_slice, 0));
            height = i32::from(read_u16_le(i_slice, 2));
            bits_per_pixel = read_u16_le(i_slice, 6);
            bytes_per_color = 3;
        }
        BmpHeaderType::Unknown => return Err(CodecResult::InvalidInput),
    }

    // Check for valid dimensions from header.
    let mut row_order = ScanlineOrder::BottomUp;
    if height < 0 {
        if height == i32::MIN {
            return Err(CodecResult::InvalidInput);
        }
        height = -height;
        row_order = ScanlineOrder::TopDown;
    }

    // Height for BMP-in-ICO is doubled because it contains XOR + AND masks.
    if in_ico {
        height /= 2;
    }

    if width <= 0 || height <= 0 || width >= BMP_MAX_DIM || height >= BMP_MAX_DIM {
        return Err(CodecResult::InvalidInput);
    }

    let mut input_masks = InputMasks::default();
    let mut mask_bytes: u32 = 0;
    let input_format: BmpInputFormat;

    match compression {
        compression::NONE => {
            if bits_per_pixel == 16 {
                // Standard 16-bit BMP is 555 (XRRRRRGGGGGBBBBB), decoded via bitmask mode.
                input_masks.red = 0x7C00;
                input_masks.green = 0x03E0;
                input_masks.blue = 0x001F;
                input_format = BmpInputFormat::BitMask;
            } else {
                input_format = BmpInputFormat::Standard;
            }
        }
        compression::RLE_8BIT => {
            if bits_per_pixel != 8 {
                bits_per_pixel = 8;
            }
            input_format = BmpInputFormat::Rle;
        }
        compression::RLE_4BIT => {
            if bits_per_pixel != 4 {
                bits_per_pixel = 4;
            }
            input_format = BmpInputFormat::Rle;
        }
        compression::ALPHA_BIT_MASKS | compression::BIT_MASKS => {
            input_format = BmpInputFormat::BitMask;
            match header_type {
                BmpHeaderType::InfoV1 => {
                    let mut m_buf = [0u8; BMP_MASK_BYTES as usize];
                    if reader.read(&mut m_buf) != BMP_MASK_BYTES as usize {
                        return Err(CodecResult::IncompleteInput);
                    }
                    mask_bytes = BMP_MASK_BYTES;
                    input_masks.red = read_u32_le(&m_buf, 0);
                    input_masks.green = read_u32_le(&m_buf, 4);
                    input_masks.blue = read_u32_le(&m_buf, 8);
                }
                BmpHeaderType::InfoV2
                | BmpHeaderType::InfoV3
                | BmpHeaderType::InfoV4
                | BmpHeaderType::InfoV5 => {
                    debug_assert!(info_bytes_remaining >= 48);
                    input_masks.red = read_u32_le(i_slice, 36);
                    input_masks.green = read_u32_le(i_slice, 40);
                    input_masks.blue = read_u32_le(i_slice, 44);

                    if header_type != BmpHeaderType::InfoV2
                        && (header_type != BmpHeaderType::InfoV3 || in_ico)
                    {
                        debug_assert!(info_bytes_remaining >= 52);
                        input_masks.alpha = read_u32_le(i_slice, 48);
                    }
                }
                BmpHeaderType::Os2Vx => return Err(CodecResult::Unimplemented),
                _ => return Err(CodecResult::InvalidInput),
            }
        }
        compression::JPEG => {
            if bits_per_pixel == 24 {
                input_format = BmpInputFormat::Rle;
            } else {
                return Err(CodecResult::Unimplemented);
            }
        }
        compression::PNG => return Err(CodecResult::Unimplemented),
        compression::CMYK | compression::CMYK_RLE_8BIT | compression::CMYK_RLE_4BIT => {
            return Err(CodecResult::Unimplemented);
        }
        _ => return Err(CodecResult::InvalidInput),
    }

    let bytes_read = BMP_HEADER_BYTES + info_bytes + mask_bytes;
    if !in_ico && offset < bytes_read {
        return Err(CodecResult::InvalidInput);
    }

    let pixel_data_offset = offset.wrapping_sub(bytes_read);

    match input_format {
        BmpInputFormat::Standard => {
            let mut alpha = if in_ico {
                EncodedAlpha::Binary
            } else {
                EncodedAlpha::Opaque
            };
            let mut is_opaque = true;

            let (color, bits_per_component) = match bits_per_pixel {
                1 | 2 | 4 | 8 => {
                    if in_ico {
                        (EncodedColor::Bgra, 8u8)
                    } else {
                        (EncodedColor::Palette, bits_per_pixel as u8)
                    }
                }
                24 => (
                    if in_ico {
                        EncodedColor::Bgra
                    } else {
                        EncodedColor::Bgr
                    },
                    8u8,
                ),
                32 => {
                    if in_ico {
                        is_opaque = false;
                        alpha = EncodedAlpha::Unpremul;
                        (EncodedColor::Bgra, 8u8)
                    } else {
                        (EncodedColor::Bgrx, 8u8)
                    }
                }
                _ => return Err(CodecResult::InvalidInput),
            };

            Ok(BmpHeaderInfo {
                input_format,
                width,
                height,
                encoded_color: color,
                encoded_alpha: alpha,
                bits_per_component,
                bits_per_pixel,
                row_order,
                num_colors,
                bytes_per_color,
                pixel_data_offset,
                is_opaque,
                input_masks,
            })
        }
        BmpInputFormat::BitMask => {
            if in_ico {
                return Err(CodecResult::InvalidInput);
            }

            if !matches!(bits_per_pixel, 16 | 24 | 32) {
                return Err(CodecResult::InvalidInput);
            }

            // Skip to the start of the pixel array (no color table in bit mask mode).
            let skip_len = pixel_data_offset as usize;
            if reader.skip(skip_len) != skip_len {
                return Err(CodecResult::IncompleteInput);
            }

            let (color, alpha) = if validate_masks {
                let masks = Masks::create(input_masks, (bits_per_pixel / 8) as i32)
                    .ok_or(CodecResult::InvalidInput)?;
                if masks.alpha.mask != 0 {
                    (EncodedColor::Bgra, EncodedAlpha::Unpremul)
                } else {
                    (EncodedColor::Bgr, EncodedAlpha::Opaque)
                }
            } else {
                (EncodedColor::Bgr, EncodedAlpha::Opaque)
            };

            Ok(BmpHeaderInfo {
                input_format,
                width,
                height,
                encoded_color: color,
                encoded_alpha: alpha,
                bits_per_component: 8,
                bits_per_pixel,
                row_order,
                num_colors,
                bytes_per_color,
                pixel_data_offset,
                is_opaque: alpha == EncodedAlpha::Opaque,
                input_masks,
            })
        }
        BmpInputFormat::Rle => {
            debug_assert!(matches!(bits_per_pixel, 4 | 8 | 24));

            if total_bytes <= offset {
                return Err(CodecResult::InvalidInput);
            }
            debug_assert!(!in_ico);

            Ok(BmpHeaderInfo {
                input_format,
                width,
                height,
                encoded_color: EncodedColor::Bgra,
                encoded_alpha: EncodedAlpha::Binary,
                bits_per_component: 8,
                bits_per_pixel,
                row_order,
                num_colors,
                bytes_per_color,
                pixel_data_offset,
                is_opaque: false,
                input_masks,
            })
        }
        BmpInputFormat::Unknown => Err(CodecResult::InvalidInput),
    }
}
