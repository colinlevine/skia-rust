//! Base64 encode/decode matching Skia's `SkBase64`.

const DECODE_PAD: i8 = -2;
const ENCODE_PAD: usize = 64;

pub const DEFAULT_ENCODE: &[u8; 65] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";

#[rustfmt::skip]
const DECODE_DATA: [i8; 80] = [
    62, -1, -1, -1, 63,
    52, 53, 54, 55, 56, 57, 58, 59, 60, 61, -1, -1, -1, DECODE_PAD, -1, -1,
    -1,  0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14,
    15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, -1, -1, -1, -1, -1,
    -1, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
    41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Base64Error {
    NoError = 0,
    PadError = 1,
    BadCharError = 2,
}

#[inline]
pub const fn encoded_size(src_data_length: usize) -> usize {
    ((src_data_length + 2) / 3) * 4
}

pub fn decode(src: &[u8], mut dst: Option<&mut [u8]>) -> (Base64Error, usize) {
    let mut i: usize = 0;
    let mut pad_two = false;
    let mut pad_three = false;
    let mut pos: usize = 0;

    'outer: while pos < src.len() {
        let mut bytes = [0u8; 4];
        let mut byte: usize = 0;

        loop {
            if pos >= src.len() {
                if byte == 0 {
                    break 'outer;
                }
                if byte < 2 {
                    return (Base64Error::PadError, 0);
                }
                pad_three = true;
                if byte == 2 {
                    pad_two = true;
                }
                break;
            }
            let src_byte = src[pos];
            pos += 1;
            if src_byte == 0 {
                break 'outer;
            }
            if src_byte <= b' ' {
                continue;
            }
            if src_byte < b'+' || src_byte > b'z' {
                return (Base64Error::BadCharError, 0);
            }
            let decoded = DECODE_DATA[(src_byte - b'+') as usize];
            bytes[byte] = decoded as u8;
            if decoded < 0 {
                if decoded == DECODE_PAD {
                    if byte < 2 {
                        return (Base64Error::PadError, 0);
                    }
                    pad_three = true;
                    if byte == 2 {
                        pad_two = true;
                    }
                    break;
                }
                return (Base64Error::BadCharError, 0);
            } else {
                byte += 1;
            }
            if pos < src.len() && src[pos] != 0 {
                if byte < 4 {
                    continue;
                }
                break;
            }
            if byte == 0 {
                break 'outer;
            }
            if byte == 4 {
                break;
            }
            if byte < 2 {
                return (Base64Error::PadError, 0);
            }
            pad_three = true;
            if byte == 2 {
                pad_two = true;
            }
            break;
        }

        let mut two: u8 = 0;
        let mut three: u8 = 0;
        if let Some(ref mut out) = dst {
            let one = bytes[0].wrapping_shl(2) | (bytes[1] >> 4);
            two = bytes[1].wrapping_shl(4) | (bytes[2] >> 2);
            three = bytes[2].wrapping_shl(6) | bytes[3];
            out[i] = one;
        }
        i += 1;
        if pad_two {
            break;
        }
        if let Some(ref mut out) = dst {
            out[i] = two;
        }
        i += 1;
        if pad_three {
            break;
        }
        if let Some(ref mut out) = dst {
            out[i] = three;
        }
        i += 1;
    }

    (Base64Error::NoError, i)
}

pub fn encode(src: &[u8], dst: Option<&mut [u8]>, encode_map: Option<&[u8; 65]>) -> usize {
    let table = encode_map.unwrap_or(DEFAULT_ENCODE);
    if let Some(out) = dst {
        let remainder = src.len() % 3;
        let full = src.len() - remainder;
        let mut di = 0usize;
        let mut si = 0usize;
        while si < full {
            let a = src[si] as usize;
            let b = src[si + 1] as usize;
            let c = src[si + 2] as usize;
            si += 3;
            let d = c & 0x3F;
            let c_idx = ((c >> 6) | (b << 2)) & 0x3F;
            let b_idx = ((b >> 4) | (a << 4)) & 0x3F;
            let a_idx = a >> 2;
            out[di] = table[a_idx];
            out[di + 1] = table[b_idx];
            out[di + 2] = table[c_idx];
            out[di + 3] = table[d];
            di += 4;
        }
        if remainder > 0 {
            let mut k1 = 0usize;
            let mut k2 = ENCODE_PAD;
            let a = src[si] as usize;
            if remainder == 2 {
                let b = src[si + 1] as usize;
                k1 = b >> 4;
                k2 = (b << 2) & 0x3F;
            }
            out[di] = table[a >> 2];
            out[di + 1] = table[(k1 | (a << 4)) & 0x3F];
            out[di + 2] = table[k2];
            out[di + 3] = table[ENCODE_PAD];
        }
    }
    encoded_size(src.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let input = b"Hello, Skia in safe Rust!";
        let mut enc = vec![0u8; encoded_size(input.len())];
        let n = encode(input, Some(&mut enc), None);
        assert_eq!(n, enc.len());
        let (err, dec_len) = decode(&enc, None);
        assert_eq!(err, Base64Error::NoError);
        assert_eq!(dec_len, input.len());
        let mut dec = vec![0u8; dec_len];
        let (err2, dec_len2) = decode(&enc, Some(&mut dec));
        assert_eq!(err2, Base64Error::NoError);
        assert_eq!(dec_len2, input.len());
        assert_eq!(&dec, input);
    }
}
