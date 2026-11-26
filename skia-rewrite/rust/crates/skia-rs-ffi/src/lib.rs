//! C++/Rust FFI boundary for skia-rs.
//!
//! `unsafe` is strictly restricted to this boundary crate; all logic crates
//! use `#![forbid(unsafe_code)]`.

pub mod color;
pub mod core_types;
pub mod geometry;
pub mod geometry_curves;
pub mod math;
pub mod matrix_rrect;
pub mod md5;
