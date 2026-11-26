//! Safe Rust port of `SkGradientBaseShader.cpp`.
//!
//! Covers:
//! - Gradient serialization flag packing / unpacking
//! - Constructor stop normalization, implicit stop insertion, uniform-stop detection,
//!   and duplicate stop deduplication
//! - Gradient fill stage precomputation (2-stop, evenly-spaced, arbitrary stops)
//! - Special color space conversions (HSL, HWB, Lab, LCH/HCL, OKLab, OKLCH/OKHCL),
//!   powerless-hue stop splitting, CSS hue interpolation methods, and premultiplication
//! - Interpolated-to-destination stage planning
//! - Luminance color, average gradient color, validation, and degenerate fallback logic

use skia_rs_color::color::Color4f;
use skia_rs_color::image_info::AlphaType;
use skia_rs_geometry::{
    cpp_max_f32, cpp_min_f32, pin_f32, scalar_nearly_equal, SK_SCALAR_NEARLY_ZERO,
};

use crate::{radians_to_degrees, TileMode, TILE_MODE_COUNT};

/// Matches `SkGradientBaseShader::kDegenerateThreshold = SK_Scalar1 / (1 << 15)`.
pub const DEGENERATE_THRESHOLD: f32 = 1.0_f32 / (1 << 15) as f32;

/// Mirrors `SkGradient::Interpolation::ColorSpace`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InterpolationColorSpace {
    Destination = 0,
    SRGBLinear = 1,
    Lab = 2,
    OKLab = 3,
    OKLabGamutMap = 4,
    LCH = 5,
    OKLCH = 6,
    OKLCHGamutMap = 7,
    SRGB = 8,
    HSL = 9,
    HWB = 10,
    DisplayP3 = 11,
    Rec2020 = 12,
    ProphotoRGB = 13,
    A98RGB = 14,
}

pub const INTERPOLATION_COLOR_SPACE_COUNT: usize = 15;

impl InterpolationColorSpace {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Destination),
            1 => Some(Self::SRGBLinear),
            2 => Some(Self::Lab),
            3 => Some(Self::OKLab),
            4 => Some(Self::OKLabGamutMap),
            5 => Some(Self::LCH),
            6 => Some(Self::OKLCH),
            7 => Some(Self::OKLCHGamutMap),
            8 => Some(Self::SRGB),
            9 => Some(Self::HSL),
            10 => Some(Self::HWB),
            11 => Some(Self::DisplayP3),
            12 => Some(Self::Rec2020),
            13 => Some(Self::ProphotoRGB),
            14 => Some(Self::A98RGB),
            _ => None,
        }
    }
}

/// Mirrors `SkGradient::Interpolation::HueMethod`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HueMethod {
    Shorter = 0,
    Longer = 1,
    Increasing = 2,
    Decreasing = 3,
}

pub const HUE_METHOD_COUNT: usize = 4;

impl HueMethod {
    #[inline]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Shorter),
            1 => Some(Self::Longer),
            2 => Some(Self::Increasing),
            3 => Some(Self::Decreasing),
            _ => None,
        }
    }
}

/// Mirrors `SkGradient::Interpolation` (exact layout: 3 bytes).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GradientInterpolation {
    pub in_premul: u8,
    pub color_space: u8,
    pub hue_method: u8,
}

impl Default for GradientInterpolation {
    fn default() -> Self {
        Self {
            in_premul: 0,
            color_space: InterpolationColorSpace::Destination as u8,
            hue_method: HueMethod::Shorter as u8,
        }
    }
}

// Serialization bit flags matching `GradientSerializationFlags` in `SkGradientBaseShader.cpp`.
pub const GSF_HAS_POSITION: u32 = 0x8000_0000;
pub const GSF_HAS_LEGACY_LOCAL_MATRIX: u32 = 0x4000_0000;
pub const GSF_HAS_COLOR_SPACE: u32 = 0x2000_0000;
pub const GSF_TILE_MODE_SHIFT: u32 = 8;
pub const GSF_TILE_MODE_MASK: u32 = 0xF;
pub const GSF_INTERP_CS_SHIFT: u32 = 4;
pub const GSF_INTERP_CS_MASK: u32 = 0xF;
pub const GSF_INTERP_HUE_SHIFT: u32 = 1;
pub const GSF_INTERP_HUE_MASK: u32 = 0x7;
pub const GSF_INTERP_IN_PREMUL: u32 = 0x1;

/// Packs `SkGradientBaseShader::flatten` flags.
#[inline]
pub const fn pack_serialization_flags(
    has_positions: bool,
    has_color_space: bool,
    tile_mode: u32,
    interp: GradientInterpolation,
) -> u32 {
    let mut flags = 0u32;
    if has_positions {
        flags |= GSF_HAS_POSITION;
    }
    if has_color_space {
        flags |= GSF_HAS_COLOR_SPACE;
    }
    if interp.in_premul != 0 {
        flags |= GSF_INTERP_IN_PREMUL;
    }
    flags |= (tile_mode & GSF_TILE_MODE_MASK) << GSF_TILE_MODE_SHIFT;
    flags |= ((interp.color_space as u32) & GSF_INTERP_CS_MASK) << GSF_INTERP_CS_SHIFT;
    flags |= ((interp.hue_method as u32) & GSF_INTERP_HUE_MASK) << GSF_INTERP_HUE_SHIFT;
    flags
}

/// Unpacked flags from `SkGradientScope::unflatten`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnpackedGradientFlags {
    pub tile_mode: u32,
    pub interpolation: GradientInterpolation,
    pub has_positions: bool,
    pub has_color_space: bool,
    pub has_legacy_local_matrix: bool,
}

/// Unpacks `SkGradientScope::unflatten` flags.
#[inline]
pub const fn unpack_serialization_flags(flags: u32) -> UnpackedGradientFlags {
    UnpackedGradientFlags {
        tile_mode: (flags >> GSF_TILE_MODE_SHIFT) & GSF_TILE_MODE_MASK,
        interpolation: GradientInterpolation {
            in_premul: if (flags & GSF_INTERP_IN_PREMUL) != 0 { 1 } else { 0 },
            color_space: ((flags >> GSF_INTERP_CS_SHIFT) & GSF_INTERP_CS_MASK) as u8,
            hue_method: ((flags >> GSF_INTERP_HUE_SHIFT) & GSF_INTERP_HUE_MASK) as u8,
        },
        has_positions: (flags & GSF_HAS_POSITION) != 0,
        has_color_space: (flags & GSF_HAS_COLOR_SPACE) != 0,
        has_legacy_local_matrix: (flags & GSF_HAS_LEGACY_LOCAL_MATRIX) != 0,
    }
}

/// Computes the slice `(start_index, count)` to serialize in `SkGradientBaseShader::flatten`,
/// omitting implicit first/last stops.
#[inline]
pub const fn serialization_slice(
    color_count: usize,
    first_stop_is_implicit: bool,
    last_stop_is_implicit: bool,
) -> (usize, usize) {
    let mut start = 0usize;
    let mut count = color_count;
    if first_stop_is_implicit {
        count = count.saturating_sub(1);
        start += 1;
    }
    if last_stop_is_implicit {
        count = count.saturating_sub(1);
    }
    (start, count)
}

/// Matches `SkGradientBaseShader::ValidGradient`.
#[inline]
pub const fn valid_gradient(
    has_colors_data: bool,
    color_count: usize,
    tile_mode: u32,
    interp: GradientInterpolation,
) -> bool {
    has_colors_data
        && color_count > 0
        && (tile_mode as usize) < TILE_MODE_COUNT
        && (interp.color_space as usize) < INTERPOLATION_COLOR_SPACE_COUNT
        && (interp.hue_method as usize) < HUE_METHOD_COUNT
}

/// Matches `SkGradientBaseShader::isOpaque()`.
#[inline]
pub const fn gradient_is_opaque(colors_are_opaque: bool, tile_mode: TileMode) -> bool {
    colors_are_opaque && !matches!(tile_mode, TileMode::Decal)
}

/// Matches `SkGradientBaseShader::onAsLuminanceColor`.
pub fn gradient_luminance_color(colors: &[Color4f]) -> Color4f {
    let mut r = 0.0_f32;
    let mut g = 0.0_f32;
    let mut b = 0.0_f32;
    for c in colors {
        r += c.r;
        g += c.g;
        b += c.b;
    }
    let scale = 1.0_f32 / (colors.len() as f32);
    Color4f {
        r: r * scale,
        g: g * scale,
        b: b * scale,
        a: 1.0,
    }
}

/// Exact port of `SkGradientBaseShader::SkGradientBaseShader` stop normalization,
/// implicit bracketing at 0 and 1, uniform stop detection, and duplicate-stop pruning.
pub struct PreparedGradientStops {
    pub colors: Vec<Color4f>,
    pub positions: Option<Vec<f32>>,
    pub first_stop_is_implicit: bool,
    pub last_stop_is_implicit: bool,
    pub colors_are_opaque: bool,
}

pub fn prepare_gradient_stops(
    colors: &[Color4f],
    pos: &[f32],
    tile_mode: TileMode,
) -> PreparedGradientStops {
    let mut first_stop_is_implicit = false;
    let mut last_stop_is_implicit = false;
    let mut colors_are_opaque = true;

    let mut color_count = colors.len();
    if !pos.is_empty() {
        first_stop_is_implicit = pos[0] > 0.0;
        last_stop_is_implicit = pos[pos.len() - 1] != 1.0;
        color_count += usize::from(first_stop_is_implicit) + usize::from(last_stop_is_implicit);
    }

    let mut out_colors = Vec::with_capacity(color_count);
    if first_stop_is_implicit {
        out_colors.push(colors[0]);
    }
    for &c in colors {
        out_colors.push(c);
        colors_are_opaque = colors_are_opaque && (c.a == 1.0);
    }
    if last_stop_is_implicit {
        out_colors.push(colors[colors.len() - 1]);
    }

    if pos.is_empty() {
        return PreparedGradientStops {
            colors: out_colors,
            positions: None,
            first_stop_is_implicit,
            last_stop_is_implicit,
            colors_are_opaque,
        };
    }

    let mut out_pos = Vec::with_capacity(color_count);
    let mut prev = 0.0_f32;
    out_pos.push(prev);

    let start_index = if first_stop_is_implicit { 0 } else { 1 };
    let count = pos.len() + usize::from(last_stop_is_implicit);

    let mut uniform_stops = true;
    let uniform_step = pos[start_index] - prev;
    for i in start_index..count {
        let mut curr = 1.0_f32;
        if i != pos.len() {
            curr = pin_f32(pos[i], prev, 1.0_f32);
            if curr == 1.0_f32 && last_stop_is_implicit {
                last_stop_is_implicit = false;
            }
        }
        uniform_stops &= scalar_nearly_equal(uniform_step, curr - prev, SK_SCALAR_NEARLY_ZERO);
        prev = curr;
        out_pos.push(curr);
    }

    if uniform_stops {
        return PreparedGradientStops {
            colors: out_colors,
            positions: None,
            first_stop_is_implicit,
            last_stop_is_implicit,
            colors_are_opaque,
        };
    }

    // Remove duplicate stops with more than two of the same stop,
    // keeping leftmost and rightmost stop colors.
    let mut i = 0usize;
    let mut deduped_count = 0usize;
    for j in 1..=color_count {
        if j == color_count || out_pos[i] != out_pos[j] {
            let dup_stop = j - i > 1;

            let ignore_leftmost =
                dup_stop && tile_mode != TileMode::Clamp && out_pos[i] == 0.0;
            if !ignore_leftmost {
                out_pos[deduped_count] = out_pos[i];
                out_colors[deduped_count] = out_colors[i];
                deduped_count += 1;
            }

            let ignore_rightmost = tile_mode != TileMode::Clamp && out_pos[j - 1] == 1.0;
            if dup_stop && !ignore_rightmost {
                out_pos[deduped_count] = out_pos[j - 1];
                out_colors[deduped_count] = out_colors[j - 1];
                deduped_count += 1;
            }
            i = j;
        }
    }
    out_colors.truncate(deduped_count);
    out_pos.truncate(deduped_count);

    PreparedGradientStops {
        colors: out_colors,
        positions: Some(out_pos),
        first_stop_is_implicit,
        last_stop_is_implicit,
        colors_are_opaque,
    }
}

/// Fills factor/bias/ts arrays for `SkGradientBaseShader::AppendGradientFillStages`.
/// Returns `(stage_kind, stop_count)` where `stage_kind`:
/// - `0` = `evenly_spaced_2_stop_gradient`
/// - `1` = `evenly_spaced_gradient`
/// - `2` = `gradient`
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GradientFillStageKind {
    EvenlySpaced2Stop = 0,
    EvenlySpaced = 1,
    Arbitrary = 2,
}

/// Computes `factor` and `bias` for `evenly_spaced_2_stop_gradient`.
#[inline]
pub fn compute_2_stop_gradient(c_l: Color4f, c_r: Color4f) -> ([f32; 4], [f32; 4]) {
    let factor = [c_r.r - c_l.r, c_r.g - c_l.g, c_r.b - c_l.b, c_r.a - c_l.a];
    let bias = [c_l.r, c_l.g, c_l.b, c_l.a];
    (factor, bias)
}

#[inline]
fn write_stop_color(
    factors: &mut [&mut [f32]; 4],
    biases: &mut [&mut [f32]; 4],
    stop: usize,
    f: [f32; 4],
    b: [f32; 4],
) {
    for ch in 0..4 {
        factors[ch][stop] = f[ch];
        biases[ch][stop] = b[ch];
    }
}

/// Computes factor/bias tables for `evenly_spaced_gradient` (`positions == nullptr`).
/// Returns `stop_count`.
pub fn fill_evenly_spaced_stops(
    pm_colors: &[Color4f],
    factors: &mut [&mut [f32]; 4],
    biases: &mut [&mut [f32]; 4],
) -> usize {
    let stop_count = pm_colors.len();
    let gap_count = (stop_count - 1) as f32;

    let mut c_l = pm_colors[0];
    for i in 0..(stop_count - 1) {
        let c_r = pm_colors[i + 1];
        let f = [
            (c_r.r - c_l.r) * gap_count,
            (c_r.g - c_l.g) * gap_count,
            (c_r.b - c_l.b) * gap_count,
            (c_r.a - c_l.a) * gap_count,
        ];
        let t = (i as f32) / gap_count;
        let b = [
            c_l.r - f[0] * t,
            c_l.g - f[1] * t,
            c_l.b - f[2] * t,
            c_l.a - f[3] * t,
        ];
        write_stop_color(factors, biases, i, f, b);
        c_l = c_r;
    }
    write_stop_color(
        factors,
        biases,
        stop_count - 1,
        [0.0; 4],
        [c_l.r, c_l.g, c_l.b, c_l.a],
    );
    stop_count
}

/// Computes factor/bias/ts tables for arbitrary-position `gradient`.
/// Returns `stop_count`.
pub fn fill_arbitrary_stops(
    pm_colors: &[Color4f],
    positions: &[f32],
    factors: &mut [&mut [f32]; 4],
    biases: &mut [&mut [f32]; 4],
    ts: &mut [f32],
) -> usize {
    let count = pm_colors.len();
    let (first_stop, last_stop) = if count > 2 {
        let first = if pm_colors[0] != pm_colors[1] { 0 } else { 1 };
        let last = if pm_colors[count - 2] != pm_colors[count - 1] {
            count - 1
        } else {
            count - 2
        };
        (first, last)
    } else {
        (0, 1)
    };

    let mut stop_count = 0usize;
    let mut t_l = positions[first_stop];
    let mut c_l = pm_colors[first_stop];
    write_stop_color(
        factors,
        biases,
        stop_count,
        [0.0; 4],
        [c_l.r, c_l.g, c_l.b, c_l.a],
    );
    stop_count += 1;

    for i in first_stop..last_stop {
        let t_r = positions[i + 1];
        let c_r = pm_colors[i + 1];
        if t_l < t_r {
            let c_scale = 1.0_f32 / (t_r - t_l);
            if c_scale.is_finite() {
                let f = [
                    (c_r.r - c_l.r) * c_scale,
                    (c_r.g - c_l.g) * c_scale,
                    (c_r.b - c_l.b) * c_scale,
                    (c_r.a - c_l.a) * c_scale,
                ];
                let b = [
                    c_l.r - f[0] * t_l,
                    c_l.g - f[1] * t_l,
                    c_l.b - f[2] * t_l,
                    c_l.a - f[3] * t_l,
                ];
                ts[stop_count] = t_l;
                write_stop_color(factors, biases, stop_count, f, b);
                stop_count += 1;
            }
        }
        t_l = t_r;
        c_l = c_r;
    }

    ts[stop_count] = t_l;
    write_stop_color(
        factors,
        biases,
        stop_count,
        [0.0; 4],
        [c_l.r, c_l.g, c_l.b, c_l.a],
    );
    stop_count += 1;
    stop_count
}

/// Unpremul kind for `AppendInterpolatedToDstStages`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PostInterpUnpremul {
    None = 0,
    Unpremul = 1,
    UnpremulPolar = 2,
}

/// Exotic-space-to-intermediate conversion kind for `AppendInterpolatedToDstStages`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PostInterpColorOp {
    None = 0,
    LabToXyz = 1,
    OKLabToLinearSrgb = 2,
    OKLabGamutMapToLinearSrgb = 3,
    LchToLabToXyz = 4,
    OKLchToLabToLinearSrgb = 5,
    OKLchGamutMapToLabToLinearSrgb = 6,
    HslToSrgb = 7,
    HwbToSrgb = 8,
}

/// Plan returned for `SkGradientBaseShader::AppendInterpolatedToDstStages`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InterpolatedToDstPlan {
    pub unpremul: PostInterpUnpremul,
    pub color_op: PostInterpColorOp,
    pub intermediate_alpha_type: AlphaType,
    pub dst_alpha_type: AlphaType,
}

/// Matches `SkGradientBaseShader::AppendInterpolatedToDstStages`.
pub fn plan_interpolated_to_dst(
    colors_are_opaque: bool,
    interp: GradientInterpolation,
) -> InterpolatedToDstPlan {
    let mut color_is_premul = interp.in_premul != 0;
    let cs = InterpolationColorSpace::from_u8(interp.color_space)
        .unwrap_or(InterpolationColorSpace::Destination);

    let unpremul = if color_is_premul && !colors_are_opaque {
        match cs {
            InterpolationColorSpace::Lab
            | InterpolationColorSpace::OKLab
            | InterpolationColorSpace::OKLabGamutMap => {
                color_is_premul = false;
                PostInterpUnpremul::Unpremul
            }
            InterpolationColorSpace::LCH
            | InterpolationColorSpace::OKLCH
            | InterpolationColorSpace::OKLCHGamutMap
            | InterpolationColorSpace::HSL
            | InterpolationColorSpace::HWB => {
                color_is_premul = false;
                PostInterpUnpremul::UnpremulPolar
            }
            _ => PostInterpUnpremul::None,
        }
    } else {
        PostInterpUnpremul::None
    };

    let color_op = match cs {
        InterpolationColorSpace::Lab => PostInterpColorOp::LabToXyz,
        InterpolationColorSpace::OKLab => PostInterpColorOp::OKLabToLinearSrgb,
        InterpolationColorSpace::OKLabGamutMap => PostInterpColorOp::OKLabGamutMapToLinearSrgb,
        InterpolationColorSpace::LCH => PostInterpColorOp::LchToLabToXyz,
        InterpolationColorSpace::OKLCH => PostInterpColorOp::OKLchToLabToLinearSrgb,
        InterpolationColorSpace::OKLCHGamutMap => PostInterpColorOp::OKLchGamutMapToLabToLinearSrgb,
        InterpolationColorSpace::HSL => PostInterpColorOp::HslToSrgb,
        InterpolationColorSpace::HWB => PostInterpColorOp::HwbToSrgb,
        _ => PostInterpColorOp::None,
    };

    let (intermediate_alpha_type, dst_alpha_type) = if colors_are_opaque {
        (AlphaType::Unpremul, AlphaType::Unpremul)
    } else {
        let inter = if color_is_premul {
            AlphaType::Premul
        } else {
            AlphaType::Unpremul
        };
        (inter, AlphaType::Premul)
    };

    InterpolatedToDstPlan {
        unpremul,
        color_op,
        intermediate_alpha_type,
        dst_alpha_type,
    }
}

/// Matches `SkBits2Float(SkFloat2Bits(1.0f) + 1)` used for `kDecal` limit_x in `appendStages`.
#[inline]
pub fn gradient_decal_limit_x() -> f32 {
    f32::from_bits(1.0_f32.to_bits() + 1)
}

/// Identifies which intermediate `SkColorSpace` to construct in `intermediate_color_space`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IntermediateColorSpaceKind {
    Destination = 0,
    SRGBLinear = 1,
    SRGB = 2,
    LinearXYZ = 3,
    DisplayP3 = 4,
    Rec2020 = 5,
    ProphotoRGB = 6,
    A98RGB = 7,
}

#[inline]
pub fn intermediate_color_space_kind(cs: InterpolationColorSpace) -> IntermediateColorSpaceKind {
    match cs {
        InterpolationColorSpace::Destination => IntermediateColorSpaceKind::Destination,
        InterpolationColorSpace::SRGBLinear
        | InterpolationColorSpace::OKLab
        | InterpolationColorSpace::OKLabGamutMap
        | InterpolationColorSpace::OKLCH
        | InterpolationColorSpace::OKLCHGamutMap => IntermediateColorSpaceKind::SRGBLinear,
        InterpolationColorSpace::SRGB
        | InterpolationColorSpace::HSL
        | InterpolationColorSpace::HWB => IntermediateColorSpaceKind::SRGB,
        InterpolationColorSpace::Lab | InterpolationColorSpace::LCH => {
            IntermediateColorSpaceKind::LinearXYZ
        }
        InterpolationColorSpace::DisplayP3 => IntermediateColorSpaceKind::DisplayP3,
        InterpolationColorSpace::Rec2020 => IntermediateColorSpaceKind::Rec2020,
        InterpolationColorSpace::ProphotoRGB => IntermediateColorSpaceKind::ProphotoRGB,
        InterpolationColorSpace::A98RGB => IntermediateColorSpaceKind::A98RGB,
    }
}

// ============================================================================
// Special color space conversions (exact bit-for-bit ports from SkGradientBaseShader.cpp)
// ============================================================================

pub fn srgb_to_hsl(rgb: Color4f) -> (Color4f, bool) {
    let mx = cpp_max_f32(cpp_max_f32(rgb.r, rgb.g), rgb.b);
    let mn = cpp_min_f32(cpp_min_f32(rgb.r, rgb.g), rgb.b);
    let mut hue = 0.0_f32;
    let mut sat = 0.0_f32;
    let light = (mn + mx) / 2.0_f32;
    let d = mx - mn;

    if d != 0.0 {
        sat = if light == 0.0 || light == 1.0 {
            0.0
        } else {
            (mx - light) / cpp_min_f32(light, 1.0 - light)
        };
        if mx == rgb.r {
            hue = (rgb.g - rgb.b) / d + if rgb.g < rgb.b { 6.0 } else { 0.0 };
        } else if mx == rgb.g {
            hue = (rgb.b - rgb.r) / d + 2.0;
        } else {
            hue = (rgb.r - rgb.g) / d + 4.0;
        }
        hue *= 60.0;
    }
    let hue_is_powerless = sat == 0.0;
    (
        Color4f {
            r: hue,
            g: sat * 100.0,
            b: light * 100.0,
            a: rgb.a,
        },
        hue_is_powerless,
    )
}

pub fn srgb_to_hwb(rgb: Color4f) -> (Color4f, bool) {
    let (hsl, hue_is_powerless) = srgb_to_hsl(rgb);
    let white = cpp_min_f32(cpp_min_f32(rgb.r, rgb.g), rgb.b);
    let black = 1.0_f32 - cpp_max_f32(cpp_max_f32(rgb.r, rgb.g), rgb.b);
    (
        Color4f {
            r: hsl.r,
            g: white * 100.0,
            b: black * 100.0,
            a: rgb.a,
        },
        hue_is_powerless,
    )
}

pub fn xyzd50_to_lab(xyz: Color4f) -> Color4f {
    const D50: [f32; 3] = [
        0.3457_f32 / 0.3585_f32,
        1.0_f32,
        (1.0_f32 - 0.3457_f32 - 0.3585_f32) / 0.3585_f32,
    ];
    const E: f32 = 216.0_f32 / 24389.0_f32;
    const K: f32 = 24389.0_f32 / 27.0_f32;

    let comps = [xyz.r, xyz.g, xyz.b];
    let mut f = [0.0_f32; 3];
    for i in 0..3 {
        let v = comps[i] / D50[i];
        f[i] = if v > E {
            v.cbrt()
        } else {
            (K * v + 16.0) / 116.0
        };
    }
    Color4f {
        r: (116.0 * f[1]) - 16.0,
        g: 500.0 * (f[0] - f[1]),
        b: 200.0 * (f[1] - f[2]),
        a: xyz.a,
    }
}

pub fn xyzd50_to_hcl(xyz: Color4f) -> (Color4f, bool) {
    let lab = xyzd50_to_lab(xyz);
    let hue = radians_to_degrees(lab.b.atan2(lab.g));
    let chroma = (lab.g * lab.g + lab.b * lab.b).sqrt();
    const MAX_CHROMA_FOR_POWERLESS_HUE: f32 = 1e-2_f32;
    let hue_is_powerless = chroma <= MAX_CHROMA_FOR_POWERLESS_HUE;
    (
        Color4f {
            r: if hue >= 0.0 { hue } else { hue + 360.0 },
            g: chroma,
            b: lab.r,
            a: xyz.a,
        },
        hue_is_powerless,
    )
}

pub fn lin_srgb_to_oklab(rgb: Color4f) -> Color4f {
    let l = (0.4122214708_f32 * rgb.r + 0.5363325363_f32 * rgb.g + 0.0514459929_f32 * rgb.b).cbrt();
    let m = (0.2119034982_f32 * rgb.r + 0.6806995451_f32 * rgb.g + 0.1073969566_f32 * rgb.b).cbrt();
    let s = (0.0883024619_f32 * rgb.r + 0.2817188376_f32 * rgb.g + 0.6299787005_f32 * rgb.b).cbrt();
    Color4f {
        r: 0.2104542553_f32 * l + 0.7936177850_f32 * m - 0.0040720468_f32 * s,
        g: 1.9779984951_f32 * l - 2.4285922050_f32 * m + 0.4505937099_f32 * s,
        b: 0.0259040371_f32 * l + 0.7827717662_f32 * m - 0.8086757660_f32 * s,
        a: rgb.a,
    }
}

pub fn lin_srgb_to_okhcl(rgb: Color4f) -> (Color4f, bool) {
    let oklab = lin_srgb_to_oklab(rgb);
    let hue = radians_to_degrees(oklab.b.atan2(oklab.g));
    let chroma = (oklab.g * oklab.g + oklab.b * oklab.b).sqrt();
    const MAX_CHROMA_FOR_POWERLESS_HUE: f32 = 1e-6_f32;
    let hue_is_powerless = chroma <= MAX_CHROMA_FOR_POWERLESS_HUE;
    (
        Color4f {
            r: if hue >= 0.0 { hue } else { hue + 360.0 },
            g: chroma,
            b: oklab.r,
            a: rgb.a,
        },
        hue_is_powerless,
    )
}

#[inline]
pub fn premul_polar(hsl: Color4f) -> Color4f {
    Color4f {
        r: hsl.r,
        g: hsl.g * hsl.a,
        b: hsl.b * hsl.a,
        a: hsl.a,
    }
}

#[inline]
pub fn premul_rgb(rgb: Color4f) -> Color4f {
    Color4f {
        r: rgb.r * rgb.a,
        g: rgb.g * rgb.a,
        b: rgb.b * rgb.a,
        a: rgb.a,
    }
}

/// Exact match with `color_space_is_polar` in `SkGradientBaseShader.cpp`.
#[inline]
pub const fn color_space_is_polar(cs: InterpolationColorSpace) -> bool {
    matches!(
        cs,
        InterpolationColorSpace::LCH
            | InterpolationColorSpace::OKLCH
            | InterpolationColorSpace::HSL
            | InterpolationColorSpace::HWB
    )
}

/// Runs steps 3, 4, 5 (and `forceExplicitPositions`) of `SkColor4fXformer::SkColor4fXformer`
/// on the intermediate colors produced by `SkConvertPixels`.
pub fn xform_intermediate_colors(
    mut colors: Vec<Color4f>,
    orig_positions: Option<&[f32]>,
    interp: GradientInterpolation,
    first_stop_is_implicit: bool,
    last_stop_is_implicit: bool,
    force_explicit_positions: bool,
) -> (Vec<Color4f>, Option<Vec<f32>>) {
    let cs = InterpolationColorSpace::from_u8(interp.color_space)
        .unwrap_or(InterpolationColorSpace::Destination);
    let hm = HueMethod::from_u8(interp.hue_method).unwrap_or(HueMethod::Shorter);

    let orig_count = colors.len();
    let mut hue_is_powerless = vec![false; orig_count];
    let mut any_powerless_hue = false;

    // Step 3: Transform to special interpolation color space
    for (i, c) in colors.iter_mut().enumerate() {
        let (converted, powerless) = match cs {
            InterpolationColorSpace::HSL => srgb_to_hsl(*c),
            InterpolationColorSpace::HWB => srgb_to_hwb(*c),
            InterpolationColorSpace::Lab => (xyzd50_to_lab(*c), false),
            InterpolationColorSpace::LCH => xyzd50_to_hcl(*c),
            InterpolationColorSpace::OKLab | InterpolationColorSpace::OKLabGamutMap => {
                (lin_srgb_to_oklab(*c), false)
            }
            InterpolationColorSpace::OKLCH | InterpolationColorSpace::OKLCHGamutMap => {
                lin_srgb_to_okhcl(*c)
            }
            _ => continue,
        };
        *c = converted;
        hue_is_powerless[i] = powerless;
        any_powerless_hue = any_powerless_hue || powerless;
    }

    let mut out_positions: Option<Vec<f32>> = None;

    if any_powerless_hue {
        let mut new_colors = Vec::with_capacity(orig_count * 2);
        let mut new_positions = Vec::with_capacity(orig_count * 2);

        for i in 0..orig_count {
            let cur_color = colors[i];
            let cur_pos = match orig_positions {
                Some(p) => p[i],
                None => (i as f32) / ((orig_count - 1) as f32),
            };
            if !hue_is_powerless[i] {
                new_colors.push(cur_color);
                new_positions.push(cur_pos);
                continue;
            }
            let color_with_hue_from = |color: Color4f, hue_color: Color4f| Color4f {
                r: hue_color.r,
                g: color.g,
                b: color.b,
                a: color.a,
            };
            if i != 0 {
                new_positions.push(cur_pos);
                new_colors.push(color_with_hue_from(cur_color, colors[i - 1]));
            }
            if i != orig_count - 1 {
                new_positions.push(cur_pos);
                new_colors.push(color_with_hue_from(cur_color, colors[i + 1]));
            }
        }
        colors = new_colors;
        out_positions = Some(new_positions);
    }

    let color_count = colors.len();

    // Step 4: Polar hue adjustment
    if color_space_is_polar(cs) && color_count >= 2 {
        let mut delta = 0.0_f32;
        for i in 0..(color_count - 1) {
            let h1 = colors[i].r;
            let h2 = &mut colors[i + 1].r;
            *h2 += delta;
            match hm {
                HueMethod::Shorter => {
                    if *h2 - h1 > 180.0 {
                        *h2 -= 360.0;
                        delta -= 360.0;
                    } else if *h2 - h1 < -180.0 {
                        *h2 += 360.0;
                        delta += 360.0;
                    }
                }
                HueMethod::Longer => {
                    if (i == 0 && first_stop_is_implicit)
                        || (i == color_count - 2 && last_stop_is_implicit)
                    {
                        // Do nothing for implicit stops
                    } else if 0.0 < *h2 - h1 && *h2 - h1 < 180.0 {
                        *h2 -= 360.0;
                        delta -= 360.0;
                    } else if -180.0 < *h2 - h1 && *h2 - h1 <= 0.0 {
                        *h2 += 360.0;
                        delta += 360.0;
                    }
                }
                HueMethod::Increasing => {
                    if *h2 < h1 {
                        *h2 += 360.0;
                        delta += 360.0;
                    }
                }
                HueMethod::Decreasing => {
                    if h1 < *h2 {
                        *h2 -= 360.0;
                        delta -= 360.0;
                    }
                }
            }
        }
    }

    // Step 5: Premultiplication
    if interp.in_premul != 0 {
        let is_polar_premul = matches!(
            cs,
            InterpolationColorSpace::HSL
                | InterpolationColorSpace::HWB
                | InterpolationColorSpace::LCH
                | InterpolationColorSpace::OKLCH
        );
        for c in &mut colors {
            *c = if is_polar_premul {
                premul_polar(*c)
            } else {
                premul_rgb(*c)
            };
        }
    }

    // Ganesh explicit positions
    if force_explicit_positions && out_positions.is_none() && orig_positions.is_none() {
        let pos_scale = 1.0_f32 / ((color_count - 1) as f32);
        let mut explicit = Vec::with_capacity(color_count);
        for i in 0..color_count {
            explicit.push((i as f32) * pos_scale);
        }
        out_positions = Some(explicit);
    }

    (colors, out_positions)
}

/// Exact port of `average_gradient_color` in `SkGradientBaseShader.cpp`.
pub fn average_gradient_color(colors: &[Color4f], pos: &[f32]) -> Color4f {
    let mut blend = [0.0_f32; 4];
    for i in 0..(colors.len() - 1) {
        let c0 = colors[i];
        let c1 = colors[i + 1];

        let w = if !pos.is_empty() {
            let p0 = pin_f32(pos[i], 0.0, 1.0);
            let p1 = pin_f32(pos[i + 1], p0, 1.0);
            if i == 0 && p0 > 0.0 {
                let c = colors[0];
                blend[0] += p0 * c.r;
                blend[1] += p0 * c.g;
                blend[2] += p0 * c.b;
                blend[3] += p0 * c.a;
            }
            if i == colors.len() - 2 && p1 < 1.0 {
                let c = colors[colors.len() - 1];
                let rem = 1.0 - p1;
                blend[0] += rem * c.r;
                blend[1] += rem * c.g;
                blend[2] += rem * c.b;
                blend[3] += rem * c.a;
            }
            p1 - p0
        } else {
            1.0_f32 / ((colors.len() - 1) as f32)
        };

        let half_w = 0.5_f32 * w;
        blend[0] += half_w * (c1.r + c0.r);
        blend[1] += half_w * (c1.g + c0.g);
        blend[2] += half_w * (c1.b + c0.b);
        blend[3] += half_w * (c1.a + c0.a);
    }
    Color4f {
        r: blend[0],
        g: blend[1],
        b: blend[2],
        a: blend[3],
    }
}

/// Result of `classify_degenerate_gradient`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DegenerateGradientResult {
    /// 0 = return nullptr (unreachable), 1 = return Empty, 2 = return Color(color, cs)
    pub kind: i32,
    pub color: Color4f,
}

/// Exact port of `SkGradientBaseShader::MakeDegenerateGradient`.
pub fn classify_degenerate_gradient(
    colors: &[Color4f],
    pos: &[f32],
    tile_mode: TileMode,
) -> DegenerateGradientResult {
    match tile_mode {
        TileMode::Decal => DegenerateGradientResult {
            kind: 1,
            color: Color4f::new(0.0, 0.0, 0.0, 0.0),
        },
        TileMode::Repeat | TileMode::Mirror => DegenerateGradientResult {
            kind: 2,
            color: average_gradient_color(colors, pos),
        },
        TileMode::Clamp => DegenerateGradientResult {
            kind: 2,
            color: colors[colors.len() - 1],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_flags_roundtrip() {
        assert_eq!(core::mem::size_of::<GradientInterpolation>(), 3);
        let interp = GradientInterpolation {
            in_premul: 1,
            color_space: InterpolationColorSpace::OKLCH as u8,
            hue_method: HueMethod::Longer as u8,
        };
        let packed = pack_serialization_flags(true, true, TileMode::Mirror as u32, interp);
        let unpacked = unpack_serialization_flags(packed);
        assert!(unpacked.has_positions);
        assert!(unpacked.has_color_space);
        assert!(!unpacked.has_legacy_local_matrix);
        assert_eq!(unpacked.tile_mode, TileMode::Mirror as u32);
        assert_eq!(unpacked.interpolation, interp);
    }

    #[test]
    fn prepare_stops_implicit_and_dedup() {
        let r = Color4f::new(1.0, 0.0, 0.0, 1.0);
        let g = Color4f::new(0.0, 1.0, 0.0, 1.0);
        let b = Color4f::new(0.0, 0.0, 1.0, 1.0);
        let w = Color4f::new(1.0, 1.0, 1.0, 1.0);

        // Implicit start and end stops
        let prep = prepare_gradient_stops(&[r, b], &[0.3, 0.7], TileMode::Clamp);
        assert!(prep.first_stop_is_implicit);
        assert!(prep.last_stop_is_implicit);
        assert!(prep.colors_are_opaque);
        assert_eq!(prep.colors.len(), 4);
        assert_eq!(prep.positions.as_deref(), Some(&[0.0, 0.3, 0.7, 1.0][..]));

        // Uniform stops become implicit (positions = None)
        let uni = prepare_gradient_stops(&[r, g, b], &[0.0, 0.5, 1.0], TileMode::Clamp);
        assert!(uni.positions.is_none());

        // Duplicate pruning: 0, 0, 0, 0.5, 1, 1
        // Clamp keeps 0, 0, 0.5, 1, 1 (5 stops)
        let dup_clamp =
            prepare_gradient_stops(&[r, g, b, w, r, g], &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0], TileMode::Clamp);
        assert_eq!(dup_clamp.positions.as_deref(), Some(&[0.0, 0.0, 0.5, 1.0, 1.0][..]));
        assert_eq!(dup_clamp.colors, vec![r, b, w, r, g]);

        // Repeat drops leftmost at 0 and rightmost at 1 -> 0, 0.5, 1 (3 stops)
        let dup_repeat =
            prepare_gradient_stops(&[r, g, b, w, r, g], &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0], TileMode::Repeat);
        assert_eq!(dup_repeat.positions.as_deref(), Some(&[0.0, 0.5, 1.0][..]));
        assert_eq!(dup_repeat.colors, vec![b, w, r]);
    }

    #[test]
    fn special_spaces_and_hue_methods() {
        // Pure red in sRGB -> HSL (0, 100, 50)
        let (hsl_red, powerless) = srgb_to_hsl(Color4f::new(1.0, 0.0, 0.0, 1.0));
        assert!(!powerless);
        assert!((hsl_red.r - 0.0).abs() < 1e-5);
        assert!((hsl_red.g - 100.0).abs() < 1e-5);
        assert!((hsl_red.b - 50.0).abs() < 1e-5);

        // Gray has powerless hue
        let (_, gray_powerless) = srgb_to_hsl(Color4f::new(0.5, 0.5, 0.5, 1.0));
        assert!(gray_powerless);

        // Powerless hue stop splitting: red -> gray -> blue in HSL
        let interp = GradientInterpolation {
            in_premul: 0,
            color_space: InterpolationColorSpace::HSL as u8,
            hue_method: HueMethod::Shorter as u8,
        };
        let (cols, pos) = xform_intermediate_colors(
            vec![
                Color4f::new(1.0, 0.0, 0.0, 1.0),
                Color4f::new(0.5, 0.5, 0.5, 1.0),
                Color4f::new(0.0, 0.0, 1.0, 1.0),
            ],
            None,
            interp,
            false,
            false,
            false,
        );
        // Gray in middle splits into 2 stops at pos 0.5 (one with red's hue=0, one with blue's hue=240).
        assert_eq!(cols.len(), 4);
        assert_eq!(pos.as_deref(), Some(&[0.0, 0.5, 0.5, 1.0][..]));
        assert!((cols[0].r - 0.0).abs() < 1e-4);
        assert!((cols[1].r - 0.0).abs() < 1e-4);
        // Shorter from 0 to 240 wraps 240 -> -120!
        assert!((cols[2].r - (-120.0)).abs() < 1e-4);
        assert!((cols[3].r - (-120.0)).abs() < 1e-4);
    }

    #[test]
    fn average_and_degenerate_gradient() {
        let r = Color4f::new(1.0, 0.0, 0.0, 1.0);
        let b = Color4f::new(0.0, 0.0, 1.0, 1.0);
        let avg = average_gradient_color(&[r, b], &[]);
        assert!((avg.r - 0.5).abs() < 1e-6);
        assert!((avg.b - 0.5).abs() < 1e-6);
        assert!((avg.a - 1.0).abs() < 1e-6);

        let deg_decal = classify_degenerate_gradient(&[r, b], &[], TileMode::Decal);
        assert_eq!(deg_decal.kind, 1);
        let deg_clamp = classify_degenerate_gradient(&[r, b], &[], TileMode::Clamp);
        assert_eq!(deg_clamp.kind, 2);
        assert_eq!(deg_clamp.color, b);
    }

    #[test]
    fn fill_evenly_and_arbitrary_stops() {
        let c0 = Color4f::new(0.0, 0.0, 0.0, 1.0);
        let c1 = Color4f::new(0.5, 0.5, 0.5, 1.0);
        let c2 = Color4f::new(1.0, 1.0, 1.0, 1.0);

        let mut f0 = [0.0; 8];
        let mut f1 = [0.0; 8];
        let mut f2 = [0.0; 8];
        let mut f3 = [0.0; 8];
        let mut b0 = [0.0; 8];
        let mut b1 = [0.0; 8];
        let mut b2 = [0.0; 8];
        let mut b3 = [0.0; 8];
        let mut factors = [&mut f0[..], &mut f1[..], &mut f2[..], &mut f3[..]];
        let mut biases = [&mut b0[..], &mut b1[..], &mut b2[..], &mut b3[..]];

        let n = fill_evenly_spaced_stops(&[c0, c1, c2], &mut factors, &mut biases);
        assert_eq!(n, 3);
        // First interval [0, 0.5]: slope = (0.5 - 0) * 2 = 1.0, bias = 0.0
        assert!((factors[0][0] - 1.0).abs() < 1e-6);
        assert!((biases[0][0] - 0.0).abs() < 1e-6);

        // Arbitrary with hard stop at 0.5: [0.0, 0.5, 0.5, 1.0]
        let mut ts = [0.0; 8];
        let n_arb = fill_arbitrary_stops(
            &[c0, c1, c2, c0],
            &[0.0, 0.5, 0.5, 1.0],
            &mut factors,
            &mut biases,
            &mut ts,
        );
        // Left const + interval [0, 0.5] + interval [0.5, 1.0] + right const = 4
        assert_eq!(n_arb, 4);
    }

    #[test]
    fn lab_lch_oklab_oklch_hwb_and_hue_methods_extended() {
        // D50 white in XYZD50 -> Lab (100, 0, 0)
        let d50 = Color4f::new(
            0.3457_f32 / 0.3585_f32,
            1.0_f32,
            (1.0_f32 - 0.3457_f32 - 0.3585_f32) / 0.3585_f32,
            1.0,
        );
        let lab_white = xyzd50_to_lab(d50);
        assert!((lab_white.r - 100.0).abs() < 1e-3);
        assert!(lab_white.g.abs() < 1e-3);
        assert!(lab_white.b.abs() < 1e-3);

        let (hcl_white, hcl_powerless) = xyzd50_to_hcl(d50);
        assert!(hcl_powerless);
        assert!((hcl_white.b - 100.0).abs() < 1e-3);

        // Linear sRGB white -> OKLab (1, 0, 0)
        let oklab_white = lin_srgb_to_oklab(Color4f::new(1.0, 1.0, 1.0, 1.0));
        assert!((oklab_white.r - 1.0).abs() < 1e-4);
        assert!(oklab_white.g.abs() < 1e-4);
        assert!(oklab_white.b.abs() < 1e-4);

        let (_, okhcl_powerless) = lin_srgb_to_okhcl(Color4f::new(1.0, 1.0, 1.0, 1.0));
        assert!(okhcl_powerless);

        // sRGB pure green -> HWB (120, 0, 0)
        let (hwb_g, hwb_powerless) = srgb_to_hwb(Color4f::new(0.0, 1.0, 0.0, 0.5));
        assert!(!hwb_powerless);
        assert!((hwb_g.r - 120.0).abs() < 1e-4);
        assert!(hwb_g.g.abs() < 1e-4);
        assert!(hwb_g.b.abs() < 1e-4);
        assert_eq!(hwb_g.a, 0.5);

        // HueMethod::Longer skips implicit first/last stops, applies to interior stops.
        let interp_longer = GradientInterpolation {
            in_premul: 1,
            color_space: InterpolationColorSpace::HSL as u8,
            hue_method: HueMethod::Longer as u8,
        };
        let red = Color4f::new(1.0, 0.0, 0.0, 0.5);
        let green = Color4f::new(0.0, 1.0, 0.0, 0.5);
        let (cols, exp_pos) = xform_intermediate_colors(
            vec![red, red, green, green],
            None,
            interp_longer,
            true,
            true,
            true,
        );
        // Implicit first stop (0 -> 0) stays 0; interior (0 -> 120) goes longer to -240;
        // implicit last stop (-240 -> -240) stays -240.
        assert!((cols[0].r - 0.0).abs() < 1e-4);
        assert!((cols[1].r - 0.0).abs() < 1e-4);
        assert!((cols[2].r - (-240.0)).abs() < 1e-4);
        assert!((cols[3].r - (-240.0)).abs() < 1e-4);
        // Polar premul preserves hue, scales S and L by alpha=0.5.
        assert!((cols[0].g - 50.0).abs() < 1e-4);
        assert!((cols[0].b - 25.0).abs() < 1e-4);
        // force_explicit_positions populated [0, 1/3, 2/3, 1].
        assert_eq!(exp_pos.as_ref().map(|v| v.len()), Some(4));

        // HueMethod::Increasing and Decreasing
        let interp_inc = GradientInterpolation {
            in_premul: 0,
            color_space: InterpolationColorSpace::HSL as u8,
            hue_method: HueMethod::Increasing as u8,
        };
        let (inc_cols, _) = xform_intermediate_colors(
            vec![Color4f::new(0.0, 0.0, 1.0, 1.0), Color4f::new(1.0, 0.0, 0.0, 1.0)],
            None,
            interp_inc,
            false,
            false,
            false,
        );
        // 240 -> 0 increases to 360
        assert!((inc_cols[0].r - 240.0).abs() < 1e-4);
        assert!((inc_cols[1].r - 360.0).abs() < 1e-4);

        let interp_dec = GradientInterpolation {
            in_premul: 0,
            color_space: InterpolationColorSpace::HSL as u8,
            hue_method: HueMethod::Decreasing as u8,
        };
        let (dec_cols, _) = xform_intermediate_colors(
            vec![Color4f::new(1.0, 0.0, 0.0, 1.0), Color4f::new(0.0, 0.0, 1.0, 1.0)],
            None,
            interp_dec,
            false,
            false,
            false,
        );
        // 0 -> 240 decreases to -120
        assert!((dec_cols[0].r - 0.0).abs() < 1e-4);
        assert!((dec_cols[1].r - (-120.0)).abs() < 1e-4);
    }

    #[test]
    fn interpolated_to_dst_and_decal_limit() {
        assert_eq!(gradient_decal_limit_x().to_bits(), 0x3f80_0001);

        let plan_oklch_gm = plan_interpolated_to_dst(
            false,
            GradientInterpolation {
                in_premul: 1,
                color_space: InterpolationColorSpace::OKLCHGamutMap as u8,
                hue_method: HueMethod::Shorter as u8,
            },
        );
        assert_eq!(plan_oklch_gm.unpremul, PostInterpUnpremul::UnpremulPolar);
        assert_eq!(plan_oklch_gm.color_op, PostInterpColorOp::OKLchGamutMapToLabToLinearSrgb);
        assert_eq!(plan_oklch_gm.intermediate_alpha_type, AlphaType::Unpremul);
        assert_eq!(plan_oklch_gm.dst_alpha_type, AlphaType::Premul);

        let plan_opaque_lab = plan_interpolated_to_dst(
            true,
            GradientInterpolation {
                in_premul: 1,
                color_space: InterpolationColorSpace::Lab as u8,
                hue_method: HueMethod::Shorter as u8,
            },
        );
        assert_eq!(plan_opaque_lab.unpremul, PostInterpUnpremul::None);
        assert_eq!(plan_opaque_lab.color_op, PostInterpColorOp::LabToXyz);
        assert_eq!(plan_opaque_lab.intermediate_alpha_type, AlphaType::Unpremul);
        assert_eq!(plan_opaque_lab.dst_alpha_type, AlphaType::Unpremul);
    }

    #[test]
    fn average_gradient_color_with_implicit_margins() {
        // Red from [0..0.25], blend Red->Blue on [0.25..0.75], Blue on [0.75..1.0].
        // Integral: 0.25*R + 0.5*(0.5*(R+B)) + 0.25*B = 0.5*R + 0.5*B.
        let r = Color4f::new(1.0, 0.0, 0.0, 1.0);
        let b = Color4f::new(0.0, 0.0, 1.0, 1.0);
        let avg = average_gradient_color(&[r, b], &[0.25, 0.75]);
        assert!((avg.r - 0.5).abs() < 1e-6);
        assert!((avg.b - 0.5).abs() < 1e-6);
    }
}
