//! Safe Rust implementation of `SkPicture`, `SkPictInfo`, `SkPictureFlat`,
//! `SkPictureData`, `SkPictureRecord`, `SkPicturePlayback`, `SkPictureRecorder`,
//! and `SkPictureImageGenerator` logic.

use std::sync::atomic::{AtomicU32, Ordering};

use skia_rs_core_types::stream::size_of_packed_uint;
use skia_rs_geometry::Rect;

/// SKP binary header magic (`"skiapict"`).
pub const SKP_MAGIC: [u8; 8] = *b"skiapict";

/// Minimum supported SKP version (`SkPicturePriv::kMin_Version == 82`).
pub const SKP_MIN_VERSION: u32 = 82;

/// Current SKP version (`SkPicturePriv::kCurrent_Version == 110`).
pub const SKP_CURRENT_VERSION: u32 = 110;

/// Version where expanding clip ops were removed (`kNoExpandingClipOps == 89`).
pub const SKP_NO_EXPANDING_CLIP_OPS_VERSION: u32 = 89;

/// Version where SaveLayer backdrop scale factor was added (`kBackdropScaleFactor == 90`).
pub const SKP_BACKDROP_SCALE_FACTOR_VERSION: u32 = 90;

/// Version where SaveLayer multiple filters were added (`kMultipleFiltersOnSaveLayer == 104`).
pub const SKP_MULTIPLE_FILTERS_VERSION: u32 = 104;

/// Version where SaveLayer backdrop tile mode was added (`kSaveLayerBackdropTileMode == 106`).
pub const SKP_BACKDROP_TILE_MODE_VERSION: u32 = 106;

/// Default recursion limit for nested pictures (`SkPicturePriv::kDefaultRecursionLimit`).
pub const DEFAULT_RECURSION_LIMIT: i32 = 100;

/// Matches `kMaxPictureOpsToUnrollInsteadOfRef` in `src/core/SkCanvasPriv.h`.
pub const MAX_PICTURE_OPS_TO_UNROLL: i32 = 1;

/// Matches `SkCanvas::kMaxFiltersPerLayer` (check exact value: 16).
pub const MAX_FILTERS_PER_LAYER: i32 = 16;

/// Trailing stream sentinel bytes after `SkPictInfo`.
pub const TRAILING_FAILURE: u8 = 0;
pub const TRAILING_PICTURE_DATA: u8 = 1;
pub const TRAILING_CUSTOM: u8 = 2;

/// `DrawType` bounds (`UNUSED = 0`, `LAST_DRAWTYPE_ENUM = DRAW_SLUG = 76`).
pub const DRAW_TYPE_UNUSED: u32 = 0;
pub const DRAW_TYPE_LAST: u32 = 76;

/// `MASK_24` used for packing `(DrawType, size)` into a 32-bit word.
pub const MASK_24: u32 = 0x00FF_FFFF;

/// `SaveLayerRecFlatFlags` bitmasks.
pub const SAVELAYERREC_HAS_BOUNDS: u32 = 1 << 0;
pub const SAVELAYERREC_HAS_PAINT: u32 = 1 << 1;
pub const SAVELAYERREC_HAS_BACKDROP: u32 = 1 << 2;
pub const SAVELAYERREC_HAS_FLAGS: u32 = 1 << 3;
pub const SAVELAYERREC_HAS_CLIPMASK_OBSOLETE: u32 = 1 << 4;
pub const SAVELAYERREC_HAS_CLIPMATRIX_OBSOLETE: u32 = 1 << 5;
pub const SAVELAYERREC_HAS_BACKDROP_SCALE: u32 = 1 << 6;
pub const SAVELAYERREC_HAS_MULTIPLE_FILTERS: u32 = 1 << 7;
pub const SAVELAYERREC_HAS_BACKDROP_TILEMODE: u32 = 1 << 8;

/// `SaveBehindFlatFlags`.
pub const SAVEBEHIND_HAS_SUBSET: u32 = 1 << 0;

/// `DrawVertexFlags` (used by `DRAW_PATCH`).
pub const DRAW_VERTICES_HAS_TEXS: u32 = 0x01;
pub const DRAW_VERTICES_HAS_COLORS: u32 = 0x02;
pub const DRAW_VERTICES_HAS_INDICES: u32 = 0x04;
pub const DRAW_VERTICES_HAS_XFER: u32 = 0x08;

/// `DrawAtlasFlags`.
pub const DRAW_ATLAS_HAS_COLORS: u32 = 1 << 0;
pub const DRAW_ATLAS_HAS_CULL: u32 = 1 << 1;
pub const DRAW_ATLAS_HAS_SAMPLING: u32 = 1 << 2;

/// `SkRegion::Op` constants for clip-op unpacking.
pub const REGION_DIFFERENCE_OP: u32 = 0;
pub const REGION_INTERSECT_OP: u32 = 1;
pub const REGION_REPLACE_OP: u32 = 5;

/// Binary layout matching `struct SkPictInfo` in `src/core/SkPictureData.h` (28 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictInfo {
    pub magic: [u8; 8],
    pub version: u32,
    pub cull_rect: Rect,
}

impl Default for PictInfo {
    fn default() -> Self {
        Self {
            magic: [0; 8],
            version: u32::MAX,
            cull_rect: Rect::make_empty(),
        }
    }
}

/// Generates the next unique non-zero 32-bit picture ID (`next_picture_id()`).
pub fn next_picture_id() -> u32 {
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// Creates a current-version `SkPictInfo` header for `cull_rect`.
#[inline]
pub fn create_header(cull_rect: Rect) -> PictInfo {
    PictInfo {
        magic: SKP_MAGIC,
        version: SKP_CURRENT_VERSION,
        cull_rect,
    }
}

/// Validates an `SkPictInfo` header (`SkPicture::IsValidPictInfo`).
#[inline]
pub fn is_valid_pict_info(info: &PictInfo) -> bool {
    info.magic == SKP_MAGIC
        && info.version >= SKP_MIN_VERSION
        && info.version <= SKP_CURRENT_VERSION
}

/// Encodes `PictInfo` into its exact 28-byte little-endian binary wire representation.
pub fn encode_pict_info(info: &PictInfo) -> [u8; 28] {
    let mut out = [0u8; 28];
    out[0..8].copy_from_slice(&info.magic);
    out[8..12].copy_from_slice(&info.version.to_le_bytes());
    out[12..16].copy_from_slice(&info.cull_rect.left.to_le_bytes());
    out[16..20].copy_from_slice(&info.cull_rect.top.to_le_bytes());
    out[20..24].copy_from_slice(&info.cull_rect.right.to_le_bytes());
    out[24..28].copy_from_slice(&info.cull_rect.bottom.to_le_bytes());
    out
}

/// Decodes a 28-byte little-endian `PictInfo`. Does not check validity; call `is_valid_pict_info`.
pub fn decode_pict_info(bytes: &[u8; 28]) -> PictInfo {
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&bytes[0..8]);
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let left = f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let top = f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let right = f32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let bottom = f32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    PictInfo {
        magic,
        version,
        cull_rect: Rect::make_ltrb(left, top, right, bottom),
    }
}

/// Exact `sk_negate_to_size_t(int32_t)` handling `i32::MIN` without UB.
#[inline]
pub const fn negate_to_size_t(value: i32) -> usize {
    value.wrapping_neg() as u32 as usize
}

/// Validates a custom-serialized `SkData` size (`SkTFitsIn<int32_t>(size) && size > 1`).
#[inline]
pub const fn validate_custom_serialize_size(size: usize) -> bool {
    size > 1 && size <= i32::MAX as usize
}

/// Number of zero padding bytes required to round `size` up to a 4-byte boundary.
#[inline]
pub const fn pad32_bytes(size: usize) -> usize {
    let rem = size & 3;
    if rem != 0 {
        4 - rem
    } else {
        0
    }
}

/// Returns whether `SkPicture::playback` should query the BBH (`!localClipBounds.contains(cullRect)`).
#[inline]
pub fn should_use_bbh(local_clip_bounds: Rect, cull_rect: Rect) -> bool {
    !local_clip_bounds.contains_rect(cull_rect)
}

/// Computes `SkPicture::approximateBytesUsed()`.
#[inline]
pub const fn picture_approx_bytes_used(
    is_placeholder: bool,
    self_size: usize,
    record_bytes: usize,
    sub_picture_bytes: usize,
    bbh_bytes: usize,
) -> usize {
    if is_placeholder {
        self_size
    } else {
        self_size + record_bytes + sub_picture_bytes + bbh_bytes
    }
}

/// Computes `SkPictureRecorder::beginRecording` cull rect normalization.
#[inline]
pub fn normalize_recording_cull_rect(user_cull: Rect) -> Rect {
    if user_cull.is_empty() {
        Rect::make_empty()
    } else {
        user_cull
    }
}

/// Computes the union of per-op bounds after `SkRecordFillBounds`.
pub fn compute_bbh_union(bounds: &[Rect]) -> Rect {
    let mut u = Rect::make_empty();
    for &b in bounds {
        u.join(b);
    }
    u
}

/// Computes `compute_chunk_size` in `SkPictureData.cpp`:
/// `4 + sum(len == 0 ? SizeOfPackedUInt(0) : SizeOfPackedUInt(len) + len)`.
pub fn compute_factory_chunk_size(name_lens: &[usize]) -> usize {
    let mut size = 4usize;
    for &len in name_lens {
        if len == 0 {
            size += size_of_packed_uint(0) as usize;
        } else {
            size += (size_of_packed_uint(len) as usize) + len;
        }
    }
    size
}

/// Validates a 1-based index into a table of length `table_len` (`SkPictureData` paint/path/etc.).
/// Returns `Some(zero_based_index)` if valid, else `None`.
#[inline]
pub const fn validate_base1_index(index: i32, table_len: i32) -> Option<usize> {
    if index > 0 && index <= table_len {
        Some((index - 1) as usize)
    } else {
        None
    }
}

/// Result of `optional_paint_index` validation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionalPaintResult {
    /// Index was 0 (valid, no paint).
    None = 0,
    /// Index was in `1..=table_len` (valid paint at `index - 1`).
    Valid = 1,
    /// Index was out of bounds (invalid).
    Invalid = 2,
}

#[inline]
pub const fn validate_optional_paint_index(index: i32, table_len: i32) -> OptionalPaintResult {
    if index == 0 {
        OptionalPaintResult::None
    } else if index > 0 && index <= table_len {
        OptionalPaintResult::Valid
    } else {
        OptionalPaintResult::Invalid
    }
}

/// Result of unpacking a 32-bit `(op, size)` header in `SkPicturePlayback::draw`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnpackedOpHeader {
    pub valid: bool,
    pub needs_extra_size: bool,
    pub op: u32,
    pub size: u32,
}

/// Unpacks the first 32-bit word of a serialized SKP op (`bits >> 24`, `bits & 0xffffff`).
#[inline]
pub const fn unpack_op_word(bits: u32) -> UnpackedOpHeader {
    let op = bits >> 24;
    let size = bits & MASK_24;
    if size == MASK_24 {
        UnpackedOpHeader {
            valid: true,
            needs_extra_size: true,
            op,
            size,
        }
    } else {
        let valid = size > 0 && op > DRAW_TYPE_UNUSED && op <= DRAW_TYPE_LAST;
        UnpackedOpHeader {
            valid,
            needs_extra_size: false,
            op,
            size,
        }
    }
}

/// Validates `(op, size)` after reading an overflow 32-bit size word.
#[inline]
pub const fn validate_op_and_size(op: u32, size: u32) -> bool {
    size > 0 && op > DRAW_TYPE_UNUSED && op <= DRAW_TYPE_LAST
}

/// Validates `offsetToRestore` in `SkPicturePlayback.cpp`:
/// `if offset_to_restore != 0 { is_align4(offset_to_restore) && offset_to_restore >= reader_offset }`.
#[inline]
pub const fn validate_offset_to_restore(offset_to_restore: usize, reader_offset: usize) -> bool {
    if offset_to_restore == 0 {
        return true;
    }
    (offset_to_restore & 3) == 0 && offset_to_restore >= reader_offset
}

/// Packs `(SkClipOp, doAA)` into 5 bits (`ClipParams_pack`).
#[inline]
pub const fn clip_params_pack(op: u32, do_aa: bool) -> u32 {
    let do_aa_bit = if do_aa { 1u32 } else { 0u32 };
    (do_aa_bit << 4) | op
}

/// Unpacks `doAA` from packed clip params (`ClipParams_unpackDoAA`).
#[inline]
pub const fn clip_params_unpack_do_aa(packed: u32) -> bool {
    ((packed >> 4) & 1) != 0
}

/// Result of `clip_params_unpack_region_op`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnpackedClipRegionOp {
    pub valid: bool,
    pub region_op: u32,
}

/// Matches `ClipParams_unpackRegionOp(SkReadBuffer*, uint32_t packed)`.
#[inline]
pub const fn clip_params_unpack_region_op(packed: u32, version: u32) -> UnpackedClipRegionOp {
    let unpacked = packed & 0xF;
    let ok = unpacked <= REGION_INTERSECT_OP
        || (unpacked <= REGION_REPLACE_OP && version < SKP_NO_EXPANDING_CLIP_OPS_VERSION);
    if ok {
        UnpackedClipRegionOp {
            valid: true,
            region_op: unpacked,
        }
    } else {
        UnpackedClipRegionOp {
            valid: false,
            region_op: REGION_INTERSECT_OP,
        }
    }
}

/// Action to take for a decoded `SkRegion::Op` in `do_clip_op` (`SkPicturePlayback.cpp`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipOpAction {
    /// Ignore expanding clip op from old SKP.
    Ignore = 0,
    /// Apply `clip_op` directly (0 = Difference, 1 = Intersect).
    DirectDifference = 1,
    DirectIntersect = 2,
    /// Reset clip first, then apply Intersect (emulated Replace).
    ResetThenIntersect = 3,
}

#[inline]
pub const fn map_region_op_to_clip_action(region_op: u32) -> ClipOpAction {
    match region_op {
        REGION_DIFFERENCE_OP => ClipOpAction::DirectDifference,
        REGION_INTERSECT_OP => ClipOpAction::DirectIntersect,
        REGION_REPLACE_OP => ClipOpAction::ResetThenIntersect,
        _ => ClipOpAction::Ignore,
    }
}

/// Computes `SaveLayerRecFlatFlags` and total byte size for `SkPictureRecord::recordSaveLayer`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveLayerFlatLayout {
    pub flat_flags: u32,
    pub total_bytes: usize,
}

#[allow(clippy::too_many_arguments)]
pub fn compute_save_layer_flat_layout(
    has_bounds: bool,
    has_paint: bool,
    has_backdrop: bool,
    save_layer_flags: u32,
    backdrop_scale: f32,
    filter_count: u32,
    backdrop_tile_mode_is_clamp: bool,
) -> SaveLayerFlatLayout {
    let mut size: usize = 2 * 4; // op + flatFlags
    let mut flat_flags: u32 = 0;

    if has_bounds {
        flat_flags |= SAVELAYERREC_HAS_BOUNDS;
        size += 16; // sizeof(SkRect)
    }
    if has_paint {
        flat_flags |= SAVELAYERREC_HAS_PAINT;
        size += 4;
    }
    if has_backdrop {
        flat_flags |= SAVELAYERREC_HAS_BACKDROP;
        size += 4;
    }
    if save_layer_flags != 0 {
        flat_flags |= SAVELAYERREC_HAS_FLAGS;
        size += 4;
    }
    if backdrop_scale != 1.0f32 {
        flat_flags |= SAVELAYERREC_HAS_BACKDROP_SCALE;
        size += 4;
    }
    if filter_count != 0 {
        flat_flags |= SAVELAYERREC_HAS_MULTIPLE_FILTERS;
        size += 4 + 4 * (filter_count as usize);
    }
    if !backdrop_tile_mode_is_clamp {
        flat_flags |= SAVELAYERREC_HAS_BACKDROP_TILEMODE;
        size += 4;
    }
    SaveLayerFlatLayout {
        flat_flags,
        total_bytes: size,
    }
}

/// Computes `DrawPatch` flags and total byte size for `SkPictureRecord::onDrawPatch`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawPatchFlatLayout {
    pub flags: u32,
    pub total_bytes: usize,
}

pub const fn compute_draw_patch_flat_layout(
    has_colors: bool,
    has_tex_coords: bool,
    is_modulate_blend: bool,
) -> DrawPatchFlatLayout {
    // op + paint index + 12 SkPoints (96 bytes) + flag
    let mut size: usize = 2 * 4 + 12 * 8 + 4;
    let mut flags: u32 = 0;
    if has_colors {
        flags |= DRAW_VERTICES_HAS_COLORS;
        size += 4 * 4;
    }
    if has_tex_coords {
        flags |= DRAW_VERTICES_HAS_TEXS;
        size += 4 * 8;
    }
    if !is_modulate_blend {
        flags |= DRAW_VERTICES_HAS_XFER;
        size += 4;
    }
    DrawPatchFlatLayout {
        flags,
        total_bytes: size,
    }
}

/// Computes `DrawAtlas` flags and total byte size for `SkPictureRecord::onDrawAtlas2`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawAtlasFlatLayout {
    pub flags: u32,
    pub total_bytes: usize,
}

pub const fn compute_draw_atlas_flat_layout(
    count: usize,
    has_colors: bool,
    has_cull: bool,
    sampling_flat_size: usize,
) -> DrawAtlasFlatLayout {
    // [op + paint + atlas + flags + count] + count*RSXform(16) + count*Rect(16) + sampling
    let mut size: usize = 5 * 4 + count * 16 + count * 16 + sampling_flat_size;
    let mut flags: u32 = DRAW_ATLAS_HAS_SAMPLING;
    if has_colors {
        flags |= DRAW_ATLAS_HAS_COLORS;
        size += count * 4 + 4;
    }
    if has_cull {
        flags |= DRAW_ATLAS_HAS_CULL;
        size += 16;
    }
    DrawAtlasFlatLayout {
        flags,
        total_bytes: size,
    }
}

/// Validates `DRAW_EDGEAA_IMAGE_SET` entry/clip/matrix counts and available bytes.
#[inline]
pub fn validate_edge_aa_entries_avail(cnt: i32, available: usize) -> bool {
    if cnt < 0 {
        return false;
    }
    const ENTRY_READ_SIZE: usize = 4 * 4 + 2 * 16 + 4; // 52 bytes
    match (cnt as usize).checked_mul(ENTRY_READ_SIZE) {
        Some(needed) => needed <= available,
        None => false,
    }
}

#[inline]
pub fn validate_edge_aa_matrices_avail(
    matrix_count: i32,
    max_matrix_index: i32,
    available: usize,
) -> bool {
    if matrix_count < 0 || max_matrix_index != matrix_count - 1 {
        return false;
    }
    const MATRIX_SIZE: usize = 9 * 4; // 36 bytes
    match (matrix_count as usize).checked_mul(MATRIX_SIZE) {
        Some(needed) => needed <= available,
        None => false,
    }
}

/// Validates `filterCount` for `SAVELAYERREC_HAS_MULTIPLE_FILTERS`.
#[inline]
pub const fn validate_save_layer_filter_count(filter_count: i32) -> bool {
    filter_count > 0 && filter_count <= MAX_FILTERS_PER_LAYER
}

/// Validates `SkImageGenerators::MakeFromPicture` parameters and chooses `SkColorType`.
/// Returns `1` for `kRGBA_F16_SkColorType`, `0` for `kN32_SkColorType`, or `-1` if invalid.
#[inline]
pub const fn validate_picture_image_generator(
    has_picture: bool,
    has_color_space: bool,
    width: i32,
    height: i32,
    is_f16_bit_depth: bool,
) -> i32 {
    if !has_picture || !has_color_space || width <= 0 || height <= 0 {
        return -1;
    }
    if is_f16_bit_depth { 1 } else { 0 }
}

/// Linear deduplication helper matching `find_or_append` in `SkPictureRecord.cpp`.
/// Returns `Some(existing_index)` if `id` is already in `ids`, else `None`.
#[inline]
pub fn find_id_linear(ids: &[u64], target: u64) -> Option<usize> {
    ids.iter().position(|&v| v == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pict_info_roundtrip_and_validation() {
        let info = create_header(Rect::make_ltrb(1.0, 2.0, 300.0, 400.0));
        assert!(is_valid_pict_info(&info));
        let bytes = encode_pict_info(&info);
        let decoded = decode_pict_info(&bytes);
        assert_eq!(info, decoded);

        let mut bad_magic = info;
        bad_magic.magic[0] = b'X';
        assert!(!is_valid_pict_info(&bad_magic));

        let mut old_ver = info;
        old_ver.version = SKP_MIN_VERSION - 1;
        assert!(!is_valid_pict_info(&old_ver));

        let mut future_ver = info;
        future_ver.version = SKP_CURRENT_VERSION + 1;
        assert!(!is_valid_pict_info(&future_ver));
    }

    #[test]
    fn test_op_header_and_clip_params() {
        let h = unpack_op_word((30 << 24) | 4);
        assert!(h.valid && !h.needs_extra_size && h.op == 30 && h.size == 4);

        let big = unpack_op_word((30 << 24) | MASK_24);
        assert!(big.valid && big.needs_extra_size && big.op == 30);
        assert!(validate_op_and_size(30, 0x0100_0000));
        assert!(!validate_op_and_size(0, 4));
        assert!(!validate_op_and_size(DRAW_TYPE_LAST + 1, 4));
        assert!(!validate_op_and_size(30, 0));

        let packed = clip_params_pack(1, true);
        assert!(clip_params_unpack_do_aa(packed));
        let u = clip_params_unpack_region_op(packed, SKP_CURRENT_VERSION);
        assert!(u.valid && u.region_op == 1);

        // Replace op (5) valid on old version (< 89), invalid on >= 89
        let packed_replace = clip_params_pack(REGION_REPLACE_OP, false);
        assert!(clip_params_unpack_region_op(packed_replace, 88).valid);
        assert!(!clip_params_unpack_region_op(packed_replace, 89).valid);
        assert_eq!(
            map_region_op_to_clip_action(REGION_REPLACE_OP),
            ClipOpAction::ResetThenIntersect
        );
    }

    #[test]
    fn test_flat_layouts_and_helpers() {
        assert_eq!(negate_to_size_t(-10), 10);
        assert_eq!(negate_to_size_t(i32::MIN), 0x8000_0000);
        assert_eq!(pad32_bytes(0), 0);
        assert_eq!(pad32_bytes(1), 3);
        assert_eq!(pad32_bytes(2), 2);
        assert_eq!(pad32_bytes(3), 1);
        assert_eq!(pad32_bytes(4), 0);

        assert_eq!(compute_factory_chunk_size(&[]), 4);
        assert_eq!(compute_factory_chunk_size(&[0, 5]), 4 + 1 + 1 + 5);

        let sl = compute_save_layer_flat_layout(true, true, false, 0, 0.5, 2, false);
        assert_eq!(
            sl.flat_flags,
            SAVELAYERREC_HAS_BOUNDS
                | SAVELAYERREC_HAS_PAINT
                | SAVELAYERREC_HAS_BACKDROP_SCALE
                | SAVELAYERREC_HAS_MULTIPLE_FILTERS
                | SAVELAYERREC_HAS_BACKDROP_TILEMODE
        );
        assert_eq!(sl.total_bytes, 8 + 16 + 4 + 4 + 12 + 4);

        let dp = compute_draw_patch_flat_layout(true, true, false);
        assert_eq!(
            dp.flags,
            DRAW_VERTICES_HAS_COLORS | DRAW_VERTICES_HAS_TEXS | DRAW_VERTICES_HAS_XFER
        );
        assert_eq!(dp.total_bytes, 8 + 96 + 4 + 16 + 32 + 4);

        let da = compute_draw_atlas_flat_layout(3, true, true, 12);
        assert_eq!(
            da.flags,
            DRAW_ATLAS_HAS_SAMPLING | DRAW_ATLAS_HAS_COLORS | DRAW_ATLAS_HAS_CULL
        );
        assert_eq!(da.total_bytes, 20 + 48 + 48 + 12 + 12 + 4 + 16);

        assert!(validate_offset_to_restore(0, 100));
        assert!(validate_offset_to_restore(104, 100));
        assert!(!validate_offset_to_restore(102, 100));
        assert!(!validate_offset_to_restore(96, 100));
    }

    #[test]
    fn test_picture_id_and_bbh_union() {
        let id1 = next_picture_id();
        let id2 = next_picture_id();
        assert_ne!(id1, 0);
        assert_ne!(id2, 0);
        assert_ne!(id1, id2);

        let u = compute_bbh_union(&[
            Rect::make_ltrb(0.0, 0.0, 10.0, 10.0),
            Rect::make_ltrb(5.0, -5.0, 20.0, 5.0),
        ]);
        assert_eq!(u, Rect::make_ltrb(0.0, -5.0, 20.0, 10.0));
    }
}
