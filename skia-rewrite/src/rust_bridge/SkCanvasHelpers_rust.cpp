/*
 * Safe Rust bridge for:
 *   src/core/SkRecorder.cpp
 *   src/core/RasterContext.cpp
 *   src/cpu/Recorder.cpp
 *   src/core/SkContext.cpp
 *   src/core/SkAutoPixmapStorage.cpp
 *   src/core/SkReadPixelsRec.cpp
 *   src/core/SkWritePixelsRec.cpp
 *   src/core/SkPixelStorage.cpp
 * (Phase 1 Layer 5.3 skia-rs-canvas, GN target :core).
 */

#include "include/core/RasterContext.h"
#include "include/core/SkContext.h"
#include "include/core/SkContextOptions.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRecorder.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkString.h"
#include "include/core/SkTraceMemoryDump.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkGPUContext.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkOnce.h"
#include "include/private/SkPixelStorage.h"
#include "src/capture/SkCapture.h"
#include "src/capture/SkCaptureAsset.h"
#include "src/capture/SkCaptureManager.h"
#include "src/core/SkAutoPixmapStorage.h"
#include "src/core/SkContextPriv.h"
#include "src/core/SkCpu.h"
#include "src/core/SkOpts.h"
#include "src/core/SkReadPixelsRec.h"
#include "src/core/SkResourceCache.h"
#include "src/core/SkSharedState.h"
#include "src/core/SkStrikeCache.h"
#include "src/core/SkSynchronizedResourceCache.h"
#include "src/core/SkTypefaceCache.h"
#include "src/core/SkWritePixelsRec.h"
#include "src/cpu/Recorder.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>

namespace {

struct SkiaRsPixelsRecTrimResult {
    bool valid;
    size_t byte_offset;
    int32_t new_width;
    int32_t new_height;
    int32_t new_x;
    int32_t new_y;
};

static_assert(sizeof(SkiaRsPixelsRecTrimResult) == 32);

extern "C" {
SkiaRsPixelsRecTrimResult skia_rs_canvas_trim_pixels_rec(
        bool has_pixels,
        int32_t info_width,
        int32_t info_height,
        size_t min_row_bytes,
        size_t bytes_per_pixel,
        size_t row_bytes,
        int32_t x,
        int32_t y,
        int32_t bound_width,
        int32_t bound_height);
uint32_t skia_rs_canvas_next_pixel_storage_id();
int32_t skia_rs_canvas_context_select_recorder(int32_t backend, bool has_gpu_context);
}

void init_globals_if_needed() {
    static SkOnce once;
    once([] {
        SkCpu::CacheRuntimeFeatures();
        SkOpts::Init();
    });
}

void sk_trace_dump_visitor(const SkResourceCache::Rec& rec, void* context) {
    SkTraceMemoryDump* dump = static_cast<SkTraceMemoryDump*>(context);
    SkString dumpName = SkStringPrintf("skia/sk_resource_cache/%s_%p", rec.getCategory(), &rec);
    SkDiscardableMemory* discardable = rec.diagnostic_only_getDiscardable();
    if (discardable) {
        dump->setDiscardableMemoryBacking(dumpName.c_str(), *discardable);
        dump->dumpNumericValue(dumpName.c_str(), "discardable_size", "bytes", rec.bytesUsed());
    } else {
        dump->dumpNumericValue(dumpName.c_str(), "size", "bytes", rec.bytesUsed());
        dump->setMemoryBacking(dumpName.c_str(), "malloc", nullptr);
    }
}

}  // namespace

// ============================================================================
// SkRecorder.cpp
// ============================================================================

SkRecorder* SkRecorder::TODO() {
    return skcpu::Recorder::TODO();
}

SkRecorder::SkRecorder() = default;
SkRecorder::~SkRecorder() = default;

// ============================================================================
// RasterContext.cpp
// ============================================================================

namespace SkContexts {

std::unique_ptr<SkContext> MakeRaster(const SkContextOptions& options) {
    return SkContextCtorAccessor::MakeContext(options);
}

}  // namespace SkContexts

// ============================================================================
// src/cpu/Recorder.cpp
// ============================================================================

namespace skcpu {

Recorder* Recorder::TODO() {
    static Recorder* gRecorder = std::make_unique<Recorder>().release();
    return gRecorder;
}

void Recorder::createCaptureBreakpoint(SkSurface*) {}

}  // namespace skcpu

// ============================================================================
// SkContext.cpp
// ============================================================================

SkContext::SkContext(const SkContextOptions& options) : SkContext(options, nullptr) {}

SkContext::SkContext(const SkContextOptions& options, std::unique_ptr<SkGPUContext> gpuContext)
        : fGPUContext(std::move(gpuContext)) {
    if (options.fEnableRuntimeOpts) {
        init_globals_if_needed();
    }

    if (options.fEnableCapture) {
        fCaptureManager = sk_make_sp<SkCaptureManager>();
    }
    fSharedState = sk_make_sp<SkSharedState>(options);
}

SkContext::~SkContext() = default;

SkContextPriv SkContext::priv() { return SkContextPriv(this); }
SkContextPrivConst SkContext::priv() const { return SkContextPrivConst(this); }

void SkContext::startCapture() {
    if (!fCaptureManager) {
        return;
    }
    fCaptureManager->toggleCapture(true);
}

sk_sp<SkCapture> SkContext::endCapture() {
    if (!fCaptureManager) {
        return nullptr;
    }
    fCaptureManager->toggleCapture(false);
    return fCaptureManager->getLastCapture();
}

std::unique_ptr<SkRecorder> SkContext::makeRecorder(
        BackendSelection backend, const skgpu::graphite::RecorderOptions* graphiteOptions) {
    int32_t plan = skia_rs_canvas_context_select_recorder(
            static_cast<int32_t>(backend), fGPUContext != nullptr);
    switch (plan) {
        case 1: return std::make_unique<skcpu::Recorder>();
        case 2: return fGPUContext->makeRecorder(graphiteOptions);
        default: return nullptr;
    }
}

size_t SkContext::fontCacheLimit() const { return fSharedState->fontCacheLimit(); }
size_t SkContext::setFontCacheLimit(size_t bytes) { return fSharedState->setFontCacheLimit(bytes); }
size_t SkContext::fontCacheUsed() const { return fSharedState->fontCacheUsed(); }
int SkContext::fontCacheCountLimit() const { return fSharedState->fontCacheCountLimit(); }
int SkContext::setFontCacheCountLimit(int count) {
    return fSharedState->setFontCacheCountLimit(count);
}
int SkContext::fontCacheCountUsed() const { return fSharedState->fontCacheCountUsed(); }
void SkContext::purgeFontCache() { fSharedState->purgeFontCache(); }
void SkContext::purgePinnedFontCache() { fSharedState->purgePinnedFontCache(); }

size_t SkContext::resourceCacheTotalBytesUsed() const {
    return fSharedState->resourceCacheTotalBytesUsed();
}
size_t SkContext::resourceCacheTotalByteLimit() const {
    return fSharedState->resourceCacheTotalByteLimit();
}
size_t SkContext::setResourceCacheTotalByteLimit(size_t newLimit) {
    return fSharedState->setResourceCacheTotalByteLimit(newLimit);
}
size_t SkContext::resourceCacheSingleAllocationByteLimit() const {
    return fSharedState->resourceCacheSingleAllocationByteLimit();
}
size_t SkContext::setResourceCacheSingleAllocationByteLimit(size_t newLimit) {
    return fSharedState->setResourceCacheSingleAllocationByteLimit(newLimit);
}
void SkContext::purgeResourceCache() { fSharedState->purgeResourceCache(); }
int SkContext::typefaceCacheCountLimit() const { return fSharedState->typefaceCacheCountLimit(); }
int SkContext::setTypefaceCacheCountLimit(int count) {
    return fSharedState->setTypefaceCacheCountLimit(count);
}

void SkContext::dumpMemoryStatistics(SkTraceMemoryDump* dump) {
    fSharedState->synchronizedResourceCache()->visitAll(sk_trace_dump_visitor, dump);
    fSharedState->strikeCache()->dumpMemoryStatistics(dump);
}

std::unique_ptr<SkContext> SkContextCtorAccessor::MakeContext(const SkContextOptions& options) {
    return std::unique_ptr<SkContext>(new SkContext(options));
}

std::unique_ptr<SkContext> SkContextCtorAccessor::MakeContext(
        const SkContextOptions& options, std::unique_ptr<SkGPUContext> gpuContext) {
    return std::unique_ptr<SkContext>(new SkContext(options, std::move(gpuContext)));
}

namespace SkContexts {

SkContext* GetGlobalSkContext(bool enableRuntimeOpts) {
    static SkContext* gSkContext = [enableRuntimeOpts] {
        SkContextOptions opts;
        opts.fEnableRuntimeOpts = enableRuntimeOpts;
        return SkContexts::MakeRaster(opts).release();
    }();
    return gSkContext;
}

}  // namespace SkContexts

// ============================================================================
// SkAutoPixmapStorage.cpp
// ============================================================================

SkAutoPixmapStorage::SkAutoPixmapStorage() = default;
SkAutoPixmapStorage::~SkAutoPixmapStorage() = default;

SkAutoPixmapStorage::SkAutoPixmapStorage(SkAutoPixmapStorage&& other) { *this = std::move(other); }

SkAutoPixmapStorage& SkAutoPixmapStorage::operator=(SkAutoPixmapStorage&& other) {
    if (this != &other) {
        this->fStorage = std::move(other.fStorage);
        this->INHERITED::reset(other.info(), other.addr(), other.rowBytes());
        other.INHERITED::reset();
    }
    return *this;
}

size_t SkAutoPixmapStorage::AllocSize(const SkImageInfo& info, size_t* rowBytes) {
    size_t rb = info.minRowBytes();
    if (rowBytes) {
        *rowBytes = rb;
    }
    return info.computeByteSize(rb);
}

bool SkAutoPixmapStorage::tryAlloc(const SkImageInfo& info) {
    this->reset();

    size_t rb;
    size_t size = AllocSize(info, &rb);
    if (SkImageInfo::ByteSizeOverflowed(size)) {
        return false;
    }
    void* pixels = sk_malloc_canfail(size);
    if (nullptr == pixels) {
        return false;
    }
    this->INHERITED::reset(info, pixels, rb);
    fStorage.reset(pixels);
    return true;
}

void SkAutoPixmapStorage::alloc(const SkImageInfo& info) {
    SkASSERT_RELEASE(this->tryAlloc(info));
}

void* SkAutoPixmapStorage::detachPixels() {
    if (!fStorage) {
        return nullptr;
    }
    void* data = fStorage.release();
    this->INHERITED::reset();
    return data;
}

sk_sp<SkData> SkAutoPixmapStorage::detachPixelsAsData() {
    if (!fStorage) {
        return nullptr;
    }
    sk_sp<SkData> data = SkData::MakeFromMalloc(fStorage.release(), this->computeByteSize());
    this->INHERITED::reset();
    return data;
}

// ============================================================================
// SkReadPixelsRec.cpp & SkWritePixelsRec.cpp
// ============================================================================

bool SkReadPixelsRec::trim(int srcWidth, int srcHeight) {
    SkiaRsPixelsRecTrimResult res = skia_rs_canvas_trim_pixels_rec(
            fPixels != nullptr,
            fInfo.width(),
            fInfo.height(),
            fInfo.minRowBytes(),
            static_cast<size_t>(fInfo.bytesPerPixel()),
            fRowBytes,
            fX,
            fY,
            srcWidth,
            srcHeight);
    if (!res.valid) {
        return false;
    }
    fPixels = static_cast<char*>(fPixels) + res.byte_offset;
    fInfo = fInfo.makeDimensions(SkISize::Make(res.new_width, res.new_height));
    fX = res.new_x;
    fY = res.new_y;
    return true;
}

bool SkWritePixelsRec::trim(int dstWidth, int dstHeight) {
    SkiaRsPixelsRecTrimResult res = skia_rs_canvas_trim_pixels_rec(
            fPixels != nullptr,
            fInfo.width(),
            fInfo.height(),
            fInfo.minRowBytes(),
            static_cast<size_t>(fInfo.bytesPerPixel()),
            fRowBytes,
            fX,
            fY,
            dstWidth,
            dstHeight);
    if (!res.valid) {
        return false;
    }
    fPixels = static_cast<const char*>(fPixels) + res.byte_offset;
    fInfo = fInfo.makeDimensions(SkISize::Make(res.new_width, res.new_height));
    fX = res.new_x;
    fY = res.new_y;
    return true;
}

// ============================================================================
// SkPixelStorage.cpp
// ============================================================================

uint32_t SkPixelStorage::NextId() {
    return skia_rs_canvas_next_pixel_storage_id();
}

SkPixelStorage::SkPixelStorage(Type type) : fType(type), fID(NextId()) {}
