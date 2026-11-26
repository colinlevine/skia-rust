#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkAnalyticEdge.h` and `src/core/SkAnalyticEdge.cpp`.

use crate::edge::{EdgeType, EdgeWinding, MAX_COEFF_SHIFT};
use crate::fixed::{
    abs32, clz_u32, fdot6_div, fdot6_round, fdot6_to_fixed, fdot6_to_fixed_div2, fixed_mul,
    fixed_round_to_fixed, fixed_to_fdot6, float_to_fdot6, float_to_i32_c, left_shift_i32,
    left_shift_i64, pin_64_to_s32, sat_add_i32, SkFDot6, SkFixed, SK_FIXED1, SK_MAX_S32,
};
use skia_rs_geometry::Point;

pub const K_INVERSE_TABLE_SIZE: i32 = 1024;
pub const K_DEFAULT_ACCURACY: i32 = 2;

static INVERSE_TABLE: [i32; 1025] = [
    -4096, -4100, -4104, -4108, -4112, -4116, -4120, -4124, -4128, -4132, -4136,
    -4140, -4144, -4148, -4152, -4156, -4161, -4165, -4169, -4173, -4177, -4181,
    -4185, -4190, -4194, -4198, -4202, -4206, -4211, -4215, -4219, -4223, -4228,
    -4232, -4236, -4240, -4245, -4249, -4253, -4258, -4262, -4266, -4271, -4275,
    -4279, -4284, -4288, -4293, -4297, -4301, -4306, -4310, -4315, -4319, -4324,
    -4328, -4332, -4337, -4341, -4346, -4350, -4355, -4359, -4364, -4369, -4373,
    -4378, -4382, -4387, -4391, -4396, -4401, -4405, -4410, -4415, -4419, -4424,
    -4429, -4433, -4438, -4443, -4447, -4452, -4457, -4462, -4466, -4471, -4476,
    -4481, -4485, -4490, -4495, -4500, -4505, -4510, -4514, -4519, -4524, -4529,
    -4534, -4539, -4544, -4549, -4554, -4559, -4563, -4568, -4573, -4578, -4583,
    -4588, -4593, -4599, -4604, -4609, -4614, -4619, -4624, -4629, -4634, -4639,
    -4644, -4650, -4655, -4660, -4665, -4670, -4675, -4681, -4686, -4691, -4696,
    -4702, -4707, -4712, -4718, -4723, -4728, -4733, -4739, -4744, -4750, -4755,
    -4760, -4766, -4771, -4777, -4782, -4788, -4793, -4798, -4804, -4809, -4815,
    -4821, -4826, -4832, -4837, -4843, -4848, -4854, -4860, -4865, -4871, -4877,
    -4882, -4888, -4894, -4899, -4905, -4911, -4917, -4922, -4928, -4934, -4940,
    -4946, -4951, -4957, -4963, -4969, -4975, -4981, -4987, -4993, -4999, -5005,
    -5011, -5017, -5023, -5029, -5035, -5041, -5047, -5053, -5059, -5065, -5071,
    -5077, -5084, -5090, -5096, -5102, -5108, -5115, -5121, -5127, -5133, -5140,
    -5146, -5152, -5159, -5165, -5171, -5178, -5184, -5190, -5197, -5203, -5210,
    -5216, -5223, -5229, -5236, -5242, -5249, -5256, -5262, -5269, -5275, -5282,
    -5289, -5295, -5302, -5309, -5315, -5322, -5329, -5336, -5343, -5349, -5356,
    -5363, -5370, -5377, -5384, -5391, -5398, -5405, -5412, -5418, -5426, -5433,
    -5440, -5447, -5454, -5461, -5468, -5475, -5482, -5489, -5497, -5504, -5511,
    -5518, -5526, -5533, -5540, -5548, -5555, -5562, -5570, -5577, -5584, -5592,
    -5599, -5607, -5614, -5622, -5629, -5637, -5645, -5652, -5660, -5667, -5675,
    -5683, -5691, -5698, -5706, -5714, -5722, -5729, -5737, -5745, -5753, -5761,
    -5769, -5777, -5785, -5793, -5801, -5809, -5817, -5825, -5833, -5841, -5849,
    -5857, -5866, -5874, -5882, -5890, -5899, -5907, -5915, -5924, -5932, -5940,
    -5949, -5957, -5966, -5974, -5983, -5991, -6000, -6009, -6017, -6026, -6034,
    -6043, -6052, -6061, -6069, -6078, -6087, -6096, -6105, -6114, -6123, -6132,
    -6141, -6150, -6159, -6168, -6177, -6186, -6195, -6204, -6213, -6223, -6232,
    -6241, -6250, -6260, -6269, -6278, -6288, -6297, -6307, -6316, -6326, -6335,
    -6345, -6355, -6364, -6374, -6384, -6393, -6403, -6413, -6423, -6432, -6442,
    -6452, -6462, -6472, -6482, -6492, -6502, -6512, -6523, -6533, -6543, -6553,
    -6563, -6574, -6584, -6594, -6605, -6615, -6626, -6636, -6647, -6657, -6668,
    -6678, -6689, -6700, -6710, -6721, -6732, -6743, -6754, -6765, -6775, -6786,
    -6797, -6808, -6820, -6831, -6842, -6853, -6864, -6875, -6887, -6898, -6909,
    -6921, -6932, -6944, -6955, -6967, -6978, -6990, -7002, -7013, -7025, -7037,
    -7049, -7061, -7073, -7084, -7096, -7108, -7121, -7133, -7145, -7157, -7169,
    -7182, -7194, -7206, -7219, -7231, -7244, -7256, -7269, -7281, -7294, -7307,
    -7319, -7332, -7345, -7358, -7371, -7384, -7397, -7410, -7423, -7436, -7449,
    -7463, -7476, -7489, -7503, -7516, -7530, -7543, -7557, -7570, -7584, -7598,
    -7612, -7626, -7639, -7653, -7667, -7681, -7695, -7710, -7724, -7738, -7752,
    -7767, -7781, -7796, -7810, -7825, -7839, -7854, -7869, -7884, -7898, -7913,
    -7928, -7943, -7958, -7973, -7989, -8004, -8019, -8035, -8050, -8065, -8081,
    -8097, -8112, -8128, -8144, -8160, -8176, -8192, -8208, -8224, -8240, -8256,
    -8272, -8289, -8305, -8322, -8338, -8355, -8371, -8388, -8405, -8422, -8439,
    -8456, -8473, -8490, -8507, -8525, -8542, -8559, -8577, -8594, -8612, -8630,
    -8648, -8665, -8683, -8701, -8719, -8738, -8756, -8774, -8793, -8811, -8830,
    -8848, -8867, -8886, -8905, -8924, -8943, -8962, -8981, -9000, -9020, -9039,
    -9058, -9078, -9098, -9118, -9137, -9157, -9177, -9198, -9218, -9238, -9258,
    -9279, -9300, -9320, -9341, -9362, -9383, -9404, -9425, -9446, -9467, -9489,
    -9510, -9532, -9554, -9576, -9597, -9619, -9642, -9664, -9686, -9709, -9731,
    -9754, -9776, -9799, -9822, -9845, -9868, -9892, -9915, -9939, -9962, -9986,
    -10010, -10034, -10058, -10082, -10106, -10131, -10155, -10180, -10205, -10230,
    -10255, -10280, -10305, -10330, -10356, -10381, -10407, -10433, -10459, -10485,
    -10512, -10538, -10564, -10591, -10618, -10645, -10672, -10699, -10727, -10754,
    -10782, -10810, -10837, -10866, -10894, -10922, -10951, -10979, -11008, -11037,
    -11066, -11096, -11125, -11155, -11184, -11214, -11244, -11275, -11305, -11335,
    -11366, -11397, -11428, -11459, -11491, -11522, -11554, -11586, -11618, -11650,
    -11683, -11715, -11748, -11781, -11814, -11848, -11881, -11915, -11949, -11983,
    -12018, -12052, -12087, -12122, -12157, -12192, -12228, -12264, -12300, -12336,
    -12372, -12409, -12446, -12483, -12520, -12557, -12595, -12633, -12671, -12710,
    -12748, -12787, -12826, -12865, -12905, -12945, -12985, -13025, -13066, -13107,
    -13148, -13189, -13231, -13273, -13315, -13357, -13400, -13443, -13486, -13530,
    -13573, -13617, -13662, -13706, -13751, -13797, -13842, -13888, -13934, -13981,
    -14027, -14074, -14122, -14169, -14217, -14266, -14315, -14364, -14413, -14463,
    -14513, -14563, -14614, -14665, -14716, -14768, -14820, -14873, -14926, -14979,
    -15033, -15087, -15141, -15196, -15252, -15307, -15363, -15420, -15477, -15534,
    -15592, -15650, -15709, -15768, -15827, -15887, -15947, -16008, -16070, -16131,
    -16194, -16256, -16320, -16384, -16448, -16513, -16578, -16644, -16710, -16777,
    -16844, -16912, -16980, -17050, -17119, -17189, -17260, -17331, -17403, -17476,
    -17549, -17623, -17697, -17772, -17848, -17924, -18001, -18078, -18157, -18236,
    -18315, -18396, -18477, -18558, -18641, -18724, -18808, -18893, -18978, -19065,
    -19152, -19239, -19328, -19418, -19508, -19599, -19691, -19784, -19878, -19972,
    -20068, -20164, -20262, -20360, -20460, -20560, -20661, -20763, -20867, -20971,
    -21076, -21183, -21290, -21399, -21509, -21620, -21732, -21845, -21959, -22075,
    -22192, -22310, -22429, -22550, -22671, -22795, -22919, -23045, -23172, -23301,
    -23431, -23563, -23696, -23831, -23967, -24105, -24244, -24385, -24528, -24672,
    -24818, -24966, -25115, -25266, -25420, -25575, -25731, -25890, -26051, -26214,
    -26379, -26546, -26715, -26886, -27060, -27235, -27413, -27594, -27776, -27962,
    -28149, -28339, -28532, -28728, -28926, -29127, -29330, -29537, -29746, -29959,
    -30174, -30393, -30615, -30840, -31068, -31300, -31536, -31775, -32017, -32263,
    -32513, -32768, -33026, -33288, -33554, -33825, -34100, -34379, -34663, -34952,
    -35246, -35544, -35848, -36157, -36472, -36792, -37117, -37449, -37786, -38130,
    -38479, -38836, -39199, -39568, -39945, -40329, -40721, -41120, -41527, -41943,
    -42366, -42799, -43240, -43690, -44150, -44620, -45100, -45590, -46091, -46603,
    -47127, -47662, -48210, -48770, -49344, -49932, -50533, -51150, -51781, -52428,
    -53092, -53773, -54471, -55188, -55924, -56679, -57456, -58254, -59074, -59918,
    -60787, -61680, -62601, -63550, -64527, -65536, -66576, -67650, -68759, -69905,
    -71089, -72315, -73584, -74898, -76260, -77672, -79137, -80659, -82241, -83886,
    -85598, -87381, -89240, -91180, -93206, -95325, -97541, -99864, -102300,
    -104857, -107546, -110376, -113359, -116508, -119837, -123361, -127100, -131072,
    -135300, -139810, -144631, -149796, -155344, -161319, -167772, -174762, -182361,
    -190650, -199728, -209715, -220752, -233016, -246723, -262144, -279620, -299593,
    -322638, -349525, -381300, -419430, -466033, -524288, -599186, -699050, -838860,
    -1048576, -1398101, -2097152, -4194304, 0,
];

#[inline]
fn quick_inverse(x: SkFDot6) -> SkFixed {
    const LAST_ENTRY: i32 = K_INVERSE_TABLE_SIZE;
    debug_assert!(abs32(x) <= LAST_ENTRY);
    if x > 0 {
        -INVERSE_TABLE[(LAST_ENTRY - x) as usize]
    } else {
        INVERSE_TABLE[(LAST_ENTRY + x) as usize]
    }
}

#[inline]
fn quick_div(a: SkFDot6, b: SkFDot6) -> SkFixed {
    const MIN_BITS: i32 = 3;
    const MAX_BITS: i32 = 31;
    const MAX_ABS_A: i32 = 1 << (MAX_BITS - (22 - MIN_BITS));
    let abs_a = abs32(a);
    let abs_b = abs32(b);
    if abs_b >= (1 << MIN_BITS) && abs_b < K_INVERSE_TABLE_SIZE && abs_a < MAX_ABS_A {
        return a.wrapping_mul(quick_inverse(b)) >> 6;
    }
    fdot6_div(a, b)
}

#[inline]
pub const fn snap_y(y: SkFixed) -> SkFixed {
    const ACCURACY: u32 = K_DEFAULT_ACCURACY as u32;
    ((((y as u32).wrapping_add((SK_FIXED1 as u32) >> (ACCURACY + 1))) >> (16 - ACCURACY))
        << (16 - ACCURACY)) as SkFixed
}

#[inline]
fn cheap_distance(mut dx: SkFDot6, mut dy: SkFDot6) -> SkFDot6 {
    dx = abs32(dx);
    dy = abs32(dy);
    if dx > dy {
        dx.wrapping_add(dy >> 1)
    } else {
        dy.wrapping_add(dx >> 1)
    }
}

#[inline]
fn diff_to_shift(dx: SkFDot6, dy: SkFDot6, shift_aa: i32) -> i32 {
    let mut dist = cheap_distance(dx, dy);
    dist = dist.wrapping_add(1 << (2 + shift_aa)) >> (3 + shift_aa);
    (32 - clz_u32(dist as u32)) >> 1
}

#[inline]
fn fdot6_up_shift(x: SkFDot6, up_shift: i32) -> i32 {
    left_shift_i32(x, up_shift as u32)
}

#[inline]
fn cubic_delta_from_line(a: SkFDot6, b: SkFDot6, c: SkFDot6, d: SkFDot6) -> SkFDot6 {
    let one_third = (a
        .wrapping_mul(8)
        .wrapping_sub(b.wrapping_mul(15))
        .wrapping_add(c.wrapping_mul(6))
        .wrapping_add(d)
        .wrapping_mul(19))
        >> 9;
    let two_third = (a
        .wrapping_add(b.wrapping_mul(6))
        .wrapping_sub(c.wrapping_mul(15))
        .wrapping_add(d.wrapping_mul(8))
        .wrapping_mul(19))
        >> 9;
    abs32(one_third).max(abs32(two_third))
}

/// Core fields of `SkAnalyticEdge` (without `fNext`/`fPrev` pointers).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalyticEdgeBase {
    pub x: SkFixed,
    pub dx: SkFixed,
    pub upper_x: SkFixed,
    pub y: SkFixed,
    pub upper_y: SkFixed,
    pub lower_y: SkFixed,
    pub dy: SkFixed,
    pub edge_type: i8,
    pub curve_count: i8,
    pub curve_shift: u8,
    pub winding: i8,
}

/// Subclass fields of `SkAnalyticQuadraticEdge`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalyticQuadState {
    pub base: AnalyticEdgeBase,
    pub qx: SkFixed,
    pub qy: SkFixed,
    pub q_dx: SkFixed,
    pub q_dy: SkFixed,
    pub q_ddx: SkFixed,
    pub q_ddy: SkFixed,
    pub q_last_x: SkFixed,
    pub q_last_y: SkFixed,
    pub snapped_x: SkFixed,
    pub snapped_y: SkFixed,
}

/// Subclass fields of `SkAnalyticCubicEdge`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalyticCubicState {
    pub base: AnalyticEdgeBase,
    pub cx: SkFixed,
    pub cy: SkFixed,
    pub c_dx: SkFixed,
    pub c_dy: SkFixed,
    pub c_ddx: SkFixed,
    pub c_ddy: SkFixed,
    pub c_dddx: SkFixed,
    pub c_dddy: SkFixed,
    pub c_last_x: SkFixed,
    pub c_last_y: SkFixed,
    pub snapped_y: SkFixed,
    pub to_fixed_shift: u8,
    pub _pad: [u8; 3],
}

impl AnalyticEdgeBase {
    #[inline]
    pub const fn edge_type(&self) -> EdgeType {
        EdgeType::from_i8(self.edge_type)
    }

    #[inline]
    pub const fn winding(&self) -> EdgeWinding {
        EdgeWinding::from_i8(self.winding)
    }

    #[inline]
    pub fn go_y(&mut self, y: SkFixed) {
        if y == self.y.wrapping_add(SK_FIXED1) {
            self.x = sat_add_i32(self.x, self.dx);
            self.y = y;
        } else if y != self.y {
            let delta = ((self.dx as i64) * ((y as i64) - (self.upper_y as i64))) >> 16;
            self.x = pin_64_to_s32((self.upper_x as i64) + delta);
            self.y = y;
        }
    }

    #[inline]
    pub fn go_y_shift(&mut self, y: SkFixed, y_shift: i32) {
        debug_assert!(y_shift >= 0 && y_shift <= K_DEFAULT_ACCURACY);
        debug_assert!(self.dx == 0 || y - self.y == (SK_FIXED1 >> y_shift));
        self.y = y;
        self.x = sat_add_i32(self.x, self.dx >> y_shift);
    }

    /// Matches `SkAnalyticEdge::setLine`.
    pub fn set_line(&mut self, p0: Point, p1: Point) -> bool {
        const ACCURACY: i32 = K_DEFAULT_ACCURACY;
        const MULTIPLIER: f32 = (1 << ACCURACY) as f32;

        let mut x0 = fdot6_to_fixed(float_to_fdot6(p0.x * MULTIPLIER)) >> ACCURACY;
        let mut y0 = snap_y(fdot6_to_fixed(float_to_fdot6(p0.y * MULTIPLIER)) >> ACCURACY);
        let mut x1 = fdot6_to_fixed(float_to_fdot6(p1.x * MULTIPLIER)) >> ACCURACY;
        let mut y1 = snap_y(fdot6_to_fixed(float_to_fdot6(p1.y * MULTIPLIER)) >> ACCURACY);

        let mut winding = EdgeWinding::Cw;
        if y0 > y1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
            winding = EdgeWinding::Ccw;
        }

        let dy = fixed_to_fdot6(y1.wrapping_sub(y0));
        if dy == 0 {
            return false;
        }
        let dx = fixed_to_fdot6(x1.wrapping_sub(x0));
        let slope = quick_div(dx, dy);
        let abs_slope = abs32(slope);

        self.x = x0;
        self.dx = slope;
        self.upper_x = x0;
        self.y = y0;
        self.upper_y = y0;
        self.lower_y = y1;
        self.dy = if dx == 0 || slope == 0 {
            SK_MAX_S32
        } else if abs_slope < K_INVERSE_TABLE_SIZE {
            quick_inverse(abs_slope)
        } else {
            abs32(quick_div(dy, dx))
        };
        self.edge_type = EdgeType::Line as i8;
        self.curve_count = 0;
        self.winding = winding as i8;
        self.curve_shift = 0;
        true
    }

    /// Matches `SkAnalyticEdge::updateLine`.
    pub fn update_line(
        &mut self,
        mut x0: SkFixed,
        mut y0: SkFixed,
        mut x1: SkFixed,
        mut y1: SkFixed,
        slope: SkFixed,
    ) -> bool {
        debug_assert!(self.winding == 1 || self.winding == -1);
        debug_assert!(self.curve_count != 0);

        if y0 > y1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
            self.winding = -self.winding;
        }

        let dx = fixed_to_fdot6(x1.wrapping_sub(x0));
        let dy = fixed_to_fdot6(y1.wrapping_sub(y0));
        if dy == 0 {
            return false;
        }

        debug_assert!(slope < SK_MAX_S32);

        let abs_slope = abs32(fixed_to_fdot6(slope));
        self.x = x0;
        self.dx = slope;
        self.upper_x = x0;
        self.y = y0;
        self.upper_y = y0;
        self.lower_y = y1;
        self.dy = if dx == 0 || slope == 0 {
            SK_MAX_S32
        } else if abs_slope < K_INVERSE_TABLE_SIZE {
            quick_inverse(abs_slope)
        } else {
            abs32(quick_div(dy, dx))
        };
        true
    }
}

impl AnalyticQuadState {
    #[inline]
    pub fn keep_continuous(&mut self) {
        self.snapped_x = self.base.x;
        self.snapped_y = self.base.y;
    }

    /// Matches `SkAnalyticQuadraticEdge::setQuadraticWithoutUpdate`.
    pub fn set_quadratic_without_update(&mut self, pts: &[Point; 3], shift_in: i32) -> bool {
        let scale = (1i32 << (shift_in + 6)) as f32;
        let mut x0 = float_to_i32_c(pts[0].x * scale);
        let mut y0 = float_to_i32_c(pts[0].y * scale);
        let x1 = float_to_i32_c(pts[1].x * scale);
        let y1 = float_to_i32_c(pts[1].y * scale);
        let mut x2 = float_to_i32_c(pts[2].x * scale);
        let mut y2 = float_to_i32_c(pts[2].y * scale);

        let mut winding = EdgeWinding::Cw;
        if y0 > y2 {
            core::mem::swap(&mut x0, &mut x2);
            core::mem::swap(&mut y0, &mut y2);
            winding = EdgeWinding::Ccw;
        }

        let top = fdot6_round(y0);
        let bot = fdot6_round(y2);
        if top == bot {
            return false;
        }

        let dx = left_shift_i32(x1, 1).wrapping_sub(x0).wrapping_sub(x2) >> 2;
        let dy = left_shift_i32(y1, 1).wrapping_sub(y0).wrapping_sub(y2) >> 2;
        let mut shift = diff_to_shift(dx, dy, shift_in);
        debug_assert!(shift >= 0);

        if shift == 0 {
            shift = 1;
        } else if shift > MAX_COEFF_SHIFT {
            shift = MAX_COEFF_SHIFT;
        }

        self.base.winding = winding as i8;
        self.base.edge_type = EdgeType::Quad as i8;
        self.base.curve_count = (1i32 << shift) as i8;
        self.base.curve_shift = (shift - 1) as u8;

        let a_x = fdot6_to_fixed_div2(x0.wrapping_sub(x1).wrapping_sub(x1).wrapping_add(x2));
        let b_x = fdot6_to_fixed(x1.wrapping_sub(x0));
        self.qx = fdot6_to_fixed(x0);
        self.q_dx = b_x.wrapping_add(a_x >> shift);
        self.q_ddx = a_x >> (shift - 1);

        let a_y = fdot6_to_fixed_div2(y0.wrapping_sub(y1).wrapping_sub(y1).wrapping_add(y2));
        let b_y = fdot6_to_fixed(y1.wrapping_sub(y0));
        self.qy = fdot6_to_fixed(y0);
        self.q_dy = b_y.wrapping_add(a_y >> shift);
        self.q_ddy = a_y >> (shift - 1);

        self.q_last_x = fdot6_to_fixed(x2);
        self.q_last_y = fdot6_to_fixed(y2);
        true
    }

    /// Matches `SkAnalyticQuadraticEdge::setQuadratic`.
    pub fn set_quadratic(&mut self, pts: &[Point; 3]) -> bool {
        if !self.set_quadratic_without_update(pts, K_DEFAULT_ACCURACY) {
            return false;
        }
        self.qx >>= K_DEFAULT_ACCURACY;
        self.qy >>= K_DEFAULT_ACCURACY;
        self.q_dx >>= K_DEFAULT_ACCURACY;
        self.q_dy >>= K_DEFAULT_ACCURACY;
        self.q_ddx >>= K_DEFAULT_ACCURACY;
        self.q_ddy >>= K_DEFAULT_ACCURACY;
        self.q_last_x >>= K_DEFAULT_ACCURACY;
        self.q_last_y >>= K_DEFAULT_ACCURACY;
        self.qy = snap_y(self.qy);
        self.q_last_y = snap_y(self.q_last_y);

        self.base.edge_type = EdgeType::Quad as i8;
        self.snapped_x = self.qx;
        self.snapped_y = self.qy;

        self.update_quadratic()
    }

    /// Matches `SkAnalyticQuadraticEdge::updateQuadratic`.
    pub fn update_quadratic(&mut self) -> bool {
        let mut success = false;
        let mut count = self.base.curve_count as i32;
        let mut oldx = self.qx;
        let mut oldy = self.qy;
        let mut dx = self.q_dx;
        let mut dy = self.q_dy;
        let shift = self.base.curve_shift as i32;

        debug_assert!(count > 0);

        let mut newx;
        let mut newy;
        let mut new_snapped_x;
        let mut new_snapped_y;
        loop {
            let slope;
            count -= 1;
            if count > 0 {
                newx = oldx.wrapping_add(dx >> shift);
                newy = oldy.wrapping_add(dy >> shift);
                if abs32(dy >> shift) >= SK_FIXED1 * 2
                    && left_shift_i64(abs32(dy) as i64, 6) > (abs32(dx) as i64)
                {
                    let diff_y = fixed_to_fdot6(newy.wrapping_sub(self.snapped_y));
                    slope = if diff_y != 0 {
                        quick_div(fixed_to_fdot6(newx.wrapping_sub(self.snapped_x)), diff_y)
                    } else {
                        SK_MAX_S32
                    };
                    new_snapped_y = self.q_last_y.min(fixed_round_to_fixed(newy));
                    new_snapped_x =
                        newx.wrapping_sub(fixed_mul(slope, newy.wrapping_sub(new_snapped_y)));
                } else {
                    new_snapped_y = self.q_last_y.min(snap_y(newy));
                    new_snapped_x = newx;
                    let diff_y = fixed_to_fdot6(new_snapped_y.wrapping_sub(self.snapped_y));
                    slope = if diff_y != 0 {
                        quick_div(fixed_to_fdot6(newx.wrapping_sub(self.snapped_x)), diff_y)
                    } else {
                        SK_MAX_S32
                    };
                }
                dx = dx.wrapping_add(self.q_ddx);
                dy = dy.wrapping_add(self.q_ddy);
            } else {
                newx = self.q_last_x;
                newy = self.q_last_y;
                new_snapped_y = newy;
                new_snapped_x = newx;
                let diff_y = fixed_to_fdot6(newy.wrapping_sub(self.snapped_y));
                slope = if diff_y != 0 {
                    quick_div(fixed_to_fdot6(newx.wrapping_sub(self.snapped_x)), diff_y)
                } else {
                    SK_MAX_S32
                };
            }
            if slope < SK_MAX_S32 {
                success = self.base.update_line(
                    self.snapped_x,
                    self.snapped_y,
                    new_snapped_x,
                    new_snapped_y,
                    slope,
                );
            }
            oldx = newx;
            oldy = newy;
            if !(count > 0 && !success) {
                break;
            }
        }

        debug_assert!(new_snapped_y <= self.q_last_y);

        self.qx = newx;
        self.qy = newy;
        self.q_dx = dx;
        self.q_dy = dy;
        self.snapped_x = new_snapped_x;
        self.snapped_y = new_snapped_y;
        self.base.curve_count = count as i8;
        success
    }
}

impl AnalyticCubicState {
    #[inline]
    pub fn keep_continuous(&mut self) {
        self.cx = self.base.x;
        self.snapped_y = self.base.y;
    }

    /// Matches `SkAnalyticCubicEdge::setCubicWithoutUpdate`.
    pub fn set_cubic_without_update(&mut self, pts: &[Point; 4], shift_in: i32) -> bool {
        let scale = (1i32 << (shift_in + 6)) as f32;
        let mut x0 = float_to_i32_c(pts[0].x * scale);
        let mut y0 = float_to_i32_c(pts[0].y * scale);
        let mut x1 = float_to_i32_c(pts[1].x * scale);
        let mut y1 = float_to_i32_c(pts[1].y * scale);
        let mut x2 = float_to_i32_c(pts[2].x * scale);
        let mut y2 = float_to_i32_c(pts[2].y * scale);
        let mut x3 = float_to_i32_c(pts[3].x * scale);
        let mut y3 = float_to_i32_c(pts[3].y * scale);

        let mut winding = EdgeWinding::Cw;
        if y0 > y3 {
            core::mem::swap(&mut x0, &mut x3);
            core::mem::swap(&mut x1, &mut x2);
            core::mem::swap(&mut y0, &mut y3);
            core::mem::swap(&mut y1, &mut y2);
            winding = EdgeWinding::Ccw;
        }

        let top = fdot6_round(y0);
        let bot = fdot6_round(y3);
        if top == bot {
            return false;
        }

        let dx = cubic_delta_from_line(x0, x1, x2, x3);
        let dy = cubic_delta_from_line(y0, y1, y2, y3);
        let mut shift = diff_to_shift(dx, dy, 2) + 1;
        debug_assert!(shift > 0);
        if shift > MAX_COEFF_SHIFT {
            shift = MAX_COEFF_SHIFT;
        }

        let mut up_shift = 6;
        let mut down_shift = shift + up_shift - 10;
        if down_shift < 0 {
            down_shift = 0;
            up_shift = 10 - shift;
        }

        self.base.winding = winding as i8;
        self.base.edge_type = EdgeType::Cubic as i8;
        self.base.curve_count = left_shift_i32(-1, shift as u32) as i8;
        self.base.curve_shift = shift as u8;
        self.to_fixed_shift = down_shift as u8;

        let b_x = fdot6_up_shift(3i32.wrapping_mul(x1.wrapping_sub(x0)), up_shift);
        let c_x = fdot6_up_shift(
            3i32.wrapping_mul(x0.wrapping_sub(x1).wrapping_sub(x1).wrapping_add(x2)),
            up_shift,
        );
        let d_x = fdot6_up_shift(
            x3.wrapping_add(3i32.wrapping_mul(x1.wrapping_sub(x2)))
                .wrapping_sub(x0),
            up_shift,
        );

        self.cx = fdot6_to_fixed(x0);
        self.c_dx = b_x
            .wrapping_add(c_x >> shift)
            .wrapping_add(d_x >> (2 * shift));
        self.c_ddx = ((2i64 * (c_x as i64)) + ((3i64 * (d_x as i64)) >> (shift - 1))) as i32;
        self.c_dddx = ((3i64 * (d_x as i64)) >> (shift - 1)) as i32;

        let b_y = fdot6_up_shift(3i32.wrapping_mul(y1.wrapping_sub(y0)), up_shift);
        let c_y = fdot6_up_shift(
            3i32.wrapping_mul(y0.wrapping_sub(y1).wrapping_sub(y1).wrapping_add(y2)),
            up_shift,
        );
        let d_y = fdot6_up_shift(
            y3.wrapping_add(3i32.wrapping_mul(y1.wrapping_sub(y2)))
                .wrapping_sub(y0),
            up_shift,
        );

        self.cy = fdot6_to_fixed(y0);
        self.c_dy = b_y
            .wrapping_add(c_y >> shift)
            .wrapping_add(d_y >> (2 * shift));
        self.c_ddy = ((2i64 * (c_y as i64)) + ((3i64 * (d_y as i64)) >> (shift - 1))) as i32;
        self.c_dddy = ((3i64 * (d_y as i64)) >> (shift - 1)) as i32;

        self.c_last_x = fdot6_to_fixed(x3);
        self.c_last_y = fdot6_to_fixed(y3);
        true
    }

    /// Matches `SkAnalyticCubicEdge::setCubic`.
    pub fn set_cubic(&mut self, pts: &[Point; 4]) -> bool {
        if !self.set_cubic_without_update(pts, K_DEFAULT_ACCURACY) {
            return false;
        }
        self.cx >>= K_DEFAULT_ACCURACY;
        self.cy >>= K_DEFAULT_ACCURACY;
        self.c_dx >>= K_DEFAULT_ACCURACY;
        self.c_dy >>= K_DEFAULT_ACCURACY;
        self.c_ddx >>= K_DEFAULT_ACCURACY;
        self.c_ddy >>= K_DEFAULT_ACCURACY;
        self.c_dddx >>= K_DEFAULT_ACCURACY;
        self.c_dddy >>= K_DEFAULT_ACCURACY;
        self.c_last_x >>= K_DEFAULT_ACCURACY;
        self.c_last_y >>= K_DEFAULT_ACCURACY;
        self.cy = snap_y(self.cy);
        self.snapped_y = self.cy;
        self.c_last_y = snap_y(self.c_last_y);

        self.base.edge_type = EdgeType::Cubic as i8;
        self.update_cubic()
    }

    /// Matches `SkAnalyticCubicEdge::updateCubic`.
    pub fn update_cubic(&mut self) -> bool {
        let mut count = self.base.curve_count as i32;
        let mut oldx = self.cx;
        let mut oldy = self.cy;
        let ddshift = self.base.curve_shift as i32;
        let dshift = self.to_fixed_shift as i32;

        debug_assert!(count < 0);

        let mut newx;
        let mut newy;
        let mut success;
        loop {
            count += 1;
            if count < 0 {
                newx = oldx.wrapping_add(self.c_dx >> dshift);
                self.c_dx = self.c_dx.wrapping_add(self.c_ddx >> ddshift);
                self.c_ddx = self.c_ddx.wrapping_add(self.c_dddx);

                newy = oldy.wrapping_add(self.c_dy >> dshift);
                self.c_dy = self.c_dy.wrapping_add(self.c_ddy >> ddshift);
                self.c_ddy = self.c_ddy.wrapping_add(self.c_dddy);
            } else {
                newx = self.c_last_x;
                newy = self.c_last_y;
            }

            if newy < oldy {
                newy = oldy;
            }

            let mut new_snapped_y = snap_y(newy);
            if self.c_last_y < new_snapped_y {
                new_snapped_y = self.c_last_y;
                count = 0;
            }

            let diff_y = fixed_to_fdot6(new_snapped_y.wrapping_sub(self.snapped_y));
            let slope = if diff_y == 0 {
                SK_MAX_S32
            } else {
                fdot6_div(fixed_to_fdot6(newx.wrapping_sub(oldx)), diff_y)
            };

            success = self
                .base
                .update_line(oldx, self.snapped_y, newx, new_snapped_y, slope);

            oldx = newx;
            oldy = newy;
            self.snapped_y = new_snapped_y;
            if !(count < 0 && !success) {
                break;
            }
        }

        self.cx = newx;
        self.cy = newy;
        self.base.curve_count = count as i8;
        success
    }
}

/// Polymorphic safe Rust analytic edge used by `AnalyticEdgeBuilder` and `aaa_path`.
#[derive(Copy, Clone, Debug)]
pub enum AnalyticEdge {
    Line(AnalyticEdgeBase),
    Quad(AnalyticQuadState),
    Cubic(AnalyticCubicState),
}

impl AnalyticEdge {
    #[inline]
    pub fn base(&self) -> &AnalyticEdgeBase {
        match self {
            Self::Line(b) => b,
            Self::Quad(q) => &q.base,
            Self::Cubic(c) => &c.base,
        }
    }

    #[inline]
    pub fn base_mut(&mut self) -> &mut AnalyticEdgeBase {
        match self {
            Self::Line(b) => b,
            Self::Quad(q) => &mut q.base,
            Self::Cubic(c) => &mut c.base,
        }
    }

    #[inline]
    pub fn update(&mut self, last_y: SkFixed) -> bool {
        debug_assert!(last_y >= self.base().lower_y);
        let cc = self.base().curve_count;
        if cc < 0 {
            match self {
                Self::Cubic(c) => c.update_cubic(),
                _ => false,
            }
        } else if cc > 0 {
            match self {
                Self::Quad(q) => q.update_quadratic(),
                _ => false,
            }
        } else {
            false
        }
    }
}
