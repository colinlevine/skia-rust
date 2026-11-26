/*
 * Safe Rust bridge for:
 *   src/core/SkMipmap.cpp
 *   src/core/SkMipmapAccessor.cpp
 *   src/core/SkMipmapBuilder.cpp
 *   src/core/SkMipmapDrawDownSampler.cpp
 *   src/core/SkMipmapHQDownSampler.cpp
 * (Layer 3.4a skia-rs-bitmap).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImage.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkScalar.h"
#include "include/core/SkSize.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAlign.h"
#include "include/private/SkAssert.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTo.h"
#include "src/core/SkArenaAlloc.h"
#include "src/core/SkBitmapCache.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkMipmapAccessor.h"
#include "src/core/SkMipmapBuilder.h"
#include "src/image/SkImage_Base.h"

#ifdef SK_USE_DRAWING_MIPMAP_DOWNSAMPLER
#include "include/core/SkBlendMode.h"
#include "include/core/SkPaint.h"
#include "src/core/SkDraw.h"
#include "src/core/SkRasterClip.h"
#endif

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <new>
#include <vector>

namespace {

struct SkiaRsMipmapAccessorPlan {
    int32_t resolved_mode;
    int32_t level_num;
    float lower_weight;
    bool need_mips;
};

static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkSize) == 8);
static_assert(sizeof(SkMatrix) == 40);
static_assert(sizeof(SkiaRsMipmapAccessorPlan) == 16);

#ifdef SK_SUPPORT_LEGACY_ANISOTROPIC_MIPMAP_SCALE
static constexpr bool kLegacyAnisotropicMipmapScale = true;
#else
static constexpr bool kLegacyAnisotropicMipmapScale = false;
#endif

}  // namespace

extern "C" {
int32_t skia_rs_mipmap_compute_level_count(int32_t base_width, int32_t base_height);
SkISize skia_rs_mipmap_compute_level_size(int32_t base_width, int32_t base_height, int32_t level);
size_t skia_rs_mipmap_alloc_levels_size(int32_t level_count,
                                        size_t level_struct_size,
                                        size_t pixel_size);
size_t skia_rs_mipmap_compute_total_pixel_size(int32_t base_width,
                                               int32_t base_height,
                                               int32_t ct);
float skia_rs_mipmap_compute_level(SkSize scale_size, bool legacy_anisotropic);
bool skia_rs_mipmap_select_extract_level(SkSize scale_size,
                                         int32_t level_count,
                                         bool legacy_anisotropic,
                                         size_t* out_index);
bool skia_rs_mipmap_valid_for_root_level(int32_t root_width,
                                         int32_t root_height,
                                         int32_t root_ct,
                                         int32_t root_at,
                                         int32_t level0_width,
                                         int32_t level0_height,
                                         const int32_t* level_cts,
                                         const int32_t* level_ats,
                                         size_t level_count);
void skia_rs_mipmap_plan_accessor(const SkMatrix* inv,
                                  int32_t requested_mode,
                                  bool legacy_anisotropic,
                                  SkiaRsMipmapAccessorPlan* out);
bool skia_rs_mipmap_draw_downsampler_use_bilerp(int32_t dst_w,
                                                int32_t dst_h,
                                                int32_t src_w,
                                                int32_t src_h);
bool skia_rs_mipmap_hq_downsample_supported(int32_t ct);
bool skia_rs_mipmap_hq_downsample_level(int32_t ct,
                                        int32_t src_w,
                                        int32_t src_h,
                                        size_t src_rb,
                                        const uint8_t* src,
                                        int32_t dst_w,
                                        int32_t dst_h,
                                        size_t dst_rb,
                                        uint8_t* dst);
}  // extern "C"

// ============================================================================
// SkMipmap.cpp
// ============================================================================

SkMipmap::SkMipmap(void* malloc, size_t size) : SkCachedData(malloc, size) {}
SkMipmap::SkMipmap(size_t size, SkDiscardableMemory* dm) : SkCachedData(size, dm) {}
SkMipmap::~SkMipmap() = default;

size_t SkMipmap::AllocLevelsSize(int levelCount, size_t pixelSize) {
    return skia_rs_mipmap_alloc_levels_size(levelCount, sizeof(Level), pixelSize);
}

SkMipmap* SkMipmap::Build(const SkPixmap& src, SkDiscardableFactoryProc fact,
                          bool computeContents) {
    if (src.width() <= 1 && src.height() <= 1) {
        return nullptr;
    }

    const SkColorType ct = src.colorType();
    const SkAlphaType at = src.alphaType();

    const int countLevels = ComputeLevelCount(src.width(), src.height());
    const size_t size = skia_rs_mipmap_compute_total_pixel_size(
            src.width(), src.height(), static_cast<int32_t>(ct));

    size_t storageSize = SkMipmap::AllocLevelsSize(countLevels, size);
    if (0 == storageSize) {
        return nullptr;
    }

    SkMipmap* mipmap;
    if (fact) {
        SkDiscardableMemory* dm = fact(storageSize);
        if (nullptr == dm) {
            return nullptr;
        }
        mipmap = new SkMipmap(storageSize, dm);
    } else {
        void* tmp = sk_malloc_canfail(storageSize);
        if (!tmp) {
            return nullptr;
        }
        mipmap = new SkMipmap(tmp, storageSize);
    }

    mipmap->fCS = sk_ref_sp(src.info().colorSpace());
    mipmap->fCount = countLevels;
    mipmap->fLevels = static_cast<Level*>(mipmap->writable_data());
    SkASSERT(mipmap->fLevels);

    Level* levels = mipmap->fLevels;
    uint8_t* baseAddr = reinterpret_cast<uint8_t*>(&levels[countLevels]);
    uint8_t* addr = baseAddr;
    int width = src.width();
    int height = src.height();
    uint32_t rowBytes;
    SkPixmap srcPM(src);

    SkASSERT(SkIsAlign8(reinterpret_cast<uintptr_t>(addr)));

    std::unique_ptr<SkMipmapDownSampler> downsampler;
    if (computeContents) {
        downsampler = MakeDownSampler(src);
        if (!downsampler) {
            delete mipmap;
            return nullptr;
        }
    }

    for (int i = 0; i < countLevels; ++i) {
        width = std::max(1, width >> 1);
        height = std::max(1, height >> 1);
        rowBytes = SkToU32(SkColorTypeMinRowBytes(ct, width));

        new (&levels[i].fPixmap) SkPixmap(SkImageInfo::Make(width, height, ct, at), addr, rowBytes);
        levels[i].fScale = SkSize::Make(SkIntToScalar(width)  / src.width(),
                                        SkIntToScalar(height) / src.height());

        const SkPixmap& dstPM = levels[i].fPixmap;
        if (downsampler) {
            downsampler->buildLevel(dstPM, srcPM);
        }
        srcPM = dstPM;
        addr += static_cast<size_t>(height) * rowBytes;
    }
    SkASSERT(addr == baseAddr + size);
    SkASSERT(mipmap->fLevels);
    return mipmap;
}

int SkMipmap::ComputeLevelCount(int baseWidth, int baseHeight) {
    return skia_rs_mipmap_compute_level_count(baseWidth, baseHeight);
}

SkISize SkMipmap::ComputeLevelSize(int baseWidth, int baseHeight, int level) {
    return skia_rs_mipmap_compute_level_size(baseWidth, baseHeight, level);
}

float SkMipmap::ComputeLevel(SkSize scaleSize) {
    SkASSERT(scaleSize.width() >= 0 && scaleSize.height() >= 0);
    return skia_rs_mipmap_compute_level(scaleSize, kLegacyAnisotropicMipmapScale);
}

bool SkMipmap::extractLevel(SkSize scaleSize, Level* levelPtr) const {
    if (nullptr == fLevels) {
        return false;
    }
    size_t idx = 0;
    if (!skia_rs_mipmap_select_extract_level(
                scaleSize, fCount, kLegacyAnisotropicMipmapScale, &idx)) {
        return false;
    }
    if (levelPtr) {
        *levelPtr = fLevels[idx];
        levelPtr->fPixmap.setColorSpace(fCS);
    }
    return true;
}

bool SkMipmap::validForRootLevel(const SkImageInfo& root) const {
    if (nullptr == fLevels || fCount <= 0) {
        return false;
    }
    const int n = this->countLevels();
    int32_t stackCts[32];
    int32_t stackAts[32];
    std::vector<int32_t> heapCts;
    std::vector<int32_t> heapAts;
    int32_t* cts = stackCts;
    int32_t* ats = stackAts;
    if (n > 32) {
        heapCts.resize(static_cast<size_t>(n));
        heapAts.resize(static_cast<size_t>(n));
        cts = heapCts.data();
        ats = heapAts.data();
    }
    for (int i = 0; i < n; ++i) {
        cts[i] = static_cast<int32_t>(fLevels[i].fPixmap.colorType());
        ats[i] = static_cast<int32_t>(fLevels[i].fPixmap.alphaType());
    }
    return skia_rs_mipmap_valid_for_root_level(root.width(),
                                               root.height(),
                                               static_cast<int32_t>(root.colorType()),
                                               static_cast<int32_t>(root.alphaType()),
                                               fLevels[0].fPixmap.width(),
                                               fLevels[0].fPixmap.height(),
                                               cts,
                                               ats,
                                               static_cast<size_t>(n));
}

SkMipmap* SkMipmap::Build(const SkBitmap& src, SkDiscardableFactoryProc fact) {
    SkPixmap srcPixmap;
    if (!src.peekPixels(&srcPixmap)) {
        return nullptr;
    }
    return Build(srcPixmap, fact);
}

int SkMipmap::countLevels() const {
    return fCount;
}

bool SkMipmap::getLevel(int index, Level* levelPtr) const {
    if (nullptr == fLevels) {
        return false;
    }
    if (index < 0 || index > fCount - 1) {
        return false;
    }
    if (levelPtr) {
        *levelPtr = fLevels[index];
        levelPtr->fPixmap.setColorSpace(fCS);
    }
    return true;
}

// ============================================================================
// SkMipmapAccessor.cpp
// ============================================================================

static sk_sp<const SkMipmap> try_load_mips(const SkImage_Base* image) {
    sk_sp<const SkMipmap> mips = image->refMips();
    if (!mips) {
        mips.reset(SkMipmapCache::FindAndRef(SkBitmapCacheDesc::Make(image)));
    }
    if (!mips) {
        mips.reset(SkMipmapCache::AddAndRef(image));
    }
    return mips;
}

SkMipmapAccessor::SkMipmapAccessor(const SkImage_Base* image, const SkMatrix& inv,
                                   SkMipmapMode requestedMode) {
    fLowerWeight = 0;

    auto load_upper_from_base = [&]() {
        if (fBaseStorage.getPixels() == nullptr) {
            auto dContext = as_IB(image)->directContext();
            (void)image->getROPixels(dContext, &fBaseStorage);
            fUpper.reset(fBaseStorage.info(), fBaseStorage.getPixels(), fBaseStorage.rowBytes());
        }
    };

    SkiaRsMipmapAccessorPlan plan{};
    skia_rs_mipmap_plan_accessor(&inv,
                                 static_cast<int32_t>(requestedMode),
                                 kLegacyAnisotropicMipmapScale,
                                 &plan);

    SkMipmapMode resolvedMode = static_cast<SkMipmapMode>(plan.resolved_mode);
    int levelNum = plan.level_num;
    float lowerWeight = plan.lower_weight;
    SkASSERT(levelNum >= 0);

    auto scale = [image](const SkPixmap& pm) {
        return SkMatrix::Scale(SkIntToScalar(pm.width())  / image->width(),
                               SkIntToScalar(pm.height()) / image->height());
    };

    if (levelNum == 0) {
        load_upper_from_base();
    }
    if (plan.need_mips) {
        fCurrMip = try_load_mips(image);
        if (!fCurrMip) {
            load_upper_from_base();
            resolvedMode = SkMipmapMode::kNone;
        } else {
            SkMipmap::Level levelRec;

            SkASSERT(resolvedMode != SkMipmapMode::kNone);
            if (levelNum > 0) {
                if (fCurrMip->getLevel(levelNum - 1, &levelRec)) {
                    fUpper = levelRec.fPixmap;
                } else {
                    load_upper_from_base();
                    resolvedMode = SkMipmapMode::kNone;
                }
            }

            if (resolvedMode == SkMipmapMode::kLinear) {
                if (fCurrMip->getLevel(levelNum, &levelRec)) {
                    fLower = levelRec.fPixmap;
                    fLowerWeight = lowerWeight;
                    fLowerInv = scale(fLower);
                } else {
                    resolvedMode = SkMipmapMode::kNearest;
                }
            }
        }
    }
    fUpperInv = scale(fUpper);
}

SkMipmapAccessor* SkMipmapAccessor::Make(SkArenaAlloc* alloc, const SkImage* image,
                                         const SkMatrix& inv, SkMipmapMode mipmap) {
    auto* access = alloc->make<SkMipmapAccessor>(as_IB(image), inv, mipmap);
    return access->fUpper.addr() ? access : nullptr;
}

// ============================================================================
// SkMipmapBuilder.cpp
// ============================================================================

SkMipmapBuilder::SkMipmapBuilder(const SkImageInfo& info) {
    fMM = sk_sp<SkMipmap>(SkMipmap::Build({info, nullptr, 0},
                                          /* factoryProc= */ nullptr,
                                          /* computeContents= */ false));
}

SkMipmapBuilder::~SkMipmapBuilder() {}

int SkMipmapBuilder::countLevels() const {
    return fMM ? fMM->countLevels() : 0;
}

SkPixmap SkMipmapBuilder::level(int index) const {
    SkPixmap pm;
    SkMipmap::Level level;
    if (fMM && fMM->getLevel(index, &level)) {
        pm = level.fPixmap;
    }
    return pm;
}

sk_sp<SkImage> SkMipmapBuilder::attachTo(const sk_sp<const SkImage>& src) {
    return src->withMipmaps(fMM);
}

// ============================================================================
// SkMipmapDrawDownSampler.cpp / SkMipmapHQDownSampler.cpp
// ============================================================================

#ifdef SK_USE_DRAWING_MIPMAP_DOWNSAMPLER

namespace {
struct DrawDownSampler : SkMipmapDownSampler {
    SkPaint fPaint;

    DrawDownSampler() {
        fPaint.setBlendMode(SkBlendMode::kSrc);
    }

    void buildLevel(const SkPixmap& dst, const SkPixmap& src) override {
        const SkRasterClip rclip(dst.bounds());
        const SkMatrix mx = SkMatrix::Scale(SkIntToScalar(dst.width())  / src.width(),
                                            SkIntToScalar(dst.height()) / src.height());
        const SkSamplingOptions sampling =
                skia_rs_mipmap_draw_downsampler_use_bilerp(
                        dst.width(), dst.height(), src.width(), src.height())
                        ? SkSamplingOptions(SkFilterMode::kLinear, SkMipmapMode::kNone)
                        : SkSamplingOptions(SkCubicResampler::CatmullRom());

        skcpu::Draw draw;
        draw.fDst = dst;
        draw.fCTM = &mx;
        draw.fRC = &rclip;

        SkBitmap bitmap;
        bitmap.installPixels(src.info(), const_cast<void*>(src.addr()), src.rowBytes());
        draw.drawBitmap(bitmap, SkMatrix::I(), nullptr, sampling, fPaint, nullptr);
    }
};
}  // namespace

std::unique_ptr<SkMipmapDownSampler> SkMipmap::MakeDownSampler(const SkPixmap& root) {
    return std::make_unique<DrawDownSampler>();
}

#else

namespace {
struct RustHQDownSampler final : SkMipmapDownSampler {
    void buildLevel(const SkPixmap& dst, const SkPixmap& src) override {
        SkAssertResult(skia_rs_mipmap_hq_downsample_level(
                static_cast<int32_t>(src.colorType()),
                src.width(),
                src.height(),
                src.rowBytes(),
                static_cast<const uint8_t*>(src.addr()),
                dst.width(),
                dst.height(),
                dst.rowBytes(),
                static_cast<uint8_t*>(dst.writable_addr())));
    }
};
}  // namespace

std::unique_ptr<SkMipmapDownSampler> SkMipmap::MakeDownSampler(const SkPixmap& root) {
    if (!skia_rs_mipmap_hq_downsample_supported(static_cast<int32_t>(root.colorType()))) {
        return nullptr;
    }
    return std::make_unique<RustHQDownSampler>();
}

#endif
