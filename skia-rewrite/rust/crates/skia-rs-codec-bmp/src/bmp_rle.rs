//! Safe Rust port of `src/codec/SkBmpRLECodec.cpp` (RLE4, RLE8, and RLE24 decoder).

use skia_rs_codec_core::{
    sampler::{compute_row_bytes, get_dst_coord, get_sampled_dimension, is_coord_necessary},
    types::{CodecResult, ScanlineOrder},
};
use skia_rs_color::{color::pmcolor_set_argb, image_info::ColorType};

use crate::{bmp_header::bmp_dst_row, bmp_standard::pack_color_argb, stream::ByteReader};

/// Size of the internal RLE stream buffer (`SkBmpRLECodec::kBufferSize`).
pub const RLE_BUFFER_SIZE: usize = 4096;

/// At most, alignedRowBytes can be 255 * 3 + 1 = 766, well below `RLE_BUFFER_SIZE`.
const _: () = assert!(255 * 3 + 1 < RLE_BUFFER_SIZE);

/// Exact port of `SkPixel32ToPixel16`.
#[inline]
pub const fn pixel32_to_pixel16(c: u32, pmcolor_is_bgra: bool) -> u16 {
    let (r, b) = if pmcolor_is_bgra {
        ((c >> 16) & 0xFF, c & 0xFF)
    } else {
        (c & 0xFF, (c >> 16) & 0xFF)
    };
    let g = (c >> 8) & 0xFF;
    (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16
}

/// Exact port of `SkPack888ToRGB16`.
#[inline]
pub const fn pack_888_to_rgb16(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3)
}

/// Exact port of `SkBmpRLECodec::fillWidth`.
#[inline]
pub const fn rle_fill_width(orig_width: i32, sample_x: i32) -> i32 {
    get_sampled_dimension(orig_width, sample_x)
}

/// Computes the F16 intermediate color-xform buffer layout matching `SkBmpRLECodec::decodeRows`
/// (`SkSafeMath` overflow-checked `(count, byte_size, row_bytes)`).
#[inline]
pub fn compute_rle_f16_xform_layout(dst_width: i32, height: i32) -> Option<(i32, usize, usize)> {
    if dst_width <= 0 || height <= 0 {
        return None;
    }
    let count = height.checked_mul(dst_width)?;
    if count <= 0 {
        return None;
    }
    let xform_bytes = (count as usize).checked_mul(4)?;
    let row_bytes = (dst_width as usize).checked_mul(4)?;
    Some((count, xform_bytes, row_bytes))
}

/// Result of consuming pending `fLinesToSkip` at the start of `decodeRows`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RleSkipPlan {
    /// Number of destination rows to advance before decoding (`0` if `fully_skipped`).
    pub rows_advanced: i32,
    /// Remaining height to decode (or original `height` to return if `fully_skipped`).
    pub remaining_height: i32,
    /// True if `height <= fLinesToSkip` and the caller should return `remaining_height` immediately.
    pub fully_skipped: bool,
}

/// Exact port of `SkBmpRLECodec::decodeRows` lines 309-320.
#[inline]
pub fn consume_rle_lines_to_skip(height: i32, lines_to_skip: &mut i32) -> RleSkipPlan {
    if height > *lines_to_skip {
        let skipped = *lines_to_skip;
        *lines_to_skip = 0;
        RleSkipPlan {
            rows_advanced: skipped,
            remaining_height: height - skipped,
            fully_skipped: false,
        }
    } else {
        *lines_to_skip -= height;
        RleSkipPlan {
            rows_advanced: 0,
            remaining_height: height,
            fully_skipped: true,
        }
    }
}

/// Exact port of `SkBmpRLECodec::createColorTable`.
///
/// On success returns `Some(max_colors)` (`1 << bits_per_pixel` for `<= 8`, or `0` for 24-bit),
/// populating `out_colors[..max_colors]`.
#[allow(clippy::too_many_arguments)]
pub fn build_rle_color_table(
    bits_per_pixel: u16,
    num_colors: u32,
    bytes_per_color: u32,
    offset: u32,
    dst_color_type: ColorType,
    has_color_xform: bool,
    pmcolor_is_bgra: bool,
    reader: &mut impl ByteReader,
    out_colors: &mut [u32; 256],
) -> Option<u32> {
    let color_table_ct = if has_color_xform {
        ColorType::Bgra8888
    } else {
        dst_color_type
    };

    let mut color_bytes: u32 = 0;
    let mut max_colors: u32 = 0;

    if bits_per_pixel <= 8 {
        max_colors = 1u32 << bits_per_pixel;
        let num_colors_to_read = if num_colors == 0 {
            max_colors
        } else {
            num_colors.min(max_colors)
        };

        color_bytes = num_colors_to_read.checked_mul(bytes_per_color)?;
        let mut c_buf = [0u8; 256 * 4];
        let c_slice = &mut c_buf[..color_bytes as usize];
        if reader.read(c_slice) != color_bytes as usize {
            return None;
        }

        let bpc = bytes_per_color as usize;
        for i in 0..(num_colors_to_read as usize) {
            let base = i * bpc;
            let blue = c_slice[base];
            let green = c_slice[base + 1];
            let red = c_slice[base + 2];
            out_colors[i] =
                pack_color_argb(0xFF, red, green, blue, false, color_table_ct, pmcolor_is_bgra);
        }

        // Fill remainder with opaque black (`SkPackARGB32(0xFF, 0, 0, 0)`).
        for slot in &mut out_colors[(num_colors_to_read as usize)..(max_colors as usize)] {
            *slot = 0xFF00_0000;
        }
    }

    if offset < color_bytes {
        return None;
    }

    let skip_bytes = (offset - color_bytes) as usize;
    if reader.skip(skip_bytes) != skip_bytes {
        return None;
    }

    Some(max_colors)
}

/// Mutable cursor over `SkBmpRLECodec`'s streaming buffer and scanline skip state.
pub struct BmpRleCursor<'a> {
    pub stream_buffer: &'a mut [u8; RLE_BUFFER_SIZE],
    pub bytes_buffered: &'a mut usize,
    pub curr_rle_byte: &'a mut u32,
    pub lines_to_skip: &'a mut i32,
}

impl BmpRleCursor<'_> {
    /// Exact port of `SkBmpRLECodec::initializeStreamBuffer`.
    pub fn initialize_stream_buffer(&mut self, reader: &mut impl ByteReader) -> bool {
        *self.bytes_buffered = reader.read(self.stream_buffer);
        if *self.bytes_buffered == 0 {
            return false;
        }
        *self.curr_rle_byte = 0;
        true
    }

    /// Exact port of `SkBmpRLECodec::checkForMoreData`.
    pub fn check_for_more_data(&mut self, reader: &mut impl ByteReader) -> usize {
        let curr = *self.curr_rle_byte as usize;
        let buffered = *self.bytes_buffered;
        let remaining = buffered.saturating_sub(curr);

        if remaining > 0 && curr > 0 {
            self.stream_buffer.copy_within(curr..buffered, 0);
        }

        let to_read = curr.min(RLE_BUFFER_SIZE - remaining);
        let additional = reader.read(&mut self.stream_buffer[remaining..remaining + to_read]);

        *self.curr_rle_byte = 0;
        *self.bytes_buffered = remaining + additional;
        *self.bytes_buffered
    }

    #[inline]
    fn available(&self) -> usize {
        self.bytes_buffered.saturating_sub(*self.curr_rle_byte as usize)
    }

    #[inline]
    fn next_byte(&mut self) -> u8 {
        let idx = *self.curr_rle_byte as usize;
        *self.curr_rle_byte += 1;
        self.stream_buffer[idx]
    }
}

/// Writes an indexed RLE pixel matching `SkBmpRLECodec::setPixel`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn set_rle_pixel(
    dst: Option<&mut [u8]>,
    dst_row_bytes: usize,
    dst_width: i32,
    dst_height: i32,
    dst_color_type: ColorType,
    row_order: ScanlineOrder,
    sample_x: i32,
    color_table: &[u32],
    pmcolor_is_bgra: bool,
    x: i32,
    y: i32,
    index: u8,
) {
    let Some(dst) = dst else {
        return;
    };
    if !is_coord_necessary(x, sample_x, dst_width) {
        return;
    }
    let row = bmp_dst_row(row_order, y, dst_height) as usize;
    let dst_x = get_dst_coord(x, sample_x) as usize;
    let color = color_table[index as usize];

    match dst_color_type {
        ColorType::Rgba8888 | ColorType::Bgra8888 => {
            let off = row * dst_row_bytes + dst_x * 4;
            dst[off..off + 4].copy_from_slice(&color.to_ne_bytes());
        }
        ColorType::Rgb565 => {
            let off = row * dst_row_bytes + dst_x * 2;
            let px16 = pixel32_to_pixel16(color, pmcolor_is_bgra);
            dst[off..off + 2].copy_from_slice(&px16.to_ne_bytes());
        }
        _ => {
            debug_assert!(false, "unexpected RLE destination color type");
        }
    }
}

/// Writes an RGB RLE24 pixel matching `SkBmpRLECodec::setRGBPixel`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn set_rle_rgb_pixel(
    dst: Option<&mut [u8]>,
    dst_row_bytes: usize,
    dst_width: i32,
    dst_height: i32,
    dst_color_type: ColorType,
    row_order: ScanlineOrder,
    sample_x: i32,
    x: i32,
    y: i32,
    red: u8,
    green: u8,
    blue: u8,
) {
    let Some(dst) = dst else {
        return;
    };
    if !is_coord_necessary(x, sample_x, dst_width) {
        return;
    }
    let row = bmp_dst_row(row_order, y, dst_height) as usize;
    let dst_x = get_dst_coord(x, sample_x) as usize;

    match dst_color_type {
        ColorType::Rgba8888 => {
            let off = row * dst_row_bytes + dst_x * 4;
            let px = pmcolor_set_argb(0xFF, red as u32, green as u32, blue as u32, false);
            dst[off..off + 4].copy_from_slice(&px.to_ne_bytes());
        }
        ColorType::Bgra8888 => {
            let off = row * dst_row_bytes + dst_x * 4;
            let px = pmcolor_set_argb(0xFF, red as u32, green as u32, blue as u32, true);
            dst[off..off + 4].copy_from_slice(&px.to_ne_bytes());
        }
        ColorType::Rgb565 => {
            let off = row * dst_row_bytes + dst_x * 2;
            let px16 = pack_888_to_rgb16(red, green, blue);
            dst[off..off + 2].copy_from_slice(&px16.to_ne_bytes());
        }
        _ => {
            debug_assert!(false, "unexpected RLE24 destination color type");
        }
    }
}

/// Exact port of `SkBmpRLECodec::decodeRLE`.
///
/// Decodes up to `dst_height` rows of RLE4, RLE8, or RLE24 data into `dst` (or skips
/// them when `dst` is `None`). Returns the number of rows decoded.
#[allow(clippy::too_many_arguments)]
pub fn decode_rle(
    cursor: &mut BmpRleCursor<'_>,
    reader: &mut impl ByteReader,
    orig_width: i32,
    bits_per_pixel: u16,
    row_order: ScanlineOrder,
    sample_x: i32,
    color_table: &[u32],
    pmcolor_is_bgra: bool,
    mut dst: Option<&mut [u8]>,
    dst_width: i32,
    dst_height: i32,
    dst_row_bytes: usize,
    dst_color_type: ColorType,
) -> i32 {
    const RLE_ESCAPE: u8 = 0;
    const RLE_EOL: u8 = 0;
    const RLE_EOF: u8 = 1;
    const RLE_DELTA: u8 = 2;

    let width = orig_width;
    let height = dst_height;

    let mut x: i32 = 0;
    let mut y: i32 = 0;

    loop {
        if y >= height {
            return height;
        }

        if cursor.available() < 2 && cursor.check_for_more_data(reader) < 2 {
            return y;
        }

        let flag = cursor.next_byte();
        let task = cursor.next_byte();

        if flag == RLE_ESCAPE {
            match task {
                RLE_EOL => {
                    x = 0;
                    y += 1;
                }
                RLE_EOF => {
                    return height;
                }
                RLE_DELTA => {
                    if cursor.available() < 2 && cursor.check_for_more_data(reader) < 2 {
                        return y;
                    }
                    let dx = i32::from(cursor.next_byte());
                    let dy = i32::from(cursor.next_byte());
                    x += dx;
                    y += dy;
                    if x > width {
                        return y - dy;
                    } else if y > height {
                        *cursor.lines_to_skip = y - height;
                        return height;
                    }
                }
                _ => {
                    // Absolute (uncompressed) run of `task` pixels.
                    let mut num_pixels = task;
                    let row_bytes = compute_row_bytes(i32::from(num_pixels), u32::from(bits_per_pixel));
                    let aligned_row_bytes = (row_bytes + 1) & !1;
                    if cursor.available() < aligned_row_bytes
                        && cursor.check_for_more_data(reader) < aligned_row_bytes
                    {
                        return y;
                    }

                    while num_pixels > 0 && x < width {
                        match bits_per_pixel {
                            4 => {
                                let val = cursor.next_byte();
                                set_rle_pixel(
                                    dst.as_deref_mut(),
                                    dst_row_bytes,
                                    dst_width,
                                    dst_height,
                                    dst_color_type,
                                    row_order,
                                    sample_x,
                                    color_table,
                                    pmcolor_is_bgra,
                                    x,
                                    y,
                                    val >> 4,
                                );
                                x += 1;
                                num_pixels -= 1;
                                if num_pixels != 0 {
                                    set_rle_pixel(
                                        dst.as_deref_mut(),
                                        dst_row_bytes,
                                        dst_width,
                                        dst_height,
                                        dst_color_type,
                                        row_order,
                                        sample_x,
                                        color_table,
                                        pmcolor_is_bgra,
                                        x,
                                        y,
                                        val & 0x0F,
                                    );
                                    x += 1;
                                    num_pixels -= 1;
                                }
                            }
                            8 => {
                                let idx = cursor.next_byte();
                                set_rle_pixel(
                                    dst.as_deref_mut(),
                                    dst_row_bytes,
                                    dst_width,
                                    dst_height,
                                    dst_color_type,
                                    row_order,
                                    sample_x,
                                    color_table,
                                    pmcolor_is_bgra,
                                    x,
                                    y,
                                    idx,
                                );
                                x += 1;
                                num_pixels -= 1;
                            }
                            24 => {
                                let blue = cursor.next_byte();
                                let green = cursor.next_byte();
                                let red = cursor.next_byte();
                                set_rle_rgb_pixel(
                                    dst.as_deref_mut(),
                                    dst_row_bytes,
                                    dst_width,
                                    dst_height,
                                    dst_color_type,
                                    row_order,
                                    sample_x,
                                    x,
                                    y,
                                    red,
                                    green,
                                    blue,
                                );
                                x += 1;
                                num_pixels -= 1;
                            }
                            _ => {
                                debug_assert!(false, "invalid RLE bpp");
                                return y;
                            }
                        }
                    }

                    // Skip 1 padding byte if row_bytes is odd to preserve 16-bit alignment.
                    if (row_bytes & 1) != 0 {
                        *cursor.curr_rle_byte += 1;
                    }
                }
            }
        } else {
            // Encoded run of `flag` pixels.
            let num_pixels = i32::from(flag);
            let end_x = (x + num_pixels).min(width);

            if bits_per_pixel == 24 {
                if cursor.available() < 2 && cursor.check_for_more_data(reader) < 2 {
                    return y;
                }
                let blue = task;
                let green = cursor.next_byte();
                let red = cursor.next_byte();
                while x < end_x {
                    set_rle_rgb_pixel(
                        dst.as_deref_mut(),
                        dst_row_bytes,
                        dst_width,
                        dst_height,
                        dst_color_type,
                        row_order,
                        sample_x,
                        x,
                        y,
                        red,
                        green,
                        blue,
                    );
                    x += 1;
                }
            } else {
                let indices = if bits_per_pixel == 4 {
                    [task >> 4, task & 0x0F]
                } else {
                    [task, task]
                };
                let mut which = 0usize;
                while x < end_x {
                    set_rle_pixel(
                        dst.as_deref_mut(),
                        dst_row_bytes,
                        dst_width,
                        dst_height,
                        dst_color_type,
                        row_order,
                        sample_x,
                        color_table,
                        pmcolor_is_bgra,
                        x,
                        y,
                        indices[which],
                    );
                    which ^= 1;
                    x += 1;
                }
            }
        }
    }
}

/// Owned convenience wrapper for pure-Rust BMP RLE decoding and unit testing.
pub struct BmpRleDecoder {
    pub width: i32,
    pub bits_per_pixel: u16,
    pub num_colors: u32,
    pub bytes_per_color: u32,
    pub offset: u32,
    pub row_order: ScanlineOrder,
    pub color_table: [u32; 256],
    pub color_count: usize,
    pub stream_buffer: [u8; RLE_BUFFER_SIZE],
    pub bytes_buffered: usize,
    pub curr_rle_byte: u32,
    pub sample_x: i32,
    pub lines_to_skip: i32,
}

impl BmpRleDecoder {
    pub fn new(
        width: i32,
        bits_per_pixel: u16,
        num_colors: u32,
        bytes_per_color: u32,
        offset: u32,
        row_order: ScanlineOrder,
    ) -> Self {
        Self {
            width,
            bits_per_pixel,
            num_colors,
            bytes_per_color,
            offset,
            row_order,
            color_table: [0u32; 256],
            color_count: 0,
            stream_buffer: [0u8; RLE_BUFFER_SIZE],
            bytes_buffered: 0,
            curr_rle_byte: 0,
            sample_x: 1,
            lines_to_skip: 0,
        }
    }

    pub fn prepare(
        &mut self,
        has_subset: bool,
        dst_color_type: ColorType,
        has_color_xform: bool,
        pmcolor_is_bgra: bool,
        reader: &mut impl ByteReader,
    ) -> CodecResult {
        if has_subset {
            return CodecResult::Unimplemented;
        }
        self.sample_x = 1;
        self.lines_to_skip = 0;

        let Some(count) = build_rle_color_table(
            self.bits_per_pixel,
            self.num_colors,
            self.bytes_per_color,
            self.offset,
            dst_color_type,
            has_color_xform,
            pmcolor_is_bgra,
            reader,
            &mut self.color_table,
        ) else {
            return CodecResult::InvalidInput;
        };
        self.color_count = count as usize;

        let mut cursor = BmpRleCursor {
            stream_buffer: &mut self.stream_buffer,
            bytes_buffered: &mut self.bytes_buffered,
            curr_rle_byte: &mut self.curr_rle_byte,
            lines_to_skip: &mut self.lines_to_skip,
        };
        if !cursor.initialize_stream_buffer(reader) {
            return CodecResult::InvalidInput;
        }
        CodecResult::Success
    }

    pub fn set_sample_x(&mut self, sample_x: i32) -> i32 {
        self.sample_x = sample_x;
        self.fill_width()
    }

    pub fn fill_width(&self) -> i32 {
        rle_fill_width(self.width, self.sample_x)
    }

    pub fn decode_rows(
        &mut self,
        reader: &mut impl ByteReader,
        mut dst: Option<&mut [u8]>,
        height: i32,
        dst_row_bytes: usize,
        dst_color_type: ColorType,
        pmcolor_is_bgra: bool,
    ) -> i32 {
        let plan = consume_rle_lines_to_skip(height, &mut self.lines_to_skip);
        if plan.fully_skipped {
            return plan.remaining_height;
        }
        if plan.rows_advanced > 0 {
            if let Some(buf) = dst.as_mut() {
                let offset = (plan.rows_advanced as usize) * dst_row_bytes;
                let moved = std::mem::take(buf);
                *buf = &mut moved[offset..];
            }
        }
        let dst_width = self.fill_width();
        let color_table = &self.color_table[..self.color_count];
        let mut cursor = BmpRleCursor {
            stream_buffer: &mut self.stream_buffer,
            bytes_buffered: &mut self.bytes_buffered,
            curr_rle_byte: &mut self.curr_rle_byte,
            lines_to_skip: &mut self.lines_to_skip,
        };
        decode_rle(
            &mut cursor,
            reader,
            self.width,
            self.bits_per_pixel,
            self.row_order,
            self.sample_x,
            color_table,
            pmcolor_is_bgra,
            dst,
            dst_width,
            plan.remaining_height,
            dst_row_bytes,
            dst_color_type,
        )
    }
}
