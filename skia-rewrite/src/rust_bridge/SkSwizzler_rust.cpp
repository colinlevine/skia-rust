/*
 * Safe Rust bridge for:
 *   src/codec/SkColorPalette.cpp
 *   src/codec/SkSampler.cpp
 *   src/codec/SkSwizzler.cpp
 *   src/codec/SkMaskSwizzler.cpp
 * (Phase 2 Module P2.1: skia-rs-codec-core).
 */

#include "include/codec/SkCodec.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRect.h"
#include "include/private/SkAssert.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTemplates.h"
#include "src/codec/SkColorPalette.h"
#include "src/codec/SkMaskSwizzler.h"
#include "src/codec/SkSampler.h"
#include "src/codec/SkSwizzler.h"
#include "src/core/SkColorPriv.h"
#include "src/core/SkMasks.h"

#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
    #include "include/android/SkAndroidFrameworkUtils.h"
#endif

#include <cstddef>
#include <cstdint>
#include <memory>

namespace {

#ifdef SK_PMCOLOR_IS_BGRA
constexpr bool kPmColorIsBgra = true;
#else
constexpr bool kPmColorIsBgra = false;
#endif

struct RustSwizzlerInit {
    int32_t fast_proc_id;
    int32_t slow_proc_id;
    int32_t src_offset;
    int32_t src_width;
    int32_t dst_offset;
    int32_t dst_width;
    int32_t src_bpp;
    int32_t dst_bpp;
};

struct RustSwizzlerSampleXResult {
    int32_t dst_offset_bytes;
    int32_t swizzle_width;
    int32_t allocated_width;
    int32_t src_offset_units;
    bool use_fast_proc;
    bool safety_net_triggered;
};

struct RustMaskSwizzlerInit {
    int32_t proc_id;
    int32_t src_offset;
    int32_t subset_width;
};

}  // namespace

extern "C" {

bool skia_rs_color_palette_copy(uint32_t* dst, const uint32_t* src, int32_t count);

void skia_rs_sampler_fill(int32_t color_type,
                          int32_t width,
                          int32_t num_rows,
                          uint8_t* dst,
                          size_t row_bytes,
                          int32_t zero_init);

bool skia_rs_swizzler_select_simple(int32_t src_bpp,
                                    int32_t dst_ct,
                                    int32_t dst_width,
                                    const SkIRect* subset,
                                    const SkIRect* frame,
                                    RustSwizzlerInit* out_init);

bool skia_rs_swizzler_select(int32_t encoded_color,
                             int32_t encoded_alpha,
                             uint8_t encoded_bpc,
                             bool has_ctable,
                             int32_t dst_ct,
                             int32_t dst_at,
                             int32_t dst_width,
                             int32_t zero_init,
                             const SkIRect* subset,
                             const SkIRect* frame,
                             RustSwizzlerInit* out_init);

RustSwizzlerSampleXResult skia_rs_swizzler_compute_sample_x(int32_t sample_x,
                                                            int32_t src_offset,
                                                            int32_t dst_offset,
                                                            int32_t src_width,
                                                            int32_t dst_width,
                                                            int32_t src_bpp,
                                                            int32_t dst_bpp,
                                                            bool has_fast_proc);

void skia_rs_swizzler_run(int32_t proc_id,
                          uint8_t* dst,
                          const uint8_t* src,
                          int32_t dst_width,
                          int32_t bpp,
                          int32_t delta_src,
                          int32_t offset,
                          const uint32_t* ctable,
                          bool pmcolor_is_bgra);

bool skia_rs_mask_swizzler_select(int32_t dst_ct,
                                  int32_t dst_at,
                                  int32_t dst_width,
                                  bool src_is_opaque,
                                  uint32_t bits_per_pixel,
                                  const SkIRect* subset,
                                  RustMaskSwizzlerInit* out_init);

void skia_rs_mask_swizzler_compute_sample_x(int32_t src_offset,
                                            int32_t subset_width,
                                            int32_t sample_x,
                                            int32_t* out_x0,
                                            int32_t* out_dst_width);

void skia_rs_mask_swizzler_run(int32_t proc_id,
                               uint8_t* dst,
                               const uint8_t* src,
                               int32_t width,
                               const SkMasks* masks,
                               uint32_t start_x,
                               uint32_t sample_x);

}  // extern "C"

// ============================================================================
// SkColorPalette.cpp
// ============================================================================

SkColorPalette::SkColorPalette(const SkPMColor colors[], int count) {
    SkASSERT(0 == count || colors);
    SkASSERT(count >= 0 && count <= 256);

    fCount = count;
    fColors = reinterpret_cast<SkPMColor*>(sk_malloc_throw(count * sizeof(SkPMColor)));
    if (count > 0) {
        SkAssertResult(skia_rs_color_palette_copy(fColors, colors, count));
    }
}

SkColorPalette::~SkColorPalette() {
    sk_free(fColors);
}

// ============================================================================
// SkSampler.cpp
// ============================================================================

void SkSampler::Fill(const SkImageInfo& info,
                     void* dst,
                     size_t rowBytes,
                     SkCodec::ZeroInitialized zeroInit) {
    SkASSERT(dst != nullptr);
    skia_rs_sampler_fill(static_cast<int32_t>(info.colorType()),
                         info.width(),
                         info.height(),
                         static_cast<uint8_t*>(dst),
                         rowBytes,
                         static_cast<int32_t>(zeroInit));
}

// ============================================================================
// SkSwizzler.cpp
// ============================================================================

namespace {

template <int ProcId>
void rust_swizzle_row_proc(void* SK_RESTRICT dstRow,
                           const uint8_t* SK_RESTRICT src,
                           int dstWidth,
                           int bpp,
                           int deltaSrc,
                           int offset,
                           const SkPMColor ctable[]) {
    skia_rs_swizzler_run(ProcId,
                         static_cast<uint8_t*>(dstRow),
                         src,
                         dstWidth,
                         bpp,
                         deltaSrc,
                         offset,
                         ctable,
                         kPmColorIsBgra);
}

using SwizzleRowProc = void (*)(void* SK_RESTRICT,
                                const uint8_t* SK_RESTRICT,
                                int,
                                int,
                                int,
                                int,
                                const SkPMColor[]);

constexpr SwizzleRowProc kSwizzleProcs[] = {
    &rust_swizzle_row_proc<0>,  &rust_swizzle_row_proc<1>,  &rust_swizzle_row_proc<2>,
    &rust_swizzle_row_proc<3>,  &rust_swizzle_row_proc<4>,  &rust_swizzle_row_proc<5>,
    &rust_swizzle_row_proc<6>,  &rust_swizzle_row_proc<7>,  &rust_swizzle_row_proc<8>,
    &rust_swizzle_row_proc<9>,  &rust_swizzle_row_proc<10>, &rust_swizzle_row_proc<11>,
    &rust_swizzle_row_proc<12>, &rust_swizzle_row_proc<13>, &rust_swizzle_row_proc<14>,
    &rust_swizzle_row_proc<15>, &rust_swizzle_row_proc<16>, &rust_swizzle_row_proc<17>,
    &rust_swizzle_row_proc<18>, &rust_swizzle_row_proc<19>, &rust_swizzle_row_proc<20>,
    &rust_swizzle_row_proc<21>, &rust_swizzle_row_proc<22>, &rust_swizzle_row_proc<23>,
    &rust_swizzle_row_proc<24>, &rust_swizzle_row_proc<25>, &rust_swizzle_row_proc<26>,
    &rust_swizzle_row_proc<27>, &rust_swizzle_row_proc<28>, &rust_swizzle_row_proc<29>,
    &rust_swizzle_row_proc<30>, &rust_swizzle_row_proc<31>, &rust_swizzle_row_proc<32>,
    &rust_swizzle_row_proc<33>, &rust_swizzle_row_proc<34>, &rust_swizzle_row_proc<35>,
    &rust_swizzle_row_proc<36>, &rust_swizzle_row_proc<37>, &rust_swizzle_row_proc<38>,
    &rust_swizzle_row_proc<39>, &rust_swizzle_row_proc<40>, &rust_swizzle_row_proc<41>,
    &rust_swizzle_row_proc<42>, &rust_swizzle_row_proc<43>, &rust_swizzle_row_proc<44>,
    &rust_swizzle_row_proc<45>, &rust_swizzle_row_proc<46>, &rust_swizzle_row_proc<47>,
    &rust_swizzle_row_proc<48>, &rust_swizzle_row_proc<49>, &rust_swizzle_row_proc<50>,
    &rust_swizzle_row_proc<51>, &rust_swizzle_row_proc<52>, &rust_swizzle_row_proc<53>,
    &rust_swizzle_row_proc<54>, &rust_swizzle_row_proc<55>, &rust_swizzle_row_proc<56>,
    &rust_swizzle_row_proc<57>, &rust_swizzle_row_proc<58>,
};

SwizzleRowProc proc_from_id(int32_t id) {
    if (id < 0 || static_cast<size_t>(id) >= std::size(kSwizzleProcs)) {
        return nullptr;
    }
    return kSwizzleProcs[id];
}

}  // namespace

std::unique_ptr<SkSwizzler> SkSwizzler::MakeSimple(int srcBPP,
                                                   const SkImageInfo& dstInfo,
                                                   const SkCodec::Options& options,
                                                   const SkIRect* frame) {
    RustSwizzlerInit init{};
    if (!skia_rs_swizzler_select_simple(srcBPP,
                                        static_cast<int32_t>(dstInfo.colorType()),
                                        dstInfo.width(),
                                        options.fSubset,
                                        frame,
                                        &init)) {
        return nullptr;
    }
    SkASSERT(!(options.fSubset && frame));
    return std::unique_ptr<SkSwizzler>(new SkSwizzler(proc_from_id(init.fast_proc_id),
                                                      proc_from_id(init.slow_proc_id),
                                                      nullptr,
                                                      init.src_offset,
                                                      init.src_width,
                                                      init.dst_offset,
                                                      init.dst_width,
                                                      init.src_bpp,
                                                      init.dst_bpp));
}

std::unique_ptr<SkSwizzler> SkSwizzler::Make(const SkEncodedInfo& encodedInfo,
                                             const SkPMColor* ctable,
                                             const SkImageInfo& dstInfo,
                                             const SkCodec::Options& options,
                                             const SkIRect* frame) {
    RustSwizzlerInit init{};
    if (!skia_rs_swizzler_select(static_cast<int32_t>(encodedInfo.color()),
                                 static_cast<int32_t>(encodedInfo.alpha()),
                                 encodedInfo.bitsPerComponent(),
                                 ctable != nullptr,
                                 static_cast<int32_t>(dstInfo.colorType()),
                                 static_cast<int32_t>(dstInfo.alphaType()),
                                 dstInfo.width(),
                                 static_cast<int32_t>(options.fZeroInitialized),
                                 options.fSubset,
                                 frame,
                                 &init)) {
        return nullptr;
    }
    SkASSERT(!(options.fSubset && frame));
    return std::unique_ptr<SkSwizzler>(new SkSwizzler(proc_from_id(init.fast_proc_id),
                                                      proc_from_id(init.slow_proc_id),
                                                      ctable,
                                                      init.src_offset,
                                                      init.src_width,
                                                      init.dst_offset,
                                                      init.dst_width,
                                                      init.src_bpp,
                                                      init.dst_bpp));
}

std::unique_ptr<SkSwizzler> SkSwizzler::Make(const SkImageInfo& dstInfo,
                                             RowProc fastProc,
                                             RowProc proc,
                                             const SkPMColor* ctable,
                                             int srcBPP,
                                             int dstBPP,
                                             const SkCodec::Options& options,
                                             const SkIRect* frame) {
    int srcOffset = 0;
    int srcWidth = dstInfo.width();
    int dstOffset = 0;
    int dstWidth = srcWidth;
    if (options.fSubset) {
        SkASSERT(!frame);
        srcOffset = options.fSubset->left();
        srcWidth = options.fSubset->width();
        dstWidth = srcWidth;
    } else if (frame) {
        dstOffset = frame->left();
        srcWidth = frame->width();
    }

    return std::unique_ptr<SkSwizzler>(new SkSwizzler(fastProc,
                                                      proc,
                                                      ctable,
                                                      srcOffset,
                                                      srcWidth,
                                                      dstOffset,
                                                      dstWidth,
                                                      srcBPP,
                                                      dstBPP));
}

SkSwizzler::SkSwizzler(RowProc fastProc,
                       RowProc proc,
                       const SkPMColor* ctable,
                       int srcOffset,
                       int srcWidth,
                       int dstOffset,
                       int dstWidth,
                       int srcBPP,
                       int dstBPP)
        : fFastProc(fastProc)
        , fSlowProc(proc)
        , fActualProc(fFastProc ? fFastProc : fSlowProc)
        , fColorTable(ctable)
        , fSrcOffset(srcOffset)
        , fDstOffset(dstOffset)
        , fSrcOffsetUnits(srcOffset * srcBPP)
        , fDstOffsetBytes(dstOffset * dstBPP)
        , fSrcWidth(srcWidth)
        , fDstWidth(dstWidth)
        , fSwizzleWidth(srcWidth)
        , fAllocatedWidth(dstWidth)
        , fSampleX(1)
        , fSrcBPP(srcBPP)
        , fDstBPP(dstBPP) {}

int SkSwizzler::onSetSampleX(int sampleX) {
    SkASSERT(sampleX > 0);
    fSampleX = sampleX;
    RustSwizzlerSampleXResult res = skia_rs_swizzler_compute_sample_x(sampleX,
                                                                      fSrcOffset,
                                                                      fDstOffset,
                                                                      fSrcWidth,
                                                                      fDstWidth,
                                                                      fSrcBPP,
                                                                      fDstBPP,
                                                                      fFastProc != nullptr);
    fDstOffsetBytes = res.dst_offset_bytes;
    fSwizzleWidth = res.swizzle_width;
    fAllocatedWidth = res.allocated_width;
    fSrcOffsetUnits = res.src_offset_units;
#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
    if (res.safety_net_triggered) {
        SkAndroidFrameworkUtils::SafetyNetLog("118143775");
    }
#endif
    fActualProc = res.use_fast_proc ? fFastProc : fSlowProc;
    return fAllocatedWidth;
}

void SkSwizzler::swizzle(void* dst, const uint8_t* SK_RESTRICT src) {
    SkASSERT(nullptr != dst && nullptr != src);
    fActualProc(SkTAddOffset<void>(dst, fDstOffsetBytes),
                src,
                fSwizzleWidth,
                fSrcBPP,
                fSampleX * fSrcBPP,
                fSrcOffsetUnits,
                fColorTable);
}

// ============================================================================
// SkMaskSwizzler.cpp
// ============================================================================

namespace {

template <int ProcId>
void rust_mask_swizzle_row_proc(void* dstRow,
                                const uint8_t* srcRow,
                                int width,
                                SkMasks* masks,
                                uint32_t startX,
                                uint32_t sampleX) {
    skia_rs_mask_swizzler_run(ProcId,
                              static_cast<uint8_t*>(dstRow),
                              srcRow,
                              width,
                              masks,
                              startX,
                              sampleX);
}

using MaskRowProc = void (*)(void*, const uint8_t*, int, SkMasks*, uint32_t, uint32_t);

constexpr MaskRowProc kMaskSwizzleProcs[] = {
    &rust_mask_swizzle_row_proc<0>,  &rust_mask_swizzle_row_proc<1>,
    &rust_mask_swizzle_row_proc<2>,  &rust_mask_swizzle_row_proc<3>,
    &rust_mask_swizzle_row_proc<4>,  &rust_mask_swizzle_row_proc<5>,
    &rust_mask_swizzle_row_proc<6>,  &rust_mask_swizzle_row_proc<7>,
    &rust_mask_swizzle_row_proc<8>,  &rust_mask_swizzle_row_proc<9>,
    &rust_mask_swizzle_row_proc<10>, &rust_mask_swizzle_row_proc<11>,
    &rust_mask_swizzle_row_proc<12>, &rust_mask_swizzle_row_proc<13>,
    &rust_mask_swizzle_row_proc<14>, &rust_mask_swizzle_row_proc<15>,
    &rust_mask_swizzle_row_proc<16>, &rust_mask_swizzle_row_proc<17>,
    &rust_mask_swizzle_row_proc<18>, &rust_mask_swizzle_row_proc<19>,
    &rust_mask_swizzle_row_proc<20>,
};

MaskRowProc mask_proc_from_id(int32_t id) {
    if (id < 0 || static_cast<size_t>(id) >= std::size(kMaskSwizzleProcs)) {
        return nullptr;
    }
    return kMaskSwizzleProcs[id];
}

}  // namespace

SkMaskSwizzler* SkMaskSwizzler::CreateMaskSwizzler(const SkImageInfo& dstInfo,
                                                   bool srcIsOpaque,
                                                   SkMasks* masks,
                                                   uint32_t bitsPerPixel,
                                                   const SkCodec::Options& options) {
    RustMaskSwizzlerInit init{};
    if (!skia_rs_mask_swizzler_select(static_cast<int32_t>(dstInfo.colorType()),
                                      static_cast<int32_t>(dstInfo.alphaType()),
                                      dstInfo.width(),
                                      srcIsOpaque,
                                      bitsPerPixel,
                                      options.fSubset,
                                      &init)) {
        SkASSERT(false);
        return nullptr;
    }
    return new SkMaskSwizzler(masks, mask_proc_from_id(init.proc_id), init.src_offset,
                              init.subset_width);
}

SkMaskSwizzler::SkMaskSwizzler(SkMasks* masks, RowProc proc, int srcOffset, int subsetWidth)
        : fMasks(masks)
        , fRowProc(proc)
        , fSubsetWidth(subsetWidth)
        , fDstWidth(subsetWidth)
        , fSampleX(1)
        , fSrcOffset(srcOffset)
        , fX0(srcOffset) {}

int SkMaskSwizzler::onSetSampleX(int sampleX) {
    SkASSERT(sampleX > 0);
    fSampleX = sampleX;
    skia_rs_mask_swizzler_compute_sample_x(fSrcOffset, fSubsetWidth, sampleX, &fX0, &fDstWidth);
    SkASSERT(fX0 >= 0);
    return fDstWidth;
}

void SkMaskSwizzler::swizzle(void* dst, const uint8_t* SK_RESTRICT src) {
    SkASSERT(nullptr != dst && nullptr != src);
    fRowProc(dst, src, fDstWidth, fMasks, fX0, fSampleX);
}
