#![forbid(unsafe_code)]

//! Safe Rust port of Skia Layer 3.4a (`skia-rs-bitmap`, part 1 of `skia-rs-image`):
//! - `SkPixmap`, `SkPixmapDraw`
//! - `SkPixelRef`, `SkMallocPixelRef`, `SkBitmapCache`
//! - `SkBitmap`
//! - `SkBitmapProcState`, `SkBitmapProcState_matrixProcs`, `SkBitmapProcState_opts`
//! - `SkMipmap`, `SkMipmapAccessor`, `SkMipmapBuilder`, `SkMipmapDrawDownSampler`, `SkMipmapHQDownSampler`
//! - `SkSpecialImage`

pub mod bitmap;
pub mod bitmap_proc_state;
pub mod mipmap;
pub mod pixel_ref;
pub mod pixmap;
pub mod special_image;
