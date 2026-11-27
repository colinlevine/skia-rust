/*
 * Safe Rust bridge for:
 *   src/codec/SkWebpCodec.cpp
 * (Phase 2 Module P2.6: skia-rs-codec-webp, GN target: webp_decode).
 *
 * Keeps thin third_party/libwebp (`WebPDemux*`, `WebPGetFeatures`, `WebPIDecode*`)
 * and Skia `SkRasterPipeline` boundary calls while delegating RIFF/WebP signature
 * sniffing, RIFF header streaming state machine, demux/feature classification,
 * canvas dimension validation, subset snapping, crop/scale/buffer/CSP planning,
 * animation repetition/frame metadata mapping, and post-decode blend planning
 * to `#![forbid(unsafe_code)]` safe Rust in `skia-rs-codec-webp`.
 */

#include "src/codec/SkWebpCodec.h"

#include "include/codec/SkCodec.h"
#include "include/codec/SkCodecAnimation.h"
#include "include/codec/SkWebpDecoder.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkRect.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/private/SkAlign.h"
#include "include/private/SkNoncopyable.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkParseEncodedOrigin.h"
#include "src/codec/SkSampler.h"
#include "src/core/SkRasterPipeline.h"
#include "src/core/SkRasterPipelineOpContexts.h"
#include "src/core/SkRasterPipelineOpList.h"
#include "src/core/SkStreamPriv.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <tuple>
#include <utility>

#include "webp/decode.h"     // NO_G3_REWRITE
#include "webp/demux.h"      // NO_G3_REWRITE
#include "webp/mux_types.h"  // NO_G3_REWRITE

namespace {

// ============================================================================
// Discriminant assertions shared across FFI with `skia-rs-codec-webp`.
// ============================================================================

static_assert(static_cast<int>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int>(kAlpha_8_SkColorType) == 1);
static_assert(static_cast<int>(kRGB_565_SkColorType) == 2);
static_assert(static_cast<int>(kARGB_4444_SkColorType) == 3);
static_assert(static_cast<int>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int>(kRGB_888x_SkColorType) == 5);
static_assert(static_cast<int>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int>(kRGBA_F16_SkColorType) == 16);
static_assert(static_cast<int>(kBGR_888x_SkColorType) == 29);

static_assert(static_cast<int>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int>(kUnpremul_SkAlphaType) == 3);

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
static_assert(SkCodec::kRepetitionCountInfinite == -1);

static_assert(static_cast<int>(SkEncodedInfo::kBGRX_Color) == 8);
static_assert(static_cast<int>(SkEncodedInfo::kBGRA_Color) == 9);
static_assert(static_cast<int>(SkEncodedInfo::kYUV_Color) == 10);
static_assert(static_cast<int>(SkEncodedInfo::kYUVA_Color) == 11);

static_assert(static_cast<int>(SkEncodedInfo::kOpaque_Alpha) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kUnpremul_Alpha) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kBinary_Alpha) == 2);

static_assert(static_cast<int>(kTopLeft_SkEncodedOrigin) == 1);
static_assert(static_cast<int>(kLeftBottom_SkEncodedOrigin) == 8);

static_assert(static_cast<int>(SkCodecAnimation::DisposalMethod::kKeep) == 1);
static_assert(static_cast<int>(SkCodecAnimation::DisposalMethod::kRestoreBGColor) == 2);
static_assert(static_cast<int>(SkCodecAnimation::DisposalMethod::kRestorePrevious) == 3);

static_assert(static_cast<int>(SkCodecAnimation::Blend::kSrcOver) == 0);
static_assert(static_cast<int>(SkCodecAnimation::Blend::kSrc) == 1);

static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kRGB) == 0);
static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kCMYK) == 1);
static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kGray) == 2);
static_assert(static_cast<int>(SkCodecs::ColorProfile::DataSpace::kOther) == 3);

static_assert(static_cast<int>(MODE_RGB) == 0);
static_assert(static_cast<int>(MODE_RGBA) == 1);
static_assert(static_cast<int>(MODE_BGR) == 2);
static_assert(static_cast<int>(MODE_BGRA) == 3);
static_assert(static_cast<int>(MODE_ARGB) == 4);
static_assert(static_cast<int>(MODE_RGBA_4444) == 5);
static_assert(static_cast<int>(MODE_RGB_565) == 6);
static_assert(static_cast<int>(MODE_rgbA) == 7);
static_assert(static_cast<int>(MODE_bgrA) == 8);
static_assert(static_cast<int>(MODE_Argb) == 9);
static_assert(static_cast<int>(MODE_rgbA_4444) == 10);
static_assert(static_cast<int>(MODE_YUV) == 11);
static_assert(static_cast<int>(MODE_YUVA) == 12);
static_assert(static_cast<int>(MODE_LAST) == 13);

static_assert(static_cast<int>(WEBP_DEMUX_PARSE_ERROR) == -1);
static_assert(static_cast<int>(WEBP_DEMUX_PARSING_HEADER) == 0);
static_assert(static_cast<int>(WEBP_DEMUX_PARSED_HEADER) == 1);
static_assert(static_cast<int>(WEBP_DEMUX_DONE) == 2);

static_assert(static_cast<int>(VP8_STATUS_OK) == 0);
static_assert(static_cast<int>(VP8_STATUS_OUT_OF_MEMORY) == 1);
static_assert(static_cast<int>(VP8_STATUS_INVALID_PARAM) == 2);
static_assert(static_cast<int>(VP8_STATUS_BITSTREAM_ERROR) == 3);
static_assert(static_cast<int>(VP8_STATUS_UNSUPPORTED_FEATURE) == 4);
static_assert(static_cast<int>(VP8_STATUS_SUSPENDED) == 5);
static_assert(static_cast<int>(VP8_STATUS_USER_ABORT) == 6);
static_assert(static_cast<int>(VP8_STATUS_NOT_ENOUGH_DATA) == 7);

static_assert(static_cast<uint32_t>(ANIMATION_FLAG) == 0x02);
static_assert(static_cast<uint32_t>(XMP_FLAG) == 0x04);
static_assert(static_cast<uint32_t>(EXIF_FLAG) == 0x08);
static_assert(static_cast<uint32_t>(ALPHA_FLAG) == 0x10);
static_assert(static_cast<uint32_t>(ICCP_FLAG) == 0x20);

static_assert(static_cast<int>(WEBP_MUX_DISPOSE_NONE) == 0);
static_assert(static_cast<int>(WEBP_MUX_DISPOSE_BACKGROUND) == 1);

static_assert(static_cast<int>(WEBP_MUX_BLEND) == 0);
static_assert(static_cast<int>(WEBP_MUX_NO_BLEND) == 1);

// ============================================================================
// FFI structs & declarations matching `skia-rs-ffi/src/codec_webp.rs`.
// ============================================================================

struct SkiaRsWebpRiffReader;

enum SkiaRsWebpRiffStepKind : int32_t {
    kRiffStep_NeedRead = 0,
    kRiffStep_Done = 1,
    kRiffStep_Invalid = 2,
};

struct SkiaRsWebpRiffReadStep {
    int32_t kind;
    size_t max_bytes;
    bool only_header_parsed;
};

struct SkiaRsWebpFrameMeta {
    int32_t reported_alpha;
    int32_t disposal_method;
    int32_t blend;
};

struct SkiaRsWebpBlendPlan {
    bool active;
    bool use_tmp_xform_row;
    int32_t blend_src_ct;
    bool premul_dst;
    bool premul_src;
    bool unpremul_dst;
};

struct SkiaRsWebpGetPixelsPlan {
    bool should_fill_bg;
    bool early_return_success;
    int32_t dst_x;
    int32_t dst_y;
    bool use_cropping;
    int32_t crop_left;
    int32_t crop_top;
    int32_t crop_width;
    int32_t crop_height;
    bool use_scaling;
    int32_t scaled_width;
    int32_t scaled_height;
    bool blend_with_prev_frame;
    int32_t webp_ct;
    int32_t webp_at;
    bool needs_temp_decode_buffer;
    int32_t csp_mode;
    size_t dst_byte_offset;
    SkiaRsWebpBlendPlan blend_plan;
};

struct SkiaRsWebpIdecUpdateOutcome {
    int32_t result;
    int32_t rows_decoded;
    bool write_rows_decoded_ptr;
    int32_t rows_decoded_out;
};

extern "C" {
bool skia_rs_webp_is_webp(const uint8_t* buf, size_t len);
SkiaRsWebpRiffReader* skia_rs_webp_riff_reader_new();
void skia_rs_webp_riff_reader_free(SkiaRsWebpRiffReader* reader);
bool skia_rs_webp_riff_reader_next_step(SkiaRsWebpRiffReader* reader,
                                        bool stream_at_end,
                                        SkiaRsWebpRiffReadStep* out_step);
bool skia_rs_webp_riff_reader_feed(SkiaRsWebpRiffReader* reader,
                                   const uint8_t* bytes,
                                   size_t len,
                                   bool stream_at_end,
                                   SkiaRsWebpRiffReadStep* out_step);

int32_t skia_rs_webp_map_demux_state(int32_t state);
bool skia_rs_webp_validate_canvas_dimensions(int32_t width, int32_t height);
bool skia_rs_webp_should_keep_color_profile(int32_t data_space);
int32_t skia_rs_webp_map_features_status(int32_t status);
bool skia_rs_webp_classify_encoded_info(int32_t canvas_width,
                                        int32_t canvas_height,
                                        int32_t frame_width,
                                        int32_t frame_height,
                                        bool frame_has_alpha,
                                        int32_t bitstream_format,
                                        int32_t* out_color,
                                        int32_t* out_alpha);
bool skia_rs_webp_get_valid_subset(int32_t bounds_w,
                                   int32_t bounds_h,
                                   int32_t* left,
                                   int32_t* top,
                                   int32_t* right,
                                   int32_t* bottom);
bool skia_rs_webp_is_animated(uint32_t format_flags);
int32_t skia_rs_webp_repetition_count(uint32_t format_flags, int32_t loop_count);
bool skia_rs_webp_map_frame_metadata(bool has_alpha,
                                     int32_t dispose_method,
                                     int32_t blend_method,
                                     SkiaRsWebpFrameMeta* out_meta);
bool skia_rs_webp_plan_get_pixels(int32_t canvas_w,
                                  int32_t canvas_h,
                                  int32_t frame_x,
                                  int32_t frame_y,
                                  int32_t frame_w,
                                  int32_t frame_h,
                                  bool frame_has_alpha,
                                  int32_t frame_blend_method,
                                  bool independent,
                                  bool has_subset,
                                  int32_t subset_left,
                                  int32_t subset_top,
                                  int32_t subset_right,
                                  int32_t subset_bottom,
                                  int32_t dst_w,
                                  int32_t dst_h,
                                  int32_t dst_ct,
                                  int32_t dst_at,
                                  size_t dst_row_bytes,
                                  bool has_color_xform,
                                  SkiaRsWebpGetPixelsPlan* out_plan);
bool skia_rs_webp_interpret_idec_update(int32_t status,
                                        int32_t scaled_height,
                                        int32_t dst_y,
                                        bool got_rgb,
                                        int32_t idec_rows_decoded,
                                        SkiaRsWebpIdecUpdateOutcome* out_outcome);
}

// Drives the safe-Rust RIFF header scanner against an SkStream.
std::pair<bool, sk_sp<SkData>> get_header_from_stream(SkStream* stream) {
    constexpr size_t kBufferSize = 4096;
    uint8_t buffer[kBufferSize];
    SkDynamicMemoryWStream tempStream;

    SkAutoTCallVProc<SkiaRsWebpRiffReader, skia_rs_webp_riff_reader_free> reader(
            skia_rs_webp_riff_reader_new());
    if (!reader) {
        return {false, nullptr};
    }

    SkiaRsWebpRiffReadStep step{};
    skia_rs_webp_riff_reader_next_step(reader.get(), stream->isAtEnd(), &step);

    while (step.kind == kRiffStep_NeedRead) {
        const size_t toRead = step.max_bytes <= kBufferSize ? step.max_bytes : kBufferSize;
        const size_t bytesRead = stream->read(buffer, toRead);
        if (bytesRead > 0) {
            tempStream.write(buffer, bytesRead);
        }
        skia_rs_webp_riff_reader_feed(reader.get(), buffer, bytesRead, stream->isAtEnd(), &step);
    }

    if (step.kind == kRiffStep_Done) {
        return {step.only_header_parsed, tempStream.detachAsData()};
    }
    return {false, nullptr};
}

// Thin SkRasterPipeline wrapper configured by `SkiaRsWebpBlendPlan` from Rust.
class RPBlender final : SkNoncopyable {
public:
    RPBlender(SkColorType dstCT, const SkiaRsWebpBlendPlan& plan) {
        fRP.appendLoadDst(dstCT, &fDstCtx);
        if (plan.premul_dst) {
            fRP.append(SkRasterPipelineOp::premul_dst);
        }

        fRP.appendLoad(static_cast<SkColorType>(plan.blend_src_ct), &fSrcCtx);
        if (plan.premul_src) {
            fRP.append(SkRasterPipelineOp::premul);
        }

        fRP.append(SkRasterPipelineOp::srcover);

        if (plan.unpremul_dst) {
            fRP.append(SkRasterPipelineOp::unpremul);
        }
        fRP.appendStore(dstCT, &fDstCtx);
    }

    void blendLine(void* dst, const void* src, int width) {
        fDstCtx = {dst, 0};
        fSrcCtx = {const_cast<void*>(src), 0};
        fRP.run(0, 0, width, 1);
    }

private:
    SkRasterPipelineContexts::MemoryCtx fDstCtx, fSrcCtx;
    SkRasterPipeline_<256> fRP;
};

}  // namespace

bool SkWebpCodec::IsWebp(const void* buf, size_t bytesRead) {
    return skia_rs_webp_is_webp(static_cast<const uint8_t*>(buf), bytesRead);
}

bool SkWebpCodec::ensureAllData() {
    if (fOnlyHeaderParsed) {
        SkDynamicMemoryWStream newData;
        newData.write(fData->data(), fData->size());
        SkStreamPriv::Copy(&newData, this->stream());
        fData = newData.detachAsData();
        fOnlyHeaderParsed = false;

        WebPData webpData = {fData->bytes(), fData->size()};
        WebPDemuxState state;
        fDemux.reset(WebPDemuxPartial(&webpData, &state));
        if (state == WEBP_DEMUX_PARSE_ERROR) {
            return false;
        }
    }
    return true;
}

std::unique_ptr<SkCodec> SkWebpCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                     Result* result) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }

    sk_sp<SkData> data = nullptr;
    bool onlyHeaderParsed = false;
    if (stream->getMemoryBase()) {
        data = SkData::MakeWithoutCopy(stream->getMemoryBase(), stream->getLength());
    } else {
        std::tie(onlyHeaderParsed, data) = get_header_from_stream(stream.get());
        if (!data) {
            *result = kIncompleteInput;
            return nullptr;
        }
        if (!onlyHeaderParsed) {
            stream.reset(nullptr);
        }
    }

    WebPData webpData = {data->bytes(), data->size()};
    WebPDemuxState state;
    SkAutoTCallVProc<WebPDemuxer, WebPDemuxDelete> demux(WebPDemuxPartial(&webpData, &state));
    const auto demuxResult =
            static_cast<SkCodec::Result>(skia_rs_webp_map_demux_state(static_cast<int32_t>(state)));
    if (demuxResult != kSuccess) {
        *result = demuxResult;
        return nullptr;
    }
    SkASSERT(demux);

    const int width = WebPDemuxGetI(demux, WEBP_FF_CANVAS_WIDTH);
    const int height = WebPDemuxGetI(demux, WEBP_FF_CANVAS_HEIGHT);
    if (!skia_rs_webp_validate_canvas_dimensions(width, height)) {
        *result = kInvalidInput;
        return nullptr;
    }

    std::unique_ptr<SkCodecs::ColorProfile> profile = nullptr;
    {
        WebPChunkIterator chunkIterator;
        SkAutoTCallVProc<WebPChunkIterator, WebPDemuxReleaseChunkIterator> autoCI(&chunkIterator);
        if (WebPDemuxGetChunk(demux, "ICCP", 1, &chunkIterator)) {
            auto chunk = SkData::MakeWithCopy(chunkIterator.chunk.bytes, chunkIterator.chunk.size);
            profile = SkCodecs::ColorProfile::MakeICCProfile(std::move(chunk));
        }
        if (profile &&
            !skia_rs_webp_should_keep_color_profile(static_cast<int32_t>(profile->dataSpace()))) {
            profile = nullptr;
        }
    }

    SkEncodedOrigin origin = kDefault_SkEncodedOrigin;
    {
        WebPChunkIterator chunkIterator;
        SkAutoTCallVProc<WebPChunkIterator, WebPDemuxReleaseChunkIterator> autoCI(&chunkIterator);
        if (WebPDemuxGetChunk(demux, "EXIF", 1, &chunkIterator)) {
            SkParseEncodedOrigin(chunkIterator.chunk.bytes, chunkIterator.chunk.size, &origin);
        }
    }

    WebPIterator frame;
    SkAutoTCallVProc<WebPIterator, WebPDemuxReleaseIterator> autoFrame(&frame);
    if (!WebPDemuxGetFrame(demux, 1, &frame)) {
        *result = kIncompleteInput;
        return nullptr;
    }

    WebPBitstreamFeatures features;
    const VP8StatusCode featStatus =
            WebPGetFeatures(frame.fragment.bytes, frame.fragment.size, &features);
    const auto featResult = static_cast<SkCodec::Result>(
            skia_rs_webp_map_features_status(static_cast<int32_t>(featStatus)));
    if (featResult != kSuccess) {
        *result = featResult;
        return nullptr;
    }

    int32_t encodedColor = 0;
    int32_t encodedAlpha = 0;
    if (!skia_rs_webp_classify_encoded_info(width,
                                            height,
                                            frame.width,
                                            frame.height,
                                            frame.has_alpha != 0,
                                            features.format,
                                            &encodedColor,
                                            &encodedAlpha)) {
        *result = kInvalidInput;
        return nullptr;
    }

    *result = kSuccess;
    SkEncodedInfo info = SkEncodedInfo::Make(width,
                                             height,
                                             static_cast<SkEncodedInfo::Color>(encodedColor),
                                             static_cast<SkEncodedInfo::Alpha>(encodedAlpha),
                                             8,
                                             std::move(profile));
    return std::unique_ptr<SkCodec>(new SkWebpCodec(std::move(info),
                                                    std::move(stream),
                                                    demux.release(),
                                                    std::move(data),
                                                    origin,
                                                    onlyHeaderParsed));
}

SkWebpCodec::Frame* SkWebpCodec::FrameHolder::appendNewFrame(bool hasAlpha) {
    const int i = this->size();
    fFrames.emplace_back(
            i, hasAlpha ? SkEncodedInfo::kUnpremul_Alpha : SkEncodedInfo::kOpaque_Alpha);
    return &fFrames[i];
}

bool SkWebpCodec::onGetValidSubset(SkIRect* desiredSubset) const {
    if (!desiredSubset) {
        return false;
    }
    const SkISize dims = this->dimensions();
    return skia_rs_webp_get_valid_subset(dims.width(),
                                         dims.height(),
                                         &desiredSubset->fLeft,
                                         &desiredSubset->fTop,
                                         &desiredSubset->fRight,
                                         &desiredSubset->fBottom);
}

int SkWebpCodec::onGetRepetitionCount() {
    const uint32_t flags = WebPDemuxGetI(fDemux.get(), WEBP_FF_FORMAT_FLAGS);
    if (!skia_rs_webp_is_animated(flags)) {
        return 0;
    }
    const int loopCount = static_cast<int>(WebPDemuxGetI(fDemux.get(), WEBP_FF_LOOP_COUNT));
    return skia_rs_webp_repetition_count(flags, loopCount);
}

SkCodec::IsAnimated SkWebpCodec::onIsAnimated() {
    const uint32_t flags = WebPDemuxGetI(fDemux.get(), WEBP_FF_FORMAT_FLAGS);
    return skia_rs_webp_is_animated(flags) ? IsAnimated::kYes : IsAnimated::kNo;
}

bool SkWebpCodec::onRewind() {
    return true;
}

int SkWebpCodec::onGetFrameCount() {
    const uint32_t flags = WebPDemuxGetI(fDemux.get(), WEBP_FF_FORMAT_FLAGS);
    if (!skia_rs_webp_is_animated(flags)) {
        return 1;
    }

    const uint32_t oldFrameCount = fFrameHolder.size();
    if (fFailed) {
        return oldFrameCount;
    }

    if (!ensureAllData()) {
        return 0;
    }

    const uint32_t frameCount = WebPDemuxGetI(fDemux, WEBP_FF_FRAME_COUNT);
    if (oldFrameCount == frameCount) {
        return frameCount;
    }

    fFrameHolder.reserve(frameCount);

    for (uint32_t i = oldFrameCount; i < frameCount; i++) {
        WebPIterator iter;
        SkAutoTCallVProc<WebPIterator, WebPDemuxReleaseIterator> autoIter(&iter);

        if (!WebPDemuxGetFrame(fDemux.get(), i + 1, &iter)) {
            fFailed = true;
            break;
        }

        SkASSERT(iter.complete);

        SkiaRsWebpFrameMeta meta{};
        skia_rs_webp_map_frame_metadata(iter.has_alpha != 0,
                                        static_cast<int32_t>(iter.dispose_method),
                                        static_cast<int32_t>(iter.blend_method),
                                        &meta);

        Frame* frame = fFrameHolder.appendNewFrame(
                static_cast<SkEncodedInfo::Alpha>(meta.reported_alpha) !=
                SkEncodedInfo::kOpaque_Alpha);
        frame->setXYWH(iter.x_offset, iter.y_offset, iter.width, iter.height);
        frame->setDisposalMethod(static_cast<SkCodecAnimation::DisposalMethod>(meta.disposal_method));
        frame->setDuration(iter.duration);
        frame->setBlend(static_cast<SkCodecAnimation::Blend>(meta.blend));
        fFrameHolder.setAlphaAndRequiredFrame(frame);
    }

    return fFrameHolder.size();
}

const SkFrame* SkWebpCodec::FrameHolder::onGetFrame(int i) const {
    return static_cast<const SkFrame*>(this->frame(i));
}

const SkWebpCodec::Frame* SkWebpCodec::FrameHolder::frame(int i) const {
    return (i >= 0 && i < this->size()) ? &fFrames[i] : nullptr;
}

bool SkWebpCodec::onGetFrameInfo(int i, FrameInfo* frameInfo) const {
    if (i >= fFrameHolder.size()) {
        return false;
    }

    const Frame* frame = fFrameHolder.frame(i);
    if (!frame) {
        return false;
    }

    if (frameInfo) {
        frame->fillIn(frameInfo, true);
    }

    return true;
}

SkCodec::Result SkWebpCodec::onGetPixels(const SkImageInfo& dstInfo,
                                         void* dst,
                                         size_t rowBytes,
                                         const Options& options,
                                         int* rowsDecodedPtr) {
    const int index = options.fFrameIndex;
    SkASSERT(0 == index || index < fFrameHolder.size());
    SkASSERT(0 == index || !options.fSubset);

    WebPDecoderConfig config;
    if (0 == WebPInitDecoderConfig(&config)) {
        return kInvalidInput;
    }

    SkAutoTCallVProc<WebPDecBuffer, WebPFreeDecBuffer> autoFree(&(config.output));

    WebPIterator frame;
    SkAutoTCallVProc<WebPIterator, WebPDemuxReleaseIterator> autoFrame(&frame);
    if (!ensureAllData()) {
        return kInvalidInput;
    }
    SkAssertResult(WebPDemuxGetFrame(fDemux, index + 1, &frame));

    const bool independent =
            index == 0 ? true : (fFrameHolder.frame(index)->getRequiredFrame() == kNoFrame);

    SkASSERT(this->bounds().contains(
            SkIRect::MakeXYWH(frame.x_offset, frame.y_offset, frame.width, frame.height)));
    if (options.fSubset) {
        SkIRect subsetCheck = *options.fSubset;
        SkASSERT(this->bounds().contains(subsetCheck));
        SkASSERT(SkIsAlign2(subsetCheck.fLeft) && SkIsAlign2(subsetCheck.fTop));
        SkASSERT(this->getValidSubset(&subsetCheck) && subsetCheck == *options.fSubset);
    }

    const SkISize dims = this->dimensions();
    SkiaRsWebpGetPixelsPlan plan{};
    if (!skia_rs_webp_plan_get_pixels(dims.width(),
                                      dims.height(),
                                      frame.x_offset,
                                      frame.y_offset,
                                      frame.width,
                                      frame.height,
                                      frame.has_alpha != 0,
                                      static_cast<int32_t>(frame.blend_method),
                                      independent,
                                      options.fSubset != nullptr,
                                      options.fSubset ? options.fSubset->fLeft : 0,
                                      options.fSubset ? options.fSubset->fTop : 0,
                                      options.fSubset ? options.fSubset->fRight : 0,
                                      options.fSubset ? options.fSubset->fBottom : 0,
                                      dstInfo.width(),
                                      dstInfo.height(),
                                      static_cast<int32_t>(dstInfo.colorType()),
                                      static_cast<int32_t>(dstInfo.alphaType()),
                                      rowBytes,
                                      this->colorXform(),
                                      &plan)) {
        return kInvalidInput;
    }

    if (plan.should_fill_bg) {
        SkSampler::Fill(dstInfo, dst, rowBytes, options.fZeroInitialized);
    }
    if (plan.early_return_success) {
        return kSuccess;
    }

    if (plan.use_cropping) {
        config.options.use_cropping = 1;
        config.options.crop_left = plan.crop_left;
        config.options.crop_top = plan.crop_top;
        config.options.crop_width = plan.crop_width;
        config.options.crop_height = plan.crop_height;
    }
    if (plan.use_scaling) {
        config.options.use_scaling = 1;
        config.options.scaled_width = plan.scaled_width;
        config.options.scaled_height = plan.scaled_height;
    }

    SkImageInfo webpInfo = dstInfo.makeColorType(static_cast<SkColorType>(plan.webp_ct))
                                   .makeAlphaType(static_cast<SkAlphaType>(plan.webp_at));

    SkBitmap webpDst;
    if (plan.needs_temp_decode_buffer) {
        webpDst.allocPixels(webpInfo);
    } else {
        webpDst.installPixels(webpInfo, dst, rowBytes);
    }

    config.output.colorspace = static_cast<WEBP_CSP_MODE>(plan.csp_mode);
    config.output.is_external_memory = 1;
    config.output.u.RGBA.rgba = reinterpret_cast<uint8_t*>(webpDst.getAddr(plan.dst_x, plan.dst_y));
    config.output.u.RGBA.stride = static_cast<int>(webpDst.rowBytes());
    config.output.u.RGBA.size = webpDst.computeByteSize();

    SkAutoTCallVProc<WebPIDecoder, WebPIDelete> idec(WebPIDecode(nullptr, 0, &config));
    if (!idec) {
        return kInvalidInput;
    }

    const VP8StatusCode status = WebPIUpdate(idec, frame.fragment.bytes, frame.fragment.size);
    int idecRows = 0;
    bool gotRgb = false;
    if (status == VP8_STATUS_SUSPENDED) {
        gotRgb = WebPIDecGetRGB(idec, &idecRows, nullptr, nullptr, nullptr) != nullptr;
    }

    SkiaRsWebpIdecUpdateOutcome outcome{};
    skia_rs_webp_interpret_idec_update(
            static_cast<int32_t>(status), plan.scaled_height, plan.dst_y, gotRgb, idecRows, &outcome);
    const auto result = static_cast<SkCodec::Result>(outcome.result);
    if (result != kSuccess && result != kIncompleteInput) {
        return result;
    }
    if (outcome.write_rows_decoded_ptr) {
        *rowsDecodedPtr = outcome.rows_decoded_out;
    }
    const int rowsDecoded = outcome.rows_decoded;

    dst = SkTAddOffset<void>(dst, plan.dst_byte_offset);
    const size_t srcRowBytes = config.output.u.RGBA.stride;
    const auto dstCT = dstInfo.colorType();

    if (this->colorXform()) {
        uint32_t* xformSrc = reinterpret_cast<uint32_t*>(config.output.u.RGBA.rgba);
        SkBitmap tmp;
        void* xformDst;

        if (plan.blend_plan.use_tmp_xform_row) {
            tmp.allocPixels(dstInfo.makeWH(plan.scaled_width, 1));
            xformDst = tmp.getPixels();
        } else {
            xformDst = dst;
        }

        std::optional<RPBlender> blender;
        if (plan.blend_plan.active) {
            blender.emplace(dstCT, plan.blend_plan);
        }

        for (int y = 0; y < rowsDecoded; y++) {
            this->applyColorXform(xformDst, xformSrc, plan.scaled_width);
            if (plan.blend_plan.active) {
                blender->blendLine(dst, xformDst, plan.scaled_width);
                dst = SkTAddOffset<void>(dst, rowBytes);
            } else {
                xformDst = SkTAddOffset<void>(xformDst, rowBytes);
            }
            xformSrc = SkTAddOffset<uint32_t>(xformSrc, srcRowBytes);
        }
    } else if (plan.blend_plan.active) {
        RPBlender blender(dstCT, plan.blend_plan);
        const uint8_t* src = config.output.u.RGBA.rgba;

        for (int y = 0; y < rowsDecoded; y++) {
            blender.blendLine(dst, src, plan.scaled_width);
            src = SkTAddOffset<const uint8_t>(src, srcRowBytes);
            dst = SkTAddOffset<void>(dst, rowBytes);
        }
    }

    return result;
}

SkWebpCodec::SkWebpCodec(SkEncodedInfo&& info,
                         std::unique_ptr<SkStream> stream,
                         WebPDemuxer* demux,
                         sk_sp<SkData> data,
                         SkEncodedOrigin origin,
                         bool onlyHeaderParsed)
        : INHERITED(std::move(info), skcms_PixelFormat_BGRA_8888, std::move(stream), origin)
        , fDemux(demux)
        , fData(std::move(data))
        , fFailed(false)
        , fOnlyHeaderParsed(onlyHeaderParsed) {
    const auto& eInfo = this->getEncodedInfo();
    fFrameHolder.setScreenSize(eInfo.width(), eInfo.height());
}

namespace SkWebpDecoder {
bool IsWebp(const void* data, size_t len) {
    return SkWebpCodec::IsWebp(data, len);
}

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    return SkWebpCodec::MakeFromStream(std::move(stream), outResult);
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
}  // namespace SkWebpDecoder
