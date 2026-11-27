#![forbid(unsafe_code)]
//! Safe Rust rewrite of Skia's RAW/DNG (`SkRawCodec`) orientation, dimension,
//! scaling, color/swizzle, stream-buffering, tile-partitioning, and preview selection logic
//! (`Phase 2 Module P2.8`).

pub mod header;
pub mod scaling;
pub mod stream;
pub mod tiling;
pub mod types;

pub use header::{
    encoded_info_spec, evaluate_piex_preview, exif_orientation_to_origin, get_endian_short,
    init_raw_image_meta, is_raw_sniff, is_tiff_header_valid, is_valid_endian_marker,
};
pub use scaling::{
    build_dng_pixel_buffer_layout, dimensions_supported, get_scaled_dimensions, max_render_size,
    plan_get_pixels, preferred_render_size, row_area, validate_rendered_size,
};
pub use stream::{
    is_asset_stream, plan_asset_transfer, safe_add_to_usize, safe_add_usize, validate_asset_read,
    validate_dng_read, BufferMoreActionKind, BufferMorePlan, BufferedTransferKind,
    BufferedTransferPlan, RawBufferedStream,
};
pub use tiling::{
    compute_task_areas, num_tasks_required, num_tiles_in_area, num_tiles_per_task,
    perform_area_task_threads,
};
pub use types::{
    DngPixelBufferLayout, DngPoint, DngRect, PiexColorSpace, PiexDecisionKind, PiexError,
    PiexImageFormat, PiexPreviewPlan, RawEncodedInfoSpec, RawGetPixelsPlan, RawImageMeta,
    DNG_MOSAIC_PLANE_ALL, DNG_TT_BYTE, RAW_MAX_RENDER_DIFF_RATIO, RAW_MAX_STREAM_SIZE,
    RAW_MIN_CHUNK_READ, RAW_MIN_SHORT_EDGE, RAW_READ_TO_END, SKCMS_PIXEL_FORMAT_RGB_888,
    TIFF_HEADER_SIZE, TIFF_MAGIC,
};
