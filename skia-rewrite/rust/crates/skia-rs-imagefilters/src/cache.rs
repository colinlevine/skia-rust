#![forbid(unsafe_code)]

//! Safe Rust LRU cache index and key hashing for `SkImageFilterCache`
//! (ported from `src/core/SkImageFilterCache.cpp`).

use std::collections::HashMap;

use skia_rs_geometry::{IRect, Matrix};
use skia_rs_math::checksum::hash32;

/// Default cache budget on non-iOS platforms (128 MiB).
pub const K_DEFAULT_CACHE_SIZE: usize = 128 * 1024 * 1024;

/// Size in bytes of `SkImageFilterCacheKey`:
/// `u64 (8) + SkMatrix (40) + SkIRect (16) + u32 (4) + SkIRect (16) + u32 (4) = 88`.
pub const CACHE_KEY_BYTES_LEN: usize = 88;

/// Exact `#[repr(C)]` match for `SkImageFilterCacheKey`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CacheKey {
    pub unique_id: u64,
    pub matrix: Matrix,
    pub clip_bounds: IRect,
    pub src_gen_id: u32,
    pub src_subset: IRect,
    pub pad: u32,
}

impl CacheKey {
    /// Serializes to the exact packed 88-byte memory image of `SkImageFilterCacheKey`.
    pub fn to_bytes(&self) -> [u8; CACHE_KEY_BYTES_LEN] {
        let mut out = [0u8; CACHE_KEY_BYTES_LEN];
        out[0..8].copy_from_slice(&self.unique_id.to_ne_bytes());
        for i in 0..9 {
            let off = 8 + i * 4;
            out[off..off + 4].copy_from_slice(&self.matrix.mat[i].to_ne_bytes());
        }
        out[44..48].copy_from_slice(&self.matrix.type_mask.to_ne_bytes());
        out[48..52].copy_from_slice(&self.clip_bounds.left.to_ne_bytes());
        out[52..56].copy_from_slice(&self.clip_bounds.top.to_ne_bytes());
        out[56..60].copy_from_slice(&self.clip_bounds.right.to_ne_bytes());
        out[60..64].copy_from_slice(&self.clip_bounds.bottom.to_ne_bytes());
        out[64..68].copy_from_slice(&self.src_gen_id.to_ne_bytes());
        out[68..72].copy_from_slice(&self.src_subset.left.to_ne_bytes());
        out[72..76].copy_from_slice(&self.src_subset.top.to_ne_bytes());
        out[76..80].copy_from_slice(&self.src_subset.right.to_ne_bytes());
        out[80..84].copy_from_slice(&self.src_subset.bottom.to_ne_bytes());
        out[84..88].copy_from_slice(&self.pad.to_ne_bytes());
        out
    }

    /// Exact match for `SkChecksum::Hash32(&key, sizeof(Key))`.
    pub fn hash32(&self) -> u32 {
        let bytes = self.to_bytes();
        hash32(&bytes, 0)
    }
}

/// Raw byte-exact key matching `memcmp(this, &other, sizeof(*this)) == 0`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKeyBytes(pub [u8; CACHE_KEY_BYTES_LEN]);

impl CacheKeyBytes {
    #[inline]
    pub fn from_key(key: &CacheKey) -> Self {
        Self(key.to_bytes())
    }

    #[inline]
    pub fn hash32(&self) -> u32 {
        hash32(&self.0, 0)
    }
}

struct LruNode {
    key: CacheKeyBytes,
    filter_tag: Option<usize>,
    entry_handle: u64,
    bytes: usize,
    prev: Option<usize>,
    next: Option<usize>,
}

/// Safe Rust O(1) LRU cache index matching `CacheImpl` in `SkImageFilterCache.cpp`.
///
/// Manages key lookup, LRU doubly-linked list ordering, byte-budget eviction,
/// and per-`SkImageFilter*` reverse index (`purgeByImageFilter`).
/// The C++ bridge stores the actual `skif::FilterResult` objects indexed by `entry_handle`.
pub struct LruCacheIndex {
    slots: Vec<Option<LruNode>>,
    free_slots: Vec<usize>,
    lookup: HashMap<CacheKeyBytes, usize>,
    filter_entries: HashMap<usize, Vec<usize>>,
    head: Option<usize>,
    tail: Option<usize>,
    max_bytes: usize,
    current_bytes: usize,
    next_handle: u64,
}

impl LruCacheIndex {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            slots: Vec::new(),
            free_slots: Vec::new(),
            lookup: HashMap::new(),
            filter_entries: HashMap::new(),
            head: None,
            tail: None,
            max_bytes,
            current_bytes: 0,
            next_handle: 1,
        }
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.lookup.len()
    }

    #[inline]
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    #[inline]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Look up `key`; on hit, move to LRU head and return its `entry_handle`.
    pub fn get(&mut self, key: &CacheKeyBytes) -> Option<u64> {
        let slot = *self.lookup.get(key)?;
        if self.head != Some(slot) {
            self.lru_remove(slot);
            self.lru_add_to_head(slot);
        }
        Some(self.slots[slot].as_ref()?.entry_handle)
    }

    /// Insert a new entry for `(key, filter_tag)` with size `bytes`.
    /// Returns `(new_entry_handle, evicted_handles)`.
    pub fn set(
        &mut self,
        key: CacheKeyBytes,
        filter_tag: usize,
        bytes: usize,
    ) -> (u64, Vec<u64>) {
        let mut evicted = Vec::new();
        if let Some(&existing_slot) = self.lookup.get(&key) {
            if let Some(h) = self.remove_internal(existing_slot) {
                evicted.push(h);
            }
        }

        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        if self.next_handle == 0 {
            self.next_handle = 1;
        }

        let node = LruNode {
            key,
            filter_tag: if filter_tag != 0 {
                Some(filter_tag)
            } else {
                None
            },
            entry_handle: handle,
            bytes,
            prev: None,
            next: None,
        };
        let slot = if let Some(s) = self.free_slots.pop() {
            self.slots[s] = Some(node);
            s
        } else {
            let s = self.slots.len();
            self.slots.push(Some(node));
            s
        };

        self.lookup.insert(key, slot);
        self.lru_add_to_head(slot);
        self.current_bytes = self.current_bytes.saturating_add(bytes);
        if filter_tag != 0 {
            self.filter_entries.entry(filter_tag).or_default().push(slot);
        }

        while self.current_bytes > self.max_bytes {
            let Some(tail_slot) = self.tail else {
                break;
            };
            if tail_slot == slot {
                break;
            }
            if let Some(h) = self.remove_internal(tail_slot) {
                evicted.push(h);
            }
        }

        (handle, evicted)
    }

    /// Purge entries from tail while `current_bytes > 0` (exact match to `CacheImpl::purge()`).
    pub fn purge(&mut self) -> Vec<u64> {
        let mut evicted = Vec::new();
        while self.current_bytes > 0 {
            let Some(tail_slot) = self.tail else {
                break;
            };
            if let Some(h) = self.remove_internal(tail_slot) {
                evicted.push(h);
            }
        }
        evicted
    }

    /// Purge all entries associated with `filter_tag` (exact match to `CacheImpl::purgeByImageFilter`).
    pub fn purge_by_filter(&mut self, filter_tag: usize) -> Vec<u64> {
        let mut evicted = Vec::new();
        let Some(slots) = self.filter_entries.remove(&filter_tag) else {
            return evicted;
        };
        for slot in slots {
            if let Some(node) = self.slots[slot].as_mut() {
                node.filter_tag = None;
            }
            if let Some(h) = self.remove_internal(slot) {
                evicted.push(h);
            }
        }
        evicted
    }

    /// Remove all entries (for destructor cleanup).
    pub fn clear_all(&mut self) -> Vec<u64> {
        let mut evicted = Vec::new();
        while let Some(tail_slot) = self.tail {
            if let Some(h) = self.remove_internal(tail_slot) {
                evicted.push(h);
            }
        }
        evicted
    }

    fn lru_add_to_head(&mut self, slot: usize) {
        let old_head = self.head;
        if let Some(node) = self.slots[slot].as_mut() {
            node.prev = None;
            node.next = old_head;
        }
        if let Some(h) = old_head {
            if let Some(h_node) = self.slots[h].as_mut() {
                h_node.prev = Some(slot);
            }
        } else {
            self.tail = Some(slot);
        }
        self.head = Some(slot);
    }

    fn lru_remove(&mut self, slot: usize) {
        let (prev, next) = match self.slots[slot].as_ref() {
            Some(n) => (n.prev, n.next),
            None => return,
        };
        if let Some(p) = prev {
            if let Some(p_node) = self.slots[p].as_mut() {
                p_node.next = next;
            }
        } else {
            self.head = next;
        }
        if let Some(n) = next {
            if let Some(n_node) = self.slots[n].as_mut() {
                n_node.prev = prev;
            }
        } else {
            self.tail = prev;
        }
        if let Some(node) = self.slots[slot].as_mut() {
            node.prev = None;
            node.next = None;
        }
    }

    fn remove_internal(&mut self, slot: usize) -> Option<u64> {
        self.lru_remove(slot);
        let node = self.slots[slot].take()?;
        if let Some(ftag) = node.filter_tag {
            if let Some(vec) = self.filter_entries.get_mut(&ftag) {
                if vec.len() == 1 && vec[0] == slot {
                    self.filter_entries.remove(&ftag);
                } else if let Some(pos) = vec.iter().position(|&s| s == slot) {
                    vec.remove(pos);
                }
            }
        }
        self.current_bytes = self.current_bytes.saturating_sub(node.bytes);
        self.lookup.remove(&node.key);
        self.free_slots.push(slot);
        Some(node.entry_handle)
    }
}
