/*
 * Safe Rust bridge for:
 *   src/core/SkSpecialImage.cpp
 * (Layer 3.4a skia-rs-bitmap).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkImage.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPoint.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkShader.h"
#include "include/private/SkAssert.h"
#include "src/core/SkNextID.h"
#include "src/core/SkSpecialImage.h"
#include "src/image/SkImage_Base.h"
#include "src/shaders/SkImageShader.h"

#include <cstddef>
#include <cstdint>

static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkRect) == 16);
static_assert(sizeof(SkMatrix) == 40);

extern "C" {
bool skia_rs_special_image_ganesh_should_downgrade_sampling(bool is_exact_fit,
                                                            int32_t max_aniso,
                                                            bool use_cubic,
                                                            int32_t mipmap_mode);
SkRect skia_rs_special_image_draw_dst_rect(float x, float y, const SkIRect* subset);
void skia_rs_special_image_shader_subset_matrix(const SkIRect* subset,
                                                const SkMatrix* lm,
                                                SkMatrix* out);
SkIRect skia_rs_special_image_copy_from_raster_subset(const SkIRect* subset);
}  // extern "C"

SkSpecialImage::SkSpecialImage(const SkIRect& subset,
                               uint32_t uniqueID,
                               const SkColorInfo& colorInfo,
                               const SkSurfaceProps& props)
    : fSubset(subset)
    , fUniqueID(kNeedNewImageUniqueID_SpecialImage == uniqueID ? SkNextID::ImageID() : uniqueID)
    , fColorInfo(colorInfo)
    , fProps(props) {}

void SkSpecialImage::draw(SkCanvas* canvas,
                          SkScalar x, SkScalar y,
                          const SkSamplingOptions& sampling,
                          const SkPaint* paint, bool strict) const {
    const SkIRect& sub = this->subset();
    SkRect dst = skia_rs_special_image_draw_dst_rect(x, y, &sub);

    canvas->drawImageRect(this->asImage(), SkRect::Make(sub), dst,
                          sampling, paint, strict ? SkCanvas::kStrict_SrcRectConstraint
                                                  : SkCanvas::kFast_SrcRectConstraint);
}

SkSamplingOptions SkSpecialImage::ganeshSafeSampling(const SkSamplingOptions& sampling) const {
    if (skia_rs_special_image_ganesh_should_downgrade_sampling(
                this->isExactFit(),
                sampling.maxAniso,
                sampling.useCubic,
                static_cast<int32_t>(sampling.mipmap))) {
        return SkSamplingOptions(SkFilterMode::kLinear);
    }
    return sampling;
}

sk_sp<SkShader> SkSpecialImage::asShader(SkTileMode tileMode,
                                         const SkSamplingOptions& sampling,
                                         const SkMatrix& lm,
                                         bool strict) const {
    const SkSamplingOptions safeSampling = this->isGaneshBacked()
            ? this->ganeshSafeSampling(sampling)
            : sampling;

    SkMatrix subsetOrigin;
    skia_rs_special_image_shader_subset_matrix(&this->subset(), &lm, &subsetOrigin);

    if (strict) {
        const SkRect subset = SkRect::Make(this->subset());
        return SkImageShader::MakeSubset(
                this->asImage(), subset, tileMode, tileMode, safeSampling, &subsetOrigin);
    } else {
        return this->asImage()->makeShader(tileMode, tileMode, safeSampling, subsetOrigin);
    }
}

class SkSpecialImage_Raster final : public SkSpecialImage {
public:
    SkSpecialImage_Raster(const SkIRect& subset, const SkBitmap& bm, const SkSurfaceProps& props)
            : SkSpecialImage(subset, bm.getGenerationID(), bm.info().colorInfo(), props)
            , fBitmap(bm) {
        SkASSERT(bm.pixelRef());
        SkASSERT(fBitmap.getPixels());
    }

    bool getROPixels(SkBitmap* bm) const {
        return fBitmap.extractSubset(bm, this->subset());
    }

    SkISize backingStoreDimensions() const override { return fBitmap.dimensions(); }

    size_t getSize() const override { return fBitmap.computeByteSize(); }

    sk_sp<SkImage> asImage() const override { return fBitmap.asImage(); }

    sk_sp<SkSpecialImage> onMakeBackingStoreSubset(const SkIRect& subset) const override {
        return SkSpecialImages::MakeFromRaster(subset, fBitmap, this->props());
    }

    sk_sp<SkShader> asShader(SkTileMode tileMode,
                             const SkSamplingOptions& sampling,
                             const SkMatrix& lm,
                             bool strict) const override {
        if (strict) {
            SkBitmap subsetBM;
            if (!this->getROPixels(&subsetBM)) {
                return nullptr;
            }
            return subsetBM.makeShader(tileMode, tileMode, sampling, lm);
        } else {
            SkMatrix subsetOrigin;
            skia_rs_special_image_shader_subset_matrix(&this->subset(), &lm, &subsetOrigin);
            return fBitmap.makeShader(tileMode, tileMode, sampling, subsetOrigin);
        }
    }

private:
    SkBitmap fBitmap;
};

namespace SkSpecialImages {

sk_sp<SkSpecialImage> MakeFromRaster(const SkIRect& subset,
                                     const SkBitmap& bm,
                                     const SkSurfaceProps& props) {
    SkASSERT(bm.bounds().contains(subset));

    if (!bm.pixelRef()) {
        return nullptr;
    }
    return sk_make_sp<SkSpecialImage_Raster>(subset, bm, props);
}

sk_sp<SkSpecialImage> CopyFromRaster(const SkIRect& subset,
                                     const SkBitmap& bm,
                                     const SkSurfaceProps& props) {
    SkASSERT(bm.bounds().contains(subset));

    if (!bm.pixelRef()) {
        return nullptr;
    }
    SkBitmap tmp;
    SkImageInfo info = bm.info().makeDimensions(subset.size());
    if (!tmp.tryAllocPixels(info)) {
        return nullptr;
    }
    if (!bm.readPixels(tmp.info(), tmp.getPixels(), tmp.rowBytes(), subset.x(), subset.y())) {
        return nullptr;
    }
    SkIRect copySubset = skia_rs_special_image_copy_from_raster_subset(&subset);
    return sk_make_sp<SkSpecialImage_Raster>(copySubset, tmp, props);
}

sk_sp<SkSpecialImage> MakeFromRaster(const SkIRect& subset,
                                     sk_sp<SkImage> image,
                                     const SkSurfaceProps& props) {
    if (!image || subset.isEmpty()) {
        return nullptr;
    }

    SkASSERT(image->bounds().contains(subset));
    SkASSERT(!image->isTextureBacked());

    SkBitmap bm;
    if (as_IB(image)->getROPixels(nullptr, &bm)) {
        return MakeFromRaster(subset, bm, props);
    }
    return nullptr;
}

bool AsBitmap(const SkSpecialImage* img, SkBitmap* result) {
    if (!img || img->isGaneshBacked() || img->isGraphiteBacked()) {
        return false;
    }
    auto rasterImg = static_cast<const SkSpecialImage_Raster*>(img);
    return rasterImg->getROPixels(result);
}

}  // namespace SkSpecialImages
