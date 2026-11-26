//! Safe Rust port of `src/core/SkBlitter_A8.cpp`.

use skia_rs_color::blend_mode::BlendMode;
use skia_rs_color::image_info::ColorType;

/// Exact port of `SkBlitter_A8.cpp`'s `div255(unsigned prod)`:
/// `((prod + 128) * 257) >> 16` for `prod <= 255 * 255`.
#[inline]
pub const fn div255(prod: u32) -> u8 {
    (((prod + 128) * 257) >> 16) as u8
}

/// Exact port of `SkBlitter_A8.cpp`'s `u8_lerp(a, b, t)`:
/// `div255((255 - t) * a + t * b)`.
#[inline]
pub const fn u8_lerp(a: u8, b: u8, t: u8) -> u8 {
    div255((255 - t as u32) * a as u32 + t as u32 * b as u32)
}

/// Supported blend modes for `SkA8_Blitter`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum A8BlendOp {
    SrcOver = 0,
    Src = 1,
}

impl A8BlendOp {
    #[inline]
    pub const fn from_blend_mode(mode: BlendMode) -> Option<Self> {
        match mode {
            BlendMode::SrcOver => Some(Self::SrcOver),
            BlendMode::Src => Some(Self::Src),
            _ => None,
        }
    }

    #[inline]
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::SrcOver),
            1 => Some(Self::Src),
            _ => None,
        }
    }

    #[inline]
    pub const fn can_fold_aa(self) -> bool {
        match self {
            Self::SrcOver => true,
            Self::Src => false,
        }
    }

    /// Exact port of `srcover_p` / `src_p`.
    #[inline]
    pub const fn blend_one(self, src: u8, dst: u8) -> u8 {
        match self {
            Self::SrcOver => src.wrapping_add(div255((255 - src as u32) * dst as u32)),
            Self::Src => src,
        }
    }
}

/// Exact port of `A8_row_bw`.
#[inline]
pub fn a8_row_bw(dst: &mut [u8], src: u8, op: A8BlendOp) {
    match op {
        A8BlendOp::Src => dst.fill(src),
        A8BlendOp::SrcOver => {
            for d in dst.iter_mut() {
                *d = op.blend_one(src, *d);
            }
        }
    }
}

/// Exact port of `A8_row_aa`.
#[inline]
pub fn a8_row_aa(dst: &mut [u8], src: u8, aa: u8, op: A8BlendOp) {
    if op.can_fold_aa() {
        let folded = div255(src as u32 * aa as u32);
        for d in dst.iter_mut() {
            *d = op.blend_one(folded, *d);
        }
    } else {
        for d in dst.iter_mut() {
            *d = u8_lerp(*d, op.blend_one(src, *d), aa);
        }
    }
}

/// Apply one RLE run for `SkA8_Blitter::blitAntiH`.
#[inline]
pub fn a8_blit_anti_run(dst: &mut [u8], src: u8, aa: u8, op: A8BlendOp) {
    if aa == 0xFF {
        a8_row_bw(dst, src, op);
    } else if aa != 0 {
        a8_row_aa(dst, src, aa, op);
    }
}

/// Exact port of `SkA8_Blitter::blitAntiH` when full `aa` and `runs` slices are available.
pub fn a8_blit_anti_h(dst: &mut [u8], src: u8, antialias: &[u8], runs: &[i16], op: A8BlendOp) {
    let mut pos = 0usize;
    while pos < runs.len() {
        let count = runs[pos];
        if count <= 0 {
            return;
        }
        let n = count as usize;
        if pos + n > dst.len() || pos >= antialias.len() {
            return;
        }
        a8_blit_anti_run(&mut dst[pos..pos + n], src, antialias[pos], op);
        pos += n;
    }
}

/// Exact port of `SkA8_Blitter::blitV`.
pub fn a8_blit_v(dst: &mut [u8], dst_rb: usize, height: usize, src: u8, aa: u8, op: A8BlendOp) {
    if height == 0 || aa == 0 {
        return;
    }
    if aa == 0xFF {
        for row in 0..height {
            let idx = row * dst_rb;
            if idx < dst.len() {
                dst[idx] = op.blend_one(src, dst[idx]);
            }
        }
    } else {
        for row in 0..height {
            let idx = row * dst_rb;
            if idx < dst.len() {
                a8_row_aa(&mut dst[idx..idx + 1], src, aa, op);
            }
        }
    }
}

/// Exact port of `SkA8_Blitter::blitRect`.
pub fn a8_blit_rect(
    dst: &mut [u8],
    dst_rb: usize,
    width: usize,
    height: usize,
    src: u8,
    op: A8BlendOp,
) {
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let start = row * dst_rb;
        let end = start + width;
        if end <= dst.len() {
            a8_row_bw(&mut dst[start..end], src, op);
        }
    }
}

/// Exact port of one row of `SkA8_Blitter::blitMask` (`kA8_Format`).
#[inline]
pub fn a8_blit_mask_row(dst: &mut [u8], mask: &[u8], src: u8, op: A8BlendOp) {
    for (d, &m) in dst.iter_mut().zip(mask.iter()) {
        *d = u8_lerp(*d, op.blend_one(src, *d), m);
    }
}

/// Exact port of `SkA8_Blitter::blitMask` (`kA8_Format`).
pub fn a8_blit_mask_a8(
    dst: &mut [u8],
    dst_rb: usize,
    mask: &[u8],
    mask_rb: usize,
    width: usize,
    height: usize,
    src: u8,
    op: A8BlendOp,
) {
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let d_start = row * dst_rb;
        let m_start = row * mask_rb;
        if d_start + width <= dst.len() && m_start + width <= mask.len() {
            a8_blit_mask_row(
                &mut dst[d_start..d_start + width],
                &mask[m_start..m_start + width],
                src,
                op,
            );
        }
    }
}

/// Exact port of `SkA8_Coverage_Blitter::blitH`.
#[inline]
pub fn a8_coverage_blit_h(dst: &mut [u8]) {
    dst.fill(0xFF);
}

/// Apply one RLE run for `SkA8_Coverage_Blitter::blitAntiH`.
#[inline]
pub fn a8_coverage_blit_anti_run(dst: &mut [u8], aa: u8) {
    if aa != 0 {
        dst.fill(aa);
    }
}

/// Exact port of `SkA8_Coverage_Blitter::blitAntiH`.
pub fn a8_coverage_blit_anti_h(dst: &mut [u8], antialias: &[u8], runs: &[i16]) {
    let mut pos = 0usize;
    while pos < runs.len() {
        let count = runs[pos];
        if count <= 0 {
            return;
        }
        let n = count as usize;
        if pos + n > dst.len() || pos >= antialias.len() {
            return;
        }
        a8_coverage_blit_anti_run(&mut dst[pos..pos + n], antialias[pos]);
        pos += n;
    }
}

/// Exact port of `SkA8_Coverage_Blitter::blitV`.
pub fn a8_coverage_blit_v(dst: &mut [u8], dst_rb: usize, height: usize, alpha: u8) {
    if alpha == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let idx = row * dst_rb;
        if idx < dst.len() {
            dst[idx] = alpha;
        }
    }
}

/// Exact port of `SkA8_Coverage_Blitter::blitRect`.
pub fn a8_coverage_blit_rect(dst: &mut [u8], dst_rb: usize, width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let start = row * dst_rb;
        let end = start + width;
        if end <= dst.len() {
            dst[start..end].fill(0xFF);
        }
    }
}

/// Exact port of `SkA8_Coverage_Blitter::blitMask` (`kA8_Format`).
pub fn a8_coverage_blit_mask_a8(
    dst: &mut [u8],
    dst_rb: usize,
    mask: &[u8],
    mask_rb: usize,
    width: usize,
    height: usize,
) {
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let d_start = row * dst_rb;
        let m_start = row * mask_rb;
        if d_start + width <= dst.len() && m_start + width <= mask.len() {
            dst[d_start..d_start + width].copy_from_slice(&mask[m_start..m_start + width]);
        }
    }
}

/// Decision result for `SkChooseA8Blitter`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum A8BlitterChoice {
    None = 0,
    Coverage = 1,
    BlendSrcOver = 2,
    BlendSrc = 3,
}

/// Exact port of `SkChooseA8Blitter` selection logic.
#[inline]
pub const fn choose_a8_blitter(
    color_type: ColorType,
    has_shader: bool,
    has_color_filter: bool,
    has_clip_shader: bool,
    draw_coverage: bool,
    blend_mode: Option<BlendMode>,
) -> A8BlitterChoice {
    if !matches!(color_type, ColorType::Alpha8) {
        return A8BlitterChoice::None;
    }
    if has_shader || has_color_filter || has_clip_shader {
        return A8BlitterChoice::None;
    }
    if draw_coverage {
        return A8BlitterChoice::Coverage;
    }
    match blend_mode {
        Some(BlendMode::SrcOver) => A8BlitterChoice::BlendSrcOver,
        Some(BlendMode::Src) => A8BlitterChoice::BlendSrc,
        _ => A8BlitterChoice::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div255_exact_over_full_domain() {
        for a in 0u32..=255 {
            for b in 0u32..=255 {
                let prod = a * b;
                let expected = ((prod + 127) / 255) as u8;
                assert_eq!(div255(prod), expected, "a={a} b={b}");
            }
        }
    }

    #[test]
    fn u8_lerp_endpoints_and_midpoint() {
        for a in [0u8, 17, 128, 255] {
            for b in [0u8, 33, 200, 255] {
                assert_eq!(u8_lerp(a, b, 0), a);
                assert_eq!(u8_lerp(a, b, 255), b);
            }
        }
        assert_eq!(u8_lerp(0, 255, 128), 128);
    }

    #[test]
    fn a8_blend_srcover_and_src() {
        let mut dst = [0u8, 64, 128, 255];
        a8_row_bw(&mut dst, 128, A8BlendOp::SrcOver);
        assert_eq!(dst, [128, 160, 192, 255]);

        let mut dst2 = [10u8, 20, 30, 40];
        a8_row_bw(&mut dst2, 99, A8BlendOp::Src);
        assert_eq!(dst2, [99, 99, 99, 99]);

        let mut dst3 = [0u8, 100, 200, 255];
        a8_row_aa(&mut dst3, 200, 128, A8BlendOp::Src);
        for (i, &orig) in [0u8, 100, 200, 255].iter().enumerate() {
            assert_eq!(dst3[i], u8_lerp(orig, 200, 128));
        }

        let mut dst4 = [0u8, 100, 200, 255];
        a8_row_aa(&mut dst4, 200, 128, A8BlendOp::SrcOver);
        let folded = div255(200 * 128);
        for (i, &orig) in [0u8, 100, 200, 255].iter().enumerate() {
            assert_eq!(dst4[i], A8BlendOp::SrcOver.blend_one(folded, orig));
        }
    }

    #[test]
    fn a8_coverage_and_blend_anti_h() {
        let mut dst = [5u8; 8];
        let mut runs = [0i16; 9];
        let mut aa = [0u8; 8];
        runs[0] = 3;
        aa[0] = 0;
        runs[3] = 2;
        aa[3] = 120;
        runs[5] = 3;
        aa[5] = 255;
        runs[8] = 0;

        a8_coverage_blit_anti_h(&mut dst, &aa, &runs);
        assert_eq!(dst, [5, 5, 5, 120, 120, 255, 255, 255]);

        let mut dst2 = [10u8; 8];
        a8_blit_anti_h(&mut dst2, 200, &aa, &runs, A8BlendOp::Src);
        assert_eq!(&dst2[0..3], &[10, 10, 10]);
        let exp_mid = u8_lerp(10, 200, 120);
        assert_eq!(&dst2[3..5], &[exp_mid, exp_mid]);
        assert_eq!(&dst2[5..8], &[200, 200, 200]);
    }

    #[test]
    fn a8_blit_v_rect_and_mask() {
        let mut buf = [0u8; 4 * 6];
        a8_coverage_blit_v(&mut buf[1..], 6, 3, 77);
        assert_eq!(buf[1], 77);
        assert_eq!(buf[7], 77);
        assert_eq!(buf[13], 77);
        assert_eq!(buf[19], 0);

        let mut rect_buf = [1u8; 3 * 5];
        a8_blit_rect(&mut rect_buf, 5, 3, 2, 128, A8BlendOp::Src);
        assert_eq!(&rect_buf[0..5], &[128, 128, 128, 1, 1]);
        assert_eq!(&rect_buf[5..10], &[128, 128, 128, 1, 1]);
        assert_eq!(&rect_buf[10..15], &[1, 1, 1, 1, 1]);

        let mask = [0u8, 128, 255, 0, 64, 255];
        let mut dst = [10u8; 6];
        a8_blit_mask_a8(&mut dst, 3, &mask, 3, 3, 2, 200, A8BlendOp::SrcOver);
        for i in 0..6 {
            let blended = A8BlendOp::SrcOver.blend_one(200, 10);
            assert_eq!(dst[i], u8_lerp(10, blended, mask[i]));
        }
    }

    #[test]
    fn choose_a8_blitter_cases() {
        assert_eq!(
            choose_a8_blitter(ColorType::Rgba8888, false, false, false, true, None),
            A8BlitterChoice::None
        );
        assert_eq!(
            choose_a8_blitter(ColorType::Alpha8, true, false, false, true, None),
            A8BlitterChoice::None
        );
        assert_eq!(
            choose_a8_blitter(ColorType::Alpha8, false, false, false, true, None),
            A8BlitterChoice::Coverage
        );
        assert_eq!(
            choose_a8_blitter(
                ColorType::Alpha8,
                false,
                false,
                false,
                false,
                Some(BlendMode::SrcOver)
            ),
            A8BlitterChoice::BlendSrcOver
        );
        assert_eq!(
            choose_a8_blitter(
                ColorType::Alpha8,
                false,
                false,
                false,
                false,
                Some(BlendMode::Src)
            ),
            A8BlitterChoice::BlendSrc
        );
        assert_eq!(
            choose_a8_blitter(
                ColorType::Alpha8,
                false,
                false,
                false,
                false,
                Some(BlendMode::Multiply)
            ),
            A8BlitterChoice::None
        );
    }
}
