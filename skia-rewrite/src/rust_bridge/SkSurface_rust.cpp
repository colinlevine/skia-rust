/*
 * Safe Rust bridge for:
 *   src/image/SkSurface.cpp
 *   src/image/SkSurface_Base.cpp
 *   src/image/SkSurface_Null.cpp
 *   src/image/SkSurface_Raster.cpp
 * (Layer 3.4b skia-rs-image).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkCapabilities.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMallocPixelRef.h"
#include "include/core/SkPixelRef.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSize.h"
#include "include/core/SkSurface.h"
#include "include/core/SkSurfaceProps.h"
#include "include/private/SkAssert.h"
#include "include/private/SkMath.h"
#include "include/private/SkPixelStorage.h"
#include "include/private/SkTemplates.h"
#include "include/utils/SkNoDrawCanvas.h"
#include "src/capture/SkCaptureCanvas.h"
#include "src/capture/SkCaptureManager.h"
#include "src/core/SkBitmapDevice.h"
#include "src/core/SkDevice.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkSurfacePriv.h"
#include "src/cpu/Recorder.h"
#include "src/image/SkImage_Raster.h"
#include "src/image/SkRescaleAndReadPixels.h"
#include "src/image/SkSurface_Base.h"
#include "src/image/SkSurface_Raster.h"

#include <cstddef>
#include <cstdint>
#include <utility>

class GrBackendSemaphore;
class GrRecordingContext;
class GrSurfaceCharacterization;
class SkPaint;
struct SkSamplingOptions;
namespace skgpu {
namespace graphite {
class Recorder;
}
}  // namespace skgpu

namespace {

struct SkiaRsSurfacePropsData {
    uint32_t flags;
    int32_t pixel_geometry;
    float text_contrast;
    float text_gamma;
};

struct SkiaRsSnapshotBoundsPlan {
    int32_t action;
    SkIRect clipped_bounds;
};

struct SkiaRsRasterSnapshotPlan {
    int32_t copy_mode;
    bool set_temporarily_immutable;
};

static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkSurfaceProps) == 16);
static_assert(sizeof(SkiaRsSurfacePropsData) == sizeof(SkSurfaceProps));
static_assert(sizeof(SkiaRsSnapshotBoundsPlan) == 20);
static_assert(sizeof(SkiaRsRasterSnapshotPlan) == 8);

}  // namespace

extern "C" {

// Shared validation from skia-rs-image
bool skia_rs_image_validate_async_rescale_rgba(int32_t src_w,
                                               int32_t src_h,
                                               const SkIRect* src_rect,
                                               int32_t dst_w,
                                               int32_t dst_h,
                                               int32_t dst_ct_raw,
                                               int32_t dst_at_raw);
bool skia_rs_image_validate_async_rescale_yuv420(int32_t src_w,
                                                 int32_t src_h,
                                                 const SkIRect* src_rect,
                                                 const SkISize* dst_size);
SkIRect skia_rs_image_async_read_effective_src_rect(const SkIRect* orig_src_rect, bool peeked);

// Surface FFI
SkiaRsSurfacePropsData skia_rs_surface_props_make(uint32_t flags,
                                                  int32_t pixel_geometry,
                                                  float text_contrast,
                                                  float text_gamma);
uint32_t skia_rs_surface_next_generation_id();
void skia_rs_surface_plan_snapshot_bounds(int32_t surf_width,
                                          int32_t surf_height,
                                          const SkIRect* src_bounds,
                                          SkiaRsSnapshotBoundsPlan* out);
int32_t skia_rs_surface_plan_write_pixels(int32_t surf_width,
                                          int32_t surf_height,
                                          bool has_addr,
                                          int32_t pm_width,
                                          int32_t pm_height,
                                          int32_t x,
                                          int32_t y);
int32_t skia_rs_surface_plan_about_to_draw(bool has_cached_image,
                                           bool cached_image_unique,
                                           int32_t mode_raw);
bool skia_rs_surface_outstanding_image_snapshot(bool has_cached_image, bool cached_image_unique);
bool skia_rs_surface_validate_null_dims(int32_t width, int32_t height);
bool skia_rs_surface_validate_raster_info(int32_t width,
                                          int32_t height,
                                          int32_t ct_raw,
                                          int32_t at_raw,
                                          size_t row_bytes);
void skia_rs_surface_plan_raster_snapshot(bool we_own_pixels, SkiaRsRasterSnapshotPlan* out);
void skia_rs_surface_copy_backing_pixels(uint8_t* dst, const uint8_t* src, size_t byte_size);

}  // extern "C"

// ============================================================================
// src/image/SkSurface.cpp
// ============================================================================

SkSurfaceProps::SkSurfaceProps() : SkSurfaceProps(0, kUnknown_SkPixelGeometry) {}

SkSurfaceProps::SkSurfaceProps(uint32_t flags, SkPixelGeometry pg)
        : SkSurfaceProps(flags, pg, SK_GAMMA_CONTRAST, SK_GAMMA_EXPONENT) {}

SkSurfaceProps::SkSurfaceProps(uint32_t flags,
                               SkPixelGeometry pg,
                               SkScalar textContrast,
                               SkScalar textGamma) {
    SkiaRsSurfacePropsData data = skia_rs_surface_props_make(
            flags, static_cast<int32_t>(pg), textContrast, textGamma);
    fFlags = data.flags;
    fPixelGeometry = static_cast<SkPixelGeometry>(data.pixel_geometry);
    fTextContrast = data.text_contrast;
    fTextGamma = data.text_gamma;
}

SkSurface::SkSurface(int width, int height, const SkSurfaceProps* props)
        : fProps(SkSurfacePropsCopyOrDefault(props)), fWidth(width), fHeight(height) {
    SkASSERT(fWidth > 0);
    SkASSERT(fHeight > 0);
    fGenerationID = 0;
}

SkSurface::SkSurface(const SkImageInfo& info, const SkSurfaceProps* props)
        : fProps(SkSurfacePropsCopyOrDefault(props))
        , fWidth(info.width())
        , fHeight(info.height()) {
    SkASSERT(fWidth > 0);
    SkASSERT(fHeight > 0);
    fGenerationID = 0;
}

uint32_t SkSurface::generationID() {
    if (0 == fGenerationID) {
        fGenerationID = asSB(this)->newGenerationID();
    }
    return fGenerationID;
}

void SkSurface::notifyContentWillChange(ContentChangeMode mode) {
    sk_ignore_unused_variable(asSB(this)->aboutToDraw(mode));
}

SkCanvas* SkSurface::getCanvas() { return asSB(this)->getCachedCanvas(); }

sk_sp<const SkCapabilities> SkSurface::capabilities() { return asSB(this)->onCapabilities(); }

sk_sp<SkImage> SkSurface::makeImageSnapshot() { return asSB(this)->refCachedImage(); }

sk_sp<SkImage> SkSurface::makeImageSnapshot(const SkIRect& srcBounds) {
    SkiaRsSnapshotBoundsPlan plan{};
    skia_rs_surface_plan_snapshot_bounds(fWidth, fHeight, &srcBounds, &plan);
    if (plan.action == 0) {
        return nullptr;
    }
    SkASSERT(!plan.clipped_bounds.isEmpty());
    if (plan.action == 1) {
        return this->makeImageSnapshot();
    }
    return asSB(this)->onNewImageSnapshot(&plan.clipped_bounds);
}

sk_sp<SkImage> SkSurface::makeTemporaryImage() { return asSB(this)->onMakeTemporaryImage(); }

sk_sp<SkSurface> SkSurface::makeSurface(const SkImageInfo& info) {
    return asSB(this)->onNewSurface(info);
}

sk_sp<SkSurface> SkSurface::makeSurface(int width, int height) {
    return this->makeSurface(this->imageInfo().makeWH(width, height));
}

void SkSurface::draw(SkCanvas* canvas,
                     SkScalar x,
                     SkScalar y,
                     const SkSamplingOptions& sampling,
                     const SkPaint* paint) {
    asSB(this)->onDraw(canvas, x, y, sampling, paint);
}

bool SkSurface::peekPixels(SkPixmap* pmap) { return this->getCanvas()->peekPixels(pmap); }

bool SkSurface::readPixels(const SkPixmap& pm, int srcX, int srcY) {
    return this->getCanvas()->readPixels(pm, srcX, srcY);
}

bool SkSurface::readPixels(const SkImageInfo& dstInfo,
                           void* dstPixels,
                           size_t dstRowBytes,
                           int srcX,
                           int srcY) {
    return this->readPixels({dstInfo, dstPixels, dstRowBytes}, srcX, srcY);
}

bool SkSurface::readPixels(const SkBitmap& bitmap, int srcX, int srcY) {
    SkPixmap pm;
    return bitmap.peekPixels(&pm) && this->readPixels(pm, srcX, srcY);
}

void SkSurface::asyncRescaleAndReadPixels(const SkImageInfo& info,
                                          const SkIRect& srcRect,
                                          RescaleGamma rescaleGamma,
                                          RescaleMode rescaleMode,
                                          ReadPixelsCallback callback,
                                          ReadPixelsContext context) {
    if (!skia_rs_image_validate_async_rescale_rgba(this->width(),
                                                   this->height(),
                                                   &srcRect,
                                                   info.width(),
                                                   info.height(),
                                                   static_cast<int32_t>(info.colorType()),
                                                   static_cast<int32_t>(info.alphaType()))) {
        callback(context, nullptr);
        return;
    }
    asSB(this)->onAsyncRescaleAndReadPixels(
            info, srcRect, rescaleGamma, rescaleMode, callback, context);
}

void SkSurface::asyncRescaleAndReadPixelsYUV420(SkYUVColorSpace yuvColorSpace,
                                                sk_sp<SkColorSpace> dstColorSpace,
                                                const SkIRect& srcRect,
                                                const SkISize& dstSize,
                                                RescaleGamma rescaleGamma,
                                                RescaleMode rescaleMode,
                                                ReadPixelsCallback callback,
                                                ReadPixelsContext context) {
    if (!skia_rs_image_validate_async_rescale_yuv420(
                this->width(), this->height(), &srcRect, &dstSize)) {
        callback(context, nullptr);
        return;
    }
    asSB(this)->onAsyncRescaleAndReadPixelsYUV420(yuvColorSpace,
                                                  /*readAlpha=*/false,
                                                  std::move(dstColorSpace),
                                                  srcRect,
                                                  dstSize,
                                                  rescaleGamma,
                                                  rescaleMode,
                                                  callback,
                                                  context);
}

void SkSurface::asyncRescaleAndReadPixelsYUVA420(SkYUVColorSpace yuvColorSpace,
                                                 sk_sp<SkColorSpace> dstColorSpace,
                                                 const SkIRect& srcRect,
                                                 const SkISize& dstSize,
                                                 RescaleGamma rescaleGamma,
                                                 RescaleMode rescaleMode,
                                                 ReadPixelsCallback callback,
                                                 ReadPixelsContext context) {
    if (!skia_rs_image_validate_async_rescale_yuv420(
                this->width(), this->height(), &srcRect, &dstSize)) {
        callback(context, nullptr);
        return;
    }
    asSB(this)->onAsyncRescaleAndReadPixelsYUV420(yuvColorSpace,
                                                  /*readAlpha=*/true,
                                                  std::move(dstColorSpace),
                                                  srcRect,
                                                  dstSize,
                                                  rescaleGamma,
                                                  rescaleMode,
                                                  callback,
                                                  context);
}

void SkSurface::writePixels(const SkPixmap& pmap, int x, int y) {
    const int32_t action = skia_rs_surface_plan_write_pixels(
            this->width(), this->height(), pmap.addr() != nullptr, pmap.width(), pmap.height(), x,
            y);
    if (action == 0) {
        return;
    }
    ContentChangeMode mode =
            (action == 2) ? kDiscard_ContentChangeMode : kRetain_ContentChangeMode;
    if (!asSB(this)->aboutToDraw(mode)) {
        return;
    }
    asSB(this)->onWritePixels(pmap, x, y);
}

void SkSurface::writePixels(const SkBitmap& src, int x, int y) {
    SkPixmap pm;
    if (src.peekPixels(&pm)) {
        this->writePixels(pm, x, y);
    }
}

GrRecordingContext* SkSurface::recordingContext() const {
    return asConstSB(this)->onGetRecordingContext();
}

skgpu::graphite::Recorder* SkSurface::recorder() const { return asConstSB(this)->onGetRecorder(); }

SkRecorder* SkSurface::baseRecorder() const { return asConstSB(this)->onGetBaseRecorder(); }

bool SkSurface::wait(int numSemaphores,
                     const GrBackendSemaphore* waitSemaphores,
                     bool deleteSemaphoresAfterWait) {
    return asSB(this)->onWait(numSemaphores, waitSemaphores, deleteSemaphoresAfterWait);
}

bool SkSurface::characterize(GrSurfaceCharacterization* characterization) const {
    return asConstSB(this)->onCharacterize(characterization);
}

bool SkSurface::isCompatible(const GrSurfaceCharacterization& characterization) const {
    return asConstSB(this)->onIsCompatible(characterization);
}

// ============================================================================
// src/image/SkSurface_Base.cpp
// ============================================================================

SkSurface_Base::SkSurface_Base(int width,
                               int height,
                               const SkSurfaceProps* props,
                               sk_sp<SkPixelStorage> storage)
        : SkSurface(width, height, props), fPixelStorage(std::move(storage)) {}

SkSurface_Base::SkSurface_Base(const SkImageInfo& info,
                               const SkSurfaceProps* props,
                               sk_sp<SkPixelStorage> storage)
        : SkSurface(info, props), fPixelStorage(std::move(storage)) {}

SkSurface_Base::~SkSurface_Base() {
    if (fCachedCanvas) {
        fCachedCanvas->setSurfaceBase(nullptr);
        if (auto recorder = this->baseRecorder()) {
            recorder->deregisterCaptureCanvas(fCachedCanvas);
        }
        fCachedCanvas = nullptr;
    }
}

GrRecordingContext* SkSurface_Base::onGetRecordingContext() const { return nullptr; }

skgpu::graphite::Recorder* SkSurface_Base::onGetRecorder() const { return nullptr; }

SkRecorder* SkSurface_Base::onGetBaseRecorder() const { return nullptr; }

void SkSurface_Base::onDraw(SkCanvas* canvas,
                            SkScalar x,
                            SkScalar y,
                            const SkSamplingOptions& sampling,
                            const SkPaint* paint) {
    auto image = this->makeTemporaryImage();
    if (image) {
        canvas->drawImage(image.get(), x, y, sampling, paint);
    }
}

void SkSurface_Base::onAsyncRescaleAndReadPixels(const SkImageInfo& info,
                                                 SkIRect origSrcRect,
                                                 SkSurface::RescaleGamma rescaleGamma,
                                                 RescaleMode rescaleMode,
                                                 SkSurface::ReadPixelsCallback callback,
                                                 SkSurface::ReadPixelsContext context) {
    SkBitmap src;
    SkPixmap peek;
    bool peeked = this->peekPixels(&peek);
    if (peeked) {
        src.installPixels(peek);
    } else {
        src.setInfo(this->imageInfo().makeDimensions(origSrcRect.size()));
        src.allocPixels();
        if (!this->readPixels(src, origSrcRect.x(), origSrcRect.y())) {
            callback(context, nullptr);
            return;
        }
    }
    SkIRect srcRect = skia_rs_image_async_read_effective_src_rect(&origSrcRect, peeked);
    return SkRescaleAndReadPixels(
            src, info, srcRect, rescaleGamma, rescaleMode, callback, context);
}

void SkSurface_Base::onAsyncRescaleAndReadPixelsYUV420(SkYUVColorSpace yuvColorSpace,
                                                       bool readAlpha,
                                                       sk_sp<SkColorSpace> dstColorSpace,
                                                       SkIRect srcRect,
                                                       SkISize dstSize,
                                                       RescaleGamma rescaleGamma,
                                                       RescaleMode,
                                                       ReadPixelsCallback callback,
                                                       ReadPixelsContext context) {
    callback(context, nullptr);
}

bool SkSurface_Base::outstandingImageSnapshot() const {
    return skia_rs_surface_outstanding_image_snapshot(
            fCachedImage != nullptr, fCachedImage ? fCachedImage->unique() : false);
}

bool SkSurface_Base::aboutToDraw(ContentChangeMode mode) {
    this->dirtyGenerationID();

    SkASSERT(!fOwnedBaseCanvas || fOwnedBaseCanvas->getSurface() == this);

    const bool hasCached = fCachedImage != nullptr;
    const bool unique = hasCached && fCachedImage->unique();
    const int32_t action =
            skia_rs_surface_plan_about_to_draw(hasCached, unique, static_cast<int32_t>(mode));

    switch (action) {
        case 0:  // NoAction
            break;
        case 1:  // Discard
            this->onDiscard();
            break;
        case 2:  // DropAndRestoreMutability
            fCachedImage.reset();
            this->onRestoreBackingMutability();
            break;
        case 3:  // CopyOnWriteAndDrop
            if (!this->onCopyOnWrite(mode)) {
                return false;
            }
            fCachedImage.reset();
            break;
        default:
            SkUNREACHABLE;
    }
    return true;
}

uint32_t SkSurface_Base::newGenerationID() {
    SkASSERT(!fOwnedBaseCanvas || fOwnedBaseCanvas->getSurface() == this);
    return skia_rs_surface_next_generation_id();
}

sk_sp<const SkCapabilities> SkSurface_Base::onCapabilities() {
    return SkCapabilities::RasterBackend();
}

void SkSurface_Base::createCaptureBreakpoint() {
    if (this->baseRecorder()) {
        this->baseRecorder()->createCaptureBreakpoint(this);
    }
}

// ============================================================================
// src/image/SkSurface_Null.cpp
// ============================================================================

class SkNullSurface : public SkSurface_Base {
public:
    SkNullSurface(int width, int height) : SkSurface_Base(width, height, nullptr, nullptr) {}

    SkSurface_Base::Type type() const override { return SkSurface_Base::Type::kNull; }

protected:
    SkCanvas* onNewCanvas() override { return new SkNoDrawCanvas(this->width(), this->height()); }
    sk_sp<SkSurface> onNewSurface(const SkImageInfo& info) override {
        return SkSurfaces::Null(info.width(), info.height());
    }
    sk_sp<SkImage> onNewImageSnapshot(const SkIRect* subsetOrNull) override { return nullptr; }
    void onWritePixels(const SkPixmap&, int x, int y) override {}
    void onDraw(SkCanvas*, SkScalar, SkScalar, const SkSamplingOptions&, const SkPaint*) override {}
    bool onCopyOnWrite(ContentChangeMode) override { return true; }
    sk_sp<const SkCapabilities> onCapabilities() override {
        return SkCapabilities::RasterBackend();
    }
    SkImageInfo imageInfo() const override {
        return SkImageInfo::MakeUnknown(this->width(), this->height());
    }
};

namespace SkSurfaces {

sk_sp<SkSurface> Null(int width, int height) {
    if (!skia_rs_surface_validate_null_dims(width, height)) {
        return nullptr;
    }
    return sk_sp<SkSurface>(new SkNullSurface(width, height));
}

}  // namespace SkSurfaces

// ============================================================================
// src/image/SkSurface_Raster.cpp
// ============================================================================

bool SkSurfaceValidateRasterInfo(const SkImageInfo& info, size_t rowBytes) {
    return skia_rs_surface_validate_raster_info(info.width(),
                                                info.height(),
                                                static_cast<int32_t>(info.colorType()),
                                                static_cast<int32_t>(info.alphaType()),
                                                rowBytes);
}

SkSurface_Raster::SkSurface_Raster(const SkImageInfo& info,
                                   void* pixels,
                                   size_t rb,
                                   void (*releaseProc)(void* pixels, void* context),
                                   void* context,
                                   const SkSurfaceProps* props)
        : SkSurface_Raster(
                  skcpu::Recorder::TODO(), info, pixels, rb, releaseProc, context, props) {}

SkSurface_Raster::SkSurface_Raster(const SkImageInfo& info,
                                   sk_sp<SkPixelRef> pr,
                                   const SkSurfaceProps* props)
        : SkSurface_Raster(skcpu::Recorder::TODO(), info, pr, props) {}

SkSurface_Raster::SkSurface_Raster(skcpu::Recorder* recorder,
                                   const SkImageInfo& info,
                                   void* pixels,
                                   size_t rowBytes,
                                   void (*releaseProc)(void* pixels, void* context),
                                   void* context,
                                   const SkSurfaceProps* props)
        : SkSurface_Base(info, props, nullptr), fRecorder(recorder) {
    fBitmap.installPixels(info, pixels, rowBytes, releaseProc, context);
    fPixelStorage = sk_ref_sp(fBitmap.pixelRef());
    if (fPixelStorage) {
        fPixelStorage->setProvenance(SkPixelStorage::Provenance::ClientRequest());
    }
    fWeOwnThePixels = false;  // We are "Direct"
}

SkSurface_Raster::SkSurface_Raster(skcpu::Recorder* recorder,
                                   const SkImageInfo& info,
                                   sk_sp<SkPixelRef> pr,
                                   const SkSurfaceProps* props)
        : SkSurface_Base(pr->width(), pr->height(), props, pr), fRecorder(recorder) {
    fBitmap.setInfo(info, pr->rowBytes());
    fBitmap.setPixelRef(std::move(pr), 0, 0);
    fWeOwnThePixels = true;
}

SkCanvas* SkSurface_Raster::onNewCanvas() {
    SkASSERT(fRecorder);
    return new SkCanvas(sk_make_sp<SkBitmapDevice>(fRecorder, fBitmap, this->props()));
}

sk_sp<SkSurface> SkSurface_Raster::onNewSurface(const SkImageInfo& info) {
    SkASSERT(fRecorder);
    return fRecorder->makeBitmapSurface(info, 0, &this->props());
}

void SkSurface_Raster::onDraw(SkCanvas* canvas,
                              SkScalar x,
                              SkScalar y,
                              const SkSamplingOptions& sampling,
                              const SkPaint* paint) {
    canvas->drawImage(fBitmap.asImage().get(), x, y, sampling, paint);
}

sk_sp<SkImage> SkSurface_Raster::onNewImageSnapshot(const SkIRect* subset) {
    if (subset) {
        SkASSERT(SkIRect::MakeWH(fBitmap.width(), fBitmap.height()).contains(*subset));
        SkBitmap dst;
        dst.allocPixels(fBitmap.info().makeDimensions(subset->size()));
        SkAssertResult(fBitmap.readPixels(dst.pixmap(), subset->left(), subset->top()));
        dst.setImmutable();
        return dst.asImage();
    }

    SkiaRsRasterSnapshotPlan plan{};
    skia_rs_surface_plan_raster_snapshot(fWeOwnThePixels, &plan);
    if (plan.set_temporarily_immutable) {
        if (SkPixelRef* pr = fBitmap.pixelRef()) {
            pr->setTemporarilyImmutable();
        }
    }

    return SkImage_Raster::MakeFromBitmap(
            fBitmap, static_cast<SkCopyPixelsMode>(plan.copy_mode));
}

void SkSurface_Raster::onWritePixels(const SkPixmap& src, int x, int y) {
    fBitmap.writePixels(src, x, y);
}

void SkSurface_Raster::onRestoreBackingMutability() {
    SkASSERT(!this->hasCachedImage());
    if (SkPixelRef* pr = fBitmap.pixelRef()) {
        pr->restoreMutability();
    }
}

bool SkSurface_Raster::onCopyOnWrite(ContentChangeMode mode) {
    sk_sp<SkImage> cached(this->refCachedImage());
    SkASSERT(cached);
    SkASSERT(as_IB(cached)->isRasterBacked());
    if (static_cast<SkImage_Raster*>(cached.get())->getPixelRef() == fBitmap.pixelRef()) {
        SkASSERT(fWeOwnThePixels);
        if (kDiscard_ContentChangeMode == mode) {
            if (!fBitmap.tryAllocPixels()) {
                return false;
            }
        } else {
            SkBitmap prev(fBitmap);
            if (!fBitmap.tryAllocPixels()) {
                return false;
            }
            SkASSERT(prev.info() == fBitmap.info());
            SkASSERT(prev.rowBytes() == fBitmap.rowBytes());
            skia_rs_surface_copy_backing_pixels(static_cast<uint8_t*>(fBitmap.getPixels()),
                                                static_cast<const uint8_t*>(prev.getPixels()),
                                                fBitmap.computeByteSize());
        }

        SkASSERT(this->getCachedCanvas());
        SkBitmapDevice* bmDev = static_cast<SkBitmapDevice*>(this->getCachedCanvas()->rootDevice());
        bmDev->replaceBitmapBackendForRasterSurface(fBitmap);
    }
    return true;
}

sk_sp<const SkCapabilities> SkSurface_Raster::onCapabilities() {
    return SkCapabilities::RasterBackend();
}

SkRecorder* SkSurface_Raster::onGetBaseRecorder() const { return fRecorder; }

namespace SkSurfaces {

sk_sp<SkSurface> WrapPixels(const SkImageInfo& info,
                            void* pixels,
                            size_t rowBytes,
                            PixelsReleaseProc releaseProc,
                            void* context,
                            const SkSurfaceProps* props) {
    if (nullptr == releaseProc) {
        context = nullptr;
    }
    if (!SkSurfaceValidateRasterInfo(info, rowBytes)) {
        return nullptr;
    }
    if (nullptr == pixels) {
        return nullptr;
    }

    return sk_make_sp<SkSurface_Raster>(info, pixels, rowBytes, releaseProc, context, props);
}

sk_sp<SkSurface> WrapPixels(const SkImageInfo& info,
                            void* pixels,
                            size_t rowBytes,
                            const SkSurfaceProps* props) {
    return WrapPixels(info, pixels, rowBytes, nullptr, nullptr, props);
}

sk_sp<SkSurface> Raster(const SkImageInfo& info, size_t rowBytes, const SkSurfaceProps* props) {
    if (!SkSurfaceValidateRasterInfo(info)) {
        return nullptr;
    }

    sk_sp<SkPixelRef> pr = SkMallocPixelRef::MakeAllocate(info, rowBytes);
    if (!pr) {
        return nullptr;
    }
    if (rowBytes) {
        SkASSERT(pr->rowBytes() == rowBytes);
    }
    pr->setProvenance(SkPixelStorage::Provenance::ClientRequest());
    return sk_make_sp<SkSurface_Raster>(info, std::move(pr), props);
}

}  // namespace SkSurfaces

namespace skcpu {

sk_sp<SkSurface> Recorder::makeBitmapSurface(const SkImageInfo& imageInfo,
                                             const SkSurfaceProps* surfaceProps) {
    return this->makeBitmapSurface(imageInfo, 0, surfaceProps);
}

sk_sp<SkSurface> Recorder::makeBitmapSurface(const SkImageInfo& imageInfo,
                                             size_t rowBytes,
                                             const SkSurfaceProps* surfaceProps) {
    if (!SkSurfaceValidateRasterInfo(imageInfo)) {
        return nullptr;
    }

    sk_sp<SkPixelRef> pr = SkMallocPixelRef::MakeAllocate(imageInfo, rowBytes);
    if (!pr) {
        return nullptr;
    }
    if (rowBytes) {
        SkASSERT(pr->rowBytes() == rowBytes);
    }
    pr->setProvenance(SkPixelStorage::Provenance::ClientRequest());

    return sk_make_sp<SkSurface_Raster>(this, imageInfo, std::move(pr), surfaceProps);
}

}  // namespace skcpu
