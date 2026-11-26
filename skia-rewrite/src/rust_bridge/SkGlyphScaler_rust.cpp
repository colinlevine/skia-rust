/*
 * Safe Rust bridge for:
 *   src/core/SkGlyph.cpp
 *   src/core/SkScalerContext.cpp
 * (Phase 1 Layer 5.1 skia-rs-text, GN target :core).
 */

#include "include/core/SkCanvas.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkFont.h"
#include "include/core/SkFontMetrics.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSerialProcs.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStrokeRec.h"
#include "include/private/SkAlign.h"
#include "include/private/SkCPUTypes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkFixed.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkMutex.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTo.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkAutoMalloc.h"
#include "src/core/SkAutoPixmapStorage.h"
#include "src/core/SkBlitter_A8.h"
#include "src/core/SkColorData.h"
#include "src/core/SkDescriptor.h"
#include "src/core/SkDraw.h"
#include "src/core/SkFontPriv.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkPaintPriv.h"
#include "src/core/SkPictureData.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkScalerContext.h"
#include "src/core/SkWriteBuffer.h"
#include "src/text/StrikeForGPU.h"

#include <algorithm>
#include <cstring>
#include <new>
#include <optional>
#include <utility>
#include <vector>

using namespace skglyph;
using namespace sktext;

namespace {

struct RsPackedGlyphMetrics {
    uint32_t packed_id;
    float advance_x;
    float advance_y;
    uint32_t dimensions;
    uint32_t left_top;
    uint32_t mask_format;
};

struct RsUnpackedGlyphMetrics {
    uint32_t packed_id;
    float advance_x;
    float advance_y;
    uint16_t width;
    uint16_t height;
    int16_t left;
    int16_t top;
    uint8_t mask_format;
};

struct RsPathSegment {
    uint8_t verb;
    uint8_t _pad[3];
    SkPoint pts[4];
};

struct RsMakeRecParams {
    uint32_t typeface_id;
    float font_size;
    float font_scale_x;
    float font_skew_x;
    uint8_t font_flags;
    uint8_t font_edging;
    uint8_t font_hinting;
    bool glyph_mask_needs_current_color;

    uint8_t paint_style;
    uint8_t stroke_join;
    uint8_t stroke_cap;
    uint8_t _pad0;
    float stroke_width;
    float stroke_miter;
    uint32_t paint_color;
    uint32_t luminance_color;

    int32_t pixel_geometry;
    float text_gamma;
    float text_contrast;
    uint32_t scaler_context_flags;
};

struct RsSaturatedGlyphBounds {
    int16_t left;
    int16_t top;
    uint16_t width;
    uint16_t height;
    uint8_t mask_format;
};

struct RsPathMaskRasterPlan {
    int32_t dst_w;
    int32_t dst_h;
    SkMatrix matrix;
    bool from_lcd;
    bool intermediate_dst;
};

struct RsGlyphPositionRoundingSpec {
    SkVector half_axis_sample_freq;
    SkIPoint ignore_position_mask;
    SkIPoint ignore_position_field_mask;
};

static_assert(sizeof(SkScalerContextRec) == 56,
              "SkScalerContextRec must match Rust ScalerContextRec (56 bytes)");
static_assert(sizeof(SkGlyphPositionRoundingSpec) == sizeof(RsGlyphPositionRoundingSpec),
              "SkGlyphPositionRoundingSpec must match Rust GlyphPositionRoundingSpec");
static_assert(sizeof(SkIRect) == 16, "SkIRect layout");
static_assert(sizeof(SkRect) == 16, "SkRect layout");
static_assert(sizeof(SkPoint) == 8, "SkPoint layout");
static_assert(sizeof(SkIPoint) == 8, "SkIPoint layout");
static_assert(sizeof(SkMatrix) == 40, "SkMatrix layout");

extern "C" {
size_t skia_rs_text_glyph_format_alignment(uint8_t format);
size_t skia_rs_text_glyph_row_bytes(int32_t width, uint8_t format);
size_t skia_rs_text_glyph_image_size(uint16_t width, uint16_t height, uint8_t format);
void skia_rs_text_glyph_pack_metrics(
        uint32_t packed_id, float advance_x, float advance_y,
        uint16_t width, uint16_t height, int16_t left, int16_t top, uint8_t mask_format,
        RsPackedGlyphMetrics* out);
bool skia_rs_text_glyph_unpack_metrics(
        const RsPackedGlyphMetrics* packed, RsUnpackedGlyphMetrics* out);
bool skia_rs_text_glyph_drawable_tag_allowed(uint32_t tag);
uint32_t skia_rs_text_glyph_digest_init_actions(bool is_empty);
bool skia_rs_text_glyph_fits_in_atlas(uint16_t width, uint16_t height);
void skia_rs_text_glyph_rounding_spec(
        bool is_subpixel, uint32_t axis_alignment, RsGlyphPositionRoundingSpec* out);
void skia_rs_text_glyph_calculate_path_gap(
        float top_offset, float bottom_offset, const RsPathSegment* segments, size_t count,
        float* out_interval);

void skia_rs_text_scaler_rec_preprocess_post_filter(
        SkScalerContextRec* rec, bool has_mask_filter);
void skia_rs_text_scaler_rec_make(
        const RsMakeRecParams* params, const SkMatrix* device_matrix,
        SkScalerContextRec* out_rec);
void skia_rs_text_scaler_rec_get_matrix_from_2x2(const SkScalerContextRec* rec, SkMatrix* out);
void skia_rs_text_scaler_rec_get_local_matrix(const SkScalerContextRec* rec, SkMatrix* out);
void skia_rs_text_scaler_rec_get_single_matrix(const SkScalerContextRec* rec, SkMatrix* out);
bool skia_rs_text_scaler_rec_compute_matrices(
        const SkScalerContextRec* rec, int32_t pre_matrix_scale,
        SkVector* out_s, SkMatrix* out_s_a, SkMatrix* out_gs_a,
        SkMatrix* out_g_inv, SkMatrix* out_a);
uint32_t skia_rs_text_scaler_rec_axis_alignment(const SkScalerContextRec* rec);
void skia_rs_text_scaler_rec_set_luminance_color(SkScalerContextRec* rec, uint32_t color);
void skia_rs_text_scaler_rec_use_stroke_for_fake_bold(SkScalerContextRec* rec);
void skia_rs_text_scaler_saturate_bounds_rect(const SkRect* rect, RsSaturatedGlyphBounds* out);
void skia_rs_text_scaler_saturate_bounds_irect(const SkIRect* irect, RsSaturatedGlyphBounds* out);
void skia_rs_text_scaler_generate_metrics_from_path_bounds(
        uint8_t mask_format, const SkRect* bounds, bool vertical_lcd, bool a8_from_lcd,
        bool hairline, RsSaturatedGlyphBounds* out);
void skia_rs_text_scaler_plan_image_from_path(
        const SkIRect* bounds, uint8_t format, bool vertical_lcd, bool a8_from_lcd,
        int32_t samples_per_pixel, RsPathMaskRasterPlan* out);
void skia_rs_text_scaler_apply_lut_a8(
        uint8_t* dst, size_t row_bytes, size_t width, size_t height, const uint8_t* lut);
void skia_rs_text_scaler_pack_a8_to_a1(
        uint8_t* dst, size_t dst_rb, const uint8_t* src, size_t src_rb,
        size_t width, size_t height);
void skia_rs_text_scaler_pack_4xh_to_mask(
        const uint8_t* src, size_t src_width, size_t src_height, size_t src_rb,
        uint8_t* dst, size_t dst_rb, size_t dst_byte_size, bool to_a8,
        const uint8_t* preblend_r, const uint8_t* preblend_g, const uint8_t* preblend_b,
        bool do_bgr, bool do_vert);
#if defined(SK_USE_LCD_TEXT_3X_FILTER)
void skia_rs_text_scaler_pack_3xh_to_mask(
        const uint8_t* src, size_t src_width, size_t src_height, size_t src_rb,
        uint8_t* dst, size_t dst_rb, size_t dst_byte_size,
        const uint8_t* preblend_r, const uint8_t* preblend_g, const uint8_t* preblend_b,
        bool do_bgr, bool do_vert);
#endif
void skia_rs_text_scaler_clip_copy_filtered_mask(
        const uint8_t* src, size_t src_total_bytes, const SkIRect* src_bounds,
        size_t src_rb, uint8_t src_format, size_t src_plane_bytes,
        uint8_t* dst, size_t dst_total_bytes, const SkIRect* dst_bounds,
        size_t dst_rb, uint8_t dst_format, size_t dst_plane_bytes);
}

}  // namespace

// ============================================================================
// SkGlyph.cpp
// ============================================================================

sk_sp<SkPictureBackedGlyphDrawable>
SkPictureBackedGlyphDrawable::MakeFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());

    sk_sp<SkData> pictureData = buffer.readByteArrayAsData();
    if (!buffer.isValid() || pictureData->empty()) {
        return nullptr;
    }

    SkDeserialProcs procs;
    procs.fAllowSkSL = false;
    procs.fAllowTagsProc = [](SkFourByteTag tag, void*) -> bool {
        return skia_rs_text_glyph_drawable_tag_allowed(tag);
    };
    sk_sp<SkPicture> picture = SkPicture::MakeFromData(pictureData.get(), &procs);
    if (!buffer.validate(picture != nullptr)) {
        return nullptr;
    }

    return sk_make_sp<SkPictureBackedGlyphDrawable>(std::move(picture));
}

void SkPictureBackedGlyphDrawable::FlattenDrawable(SkWriteBuffer& buffer, SkDrawable* drawable) {
    if (drawable == nullptr) {
        buffer.writeByteArray(nullptr, 0);
        return;
    }

    sk_sp<SkPicture> picture = drawable->makePictureSnapshot();
    sk_sp<SkData> data = picture->serialize();

    if (!SkTFitsIn<uint32_t>(data->size()) || data->empty()) {
        buffer.writeByteArray(nullptr, 0);
        return;
    }

    buffer.writeByteArray(data->data(), data->size());
}

SkPictureBackedGlyphDrawable::SkPictureBackedGlyphDrawable(sk_sp<SkPicture> picture)
        : fPicture(std::move(picture)) {}

SkRect SkPictureBackedGlyphDrawable::onGetBounds() {
    return fPicture->cullRect();
}

size_t SkPictureBackedGlyphDrawable::onApproximateBytesUsed() {
    return sizeof(SkPictureBackedGlyphDrawable) + fPicture->approximateBytesUsed();
}

void SkPictureBackedGlyphDrawable::onDraw(SkCanvas* canvas) {
    canvas->drawPicture(fPicture);
}

std::optional<SkGlyph> SkGlyph::MakeFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());
    RsPackedGlyphMetrics packed;
    packed.packed_id = buffer.readUInt();
    const SkVector advance = buffer.readPoint();
    packed.advance_x = advance.x();
    packed.advance_y = advance.y();
    packed.dimensions = buffer.readUInt();
    packed.left_top = buffer.readUInt();
    packed.mask_format = buffer.readUInt();

    RsUnpackedGlyphMetrics unpacked;
    if (!buffer.validate(skia_rs_text_glyph_unpack_metrics(&packed, &unpacked))) {
        return std::nullopt;
    }

    SkGlyph glyph{SkPackedGlyphID{unpacked.packed_id}};
    glyph.fAdvanceX = unpacked.advance_x;
    glyph.fAdvanceY = unpacked.advance_y;
    glyph.fWidth = unpacked.width;
    glyph.fHeight = unpacked.height;
    glyph.fLeft = unpacked.left;
    glyph.fTop = unpacked.top;
    glyph.fMaskFormat = static_cast<SkMask::Format>(unpacked.mask_format);
    SkDEBUGCODE(glyph.fAdvancesBoundsFormatAndInitialPathDone = true;)
    return glyph;
}

SkMask SkGlyph::mask() const {
    SkIRect bounds = SkIRect::MakeXYWH(fLeft, fTop, fWidth, fHeight);
    return SkMask(static_cast<const uint8_t*>(fImage), bounds, this->rowBytes(), fMaskFormat);
}

SkMask SkGlyph::mask(SkPoint position) const {
    SkASSERT(SkScalarIsInt(position.x()) && SkScalarIsInt(position.y()));
    SkIRect bounds = SkIRect::MakeXYWH(fLeft, fTop, fWidth, fHeight);
    bounds.offset(SkScalarFloorToInt(position.x()), SkScalarFloorToInt(position.y()));
    return SkMask(static_cast<const uint8_t*>(fImage), bounds, this->rowBytes(), fMaskFormat);
}

void SkGlyph::zeroMetrics() {
    fAdvanceX = 0;
    fAdvanceY = 0;
    fWidth    = 0;
    fHeight   = 0;
    fTop      = 0;
    fLeft     = 0;
}

size_t SkGlyph::formatAlignment() const {
    return skia_rs_text_glyph_format_alignment(static_cast<uint8_t>(this->maskFormat()));
}

size_t SkGlyph::allocImage(SkArenaAlloc* alloc) {
    SkASSERT(!this->isEmpty());
    auto size = this->imageSize();
    fImage = alloc->makeBytesAlignedTo(size, this->formatAlignment());
    return size;
}

bool SkGlyph::setImage(SkArenaAlloc* alloc, SkScalerContext* scalerContext) {
    if (!this->setImageHasBeenCalled()) {
        SkDEBUGCODE(SkMask::Format oldFormat = this->maskFormat());
        this->allocImage(alloc);
        scalerContext->getImage(*this);
        SkASSERT(oldFormat == this->maskFormat());
        return true;
    }
    return false;
}

bool SkGlyph::setImage(SkArenaAlloc* alloc, const void* image) {
    if (!this->setImageHasBeenCalled()) {
        this->allocImage(alloc);
        memcpy(fImage, image, this->imageSize());
        return true;
    }
    return false;
}

size_t SkGlyph::setMetricsAndImage(SkArenaAlloc* alloc, const SkGlyph& from) {
    SkASSERT(fImage == nullptr || from.fImage == nullptr);
    if (fImage == nullptr) {
        fAdvanceX = from.fAdvanceX;
        fAdvanceY = from.fAdvanceY;
        fWidth = from.fWidth;
        fHeight = from.fHeight;
        fTop = from.fTop;
        fLeft = from.fLeft;
        fScalerContextBits = from.fScalerContextBits;
        fMaskFormat = from.fMaskFormat;

        if (from.fImage != nullptr && this->setImage(alloc, from.image())) {
            return this->imageSize();
        }
        SkDEBUGCODE(fAdvancesBoundsFormatAndInitialPathDone = from.fAdvancesBoundsFormatAndInitialPathDone;)
    }
    return 0;
}

size_t SkGlyph::rowBytes() const {
    return skia_rs_text_glyph_row_bytes(fWidth, static_cast<uint8_t>(fMaskFormat));
}

size_t SkGlyph::rowBytesUsingFormat(SkMask::Format format) const {
    return skia_rs_text_glyph_row_bytes(fWidth, static_cast<uint8_t>(format));
}

size_t SkGlyph::imageSize() const {
    return skia_rs_text_glyph_image_size(fWidth, fHeight, static_cast<uint8_t>(fMaskFormat));
}

void SkGlyph::installPath(SkArenaAlloc* alloc, const SkPath* path, bool hairline, bool modified) {
    SkASSERT(fPathData == nullptr);
    SkASSERT(!this->setPathHasBeenCalled());
    fPathData = alloc->make<SkGlyph::PathData>();
    if (path != nullptr) {
        fPathData->fPath = *path;
        fPathData->fHasPath = true;
        fPathData->fHairline = hairline;
        fPathData->fModified = modified;
    }
}

bool SkGlyph::setPath(SkArenaAlloc* alloc, SkScalerContext* scalerContext) {
    if (!this->setPathHasBeenCalled()) {
        scalerContext->getPath(*this, alloc);
        SkASSERT(this->setPathHasBeenCalled());
        return this->path() != nullptr;
    }
    return false;
}

bool SkGlyph::setPath(SkArenaAlloc* alloc, const SkPath* path, bool hairline, bool modified) {
    if (!this->setPathHasBeenCalled()) {
        this->installPath(alloc, path, hairline, modified);
        return this->path() != nullptr;
    }
    return false;
}

const SkPath* SkGlyph::path() const {
    SkASSERT(this->setPathHasBeenCalled());
    if (fPathData->fHasPath) {
        return &fPathData->fPath;
    }
    return nullptr;
}

bool SkGlyph::pathIsHairline() const {
    SkASSERT(this->setPathHasBeenCalled());
    return fPathData->fHairline;
}

bool SkGlyph::pathIsModified() const {
    SkASSERT(this->setPathHasBeenCalled());
    return fPathData->fModified;
}

void SkGlyph::installDrawable(SkArenaAlloc* alloc, sk_sp<SkDrawable> drawable) {
    SkASSERT(fDrawableData == nullptr);
    SkASSERT(!this->setDrawableHasBeenCalled());
    fDrawableData = alloc->make<SkGlyph::DrawableData>();
    if (drawable != nullptr) {
        fDrawableData->fDrawable = std::move(drawable);
        fDrawableData->fDrawable->getGenerationID();
        fDrawableData->fHasDrawable = true;
    }
}

bool SkGlyph::setDrawable(SkArenaAlloc* alloc, SkScalerContext* scalerContext) {
    if (!this->setDrawableHasBeenCalled()) {
        sk_sp<SkDrawable> drawable = scalerContext->getDrawable(*this);
        this->installDrawable(alloc, std::move(drawable));
        return this->drawable() != nullptr;
    }
    return false;
}

bool SkGlyph::setDrawable(SkArenaAlloc* alloc, sk_sp<SkDrawable> drawable) {
    if (!this->setDrawableHasBeenCalled()) {
        this->installDrawable(alloc, std::move(drawable));
        return this->drawable() != nullptr;
    }
    return false;
}

SkDrawable* SkGlyph::drawable() const {
    SkASSERT(this->setDrawableHasBeenCalled());
    if (fDrawableData->fHasDrawable) {
        return fDrawableData->fDrawable.get();
    }
    return nullptr;
}

void SkGlyph::flattenMetrics(SkWriteBuffer& buffer) const {
    RsPackedGlyphMetrics packed;
    skia_rs_text_glyph_pack_metrics(
            fID.value(), fAdvanceX, fAdvanceY, fWidth, fHeight, fLeft, fTop,
            static_cast<uint8_t>(fMaskFormat), &packed);
    buffer.writeUInt(packed.packed_id);
    buffer.writePoint({packed.advance_x, packed.advance_y});
    buffer.writeUInt(packed.dimensions);
    buffer.writeUInt(packed.left_top);
    buffer.writeUInt(packed.mask_format);
}

void SkGlyph::flattenImage(SkWriteBuffer& buffer) const {
    if (!this->isEmpty() && SkGlyphDigest::FitsInAtlas(*this)) {
        SkASSERT(this->setImageHasBeenCalled());
        buffer.writeByteArray(this->image(), this->imageSize());
    }
}

size_t SkGlyph::addImageFromBuffer(SkReadBuffer& buffer, SkArenaAlloc* alloc) {
    SkASSERT(buffer.isValid());
    if (this->isEmpty() || !SkGlyphDigest::FitsInAtlas(*this)) {
        return 0;
    }

    size_t memoryIncrease = 0;
    void* imageData = alloc->makeBytesAlignedTo(this->imageSize(), this->formatAlignment());
    buffer.readByteArray(imageData, this->imageSize());
    if (buffer.isValid()) {
        this->installImage(imageData);
        memoryIncrease += this->imageSize();
    }
    return memoryIncrease;
}

void SkGlyph::flattenPath(SkWriteBuffer& buffer) const {
    SkASSERT(this->setPathHasBeenCalled());
    const bool hasPath = this->path() != nullptr;
    buffer.writeBool(hasPath);
    if (hasPath) {
        buffer.writeBool(this->pathIsHairline());
        buffer.writeBool(this->pathIsModified());
        buffer.writePath(*this->path());
    }
}

size_t SkGlyph::addPathFromBuffer(SkReadBuffer& buffer, SkArenaAlloc* alloc) {
    SkASSERT(buffer.isValid());
    size_t memoryIncrease = 0;
    const bool hasPath = buffer.readBool();
    if (!buffer.isValid()) {
        return 0;
    }
    if (hasPath) {
        const bool pathIsHairline = buffer.readBool();
        const bool pathIsModified = buffer.readBool();
        if (auto path = buffer.readPath()) {
            if (fMaskFormat != SkMask::kBW_Format &&
                fMaskFormat != SkMask::kA8_Format &&
                fMaskFormat != SkMask::kLCD16_Format) {
                buffer.validate(false);
                return 0;
            }
            if (this->setPath(alloc, &path.value(), pathIsHairline, pathIsModified)) {
                memoryIncrease += path->approximateBytesUsed();
            }
        }
    } else {
        this->setPath(alloc, nullptr, false, false);
    }
    return memoryIncrease;
}

void SkGlyph::flattenDrawable(SkWriteBuffer& buffer) const {
    SkASSERT(this->setDrawableHasBeenCalled());
    if (this->isEmpty() || this->drawable() == nullptr) {
        SkPictureBackedGlyphDrawable::FlattenDrawable(buffer, nullptr);
        return;
    }
    SkPictureBackedGlyphDrawable::FlattenDrawable(buffer, this->drawable());
}

size_t SkGlyph::addDrawableFromBuffer(SkReadBuffer& buffer, SkArenaAlloc* alloc) {
    SkASSERT(buffer.isValid());
    sk_sp<SkDrawable> drawable = SkPictureBackedGlyphDrawable::MakeFromBuffer(buffer);
    if (!buffer.isValid()) {
        return 0;
    }
    if (this->setDrawable(alloc, std::move(drawable))) {
        return this->drawable()->approximateBytesUsed();
    }
    return 0;
}

void SkGlyph::ensureIntercepts(const SkScalar* bounds, SkScalar scale, SkScalar xPos,
                               SkScalar* array, int* count, SkArenaAlloc* alloc) {
    auto offsetResults = [scale, xPos](
            const SkGlyph::Intercept* intercept, SkScalar* array, int* count) {
        if (array) {
            array += *count;
            for (int index = 0; index < 2; index++) {
                *array++ = intercept->fInterval[index] * scale + xPos;
            }
        }
        *count += 2;
    };

    const SkGlyph::Intercept* match = nullptr;
    if (fPathData != nullptr) {
        const SkGlyph::Intercept* it = fPathData->fIntercept;
        while (it != nullptr) {
            if (bounds[0] == it->fBounds[0] && bounds[1] == it->fBounds[1]) {
                match = it;
                break;
            }
            it = it->fNext;
        }
    }

    if (match != nullptr) {
        if (match->fInterval[0] < match->fInterval[1]) {
            offsetResults(match, array, count);
        }
        return;
    }

    SkGlyph::Intercept* intercept = alloc->make<SkGlyph::Intercept>();
    intercept->fNext = fPathData->fIntercept;
    intercept->fBounds[0] = bounds[0];
    intercept->fBounds[1] = bounds[1];
    intercept->fInterval[0] = SK_ScalarMax;
    intercept->fInterval[1] = SK_ScalarMin;
    fPathData->fIntercept = intercept;

    const SkPath* path = &(fPathData->fPath);
    const SkRect& pathBounds = path->getBounds();
    if (pathBounds.fBottom < bounds[0] || bounds[1] < pathBounds.fTop) {
        return;
    }

    std::vector<RsPathSegment> segs;
    SkPath::Iter iter(*path, false);
    while (auto rec = iter.next()) {
        RsPathSegment seg{};
        seg.verb = static_cast<uint8_t>(rec->fVerb);
        const SkSpan<const SkPoint> pts = rec->fPoints;
        size_t n = std::min<size_t>(pts.size(), 4);
        for (size_t i = 0; i < n; ++i) {
            seg.pts[i] = pts[i];
        }
        if (rec->fVerb == SkPathVerb::kConic) {
            SkDEBUGFAIL("There should be no conic primitives in glyph outlines.");
            continue;
        }
        segs.push_back(seg);
    }

    skia_rs_text_glyph_calculate_path_gap(
            bounds[0], bounds[1], segs.data(), segs.size(), intercept->fInterval);

    if (intercept->fInterval[0] >= intercept->fInterval[1]) {
        intercept->fInterval[0] = SK_ScalarMax;
        intercept->fInterval[1] = SK_ScalarMin;
        return;
    }
    offsetResults(intercept, array, count);
}

SkGlyphDigest::SkGlyphDigest(size_t index, const SkGlyph& glyph)
        : fPackedID{SkTo<uint64_t>(glyph.getPackedID().value())}
        , fIndex{SkTo<uint64_t>(index)}
        , fIsEmpty(glyph.isEmpty())
        , fFormat(glyph.maskFormat())
        , fActions{skia_rs_text_glyph_digest_init_actions(glyph.isEmpty())}
        , fLeft{SkTo<int16_t>(glyph.left())}
        , fTop{SkTo<int16_t>(glyph.top())}
        , fWidth{SkTo<uint16_t>(glyph.width())}
        , fHeight{SkTo<uint16_t>(glyph.height())} {}

void SkGlyphDigest::setActionFor(skglyph::ActionType actionType,
                                 SkGlyph* glyph,
                                 StrikeForGPU* strike) {
    if (this->actionFor(actionType) == GlyphAction::kUnset) {
        GlyphAction action = GlyphAction::kReject;
        switch (actionType) {
            case kDirectMask:
                if (this->fitsInAtlasDirect()) {
                    action = GlyphAction::kAccept;
                }
                break;
            case kDirectMaskCPU:
                if (strike->prepareForImage(glyph)) {
                    SkASSERT(!glyph->isEmpty());
                    action = GlyphAction::kAccept;
                }
                break;
            case kMask:
                if (this->fitsInAtlasInterpolated()) {
                    action = GlyphAction::kAccept;
                }
                break;
            case kSDFT:
                if (this->fitsInAtlasDirect() &&
                    this->maskFormat() == SkMask::Format::kSDF_Format) {
                    action = GlyphAction::kAccept;
                }
                break;
            case kPath:
                if (strike->prepareForPath(glyph)) {
                    action = GlyphAction::kAccept;
                }
                break;
            case kDrawable:
                if (strike->prepareForDrawable(glyph)) {
                    action = GlyphAction::kAccept;
                }
                break;
        }
        this->setAction(actionType, action);
    }
}

bool SkGlyphDigest::FitsInAtlas(const SkGlyph& glyph) {
    return skia_rs_text_glyph_fits_in_atlas(
            static_cast<uint16_t>(glyph.width()), static_cast<uint16_t>(glyph.height()));
}

SkVector SkGlyphPositionRoundingSpec::HalfAxisSampleFreq(
        bool isSubpixel, SkAxisAlignment axisAlignment) {
    RsGlyphPositionRoundingSpec spec;
    skia_rs_text_glyph_rounding_spec(isSubpixel, static_cast<uint32_t>(axisAlignment), &spec);
    return spec.half_axis_sample_freq;
}

SkIPoint SkGlyphPositionRoundingSpec::IgnorePositionMask(
        bool isSubpixel, SkAxisAlignment axisAlignment) {
    RsGlyphPositionRoundingSpec spec;
    skia_rs_text_glyph_rounding_spec(isSubpixel, static_cast<uint32_t>(axisAlignment), &spec);
    return spec.ignore_position_mask;
}

SkIPoint SkGlyphPositionRoundingSpec::IgnorePositionFieldMask(
        bool isSubpixel, SkAxisAlignment axisAlignment) {
    RsGlyphPositionRoundingSpec spec;
    skia_rs_text_glyph_rounding_spec(isSubpixel, static_cast<uint32_t>(axisAlignment), &spec);
    return spec.ignore_position_field_mask;
}

SkGlyphPositionRoundingSpec::SkGlyphPositionRoundingSpec(
        bool isSubpixel, SkAxisAlignment axisAlignment)
        : halfAxisSampleFreq{HalfAxisSampleFreq(isSubpixel, axisAlignment)}
        , ignorePositionMask{IgnorePositionMask(isSubpixel, axisAlignment)}
        , ignorePositionFieldMask{IgnorePositionFieldMask(isSubpixel, axisAlignment)} {}

// ============================================================================
// SkScalerContext.cpp
// ============================================================================

SkScalerContextRec SkScalerContext::PreprocessRec(const SkTypeface& typeface,
                                                  const SkScalerContextEffects& effects,
                                                  const SkDescriptor& desc) {
    SkScalerContextRec rec =
            *static_cast<const SkScalerContextRec*>(desc.findEntry(kRec_SkDescriptorTag, nullptr));
    typeface.onFilterRec(&rec);
    skia_rs_text_scaler_rec_preprocess_post_filter(&rec, effects.fMaskFilter != nullptr);
    return rec;
}

SkScalerContext::SkScalerContext(SkTypeface& typeface, const SkScalerContextEffects& effects,
                                 const SkDescriptor* desc)
    : fRec(PreprocessRec(typeface, effects, *desc))
    , fTypeface(typeface)
    , fPathEffect(sk_ref_sp(effects.fPathEffect))
    , fMaskFilter(sk_ref_sp(effects.fMaskFilter))
    , fGenerateImageFromPath(fRec.fFrameWidth >= 0 || fPathEffect != nullptr)
    , fPreBlend(fMaskFilter ? SkMaskGamma::PreBlend() : SkScalerContext::GetMaskPreBlend(fRec)) {}

SkScalerContext::~SkScalerContext() {}

static SkMutex& mask_gamma_cache_mutex() {
    static SkMutex& mutex = *(new SkMutex);
    return mutex;
}

static const SkMaskGamma& linear_gamma() {
    static const SkMaskGamma kLinear;
    return kLinear;
}

static SkMaskGamma* gDefaultMaskGamma = nullptr;
static SkMaskGamma* gMaskGamma = nullptr;
static uint8_t gContrast = 0;
static uint8_t gGamma = 0;

const SkMaskGamma& SkScalerContextRec::CachedMaskGamma(uint8_t contrast, uint8_t gamma) {
    mask_gamma_cache_mutex().assertHeld();

    constexpr uint8_t contrast0 = InternalContrastFromExternal(0);
    constexpr uint8_t gamma1 = InternalGammaFromExternal(1);
    if (contrast0 == contrast && gamma1 == gamma) {
        return linear_gamma();
    }
    constexpr uint8_t defaultContrast = InternalContrastFromExternal(SK_GAMMA_CONTRAST);
    constexpr uint8_t defaultGamma = InternalGammaFromExternal(SK_GAMMA_EXPONENT);
    if (defaultContrast == contrast && defaultGamma == gamma) {
        if (!gDefaultMaskGamma) {
            gDefaultMaskGamma = new SkMaskGamma(ExternalContrastFromInternal(contrast),
                                                ExternalGammaFromInternal(gamma));
        }
        return *gDefaultMaskGamma;
    }
    if (!gMaskGamma || gContrast != contrast || gGamma != gamma) {
        SkSafeUnref(gMaskGamma);
        gMaskGamma = new SkMaskGamma(ExternalContrastFromInternal(contrast),
                                     ExternalGammaFromInternal(gamma));
        gContrast = contrast;
        gGamma = gamma;
    }
    return *gMaskGamma;
}

SkMaskGamma::PreBlend SkScalerContext::GetMaskPreBlend(const SkScalerContextRec& rec) {
    SkAutoMutexExclusive ama(mask_gamma_cache_mutex());
    const SkMaskGamma& maskGamma = rec.cachedMaskGamma();
    return maskGamma.preBlend(rec.getLuminanceColor());
}

size_t SkScalerContext::GetGammaLUTSize(SkScalar contrast, SkScalar deviceGamma,
                                        int* width, int* height) {
    SkAutoMutexExclusive ama(mask_gamma_cache_mutex());
    const SkMaskGamma& maskGamma = SkScalerContextRec::CachedMaskGamma(
            SkScalerContextRec::InternalContrastFromExternal(contrast),
            SkScalerContextRec::InternalGammaFromExternal(deviceGamma));
    maskGamma.getGammaTableDimensions(width, height);
    return maskGamma.getGammaTableSizeInBytes();
}

bool SkScalerContext::GetGammaLUTData(SkScalar contrast, SkScalar deviceGamma, uint8_t* data) {
    SkAutoMutexExclusive ama(mask_gamma_cache_mutex());
    const SkMaskGamma& maskGamma = SkScalerContextRec::CachedMaskGamma(
            SkScalerContextRec::InternalContrastFromExternal(contrast),
            SkScalerContextRec::InternalGammaFromExternal(deviceGamma));
    const uint8_t* gammaTables = maskGamma.getGammaTables();
    if (!gammaTables) {
        return false;
    }
    memcpy(data, gammaTables, maskGamma.getGammaTableSizeInBytes());
    return true;
}

SkGlyph SkScalerContext::makeGlyph(SkPackedGlyphID packedID, SkArenaAlloc* alloc) {
    return internalMakeGlyph(packedID, fRec.fMaskFormat, alloc);
}

void SkScalerContext::SaturateGlyphBounds(SkGlyph* glyph, SkRect&& r) {
    RsSaturatedGlyphBounds b;
    skia_rs_text_scaler_saturate_bounds_rect(&r, &b);
    glyph->fLeft   = b.left;
    glyph->fTop    = b.top;
    glyph->fWidth  = b.width;
    glyph->fHeight = b.height;
}

void SkScalerContext::SaturateGlyphBounds(SkGlyph* glyph, SkIRect const& r) {
    RsSaturatedGlyphBounds b;
    skia_rs_text_scaler_saturate_bounds_irect(&r, &b);
    glyph->fLeft   = b.left;
    glyph->fTop    = b.top;
    glyph->fWidth  = b.width;
    glyph->fHeight = b.height;
}

void SkScalerContext::GenerateMetricsFromPath(
        SkGlyph* glyph, const SkPath& devPath, SkMask::Format /*format*/,
        const bool verticalLCD, const bool a8FromLCD, const bool hairline) {
    SkRect bounds = devPath.getBounds();
    RsSaturatedGlyphBounds out;
    skia_rs_text_scaler_generate_metrics_from_path_bounds(
            static_cast<uint8_t>(glyph->fMaskFormat), &bounds,
            verticalLCD, a8FromLCD, hairline, &out);
    glyph->fMaskFormat = static_cast<SkMask::Format>(out.mask_format);
    glyph->fLeft       = out.left;
    glyph->fTop        = out.top;
    glyph->fWidth      = out.width;
    glyph->fHeight     = out.height;
}

SkGlyph SkScalerContext::internalMakeGlyph(SkPackedGlyphID packedID, SkMask::Format format,
                                           SkArenaAlloc* alloc) {
    auto zeroBounds = [](SkGlyph& glyph) {
        glyph.fLeft   = 0;
        glyph.fTop    = 0;
        glyph.fWidth  = 0;
        glyph.fHeight = 0;
    };

    SkGlyph glyph{packedID};
    glyph.fMaskFormat = format;
    GlyphMetrics mx = this->generateMetrics(glyph, alloc);
    SkASSERT(!mx.neverRequestPath || !mx.computeFromPath);

    glyph.fAdvanceX = mx.advance.fX;
    glyph.fAdvanceY = mx.advance.fY;
    glyph.fMaskFormat = mx.maskFormat;
    glyph.fScalerContextBits = mx.extraBits;

    if (mx.computeFromPath || (fGenerateImageFromPath && !mx.neverRequestPath)) {
        SkDEBUGCODE(glyph.fAdvancesBoundsFormatAndInitialPathDone = true;)
        this->internalGetPath(glyph, alloc, std::move(mx.generatedPath));
        const SkPath* devPath = glyph.path();
        if (devPath) {
            const bool doVert = SkToBool(fRec.fFlags & SkScalerContext::kLCD_Vertical_Flag);
            const bool a8LCD = SkToBool(fRec.fFlags & SkScalerContext::kGenA8FromLCD_Flag);
            const bool hairline = glyph.pathIsHairline();
            GenerateMetricsFromPath(&glyph, *devPath, format, doVert, a8LCD, hairline);
        }
    } else {
        SaturateGlyphBounds(&glyph, std::move(mx.bounds));
        if (mx.neverRequestPath) {
            glyph.setPath(alloc, nullptr, false, false);
        }
    }
    SkDEBUGCODE(glyph.fAdvancesBoundsFormatAndInitialPathDone = true;)

    if (0 == glyph.fWidth || 0 == glyph.fHeight) {
        zeroBounds(glyph);
        return glyph;
    }

    if (fMaskFilter) {
        SkMask src(nullptr, glyph.iRect(), glyph.rowBytes(), glyph.maskFormat());
        SkMaskBuilder dst;

        if (as_MFB(fMaskFilter)->filterMask(&dst, src, fRec.getMatrixFrom2x2(), nullptr)) {
            if (dst.fBounds.isEmpty()) {
                zeroBounds(glyph);
                return glyph;
            }
            SkASSERT(dst.fImage == nullptr);
            SaturateGlyphBounds(&glyph, dst.fBounds);
            glyph.fMaskFormat = dst.fFormat;
        }
    }
    return glyph;
}

void SkScalerContext::generateImageFromPath(const SkGlyph& glyph, void* imageBuffer) {
    SkASSERT(glyph.setPathHasBeenCalled());
    const SkPath* devPath = glyph.path();
    SkASSERT_RELEASE(devPath);
    SkMaskBuilder mask(static_cast<uint8_t*>(imageBuffer),
                       glyph.iRect(), glyph.rowBytes(), glyph.maskFormat());
    SkASSERT(SkMask::kARGB32_Format != mask.fFormat);
    const bool doBGR = SkToBool(fRec.fFlags & SkScalerContext::kLCD_BGROrder_Flag);
    const bool doVert = SkToBool(fRec.fFlags & SkScalerContext::kLCD_Vertical_Flag);
    const bool a8LCD = SkToBool(fRec.fFlags & SkScalerContext::kGenA8FromLCD_Flag);
    const bool hairline = glyph.pathIsHairline();
    GenerateImageFromPath(mask, *devPath, fPreBlend, doBGR, doVert, a8LCD, hairline);
}

void SkScalerContext::GenerateImageFromPath(
        SkMaskBuilder& dstMask, const SkPath& path, const SkMaskGamma::PreBlend& maskPreBlend,
        const bool doBGR, const bool verticalLCD, const bool a8FromLCD, const bool hairline) {
    SkASSERT(dstMask.fFormat == SkMask::kBW_Format ||
             dstMask.fFormat == SkMask::kA8_Format ||
             dstMask.fFormat == SkMask::kLCD16_Format);

    SkPaint paint;
    SkPath strokePath;
    const SkPath* pathToUse = &path;

    paint.setStroke(hairline);
    paint.setAntiAlias(SkMask::kBW_Format != dstMask.fFormat);

#if defined(SK_USE_LCD_TEXT_3X_FILTER)
    const int samplesPerPixel = dstMask.fFormat == SkMask::kLCD16_Format ? 3 : 4;
#else
    constexpr int samplesPerPixel = 4;
#endif

    RsPathMaskRasterPlan plan;
    skia_rs_text_scaler_plan_image_from_path(
            &dstMask.fBounds, static_cast<uint8_t>(dstMask.fFormat),
            verticalLCD, a8FromLCD, samplesPerPixel, &plan);

    if (plan.from_lcd) {
        SkStrokeRec rec(SkStrokeRec::kFill_InitStyle);
        if (hairline) {
            rec.setStrokeStyle(1.0f, false);
            rec.setStrokeParams(SkPaint::kButt_Cap, SkPaint::kRound_Join, 0.0f);
        }

        SkPathBuilder builder;
        if (rec.needToApply() && rec.applyToPath(&builder, path)) {
            strokePath = builder.detach();
            pathToUse = &strokePath;
            paint.setStyle(SkPaint::kFill_Style);
        }
    }

    SkRasterClip clip;
    clip.setRect(SkIRect::MakeWH(plan.dst_w, plan.dst_h));

    const SkImageInfo info = SkImageInfo::MakeA8(plan.dst_w, plan.dst_h);
    SkAutoPixmapStorage dst;

    if (plan.intermediate_dst) {
        if (!dst.tryAlloc(info)) {
            sk_bzero(dstMask.image(), dstMask.computeImageSize());
            return;
        }
        sk_bzero(dst.writable_addr(), dst.computeByteSize());
    } else {
        dst.reset(info, dstMask.image(), dstMask.fRowBytes);
    }
    sk_bzero(dstMask.image(), dstMask.computeImageSize());

    skcpu::Draw draw;
    draw.fBlitterChooser = SkA8Blitter_Choose;
    draw.fDst            = dst;
    draw.fRC             = &clip;
    draw.fCTM            = &plan.matrix;
    draw.drawPath(*pathToUse, paint, nullptr);

    const bool hasPreBlend = maskPreBlend.isApplicable();
    const uint8_t* pbR = hasPreBlend ? maskPreBlend.fR : nullptr;
    const uint8_t* pbG = hasPreBlend ? maskPreBlend.fG : nullptr;
    const uint8_t* pbB = hasPreBlend ? maskPreBlend.fB : nullptr;

    switch (dstMask.fFormat) {
        case SkMask::kBW_Format:
            skia_rs_text_scaler_pack_a8_to_a1(
                    dstMask.image(), dstMask.fRowBytes,
                    dst.addr8(0, 0), dst.rowBytes(),
                    static_cast<size_t>(dstMask.fBounds.width()),
                    static_cast<size_t>(dstMask.fBounds.height()));
            break;
        case SkMask::kA8_Format:
            if (plan.from_lcd) {
                skia_rs_text_scaler_pack_4xh_to_mask(
                        dst.addr8(0, 0), static_cast<size_t>(dst.width()),
                        static_cast<size_t>(dst.height()), dst.rowBytes(),
                        dstMask.image(), dstMask.fRowBytes, dstMask.computeImageSize(),
                        true, pbR, pbG, pbB, doBGR, verticalLCD);
            } else if (hasPreBlend) {
                skia_rs_text_scaler_apply_lut_a8(
                        dstMask.image(), dstMask.fRowBytes,
                        static_cast<size_t>(dstMask.fBounds.width()),
                        static_cast<size_t>(dstMask.fBounds.height()),
                        maskPreBlend.fG);
            }
            break;
        case SkMask::kLCD16_Format:
#if defined(SK_USE_LCD_TEXT_3X_FILTER)
            skia_rs_text_scaler_pack_3xh_to_mask(
                    dst.addr8(0, 0), static_cast<size_t>(dst.width()),
                    static_cast<size_t>(dst.height()), dst.rowBytes(),
                    dstMask.image(), dstMask.fRowBytes, dstMask.computeImageSize(),
                    pbR, pbG, pbB, doBGR, verticalLCD);
#else
            skia_rs_text_scaler_pack_4xh_to_mask(
                    dst.addr8(0, 0), static_cast<size_t>(dst.width()),
                    static_cast<size_t>(dst.height()), dst.rowBytes(),
                    dstMask.image(), dstMask.fRowBytes, dstMask.computeImageSize(),
                    false, pbR, pbG, pbB, doBGR, verticalLCD);
#endif
            break;
        default:
            SkUNREACHABLE;
            break;
    }
}

void SkScalerContext::getImage(const SkGlyph& origGlyph) {
    SkASSERT(origGlyph.fAdvancesBoundsFormatAndInitialPathDone);
    SkASSERT(!origGlyph.isEmpty() && !origGlyph.imageTooLarge() && origGlyph.fImage != nullptr);

    const SkGlyph* unfilteredGlyph = &origGlyph;
    SkAutoMalloc tmpGlyphImageStorage;
    SkGlyph tmpGlyph;
    SkSTArenaAlloc<sizeof(SkGlyph::PathData)> tmpGlyphPathDataStorage;
    if (fMaskFilter) {
        sk_sp<SkMaskFilter> mf = std::move(fMaskFilter);
        tmpGlyph = this->makeGlyph(origGlyph.getPackedID(), &tmpGlyphPathDataStorage);
        fMaskFilter = std::move(mf);

        if (tmpGlyph.isEmpty() || tmpGlyph.imageTooLarge()) {
            sk_bzero(origGlyph.fImage, origGlyph.imageSize());
            return;
        }

        if (tmpGlyph.fMaskFormat == origGlyph.fMaskFormat &&
            tmpGlyph.imageSize() <= origGlyph.imageSize()) {
            tmpGlyph.fImage = origGlyph.fImage;
        } else {
            tmpGlyphImageStorage.reset(tmpGlyph.imageSize());
            tmpGlyph.fImage = tmpGlyphImageStorage.get();
        }
        unfilteredGlyph = &tmpGlyph;
    }

    if (!fGenerateImageFromPath) {
        generateImage(*unfilteredGlyph, unfilteredGlyph->fImage);
    } else {
        SkASSERT(unfilteredGlyph->setPathHasBeenCalled());
        if (!unfilteredGlyph->path()) {
            generateImage(*unfilteredGlyph, unfilteredGlyph->fImage);
        } else {
            this->generateImageFromPath(*unfilteredGlyph, unfilteredGlyph->fImage);
        }
    }

    if (fMaskFilter) {
        SkASSERT(SkMask::k3D_Format != unfilteredGlyph->fMaskFormat);

        SkMaskBuilder srcMask;
        SkAutoMaskFreeImage srcMaskOwnedImage(nullptr);

        if (as_MFB(fMaskFilter)->filterMask(&srcMask, unfilteredGlyph->mask(),
                                            fRec.getMatrixFrom2x2(), nullptr)) {
            srcMaskOwnedImage.reset(srcMask.image());
        } else if (unfilteredGlyph->fImage == tmpGlyphImageStorage.get()) {
            srcMask = SkMaskBuilder(static_cast<uint8_t*>(unfilteredGlyph->fImage),
                                    unfilteredGlyph->iRect(), unfilteredGlyph->rowBytes(),
                                    unfilteredGlyph->maskFormat());
        } else if (origGlyph.iRect() == unfilteredGlyph->iRect()) {
            return;
        } else {
            srcMask = SkMaskBuilder(static_cast<uint8_t*>(unfilteredGlyph->fImage),
                                    unfilteredGlyph->iRect(), unfilteredGlyph->rowBytes(),
                                    unfilteredGlyph->maskFormat());
            size_t imageSize = unfilteredGlyph->imageSize();
            tmpGlyphImageStorage.reset(imageSize);
            srcMask.image() = static_cast<uint8_t*>(tmpGlyphImageStorage.get());
            memcpy(srcMask.image(), unfilteredGlyph->fImage, imageSize);
        }

        SkMaskBuilder dstMask = SkMaskBuilder(static_cast<uint8_t*>(origGlyph.fImage),
                                              origGlyph.iRect(), origGlyph.rowBytes(),
                                              origGlyph.maskFormat());

        const size_t srcPlaneBytes = srcMask.computeImageSize();
        const size_t dstPlaneBytes = dstMask.computeImageSize();
        const size_t srcTotalBytes = srcMask.computeTotalImageSize();
        const size_t dstTotalBytes = origGlyph.imageSize();

        skia_rs_text_scaler_clip_copy_filtered_mask(
                srcMask.fImage, srcTotalBytes, &srcMask.fBounds, srcMask.fRowBytes,
                static_cast<uint8_t>(srcMask.fFormat), srcPlaneBytes,
                dstMask.image(), dstTotalBytes, &dstMask.fBounds, dstMask.fRowBytes,
                static_cast<uint8_t>(origGlyph.fMaskFormat), dstPlaneBytes);
    }
}

void SkScalerContext::getPath(SkGlyph& glyph, SkArenaAlloc* alloc) {
    this->internalGetPath(glyph, alloc, std::nullopt);
}

sk_sp<SkDrawable> SkScalerContext::getDrawable(SkGlyph& glyph) {
    return this->generateDrawable(glyph);
}

sk_sp<SkDrawable> SkScalerContext::generateDrawable(const SkGlyph&) {
    return nullptr;
}

void SkScalerContext::getFontMetrics(SkFontMetrics* fm) {
    SkASSERT(fm);
    this->generateFontMetrics(fm);
}

void SkScalerContext::internalGetPath(SkGlyph& glyph, SkArenaAlloc* alloc,
                                      std::optional<GeneratedPath>&& generatedPath) {
    SkASSERT(glyph.fAdvancesBoundsFormatAndInitialPathDone);
    if (glyph.setPathHasBeenCalled()) {
        return;
    }

    if (!generatedPath) {
        generatedPath = this->generatePath(glyph);
    }
    if (!generatedPath) {
        glyph.setPath(alloc, (SkPath*)nullptr, false, false);
        return;
    }

    SkPath path = std::move(generatedPath->path);
    bool pathModified = std::move(generatedPath->modified);

    if (fRec.fFlags & SkScalerContext::kSubpixelPositioning_Flag) {
        SkPackedGlyphID glyphID = glyph.getPackedID();
        SkFixed dx = glyphID.getSubXFixed();
        SkFixed dy = glyphID.getSubYFixed();
        if (dx | dy) {
            pathModified = true;
            path = path.makeOffset(SkFixedToScalar(dx), SkFixedToScalar(dy));
        }
    }

    if (fRec.fFrameWidth < 0 && fPathEffect == nullptr) {
        glyph.setPath(alloc, &path, false, pathModified);
        return;
    }

    pathModified = true;

    SkMatrix matrix = fRec.getMatrixFrom2x2();
    auto inverse = matrix.invert();
    if (!inverse) {
        SkPath empty;
        glyph.setPath(alloc, &empty, false, pathModified);
        return;
    }
    auto localPath = path.makeTransform(*inverse);

    SkStrokeRec rec(SkStrokeRec::kFill_InitStyle);
    if (fRec.fFrameWidth >= 0) {
        rec.setStrokeStyle(fRec.fFrameWidth,
                           SkToBool(fRec.fFlags & kFrameAndFill_Flag));
        rec.setStrokeParams((SkPaint::Cap)fRec.fStrokeCap,
                            (SkPaint::Join)fRec.fStrokeJoin,
                            fRec.fMiterLimit);
    }

    if (fPathEffect) {
        SkPathBuilder builder;
        if (fPathEffect->filterPath(&builder, localPath, &rec, nullptr, matrix)) {
            localPath = builder.detach();
        }
    }

    if (rec.needToApply()) {
        SkPathBuilder builder;
        if (rec.applyToPath(&builder, localPath)) {
            localPath = builder.detach();
        }
    }

    auto devPath = localPath.makeTransform(matrix);
    glyph.setPath(alloc, &devPath, rec.isHairlineStyle(), pathModified);
}

SkMatrix SkScalerContextRec::getMatrixFrom2x2() const {
    SkMatrix m;
    skia_rs_text_scaler_rec_get_matrix_from_2x2(this, &m);
    return m;
}

SkMatrix SkScalerContextRec::getLocalMatrix() const {
    SkMatrix m;
    skia_rs_text_scaler_rec_get_local_matrix(this, &m);
    return m;
}

SkMatrix SkScalerContextRec::getSingleMatrix() const {
    SkMatrix m;
    skia_rs_text_scaler_rec_get_single_matrix(this, &m);
    return m;
}

bool SkScalerContextRec::computeMatrices(PreMatrixScale preMatrixScale, SkVector* s, SkMatrix* sA,
                                         SkMatrix* GsA, SkMatrix* G_inv, SkMatrix* A_out) const {
    return skia_rs_text_scaler_rec_compute_matrices(
            this, static_cast<int32_t>(preMatrixScale), s, sA, GsA, G_inv, A_out);
}

SkAxisAlignment SkScalerContext::computeAxisAlignmentForHText() const {
    return fRec.computeAxisAlignmentForHText();
}

SkAxisAlignment SkScalerContextRec::computeAxisAlignmentForHText() const {
    return static_cast<SkAxisAlignment>(skia_rs_text_scaler_rec_axis_alignment(this));
}

void SkScalerContextRec::setLuminanceColor(SkColor c) {
    skia_rs_text_scaler_rec_set_luminance_color(this, c);
}

void SkScalerContextRec::useStrokeForFakeBold() {
    skia_rs_text_scaler_rec_use_stroke_for_fake_bold(this);
}

void SkScalerContext::MakeRecAndEffects(const SkFont& font, const SkPaint& paint,
                                        const SkSurfaceProps& surfaceProps,
                                        SkScalerContextFlags scalerContextFlags,
                                        const SkMatrix& deviceMatrix,
                                        SkScalerContextRec* rec,
                                        SkScalerContextEffects* effects) {
    SkASSERT(!deviceMatrix.hasPerspective());

    SkTypeface* typeface = font.getTypeface();
    RsMakeRecParams params{};
    params.typeface_id = typeface->uniqueID();
    params.font_size = font.getSize();
    params.font_scale_x = font.getScaleX();
    params.font_skew_x = font.getSkewX();
    params.font_flags = SkFontPriv::Flags(font);
    params.font_edging = static_cast<uint8_t>(font.getEdging());
    params.font_hinting = static_cast<uint8_t>(font.getHinting());
    params.glyph_mask_needs_current_color = typeface->glyphMaskNeedsCurrentColor();

    params.paint_style = static_cast<uint8_t>(paint.getStyle());
    params.stroke_join = static_cast<uint8_t>(paint.getStrokeJoin());
    params.stroke_cap = static_cast<uint8_t>(paint.getStrokeCap());
    params.stroke_width = paint.getStrokeWidth();
    params.stroke_miter = paint.getStrokeMiter();
    params.paint_color = paint.getColor();
    params.luminance_color = SkPaintPriv::ComputeLuminanceColor(paint);

    params.pixel_geometry = static_cast<int32_t>(surfaceProps.pixelGeometry());
    params.text_gamma = surfaceProps.textGamma();
    params.text_contrast = surfaceProps.textContrast();
    params.scaler_context_flags = static_cast<uint32_t>(scalerContextFlags);

    // Ensure deviceMatrix type mask is populated before passing to Rust.
    (void)deviceMatrix.getType();
    skia_rs_text_scaler_rec_make(&params, &deviceMatrix, rec);
    *effects = SkScalerContextEffects{paint};
}

SkDescriptor* SkScalerContext::CreateDescriptorAndEffectsUsingPaint(
        const SkFont& font, const SkPaint& paint, const SkSurfaceProps& surfaceProps,
        SkScalerContextFlags scalerContextFlags, const SkMatrix& deviceMatrix,
        SkAutoDescriptor* ad, SkScalerContextEffects* effects) {
    SkScalerContextRec rec;
    MakeRecAndEffects(font, paint, surfaceProps, scalerContextFlags, deviceMatrix, &rec, effects);
    return AutoDescriptorGivenRecAndEffects(rec, *effects, ad);
}

static size_t calculate_size_and_flatten(const SkScalerContextRec& rec,
                                         const SkScalerContextEffects& effects,
                                         SkBinaryWriteBuffer* effectBuffer) {
    size_t descSize = sizeof(rec);
    int entryCount = 1;

    if (effects.fPathEffect || effects.fMaskFilter) {
        if (effects.fPathEffect) { effectBuffer->writeFlattenable(effects.fPathEffect); }
        if (effects.fMaskFilter) { effectBuffer->writeFlattenable(effects.fMaskFilter); }
        entryCount += 1;
        descSize += effectBuffer->bytesWritten();
    }

    descSize += SkDescriptor::ComputeOverhead(entryCount);
    return descSize;
}

static void generate_descriptor(const SkScalerContextRec& rec,
                                const SkBinaryWriteBuffer& effectBuffer,
                                SkDescriptor* desc) {
    desc->addEntry(kRec_SkDescriptorTag, sizeof(rec), &rec);
    if (effectBuffer.bytesWritten() > 0) {
        effectBuffer.writeToMemory(desc->addEntry(kEffects_SkDescriptorTag,
                                                  effectBuffer.bytesWritten(),
                                                  nullptr));
    }
    desc->computeChecksum();
}

SkDescriptor* SkScalerContext::AutoDescriptorGivenRecAndEffects(
        const SkScalerContextRec& rec,
        const SkScalerContextEffects& effects,
        SkAutoDescriptor* ad) {
    SkBinaryWriteBuffer buf({});
    ad->reset(calculate_size_and_flatten(rec, effects, &buf));
    generate_descriptor(rec, buf, ad->getDesc());
    return ad->getDesc();
}

std::unique_ptr<SkDescriptor> SkScalerContext::DescriptorGivenRecAndEffects(
        const SkScalerContextRec& rec,
        const SkScalerContextEffects& effects) {
    SkBinaryWriteBuffer buf({});
    auto desc = SkDescriptor::Alloc(calculate_size_and_flatten(rec, effects, &buf));
    generate_descriptor(rec, buf, desc.get());
    return desc;
}

void SkScalerContext::DescriptorBufferGiveRec(const SkScalerContextRec& rec, void* buffer) {
    generate_descriptor(rec, SkBinaryWriteBuffer({}), (SkDescriptor*)buffer);
}

bool SkScalerContext::CheckBufferSizeForRec(const SkScalerContextRec& rec,
                                            const SkScalerContextEffects& effects,
                                            size_t size) {
    SkBinaryWriteBuffer buf({});
    return size >= calculate_size_and_flatten(rec, effects, &buf);
}

std::unique_ptr<SkScalerContext> SkScalerContext::MakeEmpty(
        SkTypeface& typeface, const SkScalerContextEffects& effects,
        const SkDescriptor* desc) {
    class SkScalerContext_Empty : public SkScalerContext {
    public:
        SkScalerContext_Empty(SkTypeface& typeface, const SkScalerContextEffects& effects,
                              const SkDescriptor* desc)
                : SkScalerContext(typeface, effects, desc) {}

    protected:
        GlyphMetrics generateMetrics(const SkGlyph& glyph, SkArenaAlloc*) override {
            return {glyph.maskFormat()};
        }
        void generateImage(const SkGlyph&, void*) override {}
        std::optional<GeneratedPath> generatePath(const SkGlyph&) override {
            return {};
        }
        void generateFontMetrics(SkFontMetrics* metrics) override {
            if (metrics) {
                sk_bzero(metrics, sizeof(*metrics));
            }
        }
    };

    return std::make_unique<SkScalerContext_Empty>(typeface, effects, desc);
}
