//! Safe Rust port of `src/codec/SkJpegSegmentScan.cpp` and `src/codec/SkJpegSegmentScan.h`.
//!
//! Pure finite-state JPEG marker/segment scanner handling SOI, EOI, SOS, APP0..APP15,
//! standalone markers (TEM, RST0..RST7, SOI, EOI), `0xFF 0x00` byte stuffing, and
//! `0xFF` fill bytes.

use crate::constants::{
    MARKER_CODE_SIZE, MARKER_END_OF_IMAGE, MARKER_START_OF_IMAGE, SEGMENT_PARAMETER_LENGTH_SIZE,
};

/// Mirrors `struct SkJpegSegment` layout exactly.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct JpegSegment {
    /// Offset in bytes from the start of the JPEG stream to the `0xFF` byte of this marker.
    pub offset: usize,
    /// Second byte of the two-byte marker code.
    pub marker: u8,
    /// Total parameter length including the two length bytes (0 for standalone/stop markers).
    pub parameter_length: u16,
}

/// Mirrors `enum class SkJpegSegmentScanner::State`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ScannerState {
    #[default]
    StartOfImageByte0 = 0,
    StartOfImageByte1 = 1,
    SecondMarkerByte0 = 2,
    SecondMarkerByte1 = 3,
    SegmentParamLengthByte0 = 4,
    SegmentParamLengthByte1 = 5,
    SegmentParam = 6,
    EntropyCodedData = 7,
    EntropyCodedDataSentinel = 8,
    PostEntropyCodedDataFill = 9,
    Done = 10,
    Error = 11,
}

impl ScannerState {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::StartOfImageByte0),
            1 => Some(Self::StartOfImageByte1),
            2 => Some(Self::SecondMarkerByte0),
            3 => Some(Self::SecondMarkerByte1),
            4 => Some(Self::SegmentParamLengthByte0),
            5 => Some(Self::SegmentParamLengthByte1),
            6 => Some(Self::SegmentParam),
            7 => Some(Self::EntropyCodedData),
            8 => Some(Self::EntropyCodedDataSentinel),
            9 => Some(Self::PostEntropyCodedDataFill),
            10 => Some(Self::Done),
            11 => Some(Self::Error),
            _ => None,
        }
    }
}

/// Mirrors all scalar fields of `SkJpegSegmentScanner` for zero-overhead FFI bridging.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SegmentScannerState {
    pub state: i32,
    pub stop_marker: u8,
    pub segment_param_length_byte0: u8,
    pub current_segment_marker: u8,
    pub _pad: u8,
    pub offset: usize,
    pub segment_param_bytes_remaining: usize,
    pub current_segment_offset: usize,
}

impl SegmentScannerState {
    #[inline]
    pub const fn new(stop_marker: u8) -> Self {
        Self {
            state: ScannerState::StartOfImageByte0 as i32,
            stop_marker,
            segment_param_length_byte0: 0,
            current_segment_marker: 0,
            _pad: 0,
            offset: 0,
            segment_param_bytes_remaining: 0,
            current_segment_offset: 0,
        }
    }

    #[inline]
    pub const fn state_enum(&self) -> ScannerState {
        match ScannerState::from_i32(self.state) {
            Some(s) => s,
            None => ScannerState::Error,
        }
    }

    #[inline]
    pub const fn is_done(&self) -> bool {
        self.state == ScannerState::Done as i32
    }

    #[inline]
    pub const fn had_error(&self) -> bool {
        self.state == ScannerState::Error as i32
    }

    #[inline]
    fn set_state(&mut self, s: ScannerState) {
        self.state = s as i32;
    }

    #[inline]
    fn save_current_segment(&mut self, length: u16, emit: &mut impl FnMut(JpegSegment)) {
        emit(JpegSegment {
            offset: self.current_segment_offset,
            marker: self.current_segment_marker,
            parameter_length: length,
        });
        self.current_segment_marker = 0;
        self.current_segment_offset = 0;
    }

    #[inline]
    pub fn on_marker_second_byte(&mut self, byte: u8, emit: &mut impl FnMut(JpegSegment)) {
        self.current_segment_marker = byte;
        self.current_segment_offset = self.offset.wrapping_sub(1);

        if byte == self.stop_marker {
            self.save_current_segment(0, emit);
            self.set_state(ScannerState::Done);
        } else if byte == MARKER_START_OF_IMAGE {
            self.save_current_segment(0, emit);
            self.set_state(ScannerState::SecondMarkerByte0);
        } else if marker_stands_alone(byte) {
            self.save_current_segment(0, emit);
            self.set_state(ScannerState::EntropyCodedData);
        } else {
            self.current_segment_marker = byte;
            self.set_state(ScannerState::SegmentParamLengthByte0);
        }
    }

    /// Process a single byte, matching `SkJpegSegmentScanner::onByte`.
    pub fn on_byte(&mut self, byte: u8, emit: &mut impl FnMut(JpegSegment)) {
        match self.state_enum() {
            ScannerState::StartOfImageByte0 => {
                if byte != 0xFF {
                    self.set_state(ScannerState::Error);
                    return;
                }
                self.set_state(ScannerState::StartOfImageByte1);
            }
            ScannerState::StartOfImageByte1 => {
                if byte != MARKER_START_OF_IMAGE {
                    self.set_state(ScannerState::Error);
                    return;
                }
                self.on_marker_second_byte(byte, emit);
            }
            ScannerState::SecondMarkerByte0 => {
                if byte != 0xFF {
                    self.set_state(ScannerState::Error);
                    return;
                }
                self.set_state(ScannerState::SecondMarkerByte1);
            }
            ScannerState::SecondMarkerByte1 => {
                // ITU-T T.81 section B.1.1.3: marker second byte cannot be 0x00 or 0xFF.
                if byte == 0xFF || byte == 0x00 {
                    self.set_state(ScannerState::Error);
                    return;
                }
                self.on_marker_second_byte(byte, emit);
            }
            ScannerState::SegmentParamLengthByte0 => {
                self.segment_param_length_byte0 = byte;
                self.set_state(ScannerState::SegmentParamLengthByte1);
            }
            ScannerState::SegmentParamLengthByte1 => {
                let param_length =
                    ((self.segment_param_length_byte0 as u16) << 8) | (byte as u16);
                self.segment_param_length_byte0 = 0;

                if (param_length as usize) < SEGMENT_PARAMETER_LENGTH_SIZE {
                    self.set_state(ScannerState::Error);
                    return;
                }
                self.save_current_segment(param_length, emit);
                self.segment_param_bytes_remaining =
                    (param_length as usize) - SEGMENT_PARAMETER_LENGTH_SIZE;
                if self.segment_param_bytes_remaining > 0 {
                    self.set_state(ScannerState::SegmentParam);
                } else {
                    self.set_state(ScannerState::EntropyCodedData);
                }
            }
            ScannerState::SegmentParam => {
                debug_assert!(self.segment_param_bytes_remaining > 0);
                self.segment_param_bytes_remaining =
                    self.segment_param_bytes_remaining.saturating_sub(1);
                if self.segment_param_bytes_remaining == 0 {
                    self.set_state(ScannerState::EntropyCodedData);
                }
            }
            ScannerState::EntropyCodedData => {
                if byte == 0xFF {
                    self.set_state(ScannerState::EntropyCodedDataSentinel);
                }
            }
            ScannerState::EntropyCodedDataSentinel => {
                if byte == 0x00 {
                    self.set_state(ScannerState::EntropyCodedData);
                } else if byte == 0xFF {
                    self.set_state(ScannerState::PostEntropyCodedDataFill);
                } else {
                    self.on_marker_second_byte(byte, emit);
                }
            }
            ScannerState::PostEntropyCodedDataFill => {
                if byte == 0xFF {
                    self.set_state(ScannerState::PostEntropyCodedDataFill);
                } else if byte == 0x00 {
                    self.set_state(ScannerState::Error);
                    return;
                } else {
                    self.on_marker_second_byte(byte, emit);
                }
            }
            ScannerState::Done | ScannerState::Error => {}
        }
    }

    /// Process a chunk of bytes with fast-path skipping for segment payloads and
    /// entropy-coded data, matching `SkJpegSegmentScanner::onBytes`.
    pub fn on_bytes(&mut self, data: &[u8], emit: &mut impl FnMut(JpegSegment)) {
        let mut idx = 0usize;
        while idx < data.len() {
            let remaining = &data[idx..];
            let bytes_to_move_forward = match self.state_enum() {
                ScannerState::SegmentParam => {
                    debug_assert!(self.segment_param_bytes_remaining > 0);
                    let skip = self.segment_param_bytes_remaining.min(remaining.len());
                    self.segment_param_bytes_remaining -= skip;
                    if self.segment_param_bytes_remaining == 0 {
                        self.set_state(ScannerState::EntropyCodedData);
                    }
                    skip
                }
                ScannerState::EntropyCodedData => {
                    if let Some(pos) = remaining.iter().position(|&b| b == 0xFF) {
                        self.set_state(ScannerState::EntropyCodedDataSentinel);
                        pos + 1
                    } else {
                        remaining.len()
                    }
                }
                ScannerState::Done => remaining.len(),
                _ => {
                    self.on_byte(remaining[0], emit);
                    1
                }
            };
            debug_assert!(bytes_to_move_forward > 0);
            self.offset = self.offset.wrapping_add(bytes_to_move_forward);
            idx += bytes_to_move_forward;
        }
    }
}

/// Standalone markers: TEM (`0x01`), RST0..RST7 (`0xD0..=0xD7`), SOI (`0xD8`), EOI (`0xD9`).
#[inline]
pub const fn marker_stands_alone(marker: u8) -> bool {
    marker == 0x01 || (marker >= 0xD0 && marker <= 0xD9)
}

/// Self-contained safe Rust JPEG segment scanner.
#[derive(Clone, Debug)]
pub struct SegmentScanner {
    state: SegmentScannerState,
    segments: Vec<JpegSegment>,
}

impl SegmentScanner {
    pub fn new(stop_marker: u8) -> Self {
        Self {
            state: SegmentScannerState::new(stop_marker),
            segments: Vec::new(),
        }
    }

    pub fn new_to_eoi() -> Self {
        Self::new(MARKER_END_OF_IMAGE)
    }

    #[inline]
    pub fn is_done(&self) -> bool {
        self.state.is_done()
    }

    #[inline]
    pub fn had_error(&self) -> bool {
        self.state.had_error()
    }

    #[inline]
    pub fn segments(&self) -> &[JpegSegment] {
        &self.segments
    }

    pub fn on_bytes(&mut self, data: &[u8]) {
        let segs = &mut self.segments;
        self.state.on_bytes(data, &mut |seg| segs.push(seg));
    }
}

/// Compute `(param_offset, param_len)` for `SkJpegSegmentScanner::GetParameters`.
/// Returns `None` if `parameter_length < 2` or out of bounds of `scanned_len`
/// (in which case `SkData::MakeSubset` returns an empty `SkData`).
pub fn segment_parameter_range(scanned_len: usize, segment: JpegSegment) -> Option<(usize, usize)> {
    let param_len = (segment.parameter_length as usize).checked_sub(SEGMENT_PARAMETER_LENGTH_SIZE)?;
    let offset = segment
        .offset
        .checked_add(MARKER_CODE_SIZE + SEGMENT_PARAMETER_LENGTH_SIZE)?;
    let end = offset.checked_add(param_len)?;
    if end > scanned_len {
        return None;
    }
    Some((offset, param_len))
}

/// Verify the 4-byte segment header in memory and return `(param_offset, param_len)`.
/// Matches `SkJpegMemorySourceMgr::getSegmentParameters` (returns `None` when
/// `parameter_length <= 2` or header is out of bounds / mismatched).
pub fn verify_memory_segment_parameters(
    data: &[u8],
    segment: JpegSegment,
) -> Option<(usize, usize)> {
    if (segment.parameter_length as usize) <= SEGMENT_PARAMETER_LENGTH_SIZE {
        return None;
    }
    let header_end = segment
        .offset
        .checked_add(MARKER_CODE_SIZE + SEGMENT_PARAMETER_LENGTH_SIZE)?;
    let total_end = segment
        .offset
        .checked_add(MARKER_CODE_SIZE + (segment.parameter_length as usize))?;
    if total_end > data.len() || header_end > data.len() {
        return None;
    }
    let hdr = &data[segment.offset..header_end];
    if hdr[0] != 0xFF || hdr[1] != segment.marker {
        return None;
    }
    let encoded_len = ((hdr[2] as u16) << 8) | (hdr[3] as u16);
    if encoded_len != segment.parameter_length {
        return None;
    }
    Some((
        header_end,
        (segment.parameter_length as usize) - SEGMENT_PARAMETER_LENGTH_SIZE,
    ))
}
