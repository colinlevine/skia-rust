//! Safe Rust implementation of `SkDrawable`, `SkRecordedDrawable`, and `SkAnnotation`
//! computational helpers.

use std::sync::atomic::{AtomicI32, Ordering};

use skia_rs_geometry::{Point, Rect};

/// Canonical annotation key for URLs (`SkAnnotationKeys::URL_Key()`).
pub const ANNOTATION_KEY_URL: &[u8] = b"SkAnnotationKey_URL\0";

/// Canonical annotation key for defining a named destination
/// (`SkAnnotationKeys::Define_Named_Dest_Key()`).
pub const ANNOTATION_KEY_DEFINE_NAMED_DEST: &[u8] = b"SkAnnotationKey_Define_Named_Dest\0";

/// Canonical annotation key for linking to a named destination
/// (`SkAnnotationKeys::Link_Named_Dest_Key()`).
pub const ANNOTATION_KEY_LINK_NAMED_DEST: &[u8] = b"SkAnnotationKey_Link_Named_Dest\0";

/// Matches `next_generation_id()` in `SkDrawable.cpp`:
/// monotonic non-zero 32-bit ID generator using relaxed atomic increment.
pub fn next_drawable_generation_id() -> u32 {
    static NEXT_ID: AtomicI32 = AtomicI32::new(1);
    loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id as u32;
        }
    }
}

/// Returns the zero-sized rect at `point` used by `SkAnnotateNamedDestination`.
#[inline]
pub fn annotate_named_dest_rect(point: Point) -> Rect {
    Rect::make_ltrb(point.x, point.y, point.x, point.y)
}

/// Computes `SkRecordedDrawable::onApproximateBytesUsed()`.
#[inline]
pub fn recorded_drawable_approx_bytes(
    self_size: usize,
    record_bytes: usize,
    bbh_bytes: usize,
    drawables_bytes: usize,
) -> usize {
    self_size + record_bytes + bbh_bytes + drawables_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generation_ids_unique_nonzero() {
        let a = next_drawable_generation_id();
        let b = next_drawable_generation_id();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn test_annotation_helpers() {
        assert_eq!(ANNOTATION_KEY_URL, b"SkAnnotationKey_URL\0");
        assert_eq!(
            ANNOTATION_KEY_DEFINE_NAMED_DEST,
            b"SkAnnotationKey_Define_Named_Dest\0"
        );
        assert_eq!(
            ANNOTATION_KEY_LINK_NAMED_DEST,
            b"SkAnnotationKey_Link_Named_Dest\0"
        );
        let r = annotate_named_dest_rect(Point::new(12.5, -4.0));
        assert_eq!(r, Rect::make_ltrb(12.5, -4.0, 12.5, -4.0));
        assert_eq!(recorded_drawable_approx_bytes(64, 128, 256, 512), 960);
    }
}
