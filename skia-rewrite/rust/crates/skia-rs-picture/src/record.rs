//! Safe Rust implementation of `SkRecord`, `SkRecords` tags/types, and `SkRecordCanvas` helpers
//! (`src/core/SkRecord.cpp`, `src/core/SkRecords.cpp`, `src/core/SkRecordCanvas.cpp`).

use skia_rs_geometry::{IRect, Rect};

/// Matches `SK_MaxS32FitsInFloat` in `include/private/SkFloatingPoint.h`.
pub const SK_MAX_S32_FITS_IN_FLOAT: i32 = 2147483520;

/// Matches `SkRecords::Tags`.
pub const TAG_DRAW: u32 = 1;
pub const TAG_HAS_IMAGE: u32 = 2;
pub const TAG_HAS_TEXT: u32 = 4;
pub const TAG_HAS_PAINT: u32 = 8;
pub const TAG_MULTI_DRAW: u32 = 16;

/// Matches `SkRecords::Type` in `src/core/SkRecords.h`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordType {
    NoOp = 0,
    Restore = 1,
    Save = 2,
    SaveLayer = 3,
    SaveBehind = 4,
    SetMatrix = 5,
    SetM44 = 6,
    Translate = 7,
    Scale = 8,
    Concat = 9,
    Concat44 = 10,
    ClipPath = 11,
    ClipRRect = 12,
    ClipRect = 13,
    ClipRegion = 14,
    ClipShader = 15,
    ResetClip = 16,
    DrawArc = 17,
    DrawDrawable = 18,
    DrawImage = 19,
    DrawImageLattice = 20,
    DrawImageRect = 21,
    DrawDRRect = 22,
    DrawOval = 23,
    DrawBehind = 24,
    DrawPaint = 25,
    DrawPath = 26,
    DrawPatch = 27,
    DrawPicture = 28,
    DrawPoints = 29,
    DrawRRect = 30,
    DrawRect = 31,
    DrawRegion = 32,
    DrawTextBlob = 33,
    DrawSlug = 34,
    DrawAtlas = 35,
    DrawVertices = 36,
    DrawMesh = 37,
    DrawShadowRec = 38,
    DrawAnnotation = 39,
    DrawEdgeAAQuad = 40,
    DrawEdgeAAImageSet = 41,
}

pub const RECORD_TYPE_COUNT: usize = 42;

/// Returns the `SkRecords::Tags` bitmask for a given `SkRecords::Type` integer value.
#[inline]
pub const fn record_type_tags(op_type: u32) -> u32 {
    match op_type {
        3 => TAG_HAS_PAINT, // SaveLayer
        17 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 30 | 31 | 32 => TAG_DRAW | TAG_HAS_PAINT,
        18 | 38 | 40 => TAG_DRAW, // DrawDrawable, DrawShadowRec, DrawEdgeAAQuad
        19 | 20 | 21 => TAG_DRAW | TAG_HAS_IMAGE | TAG_HAS_PAINT, // DrawImage*
        29 | 36 | 37 => TAG_DRAW | TAG_HAS_PAINT | TAG_MULTI_DRAW, // DrawPoints, DrawVertices, DrawMesh
        33 | 34 => TAG_DRAW | TAG_HAS_TEXT | TAG_HAS_PAINT, // DrawTextBlob, DrawSlug
        35 | 41 => TAG_DRAW | TAG_HAS_IMAGE | TAG_HAS_PAINT | TAG_MULTI_DRAW, // DrawAtlas, DrawEdgeAAImageSet
        _ => 0,
    }
}

#[inline]
pub const fn is_draw_type(op_type: u32) -> bool {
    (record_type_tags(op_type) & TAG_DRAW) != 0
}

#[inline]
pub const fn is_single_draw_type(op_type: u32) -> bool {
    let tags = record_type_tags(op_type);
    (tags & TAG_DRAW) != 0 && (tags & TAG_MULTI_DRAW) == 0
}

/// Binary layout matching `SkRecord::Record` (`SkRecords::Type fType; void* fPtr;`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordSlot {
    pub op_type: u32,
    pub ptr: usize,
}

/// Matches `SkRecord::grow()`: `fReserved ? fReserved * 2 : 4`.
#[inline]
pub const fn record_next_capacity(reserved: i32) -> i32 {
    if reserved != 0 {
        reserved * 2
    } else {
        4
    }
}

/// Matches `SkRecord::bytesUsed()`.
#[inline]
pub const fn record_bytes_used(approx_bytes_allocated: usize, sizeof_record: usize) -> usize {
    approx_bytes_allocated + sizeof_record
}

/// Matches `SkRecord::defrag()`: stably removes all `NoOp` slots and returns the new count.
pub fn record_defrag(slots: &mut [RecordSlot]) -> usize {
    let mut write = 0usize;
    for read in 0..slots.len() {
        if slots[read].op_type != RecordType::NoOp as u32 {
            if write != read {
                slots[write] = slots[read];
            }
            write += 1;
        }
    }
    write
}

/// Matches `safe_picture_bounds(const SkRect& bounds)` in `SkRecordCanvas.cpp`.
pub fn safe_picture_bounds(bounds: Rect) -> IRect {
    let mut pic_bounds = bounds.round_out();
    const SAFE_EDGE: i32 = SK_MAX_S32_FITS_IN_FLOAT / 2 - 1;
    let safe_bounds = IRect::make_ltrb(-SAFE_EDGE, -SAFE_EDGE, SAFE_EDGE, SAFE_EDGE);
    let mut result = IRect::make_empty();
    if !result.intersect_pair(pic_bounds, safe_bounds) {
        pic_bounds = IRect::make_empty();
    } else {
        pic_bounds = result;
    }
    pic_bounds
}

/// Matches `flagCount` computation in `SkRecordCanvas::onDrawImageLattice2`.
#[inline]
pub const fn lattice_flag_count(has_rect_types: bool, x_count: i32, y_count: i32) -> i32 {
    if has_rect_types {
        (x_count + 1) * (y_count + 1)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_capacity_and_defrag() {
        assert_eq!(record_next_capacity(0), 4);
        assert_eq!(record_next_capacity(4), 8);
        assert_eq!(record_next_capacity(16), 32);

        let mut slots = [
            RecordSlot { op_type: RecordType::Save as u32, ptr: 1 },
            RecordSlot { op_type: RecordType::NoOp as u32, ptr: 0 },
            RecordSlot { op_type: RecordType::DrawRect as u32, ptr: 2 },
            RecordSlot { op_type: RecordType::NoOp as u32, ptr: 0 },
            RecordSlot { op_type: RecordType::Restore as u32, ptr: 3 },
        ];
        let new_count = record_defrag(&mut slots);
        assert_eq!(new_count, 3);
        assert_eq!(slots[0].op_type, RecordType::Save as u32);
        assert_eq!(slots[1].op_type, RecordType::DrawRect as u32);
        assert_eq!(slots[2].op_type, RecordType::Restore as u32);
    }

    #[test]
    fn test_record_tags() {
        assert!(!is_draw_type(RecordType::Save as u32));
        assert!(!is_draw_type(RecordType::SaveLayer as u32));
        assert!(!is_draw_type(RecordType::DrawAnnotation as u32));
        assert!(is_draw_type(RecordType::DrawRect as u32));
        assert!(is_single_draw_type(RecordType::DrawRect as u32));
        assert!(is_draw_type(RecordType::DrawPoints as u32));
        assert!(!is_single_draw_type(RecordType::DrawPoints as u32));
        assert!(!is_single_draw_type(RecordType::DrawAtlas as u32));
        assert!(!is_single_draw_type(RecordType::DrawVertices as u32));
        assert!(!is_single_draw_type(RecordType::DrawMesh as u32));
        assert!(!is_single_draw_type(RecordType::DrawEdgeAAImageSet as u32));
    }

    #[test]
    fn test_safe_picture_bounds() {
        let normal = safe_picture_bounds(Rect::make_ltrb(0.5, 1.2, 10.7, 20.1));
        assert_eq!(normal, IRect::make_ltrb(0, 1, 11, 21));

        let huge = safe_picture_bounds(Rect::make_ltrb(-1e30, -1e30, 1e30, 1e30));
        assert!(huge.width() > 0 && huge.height() > 0);
        let safe_edge = SK_MAX_S32_FITS_IN_FLOAT / 2 - 1;
        assert_eq!(huge, IRect::make_ltrb(-safe_edge, -safe_edge, safe_edge, safe_edge));
    }
}
