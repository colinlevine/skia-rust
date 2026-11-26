//! Safe Rust algorithms for `SkData` and `SkDataTable`.

/// Computes the clamped byte count for `SkData::copyRange(offset, length, buffer)`.
#[inline]
pub fn copy_range_len(available: usize, offset: usize, length: usize) -> usize {
    if offset >= available || length == 0 {
        return 0;
    }
    let rem = available - offset;
    length.min(rem)
}

/// Copies a clamped range from `src` starting at `offset` into `dst`, returning the bytes copied.
pub fn copy_range(src: &[u8], offset: usize, length: usize, dst: Option<&mut [u8]>) -> usize {
    let n = copy_range_len(src.len(), offset, length);
    if n > 0 {
        if let Some(out) = dst {
            let copy_n = n.min(out.len());
            out[..copy_n].copy_from_slice(&src[offset..offset + copy_n]);
        }
    }
    n
}

/// Validates subset bounds matching `VALIDATE_SUBSET(size, offset, length)` in `SkData.cpp`.
#[inline]
pub const fn validate_subset(size: usize, offset: usize, length: usize) -> bool {
    offset <= size && length <= size - offset
}

/// Computes `length + obj_size` with overflow checking matching `SkData::PrivateNewWithCopy`.
/// Returns `None` on overflow.
#[inline]
pub const fn inline_alloc_size(obj_size: usize, length: usize) -> Option<usize> {
    match length.checked_add(obj_size) {
        Some(actual) if length < actual => Some(actual),
        _ => None,
    }
}

/// Computes the total buffer size for `SkDataTable::MakeCopyArrays`:
/// `count * dir_size + sum(sizes)`. Returns `None` on overflow or `count <= 0`.
pub fn data_table_copy_arrays_size(dir_size: usize, sizes: &[usize]) -> Option<usize> {
    let count = sizes.len();
    if count == 0 {
        return Some(0);
    }
    let mut data_size: usize = 0;
    for &s in sizes {
        data_size = data_size.checked_add(s)?;
    }
    let dir_bytes = count.checked_mul(dir_size)?;
    dir_bytes.checked_add(data_size)
}

/// Computes the total buffer size for `SkDataTable::MakeCopyArray`:
/// `elem_size * count`. Returns `None` on overflow.
#[inline]
pub const fn data_table_copy_array_size(elem_size: usize, count: usize) -> Option<usize> {
    elem_size.checked_mul(count)
}

/// Computes the byte offset of element `index` in a uniform `SkDataTable`.
#[inline]
pub const fn data_table_uniform_offset(index: usize, elem_size: usize) -> Option<usize> {
    index.checked_mul(elem_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_range() {
        let src = b"hello world";
        let mut dst = [0u8; 5];
        assert_eq!(copy_range(src, 6, 10, Some(&mut dst)), 5);
        assert_eq!(&dst, b"world");
        assert_eq!(copy_range(src, 11, 5, Some(&mut dst)), 0);
        assert_eq!(copy_range(src, 100, 5, None), 0);
        assert_eq!(copy_range(src, 0, 0, None), 0);
        assert_eq!(copy_range(src, 2, 3, None), 3);
    }

    #[test]
    fn test_validate_subset() {
        assert!(validate_subset(10, 0, 10));
        assert!(validate_subset(10, 10, 0));
        assert!(validate_subset(10, 3, 7));
        assert!(!validate_subset(10, 3, 8));
        assert!(!validate_subset(10, 11, 0));
        assert!(!validate_subset(10, usize::MAX, 1));
    }

    #[test]
    fn test_inline_alloc_size() {
        assert_eq!(inline_alloc_size(40, 100), Some(140));
        assert_eq!(inline_alloc_size(40, usize::MAX - 10), None);
        assert_eq!(inline_alloc_size(0, 10), None);
    }

    #[test]
    fn test_data_table_sizes() {
        assert_eq!(data_table_copy_arrays_size(16, &[3, 5, 7]), Some(48 + 15));
        assert_eq!(data_table_copy_arrays_size(16, &[]), Some(0));
        assert_eq!(data_table_copy_arrays_size(16, &[usize::MAX, 1]), None);
        assert_eq!(data_table_copy_array_size(8, 10), Some(80));
        assert_eq!(data_table_copy_array_size(usize::MAX, 2), None);
    }
}
