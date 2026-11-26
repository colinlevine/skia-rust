//! Safe Rust port of:
//! - `src/core/SkBitmapProcState.cpp`
//! - `src/core/SkBitmapProcState_matrixProcs.cpp`
//! - `src/core/SkBitmapProcState_opts.cpp` / `src/opts/SkBitmapProcState_opts.h`

use skia_rs_color::image_info::ColorType;
use skia_rs_core_types::alpha_255_to_256;
use skia_rs_geometry::{
    matrix::{MSCALE_X, MSCALE_Y, MSKEW_Y, MTRANS_X, MTRANS_Y, SCALE_MASK, TRANSLATE_MASK},
    pin_i32, scalar_nearly_equal, scalar_nearly_zero, Matrix, Point,
};

pub const SK_FIXED_1: i32 = 1 << 16;

/// Matches `enum class SkTileMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum TileMode {
    #[default]
    Clamp = 0,
    Repeat = 1,
    Mirror = 2,
    Decal = 3,
}

impl TileMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Clamp),
            1 => Some(Self::Repeat),
            2 => Some(Self::Mirror),
            3 => Some(Self::Decal),
            _ => None,
        }
    }
}

/// Matches `SkBitmapProcState::MatrixProc` variants chosen by `chooseMatrixProc`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatrixProcKind {
    None = 0,
    ClampNofilterTrans = 1,
    RepeatNofilterTrans = 2,
    MirrorNofilterTrans = 3,
    ClampNofilterScale = 4,
    ClampFilterScale = 5,
    ClampNofilterAffine = 6,
    ClampFilterAffine = 7,
    RepeatNofilterScale = 8,
    RepeatFilterScale = 9,
    RepeatNofilterAffine = 10,
    RepeatFilterAffine = 11,
    MirrorNofilterScale = 12,
    MirrorFilterScale = 13,
    MirrorNofilterAffine = 14,
    MirrorFilterAffine = 15,
}

impl MatrixProcKind {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::ClampNofilterTrans),
            2 => Some(Self::RepeatNofilterTrans),
            3 => Some(Self::MirrorNofilterTrans),
            4 => Some(Self::ClampNofilterScale),
            5 => Some(Self::ClampFilterScale),
            6 => Some(Self::ClampNofilterAffine),
            7 => Some(Self::ClampFilterAffine),
            8 => Some(Self::RepeatNofilterScale),
            9 => Some(Self::RepeatFilterScale),
            10 => Some(Self::RepeatNofilterAffine),
            11 => Some(Self::RepeatFilterAffine),
            12 => Some(Self::MirrorNofilterScale),
            13 => Some(Self::MirrorFilterScale),
            14 => Some(Self::MirrorNofilterAffine),
            15 => Some(Self::MirrorFilterAffine),
            _ => None,
        }
    }
}

/// Matches `SkBitmapProcState::ShaderProc32` variants chosen by `chooseProcs` / `chooseShaderProc32`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShaderProcKind {
    None = 0,
    ClampOpaqueNofilterDx = 1,
    ConstX = 2,
    ClampNofilterTrans = 3,
    RepeatNofilterTrans = 4,
    DoNothing = 5,
}

/// Plain-data snapshot of the fields needed by matrix/sample/shader procs.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ProcStateParams {
    pub inv_matrix: Matrix,
    pub inv_sx: i64,
    pub inv_ky: i64,
    pub filter_one_x: i32,
    pub filter_one_y: i32,
    pub width: i32,
    pub height: i32,
    pub tile_mode_x: i32,
    pub tile_mode_y: i32,
    pub alpha_scale: u16,
    pub bilerp: bool,
}

// ============================================================================
// Fixed-point and integer math helpers
// ============================================================================

const MAX_S64_FITS_IN_FLOAT: i64 = (i64::MAX >> (63 - 24)) << (63 - 24);
const MIN_S64_FITS_IN_FLOAT: i64 = -MAX_S64_FITS_IN_FLOAT;
const MAX_S64_F32: f32 = MAX_S64_FITS_IN_FLOAT as f32;
const MIN_S64_F32: f32 = MIN_S64_FITS_IN_FLOAT as f32;

/// Exact port of `sk_float_saturate2int64`.
#[inline]
pub fn float_saturate2int64(mut x: f32) -> i64 {
    x = if x < MAX_S64_F32 { x } else { MAX_S64_F32 };
    x = if x > MIN_S64_F32 { x } else { MIN_S64_F32 };
    x as i64
}

/// Exact port of `SkScalarToFixed3232`.
#[inline]
pub fn scalar_to_fixed3232(x: f32) -> i64 {
    float_saturate2int64(x * (65536.0_f32 * 65536.0_f32))
}

/// Exact port of `SkFixedToFixed3232`.
#[inline]
pub const fn fixed_to_fixed3232(x: i32) -> i64 {
    ((x as i64 as u64) << 16) as i64
}

/// Exact port of `SkFixed3232ToFixed`.
#[inline]
pub const fn fixed3232_to_fixed(x: i64) -> i32 {
    (x >> 16) as i32
}

/// Exact port of `SkFixed3232ToInt`.
#[inline]
pub const fn fixed3232_to_int(x: i64) -> i32 {
    (x >> 32) as i32
}

/// Exact port of `SkFixedToScalar`.
#[inline]
pub fn fixed_to_scalar(x: i32) -> f32 {
    (x as f32) * 1.52587890625e-5_f32
}

/// Exact port of `sk_fixed3232_saturate2fixed`.
#[inline]
pub const fn fixed3232_saturate2fixed(mut x: i64) -> i32 {
    const MAX_HI: i64 = i16::MAX as i64;
    const MIN_HI: i64 = i16::MIN as i64;
    const MAX_FX: i64 = (i16::MAX as i64) << 32;
    const MIN_FX: i64 = (i16::MIN as i64) << 32;
    x = if (x >> 32) < MAX_HI { x } else { MAX_FX };
    x = if (x >> 32) > MIN_HI { x } else { MIN_FX };
    fixed3232_to_fixed(x)
}

/// Exact port of `SkScalarRoundToScalar(x)` = `(float)std::floor((double)x + 0.5)`.
#[inline]
fn scalar_round_to_scalar(x: f32) -> f32 {
    ((x as f64) + 0.5_f64).floor() as f32
}

/// Exact port of `SkAlphaMulQ(c, scale)`.
#[inline]
pub const fn alpha_mul_q(c: u32, scale: u32) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    let rb = ((c & MASK).wrapping_mul(scale)) >> 8;
    let ag = ((c >> 8) & MASK).wrapping_mul(scale);
    (rb & MASK) | (ag & !MASK)
}

/// Exact port of `sk_int_mod(int x, int n)`.
#[inline]
pub const fn sk_int_mod(mut x: i32, n: i32) -> i32 {
    if (x as u32) >= (n as u32) {
        if x < 0 {
            x = n.wrapping_add(!((!x) % n));
        } else {
            x = x % n;
        }
    }
    x
}

/// Exact port of `sk_int_mirror(int x, int n)`.
#[inline]
pub const fn sk_int_mirror(x: i32, n: i32) -> i32 {
    let mut x = sk_int_mod(x, n.wrapping_mul(2));
    if x >= n {
        x = n.wrapping_add(!(x.wrapping_sub(n)));
    }
    x
}

#[inline]
const fn int_clamp(mut x: i32, n: i32) -> u32 {
    if x < 0 {
        x = 0;
    }
    if x >= n {
        x = n - 1;
    }
    x as u32
}

#[inline]
const fn int_repeat(x: i32, n: i32) -> u32 {
    sk_int_mod(x, n) as u32
}

#[inline]
const fn int_mirror(x: i32, n: i32) -> u32 {
    sk_int_mirror(x, n) as u32
}

#[inline]
const fn pack_two_shorts(pri: u32, sec: u32) -> u32 {
    #[cfg(target_endian = "big")]
    {
        ((pri & 0xFFFF) << 16) | (sec & 0xFFFF)
    }
    #[cfg(not(target_endian = "big"))]
    {
        (pri & 0xFFFF) | ((sec & 0xFFFF) << 16)
    }
}

#[inline]
const fn unpack_primary_short(packed: u32) -> usize {
    #[cfg(target_endian = "big")]
    {
        (packed >> 16) as usize
    }
    #[cfg(not(target_endian = "big"))]
    {
        (packed & 0xFFFF) as usize
    }
}

#[inline]
const fn unpack_secondary_short(packed: u32) -> usize {
    #[cfg(target_endian = "big")]
    {
        (packed & 0xFFFF) as usize
    }
    #[cfg(not(target_endian = "big"))]
    {
        (packed >> 16) as usize
    }
}

/// Writes `u16` values into `xy_u32[..]` in native-endian order (matching `(uint16_t*)xy`).
#[inline]
fn write_u16_at(xy_u32: &mut [u32], idx: usize, val: u16) {
    let word = idx >> 1;
    let is_second = (idx & 1) != 0;
    #[cfg(target_endian = "big")]
    let shift = if is_second { 0 } else { 16 };
    #[cfg(not(target_endian = "big"))]
    let shift = if is_second { 16 } else { 0 };
    let mask = !(0xFFFFu32 << shift);
    xy_u32[word] = (xy_u32[word] & mask) | ((val as u32) << shift);
}

#[inline]
fn read_u16_at(xy_u32: &[u32], idx: usize) -> usize {
    let word = xy_u32[idx >> 1];
    if (idx & 1) == 0 {
        unpack_primary_short(word)
    } else {
        unpack_secondary_short(word)
    }
}

// ============================================================================
// AutoMapper
// ============================================================================

#[derive(Copy, Clone, Debug)]
pub struct AutoMapper {
    pub fx: i64,
    pub fy: i64,
    pub scalar_x: f32,
    pub scalar_y: f32,
}

impl AutoMapper {
    #[inline]
    pub fn new(
        inv_matrix: &Matrix,
        bilerp: bool,
        filter_one_x: i32,
        filter_one_y: i32,
        x: i32,
        y: i32,
    ) -> Self {
        let src_pt = [Point::new((x as f32) + 0.5_f32, (y as f32) + 0.5_f32)];
        let mut dst_pt = [Point::new(0.0, 0.0)];
        inv_matrix.map_points(&mut dst_pt, &src_pt);
        let pt = dst_pt[0];

        let (bias_x, bias_y) = if bilerp {
            (filter_one_x >> 1, filter_one_y >> 1)
        } else {
            (1, 1)
        };

        let fx = (scalar_to_fixed3232(pt.x) as u64)
            .wrapping_sub(fixed_to_fixed3232(bias_x) as u64) as i64;
        let fy = (scalar_to_fixed3232(pt.y) as u64)
            .wrapping_sub(fixed_to_fixed3232(bias_y) as u64) as i64;

        Self {
            fx,
            fy,
            scalar_x: pt.x - fixed_to_scalar(bias_x),
            scalar_y: pt.y - fixed_to_scalar(bias_y),
        }
    }

    #[inline]
    pub const fn fixed_x(&self) -> i32 {
        fixed3232_to_fixed(self.fx)
    }

    #[inline]
    pub const fn fixed_y(&self) -> i32 {
        fixed3232_to_fixed(self.fy)
    }

    #[inline]
    pub const fn int_x(&self) -> i32 {
        fixed3232_to_int(self.fx)
    }

    #[inline]
    pub const fn int_y(&self) -> i32 {
        fixed3232_to_int(self.fy)
    }
}

// ============================================================================
// init / chooseProcs / chooseMatrixProc / chooseShaderProc32 / maxCountForBufferSize
// ============================================================================

#[inline]
fn matrix_only_scale_translate(m: &Matrix) -> bool {
    (m.get_type_cached() & !TRANSLATE_MASK) == SCALE_MASK
}

#[inline]
fn just_trans_general(m: &Matrix) -> bool {
    let tol = 1.0_f32 / 32768.0_f32;
    scalar_nearly_zero(m.mat[MSCALE_X] - 1.0_f32, tol)
        && scalar_nearly_zero(m.mat[MSCALE_Y] - 1.0_f32, tol)
}

#[inline]
fn just_trans_integral(m: &Matrix) -> bool {
    let tol = 1.0_f32 / 256.0_f32;
    let tx = m.mat[MTRANS_X];
    let ty = m.mat[MTRANS_Y];
    m.get_type_cached() <= TRANSLATE_MASK
        && scalar_nearly_equal(tx, scalar_round_to_scalar(tx), tol)
        && scalar_nearly_equal(ty, scalar_round_to_scalar(ty), tol)
}

#[inline]
const fn valid_for_filtering(dimension: u32) -> bool {
    (dimension & !0x3FFF) == 0
}

/// Exact port of `SkBitmapProcState::init` matrix & bilerp adjustments (after mipmap lookup).
pub fn init_adjust_matrix_and_bilerp(
    inv_matrix: &mut Matrix,
    width: i32,
    height: i32,
    tile_x: TileMode,
    tile_y: TileMode,
    bilerp: &mut bool,
) {
    let _ = inv_matrix.get_type_mut();
    let mut integral_translate_only = just_trans_integral(inv_matrix);
    if !integral_translate_only {
        if tile_x != TileMode::Clamp || tile_y != TileMode::Clamp {
            inv_matrix.post_idiv(width, height);
            let _ = inv_matrix.get_type_mut();
        }
        if matrix_only_scale_translate(inv_matrix) {
            if let Some(forward) = inv_matrix.invert() {
                if just_trans_general(&forward) {
                    inv_matrix.set_translate(-forward.mat[MTRANS_X], -forward.mat[MTRANS_Y]);
                }
            }
        }
        integral_translate_only = just_trans_integral(inv_matrix);
    }

    if *bilerp
        && (!valid_for_filtering((width as u32) | (height as u32)) || integral_translate_only)
    {
        *bilerp = false;
    }
}

/// Output of `choose_procs`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChooseProcsResult {
    pub inv_sx: i64,
    pub inv_ky: i64,
    pub filter_one_x: i32,
    pub filter_one_y: i32,
    pub alpha_scale: u16,
    pub matrix_proc: MatrixProcKind,
    pub shader_proc: ShaderProcKind,
    pub is_scale_translate: bool,
}

/// Exact port of `SkBitmapProcState::setupForTranslate()`.
pub fn setup_for_translate(
    inv_matrix: &Matrix,
    bilerp: bool,
    filter_one_x: i32,
    filter_one_y: i32,
) -> Option<(i32, i32)> {
    let mapper = AutoMapper::new(inv_matrix, bilerp, filter_one_x, filter_one_y, 0, 0);
    let too_big = (1i32 << 30) as f32;
    if mapper.scalar_x.abs() > too_big || mapper.scalar_y.abs() > too_big {
        return None;
    }
    Some((mapper.int_x(), mapper.int_y()))
}

/// Exact port of `SkBitmapProcState::chooseMatrixProc(bool translate_only_matrix)`.
pub fn choose_matrix_proc(
    inv_matrix: &Matrix,
    width: i32,
    height: i32,
    tile_x: TileMode,
    tile_y: TileMode,
    bilerp: bool,
    translate_only_matrix: bool,
    filter_one_x: &mut i32,
    filter_one_y: &mut i32,
) -> MatrixProcKind {
    if tile_x == tile_y {
        if translate_only_matrix && !bilerp {
            return match tile_x {
                TileMode::Clamp => MatrixProcKind::ClampNofilterTrans,
                TileMode::Repeat => MatrixProcKind::RepeatNofilterTrans,
                TileMode::Mirror => MatrixProcKind::MirrorNofilterTrans,
                TileMode::Decal => MatrixProcKind::ClampNofilterTrans,
            };
        }

        let mut index = if bilerp { 1 } else { 0 };
        if !inv_matrix.is_scale_translate() {
            index |= 2;
        }

        if tile_x == TileMode::Clamp {
            *filter_one_x = SK_FIXED_1;
            *filter_one_y = SK_FIXED_1;
            return match index {
                0 => MatrixProcKind::ClampNofilterScale,
                1 => MatrixProcKind::ClampFilterScale,
                2 => MatrixProcKind::ClampNofilterAffine,
                _ => MatrixProcKind::ClampFilterAffine,
            };
        }

        *filter_one_x = SK_FIXED_1 / width;
        *filter_one_y = SK_FIXED_1 / height;

        if tile_x == TileMode::Repeat {
            return match index {
                0 => MatrixProcKind::RepeatNofilterScale,
                1 => MatrixProcKind::RepeatFilterScale,
                2 => MatrixProcKind::RepeatNofilterAffine,
                _ => MatrixProcKind::RepeatFilterAffine,
            };
        }
        return match index {
            0 => MatrixProcKind::MirrorNofilterScale,
            1 => MatrixProcKind::MirrorFilterScale,
            2 => MatrixProcKind::MirrorNofilterAffine,
            _ => MatrixProcKind::MirrorFilterAffine,
        };
    }
    MatrixProcKind::None
}

/// Exact port of `SkBitmapProcState::chooseShaderProc32()`.
pub fn choose_shader_proc32(
    inv_matrix: &Matrix,
    ct: ColorType,
    n32_ct: ColorType,
    width: i32,
    tile_x: TileMode,
    tile_y: TileMode,
    bilerp: bool,
    alpha_scale: u16,
    filter_one_x: &mut i32,
    filter_one_y: &mut i32,
) -> ShaderProcKind {
    if ct != n32_ct {
        return ShaderProcKind::None;
    }

    let is_translate = (inv_matrix.get_type_cached() & !TRANSLATE_MASK) == 0;

    if width == 1 && inv_matrix.is_scale_translate() {
        if !bilerp && is_translate {
            if let Some((fx, fy)) = setup_for_translate(inv_matrix, bilerp, *filter_one_x, *filter_one_y) {
                *filter_one_x = fx;
                *filter_one_y = fy;
            } else {
                return ShaderProcKind::DoNothing;
            }
        }
        return ShaderProcKind::ConstX;
    }

    if alpha_scale < 256 {
        return ShaderProcKind::None;
    }
    if !is_translate {
        return ShaderProcKind::None;
    }
    if bilerp {
        return ShaderProcKind::None;
    }

    if tile_x == TileMode::Clamp && tile_y == TileMode::Clamp {
        if let Some((fx, fy)) = setup_for_translate(inv_matrix, bilerp, *filter_one_x, *filter_one_y) {
            *filter_one_x = fx;
            *filter_one_y = fy;
            return ShaderProcKind::ClampNofilterTrans;
        }
        return ShaderProcKind::DoNothing;
    }
    if tile_x == TileMode::Repeat && tile_y == TileMode::Repeat {
        if let Some((fx, fy)) = setup_for_translate(inv_matrix, bilerp, *filter_one_x, *filter_one_y) {
            *filter_one_x = fx;
            *filter_one_y = fy;
            return ShaderProcKind::RepeatNofilterTrans;
        }
        return ShaderProcKind::DoNothing;
    }
    ShaderProcKind::None
}

/// Exact port of `SkBitmapProcState::chooseProcs()`.
pub fn choose_procs(
    inv_matrix: &Matrix,
    ct: ColorType,
    n32_ct: ColorType,
    width: i32,
    height: i32,
    tile_x: TileMode,
    tile_y: TileMode,
    bilerp: bool,
    paint_alpha: u8,
    mut filter_one_x: i32,
    mut filter_one_y: i32,
) -> ChooseProcsResult {
    let inv_sx = scalar_to_fixed3232(inv_matrix.mat[MSCALE_X]);
    let inv_ky = scalar_to_fixed3232(inv_matrix.mat[MSKEW_Y]);
    let alpha_scale = alpha_255_to_256(paint_alpha) as u16;
    let translate_only = (inv_matrix.get_type_cached() & !TRANSLATE_MASK) == 0;
    let is_scale_translate = inv_matrix.is_scale_translate();

    let matrix_proc = choose_matrix_proc(
        inv_matrix,
        width,
        height,
        tile_x,
        tile_y,
        bilerp,
        translate_only,
        &mut filter_one_x,
        &mut filter_one_y,
    );

    let shader_proc = if alpha_scale == 256
        && !bilerp
        && tile_x == TileMode::Clamp
        && tile_y == TileMode::Clamp
        && is_scale_translate
    {
        ShaderProcKind::ClampOpaqueNofilterDx
    } else {
        choose_shader_proc32(
            inv_matrix,
            ct,
            n32_ct,
            width,
            tile_x,
            tile_y,
            bilerp,
            alpha_scale,
            &mut filter_one_x,
            &mut filter_one_y,
        )
    };

    ChooseProcsResult {
        inv_sx,
        inv_ky,
        filter_one_x,
        filter_one_y,
        alpha_scale,
        matrix_proc,
        shader_proc,
        is_scale_translate,
    }
}

/// Exact port of `SkBitmapProcState::maxCountForBufferSize(size_t bufferSize)`.
#[inline]
pub fn max_count_for_buffer_size(
    buffer_size: usize,
    is_scale_translate: bool,
    bilerp: bool,
) -> i32 {
    let mut size = (buffer_size as i32) & !3;
    if is_scale_translate {
        size -= 4;
        if size < 0 {
            size = 0;
        }
        size >>= 1;
    } else {
        size >>= 2;
    }
    if bilerp {
        size >>= 1;
    }
    size
}

// ============================================================================
// Matrix Procs (`SkBitmapProcState_matrixProcs.cpp`)
// ============================================================================

#[inline]
const fn tile_clamp(fx: i32, max: i32) -> u32 {
    let v = fx >> 16;
    let clamped = if v < 0 {
        0
    } else if v > max {
        max
    } else {
        v
    };
    clamped as u32
}

#[inline]
const fn tile_repeat(fx: i32, max: i32) -> u32 {
    (((fx as u32) & 0xFFFF).wrapping_mul((max + 1) as u32)) >> 16
}

#[inline]
const fn tile_mirror(fx: i32, max: i32) -> u32 {
    let s: i32 = (((fx as u32) << 15) as i32) >> 31;
    ((((fx ^ s) as u32) & 0xFFFF).wrapping_mul((max + 1) as u32)) >> 16
}

#[inline]
const fn extract_low_bits_clamp_clamp(fx: i32, _max: i32) -> u32 {
    ((fx >> 12) & 0xF) as u32
}

#[inline]
const fn extract_low_bits_general(fx: i32, max: i32) -> u32 {
    let scaled = (((fx as u32) & 0xFFFF).wrapping_mul((max + 1) as u32)) as i32;
    extract_low_bits_clamp_clamp(scaled, max)
}

#[inline]
fn pack_coord(
    f: i32,
    max: u32,
    one: i32,
    tile: fn(i32, i32) -> u32,
    extract_low_bits: fn(i32, i32) -> u32,
) -> u32 {
    let max_i = max as i32;
    let mut packed = tile(f, max_i);
    packed = (packed << 4) | extract_low_bits(f, max_i);
    packed = (packed << 14) | tile(f.wrapping_add(one), max_i);
    packed
}

/// Exact port of `sktests::pack_clamp`.
pub fn pack_clamp(f: i32, max: u32) -> u32 {
    pack_coord(f, max, SK_FIXED_1, tile_clamp, extract_low_bits_clamp_clamp)
}

/// Exact port of `sktests::pack_repeat`.
pub fn pack_repeat(f: i32, max: u32, width: usize) -> u32 {
    pack_coord(
        f,
        max,
        ((SK_FIXED_1 as usize) / width) as i32,
        tile_repeat,
        extract_low_bits_general,
    )
}

/// Exact port of `sktests::pack_mirror`.
pub fn pack_mirror(f: i32, max: u32, width: usize) -> u32 {
    pack_coord(
        f,
        max,
        ((SK_FIXED_1 as usize) / width) as i32,
        tile_mirror,
        extract_low_bits_general,
    )
}

#[inline]
fn can_truncate_to_fixed_for_decal(fx: i32, dx: i32, count: i32, max: u32) -> bool {
    if dx <= SK_FIXED_1 / 256 {
        return false;
    }
    if ((fx >> 16) as u32) >= max {
        return false;
    }
    let last_fx: i64 = (fx as i64) + (dx as i64) * ((count - 1) as i64);
    let last_fx_u = last_fx as u64;
    if last_fx_u > (i32::MAX as u64) {
        return false;
    }
    (((last_fx_u as i32) >> 16) as u32) < max
}

fn decal_nofilter_scale(dst: &mut [u32], mut fx: i32, dx: i32, mut count: i32) {
    let mut out_idx = 0usize;
    while count > 2 {
        dst[out_idx] = pack_two_shorts((fx >> 16) as u32, (fx.wrapping_add(dx) >> 16) as u32);
        out_idx += 1;
        fx = fx.wrapping_add(dx.wrapping_add(dx));
        count -= 2;
    }
    if count == 2 {
        write_u16_at(&mut dst[out_idx..], 1, (fx.wrapping_add(dx) >> 16) as u16);
        write_u16_at(&mut dst[out_idx..], 0, (fx >> 16) as u16);
    } else if count == 1 {
        write_u16_at(&mut dst[out_idx..], 0, (fx >> 16) as u16);
    }
}

fn nofilter_scale(
    s: &ProcStateParams,
    xy: &mut [u32],
    mut count: i32,
    x: i32,
    y: i32,
    tilex: fn(i32, i32) -> u32,
    tiley: fn(i32, i32) -> u32,
    try_decal: bool,
) {
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    xy[0] = tiley(mapper.fixed_y(), s.height - 1);
    let mut fx = mapper.fx;

    let max_x = (s.width - 1) as u32;
    let dst = &mut xy[1..];
    if max_x == 0 {
        for i in 0..(count as usize) {
            write_u16_at(dst, i, 0);
        }
        return;
    }

    let dx = s.inv_sx;
    if try_decal {
        let fixed_fx = fixed3232_to_fixed(fx);
        let fixed_dx = fixed3232_to_fixed(dx);
        if can_truncate_to_fixed_for_decal(fixed_fx, fixed_dx, count, max_x) {
            decal_nofilter_scale(dst, fixed_fx, fixed_dx, count);
            return;
        }
    }

    let max_x_i = max_x as i32;
    let mut out_idx = 0usize;
    while count >= 2 {
        dst[out_idx] = pack_two_shorts(
            tilex(fixed3232_to_fixed(fx), max_x_i),
            tilex(fixed3232_to_fixed(fx.wrapping_add(dx)), max_x_i),
        );
        out_idx += 1;
        fx = fx.wrapping_add(dx.wrapping_add(dx));
        count -= 2;
    }
    if count > 0 {
        write_u16_at(&mut dst[out_idx..], 0, tilex(fixed3232_to_fixed(fx), max_x_i) as u16);
    }
}

fn nofilter_affine(
    s: &ProcStateParams,
    xy: &mut [u32],
    count: i32,
    x: i32,
    y: i32,
    tilex: fn(i32, i32) -> u32,
    tiley: fn(i32, i32) -> u32,
) {
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    let mut fx = mapper.fx;
    let mut fy = mapper.fy;
    let dx = s.inv_sx;
    let dy = s.inv_ky;
    let max_x = s.width - 1;
    let max_y = s.height - 1;

    for slot in &mut xy[..(count as usize)] {
        *slot = (tiley(fixed3232_to_fixed(fy), max_y) << 16) | tilex(fixed3232_to_fixed(fx), max_x);
        fx = fx.wrapping_add(dx);
        fy = fy.wrapping_add(dy);
    }
}

fn filter_scale(
    s: &ProcStateParams,
    xy: &mut [u32],
    count: i32,
    x: i32,
    y: i32,
    tilex: fn(i32, i32) -> u32,
    tiley: fn(i32, i32) -> u32,
    extract_low_bits: fn(i32, i32) -> u32,
    try_decal: bool,
) {
    let max_x = (s.width - 1) as u32;
    let max_y = (s.height - 1) as u32;
    let dx = s.inv_sx;
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    xy[0] = pack_coord(
        fixed3232_saturate2fixed(mapper.fy),
        max_y,
        s.filter_one_y,
        tiley,
        extract_low_bits,
    );
    let mut fx = mapper.fx;
    let out = &mut xy[1..(1 + count as usize)];

    if try_decal
        && (fixed3232_to_int(fx) as u32) < max_x
        && (fixed3232_to_int(fx.wrapping_add(dx.wrapping_mul((count - 1) as i64))) as u32) < max_x
    {
        for slot in out.iter_mut() {
            let fixed_fx = fixed3232_saturate2fixed(fx);
            *slot = (((fixed_fx >> 12) as u32) << 14) | (((fixed_fx >> 16) + 1) as u32);
            fx = fx.wrapping_add(dx);
        }
        return;
    }

    for slot in out.iter_mut() {
        *slot = pack_coord(
            fixed3232_saturate2fixed(fx),
            max_x,
            s.filter_one_x,
            tilex,
            extract_low_bits,
        );
        fx = fx.wrapping_add(dx);
    }
}

fn filter_affine(
    s: &ProcStateParams,
    xy: &mut [u32],
    count: i32,
    x: i32,
    y: i32,
    tilex: fn(i32, i32) -> u32,
    tiley: fn(i32, i32) -> u32,
    extract_low_bits: fn(i32, i32) -> u32,
) {
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    let one_x = s.filter_one_x;
    let one_y = s.filter_one_y;
    let mut fx = mapper.fx;
    let mut fy = mapper.fy;
    let dx = s.inv_sx;
    let dy = s.inv_ky;
    let max_x = (s.width - 1) as u32;
    let max_y = (s.height - 1) as u32;

    for i in 0..(count as usize) {
        xy[2 * i] = pack_coord(fixed3232_to_fixed(fy), max_y, one_y, tiley, extract_low_bits);
        xy[2 * i + 1] = pack_coord(fixed3232_to_fixed(fx), max_x, one_x, tilex, extract_low_bits);
        fy = fy.wrapping_add(dy);
        fx = fx.wrapping_add(dx);
    }
}

fn fill_sequential(dst: &mut [u32], start_idx: usize, mut pos: i32, count: usize) {
    for i in 0..count {
        write_u16_at(dst, start_idx + i, pos as u16);
        pos += 1;
    }
}

fn fill_backwards(dst: &mut [u32], start_idx: usize, mut pos: i32, count: usize) {
    for i in 0..count {
        write_u16_at(dst, start_idx + i, pos as u16);
        pos -= 1;
    }
}

fn clampx_nofilter_trans(
    s: &ProcStateParams,
    xy: &mut [u32],
    mut count: i32,
    x: i32,
    y: i32,
) {
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    xy[0] = int_clamp(mapper.int_y(), s.height);
    let mut xpos = mapper.int_x();
    let width = s.width;
    let dst = &mut xy[1..];
    if width == 1 {
        for i in 0..(count as usize) {
            write_u16_at(dst, i, 0);
        }
        return;
    }
    let mut out_idx = 0usize;
    if xpos < 0 {
        let n = (-xpos).min(count);
        for i in 0..(n as usize) {
            write_u16_at(dst, out_idx + i, 0);
        }
        count -= n;
        if count == 0 {
            return;
        }
        out_idx += n as usize;
        xpos = 0;
    }
    if xpos < width {
        let n = (width - xpos).min(count);
        fill_sequential(dst, out_idx, xpos, n as usize);
        count -= n;
        if count == 0 {
            return;
        }
        out_idx += n as usize;
    }
    let max_val = (width - 1) as u16;
    for i in 0..(count as usize) {
        write_u16_at(dst, out_idx + i, max_val);
    }
}

fn repeatx_nofilter_trans(
    s: &ProcStateParams,
    xy: &mut [u32],
    mut count: i32,
    x: i32,
    y: i32,
) {
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    xy[0] = int_repeat(mapper.int_y(), s.height);
    let xpos = mapper.int_x();
    let width = s.width;
    let dst = &mut xy[1..];
    if width == 1 {
        for i in 0..(count as usize) {
            write_u16_at(dst, i, 0);
        }
        return;
    }
    let mut out_idx = 0usize;
    let start = sk_int_mod(xpos, width);
    let n = (width - start).min(count);
    fill_sequential(dst, out_idx, start, n as usize);
    out_idx += n as usize;
    count -= n;

    while count >= width {
        fill_sequential(dst, out_idx, 0, width as usize);
        out_idx += width as usize;
        count -= width;
    }
    if count > 0 {
        fill_sequential(dst, out_idx, 0, count as usize);
    }
}

fn mirrorx_nofilter_trans(
    s: &ProcStateParams,
    xy: &mut [u32],
    mut count: i32,
    x: i32,
    y: i32,
) {
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    xy[0] = int_mirror(mapper.int_y(), s.height);
    let xpos = mapper.int_x();
    let width = s.width;
    let dst = &mut xy[1..];
    if width == 1 {
        for i in 0..(count as usize) {
            write_u16_at(dst, i, 0);
        }
        return;
    }
    let mut out_idx = 0usize;
    let mut start = sk_int_mod(xpos, 2 * width);
    let mut forward;
    let mut n;
    if start >= width {
        start = width + !(start - width);
        forward = false;
        n = start + 1;
    } else {
        forward = true;
        n = width - start;
    }
    if n > count {
        n = count;
    }
    if forward {
        fill_sequential(dst, out_idx, start, n as usize);
    } else {
        fill_backwards(dst, out_idx, start, n as usize);
    }
    forward = !forward;
    out_idx += n as usize;
    count -= n;

    while count >= width {
        if forward {
            fill_sequential(dst, out_idx, 0, width as usize);
        } else {
            fill_backwards(dst, out_idx, width - 1, width as usize);
        }
        forward = !forward;
        out_idx += width as usize;
        count -= width;
    }
    if count > 0 {
        if forward {
            fill_sequential(dst, out_idx, 0, count as usize);
        } else {
            fill_backwards(dst, out_idx, width - 1, count as usize);
        }
    }
}

/// Dispatches and executes any of the 15 `SkBitmapProcState::MatrixProc` implementations.
pub fn run_matrix_proc(
    kind: MatrixProcKind,
    s: &ProcStateParams,
    xy: &mut [u32],
    count: i32,
    x: i32,
    y: i32,
) {
    match kind {
        MatrixProcKind::None => {}
        MatrixProcKind::ClampNofilterTrans => clampx_nofilter_trans(s, xy, count, x, y),
        MatrixProcKind::RepeatNofilterTrans => repeatx_nofilter_trans(s, xy, count, x, y),
        MatrixProcKind::MirrorNofilterTrans => mirrorx_nofilter_trans(s, xy, count, x, y),
        MatrixProcKind::ClampNofilterScale => {
            nofilter_scale(s, xy, count, x, y, tile_clamp, tile_clamp, true)
        }
        MatrixProcKind::ClampFilterScale => filter_scale(
            s,
            xy,
            count,
            x,
            y,
            tile_clamp,
            tile_clamp,
            extract_low_bits_clamp_clamp,
            true,
        ),
        MatrixProcKind::ClampNofilterAffine => {
            nofilter_affine(s, xy, count, x, y, tile_clamp, tile_clamp)
        }
        MatrixProcKind::ClampFilterAffine => filter_affine(
            s,
            xy,
            count,
            x,
            y,
            tile_clamp,
            tile_clamp,
            extract_low_bits_clamp_clamp,
        ),
        MatrixProcKind::RepeatNofilterScale => {
            nofilter_scale(s, xy, count, x, y, tile_repeat, tile_repeat, false)
        }
        MatrixProcKind::RepeatFilterScale => filter_scale(
            s,
            xy,
            count,
            x,
            y,
            tile_repeat,
            tile_repeat,
            extract_low_bits_general,
            false,
        ),
        MatrixProcKind::RepeatNofilterAffine => {
            nofilter_affine(s, xy, count, x, y, tile_repeat, tile_repeat)
        }
        MatrixProcKind::RepeatFilterAffine => filter_affine(
            s,
            xy,
            count,
            x,
            y,
            tile_repeat,
            tile_repeat,
            extract_low_bits_general,
        ),
        MatrixProcKind::MirrorNofilterScale => {
            nofilter_scale(s, xy, count, x, y, tile_mirror, tile_mirror, false)
        }
        MatrixProcKind::MirrorFilterScale => filter_scale(
            s,
            xy,
            count,
            x,
            y,
            tile_mirror,
            tile_mirror,
            extract_low_bits_general,
            false,
        ),
        MatrixProcKind::MirrorNofilterAffine => {
            nofilter_affine(s, xy, count, x, y, tile_mirror, tile_mirror)
        }
        MatrixProcKind::MirrorFilterAffine => filter_affine(
            s,
            xy,
            count,
            x,
            y,
            tile_mirror,
            tile_mirror,
            extract_low_bits_general,
        ),
    }
}

// ============================================================================
// Sample Procs (`SkBitmapProcState.cpp` & `SkBitmapProcState_opts.h`)
// ============================================================================

#[inline]
fn read_pixel32(pixels: &[u8], row_bytes: usize, x: usize, y: usize) -> u32 {
    let off = y * row_bytes + x * 4;
    u32::from_ne_bytes([
        pixels[off],
        pixels[off + 1],
        pixels[off + 2],
        pixels[off + 3],
    ])
}

#[inline]
pub const fn decode_packed_coordinates_and_weight(packed: u32) -> (usize, usize, u32) {
    let v0 = (packed >> 18) as usize;
    let v1 = (packed & 0x3FFF) as usize;
    let w = (packed >> 14) & 0xF;
    (v0, v1, w)
}

/// Exact 4-bit bilinear filter + alpha scaling matching `SkBitmapProcState_opts.h` bit-for-bit.
#[inline]
pub const fn filter_and_scale_by_alpha(
    wx: u32,
    wy: u32,
    a00: u32,
    a01: u32,
    a10: u32,
    a11: u32,
    alpha_scale: u32,
) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    let xy = wx * wy;

    let scale00 = (16 - wx) * (16 - wy);
    let mut lo = (a00 & MASK) * scale00;
    let mut hi = ((a00 >> 8) & MASK) * scale00;

    let scale01 = 16 * wx - xy;
    lo += (a01 & MASK) * scale01;
    hi += ((a01 >> 8) & MASK) * scale01;

    let scale10 = 16 * wy - xy;
    lo += (a10 & MASK) * scale10;
    hi += ((a10 >> 8) & MASK) * scale10;

    lo += (a11 & MASK) * xy;
    hi += ((a11 >> 8) & MASK) * xy;

    if alpha_scale < 256 {
        lo = ((lo >> 8) & MASK) * alpha_scale;
        hi = ((hi >> 8) & MASK) * alpha_scale;
    }

    ((lo >> 8) & MASK) | (hi & !MASK)
}

/// Exact port of `filter_32_alpha` from `SkBitmapProcState.cpp`.
#[inline]
pub const fn filter_32_alpha(t: u32, color0: u32, color1: u32, alpha_scale: u32) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    let scale0 = 256 - 16 * t;
    let mut lo = (color0 & MASK) * scale0;
    let mut hi = ((color0 >> 8) & MASK) * scale0;

    let scale1 = 16 * t;
    lo += (color1 & MASK) * scale1;
    hi += ((color1 >> 8) & MASK) * scale1;

    lo = ((lo >> 8) & MASK) * alpha_scale;
    hi = ((hi >> 8) & MASK) * alpha_scale;

    ((lo >> 8) & MASK) | (hi & !MASK)
}

/// Exact port of `SkOpts::S32_alpha_D32_filter_DX`.
pub fn s32_alpha_d32_filter_dx(
    pixels: &[u8],
    row_bytes: usize,
    alpha_scale: u32,
    xy: &[u32],
    colors: &mut [u32],
) {
    if colors.is_empty() {
        return;
    }
    let (y0, y1, wy) = decode_packed_coordinates_and_weight(xy[0]);
    let row0_off = y0 * row_bytes;
    let row1_off = y1 * row_bytes;

    for (dst, &packed_x) in colors.iter_mut().zip(&xy[1..]) {
        let (x0, x1, wx) = decode_packed_coordinates_and_weight(packed_x);
        let a00 = u32::from_ne_bytes([
            pixels[row0_off + x0 * 4],
            pixels[row0_off + x0 * 4 + 1],
            pixels[row0_off + x0 * 4 + 2],
            pixels[row0_off + x0 * 4 + 3],
        ]);
        let a01 = u32::from_ne_bytes([
            pixels[row0_off + x1 * 4],
            pixels[row0_off + x1 * 4 + 1],
            pixels[row0_off + x1 * 4 + 2],
            pixels[row0_off + x1 * 4 + 3],
        ]);
        let a10 = u32::from_ne_bytes([
            pixels[row1_off + x0 * 4],
            pixels[row1_off + x0 * 4 + 1],
            pixels[row1_off + x0 * 4 + 2],
            pixels[row1_off + x0 * 4 + 3],
        ]);
        let a11 = u32::from_ne_bytes([
            pixels[row1_off + x1 * 4],
            pixels[row1_off + x1 * 4 + 1],
            pixels[row1_off + x1 * 4 + 2],
            pixels[row1_off + x1 * 4 + 3],
        ]);
        *dst = filter_and_scale_by_alpha(wx, wy, a00, a01, a10, a11, alpha_scale);
    }
}

/// Exact port of `SkOpts::S32_alpha_D32_filter_DXDY`.
pub fn s32_alpha_d32_filter_dxdy(
    pixels: &[u8],
    row_bytes: usize,
    alpha_scale: u32,
    xy: &[u32],
    colors: &mut [u32],
) {
    for (i, dst) in colors.iter_mut().enumerate() {
        let (y0, y1, wy) = decode_packed_coordinates_and_weight(xy[2 * i]);
        let (x0, x1, wx) = decode_packed_coordinates_and_weight(xy[2 * i + 1]);
        let a00 = read_pixel32(pixels, row_bytes, x0, y0);
        let a01 = read_pixel32(pixels, row_bytes, x1, y0);
        let a10 = read_pixel32(pixels, row_bytes, x0, y1);
        let a11 = read_pixel32(pixels, row_bytes, x1, y1);
        *dst = filter_and_scale_by_alpha(wx, wy, a00, a01, a10, a11, alpha_scale);
    }
}

/// Exact port of `S32_alpha_D32_nofilter_DX`.
pub fn s32_alpha_d32_nofilter_dx(
    pixels: &[u8],
    width: i32,
    row_bytes: usize,
    alpha_scale: u32,
    xy: &[u32],
    colors: &mut [u32],
) {
    if colors.is_empty() {
        return;
    }
    let y = xy[0] as usize;
    if width == 1 {
        let c = alpha_mul_q(read_pixel32(pixels, row_bytes, 0, y), alpha_scale);
        colors.fill(c);
        return;
    }

    let row_off = y * row_bytes;
    let x_words = &xy[1..];
    for (i, dst) in colors.iter_mut().enumerate() {
        let x = read_u16_at(x_words, i);
        let p = u32::from_ne_bytes([
            pixels[row_off + x * 4],
            pixels[row_off + x * 4 + 1],
            pixels[row_off + x * 4 + 2],
            pixels[row_off + x * 4 + 3],
        ]);
        *dst = alpha_mul_q(p, alpha_scale);
    }
}

/// Exact port of `S32_alpha_D32_nofilter_DXDY`.
pub fn s32_alpha_d32_nofilter_dxdy(
    pixels: &[u8],
    row_bytes: usize,
    alpha_scale: u32,
    xy: &[u32],
    colors: &mut [u32],
) {
    for (dst, &packed) in colors.iter_mut().zip(xy.iter()) {
        let x = (packed & 0xFFFF) as usize;
        let y = (packed >> 16) as usize;
        *dst = alpha_mul_q(read_pixel32(pixels, row_bytes, x, y), alpha_scale);
    }
}

// ============================================================================
// Shader Procs (`SkBitmapProcState.cpp`)
// ============================================================================

/// Exact port of `Clamp_S32_opaque_D32_nofilter_DX_shaderproc`.
pub fn clamp_s32_opaque_d32_nofilter_dx_shaderproc(
    s: &ProcStateParams,
    pixels: &[u8],
    row_bytes: usize,
    x: i32,
    y: i32,
    dst: &mut [u32],
) {
    let count = dst.len() as i32;
    if count <= 0 {
        return;
    }
    let max_x = (s.width - 1) as u32;
    let max_y = s.height - 1;
    let mapper = AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
    let dst_y = pin_i32(mapper.int_y(), 0, max_y) as usize;
    let mut fx = mapper.fx;
    let dx = s.inv_sx;
    let row_off = dst_y * row_bytes;

    let first_idx = fixed3232_to_int(fx) as i64 as u64;
    let last_idx = fixed3232_to_int(fx.wrapping_add(dx.wrapping_mul((count - 1) as i64))) as i64 as u64;
    if first_idx <= max_x as u64 && last_idx <= max_x as u64 {
        for out in dst.iter_mut() {
            let idx = fixed3232_to_int(fx) as usize;
            let off = row_off + idx * 4;
            *out = u32::from_ne_bytes([
                pixels[off],
                pixels[off + 1],
                pixels[off + 2],
                pixels[off + 3],
            ]);
            fx = fx.wrapping_add(dx);
        }
    } else {
        let max_x_i = max_x as i32;
        for out in dst.iter_mut() {
            let idx = pin_i32(fixed3232_to_int(fx), 0, max_x_i) as usize;
            let off = row_off + idx * 4;
            *out = u32::from_ne_bytes([
                pixels[off],
                pixels[off + 1],
                pixels[off + 2],
                pixels[off + 3],
            ]);
            fx = fx.wrapping_add(dx);
        }
    }
}

/// Exact port of `Clamp_S32_D32_nofilter_trans_shaderproc`.
pub fn clamp_s32_d32_nofilter_trans_shaderproc(
    s: &ProcStateParams,
    pixels: &[u8],
    row_bytes: usize,
    x: i32,
    y: i32,
    colors: &mut [u32],
) {
    let mut count = colors.len() as i32;
    if count <= 0 {
        return;
    }
    let max_x = s.width - 1;
    let max_y = s.height - 1;
    let mut ix = s.filter_one_x.wrapping_add(x);
    let iy = pin_i32(s.filter_one_y.wrapping_add(y), 0, max_y) as usize;
    let mut out_idx = 0usize;

    if ix < 0 {
        let n = (-ix).min(count) as usize;
        let left_px = read_pixel32(pixels, row_bytes, 0, iy);
        colors[out_idx..out_idx + n].fill(left_px);
        count -= n as i32;
        if count == 0 {
            return;
        }
        out_idx += n;
        ix = 0;
    }
    if ix <= max_x {
        let n = (max_x - ix + 1).min(count) as usize;
        for i in 0..n {
            colors[out_idx + i] = read_pixel32(pixels, row_bytes, (ix as usize) + i, iy);
        }
        count -= n as i32;
        if count == 0 {
            return;
        }
        out_idx += n;
    }
    let right_px = read_pixel32(pixels, row_bytes, max_x as usize, iy);
    colors[out_idx..out_idx + (count as usize)].fill(right_px);
}

/// Exact port of `Repeat_S32_D32_nofilter_trans_shaderproc`.
pub fn repeat_s32_d32_nofilter_trans_shaderproc(
    s: &ProcStateParams,
    pixels: &[u8],
    row_bytes: usize,
    x: i32,
    y: i32,
    colors: &mut [u32],
) {
    let mut count = colors.len() as i32;
    if count <= 0 {
        return;
    }
    let stop_x = s.width;
    let stop_y = s.height;
    let mut ix = sk_int_mod(s.filter_one_x.wrapping_add(x), stop_x);
    let iy = sk_int_mod(s.filter_one_y.wrapping_add(y), stop_y) as usize;
    let mut out_idx = 0usize;

    loop {
        let n = (stop_x - ix).min(count) as usize;
        for i in 0..n {
            colors[out_idx + i] = read_pixel32(pixels, row_bytes, (ix as usize) + i, iy);
        }
        count -= n as i32;
        if count == 0 {
            return;
        }
        out_idx += n;
        ix = 0;
    }
}

/// Exact port of `S32_D32_constX_shaderproc`.
pub fn s32_d32_const_x_shaderproc(
    s: &ProcStateParams,
    matrix_proc: MatrixProcKind,
    pixels: &[u8],
    row_bytes: usize,
    x: i32,
    y: i32,
    colors: &mut [u32],
) {
    if colors.is_empty() {
        return;
    }
    let color = if s.bilerp {
        let mut xy = [0u32; 2];
        run_matrix_proc(matrix_proc, s, &mut xy, 1, x, y);
        let (iy0, iy1, isub_y) = decode_packed_coordinates_and_weight(xy[0]);
        let row0 = read_pixel32(pixels, row_bytes, 0, iy0);
        let row1 = read_pixel32(pixels, row_bytes, 0, iy1);
        filter_32_alpha(isub_y, row0, row1, s.alpha_scale as u32)
    } else {
        let is_translate = (s.inv_matrix.get_type_cached() & !TRANSLATE_MASK) == 0;
        let tile_x = TileMode::from_i32(s.tile_mode_x).unwrap_or(TileMode::Clamp);
        let tile_y = TileMode::from_i32(s.tile_mode_y).unwrap_or(TileMode::Clamp);
        let y_temp = if is_translate {
            s.filter_one_y.wrapping_add(y)
        } else {
            let mapper =
                AutoMapper::new(&s.inv_matrix, s.bilerp, s.filter_one_x, s.filter_one_y, x, y);
            if tile_x != TileMode::Clamp || tile_y != TileMode::Clamp {
                fixed3232_to_int(mapper.fy.wrapping_mul(s.height as i64))
            } else {
                mapper.int_y()
            }
        };
        let stop_y = s.height;
        let iy0 = match tile_y {
            TileMode::Clamp => pin_i32(y_temp, 0, stop_y - 1),
            TileMode::Repeat => sk_int_mod(y_temp, stop_y),
            TileMode::Mirror | TileMode::Decal => sk_int_mirror(y_temp, stop_y),
        } as usize;
        let p = read_pixel32(pixels, row_bytes, 0, iy0);
        if s.alpha_scale < 256 {
            alpha_mul_q(p, s.alpha_scale as u32)
        } else {
            p
        }
    };
    colors.fill(color);
}

/// Exact port of `DoNothing_shaderproc`.
#[inline]
pub fn do_nothing_shaderproc(colors: &mut [u32]) {
    colors.fill(0);
}

/// Debug validator matching `SkBitmapProcState::DebugMatrixProc` checks.
pub fn validate_matrix_proc_output(
    xy: &[u32],
    count: usize,
    width: u32,
    height: u32,
    is_scale_translate: bool,
    bilerp: bool,
) -> bool {
    if is_scale_translate {
        if bilerp {
            let (y0, y1, _) = decode_packed_coordinates_and_weight(xy[0]);
            if (y0 as u32) >= height || (y1 as u32) >= height {
                return false;
            }
            for &xx in &xy[1..1 + count] {
                let (x0, x1, _) = decode_packed_coordinates_and_weight(xx);
                if (x0 as u32) >= width || (x1 as u32) >= width {
                    return false;
                }
            }
        } else {
            if xy[0] >= height {
                return false;
            }
            for i in 0..count {
                if (read_u16_at(&xy[1..], i) as u32) >= width {
                    return false;
                }
            }
        }
    } else if bilerp {
        for i in 0..count {
            let (y0, y1, _) = decode_packed_coordinates_and_weight(xy[2 * i]);
            let (x0, x1, _) = decode_packed_coordinates_and_weight(xy[2 * i + 1]);
            if (y0 as u32) >= height
                || (y1 as u32) >= height
                || (x0 as u32) >= width
                || (x1 as u32) >= width
            {
                return false;
            }
        }
    } else {
        for &packed in &xy[..count] {
            let x = packed & 0xFFFF;
            let y = packed >> 16;
            if x >= width || y >= height {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_clamp_repeat_mirror() {
        // Clamp at 0.5 -> low=0, weight=8, high=1
        let p = pack_clamp(SK_FIXED_1 / 2, 10);
        assert_eq!(decode_packed_coordinates_and_weight(p), (0, 1, 8));
        // Clamp negative -> (0, 0, weight)
        let p_neg = pack_clamp(-SK_FIXED_1, 10);
        let (v0, v1, _) = decode_packed_coordinates_and_weight(p_neg);
        assert_eq!((v0, v1), (0, 0));
        // Repeat wrapping at end of width=4
        let p_rep = pack_repeat(SK_FIXED_1 - 1, 3, 4);
        let (r0, r1, _) = decode_packed_coordinates_and_weight(p_rep);
        assert_eq!(r0, 3);
        assert_eq!(r1, 0);
        // Mirror sanity
        let p_mir = pack_mirror(SK_FIXED_1 / 2, 3, 4);
        let (m0, m1, _) = decode_packed_coordinates_and_weight(p_mir);
        assert!(m0 <= 3 && m1 <= 3);
    }

    #[test]
    fn bilinear_filter_exact_weights() {
        let a00 = 0xFF_00_00_00u32;
        let a01 = 0xFF_FF_00_00u32;
        let a10 = 0xFF_00_FF_00u32;
        let a11 = 0xFF_FF_FF_FFu32;
        // wx=0, wy=0, alpha=256 -> a00
        assert_eq!(filter_and_scale_by_alpha(0, 0, a00, a01, a10, a11, 256), a00);
        // wx=8, wy=8, alpha=256 -> average of all 4 = (0+255+0+255)/4 = 127, etc.
        let mid = filter_and_scale_by_alpha(8, 8, a00, a01, a10, a11, 256);
        assert_eq!(mid >> 24, 0xFF);
        assert_eq!((mid >> 16) & 0xFF, 127);
        assert_eq!((mid >> 8) & 0xFF, 127);
        assert_eq!(mid & 0xFF, 63);
        // Half alpha (scale=128)
        let half = filter_and_scale_by_alpha(0, 0, 0xFF_80_40_20, 0, 0, 0, 128);
        assert_eq!(half, 0x7F_40_20_10);
    }

    #[test]
    fn sk_int_mod_and_mirror_examples() {
        // From Skia's own doc comment for n=5:
        let expected_mod = [2, 3, 4, 0, 1, 2, 3, 4, 0, 1, 2, 3, 4, 0, 1, 2, 3];
        for (i, x) in (-8..=8).enumerate() {
            assert_eq!(sk_int_mod(x, 5), expected_mod[i]);
        }
        assert_eq!(sk_int_mirror(0, 4), 0);
        assert_eq!(sk_int_mirror(3, 4), 3);
        assert_eq!(sk_int_mirror(4, 4), 3);
        assert_eq!(sk_int_mirror(7, 4), 0);
        assert_eq!(sk_int_mirror(-1, 4), 0);
    }

    #[test]
    fn matrix_and_sample_proc_end_to_end() {
        // 4x2 solid-colored test image
        let mut pixels = vec![0u8; 4 * 2 * 4];
        for y in 0..2 {
            for x in 0..4 {
                let c = 0xFF00_0000u32 | ((x as u32 * 40) << 16) | ((y as u32 * 80) << 8) | 0x10;
                let off = y * 16 + x * 4;
                pixels[off..off + 4].copy_from_slice(&c.to_ne_bytes());
            }
        }
        let mut inv = Matrix::scale_translate(0.5, 0.5, 0.0, 0.0);
        let mut bilerp = true;
        init_adjust_matrix_and_bilerp(&mut inv, 4, 2, TileMode::Clamp, TileMode::Clamp, &mut bilerp);
        assert!(bilerp);
        let chosen = choose_procs(
            &inv,
            ColorType::Bgra8888,
            ColorType::Bgra8888,
            4,
            2,
            TileMode::Clamp,
            TileMode::Clamp,
            bilerp,
            255,
            0,
            0,
        );
        assert_eq!(chosen.matrix_proc, MatrixProcKind::ClampFilterScale);
        let params = ProcStateParams {
            inv_matrix: inv,
            inv_sx: chosen.inv_sx,
            inv_ky: chosen.inv_ky,
            filter_one_x: chosen.filter_one_x,
            filter_one_y: chosen.filter_one_y,
            width: 4,
            height: 2,
            tile_mode_x: TileMode::Clamp as i32,
            tile_mode_y: TileMode::Clamp as i32,
            alpha_scale: chosen.alpha_scale,
            bilerp,
        };
        let mut xy = [0u32; 9];
        run_matrix_proc(chosen.matrix_proc, &params, &mut xy, 8, 0, 0);
        assert!(validate_matrix_proc_output(&xy, 8, 4, 2, true, true));

        let mut out = [0u32; 8];
        s32_alpha_d32_filter_dx(&pixels, 16, chosen.alpha_scale as u32, &xy, &mut out);
        for &c in &out {
            assert_eq!(c >> 24, 0xFF);
            assert_eq!(c & 0xFF, 0x10);
        }
    }

    #[test]
    fn max_count_for_buffer_size_cases() {
        assert_eq!(max_count_for_buffer_size(64, true, false), 30);
        assert_eq!(max_count_for_buffer_size(64, true, true), 15);
        assert_eq!(max_count_for_buffer_size(64, false, false), 16);
        assert_eq!(max_count_for_buffer_size(64, false, true), 8);
        assert_eq!(max_count_for_buffer_size(2, true, false), 0);
    }

    #[test]
    fn filter_and_scale_by_alpha_exhaustive_weights_no_underflow() {
        for wy in 0..16u32 {
            for wx in 0..16u32 {
                let c = filter_and_scale_by_alpha(
                    wx,
                    wy,
                    0xFF_10_20_30,
                    0xFF_40_50_60,
                    0xFF_70_80_90,
                    0xFF_A0_B0_C0,
                    256,
                );
                assert_eq!(c >> 24, 0xFF);
                // Uniform color must stay exact for all (wx, wy)
                let u = filter_and_scale_by_alpha(
                    wx,
                    wy,
                    0xCC_88_44_22,
                    0xCC_88_44_22,
                    0xCC_88_44_22,
                    0xCC_88_44_22,
                    256,
                );
                assert_eq!(u, 0xCC_88_44_22);
            }
        }
    }

    #[test]
    fn all_15_matrix_procs_and_sample_procs() {
        let mut pixels = vec![0u8; 6 * 5 * 4];
        for y in 0..5 {
            for x in 0..6 {
                let c = 0xFF00_0000u32 | ((x as u32 * 30) << 16) | ((y as u32 * 40) << 8) | 0x22;
                let off = y * 24 + x * 4;
                pixels[off..off + 4].copy_from_slice(&c.to_ne_bytes());
            }
        }

        let cases = [
            (TileMode::Clamp, Matrix::translate(-2.0, -1.0), false),
            (TileMode::Repeat, Matrix::translate(-2.0, -1.0), false),
            (TileMode::Mirror, Matrix::translate(-2.0, -1.0), false),
            (TileMode::Clamp, Matrix::scale_translate(0.5, 0.5, -1.0, 0.0), false),
            (TileMode::Clamp, Matrix::scale_translate(0.5, 0.5, 0.25, 0.25), true),
            (TileMode::Repeat, Matrix::scale_translate(0.5, 0.5, 0.1, 0.2), false),
            (TileMode::Repeat, Matrix::scale_translate(0.5, 0.5, 0.1, 0.2), true),
            (TileMode::Mirror, Matrix::scale_translate(0.5, 0.5, 0.1, 0.2), false),
            (TileMode::Mirror, Matrix::scale_translate(0.5, 0.5, 0.1, 0.2), true),
            (TileMode::Clamp, Matrix::make_all(0.7, 0.3, 0.0, -0.3, 0.7, 0.0, 0.0, 0.0, 1.0), false),
            (TileMode::Clamp, Matrix::make_all(0.7, 0.3, 0.0, -0.3, 0.7, 0.0, 0.0, 0.0, 1.0), true),
            (TileMode::Repeat, Matrix::make_all(0.7, 0.3, 0.0, -0.3, 0.7, 0.0, 0.0, 0.0, 1.0), false),
            (TileMode::Repeat, Matrix::make_all(0.7, 0.3, 0.0, -0.3, 0.7, 0.0, 0.0, 0.0, 1.0), true),
            (TileMode::Mirror, Matrix::make_all(0.7, 0.3, 0.0, -0.3, 0.7, 0.0, 0.0, 0.0, 1.0), false),
            (TileMode::Mirror, Matrix::make_all(0.7, 0.3, 0.0, -0.3, 0.7, 0.0, 0.0, 0.0, 1.0), true),
        ];

        for (tm, mut inv, mut bilerp) in cases {
            init_adjust_matrix_and_bilerp(&mut inv, 6, 5, tm, tm, &mut bilerp);
            let chosen = choose_procs(
                &inv,
                ColorType::Bgra8888,
                ColorType::Bgra8888,
                6,
                5,
                tm,
                tm,
                bilerp,
                200,
                0,
                0,
            );
            assert_ne!(chosen.matrix_proc, MatrixProcKind::None);
            let params = ProcStateParams {
                inv_matrix: inv,
                inv_sx: chosen.inv_sx,
                inv_ky: chosen.inv_ky,
                filter_one_x: chosen.filter_one_x,
                filter_one_y: chosen.filter_one_y,
                width: 6,
                height: 5,
                tile_mode_x: tm as i32,
                tile_mode_y: tm as i32,
                alpha_scale: chosen.alpha_scale,
                bilerp,
            };
            let count = 11usize;
            let mut xy = vec![0u32; 2 * count + 2];
            run_matrix_proc(chosen.matrix_proc, &params, &mut xy, count as i32, -2, 1);
            assert!(validate_matrix_proc_output(
                &xy,
                count,
                6,
                5,
                chosen.is_scale_translate,
                bilerp
            ));
            let mut out = vec![0u32; count];
            if chosen.is_scale_translate {
                if bilerp {
                    s32_alpha_d32_filter_dx(&pixels, 24, chosen.alpha_scale as u32, &xy, &mut out);
                } else {
                    s32_alpha_d32_nofilter_dx(&pixels, 6, 24, chosen.alpha_scale as u32, &xy, &mut out);
                }
            } else if bilerp {
                s32_alpha_d32_filter_dxdy(&pixels, 24, chosen.alpha_scale as u32, &xy, &mut out);
            } else {
                s32_alpha_d32_nofilter_dxdy(&pixels, 24, chosen.alpha_scale as u32, &xy, &mut out);
            }
            for &c in &out {
                assert_ne!(c, 0);
            }
        }
    }

    #[test]
    fn all_shader_procs() {
        // 3x2 image
        let mut pixels = vec![0u8; 3 * 2 * 4];
        for y in 0..2 {
            for x in 0..3 {
                let c = 0xFF00_0000u32 | ((x as u32 + 1) << 16) | ((y as u32 + 1) << 8);
                pixels[y * 12 + x * 4..y * 12 + x * 4 + 4].copy_from_slice(&c.to_ne_bytes());
            }
        }

        // Clamp opaque nofilter DX
        let inv = Matrix::scale_translate(1.0, 1.0, -1.0, 0.0);
        let chosen = choose_procs(&inv, ColorType::Bgra8888, ColorType::Bgra8888, 3, 2, TileMode::Clamp, TileMode::Clamp, false, 255, 0, 0);
        assert_eq!(chosen.shader_proc, ShaderProcKind::ClampOpaqueNofilterDx);
        let params = ProcStateParams {
            inv_matrix: inv,
            inv_sx: chosen.inv_sx,
            inv_ky: chosen.inv_ky,
            filter_one_x: chosen.filter_one_x,
            filter_one_y: chosen.filter_one_y,
            width: 3,
            height: 2,
            tile_mode_x: TileMode::Clamp as i32,
            tile_mode_y: TileMode::Clamp as i32,
            alpha_scale: chosen.alpha_scale,
            bilerp: false,
        };
        let mut dst = [0u32; 5];
        clamp_s32_opaque_d32_nofilter_dx_shaderproc(&params, &pixels, 12, 0, 0, &mut dst);
        assert_eq!(dst[0], 0xFF01_0100); // clamped left
        assert_eq!(dst[1], 0xFF01_0100);
        assert_eq!(dst[2], 0xFF02_0100);
        assert_eq!(dst[3], 0xFF03_0100);
        assert_eq!(dst[4], 0xFF03_0100); // clamped right

        // Repeat nofilter trans
        let inv_rep = Matrix::translate(0.0, 0.0);
        let chosen_rep = choose_procs(&inv_rep, ColorType::Bgra8888, ColorType::Bgra8888, 3, 2, TileMode::Repeat, TileMode::Repeat, false, 255, 0, 0);
        assert_eq!(chosen_rep.shader_proc, ShaderProcKind::RepeatNofilterTrans);
        let params_rep = ProcStateParams {
            inv_matrix: inv_rep,
            inv_sx: chosen_rep.inv_sx,
            inv_ky: chosen_rep.inv_ky,
            filter_one_x: chosen_rep.filter_one_x,
            filter_one_y: chosen_rep.filter_one_y,
            width: 3,
            height: 2,
            tile_mode_x: TileMode::Repeat as i32,
            tile_mode_y: TileMode::Repeat as i32,
            alpha_scale: chosen_rep.alpha_scale,
            bilerp: false,
        };
        let mut dst_rep = [0u32; 5];
        repeat_s32_d32_nofilter_trans_shaderproc(&params_rep, &pixels, 12, 1, 0, &mut dst_rep);
        assert_eq!(dst_rep, [0xFF02_0100, 0xFF03_0100, 0xFF01_0100, 0xFF02_0100, 0xFF03_0100]);

        // ConstX (width == 1, bilerp)
        let mut px1 = vec![0u8; 2 * 4];
        px1[0..4].copy_from_slice(&0xFF_00_00_00u32.to_ne_bytes());
        px1[4..8].copy_from_slice(&0xFF_FF_FF_FFu32.to_ne_bytes());
        let inv_1 = Matrix::scale_translate(1.0, 0.5, 0.0, 0.0);
        let chosen_1 = choose_procs(&inv_1, ColorType::Bgra8888, ColorType::Bgra8888, 1, 2, TileMode::Clamp, TileMode::Clamp, true, 255, 0, 0);
        assert_eq!(chosen_1.shader_proc, ShaderProcKind::ConstX);
        let params_1 = ProcStateParams {
            inv_matrix: inv_1,
            inv_sx: chosen_1.inv_sx,
            inv_ky: chosen_1.inv_ky,
            filter_one_x: chosen_1.filter_one_x,
            filter_one_y: chosen_1.filter_one_y,
            width: 1,
            height: 2,
            tile_mode_x: TileMode::Clamp as i32,
            tile_mode_y: TileMode::Clamp as i32,
            alpha_scale: chosen_1.alpha_scale,
            bilerp: true,
        };
        let mut dst_1 = [0u32; 4];
        s32_d32_const_x_shaderproc(&params_1, chosen_1.matrix_proc, &px1, 4, 0, 1, &mut dst_1);
        assert_eq!(dst_1[0], dst_1[3]);

        // DoNothing
        do_nothing_shaderproc(&mut dst_1);
        assert_eq!(dst_1, [0, 0, 0, 0]);
    }
}
