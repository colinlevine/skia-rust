use skia_rs_math::{
    base64, bezier, checksum, cubic_map, cubics, float_to_decimal, floating_point, half,
    math_priv, quads, safe_math, scalar, utf,
};

// --- SkBase64 ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_base64_decode(
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
    dst_len_out: *mut usize,
) -> i32 {
    let src_slice = if src.is_null() || src_len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(src, src_len)
    };
    let dst_opt = if dst.is_null() {
        None
    } else {
        // Caller guarantees dst is large enough for decoded output (<= src_len).
        Some(core::slice::from_raw_parts_mut(dst, src_len))
    };
    let (err, len) = base64::decode(src_slice, dst_opt);
    if !dst_len_out.is_null() && err == base64::Base64Error::NoError {
        *dst_len_out = len;
    }
    err as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_base64_encode(
    src: *const u8,
    len: usize,
    dst: *mut u8,
    encode_map: *const u8,
) -> usize {
    let src_slice = if src.is_null() || len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(src, len)
    };
    let enc_len = base64::encoded_size(len);
    let dst_opt = if dst.is_null() {
        None
    } else {
        Some(core::slice::from_raw_parts_mut(dst, enc_len))
    };
    let map_opt = if encode_map.is_null() {
        None
    } else {
        Some(&*(encode_map as *const [u8; 65]))
    };
    base64::encode(src_slice, dst_opt, map_opt)
}

// --- SkChecksum ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_checksum_hash32(data: *const u8, bytes: usize, seed: u32) -> u32 {
    let slice = if data.is_null() || bytes == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(data, bytes)
    };
    checksum::hash32(slice, seed)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_checksum_hash64(data: *const u8, bytes: usize, seed: u64) -> u64 {
    let slice = if data.is_null() || bytes == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(data, bytes)
    };
    checksum::hash64(slice, seed)
}

// --- SkHalf ---

#[no_mangle]
pub extern "C" fn skia_rs_float_to_half(f: f32) -> u16 {
    half::float_to_half(f)
}

#[no_mangle]
pub extern "C" fn skia_rs_half_to_float(h: u16) -> f32 {
    half::half_to_float(h)
}

// --- SkScalar ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_float_interp_func(
    search_key: f32,
    keys: *const f32,
    values: *const f32,
    length: i32,
) -> f32 {
    let len = length as usize;
    let ks = core::slice::from_raw_parts(keys, len);
    let vs = core::slice::from_raw_parts(values, len);
    scalar::float_interp_func(search_key, ks, vs)
}

// --- SkFloatingPoint ---

#[no_mangle]
pub extern "C" fn skia_rs_doubles_nearly_equal_ulps(a: f64, b: f64, max_ulps_diff: u8) -> bool {
    floating_point::doubles_nearly_equal_ulps(a, b, max_ulps_diff)
}

#[no_mangle]
pub extern "C" fn skia_rs_double_nearly_zero(a: f64) -> bool {
    floating_point::double_nearly_zero(a)
}

// --- SkSafeMath & SkMathPriv ---

#[no_mangle]
pub extern "C" fn skia_rs_safe_math_add(x: usize, y: usize) -> usize {
    safe_math::add(x, y)
}

#[no_mangle]
pub extern "C" fn skia_rs_safe_math_mul(x: usize, y: usize) -> usize {
    safe_math::mul(x, y)
}

#[no_mangle]
pub extern "C" fn skia_rs_sqrt_bits(x: i32, count: i32) -> i32 {
    math_priv::sqrt_bits(x, count)
}

// --- SkQuads ---

#[repr(C)]
pub struct SkiaRsQuadRoots {
    pub discriminant: f64,
    pub root0: f64,
    pub root1: f64,
}

#[no_mangle]
pub extern "C" fn skia_rs_quads_discriminant(a: f64, b: f64, c: f64) -> f64 {
    quads::discriminant(a, b, c)
}

#[no_mangle]
pub extern "C" fn skia_rs_quads_roots(a: f64, b: f64, c: f64) -> SkiaRsQuadRoots {
    let r = quads::roots(a, b, c);
    SkiaRsQuadRoots {
        discriminant: r.discriminant,
        root0: r.root0,
        root1: r.root1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_quads_roots_real(
    a: f64,
    b: f64,
    c: f64,
    solution: *mut [f64; 2],
) -> i32 {
    quads::roots_real(a, b, c, &mut *solution) as i32
}

#[no_mangle]
pub extern "C" fn skia_rs_quads_eval_at(a: f64, b: f64, c: f64, t: f64) -> f64 {
    quads::eval_at(a, b, c, t)
}

// --- SkCubics ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubics_roots_real(
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    solution: *mut [f64; 3],
) -> i32 {
    cubics::roots_real(a, b, c, d, &mut *solution) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubics_roots_valid_t(
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    solution: *mut [f64; 3],
) -> i32 {
    cubics::roots_valid_t(a, b, c, d, &mut *solution) as i32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubics_binary_search_roots_valid_t(
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    solution: *mut [f64; 3],
) -> i32 {
    cubics::binary_search_roots_valid_t(a, b, c, d, &mut *solution) as i32
}

// --- SkBezierCurves ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_cubic_eval_at(
    curve: *const [f64; 8],
    t: f64,
    out: *mut [f64; 2],
) {
    *out = bezier::cubic_eval_at(&*curve, t);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_cubic_subdivide(
    curve: *const [f64; 8],
    t: f64,
    out: *mut [f64; 14],
) {
    bezier::cubic_subdivide(&*curve, t, &mut *out);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_cubic_convert_to_poly(
    curve: *const [f64; 8],
    y_values: bool,
    out: *mut [f64; 4],
) {
    *out = bezier::cubic_convert_to_polynomial(&*curve, y_values);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_cubic_intersect_horiz(
    pts: *const [[f32; 2]; 4],
    y_intercept: f32,
    storage: *mut [f32; 3],
) -> usize {
    bezier::cubic_intersect_with_horizontal_line(&*pts, y_intercept, &mut *storage)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_cubic_intersect(
    ax: f64,
    bx: f64,
    cx: f64,
    dx: f64,
    ay: f64,
    by: f64,
    cy: f64,
    dy: f64,
    to_intersect: f32,
    storage: *mut [f32; 3],
) -> usize {
    bezier::cubic_intersect(ax, bx, cx, dx, ay, by, cy, dy, to_intersect, &mut *storage)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_quad_intersect_horiz(
    pts: *const [[f32; 2]; 3],
    y_intercept: f32,
    storage: *mut [f32; 2],
) -> usize {
    bezier::quad_intersect_with_horizontal_line(&*pts, y_intercept, &mut *storage)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_bezier_quad_intersect(
    ax: f64,
    bx: f64,
    cx: f64,
    ay: f64,
    by: f64,
    cy: f64,
    y_intercept: f64,
    storage: *mut [f32; 2],
) -> usize {
    bezier::quad_intersect(ax, bx, cx, ay, by, cy, y_intercept, &mut *storage)
}

// --- SkCubicMap ---

#[repr(C)]
pub struct SkiaRsCubicMap {
    pub coeff: [[f32; 2]; 3],
    pub map_type: i32,
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_map_init(
    p1x: f32,
    p1y: f32,
    p2x: f32,
    p2y: f32,
    out: *mut SkiaRsCubicMap,
) {
    let m = cubic_map::CubicMap::new([p1x, p1y], [p2x, p2y]);
    (*out).coeff = m.coeff;
    (*out).map_type = m.map_type as i32;
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_map_y_from_x(raw: *const SkiaRsCubicMap, x: f32) -> f32 {
    let r = &*raw;
    let map_type = match r.map_type {
        0 => cubic_map::CubicMapType::Line,
        1 => cubic_map::CubicMapType::CubeRoot,
        _ => cubic_map::CubicMapType::Solver,
    };
    let m = cubic_map::CubicMap {
        coeff: r.coeff,
        map_type,
    };
    m.compute_y_from_x(x)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_cubic_map_from_t(
    raw: *const SkiaRsCubicMap,
    t: f32,
    out: *mut [f32; 2],
) {
    let r = &*raw;
    let m = cubic_map::CubicMap {
        coeff: r.coeff,
        map_type: cubic_map::CubicMapType::Solver,
    };
    *out = m.compute_from_t(t);
}

// --- SkUTF ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_count_utf8(ptr: *const u8, len: usize) -> i32 {
    if ptr.is_null() {
        return if len == 0 { 0 } else { -1 };
    }
    utf::count_utf8(core::slice::from_raw_parts(ptr, len))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_count_utf16(ptr: *const u16, count: usize) -> i32 {
    if count == 0 {
        return 0;
    }
    utf::count_utf16(core::slice::from_raw_parts(ptr, count))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_count_utf32(ptr: *const i32, count: usize) -> i32 {
    if count == 0 {
        return 0;
    }
    utf::count_utf32(core::slice::from_raw_parts(ptr, count))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_next_utf8(
    ptr: *const u8,
    len: usize,
    consumed_out: *mut usize,
) -> i32 {
    let slice = if len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(ptr, len)
    };
    let (cp, consumed) = utf::next_utf8(slice);
    *consumed_out = consumed;
    cp
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_next_utf16(
    ptr: *const u16,
    len: usize,
    consumed_out: *mut usize,
) -> i32 {
    let slice = if len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(ptr, len)
    };
    let (cp, consumed) = utf::next_utf16(slice);
    *consumed_out = consumed;
    cp
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_next_utf32(
    ptr: *const i32,
    len: usize,
    consumed_out: *mut usize,
) -> i32 {
    let slice = if len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(ptr, len)
    };
    let (cp, consumed) = utf::next_utf32(slice);
    *consumed_out = consumed;
    cp
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_to_utf8(uni: i32, dst: *mut [u8; 4]) -> usize {
    if dst.is_null() {
        utf::to_utf8(uni, None)
    } else {
        utf::to_utf8(uni, Some(&mut *dst))
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_to_utf16(uni: i32, dst: *mut [u16; 2]) -> usize {
    if dst.is_null() {
        utf::to_utf16(uni, None)
    } else {
        utf::to_utf16(uni, Some(&mut *dst))
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_utf8_to_utf16(
    dst: *mut u16,
    dst_cap: i32,
    src: *const u8,
    src_len: usize,
) -> i32 {
    let src_slice = if src.is_null() || src_len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(src, src_len)
    };
    let dst_opt = if dst.is_null() || dst_cap <= 0 {
        None
    } else {
        Some(core::slice::from_raw_parts_mut(dst, dst_cap as usize))
    };
    utf::utf8_to_utf16(dst_opt, src_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_utf_utf16_to_utf8(
    dst: *mut u8,
    dst_cap: i32,
    src: *const u16,
    src_len: usize,
) -> i32 {
    let src_slice = if src.is_null() || src_len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(src, src_len)
    };
    let dst_opt = if dst.is_null() || dst_cap <= 0 {
        None
    } else {
        Some(core::slice::from_raw_parts_mut(dst, dst_cap as usize))
    };
    utf::utf16_to_utf8(dst_opt, src_slice)
}

// --- SkFloatToDecimal ---

#[no_mangle]
pub unsafe extern "C" fn skia_rs_float_to_decimal(
    value: f32,
    output: *mut [u8; float_to_decimal::MAX_FLOAT_TO_DECIMAL_LEN],
) -> u32 {
    float_to_decimal::float_to_decimal(value, &mut *output)
}
