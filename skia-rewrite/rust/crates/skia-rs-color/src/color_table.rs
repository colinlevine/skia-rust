//! Safe Rust port of `src/core/SkColorTable.cpp`.

/// Fill the 4x256 ARGB color table buffer (A=0..256, R=256..512, G=512..768, B=768..1024).
/// Returns `false` if all four input tables are `None` (identity table, `SkColorTable::Make` returns nullptr).
pub fn fill_color_table(
    table_a: Option<&[u8; 256]>,
    table_r: Option<&[u8; 256]>,
    table_g: Option<&[u8; 256]>,
    table_b: Option<&[u8; 256]>,
    out_argb: &mut [u8; 1024],
) -> bool {
    if table_a.is_none() && table_r.is_none() && table_g.is_none() && table_b.is_none() {
        return false;
    }

    let (a_out, rest) = out_argb.split_at_mut(256);
    let (r_out, rest) = rest.split_at_mut(256);
    let (g_out, b_out) = rest.split_at_mut(256);

    fill_channel(table_a, a_out);
    fill_channel(table_r, r_out);
    fill_channel(table_g, g_out);
    fill_channel(table_b, b_out);

    true
}

#[inline]
fn fill_channel(src: Option<&[u8; 256]>, dst: &mut [u8]) {
    if let Some(table) = src {
        dst.copy_from_slice(table);
    } else {
        for (i, slot) in dst.iter_mut().enumerate() {
            *slot = i as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_none_returns_false() {
        let mut out = [0u8; 1024];
        assert!(!fill_color_table(None, None, None, None, &mut out));
    }

    #[test]
    fn partial_tables_fill_identity() {
        let mut rev = [0u8; 256];
        for i in 0..256 {
            rev[i] = (255 - i) as u8;
        }
        let mut out = [0u8; 1024];
        assert!(fill_color_table(None, Some(&rev), None, None, &mut out));
        for i in 0..256 {
            assert_eq!(out[i], i as u8);
            assert_eq!(out[256 + i], (255 - i) as u8);
            assert_eq!(out[512 + i], i as u8);
            assert_eq!(out[768 + i], i as u8);
        }
    }
}
