#![forbid(unsafe_code)]

//! Safe Rust port of Skia Phase 1 Layer 4.2 color filters, color matrix,
//! high-contrast filter, and arithmetic blender (`skia-rs-colorfilters`).
//!
//! Covers:
//! - `src/core/SkColorFilter.cpp`
//! - `src/effects/SkBlenders.cpp`
//! - `src/effects/SkColorMatrix.cpp`
//! - `src/effects/SkColorMatrixFilter.cpp`
//! - `src/effects/SkHighContrastFilter.cpp`
//! - `src/effects/colorfilters/SkBlendModeColorFilter.cpp`
//! - `src/effects/colorfilters/SkColorFilterBase.cpp`
//! - `src/effects/colorfilters/SkColorSpaceXformColorFilter.cpp`
//! - `src/effects/colorfilters/SkComposeColorFilter.cpp`
//! - `src/effects/colorfilters/SkGaussianColorFilter.cpp`
//! - `src/effects/colorfilters/SkMatrixColorFilter.cpp`
//! - `src/effects/colorfilters/SkTableColorFilter.cpp`
//! - `src/effects/colorfilters/SkWorkingFormatColorFilter.cpp`

pub mod blend_mode_cf;
pub mod blenders;
pub mod color_filter;
pub mod color_matrix;
pub mod compose_cf;
pub mod gaussian_cf;
pub mod high_contrast;
pub mod lighting;
pub mod matrix_cf;
pub mod table_cf;
pub mod working_format_cf;
pub mod xform_cf;
