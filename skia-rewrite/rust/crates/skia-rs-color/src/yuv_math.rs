//! Safe Rust port of `src/core/SkYUVMath.cpp`.

use skia_rs_geometry::m44::M44;

/// Matches `enum SkYUVColorSpace : int` in `include/core/SkImageInfo.h`.
pub const YUV_CS_REC601_8BIT_FULL: i32 = 0;
pub const YUV_CS_REC601_8BIT_LIMITED: i32 = 1;
pub const YUV_CS_REC709_8BIT_FULL: i32 = 8;
pub const YUV_CS_REC709_8BIT_LIMITED: i32 = 9;
pub const YUV_CS_BT2020_8BIT_FULL: i32 = 16;
pub const YUV_CS_BT2020_8BIT_LIMITED: i32 = 17;
pub const YUV_CS_BT2020_10BIT_FULL: i32 = 18;
pub const YUV_CS_BT2020_10BIT_LIMITED: i32 = 19;
pub const YUV_CS_BT2020_12BIT_FULL: i32 = 20;
pub const YUV_CS_BT2020_12BIT_LIMITED: i32 = 21;
pub const YUV_CS_BT2020_16BIT_FULL: i32 = 22;
pub const YUV_CS_BT2020_16BIT_LIMITED: i32 = 23;
pub const YUV_CS_FCC_8BIT_FULL: i32 = 24;
pub const YUV_CS_FCC_8BIT_LIMITED: i32 = 25;
pub const YUV_CS_SMPTE240_8BIT_FULL: i32 = 32;
pub const YUV_CS_SMPTE240_8BIT_LIMITED: i32 = 33;
pub const YUV_CS_YDZDX_8BIT_FULL: i32 = 40;
pub const YUV_CS_YDZDX_8BIT_LIMITED: i32 = 41;
pub const YUV_CS_GBR_8BIT_FULL: i32 = 48;
pub const YUV_CS_GBR_8BIT_LIMITED: i32 = 49;
pub const YUV_CS_YCGCO_8BIT_FULL: i32 = 56;
pub const YUV_CS_YCGCO_8BIT_LIMITED: i32 = 57;
pub const YUV_CS_YCGCO_10BIT_FULL: i32 = 58;
pub const YUV_CS_YCGCO_10BIT_LIMITED: i32 = 59;
pub const YUV_CS_YCGCO_12BIT_FULL: i32 = 60;
pub const YUV_CS_YCGCO_12BIT_LIMITED: i32 = 61;
pub const YUV_CS_YCGCO_16BIT_FULL: i32 = 62;
pub const YUV_CS_YCGCO_16BIT_LIMITED: i32 = 63;
pub const YUV_CS_IDENTITY: i32 = 64;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum YuvRange {
    Full,
    Limited,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatrixCoefficients {
    Rec601,
    Rec709,
    BT2020,
    FCC,
    SMPTE240,
    YDZDX,
    GBR,
    YCgCo,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct YuvColorSpaceInfo {
    pub matrix_coefficients: MatrixCoefficients,
    pub range: YuvRange,
    pub bit_depth: i32,
}

const COLOR_SPACE_INFO: [YuvColorSpaceInfo; 64] = [
    // Rec601
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec601, range: YuvRange::Limited, bit_depth: 16 },
    // Rec709
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::Rec709, range: YuvRange::Limited, bit_depth: 16 },
    // BT2020
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::BT2020, range: YuvRange::Limited, bit_depth: 16 },
    // FCC
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::FCC, range: YuvRange::Limited, bit_depth: 16 },
    // SMPTE240
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::SMPTE240, range: YuvRange::Limited, bit_depth: 16 },
    // YDZDX
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YDZDX, range: YuvRange::Limited, bit_depth: 16 },
    // GBR
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::GBR, range: YuvRange::Limited, bit_depth: 16 },
    // YCgCo
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Full, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Limited, bit_depth: 8 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Full, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Limited, bit_depth: 10 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Full, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Limited, bit_depth: 12 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Full, bit_depth: 16 },
    YuvColorSpaceInfo { matrix_coefficients: MatrixCoefficients::YCgCo, range: YuvRange::Limited, bit_depth: 16 },
];

// Precomputed 6-decimal-place tables (exact match for SkYUVMath.cpp default precision).

const REC601_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.299000, 0.587000, 0.114000, 0.000000, 0.000000,
    -0.168736, -0.331264, 0.500000, 0.000000, 0.501961,
    0.500000, -0.418688, -0.081312, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC601_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -0.000000, 1.402000, 0.000000, -0.703749,
    1.000000, -0.344136, -0.714136, 0.000000, 0.531211,
    1.000000, 1.772000, 0.000000, 0.000000, -0.889475,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC601_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.256788, 0.504129, 0.097906, 0.000000, 0.062745,
    -0.148223, -0.290993, 0.439216, 0.000000, 0.501961,
    0.439216, -0.367788, -0.071427, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC601_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.164384, -0.000000, 1.596027, 0.000000, -0.874202,
    1.164384, -0.391762, -0.812968, 0.000000, 0.531668,
    1.164384, 2.017232, 0.000000, 0.000000, -1.085631,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC709_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.212600, 0.715200, 0.072200, 0.000000, 0.000000,
    -0.114572, -0.385428, 0.500000, 0.000000, 0.501961,
    0.500000, -0.454153, -0.045847, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC709_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -0.000000, 1.574800, 0.000000, -0.790488,
    1.000000, -0.187324, -0.468124, 0.000000, 0.329010,
    1.000000, 1.855600, -0.000000, 0.000000, -0.931439,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC709_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.182586, 0.614231, 0.062007, 0.000000, 0.062745,
    -0.100644, -0.338572, 0.439216, 0.000000, 0.501961,
    0.439216, -0.398942, -0.040274, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const REC709_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.164384, -0.000000, 1.792741, 0.000000, -0.972945,
    1.164384, -0.213249, -0.532909, 0.000000, 0.301483,
    1.164384, 2.112402, -0.000000, 0.000000, -1.133402,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.262700, 0.678000, 0.059300, 0.000000, 0.000000,
    -0.139630, -0.360370, 0.500000, 0.000000, 0.501961,
    0.500000, -0.459786, -0.040214, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -0.000000, 1.474600, 0.000000, -0.740191,
    1.000000, -0.164553, -0.571353, 0.000000, 0.369396,
    1.000000, 1.881400, -0.000000, 0.000000, -0.944389,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.225613, 0.582282, 0.050928, 0.000000, 0.062745,
    -0.122655, -0.316560, 0.439216, 0.000000, 0.501961,
    0.439216, -0.403890, -0.035326, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.164384, -0.000000, 1.678674, 0.000000, -0.915688,
    1.164384, -0.187326, -0.650424, 0.000000, 0.347458,
    1.164384, 2.141772, -0.000000, 0.000000, -1.148145,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_10BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.262700, 0.678000, 0.059300, 0.000000, 0.000000,
    -0.139630, -0.360370, 0.500000, 0.000000, 0.500489,
    0.500000, -0.459786, -0.040214, 0.000000, 0.500489,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_10BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -0.000000, 1.474600, 0.000000, -0.738021,
    1.000000, -0.164553, -0.571353, 0.000000, 0.368313,
    1.000000, 1.881400, -0.000000, 0.000000, -0.941620,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_10BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.224951, 0.580575, 0.050779, 0.000000, 0.062561,
    -0.122296, -0.315632, 0.437928, 0.000000, 0.500489,
    0.437928, -0.402706, -0.035222, 0.000000, 0.500489,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_10BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.167808, -0.000000, 1.683611, 0.000000, -0.915688,
    1.167808, -0.187877, -0.652337, 0.000000, 0.347458,
    1.167808, 2.148072, -0.000000, 0.000000, -1.148145,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_12BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.262700, 0.678000, 0.059300, 0.000000, 0.000000,
    -0.139630, -0.360370, 0.500000, 0.000000, 0.500122,
    0.500000, -0.459786, -0.040214, 0.000000, 0.500122,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_12BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -0.000000, 1.474600, 0.000000, -0.737480,
    1.000000, -0.164553, -0.571353, 0.000000, 0.368043,
    1.000000, 1.881400, -0.000000, 0.000000, -0.940930,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_12BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.224787, 0.580149, 0.050742, 0.000000, 0.062515,
    -0.122206, -0.315401, 0.437607, 0.000000, 0.500122,
    0.437607, -0.402411, -0.035196, 0.000000, 0.500122,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_12BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.168664, -0.000000, 1.684846, 0.000000, -0.915688,
    1.168664, -0.188015, -0.652816, 0.000000, 0.347458,
    1.168664, 2.149647, -0.000000, 0.000000, -1.148145,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_16BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.262700, 0.678000, 0.059300, 0.000000, 0.000000,
    -0.139630, -0.360370, 0.500000, 0.000000, 0.500008,
    0.500000, -0.459786, -0.040214, 0.000000, 0.500008,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_16BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -0.000000, 1.474600, 0.000000, -0.737311,
    1.000000, -0.164553, -0.571353, 0.000000, 0.367959,
    1.000000, 1.881400, -0.000000, 0.000000, -0.940714,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_16BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.224735, 0.580017, 0.050730, 0.000000, 0.062501,
    -0.122178, -0.315329, 0.437507, 0.000000, 0.500008,
    0.437507, -0.402319, -0.035188, 0.000000, 0.500008,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const BT2020_16BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.168932, 0.000000, 1.685231, 0.000000, -0.915688,
    1.168932, -0.188058, -0.652965, 0.000000, 0.347458,
    1.168932, 2.150139, -0.000000, 0.000000, -1.148145,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const FCC_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.300000, 0.590000, 0.110000, 0.000000, 0.000000,
    -0.168539, -0.331461, 0.500000, 0.000000, 0.501961,
    0.500000, -0.421429, -0.078571, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const FCC_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, 0.000000, 1.400000, 0.000000, -0.702745,
    1.000000, -0.331864, -0.711864, 0.000000, 0.523911,
    1.000000, 1.780000, 0.000000, 0.000000, -0.893490,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const FCC_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.257647, 0.506706, 0.094471, 0.000000, 0.062745,
    -0.148050, -0.291165, 0.439216, 0.000000, 0.501961,
    0.439216, -0.370196, -0.069020, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const FCC_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.164384, -0.000000, 1.593750, 0.000000, -0.873059,
    1.164384, -0.377792, -0.810381, 0.000000, 0.523357,
    1.164384, 2.026339, 0.000000, 0.000000, -1.090202,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const SMPTE240_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.212000, 0.701000, 0.087000, 0.000000, 0.000000,
    -0.116101, -0.383899, 0.500000, 0.000000, 0.501961,
    0.500000, -0.444797, -0.055203, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const SMPTE240_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, 0.000000, 1.576000, 0.000000, -0.791090,
    1.000000, -0.226622, -0.476622, 0.000000, 0.353001,
    1.000000, 1.826000, 0.000000, 0.000000, -0.916580,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const SMPTE240_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.182071, 0.602035, 0.074718, 0.000000, 0.062745,
    -0.101987, -0.337229, 0.439216, 0.000000, 0.501961,
    0.439216, -0.390724, -0.048492, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const SMPTE240_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.164384, -0.000000, 1.794107, 0.000000, -0.973631,
    1.164384, -0.257985, -0.542583, 0.000000, 0.328794,
    1.164384, 2.078705, 0.000000, 0.000000, -1.116488,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YDZDX_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.000000, 1.000000, 0.000000, 0.000000, 0.000000,
    0.000000, -0.500000, 0.493283, 0.000000, 0.501961,
    0.500000, -0.495951, 0.000000, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YDZDX_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    0.991902, -0.000000, 2.000000, 0.000000, -1.003922,
    1.000000, 0.000000, 0.000000, 0.000000, 0.000000,
    1.013617, 2.027234, 0.000000, 0.000000, -1.017592,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YDZDX_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.000000, 0.858824, 0.000000, 0.000000, 0.062745,
    0.000000, -0.439216, 0.433315, 0.000000, 0.501961,
    0.439216, -0.435659, 0.000000, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YDZDX_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.154954, -0.000000, 2.276786, 0.000000, -1.215325,
    1.164384, 0.000000, 0.000000, 0.000000, -0.073059,
    1.180239, 2.307788, 0.000000, 0.000000, -1.232474,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const GBR_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.000000, 1.000000, 0.000000, 0.000000, 0.000000,
    0.000000, 0.000000, 1.000000, 0.000000, 0.000000,
    1.000000, 0.000000, 0.000000, 0.000000, 0.000000,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const GBR_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    0.000000, 0.000000, 1.000000, 0.000000, 0.000000,
    1.000000, 0.000000, 0.000000, 0.000000, 0.000000,
    0.000000, 1.000000, 0.000000, 0.000000, -0.000000,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const GBR_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.000000, 0.858824, 0.000000, 0.000000, 0.062745,
    0.000000, 0.000000, 0.858824, 0.000000, 0.062745,
    0.858824, 0.000000, 0.000000, 0.000000, 0.062745,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const GBR_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    0.000000, 0.000000, 1.164384, 0.000000, -0.073059,
    1.164384, 0.000000, 0.000000, 0.000000, -0.073059,
    0.000000, 1.164384, 0.000000, 0.000000, -0.073059,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_8BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.250000, 0.500000, 0.250000, 0.000000, 0.000000,
    -0.250000, 0.500000, -0.250000, 0.000000, 0.501961,
    0.500000, 0.000000, -0.500000, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_8BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -1.000000, 1.000000, 0.000000, 0.000000,
    1.000000, 1.000000, 0.000000, 0.000000, -0.501961,
    1.000000, -1.000000, -1.000000, 0.000000, 1.003922,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_8BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.214706, 0.429412, 0.214706, 0.000000, 0.062745,
    -0.214706, 0.429412, -0.214706, 0.000000, 0.501961,
    0.429412, 0.000000, -0.429412, 0.000000, 0.501961,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_8BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.164384, -1.164384, 1.164384, 0.000000, -0.073059,
    1.164384, 1.164384, 0.000000, 0.000000, -0.657534,
    1.164384, -1.164384, -1.164384, 0.000000, 1.095891,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_10BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.250000, 0.500000, 0.250000, 0.000000, 0.000000,
    -0.250000, 0.500000, -0.250000, 0.000000, 0.500489,
    0.500000, 0.000000, -0.500000, 0.000000, 0.500489,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_10BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -1.000000, 1.000000, 0.000000, 0.000000,
    1.000000, 1.000000, 0.000000, 0.000000, -0.500489,
    1.000000, -1.000000, -1.000000, 0.000000, 1.000978,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_10BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.214076, 0.428153, 0.214076, 0.000000, 0.062561,
    -0.214076, 0.428153, -0.214076, 0.000000, 0.500489,
    0.428153, 0.000000, -0.428153, 0.000000, 0.500489,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_10BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.167808, -1.167808, 1.167808, 0.000000, -0.073059,
    1.167808, 1.167808, 0.000000, 0.000000, -0.657534,
    1.167808, -1.167808, -1.167808, 0.000000, 1.095890,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_12BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.250000, 0.500000, 0.250000, 0.000000, 0.000000,
    -0.250000, 0.500000, -0.250000, 0.000000, 0.500122,
    0.500000, 0.000000, -0.500000, 0.000000, 0.500122,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_12BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -1.000000, 1.000000, 0.000000, 0.000000,
    1.000000, 1.000000, 0.000000, 0.000000, -0.500122,
    1.000000, -1.000000, -1.000000, 0.000000, 1.000244,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_12BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.213919, 0.427839, 0.213919, 0.000000, 0.062515,
    -0.213919, 0.427839, -0.213919, 0.000000, 0.500122,
    0.427839, 0.000000, -0.427839, 0.000000, 0.500122,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_12BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.168664, -1.168664, 1.168664, 0.000000, -0.073059,
    1.168664, 1.168664, 0.000000, 0.000000, -0.657534,
    1.168664, -1.168664, -1.168664, 0.000000, 1.095891,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_16BIT_FULL_RGB_TO_YUV: [f32; 20] = [
    0.250000, 0.500000, 0.250000, 0.000000, 0.000000,
    -0.250000, 0.500000, -0.250000, 0.000000, 0.500008,
    0.500000, 0.000000, -0.500000, 0.000000, 0.500008,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_16BIT_FULL_YUV_TO_RGB: [f32; 20] = [
    1.000000, -1.000000, 1.000000, 0.000000, 0.000000,
    1.000000, 1.000000, 0.000000, 0.000000, -0.500008,
    1.000000, -1.000000, -1.000000, 0.000000, 1.000015,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_16BIT_LIMITED_RGB_TO_YUV: [f32; 20] = [
    0.213870, 0.427741, 0.213870, 0.000000, 0.062501,
    -0.213870, 0.427741, -0.213870, 0.000000, 0.500008,
    0.427741, 0.000000, -0.427741, 0.000000, 0.500008,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];
const YCGCO_16BIT_LIMITED_YUV_TO_RGB: [f32; 20] = [
    1.168932, -1.168932, 1.168932, 0.000000, -0.073059,
    1.168932, 1.168932, 0.000000, 0.000000, -0.657534,
    1.168932, -1.168932, -1.168932, 0.000000, 1.095890,
    0.000000, 0.000000, 0.000000, 1.000000, 0.000000,
];

fn get_precomputed_matrices(cs: i32) -> Option<(&'static [f32; 20], &'static [f32; 20])> {
    match cs {
        YUV_CS_REC601_8BIT_FULL => Some((&REC601_8BIT_FULL_RGB_TO_YUV, &REC601_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_REC601_8BIT_LIMITED => Some((&REC601_8BIT_LIMITED_RGB_TO_YUV, &REC601_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_REC709_8BIT_FULL => Some((&REC709_8BIT_FULL_RGB_TO_YUV, &REC709_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_REC709_8BIT_LIMITED => Some((&REC709_8BIT_LIMITED_RGB_TO_YUV, &REC709_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_BT2020_8BIT_FULL => Some((&BT2020_8BIT_FULL_RGB_TO_YUV, &BT2020_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_BT2020_8BIT_LIMITED => Some((&BT2020_8BIT_LIMITED_RGB_TO_YUV, &BT2020_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_BT2020_10BIT_FULL => Some((&BT2020_10BIT_FULL_RGB_TO_YUV, &BT2020_10BIT_FULL_YUV_TO_RGB)),
        YUV_CS_BT2020_10BIT_LIMITED => Some((&BT2020_10BIT_LIMITED_RGB_TO_YUV, &BT2020_10BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_BT2020_12BIT_FULL => Some((&BT2020_12BIT_FULL_RGB_TO_YUV, &BT2020_12BIT_FULL_YUV_TO_RGB)),
        YUV_CS_BT2020_12BIT_LIMITED => Some((&BT2020_12BIT_LIMITED_RGB_TO_YUV, &BT2020_12BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_BT2020_16BIT_FULL => Some((&BT2020_16BIT_FULL_RGB_TO_YUV, &BT2020_16BIT_FULL_YUV_TO_RGB)),
        YUV_CS_BT2020_16BIT_LIMITED => Some((&BT2020_16BIT_LIMITED_RGB_TO_YUV, &BT2020_16BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_FCC_8BIT_FULL => Some((&FCC_8BIT_FULL_RGB_TO_YUV, &FCC_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_FCC_8BIT_LIMITED => Some((&FCC_8BIT_LIMITED_RGB_TO_YUV, &FCC_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_SMPTE240_8BIT_FULL => Some((&SMPTE240_8BIT_FULL_RGB_TO_YUV, &SMPTE240_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_SMPTE240_8BIT_LIMITED => Some((&SMPTE240_8BIT_LIMITED_RGB_TO_YUV, &SMPTE240_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_YDZDX_8BIT_FULL => Some((&YDZDX_8BIT_FULL_RGB_TO_YUV, &YDZDX_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_YDZDX_8BIT_LIMITED => Some((&YDZDX_8BIT_LIMITED_RGB_TO_YUV, &YDZDX_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_GBR_8BIT_FULL => Some((&GBR_8BIT_FULL_RGB_TO_YUV, &GBR_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_GBR_8BIT_LIMITED => Some((&GBR_8BIT_LIMITED_RGB_TO_YUV, &GBR_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_YCGCO_8BIT_FULL => Some((&YCGCO_8BIT_FULL_RGB_TO_YUV, &YCGCO_8BIT_FULL_YUV_TO_RGB)),
        YUV_CS_YCGCO_8BIT_LIMITED => Some((&YCGCO_8BIT_LIMITED_RGB_TO_YUV, &YCGCO_8BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_YCGCO_10BIT_FULL => Some((&YCGCO_10BIT_FULL_RGB_TO_YUV, &YCGCO_10BIT_FULL_YUV_TO_RGB)),
        YUV_CS_YCGCO_10BIT_LIMITED => Some((&YCGCO_10BIT_LIMITED_RGB_TO_YUV, &YCGCO_10BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_YCGCO_12BIT_FULL => Some((&YCGCO_12BIT_FULL_RGB_TO_YUV, &YCGCO_12BIT_FULL_YUV_TO_RGB)),
        YUV_CS_YCGCO_12BIT_LIMITED => Some((&YCGCO_12BIT_LIMITED_RGB_TO_YUV, &YCGCO_12BIT_LIMITED_YUV_TO_RGB)),
        YUV_CS_YCGCO_16BIT_FULL => Some((&YCGCO_16BIT_FULL_RGB_TO_YUV, &YCGCO_16BIT_FULL_YUV_TO_RGB)),
        YUV_CS_YCGCO_16BIT_LIMITED => Some((&YCGCO_16BIT_LIMITED_RGB_TO_YUV, &YCGCO_16BIT_LIMITED_YUV_TO_RGB)),
        _ => None,
    }
}

#[inline]
pub fn get_color_space_info(cs: i32) -> Option<YuvColorSpaceInfo> {
    if cs < 0 || cs >= YUV_CS_IDENTITY {
        return None;
    }
    Some(COLOR_SPACE_INFO[cs as usize])
}

#[inline]
fn colormatrix_to_m44(src: &[f32; 20]) -> M44 {
    M44::from_rows_scalar(
        src[0], src[1], src[2], src[4],
        src[5], src[6], src[7], src[9],
        src[10], src[11], src[12], src[14],
        0.0, 0.0, 0.0, 1.0,
    )
}

#[inline]
fn m44_to_colormatrix(src: &M44, dst: &mut [f32; 20]) {
    dst[0] = src.rc(0, 0);
    dst[1] = src.rc(0, 1);
    dst[2] = src.rc(0, 2);
    dst[3] = 0.0;
    dst[4] = src.rc(0, 3);

    dst[5] = src.rc(1, 0);
    dst[6] = src.rc(1, 1);
    dst[7] = src.rc(1, 2);
    dst[8] = 0.0;
    dst[9] = src.rc(1, 3);

    dst[10] = src.rc(2, 0);
    dst[11] = src.rc(2, 1);
    dst[12] = src.rc(2, 2);
    dst[13] = 0.0;
    dst[14] = src.rc(2, 3);

    dst[15] = 0.0;
    dst[16] = 0.0;
    dst[17] = 0.0;
    dst[18] = 1.0;
    dst[19] = 0.0;
}

#[inline]
fn scale3(slice: &mut [f32], s: f32) {
    for v in slice.iter_mut() {
        *v *= s;
    }
}

fn make_rgb_to_yuv_matrix_ycbcr(
    mx: &mut [f32; 20],
    mc: MatrixCoefficients,
    bits: i32,
    range: YuvRange,
) {
    debug_assert!(bits >= 8);
    let (kr, kb) = match mc {
        MatrixCoefficients::Rec601 => (0.2990_f32, 0.1140_f32),
        MatrixCoefficients::Rec709 => (0.2126_f32, 0.0722_f32),
        MatrixCoefficients::BT2020 => (0.2627_f32, 0.0593_f32),
        MatrixCoefficients::FCC => (0.3000_f32, 0.1100_f32),
        MatrixCoefficients::SMPTE240 => (0.2120_f32, 0.0870_f32),
        _ => unreachable!(),
    };

    let kg = 1.0_f32 - kr - kb;
    let cr = 0.5_f32 / (1.0_f32 - kb);
    let cb = 0.5_f32 / (1.0_f32 - kr);

    let shift = bits - 8;
    let denom = ((1i32 << bits) - 1) as f32;
    let mut scale_y = 1.0_f32;
    let mut add_y = 0.0_f32;
    let mut scale_uv = 1.0_f32;
    let add_uv = ((128i32 << shift) as f32) / denom;

    if range == YuvRange::Limited {
        scale_y = ((219i32 << shift) as f32) / denom;
        add_y = ((16i32 << shift) as f32) / denom;
        scale_uv = ((224i32 << shift) as f32) / denom;
    }

    *mx = [
        kr, kg, kb, 0.0, add_y,
        -kr, -kg, 1.0 - kb, 0.0, add_uv,
        1.0 - kr, -kg, -kb, 0.0, add_uv,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    scale3(&mut mx[0..3], scale_y);
    scale3(&mut mx[5..8], cr * scale_uv);
    scale3(&mut mx[10..13], cb * scale_uv);
}

fn make_rgb_to_yuv_matrix_ydzdx(mx: &mut [f32; 20], bits: i32, range: YuvRange) {
    debug_assert!(bits >= 8);
    let shift = bits - 8;
    let denom = ((1i32 << bits) - 1) as f32;
    let mut scale_y = 1.0_f32;
    let mut add_y = 0.0_f32;
    let mut scale_uv = 1.0_f32;
    let add_uv = ((128i32 << shift) as f32) / denom;

    if range == YuvRange::Limited {
        scale_y = ((219i32 << shift) as f32) / denom;
        add_y = ((16i32 << shift) as f32) / denom;
        scale_uv = ((224i32 << shift) as f32) / denom;
    }

    *mx = [
        0.0_f32, 1.0_f32, 0.0_f32, 0.0_f32, add_y,
        0.0_f32, -0.5_f32, 0.986566_f32 / 2.0_f32, 0.0_f32, add_uv,
        0.5_f32, -0.991902_f32 / 2.0_f32, 0.0_f32, 0.0_f32, add_uv,
        0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32, 0.0_f32,
    ];
    scale3(&mut mx[0..3], scale_y);
    scale3(&mut mx[5..8], scale_uv);
    scale3(&mut mx[10..13], scale_uv);
}

fn make_rgb_to_yuv_matrix_gbr(mx: &mut [f32; 20], bits: i32, range: YuvRange) {
    debug_assert!(bits >= 8);
    let shift = bits - 8;
    let denom = ((1i32 << bits) - 1) as f32;
    let mut scale_y = 1.0_f32;
    let mut add_y = 0.0_f32;

    if range == YuvRange::Limited {
        scale_y = ((219i32 << shift) as f32) / denom;
        add_y = ((16i32 << shift) as f32) / denom;
    }

    *mx = [
        0.0_f32, 1.0_f32, 0.0_f32, 0.0_f32, add_y,
        0.0_f32, 0.0_f32, 1.0_f32, 0.0_f32, add_y,
        1.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, add_y,
        0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32, 0.0_f32,
    ];
    scale3(&mut mx[0..3], scale_y);
    scale3(&mut mx[5..8], scale_y);
    scale3(&mut mx[10..13], scale_y);
}

fn make_rgb_to_yuv_matrix_ycgco(mx: &mut [f32; 20], bits: i32, range: YuvRange) {
    debug_assert!(bits >= 8);
    let shift = bits - 8;
    let denom = ((1i32 << bits) - 1) as f32;
    let mut scale_y = 1.0_f32;
    let mut add_y = 0.0_f32;
    let chroma05 = ((1i32 << (bits - 1)) as f32) / denom;

    if range == YuvRange::Limited {
        scale_y = ((219i32 << shift) as f32) / denom;
        add_y = ((16i32 << shift) as f32) / denom;
    }

    *mx = [
        0.25_f32, 0.5_f32, 0.25_f32, 0.0_f32, add_y,
        -0.25_f32, 0.5_f32, -0.25_f32, 0.0_f32, chroma05,
        0.5_f32, 0.0_f32, -0.5_f32, 0.0_f32, chroma05,
        0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32, 0.0_f32,
    ];
    scale3(&mut mx[0..3], scale_y);
    scale3(&mut mx[5..8], scale_y);
    scale3(&mut mx[10..13], scale_y);
}

pub fn make_rgb_to_yuv_matrix(
    mx: &mut [f32; 20],
    mc: MatrixCoefficients,
    bits: i32,
    range: YuvRange,
) {
    match mc {
        MatrixCoefficients::Rec601
        | MatrixCoefficients::Rec709
        | MatrixCoefficients::BT2020
        | MatrixCoefficients::FCC
        | MatrixCoefficients::SMPTE240 => make_rgb_to_yuv_matrix_ycbcr(mx, mc, bits, range),
        MatrixCoefficients::YDZDX => make_rgb_to_yuv_matrix_ydzdx(mx, bits, range),
        MatrixCoefficients::GBR => make_rgb_to_yuv_matrix_gbr(mx, bits, range),
        MatrixCoefficients::YCgCo => make_rgb_to_yuv_matrix_ycgco(mx, bits, range),
    }
}

pub fn make_yuv_to_rgb_matrix(
    mx: &mut [f32; 20],
    mc: MatrixCoefficients,
    bits: i32,
    range: YuvRange,
) {
    let mut rgb2yuv = [0.0_f32; 20];
    make_rgb_to_yuv_matrix(&mut rgb2yuv, mc, bits, range);
    let m44 = colormatrix_to_m44(&rgb2yuv);
    let im44 = m44.invert().expect("YUV matrix must be invertible");
    m44_to_colormatrix(&im44, mx);
}

#[inline]
fn set_identity(m: &mut [f32; 20]) {
    m.fill(0.0);
    m[0] = 1.0;
    m[6] = 1.0;
    m[12] = 1.0;
    m[18] = 1.0;
}

/// Exact port of `SkColorMatrix_RGB2YUV`.
/// If `high_precision` is false, matches default Skia (`!SK_YUV_COLOR_SPACE_HIGH_PRECISION`).
pub fn color_matrix_rgb2yuv(cs: i32, m: &mut [f32; 20], high_precision: bool) {
    let Some(info) = get_color_space_info(cs) else {
        set_identity(m);
        return;
    };
    if !high_precision {
        if let Some((rgb_to_yuv, _)) = get_precomputed_matrices(cs) {
            *m = *rgb_to_yuv;
            return;
        }
    }
    make_rgb_to_yuv_matrix(m, info.matrix_coefficients, info.bit_depth, info.range);
}

/// Exact port of `SkColorMatrix_YUV2RGB`.
/// If `high_precision` is false, matches default Skia (`!SK_YUV_COLOR_SPACE_HIGH_PRECISION`).
pub fn color_matrix_yuv2rgb(cs: i32, m: &mut [f32; 20], high_precision: bool) {
    let Some(info) = get_color_space_info(cs) else {
        set_identity(m);
        return;
    };
    if !high_precision {
        if let Some((_, yuv_to_rgb)) = get_precomputed_matrices(cs) {
            *m = *yuv_to_rgb;
            return;
        }
    }
    make_yuv_to_rgb_matrix(m, info.matrix_coefficients, info.bit_depth, info.range);
}

/// Exact port of `SkYUVColorSpaceIsLimitedRange`.
#[inline]
pub fn yuv_color_space_is_limited_range(cs: i32) -> bool {
    matches!(
        get_color_space_info(cs),
        Some(YuvColorSpaceInfo {
            range: YuvRange::Limited,
            ..
        })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_matrix(m: &[f32; 20], rgb: [f32; 3]) -> [f32; 3] {
        [
            m[0] * rgb[0] + m[1] * rgb[1] + m[2] * rgb[2] + m[4],
            m[5] * rgb[0] + m[6] * rgb[1] + m[7] * rgb[2] + m[9],
            m[10] * rgb[0] + m[11] * rgb[1] + m[12] * rgb[2] + m[14],
        ]
    }

    #[test]
    fn identity_and_invalid_return_identity() {
        let mut m = [99.0_f32; 20];
        color_matrix_rgb2yuv(YUV_CS_IDENTITY, &mut m, false);
        assert_eq!(m[0], 1.0);
        assert_eq!(m[6], 1.0);
        assert_eq!(m[12], 1.0);
        assert_eq!(m[18], 1.0);
        assert_eq!(m[1], 0.0);

        color_matrix_yuv2rgb(-1, &mut m, false);
        assert_eq!(m[0], 1.0);
        assert_eq!(m[18], 1.0);
        assert!(!yuv_color_space_is_limited_range(YUV_CS_IDENTITY));
        assert!(!yuv_color_space_is_limited_range(-5));
    }

    #[test]
    fn all_color_spaces_roundtrip() {
        let test_rgb = [0.75_f32, 0.25_f32, 0.5_f32];
        for cs in 0..YUV_CS_IDENTITY {
            let mut r2y = [0.0_f32; 20];
            let mut y2r = [0.0_f32; 20];
            color_matrix_rgb2yuv(cs, &mut r2y, false);
            color_matrix_yuv2rgb(cs, &mut y2r, false);

            let yuv = apply_matrix(&r2y, test_rgb);
            let rgb = apply_matrix(&y2r, yuv);
            for i in 0..3 {
                assert!(
                    (rgb[i] - test_rgb[i]).abs() < 1e-4,
                    "cs {cs} channel {i}: got {}, expected {}",
                    rgb[i],
                    test_rgb[i]
                );
            }
            assert_eq!(yuv_color_space_is_limited_range(cs), (cs % 2) == 1);
        }
    }

    #[test]
    fn precomputed_tables_match_computed_within_6_decimals() {
        for cs in 0..YUV_CS_IDENTITY {
            if let Some((tab_r2y, tab_y2r)) = get_precomputed_matrices(cs) {
                let mut hi_r2y = [0.0_f32; 20];
                let mut hi_y2r = [0.0_f32; 20];
                color_matrix_rgb2yuv(cs, &mut hi_r2y, true);
                color_matrix_yuv2rgb(cs, &mut hi_y2r, true);
                for i in 0..20 {
                    assert!(
                        (tab_r2y[i] - hi_r2y[i]).abs() < 1e-5,
                        "r2y cs {cs}[{i}]: tab={} hi={}",
                        tab_r2y[i],
                        hi_r2y[i]
                    );
                    assert!(
                        (tab_y2r[i] - hi_y2r[i]).abs() < 1e-5,
                        "y2r cs {cs}[{i}]: tab={} hi={}",
                        tab_y2r[i],
                        hi_y2r[i]
                    );
                }
            }
        }
    }
}
