//! Floating-point comparison helpers matching Skia's `SkFloatingPoint.cpp`.

#[inline]
fn magnitude(a: f64) -> f64 {
    const EXTRACT_MAGNITUDE: u64 =
        0b0_11111111111_0000000000000000000000000000000000000000000000000000;
    f64::from_bits(a.to_bits() & EXTRACT_MAGNITUDE)
}

pub fn doubles_nearly_equal_ulps(a: f64, b: f64, max_ulps_diff: u8) -> bool {
    const MIN_MAGNITUDE: f64 = f64::MIN_POSITIVE;
    let max_magnitude = magnitude(a).max(MIN_MAGNITUDE).max(magnitude(b));
    let tolerance = max_magnitude * (f64::EPSILON * (max_ulps_diff as f64 + 1.0));
    a == b || (b - a).abs() < tolerance
}

#[inline]
pub fn double_nearly_zero(a: f64) -> bool {
    a == 0.0 || a.abs() < (f32::EPSILON as f64)
}

#[inline]
pub fn double_to_float(x: f64) -> f32 {
    x as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulps_edges() {
        assert!(doubles_nearly_equal_ulps(1.0, 1.0, 16));
        assert!(doubles_nearly_equal_ulps(f64::INFINITY, f64::INFINITY, 16));
        assert!(!doubles_nearly_equal_ulps(f64::NAN, f64::NAN, 16));
        assert!(double_nearly_zero(0.0));
        assert!(double_nearly_zero(1e-10));
        assert!(!double_nearly_zero(1e-5));
    }
}
