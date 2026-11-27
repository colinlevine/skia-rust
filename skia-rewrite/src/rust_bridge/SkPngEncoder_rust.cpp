/*
 * Safe Rust bridge for:
 *   src/encode/SkPngEncoderImpl.cpp
 * (Phase 2 Module P2.4: skia-rs-codec-png, GN target: png_encode_libpng).
 *
 * Keeps thin third_party/libpng boundary calls (`png_*` and `setjmp`) while
 * delegating pixmap validation, IHDR/sBIT/filter/zlib/comment/colorspace/HDR/gainmap
 * planning to safe Rust in `skia-rs-codec-png`.
 */

#include "src/encode/SkPngEncoderImpl.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <optional>
#include <setjmp.h>
#include <utility>
#include <vector>

#include "include/core/SkBitmap.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkDataTable.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/core/SkString.h"
#include "include/encode/SkEncoder.h"
#include "include/encode/SkICC.h"
#include "include/encode/SkPngEncoder.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/SkHdrMetadata.h"
#include "include/private/SkNoncopyable.h"
#include "src/codec/SkPngPriv.h"
#include "src/encode/SkPngEncoderBase.h"
#include "src/image/SkImage_Base.h"

#include <png.h>
#include <pngconf.h>

class GrDirectContext;
class SkImage;

namespace {

// Discriminant assertions matching `skia-rs-codec-png` and `libpng`.
static_assert(PNG_FILTER_NONE == static_cast<int>(SkPngEncoder::FilterFlag::kNone));
static_assert(PNG_FILTER_SUB == static_cast<int>(SkPngEncoder::FilterFlag::kSub));
static_assert(PNG_FILTER_UP == static_cast<int>(SkPngEncoder::FilterFlag::kUp));
static_assert(PNG_FILTER_AVG == static_cast<int>(SkPngEncoder::FilterFlag::kAvg));
static_assert(PNG_FILTER_PAETH == static_cast<int>(SkPngEncoder::FilterFlag::kPaeth));
static_assert(PNG_ALL_FILTERS == static_cast<int>(SkPngEncoder::FilterFlag::kAll));

static_assert(PNG_FILTER_NONE == 0x08);
static_assert(PNG_FILTER_SUB == 0x10);
static_assert(PNG_FILTER_UP == 0x20);
static_assert(PNG_FILTER_AVG == 0x40);
static_assert(PNG_FILTER_PAETH == 0x80);
static_assert(PNG_ALL_FILTERS == 0xF8);
static_assert(PNG_KEYWORD_MAX_LENGTH == 79);

static_assert(PNG_COLOR_TYPE_GRAY == 0);
static_assert(PNG_COLOR_TYPE_RGB == 2);
static_assert(PNG_COLOR_TYPE_GRAY_ALPHA == 4);
static_assert(PNG_COLOR_TYPE_RGB_ALPHA == 6);
static_assert(kGraySigBit_GrayAlphaIsJustAlpha == 1);

static_assert(static_cast<int>(SkEncodedInfo::kGray_Color) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kGrayAlpha_Color) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kRGB_Color) == 5);
static_assert(static_cast<int>(SkEncodedInfo::kRGBA_Color) == 6);

static_assert(static_cast<int>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int>(kAlpha_8_SkColorType) == 1);
static_assert(static_cast<int>(kRGB_565_SkColorType) == 2);
static_assert(static_cast<int>(kARGB_4444_SkColorType) == 3);
static_assert(static_cast<int>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int>(kRGB_888x_SkColorType) == 5);
static_assert(static_cast<int>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int>(kRGBA_F32_SkColorType) == 18);
static_assert(static_cast<int>(kBGR_888x_SkColorType) == 29);

static_assert(static_cast<int>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int>(kUnpremul_SkAlphaType) == 3);

struct SkiaRsPngSigBit {
    bool set;
    uint8_t red;
    uint8_t green;
    uint8_t blue;
    uint8_t gray;
    uint8_t alpha;
};

struct SkiaRsPngEncodeHeaderPlan {
    bool valid;
    int32_t png_color_type;
    SkiaRsPngSigBit sig_bit;
    int32_t filters;
    bool filters_exact;
    int32_t zlib_level;
    bool zlib_level_exact;
    bool needs_filler_after;
    bool needs_byte_swap;
};

enum SkiaRsPngColorSpaceEncodeAction : int32_t {
    kColorSpace_WriteSrgb = 0,
    kColorSpace_WriteIcc = 1,
};

enum SkiaRsPngGainmapEncodeMode : int32_t {
    kGainmap_None = 0,
    kGainmap_BaseWithGainmap = 1,
    kGainmap_InfoOnly = 2,
};

extern "C" {
bool skia_rs_png_encoder_validate_pixmap(int32_t width,
                                         int32_t height,
                                         int32_t ct,
                                         int32_t at,
                                         bool has_addr,
                                         size_t row_bytes);
bool skia_rs_png_encoder_plan_header(int32_t dst_color,
                                     int32_t dst_bpc,
                                     bool dst_row_is_opaque,
                                     int32_t src_ct,
                                     int32_t filter_flags,
                                     int32_t zlib_level,
                                     SkiaRsPngEncodeHeaderPlan* out_plan);
bool skia_rs_png_encoder_validate_comments_count(int32_t count);
size_t skia_rs_png_encoder_clip_keyword_len(size_t keyword_len);
int32_t skia_rs_png_encoder_plan_color_space(bool has_cs, bool cs_is_srgb, bool has_hdr_metadata);
int32_t skia_rs_png_encoder_plan_gainmap(bool has_gainmap_info, bool has_gainmap_pixels);
bool skia_rs_png_encoder_gainmap_supports_rgb_icc(int32_t dst_color);
}

constexpr bool kSuppressPngEncodeWarnings = false;

void sk_error_fn(png_structp png_ptr, png_const_charp msg) {
    if (!kSuppressPngEncodeWarnings) {
        SkDebugf("libpng encode error: %s\n", msg);
    }
    longjmp(png_jmpbuf(png_ptr), 1);
}

void sk_write_fn(png_structp png_ptr, png_bytep data, png_size_t len) {
    SkWStream* stream = static_cast<SkWStream*>(png_get_io_ptr(png_ptr));
    if (!stream->write(data, len)) {
        png_error(png_ptr, "sk_write_fn cannot write to stream");
    }
}

void set_icc(png_structp png_ptr,
             png_infop info_ptr,
             const SkImageInfo& info,
             const SkPngEncoder::Options& options) {
    sk_sp<SkData> icc = SkWriteICCProfile(info.colorSpace(), options.fHdrMetadata);
    if (!icc) {
        return;
    }

#if PNG_LIBPNG_VER_MAJOR > 1 || (PNG_LIBPNG_VER_MAJOR == 1 && PNG_LIBPNG_VER_MINOR >= 5)
    const char* name = "Skia";
    png_const_bytep iccPtr = icc->bytes();
#else
    SkString str("Skia");
    char* name = str.data();
    png_charp iccPtr = (png_charp)icc->writable_data();
#endif
    png_set_iCCP(png_ptr, info_ptr, name, 0, iccPtr, icc->size());
}

}  // namespace

class SkPngEncoderMgr final : SkNoncopyable {
public:
    static std::unique_ptr<SkPngEncoderMgr> Make(SkWStream* stream);
    bool setHeader(const SkPngEncoderBase::TargetInfo& targetInfo,
                   const SkImageInfo& srcInfo,
                   const SkPngEncoder::Options& options);
    bool setColorSpace(const SkImageInfo& info, const SkPngEncoder::Options& options);
    bool setHdrMetadata(const SkPngEncoder::Options& options);
    bool writeInfo(const SkImageInfo& srcInfo, const SkPngEncoderBase::TargetInfo& targetInfo);

    png_structp pngPtr() { return fPngPtr; }
    png_infop infoPtr() { return fInfoPtr; }
    bool needsByteSwap() const { return fNeedsByteSwap; }

    ~SkPngEncoderMgr() { png_destroy_write_struct(&fPngPtr, &fInfoPtr); }

private:
    SkPngEncoderMgr(png_structp pngPtr, png_infop infoPtr) : fPngPtr(pngPtr), fInfoPtr(infoPtr) {}

    png_structp fPngPtr;
    png_infop fInfoPtr;
    bool fNeedsFillerAfter = false;
    bool fNeedsByteSwap = false;
};

std::unique_ptr<SkPngEncoderMgr> SkPngEncoderMgr::Make(SkWStream* stream) {
    png_structp pngPtr =
            png_create_write_struct(PNG_LIBPNG_VER_STRING, nullptr, sk_error_fn, nullptr);
    if (!pngPtr) {
        return nullptr;
    }

    png_infop infoPtr = png_create_info_struct(pngPtr);
    if (!infoPtr) {
        png_destroy_write_struct(&pngPtr, nullptr);
        return nullptr;
    }

    png_set_write_fn(pngPtr, static_cast<void*>(stream), sk_write_fn, nullptr);
    return std::unique_ptr<SkPngEncoderMgr>(new SkPngEncoderMgr(pngPtr, infoPtr));
}

bool SkPngEncoderMgr::setHeader(const SkPngEncoderBase::TargetInfo& targetInfo,
                                const SkImageInfo& srcInfo,
                                const SkPngEncoder::Options& options) {
    if (setjmp(png_jmpbuf(fPngPtr))) {
        return false;
    }

    const SkEncodedInfo& dstInfo = targetInfo.fDstInfo;
    const std::optional<SkImageInfo>& dstRowInfo = targetInfo.fDstRowInfo;
    const bool dstRowIsOpaque = dstRowInfo.has_value() && dstRowInfo->isOpaque();

    SkiaRsPngEncodeHeaderPlan plan{};
    if (!skia_rs_png_encoder_plan_header(static_cast<int32_t>(dstInfo.color()),
                                         dstInfo.bitsPerComponent(),
                                         dstRowIsOpaque,
                                         static_cast<int32_t>(srcInfo.colorType()),
                                         static_cast<int32_t>(options.fFilterFlags),
                                         options.fZLibLevel,
                                         &plan) ||
        !plan.valid) {
        SkDEBUGFAIL("`getTargetInfo` returned unexpected `SkEncodedInfo::Color`");
        return false;
    }

    fNeedsFillerAfter = plan.needs_filler_after;
    fNeedsByteSwap = plan.needs_byte_swap;

    png_set_IHDR(fPngPtr,
                 fInfoPtr,
                 srcInfo.width(),
                 srcInfo.height(),
                 dstInfo.bitsPerComponent(),
                 plan.png_color_type,
                 PNG_INTERLACE_NONE,
                 PNG_COMPRESSION_TYPE_BASE,
                 PNG_FILTER_TYPE_BASE);

    if (plan.sig_bit.set) {
        png_color_8 sigBit{};
        sigBit.red = plan.sig_bit.red;
        sigBit.green = plan.sig_bit.green;
        sigBit.blue = plan.sig_bit.blue;
        sigBit.gray = plan.sig_bit.gray;
        sigBit.alpha = plan.sig_bit.alpha;
        png_set_sBIT(fPngPtr, fInfoPtr, &sigBit);
    } else {
        SkDEBUGFAIL("Unable to set sigBit for src colortype, unhandled value\n");
    }

    SkASSERT(plan.filters_exact);
    png_set_filter(fPngPtr, PNG_FILTER_TYPE_BASE, plan.filters);

    SkASSERT(plan.zlib_level_exact);
    png_set_compression_level(fPngPtr, plan.zlib_level);

    const sk_sp<SkDataTable>& comments = options.fComments;
    if (comments != nullptr) {
        if (!skia_rs_png_encoder_validate_comments_count(comments->count())) {
            return false;
        }

        const int numPairs = comments->count() / 2;
        std::vector<png_text> png_texts(numPairs);
        std::vector<SkString> clippedKeys;
        clippedKeys.reserve(numPairs);
        for (int i = 0; i < numPairs; ++i) {
            const char* originalKeyword = comments->atStr(2 * i);
            const char* text = comments->atStr(2 * i + 1);
            const size_t origLen = strlen(originalKeyword);
            const size_t clippedLen = skia_rs_png_encoder_clip_keyword_len(origLen);
            const char* keyword = originalKeyword;
            if (clippedLen < origLen) {
                SkDEBUGFAILF("PNG tEXt keyword should be no longer than %d.",
                             PNG_KEYWORD_MAX_LENGTH);
                clippedKeys.emplace_back(originalKeyword, clippedLen);
                keyword = clippedKeys.back().c_str();
            }
            png_texts[i].compression = PNG_TEXT_COMPRESSION_NONE;
            png_texts[i].key = const_cast<png_charp>(keyword);
            png_texts[i].text = const_cast<png_charp>(text);
        }
        png_set_text(fPngPtr, fInfoPtr, png_texts.data(), static_cast<int>(png_texts.size()));
    }

    return true;
}

bool SkPngEncoderMgr::setColorSpace(const SkImageInfo& info, const SkPngEncoder::Options& options) {
    if (setjmp(png_jmpbuf(fPngPtr))) {
        return false;
    }

    const bool hasCs = (info.colorSpace() != nullptr);
    const bool isSrgb = hasCs && info.colorSpace()->isSRGB();
    const auto action = static_cast<SkiaRsPngColorSpaceEncodeAction>(
            skia_rs_png_encoder_plan_color_space(hasCs, isSrgb, options.fHdrMetadata != nullptr));
    if (action == kColorSpace_WriteSrgb) {
        png_set_sRGB(fPngPtr, fInfoPtr, PNG_sRGB_INTENT_PERCEPTUAL);
    } else {
        set_icc(fPngPtr, fInfoPtr, info, options);
    }

    return true;
}

bool SkPngEncoderMgr::setHdrMetadata(const SkPngEncoder::Options& options) {
#ifdef PNG_STORE_UNKNOWN_CHUNKS_SUPPORTED
    if (setjmp(png_jmpbuf(fPngPtr))) {
        return false;
    }

    const char* hdrChunkNames = "gmAP\0"
                                "gdAT\0"
                                "mDCV\0"
                                "cLLI\0";
    constexpr int numHdrChunkNames = 4;
    png_set_keep_unknown_chunks(
            fPngPtr, PNG_HANDLE_CHUNK_ALWAYS, (png_const_bytep)hdrChunkNames, numHdrChunkNames);

    if (options.fHdrMetadata) {
        skhdr::ContentLightLevelInformation clli;
        if (options.fHdrMetadata->getContentLightLevelInformation(&clli)) {
            auto data = clli.serializePngChunk();
            png_unknown_chunk chunk = {
                    {'c', 'L', 'L', 'I', 0},
                    reinterpret_cast<png_byte*>(data->writable_data()),
                    data->size(),
                    PNG_HAVE_IHDR,
            };
            png_set_unknown_chunks(fPngPtr, fInfoPtr, &chunk, 1);
        }

        skhdr::MasteringDisplayColorVolume mdcv;
        if (options.fHdrMetadata->getMasteringDisplayColorVolume(&mdcv)) {
            auto data = mdcv.serialize();
            png_unknown_chunk chunk = {
                    {'m', 'D', 'C', 'V', 0},
                    reinterpret_cast<png_byte*>(data->writable_data()),
                    data->size(),
                    PNG_HAVE_IHDR,
            };
            png_set_unknown_chunks(fPngPtr, fInfoPtr, &chunk, 1);
        }
    }

    const auto gainmapMode = static_cast<SkiaRsPngGainmapEncodeMode>(
            skia_rs_png_encoder_plan_gainmap(options.fGainmapInfo != nullptr,
                                             options.fGainmap != nullptr));
    if (gainmapMode == kGainmap_BaseWithGainmap) {
        sk_sp<SkData> gainmapVersion = SkGainmapInfo::SerializeVersion();

        auto modifiedOptions = options;
        modifiedOptions.fGainmap = nullptr;
        modifiedOptions.fHdrMetadata = nullptr;

        auto gainmapInfo = *(options.fGainmapInfo);
        auto gainmapPixels = *(options.fGainmap);
        auto targetInfo = SkPngEncoderBase::getTargetInfo(gainmapPixels.info());

        if (targetInfo && skia_rs_png_encoder_gainmap_supports_rgb_icc(
                                  static_cast<int32_t>(targetInfo->fDstInfo.color()))) {
            const auto& gainmapColorSpace = options.fGainmapInfo->fGainmapMathColorSpace;
            gainmapPixels.setColorSpace(gainmapColorSpace);
        } else {
            gainmapInfo.fGainmapMathColorSpace = nullptr;
            modifiedOptions.fGainmapInfo = &gainmapInfo;
        }

        sk_sp<SkData> gainmapData = SkPngEncoder::Encode(gainmapPixels, modifiedOptions);
        if (!gainmapData) {
            return false;
        }

        png_unknown_chunk gmapChunk = {
                {'g', 'm', 'A', 'P', 0},
                reinterpret_cast<png_byte*>(gainmapVersion->writable_data()),
                gainmapVersion->size(),
                PNG_HAVE_IHDR,
        };
        png_set_unknown_chunks(fPngPtr, fInfoPtr, &gmapChunk, 1);

        png_unknown_chunk gdatChunk = {
                {'g', 'd', 'A', 'T', 0},
                reinterpret_cast<png_byte*>(gainmapData->writable_data()),
                gainmapData->size(),
                PNG_HAVE_IHDR,
        };
        png_set_unknown_chunks(fPngPtr, fInfoPtr, &gdatChunk, 1);
    } else if (gainmapMode == kGainmap_InfoOnly) {
        sk_sp<SkData> data = options.fGainmapInfo->serialize();
        png_unknown_chunk chunk = {
                {'g', 'm', 'A', 'P', 0},
                reinterpret_cast<png_byte*>(data->writable_data()),
                data->size(),
                PNG_HAVE_IHDR,
        };
        png_set_unknown_chunks(fPngPtr, fInfoPtr, &chunk, 1);
    }
#endif
    return true;
}

bool SkPngEncoderMgr::writeInfo(const SkImageInfo& /*srcInfo*/,
                                const SkPngEncoderBase::TargetInfo& /*targetInfo*/) {
    if (setjmp(png_jmpbuf(fPngPtr))) {
        return false;
    }
    png_write_info(fPngPtr, fInfoPtr);

    if (fNeedsFillerAfter) {
        png_set_filler(fPngPtr, 0, PNG_FILLER_AFTER);
    }
    return true;
}

SkPngEncoderImpl::SkPngEncoderImpl(TargetInfo targetInfo,
                                   std::unique_ptr<SkPngEncoderMgr> encoderMgr,
                                   const SkPixmap& src)
        : SkPngEncoderBase(std::move(targetInfo), src), fEncoderMgr(std::move(encoderMgr)) {}

SkPngEncoderImpl::~SkPngEncoderImpl() {}

bool SkPngEncoderImpl::onEncodeRow(SkSpan<const uint8_t> row) {
    if (setjmp(png_jmpbuf(fEncoderMgr->pngPtr()))) {
        return false;
    }

    png_bytep rowPtr = const_cast<png_bytep>(row.data());
    if (fEncoderMgr->needsByteSwap()) {
        png_set_swap(fEncoderMgr->pngPtr());
    }

    png_write_rows(fEncoderMgr->pngPtr(), &rowPtr, 1);
    return true;
}

bool SkPngEncoderImpl::onFinishEncoding() {
    if (setjmp(png_jmpbuf(fEncoderMgr->pngPtr()))) {
        return false;
    }

    png_write_end(fEncoderMgr->pngPtr(), fEncoderMgr->infoPtr());
    return true;
}

namespace SkPngEncoder {
std::unique_ptr<SkEncoder> Make(SkWStream* dst, const SkPixmap& src, const Options& options) {
    if (!skia_rs_png_encoder_validate_pixmap(src.width(),
                                             src.height(),
                                             static_cast<int32_t>(src.colorType()),
                                             static_cast<int32_t>(src.alphaType()),
                                             src.addr() != nullptr,
                                             src.rowBytes())) {
        return nullptr;
    }

    std::unique_ptr<SkPngEncoderMgr> encoderMgr = SkPngEncoderMgr::Make(dst);
    if (!encoderMgr) {
        return nullptr;
    }

    std::optional<SkPngEncoderBase::TargetInfo> targetInfo =
            SkPngEncoderBase::getTargetInfo(src.info());
    if (!targetInfo.has_value()) {
        return nullptr;
    }

    if (!encoderMgr->setHeader(targetInfo.value(), src.info(), options)) {
        return nullptr;
    }

    if (!encoderMgr->setColorSpace(src.info(), options)) {
        return nullptr;
    }

    if (!encoderMgr->setHdrMetadata(options)) {
        return nullptr;
    }

    if (!encoderMgr->writeInfo(src.info(), targetInfo.value())) {
        return nullptr;
    }
    return std::make_unique<SkPngEncoderImpl>(
            std::move(*targetInfo), std::move(encoderMgr), src);
}

bool Encode(SkWStream* dst, const SkPixmap& src, const Options& options) {
    auto encoder = Make(dst, src, options);
    return encoder.get() && encoder->encodeRows(src.height());
}

sk_sp<SkData> Encode(const SkPixmap& src, const Options& options) {
    SkDynamicMemoryWStream stream;
    return Encode(&stream, src, options) ? stream.detachAsData() : nullptr;
}

sk_sp<SkData> Encode(GrDirectContext* ctx, const SkImage* img, const Options& options) {
    if (!img) {
        return nullptr;
    }
    SkBitmap bm;
    if (!as_IB(img)->getROPixels(ctx, &bm)) {
        return nullptr;
    }
    return Encode(bm.pixmap(), options);
}

}  // namespace SkPngEncoder
