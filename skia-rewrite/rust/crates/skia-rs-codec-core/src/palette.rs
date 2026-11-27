//! Safe Rust port of `src/codec/SkColorPalette.cpp`.

/// Maximum number of entries in an `SkColorPalette`.
pub const MAX_PALETTE_COLORS: usize = 256;

/// Safe Rust representation of `SkColorPalette`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorPalette {
    colors: Vec<u32>,
}

impl ColorPalette {
    /// Creates a new color palette copying up to `MAX_PALETTE_COLORS` colors.
    pub fn new(colors: &[u32]) -> Option<Self> {
        if colors.len() > MAX_PALETTE_COLORS {
            return None;
        }
        Some(Self {
            colors: colors.to_vec(),
        })
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.colors.len()
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<u32> {
        self.colors.get(index).copied()
    }

    #[inline]
    pub fn as_slice(&self) -> &[u32] {
        &self.colors
    }
}

/// Copy `count` palette colors from `src` into `dst`.
/// Mirrors `SkColorPalette::SkColorPalette(const SkPMColor colors[], int count)`.
pub fn copy_palette(dst: &mut [u32], src: &[u32], count: usize) -> bool {
    if count > MAX_PALETTE_COLORS || dst.len() < count || src.len() < count {
        return false;
    }
    dst[..count].copy_from_slice(&src[..count]);
    true
}
