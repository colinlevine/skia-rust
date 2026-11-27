/*
 * Safe Rust bridge for:
 *   src/codec/SkWuffsCodec.cpp
 * (Phase 2 Module P2.7: skia-rs-codec-gif, GN target: optional("wuffs")).
 *
 * Keeps thin third_party/wuffs (`wuffs_gif__decoder__*`, `wuffs_base__*`)
 * bitstream/LZW/swizzle calls at the boundary while delegating GIF signature
 * sniffing, stream-seekability policy, pixel-format/one-pass decisions,
 * encoded-info classification, I/O buffer seek/compact arithmetic, frame
 * holder / dependency / disposal / duration / repetition state, and the full
 * incremental one-pass / two-pass decode state machine to `#![forbid(unsafe_code)]`
 * safe Rust in `skia-rs-codec-gif`.
 */

#include "include/codec/SkCodec.h"
#include "include/codec/SkCodecAnimation.h"
#include "include/codec/SkEncodedImageFormat.h"
#include "include/codec/SkGifDecoder.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkBlendMode.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkMatrix.h"
#include "include/core/SkPaint.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRect.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSamplingOptions.h"
#include "include/core/SkSize.h"
#include "include/core/SkStream.h"
#include "include/core/SkTypes.h"
#include "include/private/SkAssert.h"
#include "include/private/SkEncodedInfo.h"
#include "include/private/SkMalloc.h"
#include "include/private/SkTo.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkFrameHolder.h"
#include "src/codec/SkSampler.h"
#include "src/codec/SkScalingCodec.h"
#include "src/core/SkDraw.h"
#include "src/core/SkMipmap.h"
#include "src/core/SkRasterClip.h"
#include "src/core/SkStreamPriv.h"
#include "src/partition_alloc/raw_ptr.h"

#include <climits>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <utility>
#include <vector>

#if defined(WUFFS_IMPLEMENTATION)
#error "SkWuffsCodec should not #define WUFFS_IMPLEMENTATION"
#endif
#include "wuffs-v0.3.c"  // NO_G3_REWRITE
#if WUFFS_VERSION_BUILD_METADATA_COMMIT_COUNT < 2514
#error "Wuffs version is too old. Upgrade to the latest version."
#endif

#define SK_WUFFS_CODEC_BUFFER_SIZE 4096

#if defined(SK_WUFFS_FAVORS_PERFORMANCE_OVER_ADDITIONAL_MEMORY_SAFETY)
#define SK_WUFFS_INITIALIZE_FLAGS WUFFS_INITIALIZE__LEAVE_INTERNAL_BUFFERS_UNINITIALIZED
#else
#define SK_WUFFS_INITIALIZE_FLAGS WUFFS_INITIALIZE__DEFAULT_OPTIONS
#endif

namespace {

// ============================================================================
// Discriminant & constant assertions shared across FFI with `skia-rs-codec-gif`.
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
static_assert(SkCodec::kNoFrame == -1);
static_assert(SkCodec::kRepetitionCountInfinite == -1);

static_assert(static_cast<int>(SkCodec::SelectionPolicy::kPreferStillImage) == 0);
static_assert(static_cast<int>(SkCodec::SelectionPolicy::kPreferAnimation) == 1);

static_assert(static_cast<int>(SkCodec::kYes_ZeroInitialized) == 0);
static_assert(static_cast<int>(SkCodec::kNo_ZeroInitialized) == 1);

static_assert(static_cast<int>(SkCodec::IsAnimated::kYes) == 0);
static_assert(static_cast<int>(SkCodec::IsAnimated::kNo) == 1);
static_assert(static_cast<int>(SkCodec::IsAnimated::kUnknown) == 2);

static_assert(static_cast<int>(SkEncodedInfo::kRGBA_Color) == 6);
static_assert(static_cast<int>(SkEncodedInfo::kBGRA_Color) == 9);

static_assert(static_cast<int>(SkEncodedInfo::kOpaque_Alpha) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kUnpremul_Alpha) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kBinary_Alpha) == 2);

static_assert(static_cast<int>(SkCodecAnimation::DisposalMethod::kKeep) == 1);
static_assert(static_cast<int>(SkCodecAnimation::DisposalMethod::kRestoreBGColor) == 2);
static_assert(static_cast<int>(SkCodecAnimation::DisposalMethod::kRestorePrevious) == 3);

static_assert(static_cast<int>(SkCodecAnimation::Blend::kSrcOver) == 0);
static_assert(static_cast<int>(SkCodecAnimation::Blend::kSrc) == 1);

static_assert(static_cast<uint8_t>(WUFFS_BASE__ANIMATION_DISPOSAL__NONE) == 0);
static_assert(static_cast<uint8_t>(WUFFS_BASE__ANIMATION_DISPOSAL__RESTORE_BACKGROUND) == 1);
static_assert(static_cast<uint8_t>(WUFFS_BASE__ANIMATION_DISPOSAL__RESTORE_PREVIOUS) == 2);

static_assert(static_cast<uint32_t>(WUFFS_BASE__PIXEL_FORMAT__INVALID) == 0x00000000u);
static_assert(static_cast<uint32_t>(WUFFS_BASE__PIXEL_FORMAT__BGR_565) == 0x80000565u);
static_assert(static_cast<uint32_t>(WUFFS_BASE__PIXEL_FORMAT__BGRA_NONPREMUL) == 0x81008888u);
static_assert(static_cast<uint32_t>(WUFFS_BASE__PIXEL_FORMAT__RGBA_NONPREMUL) == 0xA1008888u);
static_assert(static_cast<uint32_t>(WUFFS_BASE__PIXEL_SUBSAMPLING__NONE) == 0x00000000u);

static_assert(static_cast<uint8_t>(WUFFS_BASE__PIXEL_BLEND__SRC) == 0);
static_assert(static_cast<uint8_t>(WUFFS_BASE__PIXEL_BLEND__SRC_OVER) == 1);

static_assert(WUFFS_BASE__FLICKS_PER_MILLISECOND == 705600ull);
static_assert(SK_WUFFS_CODEC_BUFFER_SIZE == 4096);

// ============================================================================
// FFI structs & declarations matching `skia-rs-ffi/src/codec_gif.rs`.
// ============================================================================

struct SkiaRsGifCodecState;

enum SkiaRsWuffsStatusKind : int32_t {
    kWuffsStatus_Ok = 0,
    kWuffsStatus_ShortRead = 1,
    kWuffsStatus_EndOfData = 2,
    kWuffsStatus_Error = 3,
};

enum SkiaRsGifSeekBufferAction : int32_t {
    kSeekAction_HitBuffer = 0,
    kSeekAction_NeedStreamSeek = 1,
    kSeekAction_Overflow = 2,
};

enum SkiaRsGifFrameCountStepAction : int32_t {
    kFrameStep_Continue = 0,
    kFrameStep_AppendedFrame = 1,
    kFrameStep_Completed = 2,
    kFrameStep_Abort = 3,
};

struct SkiaRsIRect {
    int32_t left;
    int32_t top;
    int32_t right;
    int32_t bottom;
};

struct SkiaRsWuffsRectU32 {
    uint32_t min_incl_x;
    uint32_t min_incl_y;
    uint32_t max_excl_x;
    uint32_t max_excl_y;
};

struct SkiaRsGifEncodedInfo {
    int32_t width;
    int32_t height;
    int32_t color;
    int32_t alpha;
    uint8_t bits_per_component;
};

struct SkiaRsGifIoBufferMeta {
    size_t wi;
    size_t ri;
    uint64_t pos;
    bool closed;
};

struct SkiaRsGifSeekBufferPlan {
    int32_t action;
    size_t stream_offset;
};

struct SkiaRsGifCompactPlan {
    size_t src_offset;
    size_t len;
};

struct SkiaRsGifFrameConfigInput {
    uint64_t index;
    uint64_t io_position;
    SkiaRsWuffsRectU32 bounds;
    int64_t duration_flicks;
    uint8_t disposal;
    bool opaque_within_bounds;
    bool overwrite_instead_of_blend;
};

struct SkiaRsGifFrame {
    int32_t frame_id;
    uint64_t io_position;
    int32_t reported_alpha;
    bool has_alpha;
    int32_t required_frame;
    SkiaRsIRect frame_rect;
    int32_t disposal_method;
    int32_t duration_ms;
    int32_t blend;
};

struct SkiaRsGifFrameInfoFields {
    int32_t required_frame;
    int32_t duration;
    bool fully_received;
    int32_t alpha_type;
    bool has_alpha_within_bounds;
    int32_t disposal_method;
    int32_t blend;
    SkiaRsIRect frame_rect;
};

struct SkiaRsGifGetFrameCountPlan {
    bool need_scan;
    int32_t immediate_count;
    int32_t start_index;
};

struct SkiaRsGifFrameCountStepResult {
    int32_t action;
    SkiaRsGifFrame appended_frame;
};

struct SkiaRsGifStartIncrDecodePlan {
    int32_t result;
    bool one_pass;
    uint32_t one_pass_pixel_format;
    size_t one_pass_table_width_bytes;
    bool one_pass_should_fill_dst;
    uint8_t pixel_blend;
};

struct SkiaRsGifTwoPassClearPlan {
    int32_t result;
    bool bounds_valid;
    size_t start_byte_offset;
    size_t row_byte_len;
    size_t num_rows;
    bool contiguous;
    size_t contiguous_byte_len;
};

struct SkiaRsGifTwoPassDecodePlan {
    int32_t result;
    bool early_return;
    bool should_fill_dst;
    bool should_draw;
    bool dirty_bounds_valid;
    size_t src_byte_offset;
    int32_t dirty_width;
    int32_t dirty_height;
    int32_t translate_x;
    int32_t translate_y;
    int32_t src_alpha_type;
    bool use_blend_src;
    bool should_release_two_pass_pixbuf;
};

extern "C" {
bool skia_rs_gif_is_gif(const uint8_t* buf, size_t len);
bool skia_rs_gif_should_copy_stream_to_memory(int32_t selection_policy, bool can_seek);
int32_t skia_rs_gif_n32_wuffs_pixfmt(int32_t n32_ct, uint32_t* out_pixfmt);
int32_t skia_rs_gif_classify_encoded_info(uint32_t width,
                                          uint32_t height,
                                          uint32_t pixfmt,
                                          bool first_frame_is_opaque,
                                          SkiaRsGifEncodedInfo* out_info);
int32_t skia_rs_gif_validate_buffer_len(uint64_t len_u64, size_t* out_len);
int32_t skia_rs_gif_map_wuffs_decode_status(int32_t status_kind);
int32_t skia_rs_gif_loops_to_repetition_count(uint32_t num_animation_loops);
void skia_rs_gif_scaled_dimensions(int32_t codec_w,
                                   int32_t codec_h,
                                   float desired_scale,
                                   int32_t* out_w,
                                   int32_t* out_h);
bool skia_rs_gif_dimensions_supported(int32_t codec_w,
                                      int32_t codec_h,
                                      int32_t req_w,
                                      int32_t req_h);

bool skia_rs_gif_io_buffer_plan_seek(SkiaRsGifIoBufferMeta* meta,
                                     uint64_t target_pos,
                                     SkiaRsGifSeekBufferPlan* out_plan);
void skia_rs_gif_io_buffer_apply_stream_seek(SkiaRsGifIoBufferMeta* meta, uint64_t target_pos);
bool skia_rs_gif_io_buffer_on_bytes_read(SkiaRsGifIoBufferMeta* meta, size_t num_read);
bool skia_rs_gif_io_buffer_compact(SkiaRsGifIoBufferMeta* meta, SkiaRsGifCompactPlan* out_plan);

SkiaRsGifCodecState* skia_rs_gif_state_new(int32_t width,
                                           int32_t height,
                                           bool can_seek,
                                           uint64_t first_frame_io_pos);
void skia_rs_gif_state_free(SkiaRsGifCodecState* state);
bool skia_rs_gif_state_decoder_is_suspended(const SkiaRsGifCodecState* state);
void skia_rs_gif_state_on_decode_step_finished(SkiaRsGifCodecState* state,
                                               bool is_complete,
                                               uint64_t num_decoded_frames);
void skia_rs_gif_state_update_num_fully_received_frames(SkiaRsGifCodecState* state,
                                                        uint64_t num_decoded_frames);
int32_t skia_rs_gif_state_on_reset_decoder_result(SkiaRsGifCodecState* state, int32_t reset_result);
int32_t skia_rs_gif_state_resolve_frame_io_position(const SkiaRsGifCodecState* state,
                                                    int32_t frame_index,
                                                    uint64_t* out_pos);
bool skia_rs_gif_state_plan_get_frame_count(const SkiaRsGifCodecState* state,
                                            SkiaRsGifGetFrameCountPlan* out_plan);
bool skia_rs_gif_state_on_frame_count_step(SkiaRsGifCodecState* state,
                                           int32_t index,
                                           int32_t status_kind,
                                           const SkiaRsGifFrameConfigInput* fc,
                                           SkiaRsGifFrameCountStepResult* out_step);
void skia_rs_gif_state_mark_frames_complete(SkiaRsGifCodecState* state);
int32_t skia_rs_gif_state_frame_count(const SkiaRsGifCodecState* state);
bool skia_rs_gif_state_get_frame_info(const SkiaRsGifCodecState* state,
                                      int32_t index,
                                      SkiaRsGifFrameInfoFields* out_info);
int32_t skia_rs_gif_state_is_animated(const SkiaRsGifCodecState* state);
int32_t skia_rs_gif_validate_start_incremental_decode(bool has_dst, bool has_subset);
bool skia_rs_gif_state_begin_start_incremental_decode(SkiaRsGifCodecState* state,
                                                      int32_t dst_ct,
                                                      int32_t dst_w,
                                                      int32_t dst_h,
                                                      bool has_color_profile,
                                                      int32_t frame_index,
                                                      SkiaRsGifStartIncrDecodePlan* out_plan);
bool skia_rs_gif_plan_two_pass_clear(uint32_t src_bits_per_pixel,
                                     SkiaRsWuffsRectU32 frame_rect,
                                     size_t pixels_width_bytes,
                                     size_t pixels_height,
                                     size_t pixels_stride,
                                     SkiaRsGifTwoPassClearPlan* out_plan);
void skia_rs_gif_state_commit_start_incremental_decode(SkiaRsGifCodecState* state);
int32_t skia_rs_gif_state_check_incremental_decode_active(const SkiaRsGifCodecState* state);
bool skia_rs_gif_state_is_one_pass(const SkiaRsGifCodecState* state);
uint8_t skia_rs_gif_state_pixel_blend(const SkiaRsGifCodecState* state);
bool skia_rs_gif_state_plan_two_pass_decode(SkiaRsGifCodecState* state,
                                            int32_t status_kind,
                                            int32_t frame_index,
                                            bool encoded_info_opaque,
                                            uint32_t src_bits_per_pixel,
                                            SkiaRsWuffsRectU32 frame_rect,
                                            SkiaRsWuffsRectU32 dirty_rect,
                                            size_t pixels_width_bytes,
                                            size_t pixels_height,
                                            size_t pixels_stride,
                                            SkiaRsGifTwoPassDecodePlan* out_plan);
void skia_rs_gif_state_finish_incremental_decode(SkiaRsGifCodecState* state, int32_t result);
}

// ============================================================================
// Thin Wuffs boundary helpers delegating arithmetic/state to safe Rust.
// ============================================================================

inline int32_t classify_wuffs_status(const char* repr) {
    if (repr == nullptr) {
        return kWuffsStatus_Ok;
    }
    if (repr == wuffs_base__suspension__short_read) {
        return kWuffsStatus_ShortRead;
    }
    if (repr == wuffs_base__note__end_of_data) {
        return kWuffsStatus_EndOfData;
    }
    return kWuffsStatus_Error;
}

inline SkiaRsWuffsRectU32 to_rs_rect(wuffs_base__rect_ie_u32 r) {
    return {r.min_incl_x, r.min_incl_y, r.max_excl_x, r.max_excl_y};
}

inline SkiaRsGifIoBufferMeta to_rs_meta(const wuffs_base__io_buffer_meta& m) {
    return {m.wi, m.ri, m.pos, m.closed};
}

inline void from_rs_meta(const SkiaRsGifIoBufferMeta& src, wuffs_base__io_buffer_meta* dst) {
    dst->wi = src.wi;
    dst->ri = src.ri;
    dst->pos = src.pos;
    dst->closed = src.closed;
}

bool fill_buffer(wuffs_base__io_buffer* b, SkStream* s) {
    SkiaRsGifIoBufferMeta meta = to_rs_meta(b->meta);
    SkiaRsGifCompactPlan compact{};
    skia_rs_gif_io_buffer_compact(&meta, &compact);
    if (compact.len > 0) {
        memmove(b->data.ptr, b->data.ptr + compact.src_offset, compact.len);
    }
    size_t num_read = s->read(b->data.ptr + meta.wi, b->data.len - meta.wi);
    bool got_bytes = skia_rs_gif_io_buffer_on_bytes_read(&meta, num_read);
    from_rs_meta(meta, &b->meta);
    return got_bytes;
}

bool seek_buffer(wuffs_base__io_buffer* b, SkStream* s, uint64_t pos) {
    SkiaRsGifIoBufferMeta meta = to_rs_meta(b->meta);
    SkiaRsGifSeekBufferPlan plan{};
    if (!skia_rs_gif_io_buffer_plan_seek(&meta, pos, &plan)) {
        return false;
    }
    if (plan.action == kSeekAction_HitBuffer) {
        from_rs_meta(meta, &b->meta);
        return true;
    }
    if (plan.action != kSeekAction_NeedStreamSeek || !s->seek(plan.stream_offset)) {
        return false;
    }
    skia_rs_gif_io_buffer_apply_stream_seek(&meta, pos);
    from_rs_meta(meta, &b->meta);
    return true;
}

SkCodec::Result reset_and_decode_image_config(wuffs_gif__decoder* decoder,
                                              wuffs_base__image_config* imgcfg,
                                              wuffs_base__io_buffer* b,
                                              SkStream* s) {
    wuffs_base__status status =
            decoder->initialize(sizeof__wuffs_gif__decoder(), WUFFS_VERSION, SK_WUFFS_INITIALIZE_FLAGS);
    if (status.repr != nullptr) {
        SkCodecPrintf("initialize: %s", status.message());
        return SkCodec::kInternalError;
    }

    // See https://bugs.chromium.org/p/skia/issues/detail?id=12055
    decoder->set_quirk_enabled(WUFFS_GIF__QUIRK_IGNORE_TOO_MUCH_PIXEL_DATA, true);

    while (true) {
        status = decoder->decode_image_config(imgcfg, b);
        const int32_t kind = classify_wuffs_status(status.repr);
        if (kind == kWuffsStatus_Ok) {
            break;
        }
        if (kind == kWuffsStatus_ShortRead) {
            if (fill_buffer(b, s)) {
                continue;
            }
            return static_cast<SkCodec::Result>(skia_rs_gif_map_wuffs_decode_status(kind));
        }
        SkCodecPrintf("decode_image_config: %s", status.message());
        return static_cast<SkCodec::Result>(skia_rs_gif_map_wuffs_decode_status(kind));
    }

    uint32_t pixfmt = WUFFS_BASE__PIXEL_FORMAT__INVALID;
    const auto fmtRes = static_cast<SkCodec::Result>(
            skia_rs_gif_n32_wuffs_pixfmt(static_cast<int32_t>(kN32_SkColorType), &pixfmt));
    if (fmtRes != SkCodec::kSuccess) {
        return fmtRes;
    }
    if (imgcfg) {
        imgcfg->pixcfg.set(pixfmt,
                           WUFFS_BASE__PIXEL_SUBSAMPLING__NONE,
                           imgcfg->pixcfg.width(),
                           imgcfg->pixcfg.height());
    }
    return SkCodec::kSuccess;
}

}  // namespace

// ============================================================================
// Thin C++ SkFrame / SkFrameHolder / SkWuffsCodec wrappers backed by Rust state.
// ============================================================================

class SkWuffsCodec;

class SkWuffsFrame final : public SkFrame {
public:
    explicit SkWuffsFrame(const SkiaRsGifFrame& rf)
            : INHERITED(rf.frame_id)
            , fReportedAlpha(static_cast<SkEncodedInfo::Alpha>(rf.reported_alpha)) {
        this->setXYWH(rf.frame_rect.left,
                      rf.frame_rect.top,
                      rf.frame_rect.right - rf.frame_rect.left,
                      rf.frame_rect.bottom - rf.frame_rect.top);
        this->setDisposalMethod(static_cast<SkCodecAnimation::DisposalMethod>(rf.disposal_method));
        this->setDuration(rf.duration_ms);
        this->setBlend(static_cast<SkCodecAnimation::Blend>(rf.blend));
        this->setHasAlpha(rf.has_alpha);
        this->setRequiredFrame(rf.required_frame);
    }

    SkEncodedInfo::Alpha onReportedAlpha() const override { return fReportedAlpha; }

private:
    SkEncodedInfo::Alpha fReportedAlpha;

    using INHERITED = SkFrame;
};

class SkWuffsFrameHolder final : public SkFrameHolder {
public:
    SkWuffsFrameHolder() : INHERITED() {}

    void init(SkWuffsCodec* codec, int width, int height);

    const SkFrame* onGetFrame(int i) const override;

private:
    raw_ptr<const SkWuffsCodec> fCodec;

    using INHERITED = SkFrameHolder;
};

class SkWuffsCodec final : public SkScalingCodec {
public:
    SkWuffsCodec(SkEncodedInfo&& encodedInfo,
                 std::unique_ptr<SkStream> stream,
                 std::unique_ptr<SkiaRsGifCodecState, decltype(&skia_rs_gif_state_free)> state,
                 std::unique_ptr<wuffs_gif__decoder, decltype(&sk_free)> dec,
                 std::unique_ptr<uint8_t, decltype(&sk_free)> workbuf_ptr,
                 size_t workbuf_len,
                 wuffs_base__image_config imgcfg,
                 wuffs_base__io_buffer iobuf);

    const SkWuffsFrame* frame(int i) const;

private:
    SkEncodedImageFormat onGetEncodedFormat() const override;
    SkISize onGetScaledDimensions(float desiredScale) const override;
    bool onDimensionsSupported(const SkISize& requested) override;
    Result onGetPixels(const SkImageInfo&, void*, size_t, const Options&, int*) override;
    const SkFrameHolder* getFrameHolder() const override;
    bool onSupportsIncrementalDecode(const SkImageInfo&) override { return true; }
    Result onStartIncrementalDecode(const SkImageInfo& dstInfo,
                                    void* dst,
                                    size_t rowBytes,
                                    const SkCodec::Options& options) override;
    Result onIncrementalDecode(int* rowsDecoded) override;
    int onGetFrameCount() override;
    bool onGetFrameInfo(int, FrameInfo*) const override;
    int onGetRepetitionCount() override;
    IsAnimated onIsAnimated() override;
    sk_sp<const SkData> getEncodedData() const override;

    Result onStartIncrementalDecodeOnePass(const SkImageInfo& dstInfo,
                                           uint8_t* dst,
                                           size_t rowBytes,
                                           const SkCodec::Options& options,
                                           const SkiaRsGifStartIncrDecodePlan& plan);
    Result onStartIncrementalDecodeTwoPass();
    Result onIncrementalDecodeOnePass();
    Result onIncrementalDecodeTwoPass();

    void onGetFrameCountInternal(int startIndex);
    Result seekFrame(int frameIndex);
    Result resetDecoder();
    const char* decodeFrameConfig();
    const char* decodeFrame();
    void updateNumFullyReceivedFrames();

    SkWuffsFrameHolder fFrameHolder;
    std::unique_ptr<SkStream> fPrivStream;
    std::unique_ptr<SkiaRsGifCodecState, decltype(&skia_rs_gif_state_free)> fState;
    std::unique_ptr<uint8_t, decltype(&sk_free)> fWorkbufPtr;
    size_t fWorkbufLen;

    std::unique_ptr<wuffs_gif__decoder, decltype(&sk_free)> fDecoder;

    wuffs_base__frame_config fFrameConfig;
    wuffs_base__pixel_config fPixelConfig;
    wuffs_base__pixel_buffer fPixelBuffer;
    wuffs_base__io_buffer fIOBuffer;

    // C++ pointer/stride handles for active incremental decode.
    raw_ptr<uint8_t, DanglingUntriaged> fIncrDecDst;
    size_t fIncrDecRowBytes;

    // Lazily allocated intermediate pixel buffer for two-pass decoding.
    std::unique_ptr<uint8_t, decltype(&sk_free)> fTwoPassPixbufPtr;
    size_t fTwoPassPixbufLen;

    // Mirror of Rust frame entries so SkCodec::getFrameHolder() can return `const SkFrame*`.
    std::vector<SkWuffsFrame> fFrames;

    uint8_t fBuffer[SK_WUFFS_CODEC_BUFFER_SIZE];

    using INHERITED = SkScalingCodec;
};

void SkWuffsFrameHolder::init(SkWuffsCodec* codec, int width, int height) {
    fCodec = codec;
    fScreenWidth = width;
    fScreenHeight = height;
}

const SkFrame* SkWuffsFrameHolder::onGetFrame(int i) const {
    return fCodec->frame(i);
}

SkWuffsCodec::SkWuffsCodec(
        SkEncodedInfo&& encodedInfo,
        std::unique_ptr<SkStream> stream,
        std::unique_ptr<SkiaRsGifCodecState, decltype(&skia_rs_gif_state_free)> state,
        std::unique_ptr<wuffs_gif__decoder, decltype(&sk_free)> dec,
        std::unique_ptr<uint8_t, decltype(&sk_free)> workbuf_ptr,
        size_t workbuf_len,
        wuffs_base__image_config imgcfg,
        wuffs_base__io_buffer iobuf)
        : INHERITED(std::move(encodedInfo), skcms_PixelFormat_RGBA_8888, nullptr)
        , fFrameHolder()
        , fPrivStream(std::move(stream))
        , fState(std::move(state))
        , fWorkbufPtr(std::move(workbuf_ptr))
        , fWorkbufLen(workbuf_len)
        , fDecoder(std::move(dec))
        , fFrameConfig(wuffs_base__null_frame_config())
        , fPixelConfig(imgcfg.pixcfg)
        , fPixelBuffer(wuffs_base__null_pixel_buffer())
        , fIOBuffer(wuffs_base__empty_io_buffer())
        , fIncrDecDst(nullptr)
        , fIncrDecRowBytes(0)
        , fTwoPassPixbufPtr(nullptr, &sk_free)
        , fTwoPassPixbufLen(0) {
    fFrameHolder.init(this, imgcfg.pixcfg.width(), imgcfg.pixcfg.height());

    SkASSERT(iobuf.data.len == SK_WUFFS_CODEC_BUFFER_SIZE);
    memmove(fBuffer, iobuf.data.ptr, iobuf.meta.wi);
    fIOBuffer.data = wuffs_base__make_slice_u8(fBuffer, SK_WUFFS_CODEC_BUFFER_SIZE);
    fIOBuffer.meta = iobuf.meta;
}

const SkWuffsFrame* SkWuffsCodec::frame(int i) const {
    if ((0 <= i) && (static_cast<size_t>(i) < fFrames.size())) {
        return &fFrames[i];
    }
    return nullptr;
}

SkEncodedImageFormat SkWuffsCodec::onGetEncodedFormat() const {
    return SkEncodedImageFormat::kGIF;
}

SkISize SkWuffsCodec::onGetScaledDimensions(float desiredScale) const {
    SkISize dim = this->dimensions();
    int32_t outW = 1;
    int32_t outH = 1;
    skia_rs_gif_scaled_dimensions(dim.width(), dim.height(), desiredScale, &outW, &outH);
    return SkISize::Make(outW, outH);
}

bool SkWuffsCodec::onDimensionsSupported(const SkISize& requested) {
    SkISize dim = this->dimensions();
    return skia_rs_gif_dimensions_supported(
            dim.width(), dim.height(), requested.width(), requested.height());
}

SkCodec::Result SkWuffsCodec::onGetPixels(const SkImageInfo& dstInfo,
                                          void* dst,
                                          size_t rowBytes,
                                          const Options& options,
                                          int* rowsDecoded) {
    SkCodec::Result result = this->onStartIncrementalDecode(dstInfo, dst, rowBytes, options);
    if (result != kSuccess) {
        return result;
    }
    return this->onIncrementalDecode(rowsDecoded);
}

const SkFrameHolder* SkWuffsCodec::getFrameHolder() const {
    return &fFrameHolder;
}

SkCodec::Result SkWuffsCodec::onStartIncrementalDecode(const SkImageInfo& dstInfo,
                                                       void* dst,
                                                       size_t rowBytes,
                                                       const SkCodec::Options& options) {
    const auto validRes = static_cast<SkCodec::Result>(
            skia_rs_gif_validate_start_incremental_decode(dst != nullptr, options.fSubset != nullptr));
    if (validRes != SkCodec::kSuccess) {
        return validRes;
    }

    SkCodec::Result result = this->seekFrame(options.fFrameIndex);
    if (result != SkCodec::kSuccess) {
        return result;
    }

    const char* status = this->decodeFrameConfig();
    if (status != nullptr) {
        const int32_t kind = classify_wuffs_status(status);
        if (kind != kWuffsStatus_ShortRead) {
            SkCodecPrintf("decodeFrameConfig: %s", status);
        }
        return static_cast<SkCodec::Result>(skia_rs_gif_map_wuffs_decode_status(kind));
    }

    SkiaRsGifStartIncrDecodePlan plan{};
    if (!skia_rs_gif_state_begin_start_incremental_decode(
                fState.get(),
                static_cast<int32_t>(dstInfo.colorType()),
                dstInfo.width(),
                dstInfo.height(),
                getEncodedInfo().colorProfile() != nullptr,
                options.fFrameIndex,
                &plan)) {
        return SkCodec::kInternalError;
    }

    result = plan.one_pass ? this->onStartIncrementalDecodeOnePass(
                                     dstInfo, static_cast<uint8_t*>(dst), rowBytes, options, plan)
                           : this->onStartIncrementalDecodeTwoPass();
    if (result != SkCodec::kSuccess) {
        return result;
    }

    fIncrDecDst = static_cast<uint8_t*>(dst);
    fIncrDecRowBytes = rowBytes;
    skia_rs_gif_state_commit_start_incremental_decode(fState.get());
    return SkCodec::kSuccess;
}

SkCodec::Result SkWuffsCodec::onStartIncrementalDecodeOnePass(
        const SkImageInfo& dstInfo,
        uint8_t* dst,
        size_t rowBytes,
        const SkCodec::Options& options,
        const SkiaRsGifStartIncrDecodePlan& plan) {
    wuffs_base__pixel_config pixelConfig;
    pixelConfig.set(plan.one_pass_pixel_format,
                    WUFFS_BASE__PIXEL_SUBSAMPLING__NONE,
                    dstInfo.width(),
                    dstInfo.height());

    wuffs_base__table_u8 table;
    table.ptr = dst;
    table.width = plan.one_pass_table_width_bytes;
    table.height = dstInfo.height();
    table.stride = rowBytes;

    wuffs_base__status status = fPixelBuffer.set_from_table(&pixelConfig, table);
    if (status.repr != nullptr) {
        SkCodecPrintf("set_from_table: %s", status.message());
        return SkCodec::kInternalError;
    }

    if (plan.one_pass_should_fill_dst) {
        SkSampler::Fill(dstInfo, dst, rowBytes, options.fZeroInitialized);
    }
    return SkCodec::kSuccess;
}

SkCodec::Result SkWuffsCodec::onStartIncrementalDecodeTwoPass() {
    bool alreadyZeroed = false;

    if (!fTwoPassPixbufPtr) {
        size_t pixbufLen = 0;
        if (skia_rs_gif_validate_buffer_len(fPixelConfig.pixbuf_len(), &pixbufLen) !=
            static_cast<int32_t>(SkCodec::kSuccess)) {
            return SkCodec::kInternalError;
        }
        void* pixbufPtrRaw = sk_malloc_flags(pixbufLen, SK_MALLOC_ZERO_INITIALIZE);
        if (!pixbufPtrRaw) {
            return SkCodec::kInternalError;
        }
        fTwoPassPixbufPtr.reset(reinterpret_cast<uint8_t*>(pixbufPtrRaw));
        fTwoPassPixbufLen = pixbufLen;
        alreadyZeroed = true;
    }

    wuffs_base__status status = fPixelBuffer.set_from_slice(
            &fPixelConfig, wuffs_base__make_slice_u8(fTwoPassPixbufPtr.get(), fTwoPassPixbufLen));
    if (status.repr != nullptr) {
        SkCodecPrintf("set_from_slice: %s", status.message());
        return SkCodec::kInternalError;
    }

    if (!alreadyZeroed) {
        wuffs_base__table_u8 pixels = fPixelBuffer.plane(0);
        SkiaRsGifTwoPassClearPlan clearPlan{};
        if (!skia_rs_gif_plan_two_pass_clear(fPixelConfig.pixel_format().bits_per_pixel(),
                                             to_rs_rect(fFrameConfig.bounds()),
                                             pixels.width,
                                             pixels.height,
                                             pixels.stride,
                                             &clearPlan) ||
            clearPlan.result != static_cast<int32_t>(SkCodec::kSuccess)) {
            return static_cast<SkCodec::Result>(clearPlan.result);
        }

        uint8_t* ptr = pixels.ptr + clearPlan.start_byte_offset;
        if (clearPlan.contiguous) {
            sk_bzero(ptr, clearPlan.contiguous_byte_len);
        } else {
            for (size_t row = 0; row < clearPlan.num_rows; ++row) {
                sk_bzero(ptr, clearPlan.row_byte_len);
                ptr += pixels.stride;
            }
        }
    }

    return SkCodec::kSuccess;
}

SkCodec::Result SkWuffsCodec::onIncrementalDecode(int* rowsDecoded) {
    const auto activeRes = static_cast<SkCodec::Result>(
            skia_rs_gif_state_check_incremental_decode_active(fState.get()));
    if (activeRes != SkCodec::kSuccess || !fIncrDecDst) {
        return SkCodec::kInternalError;
    }

    if (rowsDecoded) {
        *rowsDecoded = dstInfo().height();
    }

    SkCodec::Result result = skia_rs_gif_state_is_one_pass(fState.get())
                                     ? this->onIncrementalDecodeOnePass()
                                     : this->onIncrementalDecodeTwoPass();
    skia_rs_gif_state_finish_incremental_decode(fState.get(), static_cast<int32_t>(result));
    if (result == SkCodec::kSuccess) {
        fIncrDecDst = nullptr;
        fIncrDecRowBytes = 0;
    }
    return result;
}

SkCodec::Result SkWuffsCodec::onIncrementalDecodeOnePass() {
    const char* status = this->decodeFrame();
    const int32_t kind = classify_wuffs_status(status);
    if (kind != kWuffsStatus_Ok && kind != kWuffsStatus_ShortRead) {
        SkCodecPrintf("decodeFrame: %s", status);
    }
    return static_cast<SkCodec::Result>(skia_rs_gif_map_wuffs_decode_status(kind));
}

SkCodec::Result SkWuffsCodec::onIncrementalDecodeTwoPass() {
    const char* status = this->decodeFrame();
    const int32_t kind = classify_wuffs_status(status);
    if (kind != kWuffsStatus_Ok && kind != kWuffsStatus_ShortRead) {
        SkCodecPrintf("decodeFrame: %s", status);
    }

    wuffs_base__table_u8 pixels = fPixelBuffer.plane(0);
    SkiaRsGifTwoPassDecodePlan plan{};
    if (!skia_rs_gif_state_plan_two_pass_decode(fState.get(),
                                                kind,
                                                options().fFrameIndex,
                                                getEncodedInfo().opaque(),
                                                fPixelBuffer.pixcfg.pixel_format().bits_per_pixel(),
                                                to_rs_rect(fFrameConfig.bounds()),
                                                to_rs_rect(fDecoder->frame_dirty_rect()),
                                                pixels.width,
                                                pixels.height,
                                                pixels.stride,
                                                &plan)) {
        return SkCodec::kInternalError;
    }

    const auto result = static_cast<SkCodec::Result>(plan.result);
    if (plan.early_return) {
        return result;
    }

    if (plan.should_fill_dst) {
        SkSampler::Fill(dstInfo(), fIncrDecDst, fIncrDecRowBytes, options().fZeroInitialized);
    }

    if (plan.should_draw) {
        SkASSERT(!getEncodedInfo().colorProfile());
        uint8_t* s = pixels.ptr + plan.src_byte_offset;
        auto srcInfo = getInfo()
                               .makeWH(plan.dirty_width, plan.dirty_height)
                               .makeAlphaType(static_cast<SkAlphaType>(plan.src_alpha_type));
        SkBitmap src;
        src.installPixels(srcInfo, s, pixels.stride);
        SkPaint paint;
        if (plan.use_blend_src) {
            paint.setBlendMode(SkBlendMode::kSrc);
        }

        skcpu::Draw draw;
        draw.fDst.reset(dstInfo(), fIncrDecDst, fIncrDecRowBytes);
        SkMatrix matrix = SkMatrix::RectToRectOrIdentity(
                SkRect::Make(this->dimensions()), SkRect::Make(this->dstInfo().dimensions()));
        draw.fCTM = &matrix;
        SkRasterClip rc(SkIRect::MakeSize(this->dstInfo().dimensions()));
        draw.fRC = &rc;

        SkMatrix translate = SkMatrix::Translate(plan.translate_x, plan.translate_y);
        draw.drawBitmap(src, translate, nullptr, SkSamplingOptions(), paint, nullptr);
    }

    if (plan.should_release_two_pass_pixbuf) {
        fTwoPassPixbufPtr.reset(nullptr);
        fTwoPassPixbufLen = 0;
    }

    return result;
}

int SkWuffsCodec::onGetFrameCount() {
    SkiaRsGifGetFrameCountPlan plan{};
    if (!skia_rs_gif_state_plan_get_frame_count(fState.get(), &plan)) {
        return 0;
    }
    if (!plan.need_scan) {
        return plan.immediate_count;
    }
    this->onGetFrameCountInternal(plan.start_index);
    this->updateNumFullyReceivedFrames();
    return skia_rs_gif_state_frame_count(fState.get());
}

void SkWuffsCodec::onGetFrameCountInternal(int startIndex) {
    if (this->seekFrame(startIndex) != SkCodec::kSuccess) {
        return;
    }

    for (int i = startIndex; i < INT_MAX; i++) {
        const char* status = this->decodeFrameConfig();
        const int32_t kind = classify_wuffs_status(status);
        SkiaRsGifFrameConfigInput fc{};
        if (kind == kWuffsStatus_Ok) {
            fc.index = fFrameConfig.index();
            fc.io_position = fFrameConfig.io_position();
            fc.bounds = to_rs_rect(fFrameConfig.bounds());
            fc.duration_flicks = fFrameConfig.duration();
            fc.disposal = fFrameConfig.disposal();
            fc.opaque_within_bounds = fFrameConfig.opaque_within_bounds();
            fc.overwrite_instead_of_blend = fFrameConfig.overwrite_instead_of_blend();
        }

        SkiaRsGifFrameCountStepResult step{};
        if (!skia_rs_gif_state_on_frame_count_step(fState.get(), i, kind, &fc, &step)) {
            return;
        }
        switch (step.action) {
            case kFrameStep_Continue:
                break;
            case kFrameStep_AppendedFrame:
                fFrames.emplace_back(step.appended_frame);
                break;
            case kFrameStep_Completed:
            case kFrameStep_Abort:
                return;
            default:
                return;
        }
    }

    skia_rs_gif_state_mark_frames_complete(fState.get());
}

bool SkWuffsCodec::onGetFrameInfo(int i, SkCodec::FrameInfo* frameInfo) const {
    SkiaRsGifFrameInfoFields fields{};
    if (!skia_rs_gif_state_get_frame_info(fState.get(), i, &fields)) {
        return false;
    }
    if (frameInfo) {
        frameInfo->fRequiredFrame = fields.required_frame;
        frameInfo->fDuration = fields.duration;
        frameInfo->fFullyReceived = fields.fully_received;
        frameInfo->fAlphaType = static_cast<SkAlphaType>(fields.alpha_type);
        frameInfo->fHasAlphaWithinBounds = fields.has_alpha_within_bounds;
        frameInfo->fDisposalMethod =
                static_cast<SkCodecAnimation::DisposalMethod>(fields.disposal_method);
        frameInfo->fBlend = static_cast<SkCodecAnimation::Blend>(fields.blend);
        frameInfo->fFrameRect = SkIRect::MakeLTRB(fields.frame_rect.left,
                                                  fields.frame_rect.top,
                                                  fields.frame_rect.right,
                                                  fields.frame_rect.bottom);
    }
    return true;
}

int SkWuffsCodec::onGetRepetitionCount() {
    return skia_rs_gif_loops_to_repetition_count(fDecoder->num_animation_loops());
}

SkCodec::IsAnimated SkWuffsCodec::onIsAnimated() {
    return static_cast<SkCodec::IsAnimated>(skia_rs_gif_state_is_animated(fState.get()));
}

SkCodec::Result SkWuffsCodec::seekFrame(int frameIndex) {
    if (skia_rs_gif_state_decoder_is_suspended(fState.get())) {
        SkCodec::Result res = this->resetDecoder();
        if (res != SkCodec::kSuccess) {
            return res;
        }
    }

    uint64_t pos = 0;
    const auto posRes = static_cast<SkCodec::Result>(
            skia_rs_gif_state_resolve_frame_io_position(fState.get(), frameIndex, &pos));
    if (posRes != SkCodec::kSuccess) {
        return posRes;
    }

    if (!seek_buffer(&fIOBuffer, fPrivStream.get(), pos)) {
        return SkCodec::kInternalError;
    }
    wuffs_base__status status =
            fDecoder->restart_frame(frameIndex, fIOBuffer.reader_io_position());
    if (status.repr != nullptr) {
        return SkCodec::kInternalError;
    }
    return SkCodec::kSuccess;
}

SkCodec::Result SkWuffsCodec::resetDecoder() {
    if (!fPrivStream->rewind()) {
        return SkCodec::kInternalError;
    }
    fIOBuffer.meta = wuffs_base__empty_io_buffer_meta();

    SkCodec::Result result =
            reset_and_decode_image_config(fDecoder.get(), nullptr, &fIOBuffer, fPrivStream.get());
    return static_cast<SkCodec::Result>(
            skia_rs_gif_state_on_reset_decoder_result(fState.get(), static_cast<int32_t>(result)));
}

const char* SkWuffsCodec::decodeFrameConfig() {
    while (true) {
        wuffs_base__status status = fDecoder->decode_frame_config(&fFrameConfig, &fIOBuffer);
        if ((status.repr == wuffs_base__suspension__short_read) &&
            fill_buffer(&fIOBuffer, fPrivStream.get())) {
            continue;
        }
        skia_rs_gif_state_on_decode_step_finished(
                fState.get(), status.is_complete(), fDecoder->num_decoded_frames());
        return status.repr;
    }
}

const char* SkWuffsCodec::decodeFrame() {
    const wuffs_base__pixel_blend blend =
            static_cast<wuffs_base__pixel_blend>(skia_rs_gif_state_pixel_blend(fState.get()));
    while (true) {
        wuffs_base__status status = fDecoder->decode_frame(
                &fPixelBuffer,
                &fIOBuffer,
                blend,
                wuffs_base__make_slice_u8(fWorkbufPtr.get(), fWorkbufLen),
                nullptr);
        if ((status.repr == wuffs_base__suspension__short_read) &&
            fill_buffer(&fIOBuffer, fPrivStream.get())) {
            continue;
        }
        skia_rs_gif_state_on_decode_step_finished(
                fState.get(), status.is_complete(), fDecoder->num_decoded_frames());
        return status.repr;
    }
}

void SkWuffsCodec::updateNumFullyReceivedFrames() {
    skia_rs_gif_state_update_num_fully_received_frames(fState.get(),
                                                       fDecoder->num_decoded_frames());
}

sk_sp<const SkData> SkWuffsCodec::getEncodedData() const {
    SkASSERT_RELEASE(fPrivStream);
    sk_sp<const SkData> data = fPrivStream->getData();
    if (data) {
        return data;
    }
    auto dStream = fPrivStream->duplicate();
    if (!dStream->hasLength()) {
        return nullptr;
    }
    return SkData::MakeFromStream(dStream.get(), dStream->getLength());
}

namespace SkGifDecoder {

bool IsGif(const void* buf, size_t bytesRead) {
    return skia_rs_gif_is_gif(static_cast<const uint8_t*>(buf), bytesRead);
}

std::unique_ptr<SkCodec> MakeFromStream(std::unique_ptr<SkStream> stream,
                                        SkCodec::SelectionPolicy selectionPolicy,
                                        SkCodec::Result* result) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }

    bool canSeek = stream->hasPosition() && stream->hasLength();
    if (skia_rs_gif_should_copy_stream_to_memory(static_cast<int32_t>(selectionPolicy), canSeek)) {
        auto data = SkStreamPriv::CopyStreamToData(stream.get());
        stream = std::make_unique<SkMemoryStream>(std::move(data));
        canSeek = true;
    }

    uint8_t buffer[SK_WUFFS_CODEC_BUFFER_SIZE];
    wuffs_base__io_buffer iobuf = wuffs_base__make_io_buffer(
            wuffs_base__make_slice_u8(buffer, SK_WUFFS_CODEC_BUFFER_SIZE),
            wuffs_base__empty_io_buffer_meta());
    wuffs_base__image_config imgcfg = wuffs_base__null_image_config();

    void* decoder_raw = sk_malloc_canfail(sizeof__wuffs_gif__decoder());
    if (!decoder_raw) {
        *result = SkCodec::kInternalError;
        return nullptr;
    }
    std::unique_ptr<wuffs_gif__decoder, decltype(&sk_free)> decoder(
            reinterpret_cast<wuffs_gif__decoder*>(decoder_raw), &sk_free);

    SkCodec::Result reset_result =
            reset_and_decode_image_config(decoder.get(), &imgcfg, &iobuf, stream.get());
    if (reset_result != SkCodec::kSuccess) {
        *result = reset_result;
        return nullptr;
    }

    SkiaRsGifEncodedInfo info{};
    const auto infoRes = static_cast<SkCodec::Result>(skia_rs_gif_classify_encoded_info(
            imgcfg.pixcfg.width(),
            imgcfg.pixcfg.height(),
            imgcfg.pixcfg.pixel_format().repr,
            imgcfg.first_frame_is_opaque(),
            &info));
    if (infoRes != SkCodec::kSuccess) {
        *result = infoRes;
        return nullptr;
    }

    size_t workbufLen = 0;
    const auto wbRes = static_cast<SkCodec::Result>(
            skia_rs_gif_validate_buffer_len(decoder->workbuf_len().max_incl, &workbufLen));
    if (wbRes != SkCodec::kSuccess) {
        *result = wbRes;
        return nullptr;
    }

    void* workbuf_ptr_raw = nullptr;
    if (workbufLen) {
        workbuf_ptr_raw = sk_malloc_canfail(workbufLen);
        if (!workbuf_ptr_raw) {
            *result = SkCodec::kInternalError;
            return nullptr;
        }
    }
    std::unique_ptr<uint8_t, decltype(&sk_free)> workbuf_ptr(
            reinterpret_cast<uint8_t*>(workbuf_ptr_raw), &sk_free);

    std::unique_ptr<SkiaRsGifCodecState, decltype(&skia_rs_gif_state_free)> state(
            skia_rs_gif_state_new(
                    info.width, info.height, canSeek, imgcfg.first_frame_io_position()),
            &skia_rs_gif_state_free);
    if (!state) {
        *result = SkCodec::kInternalError;
        return nullptr;
    }

    SkEncodedInfo encodedInfo = SkEncodedInfo::Make(
            info.width,
            info.height,
            static_cast<SkEncodedInfo::Color>(info.color),
            static_cast<SkEncodedInfo::Alpha>(info.alpha),
            info.bits_per_component);

    *result = SkCodec::kSuccess;
    return std::unique_ptr<SkCodec>(new SkWuffsCodec(std::move(encodedInfo),
                                                     std::move(stream),
                                                     std::move(state),
                                                     std::move(decoder),
                                                     std::move(workbuf_ptr),
                                                     workbufLen,
                                                     imgcfg,
                                                     iobuf));
}

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext ctx) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    auto policy = SkCodec::SelectionPolicy::kPreferStillImage;
    if (ctx) {
        policy = *static_cast<SkCodec::SelectionPolicy*>(ctx);
    }
    return MakeFromStream(std::move(stream), policy, outResult);
}

std::unique_ptr<SkCodec> Decode(sk_sp<const SkData> data,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext ctx) {
    if (!data) {
        if (outResult) {
            *outResult = SkCodec::kInvalidInput;
        }
        return nullptr;
    }
    return Decode(SkMemoryStream::Make(std::move(data)), outResult, ctx);
}

}  // namespace SkGifDecoder
