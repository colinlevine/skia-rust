//! Safe Rust port of `src/core/SkGlyph.cpp` (`SkPackedGlyphID`, `SkGlyph`, `SkGlyphDigest`,
//! `SkGlyphPositionRoundingSpec`, and horizontal path-gap / intercept calculation).

use crate::{four_byte_tag, AxisAlignment, MaskFormat};
use skia_rs_geometry::{cpp_max_f32, cpp_min_f32, IPoint, Point, Vector};
use skia_rs_math::bezier::{cubic_intersect_with_horizontal_line, quad_intersect_with_horizontal_line};

/// Mirrors `SkPackedGlyphID` constants.
pub const PACKED_GLYPH_ID_LEN: u32 = 16;
pub const PACKED_SUBPIXEL_POS_LEN: u32 = 2;
pub const PACKED_SUBPIXEL_X: u32 = 0;
pub const PACKED_GLYPH_ID_SHIFT: u32 = PACKED_SUBPIXEL_POS_LEN;
pub const PACKED_SUBPIXEL_Y: u32 = PACKED_GLYPH_ID_LEN + PACKED_SUBPIXEL_POS_LEN;
pub const PACKED_END_DATA: u32 = PACKED_GLYPH_ID_LEN + 2 * PACKED_SUBPIXEL_POS_LEN;
pub const PACKED_GLYPH_ID_MASK: u32 = (1u32 << PACKED_GLYPH_ID_LEN) - 1;
pub const PACKED_SUBPIXEL_POS_MASK: u32 = (1u32 << PACKED_SUBPIXEL_POS_LEN) - 1;
pub const PACKED_MASK_ALL: u32 = (1u32 << PACKED_END_DATA) - 1;
pub const PACKED_FIXED_SUBPIXEL_POS_BITS: u32 = 16 - PACKED_SUBPIXEL_POS_LEN;

pub const SUBPIXEL_ROUND: f32 = 1.0_f32 / ((1u32 << (PACKED_SUBPIXEL_POS_LEN + 1)) as f32);
pub const XY_FIELD_MASK: IPoint = IPoint {
    x: (PACKED_SUBPIXEL_POS_MASK << PACKED_SUBPIXEL_X) as i32,
    y: (PACKED_SUBPIXEL_POS_MASK << PACKED_SUBPIXEL_Y) as i32,
};

/// Mirrors `SkGlyph::kMaxGlyphWidth`.
pub const MAX_GLYPH_WIDTH: u16 = 1u16 << 13;

/// Mirrors `SkGlyphDigest::kSkSideTooBigForAtlas`.
pub const SIDE_TOO_BIG_FOR_ATLAS: u16 = 256;

/// Mirrors `SkPackedGlyphID::PackIDSkPoint`.
#[inline]
pub fn pack_glyph_id_point(glyph_id: u16, pt: Point, mask: IPoint) -> u32 {
    let magic_x = (1u32 << (PACKED_SUBPIXEL_POS_LEN + PACKED_SUBPIXEL_X)) as f32;
    let magic_y = (1u32 << (PACKED_SUBPIXEL_POS_LEN + PACKED_SUBPIXEL_Y)) as f32;

    let x = (pt.x - pt.x.floor()) + 1.0_f32;
    let y = (pt.y - pt.y.floor()) + 1.0_f32;
    let sub_x = ((x * magic_x) as i32) & mask.x;
    let sub_y = ((y * magic_y) as i32) & mask.y;
    ((glyph_id as u32) << PACKED_GLYPH_ID_SHIFT) | (sub_x as u32) | (sub_y as u32)
}

/// Mirrors `SkMask::Format` alignment in `SkGlyph.cpp` (`format_alignment`).
#[inline]
pub const fn format_alignment(format: MaskFormat) -> usize {
    match format {
        MaskFormat::BW | MaskFormat::A8 | MaskFormat::Format3D | MaskFormat::SDF => 1,
        MaskFormat::ARGB32 => 4,
        MaskFormat::LCD16 => 2,
    }
}

/// Mirrors `bits_to_bytes` in `SkGlyph.cpp`.
#[inline]
pub const fn bits_to_bytes(bits: usize) -> usize {
    (bits + 7) >> 3
}

/// Mirrors `format_rowbytes` in `SkGlyph.cpp`.
#[inline]
pub const fn format_row_bytes(width: i32, format: MaskFormat) -> usize {
    if width <= 0 {
        return 0;
    }
    let w = width as usize;
    match format {
        MaskFormat::BW => bits_to_bytes(w),
        _ => w * format_alignment(format),
    }
}

/// Mirrors `SkGlyph::imageSize`.
#[inline]
pub const fn glyph_image_size(width: u16, height: u16, format: MaskFormat) -> usize {
    if width == 0 || height == 0 || width >= MAX_GLYPH_WIDTH {
        return 0;
    }
    let mut size = format_row_bytes(width as i32, format) * (height as usize);
    if matches!(format, MaskFormat::Format3D) {
        size *= 3;
    }
    size
}

/// Mirrors `SkGlyph::flattenMetrics` / `SkGlyph::MakeFromBuffer` packed fields.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PackedGlyphMetrics {
    pub packed_id: u32,
    pub advance_x: f32,
    pub advance_y: f32,
    pub dimensions: u32,
    pub left_top: u32,
    pub mask_format: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct UnpackedGlyphMetrics {
    pub packed_id: u32,
    pub advance_x: f32,
    pub advance_y: f32,
    pub width: u16,
    pub height: u16,
    pub left: i16,
    pub top: i16,
    pub mask_format: u8,
}

/// Packs glyph metrics for `SkGlyph::flattenMetrics`.
#[inline]
pub const fn pack_glyph_metrics(
    packed_id: u32,
    advance_x: f32,
    advance_y: f32,
    width: u16,
    height: u16,
    left: i16,
    top: i16,
    mask_format: u8,
) -> PackedGlyphMetrics {
    let dimensions = ((width as u32) << 16) | (height as u32);
    let left_u = (left as u16) as u32;
    let top_u = (top as u16) as u32;
    let left_top = (left_u << 16) | top_u;
    PackedGlyphMetrics {
        packed_id,
        advance_x,
        advance_y,
        dimensions,
        left_top,
        mask_format: mask_format as u32,
    }
}

/// Unpacks glyph metrics for `SkGlyph::MakeFromBuffer`. Returns `None` if `mask_format` is invalid.
#[inline]
pub const fn unpack_glyph_metrics(m: PackedGlyphMetrics) -> Option<UnpackedGlyphMetrics> {
    if m.mask_format >= crate::COUNT_MASK_FORMATS as u32 {
        return None;
    }
    Some(UnpackedGlyphMetrics {
        packed_id: m.packed_id & PACKED_MASK_ALL,
        advance_x: m.advance_x,
        advance_y: m.advance_y,
        width: (m.dimensions >> 16) as u16,
        height: (m.dimensions & 0xFFFF) as u16,
        left: (m.left_top >> 16) as u16 as i16,
        top: (m.left_top & 0xFFFF) as u16 as i16,
        mask_format: m.mask_format as u8,
    })
}

/// Mirrors `SkPictureBackedGlyphDrawable::MakeFromBuffer` allowed picture tags.
pub fn is_allowed_glyph_drawable_tag(tag: u32) -> bool {
    const PICT_BUFFER_SIZE_TAG: u32 = four_byte_tag(b'a', b'r', b'a', b'y');
    const PICT_FACTORY_TAG: u32 = four_byte_tag(b'f', b'a', b'c', b't');
    const PICT_PAINT_BUFFER_TAG: u32 = four_byte_tag(b'p', b'n', b't', b' ');
    const PICT_PATH_BUFFER_TAG: u32 = four_byte_tag(b'p', b't', b'h', b' ');
    const PICT_READER_TAG: u32 = four_byte_tag(b'r', b'e', b'a', b'd');

    matches!(
        tag,
        PICT_BUFFER_SIZE_TAG
            | PICT_FACTORY_TAG
            | PICT_PAINT_BUFFER_TAG
            | PICT_PATH_BUFFER_TAG
            | PICT_READER_TAG
    )
}

/// Mirrors `skglyph::GlyphAction`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GlyphAction {
    Unset = 0,
    Accept = 1,
    Reject = 2,
    Drop = 3,
}

/// Mirrors `skglyph::ActionType` bit shift offsets.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActionType {
    DirectMask = 0,
    DirectMaskCpu = 2,
    Mask = 4,
    Sdft = 6,
    Path = 8,
    Drawable = 10,
}

impl ActionType {
    #[inline]
    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::DirectMask),
            2 => Some(Self::DirectMaskCpu),
            4 => Some(Self::Mask),
            6 => Some(Self::Sdft),
            8 => Some(Self::Path),
            10 => Some(Self::Drawable),
            _ => None,
        }
    }
}

/// Mirrors `init_actions` in `SkGlyph.cpp`.
#[inline]
pub const fn init_digest_actions(is_empty: bool) -> u32 {
    if !is_empty {
        return 0;
    }
    let drop_val = GlyphAction::Drop as u32;
    (drop_val << ActionType::DirectMask as u32)
        | (drop_val << ActionType::DirectMaskCpu as u32)
        | (drop_val << ActionType::Mask as u32)
        | (drop_val << ActionType::Sdft as u32)
        | (drop_val << ActionType::Path as u32)
        | (drop_val << ActionType::Drawable as u32)
}

/// Mirrors `SkGlyphDigest::FitsInAtlas`.
#[inline]
pub const fn fits_in_atlas(width: u16, height: u16) -> bool {
    let max_dim = if width > height { width } else { height };
    max_dim <= SIDE_TOO_BIG_FOR_ATLAS
}

/// Mirrors `SkGlyphDigest::fitsInAtlasInterpolated`.
#[inline]
pub const fn fits_in_atlas_interpolated(width: u16, height: u16) -> bool {
    let max_dim = if width > height { width } else { height };
    max_dim <= SIDE_TOO_BIG_FOR_ATLAS - 2
}

/// Mirrors `SkGlyphPositionRoundingSpec` in `src/core/SkGlyph.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GlyphPositionRoundingSpec {
    pub half_axis_sample_freq: Vector,
    pub ignore_position_mask: IPoint,
    pub ignore_position_field_mask: IPoint,
}

impl GlyphPositionRoundingSpec {
    pub fn half_axis_sample_freq(is_subpixel: bool, axis_alignment: AxisAlignment) -> Vector {
        if !is_subpixel {
            Vector::new(0.5_f32, 0.5_f32)
        } else {
            match axis_alignment {
                AxisAlignment::X => Vector::new(SUBPIXEL_ROUND, 0.5_f32),
                AxisAlignment::Y => Vector::new(0.5_f32, SUBPIXEL_ROUND),
                AxisAlignment::None => Vector::new(SUBPIXEL_ROUND, SUBPIXEL_ROUND),
            }
        }
    }

    pub const fn ignore_position_mask(is_subpixel: bool, axis_alignment: AxisAlignment) -> IPoint {
        let x = if !is_subpixel || matches!(axis_alignment, AxisAlignment::Y) {
            0
        } else {
            !0i32
        };
        let y = if !is_subpixel || matches!(axis_alignment, AxisAlignment::X) {
            0
        } else {
            !0i32
        };
        IPoint { x, y }
    }

    pub const fn ignore_position_field_mask(
        is_subpixel: bool,
        axis_alignment: AxisAlignment,
    ) -> IPoint {
        let m = Self::ignore_position_mask(is_subpixel, axis_alignment);
        IPoint {
            x: m.x & XY_FIELD_MASK.x,
            y: m.y & XY_FIELD_MASK.y,
        }
    }

    pub fn new(is_subpixel: bool, axis_alignment: AxisAlignment) -> Self {
        Self {
            half_axis_sample_freq: Self::half_axis_sample_freq(is_subpixel, axis_alignment),
            ignore_position_mask: Self::ignore_position_mask(is_subpixel, axis_alignment),
            ignore_position_field_mask: Self::ignore_position_field_mask(
                is_subpixel,
                axis_alignment,
            ),
        }
    }
}

/// Verb codes matching `SkPathVerb` (`kMove=0, kLine=1, kQuad=2, kConic=3, kCubic=4, kClose=5`).
pub const VERB_MOVE: u8 = 0;
pub const VERB_LINE: u8 = 1;
pub const VERB_QUAD: u8 = 2;
pub const VERB_CONIC: u8 = 3;
pub const VERB_CUBIC: u8 = 4;
pub const VERB_CLOSE: u8 = 5;

/// Segment input passed from `SkPath::Iter` (each segment has a verb and its 2..4 points).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PathSegment {
    pub verb: u8,
    pub _pad: [u8; 3],
    pub pts: [Point; 4],
}

/// Mirrors `calculate_path_gap` in `src/core/SkGlyph.cpp`.
/// Computes `[left, right]` interval of intersections of a glyph outline with horizontal band
/// `[top_offset, bottom_offset]`. Returns `[SK_ScalarMax, SK_ScalarMin]` if no intersection.
pub fn calculate_path_gap(top_offset: f32, bottom_offset: f32, segments: &[PathSegment]) -> [f32; 2] {
    // SK_ScalarMax = 3.402823466e+38f == f32::MAX, SK_ScalarMin = -SK_ScalarMax == -f32::MAX
    let mut left = f32::MAX;
    let mut right = -f32::MAX;

    let expand_gap = |v: f32, left: &mut f32, right: &mut f32| {
        *left = cpp_min_f32(*left, v);
        *right = cpp_max_f32(*right, v);
    };

    let add_line = |pts: &[Point], offset: f32, left: &mut f32, right: &mut f32| {
        let t = (offset - pts[0].y) / (pts[1].y - pts[0].y);
        if 0.0 <= t && t < 1.0 {
            expand_gap(pts[0].x + t * (pts[1].x - pts[0].x), left, right);
        }
    };

    let add_quad = |pts: &[Point], offset: f32, left: &mut f32, right: &mut f32| {
        let mut storage = [0.0f32; 2];
        let qpts = [
            [pts[0].x, pts[0].y],
            [pts[1].x, pts[1].y],
            [pts[2].x, pts[2].y],
        ];
        let n = quad_intersect_with_horizontal_line(&qpts, offset, &mut storage);
        for &x in &storage[..n] {
            expand_gap(x, left, right);
        }
    };

    let add_cubic = |pts: &[Point], offset: f32, left: &mut f32, right: &mut f32| {
        let mut storage = [0.0f32; 3];
        let cpts = [
            [pts[0].x, pts[0].y],
            [pts[1].x, pts[1].y],
            [pts[2].x, pts[2].y],
            [pts[3].x, pts[3].y],
        ];
        let n = cubic_intersect_with_horizontal_line(&cpts, offset, &mut storage);
        for &x in &storage[..n] {
            expand_gap(x, left, right);
        }
    };

    let add_pts = |pts: &[Point], left: &mut f32, right: &mut f32| {
        for p in pts {
            if top_offset < p.y && p.y < bottom_offset {
                expand_gap(p.x, left, right);
            }
        }
    };

    for seg in segments {
        match seg.verb {
            VERB_LINE => {
                let pts = &seg.pts[..2];
                let line_top = cpp_min_f32(pts[0].y, pts[1].y);
                let line_bottom = cpp_max_f32(pts[0].y, pts[1].y);
                if top_offset <= line_bottom && line_top <= bottom_offset {
                    add_line(pts, top_offset, &mut left, &mut right);
                    add_line(pts, bottom_offset, &mut left, &mut right);
                    add_pts(pts, &mut left, &mut right);
                }
            }
            VERB_QUAD => {
                let pts = &seg.pts[..3];
                let quad_top = cpp_min_f32(cpp_min_f32(pts[0].y, pts[1].y), pts[2].y);
                let quad_bottom = cpp_max_f32(cpp_max_f32(pts[0].y, pts[1].y), pts[2].y);
                if top_offset <= quad_bottom && quad_top <= bottom_offset {
                    add_quad(pts, top_offset, &mut left, &mut right);
                    add_quad(pts, bottom_offset, &mut left, &mut right);
                    add_pts(pts, &mut left, &mut right);
                }
            }
            VERB_CUBIC => {
                let pts = &seg.pts[..4];
                let cubic_top = cpp_min_f32(
                    cpp_min_f32(pts[0].y, pts[1].y),
                    cpp_min_f32(pts[2].y, pts[3].y),
                );
                let cubic_bottom = cpp_max_f32(
                    cpp_max_f32(pts[0].y, pts[1].y),
                    cpp_max_f32(pts[2].y, pts[3].y),
                );
                if top_offset <= cubic_bottom && cubic_top <= bottom_offset {
                    add_cubic(pts, top_offset, &mut left, &mut right);
                    add_cubic(pts, bottom_offset, &mut left, &mut right);
                    add_pts(pts, &mut left, &mut right);
                }
            }
            _ => {}
        }
    }

    if left >= right {
        [f32::MAX, -f32::MAX]
    } else {
        [left, right]
    }
}

/// Scales and offsets an intercept interval `[left, right]` by `scale` and `x_pos`.
#[inline]
pub fn offset_intercept_interval(interval: [f32; 2], scale: f32, x_pos: f32) -> [f32; 2] {
    [interval[0] * scale + x_pos, interval[1] * scale + x_pos]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_alignment_and_row_bytes() {
        assert_eq!(format_alignment(MaskFormat::BW), 1);
        assert_eq!(format_alignment(MaskFormat::A8), 1);
        assert_eq!(format_alignment(MaskFormat::LCD16), 2);
        assert_eq!(format_alignment(MaskFormat::ARGB32), 4);
        assert_eq!(format_row_bytes(9, MaskFormat::BW), 2);
        assert_eq!(format_row_bytes(8, MaskFormat::BW), 1);
        assert_eq!(format_row_bytes(10, MaskFormat::LCD16), 20);
        assert_eq!(glyph_image_size(10, 5, MaskFormat::Format3D), 150);
        assert_eq!(glyph_image_size(MAX_GLYPH_WIDTH, 5, MaskFormat::A8), 0);
    }

    #[test]
    fn test_rounding_spec() {
        let non_sub = GlyphPositionRoundingSpec::new(false, AxisAlignment::X);
        assert_eq!(non_sub.half_axis_sample_freq, Vector::new(0.5, 0.5));
        assert_eq!(non_sub.ignore_position_mask, IPoint { x: 0, y: 0 });
        assert_eq!(non_sub.ignore_position_field_mask, IPoint { x: 0, y: 0 });

        let sub_x = GlyphPositionRoundingSpec::new(true, AxisAlignment::X);
        assert_eq!(sub_x.half_axis_sample_freq, Vector::new(0.125, 0.5));
        assert_eq!(sub_x.ignore_position_mask, IPoint { x: -1, y: 0 });
        assert_eq!(sub_x.ignore_position_field_mask, IPoint { x: 3, y: 0 });
    }

    #[test]
    fn test_pack_unpack_metrics() {
        let packed = pack_glyph_metrics(0x12345, 12.5, -1.0, 24, 32, -3, -28, MaskFormat::A8 as u8);
        let unpacked = unpack_glyph_metrics(packed).unwrap();
        assert_eq!(unpacked.packed_id, 0x12345);
        assert_eq!(unpacked.advance_x, 12.5);
        assert_eq!(unpacked.advance_y, -1.0);
        assert_eq!(unpacked.width, 24);
        assert_eq!(unpacked.height, 32);
        assert_eq!(unpacked.left, -3);
        assert_eq!(unpacked.top, -28);
        assert_eq!(unpacked.mask_format, MaskFormat::A8 as u8);
    }

    #[test]
    fn test_calculate_path_gap_line() {
        let segs = [PathSegment {
            verb: VERB_LINE,
            _pad: [0; 3],
            pts: [
                Point::new(0.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 0.0),
                Point::new(0.0, 0.0),
            ],
        }];
        let [l, r] = calculate_path_gap(2.0, 6.0, &segs);
        assert!((l - 2.0).abs() < 1e-5);
        assert!((r - 6.0).abs() < 1e-5);
    }
}
