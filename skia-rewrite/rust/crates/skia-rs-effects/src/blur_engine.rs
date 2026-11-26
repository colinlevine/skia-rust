//! Safe Rust implementation of `SkBlurEngine` & `SkShaderBlurAlgorithm` (`src/core/SkBlurEngine.cpp`):
//! - 1D / 2D Gaussian blur kernel & offset generation
//! - 1D linear-filtering blur kernel & interleaved offset/weight generation
//! - StableKey bucket mapping for known runtime blur effects
//! - CPU blur passes: `GaussianPass` (A8 & 8888), `A8Pass`, `ThreeBoxApproxPass`, `TentPass`
//! - 2D separable pass driver (`eval_blur_passes_a8`, `eval_blur_passes_8888`)
//! - Shader blur fast/strict rect & intermediate pass planning

use crate::path_effects::{float_ceil_to_int, float_floor_to_int};
use skia_rs_geometry::{IRect, ISize, Size, V4, SK_SCALAR_PI};

pub const MAX_SAMPLES: usize = 28;
pub const MAX_LINEAR_SIGMA: f32 = 4.0;
pub const MAX_RASTER_SIGMA: f32 = 135.0;

#[inline]
pub fn is_effectively_identity(sigma: f32) -> bool {
    sigma <= 0.03
}

#[inline]
pub fn sigma_to_radius(sigma: f32) -> i32 {
    if is_effectively_identity(sigma) {
        0
    } else {
        float_ceil_to_int(3.0 * sigma)
    }
}

#[inline]
pub fn kernel_width(radius: i32) -> i32 {
    2 * radius + 1
}

#[inline]
pub fn linear_kernel_width(radius: i32) -> i32 {
    radius + 1
}

/// Exact port of `SkBlurEngine::BoxBlurWindow`.
#[inline]
pub fn box_blur_window(sigma: f32) -> i32 {
    let possible = float_floor_to_int(sigma * 3.0 * (2.0 * SK_SCALAR_PI).sqrt() / 4.0 + 0.5);
    possible.max(1)
}

/// Exact port of `SkBlurEngine::A8Pass` window calculation (double precision).
#[inline]
fn a8_box_blur_window(sigma: f32) -> i32 {
    let possible = ((sigma as f64) * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5)
        .floor() as i32;
    possible.max(1)
}

// ============================================================================
// SkShaderBlurAlgorithm kernel & offset computations
// ============================================================================

/// Exact port of `SkShaderBlurAlgorithm::Compute2DBlurKernel`.
pub fn compute_2d_blur_kernel(sigma: Size, radius: ISize, kernel: &mut [f32]) {
    let width = kernel_width(radius.width) as usize;
    let height = kernel_width(radius.height) as usize;
    let kernel_size = width * height;
    debug_assert!(kernel_size <= kernel.len());

    let two_sigma_sqrd_x = 2.0_f32 * sigma.width * sigma.width;
    let two_sigma_sqrd_y = 2.0_f32 * sigma.height * sigma.height;
    let sigma_x_denom = if radius.width > 0 {
        1.0_f32 / two_sigma_sqrd_x
    } else {
        1.0_f32
    };
    let sigma_y_denom = if radius.height > 0 {
        1.0_f32 / two_sigma_sqrd_y
    } else {
        1.0_f32
    };

    let mut sum = 0.0_f32;
    for x in 0..width {
        let mut x_term = (x as i32 - radius.width) as f32;
        x_term = x_term * x_term * sigma_x_denom;
        for y in 0..height {
            let y_term = (y as i32 - radius.height) as f32;
            let xy_term = (-(x_term + y_term * y_term * sigma_y_denom)).exp();
            kernel[y * width + x] = xy_term;
            sum += xy_term;
        }
    }

    let scale = 1.0_f32 / sum;
    for val in kernel.iter_mut().take(kernel_size) {
        *val *= scale;
    }
    for val in kernel.iter_mut().skip(kernel_size) {
        *val = 0.0;
    }
}

/// Exact port of `SkShaderBlurAlgorithm::Compute1DBlurKernel`.
#[inline]
pub fn compute_1d_blur_kernel(sigma: f32, radius: i32, kernel: &mut [f32]) {
    compute_2d_blur_kernel(
        Size { width: sigma, height: 0.0 },
        ISize { width: radius, height: 0 },
        kernel,
    );
}

/// Exact port of `SkShaderBlurAlgorithm::Compute2DBlurOffsets`.
pub fn compute_2d_blur_offsets(radius: ISize, offsets: &mut [V4; MAX_SAMPLES / 2]) {
    let kernel_area = (kernel_width(radius.width) * kernel_width(radius.height)) as usize;
    debug_assert!(kernel_area <= MAX_SAMPLES);

    let mut flat = [0.0_f32; MAX_SAMPLES * 2];
    let mut i = 0usize;
    for y in -radius.height..=radius.height {
        for x in -radius.width..=radius.width {
            flat[2 * i] = x as f32;
            flat[2 * i + 1] = y as f32;
            i += 1;
        }
    }
    debug_assert_eq!(i, kernel_area);
    let last_valid = 2 * (kernel_area - 1);
    while i < MAX_SAMPLES {
        flat[2 * i] = flat[last_valid];
        flat[2 * i + 1] = flat[last_valid + 1];
        i += 1;
    }
    for k in 0..(MAX_SAMPLES / 2) {
        offsets[k] = V4 {
            x: flat[4 * k],
            y: flat[4 * k + 1],
            z: flat[4 * k + 2],
            w: flat[4 * k + 3],
        };
    }
}

/// Exact port of `SkShaderBlurAlgorithm::Compute1DBlurLinearKernel`.
pub fn compute_1d_blur_linear_kernel(
    sigma: f32,
    radius: i32,
    offsets_and_kernel: &mut [V4; MAX_SAMPLES / 2],
) {
    let get_new_weight = |wi: f32, wj: f32| -> (f32, f32) { (wi + wj, wj / (wi + wj)) };

    const MAX_KERNEL_WIDTH: usize = 2 * (MAX_SAMPLES - 1) + 1;
    let kw = kernel_width(radius) as usize;
    let mut full_kernel = [0.0_f32; MAX_KERNEL_WIDTH];
    compute_1d_blur_kernel(sigma, radius, &mut full_kernel[..kw]);

    let mut kernel = [0.0_f32; MAX_SAMPLES];
    let mut offsets = [0.0_f32; MAX_SAMPLES];

    let half_size = linear_kernel_width(radius) as usize;
    let half_radius = half_size / 2;
    let mut low_index = half_radius as i32 - 1;
    let mut index = radius as usize;

    if (radius & 1) != 0 {
        let (w, off) = get_new_weight(full_kernel[index] * 0.5, full_kernel[index + 1]);
        kernel[half_radius] = w;
        offsets[half_radius] = off;
        kernel[low_index as usize] = w;
        offsets[low_index as usize] = -off;
        index += 1;
        low_index -= 1;
    } else {
        kernel[half_radius] = full_kernel[index];
        offsets[half_radius] = 0.0;
    }
    index += 1;

    let mut i = half_radius + 1;
    while i < half_size {
        let (w, mut off) = get_new_weight(full_kernel[index], full_kernel[index + 1]);
        off += (index as i32 - radius) as f32;
        kernel[i] = w;
        offsets[i] = off;
        kernel[low_index as usize] = w;
        offsets[low_index as usize] = -off;
        index += 2;
        i += 1;
        low_index -= 1;
    }

    for slot in kernel.iter_mut().skip(half_size) {
        *slot = 0.0;
    }
    let last_off = offsets[half_size - 1];
    for slot in offsets.iter_mut().skip(half_size) {
        *slot = last_off;
    }

    for k in 0..(MAX_SAMPLES / 2) {
        offsets_and_kernel[k] = V4 {
            x: offsets[2 * k],
            y: kernel[2 * k],
            z: offsets[2 * k + 1],
            w: kernel[2 * k + 1],
        };
    }
}

/// Exact port of `to_stablekey` offset calculation in `SkBlurEngine.cpp`.
pub fn stable_key_offset(kernel_width: i32) -> u32 {
    match kernel_width {
        2..=4 => 0,
        5..=8 => 1,
        9..=12 => 2,
        13..=16 => 3,
        17..=20 => 4,
        21..=28 => 5,
        _ => 0,
    }
}

// ============================================================================
// ShaderBlurAlgorithm rect planning
// ============================================================================

/// Result of `plan_render_blur_rects` matching `SkShaderBlurAlgorithm::renderBlur`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderBlurPlan {
    pub fast_dst_rect: IRect,
    pub fast_tile_mode: i32,
    pub strict_border_count: i32,
    pub strict_borders: [IRect; 4],
}

/// Plans fast interior draw and up to 4 strict border draws for `SkShaderBlurAlgorithm::renderBlur`.
/// `tile_mode`: 0 = Clamp, 1 = Repeat, 2 = Mirror, 3 = Decal (`SkTileMode`).
pub fn plan_render_blur_rects(
    src_rect: IRect,
    backing_dims: ISize,
    radii: ISize,
    dst_rect: IRect,
    tile_mode: i32,
) -> RenderBlurPlan {
    let safe_src_rect = IRect::make_ltrb(
        src_rect.left + radii.width,
        src_rect.top + radii.height,
        src_rect.right - radii.width,
        src_rect.bottom - radii.height,
    );
    let mut fast_dst_rect = dst_rect;
    let backing_rect = IRect::make_ltrb(0, 0, backing_dims.width, backing_dims.height);

    if src_rect != backing_rect {
        let mut inter = IRect::make_ltrb(0, 0, 0, 0);
        if inter.intersect_pair(fast_dst_rect, safe_src_rect) {
            fast_dst_rect = inter;
            if fast_dst_rect != dst_rect
                && (fast_dst_rect.width() as i64) * (fast_dst_rect.height() as i64) < 128 * 128
            {
                fast_dst_rect = IRect::make_ltrb(0, 0, 0, 0);
            }
        } else {
            fast_dst_rect = IRect::make_ltrb(0, 0, 0, 0);
        }
    }

    let untiled_src_rect = IRect::make_ltrb(
        src_rect.left + 1,
        src_rect.top + 1,
        src_rect.right - 1,
        src_rect.bottom - 1,
    );
    let contains_fast = !untiled_src_rect.is_empty()
        && !fast_dst_rect.is_empty()
        && untiled_src_rect.left <= fast_dst_rect.left
        && untiled_src_rect.top <= fast_dst_rect.top
        && untiled_src_rect.right >= fast_dst_rect.right
        && untiled_src_rect.bottom >= fast_dst_rect.bottom;
    let fast_tile_mode = if contains_fast { 0 /* Clamp */ } else { tile_mode };

    let mut strict_borders = [IRect::make_ltrb(0, 0, 0, 0); 4];
    let mut strict_border_count = 0i32;

    if fast_dst_rect.is_empty() {
        strict_borders[0] = dst_rect;
        strict_border_count = 1;
    } else if fast_dst_rect != dst_rect {
        let candidates = [
            IRect::make_ltrb(dst_rect.left, dst_rect.top, fast_dst_rect.left, dst_rect.bottom),
            IRect::make_ltrb(fast_dst_rect.right, dst_rect.top, dst_rect.right, dst_rect.bottom),
            IRect::make_ltrb(fast_dst_rect.left, dst_rect.top, fast_dst_rect.right, fast_dst_rect.top),
            IRect::make_ltrb(fast_dst_rect.left, fast_dst_rect.bottom, fast_dst_rect.right, dst_rect.bottom),
        ];
        for r in candidates {
            if !r.is_empty() {
                strict_borders[strict_border_count as usize] = r;
                strict_border_count += 1;
            }
        }
    }

    RenderBlurPlan {
        fast_dst_rect,
        fast_tile_mode,
        strict_border_count,
        strict_borders,
    }
}

/// Computes `intermediateDstRect` for the X pass of a 2-pass 1D shader blur in
/// `SkShaderBlurAlgorithm::blur`. Returns `None` if the intermediate rect becomes empty.
pub fn plan_shader_blur_intermediate_dst(
    src_rect: IRect,
    dst_rect: IRect,
    radius_y: i32,
    tile_mode: i32,
) -> Option<IRect> {
    let mut intermediate = dst_rect;
    if radius_y > 0 {
        // Repeat = 1, Mirror = 2
        if tile_mode == 1 || tile_mode == 2 {
            let period = src_rect.height() * if tile_mode == 2 { 2 } else { 1 };
            if (dst_rect.top - src_rect.top).abs() % period != 0
                || dst_rect.height() != src_rect.height()
            {
                intermediate.top -= radius_y;
                intermediate.bottom += radius_y;
            }
        } else {
            intermediate.top -= radius_y;
            intermediate.bottom += radius_y;
            intermediate.top = intermediate.top.max(src_rect.top);
            intermediate.bottom = intermediate.bottom.min(src_rect.bottom);
            if intermediate.top >= intermediate.bottom {
                return None;
            }
        }
    }
    Some(intermediate)
}

// ============================================================================
// RasterBlurEngine passes (GaussianPass, A8Pass, ThreeBoxApproxPass, TentPass)
// ============================================================================

/// Exact mirror of `skvx::ScaledDividerU32`.
#[derive(Debug, Clone, Copy)]
struct ScaledDividerU32 {
    divisor_factor: u32,
    half: u32,
}

impl ScaledDividerU32 {
    #[inline]
    fn new(divisor: u32) -> Self {
        debug_assert!(divisor > 1);
        let factor = ((1.0_f64 / (divisor as f64)) * ((1u64 << 32) as f64)).round() as u32;
        Self {
            divisor_factor: factor,
            half: (divisor + 1) >> 1,
        }
    }

    #[inline]
    fn divide(&self, numerator: u32) -> u32 {
        (((numerator as u64) * (self.divisor_factor as u64)) >> 32) as u32
    }

    #[inline]
    fn half(&self) -> u32 {
        self.half
    }
}

/// Computes the window size for an A8 pass on a single axis (`RasterA8BlurAlgorithm`).
#[inline]
pub fn raster_a8_window(sigma: f32) -> i32 {
    if sigma < 2.0 {
        2 * sigma_to_radius(sigma) + 1
    } else {
        a8_box_blur_window(sigma)
    }
}

/// Computes the window size for an 8888 pass on a single axis (`Raster8888BlurAlgorithm`).
#[inline]
pub fn raster_8888_window(sigma: f32) -> i32 {
    if sigma < 2.0 {
        return 2 * sigma_to_radius(sigma) + 1;
    }
    let w = box_blur_window(sigma);
    if w < 255 {
        w
    } else {
        3 * w / 2
    }
}

/// Generic 1D pass driver matching `Pass::blur` in `SkBlurEngine.cpp`.
trait BlurPass1D<P: Copy + Default> {
    fn border(&self) -> i32;
    fn start_blur(&mut self);
    fn blur_segment(
        &mut self,
        n: usize,
        src: Option<(&[P], usize, usize)>,
        dst: Option<(&mut [P], usize, usize)>,
    );

    fn blur_line(
        &mut self,
        src_left: i32,
        src_right: i32,
        dst_right: i32,
        src: &[P],
        mut src_pos: usize,
        src_stride: usize,
        dst: &mut [P],
        mut dst_pos: usize,
        dst_stride: usize,
    ) {
        self.start_blur();

        let src_start = src_left - self.border();
        let src_end = src_right - self.border();
        let dst_end = dst_right;
        let mut src_idx = src_start;
        let mut dst_idx = 0i32;

        if dst_idx < src_idx {
            let common_end = src_idx.min(dst_end);
            while dst_idx < common_end {
                dst[dst_pos] = P::default();
                dst_pos += dst_stride;
                dst_idx += 1;
            }
        } else if src_idx < dst_idx {
            let common_end = dst_idx.min(src_end);
            if src_idx < common_end {
                let n = (common_end - src_idx) as usize;
                self.blur_segment(n, Some((src, src_pos, src_stride)), None);
                src_idx += n as i32;
                src_pos += n * src_stride;
            }
            if src_idx < dst_idx {
                let n = (dst_idx - src_idx) as usize;
                self.blur_segment(n, None, None);
                src_idx += n as i32;
            }
        }

        let common_end = dst_end.min(src_end);
        if dst_idx < common_end {
            debug_assert_eq!(src_idx, dst_idx);
            let n = (common_end - dst_idx) as usize;
            self.blur_segment(
                n,
                Some((src, src_pos, src_stride)),
                Some((dst, dst_pos, dst_stride)),
            );
            dst_pos += n * dst_stride;
            dst_idx += n as i32;
        }

        if dst_idx < dst_end {
            let n = (dst_end - dst_idx) as usize;
            self.blur_segment(n, None, Some((dst, dst_pos, dst_stride)));
        }
    }
}

struct GaussianPassA8 {
    radius: i32,
    window: usize,
    kernel: Vec<f32>,
    src_buf: Vec<f32>,
    base: usize,
}

impl GaussianPassA8 {
    fn new(sigma: f32) -> Self {
        let radius = sigma_to_radius(sigma);
        let window = (2 * radius + 1) as usize;
        let mut kernel = vec![0.0_f32; window];
        compute_1d_blur_kernel(sigma, radius, &mut kernel);
        Self {
            radius,
            window,
            kernel,
            src_buf: vec![0.0_f32; window],
            base: 0,
        }
    }
}

impl BlurPass1D<u8> for GaussianPassA8 {
    fn border(&self) -> i32 {
        self.radius
    }

    fn start_blur(&mut self) {
        self.src_buf.fill(0.0);
        self.base = 0;
    }

    fn blur_segment(
        &mut self,
        n: usize,
        src: Option<(&[u8], usize, usize)>,
        dst: Option<(&mut [u8], usize, usize)>,
    ) {
        let w = self.window;
        let mut base = self.base;
        let (src_slice, mut sp, ss) = match src {
            Some((s, p, st)) => (Some(s), p, st),
            None => (None, 0, 0),
        };
        let (mut dst_slice, mut dp, ds) = match dst {
            Some((d, p, st)) => (Some(d), p, st),
            None => (None, 0, 0),
        };

        for _ in 0..n {
            let leading = match src_slice {
                Some(s) => {
                    let v = (s[sp] as f32) * (1.0_f32 / 255.0_f32);
                    sp += ss;
                    v
                }
                None => 0.0_f32,
            };
            self.src_buf[(base + w - 1) % w] = leading;

            if let Some(ref mut d) = dst_slice {
                let mut sum = 0.0_f32;
                for i in 0..w {
                    let s = (i + base) % w;
                    sum += self.src_buf[s] * self.kernel[i];
                }
                d[dp] = (sum * 255.0_f32 + 0.5_f32).clamp(0.0, 255.0) as u8;
                dp += ds;
            }
            base = (base + 1) % w;
        }
        self.base = base;
    }
}

struct A8BoxPass {
    border: i32,
    weight: u64,
    buf0: Vec<u32>,
    buf1: Vec<u32>,
    buf2: Vec<u32>,
    c0: usize,
    c1: usize,
    c2: usize,
    sum0: u32,
    sum1: u32,
    sum2: u32,
}

impl A8BoxPass {
    fn new(window: i32) -> Self {
        let p0 = (window - 1) as usize;
        let p1 = (window - 1) as usize;
        let p2 = if (window & 1) == 1 {
            (window - 1) as usize
        } else {
            window as usize
        };
        let border = if (window & 1) == 1 {
            3 * ((window - 1) / 2)
        } else {
            3 * (window / 2) - 1
        };
        let w2 = (window as i64) * (window as i64);
        let w3 = w2 * (window as i64);
        let divisor = if (window & 1) == 1 { w3 } else { w3 + w2 };
        let weight = ((1.0 / (divisor as f64)) * ((1u64 << 32) as f64)).round() as u64;

        Self {
            border,
            weight,
            buf0: vec![0u32; p0],
            buf1: vec![0u32; p1],
            buf2: vec![0u32; p2],
            c0: 0,
            c1: 0,
            c2: 0,
            sum0: 0,
            sum1: 0,
            sum2: 0,
        }
    }

    #[inline]
    fn final_scale(&self, sum: u32) -> u8 {
        const K_HALF: u64 = 1u64 << 31;
        ((self.weight.wrapping_mul(sum as u64).wrapping_add(K_HALF)) >> 32) as u8
    }

    #[inline]
    fn process_value(&mut self, leading: u32) -> u8 {
        self.sum0 = self.sum0.wrapping_add(leading);
        self.sum1 = self.sum1.wrapping_add(self.sum0);
        self.sum2 = self.sum2.wrapping_add(self.sum1);

        let blurred = self.final_scale(self.sum2);

        self.sum2 = self.sum2.wrapping_sub(self.buf2[self.c2]);
        self.buf2[self.c2] = self.sum1;
        self.c2 += 1;
        if self.c2 >= self.buf2.len() {
            self.c2 = 0;
        }

        self.sum1 = self.sum1.wrapping_sub(self.buf1[self.c1]);
        self.buf1[self.c1] = self.sum0;
        self.c1 += 1;
        if self.c1 >= self.buf1.len() {
            self.c1 = 0;
        }

        self.sum0 = self.sum0.wrapping_sub(self.buf0[self.c0]);
        self.buf0[self.c0] = leading;
        self.c0 += 1;
        if self.c0 >= self.buf0.len() {
            self.c0 = 0;
        }

        blurred
    }
}

impl BlurPass1D<u8> for A8BoxPass {
    fn border(&self) -> i32 {
        self.border
    }

    fn start_blur(&mut self) {
        self.sum0 = 0;
        self.sum1 = 0;
        self.sum2 = 0;
        self.buf0.fill(0);
        self.buf1.fill(0);
        self.buf2.fill(0);
        self.c0 = 0;
        self.c1 = 0;
        self.c2 = 0;
    }

    fn blur_segment(
        &mut self,
        n: usize,
        src: Option<(&[u8], usize, usize)>,
        dst: Option<(&mut [u8], usize, usize)>,
    ) {
        match (src, dst) {
            (None, None) => {
                for _ in 0..n {
                    let _ = self.process_value(0);
                }
            }
            (Some((s, mut sp, ss)), None) => {
                for _ in 0..n {
                    let _ = self.process_value(s[sp] as u32);
                    sp += ss;
                }
            }
            (None, Some((d, mut dp, ds))) => {
                for _ in 0..n {
                    d[dp] = self.process_value(0);
                    dp += ds;
                }
            }
            (Some((s, mut sp, ss)), Some((d, mut dp, ds))) => {
                for _ in 0..n {
                    d[dp] = self.process_value(s[sp] as u32);
                    sp += ss;
                    dp += ds;
                }
            }
        }
    }
}

struct GaussianPass8888 {
    radius: i32,
    window: usize,
    kernel: Vec<f32>,
    src_buf: Vec<[f32; 4]>,
    base: usize,
}

impl GaussianPass8888 {
    fn new(sigma: f32) -> Self {
        let radius = sigma_to_radius(sigma);
        let window = (2 * radius + 1) as usize;
        let mut kernel = vec![0.0_f32; window];
        compute_1d_blur_kernel(sigma, radius, &mut kernel);
        Self {
            radius,
            window,
            kernel,
            src_buf: vec![[0.0_f32; 4]; window],
            base: 0,
        }
    }
}

impl BlurPass1D<[u8; 4]> for GaussianPass8888 {
    fn border(&self) -> i32 {
        self.radius
    }

    fn start_blur(&mut self) {
        self.src_buf.fill([0.0_f32; 4]);
        self.base = 0;
    }

    fn blur_segment(
        &mut self,
        n: usize,
        src: Option<(&[[u8; 4]], usize, usize)>,
        dst: Option<(&mut [[u8; 4]], usize, usize)>,
    ) {
        let w = self.window;
        let mut base = self.base;
        let (src_slice, mut sp, ss) = match src {
            Some((s, p, st)) => (Some(s), p, st),
            None => (None, 0, 0),
        };
        let (mut dst_slice, mut dp, ds) = match dst {
            Some((d, p, st)) => (Some(d), p, st),
            None => (None, 0, 0),
        };

        for _ in 0..n {
            let leading = match src_slice {
                Some(s) => {
                    let px = s[sp];
                    sp += ss;
                    [
                        (px[0] as f32) * (1.0_f32 / 255.0_f32),
                        (px[1] as f32) * (1.0_f32 / 255.0_f32),
                        (px[2] as f32) * (1.0_f32 / 255.0_f32),
                        (px[3] as f32) * (1.0_f32 / 255.0_f32),
                    ]
                }
                None => [0.0_f32; 4],
            };
            self.src_buf[(base + w - 1) % w] = leading;

            if let Some(ref mut d) = dst_slice {
                let mut sum = [0.0_f32; 4];
                for i in 0..w {
                    let s = (i + base) % w;
                    let k = self.kernel[i];
                    sum[0] += self.src_buf[s][0] * k;
                    sum[1] += self.src_buf[s][1] * k;
                    sum[2] += self.src_buf[s][2] * k;
                    sum[3] += self.src_buf[s][3] * k;
                }
                d[dp] = [
                    (sum[0] * 255.0_f32 + 0.5_f32).clamp(0.0, 255.0) as u8,
                    (sum[1] * 255.0_f32 + 0.5_f32).clamp(0.0, 255.0) as u8,
                    (sum[2] * 255.0_f32 + 0.5_f32).clamp(0.0, 255.0) as u8,
                    (sum[3] * 255.0_f32 + 0.5_f32).clamp(0.0, 255.0) as u8,
                ];
                dp += ds;
            }
            base = (base + 1) % w;
        }
        self.base = base;
    }
}

struct ThreeBoxPass8888 {
    border: i32,
    divider: ScaledDividerU32,
    buf0: Vec<[u32; 4]>,
    buf1: Vec<[u32; 4]>,
    buf2: Vec<[u32; 4]>,
    c0: usize,
    c1: usize,
    c2: usize,
    sum0: [u32; 4],
    sum1: [u32; 4],
    sum2: [u32; 4],
}

impl ThreeBoxPass8888 {
    fn new(window: i32) -> Self {
        let p = (window - 1) as usize;
        let p2 = if (window & 1) == 1 { p } else { p + 1 };
        let border = if (window & 1) == 1 {
            3 * ((window - 1) / 2)
        } else {
            3 * (window / 2) - 1
        };
        let w2 = window * window;
        let w3 = w2 * window;
        let divisor = if (window & 1) == 1 { w3 } else { w3 + w2 } as u32;
        Self {
            border,
            divider: ScaledDividerU32::new(divisor),
            buf0: vec![[0u32; 4]; p],
            buf1: vec![[0u32; 4]; p],
            buf2: vec![[0u32; 4]; p2],
            c0: 0,
            c1: 0,
            c2: 0,
            sum0: [0; 4],
            sum1: [0; 4],
            sum2: [0; 4],
        }
    }

    #[inline]
    fn process_value(&mut self, leading: [u32; 4]) -> [u8; 4] {
        let mut out = [0u8; 4];
        for c in 0..4 {
            self.sum0[c] = self.sum0[c].wrapping_add(leading[c]);
            self.sum1[c] = self.sum1[c].wrapping_add(self.sum0[c]);
            self.sum2[c] = self.sum2[c].wrapping_add(self.sum1[c]);
            out[c] = self.divider.divide(self.sum2[c]) as u8;
            self.sum2[c] = self.sum2[c].wrapping_sub(self.buf2[self.c2][c]);
            self.buf2[self.c2][c] = self.sum1[c];
            self.sum1[c] = self.sum1[c].wrapping_sub(self.buf1[self.c1][c]);
            self.buf1[self.c1][c] = self.sum0[c];
            self.sum0[c] = self.sum0[c].wrapping_sub(self.buf0[self.c0][c]);
            self.buf0[self.c0][c] = leading[c];
        }
        self.c2 += 1;
        if self.c2 >= self.buf2.len() {
            self.c2 = 0;
        }
        self.c1 += 1;
        if self.c1 >= self.buf1.len() {
            self.c1 = 0;
        }
        self.c0 += 1;
        if self.c0 >= self.buf0.len() {
            self.c0 = 0;
        }
        out
    }
}

impl BlurPass1D<[u8; 4]> for ThreeBoxPass8888 {
    fn border(&self) -> i32 {
        self.border
    }

    fn start_blur(&mut self) {
        self.sum0 = [0; 4];
        self.sum1 = [0; 4];
        let h = self.divider.half();
        self.sum2 = [h; 4];
        self.buf0.fill([0; 4]);
        self.buf1.fill([0; 4]);
        self.buf2.fill([0; 4]);
        self.c0 = 0;
        self.c1 = 0;
        self.c2 = 0;
    }

    fn blur_segment(
        &mut self,
        n: usize,
        src: Option<(&[[u8; 4]], usize, usize)>,
        dst: Option<(&mut [[u8; 4]], usize, usize)>,
    ) {
        match (src, dst) {
            (None, None) => {
                for _ in 0..n {
                    let _ = self.process_value([0; 4]);
                }
            }
            (Some((s, mut sp, ss)), None) => {
                for _ in 0..n {
                    let px = s[sp];
                    let _ = self.process_value([px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32]);
                    sp += ss;
                }
            }
            (None, Some((d, mut dp, ds))) => {
                for _ in 0..n {
                    d[dp] = self.process_value([0; 4]);
                    dp += ds;
                }
            }
            (Some((s, mut sp, ss)), Some((d, mut dp, ds))) => {
                for _ in 0..n {
                    let px = s[sp];
                    d[dp] = self.process_value([px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32]);
                    sp += ss;
                    dp += ds;
                }
            }
        }
    }
}

struct TentPass8888 {
    border: i32,
    divider: ScaledDividerU32,
    buf0: Vec<[u32; 4]>,
    buf1: Vec<[u32; 4]>,
    c0: usize,
    c1: usize,
    sum0: [u32; 4],
    sum1: [u32; 4],
}

impl TentPass8888 {
    fn new(window: i32) -> Self {
        let p = (window - 1) as usize;
        let border = window - 1;
        let divisor = (window as u32) * (window as u32);
        Self {
            border,
            divider: ScaledDividerU32::new(divisor),
            buf0: vec![[0u32; 4]; p],
            buf1: vec![[0u32; 4]; p],
            c0: 0,
            c1: 0,
            sum0: [0; 4],
            sum1: [0; 4],
        }
    }

    #[inline]
    fn process_value(&mut self, leading: [u32; 4]) -> [u8; 4] {
        let mut out = [0u8; 4];
        for c in 0..4 {
            self.sum0[c] = self.sum0[c].wrapping_add(leading[c]);
            self.sum1[c] = self.sum1[c].wrapping_add(self.sum0[c]);
            out[c] = self.divider.divide(self.sum1[c]) as u8;
            self.sum1[c] = self.sum1[c].wrapping_sub(self.buf1[self.c1][c]);
            self.buf1[self.c1][c] = self.sum0[c];
            self.sum0[c] = self.sum0[c].wrapping_sub(self.buf0[self.c0][c]);
            self.buf0[self.c0][c] = leading[c];
        }
        self.c1 += 1;
        if self.c1 >= self.buf1.len() {
            self.c1 = 0;
        }
        self.c0 += 1;
        if self.c0 >= self.buf0.len() {
            self.c0 = 0;
        }
        out
    }
}

impl BlurPass1D<[u8; 4]> for TentPass8888 {
    fn border(&self) -> i32 {
        self.border
    }

    fn start_blur(&mut self) {
        self.sum0 = [0; 4];
        let h = self.divider.half();
        self.sum1 = [h; 4];
        self.buf0.fill([0; 4]);
        self.buf1.fill([0; 4]);
        self.c0 = 0;
        self.c1 = 0;
    }

    fn blur_segment(
        &mut self,
        n: usize,
        src: Option<(&[[u8; 4]], usize, usize)>,
        dst: Option<(&mut [[u8; 4]], usize, usize)>,
    ) {
        match (src, dst) {
            (None, None) => {
                for _ in 0..n {
                    let _ = self.process_value([0; 4]);
                }
            }
            (Some((s, mut sp, ss)), None) => {
                for _ in 0..n {
                    let px = s[sp];
                    let _ = self.process_value([px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32]);
                    sp += ss;
                }
            }
            (None, Some((d, mut dp, ds))) => {
                for _ in 0..n {
                    d[dp] = self.process_value([0; 4]);
                    dp += ds;
                }
            }
            (Some((s, mut sp, ss)), Some((d, mut dp, ds))) => {
                for _ in 0..n {
                    let px = s[sp];
                    d[dp] = self.process_value([px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32]);
                    sp += ss;
                    dp += ds;
                }
            }
        }
    }
}

enum A8PassKind {
    Gaussian(GaussianPassA8),
    Box(A8BoxPass),
}

impl A8PassKind {
    fn new(sigma: f32) -> Self {
        if sigma < 2.0 {
            Self::Gaussian(GaussianPassA8::new(sigma))
        } else {
            Self::Box(A8BoxPass::new(a8_box_blur_window(sigma)))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn blur_line(
        &mut self,
        src_left: i32,
        src_right: i32,
        dst_right: i32,
        src: &[u8],
        src_pos: usize,
        src_stride: usize,
        dst: &mut [u8],
        dst_pos: usize,
        dst_stride: usize,
    ) {
        match self {
            Self::Gaussian(p) => p.blur_line(
                src_left, src_right, dst_right, src, src_pos, src_stride, dst, dst_pos, dst_stride,
            ),
            Self::Box(p) => p.blur_line(
                src_left, src_right, dst_right, src, src_pos, src_stride, dst, dst_pos, dst_stride,
            ),
        }
    }
}

enum RgbaPassKind {
    Gaussian(GaussianPass8888),
    ThreeBox(ThreeBoxPass8888),
    Tent(TentPass8888),
}

impl RgbaPassKind {
    fn new(sigma: f32) -> Self {
        if sigma < 2.0 {
            return Self::Gaussian(GaussianPass8888::new(sigma));
        }
        let w = box_blur_window(sigma);
        if w < 255 {
            Self::ThreeBox(ThreeBoxPass8888::new(w))
        } else {
            Self::Tent(TentPass8888::new(3 * w / 2))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn blur_line(
        &mut self,
        src_left: i32,
        src_right: i32,
        dst_right: i32,
        src: &[[u8; 4]],
        src_pos: usize,
        src_stride: usize,
        dst: &mut [[u8; 4]],
        dst_pos: usize,
        dst_stride: usize,
    ) {
        match self {
            Self::Gaussian(p) => p.blur_line(
                src_left, src_right, dst_right, src, src_pos, src_stride, dst, dst_pos, dst_stride,
            ),
            Self::ThreeBox(p) => p.blur_line(
                src_left, src_right, dst_right, src, src_pos, src_stride, dst, dst_pos, dst_stride,
            ),
            Self::Tent(p) => p.blur_line(
                src_left, src_right, dst_right, src, src_pos, src_stride, dst, dst_pos, dst_stride,
            ),
        }
    }
}

/// Evaluates 2D separable CPU blur passes for A8 images, matching `eval_blur_passes<uint8_t>` in
/// `SkBlurEngine.cpp`.
///
/// `dst` must be zero-initialized and sized for `allocated_dst_bounds` (which equals
/// `original_dst_bounds.outset(0, sigma_to_radius(sigma.h))` when `window_x > 1`, or
/// `original_dst_bounds` otherwise).
#[allow(clippy::too_many_arguments)]
pub fn eval_blur_passes_a8(
    sigma: Size,
    src: &[u8],
    src_row_pixels: usize,
    original_src_bounds: IRect,
    dst: &mut [u8],
    dst_row_pixels: usize,
    allocated_dst_bounds: IRect,
    original_dst_bounds: IRect,
) {
    let win_x = raster_a8_window(sigma.width);
    let win_y = raster_a8_window(sigma.height);

    let mut src_bounds = original_src_bounds;
    let mut dst_bounds = allocated_dst_bounds;

    let mut loop_start = src_bounds.left.max(dst_bounds.left);
    let mut loop_end = src_bounds.right.min(dst_bounds.right);
    let mut dst_y_offset = 0usize;
    let mut y_from_dst = false;

    if win_x > 1 {
        let y_start = src_bounds.top.max(dst_bounds.top);
        let y_end = src_bounds.bottom.min(dst_bounds.bottom);
        if y_start < y_end {
            let mut pass_x = A8PassKind::new(sigma.width);
            let mut src_off = ((y_start - src_bounds.top) as usize) * src_row_pixels;
            let mut dst_off = ((y_start - dst_bounds.top) as usize) * dst_row_pixels;
            for _ in y_start..y_end {
                pass_x.blur_line(
                    src_bounds.left - dst_bounds.left,
                    src_bounds.right - dst_bounds.left,
                    dst_bounds.width(),
                    src,
                    src_off,
                    1,
                    dst,
                    dst_off,
                    1,
                );
                src_off += src_row_pixels;
                dst_off += dst_row_pixels;
            }
        }
        y_from_dst = true;
        loop_start = original_dst_bounds.left;
        loop_end = original_dst_bounds.right;
        dst_y_offset = (original_dst_bounds.top - dst_bounds.top) as usize;
        src_bounds = dst_bounds;
        dst_bounds = original_dst_bounds;
    }

    if win_y > 1 && loop_start < loop_end {
        let mut pass_y = A8PassKind::new(sigma.height);
        if y_from_dst {
            let col_len = src_bounds.height() as usize;
            let mut col_tmp = vec![0u8; col_len];
            for x in loop_start..loop_end {
                let src_col_x = (x - src_bounds.left) as usize;
                let dst_col_x = (x - dst_bounds.left) as usize;
                for row in 0..col_len {
                    col_tmp[row] = dst[row * dst_row_pixels + src_col_x];
                }
                pass_y.blur_line(
                    src_bounds.top - dst_bounds.top,
                    src_bounds.bottom - dst_bounds.top,
                    dst_bounds.height(),
                    &col_tmp,
                    0,
                    1,
                    dst,
                    dst_y_offset * dst_row_pixels + dst_col_x,
                    dst_row_pixels,
                );
            }
        } else {
            for x in loop_start..loop_end {
                let src_col_x = (x - src_bounds.left) as usize;
                let dst_col_x = (x - dst_bounds.left) as usize;
                pass_y.blur_line(
                    src_bounds.top - dst_bounds.top,
                    src_bounds.bottom - dst_bounds.top,
                    dst_bounds.height(),
                    src,
                    src_col_x,
                    src_row_pixels,
                    dst,
                    dst_y_offset * dst_row_pixels + dst_col_x,
                    dst_row_pixels,
                );
            }
        }
    }
}

/// Evaluates 2D separable CPU blur passes for RGBA8/BGRA8 images, matching
/// `eval_blur_passes<uint32_t>` in `SkBlurEngine.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn eval_blur_passes_8888(
    sigma: Size,
    src_bytes: &[u8],
    src_row_pixels: usize,
    original_src_bounds: IRect,
    dst_bytes: &mut [u8],
    dst_row_pixels: usize,
    allocated_dst_bounds: IRect,
    original_dst_bounds: IRect,
) {
    let win_x = raster_8888_window(sigma.width);
    let win_y = raster_8888_window(sigma.height);

    let src_px_count = src_bytes.len() / 4;
    let mut src_px = Vec::with_capacity(src_px_count);
    for chunk in src_bytes.chunks_exact(4) {
        src_px.push([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    let dst_px_count = dst_bytes.len() / 4;
    let mut dst_px = vec![[0u8; 4]; dst_px_count];

    let mut src_bounds = original_src_bounds;
    let mut dst_bounds = allocated_dst_bounds;

    let mut loop_start = src_bounds.left.max(dst_bounds.left);
    let mut loop_end = src_bounds.right.min(dst_bounds.right);
    let mut dst_y_offset = 0usize;
    let mut y_from_dst = false;

    if win_x > 1 {
        let y_start = src_bounds.top.max(dst_bounds.top);
        let y_end = src_bounds.bottom.min(dst_bounds.bottom);
        if y_start < y_end {
            let mut pass_x = RgbaPassKind::new(sigma.width);
            let mut src_off = ((y_start - src_bounds.top) as usize) * src_row_pixels;
            let mut dst_off = ((y_start - dst_bounds.top) as usize) * dst_row_pixels;
            for _ in y_start..y_end {
                pass_x.blur_line(
                    src_bounds.left - dst_bounds.left,
                    src_bounds.right - dst_bounds.left,
                    dst_bounds.width(),
                    &src_px,
                    src_off,
                    1,
                    &mut dst_px,
                    dst_off,
                    1,
                );
                src_off += src_row_pixels;
                dst_off += dst_row_pixels;
            }
        }
        y_from_dst = true;
        loop_start = original_dst_bounds.left;
        loop_end = original_dst_bounds.right;
        dst_y_offset = (original_dst_bounds.top - dst_bounds.top) as usize;
        src_bounds = dst_bounds;
        dst_bounds = original_dst_bounds;
    }

    if win_y > 1 && loop_start < loop_end {
        let mut pass_y = RgbaPassKind::new(sigma.height);
        if y_from_dst {
            let col_len = src_bounds.height() as usize;
            let mut col_tmp = vec![[0u8; 4]; col_len];
            for x in loop_start..loop_end {
                let src_col_x = (x - src_bounds.left) as usize;
                let dst_col_x = (x - dst_bounds.left) as usize;
                for row in 0..col_len {
                    col_tmp[row] = dst_px[row * dst_row_pixels + src_col_x];
                }
                pass_y.blur_line(
                    src_bounds.top - dst_bounds.top,
                    src_bounds.bottom - dst_bounds.top,
                    dst_bounds.height(),
                    &col_tmp,
                    0,
                    1,
                    &mut dst_px,
                    dst_y_offset * dst_row_pixels + dst_col_x,
                    dst_row_pixels,
                );
            }
        } else {
            for x in loop_start..loop_end {
                let src_col_x = (x - src_bounds.left) as usize;
                let dst_col_x = (x - dst_bounds.left) as usize;
                pass_y.blur_line(
                    src_bounds.top - dst_bounds.top,
                    src_bounds.bottom - dst_bounds.top,
                    dst_bounds.height(),
                    &src_px,
                    src_col_x,
                    src_row_pixels,
                    &mut dst_px,
                    dst_y_offset * dst_row_pixels + dst_col_x,
                    dst_row_pixels,
                );
            }
        }
    }

    for (dst_chunk, px) in dst_bytes.chunks_exact_mut(4).zip(dst_px.iter()) {
        dst_chunk.copy_from_slice(px);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_2d_blur_kernel_normalized() {
        let mut k = [0.0_f32; MAX_SAMPLES];
        let sigma = Size { width: 0.5, height: 0.5 };
        let r = ISize { width: sigma_to_radius(0.5), height: sigma_to_radius(0.5) };
        assert_eq!(r, ISize { width: 2, height: 2 });
        compute_2d_blur_kernel(sigma, r, &mut k);
        let sum: f32 = k[..25].iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "sum={sum}");
        assert_eq!(k[25], 0.0);
        assert_eq!(k[27], 0.0);
    }

    #[test]
    fn test_compute_1d_linear_kernel_sum() {
        for &sigma in &[0.5_f32, 1.0, 2.0, 4.0] {
            let r = sigma_to_radius(sigma);
            let mut ok = [V4 { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }; MAX_SAMPLES / 2];
            compute_1d_blur_linear_kernel(sigma, r, &mut ok);
            let weight_sum: f32 = ok.iter().map(|v| v.y + v.w).sum();
            assert!((weight_sum - 1.0).abs() < 1e-4, "sigma={sigma} sum={weight_sum}");
        }
    }

    #[test]
    fn test_eval_blur_passes_a8_small_and_large() {
        for &sigma_val in &[1.0_f32, 4.0_f32] {
            let sigma = Size { width: sigma_val, height: sigma_val };
            let w = 16usize;
            let h = 16usize;
            let mut src = vec![0u8; w * h];
            for y in 4..12 {
                for x in 4..12 {
                    src[y * w + x] = 255;
                }
            }
            let src_b = IRect::make_ltrb(0, 0, w as i32, h as i32);
            let orig_dst_b = IRect::make_ltrb(0, 0, w as i32, h as i32);
            let ry = sigma_to_radius(sigma.height);
            let alloc_dst_b = IRect::make_ltrb(0, -ry, w as i32, h as i32 + ry);
            let mut dst = vec![0u8; (alloc_dst_b.width() * alloc_dst_b.height()) as usize];

            eval_blur_passes_a8(sigma, &src, w, src_b, &mut dst, w, alloc_dst_b, orig_dst_b);

            let y_off = (orig_dst_b.top - alloc_dst_b.top) as usize;
            let center = dst[(y_off + 8) * w + 8];
            assert!(center > 100, "sigma={sigma_val} center={center}");
            // Symmetry around (7.5, 7.5)
            assert_eq!(dst[(y_off + 4) * w + 4], dst[(y_off + 11) * w + 11]);
        }
    }

    #[test]
    fn test_eval_blur_passes_8888_all_tiers() {
        // Test Gaussian (<2), ThreeBox (2..135), and Tent (>=136)
        for &sigma_val in &[1.0_f32, 3.0_f32, 150.0_f32] {
            let sigma = Size { width: sigma_val, height: 0.0 };
            let w = 8usize;
            let h = 4usize;
            let src = vec![200u8; w * h * 4];
            let bounds = IRect::make_ltrb(0, 0, w as i32, h as i32);
            let mut dst = vec![0u8; w * h * 4];
            eval_blur_passes_8888(sigma, &src, w, bounds, &mut dst, w, bounds, bounds);
            // Center pixel must be non-zero
            assert!(dst[(2 * w + 4) * 4] > 0, "sigma={sigma_val}");
        }
    }

    #[test]
    fn test_stable_key_offset() {
        assert_eq!(stable_key_offset(2), 0);
        assert_eq!(stable_key_offset(4), 0);
        assert_eq!(stable_key_offset(5), 1);
        assert_eq!(stable_key_offset(28), 5);
    }
}
