#![forbid(unsafe_code)]

//! Safe Rust implementation of Skia's scan converters, edge builders,
//! alpha-run encoding, hairline/antihair rasterizers, and SDF generator
//! (Phase 1 Layer 3.2: 11 C++ files).

pub mod aaa_path;
pub mod alpha_runs;
pub mod analytic_edge;
pub mod antihair;
pub mod blitter;
pub mod distance_field;
pub mod edge;
pub mod edge_builder;
pub mod fixed;
pub mod hairline;
pub mod scan_path;
pub mod scan_rect;

#[cfg(test)]
mod tests;
