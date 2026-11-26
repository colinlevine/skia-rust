/*
 * Safe Rust bridge for:
 *   src/image/SkImage_Lazy.cpp
 *   src/image/SkImage_LazyFactories.cpp
 *   src/image/SkImage_Picture.cpp
 * (Layer 3.4b skia-rs-image).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageGenerator.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPicture.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRecorder.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSize.h"
#include "include/core/SkSurface.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkYUVAInfo.h"
#include "include/private/SkAssert.h"
#include "include/private/SkMutex.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTFitsIn.h"
#include "src/core/SkBitmapCache.h"
#include "src/core/SkCachedData.h"
#include "src/core/SkNextID.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkTLazy.h"
#include "src/core/SkYUVPlanesCache.h"
#include "src/cpu/Recorder.h"
#include "src/image/SkImageGeneratorPriv.h"
#include "src/image/SkImage_Lazy.h"
#include "src/image/SkImage_Picture.h"
#include "src/image/SkPictureImageGenerator.h"

#include <cstdint>
#include <memory>
#include <utility>

class SkPaint;

namespace {

struct SkiaRsLazyValidatorPlan {
    bool valid;
    bool apply_color_type;
    int32_t color_type;
    bool apply_color_space;
    uint32_t unique_id;
};

struct SkiaRsPictureSubsetPlan {
    SkMatrix matrix;
    SkISize dimensions;
    int32_t bit_depth;
};

static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkiaRsLazyValidatorPlan) == 16);
static_assert(sizeof(SkiaRsPictureSubsetPlan) == 52);
static_assert(SkTiledImageUtils::kNumImageKeyValues == 6);

}  // namespace

extern "C" {

void skia_rs_image_lazy_plan_validator(bool has_generator,
                                       int32_t gen_width,
                                       int32_t gen_height,
                                       int32_t gen_ct_raw,
                                       uint32_t gen_unique_id,
                                       bool has_color_type,
                                       int32_t requested_ct_raw,
                                       bool has_color_space,
                                       SkiaRsLazyValidatorPlan* out);
bool skia_rs_image_lazy_can_ref_encoded(uint32_t gen_unique_id, uint32_t img_unique_id);
bool skia_rs_image_lazy_color_type_and_space_cache_hit(bool has_cached,
                                                       int32_t target_ct_raw,
                                                       int32_t cached_ct_raw,
                                                       bool same_color_space);
bool skia_rs_image_lazy_yuva_dimensions_match(int32_t yuva_w,
                                              int32_t yuva_h,
                                              int32_t img_w,
                                              int32_t img_h);
bool skia_rs_image_picture_plan_subset(const SkMatrix* orig_matrix,
                                       const SkIRect* subset,
                                       int32_t ct_raw,
                                       SkiaRsPictureSubsetPlan* out);
bool skia_rs_image_picture_key_values(bool has_paint,
                                      bool is_srgb,
                                      const SkMatrix* matrix,
                                      int32_t ct_raw,
                                      uint32_t pixel_geometry,
                                      uint32_t surface_prop_flags,
                                      int32_t width,
                                      int32_t height,
                                      uint32_t picture_unique_id,
                                      uint32_t* out_keys);

}  // extern "C"

// ============================================================================
// src/image/SkImage_Lazy.cpp
// ============================================================================

sk_sp<SharedGenerator> SharedGenerator::Make(std::unique_ptr<SkImageGenerator> gen) {
    return gen ? sk_sp<SharedGenerator>(new SharedGenerator(std::move(gen))) : nullptr;
}

SharedGenerator::SharedGenerator(std::unique_ptr<SkImageGenerator> gen)
        : fGenerator(std::move(gen)) {
    SkASSERT(fGenerator);
}

const SkImageInfo& SharedGenerator::getInfo() const { return fGenerator->getInfo(); }

bool SharedGenerator::isTextureGenerator() { return fGenerator->isTextureGenerator(); }

SkImage_Lazy::Validator::Validator(sk_sp<SharedGenerator> gen,
                                   const SkColorType* colorType,
                                   sk_sp<SkColorSpace> colorSpace)
        : fSharedGenerator(std::move(gen)) {
    if (!fSharedGenerator) {
        return;
    }

    fInfo = fSharedGenerator->fGenerator->getInfo();
    SkiaRsLazyValidatorPlan plan{};
    skia_rs_image_lazy_plan_validator(
            true,
            fInfo.width(),
            fInfo.height(),
            static_cast<int32_t>(fInfo.colorType()),
            fSharedGenerator->fGenerator->uniqueID(),
            colorType != nullptr,
            colorType ? static_cast<int32_t>(*colorType) : 0,
            colorSpace != nullptr,
            &plan);

    if (!plan.valid) {
        fSharedGenerator.reset();
        return;
    }

    if (plan.apply_color_type) {
        fInfo = fInfo.makeColorType(static_cast<SkColorType>(plan.color_type));
    }
    if (plan.apply_color_space) {
        fInfo = fInfo.makeColorSpace(colorSpace);
    }
    fUniqueID = plan.unique_id;
}

class SkImage_Lazy::ScopedGenerator {
public:
    SK_ONLY_STACK_ALLOCATABLE;

    ScopedGenerator(const sk_sp<SharedGenerator>& gen)
            : fSharedGenerator(gen), fAutoAcquire(gen->fMutex) {}

    SkImageGenerator* operator->() const {
        fSharedGenerator->fMutex.assertHeld();
        return fSharedGenerator->fGenerator.get();
    }

    operator SkImageGenerator*() const {
        fSharedGenerator->fMutex.assertHeld();
        return fSharedGenerator->fGenerator.get();
    }

private:
    const sk_sp<SharedGenerator>& fSharedGenerator;
    SkAutoMutexExclusive fAutoAcquire;
};

SkImage_Lazy::SkImage_Lazy(Validator* validator)
        : SkImage_Base(validator->fInfo, validator->fUniqueID, /*backingStorage=*/nullptr)
        , fSharedGenerator(std::move(validator->fSharedGenerator)) {
    SkASSERT(fSharedGenerator);
}

bool SkImage_Lazy::getROPixels(GrDirectContext* ctx,
                               SkBitmap* bitmap,
                               SkImage::CachingHint chint) const {
    auto check_output_bitmap = [bitmap]() {
        SkASSERT(bitmap->isImmutable());
        SkASSERT(bitmap->getPixels());
        (void)bitmap;
    };

    auto desc = SkBitmapCacheDesc::Make(this);
    if (SkBitmapCache::Find(desc, bitmap)) {
        check_output_bitmap();
        return true;
    }

    if (SkImage::kAllow_CachingHint == chint) {
        SkPixmap pmap;
        SkBitmapCache::RecPtr cacheRec = SkBitmapCache::Alloc(desc, this->imageInfo(), &pmap);
        if (!cacheRec) {
            return false;
        }
        bool success = false;
        {
            success = ScopedGenerator(fSharedGenerator)->getPixels(pmap);
        }
        if (!success && !this->readPixelsProxy(ctx, pmap)) {
            return false;
        }
        SkBitmapCache::Add(std::move(cacheRec), bitmap);
        this->notifyAddedToRasterCache();
    } else {
        if (!bitmap->tryAllocPixels(this->imageInfo())) {
            return false;
        }
        bool success = false;
        {
            success = ScopedGenerator(fSharedGenerator)->getPixels(bitmap->pixmap());
        }
        if (!success && !this->readPixelsProxy(ctx, bitmap->pixmap())) {
            return false;
        }
        bitmap->setImmutable();
    }
    check_output_bitmap();
    return true;
}

sk_sp<SharedGenerator> SkImage_Lazy::generator() const { return fSharedGenerator; }

bool SkImage_Lazy::onIsProtected() const {
    ScopedGenerator generator(fSharedGenerator);
    return generator->isProtected();
}

bool SkImage_Lazy::onReadPixels(GrDirectContext* dContext,
                                const SkImageInfo& dstInfo,
                                void* dstPixels,
                                size_t dstRB,
                                int srcX,
                                int srcY,
                                CachingHint chint) const {
    SkBitmap bm;
    if (this->getROPixels(dContext, &bm, chint)) {
        return bm.readPixels(dstInfo, dstPixels, dstRB, srcX, srcY);
    }
    return false;
}

sk_sp<const SkData> SkImage_Lazy::onRefEncoded() const {
    if (skia_rs_image_lazy_can_ref_encoded(fSharedGenerator->fGenerator->uniqueID(),
                                           this->uniqueID())) {
        ScopedGenerator generator(fSharedGenerator);
        return generator->refEncodedData();
    }
    return nullptr;
}

bool SkImage_Lazy::isValid(SkRecorder* recorder) const {
    ScopedGenerator generator(fSharedGenerator);
    return generator->isValid(recorder);
}

sk_sp<SkImage> SkImage_Lazy::onMakeSubset(SkRecorder*,
                                          const SkIRect& subset,
                                          RequiredProperties props) const {
    sk_sp<SkImage> nonLazyImg = this->makeRasterImage(nullptr);
    if (!nonLazyImg) {
        return nullptr;
    }
    return nonLazyImg->makeSubset(nullptr, subset, props);
}

sk_sp<SkSurface> SkImage_Lazy::onMakeSurface(SkRecorder* recorder, const SkImageInfo& info) const {
    if (!recorder) {
        recorder = skcpu::Recorder::TODO();
    }
    const SkSurfaceProps* props = nullptr;
    constexpr size_t rowBytes = 0;
    return recorder->cpuRecorder()->makeBitmapSurface(info, rowBytes, props);
}

sk_sp<SkImage> SkImage_Lazy::makeColorTypeAndColorSpace(SkRecorder*,
                                                        SkColorType targetColorType,
                                                        sk_sp<SkColorSpace> targetColorSpace,
                                                        RequiredProperties) const {
    SkAutoMutexExclusive autoAcquire(fOnMakeColorTypeAndSpaceMutex);
    const bool hasCached = fOnMakeColorTypeAndSpaceResult != nullptr;
    const int32_t cachedCT =
            hasCached ? static_cast<int32_t>(fOnMakeColorTypeAndSpaceResult->colorType()) : 0;
    const bool sameCS = hasCached && SkColorSpace::Equals(
                                             targetColorSpace.get(),
                                             fOnMakeColorTypeAndSpaceResult->colorSpace());
    if (skia_rs_image_lazy_color_type_and_space_cache_hit(
                hasCached, static_cast<int32_t>(targetColorType), cachedCT, sameCS)) {
        return fOnMakeColorTypeAndSpaceResult;
    }
    Validator validator(fSharedGenerator, &targetColorType, targetColorSpace);
    sk_sp<SkImage> result = validator ? sk_sp<SkImage>(new SkImage_Lazy(&validator)) : nullptr;
    if (result) {
        fOnMakeColorTypeAndSpaceResult = result;
    }
    return result;
}

sk_sp<SkImage> SkImage_Lazy::onReinterpretColorSpace(sk_sp<SkColorSpace> newCS) const {
    SkBitmap bitmap;
    if (bitmap.tryAllocPixels(this->imageInfo().makeColorSpace(std::move(newCS)))) {
        SkPixmap pixmap = bitmap.pixmap();
        pixmap.setColorSpace(this->refColorSpace());
        if (ScopedGenerator(fSharedGenerator)->getPixels(pixmap)) {
            bitmap.setImmutable();
            return SkImages::RasterFromBitmap(bitmap);
        }
    }
    return nullptr;
}

sk_sp<SkCachedData> SkImage_Lazy::getPlanes(
        const SkYUVAPixmapInfo::SupportedDataTypes& supportedDataTypes,
        SkYUVAPixmaps* yuvaPixmaps) const {
    ScopedGenerator generator(fSharedGenerator);

    sk_sp<SkCachedData> data(SkYUVPlanesCache::FindAndRef(generator->uniqueID(), yuvaPixmaps));

    if (data) {
        SkASSERT(yuvaPixmaps->isValid());
        SkASSERT(yuvaPixmaps->yuvaInfo().dimensions() == this->dimensions());
        return data;
    }
    SkYUVAPixmapInfo yuvaPixmapInfo;
    if (!generator->queryYUVAInfo(supportedDataTypes, &yuvaPixmapInfo) ||
        !skia_rs_image_lazy_yuva_dimensions_match(yuvaPixmapInfo.yuvaInfo().width(),
                                                  yuvaPixmapInfo.yuvaInfo().height(),
                                                  this->width(),
                                                  this->height())) {
        return nullptr;
    }
    data = SkResourceCache::NewCachedData(yuvaPixmapInfo.computeTotalBytes());
    SkYUVAPixmaps tempPixmaps =
            SkYUVAPixmaps::FromExternalMemory(yuvaPixmapInfo, data->writable_data());
    SkASSERT(tempPixmaps.isValid());
    if (!generator->getYUVAPlanes(tempPixmaps)) {
        return nullptr;
    }
    *yuvaPixmaps = tempPixmaps;
    SkYUVPlanesCache::Add(this->uniqueID(), data.get(), *yuvaPixmaps);
    return data;
}

void SkImage_Lazy::addUniqueIDListener(sk_sp<SkIDChangeListener> listener) const {
    fUniqueIDListeners.add(std::move(listener));
}

namespace SkImages {

sk_sp<SkImage> DeferredFromGenerator(std::unique_ptr<SkImageGenerator> generator) {
    SkImage_Lazy::Validator validator(
            SharedGenerator::Make(std::move(generator)), nullptr, nullptr);

    return validator ? sk_make_sp<SkImage_Lazy>(&validator) : nullptr;
}

}  // namespace SkImages

// ============================================================================
// src/image/SkImage_LazyFactories.cpp
// ============================================================================

namespace SkImages {

sk_sp<SkImage> DeferredFromPicture(sk_sp<SkPicture> picture,
                                   const SkISize& dimensions,
                                   const SkMatrix* matrix,
                                   const SkPaint* paint,
                                   BitDepth bitDepth,
                                   sk_sp<SkColorSpace> colorSpace) {
    return SkImage_Picture::Make(std::move(picture),
                                 dimensions,
                                 matrix,
                                 paint,
                                 bitDepth,
                                 std::move(colorSpace),
                                 {});
}

sk_sp<SkImage> DeferredFromPicture(sk_sp<SkPicture> picture,
                                   const SkISize& dimensions,
                                   const SkMatrix* matrix,
                                   const SkPaint* paint,
                                   BitDepth bitDepth,
                                   sk_sp<SkColorSpace> colorSpace,
                                   SkSurfaceProps props) {
    return SkImage_Picture::Make(std::move(picture),
                                 dimensions,
                                 matrix,
                                 paint,
                                 bitDepth,
                                 std::move(colorSpace),
                                 props);
}

}  // namespace SkImages

// ============================================================================
// src/image/SkImage_Picture.cpp
// ============================================================================

sk_sp<SkImage> SkImage_Picture::Make(sk_sp<SkPicture> picture,
                                     const SkISize& dimensions,
                                     const SkMatrix* matrix,
                                     const SkPaint* paint,
                                     SkImages::BitDepth bitDepth,
                                     sk_sp<SkColorSpace> colorSpace,
                                     SkSurfaceProps props) {
    auto gen = SkImageGenerators::MakeFromPicture(dimensions,
                                                  std::move(picture),
                                                  matrix,
                                                  paint,
                                                  bitDepth,
                                                  std::move(colorSpace),
                                                  props);

    SkImage_Lazy::Validator validator(SharedGenerator::Make(std::move(gen)), nullptr, nullptr);

    return validator ? sk_make_sp<SkImage_Picture>(&validator) : nullptr;
}

const SkSurfaceProps* SkImage_Picture::props() const {
    auto pictureIG = static_cast<SkPictureImageGenerator*>(this->generator()->fGenerator.get());
    return &pictureIG->fProps;
}

void SkImage_Picture::replay(SkCanvas* canvas) const {
    auto sharedGenerator = this->generator();
    SkAutoMutexExclusive mutex(sharedGenerator->fMutex);

    auto pictureIG = static_cast<SkPictureImageGenerator*>(sharedGenerator->fGenerator.get());
    canvas->clear(SkColors::kTransparent);
    canvas->drawPicture(
            pictureIG->fPicture, &pictureIG->fMatrix, SkOptAddressOrNull(pictureIG->fPaint));
}

sk_sp<SkImage> SkImage_Picture::onMakeSubset(SkRecorder*,
                                             const SkIRect& subset,
                                             RequiredProperties) const {
    auto sharedGenerator = this->generator();
    auto pictureIG = static_cast<SkPictureImageGenerator*>(sharedGenerator->fGenerator.get());

    SkiaRsPictureSubsetPlan plan{};
    if (!skia_rs_image_picture_plan_subset(
                &pictureIG->fMatrix, &subset, static_cast<int32_t>(this->colorType()), &plan)) {
        return nullptr;
    }

    return SkImage_Picture::Make(pictureIG->fPicture,
                                 plan.dimensions,
                                 &plan.matrix,
                                 SkOptAddressOrNull(pictureIG->fPaint),
                                 static_cast<SkImages::BitDepth>(plan.bit_depth),
                                 this->refColorSpace(),
                                 pictureIG->fProps);
}

bool SkImage_Picture::getImageKeyValues(
        uint32_t keyValues[SkTiledImageUtils::kNumImageKeyValues]) const {
    auto sharedGenerator = this->generator();
    SkAutoMutexExclusive mutex(sharedGenerator->fMutex);

    auto pictureIG = static_cast<SkPictureImageGenerator*>(sharedGenerator->fGenerator.get());
    const SkImageInfo& ii = sharedGenerator->getInfo();
    const bool isSRGB = ii.colorSpace() && ii.colorSpace()->isSRGB();
    const uint32_t pixelGeometry = this->props()->pixelGeometry();
    const uint32_t surfacePropFlags = this->props()->flags();

    SkASSERT(pixelGeometry <= 4);
    SkASSERT(surfacePropFlags < 8);
    SkASSERT(SkTFitsIn<uint32_t>(ii.width()));
    SkASSERT(SkTFitsIn<uint32_t>(ii.height()));

    if (!skia_rs_image_picture_key_values(pictureIG->fPaint.has_value(),
                                          isSRGB,
                                          &pictureIG->fMatrix,
                                          static_cast<int32_t>(ii.colorType()),
                                          pixelGeometry,
                                          surfacePropFlags,
                                          ii.width(),
                                          ii.height(),
                                          pictureIG->fPicture->uniqueID(),
                                          keyValues)) {
        return false;
    }
    SkASSERT(keyValues[1] != 0);
    return true;
}
