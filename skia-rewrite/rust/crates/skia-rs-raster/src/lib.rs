#![forbid(unsafe_code)]

//! Safe Rust port of Skia Layer 3.3 (`skia-rs-raster`):
//!
//! - `SkMemset_opts.cpp` -> [`memset`]
//! - `SkBlitRow_D32.cpp`, `SkBlitRow_opts.cpp` -> [`blit_row`]
//! - `SkBlitMask_opts.cpp` -> [`blit_mask`]
//! - `SkBlitter_A8.cpp` -> [`blitter_a8`]
//! - `SkBlitter_ARGB32.cpp` -> [`blitter_argb32`]
//! - `SkBlitter.cpp`, `SkBlitter_Sprite.cpp`, `SkSpriteBlitter_ARGB32.cpp` -> [`blitter`]
//! - `SkRasterPipeline.cpp`, `SkRasterPipelineBlitter.cpp`, `SkOpts.cpp` -> [`raster_pipeline`]

pub mod blit_mask;
pub mod blit_row;
pub mod blitter;
pub mod blitter_a8;
pub mod blitter_argb32;
pub mod memset;
pub mod raster_pipeline;
