//! Safe Rust port of `src/encode/SkICC.cpp` and `src/encode/SkICCPriv.h`.

use skia_rs_color::color_space::{
    Matrix3x3, TransferFunction, GAMUT_DISPLAY_P3, GAMUT_REC2020, GAMUT_SRGB, TF_2DOT2, TF_HLG,
    TF_LINEAR, TF_PQ, TF_SRGB,
};
use skia_rs_math::md5::Md5State;

const NUM_CHANNELS: usize = 3;

// The D50 illuminant.
const D50_X: f32 = 0.9642_f32;
const D50_Y: f32 = 1.0000_f32;
const D50_Z: f32 = 0.8249_f32;

const ONE_16_CURVE_TYPE: u16 = 0xFFFF;
const ONE_16_XYZ: u16 = 0x8000;

const ICC_HEADER_SIZE: usize = 132;
const ICC_TAG_TABLE_ENTRY_SIZE: usize = 12;

const fn four_byte_tag(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}

pub const RGB_COLOR_SPACE: u32 = four_byte_tag(b'R', b'G', b'B', b' ');
pub const XYZ_PCS_SPACE: u32 = four_byte_tag(b'X', b'Y', b'Z', b' ');
const DISPLAY_PROFILE: u32 = four_byte_tag(b'm', b'n', b't', b'r');
const ACSP_SIGNATURE: u32 = four_byte_tag(b'a', b'c', b's', b'p');

const TAG_R_XYZ: u32 = four_byte_tag(b'r', b'X', b'Y', b'Z');
const TAG_G_XYZ: u32 = four_byte_tag(b'g', b'X', b'Y', b'Z');
const TAG_B_XYZ: u32 = four_byte_tag(b'b', b'X', b'Y', b'Z');
const TAG_R_TRC: u32 = four_byte_tag(b'r', b'T', b'R', b'C');
const TAG_G_TRC: u32 = four_byte_tag(b'g', b'T', b'R', b'C');
const TAG_B_TRC: u32 = four_byte_tag(b'b', b'T', b'R', b'C');
const TAG_A2B0: u32 = four_byte_tag(b'A', b'2', b'B', b'0');
const TAG_B2A0: u32 = four_byte_tag(b'B', b'2', b'A', b'0');
const TAG_DESC: u32 = four_byte_tag(b'd', b'e', b's', b'c');
const TAG_CICP: u32 = four_byte_tag(b'c', b'i', b'c', b'p');
const TAG_HAGC: u32 = four_byte_tag(b'H', b'A', b'G', b'C');
const TAG_WTPT: u32 = four_byte_tag(b'w', b't', b'p', b't');
const TAG_CPRT: u32 = four_byte_tag(b'c', b'p', b'r', b't');

const TAG_CURVE_TYPE: u32 = four_byte_tag(b'c', b'u', b'r', b'v');
const TAG_PARA_CURVE_TYPE: u32 = four_byte_tag(b'p', b'a', b'r', b'a');
const TAG_TEXT_TYPE: u32 = four_byte_tag(b'm', b'l', b'u', b'c');
const TAG_HAGC_TYPE: u32 = four_byte_tag(b'h', b'a', b'g', b'c');
const TAG_MAB_TYPE: u32 = four_byte_tag(b'm', b'A', b'B', b' ');
const TAG_MBA_TYPE: u32 = four_byte_tag(b'm', b'B', b'A', b' ');

const PARA_EXPONENTIAL: u16 = 0;
const PARA_GABCDEF: u16 = 4;

const CICP_PRIMARIES_SRGB: u32 = 1;
const CICP_PRIMARIES_UNSPECIFIED: u32 = 2;
const CICP_PRIMARIES_REC2020: u32 = 9;
const CICP_PRIMARIES_P3: u32 = 12;

const CICP_TRFN_SRGB: u32 = 1;
const CICP_TRFN_2DOT2: u32 = 4;
const CICP_TRFN_LINEAR: u32 = 8;
const CICP_TRFN_PQ: u32 = 16;
const CICP_TRFN_HLG: u32 = 18;

const TONE_MAP_INPUT_MAX: f32 = 1000.0_f32 / 203.0_f32;
const TONE_MAP_OUTPUT_MAX: f32 = 1.0_f32;

// ============================================================================
// Transfer function classification & skcms-exact eval (for PQ/HLG LUT parity)
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TfType {
    Invalid = 0,
    SRgbish = 1,
    PQish = 2,
    HLGish = 3,
    HLGinvish = 4,
    PQ = 5,
    HLG = 6,
}

/// Exact port of `skcms_TransferFunction_getType` / `classify`.
pub fn classify_tf(tf: &TransferFunction) -> TfType {
    if tf.g < 0.0_f32 {
        if tf.g < -128.0_f32 {
            return TfType::Invalid;
        }
        let enum_g = -(tf.g as i32);
        if (-enum_g) as f32 != tf.g {
            return TfType::Invalid;
        }
        return match enum_g {
            2 => TfType::PQish,
            3 => TfType::HLGish,
            4 => TfType::HLGinvish,
            5 => {
                if tf.b != 0.0 || tf.c != 0.0 || tf.d != 0.0 || tf.e != 0.0 || tf.f != 0.0 {
                    TfType::Invalid
                } else {
                    TfType::PQ
                }
            }
            6 => {
                if tf.d != 0.0 || tf.e != 0.0 || tf.f != 0.0 {
                    TfType::Invalid
                } else {
                    TfType::HLG
                }
            }
            _ => TfType::Invalid,
        };
    }
    let sum = tf.a + tf.b + tf.c + tf.d + tf.e + tf.f + tf.g;
    if sum.is_finite()
        && tf.a >= 0.0
        && tf.c >= 0.0
        && tf.d >= 0.0
        && tf.g >= 0.0
        && tf.a * tf.d + tf.b >= 0.0
    {
        TfType::SRgbish
    } else {
        TfType::Invalid
    }
}

// Exact port of skcms's fast approximations used by `skcms_TransferFunction_eval`.
fn skcms_log2f(x: f32) -> f32 {
    let bits = x.to_bits() as i32;
    let e = (bits as f32) * (1.0_f32 / ((1u32 << 23) as f32));
    let m_bits = (bits & 0x007f_ffff) | 0x3f00_0000;
    let m = f32::from_bits(m_bits as u32);
    e - 124.225514990_f32 - 1.498030302_f32 * m - 1.725879990_f32 / (0.3520887068_f32 + m)
}

fn skcms_exp2f(x: f32) -> f32 {
    if x > 128.0_f32 {
        return f32::INFINITY;
    } else if x < -127.0_f32 {
        return 0.0_f32;
    }
    let fract = x - x.floor();
    let fbits = (1.0_f32 * ((1u32 << 23) as f32))
        * (x + 121.274057500_f32 - 1.490129070_f32 * fract + 27.728023300_f32 / (4.84252568_f32 - fract));
    if fbits >= (i32::MAX as f32) {
        return f32::INFINITY;
    } else if fbits < 0.0_f32 {
        return 0.0_f32;
    }
    let bits = fbits as i32;
    f32::from_bits(bits as u32)
}

fn skcms_powf(x: f32, y: f32) -> f32 {
    if x <= 0.0_f32 {
        return 0.0_f32;
    }
    if x == 1.0_f32 {
        return 1.0_f32;
    }
    skcms_exp2f(skcms_log2f(x) * y)
}

fn skcms_expf(x: f32) -> f32 {
    const LOG2_E: f32 = 1.4426950408889634074_f32;
    skcms_exp2f(LOG2_E * x)
}

fn skcms_eval_hlg(x: f32) -> f32 {
    let sign = if x < 0.0_f32 { -1.0_f32 } else { 1.0_f32 };
    let ax = x * sign;
    const A: f32 = 0.17883277_f32;
    const B: f32 = 0.28466892_f32;
    const C: f32 = 0.55991073_f32;
    sign * if ax <= 0.5_f32 {
        ax * ax / 3.0_f32
    } else {
        (skcms_expf((ax - C) / A) + B) / 12.0_f32
    }
}

fn skcms_eval_pq(x: f32) -> f32 {
    let sign = if x < 0.0_f32 { -1.0_f32 } else { 1.0_f32 };
    let ax = x * sign;
    const C1: f32 = 107.0_f32 / 128.0_f32;
    const C2: f32 = 2413.0_f32 / 128.0_f32;
    const C3: f32 = 2392.0_f32 / 128.0_f32;
    const M1: f32 = 1305.0_f32 / 8192.0_f32;
    const M2: f32 = 2523.0_f32 / 32.0_f32;
    let p = skcms_powf(ax, 1.0_f32 / M2);
    skcms_powf((p - C1) / (C2 - C3 * p), 1.0_f32 / M1)
}

// ============================================================================
// Fixed-point & encoding helpers
// ============================================================================

#[inline]
fn sk_float_saturate2int(x: f32) -> i32 {
    const MAX_S32_FITS_IN_FLOAT: f32 = 2147483520.0_f32;
    const MIN_S32_FITS_IN_FLOAT: f32 = -2147483648.0_f32;
    let x = if x < MAX_S32_FITS_IN_FLOAT {
        x
    } else {
        MAX_S32_FITS_IN_FLOAT
    };
    let x = if x > MIN_S32_FITS_IN_FLOAT {
        x
    } else {
        MIN_S32_FITS_IN_FLOAT
    };
    x as i32
}

#[inline]
fn float_round_to_fixed(x: f32) -> i32 {
    const FIXED_1: f64 = 65536.0;
    sk_float_saturate2int(((x as f64) * FIXED_1 + 0.5).floor() as f32)
}

#[inline]
fn float_to_uint16_number(x: f32, one: u16) -> u16 {
    let scaled = x * (one as f32) + 0.5_f32;
    if scaled > (one as f32) {
        return one;
    }
    if scaled < 0.0_f32 {
        return 0;
    }
    scaled as u16
}

fn pad_to_align4(buf: &mut Vec<u8>) {
    let rem = buf.len() & 3;
    if rem != 0 {
        buf.resize(buf.len() + (4 - rem), 0);
    }
}

fn md5_upper_hex(digest: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(32);
    for b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xF) as usize] as char);
    }
    out
}

#[inline]
fn nearly_equal_f32(x: f32, y: f32) -> bool {
    const TOLERANCE: f32 = 1.0_f32 / ((1u32 << 11) as f32);
    (x - y).abs() <= TOLERANCE
}

fn nearly_equal_tf(u: &TransferFunction, v: &TransferFunction) -> bool {
    nearly_equal_f32(u.g, v.g)
        && nearly_equal_f32(u.a, v.a)
        && nearly_equal_f32(u.b, v.b)
        && nearly_equal_f32(u.c, v.c)
        && nearly_equal_f32(u.d, v.d)
        && nearly_equal_f32(u.e, v.e)
        && nearly_equal_f32(u.f, v.f)
}

fn nearly_equal_mat(u: &Matrix3x3, v: &Matrix3x3) -> bool {
    for r in 0..3 {
        for c in 0..3 {
            if !nearly_equal_f32(u.vals[r][c], v.vals[r][c]) {
                return false;
            }
        }
    }
    true
}

fn get_cicp_primaries_icc(to_xyzd50: &Matrix3x3) -> u32 {
    if nearly_equal_mat(to_xyzd50, &GAMUT_SRGB) {
        CICP_PRIMARIES_SRGB
    } else if nearly_equal_mat(to_xyzd50, &GAMUT_DISPLAY_P3) {
        CICP_PRIMARIES_P3
    } else if nearly_equal_mat(to_xyzd50, &GAMUT_REC2020) {
        CICP_PRIMARIES_REC2020
    } else {
        CICP_PRIMARIES_UNSPECIFIED
    }
}

fn get_cicp_trfn_icc(fn_tf: &TransferFunction) -> u32 {
    match classify_tf(fn_tf) {
        TfType::SRgbish => {
            if nearly_equal_tf(fn_tf, &TF_SRGB) {
                CICP_TRFN_SRGB
            } else if nearly_equal_tf(fn_tf, &TF_2DOT2) {
                CICP_TRFN_2DOT2
            } else if nearly_equal_tf(fn_tf, &TF_LINEAR) {
                CICP_TRFN_LINEAR
            } else {
                0
            }
        }
        TfType::PQ | TfType::PQish => CICP_TRFN_PQ,
        TfType::HLG | TfType::HLGish => CICP_TRFN_HLG,
        TfType::HLGinvish | TfType::Invalid => 0,
    }
}

fn get_desc_string(fn_tf: &TransferFunction, to_xyzd50: &Matrix3x3) -> String {
    let cicp_trfn = get_cicp_trfn_icc(fn_tf);
    let cicp_primaries = get_cicp_primaries_icc(to_xyzd50);

    if cicp_trfn == CICP_PRIMARIES_SRGB && cicp_primaries == CICP_TRFN_SRGB {
        return "sRGB".to_string();
    }

    if cicp_primaries != CICP_PRIMARIES_UNSPECIFIED && cicp_trfn != 0 {
        let p_str = match cicp_primaries {
            CICP_PRIMARIES_SRGB => "sRGB",
            CICP_PRIMARIES_P3 => "Display P3",
            CICP_PRIMARIES_REC2020 => "Rec2020",
            _ => "Unknown",
        };
        let t_str = match cicp_trfn {
            CICP_TRFN_SRGB => "sRGB",
            CICP_TRFN_LINEAR => "Linear",
            CICP_TRFN_2DOT2 => "2.2",
            CICP_TRFN_PQ => "PQ",
            CICP_TRFN_HLG => "HLG",
            _ => "Unknown",
        };
        return format!("{} Gamut with {} Transfer", p_str, t_str);
    }

    let mut md5 = Md5State::new();
    for r in 0..3 {
        for c in 0..3 {
            md5.update(&to_xyzd50.vals[r][c].to_ne_bytes());
        }
    }
    for v in fn_tf.as_array() {
        md5.update(&v.to_ne_bytes());
    }
    let digest = md5.finish();
    format!("Google/Skia/{}", md5_upper_hex(digest))
}

fn tone_map_gain(x: f32) -> f32 {
    const A: f32 = TONE_MAP_OUTPUT_MAX / (TONE_MAP_INPUT_MAX * TONE_MAP_INPUT_MAX);
    const B: f32 = 1.0_f32 / TONE_MAP_OUTPUT_MAX;
    (1.0_f32 + A * x) / (1.0_f32 + B * x)
}

fn tone_map_inverse(y: f32) -> f32 {
    const A: f32 = TONE_MAP_OUTPUT_MAX / (TONE_MAP_INPUT_MAX * TONE_MAP_INPUT_MAX);
    const B: f32 = 1.0_f32 / TONE_MAP_OUTPUT_MAX;
    let a = A;
    let b = 1.0_f32 - B * y;
    let c = -y;
    let discriminant = b * b - 4.0_f32 * a * c;
    if discriminant < 0.0_f32 {
        return 0.0_f32;
    }
    (-b + discriminant.sqrt()) / (2.0_f32 * a)
}

fn hdr_trfn_eval(tf: &TransferFunction, mut x: f32) -> f32 {
    let ty = classify_tf(tf);
    if matches!(ty, TfType::HLGish | TfType::HLG) {
        let _ = TF_HLG;
        x = skcms_eval_hlg(x);
        x = ((x as f64) * (x as f64).powf(0.2_f64)) as f32;
    } else if matches!(ty, TfType::PQish | TfType::PQ) {
        let _ = TF_PQ;
        x = 10.0_f32 * skcms_eval_pq(x);
        x = x.min(1.0_f32);
    }
    x *= TONE_MAP_INPUT_MAX;
    x
}

// ============================================================================
// ICC Profile Descriptor Types (safe Rust mirror of skcms_ICCProfile subset)
// ============================================================================

#[derive(Clone, Debug)]
pub enum IccCurve<'a> {
    Parametric(TransferFunction),
    /// Big-endian 16-bit table bytes (length = `2 * table_entries`).
    Table16 {
        table_entries: u32,
        table_16_be: &'a [u8],
    },
}

#[derive(Copy, Clone, Debug, Default)]
pub struct IccMatrix3x4 {
    pub vals: [[f32; 4]; 3],
}

#[derive(Clone, Debug)]
pub struct IccLutStage<'a> {
    pub b_curves: [IccCurve<'a>; 3],
    pub a_curves: Option<[IccCurve<'a>; 3]>,
    pub grid_points: Option<[u8; 3]>,
    pub grid_16_be: Option<&'a [u8]>,
    pub m_curves: Option<[IccCurve<'a>; 3]>,
    pub matrix: Option<IccMatrix3x4>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct IccCicp {
    pub color_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
    pub video_full_range_flag: u8,
}

#[derive(Clone, Debug)]
pub struct IccProfileDesc<'a> {
    pub data_color_space: u32,
    pub pcs: u32,
    pub to_xyzd50: Option<Matrix3x3>,
    pub trc: Option<[IccCurve<'a>; 3]>,
    pub trc1_same_as_0: bool,
    pub trc2_same_as_1: bool,
    pub cicp: Option<IccCicp>,
    pub hagc: Option<&'a [u8]>,
    pub a2b: Option<IccLutStage<'a>>,
    pub b2a: Option<IccLutStage<'a>>,
}

// ============================================================================
// Binary tag writers
// ============================================================================

fn write_xyz_tag(x: f32, y: f32, z: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.extend_from_slice(&XYZ_PCS_SPACE.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(float_round_to_fixed(x) as u32).to_be_bytes());
    out.extend_from_slice(&(float_round_to_fixed(y) as u32).to_be_bytes());
    out.extend_from_slice(&(float_round_to_fixed(z) as u32).to_be_bytes());
    out
}

fn write_matrix(matrix: &IccMatrix3x4) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    for i in 0..3 {
        for j in 0..3 {
            out.extend_from_slice(&(float_round_to_fixed(matrix.vals[i][j]) as u32).to_be_bytes());
        }
    }
    for i in 0..3 {
        out.extend_from_slice(&(float_round_to_fixed(matrix.vals[i][3]) as u32).to_be_bytes());
    }
    out
}

fn write_text_tag(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let text_length = bytes.len() as u32;
    let mut out = Vec::with_capacity(28 + bytes.len() * 2 + 4);
    out.extend_from_slice(&TAG_TEXT_TYPE.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend_from_slice(&four_byte_tag(b'e', b'n', b'U', b'S').to_be_bytes());
    out.extend_from_slice(&(2 * text_length).to_be_bytes());
    out.extend_from_slice(&28u32.to_be_bytes());
    for &b in bytes {
        out.push(0);
        out.push(b);
    }
    pad_to_align4(&mut out);
    out
}

fn write_cicp_tag(cicp: &IccCicp) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&TAG_CICP.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.push(cicp.color_primaries);
    out.push(cicp.transfer_characteristics);
    out.push(cicp.matrix_coefficients);
    out.push(cicp.video_full_range_flag);
    out
}

fn write_hagc_tag(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + payload.len() + 4);
    out.extend_from_slice(&TAG_HAGC_TYPE.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    pad_to_align4(&mut out);
    out
}

fn write_trc_tag(trc: &IccCurve<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    match trc {
        IccCurve::Table16 {
            table_entries,
            table_16_be,
        } => {
            out.extend_from_slice(&TAG_CURVE_TYPE.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&table_entries.to_be_bytes());
            let byte_len = (*table_entries as usize) * 2;
            let avail = byte_len.min(table_16_be.len());
            out.extend_from_slice(&table_16_be[..avail]);
            if avail < byte_len {
                out.resize(out.len() + (byte_len - avail), 0);
            }
        }
        IccCurve::Parametric(fn_tf) => {
            out.extend_from_slice(&TAG_PARA_CURVE_TYPE.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
            if fn_tf.a == 1.0
                && fn_tf.b == 0.0
                && fn_tf.c == 0.0
                && fn_tf.d == 0.0
                && fn_tf.e == 0.0
                && fn_tf.f == 0.0
            {
                out.extend_from_slice(&PARA_EXPONENTIAL.to_be_bytes());
                out.extend_from_slice(&0u16.to_be_bytes());
                out.extend_from_slice(&(float_round_to_fixed(fn_tf.g) as u32).to_be_bytes());
            } else {
                out.extend_from_slice(&PARA_GABCDEF.to_be_bytes());
                out.extend_from_slice(&0u16.to_be_bytes());
                for v in fn_tf.as_array() {
                    out.extend_from_slice(&(float_round_to_fixed(v) as u32).to_be_bytes());
                }
            }
        }
    }
    pad_to_align4(&mut out);
    out
}

fn write_clut(grid_points: [u8; 3], grid_16_be: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..16 {
        out.push(if i < NUM_CHANNELS { grid_points[i] } else { 0 });
    }
    out.push(2); // 16-bit width
    out.push(0);
    out.push(0);
    out.push(0);

    let value_count =
        NUM_CHANNELS * (grid_points[0] as usize) * (grid_points[1] as usize) * (grid_points[2] as usize);
    let byte_len = value_count * 2;
    let avail = byte_len.min(grid_16_be.len());
    out.extend_from_slice(&grid_16_be[..avail]);
    if avail < byte_len {
        out.resize(out.len() + (byte_len - avail), 0);
    }
    pad_to_align4(&mut out);
    out
}

fn write_mab_or_mba_tag(tag_type: u32, stage: &IccLutStage<'_>) -> Vec<u8> {
    let mut offset: u32 = 32;

    let b_curves_offset = offset;
    let b_curves_data: [Vec<u8>; 3] = [
        write_trc_tag(&stage.b_curves[0]),
        write_trc_tag(&stage.b_curves[1]),
        write_trc_tag(&stage.b_curves[2]),
    ];
    for d in &b_curves_data {
        offset += d.len() as u32;
    }

    let mut clut_offset: u32 = 0;
    let clut = if let (Some(gp), Some(g16)) = (stage.grid_points, stage.grid_16_be) {
        clut_offset = offset;
        let data = write_clut(gp, g16);
        offset += data.len() as u32;
        Some(data)
    } else {
        None
    };

    let mut a_curves_offset: u32 = 0;
    let a_curves_data = if let Some(ref ac) = stage.a_curves {
        a_curves_offset = offset;
        let data = [
            write_trc_tag(&ac[0]),
            write_trc_tag(&ac[1]),
            write_trc_tag(&ac[2]),
        ];
        for d in &data {
            offset += d.len() as u32;
        }
        Some(data)
    } else {
        None
    };

    let mut matrix_offset: u32 = 0;
    let matrix_data = if let Some(ref mat) = stage.matrix {
        matrix_offset = offset;
        let data = write_matrix(mat);
        offset += data.len() as u32;
        Some(data)
    } else {
        None
    };

    let mut m_curves_offset: u32 = 0;
    let m_curves_data = if let Some(ref mc) = stage.m_curves {
        m_curves_offset = offset;
        let data = [
            write_trc_tag(&mc[0]),
            write_trc_tag(&mc[1]),
            write_trc_tag(&mc[2]),
        ];
        for d in &data {
            offset += d.len() as u32;
        }
        Some(data)
    } else {
        None
    };

    let mut out = Vec::with_capacity(offset as usize);
    out.extend_from_slice(&tag_type.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.push(NUM_CHANNELS as u8);
    out.push(NUM_CHANNELS as u8);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&b_curves_offset.to_be_bytes());
    out.extend_from_slice(&matrix_offset.to_be_bytes());
    out.extend_from_slice(&m_curves_offset.to_be_bytes());
    out.extend_from_slice(&clut_offset.to_be_bytes());
    out.extend_from_slice(&a_curves_offset.to_be_bytes());

    for d in &b_curves_data {
        out.extend_from_slice(d);
    }
    if let Some(ref c) = clut {
        out.extend_from_slice(c);
    }
    if let Some(ref ac) = a_curves_data {
        for d in ac {
            out.extend_from_slice(d);
        }
    }
    if let Some(ref m) = matrix_data {
        out.extend_from_slice(m);
    }
    if let Some(ref mc) = m_curves_data {
        for d in mc {
            out.extend_from_slice(d);
        }
    }
    debug_assert_eq!(out.len(), offset as usize);
    out
}

/// Exact port of `SkWriteICCProfile(const skcms_ICCProfile*, const char*)`.
pub fn write_icc_profile(profile: &IccProfileDesc<'_>, desc: Option<&str>) -> Vec<u8> {
    let mut uses_4_4_features = false;
    let mut tags: Vec<(u32, Vec<u8>)> = Vec::new();

    if let Some(m) = profile.to_xyzd50 {
        tags.push((TAG_R_XYZ, write_xyz_tag(m.vals[0][0], m.vals[1][0], m.vals[2][0])));
        tags.push((TAG_G_XYZ, write_xyz_tag(m.vals[0][1], m.vals[1][1], m.vals[2][1])));
        tags.push((TAG_B_XYZ, write_xyz_tag(m.vals[0][2], m.vals[1][2], m.vals[2][2])));
    }

    tags.push((TAG_WTPT, write_xyz_tag(D50_X, D50_Y, D50_Z)));

    if let Some(ref trc) = profile.trc {
        tags.push((TAG_R_TRC, write_trc_tag(&trc[0])));
        if profile.trc1_same_as_0 {
            tags.push((TAG_G_TRC, Vec::new()));
        } else {
            tags.push((TAG_G_TRC, write_trc_tag(&trc[1])));
        }
        if profile.trc2_same_as_1 {
            tags.push((TAG_B_TRC, Vec::new()));
        } else {
            tags.push((TAG_B_TRC, write_trc_tag(&trc[2])));
        }
    }

    if let Some(ref cicp) = profile.cicp {
        uses_4_4_features = true;
        tags.push((TAG_CICP, write_cicp_tag(cicp)));
    }

    if let Some(hagc) = profile.hagc {
        uses_4_4_features = true;
        tags.push((TAG_HAGC, write_hagc_tag(hagc)));
    }

    if let Some(ref a2b) = profile.a2b {
        tags.push((TAG_A2B0, write_mab_or_mba_tag(TAG_MAB_TYPE, a2b)));
    }

    if let Some(ref b2a) = profile.b2a {
        tags.push((TAG_B2A0, write_mab_or_mba_tag(TAG_MBA_TYPE, b2a)));
    }

    tags.push((TAG_CPRT, write_text_tag("Google Inc. 2016")));

    let generated_desc;
    let final_desc = match desc {
        Some(s) if !s.is_empty() => s,
        _ => {
            let mut md5 = Md5State::new();
            for (tag_sig, tag_data) in &tags {
                md5.update(&tag_sig.to_ne_bytes());
                md5.update(tag_data);
            }
            generated_desc = format!("Google/Skia/{}", md5_upper_hex(md5.finish()));
            generated_desc.as_str()
        }
    };
    tags.insert(0, (TAG_DESC, write_text_tag(final_desc)));

    let tag_data_size: usize = tags.iter().map(|(_, d)| d.len()).sum();
    let tag_table_size = ICC_TAG_TABLE_ENTRY_SIZE * tags.len();
    let profile_size = ICC_HEADER_SIZE + tag_table_size + tag_data_size;

    let mut out = Vec::with_capacity(profile_size);

    // Header (132 bytes)
    let version: u32 = if uses_4_4_features {
        0x0440_0000
    } else {
        0x0430_0000
    };
    out.extend_from_slice(&(profile_size as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // cmm_type
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&DISPLAY_PROFILE.to_be_bytes());
    out.extend_from_slice(&profile.data_color_space.to_be_bytes());
    out.extend_from_slice(&profile.pcs.to_be_bytes());
    out.extend_from_slice(&2016u16.to_be_bytes()); // year
    out.extend_from_slice(&1u16.to_be_bytes()); // month
    out.extend_from_slice(&1u16.to_be_bytes()); // day
    out.extend_from_slice(&0u16.to_be_bytes()); // hours
    out.extend_from_slice(&0u16.to_be_bytes()); // minutes
    out.extend_from_slice(&0u16.to_be_bytes()); // seconds
    out.extend_from_slice(&ACSP_SIGNATURE.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // platform
    out.extend_from_slice(&0u32.to_be_bytes()); // flags
    out.extend_from_slice(&0u32.to_be_bytes()); // device_manufacturer
    out.extend_from_slice(&0u32.to_be_bytes()); // device_model
    out.extend_from_slice(&[0u8; 8]); // device_attributes
    out.extend_from_slice(&1u32.to_be_bytes()); // rendering_intent
    out.extend_from_slice(&(float_round_to_fixed(D50_X) as u32).to_be_bytes());
    out.extend_from_slice(&(float_round_to_fixed(D50_Y) as u32).to_be_bytes());
    out.extend_from_slice(&(float_round_to_fixed(D50_Z) as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // creator
    out.extend_from_slice(&[0u8; 16]); // profile_id
    out.extend_from_slice(&[0u8; 28]); // reserved
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    debug_assert_eq!(out.len(), ICC_HEADER_SIZE);

    // Tag table
    let mut last_tag_offset = ICC_HEADER_SIZE + tag_table_size;
    let mut last_tag_size = 0usize;
    for (tag_sig, tag_data) in &tags {
        if !tag_data.is_empty() {
            last_tag_offset += last_tag_size;
            last_tag_size = tag_data.len();
        }
        out.extend_from_slice(&tag_sig.to_be_bytes());
        out.extend_from_slice(&(last_tag_offset as u32).to_be_bytes());
        out.extend_from_slice(&(last_tag_size as u32).to_be_bytes());
    }

    // Tag payloads
    for (_, tag_data) in &tags {
        if !tag_data.is_empty() {
            out.extend_from_slice(tag_data);
        }
    }

    debug_assert_eq!(out.len(), profile_size);
    out
}

/// Exact port of `SkWriteICCProfile(const SkColorSpace*, const skhdr::Metadata*)`.
pub fn write_icc_profile_from_color_space(
    fn_tf: &TransferFunction,
    to_xyzd50: &Matrix3x3,
    serialized_agtm: Option<&[u8]>,
) -> Vec<u8> {
    let tf_type = classify_tf(fn_tf);
    let is_srgbish = matches!(tf_type, TfType::SRgbish);
    let is_hdr = matches!(
        tf_type,
        TfType::PQ | TfType::PQish | TfType::HLG | TfType::HLGish
    );
    let is_hlg = matches!(tf_type, TfType::HLG | TfType::HLGish);

    let mut trc_table_bytes: Vec<u8> = Vec::new();
    let mut a2b_grid_bytes: Vec<u8> = Vec::new();

    const TRC_TABLE_SIZE: usize = 65;
    const GRID_SIZE: usize = 11;

    if is_hdr {
        trc_table_bytes.reserve(TRC_TABLE_SIZE * 2);
        for i in 0..TRC_TABLE_SIZE {
            let mut x = (i as f32) / ((TRC_TABLE_SIZE as f32) - 1.0_f32);
            x = hdr_trfn_eval(fn_tf, x);
            x *= tone_map_gain(x);
            let v = float_to_uint16_number(x, ONE_16_CURVE_TYPE);
            trc_table_bytes.extend_from_slice(&v.to_be_bytes());
        }

        a2b_grid_bytes.reserve(GRID_SIZE * GRID_SIZE * GRID_SIZE * NUM_CHANNELS * 2);
        for r_idx in 0..GRID_SIZE {
            for g_idx in 0..GRID_SIZE {
                for b_idx in 0..GRID_SIZE {
                    let mut rgb = [
                        (r_idx as f32) / ((GRID_SIZE as f32) - 1.0_f32),
                        (g_idx as f32) / ((GRID_SIZE as f32) - 1.0_f32),
                        (b_idx as f32) / ((GRID_SIZE as f32) - 1.0_f32),
                    ];
                    for c in &mut rgb {
                        *c = tone_map_inverse(*c);
                    }
                    if is_hlg {
                        for c in &mut rgb {
                            *c /= TONE_MAP_INPUT_MAX;
                        }
                        for c in &mut rgb {
                            *c = (*c as f64).powf(1.0_f64 / 1.2_f64) as f32;
                        }
                        let y = 0.2627_f32 * rgb[0] + 0.6780_f32 * rgb[1] + 0.0593_f32 * rgb[2];
                        let y_pow = (y as f64).powf(0.2_f64);
                        for c in &mut rgb {
                            *c = ((*c as f64) * y_pow) as f32;
                        }
                        for c in &mut rgb {
                            *c *= TONE_MAP_INPUT_MAX;
                        }
                    }
                    {
                        let max_rgb = rgb[0].max(rgb[1]).max(rgb[2]);
                        for c in &mut rgb {
                            *c *= tone_map_gain((0.5_f64 * ((*c + max_rgb) as f64)) as f32);
                            *c = (*c).min(1.0_f32);
                        }
                    }
                    for &c in &rgb {
                        let v = float_to_uint16_number(c, ONE_16_XYZ);
                        a2b_grid_bytes.extend_from_slice(&v.to_be_bytes());
                    }
                }
            }
        }
    }

    let trc = if is_srgbish {
        Some([
            IccCurve::Parametric(*fn_tf),
            IccCurve::Parametric(*fn_tf),
            IccCurve::Parametric(*fn_tf),
        ])
    } else {
        None
    };

    let (a2b, b2a) = if is_hdr {
        let mut mat = IccMatrix3x4::default();
        for i in 0..3 {
            for j in 0..3 {
                mat.vals[i][j] = to_xyzd50.vals[i][j];
            }
            mat.vals[i][3] = 0.0_f32;
        }
        let table_curve = || IccCurve::Table16 {
            table_entries: TRC_TABLE_SIZE as u32,
            table_16_be: &trc_table_bytes,
        };
        let a2b_stage = IccLutStage {
            b_curves: [
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
            ],
            a_curves: Some([table_curve(), table_curve(), table_curve()]),
            grid_points: Some([GRID_SIZE as u8; 3]),
            grid_16_be: Some(&a2b_grid_bytes),
            m_curves: Some([
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
            ]),
            matrix: Some(mat),
        };
        let b2a_stage = IccLutStage {
            b_curves: [
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
            ],
            a_curves: None,
            grid_points: None,
            grid_16_be: None,
            m_curves: None,
            matrix: None,
        };
        (Some(a2b_stage), Some(b2a_stage))
    } else {
        (None, None)
    };

    let cicp = if is_hdr {
        Some(IccCicp {
            color_primaries: get_cicp_primaries_icc(to_xyzd50) as u8,
            transfer_characteristics: get_cicp_trfn_icc(fn_tf) as u8,
            matrix_coefficients: 0,
            video_full_range_flag: 1,
        })
    } else {
        None
    };

    let hagc = serialized_agtm.filter(|b| !b.is_empty());

    let desc_str = get_desc_string(fn_tf, to_xyzd50);
    let profile = IccProfileDesc {
        data_color_space: RGB_COLOR_SPACE,
        pcs: XYZ_PCS_SPACE,
        to_xyzd50: Some(*to_xyzd50),
        trc,
        trc1_same_as_0: true,
        trc2_same_as_1: true,
        cicp,
        hagc,
        a2b,
        b2a,
    };

    write_icc_profile(&profile, Some(&desc_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_color::color_space::GAMUT_ADOBE_RGB;

    #[test]
    fn write_srgb_icc_header_and_tags() {
        let bytes = write_icc_profile_from_color_space(&TF_SRGB, &GAMUT_SRGB, None);
        // Check size header matches actual length
        let hdr_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(hdr_size, bytes.len());
        // Version 4.3 (no CICP/HAGC)
        assert_eq!(&bytes[8..12], &0x0430_0000u32.to_be_bytes());
        // Signature 'acsp'
        assert_eq!(&bytes[36..40], b"acsp");
        // Tag count: desc, rXYZ, gXYZ, bXYZ, wtpt, rTRC, gTRC, bTRC, cprt = 9
        let tag_count = u32::from_be_bytes([bytes[128], bytes[129], bytes[130], bytes[131]]);
        assert_eq!(tag_count, 9);
        // rTRC, gTRC, bTRC should share offset & size
        let r_trc_off = &bytes[132 + 5 * 12 + 4..132 + 5 * 12 + 12];
        let g_trc_off = &bytes[132 + 6 * 12 + 4..132 + 6 * 12 + 12];
        let b_trc_off = &bytes[132 + 7 * 12 + 4..132 + 7 * 12 + 12];
        assert_eq!(r_trc_off, g_trc_off);
        assert_eq!(g_trc_off, b_trc_off);
    }

    #[test]
    fn write_pq_rec2020_icc_has_v44_cicp_a2b_b2a() {
        let bytes = write_icc_profile_from_color_space(&TF_PQ, &GAMUT_REC2020, None);
        let hdr_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(hdr_size, bytes.len());
        // Version 4.4 because of CICP
        assert_eq!(&bytes[8..12], &0x0440_0000u32.to_be_bytes());
        // Tag count: desc, rXYZ, gXYZ, bXYZ, wtpt, cicp, A2B0, B2A0, cprt = 9
        let tag_count = u32::from_be_bytes([bytes[128], bytes[129], bytes[130], bytes[131]]);
        assert_eq!(tag_count, 9);
    }

    #[test]
    fn desc_string_generation() {
        assert_eq!(get_desc_string(&TF_SRGB, &GAMUT_SRGB), "sRGB");
        assert_eq!(
            get_desc_string(&TF_PQ, &GAMUT_REC2020),
            "Rec2020 Gamut with PQ Transfer"
        );
        assert_eq!(
            get_desc_string(&TF_HLG, &GAMUT_DISPLAY_P3),
            "Display P3 Gamut with HLG Transfer"
        );
        let custom = get_desc_string(&TF_2DOT2, &GAMUT_ADOBE_RGB);
        assert!(custom.starts_with("Google/Skia/"));
        assert_eq!(custom.len(), "Google/Skia/".len() + 32);
    }

    #[test]
    fn write_icc_with_hagc_and_empty_desc_fallback() {
        let agtm_payload = [0x00u8, 0x40, 0x00, 0x01];
        let bytes = write_icc_profile_from_color_space(&TF_SRGB, &GAMUT_SRGB, Some(&agtm_payload));
        // Version 4.4 because of HAGC
        assert_eq!(&bytes[8..12], &0x0440_0000u32.to_be_bytes());
        let tag_count = u32::from_be_bytes([bytes[128], bytes[129], bytes[130], bytes[131]]);
        assert_eq!(tag_count, 10);

        // Empty desc triggers MD5-based description
        let profile = IccProfileDesc {
            data_color_space: RGB_COLOR_SPACE,
            pcs: XYZ_PCS_SPACE,
            to_xyzd50: Some(GAMUT_SRGB),
            trc: Some([
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
                IccCurve::Parametric(TF_LINEAR),
            ]),
            trc1_same_as_0: true,
            trc2_same_as_1: true,
            cicp: None,
            hagc: None,
            a2b: None,
            b2a: None,
        };
        let empty_desc_bytes = write_icc_profile(&profile, Some(""));
        let null_desc_bytes = write_icc_profile(&profile, None);
        assert_eq!(empty_desc_bytes, null_desc_bytes);
        let hdr_size = u32::from_be_bytes([
            empty_desc_bytes[0],
            empty_desc_bytes[1],
            empty_desc_bytes[2],
            empty_desc_bytes[3],
        ]) as usize;
        assert_eq!(hdr_size, empty_desc_bytes.len());
    }
}
