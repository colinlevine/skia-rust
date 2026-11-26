#![forbid(unsafe_code)]

//! Safe Rust port of Skia Layer 2.1 color, color space, image info, blend mode,
//! pixel swizzle/conversion, YUV math, and mask modules (`skia-rs-color`).

pub mod blend_mode;
pub mod color;
pub mod color_space;
pub mod color_space_xform_steps;
pub mod color_table;
pub mod convert_pixels;
pub mod image_info;
pub mod masks;
pub mod swizzle;
pub mod unpremultiply;
pub mod yuv_math;
pub mod yuva_info;
pub mod yuva_pixmaps;
