//! Safe Rust WebP encoder planning, validation, scanline conversion, ICC/EXIF/XMP
//! mux planning, and animated frame sequence state machine matching
//! `src/encode/SkWebpEncoderImpl.cpp`.

use skia_rs_color::image_info::{
    color_type_bytes_per_pixel, color_type_channel_flags, AlphaType, ColorType,
    COLOR_CHANNEL_FLAG_ALPHA,
};

use crate::types::WebpCompression;

/// Maximum valid `SkImageInfo` dimension (`SK_MaxS32 >> 2`).
const SK_IMAGE_INFO_MAX_DIMENSION: i32 = i32::MAX >> 2;

/// Default lossy compression method matching Chrome/Skia defaults when
/// `SK_WEBP_ENCODER_USE_DEFAULT_METHOD` is NOT defined.
pub const DEFAULT_LOSSY_METHOD: i32 = 3;

/// Default lossless compression method matching Chrome/Skia defaults.
pub const DEFAULT_LOSSLESS_METHOD: i32 = 0;

/// Maximum valid libwebp compression method (`0..=6`).
pub const MAX_WEBP_METHOD: i32 = 6;

/// How pixels should be imported into `WebPPicture`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpImportStrategy {
    /// Import directly with `WebPPictureImportRGBX`.
    DirectRgbx = 0,
    /// Import directly with `WebPPictureImportRGBA`.
    DirectRgba = 1,
    /// Import directly with `WebPPictureImportBGRA` (only when `WebPPictureImportBGRA` macro exists).
    DirectBgra = 2,
    /// Convert rows in safe Rust to unpremul `RGBA_8888`, then call `WebPPictureImportRGBA`.
    RustConvertToRgba8888 = 3,
    /// Convert via `SkPixmap::readPixels` to unpremul `RGBA_8888`, then call `WebPPictureImportRGBA`.
    ReadPixelsToRgba8888 = 4,
}

/// Specific bit-exact safe-Rust scanline converter into unpremultiplied `RGBA_8888`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WebpScanlineConvertKind {
    None = 0,
    Bgra8888UnpremulToRgba8888 = 1,
    Bgra8888OpaqueToRgba8888 = 2,
    Bgr888xToRgba8888 = 3,
    Gray8ToRgba8888 = 4,
}

impl WebpScanlineConvertKind {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Bgra8888UnpremulToRgba8888),
            2 => Some(Self::Bgra8888OpaqueToRgba8888),
            3 => Some(Self::Bgr888xToRgba8888),
            4 => Some(Self::Gray8ToRgba8888),
            _ => None,
        }
    }
}

/// Exact port of `SkColorTypeIsAlphaOnly`.
#[inline]
pub const fn color_type_is_alpha_only(ct: ColorType) -> bool {
    color_type_channel_flags(ct) == COLOR_CHANNEL_FLAG_ALPHA
}

/// Exact port of `SkPixmapIsValid(pixmap) && !SkColorTypeIsAlphaOnly(ct) && pixmap.addr() != nullptr`.
#[inline]
pub const fn validate_encode_pixmap(
    width: i32,
    height: i32,
    ct: ColorType,
    at: AlphaType,
    has_addr: bool,
    row_bytes: usize,
) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    if width > SK_IMAGE_INFO_MAX_DIMENSION || height > SK_IMAGE_INFO_MAX_DIMENSION {
        return false;
    }
    if matches!(ct, ColorType::Unknown) || matches!(at, AlphaType::Unknown) {
        return false;
    }
    if color_type_is_alpha_only(ct) {
        return false;
    }
    if !has_addr {
        return false;
    }
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let min_row_bytes = (width as usize).saturating_mul(bpp);
    row_bytes >= min_row_bytes
}

/// Validates `SkWebpEncoder::Options::fQuality` (`[0.0, 100.0]`, non-NaN).
#[inline]
pub fn validate_quality(quality: f32) -> bool {
    (0.0f32..=100.0f32).contains(&quality)
}

/// Clamps `quality` into `[0.0, 100.0]` (helper for callers that want clamped quality).
#[inline]
pub fn clamp_quality(quality: f32) -> f32 {
    if quality.is_nan() || quality < 0.0 {
        0.0
    } else if quality > 100.0 {
        100.0
    } else {
        quality
    }
}

/// Complete encoding plan for a single `WebPPicture` + `WebPConfig`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpEncodePlan {
    pub valid: bool,
    pub width: i32,
    pub height: i32,
    /// `0` for lossy, `1` for lossless.
    pub lossless: i32,
    /// If true, caller sets `webp_config.method = plan.method`.
    pub set_method: bool,
    pub method: i32,
    /// `0` for lossy (YUV), `1` for lossless (ARGB).
    pub use_argb: i32,
    /// Matches [`WebpImportStrategy`].
    pub import_strategy: i32,
    /// Matches [`WebpScanlineConvertKind`].
    pub convert_kind: i32,
}

impl WebpEncodePlan {
    const fn invalid() -> Self {
        Self {
            valid: false,
            width: 0,
            height: 0,
            lossless: 0,
            set_method: false,
            method: 0,
            use_argb: 0,
            import_strategy: WebpImportStrategy::ReadPixelsToRgba8888 as i32,
            convert_kind: WebpScanlineConvertKind::None as i32,
        }
    }
}

/// Plans `WebPConfig`, `WebPPicture`, and pixel import strategy matching
/// `preprocess_webp_picture` in `src/encode/SkWebpEncoderImpl.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn plan_encode_picture(
    width: i32,
    height: i32,
    ct: ColorType,
    at: AlphaType,
    has_addr: bool,
    row_bytes: usize,
    compression: WebpCompression,
    quality: f32,
    opt_method: i32,
    use_default_method_macro: bool,
    has_import_bgra_macro: bool,
) -> WebpEncodePlan {
    if !validate_quality(quality) {
        return WebpEncodePlan::invalid();
    }
    if !validate_encode_pixmap(width, height, ct, at, has_addr, row_bytes) {
        return WebpEncodePlan::invalid();
    }

    let (lossless, mut set_method, mut method, use_argb) = match compression {
        WebpCompression::Lossy => (
            0,
            !use_default_method_macro,
            DEFAULT_LOSSY_METHOD,
            0,
        ),
        WebpCompression::Lossless => (1, true, DEFAULT_LOSSLESS_METHOD, 1),
    };

    if opt_method >= 0 {
        if opt_method > MAX_WEBP_METHOD {
            return WebpEncodePlan::invalid();
        }
        set_method = true;
        method = opt_method;
    }

    let premul = matches!(at, AlphaType::Premul);
    let (import_strategy, convert_kind) = if matches!(ct, ColorType::Rgb888x) {
        (WebpImportStrategy::DirectRgbx, WebpScanlineConvertKind::None)
    } else if !premul && matches!(ct, ColorType::Rgba8888) {
        (WebpImportStrategy::DirectRgba, WebpScanlineConvertKind::None)
    } else if has_import_bgra_macro && !premul && matches!(ct, ColorType::Bgra8888) {
        (WebpImportStrategy::DirectBgra, WebpScanlineConvertKind::None)
    } else if matches!(ct, ColorType::Bgra8888) && matches!(at, AlphaType::Unpremul) {
        (
            WebpImportStrategy::RustConvertToRgba8888,
            WebpScanlineConvertKind::Bgra8888UnpremulToRgba8888,
        )
    } else if matches!(ct, ColorType::Bgra8888) && matches!(at, AlphaType::Opaque) {
        (
            WebpImportStrategy::RustConvertToRgba8888,
            WebpScanlineConvertKind::Bgra8888OpaqueToRgba8888,
        )
    } else if matches!(ct, ColorType::Bgr888x) {
        (
            WebpImportStrategy::RustConvertToRgba8888,
            WebpScanlineConvertKind::Bgr888xToRgba8888,
        )
    } else if matches!(ct, ColorType::Gray8) {
        (
            WebpImportStrategy::RustConvertToRgba8888,
            WebpScanlineConvertKind::Gray8ToRgba8888,
        )
    } else {
        (
            WebpImportStrategy::ReadPixelsToRgba8888,
            WebpScanlineConvertKind::None,
        )
    };

    WebpEncodePlan {
        valid: true,
        width,
        height,
        lossless,
        set_method,
        method,
        use_argb,
        import_strategy: import_strategy as i32,
        convert_kind: convert_kind as i32,
    }
}

/// Converts a single scanline into unpremultiplied `RGBA_8888` (`4 * width` bytes).
pub fn convert_scanline_to_rgba8888(
    kind: WebpScanlineConvertKind,
    dst: &mut [u8],
    src: &[u8],
    width: usize,
) -> bool {
    if dst.len() < width.saturating_mul(4) {
        return false;
    }
    match kind {
        WebpScanlineConvertKind::None => false,
        WebpScanlineConvertKind::Bgra8888UnpremulToRgba8888 => {
            if src.len() < width.saturating_mul(4) {
                return false;
            }
            for i in 0..width {
                let off = i * 4;
                dst[off] = src[off + 2];
                dst[off + 1] = src[off + 1];
                dst[off + 2] = src[off];
                dst[off + 3] = src[off + 3];
            }
            true
        }
        WebpScanlineConvertKind::Bgra8888OpaqueToRgba8888
        | WebpScanlineConvertKind::Bgr888xToRgba8888 => {
            if src.len() < width.saturating_mul(4) {
                return false;
            }
            for i in 0..width {
                let off = i * 4;
                dst[off] = src[off + 2];
                dst[off + 1] = src[off + 1];
                dst[off + 2] = src[off];
                dst[off + 3] = 0xFF;
            }
            true
        }
        WebpScanlineConvertKind::Gray8ToRgba8888 => {
            if src.len() < width {
                return false;
            }
            for i in 0..width {
                let g = src[i];
                let off = i * 4;
                dst[off] = g;
                dst[off + 1] = g;
                dst[off + 2] = g;
                dst[off + 3] = 0xFF;
            }
            true
        }
    }
}

/// Converts a full 2D image buffer into unpremultiplied `RGBA_8888` rows.
pub fn convert_image_to_rgba8888(
    kind: WebpScanlineConvertKind,
    dst: &mut [u8],
    dst_row_bytes: usize,
    src: &[u8],
    src_row_bytes: usize,
    width: usize,
    height: usize,
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let min_dst_row = match width.checked_mul(4) {
        Some(v) => v,
        None => return false,
    };
    if dst_row_bytes < min_dst_row {
        return false;
    }
    let src_bpp = match kind {
        WebpScanlineConvertKind::None => return false,
        WebpScanlineConvertKind::Bgra8888UnpremulToRgba8888
        | WebpScanlineConvertKind::Bgra8888OpaqueToRgba8888
        | WebpScanlineConvertKind::Bgr888xToRgba8888 => 4usize,
        WebpScanlineConvertKind::Gray8ToRgba8888 => 1usize,
    };
    let min_src_row = match width.checked_mul(src_bpp) {
        Some(v) => v,
        None => return false,
    };
    if src_row_bytes < min_src_row {
        return false;
    }

    for y in 0..height {
        let Some(dst_start) = y.checked_mul(dst_row_bytes) else {
            return false;
        };
        let Some(dst_end) = dst_start.checked_add(min_dst_row) else {
            return false;
        };
        let Some(src_start) = y.checked_mul(src_row_bytes) else {
            return false;
        };
        let Some(src_end) = src_start.checked_add(min_src_row) else {
            return false;
        };
        if dst_end > dst.len() || src_end > src.len() {
            return false;
        }
        if !convert_scanline_to_rgba8888(
            kind,
            &mut dst[dst_start..dst_end],
            &src[src_start..src_end],
            width,
        ) {
            return false;
        }
    }
    true
}

/// Mux decision plan for metadata chunks (ICCP / EXIF / XMP).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpMuxPlan {
    pub use_mux: bool,
    pub write_iccp_chunk: bool,
    pub write_exif_chunk: bool,
    pub write_xmp_chunk: bool,
}

/// Plans whether WebPMux assembly is required for auxiliary metadata chunks.
#[inline]
pub const fn plan_mux(has_icc: bool, has_exif: bool, has_xmp: bool) -> WebpMuxPlan {
    WebpMuxPlan {
        use_mux: has_icc || has_exif || has_xmp,
        write_iccp_chunk: has_icc,
        write_exif_chunk: has_exif,
        write_xmp_chunk: has_xmp,
    }
}

/// Frame canvas validation and timestamp accumulator for `SkWebpEncoder::EncodeAnimated`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpAnimatedEncodeState {
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub timestamp: i32,
}

impl WebpAnimatedEncodeState {
    #[inline]
    pub const fn new(canvas_width: i32, canvas_height: i32) -> Self {
        Self {
            canvas_width,
            canvas_height,
            timestamp: 0,
        }
    }

    /// Validates that a frame matches the canvas size, returning the timestamp to
    /// pass to `WebPAnimEncoderAdd` and advancing the accumulated timestamp by `duration`.
    #[inline]
    pub fn next_frame_timestamp(&mut self, frame_w: i32, frame_h: i32, duration: i32) -> Option<i32> {
        if frame_w != self.canvas_width || frame_h != self.canvas_height {
            return None;
        }
        let current = self.timestamp;
        self.timestamp = self.timestamp.wrapping_add(duration);
        Some(current)
    }
}
