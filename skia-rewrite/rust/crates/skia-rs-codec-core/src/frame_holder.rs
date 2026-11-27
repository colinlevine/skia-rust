//! Safe Rust port of `SkFrame::fillIn` and `SkFrameHolder::setAlphaAndRequiredFrame`
//! from `src/codec/SkCodec.cpp`.

use skia_rs_color::image_info::AlphaType;
use skia_rs_geometry::IRect;

use crate::types::{
    irect_contains, irect_intersect, Blend, DisposalMethod, EncodedAlpha, NO_FRAME,
};

/// Summary of an `SkFrame` used by `compute_alpha_and_required_frame`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FrameSummary {
    pub frame_id: i32,
    pub reported_alpha: i32,
    pub has_alpha: bool,
    pub required_frame: i32,
    pub disposal_method: i32,
    pub blend: i32,
    pub frame_rect: IRect,
}

/// Output of `fill_frame_info` matching `SkCodec::FrameInfo` fields.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FrameInfoFields {
    pub required_frame: i32,
    pub duration: i32,
    pub fully_received: bool,
    pub alpha_type: i32,
    pub has_alpha_within_bounds: bool,
    pub disposal_method: i32,
    pub blend: i32,
    pub frame_rect: IRect,
}

/// Result of `compute_alpha_and_required_frame`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FrameDependencyResult {
    pub has_alpha: bool,
    pub required_frame: i32,
}

#[inline]
fn frame_rect_on_screen(frame_rect: IRect, screen_rect: IRect) -> IRect {
    irect_intersect(frame_rect, screen_rect).unwrap_or(IRect::EMPTY)
}

/// Exact port of `SkFrame::fillIn`.
pub fn fill_frame_info(
    required_frame: i32,
    duration: i32,
    fully_received: bool,
    has_alpha: bool,
    reported_alpha: EncodedAlpha,
    disposal_method: i32,
    blend: i32,
    frame_rect: IRect,
) -> FrameInfoFields {
    FrameInfoFields {
        required_frame,
        duration,
        fully_received,
        alpha_type: if has_alpha {
            AlphaType::Unpremul as i32
        } else {
            AlphaType::Opaque as i32
        },
        has_alpha_within_bounds: reported_alpha != EncodedAlpha::Opaque,
        disposal_method,
        blend,
        frame_rect,
    }
}

/// Exact port of `SkFrameHolder::setAlphaAndRequiredFrame` (IND1..IND6 / DEP5 / DEP7).
pub fn compute_alpha_and_required_frame<F>(
    screen_width: i32,
    screen_height: i32,
    frame: FrameSummary,
    get_frame: F,
) -> FrameDependencyResult
where
    F: Fn(i32) -> FrameSummary,
{
    let reports_alpha =
        EncodedAlpha::from_i32(frame.reported_alpha) != Some(EncodedAlpha::Opaque);
    let screen_rect = IRect::from_wh(screen_width, screen_height);
    let frame_rect = frame_rect_on_screen(frame.frame_rect, screen_rect);

    let i = frame.frame_id;
    if i == 0 {
        // IND1: first frame.
        return FrameDependencyResult {
            has_alpha: reports_alpha || frame_rect != screen_rect,
            required_frame: NO_FRAME,
        };
    }

    let blend_with_prev_frame = Blend::from_i32(frame.blend) == Some(Blend::SrcOver);
    if (!reports_alpha || !blend_with_prev_frame) && frame_rect == screen_rect {
        // IND2: fills whole image and is opaque or overwrites previous frame.
        return FrameDependencyResult {
            has_alpha: reports_alpha,
            required_frame: NO_FRAME,
        };
    }

    let mut prev_frame = get_frame(i - 1);
    while DisposalMethod::from_i32(prev_frame.disposal_method)
        == Some(DisposalMethod::RestorePrevious)
    {
        let prev_id = prev_frame.frame_id;
        if prev_id == 0 {
            // IND3: all preceding frames' disposals are kRestorePrevious.
            return FrameDependencyResult {
                has_alpha: true,
                required_frame: NO_FRAME,
            };
        }
        prev_frame = get_frame(prev_id - 1);
    }

    let clear_prev_frame =
        DisposalMethod::from_i32(prev_frame.disposal_method) == Some(DisposalMethod::RestoreBgColor);
    let mut prev_frame_rect = frame_rect_on_screen(prev_frame.frame_rect, screen_rect);

    if clear_prev_frame {
        if prev_frame_rect == screen_rect || prev_frame.required_frame == NO_FRAME {
            // IND4: prevFrame clears to bg and covers screen or is independent.
            return FrameDependencyResult {
                has_alpha: true,
                required_frame: NO_FRAME,
            };
        }
    }

    if reports_alpha && blend_with_prev_frame {
        // DEP5: reports alpha and blends with previous frame.
        return FrameDependencyResult {
            has_alpha: prev_frame.has_alpha || clear_prev_frame,
            required_frame: prev_frame.frame_id,
        };
    }

    while irect_contains(frame_rect, prev_frame_rect) {
        let prev_required_frame = prev_frame.required_frame;
        if prev_required_frame == NO_FRAME {
            // IND6: covers rects of all preceding frames back to independent frame.
            return FrameDependencyResult {
                has_alpha: true,
                required_frame: NO_FRAME,
            };
        }
        prev_frame = get_frame(prev_required_frame);
        prev_frame_rect = frame_rect_on_screen(prev_frame.frame_rect, screen_rect);
    }

    // DEP7: unconditional.
    let required_frame = prev_frame.frame_id;
    if DisposalMethod::from_i32(prev_frame.disposal_method) == Some(DisposalMethod::RestoreBgColor)
    {
        return FrameDependencyResult {
            has_alpha: true,
            required_frame,
        };
    }

    FrameDependencyResult {
        has_alpha: prev_frame.has_alpha || (reports_alpha && !blend_with_prev_frame),
        required_frame,
    }
}
