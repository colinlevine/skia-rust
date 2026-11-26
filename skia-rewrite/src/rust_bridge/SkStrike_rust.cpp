/*
 * Safe Rust bridge for:
 *   src/core/SkStrike.cpp
 *   src/core/SkStrikeCache.cpp
 *   src/core/SkStrikeRef.cpp
 *   src/core/SkStrikeSpec.cpp
 *   src/text/StrikeForGPU.cpp
 * (Phase 1 Layer 5.1 skia-rs-text, GN target :core).
 */

#include "include/core/SkDrawable.h"
#include "include/core/SkFont.h"
#include "include/core/SkFontStyle.h"
#include "include/core/SkGraphics.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPath.h"
#include "include/core/SkPathEffect.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStrikeRef.h"
#include "include/core/SkString.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTraceMemoryDump.h"
#include "include/core/SkTypeface.h"
#include "include/private/SkAPI.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMutex.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkContextPriv.h"
#include "src/core/SkDescriptor.h"
#include "src/core/SkFontPriv.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkMask.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkScalerContext.h"
#include "src/core/SkSharedState.h"
#include "src/core/SkStrike.h"
#include "src/core/SkStrikeCache.h"
#include "src/core/SkStrikeSpec.h"
#include "src/core/SkWriteBuffer.h"
#include "src/core/SkZip.h"
#include "src/text/StrikeForGPU.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <new>
#include <optional>
#include <utility>

using namespace skglyph;
using namespace skia_private;
using namespace sktext;

namespace {

struct RsStrikePurgeTargets {
    size_t bytes_needed;
    int32_t count_needed;
};

extern "C" {
bool skia_rs_text_strike_should_draw_as_path(
        uint8_t paint_style, float stroke_width, float font_size,
        float font_scale_x, float font_skew_x, const SkMatrix* view_matrix);
void skia_rs_text_strike_purge_targets(
        size_t total_memory_used, size_t cache_size_limit, size_t min_bytes_needed,
        int32_t cache_count, int32_t cache_count_limit, int32_t pinner_count,
        bool check_pinners, RsStrikePurgeTargets* out);
void skia_rs_text_strike_sanitize_font_name(uint8_t* bytes, size_t len);
size_t skia_rs_text_strike_strided_span_len(uint32_t count, uint32_t stride_32);
void skia_rs_text_scale_rect(const SkRect* r, float s, SkRect* out);
}

static SkFontMetrics use_or_generate_metrics(
        const SkFontMetrics* metrics, SkScalerContext* context) {
    SkFontMetrics answer;
    if (metrics) {
        answer = *metrics;
    } else {
        context->getFontMetrics(&answer);
    }
    return answer;
}

}  // namespace

// ============================================================================
// SkStrike.cpp
// ============================================================================

SkStrike::SkStrike(SkStrikeCache* strikeCache,
                   const SkStrikeSpec& strikeSpec,
                   std::unique_ptr<SkScalerContext> scaler,
                   const SkFontMetrics* metrics,
                   std::unique_ptr<SkStrikePinner> pinner)
        : fFontMetrics{use_or_generate_metrics(metrics, scaler.get())}
        , fRoundingSpec{scaler->isSubpixel(),
                        scaler->computeAxisAlignmentForHText()}
        , fStrikeSpec{strikeSpec}
        , fStrikeCache{strikeCache}
        , fScalerContext{std::move(scaler)}
        , fPinner{std::move(pinner)} {
    SkASSERT(fScalerContext != nullptr);
}

class SK_SCOPED_CAPABILITY SkStrike::Monitor {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    Monitor(SkStrike* strike) SK_ACQUIRE(strike->fStrikeLock) : fStrike{strike} {
        fStrike->lock();
    }

    ~Monitor() SK_RELEASE_CAPABILITY() {
        fStrike->unlock();
    }

private:
    SkStrike* const fStrike;
};

void SkStrike::lock() {
    fStrikeLock.acquire();
    fMemoryIncrease = 0;
}

void SkStrike::unlock() {
    const size_t memoryIncrease = fMemoryIncrease;
    fStrikeLock.release();
    this->updateMemoryUsage(memoryIncrease);
}

void SkStrike::FlattenGlyphsByType(SkWriteBuffer& buffer,
                                   SkSpan<SkGlyph> images,
                                   SkSpan<SkGlyph> paths,
                                   SkSpan<SkGlyph> drawables) {
    SkASSERT_RELEASE(SkTFitsIn<int>(images.size()) &&
                     SkTFitsIn<int>(paths.size()) &&
                     SkTFitsIn<int>(drawables.size()));

    buffer.writeInt(images.size());
    for (SkGlyph& glyph : images) {
        SkASSERT(SkMask::IsValidFormat(glyph.maskFormat()));
        glyph.flattenMetrics(buffer);
        glyph.flattenImage(buffer);
    }

    buffer.writeInt(paths.size());
    for (SkGlyph& glyph : paths) {
        SkASSERT(SkMask::IsValidFormat(glyph.maskFormat()));
        glyph.flattenMetrics(buffer);
        glyph.flattenPath(buffer);
    }

    buffer.writeInt(drawables.size());
    for (SkGlyph& glyph : drawables) {
        SkASSERT(SkMask::IsValidFormat(glyph.maskFormat()));
        glyph.flattenMetrics(buffer);
        glyph.flattenDrawable(buffer);
    }
}

bool SkStrike::mergeFromBuffer(SkReadBuffer& buffer) {
    const int imagesCount = buffer.readInt();
    if (imagesCount == 0 && !buffer.isValid()) {
        return false;
    }
    {
        Monitor m{this};
        for (int curImage = 0; curImage < imagesCount; ++curImage) {
            if (!this->mergeGlyphAndImageFromBuffer(buffer)) {
                return false;
            }
        }
    }

    const int pathsCount = buffer.readInt();
    if (pathsCount == 0 && !buffer.isValid()) {
        return false;
    }
    {
        Monitor m{this};
        for (int curPath = 0; curPath < pathsCount; ++curPath) {
            if (!this->mergeGlyphAndPathFromBuffer(buffer)) {
                return false;
            }
        }
    }

    const int drawablesCount = buffer.readInt();
    if (drawablesCount == 0 && !buffer.isValid()) {
        return false;
    }
    {
        Monitor m{this};
        for (int curDrawable = 0; curDrawable < drawablesCount; ++curDrawable) {
            if (!this->mergeGlyphAndDrawableFromBuffer(buffer)) {
                return false;
            }
        }
    }

    return true;
}

SkGlyph* SkStrike::mergeGlyphAndImage(SkPackedGlyphID toID, const SkGlyph& fromGlyph) {
    Monitor m{this};
    SkGlyphDigest* digest = fDigestForPackedGlyphID.find(toID);
    if (digest != nullptr) {
        SkGlyph* glyph = fGlyphForIndex[digest->index()];
        if (fromGlyph.setImageHasBeenCalled()) {
            if (glyph->setImageHasBeenCalled()) {
                SkDEBUGFAIL("Re-adding image to existing glyph. This should not happen.");
            }
            fMemoryIncrease += glyph->setMetricsAndImage(&fAlloc, fromGlyph);
        }
        return glyph;
    } else {
        SkGlyph* glyph = fAlloc.make<SkGlyph>(toID);
        fMemoryIncrease += glyph->setMetricsAndImage(&fAlloc, fromGlyph) + sizeof(SkGlyph);
        (void)this->addGlyphAndDigest(glyph);
        return glyph;
    }
}

const SkPath* SkStrike::mergePath(SkGlyph* glyph, const SkPath* path, bool hairline, bool modified) {
    Monitor m{this};
    if (glyph->setPathHasBeenCalled()) {
        SkDEBUGFAIL("Re-adding path to existing glyph. This should not happen.");
    }
    if (glyph->setPath(&fAlloc, path, hairline, modified)) {
        fMemoryIncrease += glyph->path()->approximateBytesUsed();
    }
    return glyph->path();
}

const SkDrawable* SkStrike::mergeDrawable(SkGlyph* glyph, sk_sp<SkDrawable> drawable) {
    Monitor m{this};
    if (glyph->setDrawableHasBeenCalled()) {
        SkDEBUGFAIL("Re-adding drawable to existing glyph. This should not happen.");
    }
    if (glyph->setDrawable(&fAlloc, std::move(drawable))) {
        fMemoryIncrease += glyph->drawable()->approximateBytesUsed();
        SkASSERT(fMemoryIncrease > 0);
    }
    return glyph->drawable();
}

void SkStrike::findIntercepts(const SkScalar bounds[2], SkScalar scale, SkScalar xPos,
                              SkGlyph* glyph, SkScalar* array, int* count) {
    SkAutoMutexExclusive lock{fStrikeLock};
    glyph->ensureIntercepts(bounds, scale, xPos, array, count, &fAlloc);
}

SkSpan<const SkGlyph*> SkStrike::metrics(
        SkSpan<const SkGlyphID> glyphIDs, const SkGlyph* results[]) {
    Monitor m{this};
    return this->internalPrepare(glyphIDs, kMetricsOnly, results);
}

void SkStrike::getWidthsStrided(unsigned count,
                                SkSpan<const uint32_t> glyphs,
                                unsigned glyph_stride_32,
                                SkSpan<SkScalar> advances,
                                unsigned advance_stride_32,
                                SkScalar scale) {
    Monitor m{this};
    for (unsigned i = 0; i < count; ++i) {
        SkGlyph* glyph = this->glyph(SkPackedGlyphID{static_cast<SkGlyphID>(glyphs[0])});
        advances[0] = glyph->advanceX() * scale;
        glyphs = glyphs.subspan(glyph_stride_32);
        advances = advances.subspan(advance_stride_32);
    }
}

SkSpan<const SkGlyph*> SkStrike::preparePaths(
        SkSpan<const SkGlyphID> glyphIDs, const SkGlyph* results[]) {
    Monitor m{this};
    return this->internalPrepare(glyphIDs, kMetricsAndPath, results);
}

SkSpan<const SkGlyph*> SkStrike::prepareImages(
        SkSpan<const SkPackedGlyphID> glyphIDs, const SkGlyph* results[]) {
    const SkGlyph** cursor = results;
    Monitor m{this};
    for (auto glyphID : glyphIDs) {
        SkGlyph* glyph = this->glyph(glyphID);
        this->prepareForImage(glyph);
        *cursor++ = glyph;
    }
    return {results, glyphIDs.size()};
}

SkSpan<const SkGlyph*> SkStrike::prepareDrawables(
        SkSpan<const SkGlyphID> glyphIDs, const SkGlyph* results[]) {
    const SkGlyph** cursor = results;
    {
        Monitor m{this};
        for (auto glyphID : glyphIDs) {
            SkGlyph* glyph = this->glyph(SkPackedGlyphID{glyphID});
            this->prepareForDrawable(glyph);
            *cursor++ = glyph;
        }
    }
    return {results, glyphIDs.size()};
}

void SkStrike::glyphIDsToPaths(SkSpan<sktext::IDOrPath> idsOrPaths) {
    Monitor m{this};
    for (sktext::IDOrPath& idOrPath : idsOrPaths) {
        SkGlyph* glyph = this->glyph(SkPackedGlyphID{idOrPath.fGlyphID});
        this->prepareForPath(glyph);
        idOrPath.fPath = *glyph->path();
    }
}

void SkStrike::glyphIDsToDrawables(SkSpan<sktext::IDOrDrawable> idsOrDrawables) {
    Monitor m{this};
    for (sktext::IDOrDrawable& idOrDrawable : idsOrDrawables) {
        SkGlyph* glyph = this->glyph(SkPackedGlyphID{idOrDrawable.fGlyphID});
        this->prepareForDrawable(glyph);
        SkASSERT(glyph->drawable() != nullptr);
        idOrDrawable.fDrawable = glyph->drawable();
    }
}

void SkStrike::dump() const {
    SkAutoMutexExclusive lock{fStrikeLock};
    const SkTypeface* face = fScalerContext->getTypeface();
    const SkScalerContextRec& rec = fScalerContext->getRec();
    SkString name;
    face->getFamilyName(&name);

    SkString msg;
    SkFontStyle style = face->fontStyle();
    msg.printf("cache typeface:%x %25s:(%d,%d,%d)\n %s glyphs:%3d",
               face->uniqueID(), name.c_str(), style.weight(), style.width(), style.slant(),
               rec.dump().c_str(), fDigestForPackedGlyphID.count());
    SkDebugf("%s\n", msg.c_str());
}

void SkStrike::dumpMemoryStatistics(SkTraceMemoryDump* dump) const {
    SkAutoMutexExclusive lock{fStrikeLock};
    const SkTypeface* face = fScalerContext->getTypeface();
    const SkScalerContextRec& rec = fScalerContext->getRec();

    SkString fontName;
    face->getFamilyName(&fontName);
    skia_rs_text_strike_sanitize_font_name(
            reinterpret_cast<uint8_t*>(fontName.data()), fontName.size());

    SkString dumpName = SkStringPrintf("%s/%s_%u/%p",
                                       SkStrikeCache::kGlyphCacheDumpName,
                                       fontName.c_str(),
                                       rec.fTypefaceID,
                                       this);

    dump->dumpNumericValue(dumpName.c_str(), "size", "bytes", fMemoryUsed);
    dump->dumpNumericValue(dumpName.c_str(),
                           "glyph_count", "objects",
                           fDigestForPackedGlyphID.count());
    dump->setMemoryBacking(dumpName.c_str(), "malloc", nullptr);
}

SkGlyph* SkStrike::glyph(SkGlyphDigest digest) {
    return fGlyphForIndex[digest.index()];
}

SkGlyph* SkStrike::glyph(SkPackedGlyphID packedGlyphID) {
    SkGlyphDigest digest = this->digestFor(kDirectMask, packedGlyphID);
    return this->glyph(digest);
}

SkGlyphDigest SkStrike::digestFor(ActionType actionType, SkPackedGlyphID packedGlyphID) {
    SkGlyphDigest* digestPtr = fDigestForPackedGlyphID.find(packedGlyphID);
    if (digestPtr != nullptr && digestPtr->actionFor(actionType) != GlyphAction::kUnset) {
        return *digestPtr;
    }

    SkGlyph* glyph;
    if (digestPtr != nullptr) {
        glyph = fGlyphForIndex[digestPtr->index()];
    } else {
        glyph = fAlloc.make<SkGlyph>(fScalerContext->makeGlyph(packedGlyphID, &fAlloc));
        fMemoryIncrease += sizeof(SkGlyph);
        digestPtr = this->addGlyphAndDigest(glyph);
    }

    digestPtr->setActionFor(actionType, glyph, this);
    return *digestPtr;
}

SkGlyphDigest* SkStrike::addGlyphAndDigest(SkGlyph* glyph) {
    size_t index = fGlyphForIndex.size();
    SkGlyphDigest digest = SkGlyphDigest{index, *glyph};
    SkGlyphDigest* newDigest = fDigestForPackedGlyphID.set(digest);
    fGlyphForIndex.push_back(glyph);
    return newDigest;
}

bool SkStrike::prepareForImage(SkGlyph* glyph) {
    if (glyph->setImage(&fAlloc, fScalerContext.get())) {
        fMemoryIncrease += glyph->imageSize();
    }
    return glyph->image() != nullptr;
}

bool SkStrike::prepareForPath(SkGlyph* glyph) {
    if (glyph->setPath(&fAlloc, fScalerContext.get())) {
        fMemoryIncrease += glyph->path()->approximateBytesUsed();
    }
    return glyph->path() != nullptr;
}

bool SkStrike::prepareForDrawable(SkGlyph* glyph) {
    if (glyph->setDrawable(&fAlloc, fScalerContext.get())) {
        size_t increase = glyph->drawable()->approximateBytesUsed();
        SkASSERT(increase > 0);
        fMemoryIncrease += increase;
    }
    return glyph->drawable() != nullptr;
}

SkGlyph* SkStrike::mergeGlyphFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());
    std::optional<SkGlyph> prototypeGlyph = SkGlyph::MakeFromBuffer(buffer);
    if (!buffer.validate(prototypeGlyph.has_value())) {
        return nullptr;
    }

    SkGlyphDigest* digestPtr = fDigestForPackedGlyphID.find(prototypeGlyph->getPackedID());
    if (digestPtr != nullptr) {
        return fGlyphForIndex[digestPtr->index()];
    }

    SkGlyph* glyph = fAlloc.make<SkGlyph>(prototypeGlyph.value());
    fMemoryIncrease += sizeof(SkGlyph);
    this->addGlyphAndDigest(glyph);
    return glyph;
}

bool SkStrike::mergeGlyphAndImageFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());
    SkGlyph* glyph = this->mergeGlyphFromBuffer(buffer);
    if (!buffer.validate(glyph != nullptr)) {
        return false;
    }
    fMemoryIncrease += glyph->addImageFromBuffer(buffer, &fAlloc);
    return buffer.isValid();
}

bool SkStrike::mergeGlyphAndPathFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());
    SkGlyph* glyph = this->mergeGlyphFromBuffer(buffer);
    if (!buffer.validate(glyph != nullptr)) {
        return false;
    }
    fMemoryIncrease += glyph->addPathFromBuffer(buffer, &fAlloc);
    return buffer.isValid();
}

bool SkStrike::mergeGlyphAndDrawableFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());
    SkGlyph* glyph = this->mergeGlyphFromBuffer(buffer);
    if (!buffer.validate(glyph != nullptr)) {
        return false;
    }
    fMemoryIncrease += glyph->addDrawableFromBuffer(buffer, &fAlloc);
    return buffer.isValid();
}

SkSpan<const SkGlyph*> SkStrike::internalPrepare(
        SkSpan<const SkGlyphID> glyphIDs, PathDetail pathDetail, const SkGlyph** results) {
    const SkGlyph** cursor = results;
    for (auto glyphID : glyphIDs) {
        SkGlyph* glyph = this->glyph(SkPackedGlyphID{glyphID});
        if (pathDetail == kMetricsAndPath) {
            this->prepareForPath(glyph);
        }
        *cursor++ = glyph;
    }
    return {results, glyphIDs.size()};
}

void SkStrike::updateMemoryUsage(size_t increase) {
    if (increase > 0) {
        SkAutoMutexExclusive lock{fStrikeCache->fLock};
        fMemoryUsed += increase;
        if (!fRemoved) {
            fStrikeCache->fTotalMemoryUsed += increase;
        }
    }
}

// ============================================================================
// SkStrikeCache.cpp
// ============================================================================

SkStrikeCache* SkStrikeCache::GlobalStrikeCache() {
#if defined(SK_ENABLE_THREADLOCAL_STRIKECACHE)
    static thread_local auto* cache = new SkStrikeCache;
    return cache;
#else
    return SkContexts::GetGlobalSkContext()->priv().sharedState()->strikeCache();
#endif
}

auto SkStrikeCache::findOrCreateStrike(const SkStrikeSpec& strikeSpec) -> sk_sp<SkStrike> {
    SkAutoMutexExclusive ac(fLock);
    sk_sp<SkStrike> strike = this->internalFindStrikeOrNull(strikeSpec.descriptor());
    if (strike == nullptr) {
        strike = this->internalCreateStrike(strikeSpec);
    }
    this->internalPurge();
    return strike;
}

sk_sp<StrikeForGPU> SkStrikeCache::findOrCreateScopedStrike(const SkStrikeSpec& strikeSpec) {
    return this->findOrCreateStrike(strikeSpec);
}

void SkStrikeCache::PurgeAll() { GlobalStrikeCache()->purgeAll(); }

void SkStrikeCache::Dump() {
    SkDebugf("GlyphCache [     used    budget ]\n");
    SkDebugf("    bytes  [ %8zu  %8zu ]\n",
             SkGraphics::GetFontCacheUsed(), SkGraphics::GetFontCacheLimit());
    SkDebugf("    count  [ %8d  %8d ]\n",
             SkGraphics::GetFontCacheCountUsed(), SkGraphics::GetFontCacheCountLimit());

    auto visitor = [](const SkStrike& strike) { strike.dump(); };
    GlobalStrikeCache()->forEachStrike(visitor);
}

void SkStrikeCache::dumpMemoryStatistics(SkTraceMemoryDump* dump) const {
    dump->dumpNumericValue(kGlyphCacheDumpName, "size", "bytes", this->getTotalMemoryUsed());
    dump->dumpNumericValue(kGlyphCacheDumpName, "budget_size", "bytes", this->getCacheSizeLimit());
    dump->dumpNumericValue(
            kGlyphCacheDumpName, "glyph_count", "objects", this->getCacheCountUsed());
    dump->dumpNumericValue(
            kGlyphCacheDumpName, "budget_glyph_count", "objects", this->getCacheCountLimit());

    if (dump->getRequestedDetails() == SkTraceMemoryDump::kLight_LevelOfDetail) {
        dump->setMemoryBacking(kGlyphCacheDumpName, "malloc", nullptr);
        return;
    }

    auto visitor = [&](const SkStrike& strike) { strike.dumpMemoryStatistics(dump); };
    this->forEachStrike(visitor);
}

void SkStrikeCache::DumpMemoryStatistics(SkTraceMemoryDump* dump) {
    GlobalStrikeCache()->dumpMemoryStatistics(dump);
}

sk_sp<SkStrike> SkStrikeCache::findStrike(const SkDescriptor& desc) {
    SkAutoMutexExclusive ac(fLock);
    sk_sp<SkStrike> result = this->internalFindStrikeOrNull(desc);
    this->internalPurge();
    return result;
}

auto SkStrikeCache::internalFindStrikeOrNull(const SkDescriptor& desc) -> sk_sp<SkStrike> {
    if (fHead != nullptr && fHead->getDescriptor() == desc) { return sk_ref_sp(fHead); }

    sk_sp<SkStrike>* strikeHandle = fStrikeLookup.find(desc);
    if (strikeHandle == nullptr) { return nullptr; }
    SkStrike* strikePtr = strikeHandle->get();
    SkASSERT(strikePtr != nullptr);
    if (fHead != strikePtr) {
        strikePtr->fPrev->fNext = strikePtr->fNext;
        if (strikePtr->fNext != nullptr) {
            strikePtr->fNext->fPrev = strikePtr->fPrev;
        } else {
            fTail = strikePtr->fPrev;
        }
        fHead->fPrev = strikePtr;
        strikePtr->fNext = fHead;
        strikePtr->fPrev = nullptr;
        fHead = strikePtr;
    }
    return sk_ref_sp(strikePtr);
}

sk_sp<SkStrike> SkStrikeCache::createStrike(
        const SkStrikeSpec& strikeSpec,
        SkFontMetrics* maybeMetrics,
        std::unique_ptr<SkStrikePinner> pinner) {
    SkAutoMutexExclusive ac(fLock);
    return this->internalCreateStrike(strikeSpec, maybeMetrics, std::move(pinner));
}

auto SkStrikeCache::internalCreateStrike(
        const SkStrikeSpec& strikeSpec,
        SkFontMetrics* maybeMetrics,
        std::unique_ptr<SkStrikePinner> pinner) -> sk_sp<SkStrike> {
    std::unique_ptr<SkScalerContext> scaler = strikeSpec.createScalerContext();
    auto strike = sk_make_sp<SkStrike>(
            this, strikeSpec, std::move(scaler), maybeMetrics, std::move(pinner));
    this->internalAttachToHead(strike);
    return strike;
}

void SkStrikeCache::purgePinned(size_t minBytesNeeded) {
    SkAutoMutexExclusive ac(fLock);
    this->internalPurge(minBytesNeeded, true);
}

void SkStrikeCache::purgeAll() {
    SkAutoMutexExclusive ac(fLock);
    this->internalPurge(fTotalMemoryUsed, true);
}

size_t SkStrikeCache::getTotalMemoryUsed() const {
    SkAutoMutexExclusive ac(fLock);
    return fTotalMemoryUsed;
}

int SkStrikeCache::getCacheCountUsed() const {
    SkAutoMutexExclusive ac(fLock);
    return fCacheCount;
}

int SkStrikeCache::getCacheCountLimit() const {
    SkAutoMutexExclusive ac(fLock);
    return fCacheCountLimit;
}

size_t SkStrikeCache::setCacheSizeLimit(size_t newLimit) {
    SkAutoMutexExclusive ac(fLock);
    size_t prevLimit = fCacheSizeLimit;
    fCacheSizeLimit = newLimit;
    this->internalPurge();
    return prevLimit;
}

size_t SkStrikeCache::getCacheSizeLimit() const {
    SkAutoMutexExclusive ac(fLock);
    return fCacheSizeLimit;
}

int SkStrikeCache::setCacheCountLimit(int newCount) {
    if (newCount < 0) {
        newCount = 0;
    }
    SkAutoMutexExclusive ac(fLock);
    int prevCount = fCacheCountLimit;
    fCacheCountLimit = newCount;
    this->internalPurge();
    return prevCount;
}

void SkStrikeCache::forEachStrike(std::function<void(const SkStrike&)> visitor) const {
    SkAutoMutexExclusive ac(fLock);
    this->validate();
    for (SkStrike* strike = fHead; strike != nullptr; strike = strike->fNext) {
        visitor(*strike);
    }
}

size_t SkStrikeCache::internalPurge(size_t minBytesNeeded, bool checkPinners) {
#ifndef SK_STRIKE_CACHE_DOESNT_AUTO_CHECK_PINNERS
    checkPinners = true;
#endif

    RsStrikePurgeTargets targets;
    skia_rs_text_strike_purge_targets(
            fTotalMemoryUsed, fCacheSizeLimit, minBytesNeeded,
            fCacheCount, fCacheCountLimit, fPinnerCount, checkPinners, &targets);

    if (!targets.count_needed && !targets.bytes_needed) {
        return 0;
    }

    size_t bytesFreed = 0;
    int countFreed = 0;

    SkStrike* strike = fTail;
    while (strike != nullptr &&
           (bytesFreed < targets.bytes_needed || countFreed < targets.count_needed)) {
        SkStrike* prev = strike->fPrev;
        if (strike->fPinner == nullptr || (checkPinners && strike->fPinner->canDelete())) {
            bytesFreed += strike->fMemoryUsed;
            countFreed += 1;
            this->internalRemoveStrike(strike);
        }
        strike = prev;
    }

    this->validate();
    return bytesFreed;
}

void SkStrikeCache::internalAttachToHead(sk_sp<SkStrike> strike) {
    SkASSERT(fStrikeLookup.find(strike->getDescriptor()) == nullptr);
    SkStrike* strikePtr = strike.get();
    fStrikeLookup.set(std::move(strike));
    SkASSERT(nullptr == strikePtr->fPrev && nullptr == strikePtr->fNext);

    fCacheCount += 1;
    fPinnerCount += strikePtr->fPinner != nullptr ? 1 : 0;
    fTotalMemoryUsed += strikePtr->fMemoryUsed;

    if (fHead != nullptr) {
        fHead->fPrev = strikePtr;
        strikePtr->fNext = fHead;
    }
    if (fTail == nullptr) {
        fTail = strikePtr;
    }
    fHead = strikePtr;
}

void SkStrikeCache::internalRemoveStrike(SkStrike* strike) {
    SkASSERT(fCacheCount > 0);
    fCacheCount -= 1;
    fPinnerCount -= strike->fPinner != nullptr ? 1 : 0;
    fTotalMemoryUsed -= strike->fMemoryUsed;

    if (strike->fPrev) {
        strike->fPrev->fNext = strike->fNext;
    } else {
        fHead = strike->fNext;
    }
    if (strike->fNext) {
        strike->fNext->fPrev = strike->fPrev;
    } else {
        fTail = strike->fPrev;
    }

    strike->fPrev = strike->fNext = nullptr;
    strike->fRemoved = true;
    fStrikeLookup.remove(strike->getDescriptor());
}

void SkStrikeCache::validate() const {
#ifdef SK_DEBUG
    size_t computedBytes = 0;
    int computedCount = 0;

    const SkStrike* strike = fHead;
    while (strike != nullptr) {
        computedBytes += strike->fMemoryUsed;
        computedCount += 1;
        SkASSERT(fStrikeLookup.findOrNull(strike->getDescriptor()) != nullptr);
        strike = strike->fNext;
    }

    if (fCacheCount != computedCount) {
        SkDebugf("fCacheCount: %d, computedCount: %d", fCacheCount, computedCount);
        SK_ABORT("fCacheCount != computedCount");
    }
    if (fTotalMemoryUsed != computedBytes) {
        SkDebugf("fTotalMemoryUsed: %zu, computedBytes: %zu", fTotalMemoryUsed, computedBytes);
        SK_ABORT("fTotalMemoryUsed == computedBytes");
    }
#endif
}

const SkDescriptor& SkStrikeCache::StrikeTraits::GetKey(const sk_sp<SkStrike>& strike) {
    return strike->getDescriptor();
}

uint32_t SkStrikeCache::StrikeTraits::Hash(const SkDescriptor& descriptor) {
    return descriptor.getChecksum();
}

// ============================================================================
// SkStrikeRef.cpp
// ============================================================================

SkStrikeRef::SkStrikeRef() = default;
SkStrikeRef::~SkStrikeRef() = default;
SkStrikeRef::SkStrikeRef(const SkStrikeRef&) = default;
SkStrikeRef& SkStrikeRef::operator=(const SkStrikeRef&) = default;
SkStrikeRef::SkStrikeRef(SkStrikeRef&&) = default;
SkStrikeRef& SkStrikeRef::operator=(SkStrikeRef&&) = default;

SkStrikeRef::SkStrikeRef(sk_sp<SkStrike> strike, SkScalar strikeToSourceScale)
    : fStrike(std::move(strike))
    , fStrikeToSourceScale(strikeToSourceScale) {}

void SkStrikeRef::getWidths(SkSpan<const SkGlyphID> glyphs, SkSpan<SkScalar> widths) const {
    this->getWidthsBounds(glyphs, widths, {});
}

SkScalar SkStrikeRef::getWidth(SkGlyphID glyph) const {
    SkScalar width;
    this->getWidths({&glyph, 1}, {&width, 1});
    return width;
}

void SkStrikeRef::getWidthsBounds(SkSpan<const SkGlyphID> glyphs,
                                  SkSpan<SkScalar> widths,
                                  SkSpan<SkRect> bounds) const {
    SkASSERT(fStrike);

    AutoSTArray<64, const SkGlyph*> glyphPtrs(SkTo<int>(glyphs.size()));
    SkSpan<const SkGlyph*> results = fStrike->metrics(glyphs, glyphPtrs.get());

    if (bounds.size()) {
        const auto n = std::min(bounds.size(), results.size());
        for (size_t i = 0; i < n; ++i) {
            SkRect r = results[i]->rect();
            skia_rs_text_scale_rect(&r, fStrikeToSourceScale, &bounds[i]);
        }
    }

    if (widths.size()) {
        const auto n = std::min(widths.size(), results.size());
        for (auto [width, glyph] : SkMakeZip(widths.first(n), results.first(n))) {
            width = glyph->advanceX() * fStrikeToSourceScale;
        }
    }
}

void SkStrikeRef::getWidthsStrided(unsigned count,
                                   SkSpan<const uint32_t> glyphs,
                                   unsigned glyph_stride_32,
                                   SkSpan<SkScalar> advances,
                                   unsigned advance_stride_32) const {
    SkASSERT(fStrike);
    fStrike->getWidthsStrided(count, glyphs, glyph_stride_32, advances,
                              advance_stride_32, fStrikeToSourceScale);
}

void SkStrikeRef::getWidthsStrided(unsigned count,
                                   const uint32_t* first_glyph,
                                   unsigned glyph_stride_32,
                                   SkScalar* first_advance,
                                   unsigned advance_stride_32) const {
    if (count == 0) {
        return;
    }
    const size_t glyphCount = skia_rs_text_strike_strided_span_len(count, glyph_stride_32);
    const size_t advanceCount = skia_rs_text_strike_strided_span_len(count, advance_stride_32);
    this->getWidthsStrided(count,
                           SK_UNSAFE_TODO(SkSpan(first_glyph, glyphCount)),
                           glyph_stride_32,
                           SK_UNSAFE_TODO(SkSpan(first_advance, advanceCount)),
                           advance_stride_32);
}

// ============================================================================
// SkStrikeSpec.cpp
// ============================================================================

SkStrikeSpec::SkStrikeSpec(const SkDescriptor& descriptor, sk_sp<SkTypeface> typeface)
    : fAutoDescriptor{descriptor}
    , fTypeface{std::move(typeface)} {}

SkStrikeSpec::SkStrikeSpec(const SkStrikeSpec&) = default;
SkStrikeSpec::SkStrikeSpec(SkStrikeSpec&&) = default;
SkStrikeSpec::~SkStrikeSpec() = default;

SkStrikeSpec SkStrikeSpec::MakeMask(const SkFont& font, const SkPaint& paint,
                                    const SkSurfaceProps& surfaceProps,
                                    SkScalerContextFlags scalerContextFlags,
                                    const SkMatrix& deviceMatrix) {
    return SkStrikeSpec(font, paint, surfaceProps, scalerContextFlags, deviceMatrix);
}

SkStrikeSpec SkStrikeSpec::MakeTransformMask(const SkFont& font,
                                             const SkPaint& paint,
                                             const SkSurfaceProps& surfaceProps,
                                             SkScalerContextFlags scalerContextFlags,
                                             const SkMatrix& deviceMatrix) {
    SkFont sourceFont{font};
    sourceFont.setSubpixel(false);
    return SkStrikeSpec(sourceFont, paint, surfaceProps, scalerContextFlags, deviceMatrix);
}

std::tuple<SkStrikeSpec, SkScalar> SkStrikeSpec::MakePath(
        const SkFont& font, const SkPaint& paint,
        const SkSurfaceProps& surfaceProps,
        SkScalerContextFlags scalerContextFlags) {
    SkPaint pathPaint{paint};
    SkFont pathFont{font};
    pathFont.setSubpixel(false);
    SkScalar strikeToSourceScale = pathFont.setupForAsPaths(&pathPaint);
    return {SkStrikeSpec(pathFont, pathPaint, surfaceProps, scalerContextFlags, SkMatrix::I()),
            strikeToSourceScale};
}

std::tuple<SkStrikeSpec, SkScalar> SkStrikeSpec::MakeCanonicalized(
        const SkFont& font, const SkPaint* paint) {
    SkPaint canonicalizedPaint;
    if (paint != nullptr) {
        canonicalizedPaint = *paint;
    }

    const SkFont* canonicalizedFont = &font;
    std::optional<SkFont> pathFont;
    SkScalar strikeToSourceScale = 1;
    if (ShouldDrawAsPath(canonicalizedPaint, font, SkMatrix::I())) {
        pathFont = font;
        canonicalizedFont = &pathFont.value();
        strikeToSourceScale = pathFont->setupForAsPaths(nullptr);
        canonicalizedPaint.reset();
    }

    return {SkStrikeSpec(*canonicalizedFont, canonicalizedPaint, SkSurfaceProps(),
                         SkScalerContextFlags::kFakeGammaAndBoostContrast, SkMatrix::I()),
            strikeToSourceScale};
}

SkStrikeSpec SkStrikeSpec::MakeWithNoDevice(const SkFont& font, const SkPaint* paint,
                                            SkScalerContextFlags flags) {
    SkPaint setupPaint;
    if (paint != nullptr) {
        setupPaint = *paint;
    }
    return SkStrikeSpec(font, setupPaint, SkSurfaceProps(), flags, SkMatrix::I());
}

bool SkStrikeSpec::ShouldDrawAsPath(
        const SkPaint& paint, const SkFont& font, const SkMatrix& viewMatrix) {
    return skia_rs_text_strike_should_draw_as_path(
            static_cast<uint8_t>(paint.getStyle()), paint.getStrokeWidth(),
            font.getSize(), font.getScaleX(), font.getSkewX(), &viewMatrix);
}

SkString SkStrikeSpec::dump() const {
    return fAutoDescriptor.getDesc()->dumpRec();
}

SkStrikeSpec::SkStrikeSpec(const SkFont& font, const SkPaint& paint,
                           const SkSurfaceProps& surfaceProps,
                           SkScalerContextFlags scalerContextFlags,
                           const SkMatrix& deviceMatrix) {
    SkScalerContextEffects effects;
    SkScalerContext::CreateDescriptorAndEffectsUsingPaint(
            font, paint, surfaceProps, scalerContextFlags, deviceMatrix,
            &fAutoDescriptor, &effects);
    fMaskFilter = sk_ref_sp(effects.fMaskFilter);
    fPathEffect = sk_ref_sp(effects.fPathEffect);
    fTypeface = font.refTypeface();
}

sk_sp<sktext::StrikeForGPU> SkStrikeSpec::findOrCreateScopedStrike(
        sktext::StrikeForGPUCacheInterface* cache) const {
    return cache->findOrCreateScopedStrike(*this);
}

sk_sp<SkStrike> SkStrikeSpec::findOrCreateStrike() const {
    return SkStrikeCache::GlobalStrikeCache()->findOrCreateStrike(*this);
}

sk_sp<SkStrike> SkStrikeSpec::findOrCreateStrike(SkStrikeCache* cache) const {
    return cache->findOrCreateStrike(*this);
}

SkBulkGlyphMetrics::SkBulkGlyphMetrics(const SkStrikeSpec& spec)
    : fStrike{spec.findOrCreateStrike()} {}
SkBulkGlyphMetrics::~SkBulkGlyphMetrics() = default;

SkSpan<const SkGlyph*> SkBulkGlyphMetrics::glyphs(SkSpan<const SkGlyphID> glyphIDs) {
    fGlyphs.reset(glyphIDs.size());
    return fStrike->metrics(glyphIDs, fGlyphs.get());
}

const SkGlyph* SkBulkGlyphMetrics::glyph(SkGlyphID glyphID) {
    return this->glyphs(SkSpan<const SkGlyphID>{&glyphID, 1})[0];
}

SkBulkGlyphMetricsAndPaths::SkBulkGlyphMetricsAndPaths(const SkStrikeSpec& spec)
    : fStrike{spec.findOrCreateStrike()} {}
SkBulkGlyphMetricsAndPaths::SkBulkGlyphMetricsAndPaths(sk_sp<SkStrike>&& strike)
    : fStrike{std::move(strike)} {}
SkBulkGlyphMetricsAndPaths::~SkBulkGlyphMetricsAndPaths() = default;

SkSpan<const SkGlyph*> SkBulkGlyphMetricsAndPaths::glyphs(SkSpan<const SkGlyphID> glyphIDs) {
    fGlyphs.reset(glyphIDs.size());
    return fStrike->preparePaths(glyphIDs, fGlyphs.get());
}

const SkGlyph* SkBulkGlyphMetricsAndPaths::glyph(SkGlyphID glyphID) {
    return this->glyphs(SkSpan<const SkGlyphID>{&glyphID, 1})[0];
}

void SkBulkGlyphMetricsAndPaths::findIntercepts(
        const SkScalar* bounds, SkScalar scale, SkScalar xPos,
        const SkGlyph* glyph, SkScalar* array, int* count) {
    fStrike->findIntercepts(bounds, scale, xPos, const_cast<SkGlyph*>(glyph), array, count);
}

SkBulkGlyphMetricsAndDrawables::SkBulkGlyphMetricsAndDrawables(const SkStrikeSpec& spec)
    : fStrike{spec.findOrCreateStrike()} {}
SkBulkGlyphMetricsAndDrawables::SkBulkGlyphMetricsAndDrawables(sk_sp<SkStrike>&& strike)
    : fStrike{std::move(strike)} {}
SkBulkGlyphMetricsAndDrawables::~SkBulkGlyphMetricsAndDrawables() = default;

SkSpan<const SkGlyph*> SkBulkGlyphMetricsAndDrawables::glyphs(SkSpan<const SkGlyphID> glyphIDs) {
    fGlyphs.reset(glyphIDs.size());
    return fStrike->prepareDrawables(glyphIDs, fGlyphs.get());
}

const SkGlyph* SkBulkGlyphMetricsAndDrawables::glyph(SkGlyphID glyphID) {
    return this->glyphs(SkSpan<const SkGlyphID>{&glyphID, 1})[0];
}

SkBulkGlyphMetricsAndImages::SkBulkGlyphMetricsAndImages(const SkStrikeSpec& spec)
    : fStrike{spec.findOrCreateStrike()} {}
SkBulkGlyphMetricsAndImages::SkBulkGlyphMetricsAndImages(sk_sp<SkStrike>&& strike)
    : fStrike{std::move(strike)} {}
SkBulkGlyphMetricsAndImages::~SkBulkGlyphMetricsAndImages() = default;

SkSpan<const SkGlyph*> SkBulkGlyphMetricsAndImages::glyphs(
        SkSpan<const SkPackedGlyphID> glyphIDs) {
    fGlyphs.reset(glyphIDs.size());
    return fStrike->prepareImages(glyphIDs, fGlyphs.get());
}

const SkGlyph* SkBulkGlyphMetricsAndImages::glyph(SkPackedGlyphID packedID) {
    return this->glyphs(SkSpan<const SkPackedGlyphID>{&packedID, 1})[0];
}

const SkDescriptor& SkBulkGlyphMetricsAndImages::descriptor() const {
    return fStrike->getDescriptor();
}

// ============================================================================
// src/text/StrikeForGPU.cpp
// ============================================================================

namespace sktext {
SkStrikePromise::SkStrikePromise(sktext::SkStrikePromise&&) = default;
SkStrikePromise& SkStrikePromise::operator=(sktext::SkStrikePromise&&) = default;
SkStrikePromise::SkStrikePromise(sk_sp<SkStrike>&& strike)
        : fStrikeInfo{std::move(strike)} {}
SkStrikePromise::SkStrikePromise(const SkStrikeSpec& spec)
        : fStrikeInfo{std::make_unique<SkStrikeSpec>(spec)} {}

SkStrike* SkStrikePromise::strike() {
    if (std::holds_alternative<std::unique_ptr<SkAutoDescriptor>>(fStrikeInfo)) {
        std::unique_ptr<SkAutoDescriptor> desc =
                std::exchange(std::get<std::unique_ptr<SkAutoDescriptor>>(fStrikeInfo), nullptr);
        fStrikeInfo = SkStrikeCache::GlobalStrikeCache()->findStrike(*desc->getDesc());
    } else if (std::holds_alternative<std::unique_ptr<SkStrikeSpec>>(fStrikeInfo)) {
        std::unique_ptr<SkStrikeSpec> spec =
                std::exchange(std::get<std::unique_ptr<SkStrikeSpec>>(fStrikeInfo), nullptr);
        fStrikeInfo = SkStrikeCache::GlobalStrikeCache()->findOrCreateStrike(*spec);
    }
    return std::get<sk_sp<SkStrike>>(fStrikeInfo).get();
}

void SkStrikePromise::resetStrike() {
    if (std::holds_alternative<sk_sp<SkStrike>>(fStrikeInfo)) {
        if (sk_sp<SkStrike>& strike = std::get<sk_sp<SkStrike>>(fStrikeInfo); strike != nullptr) {
            fStrikeInfo = std::make_unique<SkAutoDescriptor>(strike->getDescriptor());
        }
    }
}

const SkDescriptor& SkStrikePromise::descriptor() const {
    if (std::holds_alternative<std::unique_ptr<SkAutoDescriptor>>(fStrikeInfo)) {
        return *std::get<std::unique_ptr<SkAutoDescriptor>>(fStrikeInfo)->getDesc();
    }
    if (std::holds_alternative<std::unique_ptr<SkStrikeSpec>>(fStrikeInfo)) {
        return std::get<std::unique_ptr<SkStrikeSpec>>(fStrikeInfo)->descriptor();
    }
    return std::get<sk_sp<SkStrike>>(fStrikeInfo)->getDescriptor();
}

void SkStrikePromise::flatten(SkWriteBuffer& buffer) const {
    this->descriptor().flatten(buffer);
}

StrikeMutationMonitor::StrikeMutationMonitor(StrikeForGPU* strike) : fStrike{strike} {
    fStrike->lock();
}

StrikeMutationMonitor::~StrikeMutationMonitor() {
    fStrike->unlock();
}
}  // namespace sktext
