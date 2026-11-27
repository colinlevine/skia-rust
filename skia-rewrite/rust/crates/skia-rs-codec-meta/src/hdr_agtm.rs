//! Safe Rust port of `src/codec/SkHdrAgtm.cpp` and `src/codec/SkHdrAgtmParse.cpp`.

use skia_rs_color::color_space::{
    ColorSpacePrimaries, Matrix3x3, PRIMARIES_REC2020, PRIMARIES_REC709, PRIMARIES_SMPTE_EG_432_1,
};

use crate::hdr_metadata::HdrMetadata;

pub const MIN_HDR_HEADROOM: f32 = 0.0_f32;
pub const MAX_HDR_HEADROOM: f32 = 6.0_f32;
pub const MAX_LINEAR_HDR_HEADROOM: f32 = 64.0_f32;
pub const DEFAULT_HDR_REFERENCE_WHITE: f32 = 203.0_f32;
pub const MIN_NUM_CONTROL_POINTS: usize = 1;
pub const MAX_NUM_CONTROL_POINTS: usize = 32;
pub const MAX_NUM_ALTERNATE_IMAGES: usize = 4;
pub const INVALID_ALTERNATE_IMAGE_INDEX: u8 = 255;

const SK_FLOAT_PI: f32 = 3.14159265_f32;
const NUM_CHROMATICITY_VALUES: usize = 8;
const NUM_MIX_COEFFICIENTS: usize = 6;

#[inline]
fn in_interval(x: f32, min: f32, max: f32) -> bool {
    x >= min && x <= max
}

#[inline]
fn in_unit_interval(x: f32) -> bool {
    in_interval(x, 0.0_f32, 1.0_f32)
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ControlPoint {
    pub x: f32,
    pub y: f32,
    pub m: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GainCurve {
    pub control_points: Vec<ControlPoint>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ComponentMixingFunction {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub max: f32,
    pub min: f32,
    pub component: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorGainFunction {
    pub component_mixing: ComponentMixingFunction,
    pub gain_curve: GainCurve,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlternateImage {
    pub hdr_headroom: f32,
    pub color_gain_function: ColorGainFunction,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadroomAdaptiveToneMap {
    pub baseline_hdr_headroom: f32,
    pub gain_application_space_primaries: ColorSpacePrimaries,
    pub alternate_images: Vec<AlternateImage>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdaptiveGlobalToneMap {
    pub hdr_reference_white: f32,
    pub headroom_adaptive_tone_map: Option<HeadroomAdaptiveToneMap>,
}

impl Default for AdaptiveGlobalToneMap {
    fn default() -> Self {
        Self {
            hdr_reference_white: DEFAULT_HDR_REFERENCE_WHITE,
            headroom_adaptive_tone_map: None,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Weighting {
    pub alternate_image_index: [u8; 2],
    pub weight: [f32; 2],
}

impl Default for Weighting {
    fn default() -> Self {
        Self {
            alternate_image_index: [INVALID_ALTERNATE_IMAGE_INDEX; 2],
            weight: [0.0_f32; 2],
        }
    }
}

/// Precomputed uniforms for the AGTM SkRuntimeEffect color filter.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ColorFilterUniforms {
    pub scale_factor: f32,
    pub weight: [f32; 2],
    pub mix_rgbx: [[f32; 4]; 2],
    pub mix_mmcx: [[f32; 4]; 2],
    pub curve_texcoord_y: [f32; 2],
    pub curve_n_cp: [f32; 2],
    pub gain_application_to_xyzd50: Matrix3x3,
    pub has_gain_application_space: bool,
}

/// Normalize the component mixing coefficients so that their sum equals 1.
pub fn normalize_component_mixing(mix: ComponentMixingFunction) -> ComponentMixingFunction {
    let sum = mix.red + mix.green + mix.blue + mix.max + mix.min + mix.component;
    if sum <= 0.0_f32 {
        return mix;
    }
    let inv_sum = 1.0_f32 / sum;
    ComponentMixingFunction {
        red: mix.red * inv_sum,
        green: mix.green * inv_sum,
        blue: mix.blue * inv_sum,
        max: mix.max * inv_sum,
        min: mix.min * inv_sum,
        component: mix.component * inv_sum,
    }
}

/// Matches `AgtmHelpers::EvaluateComponentMixingFunction`.
pub fn evaluate_component_mixing_function(
    mix: ComponentMixingFunction,
    c: [f32; 4],
) -> [f32; 4] {
    let norm = normalize_component_mixing(mix);
    let max_c = c[0].max(c[1]).max(c[2]);
    let min_c = c[0].min(c[1]).min(c[2]);
    let common = norm.red * c[0]
        + norm.green * c[1]
        + norm.blue * c[2]
        + norm.max * max_c
        + norm.min * min_c;
    if norm.component == 0.0_f32 {
        return [common, common, common, c[3]];
    }
    [
        norm.component * c[0] + common,
        norm.component * c[1] + common,
        norm.component * c[2] + common,
        c[3],
    ]
}

/// Matches `AgtmHelpers::EvaluateGainCurve`.
pub fn evaluate_gain_curve(cp: &[ControlPoint], x: f32) -> f32 {
    let n = cp.len();
    debug_assert!(n > 0 && n <= MAX_NUM_CONTROL_POINTS);
    if n == 0 {
        return 0.0_f32;
    }

    let mut i = 0usize;
    if x <= cp[i].x {
        return cp[i].y;
    }

    let mut j = n - 1;
    if x >= cp[j].x {
        return cp[j].y + (cp[j].x / x).log2();
    }

    while j - i > 1 {
        let m = (i + j) / 2;
        if x < cp[m].x {
            j = m;
        } else {
            i = m;
        }
    }

    let x_i = cp[i].x;
    let y_i = cp[i].y;
    let x_j = cp[j].x;
    let y_j = cp[j].y;
    let h_i = x_j - x_i;
    let m_hat_i = cp[i].m * h_i;
    let m_hat_j = cp[j].m * h_i;

    if h_i == 0.0_f32 {
        return y_i;
    }

    let c3 = 2.0_f32 * y_i + m_hat_i - 2.0_f32 * y_j + m_hat_j;
    let c2 = -3.0_f32 * y_i + 3.0_f32 * y_j - 2.0_f32 * m_hat_i - m_hat_j;
    let c1 = m_hat_i;
    let c0 = y_i;
    let t = (x - x_i) / h_i;

    ((c3 * t + c2) * t + c1) * t + c0
}

/// Matches `AgtmHelpers::EvaluateColorGainFunction`.
pub fn evaluate_color_gain_function(gain: &ColorGainFunction, c: [f32; 4]) -> [f32; 4] {
    let m = evaluate_component_mixing_function(gain.component_mixing, c);
    let r = evaluate_gain_curve(&gain.gain_curve.control_points, m[0]);
    if m[0] == m[1] && m[1] == m[2] {
        [r, r, r, c[3]]
    } else {
        let g = evaluate_gain_curve(&gain.gain_curve.control_points, m[1]);
        let b = evaluate_gain_curve(&gain.gain_curve.control_points, m[2]);
        [r, g, b, c[3]]
    }
}

/// Matches `AgtmHelpers::PopulateSlopeFromPCHIP`.
pub fn populate_slope_from_pchip(cp: &mut [ControlPoint]) {
    let n = cp.len();
    assert!(n >= MIN_NUM_CONTROL_POINTS && n <= MAX_NUM_CONTROL_POINTS);

    let mut h = [0.0_f32; MAX_NUM_CONTROL_POINTS];
    for i in 0..n - 1 {
        h[i] = cp[i + 1].x - cp[i].x;
    }
    let mut s = [0.0_f32; MAX_NUM_CONTROL_POINTS];
    for i in 0..n - 1 {
        if h[i] != 0.0_f32 {
            s[i] = (cp[i + 1].y - cp[i].y) / h[i];
        } else {
            s[i] = 0.0_f32;
        }
    }

    let three_point_finite_difference = |h0: f32, h1: f32, s0: f32, s1: f32| -> f32 {
        let num = (2.0_f32 * h0 + h1) * s0 - h0 * s1;
        let den = h0 + h1;
        let mut m = num / den;
        if m.is_sign_negative() != s0.is_sign_negative() {
            m = 0.0_f32;
        } else if m.is_sign_negative() != s1.is_sign_negative() && m.abs() > 3.0_f32 * s0.abs() {
            m = 3.0_f32 * s0;
        }
        m
    };

    for i in 0..n {
        if i >= 1 && i + 1 < n && cp[i - 1].x < cp[i].x && cp[i].x < cp[i + 1].x {
            if s[i - 1].is_sign_negative() != s[i].is_sign_negative()
                || (s[i - 1] == 0.0_f32 && s[i] == 0.0_f32)
            {
                cp[i].m = 0.0_f32;
            } else {
                let num = 3.0_f32 * (h[i - 1] + h[i]) * s[i - 1] * s[i];
                let den =
                    (2.0_f32 * h[i - 1] + h[i]) * s[i - 1] + (h[i - 1] + 2.0_f32 * h[i]) * s[i];
                cp[i].m = num / den;
            }
        } else if i + 1 < n && cp[i].x < cp[i + 1].x {
            if i + 2 < n && cp[i + 1].x < cp[i + 2].x {
                cp[i].m = three_point_finite_difference(h[i], h[i + 1], s[i], s[i + 1]);
            } else {
                cp[i].m = s[i];
            }
        } else if i >= 1 && cp[i - 1].x < cp[i].x {
            if i >= 2 && cp[i - 2].x < cp[i - 1].x {
                cp[i].m = three_point_finite_difference(h[i - 1], h[i - 2], s[i - 1], s[i - 2]);
            } else {
                cp[i].m = s[i - 1];
            }
        } else {
            cp[i].m = 0.0_f32;
        }
    }
}

/// Matches `AgtmHelpers::PopulateUsingRwtmo`.
pub fn populate_using_rwtmo(hatm: &mut HeadroomAdaptiveToneMap) {
    hatm.gain_application_space_primaries = PRIMARIES_REC2020;

    if hatm.baseline_hdr_headroom == 0.0_f32 {
        hatm.alternate_images.clear();
        return;
    }

    let log2_1000_203 = (1000.0_f32 / 203.0_f32).log2();
    let ratio = (hatm.baseline_hdr_headroom / log2_1000_203).min(1.0_f32);

    hatm.alternate_images.resize(2, AlternateImage::default());
    hatm.alternate_images[0].hdr_headroom = 0.0_f32;
    hatm.alternate_images[1].hdr_headroom = (8.0_f32 / 3.0_f32).log2() * ratio;

    for a in 0..hatm.alternate_images.len() {
        let alt_hdr_headroom = hatm.alternate_images[a].hdr_headroom;
        let mut gain = ColorGainFunction::default();
        gain.component_mixing.max = 1.0_f32;

        let y_white = if a == 1 {
            1.0_f32
        } else {
            1.0_f32 - 0.5_f32 * ratio
        };

        let kappa = 0.65_f32;
        let x_knee = 1.0_f32;
        let y_knee = y_white;
        let x_max = hatm.baseline_hdr_headroom.exp2();
        let y_max = alt_hdr_headroom.exp2();
        let x_mid = (1.0_f32 - kappa) * x_knee + kappa * (x_knee * y_max / y_knee);
        let y_mid = (1.0_f32 - kappa) * y_knee + kappa * y_max;

        let x_a = x_knee - 2.0_f32 * x_mid + x_max;
        let y_a = y_knee - 2.0_f32 * y_mid + y_max;
        let x_b = 2.0_f32 * x_mid - 2.0_f32 * x_knee;
        let y_b = 2.0_f32 * y_mid - 2.0_f32 * y_knee;
        let x_c = x_knee;
        let y_c = y_knee;

        let mut cps = Vec::with_capacity(8);
        let ln2 = 2.0_f32.ln();
        for c in 0..8usize {
            let t = (c as f32) / (8.0_f32 - 1.0_f32);
            let x = x_c + t * (x_b + t * x_a);
            let y = y_c + t * (y_b + t * y_a);
            let m = (2.0_f32 * y_a * t + y_b) / (2.0_f32 * x_a * t + x_b);

            cps.push(ControlPoint {
                x,
                y: (y / x).log2(),
                m: (x * m - y) / (ln2 * x * y),
            });
        }
        gain.gain_curve.control_points = cps;
        hatm.alternate_images[a].color_gain_function = gain;
    }
}

/// Matches `AgtmHelpers::ComputeWeighting`.
pub fn compute_weighting(hatm: &HeadroomAdaptiveToneMap, targeted_hdr_headroom: f32) -> Weighting {
    let mut result = Weighting::default();
    if !targeted_hdr_headroom.is_finite() {
        return result;
    }
    let target = targeted_hdr_headroom.clamp(MIN_HDR_HEADROOM, MAX_HDR_HEADROOM);

    let mut n = 0usize;
    let mut h = [0.0_f32; MAX_NUM_ALTERNATE_IMAGES + 1];
    let mut indices = [INVALID_ALTERNATE_IMAGE_INDEX; MAX_NUM_ALTERNATE_IMAGES + 1];

    let alt_len = hatm.alternate_images.len().min(MAX_NUM_ALTERNATE_IMAGES);
    for i in 0..alt_len {
        if n == i && hatm.baseline_hdr_headroom < hatm.alternate_images[i].hdr_headroom {
            indices[n] = INVALID_ALTERNATE_IMAGE_INDEX;
            h[n] = hatm.baseline_hdr_headroom;
            n += 1;
        }
        indices[n] = i as u8;
        h[n] = hatm.alternate_images[i].hdr_headroom;
        n += 1;
    }
    if n == alt_len {
        indices[n] = INVALID_ALTERNATE_IMAGE_INDEX;
        h[n] = hatm.baseline_hdr_headroom;
        n += 1;
    }

    if target <= h[0] {
        result.weight[0] = 1.0_f32;
        result.alternate_image_index[0] = indices[0];
    } else if target >= h[n - 1] {
        result.weight[0] = 1.0_f32;
        result.alternate_image_index[0] = indices[n - 1];
    } else {
        let mut idx = 0usize;
        for i in 0..n - 1 {
            if h[i] <= target && target <= h[i + 1] {
                idx = i;
                break;
            }
        }
        result.weight[0] = (target - h[idx + 1]) / (h[idx] - h[idx + 1]);
        result.weight[1] = 1.0_f32 - result.weight[0];
        result.alternate_image_index[0] = indices[idx];
        result.alternate_image_index[1] = indices[idx + 1];
    }

    for i in 0..2 {
        if result.alternate_image_index[i] == INVALID_ALTERNATE_IMAGE_INDEX {
            result.weight[i] = 0.0_f32;
        } else if result.weight[i] == 0.0_f32 {
            result.alternate_image_index[i] = INVALID_ALTERNATE_IMAGE_INDEX;
        }
    }

    if result.weight[1] > result.weight[0] {
        result.weight.swap(0, 1);
        result.alternate_image_index.swap(0, 1);
    }

    result
}

/// Matches `AgtmHelpers::ApplyGain`.
pub fn apply_gain(
    hatm: &HeadroomAdaptiveToneMap,
    colors: &mut [[f32; 4]],
    targeted_hdr_headroom: f32,
) {
    debug_assert!(validate_hatm(hatm));
    let weighting = compute_weighting(hatm, targeted_hdr_headroom);
    if weighting.weight[0] == 0.0_f32 {
        return;
    } else if weighting.weight[1] == 0.0_f32 {
        let gain = &hatm.alternate_images[weighting.alternate_image_index[0] as usize]
            .color_gain_function;
        let w = weighting.weight[0];
        for c in colors.iter_mut() {
            let g = evaluate_color_gain_function(gain, *c);
            *c = [
                c[0] * (w * g[0]).exp2(),
                c[1] * (w * g[1]).exp2(),
                c[2] * (w * g[2]).exp2(),
                c[3],
            ];
        }
    } else {
        let gain0 = &hatm.alternate_images[weighting.alternate_image_index[0] as usize]
            .color_gain_function;
        let w0 = weighting.weight[0];
        let gain1 = &hatm.alternate_images[weighting.alternate_image_index[1] as usize]
            .color_gain_function;
        let w1 = weighting.weight[1];
        for c in colors.iter_mut() {
            let g0 = evaluate_color_gain_function(gain0, *c);
            let g1 = evaluate_color_gain_function(gain1, *c);
            *c = [
                c[0] * (w0 * g0[0] + w1 * g1[0]).exp2(),
                c[1] * (w0 * g0[1] + w1 * g1[1]).exp2(),
                c[2] * (w0 * g0[2] + w1 * g1[2]).exp2(),
                c[3],
            ];
        }
    }
}

/// Build the F32 RGBA pixel buffer (`width`, `height`, row-major `[f32; 4]`) for
/// `AgtmHelpers::MakeGainCurveXYMImage`.
pub fn build_gain_curve_xym_pixels(
    hatm: &HeadroomAdaptiveToneMap,
) -> Option<(usize, usize, Vec<[f32; 4]>)> {
    if hatm.alternate_images.is_empty() {
        return None;
    }
    let mut max_num_cp = 1usize;
    for alt in &hatm.alternate_images {
        max_num_cp = max_num_cp.max(alt.color_gain_function.gain_curve.control_points.len());
    }
    let width = max_num_cp;
    let height = hatm.alternate_images.len();
    let mut pixels = vec![[0.0_f32; 4]; width * height];
    for (a, alt) in hatm.alternate_images.iter().enumerate() {
        let cps = &alt.color_gain_function.gain_curve.control_points;
        for (c, cp) in cps.iter().enumerate() {
            pixels[a * width + c] = [cp.x, cp.y, cp.m, 1.0_f32];
        }
    }
    Some((width, height, pixels))
}

/// Compute all uniforms and the gain application color space matrix for `AgtmHelpers::MakeColorFilter`.
pub fn compute_color_filter_uniforms(
    hatm: &HeadroomAdaptiveToneMap,
    targeted_hdr_headroom: f32,
    scale_factor: f32,
) -> ColorFilterUniforms {
    let weighting = compute_weighting(hatm, targeted_hdr_headroom);
    let mut out = ColorFilterUniforms {
        scale_factor,
        weight: weighting.weight,
        ..Default::default()
    };
    for a in 0..2 {
        if weighting.weight[a] == 0.0_f32 {
            continue;
        }
        let alt_idx = weighting.alternate_image_index[a] as usize;
        if let Some(alt) = hatm.alternate_images.get(alt_idx) {
            let mix = normalize_component_mixing(alt.color_gain_function.component_mixing);
            out.mix_rgbx[a] = [mix.red, mix.green, mix.blue, 0.0_f32];
            out.mix_mmcx[a] = [mix.max, mix.min, mix.component, 0.0_f32];
            out.curve_texcoord_y[a] = (alt_idx as f32) + 0.5_f32;
            out.curve_n_cp[a] = alt.color_gain_function.gain_curve.control_points.len() as f32;
        }
    }
    if let Some(mat) = hatm.gain_application_space_primaries.to_xyzd50() {
        out.gain_application_to_xyzd50 = mat;
        out.has_gain_application_space = true;
    }
    out
}

fn get_peak_luminance(metadata: &HdrMetadata) -> f32 {
    if let Some(clli) = metadata.clli {
        if clli.max_cll > 0.0_f32 {
            return clli.max_cll;
        }
    }
    if let Some(mdcv) = metadata.mdcv {
        if mdcv.maximum_display_mastering_luminance > 0.0_f32 {
            return mdcv.maximum_display_mastering_luminance;
        }
    }
    1000.0_f32
}

/// Matches `AgtmHelpers::PopulateToneMapAgtmParams`.
pub fn populate_tone_map_agtm_params(
    metadata: &HdrMetadata,
    input_is_pq_or_hlg: bool,
    input_pq_or_hlg_white: f32,
) -> Option<(AdaptiveGlobalToneMap, f32)> {
    let had_agtm = metadata.agtm.is_some();
    let mut agtm = metadata.agtm.clone().unwrap_or_default();

    if !input_is_pq_or_hlg {
        if !had_agtm || agtm.headroom_adaptive_tone_map.is_none() {
            return None;
        }
    }

    if !had_agtm {
        agtm.hdr_reference_white = input_pq_or_hlg_white;
    }

    if agtm.headroom_adaptive_tone_map.is_none() {
        let peak_luminance = get_peak_luminance(metadata);
        let baseline_hdr_headroom = if agtm.hdr_reference_white > 0.0_f32
            && peak_luminance > agtm.hdr_reference_white
        {
            (peak_luminance / agtm.hdr_reference_white)
                .log2()
                .min(MAX_HDR_HEADROOM)
        } else {
            0.0_f32
        };
        let mut hatm = HeadroomAdaptiveToneMap {
            baseline_hdr_headroom,
            ..Default::default()
        };
        populate_using_rwtmo(&mut hatm);
        agtm.headroom_adaptive_tone_map = Some(hatm);
    }

    let scale_factor = if input_is_pq_or_hlg {
        input_pq_or_hlg_white / agtm.hdr_reference_white
    } else {
        1.0_f32
    };
    Some((agtm, scale_factor))
}

/// Matches `AgtmHelpers::Validate(const AdaptiveGlobalToneMap&)`.
pub fn validate_agtm(agtm: &AdaptiveGlobalToneMap) -> bool {
    if !in_interval(agtm.hdr_reference_white, 0.0_f32, 10000.0_f32)
        || agtm.hdr_reference_white == 0.0_f32
    {
        return false;
    }
    if let Some(ref hatm) = agtm.headroom_adaptive_tone_map {
        if !validate_hatm(hatm) {
            return false;
        }
    }
    true
}

/// Matches `AgtmHelpers::Validate(const HeadroomAdaptiveToneMap&)`.
pub fn validate_hatm(hatm: &HeadroomAdaptiveToneMap) -> bool {
    if !in_interval(hatm.baseline_hdr_headroom, MIN_HDR_HEADROOM, MAX_HDR_HEADROOM) {
        return false;
    }
    if hatm.alternate_images.len() > MAX_NUM_ALTERNATE_IMAGES {
        return false;
    }
    for (a, alt) in hatm.alternate_images.iter().enumerate() {
        if !in_interval(alt.hdr_headroom, MIN_HDR_HEADROOM, MAX_HDR_HEADROOM) {
            return false;
        }
        if alt.hdr_headroom == hatm.baseline_hdr_headroom {
            return false;
        }
        if a > 0 && alt.hdr_headroom <= hatm.alternate_images[a - 1].hdr_headroom {
            return false;
        }
    }
    let p = &hatm.gain_application_space_primaries;
    if !in_unit_interval(p.rx)
        || !in_unit_interval(p.ry)
        || !in_unit_interval(p.gx)
        || !in_unit_interval(p.gy)
        || !in_unit_interval(p.bx)
        || !in_unit_interval(p.by)
        || !in_unit_interval(p.wx)
        || !in_unit_interval(p.wy)
    {
        return false;
    }

    for alt in &hatm.alternate_images {
        let curve = &alt.color_gain_function.gain_curve;
        for cp in &curve.control_points {
            if alt.hdr_headroom > hatm.baseline_hdr_headroom {
                if cp.y < 0.0_f32 {
                    return false;
                }
            } else if cp.y > 0.0_f32 {
                return false;
            }
        }
    }

    for alt in &hatm.alternate_images {
        let mix = &alt.color_gain_function.component_mixing;
        if !in_unit_interval(mix.red)
            || !in_unit_interval(mix.green)
            || !in_unit_interval(mix.blue)
            || !in_unit_interval(mix.max)
            || !in_unit_interval(mix.min)
            || !in_unit_interval(mix.component)
        {
            return false;
        }
        let sum = mix.red + mix.green + mix.blue + mix.max + mix.min + mix.component;
        if sum <= 0.0_f32 {
            return false;
        }
    }

    for alt in &hatm.alternate_images {
        let cps = &alt.color_gain_function.gain_curve.control_points;
        if cps.len() < MIN_NUM_CONTROL_POINTS || cps.len() > MAX_NUM_CONTROL_POINTS {
            return false;
        }
        for c in 0..cps.len() {
            if !in_interval(cps[c].x, 0.0_f32, MAX_LINEAR_HDR_HEADROOM) {
                return false;
            }
            if !in_interval(cps[c].y, -MAX_HDR_HEADROOM, MAX_HDR_HEADROOM) {
                return false;
            }
            if !cps[c].m.is_finite() {
                return false;
            }
            if c > 0 {
                if cps[c].x < cps[c - 1].x {
                    return false;
                }
                if cps[c].x == cps[c - 1].x && cps[c].y != cps[c - 1].y {
                    return false;
                }
            }
        }
    }

    true
}

impl AdaptiveGlobalToneMap {
    pub fn format_string(&self) -> String {
        let mut result = format!("{{hdrReferenceWhite:{:.6}", self.hdr_reference_white);
        let Some(ref hatm) = self.headroom_adaptive_tone_map else {
            result.push('}');
            return result;
        };
        result.push_str(&format!(
            ", baselineHdrHeadroom:{:.6}, alternateHdrHeadrooms:[",
            hatm.baseline_hdr_headroom
        ));
        for (a, alt) in hatm.alternate_images.iter().enumerate() {
            result.push_str(&format!("{:.6}", alt.hdr_headroom));
            if a + 1 != hatm.alternate_images.len() {
                result.push_str(", ");
            }
        }
        result.push_str("]}");
        result
    }

    pub fn is_valid(&self) -> bool {
        validate_agtm(self)
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        parse_agtm(data)
    }

    pub fn serialize(&self) -> Option<Vec<u8>> {
        serialize_agtm(self)
    }
}

// ============================================================================
// Binary parsing & serialization (SkHdrAgtmParse.cpp / SMPTE ST 2094-50 Annex C)
// ============================================================================

#[inline]
fn float_to_uint16(f: f32, clamp_min: u16, clamp_max: u16, offset: u16, scale: f32) -> u16 {
    let v = ((f * scale).round() as i64 as i32) + (offset as i32);
    v.clamp(clamp_min as i32, clamp_max as i32) as u16
}

#[inline]
fn uint16_to_float(v: u16, clamp_min: u16, clamp_max: u16, offset: u16, scale: f32) -> f32 {
    ((v.clamp(clamp_min, clamp_max) as i32) - (offset as i32)) as f32 / scale
}

struct StreamReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> StreamReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn read_u16_be(&mut self) -> Option<u16> {
        let s = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_be_bytes([s[0], s[1]]))
    }
}

struct BitfieldReader {
    bits: u8,
    bits_read: u8,
}

impl BitfieldReader {
    fn read_from_stream(s: &mut StreamReader<'_>) -> Option<Self> {
        let bits = s.read_u8()?;
        Some(Self { bits, bits_read: 0 })
    }

    fn read_bits(&mut self, bits: u8) -> u8 {
        let result = self.bits >> (8 - bits);
        self.bits = self.bits.wrapping_shl(bits as u32);
        self.bits_read += bits;
        result
    }
}

struct BitfieldWriter {
    bits: u8,
    bits_written: u8,
}

impl BitfieldWriter {
    fn new() -> Self {
        Self {
            bits: 0,
            bits_written: 0,
        }
    }

    fn write_bits(&mut self, value: u8, bits: u8) {
        debug_assert!((value as u16) <= (1u16 << bits) - 1);
        self.bits = self.bits.wrapping_shl(bits as u32) | value;
        self.bits_written += bits;
    }

    fn pad_and_write(&mut self, out: &mut Vec<u8>) {
        debug_assert!(self.bits_written <= 8);
        self.bits = self.bits.wrapping_shl((8 - self.bits_written) as u32);
        self.bits_written = 8;
        out.push(self.bits);
    }
}

#[derive(Default)]
struct AgtmSyntax {
    application_version: u8,
    minimum_application_version: u8,

    has_custom_hdr_reference_white_flag: u8,
    has_adaptive_tone_map_flag: u8,
    hdr_reference_white: u16,

    baseline_hdr_headroom: u16,
    use_reference_white_tone_mapping_flag: u8,
    num_alternate_images: u8,
    gain_application_space_chromaticities_flag: u8,
    has_common_component_mix_params_flag: u8,
    has_common_curve_params_flag: u8,
    gain_application_space_chromaticities: [u16; NUM_CHROMATICITY_VALUES],
    alternate_hdr_headrooms: [u16; MAX_NUM_ALTERNATE_IMAGES],

    component_mixing_type: [u8; MAX_NUM_ALTERNATE_IMAGES],
    has_component_mixing_coefficient_flag: [[u8; NUM_MIX_COEFFICIENTS]; MAX_NUM_ALTERNATE_IMAGES],
    component_mixing_coefficient: [[u16; NUM_MIX_COEFFICIENTS]; MAX_NUM_ALTERNATE_IMAGES],

    gain_curve_num_control_points_minus_1: [u8; MAX_NUM_ALTERNATE_IMAGES],
    gain_curve_use_pchip_slope_flag: [u8; MAX_NUM_ALTERNATE_IMAGES],
    gain_curve_control_points_x: [[u16; MAX_NUM_CONTROL_POINTS]; MAX_NUM_ALTERNATE_IMAGES],
    gain_curve_control_points_y: [[u16; MAX_NUM_CONTROL_POINTS]; MAX_NUM_ALTERNATE_IMAGES],
    gain_curve_control_points_theta: [[u16; MAX_NUM_CONTROL_POINTS]; MAX_NUM_ALTERNATE_IMAGES],
}

impl AgtmSyntax {
    fn parse_application_info(&mut self, s: &mut StreamReader<'_>) -> bool {
        let Some(mut flags) = BitfieldReader::read_from_stream(s) else {
            return false;
        };
        self.application_version = flags.read_bits(3);
        self.minimum_application_version = flags.read_bits(3);
        if self.minimum_application_version > 0 {
            return false;
        }
        let reserved_zero = flags.read_bits(2);
        if reserved_zero != 0 {
            return false;
        }
        self.parse_color_volume_transform(s)
    }

    fn write_application_info(&self, out: &mut Vec<u8>) {
        let mut flags = BitfieldWriter::new();
        flags.write_bits(self.application_version, 3);
        flags.write_bits(self.minimum_application_version, 3);
        flags.pad_and_write(out);
        self.write_color_volume_transform(out);
    }

    fn parse_color_volume_transform(&mut self, s: &mut StreamReader<'_>) -> bool {
        let Some(mut flags) = BitfieldReader::read_from_stream(s) else {
            return false;
        };
        self.has_custom_hdr_reference_white_flag = flags.read_bits(1);
        self.has_adaptive_tone_map_flag = flags.read_bits(1);
        let reserved_zero = flags.read_bits(6);
        if reserved_zero != 0 {
            return false;
        }
        if self.has_custom_hdr_reference_white_flag == 1 {
            let Some(v) = s.read_u16_be() else {
                return false;
            };
            self.hdr_reference_white = v;
        }
        if self.has_adaptive_tone_map_flag == 1 {
            if !self.parse_adaptive_tone_map(s) {
                return false;
            }
        }
        true
    }

    fn write_color_volume_transform(&self, out: &mut Vec<u8>) {
        let mut flags = BitfieldWriter::new();
        flags.write_bits(self.has_custom_hdr_reference_white_flag, 1);
        flags.write_bits(self.has_adaptive_tone_map_flag, 1);
        flags.pad_and_write(out);
        if self.has_custom_hdr_reference_white_flag != 0 {
            out.extend_from_slice(&self.hdr_reference_white.to_be_bytes());
        }
        if self.has_adaptive_tone_map_flag == 1 {
            self.write_adaptive_tone_map(out);
        }
    }

    fn parse_adaptive_tone_map(&mut self, s: &mut StreamReader<'_>) -> bool {
        let Some(bh) = s.read_u16_be() else {
            return false;
        };
        self.baseline_hdr_headroom = bh;
        let Some(mut flags) = BitfieldReader::read_from_stream(s) else {
            return false;
        };
        self.use_reference_white_tone_mapping_flag = flags.read_bits(1);
        if self.use_reference_white_tone_mapping_flag == 0 {
            self.num_alternate_images = flags.read_bits(3);
            self.gain_application_space_chromaticities_flag = flags.read_bits(2);
            self.has_common_component_mix_params_flag = flags.read_bits(1);
            self.has_common_curve_params_flag = flags.read_bits(1);
            if self.gain_application_space_chromaticities_flag == 3 {
                for r in 0..NUM_CHROMATICITY_VALUES {
                    let Some(v) = s.read_u16_be() else {
                        return false;
                    };
                    self.gain_application_space_chromaticities[r] = v;
                }
            }
            let alt_count = (self.num_alternate_images as usize).min(MAX_NUM_ALTERNATE_IMAGES);
            for a in 0..alt_count {
                let Some(ah) = s.read_u16_be() else {
                    return false;
                };
                self.alternate_hdr_headrooms[a] = ah;
                if !self.parse_component_mixing(a, s) {
                    return false;
                }
                if !self.parse_gain_curve(a, s) {
                    return false;
                }
            }
        } else {
            let reserved_zero = flags.read_bits(7);
            if reserved_zero != 0 {
                return false;
            }
        }
        true
    }

    fn write_adaptive_tone_map(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.baseline_hdr_headroom.to_be_bytes());
        let mut flags = BitfieldWriter::new();
        flags.write_bits(self.use_reference_white_tone_mapping_flag, 1);
        if self.use_reference_white_tone_mapping_flag == 0 {
            flags.write_bits(self.num_alternate_images, 3);
            flags.write_bits(self.gain_application_space_chromaticities_flag, 2);
            flags.write_bits(self.has_common_component_mix_params_flag, 1);
            flags.write_bits(self.has_common_curve_params_flag, 1);
            flags.pad_and_write(out);
            if self.gain_application_space_chromaticities_flag == 3 {
                for r in 0..NUM_CHROMATICITY_VALUES {
                    out.extend_from_slice(
                        &self.gain_application_space_chromaticities[r].to_be_bytes(),
                    );
                }
            }
            for a in 0..(self.num_alternate_images as usize) {
                out.extend_from_slice(&self.alternate_hdr_headrooms[a].to_be_bytes());
                self.write_component_mixing(a, out);
                self.write_gain_curve(a, out);
            }
        } else {
            flags.pad_and_write(out);
        }
    }

    fn parse_component_mixing(&mut self, a: usize, s: &mut StreamReader<'_>) -> bool {
        if a == 0 || self.has_common_component_mix_params_flag == 0 {
            let Some(mut flags) = BitfieldReader::read_from_stream(s) else {
                return false;
            };
            self.component_mixing_type[a] = flags.read_bits(2);
            if self.component_mixing_type[a] != 3 {
                let reserved_zero = flags.read_bits(NUM_MIX_COEFFICIENTS as u8);
                if reserved_zero != 0 {
                    return false;
                }
            } else {
                for k in 0..NUM_MIX_COEFFICIENTS {
                    self.has_component_mixing_coefficient_flag[a][k] = flags.read_bits(1);
                }
                for k in 0..NUM_MIX_COEFFICIENTS {
                    if self.has_component_mixing_coefficient_flag[a][k] == 1 {
                        let Some(v) = s.read_u16_be() else {
                            return false;
                        };
                        self.component_mixing_coefficient[a][k] = v;
                    } else {
                        self.component_mixing_coefficient[a][k] = 0;
                    }
                }
            }
        } else {
            self.component_mixing_type[a] = self.component_mixing_type[0];
            if self.component_mixing_type[a] == 3 {
                self.component_mixing_coefficient[a] = self.component_mixing_coefficient[0];
            }
        }
        true
    }

    fn write_component_mixing(&self, a: usize, out: &mut Vec<u8>) {
        if a == 0 || self.has_common_component_mix_params_flag == 0 {
            let mut flags = BitfieldWriter::new();
            flags.write_bits(self.component_mixing_type[a], 2);
            if self.component_mixing_type[a] == 3 {
                for k in 0..NUM_MIX_COEFFICIENTS {
                    flags.write_bits(self.has_component_mixing_coefficient_flag[a][k], 1);
                }
                flags.pad_and_write(out);
                for k in 0..NUM_MIX_COEFFICIENTS {
                    if self.has_component_mixing_coefficient_flag[a][k] == 1 {
                        out.extend_from_slice(&self.component_mixing_coefficient[a][k].to_be_bytes());
                    }
                }
            } else {
                flags.pad_and_write(out);
            }
        }
    }

    fn parse_gain_curve(&mut self, a: usize, s: &mut StreamReader<'_>) -> bool {
        if a == 0 || self.has_common_curve_params_flag == 0 {
            let Some(mut flags) = BitfieldReader::read_from_stream(s) else {
                return false;
            };
            self.gain_curve_num_control_points_minus_1[a] = flags.read_bits(5);
            self.gain_curve_use_pchip_slope_flag[a] = flags.read_bits(1);
            let reserved_zero = flags.read_bits(2);
            if reserved_zero != 0 {
                return false;
            }
            let num_cp = (self.gain_curve_num_control_points_minus_1[a] as usize) + 1;
            for c in 0..num_cp {
                let Some(v) = s.read_u16_be() else {
                    return false;
                };
                self.gain_curve_control_points_x[a][c] = v;
            }
        } else {
            self.gain_curve_num_control_points_minus_1[a] =
                self.gain_curve_num_control_points_minus_1[0];
            self.gain_curve_use_pchip_slope_flag[a] = self.gain_curve_use_pchip_slope_flag[0];
            let num_cp = (self.gain_curve_num_control_points_minus_1[a] as usize) + 1;
            for c in 0..num_cp {
                self.gain_curve_control_points_x[a][c] = self.gain_curve_control_points_x[0][c];
            }
        }
        let num_cp = (self.gain_curve_num_control_points_minus_1[a] as usize) + 1;
        for c in 0..num_cp {
            let Some(v) = s.read_u16_be() else {
                return false;
            };
            self.gain_curve_control_points_y[a][c] = v;
        }
        if self.gain_curve_use_pchip_slope_flag[a] == 0 {
            for c in 0..num_cp {
                let Some(v) = s.read_u16_be() else {
                    return false;
                };
                self.gain_curve_control_points_theta[a][c] = v;
            }
        }
        true
    }

    fn write_gain_curve(&self, a: usize, out: &mut Vec<u8>) {
        let num_cp = (self.gain_curve_num_control_points_minus_1[a] as usize) + 1;
        if a == 0 || self.has_common_curve_params_flag == 0 {
            let mut flags = BitfieldWriter::new();
            flags.write_bits(self.gain_curve_num_control_points_minus_1[a], 5);
            flags.write_bits(self.gain_curve_use_pchip_slope_flag[a], 1);
            flags.pad_and_write(out);
            for c in 0..num_cp {
                out.extend_from_slice(&self.gain_curve_control_points_x[a][c].to_be_bytes());
            }
        }
        for c in 0..num_cp {
            out.extend_from_slice(&self.gain_curve_control_points_y[a][c].to_be_bytes());
        }
        if self.gain_curve_use_pchip_slope_flag[a] == 0 {
            for c in 0..num_cp {
                out.extend_from_slice(&self.gain_curve_control_points_theta[a][c].to_be_bytes());
            }
        }
    }
}

/// Matches `AdaptiveGlobalToneMap::parse`.
pub fn parse_agtm(data: &[u8]) -> Option<AdaptiveGlobalToneMap> {
    let mut s = StreamReader::new(data);
    let mut syntax = AgtmSyntax::default();
    if !syntax.parse_application_info(&mut s) {
        return None;
    }

    let mut agtm = AdaptiveGlobalToneMap::default();
    if syntax.has_custom_hdr_reference_white_flag == 1 {
        agtm.hdr_reference_white =
            uint16_to_float(syntax.hdr_reference_white, 1, 50000, 0, 5.0_f32);
    } else {
        agtm.hdr_reference_white = DEFAULT_HDR_REFERENCE_WHITE;
    }

    if syntax.has_adaptive_tone_map_flag == 0 {
        return Some(agtm);
    }

    let mut hatm = HeadroomAdaptiveToneMap::default();
    hatm.baseline_hdr_headroom =
        uint16_to_float(syntax.baseline_hdr_headroom, 0, 60000, 0, 10000.0_f32);
    if syntax.use_reference_white_tone_mapping_flag == 1 {
        populate_using_rwtmo(&mut hatm);
        agtm.headroom_adaptive_tone_map = Some(hatm);
        return Some(agtm);
    }

    let num_alt = (syntax.num_alternate_images as usize).min(MAX_NUM_ALTERNATE_IMAGES);
    hatm.alternate_images
        .resize(num_alt, AlternateImage::default());
    for a in 0..num_alt {
        hatm.alternate_images[a].hdr_headroom =
            uint16_to_float(syntax.alternate_hdr_headrooms[a], 0, 60000, 0, 10000.0_f32);
    }

    hatm.gain_application_space_primaries = match syntax.gain_application_space_chromaticities_flag
    {
        0 => PRIMARIES_REC709,
        1 => PRIMARIES_SMPTE_EG_432_1,
        2 => PRIMARIES_REC2020,
        3 => ColorSpacePrimaries {
            rx: uint16_to_float(
                syntax.gain_application_space_chromaticities[0],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            ry: uint16_to_float(
                syntax.gain_application_space_chromaticities[1],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            gx: uint16_to_float(
                syntax.gain_application_space_chromaticities[2],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            gy: uint16_to_float(
                syntax.gain_application_space_chromaticities[3],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            bx: uint16_to_float(
                syntax.gain_application_space_chromaticities[4],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            by: uint16_to_float(
                syntax.gain_application_space_chromaticities[5],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            wx: uint16_to_float(
                syntax.gain_application_space_chromaticities[6],
                0,
                50000,
                0,
                50000.0_f32,
            ),
            wy: uint16_to_float(
                syntax.gain_application_space_chromaticities[7],
                0,
                50000,
                0,
                50000.0_f32,
            ),
        },
        _ => return None,
    };

    for a in 0..num_alt {
        let mix = &mut hatm.alternate_images[a].color_gain_function.component_mixing;
        match syntax.component_mixing_type[a] {
            0 => {
                *mix = ComponentMixingFunction {
                    max: 1.0_f32,
                    ..Default::default()
                };
            }
            1 => {
                *mix = ComponentMixingFunction {
                    component: 1.0_f32,
                    ..Default::default()
                };
            }
            2 => {
                *mix = ComponentMixingFunction {
                    red: 1.0_f32 / 6.0_f32,
                    green: 1.0_f32 / 6.0_f32,
                    blue: 1.0_f32 / 6.0_f32,
                    max: 1.0_f32 / 2.0_f32,
                    min: 0.0_f32,
                    component: 0.0_f32,
                };
            }
            3 => {
                let mut p = [0.0_f32; NUM_MIX_COEFFICIENTS];
                let mut p_sum = 0.0_f32;
                for k in 0..NUM_MIX_COEFFICIENTS {
                    p[k] = uint16_to_float(
                        syntax.component_mixing_coefficient[a][k],
                        0,
                        50000,
                        0,
                        50000.0_f32,
                    );
                    p_sum += p[k];
                }
                if p_sum == 0.0_f32 {
                    return None;
                }
                mix.red = p[0] / p_sum;
                mix.green = p[1] / p_sum;
                mix.blue = p[2] / p_sum;
                mix.max = p[3] / p_sum;
                mix.min = p[4] / p_sum;
                mix.component = p[5] / p_sum;
            }
            _ => return None,
        }
    }

    for a in 0..num_alt {
        let cubic = &mut hatm.alternate_images[a].color_gain_function.gain_curve;
        let num_cp = (syntax.gain_curve_num_control_points_minus_1[a] as usize) + 1;
        cubic.control_points.resize(num_cp, ControlPoint::default());
        let sign = if syntax.baseline_hdr_headroom < syntax.alternate_hdr_headrooms[a] {
            1.0_f32
        } else {
            -1.0_f32
        };
        for c in 0..num_cp {
            cubic.control_points[c].x = uint16_to_float(
                syntax.gain_curve_control_points_x[a][c],
                0,
                64000,
                0,
                1000.0_f32,
            );
            cubic.control_points[c].y = sign
                * uint16_to_float(
                    syntax.gain_curve_control_points_y[a][c],
                    0,
                    60000,
                    0,
                    10000.0_f32,
                );
        }
        if syntax.gain_curve_use_pchip_slope_flag[a] == 0 {
            for c in 0..num_cp {
                let theta = uint16_to_float(
                    syntax.gain_curve_control_points_theta[a][c],
                    1,
                    35999,
                    18000,
                    36000.0_f32 / SK_FLOAT_PI,
                );
                cubic.control_points[c].m = theta.tan();
            }
        } else {
            populate_slope_from_pchip(&mut cubic.control_points);
        }
    }

    agtm.headroom_adaptive_tone_map = Some(hatm);
    if !validate_agtm(&agtm) {
        return None;
    }
    Some(agtm)
}

/// Matches `AdaptiveGlobalToneMap::serialize`.
pub fn serialize_agtm(agtm: &AdaptiveGlobalToneMap) -> Option<Vec<u8>> {
    if !validate_agtm(agtm) {
        return None;
    }

    let mut syntax = AgtmSyntax::default();
    syntax.application_version = 0;
    syntax.minimum_application_version = 0;
    syntax.has_custom_hdr_reference_white_flag =
        u8::from(agtm.hdr_reference_white != DEFAULT_HDR_REFERENCE_WHITE);
    if syntax.has_custom_hdr_reference_white_flag != 0 {
        syntax.hdr_reference_white =
            float_to_uint16(agtm.hdr_reference_white, 1, 50000, 0, 5.0_f32);
    }

    if let Some(ref hatm) = agtm.headroom_adaptive_tone_map {
        syntax.has_adaptive_tone_map_flag = 1;
        syntax.baseline_hdr_headroom =
            float_to_uint16(hatm.baseline_hdr_headroom, 0, 60000, 0, 10000.0_f32);

        let mut hatm_rwtmo = hatm.clone();
        populate_using_rwtmo(&mut hatm_rwtmo);
        syntax.use_reference_white_tone_mapping_flag = u8::from(*hatm == hatm_rwtmo);

        syntax.num_alternate_images = hatm.alternate_images.len() as u8;
        for (a, alt) in hatm.alternate_images.iter().enumerate() {
            syntax.alternate_hdr_headrooms[a] =
                float_to_uint16(alt.hdr_headroom, 0, 60000, 0, 10000.0_f32);
        }

        let p = &hatm.gain_application_space_primaries;
        if *p == PRIMARIES_REC709 {
            syntax.gain_application_space_chromaticities_flag = 0;
        } else if *p == PRIMARIES_SMPTE_EG_432_1 {
            syntax.gain_application_space_chromaticities_flag = 1;
        } else if *p == PRIMARIES_REC2020 {
            syntax.gain_application_space_chromaticities_flag = 2;
        } else {
            syntax.gain_application_space_chromaticities_flag = 3;
            syntax.gain_application_space_chromaticities[0] =
                float_to_uint16(p.rx, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[1] =
                float_to_uint16(p.ry, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[2] =
                float_to_uint16(p.gx, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[3] =
                float_to_uint16(p.gy, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[4] =
                float_to_uint16(p.bx, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[5] =
                float_to_uint16(p.by, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[6] =
                float_to_uint16(p.wx, 0, 50000, 0, 50000.0_f32);
            syntax.gain_application_space_chromaticities[7] =
                float_to_uint16(p.wy, 0, 50000, 0, 50000.0_f32);
        }

        syntax.has_common_component_mix_params_flag = 0;
        for (a, alt) in hatm.alternate_images.iter().enumerate() {
            let mix = &alt.color_gain_function.component_mixing;
            if mix.red == 0.0_f32
                && mix.green == 0.0_f32
                && mix.blue == 0.0_f32
                && mix.max == 1.0_f32
                && mix.min == 0.0_f32
                && mix.component == 0.0_f32
            {
                syntax.component_mixing_type[a] = 0;
            } else if mix.red == 0.0_f32
                && mix.green == 0.0_f32
                && mix.blue == 0.0_f32
                && mix.max == 0.0_f32
                && mix.min == 0.0_f32
                && mix.component == 1.0_f32
            {
                syntax.component_mixing_type[a] = 1;
            } else if mix.red == 1.0_f32 / 6.0_f32
                && mix.green == 1.0_f32 / 6.0_f32
                && mix.blue == 1.0_f32 / 6.0_f32
                && mix.max == 1.0_f32 / 2.0_f32
                && mix.min == 0.0_f32
                && mix.component == 0.0_f32
            {
                syntax.component_mixing_type[a] = 2;
            } else {
                syntax.component_mixing_type[a] = 3;
                syntax.component_mixing_coefficient[a][0] =
                    float_to_uint16(mix.red, 0, 50000, 0, 50000.0_f32);
                syntax.component_mixing_coefficient[a][1] =
                    float_to_uint16(mix.green, 0, 50000, 0, 50000.0_f32);
                syntax.component_mixing_coefficient[a][2] =
                    float_to_uint16(mix.blue, 0, 50000, 0, 50000.0_f32);
                syntax.component_mixing_coefficient[a][3] =
                    float_to_uint16(mix.max, 0, 50000, 0, 50000.0_f32);
                syntax.component_mixing_coefficient[a][4] =
                    float_to_uint16(mix.min, 0, 50000, 0, 50000.0_f32);
                syntax.component_mixing_coefficient[a][5] =
                    float_to_uint16(mix.component, 0, 50000, 0, 50000.0_f32);
                for k in 0..NUM_MIX_COEFFICIENTS {
                    syntax.has_component_mixing_coefficient_flag[a][k] =
                        u8::from(syntax.component_mixing_coefficient[a][k] != 0);
                }
            }
        }

        syntax.has_common_curve_params_flag = 0;
        for (a, alt) in hatm.alternate_images.iter().enumerate() {
            let sign = if hatm.baseline_hdr_headroom < alt.hdr_headroom {
                1.0_f32
            } else {
                -1.0_f32
            };
            let cps = &alt.color_gain_function.gain_curve.control_points;
            syntax.gain_curve_num_control_points_minus_1[a] = (cps.len() - 1) as u8;
            syntax.gain_curve_use_pchip_slope_flag[a] = 0;
            for (c, cp) in cps.iter().enumerate() {
                syntax.gain_curve_control_points_x[a][c] =
                    float_to_uint16(cp.x, 0, 64000, 0, 1000.0_f32);
                syntax.gain_curve_control_points_y[a][c] =
                    float_to_uint16(sign * cp.y, 0, 60000, 0, 10000.0_f32);
                let theta = cp.m.atan();
                syntax.gain_curve_control_points_theta[a][c] =
                    float_to_uint16(theta, 1, 35999, 18000, 36000.0_f32 / SK_FLOAT_PI);
            }
        }
    }

    let mut out = Vec::new();
    syntax.write_application_info(&mut out);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pchip_and_gain_curve() {
        let mut cps = vec![
            ControlPoint { x: 0.10720647, y: 0.37384606, m: 0.0 },
            ControlPoint { x: 0.76246667, y: 0.93143060, m: 0.0 },
            ControlPoint { x: 1.39535723, y: 0.0,        m: 0.0 },
            ControlPoint { x: 2.17572099, y: 1.23009354, m: 0.0 },
            ControlPoint { x: 2.47834070, y: 1.25542898, m: 0.0 },
            ControlPoint { x: 3.14288223, y: 2.22460677, m: 0.0 },
            ControlPoint { x: 3.35428070, y: 2.69226748, m: 0.0 },
            ControlPoint { x: 4.24864607, y: 3.45838813, m: 0.0 },
            ControlPoint { x: 4.59087493, y: 4.44597502, m: 0.0 },
            ControlPoint { x: 4.80373641, y: 5.19196203, m: 0.0 },
        ];
        populate_slope_from_pchip(&mut cps);
        let m_expected = [
            2.03242568, 0.0, 0.0, 0.14042951, 0.14250506, 1.82245618, 1.35855757, 1.43703564,
            3.18918733, 3.74186390,
        ];
        for i in 0..10 {
            assert!((cps[i].m - m_expected[i]).abs() < 1e-4);
        }
        let y_expected = [
            0.37384606, 0.86280187, 0.63630745, 0.05871820, 1.05625216, 1.26009455, 1.95243885,
            2.85680727, 3.19521825, 4.14318213, 5.13419092,
        ];
        for i in 0..11 {
            let y = evaluate_gain_curve(&cps, (i as f32) / 2.0);
            assert!((y - y_expected[i]).abs() < 1e-4);
        }
    }

    #[test]
    fn rwtmo_and_roundtrip_serialization() {
        let mut hatm = HeadroomAdaptiveToneMap {
            baseline_hdr_headroom: 1.0,
            ..Default::default()
        };
        populate_using_rwtmo(&mut hatm);
        assert_eq!(hatm.gain_application_space_primaries, PRIMARIES_REC2020);
        assert_eq!(hatm.alternate_images.len(), 2);
        assert!((hatm.alternate_images[1].hdr_headroom - 0.6151138).abs() < 1e-5);

        let agtm = AdaptiveGlobalToneMap {
            hdr_reference_white: 203.0,
            headroom_adaptive_tone_map: Some(hatm),
        };
        let bytes = serialize_agtm(&agtm).unwrap();
        let parsed = parse_agtm(&bytes).unwrap();
        assert_eq!(parsed, agtm);
    }

    #[test]
    fn weighting_cases() {
        let hatm = HeadroomAdaptiveToneMap {
            baseline_hdr_headroom: 1.0,
            gain_application_space_primaries: PRIMARIES_REC2020,
            alternate_images: vec![AlternateImage {
                hdr_headroom: 0.0,
                ..Default::default()
            }],
        };
        let w = compute_weighting(&hatm, 0.25);
        assert_eq!(w.alternate_image_index, [0, INVALID_ALTERNATE_IMAGE_INDEX]);
        assert_eq!(w.weight, [0.75, 0.0]);

        // NaN target returns zero weighting
        let w_nan = compute_weighting(&hatm, f32::NAN);
        assert_eq!(w_nan, Weighting::default());
    }

    #[test]
    fn pchip_edge_cases_and_component_mixing() {
        // Single point
        let mut single = vec![ControlPoint { x: 1.0, y: 1.0, m: 99.0 }];
        populate_slope_from_pchip(&mut single);
        assert_eq!(single[0].m, 0.0);

        // Left endpoint sign change s0 vs m -> 0
        let mut left_zero = vec![
            ControlPoint { x: 0.0, y: 0.0, m: 0.0 },
            ControlPoint { x: 1.0, y: 1.0, m: 0.0 },
            ControlPoint { x: 2.0, y: 5.0, m: 0.0 },
        ];
        populate_slope_from_pchip(&mut left_zero);
        assert_eq!(left_zero[0].m, 0.0);

        // Left endpoint |m| > 3|s0| -> 3*s0
        let mut left_clamp = vec![
            ControlPoint { x: 0.0, y: 0.0, m: 0.0 },
            ControlPoint { x: 1.0, y: 1.0, m: 0.0 },
            ControlPoint { x: 2.0, y: -9.0, m: 0.0 },
        ];
        populate_slope_from_pchip(&mut left_clamp);
        assert_eq!(left_clamp[0].m, 3.0);

        // Unnormalized component mixing
        let mix = ComponentMixingFunction {
            max: 1.5,
            component: 0.5,
            ..Default::default()
        };
        let out = evaluate_component_mixing_function(mix, [0.75, 0.5, 0.25, 0.125]);
        assert!((out[0] - 0.75).abs() < 1e-6);
        assert!((out[1] - 0.6875).abs() < 1e-6);
        assert!((out[2] - 0.625).abs() < 1e-6);
        assert_eq!(out[3], 0.125);
    }

    #[test]
    fn validation_rejects_invalid_agtm() {
        let mut agtm = AdaptiveGlobalToneMap::default();
        assert!(validate_agtm(&agtm));
        agtm.hdr_reference_white = 0.0;
        assert!(!validate_agtm(&agtm));
        agtm.hdr_reference_white = f32::NAN;
        assert!(!validate_agtm(&agtm));
        agtm.hdr_reference_white = 10001.0;
        assert!(!validate_agtm(&agtm));
    }

    #[test]
    fn custom_agtm_parse_serialize_roundtrip() {
        let agtm = AdaptiveGlobalToneMap {
            hdr_reference_white: 100.0,
            headroom_adaptive_tone_map: Some(HeadroomAdaptiveToneMap {
                baseline_hdr_headroom: 2.0,
                gain_application_space_primaries: ColorSpacePrimaries {
                    rx: 0.65,
                    ry: 0.32,
                    gx: 0.28,
                    gy: 0.61,
                    bx: 0.14,
                    by: 0.07,
                    wx: 0.3127,
                    wy: 0.329,
                },
                alternate_images: vec![AlternateImage {
                    hdr_headroom: 0.5,
                    color_gain_function: ColorGainFunction {
                        component_mixing: ComponentMixingFunction {
                            red: 0.25,
                            green: 0.5,
                            blue: 0.25,
                            ..Default::default()
                        },
                        gain_curve: GainCurve {
                            control_points: vec![
                                ControlPoint { x: 0.0, y: 0.0, m: 0.0 },
                                ControlPoint { x: 2.0, y: -1.5, m: -0.5 },
                            ],
                        },
                    },
                }],
            }),
        };
        assert!(validate_agtm(&agtm));
        let bytes = serialize_agtm(&agtm).unwrap();
        let parsed = parse_agtm(&bytes).unwrap();
        assert_eq!(parsed.hdr_reference_white, 100.0);
        let hatm = parsed.headroom_adaptive_tone_map.as_ref().unwrap();
        assert_eq!(hatm.baseline_hdr_headroom, 2.0);
        assert_eq!(hatm.alternate_images.len(), 1);
        assert!((hatm.alternate_images[0].hdr_headroom - 0.5).abs() < 1e-4);
        assert!((hatm.alternate_images[0].color_gain_function.gain_curve.control_points[1].y - (-1.5)).abs() < 1e-4);
    }
}
