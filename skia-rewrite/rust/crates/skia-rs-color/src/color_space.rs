//! Safe Rust port of `src/core/SkColorSpace.cpp` and `src/core/SkColorSpacePriv.h`.

use skia_rs_math::checksum::hash32;

/// Matches `skcms_TransferFunction`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct TransferFunction {
    pub g: f32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl TransferFunction {
    pub const fn as_array(&self) -> [f32; 7] {
        [self.g, self.a, self.b, self.c, self.d, self.e, self.f]
    }

    pub const fn from_array(v: [f32; 7]) -> Self {
        Self {
            g: v[0],
            a: v[1],
            b: v[2],
            c: v[3],
            d: v[4],
            e: v[5],
            f: v[6],
        }
    }

    /// Exact bitwise equality (`memcmp == 0`).
    pub fn bit_eq(&self, other: &Self) -> bool {
        self.g.to_bits() == other.g.to_bits()
            && self.a.to_bits() == other.a.to_bits()
            && self.b.to_bits() == other.b.to_bits()
            && self.c.to_bits() == other.c.to_bits()
            && self.d.to_bits() == other.d.to_bits()
            && self.e.to_bits() == other.e.to_bits()
            && self.f.to_bits() == other.f.to_bits()
    }

    /// Exact match for `SkChecksum::Hash32(&fTransferFn, 7 * sizeof(float))`.
    pub fn hash32(&self) -> u32 {
        let mut bytes = [0u8; 28];
        for (i, v) in self.as_array().iter().enumerate() {
            bytes[i * 4..(i + 1) * 4].copy_from_slice(&v.to_ne_bytes());
        }
        hash32(&bytes, 0)
    }
}

/// Matches `skcms_Matrix3x3` (row-major `vals[row][col]`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Matrix3x3 {
    pub vals: [[f32; 3]; 3],
}

impl Matrix3x3 {
    pub fn is_finite(&self) -> bool {
        for row in &self.vals {
            for &v in row {
                if !v.is_finite() {
                    return false;
                }
            }
        }
        true
    }

    /// Exact bitwise equality (`memcmp == 0`).
    pub fn bit_eq(&self, other: &Self) -> bool {
        for r in 0..3 {
            for c in 0..3 {
                if self.vals[r][c].to_bits() != other.vals[r][c].to_bits() {
                    return false;
                }
            }
        }
        true
    }

    /// Exact match for `SkChecksum::Hash32(&fToXYZD50, 9 * sizeof(float))`.
    pub fn hash32(&self) -> u32 {
        let mut bytes = [0u8; 36];
        for r in 0..3 {
            for c in 0..3 {
                let idx = (r * 3 + c) * 4;
                bytes[idx..idx + 4].copy_from_slice(&self.vals[r][c].to_ne_bytes());
            }
        }
        hash32(&bytes, 0)
    }

    /// Exact match for `skcms_Matrix3x3_concat(A, B)`.
    pub fn concat(&self, b: &Self) -> Self {
        let mut m = [[0.0_f32; 3]; 3];
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] = self.vals[r][0] * b.vals[0][c]
                    + self.vals[r][1] * b.vals[1][c]
                    + self.vals[r][2] * b.vals[2][c];
            }
        }
        Self { vals: m }
    }

    /// Exact match for `skcms_Matrix3x3_invert(src, dst)`.
    pub fn invert(&self) -> Option<Self> {
        let a00 = self.vals[0][0] as f64;
        let a01 = self.vals[1][0] as f64;
        let a02 = self.vals[2][0] as f64;
        let a10 = self.vals[0][1] as f64;
        let a11 = self.vals[1][1] as f64;
        let a12 = self.vals[2][1] as f64;
        let a20 = self.vals[0][2] as f64;
        let a21 = self.vals[1][2] as f64;
        let a22 = self.vals[2][2] as f64;

        let mut b0 = a00 * a11 - a01 * a10;
        let mut b1 = a00 * a12 - a02 * a10;
        let mut b2 = a01 * a12 - a02 * a11;
        let mut b3 = a20;
        let mut b4 = a21;
        let mut b5 = a22;

        let determinant = b0 * b5 - b1 * b4 + b2 * b3;
        if determinant == 0.0 {
            return None;
        }

        let invdet = 1.0 / determinant;
        if invdet > (f32::MAX as f64)
            || invdet < -(f32::MAX as f64)
            || !(invdet as f32).is_finite()
        {
            return None;
        }

        b0 *= invdet;
        b1 *= invdet;
        b2 *= invdet;
        b3 *= invdet;
        b4 *= invdet;
        b5 *= invdet;

        let dst = Self {
            vals: [
                [
                    (a11 * b5 - a12 * b4) as f32,
                    (a12 * b3 - a10 * b5) as f32,
                    (a10 * b4 - a11 * b3) as f32,
                ],
                [
                    (a02 * b4 - a01 * b5) as f32,
                    (a00 * b5 - a02 * b3) as f32,
                    (a01 * b3 - a00 * b4) as f32,
                ],
                [b2 as f32, (-b1) as f32, b0 as f32],
            ],
        };

        if dst.is_finite() {
            Some(dst)
        } else {
            None
        }
    }

    #[inline]
    fn mv_mul(&self, v: [f32; 3]) -> [f32; 3] {
        [
            self.vals[0][0] * v[0] + self.vals[0][1] * v[1] + self.vals[0][2] * v[2],
            self.vals[1][0] * v[0] + self.vals[1][1] * v[1] + self.vals[1][2] * v[2],
            self.vals[2][0] * v[0] + self.vals[2][1] * v[1] + self.vals[2][2] * v[2],
        ]
    }
}

/// Matches `SkColorSpacePrimaries`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ColorSpacePrimaries {
    pub rx: f32,
    pub ry: f32,
    pub gx: f32,
    pub gy: f32,
    pub bx: f32,
    pub by: f32,
    pub wx: f32,
    pub wy: f32,
}

#[inline]
fn is_zero_to_one(v: f32) -> bool {
    v >= 0.0 && v <= 1.0
}

#[inline]
fn is_valid_white_point(wx: f32, wy: f32) -> bool {
    is_zero_to_one(wx) && is_zero_to_one(wy) && wy != 0.0
}

/// Exact match for `skcms_AdaptToXYZD50`.
pub fn adapt_to_xyzd50(wx: f32, wy: f32) -> Option<Matrix3x3> {
    if !is_valid_white_point(wx, wy) {
        return None;
    }

    let w_xyz = [wx / wy, 1.0_f32, (1.0_f32 - wx - wy) / wy];
    let w_xyz_d50 = [0.96422_f32, 1.0_f32, 0.82521_f32];

    let xyz_to_lms = Matrix3x3 {
        vals: [
            [0.8951_f32, 0.2664_f32, -0.1614_f32],
            [-0.7502_f32, 1.7135_f32, 0.0367_f32],
            [0.0389_f32, -0.0685_f32, 1.0296_f32],
        ],
    };
    let lms_to_xyz = Matrix3x3 {
        vals: [
            [0.9869929_f32, -0.1470543_f32, 0.1599627_f32],
            [0.4323053_f32, 0.5183603_f32, 0.0492912_f32],
            [-0.0085287_f32, 0.0400428_f32, 0.9684867_f32],
        ],
    };

    let src_cone = xyz_to_lms.mv_mul(w_xyz);
    let dst_cone = xyz_to_lms.mv_mul(w_xyz_d50);

    if src_cone[0] == 0.0 || src_cone[1] == 0.0 || src_cone[2] == 0.0 {
        return None;
    }

    let diag = Matrix3x3 {
        vals: [
            [dst_cone[0] / src_cone[0], 0.0, 0.0],
            [0.0, dst_cone[1] / src_cone[1], 0.0],
            [0.0, 0.0, dst_cone[2] / src_cone[2]],
        ],
    };
    let m = lms_to_xyz.concat(&diag.concat(&xyz_to_lms));
    if m.is_finite() {
        Some(m)
    } else {
        None
    }
}

impl ColorSpacePrimaries {
    /// Exact match for `SkColorSpacePrimaries::toXYZD50` (`skcms_PrimariesToXYZD50`).
    pub fn to_xyzd50(&self) -> Option<Matrix3x3> {
        if !is_zero_to_one(self.rx)
            || !is_zero_to_one(self.ry)
            || !is_zero_to_one(self.gx)
            || !is_zero_to_one(self.gy)
            || !is_zero_to_one(self.bx)
            || !is_zero_to_one(self.by)
            || !is_valid_white_point(self.wx, self.wy)
        {
            return None;
        }

        let primaries = Matrix3x3 {
            vals: [
                [self.rx, self.gx, self.bx],
                [self.ry, self.gy, self.by],
                [
                    1.0_f32 - self.rx - self.ry,
                    1.0_f32 - self.gx - self.gy,
                    1.0_f32 - self.bx - self.by,
                ],
            ],
        };
        let primaries_inv = primaries.invert()?;

        let w_xyz = [
            self.wx / self.wy,
            1.0_f32,
            (1.0_f32 - self.wx - self.wy) / self.wy,
        ];
        let xyz = primaries_inv.mv_mul(w_xyz);

        let diag = Matrix3x3 {
            vals: [
                [xyz[0], 0.0, 0.0],
                [0.0, xyz[1], 0.0],
                [0.0, 0.0, xyz[2]],
            ],
        };
        let to_xyz = primaries.concat(&diag);
        let dx_to_d50 = adapt_to_xyzd50(self.wx, self.wy)?;
        let m = dx_to_d50.concat(&to_xyz);
        if m.is_finite() {
            Some(m)
        } else {
            None
        }
    }
}

// --- Named Gamuts (exact match with SkNamedGamut in SkColorSpace.h) ---

#[inline]
const fn fixed_to_float(x: i32) -> f32 {
    (x as f32) * 1.52587890625e-5_f32
}

pub const GAMUT_SRGB: Matrix3x3 = Matrix3x3 {
    vals: [
        [
            fixed_to_float(0x6FA2),
            fixed_to_float(0x6299),
            fixed_to_float(0x24A0),
        ],
        [
            fixed_to_float(0x38F5),
            fixed_to_float(0xB785),
            fixed_to_float(0x0F84),
        ],
        [
            fixed_to_float(0x0390),
            fixed_to_float(0x18DA),
            fixed_to_float(0xB6CF),
        ],
    ],
};

pub const GAMUT_ADOBE_RGB: Matrix3x3 = Matrix3x3 {
    vals: [
        [
            fixed_to_float(0x9c18),
            fixed_to_float(0x348d),
            fixed_to_float(0x2631),
        ],
        [
            fixed_to_float(0x4fa5),
            fixed_to_float(0xa02c),
            fixed_to_float(0x102f),
        ],
        [
            fixed_to_float(0x04fc),
            fixed_to_float(0x0f95),
            fixed_to_float(0xbe9c),
        ],
    ],
};

pub const GAMUT_DISPLAY_P3: Matrix3x3 = Matrix3x3 {
    vals: [
        [0.515102_f32, 0.291965_f32, 0.157153_f32],
        [0.241182_f32, 0.692236_f32, 0.0665819_f32],
        [-0.00104941_f32, 0.0418818_f32, 0.784378_f32],
    ],
};

pub const GAMUT_REC2020: Matrix3x3 = Matrix3x3 {
    vals: [
        [0.673459_f32, 0.165661_f32, 0.125100_f32],
        [0.279033_f32, 0.675338_f32, 0.0456288_f32],
        [-0.00193139_f32, 0.0299794_f32, 0.797162_f32],
    ],
};

pub const GAMUT_XYZ: Matrix3x3 = Matrix3x3 {
    vals: [
        [1.0_f32, 0.0_f32, 0.0_f32],
        [0.0_f32, 1.0_f32, 0.0_f32],
        [0.0_f32, 0.0_f32, 1.0_f32],
    ],
};

// --- Named Transfer Functions (exact match with SkNamedTransferFn in SkColorSpace.h) ---

pub const TF_SRGB: TransferFunction = TransferFunction {
    g: 2.4_f32,
    a: (1.0_f64 / 1.055_f64) as f32,
    b: (0.055_f64 / 1.055_f64) as f32,
    c: (1.0_f64 / 12.92_f64) as f32,
    d: 0.04045_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_2DOT2: TransferFunction = TransferFunction {
    g: 2.2_f32,
    a: 1.0_f32,
    b: 0.0_f32,
    c: 0.0_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_LINEAR: TransferFunction = TransferFunction {
    g: 1.0_f32,
    a: 1.0_f32,
    b: 0.0_f32,
    c: 0.0_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_REC709: TransferFunction = TransferFunction {
    g: 2.4_f32,
    a: 1.0_f32,
    b: 0.0_f32,
    c: 0.0_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_REC470_SYSTEM_M: TransferFunction = TF_2DOT2;

pub const TF_REC470_SYSTEM_BG: TransferFunction = TransferFunction {
    g: 2.8_f32,
    a: 1.0_f32,
    b: 0.0_f32,
    c: 0.0_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_SMPTE_ST_240: TransferFunction = TransferFunction {
    g: 2.222222222222_f32,
    a: 0.899626676224_f32,
    b: 0.100373323776_f32,
    c: 0.25_f32,
    d: 0.091286342118_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_PQ: TransferFunction = TransferFunction {
    g: -5.0_f32,
    a: 203.0_f32,
    b: 0.0_f32,
    c: 0.0_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_SMPTE_ST_428_1: TransferFunction = TransferFunction {
    g: 2.6_f32,
    a: 1.034080527699_f32,
    b: 0.0_f32,
    c: 0.0_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

pub const TF_HLG: TransferFunction = TransferFunction {
    g: -6.0_f32,
    a: 203.0_f32,
    b: 1000.0_f32,
    c: 1.2_f32,
    d: 0.0_f32,
    e: 0.0_f32,
    f: 0.0_f32,
};

// --- Named Primaries (Rec. ITU-T H.273 Table 2) ---

pub const PRIMARIES_REC709: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.64,
    ry: 0.33,
    gx: 0.3,
    gy: 0.6,
    bx: 0.15,
    by: 0.06,
    wx: 0.3127,
    wy: 0.329,
};

pub const PRIMARIES_REC470_SYSTEM_M: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.67,
    ry: 0.33,
    gx: 0.21,
    gy: 0.71,
    bx: 0.14,
    by: 0.08,
    wx: 0.31,
    wy: 0.316,
};

pub const PRIMARIES_REC470_SYSTEM_BG: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.64,
    ry: 0.33,
    gx: 0.29,
    gy: 0.60,
    bx: 0.15,
    by: 0.06,
    wx: 0.3127,
    wy: 0.3290,
};

pub const PRIMARIES_REC601: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.630,
    ry: 0.340,
    gx: 0.310,
    gy: 0.595,
    bx: 0.155,
    by: 0.070,
    wx: 0.3127,
    wy: 0.3290,
};

pub const PRIMARIES_GENERIC_FILM: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.681,
    ry: 0.319,
    gx: 0.243,
    gy: 0.692,
    bx: 0.145,
    by: 0.049,
    wx: 0.310,
    wy: 0.316,
};

pub const PRIMARIES_REC2020: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.708,
    ry: 0.292,
    gx: 0.170,
    gy: 0.797,
    bx: 0.131,
    by: 0.046,
    wx: 0.3127,
    wy: 0.3290,
};

pub const PRIMARIES_SMPTE_ST_428_1: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 1.0,
    ry: 0.0,
    gx: 0.0,
    gy: 1.0,
    bx: 0.0,
    by: 0.0,
    wx: 1.0 / 3.0,
    wy: 1.0 / 3.0,
};

pub const PRIMARIES_SMPTE_RP_431_2: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.680,
    ry: 0.320,
    gx: 0.265,
    gy: 0.690,
    bx: 0.150,
    by: 0.060,
    wx: 0.314,
    wy: 0.351,
};

pub const PRIMARIES_SMPTE_EG_432_1: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.680,
    ry: 0.320,
    gx: 0.265,
    gy: 0.690,
    bx: 0.150,
    by: 0.060,
    wx: 0.3127,
    wy: 0.3290,
};

pub const PRIMARIES_ITU_T_H273_VALUE22: ColorSpacePrimaries = ColorSpacePrimaries {
    rx: 0.630,
    ry: 0.340,
    gx: 0.295,
    gy: 0.605,
    bx: 0.155,
    by: 0.077,
    wx: 0.3127,
    wy: 0.3290,
};

pub const CICP_ID_APPLICATION_DEFINED: u8 = 2;

struct PrimariesCicpEntry {
    cicp_id: u8,
    primaries: ColorSpacePrimaries,
    to_xyzd50: Option<Matrix3x3>,
}

const PRIMARIES_CICP_TABLE: [PrimariesCicpEntry; 11] = [
    PrimariesCicpEntry {
        cicp_id: 1,
        primaries: PRIMARIES_REC709,
        to_xyzd50: Some(GAMUT_SRGB),
    },
    PrimariesCicpEntry {
        cicp_id: 4,
        primaries: PRIMARIES_REC470_SYSTEM_M,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 5,
        primaries: PRIMARIES_REC470_SYSTEM_BG,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 6,
        primaries: PRIMARIES_REC601,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 7,
        primaries: PRIMARIES_REC601,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 8,
        primaries: PRIMARIES_GENERIC_FILM,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 9,
        primaries: PRIMARIES_REC2020,
        to_xyzd50: Some(GAMUT_REC2020),
    },
    PrimariesCicpEntry {
        cicp_id: 10,
        primaries: PRIMARIES_SMPTE_ST_428_1,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 11,
        primaries: PRIMARIES_SMPTE_RP_431_2,
        to_xyzd50: None,
    },
    PrimariesCicpEntry {
        cicp_id: 12,
        primaries: PRIMARIES_SMPTE_EG_432_1,
        to_xyzd50: Some(GAMUT_DISPLAY_P3),
    },
    PrimariesCicpEntry {
        cicp_id: 22,
        primaries: PRIMARIES_ITU_T_H273_VALUE22,
        to_xyzd50: None,
    },
];

const TRANSFER_FN_CICP_TABLE: [(u8, TransferFunction); 13] = [
    (1, TF_REC709),
    (4, TF_REC470_SYSTEM_M),
    (5, TF_REC470_SYSTEM_BG),
    (6, TF_REC709),
    (7, TF_SMPTE_ST_240),
    (8, TF_LINEAR),
    (11, TF_REC709),
    (13, TF_SRGB),
    (14, TF_REC709),
    (15, TF_REC709),
    (16, TF_PQ),
    (17, TF_SMPTE_ST_428_1),
    (18, TF_HLG),
];

/// Exact port of `color_space_almost_equal`.
#[inline]
pub fn color_space_almost_equal(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01_f32
}

/// Exact port of `transfer_fn_almost_equal`.
#[inline]
pub fn transfer_fn_almost_equal(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.001_f32
}

/// Exact port of `xyz_almost_equal`.
pub fn xyz_almost_equal(a: &Matrix3x3, b: &Matrix3x3) -> bool {
    for r in 0..3 {
        for c in 0..3 {
            if !color_space_almost_equal(a.vals[r][c], b.vals[r][c]) {
                return false;
            }
        }
    }
    true
}

/// Exact port of `is_almost_srgb`.
pub fn is_almost_srgb(c: &TransferFunction) -> bool {
    transfer_fn_almost_equal(TF_SRGB.a, c.a)
        && transfer_fn_almost_equal(TF_SRGB.b, c.b)
        && transfer_fn_almost_equal(TF_SRGB.c, c.c)
        && transfer_fn_almost_equal(TF_SRGB.d, c.d)
        && transfer_fn_almost_equal(TF_SRGB.e, c.e)
        && transfer_fn_almost_equal(TF_SRGB.f, c.f)
        && transfer_fn_almost_equal(TF_SRGB.g, c.g)
}

/// Exact port of `is_almost_2dot2`.
pub fn is_almost_2dot2(c: &TransferFunction) -> bool {
    transfer_fn_almost_equal(1.0, c.a)
        && transfer_fn_almost_equal(0.0, c.b)
        && transfer_fn_almost_equal(0.0, c.e)
        && transfer_fn_almost_equal(2.2, c.g)
        && c.d <= 0.0
}

/// Exact port of `is_almost_linear`.
pub fn is_almost_linear(c: &TransferFunction) -> bool {
    let linear_exp = transfer_fn_almost_equal(1.0, c.a)
        && transfer_fn_almost_equal(0.0, c.b)
        && transfer_fn_almost_equal(0.0, c.e)
        && transfer_fn_almost_equal(1.0, c.g)
        && c.d <= 0.0;
    let linear_fn = transfer_fn_almost_equal(1.0, c.c)
        && transfer_fn_almost_equal(0.0, c.f)
        && c.d >= 1.0;
    linear_exp || linear_fn
}

/// Exact port of `SkNamedPrimaries::GetCicp`.
pub fn get_cicp_primaries(primaries: u8) -> Option<Matrix3x3> {
    for entry in &PRIMARIES_CICP_TABLE {
        if entry.cicp_id != primaries {
            continue;
        }
        if let Some(m) = entry.to_xyzd50 {
            return Some(m);
        }
        if let Some(m) = entry.primaries.to_xyzd50() {
            return Some(m);
        }
    }
    None
}

/// Exact port of `SkNamedPrimaries::GetCicpFromMatrix`.
pub fn get_cicp_from_matrix(m: &Matrix3x3) -> Option<u8> {
    for entry in &PRIMARIES_CICP_TABLE {
        if let Some(table_m) = entry.primaries.to_xyzd50() {
            if xyz_almost_equal(m, &table_m) {
                return Some(entry.cicp_id);
            }
        }
    }
    None
}

/// Exact port of `SkNamedTransferFn::GetCicp`.
pub fn get_cicp_transfer_fn(transfer_characteristics: u8) -> Option<TransferFunction> {
    for &(id, tf) in &TRANSFER_FN_CICP_TABLE {
        if id == transfer_characteristics {
            return Some(tf);
        }
    }
    None
}

/// Result of canonicalizing `(transfer_fn, to_xyz)` in `SkColorSpace::MakeRGB`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MakeRgbKind {
    Custom = 0,
    SrgbSingleton = 1,
    SrgbLinearSingleton = 2,
}

/// Snap near-sRGB, near-2.2, and near-linear transfer functions and detect singletons.
pub fn canonicalize_rgb(
    transfer_fn: &TransferFunction,
    to_xyz: &Matrix3x3,
) -> (MakeRgbKind, TransferFunction) {
    if is_almost_srgb(transfer_fn) {
        if xyz_almost_equal(to_xyz, &GAMUT_SRGB) {
            return (MakeRgbKind::SrgbSingleton, TF_SRGB);
        }
        return (MakeRgbKind::Custom, TF_SRGB);
    }
    if is_almost_2dot2(transfer_fn) {
        return (MakeRgbKind::Custom, TF_2DOT2);
    }
    if is_almost_linear(transfer_fn) {
        if xyz_almost_equal(to_xyz, &GAMUT_SRGB) {
            return (MakeRgbKind::SrgbLinearSingleton, TF_LINEAR);
        }
        return (MakeRgbKind::Custom, TF_LINEAR);
    }
    (MakeRgbKind::Custom, *transfer_fn)
}

/// Exact port of `SkColorSpace::makeColorSpin`'s matrix rotation.
pub fn color_spin_matrix(to_xyzd50: &Matrix3x3) -> Matrix3x3 {
    const SPIN: Matrix3x3 = Matrix3x3 {
        vals: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    };
    to_xyzd50.concat(&SPIN)
}

pub const SERIALIZED_COLOR_SPACE_BYTES: usize = 4 + 16 * 4;
const CURRENT_VERSION: u8 = 1;

/// Exact port of `SkColorSpace::writeToMemory`.
pub fn serialize_color_space(
    tf: &TransferFunction,
    to_xyzd50: &Matrix3x3,
    out: &mut [u8; SERIALIZED_COLOR_SPACE_BYTES],
) {
    out[0] = CURRENT_VERSION;
    out[1] = 0;
    out[2] = 0;
    out[3] = 0;

    let mut offset = 4;
    for v in tf.as_array() {
        out[offset..offset + 4].copy_from_slice(&v.to_ne_bytes());
        offset += 4;
    }
    for r in 0..3 {
        for c in 0..3 {
            out[offset..offset + 4].copy_from_slice(&to_xyzd50.vals[r][c].to_ne_bytes());
            offset += 4;
        }
    }
}

/// Exact port of `SkColorSpace::Deserialize` parsing step.
pub fn deserialize_color_space(data: &[u8]) -> Option<(TransferFunction, Matrix3x3)> {
    if data.len() < SERIALIZED_COLOR_SPACE_BYTES {
        return None;
    }
    if data[0] != CURRENT_VERSION {
        return None;
    }
    let read_f32 = |off: usize| -> f32 {
        f32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
    };

    let mut tf_arr = [0.0_f32; 7];
    for (i, slot) in tf_arr.iter_mut().enumerate() {
        *slot = read_f32(4 + i * 4);
    }
    let mut mat = [[0.0_f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            mat[r][c] = read_f32(32 + (r * 3 + c) * 4);
        }
    }
    Some((TransferFunction::from_array(tf_arr), Matrix3x3 { vals: mat }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_invert_and_concat() {
        let inv = GAMUT_SRGB.invert().unwrap();
        let prod = inv.concat(&GAMUT_SRGB);
        for r in 0..3 {
            for c in 0..3 {
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!((prod.vals[r][c] - expected).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn cicp_roundtrip() {
        let m = get_cicp_primaries(1).unwrap();
        assert!(m.bit_eq(&GAMUT_SRGB));
        assert_eq!(get_cicp_from_matrix(&GAMUT_SRGB), Some(1));
        assert_eq!(get_cicp_from_matrix(&GAMUT_DISPLAY_P3), Some(12));
        assert_eq!(get_cicp_from_matrix(&GAMUT_REC2020), Some(9));
        assert_eq!(get_cicp_primaries(2), None);
        assert_eq!(get_cicp_transfer_fn(13).unwrap(), TF_SRGB);
        assert_eq!(get_cicp_transfer_fn(8).unwrap(), TF_LINEAR);
        assert_eq!(get_cicp_transfer_fn(2), None);
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut buf = [0u8; SERIALIZED_COLOR_SPACE_BYTES];
        serialize_color_space(&TF_SRGB, &GAMUT_DISPLAY_P3, &mut buf);
        let (tf, m) = deserialize_color_space(&buf).unwrap();
        assert!(tf.bit_eq(&TF_SRGB));
        assert!(m.bit_eq(&GAMUT_DISPLAY_P3));

        // Bad version rejected
        buf[0] = 0;
        assert!(deserialize_color_space(&buf).is_none());
    }

    #[test]
    fn canonicalize_snapping() {
        let mut near_srgb = TF_SRGB;
        near_srgb.g += 0.0005;
        let (kind, tf) = canonicalize_rgb(&near_srgb, &GAMUT_SRGB);
        assert_eq!(kind, MakeRgbKind::SrgbSingleton);
        assert!(tf.bit_eq(&TF_SRGB));

        let (kind2, tf2) = canonicalize_rgb(&TF_LINEAR, &GAMUT_SRGB);
        assert_eq!(kind2, MakeRgbKind::SrgbLinearSingleton);
        assert!(tf2.bit_eq(&TF_LINEAR));
    }
}
