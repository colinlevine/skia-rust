//! C++/Rust FFI boundary for skia-rs.
//!
//! `unsafe` is strictly restricted to this boundary crate; all logic crates
//! use `#![forbid(unsafe_code)]`.

pub mod bitmap;
pub mod color;
pub mod core_types;
pub mod geometry;
pub mod geometry_curves;
pub mod image;
pub mod math;
pub mod matrix_rrect;
pub mod md5;
pub mod path;
pub mod raster;
pub mod region;
pub mod scan;
