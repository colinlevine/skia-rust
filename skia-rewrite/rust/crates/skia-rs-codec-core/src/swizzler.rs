//! Safe Rust port of `src/codec/SkSwizzler.cpp`.

use skia_rs_color::color::mul_div_255_round;
use skia_rs_color::image_info::{color_type_bytes_per_pixel, AlphaType, ColorType};
use skia_rs_geometry::IRect;

use crate::encoded_info::bits_per_pixel;
use crate::sampler::{get_sampled_dimension, get_start_coord};
use crate::types::{EncodedAlpha, EncodedColor, ZeroInitialized};

/// IEEE-754 half-precision 1.0 (`SK_Half1`).
const SK_HALF_1: u16 = 0x3C00;

/// Identifies every row procedure in `SkSwizzler`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwizzleProcId {
    Copy = 0,
    Sample1 = 1,
    Sample2 = 2,
    Sample4 = 3,
    Sample6 = 4,
    Sample8 = 5,

    BitToGrayscale = 6,
    BitToN32 = 7,
    BitTo565 = 8,
    BitToF16 = 9,

    SmallIndexTo565 = 10,
    SmallIndexToN32 = 11,

    IndexToN32 = 12,
    IndexToN32SkipZ = 13,
    IndexTo565 = 14,

    GrayToN32 = 15,
    FastGrayToN32 = 16,
    GrayTo565 = 17,

    GrayAlphaToN32Unpremul = 18,
    FastGrayAlphaToN32Unpremul = 19,
    GrayAlphaToN32Premul = 20,
    FastGrayAlphaToN32Premul = 21,
    GrayAlphaToN32UnpremulSkipZ = 22,
    FastGrayAlphaToN32UnpremulSkipZ = 23,
    GrayAlphaToN32PremulSkipZ = 24,
    FastGrayAlphaToN32PremulSkipZ = 25,
    GrayAlphaToA8 = 26,

    BgrTo565 = 27,

    RgbToRgba = 28,
    RgbToBgra = 29,
    FastRgbToRgba = 30,
    FastRgbToBgra = 31,
    RgbTo565 = 32,

    RgbaToRgbaPremul = 33,
    RgbaToBgraPremul = 34,
    FastRgbaToRgbaPremul = 35,
    FastRgbaToBgraPremul = 36,
    RgbaToBgraUnpremul = 37,
    FastRgbaToBgraUnpremul = 38,

    RgbaToRgbaPremulSkipZ = 39,
    FastRgbaToRgbaPremulSkipZ = 40,
    RgbaToBgraPremulSkipZ = 41,
    FastRgbaToBgraPremulSkipZ = 42,
    RgbaToBgraUnpremulSkipZ = 43,
    FastRgbaToBgraUnpremulSkipZ = 44,
    Sample4SkipZ = 45,
    CopySkipZ = 46,

    Rgb16ToRgba = 47,
    Rgb16ToBgra = 48,
    Rgb16To565 = 49,

    Rgba16ToRgbaUnpremul = 50,
    Rgba16ToRgbaPremul = 51,
    Rgba16ToBgraUnpremul = 52,
    Rgba16ToBgraPremul = 53,

    CmykToRgba = 54,
    CmykToBgra = 55,
    FastCmykToRgba = 56,
    FastCmykToBgra = 57,
    CmykTo565 = 58,
}

impl SwizzleProcId {
    pub const fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Copy),
            1 => Some(Self::Sample1),
            2 => Some(Self::Sample2),
            3 => Some(Self::Sample4),
            4 => Some(Self::Sample6),
            5 => Some(Self::Sample8),
            6 => Some(Self::BitToGrayscale),
            7 => Some(Self::BitToN32),
            8 => Some(Self::BitTo565),
            9 => Some(Self::BitToF16),
            10 => Some(Self::SmallIndexTo565),
            11 => Some(Self::SmallIndexToN32),
            12 => Some(Self::IndexToN32),
            13 => Some(Self::IndexToN32SkipZ),
            14 => Some(Self::IndexTo565),
            15 => Some(Self::GrayToN32),
            16 => Some(Self::FastGrayToN32),
            17 => Some(Self::GrayTo565),
            18 => Some(Self::GrayAlphaToN32Unpremul),
            19 => Some(Self::FastGrayAlphaToN32Unpremul),
            20 => Some(Self::GrayAlphaToN32Premul),
            21 => Some(Self::FastGrayAlphaToN32Premul),
            22 => Some(Self::GrayAlphaToN32UnpremulSkipZ),
            23 => Some(Self::FastGrayAlphaToN32UnpremulSkipZ),
            24 => Some(Self::GrayAlphaToN32PremulSkipZ),
            25 => Some(Self::FastGrayAlphaToN32PremulSkipZ),
            26 => Some(Self::GrayAlphaToA8),
            27 => Some(Self::BgrTo565),
            28 => Some(Self::RgbToRgba),
            29 => Some(Self::RgbToBgra),
            30 => Some(Self::FastRgbToRgba),
            31 => Some(Self::FastRgbToBgra),
            32 => Some(Self::RgbTo565),
            33 => Some(Self::RgbaToRgbaPremul),
            34 => Some(Self::RgbaToBgraPremul),
            35 => Some(Self::FastRgbaToRgbaPremul),
            36 => Some(Self::FastRgbaToBgraPremul),
            37 => Some(Self::RgbaToBgraUnpremul),
            38 => Some(Self::FastRgbaToBgraUnpremul),
            39 => Some(Self::RgbaToRgbaPremulSkipZ),
            40 => Some(Self::FastRgbaToRgbaPremulSkipZ),
            41 => Some(Self::RgbaToBgraPremulSkipZ),
            42 => Some(Self::FastRgbaToBgraPremulSkipZ),
            43 => Some(Self::RgbaToBgraUnpremulSkipZ),
            44 => Some(Self::FastRgbaToBgraUnpremulSkipZ),
            45 => Some(Self::Sample4SkipZ),
            46 => Some(Self::CopySkipZ),
            47 => Some(Self::Rgb16ToRgba),
            48 => Some(Self::Rgb16ToBgra),
            49 => Some(Self::Rgb16To565),
            50 => Some(Self::Rgba16ToRgbaUnpremul),
            51 => Some(Self::Rgba16ToRgbaPremul),
            52 => Some(Self::Rgba16ToBgraUnpremul),
            53 => Some(Self::Rgba16ToBgraPremul),
            54 => Some(Self::CmykToRgba),
            55 => Some(Self::CmykToBgra),
            56 => Some(Self::FastCmykToRgba),
            57 => Some(Self::FastCmykToBgra),
            58 => Some(Self::CmykTo565),
            _ => None,
        }
    }

    /// True if `bpp`, `delta_src`, and `offset` are measured in bits (not bytes).
    #[inline]
    pub const fn is_bit_packed(self) -> bool {
        matches!(
            self,
            Self::BitToGrayscale
                | Self::BitToN32
                | Self::BitTo565
                | Self::BitToF16
                | Self::SmallIndexTo565
                | Self::SmallIndexToN32
        )
    }

    /// Returns the number of color table entries required (0 if unused).
    #[inline]
    pub const fn ctable_len(self, bpp: usize) -> usize {
        match self {
            Self::SmallIndexTo565 | Self::SmallIndexToN32 => {
                if bpp <= 8 {
                    1usize << bpp
                } else {
                    256
                }
            }
            Self::IndexToN32 | Self::IndexToN32SkipZ | Self::IndexTo565 => 256,
            _ => 0,
        }
    }

    /// Destination bytes per pixel given `src_bpp` (needed for generic `Copy`).
    #[inline]
    pub const fn dst_bytes_per_pixel(self, src_bpp: usize) -> usize {
        match self {
            Self::Copy => src_bpp,
            Self::Sample1 | Self::BitToGrayscale | Self::GrayAlphaToA8 => 1,
            Self::Sample2
            | Self::BitTo565
            | Self::SmallIndexTo565
            | Self::IndexTo565
            | Self::GrayTo565
            | Self::BgrTo565
            | Self::RgbTo565
            | Self::Rgb16To565
            | Self::CmykTo565 => 2,
            Self::Sample6 => 6,
            Self::Sample8 | Self::BitToF16 => 8,
            _ => 4,
        }
    }
}

/// Initialization parameters returned for `SkSwizzler::Make` / `SkSwizzler::MakeSimple`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SwizzlerInit {
    /// `-1` if there is no fast proc.
    pub fast_proc_id: i32,
    pub slow_proc_id: i32,
    pub src_offset: i32,
    pub src_width: i32,
    pub dst_offset: i32,
    pub dst_width: i32,
    pub src_bpp: i32,
    pub dst_bpp: i32,
}

/// Result of `compute_sample_x` for `SkSwizzler::onSetSampleX`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SwizzlerSampleXResult {
    pub dst_offset_bytes: i32,
    pub swizzle_width: i32,
    pub allocated_width: i32,
    pub src_offset_units: i32,
    pub use_fast_proc: bool,
    pub safety_net_triggered: bool,
}

fn compute_geometry(
    dst_width_in: i32,
    subset: Option<IRect>,
    frame: Option<IRect>,
) -> (i32, i32, i32, i32) {
    let mut src_offset = 0;
    let mut src_width = dst_width_in;
    let mut dst_offset = 0;
    let mut dst_width = src_width;
    if let Some(sub) = subset {
        src_offset = sub.left;
        src_width = sub.width();
        dst_width = src_width;
    } else if let Some(fr) = frame {
        dst_offset = fr.left;
        src_width = fr.width();
    }
    (src_offset, src_width, dst_offset, dst_width)
}

/// Exact port of `SkSwizzler::MakeSimple` selection and initialization.
pub fn select_simple_swizzler(
    src_bpp: i32,
    dst_ct: ColorType,
    dst_width: i32,
    subset: Option<IRect>,
    frame: Option<IRect>,
) -> Option<SwizzlerInit> {
    let proc = match src_bpp {
        1 => SwizzleProcId::Sample1,
        2 => SwizzleProcId::Sample2,
        4 => SwizzleProcId::Sample4,
        6 => SwizzleProcId::Sample6,
        8 => SwizzleProcId::Sample8,
        _ => return None,
    };
    let (src_offset, src_width, dst_offset, dst_width_out) =
        compute_geometry(dst_width, subset, frame);
    Some(SwizzlerInit {
        fast_proc_id: SwizzleProcId::Copy as i32,
        slow_proc_id: proc as i32,
        src_offset,
        src_width,
        dst_offset,
        dst_width: dst_width_out,
        src_bpp,
        dst_bpp: color_type_bytes_per_pixel(dst_ct),
    })
}

/// Exact port of `SkSwizzler::Make` selection and initialization.
pub fn select_swizzler(
    encoded_color: EncodedColor,
    encoded_alpha: EncodedAlpha,
    encoded_bpc: u8,
    has_ctable: bool,
    dst_ct: ColorType,
    dst_at: AlphaType,
    dst_width: i32,
    zero_init: ZeroInitialized,
    subset: Option<IRect>,
    frame: Option<IRect>,
) -> Option<SwizzlerInit> {
    if encoded_color == EncodedColor::Palette && !has_ctable {
        return None;
    }

    let mut fast_proc: Option<SwizzleProcId> = None;
    let proc: SwizzleProcId;
    let premultiply =
        (encoded_alpha != EncodedAlpha::Opaque) && (dst_at == AlphaType::Premul);
    let skip_z = zero_init == ZeroInitialized::Yes;

    match encoded_color {
        EncodedColor::Gray => match encoded_bpc {
            1 => {
                proc = match dst_ct {
                    ColorType::Rgba8888 | ColorType::Bgra8888 => SwizzleProcId::BitToN32,
                    ColorType::Rgb565 => SwizzleProcId::BitTo565,
                    ColorType::Gray8 => SwizzleProcId::BitToGrayscale,
                    ColorType::RgbaF16 => SwizzleProcId::BitToF16,
                    _ => return None,
                };
            }
            8 => match dst_ct {
                ColorType::Rgba8888 | ColorType::Bgra8888 => {
                    proc = SwizzleProcId::GrayToN32;
                    fast_proc = Some(SwizzleProcId::FastGrayToN32);
                }
                ColorType::Gray8 => {
                    proc = SwizzleProcId::Sample1;
                    fast_proc = Some(SwizzleProcId::Copy);
                }
                ColorType::Rgb565 => {
                    proc = SwizzleProcId::GrayTo565;
                }
                _ => return None,
            },
            _ => return None,
        },
        EncodedColor::XAlpha | EncodedColor::GrayAlpha => match dst_ct {
            ColorType::Rgba8888 | ColorType::Bgra8888 => {
                if premultiply {
                    if skip_z {
                        proc = SwizzleProcId::GrayAlphaToN32PremulSkipZ;
                        fast_proc = Some(SwizzleProcId::FastGrayAlphaToN32PremulSkipZ);
                    } else {
                        proc = SwizzleProcId::GrayAlphaToN32Premul;
                        fast_proc = Some(SwizzleProcId::FastGrayAlphaToN32Premul);
                    }
                } else if skip_z {
                    proc = SwizzleProcId::GrayAlphaToN32UnpremulSkipZ;
                    fast_proc = Some(SwizzleProcId::FastGrayAlphaToN32UnpremulSkipZ);
                } else {
                    proc = SwizzleProcId::GrayAlphaToN32Unpremul;
                    fast_proc = Some(SwizzleProcId::FastGrayAlphaToN32Unpremul);
                }
            }
            ColorType::Alpha8 => {
                proc = SwizzleProcId::GrayAlphaToA8;
            }
            _ => return None,
        },
        EncodedColor::Palette => match encoded_bpc {
            1 | 2 | 4 => {
                proc = match dst_ct {
                    ColorType::Rgba8888 | ColorType::Bgra8888 | ColorType::Bgr101010xXr => {
                        SwizzleProcId::SmallIndexToN32
                    }
                    ColorType::Rgb565 => SwizzleProcId::SmallIndexTo565,
                    _ => return None,
                };
            }
            8 => {
                proc = match dst_ct {
                    ColorType::Rgba8888 | ColorType::Bgra8888 => {
                        if skip_z {
                            SwizzleProcId::IndexToN32SkipZ
                        } else {
                            SwizzleProcId::IndexToN32
                        }
                    }
                    ColorType::Bgr101010xXr => SwizzleProcId::IndexToN32,
                    ColorType::Rgb565 => SwizzleProcId::IndexTo565,
                    _ => return None,
                };
            }
            _ => return None,
        },
        EncodedColor::Rgb565 | EncodedColor::Rgb => match dst_ct {
            ColorType::Rgba8888 => {
                if encoded_bpc == 16 {
                    proc = SwizzleProcId::Rgb16ToRgba;
                } else {
                    proc = SwizzleProcId::RgbToRgba;
                    fast_proc = Some(SwizzleProcId::FastRgbToRgba);
                }
            }
            ColorType::Bgra8888 => {
                if encoded_bpc == 16 {
                    proc = SwizzleProcId::Rgb16ToBgra;
                } else {
                    proc = SwizzleProcId::RgbToBgra;
                    fast_proc = Some(SwizzleProcId::FastRgbToBgra);
                }
            }
            ColorType::Rgb565 => {
                if encoded_bpc == 16 {
                    proc = SwizzleProcId::Rgb16To565;
                } else {
                    proc = SwizzleProcId::RgbTo565;
                }
            }
            _ => return None,
        },
        EncodedColor::Rgba => match dst_ct {
            ColorType::Rgba8888 => {
                if encoded_bpc == 16 {
                    proc = if premultiply {
                        SwizzleProcId::Rgba16ToRgbaPremul
                    } else {
                        SwizzleProcId::Rgba16ToRgbaUnpremul
                    };
                } else if premultiply {
                    if skip_z {
                        proc = SwizzleProcId::RgbaToRgbaPremulSkipZ;
                        fast_proc = Some(SwizzleProcId::FastRgbaToRgbaPremulSkipZ);
                    } else {
                        proc = SwizzleProcId::RgbaToRgbaPremul;
                        fast_proc = Some(SwizzleProcId::FastRgbaToRgbaPremul);
                    }
                } else if skip_z {
                    proc = SwizzleProcId::Sample4SkipZ;
                    fast_proc = Some(SwizzleProcId::CopySkipZ);
                } else {
                    proc = SwizzleProcId::Sample4;
                    fast_proc = Some(SwizzleProcId::Copy);
                }
            }
            ColorType::Bgra8888 => {
                if encoded_bpc == 16 {
                    proc = if premultiply {
                        SwizzleProcId::Rgba16ToBgraPremul
                    } else {
                        SwizzleProcId::Rgba16ToBgraUnpremul
                    };
                } else if premultiply {
                    if skip_z {
                        proc = SwizzleProcId::RgbaToBgraPremulSkipZ;
                        fast_proc = Some(SwizzleProcId::FastRgbaToBgraPremulSkipZ);
                    } else {
                        proc = SwizzleProcId::RgbaToBgraPremul;
                        fast_proc = Some(SwizzleProcId::FastRgbaToBgraPremul);
                    }
                } else if skip_z {
                    proc = SwizzleProcId::RgbaToBgraUnpremulSkipZ;
                    fast_proc = Some(SwizzleProcId::FastRgbaToBgraUnpremulSkipZ);
                } else {
                    proc = SwizzleProcId::RgbaToBgraUnpremul;
                    fast_proc = Some(SwizzleProcId::FastRgbaToBgraUnpremul);
                }
            }
            _ => return None,
        },
        EncodedColor::Bgr => match dst_ct {
            ColorType::Bgra8888 => {
                proc = SwizzleProcId::RgbToRgba;
                fast_proc = Some(SwizzleProcId::FastRgbToRgba);
            }
            ColorType::Rgba8888 => {
                proc = SwizzleProcId::RgbToBgra;
                fast_proc = Some(SwizzleProcId::FastRgbToBgra);
            }
            ColorType::Rgb565 => {
                proc = SwizzleProcId::BgrTo565;
            }
            _ => return None,
        },
        EncodedColor::Bgrx => match dst_ct {
            ColorType::Bgra8888 => {
                proc = SwizzleProcId::RgbToRgba;
            }
            ColorType::Rgba8888 => {
                proc = SwizzleProcId::RgbToBgra;
            }
            ColorType::Rgb565 => {
                proc = SwizzleProcId::BgrTo565;
            }
            _ => return None,
        },
        EncodedColor::Bgra => match dst_ct {
            ColorType::Bgra8888 => {
                if premultiply {
                    if skip_z {
                        proc = SwizzleProcId::RgbaToRgbaPremulSkipZ;
                        fast_proc = Some(SwizzleProcId::FastRgbaToRgbaPremulSkipZ);
                    } else {
                        proc = SwizzleProcId::RgbaToRgbaPremul;
                        fast_proc = Some(SwizzleProcId::FastRgbaToRgbaPremul);
                    }
                } else if skip_z {
                    proc = SwizzleProcId::Sample4SkipZ;
                    fast_proc = Some(SwizzleProcId::CopySkipZ);
                } else {
                    proc = SwizzleProcId::Sample4;
                    fast_proc = Some(SwizzleProcId::Copy);
                }
            }
            ColorType::Rgba8888 => {
                if premultiply {
                    if skip_z {
                        proc = SwizzleProcId::RgbaToBgraPremulSkipZ;
                        fast_proc = Some(SwizzleProcId::FastRgbaToBgraPremulSkipZ);
                    } else {
                        proc = SwizzleProcId::RgbaToBgraPremul;
                        fast_proc = Some(SwizzleProcId::FastRgbaToBgraPremul);
                    }
                } else if skip_z {
                    proc = SwizzleProcId::RgbaToBgraUnpremulSkipZ;
                    fast_proc = Some(SwizzleProcId::FastRgbaToBgraUnpremulSkipZ);
                } else {
                    proc = SwizzleProcId::RgbaToBgraUnpremul;
                    fast_proc = Some(SwizzleProcId::FastRgbaToBgraUnpremul);
                }
            }
            _ => return None,
        },
        EncodedColor::InvertedCmyk => match dst_ct {
            ColorType::Rgba8888 => {
                proc = SwizzleProcId::CmykToRgba;
                fast_proc = Some(SwizzleProcId::FastCmykToRgba);
            }
            ColorType::Bgra8888 => {
                proc = SwizzleProcId::CmykToBgra;
                fast_proc = Some(SwizzleProcId::FastCmykToBgra);
            }
            ColorType::Rgb565 => {
                proc = SwizzleProcId::CmykTo565;
            }
            _ => return None,
        },
        _ => return None,
    }

    let bpp_bits = bits_per_pixel(encoded_color, encoded_bpc);
    let src_bpp = if (bpp_bits & 7) == 0 {
        (bpp_bits / 8) as i32
    } else {
        bpp_bits as i32
    };
    let dst_bpp = color_type_bytes_per_pixel(dst_ct);
    let (src_offset, src_width, dst_offset, dst_width_out) =
        compute_geometry(dst_width, subset, frame);

    Some(SwizzlerInit {
        fast_proc_id: fast_proc.map_or(-1, |p| p as i32),
        slow_proc_id: proc as i32,
        src_offset,
        src_width,
        dst_offset,
        dst_width: dst_width_out,
        src_bpp,
        dst_bpp,
    })
}

/// Exact port of `SkSwizzler::onSetSampleX`.
pub fn compute_sample_x(
    sample_x: i32,
    src_offset: i32,
    dst_offset: i32,
    src_width: i32,
    dst_width: i32,
    src_bpp: i32,
    dst_bpp: i32,
    has_fast_proc: bool,
) -> SwizzlerSampleXResult {
    let mut dst_offset_bytes = (dst_offset / sample_x) * dst_bpp;
    let swizzle_width = get_sampled_dimension(src_width, sample_x);
    let allocated_width = get_sampled_dimension(dst_width, sample_x);

    let frame_sample_x = if src_width < dst_width {
        src_width / swizzle_width
    } else {
        sample_x
    };
    let src_offset_units = (get_start_coord(frame_sample_x) + src_offset) * src_bpp;

    let mut safety_net_triggered = false;
    if dst_offset_bytes > 0 {
        let dst_swizzle_bytes = (swizzle_width as usize) * (dst_bpp as usize);
        let dst_allocated_bytes = (allocated_width as usize) * (dst_bpp as usize);
        if (dst_offset_bytes as usize) + dst_swizzle_bytes > dst_allocated_bytes {
            safety_net_triggered = true;
            dst_offset_bytes = (dst_allocated_bytes - dst_swizzle_bytes) as i32;
        }
    }

    SwizzlerSampleXResult {
        dst_offset_bytes,
        swizzle_width,
        allocated_width,
        src_offset_units,
        use_fast_proc: sample_x == 1 && has_fast_proc,
        safety_net_triggered,
    }
}

// ============================================================================
// Pixel packing helpers (exact bit-for-bit parity with SkColorData / SkCodecPriv)
// ============================================================================

#[inline]
const fn pack_rgba(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

#[inline]
const fn pack_bgra(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[inline]
const fn pack_gray_n32(a: u8, gray: u8) -> u32 {
    let g = gray as u32;
    ((a as u32) << 24) | (g << 16) | (g << 8) | g
}

#[inline]
const fn premul_rgba(a: u8, mut r: u8, mut g: u8, mut b: u8) -> u32 {
    if a != 255 {
        r = mul_div_255_round(r as u32, a as u32) as u8;
        g = mul_div_255_round(g as u32, a as u32) as u8;
        b = mul_div_255_round(b as u32, a as u32) as u8;
    }
    pack_rgba(a, r, g, b)
}

#[inline]
const fn premul_bgra(a: u8, mut r: u8, mut g: u8, mut b: u8) -> u32 {
    if a != 255 {
        r = mul_div_255_round(r as u32, a as u32) as u8;
        g = mul_div_255_round(g as u32, a as u32) as u8;
        b = mul_div_255_round(b as u32, a as u32) as u8;
    }
    pack_bgra(a, r, g, b)
}

#[inline]
const fn pack_888_to_rgb16(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3)
}

/// Exact port of `SkPixel32ToPixel16(SkPMColor c)`.
#[inline]
const fn pixel32_to_pixel16(c: u32, pmcolor_is_bgra: bool) -> u16 {
    let (r_shift, b_shift) = if pmcolor_is_bgra { (16, 0) } else { (0, 16) };
    let r = ((c >> (r_shift + 3)) & 0x1F) << 11;
    let g = ((c >> (8 + 2)) & 0x3F) << 5;
    let b = (c >> (b_shift + 3)) & 0x1F;
    (r | g | b) as u16
}

// ============================================================================
// Leading-zero skipping helpers
// ============================================================================

#[inline]
fn skip_leading_gray_alpha_zeros(
    src: &[u8],
    offset: usize,
    delta_src: usize,
    width: usize,
) -> (usize, usize, usize) {
    let mut skipped = 0usize;
    let mut pos = offset;
    while skipped < width && src[pos] == 0 && src[pos + 1] == 0 {
        skipped += 1;
        pos += delta_src;
    }
    (skipped * 4, pos, width - skipped)
}

#[inline]
fn skip_leading_8888_zeros(
    src: &[u8],
    offset: usize,
    delta_src: usize,
    width: usize,
) -> (usize, usize, usize) {
    let mut skipped = 0usize;
    let mut pos = offset;
    while skipped < width
        && src[pos] == 0
        && src[pos + 1] == 0
        && src[pos + 2] == 0
        && src[pos + 3] == 0
    {
        skipped += 1;
        pos += delta_src;
    }
    (skipped * 4, pos, width - skipped)
}

// ============================================================================
// Row swizzle execution
// ============================================================================

/// Executes any `SkSwizzler` row procedure completely in safe Rust.
#[allow(clippy::too_many_arguments)]
pub fn swizzle_row(
    proc: SwizzleProcId,
    dst: &mut [u8],
    src: &[u8],
    dst_width: usize,
    bpp: usize,
    delta_src: usize,
    offset: usize,
    ctable: &[u32],
    pmcolor_is_bgra: bool,
) {
    if dst_width == 0 {
        return;
    }

    match proc {
        SwizzleProcId::Copy => {
            let len = dst_width * bpp;
            dst[..len].copy_from_slice(&src[offset..offset + len]);
        }
        SwizzleProcId::Sample1 => {
            let mut pos = offset;
            for d in &mut dst[..dst_width] {
                *d = src[pos];
                pos += delta_src;
            }
        }
        SwizzleProcId::Sample2 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                out.copy_from_slice(&src[pos..pos + 2]);
                pos += delta_src;
            }
        }
        SwizzleProcId::Sample4 => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                out.copy_from_slice(&src[pos..pos + 4]);
                pos += delta_src;
            }
        }
        SwizzleProcId::Sample6 => {
            let mut pos = offset;
            for out in dst[..dst_width * 6].chunks_exact_mut(6) {
                out.copy_from_slice(&src[pos..pos + 6]);
                pos += delta_src;
            }
        }
        SwizzleProcId::Sample8 => {
            let mut pos = offset;
            for out in dst[..dst_width * 8].chunks_exact_mut(8) {
                out.copy_from_slice(&src[pos..pos + 8]);
                pos += delta_src;
            }
        }

        // kBit
        SwizzleProcId::BitToGrayscale => {
            let mut byte_idx = offset / 8;
            let mut bit_idx = offset % 8;
            let mut curr_byte = src[byte_idx];
            dst[0] = if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                0xFF
            } else {
                0x00
            };
            for d in &mut dst[1..dst_width] {
                let bit_offset = bit_idx + delta_src;
                bit_idx = bit_offset % 8;
                byte_idx += bit_offset / 8;
                curr_byte = src[byte_idx];
                *d = if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                    0xFF
                } else {
                    0x00
                };
            }
        }
        SwizzleProcId::BitToN32 => {
            let white = 0xFFFF_FFFFu32.to_ne_bytes();
            let black = 0xFF00_0000u32.to_ne_bytes();
            let mut byte_idx = offset / 8;
            let mut bit_idx = offset % 8;
            let mut curr_byte = src[byte_idx];
            dst[0..4].copy_from_slice(if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                &white
            } else {
                &black
            });
            for out in dst[4..dst_width * 4].chunks_exact_mut(4) {
                let bit_offset = bit_idx + delta_src;
                bit_idx = bit_offset % 8;
                byte_idx += bit_offset / 8;
                curr_byte = src[byte_idx];
                out.copy_from_slice(if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                    &white
                } else {
                    &black
                });
            }
        }
        SwizzleProcId::BitTo565 => {
            let white = 0xFFFFu16.to_ne_bytes();
            let black = 0x0000u16.to_ne_bytes();
            let mut byte_idx = offset / 8;
            let mut bit_idx = offset % 8;
            let mut curr_byte = src[byte_idx];
            dst[0..2].copy_from_slice(if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                &white
            } else {
                &black
            });
            for out in dst[2..dst_width * 2].chunks_exact_mut(2) {
                let bit_offset = bit_idx + delta_src;
                bit_idx = bit_offset % 8;
                byte_idx += bit_offset / 8;
                curr_byte = src[byte_idx];
                out.copy_from_slice(if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                    &white
                } else {
                    &black
                });
            }
        }
        SwizzleProcId::BitToF16 => {
            let half1 = SK_HALF_1 as u64;
            let white = (half1 | (half1 << 16) | (half1 << 32) | (half1 << 48)).to_ne_bytes();
            let black = (half1 << 48).to_ne_bytes();
            let mut byte_idx = offset / 8;
            let mut bit_idx = offset % 8;
            let mut curr_byte = src[byte_idx];
            dst[0..8].copy_from_slice(if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                &white
            } else {
                &black
            });
            for out in dst[8..dst_width * 8].chunks_exact_mut(8) {
                let bit_offset = bit_idx + delta_src;
                bit_idx = bit_offset % 8;
                byte_idx += bit_offset / 8;
                curr_byte = src[byte_idx];
                out.copy_from_slice(if ((curr_byte >> (7 - bit_idx)) & 1) != 0 {
                    &white
                } else {
                    &black
                });
            }
        }

        // kIndex1, kIndex2, kIndex4
        SwizzleProcId::SmallIndexTo565 => {
            let mask = ((1u32 << bpp) - 1) as u8;
            let mut byte_idx = offset / 8;
            let mut bit_idx = offset % 8;
            let mut curr_byte = src[byte_idx];
            let mut index = ((curr_byte >> (8 - bpp - bit_idx)) & mask) as usize;
            dst[0..2]
                .copy_from_slice(&pixel32_to_pixel16(ctable[index], pmcolor_is_bgra).to_ne_bytes());
            for out in dst[2..dst_width * 2].chunks_exact_mut(2) {
                let bit_offset = bit_idx + delta_src;
                bit_idx = bit_offset % 8;
                byte_idx += bit_offset / 8;
                curr_byte = src[byte_idx];
                index = ((curr_byte >> (8 - bpp - bit_idx)) & mask) as usize;
                out.copy_from_slice(
                    &pixel32_to_pixel16(ctable[index], pmcolor_is_bgra).to_ne_bytes(),
                );
            }
        }
        SwizzleProcId::SmallIndexToN32 => {
            let mask = ((1u32 << bpp) - 1) as u8;
            let mut byte_idx = offset / 8;
            let mut bit_idx = offset % 8;
            let mut curr_byte = src[byte_idx];
            let mut index = ((curr_byte >> (8 - bpp - bit_idx)) & mask) as usize;
            dst[0..4].copy_from_slice(&ctable[index].to_ne_bytes());
            for out in dst[4..dst_width * 4].chunks_exact_mut(4) {
                let bit_offset = bit_idx + delta_src;
                bit_idx = bit_offset % 8;
                byte_idx += bit_offset / 8;
                curr_byte = src[byte_idx];
                index = ((curr_byte >> (8 - bpp - bit_idx)) & mask) as usize;
                out.copy_from_slice(&ctable[index].to_ne_bytes());
            }
        }

        // kIndex8
        SwizzleProcId::IndexToN32 => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = ctable[src[pos] as usize];
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::IndexToN32SkipZ => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = ctable[src[pos] as usize];
                if c != 0 {
                    out.copy_from_slice(&c.to_ne_bytes());
                }
                pos += delta_src;
            }
        }
        SwizzleProcId::IndexTo565 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                let c = pixel32_to_pixel16(ctable[src[pos] as usize], pmcolor_is_bgra);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }

        // kGray
        SwizzleProcId::GrayToN32 => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_gray_n32(0xFF, src[pos]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastGrayToN32 => {
            for (out, &g) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width].iter())
            {
                let c = pack_gray_n32(0xFF, g);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::GrayTo565 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                let g = src[pos];
                let c = pack_888_to_rgb16(g, g, g);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }

        // kGrayAlpha
        SwizzleProcId::GrayAlphaToN32Unpremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_gray_n32(src[pos + 1], src[pos]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastGrayAlphaToN32Unpremul => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 2].chunks_exact(2))
            {
                let c = pack_gray_n32(chunk[1], chunk[0]);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::GrayAlphaToN32Premul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let a = src[pos + 1];
                let pm = mul_div_255_round(a as u32, src[pos] as u32) as u8;
                let c = pack_gray_n32(a, pm);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastGrayAlphaToN32Premul => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 2].chunks_exact(2))
            {
                let a = chunk[1];
                let pm = mul_div_255_round(a as u32, chunk[0] as u32) as u8;
                let c = pack_gray_n32(a, pm);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::GrayAlphaToN32UnpremulSkipZ => {
            let (dst_off, src_off, rem) =
                skip_leading_gray_alpha_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::GrayAlphaToN32Unpremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::FastGrayAlphaToN32UnpremulSkipZ => {
            let (dst_off, src_off, rem) =
                skip_leading_gray_alpha_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::FastGrayAlphaToN32Unpremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::GrayAlphaToN32PremulSkipZ => {
            let (dst_off, src_off, rem) =
                skip_leading_gray_alpha_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::GrayAlphaToN32Premul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::FastGrayAlphaToN32PremulSkipZ => {
            let (dst_off, src_off, rem) =
                skip_leading_gray_alpha_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::FastGrayAlphaToN32Premul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::GrayAlphaToA8 => {
            let mut pos = offset;
            for d in &mut dst[..dst_width] {
                *d = src[pos + 1];
                pos += delta_src;
            }
        }

        // kBGR / kRGB
        SwizzleProcId::BgrTo565 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                let c = pack_888_to_rgb16(src[pos + 2], src[pos + 1], src[pos]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::RgbToRgba => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_rgba(0xFF, src[pos], src[pos + 1], src[pos + 2]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::RgbToBgra => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_bgra(0xFF, src[pos], src[pos + 1], src[pos + 2]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastRgbToRgba => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 3].chunks_exact(3))
            {
                let c = pack_rgba(0xFF, chunk[0], chunk[1], chunk[2]);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::FastRgbToBgra => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 3].chunks_exact(3))
            {
                let c = pack_bgra(0xFF, chunk[0], chunk[1], chunk[2]);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::RgbTo565 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                let c = pack_888_to_rgb16(src[pos], src[pos + 1], src[pos + 2]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }

        // kRGBA / kBGRA (8-bit)
        SwizzleProcId::RgbaToRgbaPremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = premul_rgba(src[pos + 3], src[pos], src[pos + 1], src[pos + 2]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::RgbaToBgraPremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = premul_bgra(src[pos + 3], src[pos], src[pos + 1], src[pos + 2]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastRgbaToRgbaPremul => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 4].chunks_exact(4))
            {
                let c = premul_rgba(chunk[3], chunk[0], chunk[1], chunk[2]);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::FastRgbaToBgraPremul => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 4].chunks_exact(4))
            {
                let c = premul_bgra(chunk[3], chunk[0], chunk[1], chunk[2]);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::RgbaToBgraUnpremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_bgra(src[pos + 3], src[pos], src[pos + 1], src[pos + 2]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastRgbaToBgraUnpremul => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 4].chunks_exact(4))
            {
                let c = pack_bgra(chunk[3], chunk[0], chunk[1], chunk[2]);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }

        // SkipLeading8888ZerosThen variants
        SwizzleProcId::RgbaToRgbaPremulSkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::RgbaToRgbaPremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::FastRgbaToRgbaPremulSkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::FastRgbaToRgbaPremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::RgbaToBgraPremulSkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::RgbaToBgraPremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::FastRgbaToBgraPremulSkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::FastRgbaToBgraPremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::RgbaToBgraUnpremulSkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::RgbaToBgraUnpremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::FastRgbaToBgraUnpremulSkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::FastRgbaToBgraUnpremul,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::Sample4SkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::Sample4,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }
        SwizzleProcId::CopySkipZ => {
            let (dst_off, src_off, rem) = skip_leading_8888_zeros(src, offset, delta_src, dst_width);
            swizzle_row(
                SwizzleProcId::Copy,
                &mut dst[dst_off..],
                &src[src_off..],
                rem,
                bpp,
                delta_src,
                0,
                ctable,
                pmcolor_is_bgra,
            );
        }

        // 16-bit per component (big-endian high bytes at even indices)
        SwizzleProcId::Rgb16ToRgba => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_rgba(0xFF, src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::Rgb16ToBgra => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_bgra(0xFF, src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::Rgb16To565 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                let c = pack_888_to_rgb16(src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::Rgba16ToRgbaUnpremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_rgba(src[pos + 6], src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::Rgba16ToRgbaPremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = premul_rgba(src[pos + 6], src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::Rgba16ToBgraUnpremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = pack_bgra(src[pos + 6], src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::Rgba16ToBgraPremul => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let c = premul_bgra(src[pos + 6], src[pos], src[pos + 2], src[pos + 4]);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }

        // kInvertedCMYK
        SwizzleProcId::CmykToRgba => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let k = src[pos + 3] as u32;
                let r = mul_div_255_round(src[pos] as u32, k) as u8;
                let g = mul_div_255_round(src[pos + 1] as u32, k) as u8;
                let b = mul_div_255_round(src[pos + 2] as u32, k) as u8;
                let c = pack_rgba(0xFF, r, g, b);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::CmykToBgra => {
            let mut pos = offset;
            for out in dst[..dst_width * 4].chunks_exact_mut(4) {
                let k = src[pos + 3] as u32;
                let r = mul_div_255_round(src[pos] as u32, k) as u8;
                let g = mul_div_255_round(src[pos + 1] as u32, k) as u8;
                let b = mul_div_255_round(src[pos + 2] as u32, k) as u8;
                let c = pack_bgra(0xFF, r, g, b);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
        SwizzleProcId::FastCmykToRgba => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 4].chunks_exact(4))
            {
                let k = chunk[3] as u32;
                let r = mul_div_255_round(chunk[0] as u32, k) as u8;
                let g = mul_div_255_round(chunk[1] as u32, k) as u8;
                let b = mul_div_255_round(chunk[2] as u32, k) as u8;
                let c = pack_rgba(0xFF, r, g, b);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::FastCmykToBgra => {
            for (out, chunk) in dst[..dst_width * 4]
                .chunks_exact_mut(4)
                .zip(src[offset..offset + dst_width * 4].chunks_exact(4))
            {
                let k = chunk[3] as u32;
                let r = mul_div_255_round(chunk[0] as u32, k) as u8;
                let g = mul_div_255_round(chunk[1] as u32, k) as u8;
                let b = mul_div_255_round(chunk[2] as u32, k) as u8;
                let c = pack_bgra(0xFF, r, g, b);
                out.copy_from_slice(&c.to_ne_bytes());
            }
        }
        SwizzleProcId::CmykTo565 => {
            let mut pos = offset;
            for out in dst[..dst_width * 2].chunks_exact_mut(2) {
                let k = src[pos + 3] as u32;
                let r = mul_div_255_round(src[pos] as u32, k) as u8;
                let g = mul_div_255_round(src[pos + 1] as u32, k) as u8;
                let b = mul_div_255_round(src[pos + 2] as u32, k) as u8;
                let c = pack_888_to_rgb16(r, g, b);
                out.copy_from_slice(&c.to_ne_bytes());
                pos += delta_src;
            }
        }
    }
}
