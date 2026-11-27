//! Safe Rust port of `src/codec/SkPixmapUtils.cpp`.

use skia_rs_color::image_info::ColorType;
use skia_rs_geometry::ISize;

use crate::types::EncodedOrigin;

/// Action decided by `validate_orient` for `SkPixmapUtils::Orient`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OrientAction {
    /// Invalid parameters or aliased non-identity orientation -> return `false`.
    Invalid = 0,
    /// Empty dimensions or aliased `TopLeft` identity -> return `true` without drawing.
    NoOp = 1,
    /// Perform orientation draw into destination pixmap.
    Draw = 2,
}

/// Exact port of `SkPixmapUtils::Orient` validation logic.
pub const fn validate_orient(
    src_ct: ColorType,
    dst_ct: ColorType,
    src_w: i32,
    src_h: i32,
    dst_w: i32,
    dst_h: i32,
    origin: EncodedOrigin,
    same_addr: bool,
) -> OrientAction {
    if (src_ct as i32) != (dst_ct as i32) {
        return OrientAction::Invalid;
    }

    let (w, h) = if origin.swaps_width_height() {
        (src_h, src_w)
    } else {
        (src_w, src_h)
    };

    if dst_w != w || dst_h != h {
        return OrientAction::Invalid;
    }

    if w == 0 || h == 0 {
        return OrientAction::NoOp;
    }

    if same_addr {
        return match origin {
            EncodedOrigin::TopLeft => OrientAction::NoOp,
            _ => OrientAction::Invalid,
        };
    }

    OrientAction::Draw
}

/// Exact port of `SkPixmapUtils::SwapWidthHeight` dimension swap.
#[inline]
pub const fn swap_wh(size: ISize) -> ISize {
    ISize {
        width: size.height,
        height: size.width,
    }
}
