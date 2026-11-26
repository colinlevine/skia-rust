/*
 * Safe Rust bridge for:
 *   src/core/SkFont.cpp
 *   src/core/SkFont_serial.cpp
 *   src/core/SkFontDescriptor.cpp
 *   src/core/SkFontMetricsPriv.cpp
 *   src/core/SkFontStream.cpp
 * (Phase 1 Layer 5.1 skia-rs-text, GN target :core).
 */

#include "include/core/SkData.h"
#include "include/core/SkFont.h"
#include "include/core/SkFontMetrics.h"
#include "include/core/SkFontStyle.h"
#include "include/core/SkFontTypes.h"
#include "include/core/SkFourByteTag.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkStream.h"
#include "include/core/SkStrikeRef.h"
#include "include/core/SkString.h"
#include "include/core/SkTypeface.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkAutoMalloc.h"
#include "src/core/SkFontDescriptor.h"
#include "src/core/SkFontMetricsPriv.h"
#include "src/core/SkFontPriv.h"
#include "src/core/SkFontStream.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkPaintDefaults.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkStreamPriv.h"
#include "src/core/SkStrike.h"
#include "src/core/SkStrikeSpec.h"
#include "src/core/SkWriteBuffer.h"
#include "src/core/SkZip.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

using namespace skia_private;

namespace {

struct RsSetupForAsPathsResult {
    uint8_t flags;
    uint8_t edging;
    uint8_t hinting;
    float size;
    float scale;
};

struct RsFontSerialPackResult {
    uint32_t packed;
    bool write_size;
    bool write_scale_x;
    bool write_skew_x;
    bool write_typeface;
};

struct RsFontSerialUnpackResult {
    bool has_byte_size;
    float byte_size;
    bool has_scale_x;
    bool has_skew_x;
    bool has_typeface;
    uint8_t flags;
    uint8_t edging;
    uint8_t hinting;
};

struct RsSharedHeaderProbe {
    bool is_collection;
    uint32_t ttc_count;
    uint32_t ttc_offset0;
    uint16_t single_num_tables;
};

struct RsTableRangeResult {
    bool found;
    size_t stream_offset;
    size_t clamped_length;
};

struct RsDescriptorStyleAxes {
    float weight;
    float width;
    float slant;
    float italic;
};

struct RsFontStyle {
    int32_t weight;
    int32_t width;
    uint8_t slant;
};

static_assert(sizeof(SkFontMetrics) == 16 * 4, "SkFontMetrics layout must match Rust FontMetrics");
static_assert(sizeof(SkRect) == 16, "SkRect layout");
static_assert(sizeof(SkPoint) == 8, "SkPoint layout");
static_assert(sizeof(SkMatrix) == 40, "SkMatrix layout");

extern "C" {
float skia_rs_text_font_valid_size(float size);
uint8_t skia_rs_text_font_set_clear_mask(uint8_t bits, bool cond, uint8_t mask);
bool skia_rs_text_font_has_some_aa(uint8_t edging);
void skia_rs_text_font_setup_for_as_paths(
        uint8_t flags, uint8_t edging, float size, RsSetupForAsPathsResult* out);
void skia_rs_text_font_get_bounds(
        float size, float scale_x, float skew_x, const SkRect* typeface_bounds, SkRect* out);
float skia_rs_text_font_approx_transformed_size(
        float font_size, const SkMatrix* matrix, const SkPoint* text_location);
size_t skia_rs_text_count_elements(const uint8_t* text, size_t byte_length, int32_t encoding);
float skia_rs_text_measure_glyphs(
        const float* advances_x, const SkRect* rects, size_t count,
        float strike_to_source_scale, SkRect* out_bounds);
void skia_rs_text_scale_rect(const SkRect* r, float s, SkRect* out);
void skia_rs_text_accumulate_positions(
        const SkPoint* advances, size_t count, float scale, const SkPoint* origin, SkPoint* out_pos);
void skia_rs_text_accumulate_x_positions(
        const float* advances_x, size_t count, float scale, float origin, float* out_xpos);
void skia_rs_text_glyphs_to_unichars(
        const uint16_t* glyphs, size_t count,
        const int32_t* glyph_to_unicode, size_t num_glyphs_in_typeface,
        int32_t* out_unichars);
void skia_rs_text_scale_font_metrics(SkFontMetrics* metrics, float scale);
float skia_rs_text_font_metrics_spacing(const SkFontMetrics* metrics);
void skia_rs_text_font_pack_serial(
        float size, float scale_x, float skew_x, uint8_t flags, uint8_t edging, uint8_t hinting,
        bool has_typeface, RsFontSerialPackResult* out);
void skia_rs_text_font_unpack_serial(uint32_t packed, RsFontSerialUnpackResult* out);

int32_t skia_rs_text_sfnt_count_ttc_entries(const uint8_t* header_bytes, size_t len);
bool skia_rs_text_sfnt_probe_shared_header(
        const uint8_t* header_bytes, size_t len, RsSharedHeaderProbe* out);
bool skia_rs_text_sfnt_parse_num_tables(
        const uint8_t* sfnt_header_bytes, size_t len, uint16_t* out_count);
bool skia_rs_text_sfnt_parse_ttc_offset(
        const uint8_t* ttc_header_bytes, size_t len, size_t ttc_index, uint32_t* out_offset);
size_t skia_rs_text_sfnt_extract_tags(
        const uint8_t* dir_bytes, size_t dir_len, size_t count, uint32_t* out_tags, size_t out_len);
void skia_rs_text_sfnt_find_table_range(
        const uint8_t* dir_bytes, size_t dir_len, size_t count, uint32_t tag,
        size_t offset, size_t length, RsTableRangeResult* out);

int32_t skia_rs_text_font_style_width_for_axis(float width);
float skia_rs_text_font_width_axis_for_style(int32_t width);
void skia_rs_text_descriptor_unpack_style_bits(size_t style_bits, RsDescriptorStyleAxes* out);
void skia_rs_text_descriptor_pack_style(
        int32_t weight, int32_t width, uint8_t slant,
        uint32_t* out_style_bits, RsDescriptorStyleAxes* out_axes);
void skia_rs_text_font_style_from_axes(const RsDescriptorStyleAxes* axes, RsFontStyle* out);
}

}  // namespace

// ============================================================================
// SkFont.cpp
// ============================================================================

#define kDefault_Size    SkPaintDefaults_TextSize
#define kDefault_Flags   SkFont::kBaselineSnap_PrivFlag
#define kDefault_Edging  SkFont::Edging::kAntiAlias
#define kDefault_Hinting SkPaintDefaults_Hinting

SkFont::SkFont(sk_sp<SkTypeface> face, SkScalar size, SkScalar scaleX, SkScalar skewX)
    : fTypeface(std::move(face))
    , fSize(skia_rs_text_font_valid_size(size))
    , fScaleX(scaleX)
    , fSkewX(skewX)
    , fFlags(kDefault_Flags)
    , fEdging(static_cast<unsigned>(kDefault_Edging))
    , fHinting(static_cast<unsigned>(kDefault_Hinting)) {
    if (!fTypeface) {
        fTypeface = SkTypeface::MakeEmpty();
    }
}

SkFont::SkFont(sk_sp<SkTypeface> face, SkScalar size) : SkFont(std::move(face), size, 1, 0) {}

SkFont::SkFont(sk_sp<SkTypeface> face) : SkFont(std::move(face), kDefault_Size, 1, 0) {}

SkFont::SkFont() : SkFont(nullptr, kDefault_Size) {}

bool SkFont::operator==(const SkFont& b) const {
    return  fTypeface.get() == b.fTypeface.get() &&
            fSize           == b.fSize &&
            fScaleX         == b.fScaleX &&
            fSkewX          == b.fSkewX &&
            fFlags          == b.fFlags &&
            fEdging         == b.fEdging &&
            fHinting        == b.fHinting;
}

void SkFont::dump() const {
    SkDebugf("typeface %p\n", fTypeface.get());
    SkDebugf("size %g\n", fSize);
    SkDebugf("skewx %g\n", fSkewX);
    SkDebugf("scalex %g\n", fScaleX);
    SkDebugf("flags 0x%X\n", fFlags);
    SkDebugf("edging %u\n", (unsigned)fEdging);
    SkDebugf("hinting %u\n", (unsigned)fHinting);
}

void SkFont::setTypeface(sk_sp<SkTypeface> tf) {
    fTypeface = std::move(tf);
    if (!fTypeface) {
        fTypeface = SkTypeface::MakeEmpty();
    }
}

void SkFont::setForceAutoHinting(bool predicate) {
    fFlags = skia_rs_text_font_set_clear_mask(fFlags, predicate, kForceAutoHinting_PrivFlag);
}
void SkFont::setEmbeddedBitmaps(bool predicate) {
    fFlags = skia_rs_text_font_set_clear_mask(fFlags, predicate, kEmbeddedBitmaps_PrivFlag);
}
void SkFont::setSubpixel(bool predicate) {
    fFlags = skia_rs_text_font_set_clear_mask(fFlags, predicate, kSubpixel_PrivFlag);
}
void SkFont::setLinearMetrics(bool predicate) {
    fFlags = skia_rs_text_font_set_clear_mask(fFlags, predicate, kLinearMetrics_PrivFlag);
}
void SkFont::setEmbolden(bool predicate) {
    fFlags = skia_rs_text_font_set_clear_mask(fFlags, predicate, kEmbolden_PrivFlag);
}
void SkFont::setBaselineSnap(bool predicate) {
    fFlags = skia_rs_text_font_set_clear_mask(fFlags, predicate, kBaselineSnap_PrivFlag);
}
void SkFont::setEdging(Edging e) {
    fEdging = SkToU8(e);
}
void SkFont::setHinting(SkFontHinting h) {
    fHinting = SkToU8(h);
}
void SkFont::setSize(SkScalar size) {
    fSize = skia_rs_text_font_valid_size(size);
}
void SkFont::setScaleX(SkScalar scale) {
    fScaleX = scale;
}
void SkFont::setSkewX(SkScalar skew) {
    fSkewX = skew;
}

SkFont SkFont::makeWithSize(SkScalar newSize) const {
    SkFont font = *this;
    font.setSize(newSize);
    return font;
}

SkScalar SkFont::setupForAsPaths(SkPaint* paint) {
    RsSetupForAsPathsResult res;
    skia_rs_text_font_setup_for_as_paths(fFlags, fEdging, fSize, &res);
    fFlags = res.flags;
    fEdging = res.edging;
    fHinting = res.hinting;
    fSize = res.size;

    if (paint) {
        paint->setStyle(SkPaint::kFill_Style);
        paint->setPathEffect(nullptr);
    }
    return res.scale;
}

bool SkFont::hasSomeAntiAliasing() const {
    return skia_rs_text_font_has_some_aa(fEdging);
}

SkGlyphID SkFont::unicharToGlyph(SkUnichar uni) const {
    return this->getTypeface()->unicharToGlyph(uni);
}

void SkFont::unicharsToGlyphs(SkSpan<const SkUnichar> unis, SkSpan<SkGlyphID> glyphs) const {
    this->getTypeface()->unicharsToGlyphs(unis, glyphs);
}

size_t SkFont::textToGlyphs(const void* text, size_t byteLength, SkTextEncoding encoding,
                            SkSpan<SkGlyphID> glyphs) const {
    return this->getTypeface()->textToGlyphs(text, byteLength, encoding, glyphs);
}

SkScalar SkFont::measureText(const void* text, size_t length, SkTextEncoding encoding,
                             SkRect* bounds, const SkPaint* paint) const {
    SkAutoToGlyphs atg(*this, text, length, encoding);
    const SkSpan<const SkGlyphID> glyphIDs = atg.glyphs();

    if (glyphIDs.size() == 0) {
        if (bounds) {
            bounds->setEmpty();
        }
        return 0;
    }

    auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(*this, paint);
    SkBulkGlyphMetrics metrics{strikeSpec};
    SkSpan<const SkGlyph*> glyphs = metrics.glyphs(glyphIDs);

    const size_t n = glyphs.size();
    AutoSTArray<64, float> advances(n);
    AutoSTArray<64, SkRect> rects(bounds ? n : 0);
    for (size_t i = 0; i < n; ++i) {
        advances[i] = glyphs[i]->advanceX();
        if (bounds) {
            rects[i] = glyphs[i]->rect();
        }
    }

    return skia_rs_text_measure_glyphs(
            advances.data(), bounds ? rects.data() : nullptr, n, strikeToSourceScale, bounds);
}

SkStrikeRef SkFont::makeStrikeRef() const {
    auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(*this);
    return SkStrikeRef(strikeSpec.findOrCreateStrike(), strikeToSourceScale);
}

void SkFont::getWidthsBounds(SkSpan<const SkGlyphID> glyphIDs,
                             SkSpan<SkScalar> widths,
                             SkSpan<SkRect> bounds,
                             const SkPaint* paint) const {
    auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(*this, paint);
    SkBulkGlyphMetrics metrics{strikeSpec};
    SkSpan<const SkGlyph*> glyphs = metrics.glyphs(glyphIDs);

    if (bounds.size()) {
        const auto n = std::min(bounds.size(), glyphs.size());
        for (size_t i = 0; i < n; ++i) {
            SkRect r = glyphs[i]->rect();
            skia_rs_text_scale_rect(&r, strikeToSourceScale, &bounds[i]);
        }
    }

    if (widths.size()) {
        const auto n = std::min(widths.size(), glyphs.size());
        for (auto [width, glyph] : SkMakeZip(widths.first(n), glyphs.first(n))) {
            width = glyph->advanceX() * strikeToSourceScale;
        }
    }
}

void SkFont::getPos(SkSpan<const SkGlyphID> glyphIDs, SkSpan<SkPoint> pos, SkPoint origin) const {
    auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(*this);
    SkBulkGlyphMetrics metrics{strikeSpec};
    SkSpan<const SkGlyph*> glyphs = metrics.glyphs(glyphIDs);

    const size_t n = std::min(pos.size(), glyphs.size());
    AutoSTArray<64, SkPoint> advances(n);
    for (size_t i = 0; i < n; ++i) {
        advances[i] = glyphs[i]->advanceVector();
    }
    skia_rs_text_accumulate_positions(advances.data(), n, strikeToSourceScale, &origin, pos.data());
}

void SkFont::getXPos(SkSpan<const SkGlyphID> gIDs, SkSpan<SkScalar> xpos, SkScalar origin) const {
    auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(*this);
    SkBulkGlyphMetrics metrics{strikeSpec};
    SkSpan<const SkGlyph*> glyphs = metrics.glyphs(gIDs);

    const size_t n = std::min(xpos.size(), glyphs.size());
    AutoSTArray<64, float> advances(n);
    for (size_t i = 0; i < n; ++i) {
        advances[i] = glyphs[i]->advanceX();
    }
    skia_rs_text_accumulate_x_positions(advances.data(), n, strikeToSourceScale, origin, xpos.data());
}

void SkFont::getPaths(SkSpan<const SkGlyphID> glyphIDs,
                      void (*proc)(const SkPath*, const SkMatrix&, void*), void* ctx) const {
    SkFont font(*this);
    SkScalar scale = font.setupForAsPaths(nullptr);
    const SkMatrix mx = SkMatrix::Scale(scale, scale);

    SkStrikeSpec strikeSpec = SkStrikeSpec::MakeWithNoDevice(font);
    SkBulkGlyphMetricsAndPaths paths{strikeSpec};
    SkSpan<const SkGlyph*> glyphs = paths.glyphs(glyphIDs);

    for (auto glyph : glyphs) {
        proc(glyph->path(), mx, ctx);
    }
}

std::optional<SkPath> SkFont::getPath(SkGlyphID glyphID) const {
    std::optional<SkPath> result;

    this->getPaths({&glyphID, 1}, [](const SkPath* path, const SkMatrix& mx, void* ctx) {
        if (path) {
            auto* result = static_cast<std::optional<SkPath>*>(ctx);
            *result = path->tryMakeTransform(mx);
        }
    }, &result);

    return result;
}

SkScalar SkFont::getMetrics(SkFontMetrics* metrics) const {
    auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(*this, nullptr);

    SkFontMetrics storage;
    if (nullptr == metrics) {
        metrics = &storage;
    }

    auto cache = strikeSpec.findOrCreateStrike();
    *metrics = cache->getFontMetrics();

    if (strikeToSourceScale != 1) {
        SkFontPriv::ScaleFontMetrics(metrics, strikeToSourceScale);
    }
    return skia_rs_text_font_metrics_spacing(metrics);
}

void SkFontPriv::ScaleFontMetrics(SkFontMetrics* metrics, SkScalar scale) {
    skia_rs_text_scale_font_metrics(metrics, scale);
}

SkRect SkFontPriv::GetFontBounds(const SkFont& font) {
    SkRect tfBounds = font.getTypeface()->getBounds();
    SkRect out;
    skia_rs_text_font_get_bounds(font.getSize(), font.getScaleX(), font.getSkewX(), &tfBounds, &out);
    return out;
}

SkScalar SkFontPriv::ApproximateTransformedTextSize(const SkFont& font, const SkMatrix& matrix,
                                                    const SkPoint& textLocation) {
    return skia_rs_text_font_approx_transformed_size(font.getSize(), &matrix, &textLocation);
}

size_t SkFontPriv::CountTextElements(const void* text, size_t byteLength, SkTextEncoding encoding) {
    return skia_rs_text_count_elements(
            static_cast<const uint8_t*>(text), byteLength, static_cast<int32_t>(encoding));
}

void SkFontPriv::GlyphsToUnichars(const SkFont& font, const SkGlyphID glyphs[], int count,
                                  SkUnichar text[]) {
    if (count <= 0) {
        return;
    }

    auto typeface = font.getTypeface();
    const unsigned numGlyphsInTypeface = typeface->countGlyphs();
    AutoTArray<SkUnichar> unichars(static_cast<size_t>(numGlyphsInTypeface));
    typeface->getGlyphToUnicodeMap(unichars);

    skia_rs_text_glyphs_to_unichars(
            glyphs, static_cast<size_t>(count), unichars.data(), numGlyphsInTypeface, text);
}

// ============================================================================
// SkFont_serial.cpp
// ============================================================================

void SkFontPriv::Flatten(const SkFont& font, SkWriteBuffer& buffer) {
    RsFontSerialPackResult res;
    skia_rs_text_font_pack_serial(
            font.fSize, font.fScaleX, font.fSkewX, font.fFlags, font.fEdging, font.fHinting,
            font.fTypeface != nullptr, &res);

    buffer.write32(res.packed);
    if (res.write_size) {
        buffer.writeScalar(font.fSize);
    }
    if (res.write_scale_x) {
        buffer.writeScalar(font.fScaleX);
    }
    if (res.write_skew_x) {
        buffer.writeScalar(font.fSkewX);
    }
    if (res.write_typeface) {
        buffer.writeTypeface(font.fTypeface.get());
    }
}

bool SkFontPriv::Unflatten(SkFont* font, SkReadBuffer& buffer) {
    const uint32_t packed = buffer.read32();
    RsFontSerialUnpackResult res;
    skia_rs_text_font_unpack_serial(packed, &res);

    if (res.has_byte_size) {
        font->fSize = res.byte_size;
    } else {
        font->fSize = buffer.readScalar();
    }
    if (res.has_scale_x) {
        font->fScaleX = buffer.readScalar();
    }
    if (res.has_skew_x) {
        font->fSkewX = buffer.readScalar();
    }
    if (res.has_typeface) {
        font->setTypeface(buffer.readTypeface());
    }

    font->fFlags = res.flags;
    font->fEdging = res.edging;
    font->fHinting = res.hinting;

    return buffer.isValid();
}

// ============================================================================
// SkFontMetricsPriv.cpp
// ============================================================================

void SkFontMetricsPriv::Flatten(SkWriteBuffer& buffer, const SkFontMetrics& metrics) {
    buffer.writeUInt(metrics.fFlags);
    buffer.writeScalar(metrics.fTop);
    buffer.writeScalar(metrics.fAscent);
    buffer.writeScalar(metrics.fDescent);
    buffer.writeScalar(metrics.fBottom);
    buffer.writeScalar(metrics.fLeading);
    buffer.writeScalar(metrics.fAvgCharWidth);
    buffer.writeScalar(metrics.fMaxCharWidth);
    buffer.writeScalar(metrics.fXMin);
    buffer.writeScalar(metrics.fXMax);
    buffer.writeScalar(metrics.fXHeight);
    buffer.writeScalar(metrics.fCapHeight);
    buffer.writeScalar(metrics.fUnderlineThickness);
    buffer.writeScalar(metrics.fUnderlinePosition);
    buffer.writeScalar(metrics.fStrikeoutThickness);
    buffer.writeScalar(metrics.fStrikeoutPosition);
}

std::optional<SkFontMetrics> SkFontMetricsPriv::MakeFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());

    SkFontMetrics metrics;
    metrics.fFlags = buffer.readUInt();
    metrics.fTop = buffer.readScalar();
    metrics.fAscent = buffer.readScalar();
    metrics.fDescent = buffer.readScalar();
    metrics.fBottom = buffer.readScalar();
    metrics.fLeading = buffer.readScalar();
    metrics.fAvgCharWidth = buffer.readScalar();
    metrics.fMaxCharWidth = buffer.readScalar();
    metrics.fXMin = buffer.readScalar();
    metrics.fXMax = buffer.readScalar();
    metrics.fXHeight = buffer.readScalar();
    metrics.fCapHeight = buffer.readScalar();
    metrics.fUnderlineThickness = buffer.readScalar();
    metrics.fUnderlinePosition = buffer.readScalar();
    metrics.fStrikeoutThickness = buffer.readScalar();
    metrics.fStrikeoutPosition = buffer.readScalar();

    if (buffer.isValid()) {
        return metrics;
    }

    return std::nullopt;
}

// ============================================================================
// SkFontDescriptor.cpp
// ============================================================================

namespace {
enum FontDescriptorTag {
    kInvalid               = 0x00,
    kFontFamilyName        = 0x01,
    kFullName              = 0x04,
    kPostscriptName        = 0x06,
    kWeight                = 0x10,
    kWidth                 = 0x11,
    kSlant                 = 0x12,
    kItalic                = 0x13,
    kSyntheticBold         = 0xF6,
    kSyntheticOblique      = 0xF7,
    kPaletteIndex          = 0xF8,
    kPaletteEntryOverrides = 0xF9,
    kFontVariation         = 0xFA,
    kFactoryId             = 0xFC,
    kFontIndex             = 0xFD,
    kSentinel              = 0xFF,
};

bool write_id(SkWStream* stream, uint32_t id) {
    return stream->writePackedUInt(id);
}
bool write_string(SkWStream* stream, const SkString& string, uint32_t id) {
    if (string.isEmpty()) { return true; }
    return write_id(stream, id) &&
           stream->writePackedUInt(string.size()) &&
           stream->write(string.c_str(), string.size());
}
bool write_uint(SkWStream* stream, size_t n, uint32_t id) {
    return write_id(stream, id) && stream->writePackedUInt(n);
}
bool write_scalar(SkWStream* stream, SkScalar n, uint32_t id) {
    return write_id(stream, id) && stream->writeScalar(n);
}
[[nodiscard]] size_t read_id(SkStream* stream) {
    size_t i;
    if (!stream->readPackedUInt(&i)) { return kInvalid; }
    return i;
}
[[nodiscard]] bool read_string(SkStream* stream, SkString* string) {
    size_t length;
    if (!stream->readPackedUInt(&length)) { return false; }
    if (length > 0) {
        if (SkStreamPriv::RemainingLengthIsBelow(stream, length)) {
            return false;
        }
        string->resize(length);
        if (stream->read(string->data(), length) != length) { return false; }
    }
    return true;
}
}  // namespace

SkFontDescriptor::SkFontDescriptor() {}

bool SkFontDescriptor::Deserialize(SkStream* stream,
                                   SkFontDescriptor* result,
                                   SkTypeface::SkTypefaceStreamSanitizerProc sanitizer) {
    size_t factoryId;
    using FactoryIdType = decltype(result->fFactoryId);
    size_t coordinateCount;
    using CoordinateCountType = decltype(result->fCoordinateCount);
    size_t index;
    using CollectionIndexType = decltype(result->fCollectionIndex);
    size_t paletteIndex;
    using PaletteIndexType = decltype(result->fPaletteIndex);
    size_t paletteEntryOverrideCount;
    using PaletteEntryOverrideCountType = decltype(result->fPaletteEntryOverrideCount);
    size_t paletteEntryOverrideIndex;
    using PaletteEntryOverrideIndexType = decltype(result->fPaletteEntryOverrides[0].index);

    size_t styleBits;
    if (!stream->readPackedUInt(&styleBits)) { return false; }
    RsDescriptorStyleAxes axes;
    skia_rs_text_descriptor_unpack_style_bits(styleBits, &axes);

    for (size_t id; (id = read_id(stream)) != kSentinel;) {
        switch (id) {
            case kFontFamilyName:
                if (!read_string(stream, &result->fFamilyName)) { return false; }
                break;
            case kFullName:
                if (!read_string(stream, &result->fFullName)) { return false; }
                break;
            case kPostscriptName:
                if (!read_string(stream, &result->fPostscriptName)) { return false; }
                break;
            case kWeight:
                if (!stream->readScalar(&axes.weight)) { return false; }
                break;
            case kWidth:
                if (!stream->readScalar(&axes.width)) { return false; }
                break;
            case kSlant:
                if (!stream->readScalar(&axes.slant)) { return false; }
                break;
            case kItalic:
                if (!stream->readScalar(&axes.italic)) { return false; }
                break;
            case kFontVariation:
                if (!stream->readPackedUInt(&coordinateCount)) { return false; }
                if (!SkTFitsIn<CoordinateCountType>(coordinateCount)) { return false; }
                if (SkStreamPriv::RemainingLengthIsBelow(stream, coordinateCount)) {
                    return false;
                }
                result->fCoordinateCount = SkTo<CoordinateCountType>(coordinateCount);
                result->fVariation.reset(coordinateCount);
                for (size_t i = 0; i < coordinateCount; ++i) {
                    if (!stream->readU32(&result->fVariation[i].axis)) { return false; }
                    if (!stream->readScalar(&result->fVariation[i].value)) { return false; }
                }
                break;
            case kFontIndex:
                if (!stream->readPackedUInt(&index)) { return false; }
                if (!SkTFitsIn<CollectionIndexType>(index)) { return false; }
                result->fCollectionIndex = SkTo<CollectionIndexType>(index);
                break;
            case kPaletteIndex:
                if (!stream->readPackedUInt(&paletteIndex)) { return false; }
                if (!SkTFitsIn<PaletteIndexType>(paletteIndex)) { return false; }
                result->fPaletteIndex = SkTo<PaletteIndexType>(paletteIndex);
                break;
            case kPaletteEntryOverrides:
                if (!stream->readPackedUInt(&paletteEntryOverrideCount)) { return false; }
                if (!SkTFitsIn<PaletteEntryOverrideCountType>(paletteEntryOverrideCount)) {
                    return false;
                }
                if (SkStreamPriv::RemainingLengthIsBelow(stream, paletteEntryOverrideCount)) {
                    return false;
                }
                result->fPaletteEntryOverrideCount =
                        SkTo<PaletteEntryOverrideCountType>(paletteEntryOverrideCount);
                result->fPaletteEntryOverrides.reset(paletteEntryOverrideCount);
                for (size_t i = 0; i < paletteEntryOverrideCount; ++i) {
                    if (!stream->readPackedUInt(&paletteEntryOverrideIndex)) { return false; }
                    if (!SkTFitsIn<PaletteEntryOverrideIndexType>(paletteEntryOverrideIndex)) {
                        return false;
                    }
                    result->fPaletteEntryOverrides[i].index =
                            SkTo<PaletteEntryOverrideIndexType>(paletteEntryOverrideIndex);
                    if (!stream->readU32(&result->fPaletteEntryOverrides[i].color)) {
                        return false;
                    }
                }
                break;
            case kSyntheticBold:
                result->fSyntheticBold = true;
                break;
            case kSyntheticOblique:
                result->fSyntheticOblique = true;
                break;
            case kFactoryId:
                if (!stream->readPackedUInt(&factoryId)) { return false; }
                if (!SkTFitsIn<FactoryIdType>(factoryId)) { return false; }
                result->fFactoryId = SkTo<FactoryIdType>(factoryId);
                break;
            default:
                SkDEBUGFAIL("Unknown id used by a font descriptor");
                return false;
        }
    }

    RsFontStyle style;
    skia_rs_text_font_style_from_axes(&axes, &style);
    result->fStyle = SkFontStyle(style.weight,
                                 static_cast<SkFontStyle::Width>(style.width),
                                 static_cast<SkFontStyle::Slant>(style.slant));

    size_t length;
    if (!stream->readPackedUInt(&length)) { return false; }
    if (length > 0) {
        if (SkStreamPriv::RemainingLengthIsBelow(stream, length)) {
            return false;
        }
        sk_sp<SkData> data(SkData::MakeUninitialized(length));
        if (stream->read(data->writable_data(), length) != length) {
            SkDEBUGFAIL("Could not read font data");
            return false;
        }
        if (sanitizer) {
            data = sanitizer(std::move(data));
            if (!data) {
                return false;
            }
        }
        result->fStream = SkMemoryStream::Make(std::move(data));
    }
    return true;
}

bool SkFontDescriptor::serialize(SkWStream* stream) const {
    uint32_t styleBits = 0;
    RsDescriptorStyleAxes axes;
    skia_rs_text_descriptor_pack_style(
            fStyle.weight(), fStyle.width(), static_cast<uint8_t>(fStyle.slant()),
            &styleBits, &axes);
    if (!stream->writePackedUInt(styleBits)) { return false; }

    if (!write_string(stream, fFamilyName, kFontFamilyName)) { return false; }
    if (!write_string(stream, fFullName, kFullName)) { return false; }
    if (!write_string(stream, fPostscriptName, kPostscriptName)) { return false; }

    if (!write_scalar(stream, axes.weight, kWeight)) { return false; }
    if (!write_scalar(stream, axes.width, kWidth)) { return false; }
    if (!write_scalar(stream, axes.slant, kSlant)) { return false; }
    if (!write_scalar(stream, axes.italic, kItalic)) { return false; }

    if (fCollectionIndex > 0) {
        if (!write_uint(stream, fCollectionIndex, kFontIndex)) { return false; }
    }
    if (fPaletteIndex > 0) {
        if (!write_uint(stream, fPaletteIndex, kPaletteIndex)) { return false; }
    }
    if (fCoordinateCount > 0) {
        if (!write_uint(stream, fCoordinateCount, kFontVariation)) { return false; }
        for (int i = 0; i < fCoordinateCount; ++i) {
            if (!stream->write32(fVariation[i].axis)) { return false; }
            if (!stream->writeScalar(fVariation[i].value)) { return false; }
        }
    }
    if (fPaletteEntryOverrideCount > 0) {
        if (!write_uint(stream, fPaletteEntryOverrideCount, kPaletteEntryOverrides)) { return false; }
        for (int i = 0; i < fPaletteEntryOverrideCount; ++i) {
            if (!stream->writePackedUInt(fPaletteEntryOverrides[i].index)) { return false; }
            if (!stream->write32(fPaletteEntryOverrides[i].color)) { return false; }
        }
    }
    if (fSyntheticBold) {
        if (!write_id(stream, kSyntheticBold)) { return false; }
    }
    if (fSyntheticOblique) {
        if (!write_id(stream, kSyntheticOblique)) { return false; }
    }

    if (!write_uint(stream, fFactoryId, kFactoryId)) { return false; }

    if (!stream->writePackedUInt(kSentinel)) { return false; }

    if (fStream) {
        std::unique_ptr<SkStreamAsset> fontStream = fStream->duplicate();
        size_t length = fontStream->getLength();
        if (!stream->writePackedUInt(length)) { return false; }
        if (!stream->writeStream(fontStream.get(), length)) { return false; }
    } else {
        if (!stream->writePackedUInt(0)) { return false; }
    }

    return true;
}

SkFontStyle::Width SkFontDescriptor::SkFontStyleWidthForWidthAxisValue(SkScalar width) {
    return static_cast<SkFontStyle::Width>(skia_rs_text_font_style_width_for_axis(width));
}

SkScalar SkFontDescriptor::SkFontWidthAxisValueForStyleWidth(int width) {
    return skia_rs_text_font_width_axis_for_style(width);
}

// ============================================================================
// SkFontStream.cpp
// ============================================================================

namespace {

constexpr size_t kSharedTTHeaderSize = 16;
constexpr size_t kSFNTHeaderSize     = 12;
constexpr size_t kSFNTDirEntrySize   = 16;

bool read_exact(SkStream* stream, void* buffer, size_t amount) {
    return stream->read(buffer, amount) == amount;
}

bool skip_exact(SkStream* stream, size_t amount) {
    return stream->skip(amount) == amount;
}

int count_tables_rs(SkStream* stream, int ttcIndex, size_t* offsetToDir) {
    SkASSERT(ttcIndex >= 0);

    SkAutoSMalloc<1024> storage(kSharedTTHeaderSize);
    auto* header = static_cast<uint8_t*>(storage.get());

    if (!read_exact(stream, header, kSharedTTHeaderSize)) {
        return 0;
    }

    RsSharedHeaderProbe probe;
    if (!skia_rs_text_sfnt_probe_shared_header(header, kSharedTTHeaderSize, &probe)) {
        return 0;
    }

    size_t offset = 0;
    uint16_t numTables = probe.single_num_tables;

    if (probe.is_collection) {
        if (static_cast<uint32_t>(ttcIndex) >= probe.ttc_count) {
            return 0;
        }
        uint32_t entryOffset = probe.ttc_offset0;
        if (ttcIndex > 0) {
            stream->rewind();
            size_t amount = kSharedTTHeaderSize + static_cast<size_t>(ttcIndex) * sizeof(uint32_t);
            header = static_cast<uint8_t*>(storage.reset(amount));
            if (!read_exact(stream, header, amount)) {
                return 0;
            }
            if (!skia_rs_text_sfnt_parse_ttc_offset(
                        header, amount, static_cast<size_t>(ttcIndex), &entryOffset)) {
                return 0;
            }
        }
        offset = entryOffset;
        stream->rewind();
        if (!skip_exact(stream, offset)) {
            return 0;
        }
        uint8_t sfntHdr[kSFNTHeaderSize];
        if (!read_exact(stream, sfntHdr, kSFNTHeaderSize)) {
            return 0;
        }
        if (!skia_rs_text_sfnt_parse_num_tables(sfntHdr, kSFNTHeaderSize, &numTables)) {
            return 0;
        }
    }

    if (offsetToDir) {
        *offsetToDir = offset + kSFNTHeaderSize;
    }
    return numTables;
}

struct SfntDirHolder {
    SK_ONLY_STACK_ALLOCATABLE;

    SfntDirHolder() : fCount(0), fDirBytes(nullptr), fDirSize(0) {}
    ~SfntDirHolder() { sk_free(fDirBytes); }

    bool init(SkStream* stream, int ttcIndex) {
        stream->rewind();

        size_t offsetToDir;
        fCount = count_tables_rs(stream, ttcIndex, &offsetToDir);
        if (0 == fCount) {
            return false;
        }

        stream->rewind();
        if (!skip_exact(stream, offsetToDir)) {
            return false;
        }

        fDirSize = static_cast<size_t>(fCount) * kSFNTDirEntrySize;
        fDirBytes = static_cast<uint8_t*>(sk_malloc_throw(fDirSize));
        return read_exact(stream, fDirBytes, fDirSize);
    }

    int fCount;
    uint8_t* fDirBytes;
    size_t fDirSize;
};

}  // namespace

int SkFontStream::CountTTCEntries(SkStream* stream) {
    stream->rewind();
    uint8_t shared[kSharedTTHeaderSize];
    if (!read_exact(stream, shared, kSharedTTHeaderSize)) {
        return 0;
    }
    return skia_rs_text_sfnt_count_ttc_entries(shared, kSharedTTHeaderSize);
}

int SkFontStream::GetTableTags(SkStream* stream, int ttcIndex, SkSpan<SkFontTableTag> tags) {
    SfntDirHolder header;
    if (!header.init(stream, ttcIndex)) {
        return 0;
    }

    return static_cast<int>(skia_rs_text_sfnt_extract_tags(
            header.fDirBytes, header.fDirSize, static_cast<size_t>(header.fCount),
            tags.data(), tags.size()));
}

size_t SkFontStream::GetTableData(SkStream* stream, int ttcIndex,
                                  SkFontTableTag tag,
                                  size_t offset, size_t length, void* data) {
    SfntDirHolder header;
    if (!header.init(stream, ttcIndex)) {
        return 0;
    }

    RsTableRangeResult range;
    skia_rs_text_sfnt_find_table_range(
            header.fDirBytes, header.fDirSize, static_cast<size_t>(header.fCount),
            tag, offset, length, &range);
    if (!range.found) {
        return 0;
    }

    if (data) {
        stream->rewind();
        if (!skip_exact(stream, range.stream_offset)) {
            return 0;
        }
        if (!read_exact(stream, data, range.clamped_length)) {
            return 0;
        }
    }
    return range.clamped_length;
}
