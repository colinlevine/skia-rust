//! Safe Rust implementation of `SkGaussFilter` (discrete Gaussian kernel via modified Bessel functions).

pub const GAUSS_ARRAY_MAX: usize = 6;

const GOOD_ENOUGH: f64 = 1.0 / 100.0;

#[inline]
fn bessel_i0(t: f64) -> f64 {
    let t_sq_over_4 = t * t / 4.0;
    let mut sum = 1.0;
    let mut factor = 1.0;
    let mut k = 1.0;
    while factor > 1.0 / 1_000_000.0 {
        factor *= t_sq_over_4 / (k * k);
        sum += factor;
        k += 1.0;
    }
    sum
}

#[inline]
fn bessel_i1(t: f64) -> f64 {
    let t_sq_over_4 = t * t / 4.0;
    let mut sum = t / 2.0;
    let mut factor = sum;
    let mut k = 1.0;
    while factor > 1.0 / 1_000_000.0 {
        factor *= t_sq_over_4 / (k * (k + 1.0));
        sum += factor;
        k += 1.0;
    }
    sum
}

fn normalize(gauss: &mut [f64]) {
    debug_assert!(!gauss.is_empty());

    // Carefully add from smallest to largest to calculate the normalizing sum.
    let mut sum = 0.0;
    for i in (1..gauss.len()).rev() {
        sum += 2.0 * gauss[i];
    }
    sum += gauss[0];

    // Normalize gauss.
    for g in gauss.iter_mut() {
        *g /= sum;
    }

    // Put any remaining slop into gauss[0] so the symmetric kernel sums to 1.0.
    sum = 0.0;
    for i in (1..gauss.len()).rev() {
        sum += 2.0 * gauss[i];
    }
    gauss[0] = 1.0 - sum;
}

/// Computes discrete Gaussian kernel half-basis into `basis` and returns `n` (`1..=5`),
/// matching `SkGaussFilter::SkGaussFilter(double sigma)`.
pub fn calculate_bessel_factors(sigma: f64, basis: &mut [f64; GAUSS_ARRAY_MAX]) -> i32 {
    debug_assert!((0.0..2.0).contains(&sigma));

    let var = sigma * sigma;
    let d = var.exp();
    let mut b = [0.0_f64; GAUSS_ARRAY_MAX];
    b[0] = bessel_i0(var);
    b[1] = bessel_i1(var);
    basis[0] = b[0] / d;
    basis[1] = b[1] / d;

    let mut n = 1usize;
    while basis[n] > GOOD_ENOUGH {
        b[n + 1] = -(2.0 * (n as f64) / var) * b[n] + b[n - 1];
        basis[n + 1] = b[n + 1] / d;
        n += 1;
    }

    normalize(&mut basis[..n]);
    n as i32
}

/// Safe Rust representation mirroring `SkGaussFilter`.
#[derive(Debug, Clone, Copy)]
pub struct GaussFilter {
    basis: [f64; GAUSS_ARRAY_MAX],
    n: usize,
}

impl GaussFilter {
    pub fn new(sigma: f64) -> Self {
        let mut basis = [0.0_f64; GAUSS_ARRAY_MAX];
        let n = calculate_bessel_factors(sigma, &mut basis) as usize;
        Self { basis, n }
    }

    pub fn factors(&self) -> &[f64] {
        &self.basis[..self.n]
    }

    pub fn radius(&self) -> usize {
        self.n - 1
    }

    pub fn width(&self) -> usize {
        2 * self.radius() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gauss_filter_sigma_zero() {
        let f = GaussFilter::new(0.0);
        assert_eq!(f.factors().len(), 1);
        assert_eq!(f.factors()[0], 1.0);
        assert_eq!(f.radius(), 0);
        assert_eq!(f.width(), 1);
    }

    #[test]
    fn test_gauss_filter_normalization_and_symmetry() {
        for &sigma in &[0.25, 0.5, 1.0, 1.5, 1.99] {
            let f = GaussFilter::new(sigma);
            let k = f.factors();
            assert!(!k.is_empty() && k.len() <= 5);
            let sum: f64 = k[0] + 2.0 * k[1..].iter().sum::<f64>();
            assert!((sum - 1.0).abs() < 1e-14, "sigma={sigma}, sum={sum}");
            // Monotonically decreasing from center.
            for w in k.windows(2) {
                assert!(w[0] > w[1]);
            }
        }
    }
}
