#![forbid(unsafe_code)]

//! Safe Rust port of `src/core/SkDistanceFieldGen.h` and `src/core/SkDistanceFieldGen.cpp`.

use skia_rs_geometry::{pin_f32, scalar_nearly_zero, scalar_round_to_int, Point, SK_SCALAR_NEARLY_ZERO};

pub const DISTANCE_FIELD_MAGNITUDE: i32 = 4;
pub const DISTANCE_FIELD_PAD: i32 = 4;
pub const DISTANCE_FIELD_INSET: i32 = 2;

const SK_SCALAR_SQRT_2: f32 = 1.414_213_56_f32;

#[derive(Copy, Clone, Debug, Default)]
struct DFData {
    alpha: f32,
    dist_sq: f32,
    dist_vector: Point,
}

const DF_DATA_SIZE: usize = 16; // Matches C++ sizeof(DFData) == 4 + 4 + 8

const K_LEFT: u32 = 0x01;
const K_RIGHT: u32 = 0x02;
const K_TOP_LEFT: u32 = 0x04;
const K_TOP: u32 = 0x08;
const K_TOP_RIGHT: u32 = 0x10;
const K_BOTTOM_LEFT: u32 = 0x20;
const K_BOTTOM: u32 = 0x40;
const K_BOTTOM_RIGHT: u32 = 0x80;
const K_ALL_NEIGHBORS: u32 = 0xFF;

/// Computes `(grid_count, copy_count)` if total scratch storage fits in signed `int32_t`,
/// matching `DFScratch::ComputeTotalSize` in `SkDistanceFieldGen.cpp` bit-for-bit.
fn compute_scratch_counts(w: i32, h: i32) -> Option<(usize, usize)> {
    if w <= 0 || h <= 0 {
        return None;
    }
    const K_PAD: i32 = DISTANCE_FIELD_PAD + 1;
    let gw = w.checked_add(2 * K_PAD)?;
    let gh = h.checked_add(2 * K_PAD)?;
    let grid_count = (gw as usize).checked_mul(gh as usize)?;
    let cw = w.checked_add(2)?;
    let ch = h.checked_add(2)?;
    let copy_count = (cw as usize).checked_mul(ch as usize)?;
    let data_bytes = grid_count.checked_mul(DF_DATA_SIZE)?;
    let total = data_bytes.checked_add(grid_count)?.checked_add(copy_count)?;
    if total > i32::MAX as usize {
        return None;
    }
    Some((grid_count, copy_count))
}

/// Matches `SkComputeDistanceFieldSize`.
pub fn compute_distance_field_size(w: i32, h: i32) -> usize {
    if compute_scratch_counts(w, h).is_none() {
        return 0;
    }
    ((w + 2 * DISTANCE_FIELD_PAD) as usize) * ((h + 2 * DISTANCE_FIELD_PAD) as usize)
}

fn found_edge(image: &[u8], idx: usize, width: isize, neighbor_flags: u32) -> bool {
    let offsets: [isize; 8] = [
        -1,
        1,
        -width - 1,
        -width,
        -width + 1,
        width - 1,
        width,
        width + 1,
    ];
    let curr_val = image[idx];
    let curr_check = curr_val >> 7;
    for (i, &off) in offsets.iter().enumerate() {
        let neighbor_val = if ((1u32 << i) & neighbor_flags) != 0 {
            image[(idx as isize + off) as usize]
        } else {
            0
        };
        let neighbor_check = neighbor_val >> 7;
        if curr_check != neighbor_check
            || (curr_check == 0 && neighbor_check == 0 && curr_val != 0 && neighbor_val != 0)
        {
            return true;
        }
    }
    false
}

fn init_glyph_data(
    data: &mut [DFData],
    edges: &mut [u8],
    image: &[u8],
    data_width: usize,
    image_width: usize,
    image_height: usize,
    pad: usize,
) {
    let mut data_idx = pad * data_width + pad;
    let mut img_idx = 0usize;
    let w_isize = image_width as isize;

    for j in 0..image_height {
        for i in 0..image_width {
            let px = image[img_idx];
            data[data_idx].alpha = if px == 255 {
                1.0_f32
            } else {
                (px as f32) * 0.003_921_568_62_f32
            };
            let mut check_mask = K_ALL_NEIGHBORS;
            if i == 0 {
                check_mask &= !(K_LEFT | K_TOP_LEFT | K_BOTTOM_LEFT);
            }
            if i == image_width - 1 {
                check_mask &= !(K_RIGHT | K_TOP_RIGHT | K_BOTTOM_RIGHT);
            }
            if j == 0 {
                check_mask &= !(K_TOP_LEFT | K_TOP | K_TOP_RIGHT);
            }
            if j == image_height - 1 {
                check_mask &= !(K_BOTTOM_LEFT | K_BOTTOM | K_BOTTOM_RIGHT);
            }
            if found_edge(image, img_idx, w_isize, check_mask) {
                edges[data_idx] = 255;
            }
            data_idx += 1;
            img_idx += 1;
        }
        data_idx += 2 * pad;
    }
}

fn edge_distance(direction: Point, alpha: f32) -> f32 {
    let mut dx = direction.x;
    let mut dy = direction.y;
    if scalar_nearly_zero(dx, SK_SCALAR_NEARLY_ZERO)
        || scalar_nearly_zero(dy, SK_SCALAR_NEARLY_ZERO)
    {
        0.5_f32 - alpha
    } else {
        dx = dx.abs();
        dy = dy.abs();
        if dx < dy {
            core::mem::swap(&mut dx, &mut dy);
        }
        let a1num = 0.5_f32 * dy;
        if alpha * dx < a1num {
            0.5_f32 * (dx + dy) - (2.0_f32 * dx * dy * alpha).sqrt()
        } else if alpha * dx < (dx - a1num) {
            (0.5_f32 - alpha) * dx
        } else {
            -0.5_f32 * (dx + dy) + (2.0_f32 * dx * dy * (1.0_f32 - alpha)).sqrt()
        }
    }
}

fn init_distances(data: &mut [DFData], edges: &[u8], width: usize, height: usize) {
    for j in 0..height {
        for i in 0..width {
            let idx = j * width + i;
            if edges[idx] != 0 {
                debug_assert!(i > 0 && i < width - 1 && j > 0 && j < height - 1);
                let prev = idx - width;
                let next = idx + width;
                let mut curr_grad = Point::new(
                    data[prev + 1].alpha - data[prev - 1].alpha
                        + SK_SCALAR_SQRT_2 * data[idx + 1].alpha
                        - SK_SCALAR_SQRT_2 * data[idx - 1].alpha
                        + data[next + 1].alpha
                        - data[next - 1].alpha,
                    data[next - 1].alpha - data[prev - 1].alpha
                        + SK_SCALAR_SQRT_2 * data[next].alpha
                        - SK_SCALAR_SQRT_2 * data[prev].alpha
                        + data[next + 1].alpha
                        - data[prev + 1].alpha,
                );
                // Matches SkPointPriv::SetLengthFast(&currGrad, 1.0f)
                curr_grad.set_length_in_place(1.0_f32);

                let dist = edge_distance(curr_grad, data[idx].alpha);
                data[idx].dist_vector = curr_grad.scale(dist);
                data[idx].dist_sq = dist * dist;
            } else {
                data[idx].dist_sq = 2_000_000.0_f32;
                data[idx].dist_vector = Point::new(1000.0_f32, 1000.0_f32);
            }
        }
    }
}

#[inline]
fn f1_step(data: &mut [DFData], curr: usize, width: usize) {
    // upper left
    {
        let check = data[curr - width - 1];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq - 2.0_f32 * (dist_vec.x + dist_vec.y - 1.0_f32);
        if dist_sq < data[curr].dist_sq {
            dist_vec.x -= 1.0_f32;
            dist_vec.y -= 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
    // up
    {
        let check = data[curr - width];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq - 2.0_f32 * dist_vec.y + 1.0_f32;
        if dist_sq < data[curr].dist_sq {
            dist_vec.y -= 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
    // upper right
    {
        let check = data[curr - width + 1];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq + 2.0_f32 * (dist_vec.x - dist_vec.y + 1.0_f32);
        if dist_sq < data[curr].dist_sq {
            dist_vec.x += 1.0_f32;
            dist_vec.y -= 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
    // left
    {
        let check = data[curr - 1];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq - 2.0_f32 * dist_vec.x + 1.0_f32;
        if dist_sq < data[curr].dist_sq {
            dist_vec.x -= 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
}

#[inline]
fn f2_step(data: &mut [DFData], curr: usize) {
    let check = data[curr + 1];
    let mut dist_vec = check.dist_vector;
    let dist_sq = check.dist_sq + 2.0_f32 * dist_vec.x + 1.0_f32;
    if dist_sq < data[curr].dist_sq {
        dist_vec.x += 1.0_f32;
        data[curr].dist_sq = dist_sq;
        data[curr].dist_vector = dist_vec;
    }
}

#[inline]
fn b1_step(data: &mut [DFData], curr: usize) {
    let check = data[curr - 1];
    let mut dist_vec = check.dist_vector;
    let dist_sq = check.dist_sq - 2.0_f32 * dist_vec.x + 1.0_f32;
    if dist_sq < data[curr].dist_sq {
        dist_vec.x -= 1.0_f32;
        data[curr].dist_sq = dist_sq;
        data[curr].dist_vector = dist_vec;
    }
}

#[inline]
fn b2_step(data: &mut [DFData], curr: usize, width: usize) {
    // right
    {
        let check = data[curr + 1];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq + 2.0_f32 * dist_vec.x + 1.0_f32;
        if dist_sq < data[curr].dist_sq {
            dist_vec.x += 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
    // bottom left
    {
        let check = data[curr + width - 1];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq - 2.0_f32 * (dist_vec.x - dist_vec.y - 1.0_f32);
        if dist_sq < data[curr].dist_sq {
            dist_vec.x -= 1.0_f32;
            dist_vec.y += 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
    // bottom
    {
        let check = data[curr + width];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq + 2.0_f32 * dist_vec.y + 1.0_f32;
        if dist_sq < data[curr].dist_sq {
            dist_vec.y += 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
    // bottom right
    {
        let check = data[curr + width + 1];
        let mut dist_vec = check.dist_vector;
        let dist_sq = check.dist_sq + 2.0_f32 * (dist_vec.x + dist_vec.y + 1.0_f32);
        if dist_sq < data[curr].dist_sq {
            dist_vec.x += 1.0_f32;
            dist_vec.y += 1.0_f32;
            data[curr].dist_sq = dist_sq;
            data[curr].dist_vector = dist_vec;
        }
    }
}

#[inline]
fn pack_distance_field_val(mut dist: f32) -> u8 {
    let mag = DISTANCE_FIELD_MAGNITUDE as f32;
    dist = pin_f32(-dist, -mag, mag * 127.0_f32 / 128.0_f32);
    dist += mag;
    scalar_round_to_int(dist / (2.0_f32 * mag) * 256.0_f32) as u8
}

fn generate_distance_field_from_copy(
    distance_field: &mut [u8],
    copy: &[u8],
    grid_count: usize,
    width: usize,
    height: usize,
) -> bool {
    let pad = (DISTANCE_FIELD_PAD + 1) as usize;
    let data_width = width + 2 * pad;
    let data_height = height + 2 * pad;

    let mut data = vec![DFData::default(); grid_count];
    let mut edges = vec![0u8; grid_count];

    init_glyph_data(
        &mut data,
        &mut edges,
        copy,
        data_width,
        width + 2,
        height + 2,
        DISTANCE_FIELD_PAD as usize,
    );

    init_distances(&mut data, &edges, data_width, data_height);

    // Forward pass in y (matches C++ lines 453-481 exactly)
    let mut curr = data_width + 1;
    for _j in 1..(data_height - 1) {
        for _i in 1..(data_width - 1) {
            if edges[curr] == 0 {
                f1_step(&mut data, curr, data_width);
            }
            curr += 1;
        }
        curr -= 1;
        for _i in 1..(data_width - 1) {
            if edges[curr] == 0 {
                f2_step(&mut data, curr);
            }
            curr -= 1;
        }
        curr += data_width + 1;
    }

    // Backward pass in y (matches C++ lines 483-511 bit-for-bit, including initial offset)
    let mut curr = data_width * (data_height - 2) - 1;
    for _j in 1..(data_height - 1) {
        for _i in 1..(data_width - 1) {
            if edges[curr] == 0 {
                b1_step(&mut data, curr);
            }
            curr += 1;
        }
        curr -= 1;
        for _i in 1..(data_width - 1) {
            if edges[curr] == 0 {
                b2_step(&mut data, curr, data_width);
            }
            curr -= 1;
        }
        curr = curr.wrapping_sub(data_width - 1);
    }

    // Pack output
    let mut curr = data_width + 1;
    let mut df_idx = 0usize;
    for _j in 1..(data_height - 1) {
        for _i in 1..(data_width - 1) {
            let d = data[curr].dist_sq.sqrt();
            let dist = if data[curr].alpha > 0.5_f32 { -d } else { d };
            distance_field[df_idx] = pack_distance_field_val(dist);
            df_idx += 1;
            curr += 1;
        }
        curr += 2;
    }

    true
}

/// Matches `SkGenerateDistanceFieldFromA8Image`.
pub fn generate_distance_field_from_a8_image(
    distance_field: &mut [u8],
    image: &[u8],
    width: i32,
    height: i32,
    row_bytes: usize,
) -> bool {
    let Some((grid_count, copy_count)) = compute_scratch_counts(width, height) else {
        return false;
    };
    let w = width as usize;
    let h = height as usize;
    let expected_df = (w + 2 * DISTANCE_FIELD_PAD as usize) * (h + 2 * DISTANCE_FIELD_PAD as usize);
    if distance_field.len() < expected_df {
        return false;
    }

    let mut copy = vec![0u8; copy_count];
    let mut dst_pos = w + 2;
    for row in 0..h {
        let src_start = row * row_bytes;
        copy[dst_pos] = 0;
        dst_pos += 1;
        copy[dst_pos..dst_pos + w].copy_from_slice(&image[src_start..src_start + w]);
        dst_pos += w;
        copy[dst_pos] = 0;
        dst_pos += 1;
    }

    generate_distance_field_from_copy(distance_field, &copy, grid_count, w, h)
}

#[inline]
fn lcd16_to_alpha(packed: u16) -> u8 {
    let p = packed as u32;
    let r5 = (p >> 11) & 0x1F;
    let g6 = (p >> 5) & 0x3F;
    let b5 = p & 0x1F;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g6 << 2) | (g6 >> 4);
    let b = (b5 << 3) | (b5 >> 2);
    ((r + g + b) / 3) as u8
}

/// Matches `SkGenerateDistanceFieldFromLCD16Mask`.
pub fn generate_distance_field_from_lcd16_mask(
    distance_field: &mut [u8],
    image: &[u8],
    width: i32,
    height: i32,
    row_bytes: usize,
) -> bool {
    let Some((grid_count, copy_count)) = compute_scratch_counts(width, height) else {
        return false;
    };
    let w = width as usize;
    let h = height as usize;
    let expected_df = (w + 2 * DISTANCE_FIELD_PAD as usize) * (h + 2 * DISTANCE_FIELD_PAD as usize);
    if distance_field.len() < expected_df {
        return false;
    }

    let mut copy = vec![0u8; copy_count];
    let mut dst_pos = w + 2;
    for row in 0..h {
        let src_row = row * row_bytes;
        copy[dst_pos] = 0;
        dst_pos += 1;
        for col in 0..w {
            let off = src_row + col * 2;
            let packed = u16::from_ne_bytes([image[off], image[off + 1]]);
            copy[dst_pos] = lcd16_to_alpha(packed);
            dst_pos += 1;
        }
        copy[dst_pos] = 0;
        dst_pos += 1;
    }

    generate_distance_field_from_copy(distance_field, &copy, grid_count, w, h)
}

/// Matches `SkGenerateDistanceFieldFromBWImage`.
pub fn generate_distance_field_from_bw_image(
    distance_field: &mut [u8],
    image: &[u8],
    width: i32,
    height: i32,
    row_bytes: usize,
) -> bool {
    let Some((grid_count, copy_count)) = compute_scratch_counts(width, height) else {
        return false;
    };
    let w = width as usize;
    let h = height as usize;
    let expected_df = (w + 2 * DISTANCE_FIELD_PAD as usize) * (h + 2 * DISTANCE_FIELD_PAD as usize);
    if distance_field.len() < expected_df {
        return false;
    }

    let mut copy = vec![0u8; copy_count];
    let mut dst_pos = w + 2;
    for row in 0..h {
        let mut src_idx = row * row_bytes;
        copy[dst_pos] = 0;
        dst_pos += 1;
        let mut row_writes_left = w;
        while row_writes_left > 0 {
            let mask = image[src_idx];
            src_idx += 1;
            let mut j = 7i32;
            while j >= 0 && row_writes_left > 0 {
                copy[dst_pos] = if (mask & (1u8 << (j as u32))) != 0 {
                    0xFF
                } else {
                    0
                };
                dst_pos += 1;
                j -= 1;
                row_writes_left -= 1;
            }
        }
        copy[dst_pos] = 0;
        dst_pos += 1;
    }

    generate_distance_field_from_copy(distance_field, &copy, grid_count, w, h)
}
