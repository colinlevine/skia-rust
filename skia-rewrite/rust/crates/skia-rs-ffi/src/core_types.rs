//! FFI exports for `skia-rs-core-types` (Layer 2.4).
//!
//! Exposes `extern "C"` entry points backing the 18 Layer 2.4 core C++ files:
//! `SkData`, `SkDataTable`, `SkStream`, `SkBuffer`, `SkReadBuffer`, `SkWriteBuffer`,
//! `SkWriter32`, `SkString`, `SkStringUtils`, `SkFlattenable`, `SkPaint`, `SkPaintPriv`,
//! `SkDescriptor`, `SkMask`, `SkBlurMask`, `SkGaussFilter`, `SkMaskGamma`, `SkMaskCache`.

use std::sync::{OnceLock, RwLock};

use skia_rs_core_types::{
    blur_mask, buffer, data, descriptor,
    flattenable::FlattenableRegistry,
    gauss_filter, mask,
    mask_gamma::{self, LuminanceMode},
    paint::{self, PaintUnpackedV68},
    stream, string,
};
use skia_rs_geometry::{IRect, Rect, Size};

// ===========================================================================
// SkData & SkDataTable
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_data_copy_range(
    src: *const u8,
    src_len: usize,
    offset: usize,
    length: usize,
    dst: *mut u8,
) -> usize {
    if src.is_null() || src_len == 0 {
        return 0;
    }
    let src_slice = std::slice::from_raw_parts(src, src_len);
    if dst.is_null() {
        data::copy_range(src_slice, offset, length, None)
    } else {
        let clamped = data::copy_range_len(src_len, offset, length);
        if clamped == 0 {
            return 0;
        }
        let dst_slice = std::slice::from_raw_parts_mut(dst, clamped);
        data::copy_range(src_slice, offset, length, Some(dst_slice))
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_data_validate_subset(
    size: usize,
    offset: usize,
    length: usize,
) -> bool {
    data::validate_subset(size, offset, length)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_data_inline_alloc_size(
    obj_size: usize,
    length: usize,
    out_actual: *mut usize,
) -> bool {
    match data::inline_alloc_size(obj_size, length) {
        Some(actual) => {
            if !out_actual.is_null() {
                *out_actual = actual;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_data_table_copy_arrays_size(
    dir_size: usize,
    sizes: *const usize,
    count: usize,
    out_size: *mut usize,
) -> bool {
    let slice = if count == 0 || sizes.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(sizes, count)
    };
    match data::data_table_copy_arrays_size(dir_size, slice) {
        Some(total) => {
            if !out_size.is_null() {
                *out_size = total;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_data_table_copy_array_size(
    elem_size: usize,
    count: usize,
    out_size: *mut usize,
) -> bool {
    match data::data_table_copy_array_size(elem_size, count) {
        Some(total) => {
            if !out_size.is_null() {
                *out_size = total;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_data_table_uniform_offset(
    index: usize,
    elem_size: usize,
    out_offset: *mut usize,
) -> bool {
    match data::data_table_uniform_offset(index, elem_size) {
        Some(off) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            true
        }
        None => false,
    }
}

// ===========================================================================
// SkStream / SkWStream / SkFILEStream / SkMemoryStream / SkDynamicMemoryWStream
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_size_of_packed_uint(value: usize) -> i32 {
    stream::size_of_packed_uint(value)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_encode_packed_uint(
    value: usize,
    out: *mut [u8; 5],
) -> usize {
    stream::encode_packed_uint(value, &mut *out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_decode_packed_uint(
    bytes: *const u8,
    len: usize,
    out_value: *mut usize,
    out_consumed: *mut usize,
) -> bool {
    if bytes.is_null() || len == 0 {
        return false;
    }
    let slice = std::slice::from_raw_parts(bytes, len);
    match stream::decode_packed_uint(slice) {
        Some((val, consumed)) => {
            if !out_value.is_null() {
                *out_value = val;
            }
            if !out_consumed.is_null() {
                *out_consumed = consumed;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_file_clamp_init(
    end: usize,
    start: usize,
    current: usize,
    out_start: *mut usize,
    out_current: *mut usize,
) {
    let (s, c) = stream::file_stream_clamp_init(end, start, current);
    if !out_start.is_null() {
        *out_start = s;
    }
    if !out_current.is_null() {
        *out_current = c;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_file_seek(
    start: usize,
    end: usize,
    position: usize,
) -> usize {
    stream::file_stream_seek(start, end, position)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_file_move(
    start: usize,
    end: usize,
    current: usize,
    offset: i64,
) -> usize {
    stream::file_stream_move(start, end, current, offset)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_clamp_read(remaining: usize, requested: usize) -> usize {
    stream::clamp_stream_read(remaining, requested)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_memory_seek(data_size: usize, position: usize) -> usize {
    stream::memory_stream_seek(data_size, position)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_memory_move(
    data_size: usize,
    current_offset: usize,
    delta: isize,
) -> usize {
    stream::memory_stream_move(data_size, current_offset, delta)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_dynamic_block_payload_size(
    count: usize,
    block_header_size: usize,
) -> usize {
    stream::dynamic_block_payload_size(count, block_header_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_dynamic_pad_align4_bytes(tail_written: usize) -> usize {
    stream::dynamic_pad_align4_bytes(tail_written)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_stream_remaining_length_is_below(
    has_length: bool,
    length: usize,
    has_position: bool,
    position: usize,
    requested: usize,
) -> bool {
    stream::remaining_length_is_below(has_length, length, has_position, position, requested)
}

// ===========================================================================
// SkBuffer, SkReadBuffer, SkWriteBuffer, SkWriter32
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rbuffer_skip(
    valid: bool,
    available: usize,
    size: usize,
    out_advance: *mut usize,
) -> bool {
    let (ok, adv) = buffer::rbuffer_skip(valid, available, size);
    if !out_advance.is_null() {
        *out_advance = adv;
    }
    ok
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_rbuffer_skip_to_align4(
    pos_addr: usize,
    valid: bool,
    available: usize,
    out_advance: *mut usize,
) -> bool {
    let (ok, adv) = buffer::rbuffer_skip_to_align4(pos_addr, valid, available);
    if !out_advance.is_null() {
        *out_advance = adv;
    }
    ok
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_wbuffer_pad_align4(pos: usize) -> usize {
    buffer::wbuffer_pad_align4(pos)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_read_buffer_validate_memory(addr: usize, size: usize) -> bool {
    buffer::read_buffer_validate_memory(addr, size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_read_buffer_skip(
    already_error: bool,
    curr_addr: usize,
    available: usize,
    size: usize,
    out_inc: *mut usize,
) -> bool {
    match buffer::read_buffer_skip(already_error, curr_addr, available, size) {
        Some(inc) => {
            if !out_inc.is_null() {
                *out_inc = inc;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_read_buffer_skip_mul(
    already_error: bool,
    curr_addr: usize,
    available: usize,
    count: usize,
    size: usize,
    out_inc: *mut usize,
) -> bool {
    match buffer::read_buffer_skip_mul(already_error, curr_addr, available, count, size) {
        Some(inc) => {
            if !out_inc.is_null() {
                *out_inc = inc;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_read_buffer_validate_bool(
    value: u32,
    out_valid: *mut bool,
) -> bool {
    let (valid, b) = buffer::read_buffer_validate_bool(value);
    if !out_valid.is_null() {
        *out_valid = valid;
    }
    b
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_read_buffer_check_int(
    value: i32,
    min: i32,
    max: i32,
    out_valid: *mut bool,
) -> i32 {
    let (valid, res) = buffer::read_buffer_check_int(value, min, max);
    if !out_valid.is_null() {
        *out_valid = valid;
    }
    res
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_negate_to_size_t(value: i32) -> usize {
    buffer::negate_to_size_t(value)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_encode_flattenable_dict_index(index: u32) -> u32 {
    buffer::encode_flattenable_dict_index(index)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_decode_flattenable_dict_index(word: u32) -> u32 {
    buffer::decode_flattenable_dict_index(word)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_writer32_string_size(len: usize) -> usize {
    buffer::writer32_string_size(len)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_writer32_grow_capacity(
    current_capacity: usize,
    needed_size: usize,
) -> usize {
    buffer::writer32_grow_capacity(current_capacity, needed_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_writer32_format_string(
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
    dst_cap: usize,
) -> usize {
    let src_slice = if src_len == 0 || src.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(src, src_len)
    };
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_cap);
    buffer::writer32_format_string(src_slice, dst_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_writer32_encode_sampling(
    max_aniso: i32,
    use_cubic: bool,
    cubic_b: f32,
    cubic_c: f32,
    filter: u32,
    mipmap: u32,
    out_words: *mut [u32; 4],
) -> usize {
    buffer::writer32_encode_sampling(
        max_aniso,
        use_cubic,
        cubic_b,
        cubic_c,
        filter,
        mipmap,
        &mut *out_words,
    )
}

// ===========================================================================
// SkString & SkStringUtils
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_ends_with(
    s: *const u8,
    s_len: usize,
    suffix: *const u8,
    suffix_len: usize,
) -> bool {
    let s_slice = if s_len == 0 || s.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(s, s_len)
    };
    let suf_slice = if suffix_len == 0 || suffix.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(suffix, suffix_len)
    };
    string::str_ends_with(s_slice, suf_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_ends_with_char(
    s: *const u8,
    s_len: usize,
    suffix_char: u8,
) -> bool {
    let s_slice = if s_len == 0 || s.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(s, s_len)
    };
    string::str_ends_with_char(s_slice, suffix_char)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_starts_with_one_of(
    s: *const u8,
    s_len: usize,
    prefixes: *const u8,
) -> i32 {
    if prefixes.is_null() {
        return -1;
    }
    let s_slice = if s_len == 0 || s.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(s, s_len)
    };
    // Measure double-NUL-terminated prefix list matching C++ `do { ... } while (prefixes[0]);`.
    let mut p_len = 0usize;
    loop {
        while *prefixes.add(p_len) != 0 {
            p_len += 1;
        }
        p_len += 1; // include this entry's NUL terminator
        if *prefixes.add(p_len) == 0 {
            p_len += 1; // include list-terminating NUL
            break;
        }
    }
    let p_slice = std::slice::from_raw_parts(prefixes, p_len);
    string::str_starts_with_one_of(s_slice, p_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_append_u32(out: *mut u8, dec: u32) -> usize {
    let buf = std::slice::from_raw_parts_mut(out, string::STR_APPEND_U32_MAX_SIZE);
    string::str_append_u32(dec, buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_append_s32(out: *mut u8, dec: i32) -> usize {
    let buf = std::slice::from_raw_parts_mut(out, string::STR_APPEND_S32_MAX_SIZE);
    string::str_append_s32(dec, buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_append_u64(
    out: *mut u8,
    dec: u64,
    min_digits: i32,
) -> usize {
    let buf = std::slice::from_raw_parts_mut(out, string::STR_APPEND_U64_MAX_SIZE);
    string::str_append_u64(dec, min_digits, buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_append_s64(
    out: *mut u8,
    dec: i64,
    min_digits: i32,
) -> usize {
    let buf = std::slice::from_raw_parts_mut(out, string::STR_APPEND_S64_MAX_SIZE);
    string::str_append_s64(dec, min_digits, buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_str_append_scalar(out: *mut u8, value: f32) -> usize {
    let buf = std::slice::from_raw_parts_mut(out, string::STR_APPEND_SCALAR_MAX_SIZE);
    string::str_append_scalar(value, buf)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_format_hex_u32(
    hex: u32,
    min_digits: i32,
    out: *mut [u8; 8],
) -> usize {
    string::format_hex_u32(hex, min_digits, &mut *out)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_format_append_scalar(
    value: f32,
    as_hex: bool,
    out: *mut u8,
    out_cap: usize,
) -> usize {
    let formatted = string::format_append_scalar(value, as_hex);
    let n = formatted.len().min(out_cap);
    if !out.is_null() && n > 0 {
        std::ptr::copy_nonoverlapping(formatted.as_ptr(), out, n);
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_trim_size_t_to_u32(value: usize) -> u32 {
    string::trim_size_t_to_u32(value)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_check_add32(base: usize, extra: usize) -> usize {
    string::check_add32(base, extra)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_rec_alloc_size(
    rec_header_size: usize,
    len: usize,
    out_string_len: *mut u32,
    out_alloc_size: *mut usize,
) -> bool {
    match string::string_rec_alloc_size(rec_header_size, len) {
        Some((slen, alloc)) => {
            if !out_string_len.is_null() {
                *out_string_len = slen;
            }
            if !out_alloc_size.is_null() {
                *out_alloc_size = alloc;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_can_reuse_rec_for_resize(
    is_unique: bool,
    current_len: u32,
    new_len: u32,
) -> bool {
    string::can_reuse_rec_for_resize(is_unique, current_len, new_len)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_can_insert_in_place(
    is_unique: bool,
    current_len: usize,
    insert_len: usize,
) -> bool {
    string::can_insert_in_place(is_unique, current_len, insert_len)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_clamp_remove_range(
    size: usize,
    offset: usize,
    length: usize,
    out_actual_len: *mut usize,
    out_tail: *mut usize,
) {
    let (actual_len, tail) = string::clamp_remove_range(size, offset, length);
    if !out_actual_len.is_null() {
        *out_actual_len = actual_len;
    }
    if !out_tail.is_null() {
        *out_tail = tail;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_tab_string(
    input: *const u8,
    input_len: usize,
    tab_cnt: i32,
    cb: Option<unsafe extern "C" fn(*const u8, usize, *mut std::ffi::c_void)>,
    ctx: *mut std::ffi::c_void,
) {
    let slice = if input_len == 0 || input.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(input, input_len)
    };
    let result = string::tab_string(slice, tab_cnt);
    if let Some(f) = cb {
        f(result.as_ptr(), result.len(), ctx);
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_utf16_to_utf8_clamped_len(
    src: *const u16,
    count: usize,
    out_units_used: *mut usize,
) -> usize {
    let slice = if count == 0 || src.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(src, count)
    };
    let (utf8_len, used) = string::utf16_to_utf8_clamped_len(slice);
    if !out_units_used.is_null() {
        *out_units_used = used;
    }
    utf8_len
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_utf16_to_utf8_write(
    src: *const u16,
    units_used: usize,
    dst: *mut u8,
    dst_len: usize,
) -> usize {
    if units_used == 0 || src.is_null() || dst.is_null() || dst_len == 0 {
        return 0;
    }
    let src_slice = std::slice::from_raw_parts(src, units_used);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    string::utf16_to_utf8_write(src_slice, dst_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_string_split_ranges(
    input: *const u8,
    input_len: usize,
    delims: *const u8,
    delims_len: usize,
    strict: bool,
    cb: Option<unsafe extern "C" fn(usize, usize, *mut std::ffi::c_void)>,
    ctx: *mut std::ffi::c_void,
) {
    let input_slice = if input_len == 0 || input.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(input, input_len)
    };
    let delims_slice = if delims_len == 0 || delims.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(delims, delims_len)
    };
    let ranges = string::str_split_ranges(input_slice, delims_slice, strict);
    if let Some(f) = cb {
        for (start, len) in ranges {
            f(start, len, ctx);
        }
    }
}

// ===========================================================================
// SkFlattenable
// ===========================================================================

static FLATTENABLE_REGISTRY: OnceLock<RwLock<FlattenableRegistry>> = OnceLock::new();

fn flattenable_registry() -> &'static RwLock<FlattenableRegistry> {
    FLATTENABLE_REGISTRY.get_or_init(|| RwLock::new(FlattenableRegistry::new()))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_flattenable_register(
    name: *const u8,
    name_len: usize,
    factory_token: usize,
) {
    if name.is_null() {
        return;
    }
    let name_slice = std::slice::from_raw_parts(name, name_len);
    let mut reg = flattenable_registry().write().unwrap();
    reg.register(name_slice, name as usize, factory_token);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_flattenable_finalize() {
    let mut reg = flattenable_registry().write().unwrap();
    reg.finalize();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_flattenable_name_to_factory(
    name: *const u8,
    name_len: usize,
) -> usize {
    if name.is_null() {
        return 0;
    }
    let name_slice = std::slice::from_raw_parts(name, name_len);
    let reg = flattenable_registry().read().unwrap();
    reg.name_to_factory(name_slice).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_flattenable_factory_to_name(factory_token: usize) -> usize {
    let reg = flattenable_registry().read().unwrap();
    reg.factory_to_name(factory_token).unwrap_or(0)
}

// ===========================================================================
// SkPaint & SkPaintPriv
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_pin_alphaf(a: f32) -> f32 {
    paint::pin_alphaf(a)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_nothing_to_draw(
    has_blend_mode: bool,
    blend_mode: u8,
    alpha: u8,
    cf_affects_alpha: bool,
    imf_affects_alpha: bool,
) -> bool {
    let bm = if has_blend_mode {
        Some(blend_mode)
    } else {
        None
    };
    paint::nothing_to_draw(bm, alpha, cf_affects_alpha, imf_affects_alpha)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_overwrites(
    has_paint: bool,
    override_opacity: u8,
    changes_alpha: bool,
    paint_alpha: u8,
    has_shader: bool,
    shader_is_opaque: bool,
    has_blend_mode: bool,
    blend_mode: u8,
) -> bool {
    let bm = if has_blend_mode {
        Some(blend_mode)
    } else {
        None
    };
    paint::paint_overwrites(
        has_paint,
        override_opacity,
        changes_alpha,
        paint_alpha,
        has_shader,
        shader_is_opaque,
        bm,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_should_dither(
    is_dither: bool,
    dst_color_type: i32,
    has_image_filter: bool,
    has_mask_filter: bool,
    has_shader: bool,
    shader_is_constant: bool,
) -> bool {
    paint::should_dither(
        is_dither,
        dst_color_type,
        has_image_filter,
        has_mask_filter,
        has_shader,
        shader_is_constant,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_pack_v68(
    anti_alias: bool,
    dither: bool,
    has_blend_mode: bool,
    blend_mode: u8,
    stroke_cap: u8,
    stroke_join: u8,
    style: u8,
    flat_flags: u8,
) -> u32 {
    let bm = if has_blend_mode {
        Some(blend_mode)
    } else {
        None
    };
    paint::pack_v68(anti_alias, dither, bm, stroke_cap, stroke_join, style, flat_flags)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_unpack_v68(packed: u32) -> PaintUnpackedV68 {
    paint::unpack_v68(packed)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_paint_outset_rect(
    src: *const Rect,
    radius: f32,
    out: *mut Rect,
) {
    *out = paint::outset_rect(*src, radius);
}

// ===========================================================================
// SkDescriptor
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_descriptor_compute_checksum(
    desc_bytes: *const u8,
    desc_len: usize,
) -> u32 {
    let slice = std::slice::from_raw_parts(desc_bytes, desc_len);
    descriptor::compute_checksum(slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_descriptor_add_entry(
    buf: *mut u8,
    buf_cap: usize,
    tag: u32,
    payload_len: usize,
    data: *const u8,
) -> usize {
    let buf_slice = std::slice::from_raw_parts_mut(buf, buf_cap);
    let data_opt = if data.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(data, payload_len))
    };
    descriptor::add_entry(buf_slice, tag, payload_len, data_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_descriptor_find_entry(
    buf: *const u8,
    buf_len: usize,
    tag: u32,
    out_data_offset: *mut usize,
    out_data_len: *mut u32,
) -> bool {
    let slice = std::slice::from_raw_parts(buf, buf_len);
    match descriptor::find_entry(slice, tag) {
        Some((off, len)) => {
            if !out_data_offset.is_null() {
                *out_data_offset = off;
            }
            if !out_data_len.is_null() {
                *out_data_len = len;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_descriptor_is_valid(
    buf: *const u8,
    buf_len: usize,
    rec_tag: u32,
    rec_size: usize,
) -> bool {
    let slice = std::slice::from_raw_parts(buf, buf_len);
    descriptor::is_valid(slice, rec_tag, rec_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_descriptor_equals(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
) -> bool {
    let a_slice = std::slice::from_raw_parts(a, a_len);
    let b_slice = std::slice::from_raw_parts(b, b_len);
    descriptor::descriptors_equal(a_slice, b_slice)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_descriptor_validate_header_for_read(
    header_length: u32,
    available: usize,
    out_body_len: *mut u32,
) -> bool {
    match descriptor::validate_header_for_read(header_length, available) {
        Some(body) => {
            if !out_body_len.is_null() {
                *out_body_len = body;
            }
            true
        }
        None => false,
    }
}

// ===========================================================================
// SkMask, SkMaskBuilder, SkMaskCache
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_compute_image_size(
    bounds: *const IRect,
    row_bytes: u32,
) -> usize {
    mask::compute_image_size(*bounds, row_bytes)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_compute_total_image_size(
    bounds: *const IRect,
    row_bytes: u32,
    format: u8,
) -> usize {
    mask::compute_total_image_size(*bounds, row_bytes, format)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_alloc_image_aligned_size(size: usize) -> usize {
    mask::alloc_image_aligned_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_get_addr_offset(
    bounds: *const IRect,
    row_bytes: u32,
    format: u8,
    x: i32,
    y: i32,
    out_offset: *mut isize,
) -> bool {
    match mask::get_addr_offset(*bounds, row_bytes, format, x, y) {
        Some(off) => {
            if !out_offset.is_null() {
                *out_offset = off;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_prepare_destination(
    radius_x: i32,
    radius_y: i32,
    src_bounds: *const IRect,
) -> mask::PreparedMaskDest {
    mask::prepare_destination(radius_x, radius_y, *src_bounds)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_rects_blur_key_sizes(
    rects: *const Rect,
    count: usize,
    out_sizes: *mut [Size; 4],
) {
    let slice = std::slice::from_raw_parts(rects, count);
    *out_sizes = mask::rects_blur_key_sizes(slice);
}

// ===========================================================================
// SkBlurMask
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_convert_radius_to_sigma(radius: f32) -> f32 {
    blur_mask::convert_radius_to_sigma(radius)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_convert_sigma_to_radius(sigma: f32) -> f32 {
    blur_mask::convert_sigma_to_radius(sigma)
}

#[inline]
fn src_row_bytes_needed(format: u8, sw: usize) -> usize {
    match format {
        mask::MASK_FORMAT_BW => (sw + 7) >> 3,
        mask::MASK_FORMAT_A8 => sw,
        mask::MASK_FORMAT_LCD16 => sw * 2,
        mask::MASK_FORMAT_ARGB32 => sw * 4,
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_merge_src_with_blur(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    src_format: u8,
    blur: *const u8,
    blur_rb: usize,
    sw: usize,
    sh: usize,
    a32_shift: u32,
) {
    if sw == 0 || sh == 0 {
        return;
    }
    let dst_len = (sh - 1) * dst_rb + sw;
    let src_len = (sh - 1) * src_rb + src_row_bytes_needed(src_format, sw);
    let blur_len = (sh - 1) * blur_rb + sw;
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let src_slice = std::slice::from_raw_parts(src, src_len);
    let blur_slice = std::slice::from_raw_parts(blur, blur_len);
    blur_mask::merge_src_with_blur(
        dst_slice,
        dst_rb,
        src_slice,
        src_rb,
        src_format,
        blur_slice,
        blur_rb,
        sw,
        sh,
        a32_shift,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_clamp_solid(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    src_format: u8,
    sw: usize,
    sh: usize,
    a32_shift: u32,
) {
    if sw == 0 || sh == 0 {
        return;
    }
    let dst_len = (sh - 1) * dst_rb + sw;
    let src_len = (sh - 1) * src_rb + src_row_bytes_needed(src_format, sw);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let src_slice = std::slice::from_raw_parts(src, src_len);
    blur_mask::clamp_solid_with_orig(
        dst_slice, dst_rb, src_slice, src_rb, src_format, sw, sh, a32_shift,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_clamp_outer(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    src_format: u8,
    sw: usize,
    sh: usize,
    a32_shift: u32,
) {
    if sw == 0 || sh == 0 {
        return;
    }
    let dst_len = (sh - 1) * dst_rb + sw;
    let src_len = (sh - 1) * src_rb + src_row_bytes_needed(src_format, sw);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    let src_slice = std::slice::from_raw_parts(src, src_len);
    blur_mask::clamp_outer_with_orig(
        dst_slice, dst_rb, src_slice, src_rb, src_format, sw, sh, a32_shift,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_compute_blur_profile(
    profile: *mut u8,
    size: usize,
    sigma: f32,
) {
    if size == 0 || profile.is_null() {
        return;
    }
    let slice = std::slice::from_raw_parts_mut(profile, size);
    blur_mask::compute_blur_profile(slice, sigma);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_profile_lookup(
    profile: *const u8,
    loc: i32,
    blurred_width: i32,
    sharp_width: i32,
) -> u8 {
    // Compute exact index bounds safely: ox = max((|2*loc + 1 - blurred_width| - sharp_width) >> 1, 0)
    let diff = ((loc << 1) + 1) - blurred_width;
    let dx = diff.abs() - sharp_width;
    let ox = (dx >> 1).max(0) as usize;
    let slice = std::slice::from_raw_parts(profile, ox + 1);
    blur_mask::profile_lookup(slice, loc, blurred_width, sharp_width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_compute_blurred_scanline(
    pixels: *mut u8,
    profile: *const u8,
    width: usize,
    sigma: f32,
) {
    if width == 0 || pixels.is_null() {
        return;
    }
    let profile_size = skia_rs_core_types::scalar_ceil_to_int(6.0_f32 * sigma).max(0) as usize;
    let pix_slice = std::slice::from_raw_parts_mut(pixels, width);
    let prof_slice = if profile_size == 0 || profile.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(profile, profile_size)
    };
    blur_mask::compute_blurred_scanline(pix_slice, prof_slice, sigma);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_blur_rect_render_full(
    dst: *mut u8,
    dst_w: usize,
    dst_h: usize,
    sigma: f32,
    style: i32,
    pad: usize,
    sw: usize,
) {
    if dst_w == 0 || dst_h == 0 || dst.is_null() {
        return;
    }
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_w * dst_h);
    blur_mask::blur_rect_render_full(dst_slice, dst_w, dst_h, sigma, style, pad, sw);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_crop_inner_blur_rect(
    full_blur: *const u8,
    dst_w: usize,
    dst_h: usize,
    pad: usize,
    inner_dst: *mut u8,
    sw: usize,
    sh: usize,
) {
    if sw == 0 || sh == 0 || full_blur.is_null() || inner_dst.is_null() {
        return;
    }
    let full_slice = std::slice::from_raw_parts(full_blur, dst_w * dst_h);
    let inner_slice = std::slice::from_raw_parts_mut(inner_dst, sw * sh);
    blur_mask::crop_inner_blur_rect(full_slice, dst_w, pad, inner_slice, sw, sh);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_ground_truth_pad(sigma: f32) -> i32 {
    blur_mask::ground_truth_pad(sigma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_blur_mask_ground_truth_convolve(
    sigma: f32,
    src: *const u8,
    src_w: usize,
    src_h: usize,
    src_rb: usize,
    dst: *mut u8,
    dst_w: usize,
    dst_h: usize,
    style: i32,
) {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 || src.is_null() || dst.is_null() {
        return;
    }
    let src_len = (src_h * src_w).max((src_h - 1) * src_rb + src_w);
    let dst_len = dst_h * dst_w;
    let src_slice = std::slice::from_raw_parts(src, src_len);
    let dst_slice = std::slice::from_raw_parts_mut(dst, dst_len);
    blur_mask::blur_ground_truth_convolve(
        sigma, src_slice, src_w, src_h, src_rb, dst_slice, dst_w, style,
    );
}

// ===========================================================================
// SkGaussFilter
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_gauss_filter_calculate_bessel_factors(
    sigma: f64,
    basis: *mut [f64; gauss_filter::GAUSS_ARRAY_MAX],
) -> i32 {
    gauss_filter::calculate_bessel_factors(sigma, &mut *basis)
}

// ===========================================================================
// SkMaskGamma
// ===========================================================================

#[inline]
fn decode_luminance_mode(mode: u8) -> LuminanceMode {
    match mode {
        0 => LuminanceMode::SRgb,
        1 => LuminanceMode::Linear,
        _ => LuminanceMode::Gamma,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_gamma_mode_from_gamma(gamma: f32) -> u8 {
    LuminanceMode::from_gamma(gamma) as u8
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_gamma_to_luma(
    mode: u8,
    gamma: f32,
    luminance: f32,
) -> f32 {
    mask_gamma::to_luma(decode_luminance_mode(mode), gamma, luminance)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_gamma_from_luma(mode: u8, gamma: f32, luma: f32) -> f32 {
    mask_gamma::from_luma(decode_luminance_mode(mode), gamma, luma)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_gamma_apply_contrast(srca: f32, contrast: f32) -> f32 {
    mask_gamma::apply_contrast(srca, contrast)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_mask_gamma_build_correcting_lut(
    table: *mut [u8; 256],
    src_i: u8,
    contrast: f32,
    mode: u8,
    dst_gamma: f32,
) {
    mask_gamma::build_correcting_lut(
        &mut *table,
        src_i,
        contrast,
        decode_luminance_mode(mode),
        dst_gamma,
    );
}
