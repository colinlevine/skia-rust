/*
 * Safe Rust bridge for:
 *   src/codec/SkPngCodecBase.cpp
 *   src/codec/SkPngCompositeChunkReader.cpp
 * (Phase 2 Module P2.4: skia-rs-codec-png, GN target: png_decode_common).
 */

#include "src/codec/SkPngCodecBase.h"
#include "src/codec/SkPngCompositeChunkReader.h"

#include <cstddef>
#include <cstdint>
#include <tuple>
#include <utility>

#include "include/codec/SkCodec.h"
#include "include/codec/SkEncodedImageFormat.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRect.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/private/SkAssert.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/SkHdrMetadata.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkColorPalette.h"
#include "src/codec/SkSwizzler.h"

namespace {

#ifdef SK_PMCOLOR_IS_RGBA
constexpr bool kPmColorIsRgba = true;
#else
constexpr bool kPmColorIsRgba = false;
#endif

// Discriminant verification between C++ enums and Rust `#[repr(i32)]` enums.
static_assert(static_cast<int>(SkEncodedInfo::kGray_Color) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kGrayAlpha_Color) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kXAlpha_Color) == 2);
static_assert(static_cast<int>(SkEncodedInfo::k565_Color) == 3);
static_assert(static_cast<int>(SkEncodedInfo::kPalette_Color) == 4);
static_assert(static_cast<int>(SkEncodedInfo::kRGB_Color) == 5);
static_assert(static_cast<int>(SkEncodedInfo::kRGBA_Color) == 6);
static_assert(static_cast<int>(SkEncodedInfo::kBGR_Color) == 7);
static_assert(static_cast<int>(SkEncodedInfo::kBGRX_Color) == 8);
static_assert(static_cast<int>(SkEncodedInfo::kBGRA_Color) == 9);
static_assert(static_cast<int>(SkEncodedInfo::kYUV_Color) == 10);
static_assert(static_cast<int>(SkEncodedInfo::kYUVA_Color) == 11);
static_assert(static_cast<int>(SkEncodedInfo::kInvertedCMYK_Color) == 12);
static_assert(static_cast<int>(SkEncodedInfo::kYCCK_Color) == 13);

static_assert(static_cast<int>(SkEncodedInfo::kOpaque_Alpha) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kUnpremul_Alpha) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kBinary_Alpha) == 2);

static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kRGB) == 0);
static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kCMYK) == 1);
static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kGray) == 2);
static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kOther) == 3);

static_assert(static_cast<int>(SkCodec::kSuccess) == 0);
static_assert(static_cast<int>(SkCodec::kIncompleteInput) == 1);
static_assert(static_cast<int>(SkCodec::kErrorInInput) == 2);
static_assert(static_cast<int>(SkCodec::kInvalidConversion) == 3);
static_assert(static_cast<int>(SkCodec::kInvalidScale) == 4);
static_assert(static_cast<int>(SkCodec::kInvalidParameters) == 5);
static_assert(static_cast<int>(SkCodec::kInvalidInput) == 6);
static_assert(static_cast<int>(SkCodec::kCouldNotRewind) == 7);
static_assert(static_cast<int>(SkCodec::kInternalError) == 8);
static_assert(static_cast<int>(SkCodec::kUnimplemented) == 9);
static_assert(static_cast<int>(SkCodec::kOutOfMemory) == 10);

static_assert(static_cast<int>(SkCodec::kYes_ZeroInitialized) == 0);
static_assert(static_cast<int>(SkCodec::kNo_ZeroInitialized) == 1);

static_assert(static_cast<int>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int>(kUnpremul_SkAlphaType) == 3);

static_assert(static_cast<int>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int>(kBGR_888x_SkColorType) == 29);

static_assert(static_cast<int>(skcms_PixelFormat_G_8) == 2);
static_assert(static_cast<int>(skcms_PixelFormat_RGBA_8888) == 12);
static_assert(static_cast<int>(skcms_PixelFormat_RGB_161616BE) == 22);
static_assert(static_cast<int>(skcms_PixelFormat_RGBA_16161616BE) == 24);

struct SkiaRsPngXformInitPlan {
    int32_t result;
    size_t encoded_row_bytes;
    bool skip_format_conversion;
    bool color_only_mode;
    bool need_color_table;
};

struct SkiaRsPngSwizzlerPlan {
    int32_t xform_mode;
    int32_t swizzler_ct;
    int32_t swizzler_at;
    int32_t swizzler_zero_init;
    bool use_frame_rect;
    bool use_simple_swizzler;
    int32_t simple_src_bpp;
};

struct SkiaRsPngColorTablePopulateResult {
    size_t num_colors;
    size_t max_colors;
    bool should_xform_table;
};

enum SkiaRsPngUnknownChunkKind : int32_t {
    kUnknownChunk_Ignored = 0,
    kUnknownChunk_GainmapInfo = 1,
    kUnknownChunk_GainmapData = 2,
    kUnknownChunk_ContentLightLevelInfo = 3,
    kUnknownChunk_MasteringDisplayColorVolume = 4,
};

extern "C" {
bool skia_rs_png_is_png(const uint8_t* buf, size_t len);
bool skia_rs_png_is_compatible_color_profile(bool has_profile,
                                             int32_t data_space,
                                             int32_t encoded_color);
int32_t skia_rs_png_to_skcms_pixel_format(int32_t encoded_color, uint8_t bits_per_component);
bool skia_rs_png_plan_xform_init(int32_t encoded_color,
                                 uint8_t encoded_bpc,
                                 uint32_t encoded_bpp,
                                 int32_t dst_width,
                                 int32_t frame_width,
                                 bool has_subset,
                                 bool has_color_xform,
                                 SkiaRsPngXformInitPlan* out_plan);
bool skia_rs_png_plan_swizzler(int32_t encoded_color,
                               uint8_t encoded_bpc,
                               int32_t dst_ct,
                               int32_t dst_at,
                               int32_t zero_init,
                               bool has_color_xform,
                               bool xform_on_decode,
                               bool skip_format_conversion,
                               bool has_subset,
                               SkiaRsPngSwizzlerPlan* out_plan);
bool skia_rs_png_compute_dst_row_size(int32_t xform_width,
                                      uint32_t dst_bytes_per_pixel,
                                      size_t* out_size);
size_t skia_rs_png_compute_storage_bytes(int32_t xform_mode,
                                         uint32_t encoded_bpp,
                                         int32_t dst_width);
bool skia_rs_png_populate_color_table(const uint8_t* plte_rgb,
                                      size_t plte_entries,
                                      const uint8_t* trns_alphas,
                                      size_t trns_len,
                                      uint8_t bits_per_component,
                                      int32_t encoded_alpha,
                                      int32_t dst_ct,
                                      int32_t dst_at,
                                      bool has_color_xform,
                                      bool xform_on_decode,
                                      bool pmcolor_is_rgba,
                                      uint32_t* out_table,
                                      SkiaRsPngColorTablePopulateResult* out_result);
void skia_rs_png_pad_color_table(uint32_t* table, size_t num_colors, size_t max_colors);
int32_t skia_rs_png_classify_unknown_chunk(const char* tag, bool data_is_null, size_t data_len);
}

skcms_PixelFormat ToPixelFormat(const SkEncodedInfo& info) {
    return static_cast<skcms_PixelFormat>(skia_rs_png_to_skcms_pixel_format(
            static_cast<int32_t>(info.color()), info.bitsPerComponent()));
}

}  // namespace

// ============================================================================
// SkPngCodecBase
// ============================================================================

SkPngCodecBase::~SkPngCodecBase() = default;

// static
bool SkPngCodecBase::IsPng(const void* buf, size_t len) {
    return skia_rs_png_is_png(static_cast<const uint8_t*>(buf), len);
}

// static
bool SkPngCodecBase::isCompatibleColorProfileAndType(const SkCodecs::ColorProfile* profile,
                                                     SkEncodedInfo::Color color) {
    const bool hasProfile = (profile != nullptr);
    const int32_t ds = hasProfile ? static_cast<int32_t>(profile->dataSpace()) : 0;
    return skia_rs_png_is_compatible_color_profile(hasProfile, ds, static_cast<int32_t>(color));
}

SkPngCodecBase::SkPngCodecBase(SkEncodedInfo&& encodedInfo,
                               std::unique_ptr<SkStream> stream,
                               SkEncodedOrigin origin,
                               sk_sp<SkPngCompositeChunkReader> chunkReader,
                               std::unique_ptr<SkStream> gainmapStream,
                               std::optional<SkGainmapInfo> gainmapInfo)
        : SkCodec(std::move(encodedInfo), ToPixelFormat(encodedInfo), std::move(stream), origin)
        , fPngChunkReader(std::move(chunkReader))
        , fGainmapStream(std::move(gainmapStream))
        , fGainmapInfo(gainmapInfo) {}

SkEncodedImageFormat SkPngCodecBase::onGetEncodedFormat() const {
    return SkEncodedImageFormat::kPNG;
}

SkCodec::Result SkPngCodecBase::initializeXforms(const SkImageInfo& dstInfo,
                                                 const Options& options,
                                                 int frameWidth) {
    static_assert(static_cast<int>(kSwizzleOnly_XformMode) == 0);
    static_assert(static_cast<int>(kColorOnly_XformMode) == 1);
    static_assert(static_cast<int>(kSwizzleColor_XformMode) == 2);

    SkiaRsPngXformInitPlan plan{};
    const bool ok = skia_rs_png_plan_xform_init(
            static_cast<int32_t>(this->getEncodedInfo().color()),
            this->getEncodedInfo().bitsPerComponent(),
            this->getEncodedInfo().bitsPerPixel(),
            dstInfo.width(),
            frameWidth,
            options.fSubset != nullptr,
            this->colorXform(),
            &plan);
    SkASSERT_RELEASE(ok);
    if (plan.result != static_cast<int32_t>(kSuccess)) {
        return static_cast<Result>(plan.result);
    }

    fXformWidth = frameWidth;
    fEncodedRowBytes = plan.encoded_row_bytes;
    fSwizzler.reset(nullptr);

    if (plan.color_only_mode) {
        fXformMode = kColorOnly_XformMode;
    } else {
        if (plan.need_color_table) {
            if (!this->createColorTable(dstInfo)) {
                return kInvalidInput;
            }
        }
        Result result = this->initializeSwizzler(
                dstInfo, options, plan.skip_format_conversion, frameWidth);
        if (result != kSuccess) {
            return result;
        }
    }

    if (Result result = this->allocateStorage(dstInfo); result != kSuccess) {
        return result;
    }

    return kSuccess;
}

void SkPngCodecBase::initializeXformParams() {
    switch (fXformMode) {
        case kSwizzleOnly_XformMode:
        case kSwizzleColor_XformMode:
            fXformWidth = this->swizzler()->swizzleWidth();
            break;
        case kColorOnly_XformMode:
            SkASSERT_RELEASE(!fSwizzler);
            break;
    }

    const bool ok = skia_rs_png_compute_dst_row_size(
            fXformWidth, static_cast<uint32_t>(this->dstInfo().bytesPerPixel()), &fDstRowSize);
    SkASSERT_RELEASE(ok);
}

SkCodec::Result SkPngCodecBase::allocateStorage(const SkImageInfo& dstInfo) {
    fStorage.reset();
    if (fXformMode == kSwizzleOnly_XformMode) {
        return kSuccess;
    }
    const size_t colorXformBytes = skia_rs_png_compute_storage_bytes(
            static_cast<int32_t>(fXformMode),
            this->getEncodedInfo().bitsPerPixel(),
            dstInfo.width());
    return fStorage.allocateFromBudget(this, colorXformBytes, /*zeroInit=*/false);
}

SkCodec::Result SkPngCodecBase::initializeSwizzler(const SkImageInfo& dstInfo,
                                                   const Options& options,
                                                   bool skipFormatConversion,
                                                   int frameWidth) {
    SkiaRsPngSwizzlerPlan plan{};
    const bool ok = skia_rs_png_plan_swizzler(
            static_cast<int32_t>(this->getEncodedInfo().color()),
            this->getEncodedInfo().bitsPerComponent(),
            static_cast<int32_t>(dstInfo.colorType()),
            static_cast<int32_t>(dstInfo.alphaType()),
            static_cast<int32_t>(options.fZeroInitialized),
            this->colorXform(),
            this->xformOnDecode(),
            skipFormatConversion,
            options.fSubset != nullptr,
            &plan);
    SkASSERT_RELEASE(ok);

    fXformMode = static_cast<XformMode>(plan.xform_mode);
    SkImageInfo swizzlerInfo = dstInfo.makeColorType(static_cast<SkColorType>(plan.swizzler_ct))
                                      .makeAlphaType(static_cast<SkAlphaType>(plan.swizzler_at));
    Options swizzlerOptions = options;
    swizzlerOptions.fZeroInitialized = static_cast<ZeroInitialized>(plan.swizzler_zero_init);

    SkIRect frameRect = SkIRect::MakeWH(frameWidth, 1);
    const SkIRect* frameRectPtr = nullptr;
    if (!plan.use_frame_rect) {
        SkASSERT_RELEASE(frameWidth == dstInfo.width());
    } else {
        frameRectPtr = &frameRect;
    }

    if (plan.use_simple_swizzler) {
        SkASSERT_RELEASE(!fColorTable);
        SkASSERT_RELEASE(plan.simple_src_bpp > 0);
        fSwizzler = SkSwizzler::MakeSimple(
                plan.simple_src_bpp, swizzlerInfo, swizzlerOptions, frameRectPtr);
    } else {
        const SkPMColor* colors = SkCodecPriv::GetColorPtr(fColorTable.get());
        fSwizzler = SkSwizzler::Make(
                this->getEncodedInfo(), colors, swizzlerInfo, swizzlerOptions, frameRectPtr);
    }

    return !!fSwizzler ? kSuccess : kUnimplemented;
}

SkSampler* SkPngCodecBase::getSampler(bool createIfNecessary) {
    if (fSwizzler || !createIfNecessary) {
        return fSwizzler.get();
    }

    std::ignore = this->initializeSwizzler(
            this->dstInfo(), this->options(), true, this->dstInfo().width());

    return fSwizzler.get();
}

void SkPngCodecBase::applyXformRow(SkSpan<uint8_t> dstRow, SkSpan<const uint8_t> srcRow) {
    SkASSERT_RELEASE(dstRow.size() >= fDstRowSize);
    SkASSERT_RELEASE(srcRow.size() >= fEncodedRowBytes);
    applyXformRow(dstRow.data(), srcRow.data());
}

void SkPngCodecBase::applyXformRow(void* dstRow, const uint8_t* srcRow) {
    switch (fXformMode) {
        case kSwizzleOnly_XformMode:
            fSwizzler->swizzle(dstRow, srcRow);
            break;
        case kColorOnly_XformMode:
            this->applyColorXform(dstRow, srcRow, fXformWidth);
            break;
        case kSwizzleColor_XformMode:
            SkASSERT_RELEASE(fSwizzler->swizzleWidth() == fXformWidth);
            fSwizzler->swizzle(fStorage.data(), srcRow);
            this->applyColorXform(dstRow, fStorage.data(), fXformWidth);
            break;
    }
}

bool SkPngCodecBase::createColorTable(const SkImageInfo& dstInfo) {
    if (fDstInfoOfPreviousColorTableCreation.has_value() &&
        *fDstInfoOfPreviousColorTableCreation == dstInfo) {
        return !!fColorTable;
    }
    fColorTable.reset();
    fDstInfoOfPreviousColorTableCreation = dstInfo;

    std::optional<SkSpan<const PaletteColorEntry>> maybePlteChunk = this->onTryGetPlteChunk();
    if (!maybePlteChunk.has_value()) {
        return false;
    }

    static_assert(3 == sizeof(PaletteColorEntry));
    static_assert(offsetof(PaletteColorEntry, red) == 0);
    static_assert(offsetof(PaletteColorEntry, green) == 1);
    static_assert(offsetof(PaletteColorEntry, blue) == 2);

    std::optional<SkSpan<const uint8_t>> maybeTrnsChunk = this->onTryGetTrnsChunk();
    const uint8_t* alphas = maybeTrnsChunk.has_value() ? maybeTrnsChunk->data() : nullptr;
    const size_t trnsLen = maybeTrnsChunk.has_value() ? maybeTrnsChunk->size() : 0;

    constexpr size_t kMaxCountOfPaletteEntries = 256;
    SkPMColor colorTable[kMaxCountOfPaletteEntries];
    SkiaRsPngColorTablePopulateResult popResult{};
    const bool ok = skia_rs_png_populate_color_table(
            reinterpret_cast<const uint8_t*>(maybePlteChunk->data()),
            maybePlteChunk->size(),
            alphas,
            trnsLen,
            this->getEncodedInfo().bitsPerComponent(),
            static_cast<int32_t>(this->getEncodedInfo().alpha()),
            static_cast<int32_t>(dstInfo.colorType()),
            static_cast<int32_t>(dstInfo.alphaType()),
            this->colorXform(),
            this->xformOnDecode(),
            kPmColorIsRgba,
            colorTable,
            &popResult);
    if (!ok) {
        return false;
    }

    if (popResult.should_xform_table) {
        this->applyColorXform(colorTable, colorTable, popResult.num_colors);
    }

    skia_rs_png_pad_color_table(colorTable, popResult.num_colors, popResult.max_colors);
    fColorTable.reset(new SkColorPalette(colorTable, popResult.max_colors));
    return true;
}

bool SkPngCodecBase::onGetGainmapCodec(SkGainmapInfo* info,
                                       std::unique_ptr<SkCodec>* gainmapCodec) {
    if (!fGainmapStream) {
        return false;
    }

    sk_sp<const SkData> data = fGainmapStream->getData();
    if (!data) {
        return false;
    }

    if (!SkPngCodecBase::IsPng(data->bytes(), data->size())) {
        return false;
    }

    SkCodec::Result result;
    std::unique_ptr<SkCodec> codec = this->onDecodeGainmap(fGainmapStream->duplicate(), &result);

    if (!codec || result != SkCodec::Result::kSuccess) {
        return false;
    }

    bool hasInfo = codec->onGetGainmapInfo(info);

    if (hasInfo && gainmapCodec) {
        if (info->fGainmapMathColorSpace) {
            const auto* colorProfile = codec->getEncodedInfo().colorProfile();
            if (colorProfile) {
                auto colorSpace = colorProfile->getExactColorSpace();
                if (colorSpace) {
                    info->fGainmapMathColorSpace = std::move(colorSpace);
                }
            }
        }

        *gainmapCodec = std::move(codec);
    }

    return hasInfo;
}

bool SkPngCodecBase::onGetGainmapInfo(SkGainmapInfo* info) {
    if (fGainmapInfo) {
        if (info) {
            *info = *fGainmapInfo;
        }
        return true;
    }

    return false;
}

// ============================================================================
// SkPngCompositeChunkReader
// ============================================================================

bool SkPngCompositeChunkReader::readChunk(const char tag[], const void* data, size_t length) {
    if (fChunkReader && !fChunkReader->readChunk(tag, data, length)) {
        return false;
    }

    const auto kind = static_cast<SkiaRsPngUnknownChunkKind>(
            skia_rs_png_classify_unknown_chunk(tag, data == nullptr, length));
    switch (kind) {
        case kUnknownChunk_GainmapInfo: {
            SkMemoryStream stream(data, length);
            sk_sp<const SkData> streamData = stream.getData();
            SkGainmapInfo info;
            if (SkGainmapInfo::Parse(streamData.get(), info)) {
                fGainmapInfo.emplace(std::move(info));
            }
            break;
        }
        case kUnknownChunk_GainmapData:
            fGainmapStream = SkMemoryStream::MakeCopy(data, length);
            break;
        case kUnknownChunk_ContentLightLevelInfo: {
            auto sk_data = SkData::MakeWithoutCopy(data, length);
            skhdr::ContentLightLevelInformation clli;
            if (clli.parsePngChunk(sk_data.get())) {
                fHdrMetadata.setContentLightLevelInformation(clli);
            }
            break;
        }
        case kUnknownChunk_MasteringDisplayColorVolume: {
            auto sk_data = SkData::MakeWithoutCopy(data, length);
            skhdr::MasteringDisplayColorVolume mdcv;
            if (mdcv.parse(sk_data.get())) {
                fHdrMetadata.setMasteringDisplayColorVolume(mdcv);
            }
            break;
        }
        case kUnknownChunk_Ignored:
            break;
    }
    return true;
}
