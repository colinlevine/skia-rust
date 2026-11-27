/*
 * Safe Rust bridge for:
 *   src/codec/SkIcoCodec.cpp
 * (Phase 2 Module P2.3: skia-rs-codec-bmp).
 */

#include "src/codec/SkIcoCodec.h"

#include "include/codec/SkIcoDecoder.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkTArray.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkBmpCodec.h"
#include "src/codec/SkCodecPriv.h"
#include "src/core/SkStreamPriv.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>
#include <vector>

#if defined(SK_CODEC_DECODES_PNG_WITH_LIBPNG)
#include "include/codec/SkPngDecoder.h"
#elif defined(SK_CODEC_DECODES_PNG_WITH_RUST)
#include "include/codec/SkPngRustDecoder.h"
#else
#error "Must define either SK_CODEC_DECODES_PNG_WITH_LIBPNG or SK_CODEC_DECODES_PNG_WITH_RUST"
#endif

namespace {

static_assert(SkCodec::kSuccess == 0);
static_assert(SkCodec::kIncompleteInput == 1);
static_assert(SkCodec::kInvalidScale == 4);
static_assert(SkCodec::kInvalidInput == 6);
static_assert(SkCodec::kInternalError == 8);
static_assert(SkCodec::kUnimplemented == 9);
static_assert(sizeof(SkISize) == 8);

struct RustIcoEntry {
    uint32_t offset;
    uint32_t size;
};

}  // namespace

extern "C" {

bool skia_rs_ico_is_ico(const uint8_t* buf, size_t len);
int32_t skia_rs_ico_parse_directory(const uint8_t* data,
                                    size_t len,
                                    RustIcoEntry** out_entries,
                                    size_t* out_count,
                                    int32_t* out_default_result);
void skia_rs_ico_free_entries(RustIcoEntry* entries, size_t count);
size_t skia_rs_ico_select_largest_codec(const size_t* min_byte_sizes, size_t count);
SkISize skia_rs_ico_get_scaled_dimensions(SkISize orig,
                                          float desired_scale,
                                          const SkISize* embedded_dims,
                                          size_t count);
int32_t skia_rs_ico_choose_codec(const SkISize* embedded_dims,
                                 size_t count,
                                 SkISize requested,
                                 int32_t start_index);

}  // extern "C"

bool SkIcoCodec::IsIco(const void* buffer, size_t bytesRead) {
    return skia_rs_ico_is_ico(static_cast<const uint8_t*>(buffer), bytesRead);
}

std::unique_ptr<SkCodec> SkIcoCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                    Result* result) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }

    sk_sp<const SkData> data = nullptr;
    if (stream->getMemoryBase()) {
        data = SkData::MakeWithoutCopy(stream->getMemoryBase(), stream->getLength());
    } else {
        data = SkStreamPriv::CopyStreamToData(stream.get());
        stream.reset(nullptr);
    }

    RustIcoEntry* rawEntries = nullptr;
    size_t entryCount = 0;
    int32_t defaultResult = static_cast<int32_t>(kInvalidInput);
    const auto parseRes = static_cast<Result>(skia_rs_ico_parse_directory(
            data->bytes(), data->size(), &rawEntries, &entryCount, &defaultResult));
    if (parseRes != kSuccess) {
        *result = parseRes;
        return nullptr;
    }

    struct EntriesGuard {
        RustIcoEntry* ptr;
        size_t count;
        ~EntriesGuard() { skia_rs_ico_free_entries(ptr, count); }
    } guard{rawEntries, entryCount};

    *result = static_cast<Result>(defaultResult);

    auto codecs = std::make_unique<skia_private::TArray<std::unique_ptr<SkCodec>>>(
            static_cast<int>(entryCount));
    for (size_t i = 0; i < entryCount; ++i) {
        const uint32_t offset = rawEntries[i].offset;
        const uint32_t size = rawEntries[i].size;

        sk_sp<const SkData> embeddedData(SkData::MakeSubset(data.get(), offset, size));
        auto embeddedStream = SkMemoryStream::Make(embeddedData);

        std::unique_ptr<SkCodec> codec;
        Result ignoredResult;
#if defined(SK_CODEC_DECODES_PNG_WITH_LIBPNG)
        const bool isPng = SkPngDecoder::IsPng(embeddedData->bytes(), embeddedData->size());
#elif defined(SK_CODEC_DECODES_PNG_WITH_RUST)
        const bool isPng = SkPngRustDecoder::IsPng(embeddedData->bytes(), embeddedData->size());
#endif
        if (isPng) {
            codec = SkCodec::MakeFromStream(std::move(embeddedStream), &ignoredResult);
            if (!codec && !SkCodecs::HasDecoder("png")) {
                auto fallbackStream = SkMemoryStream::Make(embeddedData);
#if defined(SK_CODEC_DECODES_PNG_WITH_LIBPNG)
                codec = SkPngDecoder::Decode(std::move(fallbackStream), &ignoredResult);
#elif defined(SK_CODEC_DECODES_PNG_WITH_RUST)
                codec = SkPngRustDecoder::Decode(std::move(fallbackStream), &ignoredResult);
#endif
            }
        } else {
            codec = SkBmpCodec::MakeFromIco(std::move(embeddedStream), &ignoredResult);
        }

        if (nullptr != codec) {
            codecs->push_back(std::move(codec));
        }
    }

    if (codecs->empty()) {
        return nullptr;
    }

    std::vector<size_t> minByteSizes;
    minByteSizes.reserve(codecs->size());
    for (int i = 0; i < codecs->size(); ++i) {
        minByteSizes.push_back(codecs->at(i)->getInfo().computeMinByteSize());
    }
    const int maxIndex = static_cast<int>(
            skia_rs_ico_select_largest_codec(minByteSizes.data(), minByteSizes.size()));
    auto maxInfo = codecs->at(maxIndex)->getEncodedInfo().copy();

    *result = kSuccess;
    return std::unique_ptr<SkCodec>(
            new SkIcoCodec(std::move(maxInfo), std::move(stream), std::move(codecs)));
}

SkIcoCodec::SkIcoCodec(
        SkEncodedInfo&& info,
        std::unique_ptr<SkStream> stream,
        std::unique_ptr<skia_private::TArray<std::unique_ptr<SkCodec>>> codecs)
        : INHERITED(std::move(info), skcms_PixelFormat(), std::move(stream))
        , fEmbeddedCodecs(std::move(codecs))
        , fCurrCodec(nullptr) {}

SkISize SkIcoCodec::onGetScaledDimensions(float desiredScale) const {
    std::vector<SkISize> dims;
    dims.reserve(fEmbeddedCodecs->size());
    for (int i = 0; i < fEmbeddedCodecs->size(); ++i) {
        dims.push_back(fEmbeddedCodecs->at(i)->dimensions());
    }
    return skia_rs_ico_get_scaled_dimensions(
            this->dimensions(), desiredScale, dims.data(), dims.size());
}

int SkIcoCodec::chooseCodec(const SkISize& requestedSize, int startIndex) {
    SkASSERT(startIndex >= 0);
    std::vector<SkISize> dims;
    dims.reserve(fEmbeddedCodecs->size());
    for (int i = 0; i < fEmbeddedCodecs->size(); ++i) {
        dims.push_back(fEmbeddedCodecs->at(i)->dimensions());
    }
    return skia_rs_ico_choose_codec(dims.data(), dims.size(), requestedSize, startIndex);
}

bool SkIcoCodec::onDimensionsSupported(const SkISize& dim) {
    return this->chooseCodec(dim, 0) >= 0;
}

SkCodec::Result SkIcoCodec::onGetPixels(const SkImageInfo& dstInfo,
                                        void* dst,
                                        size_t dstRowBytes,
                                        const Options& opts,
                                        int* rowsDecoded) {
    if (opts.fSubset) {
        return kUnimplemented;
    }

    int index = 0;
    SkCodec::Result result = kInvalidScale;
    while (true) {
        index = this->chooseCodec(dstInfo.dimensions(), index);
        if (index < 0) {
            break;
        }

        SkCodec* embeddedCodec = fEmbeddedCodecs->at(index).get();
        result = embeddedCodec->getPixels(dstInfo, dst, dstRowBytes, &opts);
        switch (result) {
            case kSuccess:
            case kIncompleteInput:
                *rowsDecoded = dstInfo.height();
                return result;
            default:
                break;
        }
        index++;
    }
    return result;
}

SkCodec::Result SkIcoCodec::onStartScanlineDecode(const SkImageInfo& dstInfo,
                                                  const SkCodec::Options& options) {
    int index = 0;
    SkCodec::Result result = kInvalidScale;
    while (true) {
        index = this->chooseCodec(dstInfo.dimensions(), index);
        if (index < 0) {
            break;
        }

        SkCodec* embeddedCodec = fEmbeddedCodecs->at(index).get();
        result = embeddedCodec->startScanlineDecode(dstInfo, &options);
        if (kSuccess == result) {
            fCurrCodec = embeddedCodec;
            return result;
        }
        index++;
    }
    return result;
}

int SkIcoCodec::onGetScanlines(void* dst, int count, size_t rowBytes) {
    SkASSERT(fCurrCodec);
    return fCurrCodec->getScanlines(dst, count, rowBytes);
}

bool SkIcoCodec::onSkipScanlines(int count) {
    SkASSERT(fCurrCodec);
    return fCurrCodec->skipScanlines(count);
}

bool SkIcoCodec::onSupportsIncrementalDecode(const SkImageInfo& dstInfo) {
    for (int i = 0;; ++i) {
        i = this->chooseCodec(dstInfo.dimensions(), i);
        if (i < 0) {
            break;
        }
        SkASSERT(i < fEmbeddedCodecs->size());
        if ((*fEmbeddedCodecs)[i]->onSupportsIncrementalDecode(dstInfo)) {
            return true;
        }
    }
    return false;
}

SkCodec::Result SkIcoCodec::onStartIncrementalDecode(const SkImageInfo& dstInfo,
                                                     void* pixels,
                                                     size_t rowBytes,
                                                     const SkCodec::Options& options) {
    int index = 0;
    while (true) {
        index = this->chooseCodec(dstInfo.dimensions(), index);
        if (index < 0) {
            break;
        }

        SkCodec* embeddedCodec = fEmbeddedCodecs->at(index).get();
        if (embeddedCodec->startIncrementalDecode(dstInfo, pixels, rowBytes, &options) ==
            kSuccess) {
            fCurrCodec = embeddedCodec;
            return kSuccess;
        }
        index++;
    }
    return kInvalidScale;
}

SkCodec::Result SkIcoCodec::onIncrementalDecode(int* rowsDecoded) {
    SkASSERT(fCurrCodec);
    return fCurrCodec->incrementalDecode(rowsDecoded);
}

SkCodec::SkScanlineOrder SkIcoCodec::onGetScanlineOrder() const {
    if (fCurrCodec) {
        return fCurrCodec->getScanlineOrder();
    }
    return INHERITED::onGetScanlineOrder();
}

SkSampler* SkIcoCodec::getSampler(bool createIfNecessary) {
    if (fCurrCodec) {
        return fCurrCodec->getSampler(createIfNecessary);
    }
    return nullptr;
}

namespace SkIcoDecoder {
bool IsIco(const void* data, size_t len) {
    return SkIcoCodec::IsIco(data, len);
}

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    return SkIcoCodec::MakeFromStream(std::move(stream), outResult);
}

std::unique_ptr<SkCodec> Decode(sk_sp<const SkData> data,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    if (!data) {
        if (outResult) {
            *outResult = SkCodec::kInvalidInput;
        }
        return nullptr;
    }
    return Decode(SkMemoryStream::Make(std::move(data)), outResult, nullptr);
}
}  // namespace SkIcoDecoder
