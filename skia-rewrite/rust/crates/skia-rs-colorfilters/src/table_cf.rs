//! Safe Rust port of `src/effects/colorfilters/SkTableColorFilter.cpp`.

use skia_rs_color::color::Color4f;
use skia_rs_geometry::pin_f32;

use crate::color_filter::{premul_color4f, unpremul_color4f};

/// Stage plan computed by `SkTableColorFilter::appendStages`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TableStagePlan {
    pub unpremul: bool,
    pub premul: bool,
}

/// Exact port of `SkTableColorFilter::appendStages` stage decision logic.
#[inline]
pub const fn plan_table_stages(shader_is_opaque: bool, alpha_table_255: u8) -> TableStagePlan {
    let definitely_opaque = shader_is_opaque && alpha_table_255 == 0xFF;
    TableStagePlan {
        unpremul: !shader_is_opaque,
        premul: !definitely_opaque,
    }
}

/// Pure-Rust evaluation of a 4-channel 256-entry lookup table filter on a premul `Color4f`.
pub fn apply_table_pm_color(
    table_a: &[u8; 256],
    table_r: &[u8; 256],
    table_g: &[u8; 256],
    table_b: &[u8; 256],
    pm: Color4f,
    shader_is_opaque: bool,
) -> Color4f {
    let plan = plan_table_stages(shader_is_opaque, table_a[0xFF]);
    let u = if plan.unpremul {
        unpremul_color4f(pm)
    } else {
        pm
    };

    let idx = |v: f32| -> usize { (pin_f32(v, 0.0_f32, 1.0_f32) * 255.0_f32 + 0.5_f32) as usize };
    let inv255 = 1.0_f32 / 255.0_f32;

    let mapped = Color4f::new(
        (table_r[idx(u.r)] as f32) * inv255,
        (table_g[idx(u.g)] as f32) * inv255,
        (table_b[idx(u.b)] as f32) * inv255,
        (table_a[idx(u.a)] as f32) * inv255,
    );

    if plan.premul {
        premul_color4f(mapped)
    } else {
        mapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_stage_plan() {
        assert_eq!(
            plan_table_stages(true, 0xFF),
            TableStagePlan {
                unpremul: false,
                premul: false,
            }
        );
        assert_eq!(
            plan_table_stages(true, 0x80),
            TableStagePlan {
                unpremul: false,
                premul: true,
            }
        );
        assert_eq!(
            plan_table_stages(false, 0xFF),
            TableStagePlan {
                unpremul: true,
                premul: true,
            }
        );
    }

    #[test]
    fn table_invert_channels() {
        let mut id = [0u8; 256];
        let mut inv = [0u8; 256];
        for i in 0..256 {
            id[i] = i as u8;
            inv[i] = (255 - i) as u8;
        }
        let out = apply_table_pm_color(&id, &inv, &inv, &inv, Color4f::new(1.0, 0.0, 0.5, 1.0), true);
        assert_eq!(out.r, 0.0);
        assert_eq!(out.g, 1.0);
        assert!((out.b - 127.0 / 255.0).abs() < 1e-6);
        assert_eq!(out.a, 1.0);
    }
}
