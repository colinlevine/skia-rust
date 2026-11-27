//! Safe Rust GIF frame record and frame holder (`SkWuffsFrame` / `SkWuffsFrameHolder`).
//!
//! Delegates frame dependency and composited-alpha calculation directly to
//! `skia_rs_codec_core::frame_holder::{compute_alpha_and_required_frame, fill_frame_info}`.

use skia_rs_codec_core::{
    frame_holder::{
        compute_alpha_and_required_frame, fill_frame_info, FrameInfoFields, FrameSummary,
    },
    types::{Blend, DisposalMethod, EncodedAlpha, NO_FRAME},
};
use skia_rs_geometry::IRect;

use crate::{
    format::{wuffs_disposal_to_skia, wuffs_duration_to_ms},
    types::{IsAnimated, WuffsFrameConfigInput},
};

/// Safe Rust representation of an `SkWuffsFrame`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GifFrame {
    pub frame_id: i32,
    pub io_position: u64,
    pub reported_alpha: i32,
    pub has_alpha: bool,
    pub required_frame: i32,
    pub frame_rect: IRect,
    pub disposal_method: i32,
    pub duration_ms: i32,
    pub blend: i32,
}

impl GifFrame {
    /// Converts a `WuffsFrameConfigInput` into a `GifFrame` prior to dependency computation.
    ///
    /// Exact port of `SkWuffsFrame::SkWuffsFrame(wuffs_base__frame_config* fc)`.
    #[inline]
    pub fn from_wuffs_config(fc: WuffsFrameConfigInput) -> Self {
        let reported_alpha = if fc.opaque_within_bounds {
            EncodedAlpha::Opaque
        } else {
            EncodedAlpha::Unpremul
        };
        let disposal_method = wuffs_disposal_to_skia(fc.disposal);
        let duration_ms = wuffs_duration_to_ms(fc.duration_flicks);
        let blend = if fc.overwrite_instead_of_blend {
            Blend::Src
        } else {
            Blend::SrcOver
        };
        let x = fc.bounds.min_incl_x as i32;
        let y = fc.bounds.min_incl_y as i32;
        let w = fc.bounds.width() as i32;
        let h = fc.bounds.height() as i32;
        let frame_rect = IRect {
            left: x,
            top: y,
            right: x.saturating_add(w),
            bottom: y.saturating_add(h),
        };
        Self {
            frame_id: fc.index as i32,
            io_position: fc.io_position,
            reported_alpha: reported_alpha as i32,
            has_alpha: false,
            required_frame: -2, // SkFrame::kUninitialized
            frame_rect,
            disposal_method: disposal_method as i32,
            duration_ms,
            blend: blend as i32,
        }
    }

    #[inline]
    pub const fn to_summary(self) -> FrameSummary {
        FrameSummary {
            frame_id: self.frame_id,
            reported_alpha: self.reported_alpha,
            has_alpha: self.has_alpha,
            required_frame: self.required_frame,
            disposal_method: self.disposal_method,
            blend: self.blend,
            frame_rect: self.frame_rect,
        }
    }

    #[inline]
    pub const fn is_independent(self) -> bool {
        self.required_frame == NO_FRAME
    }

    #[inline]
    pub const fn reported_alpha_enum(self) -> EncodedAlpha {
        match EncodedAlpha::from_i32(self.reported_alpha) {
            Some(a) => a,
            None => EncodedAlpha::Unpremul,
        }
    }

    #[inline]
    pub const fn disposal_enum(self) -> DisposalMethod {
        match DisposalMethod::from_i32(self.disposal_method) {
            Some(d) => d,
            None => DisposalMethod::Keep,
        }
    }

    #[inline]
    pub const fn blend_enum(self) -> Blend {
        match Blend::from_i32(self.blend) {
            Some(b) => b,
            None => Blend::SrcOver,
        }
    }
}

/// Safe Rust GIF frame holder managing the discovered frame table, completeness flag,
/// and high-water mark of fully received frames.
#[derive(Clone, Debug)]
pub struct GifFrameHolder {
    screen_width: i32,
    screen_height: i32,
    frames: Vec<GifFrame>,
    frames_complete: bool,
    num_fully_received_frames: u64,
}

impl GifFrameHolder {
    #[inline]
    pub fn new(screen_width: i32, screen_height: i32) -> Self {
        Self {
            screen_width,
            screen_height,
            frames: Vec::new(),
            frames_complete: false,
            num_fully_received_frames: 0,
        }
    }

    #[inline]
    pub fn screen_width(&self) -> i32 {
        self.screen_width
    }

    #[inline]
    pub fn screen_height(&self) -> i32 {
        self.screen_height
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    #[inline]
    pub fn frames_complete(&self) -> bool {
        self.frames_complete
    }

    #[inline]
    pub fn set_frames_complete(&mut self, complete: bool) {
        self.frames_complete = complete;
    }

    #[inline]
    pub fn num_fully_received_frames(&self) -> u64 {
        self.num_fully_received_frames
    }

    /// Exact port of `SkWuffsCodec::updateNumFullyReceivedFrames`:
    /// keeps the highest `num_decoded_frames` ever observed across seeks.
    #[inline]
    pub fn update_num_fully_received_frames(&mut self, num_decoded_frames: u64) {
        if self.num_fully_received_frames < num_decoded_frames {
            self.num_fully_received_frames = num_decoded_frames;
        }
    }

    #[inline]
    pub fn get_frame(&self, index: i32) -> Option<&GifFrame> {
        if index < 0 {
            return None;
        }
        self.frames.get(index as usize)
    }

    /// Appends a frame decoded from Wuffs and computes `has_alpha` and `required_frame`
    /// using `skia_rs_codec_core::frame_holder::compute_alpha_and_required_frame`.
    pub fn push_from_wuffs_config(&mut self, fc: WuffsFrameConfigInput) -> GifFrame {
        let mut frame = GifFrame::from_wuffs_config(fc);
        let summary = frame.to_summary();
        let dep = compute_alpha_and_required_frame(
            self.screen_width,
            self.screen_height,
            summary,
            |prev_idx| self.frames[prev_idx as usize].to_summary(),
        );
        frame.has_alpha = dep.has_alpha;
        frame.required_frame = dep.required_frame;
        self.frames.push(frame);
        frame
    }

    /// Exact port of `SkWuffsCodec::onGetFrameInfo` (assuming `can_seek == true`).
    pub fn fill_frame_info(&self, index: i32) -> Option<FrameInfoFields> {
        let f = self.get_frame(index)?;
        let fully_received = (index as u64) < self.num_fully_received_frames;
        Some(fill_frame_info(
            f.required_frame,
            f.duration_ms,
            fully_received,
            f.has_alpha,
            f.reported_alpha_enum(),
            f.disposal_method,
            f.blend,
            f.frame_rect,
        ))
    }

    /// Exact port of `SkWuffsCodec::onIsAnimated`.
    #[inline]
    pub fn is_animated(&self) -> IsAnimated {
        if self.frames.len() > 1 {
            IsAnimated::Yes
        } else if self.frames_complete {
            IsAnimated::No
        } else {
            IsAnimated::Unknown
        }
    }
}
