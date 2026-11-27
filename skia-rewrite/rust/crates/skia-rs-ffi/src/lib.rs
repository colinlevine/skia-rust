//! C++/Rust FFI boundary for skia-rs.
//!
//! `unsafe` is strictly restricted to this boundary crate; all logic crates
//! use `#![forbid(unsafe_code)]`.

pub mod bitmap;
pub mod canvas;
pub mod codec_bmp;
pub mod codec_core;
pub mod codec_gif;
pub mod codec_jpeg;
pub mod codec_meta;
pub mod codec_png;
pub mod codec_raw;
pub mod codec_webp;
pub mod color;
pub mod colorfilters;
pub mod core_types;
pub mod effects;
pub mod geometry;
pub mod geometry_curves;
pub mod image;
pub mod imagefilters;
pub mod math;
pub mod matrix_rrect;
pub mod md5;
pub mod path;
pub mod pathops;
pub mod picture;
pub mod raster;
pub mod region;
pub mod scan;
pub mod shaders;
pub mod text;
