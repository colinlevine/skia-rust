//! Fixed-point integer square root matching `SkMathPriv.cpp` (`SkSqrtBits`).

pub fn sqrt_bits(x: i32, mut count: i32) -> i32 {
    debug_assert!(x >= 0 && count > 0 && (count as u32) <= 30);

    let mut root: u32 = 0;
    let mut rem_hi: u32 = 0;
    let mut rem_lo: u32 = x as u32;

    loop {
        root <<= 1;
        rem_hi = (rem_hi << 2) | (rem_lo >> 30);
        rem_lo <<= 2;

        let test_div = (root << 1) + 1;
        if rem_hi >= test_div {
            rem_hi -= test_div;
            root += 1;
        }
        count -= 1;
        if count < 0 {
            break;
        }
    }

    root as i32
}
