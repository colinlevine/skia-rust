//! Cubic ease-in / ease-out curve evaluation matching Skia's `SkCubicMap`.

const SCALAR_NEARLY_ZERO: f32 = 1.0 / (1 << 12) as f32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum CubicMapType {
    Line = 0,
    CubeRoot = 1,
    Solver = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct CubicMap {
    pub coeff: [[f32; 2]; 3],
    pub map_type: CubicMapType,
}

#[inline]
fn scalar_nearly_equal(x: f32, y: f32) -> bool {
    (x - y).abs() <= SCALAR_NEARLY_ZERO
}

#[inline]
fn coeff_nearly_zero(delta: f32) -> bool {
    delta.abs() <= 0.0000001f32
}

#[inline]
fn nearly_zero_nonneg(x: f32) -> bool {
    x <= 0.0000000001f32
}

#[inline]
fn eval_poly2(t: f32, m: f32, b: f32) -> f32 {
    m.mul_add(t, b)
}

#[inline]
fn eval_poly3(t: f32, a: f32, b: f32, c: f32) -> f32 {
    eval_poly2(t, eval_poly2(t, a, b), c)
}

#[inline]
fn eval_poly4(t: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    eval_poly3(t, eval_poly2(t, a, b), c, d)
}

fn cubic_solver(a: f32, b: f32, c: f32, d: f32) -> f32 {
    let mut t = -d;
    for _ in 0..8 {
        let f = eval_poly4(t, a, b, c, d);
        if f.abs() <= 0.00005f32 {
            break;
        }
        let fp = eval_poly3(t, 3.0 * a, 2.0 * b, c);
        let fpp = eval_poly2(t, 3.0 * a + 3.0 * a, 2.0 * b);
        let numer = 2.0 * fp * f;
        let denom = (2.0 * fp).mul_add(fp, -(f * fpp));
        t -= numer / denom;
    }
    t
}

impl CubicMap {
    pub fn new(mut p1: [f32; 2], mut p2: [f32; 2]) -> Self {
        // Match C++ std::min(std::max(x, 0.0f), 1.0f) NaN behavior.
        p1[0] = if p1[0] > 0.0 { p1[0] } else { 0.0 };
        p1[0] = if p1[0] < 1.0 { p1[0] } else { 1.0 };
        p2[0] = if p2[0] > 0.0 { p2[0] } else { 0.0 };
        p2[0] = if p2[0] < 1.0 { p2[0] } else { 1.0 };

        let s1 = [p1[0] * 3.0, p1[1] * 3.0];
        let s2 = [p2[0] * 3.0, p2[1] * 3.0];

        let coeff = [
            [1.0 + s1[0] - s2[0], 1.0 + s1[1] - s2[1]],
            [s2[0] - s1[0] - s1[0], s2[1] - s1[1] - s1[1]],
            s1,
        ];

        let map_type = if scalar_nearly_equal(p1[0], p1[1]) && scalar_nearly_equal(p2[0], p2[1]) {
            CubicMapType::Line
        } else if coeff_nearly_zero(coeff[1][0]) && coeff_nearly_zero(coeff[2][0]) {
            CubicMapType::CubeRoot
        } else {
            CubicMapType::Solver
        };

        Self { coeff, map_type }
    }

    pub fn compute_y_from_x(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if nearly_zero_nonneg(x) || nearly_zero_nonneg(1.0 - x) {
            return x;
        }
        if self.map_type == CubicMapType::Line {
            return x;
        }
        let t = if self.map_type == CubicMapType::CubeRoot {
            (x / self.coeff[0][0]).powf(1.0f32 / 3.0)
        } else {
            cubic_solver(self.coeff[0][0], self.coeff[1][0], self.coeff[2][0], -x)
        };
        let a = self.coeff[0][1];
        let b = self.coeff[1][1];
        let c = self.coeff[2][1];
        ((a * t + b) * t + c) * t
    }

    pub fn compute_from_t(&self, t: f32) -> [f32; 2] {
        let a = self.coeff[0];
        let b = self.coeff[1];
        let c = self.coeff[2];
        [
            ((a[0] * t + b[0]) * t + c[0]) * t,
            ((a[1] * t + b[1]) * t + c[1]) * t,
        ]
    }
}
