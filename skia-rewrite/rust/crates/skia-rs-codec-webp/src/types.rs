//! WebP format constants and FFI-shared enum discriminants matching `libwebp` and Skia C++.

/// VP8X feature flags matching `WebPFeatureFlags` in `webp/mux_types.h`.
pub const ANIMATION_FLAG: u32 = 0x0000_0002;
pub const XMP_FLAG: u32 = 0x0000_0004;
pub const EXIF_FLAG: u32 = 0x0000_0008;
pub const ALPHA_FLAG: u32 = 0x0000_0010;
pub const ICCP_FLAG: u32 = 0x0000_0020;
pub const ALL_VALID_FLAGS: u32 = 0x0000_003E;

/// Maximum single-axis dimension allowed by WebP (`WEBP_MAX_DIMENSION`).
pub const WEBP_MAX_DIMENSION: i32 = 16_383;

/// Maximum canvas pixel count accepted by `SkWebpCodec::MakeFromStream`
/// (`0x7FFFFFFF >> 2`, ensuring 4-byte pixel allocations do not overflow `int32_t`).
pub const MAX_CANVAS_PIXELS: i64 = 0x7FFF_FFFF >> 2;

/// Matches `WEBP_CSP_MODE` in `webp/decode.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpCspMode {
    Rgb = 0,
    Rgba = 1,
    Bgr = 2,
    Bgra = 3,
    Argb = 4,
    Rgba4444 = 5,
    Rgb565 = 6,
    RgbAPremul = 7,
    BgrAPremul = 8,
    ArgbPremul = 9,
    RgbA4444Premul = 10,
    Yuv = 11,
    Yuva = 12,
    Last = 13,
}

impl WebpCspMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Rgb),
            1 => Some(Self::Rgba),
            2 => Some(Self::Bgr),
            3 => Some(Self::Bgra),
            4 => Some(Self::Argb),
            5 => Some(Self::Rgba4444),
            6 => Some(Self::Rgb565),
            7 => Some(Self::RgbAPremul),
            8 => Some(Self::BgrAPremul),
            9 => Some(Self::ArgbPremul),
            10 => Some(Self::RgbA4444Premul),
            11 => Some(Self::Yuv),
            12 => Some(Self::Yuva),
            13 => Some(Self::Last),
            _ => None,
        }
    }
}

/// Matches `WebPDemuxState` in `webp/demux.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpDemuxState {
    ParseError = -1,
    ParsingHeader = 0,
    ParsedHeader = 1,
    Done = 2,
}

impl WebpDemuxState {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            -1 => Some(Self::ParseError),
            0 => Some(Self::ParsingHeader),
            1 => Some(Self::ParsedHeader),
            2 => Some(Self::Done),
            _ => None,
        }
    }
}

/// Matches `VP8StatusCode` in `webp/decode.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Vp8StatusCode {
    Ok = 0,
    OutOfMemory = 1,
    InvalidParam = 2,
    BitstreamError = 3,
    UnsupportedFeature = 4,
    Suspended = 5,
    UserAbort = 6,
    NotEnoughData = 7,
}

impl Vp8StatusCode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Ok),
            1 => Some(Self::OutOfMemory),
            2 => Some(Self::InvalidParam),
            3 => Some(Self::BitstreamError),
            4 => Some(Self::UnsupportedFeature),
            5 => Some(Self::Suspended),
            6 => Some(Self::UserAbort),
            7 => Some(Self::NotEnoughData),
            _ => None,
        }
    }
}

/// Matches `WebPMuxAnimDispose` in `webp/mux_types.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpMuxAnimDispose {
    None = 0,
    Background = 1,
}

impl WebpMuxAnimDispose {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Background),
            _ => None,
        }
    }
}

/// Matches `WebPMuxAnimBlend` in `webp/mux_types.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpMuxAnimBlend {
    Blend = 0,
    NoBlend = 1,
}

impl WebpMuxAnimBlend {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Blend),
            1 => Some(Self::NoBlend),
            _ => None,
        }
    }
}

/// Matches `SkWebpEncoder::Compression` in `include/encode/SkWebpEncoder.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpCompression {
    Lossy = 0,
    Lossless = 1,
}

impl WebpCompression {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Lossy),
            1 => Some(Self::Lossless),
            _ => None,
        }
    }
}
