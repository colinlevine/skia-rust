//! Safe Rust port of `src/codec/SkTiffUtility.cpp` and `src/codec/SkTiffUtility.h`.

pub const ENDIAN_SIZE: usize = 4;
pub const ENDIAN_BIG: [u8; ENDIAN_SIZE] = [b'M', b'M', 0, 42];
pub const ENDIAN_LITTLE: [u8; ENDIAN_SIZE] = [b'I', b'I', 42, 0];

pub const TYPE_UNSIGNED_BYTE: u16 = 1;
pub const TYPE_ASCII_STRING: u16 = 2;
pub const TYPE_UNSIGNED_SHORT: u16 = 3;
pub const TYPE_UNSIGNED_LONG: u16 = 4;
pub const TYPE_UNSIGNED_RATIONAL: u16 = 5;
pub const TYPE_SIGNED_BYTE: u16 = 6;
pub const TYPE_UNDEFINED: u16 = 7;
pub const TYPE_SIGNED_SHORT: u16 = 8;
pub const TYPE_SIGNED_LONG: u16 = 9;
pub const TYPE_SIGNED_RATIONAL: u16 = 10;
pub const TYPE_SINGLE_FLOAT: u16 = 11;
pub const TYPE_DOUBLE_FLOAT: u16 = 12;

pub const SIZE_ENTRY: usize = 12;
pub const SIZE_SHORT: usize = 2;
pub const SIZE_LONG: usize = 4;

#[inline]
pub const fn is_valid_type(ty: u16) -> bool {
    ty >= 1 && ty <= 12
}

#[inline]
pub const fn bytes_for_type(ty: u16) -> usize {
    match ty {
        TYPE_UNSIGNED_BYTE | TYPE_ASCII_STRING | TYPE_SIGNED_BYTE | TYPE_UNDEFINED => 1,
        TYPE_UNSIGNED_SHORT | TYPE_SIGNED_SHORT => SIZE_SHORT,
        TYPE_UNSIGNED_LONG | TYPE_SIGNED_LONG | TYPE_SINGLE_FLOAT => SIZE_LONG,
        TYPE_UNSIGNED_RATIONAL | TYPE_SIGNED_RATIONAL | TYPE_DOUBLE_FLOAT => 8,
        _ => 0,
    }
}

#[inline]
pub fn is_valid_endian_marker(data: &[u8]) -> Option<bool> {
    if data.len() < 2 {
        return None;
    }
    if data[0] == b'I' && data[1] == b'I' {
        Some(true)
    } else if data[0] == b'M' && data[1] == b'M' {
        Some(false)
    } else {
        None
    }
}

#[inline]
pub fn get_endian_u16(data: &[u8], little_endian: bool) -> u16 {
    let bytes = [data[0], data[1]];
    if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    }
}

#[inline]
pub fn get_endian_u32(data: &[u8], little_endian: bool) -> u32 {
    let bytes = [data[0], data[1], data[2], data[3]];
    if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    }
}

/// Raw entry metadata and byte span offset into the underlying TIFF buffer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EntryRawData {
    pub tag: u16,
    pub ty: u16,
    pub count: u32,
    pub data_offset: usize,
    pub data_size: usize,
}

/// Parse TIFF header to read endianness and first IFD offset.
/// Matches `SkTiff::ImageFileDirectory::ParseHeader`.
pub fn parse_header(data: &[u8]) -> Option<(bool, u32)> {
    if data.len() < 8 {
        return None;
    }
    let little_endian = is_valid_endian_marker(data)?;
    let ifd_offset = get_endian_u32(&data[4..8], little_endian);
    Some((little_endian, ifd_offset))
}

/// Validate an IFD at `ifd_offset` and return `(num_entries, next_ifd_offset)`.
/// Matches `validate_ifd` in `SkTiffUtility.cpp`.
pub fn validate_ifd(
    data: &[u8],
    little_endian: bool,
    ifd_offset: u32,
    allow_truncated: bool,
) -> Option<(u16, u32)> {
    let start = ifd_offset as usize;
    if data.len() < start {
        return None;
    }
    let rem = &data[start..];
    if rem.len() < SIZE_SHORT {
        return None;
    }
    let num_entries = get_endian_u16(&rem[..SIZE_SHORT], little_endian);
    let rem = &rem[SIZE_SHORT..];

    let entries_bytes = SIZE_ENTRY.checked_mul(num_entries as usize)?;
    if rem.len() < entries_bytes {
        if allow_truncated {
            return Some(((rem.len() / SIZE_ENTRY) as u16, 0));
        }
        return None;
    }

    let rem = &rem[entries_bytes..];
    if rem.len() < SIZE_LONG {
        if allow_truncated {
            return Some((num_entries, 0));
        }
        return None;
    }

    let next_ifd_offset = get_endian_u32(&rem[..SIZE_LONG], little_endian);
    Some((num_entries, next_ifd_offset))
}

/// Parsed TIFF Image File Directory borrowing the backing slice.
#[derive(Copy, Clone, Debug)]
pub struct ImageFileDirectory<'a> {
    data: &'a [u8],
    little_endian: bool,
    offset: u32,
    num_entries: u16,
    next_ifd_offset: u32,
}

impl<'a> ImageFileDirectory<'a> {
    pub fn from_offset(
        data: &'a [u8],
        little_endian: bool,
        ifd_offset: u32,
        allow_truncated: bool,
    ) -> Option<Self> {
        let (num_entries, next_ifd_offset) =
            validate_ifd(data, little_endian, ifd_offset, allow_truncated)?;
        Some(Self {
            data,
            little_endian,
            offset: ifd_offset,
            num_entries,
            next_ifd_offset,
        })
    }

    /// Construct from already-validated fields (used by FFI bridge).
    pub fn from_parts(
        data: &'a [u8],
        little_endian: bool,
        offset: u32,
        num_entries: u16,
        next_ifd_offset: u32,
    ) -> Self {
        Self {
            data,
            little_endian,
            offset,
            num_entries,
            next_ifd_offset,
        }
    }

    #[inline]
    pub fn num_entries(&self) -> u16 {
        self.num_entries
    }

    #[inline]
    pub fn next_ifd_offset(&self) -> u32 {
        self.next_ifd_offset
    }

    #[inline]
    fn entry_offset(&self, entry_index: u16) -> Option<usize> {
        let pos = (self.offset as usize)
            .checked_add(SIZE_SHORT)?
            .checked_add(SIZE_ENTRY.checked_mul(entry_index as usize)?)?;
        if pos.checked_add(SIZE_ENTRY)? <= self.data.len() {
            Some(pos)
        } else {
            None
        }
    }

    pub fn entry_tag(&self, entry_index: u16) -> u16 {
        let Some(pos) = self.entry_offset(entry_index) else {
            return 0;
        };
        get_endian_u16(&self.data[pos..pos + SIZE_SHORT], self.little_endian)
    }

    pub fn entry_raw_data(&self, entry_index: u16) -> Option<EntryRawData> {
        let pos = self.entry_offset(entry_index)?;
        let tag = get_endian_u16(&self.data[pos..pos + 2], self.little_endian);
        let ty = get_endian_u16(&self.data[pos + 2..pos + 4], self.little_endian);
        if !is_valid_type(ty) {
            return None;
        }
        let count = get_endian_u32(&self.data[pos + 4..pos + 8], self.little_endian);
        let entry_data_bytes = bytes_for_type(ty).checked_mul(count as usize)?;
        let data_offset = if entry_data_bytes <= SIZE_LONG {
            pos + 8
        } else {
            let offset = get_endian_u32(&self.data[pos + 8..pos + 12], self.little_endian) as usize;
            if self.data.len() < offset || self.data.len() - offset < entry_data_bytes {
                return None;
            }
            offset
        };
        Some(EntryRawData {
            tag,
            ty,
            count,
            data_offset,
            data_size: entry_data_bytes,
        })
    }

    pub fn entry_undefined_data(&self, entry_index: u16) -> Option<&'a [u8]> {
        let raw = self.entry_raw_data(entry_index)?;
        if raw.ty != TYPE_UNDEFINED {
            return None;
        }
        self.data.get(raw.data_offset..raw.data_offset + raw.data_size)
    }

    pub fn entry_unsigned_short(&self, entry_index: u16, out: &mut [u16]) -> bool {
        let Some(raw) = self.entry_raw_data(entry_index) else {
            return false;
        };
        if raw.ty != TYPE_UNSIGNED_SHORT || raw.count as usize != out.len() {
            return false;
        }
        for (i, slot) in out.iter_mut().enumerate() {
            let off = raw.data_offset + i * SIZE_SHORT;
            if off + SIZE_SHORT > self.data.len() {
                return false;
            }
            *slot = get_endian_u16(&self.data[off..off + SIZE_SHORT], self.little_endian);
        }
        true
    }

    pub fn entry_unsigned_long(&self, entry_index: u16, out: &mut [u32]) -> bool {
        let Some(raw) = self.entry_raw_data(entry_index) else {
            return false;
        };
        if raw.ty != TYPE_UNSIGNED_LONG || raw.count as usize != out.len() {
            return false;
        }
        for (i, slot) in out.iter_mut().enumerate() {
            let off = raw.data_offset + i * SIZE_LONG;
            if off + SIZE_LONG > self.data.len() {
                return false;
            }
            *slot = get_endian_u32(&self.data[off..off + SIZE_LONG], self.little_endian);
        }
        true
    }

    /// Matches `SkTiff::ImageFileDirectory::getEntrySignedRational`.
    /// Note: Skia's `SkTiffUtility.cpp` reads both numerator and denominator as `uint32_t`
    /// and returns 0.0 when denominator is 0.
    pub fn entry_signed_rational(&self, entry_index: u16, out: &mut [f32]) -> bool {
        self.entry_rational_impl(entry_index, TYPE_SIGNED_RATIONAL, out)
    }

    /// Matches `SkTiff::ImageFileDirectory::getEntryUnsignedRational`.
    pub fn entry_unsigned_rational(&self, entry_index: u16, out: &mut [f32]) -> bool {
        self.entry_rational_impl(entry_index, TYPE_UNSIGNED_RATIONAL, out)
    }

    fn entry_rational_impl(&self, entry_index: u16, expected_type: u16, out: &mut [f32]) -> bool {
        let Some(raw) = self.entry_raw_data(entry_index) else {
            return false;
        };
        if raw.ty != expected_type || raw.count as usize != out.len() {
            return false;
        }
        for (i, slot) in out.iter_mut().enumerate() {
            let off = raw.data_offset + i * 8;
            if off + 8 > self.data.len() {
                return false;
            }
            let numerator = get_endian_u32(&self.data[off..off + 4], self.little_endian);
            let denominator = get_endian_u32(&self.data[off + 4..off + 8], self.little_endian);
            if denominator == 0 {
                *slot = 0.0;
            } else {
                *slot = (numerator as f32) / (denominator as f32);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_header_be_and_le() {
        let be = [b'M', b'M', 0, 42, 0, 0, 0, 8];
        assert_eq!(parse_header(&be), Some((false, 8)));

        let le = [b'I', b'I', 42, 0, 8, 0, 0, 0];
        assert_eq!(parse_header(&le), Some((true, 8)));

        assert_eq!(parse_header(&[b'X', b'X', 0, 42, 0, 0, 0, 8]), None);
        assert_eq!(parse_header(&[b'M', b'M', 0, 42, 0, 0, 0]), None);
    }

    #[test]
    fn ifd_parse_and_truncation() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&ENDIAN_BIG);
        buf.extend_from_slice(&8u32.to_be_bytes());
        // 2 entries
        buf.extend_from_slice(&2u16.to_be_bytes());
        // Entry 0: tag 0x112, type SHORT (3), count 1, value 6
        buf.extend_from_slice(&0x0112u16.to_be_bytes());
        buf.extend_from_slice(&TYPE_UNSIGNED_SHORT.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&6u16.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        // Entry 1: tag 0x011A, type RATIONAL (5), count 1, offset 38
        buf.extend_from_slice(&0x011Au16.to_be_bytes());
        buf.extend_from_slice(&TYPE_UNSIGNED_RATIONAL.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&38u32.to_be_bytes());
        // Next IFD offset = 0
        buf.extend_from_slice(&0u32.to_be_bytes());
        // Rational at offset 38: 144 / 2 = 72
        buf.extend_from_slice(&144u32.to_be_bytes());
        buf.extend_from_slice(&2u32.to_be_bytes());

        let (le, off) = parse_header(&buf).unwrap();
        let ifd = ImageFileDirectory::from_offset(&buf, le, off, false).unwrap();
        assert_eq!(ifd.num_entries(), 2);
        assert_eq!(ifd.next_ifd_offset(), 0);
        assert_eq!(ifd.entry_tag(0), 0x0112);
        let mut s = [0u16; 1];
        assert!(ifd.entry_unsigned_short(0, &mut s));
        assert_eq!(s[0], 6);

        let mut f = [0.0f32; 1];
        assert!(ifd.entry_unsigned_rational(1, &mut f));
        assert_eq!(f[0], 72.0);

        // Truncated to only 1 full entry (8 + 2 + 12 = 22 bytes)
        let trunc = &buf[..22];
        assert!(ImageFileDirectory::from_offset(trunc, le, off, false).is_none());
        let ifd_t = ImageFileDirectory::from_offset(trunc, le, off, true).unwrap();
        assert_eq!(ifd_t.num_entries(), 1);
        assert_eq!(ifd_t.next_ifd_offset(), 0);
    }

    #[test]
    fn zero_denominator_returns_zero() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&ENDIAN_BIG);
        buf.extend_from_slice(&8u32.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&0x011Au16.to_be_bytes());
        buf.extend_from_slice(&TYPE_UNSIGNED_RATIONAL.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&26u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&72u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());

        let ifd = ImageFileDirectory::from_offset(&buf, false, 8, false).unwrap();
        let mut v = [99.0f32; 1];
        assert!(ifd.entry_unsigned_rational(0, &mut v));
        assert_eq!(v[0], 0.0);
    }
}
