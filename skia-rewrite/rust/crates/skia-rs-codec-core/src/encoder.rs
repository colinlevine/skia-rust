//! Safe Rust port of `src/encode/SkEncoder.cpp`.

/// Validates and clamps `num_rows` for `SkEncoder::encodeRows`.
///
/// Returns `None` if `num_rows <= 0` or `curr_row >= height`, otherwise returns
/// `Some(clamped_rows)` clamped to `height - curr_row`.
#[inline]
pub const fn clamp_encode_rows(curr_row: i32, height: i32, num_rows: i32) -> Option<i32> {
    if num_rows <= 0 || curr_row >= height {
        return None;
    }
    let remaining = height.saturating_sub(curr_row);
    if num_rows > remaining {
        Some(remaining)
    } else {
        Some(num_rows)
    }
}
