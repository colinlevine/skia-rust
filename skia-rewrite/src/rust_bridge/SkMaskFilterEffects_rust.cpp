/*
 * Safe Rust bridge for:
 *   src/effects/SkEmbossMask.cpp
 *   src/effects/SkEmbossMaskFilter.cpp
 *   src/effects/SkShaderMaskFilterImpl.cpp
 *   src/effects/SkTableMaskFilter.cpp
 * (Phase 1 Layer 4.3 skia-rs-effects, GN target :skia).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkBlurTypes.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkFlattenable.h"
#include "include/core/SkImageFilter.h"
#include "include/core/SkMaskFilter.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPoint.h"
#include "include/core/SkPoint3.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkScalar.h"
#include "include/core/SkShader.h"
#include "include/core/SkTypes.h"
#include "include/effects/SkImageFilters.h"
#include "include/effects/SkShaderMaskFilter.h"
#include "include/effects/SkTableMaskFilter.h"
#include "include/private/SkAlign.h"
#include "include/private/SkFloatingPoint.h"
#include "src/core/SkBlurMask.h"
#include "src/core/SkMask.h"
#include "src/core/SkMaskFilterBase.h"
#include "src/core/SkReadBuffer.h"
#include "src/core/SkWriteBuffer.h"
#include "src/effects/SkEmbossMask.h"
#include "src/effects/SkEmbossMaskFilter.h"
#include "src/effects/SkShaderMaskFilterImpl.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <utility>

static_assert(sizeof(SkEmbossMaskFilter::Light) == 16);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkIPoint) == 8);

extern "C" {
bool skia_rs_emboss_make_light(float blur_sigma,
                               SkEmbossMaskFilter::Light light,
                               SkEmbossMaskFilter::Light* out_light);
SkEmbossMaskFilter::Light skia_rs_emboss_transform_light(SkEmbossMaskFilter::Light light,
                                                         const SkMatrix* matrix);
SkIPoint skia_rs_emboss_margin(float sigma);
void skia_rs_emboss_mask(uint8_t* planes,
                         size_t plane_size,
                         int32_t width,
                         int32_t height,
                         size_t row_bytes,
                         SkEmbossMaskFilter::Light light);

void skia_rs_table_make_gamma(uint8_t* table, float gamma);
void skia_rs_table_make_clip(uint8_t* table, uint8_t min, uint8_t max);
void skia_rs_table_apply_mask(const uint8_t* src,
                              size_t src_rb,
                              uint8_t* dst,
                              size_t dst_rb,
                              size_t width,
                              size_t height,
                              const uint8_t* table);
void skia_rs_mask_rect_copy_bytes(uint8_t* dst,
                                  size_t dst_rb,
                                  const uint8_t* src,
                                  size_t src_rb,
                                  size_t copy_bytes,
                                  size_t rows);
}  // extern "C"

// ============================================================================
// SkEmbossMask (src/effects/SkEmbossMask.cpp)
// ============================================================================

void SkEmbossMask::Emboss(SkMaskBuilder* mask, const SkEmbossMaskFilter::Light& light) {
    SkASSERT(mask->fFormat == SkMask::k3D_Format);
    const size_t planeSize = mask->computeImageSize();
    skia_rs_emboss_mask(mask->image(),
                        planeSize,
                        mask->fBounds.width(),
                        mask->fBounds.height(),
                        mask->fRowBytes,
                        light);
}

// ============================================================================
// SkEmbossMaskFilter (src/effects/SkEmbossMaskFilter.cpp)
// ============================================================================

sk_sp<SkMaskFilter> SkEmbossMaskFilter::Make(SkScalar blurSigma, const Light& light) {
    Light newLight;
    if (!skia_rs_emboss_make_light(blurSigma, light, &newLight)) {
        return nullptr;
    }
    return sk_sp<SkMaskFilter>(new SkEmbossMaskFilter(blurSigma, newLight));
}

SkEmbossMaskFilter::SkEmbossMaskFilter(SkScalar blurSigma, const Light& light)
    : fLight(light), fBlurSigma(blurSigma) {
    SkASSERT(fBlurSigma > 0);
    SkASSERT(SkIsFinite(fLight.fDirection, 3));
}

SkMask::Format SkEmbossMaskFilter::getFormat() const {
    return SkMask::k3D_Format;
}

bool SkEmbossMaskFilter::filterMask(SkMaskBuilder* dst, const SkMask& src,
                                    const SkMatrix& matrix, SkIPoint* margin) const {
    if (src.fFormat != SkMask::kA8_Format) {
        return false;
    }

    SkScalar sigma = matrix.mapRadius(fBlurSigma);
    if (!SkBlurMask::BoxBlur(dst, src, sigma, kInner_SkBlurStyle)) {
        return false;
    }

    dst->format() = SkMask::k3D_Format;
    if (margin) {
        *margin = skia_rs_emboss_margin(sigma);
    }

    if (src.fImage == nullptr) {
        return true;
    }

    {
        uint8_t* alphaPlane = dst->image();
        size_t totalSize = dst->computeTotalImageSize();
        if (totalSize == 0) {
            return false;
        }
        size_t planeSize = dst->computeImageSize();
        SkASSERT(planeSize != 0);
        dst->image() = SkMaskBuilder::AllocImage(totalSize);
        memcpy(dst->image(), alphaPlane, planeSize);
        SkMaskBuilder::FreeImage(alphaPlane);
    }

    Light light = skia_rs_emboss_transform_light(fLight, &matrix);
    SkEmbossMask::Emboss(dst, light);

    // Restore original alpha
    memcpy(dst->image(), src.fImage, src.computeImageSize());
    return true;
}

sk_sp<SkFlattenable> SkEmbossMaskFilter::CreateProc(SkReadBuffer& buffer) {
    Light light;
    if (buffer.readByteArray(&light, sizeof(Light))) {
        light.fPad = 0;
        const SkScalar sigma = buffer.readScalar();
        return Make(sigma, light);
    }
    return nullptr;
}

void SkEmbossMaskFilter::flatten(SkWriteBuffer& buffer) const {
    Light tmpLight = fLight;
    tmpLight.fPad = 0;
    buffer.writeByteArray(&tmpLight, sizeof(tmpLight));
    buffer.writeScalar(fBlurSigma);
}

std::pair<sk_sp<SkImageFilter>, bool> SkEmbossMaskFilter::asImageFilter(
        const SkMatrix&, const SkPaint& paint) const {
    sk_sp<SkImageFilter> coverageBlurred = SkImageFilters::Blur(fBlurSigma, fBlurSigma, nullptr);

    sk_sp<SkShader> srcShader = SkShaders::Color(paint.getColor4f(), /*cs=*/nullptr);
    if (paint.getShader()) {
        srcShader = SkShaders::Blend(SkBlendMode::kDstIn, paint.refShader(), std::move(srcShader));
    }
    srcShader = srcShader->makeWithColorFilter(paint.refColorFilter());
    sk_sp<SkImageFilter> srcColor = SkImageFilters::Shader(
            std::move(srcShader), paint.isDither() ? SkImageFilters::Dither::kYes
                                                   : SkImageFilters::Dither::kNo);

    float ambientf = fLight.fAmbient / 255.f;
    SkColor4f ambientColor = {ambientf, ambientf, ambientf, 1};
    sk_sp<SkImageFilter> ambient = SkImageFilters::Shader(SkShaders::Color(ambientColor, nullptr));

    SkPoint3 lightDirection = SkPoint3::Make(fLight.fDirection[0],
                                             fLight.fDirection[1],
                                             fLight.fDirection[2]);
    const float surfaceScale = -255.f / 32.f;

    sk_sp<SkImageFilter> diffuseCF = SkImageFilters::DistantLitDiffuse(
            lightDirection, SK_ColorWHITE, surfaceScale, 1, coverageBlurred);
    sk_sp<SkImageFilter> ambientdiffuse = SkImageFilters::Blend(
            SkBlendMode::kPlus, diffuseCF, ambient);
    sk_sp<SkImageFilter> ambientdiffuseBlend = SkImageFilters::Blend(
            SkBlendMode::kModulate, srcColor, ambientdiffuse);

    float shininess = static_cast<float>((fLight.fSpecular >> 4) + 1);
    sk_sp<SkImageFilter> specular = LegacySpecular(
            lightDirection, SK_ColorWHITE, surfaceScale, 1, shininess, coverageBlurred);

    sk_sp<SkImageFilter> finalFilter = SkImageFilters::Blend(
            SkBlendMode::kPlus, ambientdiffuseBlend, specular);
    return {SkImageFilters::Blend(SkBlendMode::kDstIn, finalFilter, nullptr), true};
}

// ============================================================================
// SkShaderMaskFilterImpl (src/effects/SkShaderMaskFilterImpl.cpp)
// ============================================================================

sk_sp<SkFlattenable> SkShaderMaskFilterImpl::CreateProc(SkReadBuffer& buffer) {
    return SkShaderMaskFilter::Make(buffer.readShader());
}

void SkShaderMaskFilterImpl::flatten(SkWriteBuffer& buffer) const {
    buffer.writeFlattenable(fShader.get());
}

bool SkShaderMaskFilterImpl::filterMask(SkMaskBuilder* dst, const SkMask& src, const SkMatrix& ctm,
                                        SkIPoint* margin) const {
    if (src.fFormat != SkMask::kA8_Format) {
        return false;
    }

    if (margin) {
        margin->set(0, 0);
    }
    dst->bounds() = src.fBounds;
    dst->rowBytes() = src.fBounds.width();
    dst->format() = SkMask::kA8_Format;

    if (src.fImage == nullptr) {
        dst->image() = nullptr;
        return true;
    }
    size_t size = dst->computeImageSize();
    if (0 == size) {
        return false;
    }

    dst->image() = SkMaskBuilder::AllocImage(size);
    skia_rs_mask_rect_copy_bytes(dst->image(),
                                 dst->fRowBytes,
                                 src.fImage,
                                 src.fRowBytes,
                                 static_cast<size_t>(src.fBounds.width()) * sizeof(uint8_t),
                                 static_cast<size_t>(src.fBounds.height()));

    SkBitmap bitmap;
    if (!bitmap.installPixels(SkImageInfo::MakeA8(dst->fBounds.width(), dst->fBounds.height()),
                              dst->image(),
                              dst->fRowBytes)) {
        return false;
    }

    SkPaint paint;
    paint.setShader(fShader);
    paint.setBlendMode(SkBlendMode::kSrcIn);

    SkCanvas canvas(bitmap);
    canvas.translate(-SkIntToScalar(dst->fBounds.fLeft), -SkIntToScalar(dst->fBounds.fTop));
    canvas.concat(ctm);
    canvas.drawPaint(paint);
    return true;
}

std::pair<sk_sp<SkImageFilter>, bool> SkShaderMaskFilterImpl::asImageFilter(const SkMatrix&,
                                                                            const SkPaint&) const {
    sk_sp<SkImageFilter> filter = SkImageFilters::Shader(fShader);
    return {SkImageFilters::Blend(SkBlendMode::kDstIn, std::move(filter), nullptr), false};
}

sk_sp<SkMaskFilter> SkShaderMaskFilter::Make(sk_sp<SkShader> shader) {
    return shader ? sk_sp<SkMaskFilter>(new SkShaderMaskFilterImpl(std::move(shader))) : nullptr;
}

void SkShaderMaskFilter::RegisterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkShaderMaskFilterImpl);
    SkFlattenable::Register("SkShaderMF", SkShaderMaskFilterImpl::CreateProc);
}

// ============================================================================
// SkTableMaskFilter (src/effects/SkTableMaskFilter.cpp)
// ============================================================================

class SkTableMaskFilterImpl : public SkMaskFilterBase {
public:
    explicit SkTableMaskFilterImpl(const uint8_t table[256]);

    SkMask::Format getFormat() const override;
    bool filterMask(SkMaskBuilder*, const SkMask&, const SkMatrix&, SkIPoint*) const override;
    SkMaskFilterBase::Type type() const override { return SkMaskFilterBase::Type::kTable; }
    std::pair<sk_sp<SkImageFilter>, bool> asImageFilter(const SkMatrix&,
                                                        const SkPaint&) const override;

protected:
    ~SkTableMaskFilterImpl() override;

    void flatten(SkWriteBuffer&) const override;

private:
    SK_FLATTENABLE_HOOKS(SkTableMaskFilterImpl)

    SkTableMaskFilterImpl();

    uint8_t fTable[256];

    using INHERITED = SkMaskFilter;
    friend class SkTableMaskFilter;
};

SkTableMaskFilterImpl::SkTableMaskFilterImpl() {
    for (int i = 0; i < 256; i++) {
        fTable[i] = static_cast<uint8_t>(i);
    }
}

SkTableMaskFilterImpl::SkTableMaskFilterImpl(const uint8_t table[256]) {
    memcpy(fTable, table, sizeof(fTable));
}

SkTableMaskFilterImpl::~SkTableMaskFilterImpl() {}

bool SkTableMaskFilterImpl::filterMask(SkMaskBuilder* dst, const SkMask& src,
                                       const SkMatrix&, SkIPoint* margin) const {
    if (src.fFormat != SkMask::kA8_Format) {
        return false;
    }
    constexpr int32_t kMaxWidth = 1 << 30;
    if (src.fBounds.width() > kMaxWidth) {
        return false;
    }
    dst->bounds() = src.fBounds;
    dst->rowBytes() = SkAlign4(dst->fBounds.width());
    dst->format() = SkMask::kA8_Format;
    dst->image() = nullptr;

    if (src.fImage) {
        auto imgSize = dst->computeImageSize();
        if (imgSize == 0) {
            return false;
        }
        dst->image() = SkMaskBuilder::AllocImage(imgSize);
        skia_rs_table_apply_mask(src.fImage,
                                 src.fRowBytes,
                                 dst->image(),
                                 dst->fRowBytes,
                                 static_cast<size_t>(dst->fBounds.width()),
                                 static_cast<size_t>(dst->fBounds.height()),
                                 fTable);
    }

    if (margin) {
        margin->set(0, 0);
    }
    return true;
}

SkMask::Format SkTableMaskFilterImpl::getFormat() const {
    return SkMask::kA8_Format;
}

void SkTableMaskFilterImpl::flatten(SkWriteBuffer& wb) const {
    wb.writeByteArray(fTable, 256);
}

sk_sp<SkFlattenable> SkTableMaskFilterImpl::CreateProc(SkReadBuffer& buffer) {
    uint8_t table[256];
    if (!buffer.readByteArray(table, 256)) {
        return nullptr;
    }
    return sk_sp<SkFlattenable>(SkTableMaskFilter::Create(table));
}

std::pair<sk_sp<SkImageFilter>, bool> SkTableMaskFilterImpl::asImageFilter(const SkMatrix&,
                                                                           const SkPaint&) const {
    sk_sp<SkColorFilter> colorFilter = SkColorFilters::TableARGB(fTable, nullptr, nullptr, nullptr);
    return std::make_pair(SkImageFilters::ColorFilter(colorFilter, nullptr), false);
}

SkMaskFilter* SkTableMaskFilter::Create(const uint8_t table[256]) {
    return new SkTableMaskFilterImpl(table);
}

SkMaskFilter* SkTableMaskFilter::CreateGamma(SkScalar gamma) {
    uint8_t table[256];
    MakeGammaTable(table, gamma);
    return new SkTableMaskFilterImpl(table);
}

SkMaskFilter* SkTableMaskFilter::CreateClip(uint8_t min, uint8_t max) {
    uint8_t table[256];
    MakeClipTable(table, min, max);
    return new SkTableMaskFilterImpl(table);
}

void SkTableMaskFilter::MakeGammaTable(uint8_t table[256], SkScalar gamma) {
    skia_rs_table_make_gamma(table, gamma);
}

void SkTableMaskFilter::MakeClipTable(uint8_t table[256], uint8_t min, uint8_t max) {
    skia_rs_table_make_clip(table, min, max);
}

void SkTableMaskFilter::RegisterFlattenables() {
    SK_REGISTER_FLATTENABLE(SkTableMaskFilterImpl);
    SkFlattenable::Register("SkTableMF", SkTableMaskFilterImpl::CreateProc);
}
