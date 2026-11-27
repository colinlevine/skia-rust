//! Safe Rust port of `src/codec/SkCodecColorProfile.cpp`.

use skia_rs_color::color_space::{
    get_cicp_primaries, get_cicp_transfer_fn, Matrix3x3, TransferFunction,
};

pub const SKCMS_SIGNATURE_RGB: u32 = 0x5247_4220;
pub const SKCMS_SIGNATURE_CMYK: u32 = 0x434D_594B;
pub const SKCMS_SIGNATURE_GRAY: u32 = 0x4752_4159;

/// Matches `SkCodecs::ColorProfile::DataSpace`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DataSpace {
    Rgb = 0,
    Cmyk = 1,
    Gray = 2,
    Other = 3,
}

#[inline]
pub const fn classify_data_space(data_color_space: u32) -> DataSpace {
    match data_color_space {
        SKCMS_SIGNATURE_RGB => DataSpace::Rgb,
        SKCMS_SIGNATURE_CMYK => DataSpace::Cmyk,
        SKCMS_SIGNATURE_GRAY => DataSpace::Gray,
        _ => DataSpace::Other,
    }
}

/// Exact port of `cicp_get_android_sk_color_space` in `SkCodecColorProfile.cpp`.
/// Returns `(TransferFunction, Matrix3x3)` to pass to `SkColorSpace::MakeRGB`.
pub fn cicp_get_android_color_space(
    color_primaries: u8,
    transfer_characteristics: u8,
    matrix_coefficients: u8,
    full_range_flag: u8,
) -> Option<(TransferFunction, Matrix3x3)> {
    if matrix_coefficients != 0 || full_range_flag != 1 {
        return None;
    }
    let primaries = get_cicp_primaries(color_primaries)?;
    let mut trfn = get_cicp_transfer_fn(transfer_characteristics)?;
    match transfer_characteristics {
        16 => {
            // Android expects PQ to match 203 nits to SDR white.
            trfn = TransferFunction {
                g: -2.0_f32,
                a: -1.55522297832_f32,
                b: 1.86045365631_f32,
                c: 32.0_f32 / 2523.0_f32,
                d: 2413.0_f32 / 128.0_f32,
                e: -2392.0_f32 / 128.0_f32,
                f: 8192.0_f32 / 1305.0_f32,
            };
        }
        18 => {
            // Android expects HLG to match 203 nits to SDR white.
            // skcms_TransferFunction_makeScaledHLGish(&trfn, K, R, G, a, b, c) sets
            // { -3.0, R, G, a, b, c, K - 1.0 }.
            trfn = TransferFunction {
                g: -3.0_f32,
                a: 2.0_f32,
                b: 2.0_f32,
                c: 1.0_f32 / 0.17883277_f32,
                d: 0.28466892_f32,
                e: 0.55991073_f32,
                f: 0.314509843_f32 - 1.0_f32,
            };
        }
        _ => {}
    }
    Some((trfn, primaries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_rs_color::color_space::GAMUT_REC2020;

    #[test]
    fn data_space_classification() {
        assert_eq!(classify_data_space(SKCMS_SIGNATURE_RGB), DataSpace::Rgb);
        assert_eq!(classify_data_space(SKCMS_SIGNATURE_CMYK), DataSpace::Cmyk);
        assert_eq!(classify_data_space(SKCMS_SIGNATURE_GRAY), DataSpace::Gray);
        assert_eq!(classify_data_space(0x12345678), DataSpace::Other);
    }

    #[test]
    fn cicp_android_pq_and_hlg() {
        let (tf_pq, mat_pq) = cicp_get_android_color_space(9, 16, 0, 1).unwrap();
        assert_eq!(tf_pq.g, -2.0);
        assert_eq!(mat_pq, GAMUT_REC2020);

        let (tf_hlg, _) = cicp_get_android_color_space(9, 18, 0, 1).unwrap();
        assert_eq!(tf_hlg.g, -3.0);
        assert!((tf_hlg.f - (0.314509843 - 1.0)).abs() < 1e-7);

        // Non-zero matrix or limited range rejected
        assert!(cicp_get_android_color_space(9, 16, 1, 1).is_none());
        assert!(cicp_get_android_color_space(9, 16, 0, 0).is_none());
    }
}
