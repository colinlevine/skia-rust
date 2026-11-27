/*
 * Safe Rust bridge for:
 *   src/encode/SkWebpEncoderImpl.cpp
 * (Phase 2 Module P2.6: skia-rs-codec-webp, GN target: webp_encode).
 *
 * Keeps thin third_party/libwebp (`WebPConfigPreset`, `WebPPicture*`, `WebPEncode`,
 * `WebPMux*`, `WebPAnimEncoder*`) calls while delegating pixmap & option validation,
 * lossy/lossless/method/use_argb planning, pixel import strategy selection,
 * safe-Rust scanline conversion, ICC mux planning, and animated frame validation
 * & timestamp accumulation to `#![forbid(unsafe_code)]` safe Rust in `skia-rs-codec-webp`.
 */

#include "include/encode/SkWebpEncoder.h"

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/encode/SkEncoder.h"
#include "include/encode/SkICC.h"
#include "include/private/SkTemplates.h"
#include "src/image/SkImage_Base.h"

#include <cstddef>
#include <cstdint>
#include <memory>

class GrDirectContext;
class SkImage;

extern "C" {
#include "webp/encode.h"     // NO_G3_REWRITE
#include "webp/mux.h"        // NO_G3_REWRITE
#include "webp/mux_types.h"  // NO_G3_REWRITE
}

namespace {

// ============================================================================
// Discriminant assertions shared across FFI with `skia-rs-codec-webp`.
// ============================================================================

static_assert(static_cast<int>(SkWebpEncoder::Compression::kLossy) == 0);
static_assert(static_cast<int>(SkWebpEncoder::Compression::kLossless) == 1);

static_assert(static_cast<int>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int>(kAlpha_8_SkColorType) == 1);
static_assert(static_cast<int>(kRGB_565_SkColorType) == 2);
static_assert(static_cast<int>(kARGB_4444_SkColorType) == 3);
static_assert(static_cast<int>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int>(kRGB_888x_SkColorType) == 5);
static_assert(static_cast<int>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int>(kRGBA_F16_SkColorType) == 16);
static_assert(static_cast<int>(kRGBA_F32_SkColorType) == 18);
static_assert(static_cast<int>(kBGR_888x_SkColorType) == 29);

static_assert(static_cast<int>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int>(kUnpremul_SkAlphaType) == 3);

static_assert(WEBP_MAX_DIMENSION == 16383);
static_assert(static_cast<int>(WEBP_MUX_OK) == 1);

// ============================================================================
// FFI structs & declarations matching `skia-rs-ffi/src/codec_webp.rs`.
// ============================================================================

enum SkiaRsWebpImportStrategy : int32_t {
    kImport_DirectRgbx = 0,
    kImport_DirectRgba = 1,
    kImport_DirectBgra = 2,
    kImport_RustConvertToRgba8888 = 3,
    kImport_ReadPixelsToRgba8888 = 4,
};

struct SkiaRsWebpEncodePlan {
    bool valid;
    int32_t width;
    int32_t height;
    int32_t lossless;
    bool set_method;
    int32_t method;
    int32_t use_argb;
    int32_t import_strategy;
    int32_t convert_kind;
};

struct SkiaRsWebpMuxPlan {
    bool use_mux;
    bool write_iccp_chunk;
    bool write_exif_chunk;
    bool write_xmp_chunk;
};

struct SkiaRsWebpAnimatedEncodeState {
    int32_t canvas_width;
    int32_t canvas_height;
    int32_t timestamp;
};

extern "C" {
bool skia_rs_webp_encoder_plan_picture(int32_t width,
                                       int32_t height,
                                       int32_t ct,
                                       int32_t at,
                                       bool has_addr,
                                       size_t row_bytes,
                                       int32_t compression,
                                       float quality,
                                       int32_t opt_method,
                                       bool use_default_method_macro,
                                       bool has_import_bgra_macro,
                                       SkiaRsWebpEncodePlan* out_plan);

bool skia_rs_webp_encoder_convert_image_to_rgba8888(int32_t convert_kind,
                                                    uint8_t* dst,
                                                    size_t dst_len,
                                                    size_t dst_row_bytes,
                                                    const uint8_t* src,
                                                    size_t src_len,
                                                    size_t src_row_bytes,
                                                    size_t width,
                                                    size_t height);

bool skia_rs_webp_encoder_plan_mux(bool has_icc,
                                   bool has_exif,
                                   bool has_xmp,
                                   SkiaRsWebpMuxPlan* out_plan);

bool skia_rs_webp_encoder_anim_init(int32_t canvas_width,
                                    int32_t canvas_height,
                                    SkiaRsWebpAnimatedEncodeState* out_state);

bool skia_rs_webp_encoder_anim_next_frame(SkiaRsWebpAnimatedEncodeState* state,
                                          int32_t frame_w,
                                          int32_t frame_h,
                                          int32_t duration,
                                          int32_t* out_timestamp);
}

int stream_writer(const uint8_t* data, size_t data_size, const WebPPicture* const picture) {
    SkWStream* const stream = static_cast<SkWStream*>(picture->custom_ptr);
    return stream->write(data, data_size) ? 1 : 0;
}

using WebPPictureImportProc = int (*)(WebPPicture* picture, const uint8_t* pixels, int stride);

bool preprocess_webp_picture(WebPPicture* pic,
                             WebPConfig* webp_config,
                             const SkPixmap& pixmap,
                             const SkWebpEncoder::Options& opts) {
#ifdef SK_WEBP_ENCODER_USE_DEFAULT_METHOD
    constexpr bool kUseDefaultMethodMacro = true;
#else
    constexpr bool kUseDefaultMethodMacro = false;
#endif
#ifdef WebPPictureImportBGRA
    constexpr bool kHasImportBgraMacro = true;
#else
    constexpr bool kHasImportBgraMacro = false;
#endif

    SkiaRsWebpEncodePlan plan{};
    if (!skia_rs_webp_encoder_plan_picture(pixmap.width(),
                                           pixmap.height(),
                                           static_cast<int32_t>(pixmap.colorType()),
                                           static_cast<int32_t>(pixmap.alphaType()),
                                           pixmap.addr() != nullptr,
                                           pixmap.rowBytes(),
                                           static_cast<int32_t>(opts.fCompression),
                                           opts.fQuality,
                                           opts.fMethod,
                                           kUseDefaultMethodMacro,
                                           kHasImportBgraMacro,
                                           &plan) ||
        !plan.valid) {
        return false;
    }

    pic->width = plan.width;
    pic->height = plan.height;
    webp_config->lossless = plan.lossless;
    if (plan.set_method) {
        webp_config->method = plan.method;
    }
    pic->use_argb = plan.use_argb;

    SkBitmap tmpBm;
    WebPPictureImportProc importProc = nullptr;
    const SkPixmap* src = &pixmap;

    switch (plan.import_strategy) {
        case kImport_DirectRgbx:
            importProc = WebPPictureImportRGBX;
            break;
        case kImport_DirectRgba:
            importProc = WebPPictureImportRGBA;
            break;
#ifdef WebPPictureImportBGRA
        case kImport_DirectBgra:
            importProc = WebPPictureImportBGRA;
            break;
#endif
        case kImport_RustConvertToRgba8888: {
            importProc = WebPPictureImportRGBA;
            auto info = pixmap.info()
                                .makeColorType(kRGBA_8888_SkColorType)
                                .makeAlphaType(kUnpremul_SkAlphaType);
            if (!tmpBm.tryAllocPixels(info)) {
                return false;
            }
            const size_t dstLen = tmpBm.computeByteSize();
            const size_t srcLen = pixmap.computeByteSize();
            if (dstLen == SIZE_MAX || srcLen == SIZE_MAX) {
                return false;
            }
            if (!skia_rs_webp_encoder_convert_image_to_rgba8888(
                        plan.convert_kind,
                        static_cast<uint8_t*>(tmpBm.getPixels()),
                        dstLen,
                        tmpBm.rowBytes(),
                        static_cast<const uint8_t*>(pixmap.addr()),
                        srcLen,
                        pixmap.rowBytes(),
                        static_cast<size_t>(plan.width),
                        static_cast<size_t>(plan.height))) {
                return false;
            }
            src = &tmpBm.pixmap();
            break;
        }
        case kImport_ReadPixelsToRgba8888:
        default: {
            importProc = WebPPictureImportRGBA;
            auto info = pixmap.info()
                                .makeColorType(kRGBA_8888_SkColorType)
                                .makeAlphaType(kUnpremul_SkAlphaType);
            if (!tmpBm.tryAllocPixels(info) ||
                !pixmap.readPixels(tmpBm.info(), tmpBm.getPixels(), tmpBm.rowBytes())) {
                return false;
            }
            src = &tmpBm.pixmap();
            break;
        }
    }

    if (!importProc(pic, static_cast<const uint8_t*>(src->addr()), static_cast<int>(src->rowBytes()))) {
        return false;
    }

    return true;
}

}  // namespace

namespace SkWebpEncoder {

bool Encode(SkWStream* stream, const SkPixmap& pixmap, const Options& opts) {
    if (!stream) {
        return false;
    }

    WebPConfig webp_config;
    if (!WebPConfigPreset(&webp_config, WEBP_PRESET_DEFAULT, opts.fQuality)) {
        return false;
    }

    WebPPicture pic;
    if (!WebPPictureInit(&pic)) {
        return false;
    }
    SkAutoTCallVProc<WebPPicture, WebPPictureFree> autoPic(&pic);

    if (!preprocess_webp_picture(&pic, &webp_config, pixmap, opts)) {
        return false;
    }

    sk_sp<SkData> icc = SkWriteICCProfile(pixmap.colorSpace(), opts.fHdrMetadata);
    SkiaRsWebpMuxPlan muxPlan{};
    skia_rs_webp_encoder_plan_mux(icc != nullptr, false, false, &muxPlan);

    SkDynamicMemoryWStream tmp;
    pic.custom_ptr = muxPlan.use_mux ? static_cast<void*>(&tmp) : static_cast<void*>(stream);
    pic.writer = stream_writer;

    if (!WebPEncode(&webp_config, &pic)) {
        return false;
    }

    if (muxPlan.use_mux) {
        sk_sp<SkData> encodedData = tmp.detachAsData();
        WebPData encoded = {encodedData->bytes(), encodedData->size()};

        SkAutoTCallVProc<WebPMux, WebPMuxDelete> mux(WebPMuxNew());
        if (WEBP_MUX_OK != WebPMuxSetImage(mux, &encoded, 0)) {
            return false;
        }

        if (muxPlan.write_iccp_chunk) {
            WebPData iccChunk = {icc->bytes(), icc->size()};
            if (WEBP_MUX_OK != WebPMuxSetChunk(mux, "ICCP", &iccChunk, 0)) {
                return false;
            }
        }

        WebPData assembled;
        SkAutoTCallVProc<WebPData, WebPDataClear> autoWebPData(&assembled);
        if (WEBP_MUX_OK != WebPMuxAssemble(mux, &assembled)) {
            return false;
        }

        if (!stream->write(assembled.bytes, assembled.size)) {
            return false;
        }
    }

    return true;
}

sk_sp<SkData> Encode(const SkPixmap& pixmap, const Options& opts) {
    SkDynamicMemoryWStream stream;
    return Encode(&stream, pixmap, opts) ? stream.detachAsData() : nullptr;
}

bool EncodeAnimated(SkWStream* stream, SkSpan<const SkEncoder::Frame> frames, const Options& opts) {
    if (!stream || frames.empty()) {
        return false;
    }

    SkiaRsWebpAnimatedEncodeState animState{};
    skia_rs_webp_encoder_anim_init(
            frames.front().pixmap.width(), frames.front().pixmap.height(), &animState);

    std::unique_ptr<WebPAnimEncoder, void (*)(WebPAnimEncoder*)> enc(
            WebPAnimEncoderNew(animState.canvas_width, animState.canvas_height, nullptr),
            WebPAnimEncoderDelete);
    if (!enc) {
        return false;
    }

    for (const auto& frame : frames) {
        const auto& pixmap = frame.pixmap;
        int32_t frameTimestamp = 0;
        if (!skia_rs_webp_encoder_anim_next_frame(
                    &animState, pixmap.width(), pixmap.height(), frame.duration, &frameTimestamp)) {
            return false;
        }

        WebPConfig webp_config;
        if (!WebPConfigPreset(&webp_config, WEBP_PRESET_DEFAULT, opts.fQuality)) {
            return false;
        }

        WebPPicture pic;
        if (!WebPPictureInit(&pic)) {
            return false;
        }
        SkAutoTCallVProc<WebPPicture, WebPPictureFree> autoPic(&pic);

        if (!preprocess_webp_picture(&pic, &webp_config, pixmap, opts)) {
            return false;
        }

        if (!WebPEncode(&webp_config, &pic)) {
            return false;
        }

        if (!WebPAnimEncoderAdd(enc.get(), &pic, frameTimestamp, &webp_config)) {
            return false;
        }
    }

    if (!WebPAnimEncoderAdd(enc.get(), nullptr, animState.timestamp, nullptr)) {
        return false;
    }

    WebPData assembled;
    SkAutoTCallVProc<WebPData, WebPDataClear> autoWebPData(&assembled);
    if (!WebPAnimEncoderAssemble(enc.get(), &assembled)) {
        return false;
    }

    enc.reset();

    return stream->write(assembled.bytes, assembled.size);
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

}  // namespace SkWebpEncoder
