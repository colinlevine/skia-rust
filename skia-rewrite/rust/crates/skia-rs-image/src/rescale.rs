//! Safe Rust port of `src/image/SkRescaleAndReadPixels.cpp` multi-step rescaling planner.

use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::IRect;

use crate::image::image_info_min_row_bytes;

/// Maximum possible rescale steps for valid Skia image dimensions (`<= 2^29 - 1`, so `<= 29` steps).
pub const RESCALE_MAX_STEPS: usize = 32;

/// Matches `SkImage::RescaleGamma` (`enum class RescaleGamma : bool { kSrc, kLinear }`).
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RescaleGamma {
    Src = 0,
    Linear = 1,
}

impl RescaleGamma {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Src),
            1 => Some(Self::Linear),
            _ => None,
        }
    }
}

/// Matches `SkImage::RescaleMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RescaleMode {
    Nearest = 0,
    Linear = 1,
    RepeatedLinear = 2,
    RepeatedCubic = 3,
}

impl RescaleMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Nearest),
            1 => Some(Self::Linear),
            2 => Some(Self::RepeatedLinear),
            3 => Some(Self::RepeatedCubic),
            _ => None,
        }
    }
}

/// Sampling configuration selected by `rescaling_to_sampling` in `SkRescaleAndReadPixels.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RescaleSamplingKind {
    /// Default `SkSamplingOptions()` (nearest filter, no cubic).
    Nearest = 0,
    /// `SkSamplingOptions(SkFilterMode::kLinear)`.
    Linear = 1,
    /// `SkSamplingOptions(SkCubicResampler::Mitchell())` = `{1.0f/3, 1.0f/3}`.
    MitchellCubic = 2,
}

/// One intermediate or final draw step in `SkRescaleAndReadPixels`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RescaleStep {
    pub src_rect: IRect,
    pub dst_width: i32,
    pub dst_height: i32,
    pub use_result_info: bool,
    pub strict_constraint: bool,
}

/// Full execution plan for `SkRescaleAndReadPixels`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RescalePlan {
    pub sampling: RescaleSamplingKind,
    pub needs_linearize: bool,
    pub linearize_src_x: i32,
    pub linearize_src_y: i32,
    pub linearize_width: i32,
    pub linearize_height: i32,
    pub step_count: usize,
    pub steps: [RescaleStep; RESCALE_MAX_STEPS],
    pub final_read_x: i32,
    pub final_read_y: i32,
    pub final_row_bytes: usize,
    pub final_byte_size: usize,
}

/// Exact port of the step schedule, sampling selection, linearization check, and final buffer
/// sizing in `SkRescaleAndReadPixels`.
pub fn build_rescale_plan(
    src_rect: IRect,
    dst_width: i32,
    dst_height: i32,
    dst_ct: ColorType,
    src_has_cs: bool,
    src_cs_is_linear: bool,
    rescale_gamma: RescaleGamma,
    mut rescale_mode: RescaleMode,
) -> RescalePlan {
    let mut src_w = src_rect.width();
    let mut src_h = src_rect.height();

    let sx: f32 = (dst_width as f32) / (src_w as f32);
    let sy: f32 = (dst_height as f32) / (src_h as f32);

    let (mut steps_x, mut steps_y): (i32, i32) = if rescale_mode != RescaleMode::Nearest {
        let step_x = if sx > 1.0f32 {
            sx.log2().ceil()
        } else {
            sx.log2().floor()
        } as i32;
        let step_y = if sy > 1.0f32 {
            sy.log2().ceil()
        } else {
            sy.log2().floor()
        } as i32;
        (step_x, step_y)
    } else {
        ((sx != 1.0f32) as i32, (sy != 1.0f32) as i32)
    };

    if steps_x < 0 || steps_y < 0 {
        if rescale_mode != RescaleMode::Nearest {
            rescale_mode = RescaleMode::RepeatedLinear;
        }
    }

    // Exact match to `rescaling_to_sampling` in SkRescaleAndReadPixels.cpp:
    // kRepeatedLinear -> Linear, kRepeatedCubic -> MitchellCubic, everything else -> default Nearest.
    let sampling = match rescale_mode {
        RescaleMode::RepeatedLinear => RescaleSamplingKind::Linear,
        RescaleMode::RepeatedCubic => RescaleSamplingKind::MitchellCubic,
        RescaleMode::Nearest | RescaleMode::Linear => RescaleSamplingKind::Nearest,
    };

    let needs_linearize =
        rescale_gamma == RescaleGamma::Linear && src_has_cs && !src_cs_is_linear;

    let mut src_x = if needs_linearize { 0 } else { src_rect.left };
    let mut src_y = if needs_linearize { 0 } else { src_rect.top };
    let mut strict_constraint = !needs_linearize;

    let mut steps = [RescaleStep::default(); RESCALE_MAX_STEPS];
    let mut step_count = 0usize;

    while (steps_x != 0 || steps_y != 0) && step_count < RESCALE_MAX_STEPS {
        let mut next_w = dst_width;
        let mut next_h = dst_height;

        if steps_x < 0 {
            next_w = dst_width.wrapping_shl((-steps_x - 1) as u32);
            steps_x += 1;
        } else if steps_x != 0 {
            if steps_x > 1 {
                next_w = src_w.wrapping_mul(2);
            }
            steps_x -= 1;
        }

        if steps_y < 0 {
            next_h = dst_height.wrapping_shl((-steps_y - 1) as u32);
            steps_y += 1;
        } else if steps_y != 0 {
            if steps_y > 1 {
                next_h = src_h.wrapping_mul(2);
            }
            steps_y -= 1;
        }

        let use_result_info = steps_x == 0 && steps_y == 0;
        steps[step_count] = RescaleStep {
            src_rect: IRect::make_ltrb(src_x, src_y, src_x + src_w, src_y + src_h),
            dst_width: next_w,
            dst_height: next_h,
            use_result_info,
            strict_constraint,
        };
        step_count += 1;

        src_x = 0;
        src_y = 0;
        src_w = next_w;
        src_h = next_h;
        strict_constraint = false;
    }

    let final_row_bytes = image_info_min_row_bytes(dst_width, dst_ct);
    let final_byte_size = (dst_height.max(0) as usize).saturating_mul(final_row_bytes);

    RescalePlan {
        sampling,
        needs_linearize,
        linearize_src_x: src_rect.left,
        linearize_src_y: src_rect.top,
        linearize_width: src_rect.width(),
        linearize_height: src_rect.height(),
        step_count,
        steps,
        final_read_x: src_x,
        final_read_y: src_y,
        final_row_bytes,
        final_byte_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_no_steps_preserves_src_offset() {
        let plan = build_rescale_plan(
            IRect::from_ltrb(5, 7, 25, 37),
            20,
            30,
            ColorType::Rgba8888,
            true,
            false,
            RescaleGamma::Src,
            RescaleMode::RepeatedLinear,
        );
        assert!(!plan.needs_linearize);
        assert_eq!(plan.step_count, 0);
        assert_eq!(plan.final_read_x, 5);
        assert_eq!(plan.final_read_y, 7);
        assert_eq!(plan.final_row_bytes, 80);
        assert_eq!(plan.final_byte_size, 2400);
        assert_eq!(plan.sampling, RescaleSamplingKind::Linear);
    }

    #[test]
    fn linearize_resets_origin_even_with_zero_steps() {
        let plan = build_rescale_plan(
            IRect::from_ltrb(5, 7, 25, 37),
            20,
            30,
            ColorType::Rgba8888,
            true,
            false,
            RescaleGamma::Linear,
            RescaleMode::RepeatedCubic,
        );
        assert!(plan.needs_linearize);
        assert_eq!(plan.linearize_src_x, 5);
        assert_eq!(plan.linearize_src_y, 7);
        assert_eq!(plan.step_count, 0);
        assert_eq!(plan.final_read_x, 0);
        assert_eq!(plan.final_read_y, 0);
        assert_eq!(plan.sampling, RescaleSamplingKind::MitchellCubic);
    }

    #[test]
    fn linearize_ignored_when_already_linear_or_no_cs() {
        let p_linear = build_rescale_plan(
            IRect::from_wh(16, 16),
            8,
            8,
            ColorType::Rgba8888,
            true,
            true,
            RescaleGamma::Linear,
            RescaleMode::RepeatedLinear,
        );
        assert!(!p_linear.needs_linearize);

        let p_no_cs = build_rescale_plan(
            IRect::from_wh(16, 16),
            8,
            8,
            ColorType::Rgba8888,
            false,
            false,
            RescaleGamma::Linear,
            RescaleMode::RepeatedLinear,
        );
        assert!(!p_no_cs.needs_linearize);
    }

    #[test]
    fn multistep_downscale_downgrades_cubic_to_linear() {
        // 100x100 -> 10x10: sx = 0.1, floor(log2(0.1)) = -4 -> 4 steps:
        // step 0: 100 -> 10 << 3 = 80 (strict=true)
        // step 1: 80  -> 10 << 2 = 40 (strict=false)
        // step 2: 40  -> 10 << 1 = 20 (strict=false)
        // step 3: 20  -> 10 << 0 = 10 (use_result_info=true)
        let plan = build_rescale_plan(
            IRect::from_ltrb(2, 3, 102, 103),
            10,
            10,
            ColorType::Rgba8888,
            true,
            false,
            RescaleGamma::Src,
            RescaleMode::RepeatedCubic,
        );
        assert_eq!(plan.sampling, RescaleSamplingKind::Linear);
        assert_eq!(plan.step_count, 4);
        assert_eq!(plan.steps[0].src_rect, IRect::from_ltrb(2, 3, 102, 103));
        assert_eq!((plan.steps[0].dst_width, plan.steps[0].dst_height), (80, 80));
        assert!(plan.steps[0].strict_constraint);
        assert!(!plan.steps[0].use_result_info);

        assert_eq!(plan.steps[1].src_rect, IRect::from_wh(80, 80));
        assert_eq!((plan.steps[1].dst_width, plan.steps[1].dst_height), (40, 40));
        assert!(!plan.steps[1].strict_constraint);

        assert_eq!((plan.steps[2].dst_width, plan.steps[2].dst_height), (20, 20));
        assert!(!plan.steps[2].use_result_info);

        assert_eq!((plan.steps[3].dst_width, plan.steps[3].dst_height), (10, 10));
        assert!(plan.steps[3].use_result_info);
        assert_eq!((plan.final_read_x, plan.final_read_y), (0, 0));
    }

    #[test]
    fn multistep_upscale_cubic_doubles_each_step() {
        // 10x10 -> 50x20:
        // sx = 5.0 -> ceil(log2(5)) = 3 steps: 10->20->40->50
        // sy = 2.0 -> ceil(log2(2)) = 1 step:  10->20 (then stays 20)
        let plan = build_rescale_plan(
            IRect::from_wh(10, 10),
            50,
            20,
            ColorType::Rgba8888,
            false,
            false,
            RescaleGamma::Src,
            RescaleMode::RepeatedCubic,
        );
        assert_eq!(plan.sampling, RescaleSamplingKind::MitchellCubic);
        assert_eq!(plan.step_count, 3);
        assert_eq!((plan.steps[0].dst_width, plan.steps[0].dst_height), (20, 20));
        assert_eq!((plan.steps[1].dst_width, plan.steps[1].dst_height), (40, 20));
        assert_eq!((plan.steps[2].dst_width, plan.steps[2].dst_height), (50, 20));
        assert!(plan.steps[2].use_result_info);
    }

    #[test]
    fn nearest_is_always_single_step_when_resized() {
        let plan = build_rescale_plan(
            IRect::from_wh(100, 100),
            5,
            5,
            ColorType::Rgba8888,
            false,
            false,
            RescaleGamma::Src,
            RescaleMode::Nearest,
        );
        assert_eq!(plan.sampling, RescaleSamplingKind::Nearest);
        assert_eq!(plan.step_count, 1);
        assert_eq!((plan.steps[0].dst_width, plan.steps[0].dst_height), (5, 5));
        assert!(plan.steps[0].use_result_info);
    }
}
