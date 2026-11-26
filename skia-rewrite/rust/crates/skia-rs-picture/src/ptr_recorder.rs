//! Safe Rust implementation of `SkPtrSet` (`src/core/SkPtrRecorder.cpp`).

/// Binary layout matching `SkPtrSet::Pair` (`void* fPtr; uint32_t fIndex;`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtrPair {
    pub ptr: usize,
    pub index: u32,
}

/// Result of looking up or computing the insertion position for a pointer in `SkPtrSet`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtrInsertResult {
    /// `true` if the pointer was already present (or null), `false` if it must be inserted at `pos`.
    pub found: bool,
    /// Insertion position (when `!found`) or existing slot index (when `found`).
    pub pos: usize,
    /// 1-based ID (0 for null pointer).
    pub index: u32,
}

/// Ordering matching `SkPtrSet::Less(const Pair& a, const Pair& b)`.
#[inline]
pub fn ptr_pair_less(a: PtrPair, b: PtrPair) -> bool {
    a.ptr < b.ptr
}

/// Binary search matching `SkPtrSet::find(void* ptr) const`.
pub fn find_ptr(pairs: &[PtrPair], target: usize) -> u32 {
    if target == 0 {
        return 0;
    }
    match pairs.binary_search_by_key(&target, |p| p.ptr) {
        Ok(idx) => pairs[idx].index,
        Err(_) => 0,
    }
}

/// Lookup or compute insertion position matching `SkPtrSet::add(void* ptr)`.
pub fn find_or_insert_pos(pairs: &[PtrPair], target: usize) -> PtrInsertResult {
    if target == 0 {
        return PtrInsertResult {
            found: true,
            pos: 0,
            index: 0,
        };
    }
    match pairs.binary_search_by_key(&target, |p| p.ptr) {
        Ok(idx) => PtrInsertResult {
            found: true,
            pos: idx,
            index: pairs[idx].index,
        },
        Err(ins) => PtrInsertResult {
            found: false,
            pos: ins,
            index: (pairs.len() as u32) + 1,
        },
    }
}

/// Scatters `pairs` by 1-based `index` into `out`, matching `SkPtrSet::copyToArray`.
pub fn copy_to_array(pairs: &[PtrPair], out: &mut [usize]) {
    let count = pairs.len();
    debug_assert!(out.len() >= count);
    for pair in pairs {
        let slot = (pair.index as usize).wrapping_sub(1);
        if slot < out.len() {
            out[slot] = pair.ptr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptr_set_operations() {
        let mut pairs: Vec<PtrPair> = Vec::new();
        assert_eq!(find_ptr(&pairs, 0), 0);
        assert_eq!(find_ptr(&pairs, 0x1000), 0);

        let r0 = find_or_insert_pos(&pairs, 0);
        assert!(r0.found && r0.index == 0);

        for &(addr, expected_id) in &[(0x3000usize, 1u32), (0x1000, 2), (0x2000, 3), (0x1000, 2)] {
            let r = find_or_insert_pos(&pairs, addr);
            assert_eq!(r.index, expected_id);
            if !r.found {
                pairs.insert(
                    r.pos,
                    PtrPair {
                        ptr: addr,
                        index: r.index,
                    },
                );
            }
        }

        assert_eq!(find_ptr(&pairs, 0x3000), 1);
        assert_eq!(find_ptr(&pairs, 0x1000), 2);
        assert_eq!(find_ptr(&pairs, 0x2000), 3);
        assert_eq!(find_ptr(&pairs, 0x4000), 0);

        let mut arr = vec![0usize; pairs.len()];
        copy_to_array(&pairs, &mut arr);
        assert_eq!(arr, vec![0x3000, 0x1000, 0x2000]);
    }
}
