/*
 * Safe Rust bridge for:
 *   src/core/SkImageGenerator.cpp
 *   src/image/SkImage.cpp
 *   src/image/SkImage_Base.cpp
 *   src/image/SkImage_Raster.cpp
 *   src/image/SkImage_RasterFactories.cpp
 *   src/image/SkRescaleAndReadPixels.cpp
 * (Layer 3.4b skia-rs-image).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageGenerator.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixelRef.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRecorder.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkSize.h"
#include "include/core/SkSurface.h"
#include "include/core/SkSurfaceProps.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkMath.h"
#include "src/core/SkBitmapCache.h"
#include "src/core/SkColorSpacePriv.h"
#include "src/core/SkCompressedDataUtils.h"
#include "src/core/SkImageFilterTypes.h"
#include "src/core/SkImageFilter_Base.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkNextID.h"
#include "src/cpu/Recorder.h"
#include "src/image/SkImage_Base.h"
#include "src/image/SkImage_Raster.h"
#include "src/image/SkRescaleAndReadPixels.h"
#include "src/shaders/SkImageShader.h"

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>

class GrDirectContext;
class SkImageFilter;
class SkShader;
enum class SkTextureCompressionType;

namespace {

static constexpr size_t kSkiaRsRescaleMaxSteps = 32;

struct SkiaRsMakeRasterImagePlan {
    size_t row_bytes;
    size_t byte_size;
};

struct SkiaRsRasterFromBitmapPlan {
    int32_t action;
    size_t byte_size;
    bool bitmap_may_be_mutable;
};

struct SkiaRsRescaleStep {
    SkIRect src_rect;
    int32_t dst_width;
    int32_t dst_height;
    bool use_result_info;
    bool strict_constraint;
};

struct SkiaRsRescalePlan {
    int32_t sampling;
    bool needs_linearize;
    int32_t linearize_src_x;
    int32_t linearize_src_y;
    int32_t linearize_width;
    int32_t linearize_height;
    size_t step_count;
    SkiaRsRescaleStep steps[kSkiaRsRescaleMaxSteps];
    int32_t final_read_x;
    int32_t final_read_y;
    size_t final_row_bytes;
    size_t final_byte_size;
};

static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkiaRsMakeRasterImagePlan) == 2 * sizeof(size_t));
static_assert(sizeof(SkiaRsRescaleStep) == 28);

}  // namespace

extern "C" {

// SkImageGenerator
uint32_t skia_rs_image_resolve_unique_id(uint32_t unique_id);
bool skia_rs_image_generator_validate_get_pixels(int32_t width,
                                                 int32_t ct_raw,
                                                 bool has_pixels,
                                                 size_t row_bytes);

// SkImage & SkImage_Base
int32_t skia_rs_image_plan_make_scaled(int32_t src_w,
                                       int32_t src_h,
                                       int32_t src_ct_raw,
                                       int32_t src_at_raw,
                                       bool same_color_space,
                                       int32_t new_w,
                                       int32_t new_h,
                                       int32_t new_ct_raw,
                                       int32_t new_at_raw);
SkRect skia_rs_image_make_scaled_dst_rect(int32_t new_w, int32_t new_h);
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
bool skia_rs_image_scale_pixels_is_identity_size(int32_t src_w,
                                                 int32_t src_h,
                                                 int32_t dst_w,
                                                 int32_t dst_h);
bool skia_rs_image_is_alpha_only(int32_t ct_raw);
bool skia_rs_image_should_reinterpret_return_self(bool same_color_space, int32_t ct_raw);
bool skia_rs_image_plan_make_raster_image(int32_t width,
                                          int32_t height,
                                          int32_t ct_raw,
                                          SkiaRsMakeRasterImagePlan* out);
bool skia_rs_image_validate_subset(int32_t width, int32_t height, const SkIRect* subset);
bool skia_rs_image_should_invoke_on_make_with_mipmaps(bool has_mips, bool mips_valid_for_root);
SkIRect skia_rs_image_async_read_effective_src_rect(const SkIRect* orig_src_rect, bool peeked);

// SkImage_Raster & SkImage_RasterFactories
uint32_t skia_rs_image_raster_unique_id_from_bitmap(int32_t pr_w,
                                                    int32_t pr_h,
                                                    int32_t bm_w,
                                                    int32_t bm_h,
                                                    uint32_t bm_gen_id);
void skia_rs_image_raster_plan_from_bitmap(int32_t width,
                                           int32_t height,
                                           int32_t ct_raw,
                                           int32_t at_raw,
                                           size_t row_bytes,
                                           bool has_pixels,
                                           bool is_immutable,
                                           int32_t cpm_raw,
                                           SkiaRsRasterFromBitmapPlan* out);
void skia_rs_image_raster_copy_rect_bytes(uint8_t* dst,
                                          size_t dst_rb,
                                          const uint8_t* src,
                                          size_t src_rb,
                                          size_t copy_bytes_per_row,
                                          size_t height);
bool skia_rs_image_raster_can_share_legacy_pixel_ref(bool is_immutable,
                                                     bool is_temporarily_immutable);
bool skia_rs_image_raster_should_compose_alpha_shader(int32_t ct_raw, bool paint_has_shader);
bool skia_rs_image_raster_is_full_subset(int32_t bm_w, int32_t bm_h, const SkIRect* subset);
bool skia_rs_image_raster_validate_factory_args(int32_t width,
                                                int32_t height,
                                                int32_t ct_raw,
                                                int32_t at_raw,
                                                size_t row_bytes,
                                                size_t* out_min_size);
bool skia_rs_image_raster_validate_from_data(int32_t width,
                                             int32_t height,
                                             int32_t ct_raw,
                                             int32_t at_raw,
                                             size_t row_bytes,
                                             bool has_data,
                                             size_t data_size);
int32_t skia_rs_image_compressed_alpha_type(bool is_opaque);

// SkRescaleAndReadPixels
bool skia_rs_rescale_build_plan(const SkIRect* src_rect,
                                int32_t dst_width,
                                int32_t dst_height,
                                int32_t dst_ct_raw,
                                bool src_has_cs,
                                bool src_cs_is_linear,
                                uint8_t rescale_gamma_raw,
                                int32_t rescale_mode_raw,
                                SkiaRsRescalePlan* out);

}  // extern "C"

// ============================================================================
// src/core/SkImageGenerator.cpp
// ============================================================================

SkImageGenerator::SkImageGenerator(const SkImageInfo& info, uint32_t uniqueID)
        : fInfo(info), fUniqueID(skia_rs_image_resolve_unique_id(uniqueID)) {}

bool SkImageGenerator::getPixels(const SkImageInfo& info, void* pixels, size_t rowBytes) {
    if (!skia_rs_image_generator_validate_get_pixels(
                info.width(), static_cast<int32_t>(info.colorType()), pixels != nullptr,
                rowBytes)) {
        return false;
    }

    Options defaultOpts;
    return this->onGetPixels(info, pixels, rowBytes, defaultOpts);
}

bool SkImageGenerator::queryYUVAInfo(
        const SkYUVAPixmapInfo::SupportedDataTypes& supportedDataTypes,
        SkYUVAPixmapInfo* yuvaPixmapInfo) const {
    SkASSERT(yuvaPixmapInfo);

    return this->onQueryYUVAInfo(supportedDataTypes, yuvaPixmapInfo) &&
           yuvaPixmapInfo->isSupported(supportedDataTypes);
}

bool SkImageGenerator::getYUVAPlanes(const SkYUVAPixmaps& yuvaPixmaps) {
    return this->onGetYUVAPlanes(yuvaPixmaps);
}

// ============================================================================
// src/image/SkImage.cpp
// ============================================================================

SkImage::SkImage(const SkImageInfo& info, uint32_t uniqueID)
        : fInfo(info), fUniqueID(skia_rs_image_resolve_unique_id(uniqueID)) {
    SkASSERT(info.width() > 0);
    SkASSERT(info.height() > 0);
}

bool SkImage::peekPixels(SkPixmap* pm) const {
    SkPixmap tmp;
    if (!pm) {
        pm = &tmp;
    }
    return as_IB(this)->onPeekPixels(pm);
}

bool SkImage::readPixels(GrDirectContext* dContext,
                         const SkImageInfo& dstInfo,
                         void* dstPixels,
                         size_t dstRowBytes,
                         int srcX,
                         int srcY,
                         CachingHint chint) const {
    return as_IB(this)->onReadPixels(dContext, dstInfo, dstPixels, dstRowBytes, srcX, srcY, chint);
}

sk_sp<SkImage> SkImage::makeScaled(const SkImageInfo& newInfo,
                                   const SkSamplingOptions& sampling) const {
    return makeScaled(nullptr, newInfo, sampling, SkSurfaceProps{});
}

sk_sp<SkImage> SkImage::makeScaled(SkRecorder* recorder,
                                   const SkImageInfo& newInfo,
                                   const SkSamplingOptions& sampling) const {
    return makeScaled(recorder, newInfo, sampling, SkSurfaceProps{});
}

sk_sp<SkImage> SkImage::makeScaled(SkRecorder* recorder,
                                   const SkImageInfo& newInfo,
                                   const SkSamplingOptions& sampling,
                                   const SkSurfaceProps& props) const {
    const int32_t action = skia_rs_image_plan_make_scaled(
            fInfo.width(),
            fInfo.height(),
            static_cast<int32_t>(fInfo.colorType()),
            static_cast<int32_t>(fInfo.alphaType()),
            SkColorSpace::Equals(fInfo.colorSpace(), newInfo.colorSpace()),
            newInfo.width(),
            newInfo.height(),
            static_cast<int32_t>(newInfo.colorType()),
            static_cast<int32_t>(newInfo.alphaType()));
    if (action == 0) {
        return nullptr;
    }
    if (action == 1) {
        return sk_ref_sp(this);
    }

    auto surf = as_IB(this)->onMakeSurface(recorder, newInfo);
    if (!surf) {
        return nullptr;
    }

    SkPaint paint;
    paint.setBlendMode(SkBlendMode::kSrc);
    SkRect dst = skia_rs_image_make_scaled_dst_rect(newInfo.width(), newInfo.height());
    surf->getCanvas()->drawImageRect(this, dst, sampling, &paint);
    return surf->makeImageSnapshot();
}

#ifndef SK_IMAGE_READ_PIXELS_DISABLE_LEGACY_API
bool SkImage::readPixels(const SkImageInfo& dstInfo,
                         void* dstPixels,
                         size_t dstRowBytes,
                         int srcX,
                         int srcY,
                         CachingHint chint) const {
    auto dContext = as_IB(this)->directContext();
    return this->readPixels(dContext, dstInfo, dstPixels, dstRowBytes, srcX, srcY, chint);
}
#endif

void SkImage::asyncRescaleAndReadPixels(const SkImageInfo& info,
                                        const SkIRect& srcRect,
                                        RescaleGamma rescaleGamma,
                                        RescaleMode rescaleMode,
                                        ReadPixelsCallback callback,
                                        ReadPixelsContext context) const {
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
    as_IB(this)->onAsyncRescaleAndReadPixels(
            info, srcRect, rescaleGamma, rescaleMode, callback, context);
}

void SkImage::asyncRescaleAndReadPixelsYUV420(SkYUVColorSpace yuvColorSpace,
                                              sk_sp<SkColorSpace> dstColorSpace,
                                              const SkIRect& srcRect,
                                              const SkISize& dstSize,
                                              RescaleGamma rescaleGamma,
                                              RescaleMode rescaleMode,
                                              ReadPixelsCallback callback,
                                              ReadPixelsContext context) const {
    if (!skia_rs_image_validate_async_rescale_yuv420(
                this->width(), this->height(), &srcRect, &dstSize)) {
        callback(context, nullptr);
        return;
    }
    as_IB(this)->onAsyncRescaleAndReadPixelsYUV420(yuvColorSpace,
                                                   /*readAlpha=*/false,
                                                   std::move(dstColorSpace),
                                                   srcRect,
                                                   dstSize,
                                                   rescaleGamma,
                                                   rescaleMode,
                                                   callback,
                                                   context);
}

void SkImage::asyncRescaleAndReadPixelsYUVA420(SkYUVColorSpace yuvColorSpace,
                                               sk_sp<SkColorSpace> dstColorSpace,
                                               const SkIRect& srcRect,
                                               const SkISize& dstSize,
                                               RescaleGamma rescaleGamma,
                                               RescaleMode rescaleMode,
                                               ReadPixelsCallback callback,
                                               ReadPixelsContext context) const {
    if (!skia_rs_image_validate_async_rescale_yuv420(
                this->width(), this->height(), &srcRect, &dstSize)) {
        callback(context, nullptr);
        return;
    }
    as_IB(this)->onAsyncRescaleAndReadPixelsYUV420(yuvColorSpace,
                                                   /*readAlpha=*/true,
                                                   std::move(dstColorSpace),
                                                   srcRect,
                                                   dstSize,
                                                   rescaleGamma,
                                                   rescaleMode,
                                                   callback,
                                                   context);
}

bool SkImage::scalePixels(const SkPixmap& dst,
                          const SkSamplingOptions& sampling,
                          CachingHint chint) const {
    auto dContext = as_IB(this)->directContext();
    if (skia_rs_image_scale_pixels_is_identity_size(
                this->width(), this->height(), dst.width(), dst.height())) {
        return this->readPixels(dContext, dst, 0, 0, chint);
    }

    SkBitmap bm;
    if (as_IB(this)->getROPixels(dContext, &bm, chint)) {
        SkPixmap pmap;
        return bm.peekPixels(&pmap) && pmap.scalePixels(dst, sampling);
    }
    return false;
}

SkColorType SkImage::colorType() const { return fInfo.colorType(); }

SkAlphaType SkImage::alphaType() const { return fInfo.alphaType(); }

SkColorSpace* SkImage::colorSpace() const { return fInfo.colorSpace(); }

sk_sp<SkColorSpace> SkImage::refColorSpace() const { return fInfo.refColorSpace(); }

sk_sp<SkShader> SkImage::makeShader(const SkSamplingOptions& sampling, const SkMatrix& lm) const {
    return SkImageShader::Make(sk_ref_sp(const_cast<SkImage*>(this)),
                               SkTileMode::kClamp,
                               SkTileMode::kClamp,
                               sampling,
                               &lm);
}

sk_sp<SkShader> SkImage::makeShader(const SkSamplingOptions& sampling, const SkMatrix* lm) const {
    return SkImageShader::Make(sk_ref_sp(const_cast<SkImage*>(this)),
                               SkTileMode::kClamp,
                               SkTileMode::kClamp,
                               sampling,
                               lm);
}

sk_sp<SkShader> SkImage::makeShader(SkTileMode tmx,
                                    SkTileMode tmy,
                                    const SkSamplingOptions& sampling,
                                    const SkMatrix& lm) const {
    return SkImageShader::Make(sk_ref_sp(const_cast<SkImage*>(this)), tmx, tmy, sampling, &lm);
}

sk_sp<SkShader> SkImage::makeShader(SkTileMode tmx,
                                    SkTileMode tmy,
                                    const SkSamplingOptions& sampling,
                                    const SkMatrix* localMatrix) const {
    return SkImageShader::Make(
            sk_ref_sp(const_cast<SkImage*>(this)), tmx, tmy, sampling, localMatrix);
}

sk_sp<SkShader> SkImage::makeRawShader(SkTileMode tmx,
                                       SkTileMode tmy,
                                       const SkSamplingOptions& sampling,
                                       const SkMatrix& lm) const {
    return SkImageShader::MakeRaw(sk_ref_sp(const_cast<SkImage*>(this)), tmx, tmy, sampling, &lm);
}

sk_sp<SkShader> SkImage::makeRawShader(const SkSamplingOptions& sampling,
                                       const SkMatrix& lm) const {
    return SkImageShader::MakeRaw(sk_ref_sp(const_cast<SkImage*>(this)),
                                  SkTileMode::kClamp,
                                  SkTileMode::kClamp,
                                  sampling,
                                  &lm);
}

sk_sp<SkShader> SkImage::makeRawShader(const SkSamplingOptions& sampling,
                                       const SkMatrix* localMatrix) const {
    return SkImageShader::MakeRaw(sk_ref_sp(const_cast<SkImage*>(this)),
                                  SkTileMode::kClamp,
                                  SkTileMode::kClamp,
                                  sampling,
                                  localMatrix);
}

sk_sp<SkShader> SkImage::makeRawShader(SkTileMode tmx,
                                       SkTileMode tmy,
                                       const SkSamplingOptions& sampling,
                                       const SkMatrix* localMatrix) const {
    return SkImageShader::MakeRaw(
            sk_ref_sp(const_cast<SkImage*>(this)), tmx, tmy, sampling, localMatrix);
}

sk_sp<const SkData> SkImage::refEncodedData() const { return as_IB(this)->onRefEncoded(); }

bool SkImage::readPixels(GrDirectContext* dContext,
                         const SkPixmap& pmap,
                         int srcX,
                         int srcY,
                         CachingHint chint) const {
    return this->readPixels(
            dContext, pmap.info(), pmap.writable_addr(), pmap.rowBytes(), srcX, srcY, chint);
}

#ifndef SK_IMAGE_READ_PIXELS_DISABLE_LEGACY_API
bool SkImage::readPixels(const SkPixmap& pmap, int srcX, int srcY, CachingHint chint) const {
    auto dContext = as_IB(this)->directContext();
    return this->readPixels(dContext, pmap, srcX, srcY, chint);
}
#endif

bool SkImage::asLegacyBitmap(SkBitmap* bitmap, LegacyBitmapMode) const {
    auto dContext = as_IB(this)->directContext();
    return as_IB(this)->onAsLegacyBitmap(dContext, bitmap);
}

bool SkImage::isAlphaOnly() const {
    return skia_rs_image_is_alpha_only(static_cast<int32_t>(fInfo.colorType()));
}

sk_sp<SkImage> SkImage::reinterpretColorSpace(sk_sp<SkColorSpace> target) const {
    if (!target) {
        return nullptr;
    }

    SkColorSpace* colorSpace = this->colorSpace();
    if (!colorSpace) {
        colorSpace = sk_srgb_singleton();
    }
    if (skia_rs_image_should_reinterpret_return_self(
                SkColorSpace::Equals(colorSpace, target.get()),
                static_cast<int32_t>(fInfo.colorType()))) {
        return sk_ref_sp(const_cast<SkImage*>(this));
    }

    return as_IB(this)->onReinterpretColorSpace(std::move(target));
}

sk_sp<SkImage> SkImage::makeNonTextureImage(GrDirectContext* dContext) const {
    if (!this->isTextureBacked()) {
        return sk_ref_sp(const_cast<SkImage*>(this));
    }
    return this->makeRasterImage(dContext, kDisallow_CachingHint);
}

sk_sp<SkImage> SkImage::makeRasterImage(GrDirectContext* dContext, CachingHint chint) const {
    SkPixmap pm;
    if (this->peekPixels(&pm)) {
        return sk_ref_sp(const_cast<SkImage*>(this));
    }

    SkiaRsMakeRasterImagePlan plan{};
    if (!skia_rs_image_plan_make_raster_image(
                fInfo.width(), fInfo.height(), static_cast<int32_t>(fInfo.colorType()), &plan)) {
        return nullptr;
    }

    if (!dContext) {
        dContext = as_IB(this)->directContext();
    }
    sk_sp<SkData> data = SkData::MakeUninitialized(plan.byte_size);
    pm = {fInfo.makeColorSpace(nullptr), data->writable_data(), plan.row_bytes};
    if (!this->readPixels(dContext, pm, 0, 0, chint)) {
        return nullptr;
    }

    return SkImages::RasterFromData(fInfo, std::move(data), plan.row_bytes);
}

bool SkImage::hasMipmaps() const { return as_IB(this)->onHasMipmaps(); }

bool SkImage::isProtected() const { return as_IB(this)->onIsProtected(); }

sk_sp<SkImage> SkImage::withMipmaps(sk_sp<SkMipmap> mips) const {
    const bool validForRoot = mips ? mips->validForRootLevel(this->imageInfo()) : true;
    if (skia_rs_image_should_invoke_on_make_with_mipmaps(mips != nullptr, validForRoot)) {
        if (auto result = as_IB(this)->onMakeWithMipmaps(std::move(mips))) {
            return result;
        }
    }
    return sk_ref_sp(const_cast<SkImage*>(this));
}

sk_sp<SkImage> SkImage::withDefaultMipmaps() const { return this->withMipmaps(nullptr); }

// ============================================================================
// src/image/SkImage_Base.cpp
// ============================================================================

SkImage_Base::SkImage_Base(const SkImageInfo& info,
                           uint32_t uniqueID,
                           sk_sp<SkPixelStorage> storage)
        : SkImage(info, uniqueID), fAddedToRasterCache(false) {
    if (storage) {
        fPixelStorages.push_back(std::move(storage));
    }
}

SkImage_Base::~SkImage_Base() {
    if (fAddedToRasterCache.load()) {
        SkNotifyBitmapGenIDIsStale(this->uniqueID());
    }
}

void SkImage_Base::onAsyncRescaleAndReadPixels(const SkImageInfo& info,
                                               SkIRect origSrcRect,
                                               RescaleGamma rescaleGamma,
                                               RescaleMode rescaleMode,
                                               ReadPixelsCallback callback,
                                               ReadPixelsContext context) const {
    SkBitmap src;
    SkPixmap peek;
    bool peeked = this->peekPixels(&peek);
    if (peeked) {
        src.installPixels(peek);
    } else {
        auto dContext = as_IB(this)->directContext();
        src.setInfo(this->imageInfo().makeDimensions(origSrcRect.size()));
        src.allocPixels();
        if (!this->readPixels(dContext, src.pixmap(), origSrcRect.x(), origSrcRect.y())) {
            callback(context, nullptr);
            return;
        }
    }
    SkIRect srcRect = skia_rs_image_async_read_effective_src_rect(&origSrcRect, peeked);
    return SkRescaleAndReadPixels(src, info, srcRect, rescaleGamma, rescaleMode, callback, context);
}

bool SkImage_Base::onAsLegacyBitmap(GrDirectContext* dContext, SkBitmap* bitmap) const {
    SkImageInfo info = fInfo.makeColorType(kN32_SkColorType).makeColorSpace(nullptr);
    if (!bitmap->tryAllocPixels(info)) {
        return false;
    }

    if (!this->readPixels(
                dContext, bitmap->info(), bitmap->getPixels(), bitmap->rowBytes(), 0, 0)) {
        bitmap->reset();
        return false;
    }

    bitmap->setImmutable();
    return true;
}

sk_sp<SkImage> SkImage_Base::makeSubset(SkRecorder* recorder,
                                        const SkIRect& subset,
                                        RequiredProperties requiredProps) const {
    if (!skia_rs_image_validate_subset(this->width(), this->height(), &subset)) {
        return nullptr;
    }

    return this->onMakeSubset(recorder, subset, requiredProps);
}

void SkImage_Base::onAsyncRescaleAndReadPixelsYUV420(SkYUVColorSpace,
                                                     bool readAlpha,
                                                     sk_sp<SkColorSpace> dstColorSpace,
                                                     SkIRect srcRect,
                                                     SkISize dstSize,
                                                     RescaleGamma,
                                                     RescaleMode,
                                                     ReadPixelsCallback callback,
                                                     ReadPixelsContext context) const {
    callback(context, nullptr);
}

sk_sp<SkImage> SkImage_Base::makeColorSpace(SkRecorder* recorder,
                                            sk_sp<SkColorSpace> target,
                                            RequiredProperties props) const {
    return this->makeColorTypeAndColorSpace(recorder, this->colorType(), std::move(target), props);
}

// ============================================================================
// src/image/SkImage_Raster.cpp
// ============================================================================

static void release_data(void* addr, void* context) {
    SkData* data = static_cast<SkData*>(context);
    data->unref();
}

static uint32_t raster_unique_id_from_bm(const SkBitmap& bm) {
    SkASSERT(bm.pixelRef());
    SkISize dim = SkISize::Make(bm.pixelRef()->width(), bm.pixelRef()->height());
    SkASSERT(dim != bm.dimensions() || bm.pixelRefOrigin().isZero());
    return skia_rs_image_raster_unique_id_from_bitmap(
            dim.width(), dim.height(), bm.width(), bm.height(), bm.getGenerationID());
}

SkImage_Raster::SkImage_Raster(const SkImageInfo& info,
                               sk_sp<SkData> data,
                               size_t rowBytes,
                               sk_sp<SkMipmap> mips,
                               uint32_t id)
        : SkImage_Base(info, id, /*backingStorage=*/nullptr), fMips(mips) {
    void* addr = const_cast<void*>(data->data());

    fBitmap.installPixels(info, addr, rowBytes, release_data, data.release());
    fBitmap.setImmutable();
    fPixelStorages.push_back(sk_ref_sp(fBitmap.pixelRef()));
}

SkImage_Raster::SkImage_Raster(const SkBitmap& bm, sk_sp<SkMipmap> mips, bool bitmapMayBeMutable)
        : SkImage_Base(bm.info(), raster_unique_id_from_bm(bm), sk_ref_sp(bm.pixelRef()))
        , fBitmap(bm)
        , fMips(mips) {
    SkASSERT(bitmapMayBeMutable || fBitmap.isImmutable());
}

SkImage_Raster::~SkImage_Raster() {}

bool SkImage_Raster::onReadPixels(GrDirectContext*,
                                  const SkImageInfo& dstInfo,
                                  void* dstPixels,
                                  size_t dstRowBytes,
                                  int srcX,
                                  int srcY,
                                  CachingHint) const {
    SkBitmap shallowCopy(fBitmap);
    return shallowCopy.readPixels(dstInfo, dstPixels, dstRowBytes, srcX, srcY);
}

bool SkImage_Raster::onPeekPixels(SkPixmap* pm) const { return fBitmap.peekPixels(pm); }

bool SkImage_Raster::getROPixels(GrDirectContext*, SkBitmap* dst, CachingHint) const {
    *dst = fBitmap;
    return true;
}

sk_sp<SkSurface> SkImage_Raster::onMakeSurface(SkRecorder* recorder,
                                               const SkImageInfo& info) const {
    if (!recorder) {
        recorder = skcpu::Recorder::TODO();
    }
    const SkSurfaceProps* props = nullptr;
    constexpr size_t rowBytes = 0;
    return recorder->cpuRecorder()->makeBitmapSurface(info, rowBytes, props);
}

static SkBitmap copy_bitmap_subset(const SkBitmap& orig, const SkIRect& subset) {
    SkImageInfo info = orig.info().makeDimensions(subset.size());
    SkBitmap bitmap;
    if (!bitmap.tryAllocPixels(info)) {
        return {};
    }

    void* dst = bitmap.getPixels();
    void* src = orig.getAddr(subset.x(), subset.y());
    if (!dst || !src) {
        SkDEBUGFAIL("SkImage_Raster::onMakeSubset with nullptr src or dst");
        return {};
    }

    skia_rs_image_raster_copy_rect_bytes(static_cast<uint8_t*>(dst),
                                         bitmap.rowBytes(),
                                         static_cast<const uint8_t*>(src),
                                         orig.rowBytes(),
                                         bitmap.rowBytes(),
                                         static_cast<size_t>(subset.height()));

    bitmap.setImmutable();
    return bitmap;
}

static sk_sp<SkMipmap> copy_mipmaps(const SkBitmap& src, SkMipmap* srcMips) {
    if (!srcMips) {
        return nullptr;
    }

    sk_sp<SkMipmap> dst;
    dst.reset(SkMipmap::Build(src.pixmap(),
                              /* factoryProc= */ nullptr,
                              /* computeContents= */ false));
    if (!dst) {
        return nullptr;
    }
    for (int i = 0; i < dst->countLevels(); ++i) {
        SkMipmap::Level srcLevel, dstLevel;
        srcMips->getLevel(i, &srcLevel);
        dst->getLevel(i, &dstLevel);
        srcLevel.fPixmap.readPixels(dstLevel.fPixmap);
    }

    return dst;
}

sk_sp<SkImage> SkImage_Raster::onMakeSubset(SkRecorder*,
                                            const SkIRect& subset,
                                            RequiredProperties requiredProperties) const {
    sk_sp<SkImage> img;

    if (requiredProperties.fMipmapped) {
        bool fullCopy =
                skia_rs_image_raster_is_full_subset(fBitmap.width(), fBitmap.height(), &subset);

        sk_sp<SkMipmap> mips = fullCopy ? copy_mipmaps(fBitmap, fMips.get()) : nullptr;

        SkBitmap tmpSubset;
        if (!fBitmap.extractSubset(&tmpSubset, subset)) {
            return nullptr;
        }

        sk_sp<SkImage> tmp(
                new SkImage_Raster(tmpSubset, /*mips=*/nullptr, /* bitmapMayBeMutable= */ true));

        SkASSERT(!mips || mips->validForRootLevel(tmp->imageInfo()));
        img = tmp->withMipmaps(std::move(mips));
    } else {
        SkBitmap copy = copy_bitmap_subset(fBitmap, subset);
        if (!copy.isNull()) {
            img = SkImages::RasterFromBitmap(copy);
        }
    }

    return img;
}

sk_sp<SkImage_Raster> SkImage_Raster::MakeFromBitmap(const SkBitmap& bm,
                                                     SkCopyPixelsMode cpm,
                                                     sk_sp<SkMipmap> mips) {
    SkiaRsRasterFromBitmapPlan plan{};
    skia_rs_image_raster_plan_from_bitmap(bm.width(),
                                          bm.height(),
                                          static_cast<int32_t>(bm.colorType()),
                                          static_cast<int32_t>(bm.alphaType()),
                                          bm.rowBytes(),
                                          bm.getPixels() != nullptr,
                                          bm.isImmutable(),
                                          static_cast<int32_t>(cpm),
                                          &plan);

    if (plan.action == 0) {
        return nullptr;
    }
    if (plan.action == 1) {
        sk_sp<SkData> data(SkData::MakeWithCopy(bm.getPixels(), plan.byte_size));
        return sk_sp<SkImage_Raster>(new SkImage_Raster(
                bm.info(), std::move(data), bm.rowBytes(), std::move(mips), kNeedNewImageUniqueID));
    }
    return sk_sp<SkImage_Raster>(
            new SkImage_Raster(bm, std::move(mips), plan.bitmap_may_be_mutable));
}

bool SkImage_Raster::onAsLegacyBitmap(GrDirectContext*, SkBitmap* bitmap) const {
    const bool isImmutable = fBitmap.isImmutable();
    const bool isTempImmutable = isImmutable && fBitmap.pixelRef()->isTemporarilyImmutable();
    if (skia_rs_image_raster_can_share_legacy_pixel_ref(isImmutable, isTempImmutable)) {
        SkIPoint origin = fBitmap.pixelRefOrigin();
        bitmap->setInfo(fBitmap.info(), fBitmap.rowBytes());
        bitmap->setPixelRef(sk_ref_sp(fBitmap.pixelRef()), origin.x(), origin.y());
        return true;
    }
    return this->SkImage_Base::onAsLegacyBitmap(nullptr, bitmap);
}

sk_sp<SkShader> SkImage_Raster::makeShaderForPaint(const SkPaint& paint,
                                                   SkTileMode tmx,
                                                   SkTileMode tmy,
                                                   const SkSamplingOptions& sampling,
                                                   const SkMatrix* localMatrix) {
    auto s = SkImageShader::Make(sk_ref_sp<SkImage>(this), tmx, tmy, sampling, localMatrix);
    if (!s) {
        return nullptr;
    }
    if (skia_rs_image_raster_should_compose_alpha_shader(
                static_cast<int32_t>(this->colorType()), paint.getShader() != nullptr)) {
        s = SkShaders::Blend(SkBlendMode::kDstIn, paint.refShader(), std::move(s));
    }
    return s;
}

sk_sp<SkImage> SkImage_Raster::onMakeWithMipmaps(sk_sp<SkMipmap> mips) const {
    if (!mips) {
        mips.reset(SkMipmap::Build(fBitmap.pixmap(), nullptr));
    }
    return SkImage_Raster::MakeFromBitmap(fBitmap, SkCopyPixelsMode::kAlways, std::move(mips));
}

sk_sp<SkImage> SkImage_Raster::makeColorTypeAndColorSpace(SkRecorder*,
                                                          SkColorType targetColorType,
                                                          sk_sp<SkColorSpace> targetColorSpace,
                                                          RequiredProperties) const {
    SkPixmap src;
    SkAssertResult(fBitmap.peekPixels(&src));

    SkBitmap dst;
    if (!dst.tryAllocPixels(
                fBitmap.info().makeColorType(targetColorType).makeColorSpace(targetColorSpace))) {
        return nullptr;
    }

    SkAssertResult(dst.writePixels(src));
    dst.setImmutable();
    return SkImages::RasterFromBitmap(dst);
}

sk_sp<SkImage> SkImage_Raster::onReinterpretColorSpace(sk_sp<SkColorSpace> newCS) const {
    SkPixmap pixmap = fBitmap.pixmap();
    pixmap.setColorSpace(std::move(newCS));
    return SkImages::RasterFromPixmapCopy(pixmap);
}

// ============================================================================
// src/image/SkImage_RasterFactories.cpp
// ============================================================================

namespace SkImages {

sk_sp<SkImage> RasterFromBitmap(const SkBitmap& bm) {
    if (!bm.pixelRef()) {
        return nullptr;
    }

    return SkImage_Raster::MakeFromBitmap(bm, SkCopyPixelsMode::kIfMutable);
}

sk_sp<SkImage> RasterFromPixmapCopy(const SkPixmap& pmap) {
    size_t size = 0;
    const SkImageInfo& info = pmap.info();
    if (!skia_rs_image_raster_validate_factory_args(info.width(),
                                                    info.height(),
                                                    static_cast<int32_t>(info.colorType()),
                                                    static_cast<int32_t>(info.alphaType()),
                                                    pmap.rowBytes(),
                                                    &size) ||
        !pmap.addr()) {
        return nullptr;
    }

    sk_sp<SkData> data(SkData::MakeWithCopy(pmap.addr(), size));
    return sk_make_sp<SkImage_Raster>(
            pmap.info(), std::move(data), pmap.rowBytes(), /*mips=*/nullptr, kNeedNewImageUniqueID);
}

sk_sp<SkImage> RasterFromData(const SkImageInfo& info, sk_sp<SkData> data, size_t rowBytes) {
    if (!skia_rs_image_raster_validate_from_data(info.width(),
                                                 info.height(),
                                                 static_cast<int32_t>(info.colorType()),
                                                 static_cast<int32_t>(info.alphaType()),
                                                 rowBytes,
                                                 data != nullptr,
                                                 data ? data->size() : 0)) {
        return nullptr;
    }

    return sk_make_sp<SkImage_Raster>(
            info, std::move(data), rowBytes, /*mips=*/nullptr, kNeedNewImageUniqueID);
}

sk_sp<SkImage> MakeWithFilter(sk_sp<SkImage> src,
                              const SkImageFilter* filter,
                              const SkIRect& subset,
                              const SkIRect& clipBounds,
                              SkIRect* outSubset,
                              SkIPoint* offset) {
    if (!src || !filter) {
        return nullptr;
    }

    sk_sp<skif::Backend> backend = skif::MakeRasterBackend({}, src->colorType());
    return as_IFB(filter)->makeImageWithFilter(std::move(backend),
                                               std::move(src),
                                               subset,
                                               clipBounds,
                                               outSubset,
                                               offset);
}

sk_sp<SkImage> RasterFromCompressedTextureData(sk_sp<SkData> data,
                                               int width,
                                               int height,
                                               SkTextureCompressionType type) {
    size_t expectedSize = SkCompressedFormatDataSize(type, {width, height}, false);
    if (!data || data->size() < expectedSize) {
        return nullptr;
    }

    SkAlphaType at = static_cast<SkAlphaType>(
            skia_rs_image_compressed_alpha_type(SkTextureCompressionTypeIsOpaque(type)));
    SkImageInfo ii = SkImageInfo::MakeN32(width, height, at);

    if (!skia_rs_image_raster_validate_factory_args(ii.width(),
                                                    ii.height(),
                                                    static_cast<int32_t>(ii.colorType()),
                                                    static_cast<int32_t>(ii.alphaType()),
                                                    ii.minRowBytes(),
                                                    nullptr)) {
        return nullptr;
    }

    SkBitmap bitmap;
    if (!bitmap.tryAllocPixels(ii)) {
        return nullptr;
    }

    if (!SkDecompress(std::move(data), {width, height}, type, &bitmap)) {
        return nullptr;
    }

    bitmap.setImmutable();
    return RasterFromBitmap(bitmap);
}

sk_sp<SkImage> RasterFromPixmap(const SkPixmap& pmap,
                                RasterReleaseProc proc,
                                ReleaseContext ctx) {
    size_t size = 0;
    const SkImageInfo& info = pmap.info();
    if (!skia_rs_image_raster_validate_factory_args(info.width(),
                                                    info.height(),
                                                    static_cast<int32_t>(info.colorType()),
                                                    static_cast<int32_t>(info.alphaType()),
                                                    pmap.rowBytes(),
                                                    &size) ||
        !pmap.addr()) {
        return nullptr;
    }

    sk_sp<SkData> data(SkData::MakeWithProc(pmap.addr(), size, proc, ctx));
    return sk_make_sp<SkImage_Raster>(
            pmap.info(), std::move(data), pmap.rowBytes(), /*mips=*/nullptr, kNeedNewImageUniqueID);
}

}  // namespace SkImages

// ============================================================================
// src/image/SkRescaleAndReadPixels.cpp
// ============================================================================

void SkRescaleAndReadPixels(SkBitmap bmp,
                            const SkImageInfo& resultInfo,
                            const SkIRect& srcRect,
                            SkImage::RescaleGamma rescaleGamma,
                            SkImage::RescaleMode rescaleMode,
                            SkImage::ReadPixelsCallback callback,
                            SkImage::ReadPixelsContext context) {
    SkiaRsRescalePlan plan{};
    const SkColorSpace* srcCS = bmp.info().colorSpace();
    if (!skia_rs_rescale_build_plan(&srcRect,
                                    resultInfo.width(),
                                    resultInfo.height(),
                                    static_cast<int32_t>(resultInfo.colorType()),
                                    srcCS != nullptr,
                                    srcCS ? srcCS->gammaIsLinear() : false,
                                    static_cast<uint8_t>(rescaleGamma),
                                    static_cast<int32_t>(rescaleMode),
                                    &plan)) {
        callback(context, nullptr);
        return;
    }

    SkPaint paint;
    paint.setBlendMode(SkBlendMode::kSrc);

    SkSamplingOptions sampling;
    if (plan.sampling == 1) {
        sampling = SkSamplingOptions(SkFilterMode::kLinear);
    } else if (plan.sampling == 2) {
        sampling = SkSamplingOptions({1.0f / 3, 1.0f / 3});
    }

    sk_sp<SkSurface> tempSurf;
    sk_sp<SkImage> srcImage;
    if (plan.needs_linearize) {
        auto cs = bmp.info().colorSpace()->makeLinearGamma();
        auto ii = SkImageInfo::Make(plan.linearize_width,
                                    plan.linearize_height,
                                    kRGBA_F16_SkColorType,
                                    bmp.info().alphaType(),
                                    std::move(cs));
        auto linearSurf = SkSurfaces::Raster(ii);
        if (!linearSurf) {
            callback(context, nullptr);
            return;
        }
        linearSurf->getCanvas()->drawImage(
                bmp.asImage().get(), -plan.linearize_src_x, -plan.linearize_src_y, sampling,
                &paint);
        tempSurf = std::move(linearSurf);
        srcImage = tempSurf->makeImageSnapshot();
    } else {
        srcImage = SkImages::RasterFromPixmap(bmp.pixmap(), nullptr, nullptr);
    }

    for (size_t i = 0; i < plan.step_count; ++i) {
        const SkiaRsRescaleStep& step = plan.steps[i];
        SkImageInfo ii = step.use_result_info
                                 ? resultInfo
                                 : srcImage->imageInfo().makeWH(step.dst_width, step.dst_height);
        auto next = SkSurfaces::Raster(ii);
        if (!next) {
            callback(context, nullptr);
            return;
        }
        SkCanvas::SrcRectConstraint constraint = step.strict_constraint
                                                         ? SkCanvas::kStrict_SrcRectConstraint
                                                         : SkCanvas::kFast_SrcRectConstraint;
        next->getCanvas()->drawImageRect(srcImage.get(),
                                         SkRect::Make(step.src_rect),
                                         SkRect::MakeIWH(step.dst_width, step.dst_height),
                                         sampling,
                                         &paint,
                                         constraint);
        tempSurf = std::move(next);
        srcImage = tempSurf->makeImageSnapshot();
    }

    std::unique_ptr<char[]> data(new char[plan.final_byte_size]);
    SkPixmap pm(resultInfo, data.get(), plan.final_row_bytes);
    if (srcImage->readPixels(nullptr, pm, plan.final_read_x, plan.final_read_y)) {
        class Result : public SkImage::AsyncReadResult {
        public:
            Result(std::unique_ptr<const char[]> data, size_t rowBytes)
                    : fData(std::move(data)), fRowBytes(rowBytes) {}
            int count() const override { return 1; }
            const void* data(int i) const override { return fData.get(); }
            size_t rowBytes(int i) const override { return fRowBytes; }

        private:
            std::unique_ptr<const char[]> fData;
            size_t fRowBytes;
        };
        callback(context, std::make_unique<Result>(std::move(data), plan.final_row_bytes));
    } else {
        callback(context, nullptr);
    }
}
