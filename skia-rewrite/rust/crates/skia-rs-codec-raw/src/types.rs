//! Core FFI-safe types, enums, and constants for `skia-rs-codec-raw` (`P2.8`).

/// Matches `ttByte` in `dng_tag_types.h`.
pub const DNG_TT_BYTE: u32 = 1;

/// Matches `kMosaicPlane = -1` passed to `dng_negative::BuildStage3Image`.
pub const DNG_MOSAIC_PLANE_ALL: i32 = -1;

/// Matches `skcms_PixelFormat_RGB_888` in `modules/skcms/src/skcms_public.h`.
pub const SKCMS_PIXEL_FORMAT_RGB_888: i32 = 10;

/// Minimum short-edge dimension enforced by DNG SDK downscaling (`80.f`).
pub const RAW_MIN_SHORT_EDGE: f32 = 80.0;

/// Maximum tolerated rendered-to-requested ratio in `SkRawCodec::onGetPixels` (`1.03f`).
pub const RAW_MAX_RENDER_DIFF_RATIO: f32 = 1.03;

/// Maximum buffered RAW stream size (100 MiB) matching `SkRawLimitedDynamicMemoryWStream`.
pub const RAW_MAX_STREAM_SIZE: usize = 100 * 1024 * 1024;

/// Minimum read chunk size (8 KiB) matching `SkRawBufferedStream::bufferMoreData`.
pub const RAW_MIN_CHUNK_READ: usize = 8192;

/// Sentinel passed to `plan_buffer_more` indicating read-to-end (`kReadToEnd = 0`).
pub const RAW_READ_TO_END: usize = 0;

/// TIFF header byte count inspected by `is_tiff_header_valid`.
pub const TIFF_HEADER_SIZE: usize = 4;

/// TIFF magic number (`42` / `0x002A`).
pub const TIFF_MAGIC: u16 = 0x002A;

/// Matches `dng_point` (`int32 v, h`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DngPoint {
    pub v: i32,
    pub h: i32,
}

impl DngPoint {
    #[inline]
    pub const fn new(v: i32, h: i32) -> Self {
        Self { v, h }
    }
}

/// Matches `dng_rect` (`int32 t, l, b, r`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DngRect {
    pub t: i32,
    pub l: i32,
    pub b: i32,
    pub r: i32,
}

impl DngRect {
    #[inline]
    pub const fn new(t: i32, l: i32, b: i32, r: i32) -> Self {
        Self { t, l, b, r }
    }

    /// Matches `dng_rect::Size()` (`v = b - t`, `h = r - l`).
    #[inline]
    pub const fn size(self) -> DngPoint {
        DngPoint {
            v: self.b - self.t,
            h: self.r - self.l,
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.t >= self.b || self.l >= self.r
    }
}

/// Matches `::piex::Error` in `third_party/externals/piex/src/piex_types.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PiexError {
    Ok = 0,
    Fail = 1,
    Unsupported = 2,
}

impl PiexError {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Ok),
            1 => Some(Self::Fail),
            2 => Some(Self::Unsupported),
            _ => None,
        }
    }
}

/// Matches `::piex::Image::Format` in `third_party/externals/piex/src/piex_types.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PiexImageFormat {
    JpegCompressed = 0,
    UncompressedRgb = 1,
}

impl PiexImageFormat {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::JpegCompressed),
            1 => Some(Self::UncompressedRgb),
            _ => None,
        }
    }
}

/// Matches `::piex::PreviewImageData::ColorSpace` in `third_party/externals/piex/src/piex_types.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PiexColorSpace {
    Srgb = 0,
    AdobeRgb = 1,
}

impl PiexColorSpace {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Srgb),
            1 => Some(Self::AdobeRgb),
            _ => None,
        }
    }
}

/// High-level branch chosen after `::piex::GetPreviewImageData`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PiexDecisionKind {
    /// PIEX reported `kFail`: reject the input with `CodecResult::InvalidInput`.
    Fail = 0,
    /// PIEX found a valid JPEG-compressed preview: transfer its slice to `SkJpegCodec`.
    UseJpegPreview = 1,
    /// No usable JPEG preview (or `kUnsupported`): fall back to TIFF/DNG decoding.
    FallbackToDng = 2,
}

/// Complete decision plan produced from `::piex::PreviewImageData`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PiexPreviewPlan {
    /// Discriminant of [`PiexDecisionKind`].
    pub decision: i32,
    /// Discriminant of `SkCodec::Result` (`CodecResult`) when failing.
    pub result: i32,
    pub preview_offset: u32,
    pub preview_length: u32,
    pub use_adobe_rgb: bool,
    /// Mapped `SkEncodedOrigin` (`EncodedOrigin`) discriminant (`1..=8`).
    pub origin: i32,
}

/// Metadata & scaling capabilities computed by `SkDngImage::init`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RawImageMeta {
    pub width: i32,
    pub height: i32,
    pub is_scalable: bool,
    pub is_xtrans_image: bool,
    pub valid: bool,
}

/// `SkEncodedInfo` and `SkCodec` constructor parameters for `SkRawCodec`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RawEncodedInfoSpec {
    pub width: i32,
    pub height: i32,
    /// `SkEncodedInfo::Color` (`EncodedColor::Rgb`).
    pub encoded_color: i32,
    /// `SkEncodedInfo::Alpha` (`EncodedAlpha::Opaque`).
    pub encoded_alpha: i32,
    pub bits_per_component: u8,
    /// `skcms_PixelFormat` passed to `SkCodec` base constructor (`RGBA_8888`).
    pub codec_xform_format: i32,
    /// `SkEncodedImageFormat` (`EncodedImageFormat::Dng`).
    pub encoded_format: i32,
    /// `SkRawCodec::usesColorXform()` (always `false`).
    pub uses_color_xform: bool,
}

/// Layout parameters for `dng_pixel_buffer` used to read a single RGB888 scanline.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DngPixelBufferLayout {
    pub plane: u32,
    pub planes: u32,
    pub col_step: i32,
    pub plane_step: i32,
    pub pixel_type: u32,
    pub pixel_size: u32,
    pub row_step: i32,
    pub src_row_bytes: usize,
}

/// Validation & row-conversion plan for `SkRawCodec::onGetPixels`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RawGetPixelsPlan {
    /// `SkCodec::Result` (`CodecResult::Success` if ready to decode rows).
    pub result: i32,
    /// Source `skcms_PixelFormat` (`skcms_PixelFormat_RGB_888`).
    pub src_format: i32,
    /// Destination `skcms_PixelFormat` selected via `SelectXformFormat`.
    pub dst_format: i32,
    /// Source `skcms_AlphaFormat` (`skcms_AlphaFormat_Unpremul`).
    pub src_alpha_format: i32,
    /// Destination `skcms_AlphaFormat` (`skcms_AlphaFormat_Unpremul`).
    pub dst_alpha_format: i32,
    pub row_count: i32,
    pub pixel_count: i32,
    pub buffer_layout: DngPixelBufferLayout,
}
