//! IEEE 754 binary16 (half float) conversions matching Skia's `SkHalf` / `SkVx.h`.

pub const SK_HALF_NAN: u16 = 0x7E00;

pub fn float_to_half(f: f32) -> u16 {
    if f.is_nan() {
        return SK_HALF_NAN;
    }
    let sem = f.to_bits() as i32;
    let s = sem & (0x8000_0000u32 as i32);
    let em = (sem ^ s).min(0x4780_0000);
    let em_f = f32::from_bits(em as u32);
    let magic = ((em_f * 8192.0f32).max(0.5f32).to_bits() as i32) & (255 << 23);
    let rounded = (em_f + f32::from_bits(magic as u32)).to_bits() as i32;
    let exp = (magic >> 13).wrapping_sub((127 - 15 + 13 + 1) << 10);
    let f16 = rounded.wrapping_add(exp);
    (((s >> 16) | f16) as u32) as u16
}

pub fn half_to_float(h: u16) -> f32 {
    let wide = h as i32;
    let s = wide & 0x8000;
    let em = wide ^ s;
    let inf_or_nan = if em >= (31 << 10) { 255 << 23 } else { 0 };
    let is_norm = em > 0x3ff;
    let sub = ((em as f32) * (1.0f32 / ((1u32 << 24) as f32))).to_bits() as i32;
    let norm = (em << 13).wrapping_add((127 - 15) << 23);
    let finite = if is_norm { norm } else { sub };
    f32::from_bits(((s << 16) | finite | inf_or_nan) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_finite_halves() {
        for h in 0u16..=u16::MAX {
            let f = half_to_float(h);
            if f.is_nan() {
                assert_eq!(float_to_half(f), SK_HALF_NAN);
            } else {
                assert_eq!(float_to_half(f), h, "failed for half 0x{h:04x} ({f})");
            }
        }
    }
}
