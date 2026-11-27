//! Core GIF / Wuffs constants, status types, rectangle helpers, and signature sniffing.
//!
//! All numeric constants mirror `third_party/externals/wuffs/release/c/wuffs-v0.3.c`
//! and Skia's public codec headers (`include/codec/SkCodec.h`).

use skia_rs_geometry::IRect;

/// Matches `SK_WUFFS_CODEC_BUFFER_SIZE` in `SkWuffsCodec.cpp`.
pub const WUFFS_CODEC_BUFFER_SIZE: usize = 4096;

/// Matches `WUFFS_BASE__FLICKS_PER_MILLISECOND` (`705600`).
pub const WUFFS_FLICKS_PER_MILLISECOND: u64 = 705_600;

/// Matches `WUFFS_BASE__PIXEL_SUBSAMPLING__NONE`.
pub const WUFFS_PIXEL_SUBSAMPLING_NONE: u32 = 0x0000_0000;

/// Subset of `WUFFS_BASE__PIXEL_FORMAT__*` constants used by `SkWuffsCodec`.
pub mod wuffs_pixel_format {
    pub const INVALID: u32 = 0x0000_0000;
    pub const BGR_565: u32 = 0x8000_0565;
    pub const BGRA_NONPREMUL: u32 = 0x8100_8888;
    pub const RGBA_NONPREMUL: u32 = 0xA100_8888;
}

/// Matches `WUFFS_BASE__PIXEL_BLEND__*`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WuffsPixelBlend {
    Src = 0,
    SrcOver = 1,
}

impl WuffsPixelBlend {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Src),
            1 => Some(Self::SrcOver),
            _ => None,
        }
    }
}

/// Matches `WUFFS_BASE__ANIMATION_DISPOSAL__*`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WuffsAnimationDisposal {
    None = 0,
    RestoreBackground = 1,
    RestorePrevious = 2,
}

impl WuffsAnimationDisposal {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::RestoreBackground),
            2 => Some(Self::RestorePrevious),
            _ => None,
        }
    }
}

/// Classification of a `wuffs_base__status::repr` pointer at the C++ boundary.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WuffsStatusKind {
    /// `status.repr == nullptr`
    Ok = 0,
    /// `status.repr == wuffs_base__suspension__short_read`
    ShortRead = 1,
    /// `status.repr == wuffs_base__note__end_of_data`
    EndOfData = 2,
    /// Any other non-null status message (error / unexpected suspension).
    Error = 3,
}

impl WuffsStatusKind {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Ok),
            1 => Some(Self::ShortRead),
            2 => Some(Self::EndOfData),
            3 => Some(Self::Error),
            _ => None,
        }
    }
}

/// Matches `SkCodec::IsAnimated`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IsAnimated {
    Yes = 0,
    No = 1,
    Unknown = 2,
}

/// Standard GIF header version variants.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GifVersion {
    Gif87a,
    Gif89a,
}

/// Exact port of `SkGifDecoder::IsGif`: checks for the 4-byte `"GIF8"` prefix.
///
/// Checking 4 bytes (`"GIF8"`) rather than full 6-byte `"GIF87a"`/`"GIF89a"` matches
/// Skia's `SkWuffsCodec.cpp` so that malformed `GIF8xx` headers are routed to the
/// GIF decoder (which reports `kErrorInInput`) rather than failing format sniffing.
#[inline]
pub fn is_gif(buf: &[u8]) -> bool {
    buf.len() >= 4 && &buf[..4] == b"GIF8"
}

/// Strict GIF version parser (`"GIF87a"` or `"GIF89a"`).
#[inline]
pub fn parse_gif_version(buf: &[u8]) -> Option<GifVersion> {
    if buf.len() < 6 {
        return None;
    }
    match &buf[..6] {
        b"GIF87a" => Some(GifVersion::Gif87a),
        b"GIF89a" => Some(GifVersion::Gif89a),
        _ => None,
    }
}

/// Matches `wuffs_base__rect_ie_u32` (inclusive-exclusive unsigned rectangle).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WuffsRectU32 {
    pub min_incl_x: u32,
    pub min_incl_y: u32,
    pub max_excl_x: u32,
    pub max_excl_y: u32,
}

impl WuffsRectU32 {
    #[inline]
    pub const fn new(min_incl_x: u32, min_incl_y: u32, max_excl_x: u32, max_excl_y: u32) -> Self {
        Self {
            min_incl_x,
            min_incl_y,
            max_excl_x,
            max_excl_y,
        }
    }

    #[inline]
    pub const fn width(self) -> u32 {
        self.max_excl_x.saturating_sub(self.min_incl_x)
    }

    #[inline]
    pub const fn height(self) -> u32 {
        self.max_excl_y.saturating_sub(self.min_incl_y)
    }

    /// Exact match of `wuffs_base__rect_ie_u32::is_empty()`.
    #[inline]
    pub const fn is_empty(self) -> bool {
        (self.min_incl_x >= self.max_excl_x) || (self.min_incl_y >= self.max_excl_y)
    }

    /// Checks whether all coordinates lie within `[0, pixels_w] x [0, pixels_h]`.
    #[inline]
    pub const fn fits_in_surface(self, pixels_w: usize, pixels_h: usize) -> bool {
        (self.min_incl_x as usize) <= pixels_w
            && (self.min_incl_y as usize) <= pixels_h
            && (self.max_excl_x as usize) <= pixels_w
            && (self.max_excl_y as usize) <= pixels_h
    }

    /// Exact match of `SkIRect::MakeLTRB(min_incl_x, min_incl_y, max_excl_x, max_excl_y)`.
    #[inline]
    pub const fn to_irect(self) -> IRect {
        IRect {
            left: self.min_incl_x as i32,
            top: self.min_incl_y as i32,
            right: self.max_excl_x as i32,
            bottom: self.max_excl_y as i32,
        }
    }
}

/// Snapshot of fields extracted from a `wuffs_base__frame_config`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WuffsFrameConfigInput {
    pub index: u64,
    pub io_position: u64,
    pub bounds: WuffsRectU32,
    pub duration_flicks: i64,
    pub disposal: u8,
    pub opaque_within_bounds: bool,
    pub overwrite_instead_of_blend: bool,
}
