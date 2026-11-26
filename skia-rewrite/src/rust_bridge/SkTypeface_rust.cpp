/*
 * Safe Rust bridge for:
 *   src/core/SkTypeface.cpp
 *   src/core/SkTypefaceCache.cpp
 *   src/core/SkTypeface_remote.cpp
 *   src/core/SkFontMgr.cpp
 * (Phase 1 Layer 5.1 skia-rs-text, GN target :core).
 */

#include "include/core/SkData.h"
#include "include/core/SkDrawable.h"
#include "include/core/SkFont.h"
#include "include/core/SkFontArguments.h"
#include "include/core/SkFontMetrics.h"
#include "include/core/SkFontMgr.h"
#include "include/core/SkFontStyle.h"
#include "include/core/SkFontTypes.h"
#include "include/core/SkGraphics.h"
#include "include/core/SkScalar.h"
#include "include/core/SkStream.h"
#include "include/core/SkString.h"
#include "include/core/SkTypeface.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkMutex.h"
#include "include/private/SkOnce.h"
#include "include/private/SkTemplates.h"
#include "include/private/chromium/SkChromeRemoteGlyphCache.h"
#include "include/utils/SkCustomTypeface.h"
#include "src/core/SkAdvancedTypefaceMetrics.h"
#include "src/core/SkBitmaskEnum.h"
#include "src/core/SkContextPriv.h"
#include "src/core/SkDescriptor.h"
#include "src/core/SkEndian.h"
#include "src/core/SkFontDescriptor.h"
#include "src/core/SkFontPriv.h"
#include "src/core/SkGlyph.h"
#include "src/core/SkNoDestructor.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkScalerContext.h"
#include "src/core/SkSharedState.h"
#include "src/core/SkTraceEvent.h"
#include "src/core/SkTypefaceCache.h"
#include "src/core/SkTypeface_remote.h"
#include "src/core/SkWriteBuffer.h"
#include "src/sfnt/SkOTTable_OS_2.h"

#if defined(SK_TYPEFACE_FACTORY_FREETYPE)
#include "src/ports/SkTypeface_FreeType.h"
#endif

#if defined(SK_TYPEFACE_FACTORY_CORETEXT)
#include "src/ports/SkTypeface_mac_ct.h"
#endif

#if defined(SK_TYPEFACE_FACTORY_DIRECTWRITE)
#include "src/ports/SkTypeface_win_dw.h"
#endif

#if defined(SK_TYPEFACE_FACTORY_FONTATIONS)
#include "include/ports/SkTypeface_fontations.h"
#include "src/ports/SkTypeface_fontations_factory.h"
#endif

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <utility>
#include <vector>

using namespace skia_private;

namespace {

struct RsFontStyle {
    int32_t weight;
    int32_t width;
    uint8_t slant;
};

static_assert(sizeof(SkFontArguments::VariationPosition::Coordinate) == 8,
              "VariationPosition::Coordinate layout must match Rust VariationCoordinate");

extern "C" {
size_t skia_rs_text_decode_to_utf32(
        const uint8_t* text, size_t byte_length, int32_t encoding,
        int32_t* out, size_t out_len);
void skia_rs_text_font_style_from_model(
        const SkFontArguments::VariationPosition::Coordinate* coords, size_t count,
        RsFontStyle* out);
void skia_rs_text_set_model_from_style(
        int32_t weight, int32_t width, uint8_t slant,
        SkFontArguments::VariationPosition::Coordinate* out_model);
size_t skia_rs_text_css3_match_best_index(
        int32_t pattern_weight, int32_t pattern_width, uint8_t pattern_slant,
        const RsFontStyle* candidates, size_t count);
bool skia_rs_text_should_serialize_typeface_data(int32_t behavior, bool is_local_data);
uint8_t skia_rs_text_os2_fstype_to_advanced_flags(uint8_t b0, uint8_t b1);
bool skia_rs_text_next_typeface_id_checked(uint64_t id, uint32_t* out_id);
int32_t skia_rs_text_typeface_cache_purge_count(int32_t current_size, int32_t limit);
}

}  // namespace

// ============================================================================
// SkTypeface.cpp
// ============================================================================

SkTypeface::SkTypeface(const SkFontStyle& style, bool isFixedPitch)
    : fUniqueID(SkTypefaceCache::NewTypefaceID()), fStyle(style), fIsFixedPitch(isFixedPitch) {}

SkTypeface::~SkTypeface() {}

namespace {

class SkEmptyTypeface : public SkTypeface {
public:
    static sk_sp<SkTypeface> Make() {
        static SkNoDestructor<SkEmptyTypeface> instance;
        return sk_ref_sp(instance.get());
    }

    static constexpr SkTypeface::FactoryId FactoryId = SkSetFourByteTag('e','m','t','y');
    static sk_sp<SkTypeface> MakeFromStream(std::unique_ptr<SkStreamAsset> stream,
                                            const SkFontArguments&) {
        if (stream->getLength() == 0) {
            return SkEmptyTypeface::Make();
        }
        return nullptr;
    }
protected:
    friend SkNoDestructor<SkEmptyTypeface>;
    SkEmptyTypeface() : SkTypeface(SkFontStyle(), true) {}

    std::unique_ptr<SkStreamAsset> onOpenStream(int* ttcIndex) const override { return nullptr; }
    sk_sp<SkTypeface> onMakeClone(const SkFontArguments& args) const override {
        return sk_ref_sp(this);
    }
    std::unique_ptr<SkScalerContext> onCreateScalerContext(
        const SkScalerContextEffects& effects, const SkDescriptor* desc) const override {
        return SkScalerContext::MakeEmpty(*const_cast<SkEmptyTypeface*>(this), effects, desc);
    }
    void onFilterRec(SkScalerContextRec*) const override {}
    std::unique_ptr<SkAdvancedTypefaceMetrics> onGetAdvancedMetrics() const override {
        return nullptr;
    }
    void onGetFontDescriptor(SkFontDescriptor* desc, bool* serialize) const override {
        desc->setFactoryId(FactoryId);
        *serialize = false;
    }
    void onCharsToGlyphs(SkSpan<const SkUnichar> chars, SkSpan<SkGlyphID> glyphs) const override {
        sk_bzero(glyphs.data(), glyphs.size_bytes());
    }
    int onCountGlyphs() const override { return 0; }
    void getPostScriptGlyphNames(SkString*) const override {}
    void getGlyphToUnicodeMap(SkSpan<SkUnichar>) const override {}
    int onGetUPEM() const override { return 0; }
    bool onComputeBounds(SkRect* bounds) const override { return false; }

    class EmptyLocalizedStrings : public SkTypeface::LocalizedStrings {
    public:
        bool next(SkTypeface::LocalizedString*) override { return false; }
    };
    void onGetFamilyName(SkString* familyName) const override {
        familyName->reset();
    }
    bool onGetPostScriptName(SkString*) const override {
        return false;
    }
    SkTypeface::LocalizedStrings* onCreateFamilyNameIterator() const override {
        return new EmptyLocalizedStrings;
    }
    bool onGlyphMaskNeedsCurrentColor() const override {
        return false;
    }
    int onGetVariationDesignPosition(
            SkSpan<SkFontArguments::VariationPosition::Coordinate>) const override {
        return 0;
    }
    int onGetVariationDesignParameters(SkSpan<SkFontParameters::Variation::Axis>) const override {
        return 0;
    }
    int onGetTableTags(SkSpan<SkFontTableTag>) const override { return 0; }
    size_t onGetTableData(SkFontTableTag, size_t, size_t, void*) const override {
        return 0;
    }
};

struct DecoderProc {
    SkFourByteTag id;
    sk_sp<SkTypeface> (*makeFromStream)(std::unique_ptr<SkStreamAsset>, const SkFontArguments&);
};

std::vector<DecoderProc>* decoders() {
    static SkNoDestructor<std::vector<DecoderProc>> decoders{{
        { SkEmptyTypeface::FactoryId, SkEmptyTypeface::MakeFromStream },
        { SkCustomTypefaceBuilder::FactoryId, SkCustomTypefaceBuilder::MakeFromStream },
#ifdef SK_TYPEFACE_FACTORY_CORETEXT
        { SkTypeface_Mac::FactoryId, SkTypeface_Mac::MakeFromStream },
#endif
#ifdef SK_TYPEFACE_FACTORY_DIRECTWRITE
        { DWriteFontTypeface::FactoryId, DWriteFontTypeface::MakeFromStream },
#endif
#ifdef SK_TYPEFACE_FACTORY_FREETYPE
        { SkTypeface_FreeType::FactoryId, SkTypeface_FreeType::MakeFromStream },
#endif
#ifdef SK_TYPEFACE_FACTORY_FONTATIONS
        { SkTypefaces::Fontations::FactoryId, SkTypeface_Make_Fontations },
#endif
    }};
    return decoders.get();
}

}  // namespace

sk_sp<SkTypeface> SkTypeface::MakeEmpty() {
    return SkEmptyTypeface::Make();
}

bool SkTypeface::Equal(const SkTypeface* facea, const SkTypeface* faceb) {
    if (facea == faceb) {
        return true;
    }
    if (!facea || !faceb) {
        return false;
    }
    return facea->uniqueID() == faceb->uniqueID();
}

sk_sp<SkTypeface> SkTypeface::makeClone(const SkFontArguments& args) const {
    return this->onMakeClone(args);
}

void SkTypeface::Register(
        FactoryId id,
        sk_sp<SkTypeface> (*make)(std::unique_ptr<SkStreamAsset>, const SkFontArguments&)) {
    decoders()->push_back(DecoderProc{id, make});
}

bool SkTypeface::serialize(SkWStream* wstream, SerializeBehavior behavior) const {
    bool isLocalData = false;
    SkFontDescriptor desc;
    this->onGetFontDescriptor(&desc, &isLocalData);
    if (desc.getFactoryId() == 0) {
        SkDEBUGF("Factory was not set for %s.\n", desc.getFamilyName());
    }

    const bool shouldSerializeData = skia_rs_text_should_serialize_typeface_data(
            static_cast<int32_t>(behavior), isLocalData);

    if (shouldSerializeData) {
        int index;
        desc.setStream(this->openStream(&index));
        if (desc.hasStream()) {
            desc.setCollectionIndex(index);
        }

        int numAxes = this->getVariationDesignPosition({});
        if (0 < numAxes) {
            numAxes = this->getVariationDesignPosition({desc.setVariationCoordinates(numAxes),
                                                        static_cast<size_t>(numAxes)});
            if (numAxes <= 0) {
                desc.setVariationCoordinates(0);
            }
        }
    }
    return desc.serialize(wstream);
}

sk_sp<SkData> SkTypeface::serialize(SerializeBehavior behavior) const {
    SkDynamicMemoryWStream stream;
    return this->serialize(&stream, behavior) ? stream.detachAsData() : nullptr;
}

sk_sp<SkTypeface> SkTypeface::MakeDeserialize(SkStream* stream,
                                              sk_sp<SkFontMgr> lastResortMgr,
                                              SkTypefaceStreamSanitizerProc sanitizer) {
    SkFontDescriptor desc;
    if (!SkFontDescriptor::Deserialize(stream, &desc, sanitizer)) {
        return nullptr;
    }

    if (desc.hasStream()) {
        for (const DecoderProc& proc : *decoders()) {
            if (proc.id == desc.getFactoryId()) {
                return proc.makeFromStream(desc.detachStream(), desc.getFontArguments());
            }
        }

        [[maybe_unused]] FactoryId id = desc.getFactoryId();
        SkDEBUGF("Could not find factory %c%c%c%c for %s.\n",
                 (char)((id >> 24) & 0xFF),
                 (char)((id >> 16) & 0xFF),
                 (char)((id >> 8) & 0xFF),
                 (char)((id >> 0) & 0xFF),
                 desc.getFamilyName());

        if (lastResortMgr) {
            sk_sp<SkTypeface> typeface = lastResortMgr->makeFromStream(desc.detachStream(),
                                                                       desc.getFontArguments());
            if (typeface) {
                return typeface;
            }
        }
    }
    if (lastResortMgr) {
        return lastResortMgr->legacyMakeTypeface(desc.getFamilyName(), desc.getStyle());
    }
    return SkEmptyTypeface::Make();
}

bool SkTypeface::glyphMaskNeedsCurrentColor() const {
    return this->onGlyphMaskNeedsCurrentColor();
}

int SkTypeface::getVariationDesignPosition(
        SkSpan<SkFontArguments::VariationPosition::Coordinate> coordinates) const {
    return this->onGetVariationDesignPosition(coordinates);
}

int SkTypeface::getVariationDesignParameters(
        SkSpan<SkFontParameters::Variation::Axis> parameters) const {
    return this->onGetVariationDesignParameters(parameters);
}

int SkTypeface::countTables() const {
    return this->onGetTableTags({});
}

int SkTypeface::readTableTags(SkSpan<SkFontTableTag> tags) const {
    return this->onGetTableTags(tags);
}

size_t SkTypeface::getTableSize(SkFontTableTag tag) const {
    return this->onGetTableData(tag, 0, ~0U, nullptr);
}

size_t SkTypeface::getTableData(SkFontTableTag tag, size_t offset, size_t length,
                                void* data) const {
    return this->onGetTableData(tag, offset, length, data);
}

sk_sp<SkData> SkTypeface::copyTableData(SkFontTableTag tag) const {
    return this->onCopyTableData(tag);
}

sk_sp<SkData> SkTypeface::onCopyTableData(SkFontTableTag tag) const {
    size_t size = this->getTableSize(tag);
    if (size) {
        sk_sp<SkData> data = SkData::MakeUninitialized(size);
        (void)this->getTableData(tag, 0, size, data->writable_data());
        return data;
    }
    return nullptr;
}

std::unique_ptr<SkStreamAsset> SkTypeface::openStream(int* ttcIndex) const {
    int ttcIndexStorage;
    if (nullptr == ttcIndex) {
        ttcIndex = &ttcIndexStorage;
    }
    return this->onOpenStream(ttcIndex);
}

std::unique_ptr<SkStreamAsset> SkTypeface::openExistingStream(int* ttcIndex) const {
    int ttcIndexStorage;
    if (nullptr == ttcIndex) {
        ttcIndex = &ttcIndexStorage;
    }
    return this->onOpenExistingStream(ttcIndex);
}

std::unique_ptr<SkScalerContext> SkTypeface::createScalerContext(
        const SkScalerContextEffects& effects, const SkDescriptor* desc) const {
    std::unique_ptr<SkScalerContext> scalerContext = this->onCreateScalerContext(effects, desc);
    SkASSERT(scalerContext);
    return scalerContext;
}

std::unique_ptr<SkScalerContext> SkTypeface::onCreateScalerContextAsProxyTypeface(
        const SkScalerContextEffects&, const SkDescriptor*, SkTypeface*) const {
    SK_ABORT("Not implemented.");
}

void SkTypeface::unicharsToGlyphs(SkSpan<const SkUnichar> uni, SkSpan<SkGlyphID> glyphs) const {
    if (const size_t n = std::min(uni.size(), glyphs.size())) {
        this->onCharsToGlyphs(uni.first(n), glyphs.first(n));
    }
}

SkGlyphID SkTypeface::unicharToGlyph(SkUnichar uni) const {
    SkGlyphID glyphs[1] = {0};
    this->onCharsToGlyphs({&uni, 1}, glyphs);
    return glyphs[0];
}

size_t SkTypeface::textToGlyphs(const void* text, size_t byteLength, SkTextEncoding encoding,
                                SkSpan<SkGlyphID> glyphs) const {
    if (0 == byteLength) {
        return 0;
    }

    SkASSERT(text);

    size_t count = SkFontPriv::CountTextElements(text, byteLength, encoding);
    if (count > glyphs.size()) {
        return count;
    }

    if (encoding == SkTextEncoding::kGlyphID) {
        memcpy(glyphs.data(), text, count << 1);
        return count;
    }

    if (encoding == SkTextEncoding::kUTF32) {
        this->unicharsToGlyphs({static_cast<const SkUnichar*>(text), count}, glyphs);
        return count;
    }

    AutoSTMalloc<256, SkUnichar> storage(byteLength);
    skia_rs_text_decode_to_utf32(
            static_cast<const uint8_t*>(text), byteLength,
            static_cast<int32_t>(encoding), storage.get(), byteLength);
    this->unicharsToGlyphs({storage.get(), count}, glyphs);
    return count;
}

int SkTypeface::countGlyphs() const {
    return this->onCountGlyphs();
}

int SkTypeface::getUnitsPerEm() const {
    return this->onGetUPEM();
}

bool SkTypeface::getKerningPairAdjustments(SkSpan<const SkGlyphID> glyphs,
                                           SkSpan<int32_t> adjustments) const {
    if (glyphs.size() <= 1 || adjustments.empty()) {
        return this->onGetKerningPairAdjustments({}, {});
    }

    const size_t n = std::min(glyphs.size() - 1, adjustments.size());
    return this->onGetKerningPairAdjustments(glyphs.first(n + 1), adjustments.first(n));
}

SkTypeface::LocalizedStrings* SkTypeface::createFamilyNameIterator() const {
    return this->onCreateFamilyNameIterator();
}

void SkTypeface::getFamilyName(SkString* name) const {
    SkASSERT(name);
    this->onGetFamilyName(name);
}

bool SkTypeface::getPostScriptName(SkString* name) const {
    return this->onGetPostScriptName(name);
}

int SkTypeface::getResourceName(SkString* resourceName) const {
    return this->onGetResourceName(resourceName);
}

int SkTypeface::onGetResourceName(SkString* resourceName) const {
    return 0;
}

SkFontStyle SkTypeface::fontStyle() const {
    return this->onGetFontStyle();
}

SkFontStyle SkTypeface::onGetFontStyle() const {
    return fStyle;
}

bool SkTypeface::isBold() const {
    return this->onGetFontStyle().weight() >= SkFontStyle::kSemiBold_Weight;
}

bool SkTypeface::isItalic() const {
    return this->onGetFontStyle().slant() != SkFontStyle::kUpright_Slant;
}

bool SkTypeface::isFixedPitch() const {
    return this->onGetFixedPitch();
}

bool SkTypeface::onGetFixedPitch() const {
    return fIsFixedPitch;
}

bool SkTypeface::isSyntheticBold() const { return this->onIsSyntheticBold(); }
bool SkTypeface::isSyntheticOblique() const { return this->onIsSyntheticOblique(); }
bool SkTypeface::onIsSyntheticBold() const { return false; }
bool SkTypeface::onIsSyntheticOblique() const { return false; }

void SkTypeface::getGlyphToUnicodeMap(SkSpan<SkUnichar> dst) const {
    sk_bzero(dst.data(), dst.size_bytes());
}

std::unique_ptr<SkAdvancedTypefaceMetrics> SkTypeface::getAdvancedMetrics() const {
    std::unique_ptr<SkAdvancedTypefaceMetrics> result = this->onGetAdvancedMetrics();
    if (result && result->fPostScriptName.isEmpty()) {
        if (!this->getPostScriptName(&result->fPostScriptName)) {
            this->getFamilyName(&result->fPostScriptName);
        }
    }
    if (result && (result->fType == SkAdvancedTypefaceMetrics::kTrueType_Font ||
                   result->fType == SkAdvancedTypefaceMetrics::kCFF_Font)) {
        uint8_t fsTypeRaw[2];
        constexpr SkFontTableTag os2Tag = SkTEndian_SwapBE32(SkOTTableOS2::TAG);
        constexpr size_t fsTypeOffset = offsetof(SkOTTableOS2::Version::V2, fsType);
        if (this->getTableData(os2Tag, fsTypeOffset, sizeof(fsTypeRaw), fsTypeRaw) == sizeof(fsTypeRaw)) {
            uint8_t addFlags = skia_rs_text_os2_fstype_to_advanced_flags(fsTypeRaw[0], fsTypeRaw[1]);
            result->fFlags |= static_cast<SkAdvancedTypefaceMetrics::FontFlags>(addFlags);
        }
    }
    return result;
}

bool SkTypeface::onGetKerningPairAdjustments(SkSpan<const SkGlyphID>, SkSpan<int32_t>) const {
    return false;
}

std::unique_ptr<SkStreamAsset> SkTypeface::onOpenExistingStream(int* ttcIndex) const {
    return this->onOpenStream(ttcIndex);
}

SkRect SkTypeface::getBounds() const {
    fBoundsOnce([this] {
        if (!this->onComputeBounds(&fBounds)) {
            fBounds.setEmpty();
        }
    });
    return fBounds;
}

bool SkTypeface::onComputeBounds(SkRect* bounds) const {
    const SkScalar textSize = 2048;
    const SkScalar invTextSize = 1 / textSize;

    SkFont font;
    font.setTypeface(sk_ref_sp(const_cast<SkTypeface*>(this)));
    font.setSize(textSize);
    font.setLinearMetrics(true);

    SkScalerContextRec rec;
    SkScalerContextEffects effects;

    SkScalerContext::MakeRecAndEffectsFromFont(font, &rec, &effects);

    SkAutoDescriptor ad;
    SkScalerContextEffects noeffects;
    SkScalerContext::AutoDescriptorGivenRecAndEffects(rec, noeffects, &ad);

    std::unique_ptr<SkScalerContext> ctx = this->createScalerContext(noeffects, ad.getDesc());

    SkFontMetrics fm;
    ctx->getFontMetrics(&fm);
    if (!fm.hasBounds()) {
        return false;
    }
    bounds->setLTRB(fm.fXMin * invTextSize, fm.fTop * invTextSize,
                    fm.fXMax * invTextSize, fm.fBottom * invTextSize);
    return true;
}

// ============================================================================
// SkTypefaceCache.cpp
// ============================================================================

SkTypefaceCache::SkTypefaceCache() {}

void SkTypefaceCache::add(sk_sp<SkTypeface> face) {
    SkASSERT_RELEASE(face);
    const auto limit = SkGraphics::GetTypefaceCacheCountLimit();

    const int toPurge = skia_rs_text_typeface_cache_purge_count(fTypefaces.size(), limit);
    if (toPurge > 0) {
        this->purge(toPurge);
    } else if (fTypefaces.size() >= limit) {
        this->purge(limit >> 2);
    }
    if (limit > 0) {
        fTypefaces.emplace_back(std::move(face));
    }
}

sk_sp<SkTypeface> SkTypefaceCache::findByProcAndRef(FindProc proc, void* ctx) const {
    for (const sk_sp<SkTypeface>& typeface : fTypefaces) {
        if (proc(typeface.get(), ctx)) {
            return typeface;
        }
    }
    return nullptr;
}

void SkTypefaceCache::purge(int numToPurge) {
    int count = fTypefaces.size();
    int i = 0;
    while (i < count) {
        if (fTypefaces[i]->unique()) {
            fTypefaces.removeShuffle(i);
            --count;
            if (--numToPurge == 0) {
                return;
            }
        } else {
            ++i;
        }
    }
}

void SkTypefaceCache::purgeAll() {
    this->purge(fTypefaces.size());
}

SkTypefaceCache& SkTypefaceCache::Get() {
    return *SkContexts::GetGlobalSkContext()->priv().sharedState()->typefaceCache();
}

SkTypefaceID SkTypefaceCache::NewTypefaceID() {
    static std::atomic<uint64_t> nextID{1};
    std::optional<SkTypefaceID> id = NextTypefaceID(&nextID);
    SkASSERT_RELEASE(id.has_value());
    return *id;
}

std::optional<SkTypefaceID> SkTypefaceCache::NextTypefaceID(std::atomic<uint64_t>* counter) {
    uint64_t id = counter->fetch_add(1, std::memory_order_relaxed);
    uint32_t outId = 0;
    if (!skia_rs_text_next_typeface_id_checked(id, &outId)) {
        return std::nullopt;
    }
    return static_cast<SkTypefaceID>(outId);
}

static SkMutex& typeface_cache_mutex() {
    static SkMutex& mutex = *(new SkMutex);
    return mutex;
}

void SkTypefaceCache::Add(sk_sp<SkTypeface> face) {
    SkAutoMutexExclusive ama(typeface_cache_mutex());
    Get().add(std::move(face));
}

sk_sp<SkTypeface> SkTypefaceCache::FindByProcAndRef(FindProc proc, void* ctx) {
    SkAutoMutexExclusive ama(typeface_cache_mutex());
    return Get().findByProcAndRef(proc, ctx);
}

void SkTypefaceCache::PurgeAll() {
    SkAutoMutexExclusive ama(typeface_cache_mutex());
    Get().purgeAll();
}

#ifdef SK_DEBUG
static bool DumpProc(SkTypeface* face, void* ctx) {
    SkString n;
    face->getFamilyName(&n);
    SkFontStyle s = face->fontStyle();
    SkTypefaceID id = face->uniqueID();
    SkDebugf("SkTypefaceCache: face %p typefaceID %u weight %d width %d style %d name %s\n",
             face, id, s.weight(), s.width(), s.slant(), n.c_str());
    return false;
}
#endif

void SkTypefaceCache::Dump() {
#ifdef SK_DEBUG
    (void)Get().findByProcAndRef(DumpProc, nullptr);
#endif
}

// ============================================================================
// SkTypeface_remote.cpp
// ============================================================================

SkScalerContextProxy::SkScalerContextProxy(SkTypeface& tf,
                                           const SkScalerContextEffects& effects,
                                           const SkDescriptor* desc,
                                           sk_sp<SkStrikeClient::DiscardableHandleManager> manager)
        : SkScalerContext{tf, effects, desc}
        , fDiscardableManager{std::move(manager)} {}

SkScalerContext::GlyphMetrics SkScalerContextProxy::generateMetrics(const SkGlyph& glyph,
                                                                    SkArenaAlloc*) {
    TRACE_EVENT1("skia", "generateMetrics", "rec", TRACE_STR_COPY(this->getRec().dump().c_str()));
    if (this->getProxyTypeface()->isLogging()) {
        SkDebugf("GlyphCacheMiss generateMetrics looking for glyph: %x\n  generateMetrics: %s\n",
                 glyph.getPackedID().value(), this->getRec().dump().c_str());
    }

    fDiscardableManager->notifyCacheMiss(
            SkStrikeClient::CacheMissType::kGlyphMetrics, fRec.fTextSize);

    return {glyph.maskFormat()};
}

void SkScalerContextProxy::generateImage(const SkGlyph& glyph, void* imageBuffer) {
    TRACE_EVENT1("skia", "generateImage", "rec", TRACE_STR_COPY(this->getRec().dump().c_str()));
    if (this->getProxyTypeface()->isLogging()) {
        SkDebugf("GlyphCacheMiss generateImage: %s\n", this->getRec().dump().c_str());
    }

    fDiscardableManager->notifyCacheMiss(
            SkStrikeClient::CacheMissType::kGlyphImage, fRec.fTextSize);
    sk_bzero(imageBuffer, glyph.imageSize());
}

std::optional<SkScalerContext::GeneratedPath> SkScalerContextProxy::generatePath(const SkGlyph&) {
    TRACE_EVENT1("skia", "generatePath", "rec", TRACE_STR_COPY(this->getRec().dump().c_str()));
    if (this->getProxyTypeface()->isLogging()) {
        SkDebugf("GlyphCacheMiss generatePath: %s\n", this->getRec().dump().c_str());
    }

    fDiscardableManager->notifyCacheMiss(
            SkStrikeClient::CacheMissType::kGlyphPath, fRec.fTextSize);
    return {};
}

sk_sp<SkDrawable> SkScalerContextProxy::generateDrawable(const SkGlyph&) {
    TRACE_EVENT1("skia", "generateDrawable", "rec", TRACE_STR_COPY(this->getRec().dump().c_str()));
    if (this->getProxyTypeface()->isLogging()) {
        SkDebugf("GlyphCacheMiss generateDrawable: %s\n", this->getRec().dump().c_str());
    }

    fDiscardableManager->notifyCacheMiss(
            SkStrikeClient::CacheMissType::kGlyphDrawable, fRec.fTextSize);
    return nullptr;
}

void SkScalerContextProxy::generateFontMetrics(SkFontMetrics* metrics) {
    TRACE_EVENT1(
            "skia", "generateFontMetrics", "rec", TRACE_STR_COPY(this->getRec().dump().c_str()));
    if (this->getProxyTypeface()->isLogging()) {
        SkDebugf("GlyphCacheMiss generateFontMetrics: %s\n", this->getRec().dump().c_str());
    }

    fDiscardableManager->notifyCacheMiss(
            SkStrikeClient::CacheMissType::kFontMetrics, fRec.fTextSize);
    sk_bzero(metrics, sizeof(*metrics));
}

std::optional<SkTypefaceProxyPrototype>
SkTypefaceProxyPrototype::MakeFromBuffer(SkReadBuffer& buffer) {
    SkASSERT(buffer.isValid());
    const SkTypefaceID typefaceID = buffer.readUInt();
    const int glyphCount = buffer.readInt();
    const int32_t styleValue = buffer.read32();
    const bool isFixedPitch = buffer.readBool();
    const bool glyphMaskNeedsCurrentColor = buffer.readBool();

    if (buffer.isValid()) {
        return SkTypefaceProxyPrototype{
            typefaceID, glyphCount, styleValue, isFixedPitch, glyphMaskNeedsCurrentColor};
    }

    return std::nullopt;
}

SkTypefaceProxyPrototype::SkTypefaceProxyPrototype(const SkTypeface& typeface)
        : fServerTypefaceID{typeface.uniqueID()}
        , fGlyphCount{typeface.countGlyphs()}
        , fStyleValue{typeface.fontStyle().fValue}
        , fIsFixedPitch{typeface.isFixedPitch()}
        , fGlyphMaskNeedsCurrentColor{typeface.glyphMaskNeedsCurrentColor()} {}

SkTypefaceProxyPrototype::SkTypefaceProxyPrototype(SkTypefaceID typefaceID, int glyphCount,
                                                   int32_t styleValue, bool isFixedPitch,
                                                   bool glyphMaskNeedsCurrentColor)
        : fServerTypefaceID{typefaceID}
        , fGlyphCount{glyphCount}
        , fStyleValue{styleValue}
        , fIsFixedPitch{isFixedPitch}
        , fGlyphMaskNeedsCurrentColor{glyphMaskNeedsCurrentColor} {}

void SkTypefaceProxyPrototype::flatten(SkWriteBuffer& buffer) const {
    buffer.writeUInt(fServerTypefaceID);
    buffer.writeInt(fGlyphCount);
    buffer.write32(fStyleValue);
    buffer.writeBool(fIsFixedPitch);
    buffer.writeBool(fGlyphMaskNeedsCurrentColor);
}

SkTypefaceProxy::SkTypefaceProxy(const SkTypefaceProxyPrototype& prototype,
                                 sk_sp<SkStrikeClient::DiscardableHandleManager> manager,
                                 bool isLogging)
        : SkTypeface{prototype.style(), prototype.fIsFixedPitch}
        , fTypefaceID{prototype.fServerTypefaceID}
        , fGlyphCount{prototype.fGlyphCount}
        , fIsLogging{isLogging}
        , fGlyphMaskNeedsCurrentColor{prototype.fGlyphMaskNeedsCurrentColor}
        , fDiscardableManager{std::move(manager)} {}

SkTypefaceProxy::SkTypefaceProxy(SkTypefaceID typefaceID,
                                 int glyphCount,
                                 const SkFontStyle& style,
                                 bool isFixedPitch,
                                 bool glyphMaskNeedsCurrentColor,
                                 sk_sp<SkStrikeClient::DiscardableHandleManager> manager,
                                 bool isLogging)
        : SkTypeface{style, isFixedPitch}
        , fTypefaceID{typefaceID}
        , fGlyphCount{glyphCount}
        , fIsLogging{isLogging}
        , fGlyphMaskNeedsCurrentColor(glyphMaskNeedsCurrentColor)
        , fDiscardableManager{std::move(manager)} {}

SkTypefaceProxy* SkScalerContextProxy::getProxyTypeface() const {
    return (SkTypefaceProxy*)this->getTypeface();
}

// ============================================================================
// SkFontMgr.cpp
// ============================================================================

class SkEmptyFontStyleSet : public SkFontStyleSet {
public:
    int count() override { return 0; }
    void getStyle(int, SkFontStyle*, SkString*) override {
        SkDEBUGFAIL("SkFontStyleSet::getStyle called on empty set");
    }
    sk_sp<SkTypeface> createTypeface(int index) override {
        SkDEBUGFAIL("SkFontStyleSet::createTypeface called on empty set");
        return nullptr;
    }
    sk_sp<SkTypeface> matchStyle(const SkFontStyle&) override {
        return nullptr;
    }
};

sk_sp<SkFontStyleSet> SkFontStyleSet::CreateEmpty() {
    return sk_sp<SkFontStyleSet>(new SkEmptyFontStyleSet);
}

class SkEmptyFontMgr : public SkFontMgr {
protected:
    int onCountFamilies() const override { return 0; }
    void onGetFamilyName(int index, SkString* familyName) const override {
        SkDEBUGFAIL("onGetFamilyName called with bad index");
    }
    sk_sp<SkFontStyleSet> onCreateStyleSet(int index) const override {
        SkDEBUGFAIL("onCreateStyleSet called with bad index");
        return nullptr;
    }
    sk_sp<SkFontStyleSet> onMatchFamily(const char[]) const override {
        return SkFontStyleSet::CreateEmpty();
    }
    sk_sp<SkTypeface> onMatchFamilyStyle(const char[], const SkFontStyle&) const override {
        return nullptr;
    }
    sk_sp<SkTypeface> onMatchFamilyStyleCharacter(const char[], const SkFontStyle&,
                                                  const char*[], int, SkUnichar) const override {
        return nullptr;
    }
    sk_sp<SkTypeface> onMakeFromData(sk_sp<SkData>, int) const override { return nullptr; }
    sk_sp<SkTypeface> onMakeFromStreamIndex(std::unique_ptr<SkStreamAsset>, int) const override {
        return nullptr;
    }
    sk_sp<SkTypeface> onMakeFromStreamArgs(std::unique_ptr<SkStreamAsset>,
                                           const SkFontArguments&) const override {
        return nullptr;
    }
    sk_sp<SkTypeface> onMakeFromFile(const char[], int) const override { return nullptr; }
    sk_sp<SkTypeface> onLegacyMakeTypeface(const char[], SkFontStyle) const override {
        return nullptr;
    }
};

static sk_sp<SkFontStyleSet> emptyOnNull(sk_sp<SkFontStyleSet>&& fsset) {
    if (!fsset) {
        fsset = SkFontStyleSet::CreateEmpty();
    }
    return std::move(fsset);
}

int SkFontMgr::countFamilies() const { return this->onCountFamilies(); }
void SkFontMgr::getFamilyName(int index, SkString* familyName) const {
    this->onGetFamilyName(index, familyName);
}
sk_sp<SkFontStyleSet> SkFontMgr::createStyleSet(int index) const {
    return emptyOnNull(this->onCreateStyleSet(index));
}
sk_sp<SkFontStyleSet> SkFontMgr::matchFamily(const char familyName[]) const {
    return emptyOnNull(this->onMatchFamily(familyName));
}
sk_sp<SkTypeface> SkFontMgr::matchFamilyStyle(const char familyName[],
                                              const SkFontStyle& fs) const {
    return this->onMatchFamilyStyle(familyName, fs);
}
sk_sp<SkTypeface> SkFontMgr::matchFamilyStyleCharacter(const char familyName[],
                                                       const SkFontStyle& style,
                                                       const char* bcp47[], int bcp47Count,
                                                       SkUnichar character) const {
    return this->onMatchFamilyStyleCharacter(familyName, style, bcp47, bcp47Count, character);
}
sk_sp<SkTypeface> SkFontMgr::match(const Request& request) const {
    return this->onMatch(request);
}
sk_sp<SkTypeface> SkFontMgr::fallback(const Request& request) const {
    return this->onFallback(request);
}

SkFontStyle SkFontMgr::Request::fontStyleFromModel() const {
    RsFontStyle out;
    skia_rs_text_font_style_from_model(model.data(), model.size(), &out);
    return SkFontStyle(out.weight,
                       static_cast<SkFontStyle::Width>(out.width),
                       static_cast<SkFontStyle::Slant>(out.slant));
}

void SkFontMgr::Request::SetModel(SkFontStyle fontStyle,
                                  SkFontArguments::VariationPosition::Coordinate(&model)[4]) {
    skia_rs_text_set_model_from_style(
            fontStyle.weight(), fontStyle.width(), static_cast<uint8_t>(fontStyle.slant()), model);
}

sk_sp<SkTypeface> SkFontMgr::onMatch(const Request& request) const {
    SkFontStyle style = request.fontStyleFromModel();
    return this->matchFamilyStyle(request.familyName, style);
}

sk_sp<SkTypeface> SkFontMgr::onFallback(const Request& request) const {
    SkFontStyle style = request.fontStyleFromModel();
    SkUnichar character = 0x20;
    if (!request.cmapEntries.empty()) {
        character = request.cmapEntries[0].character;
    }
    return this->matchFamilyStyleCharacter(request.familyName, style,
                                           request.bcp47.data(), request.bcp47.size(), character);
}

sk_sp<SkTypeface> SkFontMgr::makeFromData(sk_sp<SkData> data, int ttcIndex) const {
    if (nullptr == data) { return nullptr; }
    return this->onMakeFromData(std::move(data), ttcIndex);
}

sk_sp<SkTypeface> SkFontMgr::makeFromStream(std::unique_ptr<SkStreamAsset> stream,
                                            int ttcIndex) const {
    if (nullptr == stream) { return nullptr; }
    return this->onMakeFromStreamIndex(std::move(stream), ttcIndex);
}

sk_sp<SkTypeface> SkFontMgr::makeFromStream(std::unique_ptr<SkStreamAsset> stream,
                                            const SkFontArguments& args) const {
    if (nullptr == stream) { return nullptr; }
    return this->onMakeFromStreamArgs(std::move(stream), args);
}

sk_sp<SkTypeface> SkFontMgr::makeFromFile(const char path[], int ttcIndex) const {
    if (nullptr == path) { return nullptr; }
    return this->onMakeFromFile(path, ttcIndex);
}

sk_sp<SkTypeface> SkFontMgr::legacyMakeTypeface(const char familyName[], SkFontStyle style) const {
    return this->onLegacyMakeTypeface(familyName, style);
}

sk_sp<SkFontMgr> SkFontMgr::RefEmpty() {
    static SkFontMgr* singleton = new SkEmptyFontMgr();
    return sk_ref_sp(singleton);
}

sk_sp<SkTypeface> SkFontStyleSet::matchStyleCSS3(const SkFontStyle& pattern) {
    int count = this->count();
    if (0 == count) {
        return nullptr;
    }

    AutoSTArray<16, RsFontStyle> candidates(count);
    for (int i = 0; i < count; ++i) {
        SkFontStyle current;
        this->getStyle(i, &current, nullptr);
        candidates[i] = RsFontStyle{
            current.weight(), current.width(), static_cast<uint8_t>(current.slant())
        };
    }

    size_t bestIdx = skia_rs_text_css3_match_best_index(
            pattern.weight(), pattern.width(), static_cast<uint8_t>(pattern.slant()),
            candidates.data(), static_cast<size_t>(count));
    return this->createTypeface(static_cast<int>(bestIdx));
}
