/*
 * Safe Rust bridge for:
 *   src/codec/SkAndroidCodec.cpp
 *   src/codec/SkAndroidCodecAdapter.cpp
 *   src/codec/SkSampledCodec.cpp
 * (Phase 2 Module P2.1: skia-rs-codec-core).
 */

#include "include/codec/SkAndroidCodec.h"
#include "include/codec/SkCodec.h"
#include "include/codec/SkEncodedImageFormat.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRect.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/core/SkTypes.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkAndroidCodecAdapter.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkSampledCodec.h"
#include "src/codec/SkSampler.h"
#include "src/core/SkColorPriv.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>

class SkPngChunkReader;

namespace {

#ifdef SK_PMCOLOR_IS_BGRA
constexpr bool kPmColorIsBgra = true;
#else
constexpr bool kPmColorIsBgra = false;
#endif

struct RustAndroidPixelsValidation {
    int32_t result;
    bool strip_full_subset;
};

struct RustNativeScalingPlan {
    bool use_native_scale;
    float scale;
    int32_t remaining_sample_size;
    int32_t native_sample_size;
};

struct RustScaledSubsetPlan {
    int32_t scaled_subset_x;
    int32_t scaled_subset_y;
    SkIRect incremental_subset;
    SkIRect scanline_subset;
};

struct RustSampledDecodeParams {
    int32_t subset_y;
    int32_t subset_width;
    int32_t subset_height;
    SkIRect scanline_subset;
    SkIRect incremental_subset;
    int32_t sample_x;
    int32_t sample_y;
    int32_t start_y;
};

using RustIsAnimatedWebpFn = bool (*)(const void* ctx);
using RustGetSampledDimsFn = SkISize (*)(const void* ctx, int32_t sample_size);

}  // namespace

extern "C" {

int32_t skia_rs_android_codec_select_kind(int32_t format);
int32_t skia_rs_android_codec_compute_output_color_type(int32_t requested_ct,
                                                        int32_t info_ct,
                                                        int32_t info_at,
                                                        uint8_t bits_per_component,
                                                        uint8_t color_depth,
                                                        bool pmcolor_is_bgra);
int32_t skia_rs_android_codec_compute_output_alpha_type(int32_t info_at, bool requested_unpremul);
bool skia_rs_android_codec_supports_output_color_space(int32_t output_ct);
int32_t skia_rs_android_codec_compute_sample_size(SkISize orig_dims,
                                                  SkISize* desired_size,
                                                  bool is_webp,
                                                  const void* ctx,
                                                  RustIsAnimatedWebpFn is_animated_webp,
                                                  RustGetSampledDimsFn get_sampled_dims);
bool skia_rs_android_codec_is_valid_sample_size(int32_t sample_size);
int32_t skia_rs_android_codec_get_sampled_dimension(int32_t src_dimension, int32_t sample_size);
float skia_rs_android_codec_get_scale_from_sample_size(int32_t sample_size);
bool skia_rs_android_codec_is_valid_subset(SkIRect subset, SkISize image_dims);
RustAndroidPixelsValidation skia_rs_android_codec_validate_pixels(bool has_pixels,
                                                                  size_t row_bytes,
                                                                  size_t min_row_bytes,
                                                                  SkISize codec_dims,
                                                                  const SkIRect* subset);

RustNativeScalingPlan skia_rs_sampled_codec_plan_native_scaling(int32_t format,
                                                                int32_t sample_size);
RustScaledSubsetPlan skia_rs_sampled_codec_compute_scaled_subset(SkIRect subset,
                                                                 int32_t sample_size,
                                                                 SkISize dst_dims,
                                                                 SkISize scaled_size);
RustSampledDecodeParams skia_rs_sampled_codec_compute_decode_params(SkISize native_size,
                                                                    SkISize dst_dims,
                                                                    int32_t native_sample_size,
                                                                    const SkIRect* subset);
bool skia_rs_sampled_codec_is_coord_necessary(int32_t src_coord,
                                              int32_t sample_factor,
                                              int32_t scaled_dim);
int32_t skia_rs_sampled_codec_get_dst_coord(int32_t src_coord, int32_t sample_factor);

}  // extern "C"

// ============================================================================
// SkAndroidCodec.cpp
// ============================================================================

SkAndroidCodec::SkAndroidCodec(SkCodec* codec)
        : fInfo(codec->getInfo()), fCodec(codec) {}

SkAndroidCodec::~SkAndroidCodec() {}

std::unique_ptr<SkAndroidCodec> SkAndroidCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                               SkPngChunkReader* chunkReader) {
    auto codec = SkCodec::MakeFromStream(std::move(stream), nullptr, chunkReader);
    return MakeFromCodec(std::move(codec));
}

std::unique_ptr<SkAndroidCodec> SkAndroidCodec::MakeFromCodec(std::unique_ptr<SkCodec> codec) {
    if (nullptr == codec) {
        return nullptr;
    }

    int32_t kind = skia_rs_android_codec_select_kind(
            static_cast<int32_t>(codec->getEncodedFormat()));
    if (kind == 1) {
        return std::make_unique<SkSampledCodec>(codec.release());
    }
    if (kind == 2) {
        return std::make_unique<SkAndroidCodecAdapter>(codec.release());
    }
    return nullptr;
}

std::unique_ptr<SkAndroidCodec> SkAndroidCodec::MakeFromData(sk_sp<const SkData> data,
                                                             SkPngChunkReader* chunkReader) {
    if (!data) {
        return nullptr;
    }
    return MakeFromStream(SkMemoryStream::Make(std::move(data)), chunkReader);
}

SkColorType SkAndroidCodec::computeOutputColorType(SkColorType requestedColorType) {
    return static_cast<SkColorType>(skia_rs_android_codec_compute_output_color_type(
            static_cast<int32_t>(requestedColorType),
            static_cast<int32_t>(this->getInfo().colorType()),
            static_cast<int32_t>(this->getInfo().alphaType()),
            fCodec->getEncodedInfo().bitsPerComponent(),
            fCodec->getEncodedInfo().getColorDepth(),
            kPmColorIsBgra));
}

SkAlphaType SkAndroidCodec::computeOutputAlphaType(bool requestedUnpremul) {
    return static_cast<SkAlphaType>(skia_rs_android_codec_compute_output_alpha_type(
            static_cast<int32_t>(this->getInfo().alphaType()), requestedUnpremul));
}

sk_sp<SkColorSpace> SkAndroidCodec::computeOutputColorSpace(SkColorType outputColorType,
                                                            sk_sp<SkColorSpace> prefColorSpace) {
    if (!skia_rs_android_codec_supports_output_color_space(static_cast<int32_t>(outputColorType))) {
        return nullptr;
    }
    if (prefColorSpace) {
        return prefColorSpace;
    }
    if (const auto* colorProfile = fCodec->getEncodedInfo().colorProfile()) {
        return colorProfile->getAndroidOutputColorSpace();
    }
    return SkColorSpace::MakeSRGB();
}

static bool android_codec_is_animated_webp_cb(const void* ctx) {
    const auto* self = static_cast<const SkAndroidCodec*>(ctx);
    return self->codec()->getFrameCount() > 1;
}

static SkISize android_codec_get_sampled_dims_cb(const void* ctx, int32_t sampleSize) {
    const auto* self = static_cast<const SkAndroidCodec*>(ctx);
    return self->getSampledDimensions(sampleSize);
}

int SkAndroidCodec::computeSampleSize(SkISize* desiredSize) const {
    SkASSERT(desiredSize);
    if (!desiredSize) {
        return 1;
    }
    const bool isWebp = (fCodec->getEncodedFormat() == SkEncodedImageFormat::kWEBP);
    return skia_rs_android_codec_compute_sample_size(fCodec->dimensions(),
                                                     desiredSize,
                                                     isWebp,
                                                     this,
                                                     &android_codec_is_animated_webp_cb,
                                                     &android_codec_get_sampled_dims_cb);
}

SkISize SkAndroidCodec::getSampledDimensions(int sampleSize) const {
    if (!skia_rs_android_codec_is_valid_sample_size(sampleSize)) {
        return {0, 0};
    }
    if (1 == sampleSize) {
        return fCodec->dimensions();
    }
    return this->onGetSampledDimensions(sampleSize);
}

bool SkAndroidCodec::getSupportedSubset(SkIRect* desiredSubset) const {
    if (!desiredSubset ||
        !skia_rs_android_codec_is_valid_subset(*desiredSubset, fCodec->dimensions())) {
        return false;
    }
    return this->onGetSupportedSubset(desiredSubset);
}

SkISize SkAndroidCodec::getSampledSubsetDimensions(int sampleSize, const SkIRect& subset) const {
    if (!skia_rs_android_codec_is_valid_sample_size(sampleSize)) {
        return {0, 0};
    }

    SkIRect copySubset = subset;
    if (!this->getSupportedSubset(&copySubset) || copySubset != subset) {
        return {0, 0};
    }

    if (fCodec->dimensions() == subset.size()) {
        return this->getSampledDimensions(sampleSize);
    }

    return {skia_rs_android_codec_get_sampled_dimension(subset.width(), sampleSize),
            skia_rs_android_codec_get_sampled_dimension(subset.height(), sampleSize)};
}

SkCodec::Result SkAndroidCodec::getAndroidPixels(const SkImageInfo& requestInfo,
                                                 void* requestPixels,
                                                 size_t requestRowBytes,
                                                 const AndroidOptions* options) {
    AndroidOptions defaultOptions;
    if (!options) {
        options = &defaultOptions;
    }

    RustAndroidPixelsValidation check = skia_rs_android_codec_validate_pixels(
            requestPixels != nullptr,
            requestRowBytes,
            requestInfo.minRowBytes(),
            fCodec->dimensions(),
            options->fSubset);
    if (check.result != static_cast<int32_t>(SkCodec::kSuccess)) {
        return static_cast<SkCodec::Result>(check.result);
    }
    if (check.strip_full_subset) {
        defaultOptions = *options;
        defaultOptions.fSubset = nullptr;
        options = &defaultOptions;
    }

    auto getPixelsFn = [&](const SkImageInfo& info,
                           void* pixels,
                           size_t rowBytes,
                           const SkCodec::Options& opts,
                           int requiredFrame) -> SkCodec::Result {
        SkAndroidCodec::AndroidOptions prevFrameOptions(
                reinterpret_cast<const SkAndroidCodec::AndroidOptions&>(opts));
        prevFrameOptions.fFrameIndex = requiredFrame;
        return this->getAndroidPixels(info, pixels, rowBytes, &prevFrameOptions);
    };
    if (auto result = fCodec->handleFrameIndex(
                requestInfo, requestPixels, requestRowBytes, *options, getPixelsFn);
        result != SkCodec::kSuccess) {
        return result;
    }

    return this->onGetAndroidPixels(requestInfo, requestPixels, requestRowBytes, *options);
}

SkCodec::Result SkAndroidCodec::getAndroidPixels(const SkImageInfo& info,
                                                 void* pixels,
                                                 size_t rowBytes) {
    return this->getAndroidPixels(info, pixels, rowBytes, nullptr);
}

bool SkAndroidCodec::getGainmapAndroidCodec(SkGainmapInfo* info,
                                            std::unique_ptr<SkAndroidCodec>* outCodec) {
    if (outCodec) {
        std::unique_ptr<SkCodec> gainmapCodec;
        if (!fCodec->onGetGainmapCodec(info, &gainmapCodec)) {
            return false;
        }
        *outCodec = MakeFromCodec(std::move(gainmapCodec));
        return true;
    }
    return fCodec->onGetGainmapCodec(info, nullptr);
}

bool SkAndroidCodec::getAndroidGainmap(SkGainmapInfo* info,
                                       std::unique_ptr<SkStream>* outGainmapImageStream) {
    return fCodec->onGetGainmapInfo(info, outGainmapImageStream);
}

// ============================================================================
// SkAndroidCodecAdapter.cpp
// ============================================================================

SkAndroidCodecAdapter::SkAndroidCodecAdapter(SkCodec* codec) : INHERITED(codec) {}

SkISize SkAndroidCodecAdapter::onGetSampledDimensions(int sampleSize) const {
    float scale = skia_rs_android_codec_get_scale_from_sample_size(sampleSize);
    return this->codec()->getScaledDimensions(scale);
}

bool SkAndroidCodecAdapter::onGetSupportedSubset(SkIRect* desiredSubset) const {
    return this->codec()->getValidSubset(desiredSubset);
}

SkCodec::Result SkAndroidCodecAdapter::onGetAndroidPixels(const SkImageInfo& info,
                                                          void* pixels,
                                                          size_t rowBytes,
                                                          const AndroidOptions& options) {
    return this->codec()->getPixels(info, pixels, rowBytes, &options);
}

// ============================================================================
// SkSampledCodec.cpp
// ============================================================================

SkSampledCodec::SkSampledCodec(SkCodec* codec) : INHERITED(codec) {}

SkISize SkSampledCodec::accountForNativeScaling(int* sampleSizePtr, int* nativeSampleSize) const {
    int sampleSize = *sampleSizePtr;
    SkASSERT(sampleSize > 1);

    RustNativeScalingPlan plan = skia_rs_sampled_codec_plan_native_scaling(
            static_cast<int32_t>(this->codec()->getEncodedFormat()), sampleSize);
    *sampleSizePtr = plan.remaining_sample_size;
    if (nativeSampleSize) {
        *nativeSampleSize = plan.native_sample_size;
    }
    if (plan.use_native_scale) {
        return this->codec()->getScaledDimensions(plan.scale);
    }
    return this->codec()->dimensions();
}

SkISize SkSampledCodec::onGetSampledDimensions(int sampleSize) const {
    const SkISize size = this->accountForNativeScaling(&sampleSize);
    return SkISize::Make(skia_rs_android_codec_get_sampled_dimension(size.width(), sampleSize),
                         skia_rs_android_codec_get_sampled_dimension(size.height(), sampleSize));
}

SkCodec::Result SkSampledCodec::onGetAndroidPixels(const SkImageInfo& info,
                                                   void* pixels,
                                                   size_t rowBytes,
                                                   const AndroidOptions& options) {
    const SkIRect* subset = options.fSubset;
    if (!subset || subset->size() == this->codec()->dimensions()) {
        if (this->codec()->dimensionsSupported(info.dimensions())) {
            return this->codec()->getPixels(info, pixels, rowBytes, &options);
        }
        return this->sampledDecode(info, pixels, rowBytes, options);
    }

    int sampleSize = options.fSampleSize;
    SkISize scaledSize = this->getSampledDimensions(sampleSize);
    if (!this->codec()->dimensionsSupported(scaledSize)) {
        return this->sampledDecode(info, pixels, rowBytes, options);
    }

    RustScaledSubsetPlan plan = skia_rs_sampled_codec_compute_scaled_subset(
            *subset, sampleSize, info.dimensions(), scaledSize);
    const int scaledSubsetHeight = info.height();
    const SkImageInfo scaledInfo = info.makeDimensions(scaledSize);

    AndroidOptions subsetOptions = options;
    {
        SkIRect incrementalSubset = plan.incremental_subset;
        subsetOptions.fSubset = &incrementalSubset;
        const SkCodec::Result startResult = this->codec()->startIncrementalDecode(
                scaledInfo, pixels, rowBytes, &subsetOptions);
        if (SkCodec::kSuccess == startResult) {
            int rowsDecoded = 0;
            const SkCodec::Result incResult = this->codec()->incrementalDecode(&rowsDecoded);
            if (incResult == SkCodec::kSuccess) {
                return SkCodec::kSuccess;
            }
            if (incResult == SkCodec::kIncompleteInput || incResult == SkCodec::kErrorInInput) {
                this->codec()->fillIncompleteImage(scaledInfo,
                                                   pixels,
                                                   rowBytes,
                                                   this->codec()->options().fZeroInitialized,
                                                   scaledSubsetHeight,
                                                   rowsDecoded);
            }
            return incResult;
        } else if (startResult != SkCodec::kUnimplemented) {
            return startResult;
        }
    }

    SkIRect scanlineSubset = plan.scanline_subset;
    subsetOptions.fSubset = &scanlineSubset;

    SkCodec::Result result = this->codec()->startScanlineDecode(scaledInfo, &subsetOptions);
    if (SkCodec::kSuccess != result) {
        return result;
    }

    SkASSERT(this->codec()->getScanlineOrder() == SkCodec::kTopDown_SkScanlineOrder);
    if (!this->codec()->skipScanlines(plan.scaled_subset_y)) {
        this->codec()->fillIncompleteImage(
                info, pixels, rowBytes, options.fZeroInitialized, scaledSubsetHeight, 0);
        return SkCodec::kIncompleteInput;
    }

    int decodedLines = this->codec()->getScanlines(pixels, scaledSubsetHeight, rowBytes);
    if (decodedLines != scaledSubsetHeight) {
        return SkCodec::kIncompleteInput;
    }
    return SkCodec::kSuccess;
}

SkCodec::Result SkSampledCodec::sampledDecode(const SkImageInfo& info,
                                              void* pixels,
                                              size_t rowBytes,
                                              const AndroidOptions& options) {
    SkASSERT(options.fSampleSize > 1);

    int sampleSize = options.fSampleSize;
    int nativeSampleSize;
    SkISize nativeSize = this->accountForNativeScaling(&sampleSize, &nativeSampleSize);

    RustSampledDecodeParams params = skia_rs_sampled_codec_compute_decode_params(
            nativeSize, info.dimensions(), nativeSampleSize, options.fSubset);

    const int sampleX = params.sample_x;
    const int sampleY = params.sample_y;
    const int startY = params.start_y;
    const int dstHeight = info.height();
    const SkImageInfo nativeInfo = info.makeDimensions(nativeSize);

    {
        AndroidOptions incrementalOptions = options;
        SkIRect incrementalSubset = params.incremental_subset;
        if (options.fSubset) {
            incrementalOptions.fSubset = &incrementalSubset;
        }
        const SkCodec::Result startResult = this->codec()->startIncrementalDecode(
                nativeInfo, pixels, rowBytes, &incrementalOptions);
        if (SkCodec::kSuccess == startResult) {
            SkSampler* sampler = this->codec()->getSampler(true);
            if (!sampler) {
                return SkCodec::kUnimplemented;
            }

            if (sampler->setSampleX(sampleX) != info.width()) {
                return SkCodec::kInvalidScale;
            }
            if (skia_rs_android_codec_get_sampled_dimension(params.subset_height, sampleY) !=
                info.height()) {
                return SkCodec::kInvalidScale;
            }

            sampler->setSampleY(sampleY);

            int rowsDecoded = 0;
            const SkCodec::Result incResult = this->codec()->incrementalDecode(&rowsDecoded);
            if (incResult == SkCodec::kSuccess) {
                return SkCodec::kSuccess;
            }
            if (incResult == SkCodec::kIncompleteInput || incResult == SkCodec::kErrorInInput) {
                SkASSERT(rowsDecoded <= info.height());
                this->codec()->fillIncompleteImage(info,
                                                   pixels,
                                                   rowBytes,
                                                   this->codec()->options().fZeroInitialized,
                                                   info.height(),
                                                   rowsDecoded);
            }
            return incResult;
        } else if (startResult == SkCodec::kIncompleteInput ||
                   startResult == SkCodec::kErrorInInput) {
            return SkCodec::kInvalidInput;
        } else if (startResult != SkCodec::kUnimplemented) {
            return startResult;
        }
    }

    AndroidOptions sampledOptions = options;
    SkIRect subset = params.scanline_subset;
    if (options.fSubset) {
        sampledOptions.fSubset = &subset;
    }
    SkCodec::Result result = this->codec()->startScanlineDecode(nativeInfo, &sampledOptions);
    if (SkCodec::kIncompleteInput == result || SkCodec::kErrorInInput == result) {
        return SkCodec::kInvalidInput;
    } else if (SkCodec::kSuccess != result) {
        return result;
    }

    SkSampler* sampler = this->codec()->getSampler(true);
    if (!sampler) {
        return SkCodec::kInternalError;
    }

    if (sampler->setSampleX(sampleX) != info.width()) {
        return SkCodec::kInvalidScale;
    }
    if (skia_rs_android_codec_get_sampled_dimension(params.subset_height, sampleY) !=
        info.height()) {
        return SkCodec::kInvalidScale;
    }

    switch (this->codec()->getScanlineOrder()) {
        case SkCodec::kTopDown_SkScanlineOrder: {
            if (!this->codec()->skipScanlines(startY)) {
                this->codec()->fillIncompleteImage(
                        info, pixels, rowBytes, options.fZeroInitialized, dstHeight, 0);
                return SkCodec::kIncompleteInput;
            }
            void* pixelPtr = pixels;
            for (int y = 0; y < dstHeight; y++) {
                if (1 != this->codec()->getScanlines(pixelPtr, 1, rowBytes)) {
                    this->codec()->fillIncompleteImage(
                            info, pixels, rowBytes, options.fZeroInitialized, dstHeight, y + 1);
                    return SkCodec::kIncompleteInput;
                }
                if (y < dstHeight - 1) {
                    if (!this->codec()->skipScanlines(sampleY - 1)) {
                        this->codec()->fillIncompleteImage(
                                info, pixels, rowBytes, options.fZeroInitialized, dstHeight, y + 1);
                        return SkCodec::kIncompleteInput;
                    }
                }
                pixelPtr = SkTAddOffset<void>(pixelPtr, rowBytes);
            }
            return SkCodec::kSuccess;
        }
        case SkCodec::kBottomUp_SkScanlineOrder: {
            SkASSERT(0 == params.subset_y && nativeSize.height() == params.subset_height);
            int y;
            for (y = 0; y < nativeSize.height(); y++) {
                int srcY = this->codec()->nextScanline();
                if (skia_rs_sampled_codec_is_coord_necessary(srcY, sampleY, dstHeight)) {
                    void* pixelPtr = SkTAddOffset<void>(
                            pixels,
                            rowBytes * skia_rs_sampled_codec_get_dst_coord(srcY, sampleY));
                    if (1 != this->codec()->getScanlines(pixelPtr, 1, rowBytes)) {
                        break;
                    }
                } else {
                    if (!this->codec()->skipScanlines(1)) {
                        break;
                    }
                }
            }

            if (nativeSize.height() == y) {
                return SkCodec::kSuccess;
            }

            const SkImageInfo fillInfo = info.makeWH(info.width(), 1);
            for (; y < nativeSize.height(); y++) {
                int srcY = this->codec()->outputScanline(y);
                if (!skia_rs_sampled_codec_is_coord_necessary(srcY, sampleY, dstHeight)) {
                    continue;
                }
                void* rowPtr = SkTAddOffset<void>(
                        pixels, rowBytes * skia_rs_sampled_codec_get_dst_coord(srcY, sampleY));
                SkSampler::Fill(fillInfo, rowPtr, rowBytes, options.fZeroInitialized);
            }
            return SkCodec::kIncompleteInput;
        }
        default:
            SkASSERT(false);
            return SkCodec::kUnimplemented;
    }
}
