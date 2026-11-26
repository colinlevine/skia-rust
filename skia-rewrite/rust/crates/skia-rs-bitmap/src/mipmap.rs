//! Safe Rust port of:
//! - `src/core/SkMipmap.cpp`
//! - `src/core/SkMipmapAccessor.cpp`
//! - `src/core/SkMipmapBuilder.cpp`
//! - `src/core/SkMipmapDrawDownSampler.cpp`
//! - `src/core/SkMipmapHQDownSampler.cpp`

use skia_rs_color::image_info::{color_type_bytes_per_pixel, AlphaType, ColorType};
use skia_rs_geometry::{cpp_max_f32, cpp_min_f32, float_saturate2int, ISize, Matrix, Size};
use skia_rs_math::half::{float_to_half, half_to_float};

/// Exact port of `sk_float_round2int(x)` (using f64 `+ 0.5` then floor).
#[inline]
fn float_round2int(x: f32) -> i32 {
    float_saturate2int(((x as f64) + 0.5_f64).floor() as f32)
}

#[inline]
fn float_floor2int(x: f32) -> i32 {
    float_saturate2int(x.floor())
}

/// Exact port of `SkMipmap::ComputeLevelCount(int baseWidth, int baseHeight)`.
pub fn compute_level_count(base_width: i32, base_height: i32) -> i32 {
    if base_width < 1 || base_height < 1 {
        return 0;
    }
    let largest_axis = base_width.max(base_height);
    if largest_axis < 2 {
        return 0;
    }
    let leading_zeros = (largest_axis as u32).leading_zeros() as i32;
    let significant_bits = 32 - leading_zeros;
    let mut mip_level_count = significant_bits;
    if mip_level_count > 0 {
        mip_level_count -= 1;
    }
    mip_level_count
}

/// Exact port of `SkMipmap::ComputeLevelSize(int baseWidth, int baseHeight, int level)`.
pub fn compute_level_size(base_width: i32, base_height: i32, level: i32) -> ISize {
    if base_width < 1 || base_height < 1 {
        return ISize { width: 0, height: 0 };
    }
    let max_level_count = compute_level_count(base_width, base_height);
    if level >= max_level_count || level < 0 {
        return ISize { width: 0, height: 0 };
    }
    let width = (base_width >> (level + 1)).max(1);
    let height = (base_height >> (level + 1)).max(1);
    ISize { width, height }
}

/// Exact port of `SkMipmap::AllocLevelsSize(int levelCount, size_t pixelSize)`.
pub fn alloc_levels_size(level_count: i32, level_struct_size: usize, pixel_size: usize) -> usize {
    if level_count < 0 || pixel_size > i32::MAX as usize {
        return 0;
    }
    let Some(levels_bytes) = ((level_count as i64) + 1).checked_mul(level_struct_size as i64) else {
        return 0;
    };
    let Some(total) = levels_bytes.checked_add(pixel_size as i64) else {
        return 0;
    };
    if total < 0 || total > i32::MAX as i64 {
        return 0;
    }
    total as usize
}

/// Computes total pixel bytes needed for all mip levels (`SkMipmap::Build` pre-pass).
pub fn compute_total_pixel_size(base_width: i32, base_height: i32, ct: ColorType) -> usize {
    let count_levels = compute_level_count(base_width, base_height);
    let bpp = color_type_bytes_per_pixel(ct) as usize;
    let mut size = 0usize;
    for level in (0..=count_levels).rev() {
        let mip_size = compute_level_size(base_width, base_height, level);
        let row_bytes = (mip_size.width as usize).wrapping_mul(bpp);
        size = size.wrapping_add(row_bytes.wrapping_mul(mip_size.height as usize));
    }
    size
}

/// Exact port of `SkMipmap::ComputeLevel(SkSize scaleSize)`.
pub fn compute_level(scale_size: Size, legacy_anisotropic: bool) -> f32 {
    let scale = if legacy_anisotropic {
        (scale_size.width * scale_size.height).sqrt()
    } else {
        cpp_min_f32(scale_size.width, scale_size.height)
    };

    if scale >= 1.0_f32 || scale <= 0.0_f32 || !scale.is_finite() {
        return -1.0_f32;
    }

    let l = cpp_max_f32(-scale.log2() - 0.5_f32, 0.0_f32);
    if !l.is_finite() {
        return -1.0_f32;
    }
    l
}

/// Selects the 0-based mipmap level index for `SkMipmap::extractLevel`, or `None` if base level should be used.
pub fn select_extract_level(
    scale_size: Size,
    level_count: i32,
    legacy_anisotropic: bool,
) -> Option<usize> {
    let l = compute_level(scale_size, legacy_anisotropic);
    let mut level = float_round2int(l);
    if level <= 0 {
        return None;
    }
    if level > level_count {
        level = level_count;
    }
    if level <= 0 {
        return None;
    }
    Some((level - 1) as usize)
}

/// Validates root dimensions & level0 dimensions for `SkMipmap::validForRootLevel`.
pub fn valid_for_root_level_header(
    root_width: i32,
    root_height: i32,
    level0_width: i32,
    level0_height: i32,
) -> bool {
    if root_width <= 1 && root_height <= 1 {
        return false;
    }
    level0_width == (root_width >> 1).max(1) && level0_height == (root_height >> 1).max(1)
}

/// Validates all levels for `SkMipmap::validForRootLevel`.
pub fn valid_for_root_level(
    root_width: i32,
    root_height: i32,
    root_ct: ColorType,
    root_at: AlphaType,
    level0_width: i32,
    level0_height: i32,
    level_cts: &[i32],
    level_ats: &[i32],
) -> bool {
    if !valid_for_root_level_header(root_width, root_height, level0_width, level0_height) {
        return false;
    }
    let root_ct_i = root_ct as i32;
    let root_at_i = root_at as i32;
    for (&ct, &at) in level_cts.iter().zip(level_ats.iter()) {
        if ct != root_ct_i || at != root_at_i {
            return false;
        }
    }
    true
}

/// Matches `enum class SkMipmapMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MipmapMode {
    None = 0,
    Nearest = 1,
    Linear = 2,
}

impl MipmapMode {
    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Nearest),
            2 => Some(Self::Linear),
            _ => None,
        }
    }
}

/// Planning output for `SkMipmapAccessor::SkMipmapAccessor`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MipmapAccessorPlan {
    pub resolved_mode: i32,
    pub level_num: i32,
    pub lower_weight: f32,
    pub need_mips: bool,
}

/// Exact port of the level/weight/mode calculation in `SkMipmapAccessor::SkMipmapAccessor`.
pub fn plan_mipmap_accessor(
    inv: &Matrix,
    requested_mode: MipmapMode,
    legacy_anisotropic: bool,
) -> MipmapAccessorPlan {
    let mut resolved_mode = requested_mode;
    let mut level = 0.0_f32;

    if requested_mode != MipmapMode::None {
        match inv.decompose_scale() {
            None => {
                resolved_mode = MipmapMode::None;
            }
            Some((scale, _)) => {
                level = compute_level(
                    Size {
                        width: 1.0_f32 / scale.width,
                        height: 1.0_f32 / scale.height,
                    },
                    legacy_anisotropic,
                );
                if level <= 0.0_f32 {
                    resolved_mode = MipmapMode::None;
                    level = 0.0_f32;
                }
            }
        }
    }

    let level_num = if resolved_mode == MipmapMode::Nearest {
        float_round2int(level)
    } else {
        float_floor2int(level)
    };
    let lower_weight = level - (level_num as f32);
    let need_mips =
        level_num > 0 || (resolved_mode == MipmapMode::Linear && lower_weight > 0.0_f32);

    MipmapAccessorPlan {
        resolved_mode: resolved_mode as i32,
        level_num,
        lower_weight,
        need_mips,
    }
}

/// Exact port of `SkMipmapDrawDownSampler.cpp` `choose_options` predicate:
/// returns `true` if perfect 2x2 downscale (use bilerp), `false` for Catmull-Rom cubic.
#[inline]
pub const fn draw_downsampler_use_bilerp(dst_w: i32, dst_h: i32, src_w: i32, src_h: i32) -> bool {
    dst_w.wrapping_mul(2) == src_w && dst_h.wrapping_mul(2) == src_h
}

// ============================================================================
// HQ Downsampler (`src/core/SkMipmapHQDownSampler.cpp`)
// ============================================================================

trait WideOps: Copy {
    fn add(self, rhs: Self) -> Self;
    fn shr(self, bits: u32) -> Self;
    fn shl(self, bits: u32) -> Self;
    #[inline]
    fn add_121(a: Self, b: Self, c: Self) -> Self {
        a.add(b).add(b).add(c)
    }
}

impl WideOps for u32 {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.wrapping_add(rhs)
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        self >> bits
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        self.wrapping_shl(bits)
    }
}

impl WideOps for u64 {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.wrapping_add(rhs)
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        self >> bits
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        self.wrapping_shl(bits)
    }
}

impl WideOps for [u16; 4] {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        [
            self[0].wrapping_add(rhs[0]),
            self[1].wrapping_add(rhs[1]),
            self[2].wrapping_add(rhs[2]),
            self[3].wrapping_add(rhs[3]),
        ]
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        [self[0] >> bits, self[1] >> bits, self[2] >> bits, self[3] >> bits]
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        [
            self[0].wrapping_shl(bits),
            self[1].wrapping_shl(bits),
            self[2].wrapping_shl(bits),
            self[3].wrapping_shl(bits),
        ]
    }
}

impl WideOps for [u32; 4] {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        [
            self[0].wrapping_add(rhs[0]),
            self[1].wrapping_add(rhs[1]),
            self[2].wrapping_add(rhs[2]),
            self[3].wrapping_add(rhs[3]),
        ]
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        [self[0] >> bits, self[1] >> bits, self[2] >> bits, self[3] >> bits]
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        [
            self[0].wrapping_shl(bits),
            self[1].wrapping_shl(bits),
            self[2].wrapping_shl(bits),
            self[3].wrapping_shl(bits),
        ]
    }
}

impl WideOps for f32 {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self + rhs
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        self * (1.0_f32 / ((1u32 << bits) as f32))
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        self * ((1u32 << bits) as f32)
    }
}

impl WideOps for [f32; 2] {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        [self[0] + rhs[0], self[1] + rhs[1]]
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        let s = 1.0_f32 / ((1u32 << bits) as f32);
        [self[0] * s, self[1] * s]
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        let s = (1u32 << bits) as f32;
        [self[0] * s, self[1] * s]
    }
}

impl WideOps for [f32; 4] {
    #[inline]
    fn add(self, rhs: Self) -> Self {
        [
            self[0] + rhs[0],
            self[1] + rhs[1],
            self[2] + rhs[2],
            self[3] + rhs[3],
        ]
    }
    #[inline]
    fn shr(self, bits: u32) -> Self {
        let s = 1.0_f32 / ((1u32 << bits) as f32);
        [self[0] * s, self[1] * s, self[2] * s, self[3] * s]
    }
    #[inline]
    fn shl(self, bits: u32) -> Self {
        let s = (1u32 << bits) as f32;
        [self[0] * s, self[1] * s, self[2] * s, self[3] * s]
    }
}

trait ColorTypeFilter {
    const BPP: usize;
    type Wide: WideOps;
    fn load_expand(row: &[u8], idx: usize) -> Self::Wide;
    fn compact_store(row: &mut [u8], idx: usize, v: Self::Wide);
}

struct Filter8888;
impl ColorTypeFilter for Filter8888 {
    const BPP: usize = 4;
    type Wide = [u16; 4];
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> [u16; 4] {
        let o = idx * 4;
        [row[o] as u16, row[o + 1] as u16, row[o + 2] as u16, row[o + 3] as u16]
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, v: [u16; 4]) {
        let o = idx * 4;
        row[o] = v[0] as u8;
        row[o + 1] = v[1] as u8;
        row[o + 2] = v[2] as u8;
        row[o + 3] = v[3] as u8;
    }
}

struct Filter565;
impl ColorTypeFilter for Filter565 {
    const BPP: usize = 2;
    type Wide = u32;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u32 {
        const G_MASK: u32 = 0x07E0;
        let o = idx * 2;
        let x = u16::from_ne_bytes([row[o], row[o + 1]]) as u32;
        (x & !G_MASK) | ((x & G_MASK) << 16)
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u32) {
        const G_MASK: u32 = 0x07E0;
        let c = (((x & !G_MASK) & 0xFFFF) | ((x >> 16) & G_MASK)) as u16;
        let o = idx * 2;
        row[o..o + 2].copy_from_slice(&c.to_ne_bytes());
    }
}

struct Filter4444;
impl ColorTypeFilter for Filter4444 {
    const BPP: usize = 2;
    type Wide = u32;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u32 {
        let o = idx * 2;
        let x = u16::from_ne_bytes([row[o], row[o + 1]]) as u32;
        (x & 0x0F0F) | ((x & 0xF0F0) << 12)
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u32) {
        let c = ((x & 0x0F0F) | ((x >> 12) & 0xF0F0)) as u16;
        let o = idx * 2;
        row[o..o + 2].copy_from_slice(&c.to_ne_bytes());
    }
}

struct Filter8;
impl ColorTypeFilter for Filter8 {
    const BPP: usize = 1;
    type Wide = u32;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u32 {
        row[idx] as u32
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u32) {
        row[idx] = x as u8;
    }
}

struct FilterF16;
impl ColorTypeFilter for FilterF16 {
    const BPP: usize = 2;
    type Wide = f32;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> f32 {
        let o = idx * 2;
        half_to_float(u16::from_ne_bytes([row[o], row[o + 1]]))
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: f32) {
        let o = idx * 2;
        row[o..o + 2].copy_from_slice(&float_to_half(x).to_ne_bytes());
    }
}

struct FilterRgbaF16;
impl ColorTypeFilter for FilterRgbaF16 {
    const BPP: usize = 8;
    type Wide = [f32; 4];
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> [f32; 4] {
        let o = idx * 8;
        [
            half_to_float(u16::from_ne_bytes([row[o], row[o + 1]])),
            half_to_float(u16::from_ne_bytes([row[o + 2], row[o + 3]])),
            half_to_float(u16::from_ne_bytes([row[o + 4], row[o + 5]])),
            half_to_float(u16::from_ne_bytes([row[o + 6], row[o + 7]])),
        ]
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: [f32; 4]) {
        let o = idx * 8;
        row[o..o + 2].copy_from_slice(&float_to_half(x[0]).to_ne_bytes());
        row[o + 2..o + 4].copy_from_slice(&float_to_half(x[1]).to_ne_bytes());
        row[o + 4..o + 6].copy_from_slice(&float_to_half(x[2]).to_ne_bytes());
        row[o + 6..o + 8].copy_from_slice(&float_to_half(x[3]).to_ne_bytes());
    }
}

struct Filter88;
impl ColorTypeFilter for Filter88 {
    const BPP: usize = 2;
    type Wide = u32;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u32 {
        let o = idx * 2;
        let x = u16::from_ne_bytes([row[o], row[o + 1]]) as u32;
        (x & 0xFF) | ((x & 0xFF00) << 8)
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u32) {
        let c = ((x & 0xFF) | ((x >> 8) & 0xFF00)) as u16;
        let o = idx * 2;
        row[o..o + 2].copy_from_slice(&c.to_ne_bytes());
    }
}

/// Exact port of C++ `ColorTypeFilter_1616` (preserving C++'s exact 32-bit shift & uint16_t return).
struct Filter1616;
impl ColorTypeFilter for Filter1616 {
    const BPP: usize = 4;
    type Wide = u64;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u64 {
        let o = idx * 4;
        let x = u32::from_ne_bytes([row[o], row[o + 1], row[o + 2], row[o + 3]]);
        ((x & 0xFFFF) | ((x & !0xFFFF).wrapping_shl(16))) as u64
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u64) {
        let c = (((x & 0xFFFF) | ((x >> 16) & !0xFFFFu64)) as u16) as u32;
        let o = idx * 4;
        row[o..o + 4].copy_from_slice(&c.to_ne_bytes());
    }
}

struct FilterF16F16;
impl ColorTypeFilter for FilterF16F16 {
    const BPP: usize = 4;
    type Wide = [f32; 2];
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> [f32; 2] {
        let o = idx * 4;
        [
            half_to_float(u16::from_ne_bytes([row[o], row[o + 1]])),
            half_to_float(u16::from_ne_bytes([row[o + 2], row[o + 3]])),
        ]
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: [f32; 2]) {
        let o = idx * 4;
        row[o..o + 2].copy_from_slice(&float_to_half(x[0]).to_ne_bytes());
        row[o + 2..o + 4].copy_from_slice(&float_to_half(x[1]).to_ne_bytes());
    }
}

struct Filter16161616;
impl ColorTypeFilter for Filter16161616 {
    const BPP: usize = 8;
    type Wide = [u32; 4];
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> [u32; 4] {
        let o = idx * 8;
        [
            u16::from_ne_bytes([row[o], row[o + 1]]) as u32,
            u16::from_ne_bytes([row[o + 2], row[o + 3]]) as u32,
            u16::from_ne_bytes([row[o + 4], row[o + 5]]) as u32,
            u16::from_ne_bytes([row[o + 6], row[o + 7]]) as u32,
        ]
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: [u32; 4]) {
        let o = idx * 8;
        row[o..o + 2].copy_from_slice(&(x[0] as u16).to_ne_bytes());
        row[o + 2..o + 4].copy_from_slice(&(x[1] as u16).to_ne_bytes());
        row[o + 4..o + 6].copy_from_slice(&(x[2] as u16).to_ne_bytes());
        row[o + 6..o + 8].copy_from_slice(&(x[3] as u16).to_ne_bytes());
    }
}

struct Filter16;
impl ColorTypeFilter for Filter16 {
    const BPP: usize = 2;
    type Wide = u32;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u32 {
        let o = idx * 2;
        u16::from_ne_bytes([row[o], row[o + 1]]) as u32
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u32) {
        let o = idx * 2;
        row[o..o + 2].copy_from_slice(&(x as u16).to_ne_bytes());
    }
}

struct Filter1010102;
impl ColorTypeFilter for Filter1010102 {
    const BPP: usize = 4;
    type Wide = u64;
    #[inline]
    fn load_expand(row: &[u8], idx: usize) -> u64 {
        let o = idx * 4;
        let x = u32::from_ne_bytes([row[o], row[o + 1], row[o + 2], row[o + 3]]) as u64;
        (x & 0x3FF)
            | (((x >> 10) & 0x3FF) << 20)
            | (((x >> 20) & 0x3FF) << 40)
            | (((x >> 30) & 0x3) << 60)
    }
    #[inline]
    fn compact_store(row: &mut [u8], idx: usize, x: u64) {
        let c = ((x & 0x3FF)
            | (((x >> 20) & 0x3FF) << 10)
            | (((x >> 40) & 0x3FF) << 20)
            | (((x >> 60) & 0x3) << 30)) as u32;
        let o = idx * 4;
        row[o..o + 4].copy_from_slice(&c.to_ne_bytes());
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DownsampleKernel {
    K1x2,
    K1x3,
    K2x1,
    K2x2,
    K2x3,
    K3x1,
    K3x2,
    K3x3,
}

#[inline]
fn select_kernel(src_w: i32, src_h: i32) -> DownsampleKernel {
    if (src_h & 1) != 0 {
        if src_h == 1 {
            if (src_w & 1) != 0 {
                DownsampleKernel::K3x1
            } else {
                DownsampleKernel::K2x1
            }
        } else if (src_w & 1) != 0 {
            if src_w == 1 {
                DownsampleKernel::K1x3
            } else {
                DownsampleKernel::K3x3
            }
        } else {
            DownsampleKernel::K2x3
        }
    } else if (src_w & 1) != 0 {
        if src_w == 1 {
            DownsampleKernel::K1x2
        } else {
            DownsampleKernel::K3x2
        }
    } else {
        DownsampleKernel::K2x2
    }
}

fn downsample_rows<F: ColorTypeFilter>(
    kernel: DownsampleKernel,
    src: &[u8],
    src_rb: usize,
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    dst_rb: usize,
) {
    let _ = F::BPP;
    for y in 0..dst_h {
        let src_off = y * 2 * src_rb;
        let dst_row = &mut dst[y * dst_rb..];
        let p0 = &src[src_off..];
        match kernel {
            DownsampleKernel::K1x2 => {
                let p1 = &src[src_off + src_rb..];
                for i in 0..dst_w {
                    let s = i * 2;
                    let c = F::load_expand(p0, s).add(F::load_expand(p1, s));
                    F::compact_store(dst_row, i, c.shr(1));
                }
            }
            DownsampleKernel::K1x3 => {
                let p1 = &src[src_off + src_rb..];
                let p2 = &src[src_off + src_rb * 2..];
                for i in 0..dst_w {
                    let s = i * 2;
                    let c = F::Wide::add_121(
                        F::load_expand(p0, s),
                        F::load_expand(p1, s),
                        F::load_expand(p2, s),
                    );
                    F::compact_store(dst_row, i, c.shr(2));
                }
            }
            DownsampleKernel::K2x1 => {
                for i in 0..dst_w {
                    let s = i * 2;
                    let c = F::load_expand(p0, s).add(F::load_expand(p0, s + 1));
                    F::compact_store(dst_row, i, c.shr(1));
                }
            }
            DownsampleKernel::K2x2 => {
                let p1 = &src[src_off + src_rb..];
                for i in 0..dst_w {
                    let s = i * 2;
                    let c = F::load_expand(p0, s)
                        .add(F::load_expand(p1, s))
                        .add(F::load_expand(p0, s + 1))
                        .add(F::load_expand(p1, s + 1));
                    F::compact_store(dst_row, i, c.shr(2));
                }
            }
            DownsampleKernel::K2x3 => {
                let p1 = &src[src_off + src_rb..];
                let p2 = &src[src_off + src_rb * 2..];
                for i in 0..dst_w {
                    let s = i * 2;
                    let left = F::Wide::add_121(
                        F::load_expand(p0, s),
                        F::load_expand(p1, s),
                        F::load_expand(p2, s),
                    );
                    let right = F::Wide::add_121(
                        F::load_expand(p0, s + 1),
                        F::load_expand(p1, s + 1),
                        F::load_expand(p2, s + 1),
                    );
                    F::compact_store(dst_row, i, left.add(right).shr(3));
                }
            }
            DownsampleKernel::K3x1 => {
                let mut c02 = F::load_expand(p0, 0);
                for i in 0..dst_w {
                    let s = i * 2;
                    let c00 = c02;
                    let c01 = F::load_expand(p0, s + 1);
                    c02 = F::load_expand(p0, s + 2);
                    let c = F::Wide::add_121(c00, c01, c02);
                    F::compact_store(dst_row, i, c.shr(2));
                }
            }
            DownsampleKernel::K3x2 => {
                let p1 = &src[src_off + src_rb..];
                let mut c = F::load_expand(p0, 0).add(F::load_expand(p1, 0));
                for i in 0..dst_w {
                    let s = i * 2;
                    let a = c;
                    let b0 = F::load_expand(p0, s + 1);
                    let b1 = F::load_expand(p1, s + 1);
                    let b = b0.add(b0).add(b1).add(b1);
                    c = F::load_expand(p0, s + 2).add(F::load_expand(p1, s + 2));
                    let sum = a.add(b).add(c);
                    F::compact_store(dst_row, i, sum.shr(3));
                }
            }
            DownsampleKernel::K3x3 => {
                let p1 = &src[src_off + src_rb..];
                let p2 = &src[src_off + src_rb * 2..];
                let mut c = F::Wide::add_121(
                    F::load_expand(p0, 0),
                    F::load_expand(p1, 0),
                    F::load_expand(p2, 0),
                );
                for i in 0..dst_w {
                    let s = i * 2;
                    let a = c;
                    let b = F::Wide::add_121(
                        F::load_expand(p0, s + 1),
                        F::load_expand(p1, s + 1),
                        F::load_expand(p2, s + 1),
                    )
                    .shl(1);
                    c = F::Wide::add_121(
                        F::load_expand(p0, s + 2),
                        F::load_expand(p1, s + 2),
                        F::load_expand(p2, s + 2),
                    );
                    let sum = a.add(b).add(c);
                    F::compact_store(dst_row, i, sum.shr(4));
                }
            }
        }
    }
}

/// Returns whether `HQDownSampler` supports the given `ColorType`.
pub const fn hq_downsample_supported(ct: ColorType) -> bool {
    matches!(
        ct,
        ColorType::Rgba8888
            | ColorType::Bgra8888
            | ColorType::Rgb888x
            | ColorType::Bgr888x
            | ColorType::Rgb565
            | ColorType::Argb4444
            | ColorType::Alpha8
            | ColorType::Gray8
            | ColorType::R8Unorm
            | ColorType::RgbaF16Norm
            | ColorType::RgbaF16
            | ColorType::R8G8Unorm
            | ColorType::R16G16Unorm
            | ColorType::A16Unorm
            | ColorType::R16Unorm
            | ColorType::Rgba1010102
            | ColorType::Bgra1010102
            | ColorType::Rgb101010x
            | ColorType::Bgr101010x
            | ColorType::A16Float
            | ColorType::R16Float
            | ColorType::R16G16Float
            | ColorType::R16G16B16A16Unorm
    )
}

/// Exact port of `HQDownSampler::buildLevel`.
pub fn hq_downsample_level(
    ct: ColorType,
    src_w: i32,
    src_h: i32,
    src_rb: usize,
    src: &[u8],
    dst_w: i32,
    dst_h: i32,
    dst_rb: usize,
    dst: &mut [u8],
) -> bool {
    if dst_w <= 0 || dst_h <= 0 || src_w <= 0 || src_h <= 0 {
        return false;
    }
    let kernel = select_kernel(src_w, src_h);
    let dw = dst_w as usize;
    let dh = dst_h as usize;
    match ct {
        ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::Rgb888x | ColorType::Bgr888x => {
            downsample_rows::<Filter8888>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::Rgb565 => {
            downsample_rows::<Filter565>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::Argb4444 => {
            downsample_rows::<Filter4444>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::Alpha8 | ColorType::Gray8 | ColorType::R8Unorm => {
            downsample_rows::<Filter8>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::RgbaF16Norm | ColorType::RgbaF16 => {
            downsample_rows::<FilterRgbaF16>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::R8G8Unorm => {
            downsample_rows::<Filter88>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::R16G16Unorm => {
            downsample_rows::<Filter1616>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::A16Unorm | ColorType::R16Unorm => {
            downsample_rows::<Filter16>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::Rgba1010102
        | ColorType::Bgra1010102
        | ColorType::Rgb101010x
        | ColorType::Bgr101010x => {
            downsample_rows::<Filter1010102>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::A16Float | ColorType::R16Float => {
            downsample_rows::<FilterF16>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::R16G16Float => {
            downsample_rows::<FilterF16F16>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::R16G16B16A16Unorm => {
            downsample_rows::<Filter16161616>(kernel, src, src_rb, dst, dw, dh, dst_rb);
            true
        }
        ColorType::Unknown
        | ColorType::Bgr101010xXr
        | ColorType::RgbF16F16F16x
        | ColorType::Bgra10101010Xr
        | ColorType::Rgba10x6
        | ColorType::RgbaF32
        | ColorType::Srgba8888 => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_count_and_sizes() {
        assert_eq!(compute_level_count(1, 1), 0);
        assert_eq!(compute_level_count(2, 2), 1);
        assert_eq!(compute_level_count(8, 8), 3);
        assert_eq!(compute_level_count(16, 4), 4);
        assert_eq!(compute_level_size(16, 4, 0), ISize { width: 8, height: 2 });
        assert_eq!(compute_level_size(16, 4, 1), ISize { width: 4, height: 1 });
        assert_eq!(compute_level_size(16, 4, 2), ISize { width: 2, height: 1 });
        assert_eq!(compute_level_size(16, 4, 3), ISize { width: 1, height: 1 });
        assert_eq!(compute_level_size(16, 4, 4), ISize { width: 0, height: 0 });
    }

    #[test]
    fn compute_level_and_accessor_plan() {
        assert_eq!(compute_level(Size { width: 1.0, height: 1.0 }, false), -1.0);
        assert_eq!(compute_level(Size { width: 0.0, height: 0.5 }, false), -1.0);
        // scale = 0.25 -> -log2(0.25) - 0.5 = 2.0 - 0.5 = 1.5
        assert!((compute_level(Size { width: 0.25, height: 0.5 }, false) - 1.5).abs() < 1e-6);

        let inv = Matrix::scale_translate(4.0, 4.0, 0.0, 0.0);
        let plan_lin = plan_mipmap_accessor(&inv, MipmapMode::Linear, false);
        assert_eq!(plan_lin.level_num, 1);
        assert!((plan_lin.lower_weight - 0.5).abs() < 1e-6);
        assert!(plan_lin.need_mips);

        let plan_near = plan_mipmap_accessor(&inv, MipmapMode::Nearest, false);
        assert_eq!(plan_near.level_num, 2);
        assert!(plan_near.need_mips);
    }

    #[test]
    fn hq_downsample_8888_even_and_odd() {
        // 4x4 uniform color -> 2x2 same color
        let mut src = vec![0u8; 4 * 4 * 4];
        for chunk in src.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[0x40, 0x80, 0xC0, 0xFF]);
        }
        let mut dst = vec![0u8; 2 * 2 * 4];
        assert!(hq_downsample_level(
            ColorType::Rgba8888,
            4,
            4,
            16,
            &src,
            2,
            2,
            8,
            &mut dst
        ));
        for chunk in dst.chunks_exact(4) {
            assert_eq!(chunk, &[0x40, 0x80, 0xC0, 0xFF]);
        }

        // 5x5 uniform color (K3x3) -> 2x2 same color
        let mut src5 = vec![0u8; 5 * 5 * 4];
        for chunk in src5.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[0x20, 0x60, 0xA0, 0xFF]);
        }
        let mut dst2 = vec![0u8; 2 * 2 * 4];
        assert!(hq_downsample_level(
            ColorType::Rgba8888,
            5,
            5,
            20,
            &src5,
            2,
            2,
            8,
            &mut dst2
        ));
        for chunk in dst2.chunks_exact(4) {
            assert_eq!(chunk, &[0x20, 0x60, 0xA0, 0xFF]);
        }
    }

    #[test]
    fn hq_downsample_1010102_preserves_opaque_alpha() {
        // 4x4 opaque 1010102 pixel
        let px: u32 = (0x3u32 << 30) | (0x200u32 << 20) | (0x100u32 << 10) | 0x080u32;
        let mut src = vec![0u8; 4 * 4 * 4];
        for chunk in src.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px.to_ne_bytes());
        }
        let mut dst = vec![0u8; 2 * 2 * 4];
        assert!(hq_downsample_level(
            ColorType::Rgba1010102,
            4,
            4,
            16,
            &src,
            2,
            2,
            8,
            &mut dst
        ));
        for chunk in dst.chunks_exact(4) {
            let v = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            assert_eq!(v, px);
        }
    }

    #[test]
    fn hq_downsample_f16_and_565() {
        // F16 2x2 -> 1x1
        let mut src_f16 = vec![0u8; 2 * 2 * 8];
        let one = float_to_half(1.0).to_ne_bytes();
        let half = float_to_half(0.5).to_ne_bytes();
        for chunk in src_f16.chunks_exact_mut(8) {
            chunk[0..2].copy_from_slice(&half);
            chunk[2..4].copy_from_slice(&one);
            chunk[4..6].copy_from_slice(&half);
            chunk[6..8].copy_from_slice(&one);
        }
        let mut dst_f16 = vec![0u8; 8];
        assert!(hq_downsample_level(
            ColorType::RgbaF16,
            2,
            2,
            16,
            &src_f16,
            1,
            1,
            8,
            &mut dst_f16
        ));
        assert_eq!(dst_f16[0..2], half);
        assert_eq!(dst_f16[2..4], one);
        assert_eq!(dst_f16[6..8], one);

        // 565 4x1 -> 2x1 (K2x1)
        let c565: u16 = 0xF800; // pure red
        let mut src_565 = vec![0u8; 4 * 2];
        for chunk in src_565.chunks_exact_mut(2) {
            chunk.copy_from_slice(&c565.to_ne_bytes());
        }
        let mut dst_565 = vec![0u8; 2 * 2];
        assert!(hq_downsample_level(
            ColorType::Rgb565,
            4,
            1,
            8,
            &src_565,
            2,
            1,
            4,
            &mut dst_565
        ));
        assert_eq!(u16::from_ne_bytes([dst_565[0], dst_565[1]]), c565);
    }

    #[test]
    fn hq_downsample_all_8_kernels_and_misc_helpers() {
        // K1x2: 1x4 -> 1x2 (Alpha8)
        let src1x4 = [10u8, 30, 50, 70];
        let mut dst1x2 = [0u8; 2];
        assert!(hq_downsample_level(ColorType::Alpha8, 1, 4, 1, &src1x4, 1, 2, 1, &mut dst1x2));
        assert_eq!(dst1x2, [20, 60]);

        // K1x3: 1x5 -> 1x2 (Alpha8): (10+2*30+50)/4 = 30, (50+2*70+90)/4 = 70
        let src1x5 = [10u8, 30, 50, 70, 90];
        let mut dst1x2_odd = [0u8; 2];
        assert!(hq_downsample_level(ColorType::Alpha8, 1, 5, 1, &src1x5, 1, 2, 1, &mut dst1x2_odd));
        assert_eq!(dst1x2_odd, [30, 70]);

        // K3x1: 5x1 -> 2x1 (Alpha8)
        let mut dst2x1 = [0u8; 2];
        assert!(hq_downsample_level(ColorType::Alpha8, 5, 1, 5, &src1x5, 2, 1, 2, &mut dst2x1));
        assert_eq!(dst2x1, [30, 70]);

        // K2x3: 2x3 -> 1x1 (Alpha8)
        let src2x3 = [0u8, 8, 8, 16, 16, 24];
        let mut dst1x1 = [0u8; 1];
        assert!(hq_downsample_level(ColorType::Alpha8, 2, 3, 2, &src2x3, 1, 1, 1, &mut dst1x1));
        // left = 0 + 16 + 16 = 32, right = 8 + 32 + 24 = 64 -> (32+64)>>3 = 12
        assert_eq!(dst1x1[0], 12);

        // K3x2: 3x2 -> 1x1 (Alpha8)
        let src3x2 = [0u8, 8, 16, 8, 16, 24];
        assert!(hq_downsample_level(ColorType::Alpha8, 3, 2, 3, &src3x2, 1, 1, 1, &mut dst1x1));
        assert_eq!(dst1x1[0], 12);

        // Unsupported formats return false
        assert!(!hq_downsample_supported(ColorType::RgbaF32));
        assert!(!hq_downsample_supported(ColorType::Srgba8888));
        assert!(!hq_downsample_level(ColorType::RgbaF32, 2, 2, 32, &[0; 64], 1, 1, 16, &mut [0; 16]));

        // AllocLevelsSize & validForRootLevel & drawDownSampler
        assert_eq!(alloc_levels_size(-1, 32, 100), 0);
        assert_eq!(alloc_levels_size(3, 32, 100), 4 * 32 + 100);
        assert_eq!(alloc_levels_size(3, 32, i32::MAX as usize), 0);
        assert_eq!(compute_total_pixel_size(4, 4, ColorType::Rgba8888), (2 * 2 + 1 * 1) * 4);
        assert_eq!(select_extract_level(Size { width: 0.25, height: 0.25 }, 3, false), Some(1));
        assert_eq!(select_extract_level(Size { width: 1.0, height: 1.0 }, 3, false), None);
        assert!(valid_for_root_level(
            8, 4, ColorType::Rgba8888, AlphaType::Premul,
            4, 2,
            &[ColorType::Rgba8888 as i32; 3],
            &[AlphaType::Premul as i32; 3],
        ));
        assert!(!valid_for_root_level(
            8, 4, ColorType::Rgba8888, AlphaType::Premul,
            3, 2,
            &[ColorType::Rgba8888 as i32; 3],
            &[AlphaType::Premul as i32; 3],
        ));
        assert!(draw_downsampler_use_bilerp(4, 4, 8, 8));
        assert!(!draw_downsampler_use_bilerp(4, 4, 9, 8));
    }
}
