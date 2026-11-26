//! Saturating size arithmetic matching `SkSafeMath.cpp`.

#[inline]
pub fn add(x: usize, y: usize) -> usize {
    x.checked_add(y).unwrap_or(usize::MAX)
}

#[inline]
pub fn mul(x: usize, y: usize) -> usize {
    x.checked_mul(y).unwrap_or(usize::MAX)
}
