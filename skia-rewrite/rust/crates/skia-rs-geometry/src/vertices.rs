#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU32, Ordering};

pub const MODE_TRIANGLES: i32 = 0;
pub const MODE_TRIANGLE_STRIP: i32 = 1;
pub const MODE_TRIANGLE_FAN: i32 = 2;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

pub fn next_vertices_id() -> u32 {
    loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// Safe Rust equivalent of `SkVertices::Sizes`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct VerticesSizes {
    pub total: usize,
    pub arrays: usize,
    pub v_size: usize,
    pub t_size: usize,
    pub c_size: usize,
    pub i_size: usize,
    pub builder_tri_fan_i_size: usize,
}

impl VerticesSizes {
    pub fn compute(
        obj_size: usize,
        mode: i32,
        vertex_count: i32,
        index_count: i32,
        has_texs: bool,
        has_colors: bool,
    ) -> Self {
        if vertex_count < 0 || index_count < 0 {
            return Self::default();
        }
        let vc = vertex_count as usize;
        let ic = index_count as usize;

        let Some(v_size) = vc.checked_mul(8) else {
            return Self::default();
        };
        let t_size = if has_texs {
            match vc.checked_mul(8) {
                Some(s) => s,
                None => return Self::default(),
            }
        } else {
            0
        };
        let c_size = if has_colors {
            match vc.checked_mul(4) {
                Some(s) => s,
                None => return Self::default(),
            }
        } else {
            0
        };

        let mut builder_tri_fan_i_size = 0usize;
        let Some(mut i_size) = ic.checked_mul(2) else {
            return Self::default();
        };

        if mode == MODE_TRIANGLE_FAN {
            let num_fan_tris = if index_count != 0 {
                builder_tri_fan_i_size = i_size;
                index_count - 2
            } else {
                if vertex_count > (u16::MAX as i32) + 1 {
                    return Self::default();
                }
                vertex_count - 2
            };
            if num_fan_tris <= 0 {
                return Self::default();
            }
            i_size = match (num_fan_tris as usize).checked_mul(6) {
                Some(s) => s,
                None => return Self::default(),
            };
        }

        let Some(arrays) = v_size
            .checked_add(t_size)
            .and_then(|s| s.checked_add(c_size))
            .and_then(|s| s.checked_add(i_size))
        else {
            return Self::default();
        };
        let Some(total) = obj_size.checked_add(arrays) else {
            return Self::default();
        };

        Self {
            total,
            arrays,
            v_size,
            t_size,
            c_size,
            i_size,
            builder_tri_fan_i_size,
        }
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.v_size > 0
    }
}

/// Converts triangle-fan indices into triangle-list indices.
pub fn convert_tri_fan_indices(
    src_indices: Option<&[u16]>,
    vertex_count: i32,
    dst_indices: &mut [u16],
) -> i32 {
    if let Some(src) = src_indices {
        let tris = (src.len() as i32) - 2;
        for t in 0..(tris as usize) {
            dst_indices[3 * t] = src[0];
            dst_indices[3 * t + 1] = src[t + 1];
            dst_indices[3 * t + 2] = src[t + 2];
        }
        3 * tris
    } else {
        let tris = vertex_count - 2;
        for t in 0..(tris as usize) {
            dst_indices[3 * t] = 0;
            dst_indices[3 * t + 1] = (t + 1) as u16;
            dst_indices[3 * t + 2] = (t + 2) as u16;
        }
        3 * tris
    }
}

pub fn clamp_indices(src: &[u16], dst: &mut [u16], vertex_count: i32) {
    let max_index = (vertex_count - 1) as u16;
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = s.min(max_index);
    }
}

pub fn validate_indices(indices: &[u16], vertex_count: i32) -> bool {
    let vc = vertex_count as u32;
    indices.iter().all(|&idx| (idx as u32) < vc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertices_sizes_and_fan() {
        let sz = VerticesSizes::compute(64, MODE_TRIANGLE_FAN, 5, 0, true, false);
        assert!(sz.is_valid());
        assert_eq!(sz.v_size, 40);
        assert_eq!(sz.t_size, 40);
        assert_eq!(sz.i_size, 18); // 3 tris * 3 indices * 2 bytes
        let mut dst = [0u16; 9];
        assert_eq!(convert_tri_fan_indices(None, 5, &mut dst), 9);
        assert_eq!(dst, [0, 1, 2, 0, 2, 3, 0, 3, 4]);
    }
}
