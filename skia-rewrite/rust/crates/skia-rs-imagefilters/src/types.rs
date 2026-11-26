#![forbid(unsafe_code)]

//! Core coordinate-space, mapping, rounding, and geometric types for `skif::*`
//! (ported from `src/core/SkImageFilterTypes.cpp`).

use skia_rs_geometry::{
    float_saturate2int, scalar_nearly_equal, scalar_nearly_zero, scalar_round_to_int, IPoint,
    IRect, ISize, Matrix, Point, Rect, Size, M44, SK_SCALAR_NEARLY_ZERO,
};

/// Rounding epsilon used by `skif::RoundOut` / `skif::RoundIn` / integer-alignment checks.
pub const K_ROUND_EPSILON: f32 = 1e-3_f32;

/// Layer padding in pixels (`skif::kLayerPadding`).
pub const K_LAYER_PADDING: i32 = 1;

/// `SK_MaxS32` (`INT32_MAX`).
pub const SK_MAX_S32: i32 = i32::MAX;
/// `SK_MinS32` (`-SK_MaxS32`, note: NOT `i32::MIN`).
pub const SK_MIN_S32: i32 = -i32::MAX;

/// `SkRectPriv::MakeILarge()` half-extent (`1 << 29`).
pub const LARGE_S32: i32 = 1 << 29;

/// Exact match for `Sk64_pin_to_s32`.
#[inline]
pub fn sk64_pin_to_s32(x: i64) -> i32 {
    if x < SK_MIN_S32 as i64 {
        SK_MIN_S32
    } else if x > SK_MAX_S32 as i64 {
        SK_MAX_S32
    } else {
        x as i32
    }
}

/// Exact match for `Sk32_sat_add`.
#[inline]
pub fn sk32_sat_add(a: i32, b: i32) -> i32 {
    sk64_pin_to_s32(a as i64 + b as i64)
}

/// Exact match for `Sk32_sat_sub`.
#[inline]
pub fn sk32_sat_sub(a: i32, b: i32) -> i32 {
    sk64_pin_to_s32(a as i64 - b as i64)
}

/// Exact match for `sk_double_saturate2int`.
#[inline]
pub fn double_saturate2int(mut x: f64) -> i32 {
    if x.is_nan() {
        return SK_MAX_S32;
    }
    let max_d = SK_MAX_S32 as f64;
    let min_d = SK_MIN_S32 as f64;
    x = if x < max_d { x } else { max_d };
    x = if x > min_d { x } else { min_d };
    x as i32
}

/// Exact match for `sk_float_ceil2int`.
#[inline]
pub fn float_ceil2int(x: f32) -> i32 {
    float_saturate2int(x.ceil())
}

/// Exact match for `sk_float_floor2int`.
#[inline]
pub fn float_floor2int(x: f32) -> i32 {
    float_saturate2int(x.floor())
}

/// Exact match for `SkScalarRoundToScalar(x)` (`std::floor(x + 0.5f)`).
#[inline]
pub fn scalar_round_to_scalar(x: f32) -> f32 {
    (x + 0.5_f32).floor()
}

/// Exact match for `sk_ieee_float_divide(numer, denom)`.
#[inline]
pub fn ieee_float_divide(numer: f32, denom: f32) -> f32 {
    numer / denom
}

/// Exact match for `SkNextLog2(uint32_t value)`.
#[inline]
pub fn next_log2(value: u32) -> i32 {
    if value <= 1 {
        return 0;
    }
    (32 - (value - 1).leading_zeros()) as i32
}

/// Exact match for `SkRectPriv::MakeILarge()`.
#[inline]
pub const fn make_ilarge() -> IRect {
    IRect {
        left: -LARGE_S32,
        top: -LARGE_S32,
        right: LARGE_S32,
        bottom: LARGE_S32,
    }
}

/// Exact match for `SkRectPriv::MakeLargeS32()`.
#[inline]
pub const fn make_large_s32() -> Rect {
    Rect {
        left: -LARGE_S32 as f32,
        top: -LARGE_S32 as f32,
        right: LARGE_S32 as f32,
        bottom: LARGE_S32 as f32,
    }
}

/// Exact match for `SkIsValidRect(const SkRect&)`.
#[inline]
pub fn is_valid_rect(r: Rect) -> bool {
    r.left <= r.right && r.top <= r.bottom && r.width().is_finite() && r.height().is_finite()
}

/// Exact match for `SkIRect::isEmpty64()`.
#[inline]
pub const fn irect_is_empty64(r: IRect) -> bool {
    r.right <= r.left || r.bottom <= r.top
}

/// Exact match for `SkIRect::MakeXYWH(x, y, w, h)`.
#[inline]
pub fn irect_make_xywh(x: i32, y: i32, w: i32, h: i32) -> IRect {
    IRect {
        left: x,
        top: y,
        right: sk32_sat_add(x, w),
        bottom: sk32_sat_add(y, h),
    }
}

/// Exact match for `SkIRect::inset(dx, dy)`.
#[inline]
pub fn irect_inset(r: IRect, dx: i32, dy: i32) -> IRect {
    IRect {
        left: sk32_sat_add(r.left, dx),
        top: sk32_sat_add(r.top, dy),
        right: sk32_sat_sub(r.right, dx),
        bottom: sk32_sat_sub(r.bottom, dy),
    }
}

/// Exact match for `SkIRect::outset(dx, dy)` (`inset(-dx, -dy)`).
#[inline]
pub fn irect_outset(r: IRect, dx: i32, dy: i32) -> IRect {
    irect_inset(r, dx.wrapping_neg(), dy.wrapping_neg())
}

/// Exact match for `SkIRect::adjust(dl, dt, dr, db)`.
#[inline]
pub fn irect_adjust(r: IRect, dl: i32, dt: i32, dr: i32, db: i32) -> IRect {
    IRect {
        left: sk32_sat_add(r.left, dl),
        top: sk32_sat_add(r.top, dt),
        right: sk32_sat_add(r.right, dr),
        bottom: sk32_sat_add(r.bottom, db),
    }
}

/// Exact match for `SkIRect::contains(const SkIRect& r)`.
#[inline]
pub const fn irect_contains_rect(outer: IRect, inner: IRect) -> bool {
    !irect_is_empty64(inner)
        && !irect_is_empty64(outer)
        && outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Exact match for `SkIRect::intersect(const SkIRect& r)`.
#[inline]
pub fn irect_intersect(a: IRect, b: IRect) -> Option<IRect> {
    let mut out = IRect::make_empty();
    if out.intersect_pair(a, b) {
        Some(out)
    } else {
        None
    }
}

/// Exact match for `SkRect::makeInset(dx, dy)`.
#[inline]
pub fn rect_make_inset(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect {
        left: r.left + dx,
        top: r.top + dy,
        right: r.right - dx,
        bottom: r.bottom - dy,
    }
}

/// Exact match for `SkRect::makeOutset(dx, dy)`.
#[inline]
pub fn rect_make_outset(r: Rect, dx: f32, dy: f32) -> Rect {
    rect_make_inset(r, -dx, -dy)
}

/// Exact match for `SkRect::Make(const SkIRect&)`.
#[inline]
pub const fn rect_from_irect(r: IRect) -> Rect {
    Rect {
        left: r.left as f32,
        top: r.top as f32,
        right: r.right as f32,
        bottom: r.bottom as f32,
    }
}

/// Exact match for `SkRect::contains(const SkRect&)`.
#[inline]
pub fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
    !inner.is_empty()
        && !outer.is_empty()
        && outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Exact match for `SkRect::intersect(const SkRect&)`.
#[inline]
pub fn rect_intersect(a: Rect, b: Rect) -> Option<Rect> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let left = if a.left > b.left { a.left } else { b.left };
    let top = if a.top > b.top { a.top } else { b.top };
    let right = if a.right < b.right { a.right } else { b.right };
    let bottom = if a.bottom < b.bottom {
        a.bottom
    } else {
        b.bottom
    };
    let res = Rect {
        left,
        top,
        right,
        bottom,
    };
    if !res.is_empty() {
        Some(res)
    } else {
        None
    }
}

/// Exact match for `SkM44::asM33()`.
#[inline]
pub fn m44_as_m33(m: &M44) -> Matrix {
    let mut out = Matrix::identity();
    out.set9(&[
        m.mat[0], m.mat[4], m.mat[12], m.mat[1], m.mat[5], m.mat[13], m.mat[3], m.mat[7], m.mat[15],
    ]);
    out
}

/// Exact match for `SkM44(const SkMatrix&)`.
#[inline]
pub fn m44_from_m33(m: &Matrix) -> M44 {
    M44::from_rows_scalar(
        m.mat[0], m.mat[1], 0.0, m.mat[2], m.mat[3], m.mat[4], 0.0, m.mat[5], 0.0, 0.0, 1.0, 0.0,
        m.mat[6], m.mat[7], 0.0, m.mat[8],
    )
}

/// Exact match for `SkMatrixPriv::IsScaleTranslateAsM33(const SkM44&)`.
#[inline]
pub fn is_scale_translate_as_m33(m: &M44) -> bool {
    m.rc(1, 0) == 0.0
        && m.rc(3, 0) == 0.0
        && m.rc(0, 1) == 0.0
        && m.rc(3, 1) == 0.0
        && m.rc(3, 3) == 1.0
}

/// Exact match for `SkMatrix::RectToRectOrIdentity(src, dst)`.
#[inline]
pub fn matrix_rect_to_rect_or_identity(src: Rect, dst: Rect) -> Matrix {
    Matrix::rect_to_rect(src, dst, skia_rs_geometry::matrix::ScaleToFit::Fill)
        .unwrap_or_else(Matrix::identity)
}

/// Exact match for `SkMatrix::mapRect(src)`.
#[inline]
pub fn matrix_map_rect(m: &Matrix, src: Rect) -> Rect {
    if m.has_perspective() {
        m44_from_m33(m).map_rect(src)
    } else if let Some((dst, _)) = m.map_rect_non_perspective(src) {
        dst
    } else {
        m44_from_m33(m).map_rect(src)
    }
}

/// Exact match for `SkMatrix::mapPoint(pt)`.
#[inline]
pub fn matrix_map_point(m: &Matrix, pt: Point) -> Point {
    let mut dst = [Point::new(0.0, 0.0)];
    m.map_points(&mut dst, &[pt]);
    dst[0]
}

/// Exact match for `SkMatrix::mapVector(dx, dy)`.
#[inline]
pub fn matrix_map_vector(m: &Matrix, dx: f32, dy: f32) -> Point {
    let mut dst = [Point::new(0.0, 0.0)];
    m.map_vectors(&mut dst, &[Point::new(dx, dy)]);
    dst[0]
}

/// Exact match for `SkMatrixPriv::InverseMapRect(mx, dst, src)`.
pub fn matrix_inverse_map_rect(mx: &Matrix, src: Rect) -> Option<Rect> {
    if mx.is_scale_translate() {
        let sx_raw = mx.mat[0];
        let sy_raw = mx.mat[4];
        if sx_raw == 0.0 || sy_raw == 0.0 {
            return None;
        }
        let tx = mx.mat[2];
        let ty = mx.mat[5];
        let mut l = src.left - tx;
        let mut t = src.top - ty;
        let mut r = src.right - tx;
        let mut b = src.bottom - ty;
        if mx.get_type_cached() > skia_rs_geometry::matrix::TRANSLATE_MASK {
            let sx = 1.0_f32 / sx_raw;
            let sy = 1.0_f32 / sy_raw;
            l *= sx;
            t *= sy;
            r *= sx;
            b *= sy;
            if sx < 0.0 && sy < 0.0 {
                return Some(Rect {
                    left: r,
                    top: b,
                    right: l,
                    bottom: t,
                });
            } else if sx < 0.0 {
                return Some(Rect {
                    left: r,
                    top: t,
                    right: l,
                    bottom: b,
                });
            } else if sy < 0.0 {
                return Some(Rect {
                    left: l,
                    top: b,
                    right: r,
                    bottom: t,
                });
            }
        }
        return Some(Rect {
            left: l,
            top: t,
            right: r,
            bottom: b,
        });
    }

    let inv = mx.invert()?;
    Some(matrix_map_rect(&inv, src))
}

/// Exact match for `SkTileMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TileMode {
    Clamp = 0,
    Repeat = 1,
    Mirror = 2,
    Decal = 3,
}

impl TileMode {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Clamp),
            1 => Some(Self::Repeat),
            2 => Some(Self::Mirror),
            3 => Some(Self::Decal),
            _ => None,
        }
    }
}

/// Exact match for `skif::MatrixCapability`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatrixCapability {
    Translate = 0,
    ScaleTranslate = 1,
    Complex = 2,
}

impl MatrixCapability {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Translate),
            1 => Some(Self::ScaleTranslate),
            2 => Some(Self::Complex),
            _ => None,
        }
    }
}

/// Exact match for `skif::FilterResult::PixelBoundary`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PixelBoundary {
    Unknown = 0,
    Transparent = 1,
    Initialized = 2,
}

impl PixelBoundary {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::Transparent),
            2 => Some(Self::Initialized),
            _ => None,
        }
    }
}

/// Exact match for `skif::FilterResult::BoundsScope`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BoundsScope {
    Deferred = 0,
    CanDrawDirectly = 1,
    ShaderOnly = 2,
    Rescale = 3,
}

impl BoundsScope {
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Deferred),
            1 => Some(Self::CanDrawDirectly),
            2 => Some(Self::ShaderOnly),
            3 => Some(Self::Rescale),
            _ => None,
        }
    }
}

/// Exact `#[repr(C)]` match for `SkSamplingOptions` (24 bytes, align 4).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SamplingOptions {
    pub max_aniso: i32,
    pub use_cubic: bool,
    pub cubic_b: f32,
    pub cubic_c: f32,
    pub filter: i32, // 0 = Nearest, 1 = Linear
    pub mipmap: i32, // 0 = None, 1 = Nearest, 2 = Linear
}

impl SamplingOptions {
    pub const NEAREST: Self = Self {
        max_aniso: 0,
        use_cubic: false,
        cubic_b: 0.0,
        cubic_c: 0.0,
        filter: 0,
        mipmap: 0,
    };

    pub const DEFAULT_LINEAR: Self = Self {
        max_aniso: 0,
        use_cubic: false,
        cubic_b: 0.0,
        cubic_c: 0.0,
        filter: 1,
        mipmap: 0,
    };

    #[inline]
    pub const fn aniso(max_aniso: i32) -> Self {
        Self {
            max_aniso: if max_aniso > 1 { max_aniso } else { 1 },
            use_cubic: false,
            cubic_b: 0.0,
            cubic_c: 0.0,
            filter: 0,
            mipmap: 0,
        }
    }

    #[inline]
    pub const fn is_aniso(self) -> bool {
        self.max_aniso != 0
    }
}

/// Bitflags matching `skif::FilterResult::BoundsAnalysis`.
pub mod bounds_analysis {
    pub const SIMPLE: u32 = 0;
    pub const DST_BOUNDS_NOT_COVERED: u32 = 1 << 0;
    pub const HAS_LAYER_FILLING_EFFECT: u32 = 1 << 1;
    pub const REQUIRES_LAYER_CROP: u32 = 1 << 2;
    pub const REQUIRES_SHADER_TILING: u32 = 1 << 3;
    pub const REQUIRES_DECAL_IN_LAYER_SPACE: u32 = 1 << 4;
}

/// Bitflags matching `skif::FilterResult::ShaderFlags`.
pub mod shader_flags {
    pub const NONE: u32 = 0;
    pub const SAMPLED_REPEATEDLY: u32 = 1 << 0;
    pub const NON_TRIVIAL_SAMPLING: u32 = 1 << 1;
}

/// Exact `#[repr(C)]` match for `skif::IVector`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IVector {
    pub x: i32,
    pub y: i32,
}

/// Exact `#[repr(C)]` match for `skif::Vector`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct FVector {
    pub x: f32,
    pub y: f32,
}

impl FVector {
    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Exact `#[repr(C)]` match for `skif::Mapping` (192 bytes: 3 x `SkM44`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mapping {
    pub layer_to_dev: M44,
    pub param_to_layer: M44,
    pub dev_to_layer: M44,
}

impl Default for Mapping {
    fn default() -> Self {
        Self::identity()
    }
}

impl Mapping {
    #[inline]
    pub const fn identity() -> Self {
        Self {
            layer_to_dev: M44::identity(),
            param_to_layer: M44::identity(),
            dev_to_layer: M44::identity(),
        }
    }

    #[inline]
    pub const fn from_param_to_layer(param_to_layer: M44) -> Self {
        Self {
            layer_to_dev: M44::identity(),
            param_to_layer,
            dev_to_layer: M44::identity(),
        }
    }

    #[inline]
    pub fn layer_matrix_33(&self) -> Matrix {
        m44_as_m33(&self.param_to_layer)
    }

    /// Exact port of `skif::Mapping::decomposeCTM(ctm, capability, representativePt)`.
    pub fn decompose_ctm(
        &mut self,
        ctm: &M44,
        capability: MatrixCapability,
        representative_pt: Point,
    ) -> bool {
        let (remainder, layer) = if capability == MatrixCapability::Translate {
            (*ctm, M44::identity())
        } else if is_scale_translate_as_m33(ctm) || capability == MatrixCapability::Complex {
            (M44::identity(), *ctm)
        } else {
            let (_, layer33) = decompose_transform(&m44_as_m33(ctm), representative_pt);
            let layer = m44_from_m33(&layer33);
            let mut remainder = *ctm;
            remainder.pre_scale_xy(1.0_f32 / layer.rc(0, 0), 1.0_f32 / layer.rc(1, 1));
            (remainder, layer)
        };

        if let Some(inv_remainder) = remainder.invert() {
            self.param_to_layer = layer;
            self.layer_to_dev = remainder;
            self.dev_to_layer = inv_remainder;
            true
        } else {
            false
        }
    }

    /// Exact port of `skif::Mapping::adjustLayerSpace(layer)`.
    pub fn adjust_layer_space(&mut self, layer: &M44) -> bool {
        let Some(inv_layer) = layer.invert() else {
            return false;
        };
        let mut new_param_to_layer = M44::identity();
        new_param_to_layer.set_concat(layer, &self.param_to_layer);
        self.param_to_layer = new_param_to_layer;

        let mut new_dev_to_layer = M44::identity();
        new_dev_to_layer.set_concat(layer, &self.dev_to_layer);
        self.dev_to_layer = new_dev_to_layer;

        let mut new_layer_to_dev = M44::identity();
        new_layer_to_dev.set_concat(&self.layer_to_dev, &inv_layer);
        self.layer_to_dev = new_layer_to_dev;
        true
    }

    #[inline]
    pub fn concat_local(&mut self, local: &Matrix) {
        self.param_to_layer.pre_concat_m33(&local.mat);
    }

    #[inline]
    pub fn param_to_layer_rect(&self, r: Rect) -> Rect {
        map_rect(r, &self.layer_matrix_33())
    }

    #[inline]
    pub fn param_to_layer_size(&self, s: Size) -> Size {
        map_size(s, &self.layer_matrix_33())
    }

    #[inline]
    pub fn param_to_layer_point(&self, p: Point) -> Point {
        map_point(p, &self.layer_matrix_33())
    }

    #[inline]
    pub fn param_to_layer_vector(&self, v: FVector) -> FVector {
        map_vector(v, &self.layer_matrix_33())
    }

    #[inline]
    pub fn param_to_layer_matrix(&self, m: &Matrix) -> Matrix {
        map_matrix(m, &self.layer_matrix_33())
    }
}

/// Exact port of `skif::RoundOut(SkRect)`.
#[inline]
pub fn round_out(r: Rect) -> IRect {
    rect_make_inset(r, K_ROUND_EPSILON, K_ROUND_EPSILON).round_out()
}

/// Exact port of `skif::RoundIn(SkRect)`.
#[inline]
pub fn round_in(r: Rect) -> IRect {
    rect_make_outset(r, K_ROUND_EPSILON, K_ROUND_EPSILON).round_in()
}

/// Exact port of `LayerSpace<SkSize>::round()`.
#[inline]
pub fn size_round(s: Size) -> ISize {
    ISize {
        width: scalar_round_to_int(s.width),
        height: scalar_round_to_int(s.height),
    }
}

/// Exact port of `LayerSpace<SkSize>::ceil()`.
#[inline]
pub fn size_ceil(s: Size) -> ISize {
    ISize {
        width: float_ceil2int(s.width - K_ROUND_EPSILON),
        height: float_ceil2int(s.height - K_ROUND_EPSILON),
    }
}

/// Exact port of `LayerSpace<SkSize>::floor()`.
#[inline]
pub fn size_floor(s: Size) -> ISize {
    ISize {
        width: float_floor2int(s.width + K_ROUND_EPSILON),
        height: float_floor2int(s.height + K_ROUND_EPSILON),
    }
}

/// Exact port of `are_axes_nearly_integer_aligned(m, out)`.
pub fn are_axes_nearly_integer_aligned(m: &Matrix) -> (bool, bool, Option<IPoint>) {
    let inv_w = ieee_float_divide(1.0_f32, m.mat[8]);
    let tx = scalar_round_to_scalar(m.mat[2] * inv_w);
    let ty = scalar_round_to_scalar(m.mat[5] * inv_w);

    let affine = scalar_nearly_equal(m.mat[6] * inv_w, 0.0_f32, K_ROUND_EPSILON)
        && scalar_nearly_equal(m.mat[7] * inv_w, 0.0_f32, K_ROUND_EPSILON);
    if !affine {
        return (false, false, None);
    }

    let x_axis = scalar_nearly_equal(1.0_f32, m.mat[0] * inv_w, K_ROUND_EPSILON)
        && scalar_nearly_equal(0.0_f32, m.mat[1] * inv_w, K_ROUND_EPSILON)
        && scalar_nearly_equal(tx, m.mat[2] * inv_w, K_ROUND_EPSILON);
    let y_axis = scalar_nearly_equal(0.0_f32, m.mat[3] * inv_w, K_ROUND_EPSILON)
        && scalar_nearly_equal(1.0_f32, m.mat[4] * inv_w, K_ROUND_EPSILON)
        && scalar_nearly_equal(ty, m.mat[5] * inv_w, K_ROUND_EPSILON);

    let origin = if x_axis && y_axis {
        Some(IPoint {
            x: tx as i32,
            y: ty as i32,
        })
    } else {
        None
    };
    (x_axis, y_axis, origin)
}

/// Exact port of `is_nearly_integer_translation(m, out)`.
#[inline]
pub fn is_nearly_integer_translation(m: &Matrix) -> Option<IPoint> {
    let (ax, ay, pt) = are_axes_nearly_integer_aligned(m);
    if ax && ay {
        pt
    } else {
        None
    }
}

/// Exact port of `decompose_transform(transform, representativePoint, postScaling, scaling)`.
/// Returns `(post_scaling, scaling)`.
pub fn decompose_transform(transform: &Matrix, representative_pt: Point) -> (Matrix, Matrix) {
    if let Some((scale, remaining)) = transform.decompose_scale() {
        let mut scaling = Matrix::identity();
        scaling.set_scale(scale.width, scale.height);
        (remaining, scaling)
    } else {
        let mut approx_scale = transform.differential_area_scale(representative_pt);
        if approx_scale.is_finite() && !scalar_nearly_zero(approx_scale, SK_SCALAR_NEARLY_ZERO) {
            approx_scale = approx_scale.sqrt();
        } else {
            approx_scale = 1.0_f32;
        }
        let mut post_scaling = *transform;
        let inv_scale = 1.0_f32 / approx_scale;
        post_scaling.pre_scale(inv_scale, inv_scale);
        let mut scaling = Matrix::identity();
        scaling.set_scale(approx_scale, approx_scale);
        (post_scaling, scaling)
    }
}

/// Exact port of `periodic_axis_transform(tileMode, crop, output)`.
pub fn periodic_axis_transform(tile_mode: TileMode, crop: IRect, output: IRect) -> Option<Matrix> {
    if tile_mode == TileMode::Clamp || tile_mode == TileMode::Decal {
        return None;
    }

    let crop_l = crop.left as f64;
    let crop_t = crop.top as f64;
    let crop_width = crop.right as f64 - crop_l;
    let crop_height = crop.bottom as f64 - crop_t;

    let period_l = ((output.left as f64 - crop_l) / crop_width).floor();
    let period_t = ((output.top as f64 - crop_t) / crop_height).floor();
    let period_r = ((output.right as f64 - crop_l) / crop_width).ceil();
    let period_b = ((output.bottom as f64 - crop_t) / crop_height).ceil();

    if period_r - period_l <= 1.0 && period_b - period_t <= 1.0 {
        let mut sx = 1.0_f32;
        let mut sy = 1.0_f32;
        let mut tx = -crop_l;
        let mut ty = -crop_t;

        if tile_mode == TileMode::Mirror {
            if (period_l % 2.0) > SK_SCALAR_NEARLY_ZERO as f64 {
                sx = -1.0_f32;
                tx = crop_width - tx;
            }
            if (period_t % 2.0) > SK_SCALAR_NEARLY_ZERO as f64 {
                sy = -1.0_f32;
                ty = crop_height - ty;
            }
        }
        tx += period_l * crop_width + crop_l;
        ty += period_t * crop_height + crop_t;

        if (double_saturate2int(tx) as f32) != (tx as f32)
            || (double_saturate2int(ty) as f32) != (ty as f32)
        {
            return None;
        }

        Some(Matrix::scale_translate(sx, sy, tx as f32, ty as f32))
    } else {
        None
    }
}

/// Exact port of `Mapping::map<SkRect>`.
#[inline]
pub fn map_rect(geom: Rect, matrix: &Matrix) -> Rect {
    if geom.is_empty() {
        Rect::make_empty()
    } else {
        matrix_map_rect(matrix, geom)
    }
}

/// Exact port of `Mapping::map<SkIRect>`.
pub fn map_irect(geom: IRect, matrix: &Matrix) -> IRect {
    if geom.is_empty() {
        return IRect::make_empty();
    }
    if matrix.is_scale_translate() {
        let sx = matrix.mat[0] as f64;
        let sy = matrix.mat[4] as f64;
        let tx = matrix.mat[2] as f64;
        let ty = matrix.mat[5] as f64;
        let l = sx * geom.left as f64 + tx;
        let r = sx * geom.right as f64 + tx;
        let t = sy * geom.top as f64 + ty;
        let b = sy * geom.bottom as f64 + ty;
        let eps = K_ROUND_EPSILON as f64;
        let min_lr = if r < l { r } else { l };
        let max_lr = if l < r { r } else { l };
        let min_tb = if b < t { b } else { t };
        let max_tb = if t < b { b } else { t };
        IRect {
            left: double_saturate2int((min_lr + eps).floor()),
            top: double_saturate2int((min_tb + eps).floor()),
            right: double_saturate2int((max_lr - eps).ceil()),
            bottom: double_saturate2int((max_tb - eps).ceil()),
        }
    } else {
        round_out(matrix_map_rect(matrix, rect_from_irect(geom)))
    }
}

/// Exact port of `Mapping::map<SkIPoint>`.
#[inline]
pub fn map_ipoint(geom: IPoint, matrix: &Matrix) -> IPoint {
    let p = matrix_map_point(matrix, Point::new(geom.x as f32, geom.y as f32));
    IPoint {
        x: scalar_round_to_int(p.x),
        y: scalar_round_to_int(p.y),
    }
}

/// Exact port of `Mapping::map<SkPoint>`.
#[inline]
pub fn map_point(geom: Point, matrix: &Matrix) -> Point {
    matrix_map_point(matrix, geom)
}

/// Exact port of `Mapping::map<Vector>`.
#[inline]
pub fn map_vector(geom: FVector, matrix: &Matrix) -> FVector {
    let v = matrix_map_vector(matrix, geom.x, geom.y);
    FVector { x: v.x, y: v.y }
}

/// Exact port of `Mapping::map<IVector>`.
#[inline]
pub fn map_ivector(geom: IVector, matrix: &Matrix) -> IVector {
    let v = matrix_map_vector(matrix, geom.x as f32, geom.y as f32);
    IVector {
        x: scalar_round_to_int(v.x),
        y: scalar_round_to_int(v.y),
    }
}

/// Exact port of `Mapping::map<SkSize>`.
pub fn map_size(geom: Size, matrix: &Matrix) -> Size {
    if matrix.is_scale_translate() {
        let sizes = matrix_map_vector(matrix, geom.width, geom.height);
        return Size {
            width: sizes.x.abs(),
            height: sizes.y.abs(),
        };
    }
    let x_axis = matrix_map_vector(matrix, geom.width, 0.0_f32);
    let y_axis = matrix_map_vector(matrix, 0.0_f32, geom.height);
    Size {
        width: Point::length(x_axis.x, x_axis.y),
        height: Point::length(y_axis.x, y_axis.y),
    }
}

/// Exact port of `Mapping::map<SkISize>`.
#[inline]
pub fn map_isize(geom: ISize, matrix: &Matrix) -> ISize {
    let size = map_size(
        Size {
            width: geom.width as f32,
            height: geom.height as f32,
        },
        matrix,
    );
    ISize {
        width: float_ceil2int(size.width - K_ROUND_EPSILON),
        height: float_ceil2int(size.height - K_ROUND_EPSILON),
    }
}

/// Exact port of `Mapping::map<SkMatrix>`.
pub fn map_matrix(m: &Matrix, matrix: &Matrix) -> Matrix {
    let mut inv = matrix.invert().unwrap_or_else(Matrix::identity);
    inv.post_concat(m);
    inv.post_concat(matrix);
    inv
}

/// Exact port of `LayerSpace<SkMatrix>::inverseMapRect(SkRect)`.
pub fn inverse_map_rect(m: &Matrix, r: Rect) -> Option<Rect> {
    if r.is_empty() {
        Some(Rect::make_empty())
    } else {
        matrix_inverse_map_rect(m, r)
    }
}

/// Exact port of `LayerSpace<SkMatrix>::inverseMapRect(SkIRect)`.
pub fn inverse_map_irect(m: &Matrix, rect: IRect) -> Option<IRect> {
    if irect_is_empty64(rect) {
        return Some(IRect::make_empty());
    }
    if m.is_scale_translate() {
        let sx = m.mat[0];
        let sy = m.mat[4];
        if sx == 0.0 || sy == 0.0 {
            return None;
        }
        let tx = m.mat[2] as f64;
        let ty = m.mat[5] as f64;
        let sxd = sx as f64;
        let syd = sy as f64;
        let l = (rect.left as f64 - tx) / sxd;
        let r = (rect.right as f64 - tx) / sxd;
        let t = (rect.top as f64 - ty) / syd;
        let b = (rect.bottom as f64 - ty) / syd;
        let eps = K_ROUND_EPSILON as f64;
        let min_lr = if r < l { r } else { l };
        let max_lr = if l < r { r } else { l };
        let min_tb = if b < t { b } else { t };
        let max_tb = if t < b { b } else { t };
        return Some(IRect {
            left: double_saturate2int((min_lr + eps).floor()),
            top: double_saturate2int((min_tb + eps).floor()),
            right: double_saturate2int((max_lr - eps).ceil()),
            bottom: double_saturate2int((max_tb - eps).ceil()),
        });
    }

    let mapped = matrix_inverse_map_rect(m, rect_from_irect(rect))?;
    Some(round_out(mapped))
}

/// Exact port of `LayerSpace<SkIRect>::relevantSubset(dstRect, tileMode)`.
pub fn relevant_subset(src: IRect, dst: IRect, tile_mode: TileMode) -> IRect {
    if tile_mode == TileMode::Decal || tile_mode == TileMode::Clamp {
        if let Some(inter) = irect_intersect(src, dst) {
            inter
        } else if tile_mode == TileMode::Decal {
            IRect::make_empty()
        } else {
            IRect::closest_disjoint_edge(src, dst)
        }
    } else {
        src
    }
}
