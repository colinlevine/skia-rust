/*
 * Safe Rust bridge for:
 *   src/core/SkTextBlob.cpp
 *   src/text/GlyphRun.cpp
 *   src/core/SkGlyphRunPainter.cpp
 *   src/text/SlugFromBuffer.cpp
 * (Phase 1 Layer 5.1 skia-rs-text, GN target :core).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkFont.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathBuilder.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRSXform.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSerialProcs.h"
#include "include/core/SkSpan.h"
#include "include/core/SkTextBlob.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkFloatingPoint.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTLogic.h"
#include "include/private/SkTo.h"
#include "include/private/chromium/Slug.h"
#include "src/core/SkDraw.h"
#include "src/core/SkFontPriv.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkGlyphRunPainter.h"
#include "src/core/SkMask.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkSafeMath.h"
#include "src/core/SkScalerContext.h"
#include "src/core/SkStrike.h"
#include "src/core/SkStrikeSpec.h"
#include "src/core/SkTextBlobPriv.h"
#include "src/core/SkWriteBuffer.h"
#include "src/text/GlyphRun.h"

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <optional>
#include <tuple>
#include <vector>

using namespace skglyph;
using namespace skia_private;
using namespace sktext;

namespace {

struct RsRunDeserialSizes {
    bool valid;
    size_t glyph_size;
    size_t pos_size;
    size_t cluster_size;
    size_t total_size;
};

struct RsDirectMaskQuantized {
    bool finite;
    uint32_t packed_glyph_id;
    SkPoint rounded_pos;
};

struct RsInterceptSetup {
    float scale;
    float stroke_width;
};

struct RunFontStorageEquivalent {
    SkScalar fSize, fScaleX;
    void*    fTypeface;
    SkScalar fSkewX;
    uint32_t fFlags;
};
static_assert(sizeof(SkFont) == sizeof(RunFontStorageEquivalent), "runfont_should_stay_packed");

struct RunRecordStorageEquivalent {
    SkFont   fFont;
    SkPoint  fOffset;
    uint32_t fCount;
    uint32_t fFlags;
    SkDEBUGCODE(unsigned fMagic;)
};

union PositioningAndExtended {
    int32_t intValue;
    struct {
        uint8_t  positioning;
        uint8_t  extended;
        uint16_t padding;
    };
};
static_assert(sizeof(PositioningAndExtended) == sizeof(int32_t), "");
static_assert(sizeof(SkRect) == 16, "SkRect layout");
static_assert(sizeof(SkPoint) == 8, "SkPoint layout");
static_assert(sizeof(SkIPoint) == 8, "SkIPoint layout");
static_assert(sizeof(SkMatrix) == 40, "SkMatrix layout");

extern "C" {
uint32_t skia_rs_text_blob_scalars_per_glyph(uint8_t positioning);
size_t skia_rs_text_blob_run_pos_count(uint32_t glyph_count, uint8_t positioning, bool* out_ok);
size_t skia_rs_text_blob_run_storage_size(
        size_t run_record_size, size_t ptr_size, uint32_t glyph_count, uint32_t text_size,
        uint8_t positioning, bool* out_ok);
void skia_rs_text_blob_tight_run_bounds(
        uint8_t positioning, const SkRect* glyph_bounds, size_t glyph_count,
        const float* scalars, const SkPoint* offset, SkRect* out);
void skia_rs_text_blob_conservative_run_bounds(
        uint8_t positioning, size_t glyph_count, const float* scalars,
        const SkRect* font_bounds, const SkPoint* offset, SkRect* out);
void skia_rs_text_blob_validate_deserial_run_sizes(
        int32_t glyph_count, uint8_t positioning, bool extended, int32_t text_size,
        size_t available_bytes, RsRunDeserialSizes* out);
bool skia_rs_text_blob_validate_clusters(const uint32_t* clusters, size_t count, uint32_t text_size);

void skia_rs_text_glyphrun_conservative_bounds(
        const SkRect* font_bounds, const SkPoint* positions, size_t pos_count,
        const SkVector* scaled_rotations, size_t sr_count, SkRect* out);
void skia_rs_text_glyphrun_tight_bounds(
        const SkRect* glyph_rects, const SkPoint* positions, size_t count,
        const SkVector* scaled_rotations, size_t sr_count, float strike_to_source_scale,
        SkRect* out);

uint32_t skia_rs_text_painter_scaler_flags(bool gamma_is_linear);
void skia_rs_text_painter_quantize_direct_mask(
        uint16_t glyph_id, const SkPoint* pos, const SkMatrix* pos_matrix_with_rounding,
        const SkIPoint* ignore_pos_field_mask, RsDirectMaskQuantized* out);
float skia_rs_text_painter_perspective_max_scale(
        const SkRect* glyph_rects, const SkPoint* positions, size_t count,
        const SkPoint* draw_origin, const SkMatrix* position_matrix, float font_size);

void skia_rs_text_intercept_setup(
        bool has_path_effect, float font_size, uint8_t paint_style, float stroke_width,
        RsInterceptSetup* out);
}

static inline void mark_safe_overflow(SkSafeMath* safe) {
    (void)safe->add(std::numeric_limits<size_t>::max(), 1);
}

}  // namespace

// ============================================================================
// SkTextBlob.cpp
// ============================================================================

enum SkTextBlob::GlyphPositioning : uint8_t {
    kDefault_Positioning    = 0,
    kHorizontal_Positioning = 1,
    kFull_Positioning       = 2,
    kRSXform_Positioning    = 3,
};

size_t SkTextBlob::RunRecord::StorageSize(uint32_t glyphCount, uint32_t textSize,
                                          SkTextBlob::GlyphPositioning positioning,
                                          SkSafeMath* safe) {
    static_assert(SkIsAlign4(sizeof(SkScalar)), "SkScalar size alignment");
    bool ok = false;
    size_t sz = skia_rs_text_blob_run_storage_size(
            sizeof(SkTextBlob::RunRecord), sizeof(void*), glyphCount, textSize,
            static_cast<uint8_t>(positioning), &ok);
    if (!ok) {
        mark_safe_overflow(safe);
    }
    return sz;
}

const SkTextBlob::RunRecord* SkTextBlob::RunRecord::First(const SkTextBlob* blob) {
    return reinterpret_cast<const RunRecord*>(SkAlignPtr((uintptr_t)(blob + 1)));
}

const SkTextBlob::RunRecord* SkTextBlob::RunRecord::Next(const RunRecord* run) {
    return SkToBool(run->fFlags & kLast_Flag) ? nullptr : NextUnchecked(run);
}

void SkTextBlob::RunRecord::validate(const uint8_t* storageTop) const {
    SkASSERT(kRunRecordMagic == fMagic);
    SkASSERT((const uint8_t*)NextUnchecked(this) <= storageTop);
    SkASSERT(glyphBuffer() + fCount <= (SkGlyphID*)posBuffer());
    SkASSERT(posBuffer() + fCount * ScalarsPerGlyph(positioning())
             <= (const SkScalar*)NextUnchecked(this));
    if (isExtended()) {
        SkASSERT(textSize() > 0);
        SkASSERT(textSizePtr() < (const uint32_t*)NextUnchecked(this));
        SkASSERT(clusterBuffer() < (const uint32_t*)NextUnchecked(this));
        SkASSERT(textBuffer() + textSize() <= (const char*)NextUnchecked(this));
    }
    static_assert(sizeof(SkTextBlob::RunRecord) == sizeof(RunRecordStorageEquivalent),
                  "runrecord_should_stay_packed");
}

const SkTextBlob::RunRecord* SkTextBlob::RunRecord::NextUnchecked(const RunRecord* run) {
    SkSafeMath safe;
    auto res = reinterpret_cast<const RunRecord*>(
            reinterpret_cast<const uint8_t*>(run)
            + StorageSize(run->glyphCount(), run->textSize(), run->positioning(), &safe));
    SkASSERT(safe);
    return res;
}

size_t SkTextBlob::RunRecord::PosCount(uint32_t glyphCount,
                                       SkTextBlob::GlyphPositioning positioning,
                                       SkSafeMath* safe) {
    bool ok = false;
    size_t res = skia_rs_text_blob_run_pos_count(
            glyphCount, static_cast<uint8_t>(positioning), &ok);
    if (!ok) {
        mark_safe_overflow(safe);
    }
    return res;
}

uint32_t* SkTextBlob::RunRecord::textSizePtr() const {
    SkASSERT(isExtended());
    SkSafeMath safe;
    auto res = (uint32_t*)(&this->posBuffer()[PosCount(fCount, positioning(), &safe)]);
    SkASSERT(safe);
    return res;
}

void SkTextBlob::RunRecord::grow(uint32_t count) {
    SkScalar* initialPosBuffer = posBuffer();
    uint32_t initialCount = fCount;
    fCount += count;

    size_t copySize = initialCount * sizeof(SkScalar) * ScalarsPerGlyph(positioning());
    SkASSERT((uint8_t*)posBuffer() + copySize <= (const uint8_t*)NextUnchecked(this));
    memmove(posBuffer(), initialPosBuffer, copySize);
}

static uint32_t next_id() {
    static std::atomic<uint32_t> nextID{1};
    uint32_t id;
    do {
        id = nextID.fetch_add(1, std::memory_order_relaxed);
    } while (id == SK_InvalidGenID);
    return id;
}

SkTextBlob::SkTextBlob(const SkRect& bounds)
    : fBounds(bounds)
    , fUniqueID(next_id())
    , fCacheID(SK_InvalidUniqueID)
    , fPurgeDelegate(nullptr) {}

SkTextBlob::~SkTextBlob() {
    if (SK_InvalidUniqueID != fCacheID.load()) {
        PurgeDelegate f = fPurgeDelegate.load();
        SkASSERT(f);
        f(fUniqueID, fCacheID);
    }

    const auto* run = RunRecord::First(this);
    do {
        const auto* nextRun = RunRecord::Next(run);
        SkDEBUGCODE(run->validate((uint8_t*)this + fStorageSize);)
        run->~RunRecord();
        run = nextRun;
    } while (run);
}

unsigned SkTextBlob::ScalarsPerGlyph(GlyphPositioning pos) {
    SkASSERT((unsigned)pos <= 3);
    return skia_rs_text_blob_scalars_per_glyph(static_cast<uint8_t>(pos));
}

void SkTextBlob::operator delete(void* p) {
    sk_free(p);
}

void* SkTextBlob::operator new(size_t) {
    SK_ABORT("All blobs are created by placement new.");
}

void* SkTextBlob::operator new(size_t, void* p) {
    return p;
}

SkTextBlobRunIterator::SkTextBlobRunIterator(const SkTextBlob* blob)
    : fCurrentRun(SkTextBlob::RunRecord::First(blob)) {
    SkDEBUGCODE(fStorageTop = (const uint8_t*)blob + blob->fStorageSize;)
}

void SkTextBlobRunIterator::next() {
    SkASSERT(!this->done());
    if (!this->done()) {
        SkDEBUGCODE(fCurrentRun->validate(fStorageTop);)
        fCurrentRun = SkTextBlob::RunRecord::Next(fCurrentRun);
    }
}

SkTextBlobRunIterator::GlyphPositioning SkTextBlobRunIterator::positioning() const {
    SkASSERT(!this->done());
    return SkTo<GlyphPositioning>(fCurrentRun->positioning());
}

unsigned SkTextBlobRunIterator::scalarsPerGlyph() const {
    return SkTextBlob::ScalarsPerGlyph(fCurrentRun->positioning());
}

bool SkTextBlobRunIterator::isLCD() const {
    return fCurrentRun->font().getEdging() == SkFont::Edging::kSubpixelAntiAlias;
}

SkTextBlobBuilder::SkTextBlobBuilder()
    : fStorageSize(0)
    , fStorageUsed(0)
    , fRunCount(0)
    , fDeferredBounds(false)
    , fLastRun(0) {
    fBounds.setEmpty();
}

SkTextBlobBuilder::~SkTextBlobBuilder() {
    if (nullptr != fStorage.get()) {
        this->make();
    }
}

SkRect SkTextBlobBuilder::TightRunBounds(const SkTextBlob::RunRecord& run) {
    const SkFont& font = run.font();
    SkRect bounds;

    if (SkTextBlob::kDefault_Positioning == run.positioning()) {
        font.measureText(run.glyphBuffer(), run.glyphCount() * sizeof(SkGlyphID),
                         SkTextEncoding::kGlyphID, &bounds);
        return bounds.makeOffset(run.offset().x(), run.offset().y());
    }

    AutoSTArray<16, SkRect> glyphBounds(run.glyphCount());
    font.getBounds({run.glyphBuffer(), run.glyphCount()}, glyphBounds, nullptr);

    const SkPoint offset = run.offset();
    skia_rs_text_blob_tight_run_bounds(
            static_cast<uint8_t>(run.positioning()), glyphBounds.data(), run.glyphCount(),
            run.posBuffer(), &offset, &bounds);
    return bounds;
}

SkRect SkTextBlobBuilder::ConservativeRunBounds(const SkTextBlob::RunRecord& run) {
    SkASSERT(run.glyphCount() > 0);
    SkASSERT(SkTextBlob::kFull_Positioning == run.positioning() ||
             SkTextBlob::kHorizontal_Positioning == run.positioning() ||
             SkTextBlob::kRSXform_Positioning == run.positioning());

    const SkRect fontBounds = SkFontPriv::GetFontBounds(run.font());
    if (fontBounds.isEmpty()) {
        return TightRunBounds(run);
    }

    const SkPoint offset = run.offset();
    SkRect bounds;
    skia_rs_text_blob_conservative_run_bounds(
            static_cast<uint8_t>(run.positioning()), run.glyphCount(),
            run.posBuffer(), &fontBounds, &offset, &bounds);
    return bounds;
}

void SkTextBlobBuilder::updateDeferredBounds() {
    SkASSERT(!fDeferredBounds || fRunCount > 0);
    if (!fDeferredBounds) {
        return;
    }

    SkASSERT(fLastRun >= SkAlignPtr(sizeof(SkTextBlob)));
    SkTextBlob::RunRecord* run = reinterpret_cast<SkTextBlob::RunRecord*>(fStorage.get() + fLastRun);

    SkRect runBounds = SkTextBlob::kDefault_Positioning == run->positioning()
                               ? TightRunBounds(*run)
                               : ConservativeRunBounds(*run);
    fBounds.join(runBounds);
    fDeferredBounds = false;
}

void SkTextBlobBuilder::reserve(size_t size) {
    SkSafeMath safe;
    if (safe.add(fStorageUsed, size) <= fStorageSize && safe) {
        return;
    }

    if (0 == fRunCount) {
        SkASSERT(nullptr == fStorage.get());
        SkASSERT(0 == fStorageSize);
        SkASSERT(0 == fStorageUsed);
        fStorageUsed = SkAlignPtr(sizeof(SkTextBlob));
    }

    fStorageSize = safe.add(fStorageUsed, size);
    fStorage.realloc(safe ? fStorageSize : std::numeric_limits<size_t>::max());
}

bool SkTextBlobBuilder::mergeRun(const SkFont& font, SkTextBlob::GlyphPositioning positioning,
                                 uint32_t count, SkPoint offset) {
    if (0 == fLastRun) {
        SkASSERT(0 == fRunCount);
        return false;
    }

    SkASSERT(fLastRun >= SkAlignPtr(sizeof(SkTextBlob)));
    SkTextBlob::RunRecord* run = reinterpret_cast<SkTextBlob::RunRecord*>(fStorage.get() + fLastRun);
    SkASSERT(run->glyphCount() > 0);

    if (run->textSize() != 0) {
        return false;
    }

    if (run->positioning() != positioning
        || run->font() != font
        || (run->glyphCount() + count < run->glyphCount())) {
        return false;
    }

    if (SkTextBlob::kFull_Positioning != positioning
        && (SkTextBlob::kHorizontal_Positioning != positioning
            || run->offset().y() != offset.y())) {
        return false;
    }

    SkSafeMath safe;
    size_t sizeDelta =
        SkTextBlob::RunRecord::StorageSize(run->glyphCount() + count, 0, positioning, &safe) -
        SkTextBlob::RunRecord::StorageSize(run->glyphCount()        , 0, positioning, &safe);
    if (!safe) {
        return false;
    }

    this->reserve(sizeDelta);

    run = reinterpret_cast<SkTextBlob::RunRecord*>(fStorage.get() + fLastRun);
    uint32_t preMergeCount = run->glyphCount();
    run->grow(count);

    fCurrentRunBuffer.glyphs = run->glyphBuffer() + preMergeCount;
    fCurrentRunBuffer.pos = run->posBuffer()
                          + preMergeCount * SkTextBlob::ScalarsPerGlyph(positioning);

    fStorageUsed += sizeDelta;
    SkASSERT(fStorageUsed <= fStorageSize);
    run->validate(fStorage.get() + fStorageUsed);
    return true;
}

void SkTextBlobBuilder::allocInternal(const SkFont& font,
                                      SkTextBlob::GlyphPositioning positioning,
                                      int count, int textSize, SkPoint offset,
                                      const SkRect* bounds) {
    if (count <= 0 || textSize < 0) {
        fCurrentRunBuffer = {nullptr, nullptr, nullptr, nullptr};
        return;
    }

    if (textSize != 0 || !this->mergeRun(font, positioning, count, offset)) {
        this->updateDeferredBounds();

        SkSafeMath safe;
        size_t runSize = SkTextBlob::RunRecord::StorageSize(count, textSize, positioning, &safe);
        if (!safe) {
            fCurrentRunBuffer = {nullptr, nullptr, nullptr, nullptr};
            return;
        }

        this->reserve(runSize);

        SkASSERT(fStorageUsed >= SkAlignPtr(sizeof(SkTextBlob)));
        SkASSERT(fStorageUsed + runSize <= fStorageSize);

        SkTextBlob::RunRecord* run = new (fStorage.get() + fStorageUsed)
            SkTextBlob::RunRecord(count, textSize, offset, font, positioning);
        fCurrentRunBuffer.glyphs = run->glyphBuffer();
        fCurrentRunBuffer.pos = run->posBuffer();
        fCurrentRunBuffer.utf8text = run->textBuffer();
        fCurrentRunBuffer.clusters = run->clusterBuffer();

        fLastRun = fStorageUsed;
        fStorageUsed += runSize;
        fRunCount++;

        SkASSERT(fStorageUsed <= fStorageSize);
        run->validate(fStorage.get() + fStorageUsed);
    }
    SkASSERT(textSize > 0 || nullptr == fCurrentRunBuffer.utf8text);
    SkASSERT(textSize > 0 || nullptr == fCurrentRunBuffer.clusters);
    if (!fDeferredBounds) {
        if (bounds) {
            fBounds.join(*bounds);
        } else {
            fDeferredBounds = true;
        }
    }
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRun(const SkFont& font, int count,
                                                                SkScalar x, SkScalar y,
                                                                const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kDefault_Positioning, count, 0, {x, y}, bounds);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRunPosH(const SkFont& font, int count,
                                                                    SkScalar y,
                                                                    const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kHorizontal_Positioning, count, 0, {0, y}, bounds);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRunPos(const SkFont& font, int count,
                                                                   const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kFull_Positioning, count, 0, {0, 0}, bounds);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer&
SkTextBlobBuilder::allocRunRSXform(const SkFont& font, int count) {
    this->allocInternal(font, SkTextBlob::kRSXform_Positioning, count, 0, {0, 0}, nullptr);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRunText(const SkFont& font, int count,
                                                                    SkScalar x, SkScalar y,
                                                                    int textByteCount,
                                                                    const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kDefault_Positioning, count, textByteCount,
                        SkPoint::Make(x, y), bounds);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRunTextPosH(const SkFont& font,
                                                                        int count,
                                                                        SkScalar y,
                                                                        int textByteCount,
                                                                        const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kHorizontal_Positioning, count, textByteCount,
                        SkPoint::Make(0, y), bounds);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRunTextPos(const SkFont& font,
                                                                       int count,
                                                                       int textByteCount,
                                                                       const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kFull_Positioning, count, textByteCount,
                        SkPoint::Make(0, 0), bounds);
    return fCurrentRunBuffer;
}

const SkTextBlobBuilder::RunBuffer& SkTextBlobBuilder::allocRunTextRSXform(const SkFont& font,
                                                                           int count,
                                                                           int textByteCount,
                                                                           const SkRect* bounds) {
    this->allocInternal(font, SkTextBlob::kRSXform_Positioning, count, textByteCount,
                        {0, 0}, bounds);
    return fCurrentRunBuffer;
}

sk_sp<SkTextBlob> SkTextBlobBuilder::make() {
    if (!fRunCount) {
        SkASSERT(!fStorage.get());
        SkASSERT(fStorageUsed == 0);
        SkASSERT(fStorageSize == 0);
        SkASSERT(fLastRun == 0);
        SkASSERT(fBounds.isEmpty());
        return nullptr;
    }

    this->updateDeferredBounds();

    auto* lastRun = reinterpret_cast<SkTextBlob::RunRecord*>(fStorage.get() + fLastRun);
    lastRun->fFlags |= SkTextBlob::RunRecord::kLast_Flag;

    SkTextBlob* blob = new (fStorage.release()) SkTextBlob(fBounds);
    SkDEBUGCODE(const_cast<SkTextBlob*>(blob)->fStorageSize = fStorageSize;)

    SkDEBUGCODE(
        SkSafeMath safe;
        size_t validateSize = SkAlignPtr(sizeof(SkTextBlob));
        for (const auto* run = SkTextBlob::RunRecord::First(blob); run;
             run = SkTextBlob::RunRecord::Next(run)) {
            validateSize += SkTextBlob::RunRecord::StorageSize(
                    run->fCount, run->textSize(), run->positioning(), &safe);
            run->validate(reinterpret_cast<const uint8_t*>(blob) + fStorageUsed);
            fRunCount--;
        }
        SkASSERT(validateSize == fStorageUsed);
        SkASSERT(fRunCount == 0);
        SkASSERT(safe);
    )

    fStorageUsed = 0;
    fStorageSize = 0;
    fRunCount = 0;
    fLastRun = 0;
    fBounds.setEmpty();

    return sk_sp<SkTextBlob>(blob);
}

void SkTextBlobPriv::Flatten(const SkTextBlob& blob, SkWriteBuffer& buffer) {
    buffer.writeRect(blob.bounds());

    SkTextBlobRunIterator it(&blob);
    while (!it.done()) {
        SkASSERT(it.glyphCount() > 0);
        buffer.write32(it.glyphCount());
        PositioningAndExtended pe;
        pe.intValue = 0;
        pe.positioning = it.positioning();
        SkASSERT((int32_t)it.positioning() == pe.intValue);

        uint32_t textSize = it.textSize();
        pe.extended = textSize > 0;
        buffer.write32(pe.intValue);
        if (pe.extended) {
            buffer.write32(textSize);
        }
        buffer.writePoint(it.offset());

        SkFontPriv::Flatten(it.font(), buffer);

        buffer.writeByteArray(it.glyphs(), it.glyphCount() * sizeof(SkGlyphID));
        buffer.writeByteArray(it.pos(),
                              it.glyphCount() * sizeof(SkScalar) *
                              SkTextBlob::ScalarsPerGlyph(
                                  SkTo<SkTextBlob::GlyphPositioning>(it.positioning())));
        if (pe.extended) {
            buffer.writeByteArray(it.clusters(), sizeof(uint32_t) * it.glyphCount());
            buffer.writeByteArray(it.text(), it.textSize());
        }

        it.next();
    }

    buffer.write32(0);
}

sk_sp<SkTextBlob> SkTextBlobPriv::MakeFromBuffer(SkReadBuffer& reader) {
    SkRect bounds;
    reader.readRect(&bounds);

    SkTextBlobBuilder blobBuilder;
    for (;;) {
        int glyphCount = reader.read32();
        if (glyphCount == 0) {
            break;
        }

        PositioningAndExtended pe;
        pe.intValue = reader.read32();
        const auto pos = SkTo<SkTextBlob::GlyphPositioning>(pe.positioning);
        if (glyphCount <= 0 || pos > SkTextBlob::kRSXform_Positioning) {
            return nullptr;
        }
        int textSize = pe.extended ? reader.read32() : 0;
        if (textSize < 0) {
            return nullptr;
        }

        SkPoint offset;
        reader.readPoint(&offset);
        SkFont font;
        SkFontPriv::Unflatten(&font, reader);

        RsRunDeserialSizes sz;
        skia_rs_text_blob_validate_deserial_run_sizes(
                glyphCount, pe.positioning, pe.extended != 0, textSize, reader.available(), &sz);

        if (!reader.isValid() || !sz.valid) {
            return nullptr;
        }

        const SkTextBlobBuilder::RunBuffer* buf = nullptr;
        switch (pos) {
            case SkTextBlob::kDefault_Positioning:
                buf = &blobBuilder.allocRunText(font, glyphCount, offset.x(), offset.y(),
                                                textSize, &bounds);
                break;
            case SkTextBlob::kHorizontal_Positioning:
                buf = &blobBuilder.allocRunTextPosH(font, glyphCount, offset.y(),
                                                    textSize, &bounds);
                break;
            case SkTextBlob::kFull_Positioning:
                buf = &blobBuilder.allocRunTextPos(font, glyphCount, textSize, &bounds);
                break;
            case SkTextBlob::kRSXform_Positioning:
                buf = &blobBuilder.allocRunTextRSXform(font, glyphCount, textSize, &bounds);
                break;
        }

        if (!buf->glyphs ||
            !buf->pos ||
            (pe.extended && (!buf->clusters || !buf->utf8text))) {
            return nullptr;
        }

        if (!reader.readByteArray(buf->glyphs, sz.glyph_size) ||
            !reader.readByteArray(buf->pos, sz.pos_size)) {
            return nullptr;
        }

        if (pe.extended) {
            if (!reader.readByteArray(buf->clusters, sz.cluster_size) ||
                !reader.readByteArray(buf->utf8text, textSize)) {
                return nullptr;
            }
            if (!skia_rs_text_blob_validate_clusters(
                        buf->clusters, static_cast<size_t>(glyphCount),
                        static_cast<uint32_t>(textSize))) {
                return nullptr;
            }
        }
    }

    return blobBuilder.make();
}

sk_sp<SkTextBlob> SkTextBlob::MakeFromText(const void* text, size_t byteLength, const SkFont& font,
                                           SkTextEncoding encoding) {
    const size_t count = font.countText(text, byteLength, encoding);
    if (count == 0) {
        return nullptr;
    }
    SkTextBlobBuilder builder;
    auto buffer = builder.allocRunPos(font, count);
    font.textToGlyphs(text, byteLength, encoding, {buffer.glyphs, count});
    font.getPos({buffer.glyphs, count}, {buffer.points(), count}, {0, 0});
    return builder.make();
}

sk_sp<SkTextBlob> SkTextBlob::MakeFromPosText(const void* text, size_t byteLength,
                                              SkSpan<const SkPoint> pos, const SkFont& font,
                                              SkTextEncoding encoding) {
    const size_t count = font.countText(text, byteLength, encoding);
    if (count == 0 || pos.size() < count) {
        return nullptr;
    }
    SkTextBlobBuilder builder;
    auto buffer = builder.allocRunPos(font, count);
    font.textToGlyphs(text, byteLength, encoding, {buffer.glyphs, count});
    memcpy(buffer.points(), pos.data(), count * sizeof(SkPoint));
    return builder.make();
}

sk_sp<SkTextBlob> SkTextBlob::MakeFromPosTextH(const void* text, size_t byteLength,
                                               SkSpan<const SkScalar> xpos, SkScalar constY,
                                               const SkFont& font, SkTextEncoding encoding) {
    const size_t count = font.countText(text, byteLength, encoding);
    if (count == 0 || xpos.size() < count) {
        return nullptr;
    }
    SkTextBlobBuilder builder;
    auto buffer = builder.allocRunPosH(font, count, constY);
    font.textToGlyphs(text, byteLength, encoding, {buffer.glyphs, count});
    memcpy(buffer.pos, xpos.data(), count * sizeof(SkScalar));
    return builder.make();
}

sk_sp<SkTextBlob> SkTextBlob::MakeFromRSXform(const void* text, size_t byteLength,
                                              SkSpan<const SkRSXform> xform, const SkFont& font,
                                              SkTextEncoding encoding) {
    const size_t count = font.countText(text, byteLength, encoding);
    if (count == 0 || xform.size() < count) {
        return nullptr;
    }
    SkTextBlobBuilder builder;
    auto buffer = builder.allocRunRSXform(font, count);
    font.textToGlyphs(text, byteLength, encoding, {buffer.glyphs, count});
    memcpy(buffer.xforms(), xform.data(), count * sizeof(SkRSXform));
    return builder.make();
}

sk_sp<SkData> SkTextBlob::serialize(const SkSerialProcs& procs) const {
    SkBinaryWriteBuffer buffer(procs);
    SkTextBlobPriv::Flatten(*this, buffer);

    size_t total = buffer.bytesWritten();
    sk_sp<SkData> data = SkData::MakeUninitialized(total);
    buffer.writeToMemory(data->writable_data());
    return data;
}

sk_sp<SkTextBlob> SkTextBlob::Deserialize(const void* data, size_t length,
                                          const SkDeserialProcs& procs) {
    SkReadBuffer buffer(data, length);
    buffer.setDeserialProcs(procs);
    return SkTextBlobPriv::MakeFromBuffer(buffer);
}

size_t SkTextBlob::serialize(const SkSerialProcs& procs, void* memory, size_t memory_size) const {
    SkBinaryWriteBuffer buffer(memory, memory_size, procs);
    SkTextBlobPriv::Flatten(*this, buffer);
    return buffer.usingInitialStorage() ? buffer.bytesWritten() : 0u;
}

namespace {
int get_glyph_run_intercepts(const sktext::GlyphRun& glyphRun,
                             const SkPaint& paint,
                             SkSpan<const SkScalar, 2> bounds,
                             SkScalar intervals[],
                             int* intervalCount) {
    SkPaint interceptPaint{paint};
    SkFont interceptFont{glyphRun.font()};
    interceptPaint.setMaskFilter(nullptr);

    RsInterceptSetup setup;
    skia_rs_text_intercept_setup(
            interceptPaint.getPathEffect() != nullptr, interceptFont.getSize(),
            static_cast<uint8_t>(interceptPaint.getStyle()), interceptPaint.getStrokeWidth(),
            &setup);

    if (interceptPaint.getPathEffect() == nullptr) {
        interceptFont.setHinting(SkFontHinting::kNone);
        interceptFont.setSubpixel(true);
        interceptFont.setSize(SkIntToScalar(SkFontPriv::kCanonicalTextSizeForPaths));
        if (interceptPaint.getStrokeWidth() > 0 &&
            interceptPaint.getStyle() != SkPaint::kFill_Style) {
            interceptPaint.setStrokeWidth(setup.stroke_width);
        }
    }

    interceptPaint.setStyle(SkPaint::kFill_Style);
    interceptPaint.setPathEffect(nullptr);

    SkStrikeSpec strikeSpec = SkStrikeSpec::MakeWithNoDevice(interceptFont, &interceptPaint);
    SkBulkGlyphMetricsAndPaths metricsAndPaths{strikeSpec};

    const SkPoint* posCursor = glyphRun.positions().data();
    for (const SkGlyph* glyph : metricsAndPaths.glyphs(glyphRun.glyphsIDs())) {
        SkPoint pos = *posCursor++;
        if (glyph->path() != nullptr) {
            SkScalar scaledBounds[2] = {
                (bounds[0] - pos.y()) / setup.scale,
                (bounds[1] - pos.y()) / setup.scale
            };
            metricsAndPaths.findIntercepts(
                    scaledBounds, setup.scale, pos.x(), glyph, intervals, intervalCount);
        }
    }
    return *intervalCount;
}
}  // namespace

int SkTextBlob::getIntercepts(SkSpan<const SkScalar, 2> bounds,
                              SkScalar intervals[],
                              const SkPaint* paint) const {
    std::optional<SkPaint> defaultPaint;
    if (paint == nullptr) {
        defaultPaint.emplace();
        paint = &defaultPaint.value();
    }

    sktext::GlyphRunBuilder builder;
    auto glyphRunList = builder.blobToGlyphRunList(*this, {0, 0});

    int intervalCount = 0;
    for (const sktext::GlyphRun& glyphRun : glyphRunList) {
        if (glyphRun.scaledRotations().empty()) {
            intervalCount = get_glyph_run_intercepts(
                    glyphRun, *paint, bounds, intervals, &intervalCount);
        }
    }
    return intervalCount;
}

int SkTextBlob::getIntercepts(const SkScalar bounds[2],
                              SkScalar intervals[],
                              const SkPaint* paint) const {
    return this->getIntercepts(
            SK_UNSAFE_TODO(SkSpan<const SkScalar, 2>(bounds, 2)), intervals, paint);
}

std::vector<SkScalar> SkFont::getIntercepts(SkSpan<const SkGlyphID> glyphs,
                                            SkSpan<const SkPoint> positions,
                                            SkScalar top, SkScalar bottom,
                                            const SkPaint* paintPtr) const {
    const auto count = std::min(glyphs.size(), positions.size());
    if (count == 0) {
        return std::vector<SkScalar>();
    }

    const SkPaint paint(paintPtr ? *paintPtr : SkPaint());
    const SkScalar bounds[] = {top, bottom};
    const sktext::GlyphRun run(*this, positions, glyphs, {}, {}, {});

    std::vector<SkScalar> result;
    result.resize(count * 2);
    int intervalCount = 0;
    intervalCount = get_glyph_run_intercepts(run, paint, bounds, result.data(), &intervalCount);
    result.resize(intervalCount);
    return result;
}

SkTextBlob::Iter::Iter(const SkTextBlob& blob) {
    fRunRecord = RunRecord::First(&blob);
}

bool SkTextBlob::Iter::next(Run* rec) {
    if (fRunRecord) {
        if (rec) {
            rec->fTypeface = fRunRecord->font().getTypeface();
            rec->fGlyphCount = fRunRecord->glyphCount();
            rec->fGlyphIndices = fRunRecord->glyphBuffer();
#ifdef SK_UNTIL_CRBUG_1187654_IS_FIXED
            rec->fClusterIndex_forTest = fRunRecord->clusterBuffer();
            rec->fUtf8Size_forTest = fRunRecord->textSize();
            rec->fUtf8_forTest = fRunRecord->textBuffer();
#endif
        }
        if (fRunRecord->isLastRun()) {
            fRunRecord = nullptr;
        } else {
            fRunRecord = RunRecord::Next(fRunRecord);
        }
        return true;
    }
    return false;
}

bool SkTextBlob::Iter::experimentalNext(ExperimentalRun* rec) {
    if (fRunRecord) {
        if (rec) {
            rec->font = fRunRecord->font();
            rec->count = fRunRecord->glyphCount();
            rec->glyphs = fRunRecord->glyphBuffer();
            rec->positions = fRunRecord->pointBuffer();
        }
        if (fRunRecord->isLastRun()) {
            fRunRecord = nullptr;
        } else {
            fRunRecord = RunRecord::Next(fRunRecord);
        }
        return true;
    }
    return false;
}

// ============================================================================
// src/text/GlyphRun.cpp
// ============================================================================

namespace sktext {

GlyphRun::GlyphRun(const SkFont& font,
                   SkSpan<const SkPoint> positions,
                   SkSpan<const SkGlyphID> glyphIDs,
                   SkSpan<const char> text,
                   SkSpan<const uint32_t> clusters,
                   SkSpan<const SkVector> scaledRotations)
        : fSource{SkMakeZip(glyphIDs, positions)}
        , fText{text}
        , fClusters{clusters}
        , fScaledRotations{scaledRotations}
        , fFont{font} {}

GlyphRun::GlyphRun(const GlyphRun& that, const SkFont& font)
        : fSource{that.fSource}
        , fText{that.fText}
        , fClusters{that.fClusters}
        , fFont{font} {}

GlyphRunList::GlyphRunList(const SkTextBlob* blob,
                           SkRect bounds,
                           SkPoint origin,
                           SkSpan<const GlyphRun> glyphRunList,
                           GlyphRunBuilder* builder)
        : fGlyphRuns{glyphRunList}
        , fOriginalTextBlob{blob}
        , fSourceBounds{bounds}
        , fOrigin{origin}
        , fBuilder{builder} {}

GlyphRunList::GlyphRunList(const GlyphRun& glyphRun,
                           const SkRect& bounds,
                           SkPoint origin,
                           GlyphRunBuilder* builder)
        : fGlyphRuns{SkSpan<const GlyphRun>{&glyphRun, 1}}
        , fOriginalTextBlob{nullptr}
        , fSourceBounds{bounds}
        , fOrigin{origin}
        , fBuilder{builder} {}

uint64_t GlyphRunList::uniqueID() const {
    return fOriginalTextBlob != nullptr ? fOriginalTextBlob->uniqueID() : SK_InvalidUniqueID;
}

bool GlyphRunList::anyRunsLCD() const {
    for (const auto& r : fGlyphRuns) {
        if (r.font().getEdging() == SkFont::Edging::kSubpixelAntiAlias) {
            return true;
        }
    }
    return false;
}

void GlyphRunList::temporaryShuntBlobNotifyAddedToCache(uint32_t cacheID,
                                                        SkTextBlob::PurgeDelegate pd) const {
    SkASSERT(fOriginalTextBlob != nullptr);
    SkASSERT(pd != nullptr);
    fOriginalTextBlob->notifyAddedToCache(cacheID, pd);
}

sk_sp<SkTextBlob> GlyphRunList::makeBlob() const {
    SkTextBlobBuilder builder;
    for (auto& run : *this) {
        SkTextBlobBuilder::RunBuffer buffer;
        if (run.scaledRotations().empty()) {
            if (run.text().empty()) {
                buffer = builder.allocRunPos(run.font(), run.runSize(), nullptr);
            } else {
                buffer = builder.allocRunTextPos(run.font(), run.runSize(), run.text().size(), nullptr);
                auto text = run.text();
                memcpy(buffer.utf8text, text.data(), text.size_bytes());
                auto clusters = run.clusters();
                memcpy(buffer.clusters, clusters.data(), clusters.size_bytes());
            }
            auto positions = run.positions();
            memcpy(buffer.points(), positions.data(), positions.size_bytes());
        } else {
            buffer = builder.allocRunRSXform(run.font(), run.runSize());
            for (auto [xform, pos, sr] : SkMakeZip(buffer.xforms(),
                                                   run.positions(),
                                                   run.scaledRotations())) {
                xform = SkRSXform::Make(sr.x(), sr.y(), pos.x(), pos.y());
            }
        }
        auto glyphIDs = run.glyphsIDs();
        memcpy(buffer.glyphs, glyphIDs.data(), glyphIDs.size_bytes());
    }
    return builder.make();
}

static SkRect glyphrun_source_bounds(
        const SkFont& font,
        const SkPaint& paint,
        SkZip<const SkGlyphID, const SkPoint> source,
        SkSpan<const SkVector> scaledRotations) {
    SkASSERT(!source.empty());
    const SkRect fontBounds = SkFontPriv::GetFontBounds(font);
    SkSpan<const SkGlyphID> glyphIDs = source.get<0>();
    SkSpan<const SkPoint> positions = source.get<1>();

    SkRect bounds;
    if (fontBounds.isEmpty()) {
        auto [strikeSpec, strikeToSourceScale] = SkStrikeSpec::MakeCanonicalized(font, &paint);
        SkBulkGlyphMetrics metrics{strikeSpec};
        SkSpan<const SkGlyph*> glyphs = metrics.glyphs(glyphIDs);

        AutoSTArray<64, SkRect> glyphRects(glyphs.size());
        for (size_t i = 0; i < glyphs.size(); ++i) {
            glyphRects[i] = glyphs[i]->rect();
        }
        skia_rs_text_glyphrun_tight_bounds(
                glyphRects.data(), positions.data(), glyphs.size(),
                scaledRotations.data(), scaledRotations.size(), strikeToSourceScale, &bounds);
        return bounds;
    }

    skia_rs_text_glyphrun_conservative_bounds(
            &fontBounds, positions.data(), positions.size(),
            scaledRotations.data(), scaledRotations.size(), &bounds);
    return bounds;
}

GlyphRunList GlyphRunBuilder::makeGlyphRunList(
        const GlyphRun& run, const SkPaint& paint, SkPoint origin) {
    const SkRect bounds =
            glyphrun_source_bounds(run.font(), paint, run.source(), run.scaledRotations());
    return GlyphRunList{run, bounds, origin, this};
}

static SkSpan<const SkPoint> draw_text_positions(
        const SkFont& font, SkSpan<const SkGlyphID> glyphIDs, SkPoint origin, SkPoint* buffer) {
    SkStrikeSpec strikeSpec = SkStrikeSpec::MakeWithNoDevice(font);
    SkBulkGlyphMetrics storage{strikeSpec};
    auto glyphs = storage.glyphs(glyphIDs);

    SkPoint* positionCursor = buffer;
    SkPoint endOfLastGlyph = origin;
    for (auto glyph : glyphs) {
        *positionCursor++ = endOfLastGlyph;
        endOfLastGlyph += glyph->advanceVector();
    }
    return SkSpan(buffer, glyphIDs.size());
}

const GlyphRunList& GlyphRunBuilder::textToGlyphRunList(
        const SkFont& font, const SkPaint& paint,
        const void* bytes, size_t byteLength, SkPoint origin,
        SkTextEncoding encoding) {
    auto glyphIDs = textToGlyphIDs(font, bytes, byteLength, encoding);
    SkRect bounds = SkRect::MakeEmpty();
    this->prepareBuffers(glyphIDs.size(), 0);
    if (!glyphIDs.empty()) {
        SkSpan<const SkPoint> positions = draw_text_positions(font, glyphIDs, {0, 0}, fPositions);
        this->makeGlyphRun(font, glyphIDs, positions, SkSpan<const char>{},
                           SkSpan<const uint32_t>{}, SkSpan<const SkVector>{});
        auto run = fGlyphRunListStorage.front();
        bounds = glyphrun_source_bounds(run.font(), paint, run.source(), run.scaledRotations());
    }
    return this->setGlyphRunList(nullptr, bounds, origin);
}

const GlyphRunList& GlyphRunBuilder::blobToGlyphRunList(const SkTextBlob& blob, SkPoint origin) {
    this->initialize(blob);

    SkPoint* positionCursor = fPositions;
    SkVector* scaledRotationsCursor = fScaledRotations;
    for (SkTextBlobRunIterator it(&blob); !it.done(); it.next()) {
        size_t runSize = it.glyphCount();
        if (runSize == 0 || !SkFontPriv::IsFinite(it.font())) {
            continue;
        }

        const SkFont& font = it.font();
        auto glyphIDs = SkSpan<const SkGlyphID>{it.glyphs(), runSize};

        SkSpan<const SkPoint> positions;
        SkSpan<const SkVector> scaledRotations;
        switch (it.positioning()) {
            case SkTextBlobRunIterator::kDefault_Positioning: {
                positions = draw_text_positions(font, glyphIDs, it.offset(), positionCursor);
                positionCursor += positions.size();
                break;
            }
            case SkTextBlobRunIterator::kHorizontal_Positioning: {
                positions = SkSpan(positionCursor, runSize);
                for (auto x : SkSpan<const SkScalar>{it.pos(), glyphIDs.size()}) {
                    *positionCursor++ = SkPoint::Make(x, it.offset().y());
                }
                break;
            }
            case SkTextBlobRunIterator::kFull_Positioning: {
                positions = SkSpan(it.points(), runSize);
                break;
            }
            case SkTextBlobRunIterator::kRSXform_Positioning: {
                positions = SkSpan(positionCursor, runSize);
                scaledRotations = SkSpan(scaledRotationsCursor, runSize);
                for (const SkRSXform& xform : SkSpan(it.xforms(), runSize)) {
                    *positionCursor++ = {xform.fTx, xform.fTy};
                    *scaledRotationsCursor++ = {xform.fSCos, xform.fSSin};
                }
                break;
            }
        }

        const uint32_t* clusters = it.clusters();
        this->makeGlyphRun(font, glyphIDs, positions,
                           SkSpan<const char>(it.text(), it.textSize()),
                           SkSpan<const uint32_t>(clusters, clusters ? runSize : 0),
                           scaledRotations);
    }

    return this->setGlyphRunList(&blob, blob.bounds(), origin);
}

std::tuple<SkSpan<const SkPoint>, SkSpan<const SkVector>>
GlyphRunBuilder::convertRSXForm(SkSpan<const SkRSXform> xforms) {
    const int count = SkCount(xforms);
    this->prepareBuffers(count, count);
    auto positions = SkSpan(fPositions.get(), count);
    auto scaledRotations = SkSpan(fScaledRotations.get(), count);
    for (auto [pos, sr, xform] : SkMakeZip(positions, scaledRotations, xforms)) {
        auto [scos, ssin, tx, ty] = xform;
        pos = {tx, ty};
        sr = {scos, ssin};
    }
    return {positions, scaledRotations};
}

void GlyphRunBuilder::initialize(const SkTextBlob& blob) {
    int positionCount = 0;
    int rsxFormCount = 0;
    for (SkTextBlobRunIterator it(&blob); !it.done(); it.next()) {
        if (it.positioning() != SkTextBlobRunIterator::kFull_Positioning) {
            positionCount += it.glyphCount();
        }
        if (it.positioning() == SkTextBlobRunIterator::kRSXform_Positioning) {
            rsxFormCount += it.glyphCount();
        }
    }
    prepareBuffers(positionCount, rsxFormCount);
}

void GlyphRunBuilder::prepareBuffers(int positionCount, int RSXFormCount) {
    if (positionCount > fMaxTotalRunSize) {
        fMaxTotalRunSize = positionCount;
        fPositions.reset(fMaxTotalRunSize);
    }
    if (RSXFormCount > fMaxScaledRotations) {
        fMaxScaledRotations = RSXFormCount;
        fScaledRotations.reset(RSXFormCount);
    }
    fGlyphRunListStorage.clear();
}

SkSpan<const SkGlyphID> GlyphRunBuilder::textToGlyphIDs(
        const SkFont& font, const void* bytes, size_t byteLength, SkTextEncoding encoding) {
    if (encoding != SkTextEncoding::kGlyphID) {
        int count = font.countText(bytes, byteLength, encoding);
        if (count > 0) {
            fScratchGlyphIDs.resize(count);
            font.textToGlyphs(bytes, byteLength, encoding, fScratchGlyphIDs);
            return SkSpan(fScratchGlyphIDs);
        } else {
            return SkSpan<const SkGlyphID>();
        }
    } else {
        return SkSpan<const SkGlyphID>((const SkGlyphID*)bytes, byteLength / 2);
    }
}

void GlyphRunBuilder::makeGlyphRun(
        const SkFont& font,
        SkSpan<const SkGlyphID> glyphIDs,
        SkSpan<const SkPoint> positions,
        SkSpan<const char> text,
        SkSpan<const uint32_t> clusters,
        SkSpan<const SkVector> scaledRotations) {
    if (!glyphIDs.empty()) {
        fGlyphRunListStorage.emplace_back(
                font, positions, glyphIDs, text, clusters, scaledRotations);
    }
}

const GlyphRunList& GlyphRunBuilder::setGlyphRunList(
        const SkTextBlob* blob, const SkRect& bounds, SkPoint origin) {
    fGlyphRunList.emplace(blob, bounds, origin, SkSpan(fGlyphRunListStorage), this);
    return fGlyphRunList.value();
}

}  // namespace sktext

// ============================================================================
// SkGlyphRunPainter.cpp
// ============================================================================

namespace {

std::tuple<SkZip<const SkGlyph*, SkPoint>, SkZip<SkGlyphID, SkPoint>>
prepare_for_path_drawing(SkStrike* strike,
                         SkZip<const SkGlyphID, const SkPoint> source,
                         SkZip<const SkGlyph*, SkPoint> acceptedBuffer,
                         SkZip<SkGlyphID, SkPoint> rejectedBuffer) {
    int acceptedSize = 0;
    int rejectedSize = 0;
    strike->lock();
    for (auto [glyphID, pos] : source) {
        if (!SkIsFinite(pos.x(), pos.y())) {
            continue;
        }
        const SkPackedGlyphID packedID{glyphID};
        switch (SkGlyphDigest digest = strike->digestFor(kPath, packedID);
                digest.actionFor(kPath)) {
            case GlyphAction::kAccept:
                acceptedBuffer[acceptedSize++] = std::make_tuple(strike->glyph(digest), pos);
                break;
            case GlyphAction::kReject:
                rejectedBuffer[rejectedSize++] = std::make_tuple(glyphID, pos);
                break;
            default:
                break;
        }
    }
    strike->unlock();
    return {acceptedBuffer.first(acceptedSize), rejectedBuffer.first(rejectedSize)};
}

std::tuple<SkZip<const SkGlyph*, SkPoint>, SkZip<SkGlyphID, SkPoint>>
prepare_for_drawable_drawing(SkStrike* strike,
                             SkZip<const SkGlyphID, const SkPoint> source,
                             SkZip<const SkGlyph*, SkPoint> acceptedBuffer,
                             SkZip<SkGlyphID, SkPoint> rejectedBuffer) {
    int acceptedSize = 0;
    int rejectedSize = 0;
    strike->lock();
    for (auto [glyphID, pos] : source) {
        if (!SkIsFinite(pos.x(), pos.y())) {
            continue;
        }
        const SkPackedGlyphID packedID{glyphID};
        switch (SkGlyphDigest digest = strike->digestFor(kDrawable, packedID);
                digest.actionFor(kDrawable)) {
            case GlyphAction::kAccept:
                acceptedBuffer[acceptedSize++] = std::make_tuple(strike->glyph(digest), pos);
                break;
            case GlyphAction::kReject:
                rejectedBuffer[rejectedSize++] = std::make_tuple(glyphID, pos);
                break;
            default:
                break;
        }
    }
    strike->unlock();
    return {acceptedBuffer.first(acceptedSize), rejectedBuffer.first(rejectedSize)};
}

std::tuple<SkZip<const SkGlyph*, SkPoint>, SkZip<SkGlyphID, SkPoint>>
prepare_for_direct_mask_drawing(SkStrike* strike,
                                const SkMatrix& creationMatrix,
                                SkZip<const SkGlyphID, const SkPoint> source,
                                SkZip<const SkGlyph*, SkPoint> acceptedBuffer,
                                SkZip<SkGlyphID, SkPoint> rejectedBuffer) {
    const SkIPoint mask = strike->roundingSpec().ignorePositionFieldMask;
    const SkPoint halfSampleFreq = strike->roundingSpec().halfAxisSampleFreq;

    SkMatrix positionMatrixWithRounding = creationMatrix;
    positionMatrixWithRounding.postTranslate(halfSampleFreq.x(), halfSampleFreq.y());
    (void)positionMatrixWithRounding.getType();

    int acceptedSize = 0;
    int rejectedSize = 0;
    strike->lock();
    for (auto [glyphID, pos] : source) {
        RsDirectMaskQuantized q;
        skia_rs_text_painter_quantize_direct_mask(
                glyphID, &pos, &positionMatrixWithRounding, &mask, &q);
        if (!q.finite) {
            continue;
        }

        const SkPackedGlyphID packedGlyphID{q.packed_glyph_id};
        switch (SkGlyphDigest digest = strike->digestFor(kDirectMaskCPU, packedGlyphID);
                digest.actionFor(kDirectMaskCPU)) {
            case GlyphAction::kAccept:
                acceptedBuffer[acceptedSize++] = std::make_tuple(strike->glyph(digest), q.rounded_pos);
                break;
            case GlyphAction::kReject:
                rejectedBuffer[rejectedSize++] = std::make_tuple(glyphID, pos);
                break;
            default:
                break;
        }
    }
    strike->unlock();
    return {acceptedBuffer.first(acceptedSize), rejectedBuffer.first(rejectedSize)};
}

std::tuple<SkZip<const SkGlyph*, SkPoint>, SkZip<SkGlyphID, SkPoint>>
prepare_for_direct_bitmap_drawing(SkStrike* strike,
                                  const SkMatrix& creationMatrix,
                                  SkZip<const SkGlyphID, const SkPoint> source,
                                  SkZip<const SkGlyph*, SkPoint> acceptedBuffer,
                                  SkZip<SkGlyphID, SkPoint> rejectedBuffer) {
    const SkIPoint mask = strike->roundingSpec().ignorePositionFieldMask;
    const SkPoint halfSampleFreq = strike->roundingSpec().halfAxisSampleFreq;

    SkMatrix positionMatrixWithRounding = creationMatrix;
    positionMatrixWithRounding.postTranslate(halfSampleFreq.x(), halfSampleFreq.y());
    (void)positionMatrixWithRounding.getType();

    int acceptedSize = 0;
    int rejectedSize = 0;
    strike->lock();
    for (auto [glyphID, pos] : source) {
        RsDirectMaskQuantized q;
        skia_rs_text_painter_quantize_direct_mask(
                glyphID, &pos, &positionMatrixWithRounding, &mask, &q);
        if (!q.finite) {
            continue;
        }

        const SkPackedGlyphID packedGlyphID{q.packed_glyph_id};
        switch (SkGlyphDigest digest = strike->digestFor(kDirectMaskCPU, packedGlyphID);
                digest.actionFor(kDirectMaskCPU)) {
            case GlyphAction::kAccept:
                acceptedBuffer[acceptedSize++] = std::make_tuple(strike->glyph(digest), pos);
                break;
            case GlyphAction::kReject:
                rejectedBuffer[rejectedSize++] = std::make_tuple(glyphID, pos);
                break;
            default:
                break;
        }
    }
    strike->unlock();
    return {acceptedBuffer.first(acceptedSize), rejectedBuffer.first(rejectedSize)};
}

}  // namespace

namespace skcpu {

GlyphRunListPainter::GlyphRunListPainter(const SkSurfaceProps& props,
                                         SkColorType colorType,
                                         SkColorSpace* cs)
        : fDeviceProps{props}
        , fBitmapFallbackProps{props.cloneWithPixelGeometry(kUnknown_SkPixelGeometry)}
        , fColorType{colorType}
        , fScalerContextFlags{static_cast<SkScalerContextFlags>(
                  skia_rs_text_painter_scaler_flags(cs && cs->gammaIsLinear()))} {}

void GlyphRunListPainter::drawForBitmapDevice(SkCanvas* canvas,
                                              const BitmapDevicePainter* bitmapDevice,
                                              const sktext::GlyphRunList& glyphRunList,
                                              const SkPaint& paint,
                                              const SkMatrix& drawMatrix) {
    STArray<64, const SkGlyph*> acceptedPackedGlyphIDs;
    STArray<64, SkPoint> acceptedPositions;
    STArray<64, SkGlyphID> rejectedGlyphIDs;
    STArray<64, SkPoint> rejectedPositions;
    const int maxGlyphRunSize = glyphRunList.maxGlyphRunSize();
    acceptedPackedGlyphIDs.resize(maxGlyphRunSize);
    acceptedPositions.resize(maxGlyphRunSize);
    const auto acceptedBuffer = SkMakeZip(acceptedPackedGlyphIDs, acceptedPositions);
    rejectedGlyphIDs.resize(maxGlyphRunSize);
    rejectedPositions.resize(maxGlyphRunSize);
    const auto rejectedBuffer = SkMakeZip(rejectedGlyphIDs, rejectedPositions);

    auto& props = (kN32_SkColorType == fColorType && paint.isSrcOver())
                          ? fDeviceProps
                          : fBitmapFallbackProps;

    SkPoint drawOrigin = glyphRunList.origin();
    SkMatrix positionMatrix{drawMatrix};
    positionMatrix.preTranslate(drawOrigin.x(), drawOrigin.y());
    for (auto& glyphRun : glyphRunList) {
        const SkFont& runFont = glyphRun.font();
        SkZip<const SkGlyphID, const SkPoint> source = glyphRun.source();

        if (SkStrikeSpec::ShouldDrawAsPath(paint, runFont, positionMatrix)) {
            auto [strikeSpec, strikeToSourceScale] =
                    SkStrikeSpec::MakePath(runFont, paint, props, fScalerContextFlags);
            auto strike = strikeSpec.findOrCreateStrike();

            {
                auto [accepted, rejected] = prepare_for_path_drawing(
                        strike.get(), source, acceptedBuffer, rejectedBuffer);
                source = rejected;

                SkPaint pathPaint = paint;
                pathPaint.setAntiAlias(runFont.hasSomeAntiAliasing());

                const bool stroking = pathPaint.getStyle() != SkPaint::kFill_Style;
                const bool hairline = pathPaint.getStrokeWidth() == 0;
                const bool needsExactCTM = pathPaint.getShader()     ||
                                           pathPaint.getPathEffect() ||
                                           pathPaint.getMaskFilter() ||
                                           (stroking && !hairline);

                if (!needsExactCTM) {
                    for (auto [glyph, pos] : accepted) {
                        const SkPath* path = glyph->path();
                        SkMatrix m;
                        SkPoint translate = drawOrigin + pos;
                        m.setScaleTranslate(strikeToSourceScale, strikeToSourceScale,
                                            translate.x(), translate.y());
                        SkAutoCanvasRestore acr(canvas, true);
                        canvas->concat(m);
                        canvas->drawPath(*path, pathPaint);
                    }
                } else {
                    for (auto [glyph, pos] : accepted) {
                        const SkPath* path = glyph->path();
                        SkMatrix m;
                        SkPoint translate = drawOrigin + pos;
                        m.setScaleTranslate(strikeToSourceScale, strikeToSourceScale,
                                            translate.x(), translate.y());
                        SkPathBuilder builder;
                        builder.addPath(*path, m);
                        builder.setIsVolatile(true);
                        canvas->drawPath(builder.detach(), pathPaint);
                    }
                }
            }

            if (!source.empty()) {
                auto [accepted, rejected] = prepare_for_drawable_drawing(
                        strike.get(), source, acceptedBuffer, rejectedBuffer);
                source = rejected;

                for (auto [glyph, pos] : accepted) {
                    SkDrawable* drawable = glyph->drawable();
                    SkMatrix m;
                    SkPoint translate = drawOrigin + pos;
                    m.setScaleTranslate(strikeToSourceScale, strikeToSourceScale,
                                        translate.x(), translate.y());
                    SkAutoCanvasRestore acr(canvas, false);
                    SkRect drawableBounds = drawable->getBounds();
                    m.mapRect(&drawableBounds);
                    canvas->saveLayer(&drawableBounds, &paint);
                    drawable->draw(canvas, &m);
                }
            }
        }
        if (!source.empty() && !positionMatrix.hasPerspective()) {
            SkStrikeSpec strikeSpec = SkStrikeSpec::MakeMask(
                    runFont, paint, props, fScalerContextFlags, positionMatrix);
            auto strike = strikeSpec.findOrCreateStrike();
            auto [accepted, rejected] = prepare_for_direct_mask_drawing(
                    strike.get(), positionMatrix, source, acceptedBuffer, rejectedBuffer);
            source = rejected;
            bitmapDevice->paintMasks(accepted, paint);
        }
        if (!source.empty()) {
            SkStrikeSpec scaleStrikeSpec = SkStrikeSpec::MakeMask(
                    runFont, paint, props, fScalerContextFlags, SkMatrix::I());
            SkBulkGlyphMetrics metrics{scaleStrikeSpec};

            auto glyphIDs = source.get<0>();
            auto positions = source.get<1>();
            SkSpan<const SkGlyph*> glyphs = metrics.glyphs(glyphIDs);

            AutoSTArray<64, SkRect> glyphRects(glyphs.size());
            for (size_t i = 0; i < glyphs.size(); ++i) {
                glyphRects[i] = glyphs[i]->isEmpty() ? SkRect::MakeEmpty() : glyphs[i]->rect();
            }
            (void)positionMatrix.getType();
            SkScalar maxScale = skia_rs_text_painter_perspective_max_scale(
                    glyphRects.data(), positions.data(), glyphs.size(),
                    &drawOrigin, &positionMatrix, runFont.getSize());

            if (maxScale <= 0) {
                continue;
            }

            SkMatrix cacheScale = SkMatrix::Scale(maxScale, maxScale);
            SkStrikeSpec strikeSpec = SkStrikeSpec::MakeMask(
                    runFont, paint, props, fScalerContextFlags, cacheScale);
            auto strike = strikeSpec.findOrCreateStrike();

            auto [accepted, rejected] = prepare_for_direct_bitmap_drawing(
                    strike.get(), positionMatrix, source, acceptedBuffer, rejectedBuffer);
            const SkScalar invMaxScale = 1.0f / maxScale;
            for (auto [glyph, srcPos] : accepted) {
                SkMask mask = glyph->mask();
                if (mask.fFormat != SkMask::kARGB32_Format) {
                    continue;
                }
                SkBitmap bm;
                bm.installPixels(SkImageInfo::MakeN32Premul(mask.fBounds.size()),
                                 const_cast<uint8_t*>(mask.fImage),
                                 mask.fRowBytes);
                bm.setImmutable();

                SkPoint pos = drawOrigin + srcPos
                            + SkPoint::Make(mask.fBounds.left(), mask.fBounds.top()) * invMaxScale;
                SkMatrix translate = SkMatrix::Translate(pos);
                translate.preScale(invMaxScale, invMaxScale);
                bitmapDevice->drawBitmap(
                        bm, translate, nullptr, SkFilterMode::kLinear, paint, nullptr);
            }
        }
    }
}

}  // namespace skcpu

// ============================================================================
// src/text/SlugFromBuffer.cpp
// ============================================================================

namespace sktext::gpu {

sk_sp<Slug> Slug::MakeFromBuffer(SkReadBuffer& buffer) {
    auto procs = buffer.getDeserialProcs();
    if (procs.fSlugProc) {
        return procs.fSlugProc(buffer, procs.fSlugCtx);
    }
    SkDEBUGFAIL("Should have set serial procs");
    return nullptr;
}

uint32_t Slug::NextUniqueID() {
    static std::atomic<uint32_t> nextUnique = 1;
    return nextUnique++;
}

}  // namespace sktext::gpu
