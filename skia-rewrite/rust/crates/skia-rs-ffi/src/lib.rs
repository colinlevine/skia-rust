//! C++/Rust FFI boundary for skia-rs.
//!
//! `unsafe` is strictly restricted to this boundary crate; all logic crates
//! use `#![forbid(unsafe_code)]`.

pub mod md5;
