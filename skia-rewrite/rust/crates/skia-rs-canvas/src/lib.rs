#![forbid(unsafe_code)]

//! Safe Rust implementations for Phase 1 Layer 5.3 (`skia-rs-canvas`):
//! drawing pipeline (`SkDraw*`), devices (`SkDevice`, `SkBitmapDevice`,
//! `SkClipStackDevice`, `SkTiledImageUtils`), canvas (`SkCanvas`,
//! `SkCanvas_Raster`, `SkCanvasPriv`, `SkOverdrawCanvas`), pixel read/write
//! records and storage (`SkAutoPixmapStorage`, `SkReadPixelsRec`,
//! `SkWritePixelsRec`, `SkPixelStorage`, `SkContext`/`SkRecorder`),
//! mesh/document helpers (`SkMesh`, `SkDocument`, `SkCapabilities`,
//! `SkGraphics`), and canvas utilities (`SkNWayCanvas`, `SkNullCanvas`,
//! `SkPaintFilterCanvas`, `SkCanvasStack`, `SkCanvasStateUtils`).

pub mod canvas;
pub mod canvas_utils;
pub mod device;
pub mod draw;
pub mod mesh_doc;
pub mod pixel_recs;

/// `SkClipOp` discriminants matching `include/core/SkClipOp.h`.
pub const CLIP_OP_DIFFERENCE: i32 = 0;
pub const CLIP_OP_INTERSECT: i32 = 1;

/// `SkPaint::Style` discriminants matching `include/core/SkPaint.h`.
pub const PAINT_STYLE_FILL: i32 = 0;
pub const PAINT_STYLE_STROKE: i32 = 1;
pub const PAINT_STYLE_STROKE_AND_FILL: i32 = 2;

/// `SkPaint::Cap` discriminants matching `include/core/SkPaint.h`.
pub const PAINT_CAP_BUTT: i32 = 0;
pub const PAINT_CAP_ROUND: i32 = 1;
pub const PAINT_CAP_SQUARE: i32 = 2;

/// `SkPaint::Join` discriminants matching `include/core/SkPaint.h`.
pub const PAINT_JOIN_MITER: i32 = 0;
pub const PAINT_JOIN_ROUND: i32 = 1;
pub const PAINT_JOIN_BEVEL: i32 = 2;

/// `SkCanvas::PointMode` discriminants matching `include/core/SkCanvas.h`.
pub const POINT_MODE_POINTS: i32 = 0;
pub const POINT_MODE_LINES: i32 = 1;
pub const POINT_MODE_POLYGON: i32 = 2;

/// `SkColorType` constants matching `include/core/SkColorType.h`.
pub const COLOR_TYPE_UNKNOWN: i32 = 0;
pub const COLOR_TYPE_ALPHA_8: i32 = 1;
pub const COLOR_TYPE_RGB_565: i32 = 2;
pub const COLOR_TYPE_RGBA_8888: i32 = 4;
pub const COLOR_TYPE_BGRA_8888: i32 = 6;

/// `SkAlphaType` constants matching `include/core/SkAlphaType.h`.
pub const ALPHA_TYPE_UNKNOWN: i32 = 0;
pub const ALPHA_TYPE_OPAQUE: i32 = 1;
pub const ALPHA_TYPE_PREMUL: i32 = 2;

/// `SkBlendMode` constants matching `include/core/SkBlendMode.h`.
pub const BLEND_MODE_SRC: i32 = 1;
pub const BLEND_MODE_DST: i32 = 2;
pub const BLEND_MODE_SRC_OVER: i32 = 3;

/// `SK_ScalarSqrt2` matching `include/core/SkScalar.h`.
pub const SK_SCALAR_SQRT2: f32 = 1.41421356;

/// `SK_FixedMax` / `SK_FixedMin` bounds check matching `SkRectPriv::FitsInFixed`.
pub const SK_FIXED_MAX_FLOAT: f32 = 32767.0;

/// Checks `SkRectPriv::FitsInFixed(r)`:
/// `r.left >= -32767 && r.top >= -32767 && r.right <= 32767 && r.bottom <= 32767`
/// (exact C++ `SkScalarAbs(x) <= 32767.0f` on all four edges, NaN returns false).
#[inline]
pub fn rect_fits_in_fixed(r: skia_rs_geometry::Rect) -> bool {
    const LIMIT: f32 = 32767.0;
    r.left.abs() <= LIMIT
        && r.top.abs() <= LIMIT
        && r.right.abs() <= LIMIT
        && r.bottom.abs() <= LIMIT
}

/// Checks `SkPathPriv::TooBigForMath(r)` — matches `SkPathPriv::TooBigForMath(SkRect)`.
/// Uses `!(left >= -max && top >= -max && right <= max && bottom <= max)` where
/// `max = SK_ScalarMax * 0.25f` so NaN returns `true`.
#[inline]
pub fn rect_too_big_for_math(r: skia_rs_geometry::Rect) -> bool {
    const SK_SCALAR_MAX: f32 = 3.402823466e+38_f32;
    const MAX: f32 = SK_SCALAR_MAX * 0.25_f32;
    !(r.left >= -MAX && r.top >= -MAX && r.right <= MAX && r.bottom <= MAX)
}

/// Checks `SkMatrixPriv::IsScaleTranslateAsM33(m)`.
#[inline]
pub fn is_scale_translate_as_m33(m: &skia_rs_geometry::M44) -> bool {
    m.rc(1, 0) == 0.0
        && m.rc(3, 0) == 0.0
        && m.rc(0, 1) == 0.0
        && m.rc(3, 1) == 0.0
        && m.rc(3, 3) == 1.0
}

/// Converts an `M44` to a 3x3 `Matrix` matching `SkM44::asM33()`.
#[inline]
pub fn m44_as_m33(m: &skia_rs_geometry::M44) -> skia_rs_geometry::Matrix {
    let mut out = skia_rs_geometry::Matrix::identity();
    out.set9(&[
        m.rc(0, 0),
        m.rc(0, 1),
        m.rc(0, 3),
        m.rc(1, 0),
        m.rc(1, 1),
        m.rc(1, 3),
        m.rc(3, 0),
        m.rc(3, 1),
        m.rc(3, 3),
    ]);
    out
}

/// Converts a 3x3 `Matrix` to `M44` matching `SkM44(const SkMatrix&)`.
#[inline]
pub fn m44_from_m33(m: &skia_rs_geometry::Matrix) -> skia_rs_geometry::M44 {
    skia_rs_geometry::M44 {
        mat: [
            m.mat[0], m.mat[3], 0.0, m.mat[6],
            m.mat[1], m.mat[4], 0.0, m.mat[7],
            0.0,      0.0,      1.0, 0.0,
            m.mat[2], m.mat[5], 0.0, m.mat[8],
        ],
    }
}

/// Checks `is_int(float x)` matching `x == (float)sk_float_round2int(x)` in `SkDevice.cpp`.
#[inline]
pub fn is_int_f32(x: f32) -> bool {
    x == (skia_rs_geometry::scalar_round_to_int(x) as f32)
}

/// Checks `SkScalarIsInt(x)` matching `x == floorf(x)`.
#[inline]
pub fn scalar_is_int(x: f32) -> bool {
    x == x.floor()
}

/// Matching `sk_ieee_float_divide(numer, denom)`.
#[inline]
pub fn ieee_float_divide(numer: f32, denom: f32) -> f32 {
    numer / denom
}

/// Matching `Sk64_pin_to_s32(int64_t x)`.
#[inline]
pub fn pin_i64_to_i32(x: i64) -> i32 {
    if x < i32::MIN as i64 {
        i32::MIN
    } else if x > i32::MAX as i64 {
        i32::MAX
    } else {
        x as i32
    }
}

/// Matching `SkScalarToFixed(x)` -> `sk_float_saturate2int(x * 65536.0f)`.
#[inline]
pub fn scalar_to_fixed(x: f32) -> i32 {
    skia_rs_geometry::float_saturate2int(x * 65536.0_f32)
}

/// Matching `SkIRect::MakeXYWH(x, y, w, h)` using saturating 32-bit addition.
#[inline]
pub fn irect_from_xywh(x: i32, y: i32, w: i32, h: i32) -> skia_rs_geometry::IRect {
    skia_rs_geometry::IRect::from_ltrb(x, y, x.saturating_add(w), y.saturating_add(h))
}

/// Matching `SkRect::MakeWH(w, h)`.
#[inline]
pub fn rect_from_wh(w: f32, h: f32) -> skia_rs_geometry::Rect {
    skia_rs_geometry::Rect::from_ltrb(0.0, 0.0, w, h)
}

/// Matching `SkRect::Make(const SkIRect&)`.
#[inline]
pub fn rect_from_irect(ir: skia_rs_geometry::IRect) -> skia_rs_geometry::Rect {
    skia_rs_geometry::Rect::from_ltrb(
        ir.left as f32,
        ir.top as f32,
        ir.right as f32,
        ir.bottom as f32,
    )
}

/// Matching `SkMatrix::mapRect(const SkRect&)`.
#[inline]
pub fn matrix_map_rect(m: &skia_rs_geometry::Matrix, src: skia_rs_geometry::Rect) -> skia_rs_geometry::Rect {
    if let Some((dst, _)) = m.map_rect_non_perspective(src) {
        dst
    } else {
        m44_from_m33(m).map_rect(src)
    }
}

