#![forbid(unsafe_code)]

//! Layer 2.3 (`skia-rs-region`): safe Rust implementations of Skia's region,
//! antialiased clip, raster clip, clip stack, and R-Tree spatial indexing
//! algorithms.

pub mod aa_clip;
pub mod clip_stack;
pub mod raster_clip;
pub mod region;
pub mod region_path;
pub mod rtree;

pub use aa_clip::{AAClip, AAClipBuilder, YOffset};
pub use clip_stack::{
    BoundsType, ClipStack, ClipStackElement, DeviceSpaceType, FillCombo, PushDecision,
    EMPTY_GEN_ID, INVALID_GEN_ID, WIDE_OPEN_GEN_ID,
};
pub use raster_clip::{nearly_integral, rect_is_nearly_integral, RasterClip, RasterClipStack};
pub use region::{
    Cliperator, Region, RegionIterator, RegionOp, Spanerator, RECT_REGION_RUNS, RUN_TYPE_SENTINEL,
};
pub use region_path::{PathCmd, RegionBuilder};
pub use rtree::{RTree, RTreeBranch, RTreeNode, MAX_CHILDREN, MIN_CHILDREN};

use skia_rs_geometry::{IRect, Rect};

/// Matches `SkClipOp` (`include/core/SkClipOp.h`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClipOp {
    Difference = 0,
    Intersect = 1,
}

impl ClipOp {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Difference),
            1 => Some(Self::Intersect),
            _ => None,
        }
    }
}

/// Matches `SkMulDiv255Round(a, b)` (`include/private/SkMath.h`).
#[inline]
pub fn mul_div_255_round(a: u32, b: u32) -> u8 {
    let prod = a * b + 128;
    ((prod + (prod >> 8)) >> 8) as u8
}

/// Matches `SkRectPriv::MakeILarge()`.
#[inline]
pub const fn make_ilarge() -> IRect {
    let large: i32 = 1 << 29;
    IRect {
        left: -large,
        top: -large,
        right: large,
        bottom: large,
    }
}

/// Matches `SkRectPriv::MakeLargeS32()`.
#[inline]
pub const fn make_large_s32() -> Rect {
    Rect::from_irect(make_ilarge())
}

/// Matches `SkIRect::contains(int32_t x, int32_t y)`.
#[inline]
pub fn irect_contains_xy(r: IRect, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// Matches `SkIRect::contains(const SkIRect& r)`.
#[inline]
pub fn irect_contains_rect(outer: IRect, inner: IRect) -> bool {
    !inner.is_empty()
        && !outer.is_empty()
        && outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Matches `SkIRect::containsNoEmptyCheck(const SkIRect& r)`.
#[inline]
pub fn irect_contains_no_empty_check(outer: IRect, inner: IRect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Matches `SkIRect::isEmpty64()`.
#[inline]
pub fn irect_is_empty_64(r: IRect) -> bool {
    r.right <= r.left || r.bottom <= r.top
}

/// Matches `SkIRect::makeOffset(int32_t dx, int32_t dy)` (32-bit saturating add as in C++).
#[inline]
pub fn irect_make_offset(r: IRect, dx: i32, dy: i32) -> IRect {
    r.make_offset(dx, dy)
}

/// Matches `SkRect::round()` (`SkScalarRoundToInt` for each coordinate).
#[inline]
pub fn rect_round(r: Rect) -> IRect {
    IRect {
        left: skia_rs_geometry::scalar_round_to_int(r.left),
        top: skia_rs_geometry::scalar_round_to_int(r.top),
        right: skia_rs_geometry::scalar_round_to_int(r.right),
        bottom: skia_rs_geometry::scalar_round_to_int(r.bottom),
    }
}

/// Matches `SkRect::roundOut()`.
#[inline]
pub fn rect_round_out(r: Rect) -> IRect {
    IRect {
        left: skia_rs_geometry::float_saturate2int(r.left.floor()),
        top: skia_rs_geometry::float_saturate2int(r.top.floor()),
        right: skia_rs_geometry::float_saturate2int(r.right.ceil()),
        bottom: skia_rs_geometry::float_saturate2int(r.bottom.ceil()),
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn ffi_repr_c_layouts() {
        assert_eq!(size_of::<IRect>(), 16);
        assert_eq!(size_of::<Rect>(), 16);
        assert_eq!(size_of::<region::RunBoundsInfo>(), 24);
        assert_eq!(size_of::<region_path::PathTileGrid>(), 28);
        assert_eq!(size_of::<region_path::PathCmd>(), 12);
        assert_eq!(size_of::<aa_clip::YOffset>(), 8);
        assert_eq!(size_of::<clip_stack::PriorBoundInfo>(), 44);
        assert_eq!(size_of::<clip_stack::BoundUpdateResult>(), 32);
        assert_eq!(size_of::<clip_stack::PushDecision>(), 20);
        assert_eq!(size_of::<rtree::RTreeBranch>(), 20);
        assert_eq!(size_of::<rtree::RTreeNode>(), 224);
    }
}
