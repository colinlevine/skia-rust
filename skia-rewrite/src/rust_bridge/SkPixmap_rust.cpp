/*
 * Safe Rust bridge for:
 *   src/core/SkPixmap.cpp
 *   src/core/SkPixmapDraw.cpp
 * (Layer 3.4a skia-rs-bitmap).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkCanvas.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkShader.h"
#include "include/core/SkSurface.h"
#include "include/core/SkTileMode.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMask.h"
#include "src/core/SkReadPixelsRec.h"
#include "src/shaders/SkImageShader.h"

#include <cstddef>
#include <cstdint>
#include <utility>

struct SkSamplingOptions;

namespace {

struct SkiaRsPixmapSubsetResult {
    SkIRect clipped;
    size_t byte_offset;
};

struct SkiaRsScalePixelsPlan {
    int32_t action;
    int32_t src_alpha_type;
    int32_t dst_alpha_type;
    bool clamp_as_if_unpremul;
    SkMatrix scale_matrix;
};

static_assert(sizeof(SkIRect) == 16);
static_assert(sizeof(SkColor4f) == 16);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkiaRsScalePixelsPlan) == 56);

static constexpr bool kPMColorIsBGRA = (SK_R32_SHIFT == 16);

static const uint8_t* fast_getaddr(const SkPixmap& pm, int x, int y) {
    x <<= SkColorTypeShiftPerPixel(pm.colorType());
    return static_cast<const uint8_t*>(pm.addr()) + static_cast<size_t>(y) * pm.rowBytes() + x;
}

}  // namespace

extern "C" {
bool skia_rs_pixmap_extract_subset(int32_t width,
                                   int32_t height,
                                   int32_t ct,
                                   size_t row_bytes,
                                   const SkIRect* subset,
                                   SkiaRsPixmapSubsetResult* out);
float skia_rs_pixmap_get_alphaf(int32_t ct, const uint8_t* pixel);
SkColor skia_rs_pixmap_get_color(int32_t ct, int32_t at, const uint8_t* pixel, bool is_bgra);
SkColor4f skia_rs_pixmap_get_color4f(int32_t ct, int32_t at, const uint8_t* pixel, bool is_bgra);
bool skia_rs_pixmap_compute_is_opaque(
        int32_t ct, int32_t width, int32_t height, size_t row_bytes, const uint8_t* pixels);
bool skia_rs_pixmap_erase_clip(
        int32_t ct, int32_t width, int32_t height, const SkIRect* subset, SkIRect* out_clip);
void skia_rs_pixmap_fill_rect(int32_t ct,
                              int32_t width,
                              int32_t height,
                              uint8_t* pixels,
                              size_t row_bytes,
                              const SkIRect* clip,
                              const uint8_t* dst_pixel);
void skia_rs_pixmap_plan_scale(int32_t src_w,
                               int32_t src_h,
                               int32_t src_at,
                               int32_t dst_w,
                               int32_t dst_h,
                               int32_t dst_at,
                               SkiaRsScalePixelsPlan* out);
}  // extern "C"

// ============================================================================
// SkPixmap.cpp
// ============================================================================

void SkPixmap::reset() {
    fPixels = nullptr;
    fRowBytes = 0;
    fInfo = SkImageInfo::MakeUnknown();
}

void SkPixmap::reset(const SkImageInfo& info, const void* addr, size_t rowBytes) {
    if (addr) {
        SkDEBUGCODE(info.validate());
        SkASSERT(info.validRowBytes(rowBytes));
    }
    fPixels = addr;
    fRowBytes = rowBytes;
    fInfo = info;
}

bool SkPixmap::reset(const SkMask& src) {
    if (SkMask::kA8_Format == src.fFormat) {
        this->reset(SkImageInfo::MakeA8(src.fBounds.width(), src.fBounds.height()),
                    src.fImage, src.fRowBytes);
        return true;
    }
    this->reset();
    return false;
}

void SkPixmap::setColorSpace(sk_sp<SkColorSpace> cs) {
    fInfo = fInfo.makeColorSpace(std::move(cs));
}

SkColorSpace* SkPixmap::colorSpace() const { return fInfo.colorSpace(); }

sk_sp<SkColorSpace> SkPixmap::refColorSpace() const { return fInfo.refColorSpace(); }

bool SkPixmap::extractSubset(SkPixmap* result, const SkIRect& subset) const {
    SkiaRsPixmapSubsetResult res{};
    if (!skia_rs_pixmap_extract_subset(this->width(),
                                       this->height(),
                                       static_cast<int32_t>(fInfo.colorType()),
                                       fRowBytes,
                                       &subset,
                                       &res)) {
        return false;
    }

    SkASSERT(static_cast<unsigned>(res.clipped.fLeft) < static_cast<unsigned>(this->width()));
    SkASSERT(static_cast<unsigned>(res.clipped.fTop) < static_cast<unsigned>(this->height()));

    const void* pixels = nullptr;
    if (fPixels) {
        pixels = static_cast<const uint8_t*>(fPixels) + res.byte_offset;
    }
    result->reset(fInfo.makeDimensions(res.clipped.size()), pixels, fRowBytes);
    return true;
}

float SkPixmap::getAlphaf(int x, int y) const {
    SkASSERT(this->addr());
    SkASSERT((unsigned)x < (unsigned)this->width());
    SkASSERT((unsigned)y < (unsigned)this->height());
    return skia_rs_pixmap_get_alphaf(static_cast<int32_t>(this->colorType()),
                                     fast_getaddr(*this, x, y));
}

bool SkPixmap::readPixels(const SkImageInfo& dstInfo, void* dstPixels, size_t dstRB,
                          int x, int y) const {
    if (!SkImageInfoValidConversion(dstInfo, fInfo)) {
        return false;
    }

    SkReadPixelsRec rec(dstInfo, dstPixels, dstRB, x, y);
    if (!rec.trim(fInfo.width(), fInfo.height())) {
        return false;
    }

    const void* srcPixels = this->addr(rec.fX, rec.fY);
    const SkImageInfo srcInfo = fInfo.makeDimensions(rec.fInfo.dimensions());
    return SkConvertPixels(rec.fInfo, rec.fPixels, rec.fRowBytes, srcInfo, srcPixels,
                           this->rowBytes());
}

SkColor SkPixmap::getColor(int x, int y) const {
    SkASSERT(this->addr());
    SkASSERT((unsigned)x < (unsigned)this->width());
    SkASSERT((unsigned)y < (unsigned)this->height());
    SkASSERT(this->colorType() != kUnknown_SkColorType &&
             this->colorType() != kBGR_101010x_XR_SkColorType &&
             this->colorType() != kBGRA_10101010_XR_SkColorType);
    return skia_rs_pixmap_get_color(static_cast<int32_t>(this->colorType()),
                                    static_cast<int32_t>(fInfo.alphaType()),
                                    fast_getaddr(*this, x, y),
                                    kPMColorIsBGRA);
}

SkColor4f SkPixmap::getColor4f(int x, int y) const {
    SkASSERT(this->addr());
    SkASSERT((unsigned)x < (unsigned)this->width());
    SkASSERT((unsigned)y < (unsigned)this->height());
    SkASSERT(this->colorType() != kUnknown_SkColorType &&
             this->colorType() != kBGR_101010x_XR_SkColorType &&
             this->colorType() != kBGRA_10101010_XR_SkColorType);
    return skia_rs_pixmap_get_color4f(static_cast<int32_t>(this->colorType()),
                                      static_cast<int32_t>(fInfo.alphaType()),
                                      fast_getaddr(*this, x, y),
                                      kPMColorIsBGRA);
}

bool SkPixmap::computeIsOpaque() const {
    SkASSERT(this->colorType() != kUnknown_SkColorType);
    return skia_rs_pixmap_compute_is_opaque(static_cast<int32_t>(this->colorType()),
                                            this->width(),
                                            this->height(),
                                            this->rowBytes(),
                                            static_cast<const uint8_t*>(this->addr()));
}

bool SkPixmap::erase(SkColor color, const SkIRect& subset) const {
    return this->erase(SkColor4f::FromColor(color), &subset);
}

bool SkPixmap::erase(const SkColor4f& color, const SkIRect* subset) const {
    SkIRect clip;
    if (!skia_rs_pixmap_erase_clip(static_cast<int32_t>(this->colorType()),
                                   this->width(),
                                   this->height(),
                                   subset,
                                   &clip)) {
        return false;
    }

    const SkIRect thisBounds = this->bounds();
    SkASSERT_RELEASE(!clip.isEmpty());
    SkASSERTF_RELEASE(thisBounds.contains(clip),
                      "clip bounds: [%d, %d, %d, %d], this bounds: [%d, %d, %d, %d]",
                      clip.fLeft, clip.fTop, clip.fRight, clip.fBottom,
                      thisBounds.fLeft, thisBounds.fTop, thisBounds.fRight, thisBounds.fBottom);

    const auto c = color.premul();

    const auto dst = SkImageInfo::Make(1, 1, this->colorType(), this->alphaType(),
                                       sk_ref_sp(this->colorSpace()));
    const auto src = SkImageInfo::Make(1, 1, kRGBA_F32_SkColorType, kPremul_SkAlphaType, nullptr);

    uint64_t dstPixel[2] = {};
    SkASSERT((size_t)dst.bytesPerPixel() <= sizeof(dstPixel));

    if (!SkConvertPixels(dst, dstPixel, sizeof(dstPixel), src, &c, sizeof(c))) {
        return false;
    }

    skia_rs_pixmap_fill_rect(static_cast<int32_t>(this->colorType()),
                             this->width(),
                             this->height(),
                             static_cast<uint8_t*>(this->writable_addr()),
                             this->rowBytes(),
                             &clip,
                             reinterpret_cast<const uint8_t*>(dstPixel));
    return true;
}

// ============================================================================
// SkPixmapDraw.cpp
// ============================================================================

bool SkPixmap::scalePixels(const SkPixmap& actualDst, const SkSamplingOptions& sampling) const {
    SkPixmap src = *this;
    SkPixmap dst = actualDst;

    SkiaRsScalePixelsPlan plan{};
    skia_rs_pixmap_plan_scale(src.width(),
                              src.height(),
                              static_cast<int32_t>(src.alphaType()),
                              dst.width(),
                              dst.height(),
                              static_cast<int32_t>(dst.alphaType()),
                              &plan);

    if (plan.action == 0) {
        return false;
    }
    if (plan.action == 1) {
        return src.readPixels(dst);
    }

    if (plan.clamp_as_if_unpremul) {
        src.reset(src.info().makeAlphaType(static_cast<SkAlphaType>(plan.src_alpha_type)),
                  src.addr(),
                  src.rowBytes());
        dst.reset(dst.info().makeAlphaType(static_cast<SkAlphaType>(plan.dst_alpha_type)),
                  dst.addr(),
                  dst.rowBytes());
    }

    SkBitmap bitmap;
    if (!bitmap.installPixels(src)) {
        return false;
    }
    bitmap.setImmutable();

    sk_sp<SkShader> shader = SkImageShader::Make(bitmap.asImage(),
                                                 SkTileMode::kClamp,
                                                 SkTileMode::kClamp,
                                                 sampling,
                                                 &plan.scale_matrix,
                                                 plan.clamp_as_if_unpremul);

    sk_sp<SkSurface> surface =
            SkSurfaces::WrapPixels(dst.info(), dst.writable_addr(), dst.rowBytes());
    if (!shader || !surface) {
        return false;
    }

    SkPaint paint;
    paint.setBlendMode(SkBlendMode::kSrc);
    paint.setShader(std::move(shader));
    surface->getCanvas()->drawPaint(paint);
    return true;
}
