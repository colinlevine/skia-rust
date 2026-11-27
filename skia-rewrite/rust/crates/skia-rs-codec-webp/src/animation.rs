//! WebP animation metadata, repetition count, frame disposal/blend mapping,
//! and SrcOver row blending logic matching `src/codec/SkWebpCodec.cpp`.

use skia_rs_codec_core::types::{
    Blend, DisposalMethod, EncodedAlpha, REPETITION_COUNT_INFINITE,
};
use skia_rs_color::image_info::{AlphaType, ColorType};

use crate::types::{WebpMuxAnimBlend, WebpMuxAnimDispose, ANIMATION_FLAG};

/// Exact port of `SkWebpCodec::onIsAnimated`.
#[inline]
pub const fn is_animated(format_flags: u32) -> bool {
    (format_flags & ANIMATION_FLAG) != 0
}

/// Exact port of `SkWebpCodec::onGetRepetitionCount`.
///
/// - Non-animated images return `0`.
/// - Animated images with `loop_count == 0` return `kRepetitionCountInfinite` (`-1`).
/// - Otherwise returns `loop_count - 1`.
#[inline]
pub const fn compute_repetition_count(format_flags: u32, loop_count: i32) -> i32 {
    if (format_flags & ANIMATION_FLAG) == 0 {
        return 0;
    }
    if loop_count == 0 {
        return REPETITION_COUNT_INFINITE;
    }
    loop_count.wrapping_sub(1)
}

/// Mapped Skia frame metadata produced from a `WebPIterator`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpFrameMeta {
    /// Matches `SkEncodedInfo::Alpha`.
    pub reported_alpha: i32,
    /// Matches `SkCodecAnimation::DisposalMethod`.
    pub disposal_method: i32,
    /// Matches `SkCodecAnimation::Blend`.
    pub blend: i32,
}

/// Maps `WebPIterator` fields (`has_alpha`, `dispose_method`, `blend_method`)
/// into Skia's `SkFrame` properties, matching `SkWebpCodec::onGetFrameCount`.
#[inline]
pub const fn map_frame_metadata(
    has_alpha: bool,
    dispose_method: WebpMuxAnimDispose,
    blend_method: WebpMuxAnimBlend,
) -> WebpFrameMeta {
    let reported_alpha = if has_alpha {
        EncodedAlpha::Unpremul
    } else {
        EncodedAlpha::Opaque
    };
    let disposal = match dispose_method {
        WebpMuxAnimDispose::Background => DisposalMethod::RestoreBgColor,
        WebpMuxAnimDispose::None => DisposalMethod::Keep,
    };
    let blend = match blend_method {
        WebpMuxAnimBlend::Blend => Blend::SrcOver,
        WebpMuxAnimBlend::NoBlend => Blend::Src,
    };
    WebpFrameMeta {
        reported_alpha: reported_alpha as i32,
        disposal_method: disposal as i32,
        blend: blend as i32,
    }
}

/// Exact blend-pipeline configuration corresponding to `RPBlender` in `SkWebpCodec.cpp`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WebpBlendPlan {
    /// True when previous-frame SrcOver blending must be executed.
    pub active: bool,
    /// True when colorXform output must go into a 1-row temp buffer before blending.
    pub use_tmp_xform_row: bool,
    /// `SkColorType` of the source pixels fed into the blender.
    pub blend_src_ct: i32,
    /// Whether `dst` needs premultiplication prior to `srcover` (`dst_at == kUnpremul`).
    pub premul_dst: bool,
    /// Whether `src` needs premultiplication prior to `srcover` (`frame_has_alpha`).
    pub premul_src: bool,
    /// Whether blended output needs unpremultiplication before store (`dst_at == kUnpremul`).
    pub unpremul_dst: bool,
}

/// Plans the post-decode color-xform and SrcOver blend pipeline stages.
#[inline]
pub const fn plan_blend_pipeline(
    dst_ct: ColorType,
    webp_ct: ColorType,
    dst_at: AlphaType,
    frame_has_alpha: bool,
    has_color_xform: bool,
    blend_with_prev_frame: bool,
) -> WebpBlendPlan {
    if !blend_with_prev_frame {
        return WebpBlendPlan {
            active: false,
            use_tmp_xform_row: false,
            blend_src_ct: webp_ct as i32,
            premul_dst: false,
            premul_src: false,
            unpremul_dst: false,
        };
    }

    let is_unpremul = matches!(dst_at, AlphaType::Unpremul);
    let blend_src_ct = if has_color_xform { dst_ct } else { webp_ct };

    WebpBlendPlan {
        active: true,
        use_tmp_xform_row: has_color_xform,
        blend_src_ct: blend_src_ct as i32,
        premul_dst: is_unpremul,
        premul_src: frame_has_alpha,
        unpremul_dst: is_unpremul,
    }
}

#[inline]
fn round_to_u8(v: f32) -> u8 {
    let clamped = if v <= 0.0 {
        0.0
    } else if v >= 255.0 {
        255.0
    } else {
        v
    };
    (clamped + 0.5) as u8
}

/// Pure-Rust float `SrcOver` pixel blender matching `RPBlender` for 8888 formats.
///
/// `src` is expected to be unpremultiplied (or opaque when `!src_has_alpha`).
/// Channel order is RGBA when `!swap_rb`, or BGRA when `swap_rb`.
pub fn blend_src_over_row_8888(
    dst: &mut [u8],
    src: &[u8],
    width: usize,
    dst_ct: ColorType,
    src_ct: ColorType,
    dst_at: AlphaType,
    src_has_alpha: bool,
) {
    let dst_bgra = matches!(dst_ct, ColorType::Bgra8888);
    let src_bgra = matches!(src_ct, ColorType::Bgra8888);
    let dst_unpremul = matches!(dst_at, AlphaType::Unpremul);
    let inv255 = 1.0f32 / 255.0f32;

    let count = width.min(dst.len() / 4).min(src.len() / 4);
    for i in 0..count {
        let off = i * 4;
        let (mut dr, mut dg, mut db, da) = if dst_bgra {
            (
                dst[off + 2] as f32 * inv255,
                dst[off + 1] as f32 * inv255,
                dst[off] as f32 * inv255,
                dst[off + 3] as f32 * inv255,
            )
        } else {
            (
                dst[off] as f32 * inv255,
                dst[off + 1] as f32 * inv255,
                dst[off + 2] as f32 * inv255,
                dst[off + 3] as f32 * inv255,
            )
        };
        if dst_unpremul {
            dr *= da;
            dg *= da;
            db *= da;
        }

        let (mut sr, mut sg, mut sb, sa) = if src_bgra {
            (
                src[off + 2] as f32 * inv255,
                src[off + 1] as f32 * inv255,
                src[off] as f32 * inv255,
                src[off + 3] as f32 * inv255,
            )
        } else {
            (
                src[off] as f32 * inv255,
                src[off + 1] as f32 * inv255,
                src[off + 2] as f32 * inv255,
                src[off + 3] as f32 * inv255,
            )
        };
        if src_has_alpha {
            sr *= sa;
            sg *= sa;
            sb *= sa;
        }

        let inv_sa = 1.0f32 - sa;
        let mut out_r = sr + dr * inv_sa;
        let mut out_g = sg + dg * inv_sa;
        let mut out_b = sb + db * inv_sa;
        let out_a = sa + da * inv_sa;

        if dst_unpremul {
            if out_a == 0.0 {
                out_r = 0.0;
                out_g = 0.0;
                out_b = 0.0;
            } else {
                let scale = 1.0f32 / out_a;
                out_r *= scale;
                out_g *= scale;
                out_b *= scale;
            }
        }

        let r8 = round_to_u8(out_r * 255.0);
        let g8 = round_to_u8(out_g * 255.0);
        let b8 = round_to_u8(out_b * 255.0);
        let a8 = round_to_u8(out_a * 255.0);

        if dst_bgra {
            dst[off] = b8;
            dst[off + 1] = g8;
            dst[off + 2] = r8;
            dst[off + 3] = a8;
        } else {
            dst[off] = r8;
            dst[off + 1] = g8;
            dst[off + 2] = b8;
            dst[off + 3] = a8;
        }
    }
}
