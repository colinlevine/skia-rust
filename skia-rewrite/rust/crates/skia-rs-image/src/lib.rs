#![forbid(unsafe_code)]

//! Safe Rust port of Skia Layer 3.4b (`skia-rs-image`, completing Layer 3.4 alongside
//! `skia-rs-bitmap`):
//! - `SkImageGenerator`
//! - `SkImage`, `SkImage_Base`
//! - `SkImage_Lazy`, `SkImage_LazyFactories`, `SkImage_Picture`
//! - `SkImage_Raster`, `SkImage_RasterFactories`
//! - `SkRescaleAndReadPixels`
//! - `SkSurface`, `SkSurface_Base`, `SkSurface_Null`, `SkSurface_Raster`

pub mod generator;
pub mod image;
pub mod lazy_picture;
pub mod raster;
pub mod rescale;
pub mod surface;
