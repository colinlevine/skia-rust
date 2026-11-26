//! Scalar interpolation matching Skia's `SkFloatInterpFunc`.

pub fn float_interp_func(search_key: f32, keys: &[f32], values: &[f32]) -> f32 {
    debug_assert!(!keys.is_empty() && keys.len() == values.len());
    let length = keys.len();
    let mut right = 0usize;
    while right < length && keys[right] < search_key {
        right += 1;
    }
    if right == length {
        return values[length - 1];
    }
    if right == 0 {
        return values[0];
    }
    let left_key = keys[right - 1];
    let right_key = keys[right];
    let fract = (search_key - left_key) / (right_key - left_key);
    let a = values[right - 1];
    let b = values[right];
    a + (b - a) * fract
}
