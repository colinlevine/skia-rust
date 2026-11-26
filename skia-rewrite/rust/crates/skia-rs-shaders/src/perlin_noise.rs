//! Safe Rust port of `SkPerlinNoiseShaderImpl.cpp` and `SkPerlinNoiseShaderImpl.h`
//! (validation, SVG feTurbulence PRNG, permutation lattice, normalized 16-bit gradients,
//! and tile stitching frequency adjustment).

use skia_rs_geometry::{float_saturate2int, ISize, Point};

use crate::float_round2int;

pub const BLOCK_SIZE: usize = 256;
pub const PERLIN_NOISE_OFFSET: i32 = 4096;
pub const RAND_MAXIMUM: i32 = i32::MAX; // 2^31 - 1
pub const MAX_OCTAVES: i32 = 255;

/// Mirrors `SkPerlinNoiseShaderType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PerlinNoiseType {
    FractalNoise = 0,
    Turbulence = 1,
}

impl PerlinNoiseType {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::FractalNoise),
            1 => Some(Self::Turbulence),
            _ => None,
        }
    }
}

/// Action returned by `classify_perlin_noise`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PerlinNoiseAction {
    ReturnNull = 0,
    ReturnFractalZeroOctaves = 1,
    ReturnTurbulenceZeroOctaves = 2,
    Create = 3,
}

/// Matches `valid_input(...)` in `SkPerlinNoiseShaderImpl.cpp`.
pub fn valid_input(
    base_x: f32,
    base_y: f32,
    num_octaves: i32,
    tile_size: Option<ISize>,
    seed: f32,
) -> bool {
    if !(base_x >= 0.0 && base_y >= 0.0) {
        return false;
    }
    if !(0..=MAX_OCTAVES).contains(&num_octaves) {
        return false;
    }
    if let Some(ts) = tile_size {
        if !(ts.width >= 0 && ts.height >= 0) {
            return false;
        }
    }
    if !seed.is_finite() {
        return false;
    }
    true
}

/// Matches `SkShaders::MakeFractalNoise` and `SkShaders::MakeTurbulence` factory classification.
pub fn classify_perlin_noise(
    noise_type: PerlinNoiseType,
    base_x: f32,
    base_y: f32,
    num_octaves: i32,
    seed: f32,
    tile_size: Option<ISize>,
) -> PerlinNoiseAction {
    if !valid_input(base_x, base_y, num_octaves, tile_size, seed) {
        return PerlinNoiseAction::ReturnNull;
    }
    if num_octaves == 0 {
        match noise_type {
            PerlinNoiseType::FractalNoise => PerlinNoiseAction::ReturnFractalZeroOctaves,
            PerlinNoiseType::Turbulence => PerlinNoiseAction::ReturnTurbulenceZeroOctaves,
        }
    } else {
        PerlinNoiseAction::Create
    }
}

/// Mirrors `SkPerlinNoiseShader::StitchData`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct StitchData {
    pub width: i32,
    pub wrap_x: i32,
    pub height: i32,
    pub wrap_y: i32,
}

impl StitchData {
    pub fn from_scalars(w: f32, h: f32) -> Self {
        let width = float_round2int(w).min(i32::MAX - PERLIN_NOISE_OFFSET);
        let wrap_x = PERLIN_NOISE_OFFSET + width;
        let height = float_round2int(h).min(i32::MAX - PERLIN_NOISE_OFFSET);
        let wrap_y = PERLIN_NOISE_OFFSET + height;
        Self {
            width,
            wrap_x,
            height,
            wrap_y,
        }
    }
}

/// Full computed Perlin noise tables matching `SkPerlinNoiseShader::PaintingData`.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct PaintingDataTables {
    pub seed: i32,
    pub lattice_selector: [u8; BLOCK_SIZE],
    pub noise: [[[u16; 2]; BLOCK_SIZE]; 4],
    pub tile_size: ISize,
    pub base_frequency_x: f32,
    pub base_frequency_y: f32,
    pub stitch_data_init: StitchData,
}

/// SVG feTurbulence Lehmer LCG step (exact match with `PaintingData::random()`).
#[inline]
fn next_random(seed: &mut i32) -> i32 {
    const RAND_AMPLITUDE: i32 = 16807;
    const RAND_Q: i32 = 127773;
    const RAND_R: i32 = 2836;

    let mut result = RAND_AMPLITUDE * (*seed % RAND_Q) - RAND_R * (*seed / RAND_Q);
    if result <= 0 {
        result += RAND_MAXIMUM;
    }
    *seed = result;
    result
}

/// Computes the exact Perlin noise permutation lattice, normalized 16-bit gradient tables,
/// and optional tile-stitching adjusted frequencies and `StitchData`.
pub fn compute_painting_data(
    tile_size_in: ISize,
    seed_scalar: f32,
    base_frequency_x: f32,
    base_frequency_y: f32,
) -> PaintingDataTables {
    let mut base_freq_x = base_frequency_x;
    let mut base_freq_y = base_frequency_y;
    let tile_size = ISize {
        width: float_round2int(tile_size_in.width as f32),
        height: float_round2int(tile_size_in.height as f32),
    };

    // init(seed):
    // According to the SVG spec, truncate (not round) the seed value.
    let mut seed = float_saturate2int(seed_scalar);
    if seed <= 0 {
        seed = -(seed % (RAND_MAXIMUM - 1)) + 1;
    }
    if seed > RAND_MAXIMUM - 1 {
        seed = RAND_MAXIMUM - 1;
    }

    let mut lattice_selector = [0u8; BLOCK_SIZE];
    let mut noise = [[[0u16; 2]; BLOCK_SIZE]; 4];

    for channel in 0..4 {
        for i in 0..BLOCK_SIZE {
            lattice_selector[i] = i as u8;
            noise[channel][i][0] = (next_random(&mut seed) % (2 * BLOCK_SIZE as i32)) as u16;
            noise[channel][i][1] = (next_random(&mut seed) % (2 * BLOCK_SIZE as i32)) as u16;
        }
    }

    for i in (1..BLOCK_SIZE).rev() {
        let k = lattice_selector[i];
        let j = (next_random(&mut seed) % (BLOCK_SIZE as i32)) as usize;
        lattice_selector[i] = lattice_selector[j];
        lattice_selector[j] = k;
    }

    // Permute noise data using lattice_selector
    let noise_copy = noise;
    for i in 0..BLOCK_SIZE {
        let sel = lattice_selector[i] as usize;
        for channel in 0..4 {
            noise[channel][i][0] = noise_copy[channel][sel][0];
            noise[channel][i][1] = noise_copy[channel][sel][1];
        }
    }

    // Compute normalized gradients and quantize to 16-bit
    const HALF_MAX_16BITS: f32 = 32767.5;
    const INV_BLOCK_SIZE_F: f32 = 1.0 / (BLOCK_SIZE as f32);
    for channel in 0..4 {
        for i in 0..BLOCK_SIZE {
            let mut gradient = Point::new(
                ((noise[channel][i][0] as i32) - (BLOCK_SIZE as i32)) as f32 * INV_BLOCK_SIZE_F,
                ((noise[channel][i][1] as i32) - (BLOCK_SIZE as i32)) as f32 * INV_BLOCK_SIZE_F,
            );
            gradient.normalize();
            noise[channel][i][0] = float_round2int((gradient.x + 1.0) * HALF_MAX_16BITS) as u16;
            noise[channel][i][1] = float_round2int((gradient.y + 1.0) * HALF_MAX_16BITS) as u16;
        }
    }

    // stitch() if !tile_size.isEmpty()
    let mut stitch_data_init = StitchData::default();
    if tile_size.width > 0 && tile_size.height > 0 {
        let tile_width = tile_size.width as f32;
        let tile_height = tile_size.height as f32;
        if base_freq_x != 0.0 {
            let low_freq = (tile_width * base_freq_x).floor() / tile_width;
            let high_freq = (tile_width * base_freq_x).ceil() / tile_width;
            if (base_freq_x / low_freq) < (high_freq / base_freq_x) {
                base_freq_x = low_freq;
            } else {
                base_freq_x = high_freq;
            }
        }
        if base_freq_y != 0.0 {
            let low_freq = (tile_height * base_freq_y).floor() / tile_height;
            let high_freq = (tile_height * base_freq_y).ceil() / tile_height;
            if (base_freq_y / low_freq) < (high_freq / base_freq_y) {
                base_freq_y = low_freq;
            } else {
                base_freq_y = high_freq;
            }
        }
        stitch_data_init = StitchData::from_scalars(
            tile_width * base_freq_x,
            tile_height * base_freq_y,
        );
    }

    PaintingDataTables {
        seed,
        lattice_selector,
        noise,
        tile_size,
        base_frequency_x: base_freq_x,
        base_frequency_y: base_freq_y,
        stitch_data_init,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_validation() {
        assert!(valid_input(0.05, 0.05, 4, None, 0.0));
        assert!(!valid_input(-0.1, 0.05, 4, None, 0.0));
        assert!(!valid_input(0.05, f32::NAN, 4, None, 0.0));
        assert!(!valid_input(0.05, 0.05, -1, None, 0.0));
        assert!(!valid_input(0.05, 0.05, 256, None, 0.0));
        assert!(!valid_input(0.05, 0.05, 4, Some(ISize { width: -1, height: 10 }), 0.0));
        assert!(!valid_input(0.05, 0.05, 4, None, f32::INFINITY));

        assert_eq!(
            classify_perlin_noise(PerlinNoiseType::FractalNoise, 0.1, 0.1, 0, 0.0, None),
            PerlinNoiseAction::ReturnFractalZeroOctaves
        );
        assert_eq!(
            classify_perlin_noise(PerlinNoiseType::Turbulence, 0.1, 0.1, 0, 0.0, None),
            PerlinNoiseAction::ReturnTurbulenceZeroOctaves
        );
    }

    #[test]
    fn perlin_painting_data_permutation_and_stitch() {
        let tables = compute_painting_data(ISize { width: 64, height: 64 }, 0.0, 0.05, 0.05);
        // Lattice selector must be a bijection of 0..=255.
        let mut seen = [false; BLOCK_SIZE];
        for &v in &tables.lattice_selector {
            seen[v as usize] = true;
        }
        assert!(seen.iter().all(|&b| b));

        // 64 * 0.05 = 3.2 -> low = 3/64 = 0.046875, high = 4/64 = 0.0625.
        // 0.05 / 0.046875 = 1.0666... < 0.0625 / 0.05 = 1.25 -> chooses low (3/64).
        assert!((tables.base_frequency_x - 3.0 / 64.0).abs() < 1e-6);
        assert_eq!(tables.stitch_data_init.width, 3);
        assert_eq!(tables.stitch_data_init.wrap_x, 4096 + 3);
        assert_eq!(tables.stitch_data_init.height, 3);
        assert_eq!(tables.stitch_data_init.wrap_y, 4096 + 3);

        // Verify normalized gradients have length ~1 (or 0 for degenerate (0,0)).
        for ch in 0..4 {
            for i in 0..BLOCK_SIZE {
                let gx = (tables.noise[ch][i][0] as f32) / 32767.5 - 1.0;
                let gy = (tables.noise[ch][i][1] as f32) / 32767.5 - 1.0;
                let len = (gx * gx + gy * gy).sqrt();
                assert!(
                    (len - 1.0).abs() < 1e-3 || len < 1e-3,
                    "unexpected gradient len {len} at ch={ch} i={i}"
                );
            }
        }
    }

    #[test]
    fn perlin_negative_seed_wrapping() {
        let t1 = compute_painting_data(ISize { width: 0, height: 0 }, -5.0, 0.1, 0.1);
        let t2 = compute_painting_data(ISize { width: 0, height: 0 }, 6.0, 0.1, 0.1);
        // -(-5 % (2^31 - 2)) + 1 = 5 + 1 = 6
        assert_eq!(t1.lattice_selector, t2.lattice_selector);
        assert_eq!(t1.noise, t2.noise);
    }
}
