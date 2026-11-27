//! Safe Rust port of `src/codec/SkPngCodec.cpp`:
//! IHDR/tRNS/sBIT decode transformation planning, `SkPngNormalDecoder` state machine,
//! and `SkPngInterlacedDecoder` pass/row/buffer state machine.

use skia_rs_codec_core::types::{CodecResult, EncodedAlpha, EncodedColor};

/// Matches `PNG_COLOR_TYPE_GRAY` (`0`).
pub const PNG_COLOR_TYPE_GRAY: i32 = 0;
/// Matches `PNG_COLOR_TYPE_RGB` (`2`).
pub const PNG_COLOR_TYPE_RGB: i32 = 2;
/// Matches `PNG_COLOR_TYPE_PALETTE` (`3`).
pub const PNG_COLOR_TYPE_PALETTE: i32 = 3;
/// Matches `PNG_COLOR_TYPE_GRAY_ALPHA` (`4`).
pub const PNG_COLOR_TYPE_GRAY_ALPHA: i32 = 4;
/// Matches `PNG_COLOR_TYPE_RGBA` (`6`).
pub const PNG_COLOR_TYPE_RGBA: i32 = 6;

/// Matches `kGraySigBit_GrayAlphaIsJustAlpha` in `src/codec/SkPngPriv.h`.
pub const GRAY_SIG_BIT_GRAY_ALPHA_IS_JUST_ALPHA: u8 = 1;

/// Exact port of `SkCodecPriv::GetSampledDimension`.
#[inline]
pub const fn get_sampled_dimension(src_dimension: i32, sample_size: i32) -> i32 {
    if sample_size > src_dimension {
        1
    } else if sample_size == 0 {
        0
    } else {
        src_dimension / sample_size
    }
}

/// Exact port of `SkCodecPriv::GetStartCoord`.
#[inline]
pub const fn get_start_coord(sample_factor: i32) -> i32 {
    sample_factor / 2
}

/// Exact port of `log_and_return_error` result mapping in `SkPngCodec.cpp`.
#[inline]
pub const fn decode_error_result(process_succeeded: bool) -> CodecResult {
    if process_succeeded {
        CodecResult::IncompleteInput
    } else {
        CodecResult::ErrorInInput
    }
}

/// Decode transformation plan computed from PNG `IHDR`, `tRNS`, and `sBIT` chunks.
/// Exact port of `AutoCleanPng::infoCallback` in `SkPngCodec.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PngDecodeHeaderPlan {
    pub strip_16: bool,
    pub set_packing: bool,
    pub expand_gray_1_2_4_to_8: bool,
    pub trns_to_alpha: bool,
    pub output_bit_depth: i32,
    /// Color used to check `isCompatibleColorProfileAndType` (before sBIT overrides).
    pub profile_check_color: EncodedColor,
    /// Final `SkEncodedInfo::Color` after sBIT overrides (e.g. `XAlpha`, `Rgb565`).
    pub final_encoded_color: EncodedColor,
    pub encoded_alpha: EncodedAlpha,
    /// True if Android SafetyNet 565 sBIT log condition matches.
    pub android_565_sbit_warning: bool,
}

/// Exact port of `AutoCleanPng::infoCallback` color-type, bit-depth, tRNS, and sBIT planning.
#[allow(clippy::too_many_arguments)]
pub const fn plan_decode_header(
    encoded_color_type: i32,
    bit_depth: i32,
    has_trns: bool,
    has_sbit: bool,
    sbit_red: u8,
    sbit_green: u8,
    sbit_blue: u8,
    sbit_gray: u8,
    sbit_alpha: u8,
) -> PngDecodeHeaderPlan {
    let mut out_bit_depth = bit_depth;
    let mut strip_16 = false;
    let mut set_packing = false;
    let mut expand_gray_1_2_4_to_8 = false;
    let mut trns_to_alpha = false;

    if out_bit_depth == 16
        && (encoded_color_type == PNG_COLOR_TYPE_GRAY
            || encoded_color_type == PNG_COLOR_TYPE_GRAY_ALPHA)
    {
        out_bit_depth = 8;
        strip_16 = true;
    }

    let (profile_check_color, encoded_alpha) = match encoded_color_type {
        PNG_COLOR_TYPE_PALETTE => {
            if out_bit_depth < 8 {
                out_bit_depth = 8;
                set_packing = true;
            }
            let alpha = if has_trns {
                EncodedAlpha::Unpremul
            } else {
                EncodedAlpha::Opaque
            };
            (EncodedColor::Palette, alpha)
        }
        PNG_COLOR_TYPE_RGB => {
            if has_trns {
                trns_to_alpha = true;
                (EncodedColor::Rgba, EncodedAlpha::Binary)
            } else {
                (EncodedColor::Rgb, EncodedAlpha::Opaque)
            }
        }
        PNG_COLOR_TYPE_GRAY => {
            if out_bit_depth < 8 {
                out_bit_depth = 8;
                expand_gray_1_2_4_to_8 = true;
            }
            if has_trns {
                trns_to_alpha = true;
                (EncodedColor::GrayAlpha, EncodedAlpha::Binary)
            } else {
                (EncodedColor::Gray, EncodedAlpha::Opaque)
            }
        }
        PNG_COLOR_TYPE_GRAY_ALPHA => (EncodedColor::GrayAlpha, EncodedAlpha::Unpremul),
        PNG_COLOR_TYPE_RGBA => (EncodedColor::Rgba, EncodedAlpha::Unpremul),
        _ => (EncodedColor::Rgba, EncodedAlpha::Unpremul),
    };

    let mut final_encoded_color = profile_check_color;
    if has_sbit {
        match encoded_color_type {
            PNG_COLOR_TYPE_GRAY_ALPHA => {
                if sbit_alpha == 8 && sbit_gray == GRAY_SIG_BIT_GRAY_ALPHA_IS_JUST_ALPHA {
                    final_encoded_color = EncodedColor::XAlpha;
                }
            }
            PNG_COLOR_TYPE_RGB => {
                if sbit_red == 5 && sbit_green == 6 && sbit_blue == 5 {
                    final_encoded_color = EncodedColor::Rgb565;
                }
            }
            _ => {}
        }
    }

    let android_565_sbit_warning = encoded_color_type != PNG_COLOR_TYPE_GRAY_ALPHA
        && matches!(encoded_alpha, EncodedAlpha::Opaque)
        && has_sbit
        && sbit_red == 5
        && sbit_green == 6
        && sbit_blue == 5;

    PngDecodeHeaderPlan {
        strip_16,
        set_packing,
        expand_gray_1_2_4_to_8,
        trns_to_alpha,
        output_bit_depth: out_bit_depth,
        profile_check_color,
        final_encoded_color,
        encoded_alpha,
        android_565_sbit_warning,
    }
}

/// Action returned by `NormalDecodeState::on_row`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NormalRowAction {
    /// Ignore this row and continue decoding.
    Ignore = 0,
    /// Transform this row into dst, advance dst, and continue decoding.
    TransformAndContinue = 1,
    /// Transform this row into dst, advance dst, and stop decoding (`longjmp(kStopDecoding)`).
    TransformAndStop = 2,
    /// Do not transform this row, but stop decoding (`rows_written == rows_needed`).
    Stop = 3,
}

/// Safe Rust state machine for `SkPngNormalDecoder`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct NormalDecodeState {
    pub rows_written: i32,
    pub first_row: i32,
    pub last_row: i32,
    pub rows_needed: i32,
}

impl NormalDecodeState {
    #[inline]
    pub const fn new() -> Self {
        Self {
            rows_written: 0,
            first_row: 0,
            last_row: 0,
            rows_needed: 0,
        }
    }

    /// Initializes state for `SkPngNormalDecoder::decodeAllRows`.
    #[inline]
    pub fn start_all_rows(&mut self, height: i32) {
        self.rows_written = 0;
        self.first_row = 0;
        self.last_row = height - 1;
        self.rows_needed = height;
    }

    /// Handles a row callback during `decodeAllRows`.
    #[inline]
    pub fn on_all_rows_callback(&mut self, _row_num: i32) {
        self.rows_written += 1;
    }

    /// Finalizes `decodeAllRows`, returning `(result, rows_written)`.
    #[inline]
    pub const fn finish_all_rows(&self, process_succeeded: bool, height: i32) -> (CodecResult, i32) {
        if process_succeeded && self.rows_written == height {
            (CodecResult::Success, self.rows_written)
        } else {
            (decode_error_result(process_succeeded), self.rows_written)
        }
    }

    /// Initializes state for `SkPngNormalDecoder::setRange`.
    #[inline]
    pub fn set_range(&mut self, first_row: i32, last_row: i32) {
        self.first_row = first_row;
        self.last_row = last_row;
        self.rows_written = 0;
        self.rows_needed = last_row - first_row + 1;
    }

    /// Updates `rows_needed` before `processData()` in `SkPngNormalDecoder::decode`.
    #[inline]
    pub fn prepare_decode(&mut self, sample_y: Option<i32>) {
        if let Some(sy) = sample_y {
            self.rows_needed = get_sampled_dimension(self.last_row - self.first_row + 1, sy);
        }
    }

    /// Returns `Some(row_num - first_row)` if `row_num >= first_row`, else `None`.
    #[inline]
    pub const fn relative_row(&self, row_num: i32) -> Option<i32> {
        if row_num < self.first_row {
            None
        } else {
            Some(row_num - self.first_row)
        }
    }

    /// Processes a row callback during incremental/subset `decode()`.
    #[inline]
    pub fn on_row_callback(&mut self, row_num: i32, row_needed: bool) -> NormalRowAction {
        if row_num < self.first_row {
            return NormalRowAction::Ignore;
        }
        if row_needed {
            self.rows_written += 1;
            if self.rows_written == self.rows_needed {
                NormalRowAction::TransformAndStop
            } else {
                NormalRowAction::TransformAndContinue
            }
        } else if self.rows_written == self.rows_needed {
            NormalRowAction::Stop
        } else {
            NormalRowAction::Ignore
        }
    }

    /// Finalizes `decode()`, returning `(result, rows_written)`.
    #[inline]
    pub const fn finish_decode(&self, process_succeeded: bool) -> (CodecResult, i32) {
        if process_succeeded && self.rows_written == self.rows_needed {
            (CodecResult::Success, self.rows_written)
        } else {
            (decode_error_result(process_succeeded), self.rows_written)
        }
    }
}

/// Safe Rust state machine for `SkPngInterlacedDecoder`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct InterlacedDecodeState {
    pub number_passes: i32,
    pub image_height: i32,
    pub first_row: i32,
    pub last_row: i32,
    pub lines_decoded: i32,
    pub interlaced_complete: bool,
    pub png_rowbytes: usize,
}

/// Action returned by `InterlacedDecodeState::on_row`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InterlacedRowAction {
    pub ignore: bool,
    pub buffer_offset: usize,
    pub stop_decoding: bool,
}

/// Output row transform schedule for `SkPngInterlacedDecoder::decode`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InterlacedOutputSchedule {
    pub has_lines: bool,
    pub start_src_row: i32,
    pub step: i32,
    pub rows_to_write: i32,
}

impl InterlacedDecodeState {
    #[inline]
    pub const fn new(number_passes: i32, image_height: i32) -> Self {
        Self {
            number_passes,
            image_height,
            first_row: 0,
            last_row: 0,
            lines_decoded: 0,
            interlaced_complete: false,
            png_rowbytes: 0,
        }
    }

    /// Exact port of `SkPngInterlacedDecoder::setUpInterlaceBuffer` size overflow check.
    #[inline]
    pub fn compute_buffer_size(png_rowbytes: usize, height: i32) -> Option<usize> {
        let h = usize::try_from(height).ok()?;
        png_rowbytes.checked_mul(h)
    }

    /// Resets per-decode interlace tracking state after buffer allocation succeeds.
    #[inline]
    pub fn reset_range(&mut self, first_row: i32, last_row: i32, png_rowbytes: usize) {
        self.first_row = first_row;
        self.last_row = last_row;
        self.lines_decoded = 0;
        self.interlaced_complete = false;
        self.png_rowbytes = png_rowbytes;
    }

    /// Exact port of `SkPngInterlacedDecoder::interlacedRowCallback`.
    ///
    /// When `ignore == false`, the caller must invoke `png_progressive_combine_row`
    /// at `buffer + buffer_offset`, and then if `stop_decoding == true` perform
    /// `longjmp(kStopDecoding)`.
    #[inline]
    pub fn on_row(&mut self, row_num: i32, pass: i32, sample_y: i32) -> InterlacedRowAction {
        if row_num < self.first_row || row_num > self.last_row || self.interlaced_complete {
            return InterlacedRowAction {
                ignore: true,
                buffer_offset: 0,
                stop_decoding: false,
            };
        }

        let buffer_offset = ((row_num - self.first_row) as usize) * self.png_rowbytes;
        let mut stop_decoding = false;

        if pass == 0 {
            self.lines_decoded += 1;
        } else if self.number_passes - 1 == pass && row_num == self.last_row {
            self.interlaced_complete = true;
            if self.last_row != self.image_height - 1 || sample_y != 1 {
                stop_decoding = true;
            }
        }

        InterlacedRowAction {
            ignore: false,
            buffer_offset,
            stop_decoding,
        }
    }

    /// Computes the output row schedule for `SkPngInterlacedDecoder::decode`.
    #[inline]
    pub fn output_schedule(&self, sample_y: i32) -> InterlacedOutputSchedule {
        if self.lines_decoded <= 0 {
            return InterlacedOutputSchedule {
                has_lines: false,
                start_src_row: 0,
                step: sample_y,
                rows_to_write: 0,
            };
        }

        let rows_needed = get_sampled_dimension(self.last_row - self.first_row + 1, sample_y);
        let start_src_row = get_start_coord(sample_y);
        let mut src_row = start_src_row;
        let mut rows_to_write = 0;
        while rows_to_write < rows_needed && src_row < self.lines_decoded {
            rows_to_write += 1;
            src_row += sample_y;
        }

        InterlacedOutputSchedule {
            has_lines: true,
            start_src_row,
            step: sample_y,
            rows_to_write,
        }
    }

    /// Finalizes `decodeAllRows` or `decode`, returning `(result, rows_written)`.
    #[inline]
    pub const fn finish(&self, process_succeeded: bool, rows_written: i32) -> (CodecResult, i32) {
        if process_succeeded && self.interlaced_complete {
            (CodecResult::Success, rows_written)
        } else {
            (decode_error_result(process_succeeded), rows_written)
        }
    }
}
