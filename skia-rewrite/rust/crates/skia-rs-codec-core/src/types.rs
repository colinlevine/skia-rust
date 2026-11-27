//! Core codec types, enums, constants, and `IRect` helpers matching Skia's C++ definitions.

use skia_rs_geometry::{IRect, ISize};

/// Matches `SkCodec::kNoFrame`.
pub const NO_FRAME: i32 = -1;

/// Matches `SkCodec::MinBufferedBytesNeeded()`.
pub const MIN_BUFFERED_BYTES_NEEDED: usize = 32;

/// Matches `SkCodec::kRepetitionCountInfinite`.
pub const REPETITION_COUNT_INFINITE: i32 = -1;

/// Matches `SkEncodedInfo::Alpha`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EncodedAlpha {
    Opaque = 0,
    Unpremul = 1,
    Binary = 2,
}

impl EncodedAlpha {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Opaque),
            1 => Some(Self::Unpremul),
            2 => Some(Self::Binary),
            _ => None,
        }
    }
}

/// Matches `SkEncodedInfo::Color`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EncodedColor {
    Gray = 0,
    GrayAlpha = 1,
    XAlpha = 2,
    Rgb565 = 3,
    Palette = 4,
    Rgb = 5,
    Rgba = 6,
    Bgr = 7,
    Bgrx = 8,
    Bgra = 9,
    Yuv = 10,
    Yuva = 11,
    InvertedCmyk = 12,
    Ycck = 13,
}

impl EncodedColor {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Gray),
            1 => Some(Self::GrayAlpha),
            2 => Some(Self::XAlpha),
            3 => Some(Self::Rgb565),
            4 => Some(Self::Palette),
            5 => Some(Self::Rgb),
            6 => Some(Self::Rgba),
            7 => Some(Self::Bgr),
            8 => Some(Self::Bgrx),
            9 => Some(Self::Bgra),
            10 => Some(Self::Yuv),
            11 => Some(Self::Yuva),
            12 => Some(Self::InvertedCmyk),
            13 => Some(Self::Ycck),
            _ => None,
        }
    }
}

/// Matches `SkEncodedImageFormat`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EncodedImageFormat {
    Bmp = 0,
    Gif = 1,
    Ico = 2,
    Jpeg = 3,
    Png = 4,
    Wbmp = 5,
    Webp = 6,
    Pkm = 7,
    Ktx = 8,
    Astc = 9,
    Dng = 10,
    Heif = 11,
    Avif = 12,
    Jpegxl = 13,
}

impl EncodedImageFormat {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Bmp),
            1 => Some(Self::Gif),
            2 => Some(Self::Ico),
            3 => Some(Self::Jpeg),
            4 => Some(Self::Png),
            5 => Some(Self::Wbmp),
            6 => Some(Self::Webp),
            7 => Some(Self::Pkm),
            8 => Some(Self::Ktx),
            9 => Some(Self::Astc),
            10 => Some(Self::Dng),
            11 => Some(Self::Heif),
            12 => Some(Self::Avif),
            13 => Some(Self::Jpegxl),
            _ => None,
        }
    }
}

/// Matches `SkEncodedOrigin`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EncodedOrigin {
    TopLeft = 1,
    TopRight = 2,
    BottomRight = 3,
    BottomLeft = 4,
    LeftTop = 5,
    RightTop = 6,
    RightBottom = 7,
    LeftBottom = 8,
}

impl EncodedOrigin {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::TopLeft),
            2 => Some(Self::TopRight),
            3 => Some(Self::BottomRight),
            4 => Some(Self::BottomLeft),
            5 => Some(Self::LeftTop),
            6 => Some(Self::RightTop),
            7 => Some(Self::RightBottom),
            8 => Some(Self::LeftBottom),
            _ => None,
        }
    }

    #[inline]
    pub const fn swaps_width_height(self) -> bool {
        (self as i32) >= (Self::LeftTop as i32)
    }
}

/// Matches `SkCodec::Result`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CodecResult {
    Success = 0,
    IncompleteInput = 1,
    ErrorInInput = 2,
    InvalidConversion = 3,
    InvalidScale = 4,
    InvalidParameters = 5,
    InvalidInput = 6,
    CouldNotRewind = 7,
    InternalError = 8,
    Unimplemented = 9,
    OutOfMemory = 10,
}

impl CodecResult {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Success),
            1 => Some(Self::IncompleteInput),
            2 => Some(Self::ErrorInInput),
            3 => Some(Self::InvalidConversion),
            4 => Some(Self::InvalidScale),
            5 => Some(Self::InvalidParameters),
            6 => Some(Self::InvalidInput),
            7 => Some(Self::CouldNotRewind),
            8 => Some(Self::InternalError),
            9 => Some(Self::Unimplemented),
            10 => Some(Self::OutOfMemory),
            _ => None,
        }
    }
}

/// Matches `SkCodec::ZeroInitialized`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZeroInitialized {
    Yes = 0,
    No = 1,
}

impl ZeroInitialized {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Yes),
            1 => Some(Self::No),
            _ => None,
        }
    }
}

/// Matches `SkCodec::SkScanlineOrder`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScanlineOrder {
    TopDown = 0,
    BottomUp = 1,
}

impl ScanlineOrder {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::TopDown),
            1 => Some(Self::BottomUp),
            _ => None,
        }
    }
}

/// Matches `SkCodec::SelectionPolicy`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SelectionPolicy {
    PreferStillImage = 0,
    PreferAnimation = 1,
}

impl SelectionPolicy {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::PreferStillImage),
            1 => Some(Self::PreferAnimation),
            _ => None,
        }
    }
}

/// Matches `SkCodecAnimation::DisposalMethod`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DisposalMethod {
    Keep = 1,
    RestoreBgColor = 2,
    RestorePrevious = 3,
}

impl DisposalMethod {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Keep),
            2 => Some(Self::RestoreBgColor),
            3 => Some(Self::RestorePrevious),
            _ => None,
        }
    }
}

/// Matches `SkCodecAnimation::Blend`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Blend {
    SrcOver = 0,
    Src = 1,
}

impl Blend {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::SrcOver),
            1 => Some(Self::Src),
            _ => None,
        }
    }
}

/// Matches `SkCodec::XformTime`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum XformTime {
    No = 0,
    Palette = 1,
    DecodeRow = 2,
}

/// Subset of `skcms_PixelFormat` enum discriminants used by `SkCodec`.
/// Values MUST match `modules/skcms/src/skcms_public.h`.
pub mod skcms_pixel_format {
    pub const G_8: i32 = 2;
    pub const BGR_565: i32 = 7;
    pub const RGBA_8888: i32 = 12;
    pub const BGRA_8888: i32 = 13;
    pub const RGBA_1010102: i32 = 16;
    pub const RGBA_HHHH: i32 = 32;
    pub const BGR_101010X_XR: i32 = 39;
}

/// Matches `skcms_AlphaFormat` in `modules/skcms/src/skcms_public.h`.
pub mod skcms_alpha_format {
    pub const OPAQUE: i32 = 0;
    pub const UNPREMUL: i32 = 1;
    pub const PREMUL_AS_ENCODED: i32 = 2;
}

/// Exact port of `SkIRect::MakeXYWH(x, y, w, h)` using saturating add.
#[inline]
pub const fn irect_make_xywh(x: i32, y: i32, w: i32, h: i32) -> IRect {
    IRect {
        left: x,
        top: y,
        right: x.saturating_add(w),
        bottom: y.saturating_add(h),
    }
}

/// Exact port of `SkIRect::MakeSize(ISize)`.
#[inline]
pub const fn irect_make_size(size: ISize) -> IRect {
    IRect {
        left: 0,
        top: 0,
        right: size.width,
        bottom: size.height,
    }
}

/// Exact port of `SkIRect::contains(const SkIRect& r)`.
#[inline]
pub fn irect_contains(outer: IRect, inner: IRect) -> bool {
    !inner.is_empty()
        && !outer.is_empty()
        && outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Exact port of `SkIRect::intersect(const SkIRect&)`: returns `Some(intersection)` if non-empty.
#[inline]
pub fn irect_intersect(a: IRect, b: IRect) -> Option<IRect> {
    let mut out = IRect::make_empty();
    if out.intersect_pair(a, b) {
        Some(out)
    } else {
        None
    }
}

/// Exact port of `SkIRect::size()`.
#[inline]
pub fn irect_size(r: IRect) -> ISize {
    ISize {
        width: r.width(),
        height: r.height(),
    }
}
