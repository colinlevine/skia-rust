#![forbid(unsafe_code)]

//! Safe Rust port of Skia Phase 1 Layer 4.4 image filters (`skia-rs-imagefilters`).
//!
//! Covers:
//! - `src/core/SkImageFilterTypes.cpp` (`skif::Mapping`, `LayerSpace`, `FilterResult`, `Builder`)
//! - `src/core/SkImageFilter.cpp` (`SkImageFilter`, `SkImageFilter_Base`)
//! - `src/core/SkImageFilterCache.cpp` (`SkImageFilterCache`, `SkImageFilterCacheKey`)
//! - `src/core/SkLocalMatrixImageFilter.cpp` (`SkLocalMatrixImageFilter`)
//! - 16 `src/effects/imagefilters/*.cpp` effect image filters (excluding `SkRuntimeImageFilter.cpp`)

pub mod cache;
pub mod core_filter;
pub mod effects;
pub mod filter_result;
pub mod types;

#[cfg(test)]
mod tests;
