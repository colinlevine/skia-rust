/*
 * Safe Rust bridge for:
 *   src/core/SkPixelRef.cpp
 *   src/core/SkMallocPixelRef.cpp
 *   src/core/SkBitmapCache.cpp
 *   src/core/SkBitmap.cpp
 * (Layer 3.4a skia-rs-bitmap).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkColorSpace.h"  // IWYU pragma: keep
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkFourByteTag.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMallocPixelRef.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixelRef.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkShader.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkMutex.h"
#include "include/private/SkPixelStorage.h"
#include "include/private/SkTFitsIn.h"
#include "include/private/SkTemplates.h"
#include "include/private/SkTo.h"
#include "include/private/chromium/SkDiscardableMemory.h"
#include "src/core/SkBitmapCache.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkNextID.h"
#include "src/core/SkPixelRefPriv.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkWritePixelsRec.h"
#include "src/image/SkImage_Base.h"
#include "src/image/SkImage_Raster.h"
#include "src/partition_alloc/raw_ptr.h"
#include "src/partition_alloc/raw_ptr_exclusion.h"
#include "src/shaders/SkImageShader.h"

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <utility>

class SkMaskFilter;

namespace {

struct SkiaRsMallocAllocPlan {
    size_t row_bytes;
    size_t byte_size;
};

struct SkiaRsBitmapSetInfoResult {
    int32_t alpha_type;
    size_t row_bytes;
};

struct SkiaRsBitmapExtractSubsetResult {
    SkIRect clipped;
    SkIPoint new_origin;
};

struct SkiaRsExtractAlphaMaskSetup {
    SkIRect bounds;
    uint32_t row_bytes;
};

static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkIPoint) == 8);
static_assert(sizeof(SkiaRsMallocAllocPlan) == 2 * sizeof(size_t));
static_assert(sizeof(SkiaRsBitmapExtractSubsetResult) == 24);
static_assert(sizeof(SkiaRsExtractAlphaMaskSetup) == 20);

}  // namespace

extern "C" {
uint32_t skia_rs_next_image_id();
uint64_t skia_rs_make_resource_cache_shared_id_for_bitmap(uint32_t bitmap_gen_id);
bool skia_rs_validate_malloc_allocate(int32_t width,
                                      int32_t height,
                                      int32_t ct,
                                      int32_t at,
                                      size_t row_bytes,
                                      bool fuzzer_limit,
                                      SkiaRsMallocAllocPlan* out);
bool skia_rs_validate_make_with_data(int32_t width,
                                     int32_t height,
                                     int32_t ct,
                                     int32_t at,
                                     size_t row_bytes,
                                     size_t data_size);
bool skia_rs_validate_bitmap_cache_alloc(int32_t width,
                                         int32_t height,
                                         int32_t ct,
                                         SkiaRsMallocAllocPlan* out);

bool skia_rs_bitmap_validate_set_info(int32_t width,
                                      int32_t height,
                                      int32_t ct,
                                      int32_t at,
                                      size_t row_bytes,
                                      SkiaRsBitmapSetInfoResult* out);
SkIPoint skia_rs_bitmap_pixel_ref_origin(size_t byte_offset, size_t row_bytes, int32_t ct);
size_t skia_rs_bitmap_addr_offset(int32_t x, int32_t y, size_t row_bytes, int32_t ct);
size_t skia_rs_bitmap_pixel_ref_byte_offset(int32_t dx, int32_t dy, size_t row_bytes, int32_t ct);
bool skia_rs_bitmap_extract_subset(int32_t width,
                                   int32_t height,
                                   SkIPoint origin,
                                   const SkIRect* subset,
                                   SkiaRsBitmapExtractSubsetResult* out);
bool skia_rs_bitmap_extract_alpha_setup(int32_t width,
                                        int32_t height,
                                        SkiaRsExtractAlphaMaskSetup* out);
void skia_rs_bitmap_zero_alpha_buffer(uint8_t* alpha,
                                      size_t width,
                                      size_t height,
                                      size_t alpha_row_bytes);
}  // extern "C"

// ============================================================================
// SkPixelRef.cpp
// ============================================================================

uint32_t SkNextID::ImageID() {
    return skia_rs_next_image_id();
}

SkPixelRef::SkPixelRef(int width, int height, void* pixels, size_t rowBytes)
    : SkPixelStorage(SkPixelStorage::Type::kPixelRef)
    , fWidth(width)
    , fHeight(height)
    , fPixels(pixels)
    , fRowBytes(rowBytes)
    , fAddedToCache(false) {
    this->needsNewGenID();
    fMutability = kMutable;
}

SkPixelRef::~SkPixelRef() {
    this->callGenIDChangeListeners();
}

void SkPixelRef::android_only_reset(int width, int height, size_t rowBytes) {
    fWidth = width;
    fHeight = height;
    fRowBytes = rowBytes;
    this->notifyPixelsChanged();
}

void SkPixelRef::needsNewGenID() {
    fTaggedGenID.store(0);
    SkASSERT(!this->genIDIsUnique());
}

uint32_t SkPixelRef::getGenerationID() const {
    uint32_t id = fTaggedGenID.load();
    if (0 == id) {
        uint32_t next = SkNextID::ImageID() | 1u;
        if (fTaggedGenID.compare_exchange_strong(id, next)) {
            id = next;
        }
    }
    return id & ~1u;
}

void SkPixelRef::addGenIDChangeListener(sk_sp<SkIDChangeListener> listener) {
    if (!listener || !this->genIDIsUnique()) {
        return;
    }
    SkASSERT(!listener->shouldDeregister());
    fGenIDChangeListeners.add(std::move(listener));
}

void SkPixelRef::callGenIDChangeListeners() {
    if (this->genIDIsUnique()) {
        fGenIDChangeListeners.changed();
        if (fAddedToCache.exchange(false)) {
            SkNotifyBitmapGenIDIsStale(this->getGenerationID());
        }
    } else {
        fGenIDChangeListeners.reset();
    }
}

void SkPixelRef::notifyPixelsChanged() {
    SkASSERT(!this->isTemporarilyImmutable());
#ifdef SK_DEBUG
    if (this->isImmutable()) {
        SkDebugf("========== notifyPixelsChanged called on immutable pixelref");
    }
#endif
    this->callGenIDChangeListeners();
    this->needsNewGenID();
}

void SkPixelRef::setImmutable() {
    fMutability = kImmutable;
}

void SkPixelRef::setImmutableWithID(uint32_t genID) {
    fMutability = kImmutable;
    fTaggedGenID.store(genID);
}

void SkPixelRef::setTemporarilyImmutable() {
    SkASSERT(fMutability != kImmutable);
    fMutability = kTemporarilyImmutable;
}

void SkPixelRef::restoreMutability() {
    SkASSERT(fMutability != kImmutable);
    fMutability = kMutable;
}

sk_sp<SkPixelRef> SkMakePixelRefWithProc(int width, int height, size_t rowBytes, void* addr,
                                         void (*releaseProc)(void* addr, void* ctx), void* ctx) {
    SkASSERT(width >= 0 && height >= 0);
    if (nullptr == releaseProc) {
        return sk_make_sp<SkPixelRef>(width, height, addr, rowBytes);
    }
    struct PixelRef final : public SkPixelRef {
        void (*fReleaseProc)(void*, void*);
        RAW_PTR_EXCLUSION void* fReleaseProcContext;
        PixelRef(int w, int h, void* s, size_t r, void (*proc)(void*, void*), void* ctx)
            : SkPixelRef(w, h, s, r), fReleaseProc(proc), fReleaseProcContext(ctx) {}
        ~PixelRef() override { fReleaseProc(this->pixels(), fReleaseProcContext); }
    };
    return sk_sp<SkPixelRef>(new PixelRef(width, height, addr, rowBytes, releaseProc, ctx));
}

// ============================================================================
// SkMallocPixelRef.cpp
// ============================================================================

sk_sp<SkPixelRef> SkMallocPixelRef::MakeAllocate(const SkImageInfo& info, size_t rowBytes) {
#if defined(SK_BUILD_FOR_FUZZER)
    constexpr bool kFuzzerLimit = true;
#else
    constexpr bool kFuzzerLimit = false;
#endif
    SkiaRsMallocAllocPlan plan{};
    if (!skia_rs_validate_malloc_allocate(info.width(),
                                          info.height(),
                                          static_cast<int32_t>(info.colorType()),
                                          static_cast<int32_t>(info.alphaType()),
                                          rowBytes,
                                          kFuzzerLimit,
                                          &plan)) {
        return nullptr;
    }

    void* addr = sk_calloc_canfail(plan.byte_size);
    if (nullptr == addr) {
        return nullptr;
    }

    struct PixelRef final : public SkPixelRef {
        PixelRef(int w, int h, void* s, size_t r) : SkPixelRef(w, h, s, r) {}
        ~PixelRef() override { sk_free(this->pixels()); }
    };
    return sk_sp<SkPixelRef>(new PixelRef(info.width(), info.height(), addr, plan.row_bytes));
}

sk_sp<SkPixelRef> SkMallocPixelRef::MakeWithData(const SkImageInfo& info,
                                                 size_t rowBytes,
                                                 sk_sp<SkData> data) {
    SkASSERT(data != nullptr);
    if (!skia_rs_validate_make_with_data(info.width(),
                                         info.height(),
                                         static_cast<int32_t>(info.colorType()),
                                         static_cast<int32_t>(info.alphaType()),
                                         rowBytes,
                                         data->size())) {
        return nullptr;
    }
    struct PixelRef final : public SkPixelRef {
        sk_sp<SkData> fData;
        PixelRef(int w, int h, void* s, size_t r, sk_sp<SkData> d)
            : SkPixelRef(w, h, s, r), fData(std::move(d)) {}
    };
    void* pixels = const_cast<void*>(data->data());
    sk_sp<SkPixelRef> pr(new PixelRef(info.width(), info.height(), pixels, rowBytes,
                                      std::move(data)));
    pr->setImmutable();
    return pr;
}

// ============================================================================
// SkBitmapCache.cpp
// ============================================================================

uint64_t SkMakeResourceCacheSharedIDForBitmap(uint32_t bitmapGenID) {
    return skia_rs_make_resource_cache_shared_id_for_bitmap(bitmapGenID);
}

void SkNotifyBitmapGenIDIsStale(uint32_t bitmapGenID) {
    SkResourceCache::PostPurgeSharedID(SkMakeResourceCacheSharedIDForBitmap(bitmapGenID));
}

SkBitmapCacheDesc SkBitmapCacheDesc::Make(uint32_t imageID, const SkIRect& subset) {
    SkASSERT(imageID);
    SkASSERT(subset.width() > 0 && subset.height() > 0);
    return { imageID, subset };
}

SkBitmapCacheDesc SkBitmapCacheDesc::Make(const SkImage* image) {
    SkIRect bounds = SkIRect::MakeWH(image->width(), image->height());
    return Make(image->uniqueID(), bounds);
}

namespace {
static unsigned gBitmapKeyNamespaceLabel;

struct BitmapKey : public SkResourceCache::Key {
public:
    BitmapKey(const SkBitmapCacheDesc& desc) : fDesc(desc) {
        this->init(&gBitmapKeyNamespaceLabel, SkMakeResourceCacheSharedIDForBitmap(fDesc.fImageID),
                   sizeof(fDesc));
    }

    const SkBitmapCacheDesc fDesc;
};
}  // namespace

void SkBitmapCache_setImmutableWithID(SkPixelRef* pr, uint32_t id) {
    pr->setImmutableWithID(id);
}

class SkBitmapCache::Rec : public SkResourceCache::Rec {
public:
    Rec(const SkBitmapCacheDesc& desc, const SkImageInfo& info, size_t rowBytes,
        std::unique_ptr<SkDiscardableMemory> dm, void* block)
        : fKey(desc)
        , fDM(std::move(dm))
        , fMalloc(block)
        , fInfo(info)
        , fRowBytes(rowBytes) {
        SkASSERT(!(fDM && fMalloc));
        fPrUniqueID = SkNextID::ImageID();
    }

    ~Rec() override {
        SkASSERT(0 == fExternalCounter);
        if (fDM && fDiscardableIsLocked) {
            SkASSERT(fDM->data());
            fDM->unlock();
        }
        sk_free(fMalloc);
    }

    const Key& getKey() const override { return fKey; }
    size_t bytesUsed() const override {
        return sizeof(fKey) + fInfo.computeByteSize(fRowBytes);
    }
    bool canBePurged() override {
        SkAutoMutexExclusive ama(fMutex);
        return fExternalCounter == 0;
    }
    void postAddInstall(void* payload) override {
        SkAssertResult(this->install(static_cast<SkBitmap*>(payload)));
    }

    const char* getCategory() const override { return "bitmap"; }
    SkDiscardableMemory* diagnostic_only_getDiscardable() const override {
        return fDM.get();
    }

    static void ReleaseProc(void* addr, void* ctx) {
        Rec* rec = static_cast<Rec*>(ctx);
        SkAutoMutexExclusive ama(rec->fMutex);

        SkASSERT(rec->fExternalCounter > 0);
        rec->fExternalCounter -= 1;
        if (rec->fDM) {
            SkASSERT(rec->fMalloc == nullptr);
            if (rec->fExternalCounter == 0) {
                rec->fDM->unlock();
                rec->fDiscardableIsLocked = false;
            }
        } else {
            SkASSERT(rec->fMalloc != nullptr);
        }
    }

    bool install(SkBitmap* bitmap) {
        SkAutoMutexExclusive ama(fMutex);

        if (!fDM && !fMalloc) {
            return false;
        }

        if (fDM) {
            if (!fDiscardableIsLocked) {
                SkASSERT(fExternalCounter == 0);
                if (!fDM->lock()) {
                    fDM.reset(nullptr);
                    return false;
                }
                fDiscardableIsLocked = true;
            }
            SkASSERT(fDM->data());
        }

        bitmap->installPixels(
                fInfo, fDM ? fDM->data() : fMalloc.get(), fRowBytes, ReleaseProc, this);
        SkBitmapCache_setImmutableWithID(bitmap->pixelRef(), fPrUniqueID);
        fExternalCounter++;

        return true;
    }

    static bool Finder(const SkResourceCache::Rec& baseRec, void* contextBitmap) {
        Rec* rec = const_cast<Rec*>(static_cast<const Rec*>(&baseRec));
        SkBitmap* result = static_cast<SkBitmap*>(contextBitmap);
        return rec->install(result);
    }

private:
    BitmapKey   fKey;
    SkMutex     fMutex;
    std::unique_ptr<SkDiscardableMemory> fDM;
    raw_ptr<void, DanglingUntriaged> fMalloc;
    SkImageInfo fInfo;
    size_t      fRowBytes;
    uint32_t    fPrUniqueID;
    int  fExternalCounter     = 0;
    bool fDiscardableIsLocked = true;
};

void SkBitmapCache::PrivateDeleteRec(Rec* rec) { delete rec; }

SkBitmapCache::RecPtr SkBitmapCache::Alloc(const SkBitmapCacheDesc& desc, const SkImageInfo& info,
                                           SkPixmap* pmap) {
    SkASSERT(info.width() == desc.fSubset.width());
    SkASSERT(info.height() == desc.fSubset.height());

    SkiaRsMallocAllocPlan plan{};
    if (!skia_rs_validate_bitmap_cache_alloc(
                info.width(), info.height(), static_cast<int32_t>(info.colorType()), &plan)) {
        return nullptr;
    }

    std::unique_ptr<SkDiscardableMemory> dm;
    void* block = nullptr;

    auto factory = SkResourceCache::GetDiscardableFactory();
    if (factory) {
        dm.reset(factory(plan.byte_size));
    } else {
        block = sk_malloc_canfail(plan.byte_size);
    }
    if (!dm && !block) {
        return nullptr;
    }
    *pmap = SkPixmap(info, dm ? dm->data() : block, plan.row_bytes);
    return RecPtr(new Rec(desc, info, plan.row_bytes, std::move(dm), block));
}

void SkBitmapCache::Add(RecPtr rec, SkBitmap* bitmap) {
    SkResourceCache::Add(rec.release(), bitmap);
}

bool SkBitmapCache::Find(const SkBitmapCacheDesc& desc, SkBitmap* result) {
    desc.validate();
    return SkResourceCache::Find(BitmapKey(desc), SkBitmapCache::Rec::Finder, result);
}

#define CHECK_LOCAL(localCache, localName, globalName, ...) \
    ((localCache) ? localCache->localName(__VA_ARGS__) : SkResourceCache::globalName(__VA_ARGS__))

namespace {
static unsigned gMipMapKeyNamespaceLabel;

struct MipMapKey : public SkResourceCache::Key {
public:
    MipMapKey(const SkBitmapCacheDesc& desc) : fDesc(desc) {
        this->init(&gMipMapKeyNamespaceLabel, SkMakeResourceCacheSharedIDForBitmap(fDesc.fImageID),
                   sizeof(fDesc));
    }

    const SkBitmapCacheDesc fDesc;
};

struct MipMapRec : public SkResourceCache::Rec {
    MipMapRec(const SkBitmapCacheDesc& desc, const SkMipmap* result)
        : fKey(desc)
        , fMipMap(result) {
        fMipMap->attachToCacheAndRef();
    }

    ~MipMapRec() override {
        fMipMap->detachFromCacheAndUnref();
    }

    const Key& getKey() const override { return fKey; }
    size_t bytesUsed() const override { return sizeof(fKey) + fMipMap->size(); }
    const char* getCategory() const override { return "mipmap"; }
    SkDiscardableMemory* diagnostic_only_getDiscardable() const override {
        return fMipMap->diagnostic_only_getDiscardable();
    }

    static bool Finder(const SkResourceCache::Rec& baseRec, void* contextMip) {
        const MipMapRec& rec = static_cast<const MipMapRec&>(baseRec);
        const SkMipmap* mm = SkRef(rec.fMipMap.get());
        if (nullptr == mm->data()) {
            mm->unref();
            return false;
        }
        *(const SkMipmap**)contextMip = mm;
        return true;
    }

private:
    MipMapKey                                  fKey;
    raw_ptr<const SkMipmap, DanglingUntriaged> fMipMap;
};
}  // namespace

const SkMipmap* SkMipmapCache::FindAndRef(const SkBitmapCacheDesc& desc,
                                          SkResourceCache* localCache) {
    MipMapKey key(desc);
    const SkMipmap* result;

    if (!CHECK_LOCAL(localCache, find, Find, key, MipMapRec::Finder, &result)) {
        result = nullptr;
    }
    return result;
}

static SkResourceCache::DiscardableFactory get_fact(SkResourceCache* localCache) {
    return localCache ? localCache->discardableFactory()
                      : SkResourceCache::GetDiscardableFactory();
}

const SkMipmap* SkMipmapCache::AddAndRef(const SkImage_Base* image, SkResourceCache* localCache) {
    SkBitmap src;
    if (!image->getROPixels(nullptr, &src)) {
        return nullptr;
    }

    SkMipmap* mipmap = SkMipmap::Build(src, get_fact(localCache));
    if (mipmap) {
        MipMapRec* rec = new MipMapRec(SkBitmapCacheDesc::Make(image), mipmap);
        CHECK_LOCAL(localCache, add, Add, rec);
        image->notifyAddedToRasterCache();
    }
    return mipmap;
}

// ============================================================================
// SkBitmap.cpp
// ============================================================================

static bool reset_return_false(SkBitmap* bm) {
    bm->reset();
    return false;
}

SkBitmap::SkBitmap() {}

SkBitmap::SkBitmap(const SkBitmap& src) : fPixelRef(src.fPixelRef), fPixmap(src.fPixmap) {
    SkDEBUGCODE(src.validate();)
    SkDEBUGCODE(this->validate();)
}

SkBitmap::SkBitmap(SkBitmap&& other)
        : fPixelRef(std::move(other.fPixelRef)), fPixmap(std::move(other.fPixmap)) {
    SkASSERT(!other.fPixelRef);
    other.fPixmap.reset();
}

SkBitmap::~SkBitmap() {}

SkBitmap& SkBitmap::operator=(const SkBitmap& src) {
    if (this != &src) {
        fPixelRef = src.fPixelRef;
        fPixmap = src.fPixmap;
    }
    SkDEBUGCODE(this->validate();)
    return *this;
}

SkBitmap& SkBitmap::operator=(SkBitmap&& other) {
    if (this != &other) {
        fPixelRef = std::move(other.fPixelRef);
        fPixmap = std::move(other.fPixmap);
        SkASSERT(!other.fPixelRef);
        other.fPixmap.reset();
    }
    return *this;
}

void SkBitmap::swap(SkBitmap& other) {
    using std::swap;
    swap(*this, other);
    SkDEBUGCODE(this->validate();)
}

void SkBitmap::reset() {
    fPixelRef = nullptr;
    fPixmap.reset();
}

void SkBitmap::getBounds(SkRect* bounds) const {
    SkASSERT(bounds);
    *bounds = SkRect::Make(this->dimensions());
}

void SkBitmap::getBounds(SkIRect* bounds) const {
    SkASSERT(bounds);
    *bounds = fPixmap.bounds();
}

SkColorSpace* SkBitmap::colorSpace() const { return fPixmap.colorSpace(); }

sk_sp<SkColorSpace> SkBitmap::refColorSpace() const { return fPixmap.info().refColorSpace(); }

bool SkBitmap::setInfo(const SkImageInfo& info, size_t rowBytes) {
    SkiaRsBitmapSetInfoResult res{};
    if (!skia_rs_bitmap_validate_set_info(info.width(),
                                          info.height(),
                                          static_cast<int32_t>(info.colorType()),
                                          static_cast<int32_t>(info.alphaType()),
                                          rowBytes,
                                          &res)) {
        return reset_return_false(this);
    }

    fPixelRef = nullptr;
    fPixmap.reset(info.makeAlphaType(static_cast<SkAlphaType>(res.alpha_type)),
                  nullptr,
                  SkToU32(res.row_bytes));
    SkDEBUGCODE(this->validate();)
    return true;
}

bool SkBitmap::setAlphaType(SkAlphaType newAlphaType) {
    if (!SkColorTypeValidateAlphaType(this->colorType(), newAlphaType, &newAlphaType)) {
        return false;
    }
    if (this->alphaType() != newAlphaType) {
        auto newInfo = fPixmap.info().makeAlphaType(newAlphaType);
        fPixmap.reset(std::move(newInfo), fPixmap.addr(), fPixmap.rowBytes());
    }
    SkDEBUGCODE(this->validate();)
    return true;
}

void SkBitmap::setColorSpace(sk_sp<SkColorSpace> newColorSpace) {
    if (this->colorSpace() != newColorSpace.get()) {
        SkImageInfo newInfo = fPixmap.info().makeColorSpace(std::move(newColorSpace));
        fPixmap.reset(std::move(newInfo), fPixmap.addr(), fPixmap.rowBytes());
    }
    SkDEBUGCODE(this->validate();)
}

SkIPoint SkBitmap::pixelRefOrigin() const {
    const char* addr = static_cast<const char*>(fPixmap.addr());
    const char* pix = static_cast<const char*>(fPixelRef ? fPixelRef->pixels() : nullptr);
    size_t rb = this->rowBytes();
    if (!pix || 0 == rb) {
        return {0, 0};
    }
    SkASSERT(this->bytesPerPixel() > 0);
    SkASSERT(this->bytesPerPixel() == (1 << this->shiftPerPixel()));
    SkASSERT(addr >= pix);
    return skia_rs_bitmap_pixel_ref_origin(
            static_cast<size_t>(addr - pix), rb, static_cast<int32_t>(this->colorType()));
}

void SkBitmap::setPixelRef(sk_sp<SkPixelRef> pr, int dx, int dy) {
#ifdef SK_DEBUG
    if (pr) {
        if (kUnknown_SkColorType != this->colorType()) {
            SkASSERT(dx >= 0 && this->width() + dx <= pr->width());
            SkASSERT(dy >= 0 && this->height() + dy <= pr->height());
        }
    }
#endif
    fPixelRef = kUnknown_SkColorType != this->colorType() ? std::move(pr) : nullptr;
    void* p = nullptr;
    size_t rowBytes = this->rowBytes();
    if (fPixelRef) {
        rowBytes = fPixelRef->rowBytes();
        p = fPixelRef->pixels();
        if (p) {
            size_t off = skia_rs_bitmap_pixel_ref_byte_offset(
                    dx, dy, rowBytes, static_cast<int32_t>(this->colorType()));
            p = static_cast<char*>(p) + off;
        }
    }
    fPixmap.reset(fPixmap.info(), p, rowBytes);
    SkDEBUGCODE(this->validate();)
}

void SkBitmap::setPixels(void* p) {
    if (kUnknown_SkColorType == this->colorType()) {
        p = nullptr;
    }
    size_t rb = this->rowBytes();
    fPixmap.reset(fPixmap.info(), p, rb);
    fPixelRef = p ? sk_make_sp<SkPixelRef>(this->width(), this->height(), p, rb) : nullptr;
    SkDEBUGCODE(this->validate();)
}

bool SkBitmap::tryAllocPixels(Allocator* allocator) {
    HeapAllocator stdalloc;
    if (nullptr == allocator) {
        allocator = &stdalloc;
    }
    return allocator->allocPixelRef(this);
}

bool SkBitmap::tryAllocN32Pixels(int width, int height, bool isOpaque) {
    SkImageInfo info = SkImageInfo::MakeN32(width, height,
            isOpaque ? kOpaque_SkAlphaType : kPremul_SkAlphaType);
    return this->tryAllocPixels(info);
}

void SkBitmap::allocN32Pixels(int width, int height, bool isOpaque) {
    SkImageInfo info = SkImageInfo::MakeN32(width, height,
                                        isOpaque ? kOpaque_SkAlphaType : kPremul_SkAlphaType);
    this->allocPixels(info);
}

void SkBitmap::allocPixels() {
    this->allocPixels((Allocator*)nullptr);
}

void SkBitmap::allocPixels(Allocator* allocator) {
    if (!this->tryAllocPixels(allocator)) {
        const SkImageInfo& info = this->info();
        SK_ABORT("SkBitmap::tryAllocPixels failed "
                 "ColorType:%d AlphaType:%d [w:%d h:%d] rb:%zu",
                 info.colorType(), info.alphaType(), info.width(), info.height(), this->rowBytes());
    }
}

void SkBitmap::allocPixelsFlags(const SkImageInfo& info, uint32_t flags) {
    SkASSERTF_RELEASE(this->tryAllocPixelsFlags(info, flags),
                      "ColorType:%d AlphaType:%d [w:%d h:%d] rb:%zu flags: 0x%x",
                      info.colorType(), info.alphaType(), info.width(), info.height(),
                      this->rowBytes(), flags);
}

void SkBitmap::allocPixels(const SkImageInfo& info, size_t rowBytes) {
    SkASSERTF_RELEASE(this->tryAllocPixels(info, rowBytes),
                      "ColorType:%d AlphaType:%d [w:%d h:%d] rb:%zu",
                      info.colorType(), info.alphaType(), info.width(), info.height(),
                      this->rowBytes());
}

void SkBitmap::allocPixels(const SkImageInfo& info) {
    this->allocPixels(info, info.minRowBytes());
}

bool SkBitmap::tryAllocPixels(const SkImageInfo& requestedInfo, size_t rowBytes) {
    if (!this->setInfo(requestedInfo, rowBytes)) {
        return reset_return_false(this);
    }

    const SkImageInfo& correctedInfo = this->info();
    if (kUnknown_SkColorType == correctedInfo.colorType()) {
        return true;
    }
    rowBytes = this->rowBytes();

    sk_sp<SkPixelRef> pr = SkMallocPixelRef::MakeAllocate(correctedInfo, rowBytes);
    if (!pr) {
        return reset_return_false(this);
    }
    this->setPixelRef(std::move(pr), 0, 0);
    if (nullptr == this->getPixels()) {
        return reset_return_false(this);
    }
    SkDEBUGCODE(this->validate();)
    return true;
}

bool SkBitmap::tryAllocPixelsFlags(const SkImageInfo& requestedInfo, uint32_t allocFlags) {
    if (!this->setInfo(requestedInfo)) {
        return reset_return_false(this);
    }

    const SkImageInfo& correctedInfo = this->info();

    sk_sp<SkPixelRef> pr = SkMallocPixelRef::MakeAllocate(correctedInfo,
                                                          correctedInfo.minRowBytes());
    if (!pr) {
        return reset_return_false(this);
    }
    this->setPixelRef(std::move(pr), 0, 0);
    if (nullptr == this->getPixels()) {
        return reset_return_false(this);
    }
    SkDEBUGCODE(this->validate();)
    return true;
}

static void invoke_release_proc(void (*proc)(void* pixels, void* ctx), void* pixels, void* ctx) {
    if (proc) {
        proc(pixels, ctx);
    }
}

bool SkBitmap::installPixels(const SkImageInfo& requestedInfo, void* pixels, size_t rb,
                             void (*releaseProc)(void* addr, void* context), void* context) {
    if (!this->setInfo(requestedInfo, rb)) {
        invoke_release_proc(releaseProc, pixels, context);
        this->reset();
        return false;
    }
    if (nullptr == pixels) {
        invoke_release_proc(releaseProc, pixels, context);
        return true;
    }

    const SkImageInfo& correctedInfo = this->info();
    this->setPixelRef(
            SkMakePixelRefWithProc(correctedInfo.width(), correctedInfo.height(),
                                   rb, pixels, releaseProc, context), 0, 0);
    SkDEBUGCODE(this->validate();)
    return true;
}

bool SkBitmap::installPixels(const SkPixmap& pixmap) {
    return this->installPixels(pixmap.info(), pixmap.writable_addr(), pixmap.rowBytes(),
                               nullptr, nullptr);
}

uint32_t SkBitmap::getGenerationID() const {
    return fPixelRef ? fPixelRef->getGenerationID() : 0;
}

void SkBitmap::notifyPixelsChanged() const {
    SkASSERT(!this->isImmutable());
    if (fPixelRef) {
        fPixelRef->notifyPixelsChanged();
    }
}

bool SkBitmap::HeapAllocator::allocPixelRef(SkBitmap* dst) {
    const SkImageInfo& info = dst->info();
    if (kUnknown_SkColorType == info.colorType()) {
        return false;
    }

    sk_sp<SkPixelRef> pr = SkMallocPixelRef::MakeAllocate(info, dst->rowBytes());
    if (!pr) {
        return false;
    }

    dst->setPixelRef(std::move(pr), 0, 0);
    SkDEBUGCODE(dst->validate();)
    return true;
}

bool SkBitmap::isImmutable() const {
    return fPixelRef ? fPixelRef->isImmutable() : false;
}

void SkBitmap::setImmutable() {
    if (fPixelRef) {
        fPixelRef->setImmutable();
    }
}

void* SkBitmap::getAddr(int x, int y) const {
    SkASSERT((unsigned)x < (unsigned)this->width());
    SkASSERT((unsigned)y < (unsigned)this->height());

    char* base = static_cast<char*>(this->getPixels());
    if (base) {
        base += skia_rs_bitmap_addr_offset(
                x, y, this->rowBytes(), static_cast<int32_t>(this->colorType()));
    }
    return base;
}

void SkBitmap::erase(SkColor4f c, const SkIRect& area) const {
    SkDEBUGCODE(this->validate();)

    if (kUnknown_SkColorType == this->colorType()) {
        return;
    }

    SkPixmap result;
    if (!this->peekPixels(&result)) {
        return;
    }

    if (result.erase(c, &area)) {
        this->notifyPixelsChanged();
    }
}

void SkBitmap::erase(SkColor c, const SkIRect& area) const {
    this->erase(SkColor4f::FromColor(c), area);
}

void SkBitmap::eraseColor(SkColor4f c) const {
    this->erase(c, SkIRect::MakeWH(this->width(), this->height()));
}

void SkBitmap::eraseColor(SkColor c) const {
    this->erase(SkColor4f::FromColor(c), SkIRect::MakeWH(this->width(), this->height()));
}

bool SkBitmap::extractSubset(SkBitmap* result, const SkIRect& subset) const {
    SkDEBUGCODE(this->validate();)

    if (nullptr == result || !fPixelRef) {
        return false;
    }

    SkIPoint origin = this->pixelRefOrigin();
    SkiaRsBitmapExtractSubsetResult res{};
    if (!skia_rs_bitmap_extract_subset(this->width(), this->height(), origin, &subset, &res)) {
        return false;
    }

    SkASSERT(static_cast<unsigned>(res.clipped.fLeft) < static_cast<unsigned>(this->width()));
    SkASSERT(static_cast<unsigned>(res.clipped.fTop) < static_cast<unsigned>(this->height()));

    SkBitmap dst;
    dst.setInfo(this->info().makeDimensions(res.clipped.size()), this->rowBytes());

    if (fPixelRef) {
        dst.setPixelRef(fPixelRef, res.new_origin.x(), res.new_origin.y());
    }
    SkDEBUGCODE(dst.validate();)

    result->swap(dst);
    return true;
}

bool SkBitmap::readPixels(const SkImageInfo& requestedDstInfo, void* dstPixels, size_t dstRB,
                          int x, int y) const {
    SkPixmap src;
    if (!this->peekPixels(&src)) {
        return false;
    }
    return src.readPixels(requestedDstInfo, dstPixels, dstRB, x, y);
}

bool SkBitmap::readPixels(const SkPixmap& dst, int srcX, int srcY) const {
    return this->readPixels(dst.info(), dst.writable_addr(), dst.rowBytes(), srcX, srcY);
}

bool SkBitmap::writePixels(const SkPixmap& src, int dstX, int dstY) {
    if (!SkImageInfoValidConversion(this->info(), src.info())) {
        return false;
    }

    SkWritePixelsRec rec(src.info(), src.addr(), src.rowBytes(), dstX, dstY);
    if (!rec.trim(this->width(), this->height())) {
        return false;
    }

    void* dstPixels = this->getAddr(rec.fX, rec.fY);
    const SkImageInfo dstInfo = this->info().makeDimensions(rec.fInfo.dimensions());
    if (!SkConvertPixels(dstInfo,     dstPixels, this->rowBytes(),
                         rec.fInfo, rec.fPixels,   rec.fRowBytes)) {
        return false;
    }
    this->notifyPixelsChanged();
    return true;
}

static bool GetBitmapAlpha(const SkBitmap& src, uint8_t* SK_RESTRICT alpha, int alphaRowBytes) {
    SkASSERT(alpha != nullptr);
    SkASSERT(alphaRowBytes >= src.width());

    SkPixmap pmap;
    if (!src.peekPixels(&pmap)) {
        skia_rs_bitmap_zero_alpha_buffer(alpha,
                                         static_cast<size_t>(src.width()),
                                         static_cast<size_t>(src.height()),
                                         static_cast<size_t>(alphaRowBytes));
        return false;
    }
    return SkConvertPixels(SkImageInfo::MakeA8(pmap.width(), pmap.height()), alpha, alphaRowBytes,
                           pmap.info(), pmap.addr(), pmap.rowBytes());
}

bool SkBitmap::extractAlpha(SkBitmap* dst, const SkPaint* paint,
                            Allocator* allocator, SkIPoint* offset) const {
    SkDEBUGCODE(this->validate();)

    SkiaRsExtractAlphaMaskSetup setup{};
    if (!skia_rs_bitmap_extract_alpha_setup(this->width(), this->height(), &setup)) {
        return false;
    }

    SkBitmap tmpBitmap;
    SkMatrix identity;
    SkMaskBuilder srcM, dstM;
    srcM.bounds() = setup.bounds;
    srcM.rowBytes() = setup.row_bytes;
    srcM.format() = SkMask::kA8_Format;

    SkMaskFilter* filter = paint ? paint->getMaskFilter() : nullptr;

    if (filter) {
        identity.reset();
        if (!as_MFB(filter)->filterMask(&dstM, srcM, identity, nullptr)) {
            goto NO_FILTER_CASE;
        }
        dstM.rowBytes() = SkAlign4(dstM.fBounds.width());
    } else {
    NO_FILTER_CASE:
        tmpBitmap.setInfo(SkImageInfo::MakeA8(this->width(), this->height()), srcM.fRowBytes);
        if (!tmpBitmap.tryAllocPixels(allocator)) {
            SkDebugf("extractAlpha failed to allocate (%d,%d) alpha bitmap\n",
                     tmpBitmap.width(), tmpBitmap.height());
            return false;
        }
        GetBitmapAlpha(*this, tmpBitmap.getAddr8(0, 0), srcM.fRowBytes);
        if (offset) {
            offset->set(0, 0);
        }
        tmpBitmap.swap(*dst);
        return true;
    }
    srcM.image() = SkMaskBuilder::AllocImage(srcM.computeImageSize());
    SkAutoMaskFreeImage srcCleanup(srcM.image());

    GetBitmapAlpha(*this, srcM.image(), srcM.fRowBytes);
    if (!as_MFB(filter)->filterMask(&dstM, srcM, identity, nullptr)) {
        goto NO_FILTER_CASE;
    }
    SkAutoMaskFreeImage dstCleanup(dstM.image());

    tmpBitmap.setInfo(SkImageInfo::MakeA8(dstM.fBounds.width(), dstM.fBounds.height()),
                      dstM.fRowBytes);
    if (!tmpBitmap.tryAllocPixels(allocator)) {
        SkDebugf("extractAlpha failed to allocate (%d,%d) alpha bitmap\n",
                 tmpBitmap.width(), tmpBitmap.height());
        return false;
    }
    memcpy(tmpBitmap.getPixels(), dstM.fImage, dstM.computeImageSize());
    if (offset) {
        offset->set(dstM.fBounds.fLeft, dstM.fBounds.fTop);
    }
    SkDEBUGCODE(tmpBitmap.validate();)

    tmpBitmap.swap(*dst);
    return true;
}

#ifdef SK_DEBUG
void SkBitmap::validate() const {
    this->info().validate();

    SkASSERT(this->info().validRowBytes(this->rowBytes()));

    if (fPixelRef && fPixelRef->pixels()) {
        SkASSERT(this->getPixels());
    } else {
        SkASSERT(!this->getPixels());
    }

    if (this->getPixels()) {
        SkASSERT(fPixelRef);
        SkASSERT(fPixelRef->rowBytes() == this->rowBytes());
        SkIPoint origin = this->pixelRefOrigin();
        SkASSERT(origin.fX >= 0);
        SkASSERT(origin.fY >= 0);
        SkASSERT(fPixelRef->width() >= (int)this->width() + origin.fX);
        SkASSERT(fPixelRef->height() >= (int)this->height() + origin.fY);
        SkASSERT(fPixelRef->rowBytes() >= this->info().minRowBytes());
    }
}
#endif

bool SkBitmap::peekPixels(SkPixmap* pmap) const {
    if (this->getPixels()) {
        if (pmap) {
            *pmap = fPixmap;
        }
        return true;
    }
    return false;
}

sk_sp<SkImage> SkBitmap::asImage() const { return SkImages::RasterFromBitmap(*this); }

sk_sp<SkShader> SkBitmap::makeShader(const SkSamplingOptions& sampling,
                                     const SkMatrix& lm) const {
    return this->makeShader(SkTileMode::kClamp, SkTileMode::kClamp, sampling, &lm);
}

sk_sp<SkShader> SkBitmap::makeShader(const SkSamplingOptions& sampling,
                                     const SkMatrix* lm) const {
    return this->makeShader(SkTileMode::kClamp, SkTileMode::kClamp, sampling, lm);
}

sk_sp<SkShader> SkBitmap::makeShader(SkTileMode tmx, SkTileMode tmy,
                                     const SkSamplingOptions& sampling,
                                     const SkMatrix& lm) const {
    if (!lm.invert()) {
        return nullptr;
    }
    return SkImageShader::Make(SkImage_Raster::MakeFromBitmap(*this, SkCopyPixelsMode::kIfMutable),
                               tmx,
                               tmy,
                               sampling,
                               &lm);
}

sk_sp<SkShader> SkBitmap::makeShader(SkTileMode tmx, SkTileMode tmy,
                                     const SkSamplingOptions& sampling,
                                     const SkMatrix* lm) const {
    if (lm && !lm->invert()) {
        return nullptr;
    }
    return SkImageShader::Make(SkImage_Raster::MakeFromBitmap(*this, SkCopyPixelsMode::kIfMutable),
                               tmx,
                               tmy,
                               sampling,
                               lm);
}
