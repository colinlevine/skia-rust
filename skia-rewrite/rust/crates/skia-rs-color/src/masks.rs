//! Safe Rust port of `src/core/SkMasks.cpp`.

/// Lookup table converting 1..7 bit color components to 8-bit components.
const N_BIT_TO_8_BIT_LOOKUP_TABLE: [u8; 254] = [
    // 1 bit
    0, 255,
    // 2 bits
    0, 85, 170, 255,
    // 3 bits
    0, 36, 73, 109, 146, 182, 219, 255,
    // 4 bits
    0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
    // 5 bits
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140,
    148, 156, 165, 173, 181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
    // 6 bits
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 45, 49, 53, 57, 61, 65, 69, 73,
    77, 81, 85, 89, 93, 97, 101, 105, 109, 113, 117, 121, 125, 130, 134, 138,
    142, 146, 150, 154, 158, 162, 166, 170, 174, 178, 182, 186, 190, 194, 198,
    202, 206, 210, 215, 219, 223, 227, 231, 235, 239, 243, 247, 251, 255,
    // 7 bits
    0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38,
    40, 42, 44, 46, 48, 50, 52, 54, 56, 58, 60, 62, 64, 66, 68, 70, 72, 74, 76,
    78, 80, 82, 84, 86, 88, 90, 92, 94, 96, 98, 100, 102, 104, 106, 108, 110,
    112, 114, 116, 118, 120, 122, 124, 126, 129, 131, 133, 135, 137, 139, 141,
    143, 145, 147, 149, 151, 153, 155, 157, 159, 161, 163, 165, 167, 169, 171,
    173, 175, 177, 179, 181, 183, 185, 187, 189, 191, 193, 195, 197, 199, 201,
    203, 205, 207, 209, 211, 213, 215, 217, 219, 221, 223, 225, 227, 229, 231,
    233, 235, 237, 239, 241, 243, 245, 247, 249, 251, 253, 255,
];

/// Matches `SkMasks::MaskInfo`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MaskInfo {
    pub mask: u32,
    pub shift: u32,
    pub size: u32,
}

/// Matches `SkMasks::InputMasks`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct InputMasks {
    pub red: u32,
    pub green: u32,
    pub blue: u32,
    pub alpha: u32,
}

/// Matches `SkMasks` layout (`fRed`, `fGreen`, `fBlue`, `fAlpha`).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Masks {
    pub red: MaskInfo,
    pub green: MaskInfo,
    pub blue: MaskInfo,
    pub alpha: MaskInfo,
}

#[inline]
fn convert_to_8(component: u8, n: u32) -> u8 {
    if n == 0 {
        0
    } else if n < 8 {
        let idx = ((1usize << n) - 2) + component as usize;
        N_BIT_TO_8_BIT_LOOKUP_TABLE[idx]
    } else {
        debug_assert_eq!(n, 8);
        component
    }
}

#[inline]
fn get_comp(pixel: u32, info: MaskInfo) -> u8 {
    convert_to_8(((pixel & info.mask) >> info.shift) as u8, info.size)
}

/// Exact port of `process_mask`.
pub fn process_mask(mut mask: u32) -> MaskInfo {
    let mut temp_mask = mask;
    let mut shift = 0u32;
    let mut size = 0u32;
    if temp_mask != 0 {
        while (temp_mask & 1) == 0 {
            temp_mask >>= 1;
            shift += 1;
        }
        while (temp_mask & 1) != 0 {
            temp_mask >>= 1;
            size += 1;
        }
        if temp_mask != 0 {
            // Non-continuous bit mask: continue counting width as Skia does.
            while temp_mask != 0 {
                temp_mask >>= 1;
                size += 1;
            }
        }
        if size > 8 {
            shift += size - 8;
            size = 8;
            mask &= 0xFFu32 << shift;
        }
    }
    MaskInfo { mask, shift, size }
}

impl Masks {
    /// Exact port of `SkMasks::CreateMasks`. Returns `None` if masks overlap.
    pub fn create(mut masks: InputMasks, bytes_per_pixel: i32) -> Option<Self> {
        debug_assert!(bytes_per_pixel > 0 && bytes_per_pixel <= 4);

        if bytes_per_pixel < 4 {
            let bits_per_pixel = 8 * bytes_per_pixel;
            let trim = (1u32 << bits_per_pixel) - 1;
            masks.red &= trim;
            masks.green &= trim;
            masks.blue &= trim;
            masks.alpha &= trim;
        }

        if ((masks.red & masks.green)
            | (masks.red & masks.blue)
            | (masks.red & masks.alpha)
            | (masks.green & masks.blue)
            | (masks.green & masks.alpha)
            | (masks.blue & masks.alpha))
            != 0
        {
            return None;
        }

        Some(Self {
            red: process_mask(masks.red),
            green: process_mask(masks.green),
            blue: process_mask(masks.blue),
            alpha: process_mask(masks.alpha),
        })
    }

    #[inline]
    pub fn get_red(&self, pixel: u32) -> u8 {
        get_comp(pixel, self.red)
    }

    #[inline]
    pub fn get_green(&self, pixel: u32) -> u8 {
        get_comp(pixel, self.green)
    }

    #[inline]
    pub fn get_blue(&self, pixel: u32) -> u8 {
        get_comp(pixel, self.blue)
    }

    #[inline]
    pub fn get_alpha(&self, pixel: u32) -> u8 {
        get_comp(pixel, self.alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_masks() {
        let m = Masks::create(
            InputMasks {
                red: 0xF800,
                green: 0x07E0,
                blue: 0x001F,
                alpha: 0,
            },
            2,
        )
        .unwrap();
        assert_eq!(m.get_red(0xFFFF), 255);
        assert_eq!(m.get_green(0xFFFF), 255);
        assert_eq!(m.get_blue(0xFFFF), 255);
        assert_eq!(m.get_alpha(0xFFFF), 0);
        assert_eq!(m.get_red(0x0000), 0);
        assert_eq!(m.get_green(0x03E0), 125);
    }

    #[test]
    fn overlapping_masks_rejected() {
        assert!(Masks::create(
            InputMasks {
                red: 0xFF00,
                green: 0x0FF0,
                blue: 0x000F,
                alpha: 0,
            },
            2,
        )
        .is_none());
    }

    #[test]
    fn wide_mask_truncated_to_8_bits() {
        let m = Masks::create(
            InputMasks {
                red: 0x0000_03FF, // 10 bits -> truncated to top 8 bits (shift=2, mask=0x3FC)
                green: 0,
                blue: 0,
                alpha: 0,
            },
            4,
        )
        .unwrap();
        assert_eq!(m.red, MaskInfo { mask: 0x3FC, shift: 2, size: 8 });
        assert_eq!(m.get_red(0x3FF), 255);
        assert_eq!(m.get_red(0x200), 128);
    }
}
