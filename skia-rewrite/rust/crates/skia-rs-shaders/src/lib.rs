#![forbid(unsafe_code)]

//! Safe Rust port of Skia Phase 1 Layer 4.1 shaders and gradients (`skia-rs-shaders`).
//!
//! Covers 21 C++ files:
//! - `src/shaders/` (16 files, `:core`):
//!   `SkShader.cpp`, `SkShaderBase.cpp`, `SkColorShader.cpp`, `SkEmptyShader.cpp`,
//!   `SkBlendShader.cpp`, `SkColorFilterShader.cpp`, `SkCoordClampShader.cpp`,
//!   `SkLocalMatrixShader.cpp`, `SkTransformShader.cpp`, `SkTriColorShader.cpp`,
//!   `SkWorkingColorSpaceShader.cpp`, `SkImageShader.cpp`, `SkBitmapProcShader.cpp`,
//!   `SkPictureShader.cpp`, `SkPerlinNoiseShaderImpl.cpp`, `SkGainmapShader.cpp`
//! - `src/shaders/gradients/` (5 files, `:skia`):
//!   `SkGradientBaseShader.cpp`, `SkLinearGradient.cpp`, `SkRadialGradient.cpp`,
//!   `SkSweepGradient.cpp`, `SkConicalGradient.cpp`

pub mod blend_shader;
pub mod compose_shaders;
pub mod gainmap;
pub mod gradient_base;
pub mod gradients;
pub mod image_shader;
pub mod perlin_noise;
pub mod picture_shader;
pub mod shader_base;

/// Mirrors `SkTileMode` in `include/core/SkTileMode.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TileMode {
    Clamp = 0,
    Repeat = 1,
    Mirror = 2,
    Decal = 3,
}

pub const TILE_MODE_COUNT: usize = 4;

impl TileMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Clamp),
            1 => Some(Self::Repeat),
            2 => Some(Self::Mirror),
            3 => Some(Self::Decal),
            _ => None,
        }
    }

    #[inline]
    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Clamp),
            1 => Some(Self::Repeat),
            2 => Some(Self::Mirror),
            3 => Some(Self::Decal),
            _ => None,
        }
    }
}

/// Mirrors `SkFilterMode` in `include/core/SkSamplingOptions.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Nearest = 0,
    Linear = 1,
}

impl FilterMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Nearest),
            1 => Some(Self::Linear),
            _ => None,
        }
    }
}

/// Mirrors `SkMipmapMode` in `include/core/SkSamplingOptions.h`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MipmapMode {
    None = 0,
    Nearest = 1,
    Linear = 2,
}

impl MipmapMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Nearest),
            2 => Some(Self::Linear),
            _ => None,
        }
    }
}

/// Mirrors `SkCubicResampler` in `include/core/SkSamplingOptions.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CubicResampler {
    pub b: f32,
    pub c: f32,
}

/// Mirrors `SkSamplingOptions` in `include/core/SkSamplingOptions.h` (exact layout: 24 bytes).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SamplingOptions {
    pub max_aniso: i32,
    pub use_cubic: bool,
    pub _pad0: [u8; 3],
    pub cubic: CubicResampler,
    pub filter: FilterMode,
    pub mipmap: MipmapMode,
}

impl SamplingOptions {
    #[inline]
    pub const fn new(filter: FilterMode, mipmap: MipmapMode) -> Self {
        Self {
            max_aniso: 0,
            use_cubic: false,
            _pad0: [0; 3],
            cubic: CubicResampler { b: 0.0, c: 0.0 },
            filter,
            mipmap,
        }
    }

    #[inline]
    pub const fn cubic(b: f32, c: f32) -> Self {
        Self {
            max_aniso: 0,
            use_cubic: true,
            _pad0: [0; 3],
            cubic: CubicResampler { b, c },
            filter: FilterMode::Nearest,
            mipmap: MipmapMode::None,
        }
    }

    #[inline]
    pub const fn aniso(max_aniso: i32) -> Self {
        let clamped = if max_aniso > 1 { max_aniso } else { 1 };
        Self {
            max_aniso: clamped,
            use_cubic: false,
            _pad0: [0; 3],
            cubic: CubicResampler { b: 0.0, c: 0.0 },
            filter: FilterMode::Nearest,
            mipmap: MipmapMode::None,
        }
    }

    #[inline]
    pub const fn is_aniso(&self) -> bool {
        self.max_aniso != 0
    }
}

/// Matches Skia's `sk_float_round2int(x)` = `sk_float_saturate2int((float)floor((double)x + 0.5))`.
#[inline]
pub fn float_round2int(x: f32) -> i32 {
    let r = ((x as f64) + 0.5).floor() as f32;
    skia_rs_geometry::float_saturate2int(r)
}

/// Matches Skia's `sk_float_ceil2int(x)` = `sk_float_saturate2int(ceilf(x))`.
#[inline]
pub fn float_ceil2int(x: f32) -> i32 {
    skia_rs_geometry::float_saturate2int(x.ceil())
}

/// Matches Skia's `sk_float_radians_to_degrees(radians)` = `radians * (180.0 / SK_FloatPI)`.
#[inline]
pub fn radians_to_degrees(radians: f32) -> f32 {
    radians * (180.0_f32 / skia_rs_geometry::SK_SCALAR_PI)
}

/// Matches Skia's `SkMatrix::RectToRectOrIdentity(src, dst)`.
#[inline]
pub fn rect_to_rect_or_identity(
    src: skia_rs_geometry::Rect,
    dst: skia_rs_geometry::Rect,
) -> skia_rs_geometry::Matrix {
    skia_rs_geometry::Matrix::rect_to_rect(src, dst, skia_rs_geometry::matrix::ScaleToFit::Fill)
        .unwrap_or_else(skia_rs_geometry::Matrix::identity)
}
