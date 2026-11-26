//! FFI exports for `skia-rs-text` (Phase 1 Layer 5.1).

use skia_rs_geometry::{IPoint, IRect, Matrix, Point, Rect, Vector};
use skia_rs_text::{
    font::{
        self, FontSerialPackResult, FontSerialUnpackResult, SetupForAsPathsResult,
    },
    font_descriptor::{
        self, DescriptorStyleAxes, SerializeBehavior, VariationCoordinate,
    },
    glyph::{
        self, GlyphPositionRoundingSpec, PackedGlyphMetrics, PathSegment, UnpackedGlyphMetrics,
    },
    scaler_context::{
        self, MakeRecParams, PathMaskRasterPlan, PreMatrixScale, SaturatedGlyphBounds,
        ScalerContextRec,
    },
    sfnt::{self, SharedHeaderProbe, TableRangeResult},
    strike::{self, StrikePurgeTargets},
    text_blob::{
        self, DirectMaskQuantized, InterceptSetup, RunDeserialSizes,
    },
    AxisAlignment, FontMetrics, FontStyle, GlyphPositioning, MaskFormat, TextAlign, TextEncoding,
};
use std::slice;

// ============================================================================
// SkFont / SkFont_serial / SkFontMetricsPriv / SkFontPriv
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_valid_size(size: f32) -> f32 {
    font::valid_size(size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_set_clear_mask(bits: u8, cond: bool, mask: u8) -> u8 {
    font::set_clear_mask(bits, cond, mask)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_has_some_aa(edging: u8) -> bool {
    font::has_some_anti_aliasing(edging)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_setup_for_as_paths(
    flags: u8,
    edging: u8,
    size: f32,
    out: *mut SetupForAsPathsResult,
) {
    if out.is_null() {
        return;
    }
    *out = font::setup_for_as_paths(flags, edging, size);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_get_bounds(
    size: f32,
    scale_x: f32,
    skew_x: f32,
    typeface_bounds: *const Rect,
    out: *mut Rect,
) {
    if typeface_bounds.is_null() || out.is_null() {
        return;
    }
    *out = font::get_font_bounds(size, scale_x, skew_x, *typeface_bounds);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_approx_transformed_size(
    font_size: f32,
    matrix: *const Matrix,
    text_location: *const Point,
) -> f32 {
    if matrix.is_null() || text_location.is_null() {
        return 0.0;
    }
    font::approximate_transformed_text_size(font_size, &*matrix, *text_location)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_count_elements(
    text: *const u8,
    byte_length: usize,
    encoding: i32,
) -> usize {
    let Some(enc) = TextEncoding::from_i32(encoding) else {
        return 0;
    };
    if text.is_null() {
        return match enc {
            TextEncoding::Utf8 | TextEncoding::Utf16 => (-1isize) as usize,
            TextEncoding::Utf32 => byte_length >> 2,
            TextEncoding::GlyphId => byte_length >> 1,
        };
    }
    let bytes = slice::from_raw_parts(text, byte_length);
    let aligned2 = ((text as usize) & 1) == 0;
    font::count_text_elements(bytes, enc, aligned2)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_decode_to_utf32(
    text: *const u8,
    byte_length: usize,
    encoding: i32,
    out: *mut i32,
    out_len: usize,
) -> usize {
    let Some(enc) = TextEncoding::from_i32(encoding) else {
        return 0;
    };
    if text.is_null() || out.is_null() || byte_length == 0 || out_len == 0 {
        return 0;
    }
    let bytes = slice::from_raw_parts(text, byte_length);
    let dst = slice::from_raw_parts_mut(out, out_len);
    font::decode_text_to_utf32(bytes, enc, dst)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_measure_glyphs(
    advances_x: *const f32,
    rects: *const Rect,
    count: usize,
    strike_to_source_scale: f32,
    out_bounds: *mut Rect,
) -> f32 {
    if count == 0 || advances_x.is_null() {
        if !out_bounds.is_null() {
            *out_bounds = Rect::make_empty();
        }
        return 0.0;
    }
    let adv = slice::from_raw_parts(advances_x, count);
    let rect_slice = if !rects.is_null() && !out_bounds.is_null() {
        Some(slice::from_raw_parts(rects, count))
    } else {
        None
    };
    let bounds_opt = if !out_bounds.is_null() {
        Some(&mut *out_bounds)
    } else {
        None
    };
    font::measure_glyphs(adv, rect_slice, strike_to_source_scale, bounds_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scale_rect(r: *const Rect, s: f32, out: *mut Rect) {
    if r.is_null() || out.is_null() {
        return;
    }
    *out = font::scale_rect(*r, s);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_accumulate_positions(
    advances: *const Point,
    count: usize,
    scale: f32,
    origin: *const Point,
    out_pos: *mut Point,
) {
    if count == 0 || advances.is_null() || origin.is_null() || out_pos.is_null() {
        return;
    }
    let adv = slice::from_raw_parts(advances, count);
    let dst = slice::from_raw_parts_mut(out_pos, count);
    font::accumulate_positions(adv, scale, *origin, dst);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_accumulate_x_positions(
    advances_x: *const f32,
    count: usize,
    scale: f32,
    origin: f32,
    out_xpos: *mut f32,
) {
    if count == 0 || advances_x.is_null() || out_xpos.is_null() {
        return;
    }
    let adv = slice::from_raw_parts(advances_x, count);
    let dst = slice::from_raw_parts_mut(out_xpos, count);
    font::accumulate_x_positions(adv, scale, origin, dst);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyphs_to_unichars(
    glyphs: *const u16,
    count: usize,
    glyph_to_unicode: *const i32,
    num_glyphs_in_typeface: usize,
    out_unichars: *mut i32,
) {
    if count == 0 || glyphs.is_null() || out_unichars.is_null() {
        return;
    }
    let g = slice::from_raw_parts(glyphs, count);
    let map = if glyph_to_unicode.is_null() || num_glyphs_in_typeface == 0 {
        &[]
    } else {
        slice::from_raw_parts(glyph_to_unicode, num_glyphs_in_typeface)
    };
    let dst = slice::from_raw_parts_mut(out_unichars, count);
    font::glyphs_to_unichars(g, map, dst);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scale_font_metrics(metrics: *mut FontMetrics, scale: f32) {
    if metrics.is_null() {
        return;
    }
    font::scale_font_metrics(&mut *metrics, scale);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_metrics_spacing(metrics: *const FontMetrics) -> f32 {
    if metrics.is_null() {
        return 0.0;
    }
    (*metrics).spacing()
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_pack_serial(
    size: f32,
    scale_x: f32,
    skew_x: f32,
    flags: u8,
    edging: u8,
    hinting: u8,
    has_typeface: bool,
    out: *mut FontSerialPackResult,
) {
    if out.is_null() {
        return;
    }
    *out = font::pack_font_serial(size, scale_x, skew_x, flags, edging, hinting, has_typeface);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_unpack_serial(
    packed: u32,
    out: *mut FontSerialUnpackResult,
) {
    if out.is_null() {
        return;
    }
    *out = font::unpack_font_serial(packed);
}

// ============================================================================
// SkFontStream (SFNT / TTC)
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_sfnt_count_ttc_entries(
    header_bytes: *const u8,
    len: usize,
) -> i32 {
    if header_bytes.is_null() {
        return 0;
    }
    sfnt::count_ttc_entries(slice::from_raw_parts(header_bytes, len))
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_sfnt_probe_shared_header(
    header_bytes: *const u8,
    len: usize,
    out: *mut SharedHeaderProbe,
) -> bool {
    if header_bytes.is_null() || out.is_null() {
        return false;
    }
    match sfnt::probe_shared_header(slice::from_raw_parts(header_bytes, len)) {
        Some(p) => {
            *out = p;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_sfnt_parse_num_tables(
    sfnt_header_bytes: *const u8,
    len: usize,
    out_count: *mut u16,
) -> bool {
    if sfnt_header_bytes.is_null() || out_count.is_null() {
        return false;
    }
    match sfnt::parse_sfnt_num_tables(slice::from_raw_parts(sfnt_header_bytes, len)) {
        Some(n) => {
            *out_count = n;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_sfnt_parse_ttc_offset(
    ttc_header_bytes: *const u8,
    len: usize,
    ttc_index: usize,
    out_offset: *mut u32,
) -> bool {
    if ttc_header_bytes.is_null() || out_offset.is_null() {
        return false;
    }
    match sfnt::parse_ttc_entry_offset(slice::from_raw_parts(ttc_header_bytes, len), ttc_index) {
        Some(off) => {
            *out_offset = off;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_sfnt_extract_tags(
    dir_bytes: *const u8,
    dir_len: usize,
    count: usize,
    out_tags: *mut u32,
    out_len: usize,
) -> usize {
    if dir_bytes.is_null() || count == 0 {
        return count;
    }
    let dir = slice::from_raw_parts(dir_bytes, dir_len);
    let tags = if out_tags.is_null() || out_len == 0 {
        &mut []
    } else {
        slice::from_raw_parts_mut(out_tags, out_len)
    };
    sfnt::extract_table_tags(dir, count, tags)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_sfnt_find_table_range(
    dir_bytes: *const u8,
    dir_len: usize,
    count: usize,
    tag: u32,
    offset: usize,
    length: usize,
    out: *mut TableRangeResult,
) {
    if out.is_null() {
        return;
    }
    if dir_bytes.is_null() || count == 0 {
        *out = TableRangeResult::default();
        return;
    }
    let dir = slice::from_raw_parts(dir_bytes, dir_len);
    *out = sfnt::find_table_range(dir, count, tag, offset, length);
}

// ============================================================================
// SkFontDescriptor / SkFontMgr / SkTypeface / SkTypefaceCache
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_style_width_for_axis(width: f32) -> i32 {
    font_descriptor::font_style_width_for_width_axis_value(width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_width_axis_for_style(width: i32) -> f32 {
    font_descriptor::font_width_axis_value_for_style_width(width)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_descriptor_unpack_style_bits(
    style_bits: usize,
    out: *mut DescriptorStyleAxes,
) {
    if out.is_null() {
        return;
    }
    *out = font_descriptor::unpack_descriptor_style_bits(style_bits);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_descriptor_pack_style(
    weight: i32,
    width: i32,
    slant: u8,
    out_style_bits: *mut u32,
    out_axes: *mut DescriptorStyleAxes,
) {
    let style = FontStyle {
        weight,
        width,
        slant,
    };
    let (bits, axes) = font_descriptor::pack_descriptor_style(style);
    if !out_style_bits.is_null() {
        *out_style_bits = bits;
    }
    if !out_axes.is_null() {
        *out_axes = axes;
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_style_from_axes(
    axes: *const DescriptorStyleAxes,
    out: *mut FontStyle,
) {
    if axes.is_null() || out.is_null() {
        return;
    }
    *out = font_descriptor::font_style_from_axes(*axes);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_font_style_from_model(
    coords: *const VariationCoordinate,
    count: usize,
    out: *mut FontStyle,
) {
    if out.is_null() {
        return;
    }
    let slice_coords = if coords.is_null() || count == 0 {
        &[]
    } else {
        slice::from_raw_parts(coords, count)
    };
    *out = font_descriptor::font_style_from_model(slice_coords);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_set_model_from_style(
    weight: i32,
    width: i32,
    slant: u8,
    out_model: *mut VariationCoordinate,
) {
    if out_model.is_null() {
        return;
    }
    let model = font_descriptor::set_model_from_font_style(FontStyle {
        weight,
        width,
        slant,
    });
    let dst = slice::from_raw_parts_mut(out_model, 4);
    dst.copy_from_slice(&model);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_css3_match_best_index(
    pattern_weight: i32,
    pattern_width: i32,
    pattern_slant: u8,
    candidates: *const FontStyle,
    count: usize,
) -> usize {
    if candidates.is_null() || count == 0 {
        return 0;
    }
    let pat = FontStyle {
        weight: pattern_weight,
        width: pattern_width,
        slant: pattern_slant,
    };
    let cands = slice::from_raw_parts(candidates, count);
    font_descriptor::match_style_css3_best_index(pat, cands)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_should_serialize_typeface_data(
    behavior: i32,
    is_local_data: bool,
) -> bool {
    SerializeBehavior::from_i32(behavior)
        .map(|b| b.should_serialize_data(is_local_data))
        .unwrap_or(false)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_os2_fstype_to_advanced_flags(b0: u8, b1: u8) -> u8 {
    font_descriptor::os2_fstype_to_advanced_flags([b0, b1])
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_next_typeface_id_checked(
    id: u64,
    out_id: *mut u32,
) -> bool {
    match font_descriptor::next_typeface_id_checked(id) {
        Some(v) => {
            if !out_id.is_null() {
                *out_id = v;
            }
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_typeface_cache_purge_count(
    current_size: i32,
    limit: i32,
) -> i32 {
    font_descriptor::typeface_cache_purge_count(current_size, limit)
}

// ============================================================================
// SkGlyph / SkGlyphDigest / SkGlyphPositionRoundingSpec
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_format_alignment(format: u8) -> usize {
    MaskFormat::from_u8(format)
        .map(glyph::format_alignment)
        .unwrap_or(1)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_row_bytes(width: i32, format: u8) -> usize {
    MaskFormat::from_u8(format)
        .map(|f| glyph::format_row_bytes(width, f))
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_image_size(
    width: u16,
    height: u16,
    format: u8,
) -> usize {
    MaskFormat::from_u8(format)
        .map(|f| glyph::glyph_image_size(width, height, f))
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_pack_metrics(
    packed_id: u32,
    advance_x: f32,
    advance_y: f32,
    width: u16,
    height: u16,
    left: i16,
    top: i16,
    mask_format: u8,
    out: *mut PackedGlyphMetrics,
) {
    if out.is_null() {
        return;
    }
    *out = glyph::pack_glyph_metrics(
        packed_id,
        advance_x,
        advance_y,
        width,
        height,
        left,
        top,
        mask_format,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_unpack_metrics(
    packed: *const PackedGlyphMetrics,
    out: *mut UnpackedGlyphMetrics,
) -> bool {
    if packed.is_null() || out.is_null() {
        return false;
    }
    match glyph::unpack_glyph_metrics(*packed) {
        Some(u) => {
            *out = u;
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_drawable_tag_allowed(tag: u32) -> bool {
    glyph::is_allowed_glyph_drawable_tag(tag)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_digest_init_actions(is_empty: bool) -> u32 {
    glyph::init_digest_actions(is_empty)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_fits_in_atlas(width: u16, height: u16) -> bool {
    glyph::fits_in_atlas(width, height)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_rounding_spec(
    is_subpixel: bool,
    axis_alignment: u32,
    out: *mut GlyphPositionRoundingSpec,
) {
    if out.is_null() {
        return;
    }
    let aa = AxisAlignment::from_u32(axis_alignment).unwrap_or(AxisAlignment::None);
    *out = GlyphPositionRoundingSpec::new(is_subpixel, aa);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyph_calculate_path_gap(
    top_offset: f32,
    bottom_offset: f32,
    segments: *const PathSegment,
    count: usize,
    out_interval: *mut f32,
) {
    if out_interval.is_null() {
        return;
    }
    let segs = if segments.is_null() || count == 0 {
        &[]
    } else {
        slice::from_raw_parts(segments, count)
    };
    let [l, r] = glyph::calculate_path_gap(top_offset, bottom_offset, segs);
    *out_interval = l;
    *out_interval.add(1) = r;
}

// ============================================================================
// SkScalerContext / SkScalerContextRec
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_preprocess_post_filter(
    rec: *mut ScalerContextRec,
    has_mask_filter: bool,
) {
    if rec.is_null() {
        return;
    }
    (*rec).preprocess_post_filter(has_mask_filter);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_make(
    params: *const MakeRecParams,
    device_matrix: *const Matrix,
    out_rec: *mut ScalerContextRec,
) {
    if params.is_null() || device_matrix.is_null() || out_rec.is_null() {
        return;
    }
    *out_rec = scaler_context::make_scaler_context_rec(&*params, &*device_matrix);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_get_matrix_from_2x2(
    rec: *const ScalerContextRec,
    out: *mut Matrix,
) {
    if rec.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).get_matrix_from_2x2();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_get_local_matrix(
    rec: *const ScalerContextRec,
    out: *mut Matrix,
) {
    if rec.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).get_local_matrix();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_get_single_matrix(
    rec: *const ScalerContextRec,
    out: *mut Matrix,
) {
    if rec.is_null() || out.is_null() {
        return;
    }
    *out = (*rec).get_single_matrix();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_compute_matrices(
    rec: *const ScalerContextRec,
    pre_matrix_scale: i32,
    out_s: *mut Vector,
    out_s_a: *mut Matrix,
    out_gs_a: *mut Matrix,
    out_g_inv: *mut Matrix,
    out_a: *mut Matrix,
) -> bool {
    if rec.is_null() || out_s.is_null() || out_s_a.is_null() {
        return false;
    }
    let pms = PreMatrixScale::from_i32(pre_matrix_scale).unwrap_or(PreMatrixScale::Full);
    let gs_a_opt = if out_gs_a.is_null() {
        None
    } else {
        Some(&mut *out_gs_a)
    };
    let g_inv_opt = if out_g_inv.is_null() {
        None
    } else {
        Some(&mut *out_g_inv)
    };
    let a_opt = if out_a.is_null() {
        None
    } else {
        Some(&mut *out_a)
    };
    (*rec).compute_matrices(pms, &mut *out_s, &mut *out_s_a, gs_a_opt, g_inv_opt, a_opt)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_axis_alignment(
    rec: *const ScalerContextRec,
) -> u32 {
    if rec.is_null() {
        return AxisAlignment::None as u32;
    }
    (*rec).compute_axis_alignment_for_h_text() as u32
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_set_luminance_color(
    rec: *mut ScalerContextRec,
    color: u32,
) {
    if rec.is_null() {
        return;
    }
    (*rec).set_luminance_color(color);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_rec_use_stroke_for_fake_bold(
    rec: *mut ScalerContextRec,
) {
    if rec.is_null() {
        return;
    }
    (*rec).use_stroke_for_fake_bold();
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_saturate_bounds_rect(
    rect: *const Rect,
    out: *mut SaturatedGlyphBounds,
) {
    if rect.is_null() || out.is_null() {
        return;
    }
    *out = scaler_context::saturate_glyph_bounds_rect(*rect);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_saturate_bounds_irect(
    irect: *const IRect,
    out: *mut SaturatedGlyphBounds,
) {
    if irect.is_null() || out.is_null() {
        return;
    }
    *out = scaler_context::saturate_glyph_bounds_irect(*irect);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_generate_metrics_from_path_bounds(
    mask_format: u8,
    bounds: *const Rect,
    vertical_lcd: bool,
    a8_from_lcd: bool,
    hairline: bool,
    out: *mut SaturatedGlyphBounds,
) {
    if bounds.is_null() || out.is_null() {
        return;
    }
    *out = scaler_context::generate_metrics_from_path_bounds(
        mask_format,
        *bounds,
        vertical_lcd,
        a8_from_lcd,
        hairline,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_plan_image_from_path(
    bounds: *const IRect,
    format: u8,
    vertical_lcd: bool,
    a8_from_lcd: bool,
    samples_per_pixel: i32,
    out: *mut PathMaskRasterPlan,
) {
    if bounds.is_null() || out.is_null() {
        return;
    }
    *out = scaler_context::plan_generate_image_from_path(
        *bounds,
        format,
        vertical_lcd,
        a8_from_lcd,
        samples_per_pixel,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_apply_lut_a8(
    dst: *mut u8,
    row_bytes: usize,
    width: usize,
    height: usize,
    lut: *const u8,
) {
    if dst.is_null() || lut.is_null() || width == 0 || height == 0 {
        return;
    }
    let total = row_bytes.saturating_mul(height);
    let dst_slice = slice::from_raw_parts_mut(dst, total);
    let lut_slice = slice::from_raw_parts(lut, 256);
    let lut_arr: &[u8; 256] = match lut_slice.try_into() {
        Ok(a) => a,
        Err(_) => return,
    };
    scaler_context::apply_lut_to_a8_mask(dst_slice, row_bytes, width, height, lut_arr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_pack_a8_to_a1(
    dst: *mut u8,
    dst_rb: usize,
    src: *const u8,
    src_rb: usize,
    width: usize,
    height: usize,
) {
    if dst.is_null() || src.is_null() || width == 0 || height == 0 {
        return;
    }
    let dst_slice = slice::from_raw_parts_mut(dst, dst_rb.saturating_mul(height));
    let src_slice = slice::from_raw_parts(src, src_rb.saturating_mul(height));
    scaler_context::pack_a8_to_a1(dst_slice, dst_rb, src_slice, src_rb, width, height);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_pack_4xh_to_mask(
    src: *const u8,
    src_width: usize,
    src_height: usize,
    src_rb: usize,
    dst: *mut u8,
    dst_rb: usize,
    dst_byte_size: usize,
    to_a8: bool,
    preblend_r: *const u8,
    preblend_g: *const u8,
    preblend_b: *const u8,
    do_bgr: bool,
    do_vert: bool,
) {
    if src.is_null() || dst.is_null() || src_height == 0 {
        return;
    }
    let src_slice = slice::from_raw_parts(src, src_rb.saturating_mul(src_height));
    let dst_slice = slice::from_raw_parts_mut(dst, dst_byte_size);
    let lr: Option<&[u8; 256]> = if preblend_r.is_null() {
        None
    } else {
        slice::from_raw_parts(preblend_r, 256).try_into().ok()
    };
    let lg: Option<&[u8; 256]> = if preblend_g.is_null() {
        None
    } else {
        slice::from_raw_parts(preblend_g, 256).try_into().ok()
    };
    let lb: Option<&[u8; 256]> = if preblend_b.is_null() {
        None
    } else {
        slice::from_raw_parts(preblend_b, 256).try_into().ok()
    };
    scaler_context::pack_4xh_to_mask(
        src_slice, src_width, src_height, src_rb, dst_slice, dst_rb, to_a8, lr, lg, lb, do_bgr,
        do_vert,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_pack_3xh_to_mask(
    src: *const u8,
    src_width: usize,
    src_height: usize,
    src_rb: usize,
    dst: *mut u8,
    dst_rb: usize,
    dst_byte_size: usize,
    preblend_r: *const u8,
    preblend_g: *const u8,
    preblend_b: *const u8,
    do_bgr: bool,
    do_vert: bool,
) {
    if src.is_null() || dst.is_null() || src_height == 0 {
        return;
    }
    let src_slice = slice::from_raw_parts(src, src_rb.saturating_mul(src_height));
    let dst_slice = slice::from_raw_parts_mut(dst, dst_byte_size);
    let lr: Option<&[u8; 256]> = if preblend_r.is_null() {
        None
    } else {
        slice::from_raw_parts(preblend_r, 256).try_into().ok()
    };
    let lg: Option<&[u8; 256]> = if preblend_g.is_null() {
        None
    } else {
        slice::from_raw_parts(preblend_g, 256).try_into().ok()
    };
    let lb: Option<&[u8; 256]> = if preblend_b.is_null() {
        None
    } else {
        slice::from_raw_parts(preblend_b, 256).try_into().ok()
    };
    scaler_context::pack_3xh_to_mask(
        src_slice, src_width, src_height, src_rb, dst_slice, dst_rb, lr, lg, lb, do_bgr, do_vert,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_scaler_clip_copy_filtered_mask(
    src: *const u8,
    src_total_bytes: usize,
    src_bounds: *const IRect,
    src_rb: usize,
    src_format: u8,
    src_plane_bytes: usize,
    dst: *mut u8,
    dst_total_bytes: usize,
    dst_bounds: *const IRect,
    dst_rb: usize,
    dst_format: u8,
    dst_plane_bytes: usize,
) {
    if src.is_null() || dst.is_null() || src_bounds.is_null() || dst_bounds.is_null() {
        return;
    }
    let src_slice = slice::from_raw_parts(src, src_total_bytes);
    let dst_slice = slice::from_raw_parts_mut(dst, dst_total_bytes);
    scaler_context::clip_and_copy_filtered_mask(
        src_slice,
        *src_bounds,
        src_rb,
        src_format,
        src_plane_bytes,
        dst_slice,
        *dst_bounds,
        dst_rb,
        dst_format,
        dst_plane_bytes,
    );
}

// ============================================================================
// SkStrike / SkStrikeCache / SkStrikeSpec / SkStrikeRef
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_strike_should_draw_as_path(
    paint_style: u8,
    stroke_width: f32,
    font_size: f32,
    font_scale_x: f32,
    font_skew_x: f32,
    view_matrix: *const Matrix,
) -> bool {
    if view_matrix.is_null() {
        return false;
    }
    strike::should_draw_as_path(
        paint_style,
        stroke_width,
        font_size,
        font_scale_x,
        font_skew_x,
        &*view_matrix,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_strike_purge_targets(
    total_memory_used: usize,
    cache_size_limit: usize,
    min_bytes_needed: usize,
    cache_count: i32,
    cache_count_limit: i32,
    pinner_count: i32,
    check_pinners: bool,
    out: *mut StrikePurgeTargets,
) {
    if out.is_null() {
        return;
    }
    *out = strike::compute_strike_purge_targets(
        total_memory_used,
        cache_size_limit,
        min_bytes_needed,
        cache_count,
        cache_count_limit,
        pinner_count,
        check_pinners,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_strike_sanitize_font_name(bytes: *mut u8, len: usize) {
    if bytes.is_null() || len == 0 {
        return;
    }
    strike::sanitize_font_name_for_dump(slice::from_raw_parts_mut(bytes, len));
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_strike_strided_span_len(count: u32, stride_32: u32) -> usize {
    strike::strided_span_len(count, stride_32)
}

// ============================================================================
// SkTextBlob / GlyphRun / SkGlyphRunPainter / SkTextUtils
// ============================================================================

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_scalars_per_glyph(positioning: u8) -> u32 {
    GlyphPositioning::from_u8(positioning)
        .map(text_blob::scalars_per_glyph)
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_run_pos_count(
    glyph_count: u32,
    positioning: u8,
    out_ok: *mut bool,
) -> usize {
    let pos = GlyphPositioning::from_u8(positioning).unwrap_or(GlyphPositioning::Default);
    match text_blob::run_record_pos_count(glyph_count, pos) {
        Some(v) => {
            if !out_ok.is_null() {
                *out_ok = true;
            }
            v
        }
        None => {
            if !out_ok.is_null() {
                *out_ok = false;
            }
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_run_storage_size(
    run_record_size: usize,
    ptr_size: usize,
    glyph_count: u32,
    text_size: u32,
    positioning: u8,
    out_ok: *mut bool,
) -> usize {
    let pos = GlyphPositioning::from_u8(positioning).unwrap_or(GlyphPositioning::Default);
    match text_blob::run_record_storage_size(run_record_size, ptr_size, glyph_count, text_size, pos)
    {
        Some(v) => {
            if !out_ok.is_null() {
                *out_ok = true;
            }
            v
        }
        None => {
            if !out_ok.is_null() {
                *out_ok = false;
            }
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_tight_run_bounds(
    positioning: u8,
    glyph_bounds: *const Rect,
    glyph_count: usize,
    scalars: *const f32,
    offset: *const Point,
    out: *mut Rect,
) {
    if out.is_null() || offset.is_null() {
        return;
    }
    let Some(pos) = GlyphPositioning::from_u8(positioning) else {
        *out = Rect::make_empty();
        return;
    };
    if glyph_count == 0 || glyph_bounds.is_null() || scalars.is_null() {
        *out = Rect::make_empty().make_offset((*offset).x, (*offset).y);
        return;
    }
    let gb = slice::from_raw_parts(glyph_bounds, glyph_count);
    let spg = text_blob::scalars_per_glyph(pos) as usize;
    let sc = slice::from_raw_parts(scalars, glyph_count * spg);
    *out = text_blob::tight_run_bounds(pos, gb, sc, *offset);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_conservative_run_bounds(
    positioning: u8,
    glyph_count: usize,
    scalars: *const f32,
    font_bounds: *const Rect,
    offset: *const Point,
    out: *mut Rect,
) {
    if out.is_null() || font_bounds.is_null() || offset.is_null() || scalars.is_null() || glyph_count == 0 {
        return;
    }
    let Some(pos) = GlyphPositioning::from_u8(positioning) else {
        *out = Rect::make_empty();
        return;
    };
    let spg = text_blob::scalars_per_glyph(pos) as usize;
    let sc = slice::from_raw_parts(scalars, glyph_count * spg);
    *out = text_blob::conservative_run_bounds(pos, glyph_count, sc, *font_bounds, *offset);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_validate_deserial_run_sizes(
    glyph_count: i32,
    positioning: u8,
    extended: bool,
    text_size: i32,
    available_bytes: usize,
    out: *mut RunDeserialSizes,
) {
    if out.is_null() {
        return;
    }
    let Some(pos) = GlyphPositioning::from_u8(positioning) else {
        *out = RunDeserialSizes::default();
        return;
    };
    *out = text_blob::validate_deserialized_run_sizes(
        glyph_count,
        pos,
        extended,
        text_size,
        available_bytes,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_blob_validate_clusters(
    clusters: *const u32,
    count: usize,
    text_size: u32,
) -> bool {
    if clusters.is_null() || count == 0 {
        return true;
    }
    text_blob::validate_clusters(slice::from_raw_parts(clusters, count), text_size)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyphrun_conservative_bounds(
    font_bounds: *const Rect,
    positions: *const Point,
    pos_count: usize,
    scaled_rotations: *const Vector,
    sr_count: usize,
    out: *mut Rect,
) {
    if font_bounds.is_null() || positions.is_null() || out.is_null() {
        return;
    }
    let pos = slice::from_raw_parts(positions, pos_count);
    let sr = if scaled_rotations.is_null() || sr_count == 0 {
        &[]
    } else {
        slice::from_raw_parts(scaled_rotations, sr_count)
    };
    *out = text_blob::glyphrun_conservative_bounds(*font_bounds, pos, sr);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_glyphrun_tight_bounds(
    glyph_rects: *const Rect,
    positions: *const Point,
    count: usize,
    scaled_rotations: *const Vector,
    sr_count: usize,
    strike_to_source_scale: f32,
    out: *mut Rect,
) {
    if glyph_rects.is_null() || positions.is_null() || out.is_null() {
        return;
    }
    let rects = slice::from_raw_parts(glyph_rects, count);
    let pos = slice::from_raw_parts(positions, count);
    let sr = if scaled_rotations.is_null() || sr_count == 0 {
        &[]
    } else {
        slice::from_raw_parts(scaled_rotations, sr_count)
    };
    *out = text_blob::glyphrun_tight_bounds(rects, pos, sr, strike_to_source_scale);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_painter_scaler_flags(gamma_is_linear: bool) -> u32 {
    text_blob::compute_scaler_context_flags(gamma_is_linear)
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_painter_quantize_direct_mask(
    glyph_id: u16,
    pos: *const Point,
    pos_matrix_with_rounding: *const Matrix,
    ignore_pos_field_mask: *const IPoint,
    out: *mut DirectMaskQuantized,
) {
    if pos.is_null()
        || pos_matrix_with_rounding.is_null()
        || ignore_pos_field_mask.is_null()
        || out.is_null()
    {
        return;
    }
    *out = text_blob::quantize_direct_mask_glyph(
        glyph_id,
        *pos,
        &*pos_matrix_with_rounding,
        *ignore_pos_field_mask,
    );
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_painter_perspective_max_scale(
    glyph_rects: *const Rect,
    positions: *const Point,
    count: usize,
    draw_origin: *const Point,
    position_matrix: *const Matrix,
    font_size: f32,
) -> f32 {
    if glyph_rects.is_null()
        || positions.is_null()
        || draw_origin.is_null()
        || position_matrix.is_null()
        || count == 0
    {
        return -f32::MAX;
    }
    let rects = slice::from_raw_parts(glyph_rects, count);
    let pos = slice::from_raw_parts(positions, count);
    text_blob::compute_perspective_bitmap_max_scale(
        rects,
        pos,
        *draw_origin,
        &*position_matrix,
        font_size,
    )
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_intercept_setup(
    has_path_effect: bool,
    font_size: f32,
    paint_style: u8,
    stroke_width: f32,
    out: *mut InterceptSetup,
) {
    if out.is_null() {
        return;
    }
    *out = text_blob::compute_intercept_setup(has_path_effect, font_size, paint_style, stroke_width);
}

#[no_mangle]
pub unsafe extern "C" fn skia_rs_text_utils_align_x(x: f32, width: f32, align: i32) -> f32 {
    let a = TextAlign::from_i32(align).unwrap_or(TextAlign::Left);
    text_blob::align_text_x(x, width, a)
}
