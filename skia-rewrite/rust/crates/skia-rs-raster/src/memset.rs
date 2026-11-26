//! Safe Rust port of `src/core/SkMemset_opts.cpp` and `src/opts/SkMemset_opts.h`.

/// Fill a slice of `u16` values with `value` (`SkOpts::memset16`).
#[inline]
pub fn memset16(dst: &mut [u16], value: u16) {
    dst.fill(value);
}

/// Fill a slice of `u32` values with `value` (`SkOpts::memset32`).
#[inline]
pub fn memset32(dst: &mut [u32], value: u32) {
    dst.fill(value);
}

/// Fill a slice of `u64` values with `value` (`SkOpts::memset64`).
#[inline]
pub fn memset64(dst: &mut [u64], value: u64) {
    dst.fill(value);
}

/// Fill a 2D rectangle of `u16` values in a byte buffer (`SkOpts::rect_memset16`).
pub fn rect_memset16_bytes(
    dst: &mut [u8],
    value: u16,
    count: usize,
    row_bytes: usize,
    height: usize,
) {
    let bytes = value.to_ne_bytes();
    for y in 0..height {
        let row_start = y * row_bytes;
        let row_end = row_start + count * 2;
        if row_end <= dst.len() {
            for chunk in dst[row_start..row_end].chunks_exact_mut(2) {
                chunk[0] = bytes[0];
                chunk[1] = bytes[1];
            }
        }
    }
}

/// Fill a 2D rectangle of `u32` values in a byte buffer (`SkOpts::rect_memset32`).
pub fn rect_memset32_bytes(
    dst: &mut [u8],
    value: u32,
    count: usize,
    row_bytes: usize,
    height: usize,
) {
    let bytes = value.to_ne_bytes();
    for y in 0..height {
        let row_start = y * row_bytes;
        let row_end = row_start + count * 4;
        if row_end <= dst.len() {
            for chunk in dst[row_start..row_end].chunks_exact_mut(4) {
                chunk[0] = bytes[0];
                chunk[1] = bytes[1];
                chunk[2] = bytes[2];
                chunk[3] = bytes[3];
            }
        }
    }
}

/// Fill a 2D rectangle of `u64` values in a byte buffer (`SkOpts::rect_memset64`).
pub fn rect_memset64_bytes(
    dst: &mut [u8],
    value: u64,
    count: usize,
    row_bytes: usize,
    height: usize,
) {
    let bytes = value.to_ne_bytes();
    for y in 0..height {
        let row_start = y * row_bytes;
        let row_end = row_start + count * 8;
        if row_end <= dst.len() {
            for chunk in dst[row_start..row_end].chunks_exact_mut(8) {
                chunk.copy_from_slice(&bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memset_basic_and_unaligned_counts() {
        for len in 0..=67 {
            let mut b16 = vec![0u16; len + 4];
            memset16(&mut b16[2..2 + len], 0x1234);
            assert_eq!(&b16[..2], &[0, 0]);
            assert!(b16[2..2 + len].iter().all(|&v| v == 0x1234));
            assert_eq!(&b16[2 + len..], &[0, 0]);

            let mut b32 = vec![0u32; len + 4];
            memset32(&mut b32[2..2 + len], 0x1234_5678);
            assert_eq!(&b32[..2], &[0, 0]);
            assert!(b32[2..2 + len].iter().all(|&v| v == 0x1234_5678));
            assert_eq!(&b32[2 + len..], &[0, 0]);

            let mut b64 = vec![0u64; len + 4];
            memset64(&mut b64[2..2 + len], 0x0123_4567_89AB_CDEF);
            assert_eq!(&b64[..2], &[0, 0]);
            assert!(b64[2..2 + len].iter().all(|&v| v == 0x0123_4567_89AB_CDEF));
            assert_eq!(&b64[2 + len..], &[0, 0]);
        }
    }

    #[test]
    fn rect_memset_respects_row_bytes() {
        let mut buf = vec![0u8; 3 * 20];
        rect_memset32_bytes(&mut buf, 0xAABB_CCDD, 3, 20, 3);
        let expected = 0xAABB_CCDDu32.to_ne_bytes();
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(&buf[y * 20 + x * 4..y * 20 + (x + 1) * 4], &expected);
            }
            assert_eq!(&buf[y * 20 + 12..y * 20 + 20], &[0u8; 8]);
        }
    }
}
