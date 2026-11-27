#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's codec metadata, EXIF/TIFF, color profile,
//! gainmap, HDR/AGTM, XMP, and ICC profile encoder (`P2.2`).

pub mod color_profile;
pub mod exif;
pub mod gainmap;
pub mod hdr_agtm;
pub mod hdr_metadata;
pub mod icc;
pub mod tiff;
pub mod xmp;
