/*
 * Safe Rust bridge for:
 *   src/core/SkImageFilter.cpp
 *   src/core/SkImageFilterCache.cpp
 *   src/core/SkLocalMatrixImageFilter.cpp
 * (Phase 1 Layer 4.4 skia-rs-imagefilters, GN target :core).
 */

#include "include/core/SkColorFilter.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkM44.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkTypes.h"
#include "include/private/SkMutex.h"
#include "include/private/SkOnce.h"
#include "include/private/SkTArray.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "src/core/SkImageFilterCache.h"
#include "src/core/SkImageFilterTypes.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkLocalMatrixImageFilter.h"
#include "src/core/SkPicturePriv.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkRectPriv.h"
#include "src/core/SkSpecialImage.h"
#include "src/core/SkTHash.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/colorfilters/SkColorFilterBase.h"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkImageFilterCacheKey) == 88);

namespace {

struct SkiaRsIfCache;

enum class RsLegacyCropResult : int32_t {
    kInvalid = 0,
    kNoCrop = 1,
    kHasCrop = 2,
};

enum class RsLocalMatrixAction : int32_t {
    kReject = 0,
    kReturnInput = 1,
    kCreate = 2,
};

struct RsLocalMatrixDecision {
    RsLocalMatrixAction action;
    SkMatrix invLocal;
};

using RsCacheEvictFn = void (*)(void*, uint64_t);

}  // namespace

extern "C" {
uint64_t skia_rs_if_next_unique_id();
bool skia_rs_if_validate_common_input_count(int32_t count, int32_t expected_count);
RsLegacyCropResult skia_rs_if_validate_legacy_crop(SkRect rect, uint32_t flags);
SkIRect skia_rs_if_child_input_layer_bounds_null(
        SkIRect desired_output, bool has_content_bounds, SkIRect content_bounds);
int32_t skia_rs_if_combine_ctm_capability(int32_t a, int32_t b);
void skia_rs_if_validate_local_matrix(
        const SkMatrix* local_matrix, int32_t input_capability, RsLocalMatrixDecision* out);
SkRect skia_rs_if_local_matrix_map_rect(const SkMatrix* m, SkRect r);

SkiaRsIfCache* skia_rs_if_cache_create(size_t max_bytes);
void skia_rs_if_cache_destroy(SkiaRsIfCache* cache);
int32_t skia_rs_if_cache_count(const SkiaRsIfCache* cache);
bool skia_rs_if_cache_get(
        SkiaRsIfCache* cache, const SkImageFilterCacheKey* key, uint64_t* out_handle);
uint64_t skia_rs_if_cache_set(
        SkiaRsIfCache* cache,
        const SkImageFilterCacheKey* key,
        size_t filter_tag,
        size_t bytes,
        RsCacheEvictFn evict_cb,
        void* user_data);
void skia_rs_if_cache_purge(SkiaRsIfCache* cache, RsCacheEvictFn evict_cb, void* user_data);
void skia_rs_if_cache_purge_by_filter(
        SkiaRsIfCache* cache, size_t filter_tag, RsCacheEvictFn evict_cb, void* user_data);
}  // extern "C"

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkImageFilter & SkImageFilter_Base
///////////////////////////////////////////////////////////////////////////////////////////////////

int SkImageFilter::countInputs() const { return as_IFB(this)->fInputs.count(); }

const SkImageFilter* SkImageFilter::getInput(int i) const {
    SkASSERT(i < this->countInputs());
    return as_IFB(this)->fInputs[i].get();
}

bool SkImageFilter::isColorFilterNode(SkColorFilter** filterPtr) const {
    return as_IFB(this)->onIsColorFilterNode(filterPtr);
}

SkIRect SkImageFilter::filterBounds(const SkIRect& src, const SkMatrix& ctm,
                                    MapDirection direction, const SkIRect* inputRect) const {
    skif::Mapping mapping{SkM44(ctm)};
    if (kReverse_MapDirection == direction) {
        skif::LayerSpace<SkIRect> targetOutput(src);
        std::optional<skif::LayerSpace<SkIRect>> content;
        if (inputRect) {
            content = skif::LayerSpace<SkIRect>(*inputRect);
        }
        return SkIRect(as_IFB(this)->onGetInputLayerBounds(mapping, targetOutput, content));
    } else {
        SkASSERT(!inputRect);
        skif::LayerSpace<SkIRect> content(src);
        auto output = as_IFB(this)->onGetOutputLayerBounds(mapping, content);
        return output ? SkIRect(*output) : SkRectPriv::MakeILarge();
    }
}

SkRect SkImageFilter::computeFastBounds(const SkRect& src) const {
    if (0 == this->countInputs()) {
        return src;
    }
    SkRect combinedBounds = this->getInput(0) ? this->getInput(0)->computeFastBounds(src) : src;
    for (int i = 1; i < this->countInputs(); i++) {
        const SkImageFilter* input = this->getInput(i);
        if (input) {
            combinedBounds.join(input->computeFastBounds(src));
        } else {
            combinedBounds.join(src);
        }
    }
    return combinedBounds;
}

bool SkImageFilter::canComputeFastBounds() const {
    return !as_IFB(this)->affectsTransparentBlack();
}

bool SkImageFilter_Base::affectsTransparentBlack() const {
    if (this->onAffectsTransparentBlack()) {
        return true;
    } else if (this->ignoreInputsAffectsTransparentBlack()) {
        return false;
    }
    for (int i = 0; i < this->countInputs(); i++) {
        const SkImageFilter* input = this->getInput(i);
        if (input && as_IFB(input)->affectsTransparentBlack()) {
            return true;
        }
    }
    return false;
}

bool SkImageFilter::asAColorFilter(SkColorFilter** filterPtr) const {
    SkASSERT(nullptr != filterPtr);
    if (!this->isColorFilterNode(filterPtr)) {
        return false;
    }
    if (nullptr != this->getInput(0) || as_CFB(*filterPtr)->affectsTransparentBlack()) {
        (*filterPtr)->unref();
        return false;
    }
    return true;
}

sk_sp<SkImageFilter> SkImageFilter::makeWithLocalMatrix(const SkMatrix& matrix) const {
    return SkLocalMatrixImageFilter::Make(matrix, this->refMe());
}

SkImageFilter_Base::SkImageFilter_Base(SkSpan<const sk_sp<SkImageFilter>> inputs,
                                       std::optional<bool> usesSrc)
        : fUsesSrcInput(usesSrc.has_value() ? *usesSrc : false)
        , fUniqueID(skia_rs_if_next_unique_id()) {
    const int inputCount = SkToInt(inputs.size());
    fInputs.reset(inputCount);

    for (int i = 0; i < inputCount; ++i) {
        if (!usesSrc.has_value() && (!inputs[i] || as_IFB(inputs[i])->usesSource())) {
            fUsesSrcInput = true;
        }
        fInputs[i] = inputs[i];
    }
}

SkImageFilter_Base::SkImageFilter_Base(sk_sp<SkImageFilter> const* inputs,
                                       int inputCount,
                                       std::optional<bool> usesSrc)
        : SkImageFilter_Base(
                  SK_UNSAFE_TODO(SkSpan<const sk_sp<SkImageFilter>>(inputs, SkToSizeT(inputCount))),
                  usesSrc) {}

SkImageFilter_Base::~SkImageFilter_Base() {
    SkImageFilterCache::Get()->purgeByImageFilter(this);
}

std::pair<sk_sp<SkImageFilter>, std::optional<SkRect>>
SkImageFilter_Base::Unflatten(SkReadBuffer& buffer) {
    Common common;
    if (!common.unflatten(buffer, 1)) {
        return {nullptr, std::nullopt};
    } else {
        return {common.getInput(0), common.cropRect()};
    }
}

bool SkImageFilter_Base::Common::unflatten(SkReadBuffer& buffer, int expectedCount) {
    const int count = buffer.readInt();
    if (!buffer.validate(skia_rs_if_validate_common_input_count(count, expectedCount))) {
        return false;
    }

#if defined(SK_BUILD_FOR_FUZZER)
    if (count > 4) {
        return false;
    }
#endif

    SkASSERT(fInputs.empty());
    for (int i = 0; i < count; i++) {
        fInputs.push_back(buffer.readBool() ? buffer.readImageFilter() : nullptr);
        if (!buffer.isValid()) {
            return false;
        }
    }

    if (buffer.isVersionLT(SkPicturePriv::kRemoveDeprecatedCropRect)) {
        SkRect rect;
        buffer.readRect(&rect);
        if (!buffer.isValid()) {
            return false;
        }
        uint32_t flags = buffer.readUInt();
        if (!buffer.isValid()) {
            return false;
        }
        RsLegacyCropResult res = skia_rs_if_validate_legacy_crop(rect, flags);
        if (!buffer.validate(res != RsLegacyCropResult::kInvalid)) {
            return false;
        }
        if (res == RsLegacyCropResult::kHasCrop) {
            fCropRect = rect;
        }
    }
    return buffer.isValid();
}

void SkImageFilter_Base::flatten(SkWriteBuffer& buffer) const {
    buffer.writeInt(fInputs.count());
    for (int i = 0; i < fInputs.count(); i++) {
        const SkImageFilter* input = this->getInput(i);
        buffer.writeBool(input != nullptr);
        if (input != nullptr) {
            buffer.writeFlattenable(input);
        }
    }
}

skif::FilterResult SkImageFilter_Base::filterImage(const skif::Context& context) const {
    context.markVisitedImageFilter();

    skif::FilterResult result;
    if (context.desiredOutput().isEmpty() || !context.mapping().layerMatrix().isFinite()) {
        return result;
    }

    const bool srcInKey = fUsesSrcInput && context.source();
    uint32_t srcGenID = srcInKey ? context.source().image()->uniqueID() : SK_InvalidUniqueID;
    const SkIRect srcSubset = srcInKey ? context.source().image()->subset() : SkIRect::MakeWH(0, 0);

    SkImageFilterCacheKey key(fUniqueID,
                              context.mapping().layerMatrix().asM33(),
                              SkIRect(context.desiredOutput()),
                              srcGenID, srcSubset);
    if (context.backend()->cache() && context.backend()->cache()->get(key, &result)) {
        context.markCacheHit();
        return result;
    }

    result = this->onFilterImage(context);

    if (context.backend()->cache()) {
        context.backend()->cache()->set(key, this, result);
    }

    return result;
}

sk_sp<SkImage> SkImageFilter_Base::makeImageWithFilter(sk_sp<skif::Backend> backend,
                                                       sk_sp<SkImage> src,
                                                       const SkIRect& subset,
                                                       const SkIRect& clipBounds,
                                                       SkIRect* outSubset,
                                                       SkIPoint* offset) const {
    if (!outSubset || !offset || !src->bounds().contains(subset)) {
        return nullptr;
    }

    auto srcSpecialImage = backend->makeImage(subset, src);
    if (!srcSpecialImage) {
        return nullptr;
    }

    skif::Stats stats;
    const skif::Context context{std::move(backend),
                                skif::Mapping(SkM44()),
                                skif::LayerSpace<SkIRect>(clipBounds),
                                skif::FilterResult(std::move(srcSpecialImage),
                                                   skif::LayerSpace<SkIPoint>(subset.topLeft())),
                                src->imageInfo().colorSpace(),
                                &stats};

    sk_sp<SkSpecialImage> result = this->filterImage(context).imageAndOffset(context, offset);
    stats.reportStats();

    if (!result) {
        return nullptr;
    }

    SkASSERT(clipBounds.contains(SkIRect::MakeXYWH(offset->fX, offset->fY,
                                                   result->width(), result->height())));
    *outSubset = result->subset();
    return result->asImage();
}

skif::LayerSpace<SkIRect> SkImageFilter_Base::getInputBounds(
        const skif::Mapping& mapping,
        const skif::DeviceSpace<SkIRect>& desiredOutput,
        std::optional<skif::ParameterSpace<SkRect>> knownContentBounds) const {
    skif::LayerSpace<SkIRect> desiredBounds = mapping.deviceToLayer(desiredOutput);

    std::optional<skif::LayerSpace<SkIRect>> contentBounds;
    if (knownContentBounds) {
        contentBounds = mapping.paramToLayer(*knownContentBounds).roundOut();
    }

    return this->onGetInputLayerBounds(mapping, desiredBounds, contentBounds);
}

std::optional<skif::DeviceSpace<SkIRect>> SkImageFilter_Base::getOutputBounds(
        const skif::Mapping& mapping,
        const skif::ParameterSpace<SkRect>& contentBounds) const {
    skif::LayerSpace<SkRect> layerContent = mapping.paramToLayer(contentBounds);
    std::optional<skif::LayerSpace<SkIRect>> filterOutput =
            this->onGetOutputLayerBounds(mapping, layerContent.roundOut());
    if (filterOutput) {
        return mapping.layerToDevice(*filterOutput);
    } else {
        return {};
    }
}

SkImageFilter_Base::MatrixCapability SkImageFilter_Base::getCTMCapability() const {
    MatrixCapability result = this->onGetCTMCapability();
    const int count = this->countInputs();
    for (int i = 0; i < count; ++i) {
        if (const SkImageFilter_Base* input = as_IFB(this->getInput(i))) {
            result = static_cast<MatrixCapability>(skia_rs_if_combine_ctm_capability(
                    static_cast<int32_t>(result),
                    static_cast<int32_t>(input->getCTMCapability())));
        }
    }
    return result;
}

skif::LayerSpace<SkIRect> SkImageFilter_Base::getChildInputLayerBounds(
        int index,
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    const SkImageFilter* childFilter = this->getInput(index);
    if (childFilter) {
        return as_IFB(childFilter)->onGetInputLayerBounds(mapping, desiredOutput, contentBounds);
    } else {
        SkIRect cb = contentBounds ? SkIRect(*contentBounds) : SkIRect::MakeEmpty();
        return skif::LayerSpace<SkIRect>(skia_rs_if_child_input_layer_bounds_null(
                SkIRect(desiredOutput), contentBounds.has_value(), cb));
    }
}

std::optional<skif::LayerSpace<SkIRect>> SkImageFilter_Base::getChildOutputLayerBounds(
        int index,
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    const SkImageFilter* childFilter = this->getInput(index);
    return childFilter ? as_IFB(childFilter)->onGetOutputLayerBounds(mapping, contentBounds)
                       : contentBounds;
}

skif::FilterResult SkImageFilter_Base::getChildOutput(int index, const skif::Context& ctx) const {
    const SkImageFilter* input = this->getInput(index);
    return input ? as_IFB(input)->filterImage(ctx) : ctx.source();
}

void SkImageFilter_Base::PurgeCache() {
    auto cache = SkImageFilterCache::Get(SkImageFilterCache::CreateIfNecessary::kNo);
    if (cache) {
        cache->purge();
    }
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkImageFilterCache (backed by safe Rust LRU index)
///////////////////////////////////////////////////////////////////////////////////////////////////

namespace {

#ifdef SK_BUILD_FOR_IOS
enum { kDefaultCacheSize = 2 * 1024 * 1024 };
#else
enum { kDefaultCacheSize = 128 * 1024 * 1024 };
#endif

class CacheImpl final : public SkImageFilterCache {
public:
    explicit CacheImpl(size_t maxBytes) : fIndex(skia_rs_if_cache_create(maxBytes)) {}

    ~CacheImpl() override {
        skia_rs_if_cache_destroy(fIndex);
        fValues.reset();
    }

    bool get(const SkImageFilterCacheKey& key, skif::FilterResult* result) const override {
        SkASSERT(result);
        SkAutoMutexExclusive mutex(fMutex);
        uint64_t handle = 0;
        if (skia_rs_if_cache_get(fIndex, &key, &handle)) {
            if (const skif::FilterResult* found = fValues.find(handle)) {
                *result = *found;
                return true;
            }
        }
        return false;
    }

    void set(const SkImageFilterCacheKey& key, const SkImageFilter* filter,
             const skif::FilterResult& result) override {
        SkAutoMutexExclusive mutex(fMutex);
        const size_t bytes = result.image() ? result.image()->getSize() : 0;
        const size_t filterTag = reinterpret_cast<size_t>(filter);
        uint64_t handle = skia_rs_if_cache_set(
                fIndex, &key, filterTag, bytes, &CacheImpl::OnEvict, this);
        fValues.set(handle, result);
    }

    void purge() override {
        SkAutoMutexExclusive mutex(fMutex);
        skia_rs_if_cache_purge(fIndex, &CacheImpl::OnEvict, this);
    }

    void purgeByImageFilter(const SkImageFilter* filter) override {
        SkAutoMutexExclusive mutex(fMutex);
        skia_rs_if_cache_purge_by_filter(
                fIndex, reinterpret_cast<size_t>(filter), &CacheImpl::OnEvict, this);
    }

    SkDEBUGCODE(int count() const override { return skia_rs_if_cache_count(fIndex); })

private:
    static void OnEvict(void* userData, uint64_t handle) {
        auto* self = static_cast<CacheImpl*>(userData);
        self->fValues.remove(handle);
    }

    mutable SkiaRsIfCache* fIndex;
    skia_private::THashMap<uint64_t, skif::FilterResult> fValues;
    mutable SkMutex fMutex;
};

}  // namespace

sk_sp<SkImageFilterCache> SkImageFilterCache::Create(size_t maxBytes) {
    return sk_make_sp<CacheImpl>(maxBytes);
}

sk_sp<SkImageFilterCache> SkImageFilterCache::Get(CreateIfNecessary createIfNecessary) {
    static SkOnce once;
    static SkImageFilterCache* cache = nullptr;

    if (createIfNecessary == CreateIfNecessary::kNo) {
        return sk_ref_sp(cache);
    }
    once([] { cache = SkImageFilterCache::Create(kDefaultCacheSize).release(); });
    return sk_ref_sp(cache);
}

///////////////////////////////////////////////////////////////////////////////////////////////////
// SkLocalMatrixImageFilter
///////////////////////////////////////////////////////////////////////////////////////////////////

sk_sp<SkImageFilter> SkLocalMatrixImageFilter::Make(const SkMatrix& localMatrix,
                                                    sk_sp<SkImageFilter> input) {
    if (!input) {
        return nullptr;
    }

    MatrixCapability inputCapability = as_IFB(input)->getCTMCapability();
    RsLocalMatrixDecision decision;
    skia_rs_if_validate_local_matrix(&localMatrix, static_cast<int32_t>(inputCapability), &decision);
    switch (decision.action) {
        case RsLocalMatrixAction::kReturnInput:
            return input;
        case RsLocalMatrixAction::kCreate:
            return sk_sp<SkImageFilter>(
                    new SkLocalMatrixImageFilter(localMatrix, decision.invLocal, &input));
        case RsLocalMatrixAction::kReject:
        default:
            return nullptr;
    }
}

sk_sp<SkFlattenable> SkLocalMatrixImageFilter::CreateProc(SkReadBuffer& buffer) {
    SK_IMAGEFILTER_UNFLATTEN_COMMON(common, 1);
    SkMatrix lm;
    buffer.readMatrix(&lm);
    return SkLocalMatrixImageFilter::Make(lm, common.getInput(0));
}

void SkLocalMatrixImageFilter::flatten(SkWriteBuffer& buffer) const {
    this->SkImageFilter_Base::flatten(buffer);
    buffer.writeMatrix(fLocalMatrix);
}

skif::Mapping SkLocalMatrixImageFilter::localMapping(const skif::Mapping& mapping) const {
    skif::Mapping localMapping = mapping;
    localMapping.concatLocal(fLocalMatrix);
    return localMapping;
}

skif::FilterResult SkLocalMatrixImageFilter::onFilterImage(const skif::Context& ctx) const {
    skif::Mapping localMapping = this->localMapping(ctx.mapping());
    return this->getChildOutput(0, ctx.withNewMapping(localMapping));
}

skif::LayerSpace<SkIRect> SkLocalMatrixImageFilter::onGetInputLayerBounds(
        const skif::Mapping& mapping,
        const skif::LayerSpace<SkIRect>& desiredOutput,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    return this->getChildInputLayerBounds(0, this->localMapping(mapping),
                                          desiredOutput, contentBounds);
}

std::optional<skif::LayerSpace<SkIRect>> SkLocalMatrixImageFilter::onGetOutputLayerBounds(
        const skif::Mapping& mapping,
        std::optional<skif::LayerSpace<SkIRect>> contentBounds) const {
    return this->getChildOutputLayerBounds(0, this->localMapping(mapping), contentBounds);
}

SkRect SkLocalMatrixImageFilter::computeFastBounds(const SkRect& bounds) const {
    SkRect localBounds = skia_rs_if_local_matrix_map_rect(&fInvLocalMatrix, bounds);
    return skia_rs_if_local_matrix_map_rect(
            &fLocalMatrix, this->getInput(0)->computeFastBounds(localBounds));
}
