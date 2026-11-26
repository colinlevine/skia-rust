//! RFC 1321 MD5 implementation matching Skia's `SkMD5`.

pub struct Md5State {
    pub byte_count: u64,
    pub state: [u32; 4],
    pub buffer: [u8; 64],
}

impl Md5State {
    pub const fn new() -> Self {
        Self {
            byte_count: 0,
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            buffer: [0u8; 64],
        }
    }

    pub fn update(&mut self, input: &[u8]) {
        let mut buffer_index = (self.byte_count & 0x3F) as usize;
        let buffer_available = 64 - buffer_index;

        let mut input_index = 0usize;
        if input.len() >= buffer_available {
            if buffer_index != 0 {
                self.buffer[buffer_index..64].copy_from_slice(&input[..buffer_available]);
                transform(&mut self.state, &self.buffer);
                input_index = buffer_available;
            }
            while input_index + 64 <= input.len() {
                let block: &[u8; 64] = input[input_index..input_index + 64]
                    .try_into()
                    .expect("slice length is 64");
                transform(&mut self.state, block);
                input_index += 64;
            }
            buffer_index = 0;
        }

        let remaining = &input[input_index..];
        self.buffer[buffer_index..buffer_index + remaining.len()].copy_from_slice(remaining);
        self.byte_count = self.byte_count.wrapping_add(input.len() as u64);
    }

    pub fn finish(&mut self) -> [u8; 16] {
        let bits = self.byte_count.wrapping_shl(3).to_le_bytes();
        let buffer_index = (self.byte_count & 0x3F) as usize;
        let padding_len = if buffer_index < 56 {
            56 - buffer_index
        } else {
            120 - buffer_index
        };

        let mut padding = [0u8; 64];
        padding[0] = 0x80;
        self.update(&padding[..padding_len]);
        self.update(&bits);

        let mut digest = [0u8; 16];
        for (i, word) in self.state.iter().enumerate() {
            digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        digest
    }
}

impl Default for Md5State {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn f(x: u32, y: u32, z: u32) -> u32 {
    ((y ^ z) & x) ^ z
}

#[inline]
fn g(x: u32, y: u32, z: u32) -> u32 {
    (x & z) | (y & !z)
}

#[inline]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline]
fn i_fn(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

#[inline]
fn op<Func: Fn(u32, u32, u32) -> u32>(
    func: Func,
    a: &mut u32,
    b: u32,
    c: u32,
    d: u32,
    x: u32,
    s: u32,
    t: u32,
) {
    *a = b.wrapping_add(
        a.wrapping_add(func(b, c, d))
            .wrapping_add(x)
            .wrapping_add(t)
            .rotate_left(s),
    );
}

fn transform(state: &mut [u32; 4], block: &[u8; 64]) {
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];

    let mut x = [0u32; 16];
    for (idx, chunk) in block.chunks_exact(4).enumerate() {
        x[idx] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    // Round 1
    op(f, &mut a, b, c, d, x[0], 7, 0xd76aa478);
    op(f, &mut d, a, b, c, x[1], 12, 0xe8c7b756);
    op(f, &mut c, d, a, b, x[2], 17, 0x242070db);
    op(f, &mut b, c, d, a, x[3], 22, 0xc1bdceee);
    op(f, &mut a, b, c, d, x[4], 7, 0xf57c0faf);
    op(f, &mut d, a, b, c, x[5], 12, 0x4787c62a);
    op(f, &mut c, d, a, b, x[6], 17, 0xa8304613);
    op(f, &mut b, c, d, a, x[7], 22, 0xfd469501);
    op(f, &mut a, b, c, d, x[8], 7, 0x698098d8);
    op(f, &mut d, a, b, c, x[9], 12, 0x8b44f7af);
    op(f, &mut c, d, a, b, x[10], 17, 0xffff5bb1);
    op(f, &mut b, c, d, a, x[11], 22, 0x895cd7be);
    op(f, &mut a, b, c, d, x[12], 7, 0x6b901122);
    op(f, &mut d, a, b, c, x[13], 12, 0xfd987193);
    op(f, &mut c, d, a, b, x[14], 17, 0xa679438e);
    op(f, &mut b, c, d, a, x[15], 22, 0x49b40821);

    // Round 2
    op(g, &mut a, b, c, d, x[1], 5, 0xf61e2562);
    op(g, &mut d, a, b, c, x[6], 9, 0xc040b340);
    op(g, &mut c, d, a, b, x[11], 14, 0x265e5a51);
    op(g, &mut b, c, d, a, x[0], 20, 0xe9b6c7aa);
    op(g, &mut a, b, c, d, x[5], 5, 0xd62f105d);
    op(g, &mut d, a, b, c, x[10], 9, 0x02441453);
    op(g, &mut c, d, a, b, x[15], 14, 0xd8a1e681);
    op(g, &mut b, c, d, a, x[4], 20, 0xe7d3fbc8);
    op(g, &mut a, b, c, d, x[9], 5, 0x21e1cde6);
    op(g, &mut d, a, b, c, x[14], 9, 0xc33707d6);
    op(g, &mut c, d, a, b, x[3], 14, 0xf4d50d87);
    op(g, &mut b, c, d, a, x[8], 20, 0x455a14ed);
    op(g, &mut a, b, c, d, x[13], 5, 0xa9e3e905);
    op(g, &mut d, a, b, c, x[2], 9, 0xfcefa3f8);
    op(g, &mut c, d, a, b, x[7], 14, 0x676f02d9);
    op(g, &mut b, c, d, a, x[12], 20, 0x8d2a4c8a);

    // Round 3
    op(h, &mut a, b, c, d, x[5], 4, 0xfffa3942);
    op(h, &mut d, a, b, c, x[8], 11, 0x8771f681);
    op(h, &mut c, d, a, b, x[11], 16, 0x6d9d6122);
    op(h, &mut b, c, d, a, x[14], 23, 0xfde5380c);
    op(h, &mut a, b, c, d, x[1], 4, 0xa4beea44);
    op(h, &mut d, a, b, c, x[4], 11, 0x4bdecfa9);
    op(h, &mut c, d, a, b, x[7], 16, 0xf6bb4b60);
    op(h, &mut b, c, d, a, x[10], 23, 0xbebfbc70);
    op(h, &mut a, b, c, d, x[13], 4, 0x289b7ec6);
    op(h, &mut d, a, b, c, x[0], 11, 0xeaa127fa);
    op(h, &mut c, d, a, b, x[3], 16, 0xd4ef3085);
    op(h, &mut b, c, d, a, x[6], 23, 0x04881d05);
    op(h, &mut a, b, c, d, x[9], 4, 0xd9d4d039);
    op(h, &mut d, a, b, c, x[12], 11, 0xe6db99e5);
    op(h, &mut c, d, a, b, x[15], 16, 0x1fa27cf8);
    op(h, &mut b, c, d, a, x[2], 23, 0xc4ac5665);

    // Round 4
    op(i_fn, &mut a, b, c, d, x[0], 6, 0xf4292244);
    op(i_fn, &mut d, a, b, c, x[7], 10, 0x432aff97);
    op(i_fn, &mut c, d, a, b, x[14], 15, 0xab9423a7);
    op(i_fn, &mut b, c, d, a, x[5], 21, 0xfc93a039);
    op(i_fn, &mut a, b, c, d, x[12], 6, 0x655b59c3);
    op(i_fn, &mut d, a, b, c, x[3], 10, 0x8f0ccc92);
    op(i_fn, &mut c, d, a, b, x[10], 15, 0xffeff47d);
    op(i_fn, &mut b, c, d, a, x[1], 21, 0x85845dd1);
    op(i_fn, &mut a, b, c, d, x[8], 6, 0x6fa87e4f);
    op(i_fn, &mut d, a, b, c, x[15], 10, 0xfe2ce6e0);
    op(i_fn, &mut c, d, a, b, x[6], 15, 0xa3014314);
    op(i_fn, &mut b, c, d, a, x[13], 21, 0x4e0811a1);
    op(i_fn, &mut a, b, c, d, x[4], 6, 0xf7537e82);
    op(i_fn, &mut d, a, b, c, x[11], 10, 0xbd3af235);
    op(i_fn, &mut c, d, a, b, x[2], 15, 0x2ad7d2bb);
    op(i_fn, &mut b, c, d, a, x[9], 21, 0xeb86d391);

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[cfg(test)]
mod tests {
    use super::Md5State;

    fn md5_hex(data: &[u8]) -> String {
        let mut s = Md5State::new();
        s.update(data);
        let d = s.finish();
        d.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn rfc1321_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        assert_eq!(
            md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }
}
