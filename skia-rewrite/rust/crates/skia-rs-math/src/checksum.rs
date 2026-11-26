//! wyhash implementation matching Skia's `SkChecksum`.

const WYP: [u64; 4] = [
    0xa0761d6478bd642f,
    0xe7037ed1a0b428db,
    0x8ebc6af09c88c6e3,
    0x589965cc75374cc3,
];

#[inline]
fn wymum(a: u64, b: u64) -> (u64, u64) {
    let r = (a as u128) * (b as u128);
    (r as u64, (r >> 64) as u64)
}

#[inline]
fn wymix(a: u64, b: u64) -> u64 {
    let (lo, hi) = wymum(a, b);
    lo ^ hi
}

#[inline]
fn wyr8(p: &[u8]) -> u64 {
    u64::from_ne_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}

#[inline]
fn wyr4(p: &[u8]) -> u64 {
    u32::from_ne_bytes([p[0], p[1], p[2], p[3]]) as u64
}

#[inline]
fn wyr3(p: &[u8], k: usize) -> u64 {
    ((p[0] as u64) << 16) | ((p[k >> 1] as u64) << 8) | (p[k - 1] as u64)
}

pub fn hash64(data: &[u8], mut seed: u64) -> u64 {
    let len = data.len();
    seed ^= wymix(seed ^ WYP[0], WYP[1]);
    let (mut a, mut b): (u64, u64);

    if len <= 16 {
        if len >= 4 {
            let off = (len >> 3) << 2;
            a = (wyr4(&data[0..4]) << 32) | wyr4(&data[off..off + 4]);
            let end4 = len - 4;
            let end_off = end4 - off;
            b = (wyr4(&data[end4..end4 + 4]) << 32) | wyr4(&data[end_off..end_off + 4]);
        } else if len > 0 {
            a = wyr3(data, len);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut p = data;
        let mut i = len;
        if i > 48 {
            let mut see1 = seed;
            let mut see2 = seed;
            while i > 48 {
                seed = wymix(wyr8(&p[0..8]) ^ WYP[1], wyr8(&p[8..16]) ^ seed);
                see1 = wymix(wyr8(&p[16..24]) ^ WYP[2], wyr8(&p[24..32]) ^ see1);
                see2 = wymix(wyr8(&p[32..40]) ^ WYP[3], wyr8(&p[40..48]) ^ see2);
                p = &p[48..];
                i -= 48;
            }
            seed ^= see1 ^ see2;
        }
        while i > 16 {
            seed = wymix(wyr8(&p[0..8]) ^ WYP[1], wyr8(&p[8..16]) ^ seed);
            i -= 16;
            p = &p[16..];
        }
        // In the original C++, p + i - 16 may point slightly before p (into previous 16 bytes),
        // which is always valid within `data` because `len > 16`.
        let tail_start = len - 16;
        a = wyr8(&data[tail_start..tail_start + 8]);
        b = wyr8(&data[tail_start + 8..tail_start + 16]);
    }

    a ^= WYP[1];
    b ^= seed;
    let (lo, hi) = wymum(a, b);
    wymix(lo ^ WYP[0] ^ (len as u64), hi ^ WYP[1])
}

#[inline]
pub fn hash32(data: &[u8], seed: u32) -> u32 {
    hash64(data, seed as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_long() {
        let h0 = hash64(&[], 0);
        assert_ne!(h0, 0);
        let buf = [42u8; 100];
        assert_eq!(hash32(&buf, 7), hash64(&buf, 7) as u32);
    }
}
