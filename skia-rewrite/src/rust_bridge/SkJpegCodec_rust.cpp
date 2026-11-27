/*
 * Safe Rust bridge for:
 *   src/codec/SkJpegCodec.cpp
 *   src/codec/SkJpegDecoderMgr.cpp
 *   src/codec/SkJpegMetadataDecoderImpl.cpp
 *   src/codec/SkJpegSourceMgr.cpp
 *   src/codec/SkJpegUtility.cpp
 *   src/codec/SkJpegXmp.cpp
 * (Phase 2 Module P2.5: skia-rs-codec-jpeg, GN target: jpeg_decode).
 *
 * Keeps thin third_party/libjpeg-turbo boundary calls (`jpeg_*` and `setjmp`)
 * while delegating signature sniffing, source-manager buffering/skip/subset state
 * machines, ICC/EXIF/XMP/ISO21496/JUMBF/MPF metadata extraction, color-space &
 * scaling/crop/storage/YUVA planning to `#![forbid(unsafe_code)]` safe Rust.
 */

#include "src/codec/SkJpegCodec.h"
#include "src/codec/SkJpegDecoderMgr.h"
#include "src/codec/SkJpegMetadataDecoderImpl.h"
#include "src/codec/SkJpegSourceMgr.h"
#include "src/codec/SkJpegUtility.h"

#include "include/codec/SkCodec.h"
#include "include/codec/SkEncodedOrigin.h"
#include "include/codec/SkJpegDecoder.h"
#include "include/core/SkAlphaType.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkStream.h"
#include "include/core/SkTypes.h"
#include "include/core/SkYUVAInfo.h"
#include "include/core/SkYUVAPixmaps.h"
#include "include/private/SkAlign.h"
#include "include/private/SkOnlyStackAllocatable.h"
#include "include/private/SkTemplates.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkJpegConstants.h"
#include "src/codec/SkJpegPriv.h"
#include "src/codec/SkParseEncodedOrigin.h"
#include "src/codec/SkSwizzler.h"

#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
#include "include/android/SkAndroidFrameworkUtils.h"
#endif

#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
#include "include/private/SkExif.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/SkXmp.h"
#include "src/codec/SkJpegMultiPicture.h"
#include "src/codec/SkJpegSegmentScan.h"
#include "src/codec/SkJpegXmp.h"
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS

#include <setjmp.h>
#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <optional>
#include <tuple>
#include <utility>
#include <vector>

using namespace skia_private;

class SkSampler;
struct SkGainmapInfo;

#if defined(__GNUC__) && !defined(__clang__)
#pragma GCC diagnostic ignored "-Wclobbered"
#endif

extern "C" {
#include "jpeglib.h"  // NO_G3_REWRITE
}

namespace {

// Discriminant & layout assertions shared across FFI with `skia-rs-codec-jpeg`.
static_assert(DCTSIZE == 8);
static_assert(JCS_UNKNOWN == 0);
static_assert(JCS_GRAYSCALE == 1);
static_assert(JCS_RGB == 2);
static_assert(JCS_YCbCr == 3);
static_assert(JCS_CMYK == 4);
static_assert(JCS_YCCK == 5);
static_assert(JCS_EXT_RGB == 6);
static_assert(JCS_EXT_RGBX == 7);
static_assert(JCS_EXT_BGR == 8);
static_assert(JCS_EXT_BGRX == 9);
static_assert(JCS_EXT_RGBA == 12);
static_assert(JCS_EXT_BGRA == 13);
static_assert(JCS_RGB565 == 16);

static_assert(static_cast<int32_t>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int32_t>(kRGB_565_SkColorType) == 2);
static_assert(static_cast<int32_t>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int32_t>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int32_t>(kBGR_101010x_XR_SkColorType) == 11);
static_assert(static_cast<int32_t>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int32_t>(kRGBA_F16_SkColorType) == 16);

static_assert(static_cast<int32_t>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int32_t>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int32_t>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int32_t>(kUnpremul_SkAlphaType) == 3);

static_assert(static_cast<int32_t>(SkEncodedInfo::kGray_Color) == 0);
static_assert(static_cast<int32_t>(SkEncodedInfo::kRGB_Color) == 5);
static_assert(static_cast<int32_t>(SkEncodedInfo::kYUV_Color) == 10);
static_assert(static_cast<int32_t>(SkEncodedInfo::kInvertedCMYK_Color) == 12);
static_assert(static_cast<int32_t>(SkEncodedInfo::kYCCK_Color) == 13);

static_assert(static_cast<int32_t>(SkCodecs::ColorProfile::DataSpace::kRGB) == 0);
static_assert(static_cast<int32_t>(SkCodecs::ColorProfile::DataSpace::kCMYK) == 1);
static_assert(static_cast<int32_t>(SkCodecs::ColorProfile::DataSpace::kGray) == 2);
static_assert(static_cast<int32_t>(SkCodecs::ColorProfile::DataSpace::kOther) == 3);

static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::kUnknown) == 0);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k444) == 1);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k422) == 2);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k420) == 3);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k440) == 4);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k411) == 5);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k410) == 6);

static_assert(static_cast<int32_t>(SkCodec::kSuccess) == 0);
static_assert(static_cast<int32_t>(SkCodec::kIncompleteInput) == 1);
static_assert(static_cast<int32_t>(SkCodec::kInvalidInput) == 6);
static_assert(static_cast<int32_t>(SkCodec::kInternalError) == 8);
static_assert(static_cast<int32_t>(SkCodec::kUnimplemented) == 9);
static_assert(static_cast<int32_t>(SkCodec::kOutOfMemory) == 10);

struct SkiaRsJpegBytes {
    uint8_t* ptr;
    size_t len;
    size_t cap;
};

struct SkiaRsJpegSlice {
    const uint8_t* ptr;
    size_t len;
};

struct SkiaRsJpegMarkerView {
    uint8_t marker;
    const uint8_t* ptr;
    size_t len;
};

struct SkiaRsJpegMetadataResult {
    int32_t kind;
    size_t marker_index;
    size_t offset;
    size_t len;
    SkiaRsJpegBytes owned;
};

struct SkiaRsJpegSegment {
    size_t offset;
    uint8_t marker;
    uint16_t parameter_length;
};

struct SkiaRsJpegMpImage {
    uint32_t size;
    uint32_t data_offset;
};

#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
static_assert(sizeof(SkJpegSegment) == sizeof(SkiaRsJpegSegment));
static_assert(offsetof(SkJpegSegment, offset) == offsetof(SkiaRsJpegSegment, offset));
static_assert(offsetof(SkJpegSegment, marker) == offsetof(SkiaRsJpegSegment, marker));
static_assert(offsetof(SkJpegSegment, parameterLength) ==
              offsetof(SkiaRsJpegSegment, parameter_length));
static_assert(sizeof(SkJpegMultiPictureParameters::Image) == sizeof(SkiaRsJpegMpImage));
#endif

struct SkiaRsJpegMemorySkipPlan {
    bool ok;
    size_t advance;
    size_t new_bytes_in_buffer;
};

struct SkiaRsJpegBufferedSkipPlan {
    size_t advance_in_buffer;
    size_t new_bytes_in_buffer;
    size_t stream_bytes_to_skip;
    bool reset_buffer_ptr;
};

struct SkiaRsJpegUnseekableSubsetPlan {
    bool ok;
    size_t stream_bytes_to_skip;
    size_t copy_from_buffer_offset;
    size_t copy_from_buffer_len;
    size_t stream_bytes_to_read;
    size_t next_last_read_offset;
    size_t next_last_read_size;
};

struct SkiaRsJpegEncodedColorResult {
    bool valid;
    int32_t encoded_color;
    bool android_rgb_safety_log;
};

struct SkiaRsJpegConversionPlan {
    bool supported;
    bool warn_non_opaque;
    int32_t out_color_space;
    bool set_dither_none;
};

struct SkiaRsJpegStoragePlan {
    size_t swizzle_bytes;
    size_t xform_bytes;
    size_t total_bytes;
};

struct SkiaRsJpegSubsetCropPlan {
    int32_t swizzler_subset_x;
    int32_t swizzler_subset_y;
    int32_t swizzler_subset_w;
    int32_t swizzler_subset_h;
    bool needs_swizzler_for_subset;
};

struct SkiaRsJpegYuvaQueryPlan {
    bool supported;
    int32_t subsampling;
    size_t row_bytes[4];
};

struct SkiaRsJpegYuvaPlanesPlan {
    int32_t num_y_rows_per_block;
    int32_t num_iters;
    uint32_t remaining_rows;
    int32_t remaining_uv_rows;
};

extern "C" {
void skia_rs_jpeg_free_bytes(SkiaRsJpegBytes buf);

// SourceMgr & Utility
int32_t skia_rs_jpeg_select_source_mgr(bool decode_gainmaps,
                                       bool has_position,
                                       bool has_length,
                                       bool has_memory_base);
SkiaRsJpegMemorySkipPlan skia_rs_jpeg_plan_memory_skip(size_t bytes_to_skip,
                                                       size_t bytes_in_buffer);
SkiaRsJpegBufferedSkipPlan skia_rs_jpeg_plan_buffered_skip(size_t bytes_to_skip,
                                                           size_t bytes_in_buffer);
bool skia_rs_jpeg_validate_memory_subset(size_t stream_length, size_t offset, size_t size);
bool skia_rs_jpeg_verify_memory_segment_params(const uint8_t* data,
                                               size_t data_len,
                                               SkiaRsJpegSegment segment,
                                               size_t* out_offset,
                                               size_t* out_len);
SkiaRsJpegUnseekableSubsetPlan skia_rs_jpeg_plan_unseekable_subset(size_t last_read_offset,
                                                                   size_t last_read_size,
                                                                   size_t req_offset,
                                                                   size_t req_size);

// XMP & Metadata
bool skia_rs_jpeg_find_xmp_standard(const SkiaRsJpegSlice* slices,
                                    size_t count,
                                    size_t* out_index,
                                    size_t* out_offset);
bool skia_rs_jpeg_read_xmp_extended(const SkiaRsJpegSlice* slices,
                                    size_t count,
                                    const uint8_t* guid,
                                    size_t guid_len,
                                    SkiaRsJpegBytes* out_bytes);
SkiaRsJpegMetadataResult skia_rs_jpeg_read_exif(const SkiaRsJpegMarkerView* markers,
                                                size_t count,
                                                bool copy_data);
SkiaRsJpegMetadataResult skia_rs_jpeg_read_icc(const SkiaRsJpegMarkerView* markers,
                                               size_t count,
                                               bool copy_data);
SkiaRsJpegMetadataResult skia_rs_jpeg_read_iso_gainmap(const SkiaRsJpegMarkerView* markers,
                                                       size_t count,
                                                       bool copy_data);
SkiaRsJpegMetadataResult skia_rs_jpeg_read_jumbf(const SkiaRsJpegMarkerView* markers,
                                                 size_t count,
                                                 bool copy_data);
void skia_rs_jpeg_scan_header_app1_app2(const uint8_t* data,
                                        size_t len,
                                        void (*emit_cb)(void*, SkiaRsJpegSegment),
                                        void* ctx);
bool skia_rs_jpeg_might_have_gainmap(const SkiaRsJpegMarkerView* markers, size_t count);
bool skia_rs_jpeg_find_mp_in_markers(const SkiaRsJpegMarkerView* markers,
                                     size_t count,
                                     void (*emit_cb)(void*, SkiaRsJpegMpImage),
                                     void* ctx,
                                     size_t* out_skipped_count);
bool skia_rs_jpeg_find_mp_segment_by_skip(const SkiaRsJpegSegment* segments,
                                          size_t count,
                                          size_t skipped_count,
                                          SkiaRsJpegSegment* out_segment);
size_t skia_rs_jpeg_compute_container_gainmap_offset(const SkiaRsJpegSegment* segments,
                                                     size_t count,
                                                     size_t relative_offset);
size_t skia_rs_jpeg_mpf_get_image_absolute_offset(uint32_t data_offset, size_t mp_segment_offset);

// Codec & DecoderMgr
bool skia_rs_jpeg_is_jpeg(const uint8_t* buf, size_t len);
bool skia_rs_jpeg_should_abort_progress(int32_t input_scan_number);
SkiaRsJpegEncodedColorResult skia_rs_jpeg_encoded_color(int32_t jpeg_color_space);
bool skia_rs_jpeg_is_icc_compatible(int32_t jpeg_color_space, int32_t data_space);
size_t skia_rs_jpeg_get_row_bytes(int32_t out_color_space,
                                  uint32_t output_width,
                                  int32_t out_color_components);
uint32_t skia_rs_jpeg_scale_numerator(float desired_scale);
SkiaRsJpegConversionPlan skia_rs_jpeg_plan_conversion(int32_t encoded_jcs,
                                                      int32_t dst_ct,
                                                      int32_t dst_at,
                                                      bool needs_color_xform);
bool skia_rs_jpeg_needs_cmyk_swizzler(int32_t out_jcs,
                                      bool has_profile,
                                      int32_t profile_data_space,
                                      bool has_color_xform);
int32_t skia_rs_jpeg_src_bpp_for_out_cs(int32_t out_jcs);
SkiaRsJpegStoragePlan skia_rs_jpeg_plan_storage(int32_t dst_width,
                                                size_t dst_bpp,
                                                bool has_swizzler,
                                                int32_t swizzler_width,
                                                size_t dinfo_row_bytes,
                                                bool has_color_xform);
SkiaRsJpegSubsetCropPlan skia_rs_jpeg_plan_subset_after_crop(int32_t req_x,
                                                             int32_t req_w,
                                                             int32_t req_h,
                                                             uint32_t cropped_start_x,
                                                             uint32_t cropped_width);
size_t skia_rs_jpeg_estimate_libjpeg_memory(bool is_progressive, int32_t width, int32_t height);
SkiaRsJpegYuvaQueryPlan skia_rs_jpeg_query_yuva_info(int32_t jcs,
                                                     const int32_t* h_samp,
                                                     const int32_t* v_samp,
                                                     const uint32_t* width_in_blocks,
                                                     bool check_supported,
                                                     bool supports_yuv_unorm8);
SkiaRsJpegYuvaPlanesPlan skia_rs_jpeg_plan_yuva_planes(uint32_t output_height,
                                                       int32_t y_v_samp,
                                                       uint32_t u_downsampled_height);
}

std::vector<SkiaRsJpegMarkerView> build_marker_views(const SkJpegMarkerList& markerList) {
    std::vector<SkiaRsJpegMarkerView> views;
    views.reserve(markerList.size());
    for (const auto& m : markerList) {
        views.push_back(SkiaRsJpegMarkerView{
                m.fMarker,
                m.fData ? m.fData->bytes() : nullptr,
                m.fData ? m.fData->size() : 0,
        });
    }
    return views;
}

sk_sp<const SkData> metadata_result_to_skdata(const SkJpegMarkerList& markerList,
                                              SkiaRsJpegMetadataResult res) {
    if (res.kind == 1) {
        const auto& src = markerList[res.marker_index].fData;
        return SkData::MakeWithoutCopy(src->bytes() + res.offset, res.len);
    }
    if (res.kind == 2) {
        sk_sp<SkData> data = res.owned.len > 0 ? SkData::MakeWithCopy(res.owned.ptr, res.owned.len)
                                               : SkData::MakeEmpty();
        skia_rs_jpeg_free_bytes(res.owned);
        return data;
    }
    return nullptr;
}

}  // namespace

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegUtility (skjpeg_err_exit & legacy skjpeg_source_mgr)

void skjpeg_err_exit(j_common_ptr dinfo) {
    skjpeg_error_mgr* error = static_cast<skjpeg_error_mgr*>(dinfo->err);
    (*error->output_message)(dinfo);
    if (error->fStack[0] == nullptr) {
        SK_ABORT("JPEG error with no jmp_buf set.");
    }
    longjmp(*error->fStack[0], 1);
}

static void sk_init_buffered_source(j_decompress_ptr dinfo) {
    skjpeg_source_mgr* src = (skjpeg_source_mgr*)dinfo->src;
    src->next_input_byte = (const JOCTET*)src->fBuffer;
    src->bytes_in_buffer = 0;
}

static boolean sk_fill_buffered_input_buffer(j_decompress_ptr dinfo) {
    skjpeg_source_mgr* src = (skjpeg_source_mgr*)dinfo->src;
    size_t bytes = src->fStream->read(src->fBuffer, skjpeg_source_mgr::kBufferSize);
    if (bytes == 0) {
        src->next_input_byte = nullptr;
        src->bytes_in_buffer = 0;
        return FALSE;
    }
    src->next_input_byte = (const JOCTET*)src->fBuffer;
    src->bytes_in_buffer = bytes;
    return TRUE;
}

static void sk_skip_buffered_input_data(j_decompress_ptr dinfo, long numBytes) {
    skjpeg_source_mgr* src = (skjpeg_source_mgr*)dinfo->src;
    SkiaRsJpegBufferedSkipPlan plan =
            skia_rs_jpeg_plan_buffered_skip(static_cast<size_t>(numBytes), src->bytes_in_buffer);
    if (plan.reset_buffer_ptr) {
        if (plan.stream_bytes_to_skip != src->fStream->skip(plan.stream_bytes_to_skip)) {
            SkCodecPrintf("Failure to skip.\n");
            dinfo->err->error_exit((j_common_ptr)dinfo);
            return;
        }
        src->next_input_byte = (const JOCTET*)src->fBuffer;
        src->bytes_in_buffer = 0;
    } else {
        src->next_input_byte += plan.advance_in_buffer;
        src->bytes_in_buffer = plan.new_bytes_in_buffer;
    }
}

static void sk_term_source(j_decompress_ptr) {}

static void sk_init_mem_source(j_decompress_ptr) {}

static void sk_skip_mem_input_data(j_decompress_ptr cinfo, long num_bytes) {
    jpeg_source_mgr* src = cinfo->src;
    SkiaRsJpegMemorySkipPlan plan =
            skia_rs_jpeg_plan_memory_skip(static_cast<size_t>(num_bytes), src->bytes_in_buffer);
    if (!plan.ok) {
        src->next_input_byte = nullptr;
        src->bytes_in_buffer = 0;
    } else {
        src->next_input_byte += plan.advance;
        src->bytes_in_buffer = plan.new_bytes_in_buffer;
    }
}

static boolean sk_fill_mem_input_buffer(j_decompress_ptr) { return FALSE; }

skjpeg_source_mgr::skjpeg_source_mgr(SkStream* stream) : fStream(stream) {
    if (stream->hasLength() && stream->getMemoryBase()) {
        init_source = sk_init_mem_source;
        fill_input_buffer = sk_fill_mem_input_buffer;
        skip_input_data = sk_skip_mem_input_data;
        resync_to_restart = jpeg_resync_to_restart;
        term_source = sk_term_source;
        bytes_in_buffer = static_cast<size_t>(stream->getLength());
        next_input_byte = static_cast<const JOCTET*>(stream->getMemoryBase());
    } else {
        init_source = sk_init_buffered_source;
        fill_input_buffer = sk_fill_buffered_input_buffer;
        skip_input_data = sk_skip_buffered_input_data;
        resync_to_restart = jpeg_resync_to_restart;
        term_source = sk_term_source;
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegSourceMgr

class ScopedSkStreamRestorer {
    SK_ONLY_STACK_ALLOCATABLE;

public:
    ScopedSkStreamRestorer(SkStream* stream) : fStream(stream), fPosition(stream->getPosition()) {
        if (!fStream->rewind()) {
            SkCodecPrintf("Failed to rewind decoder stream.\n");
        }
    }
    ~ScopedSkStreamRestorer() {
        if (!fStream->seek(fPosition)) {
            SkCodecPrintf("Failed to restore decoder stream.\n");
        }
    }

private:
    SkStream* const fStream;
    const size_t fPosition;
};

class SkJpegMemorySourceMgr : public SkJpegSourceMgr {
public:
    SkJpegMemorySourceMgr(SkStream* stream) : SkJpegSourceMgr(stream) {}
    ~SkJpegMemorySourceMgr() override {}

    void initSource(const uint8_t*& nextInputByte, size_t& bytesInBuffer) override {
        nextInputByte = reinterpret_cast<const uint8_t*>(fStream->getMemoryBase());
        bytesInBuffer = static_cast<size_t>(fStream->getLength());
    }
    bool fillInputBuffer(const uint8_t*&, size_t&) override {
        SkCodecPrintf("Asked to re-fill a memory-mapped stream.\n");
        return false;
    }
    bool skipInputBytes(size_t bytesToSkip,
                        const uint8_t*& nextInputByte,
                        size_t& bytesInBuffer) override {
        SkiaRsJpegMemorySkipPlan plan = skia_rs_jpeg_plan_memory_skip(bytesToSkip, bytesInBuffer);
        if (!plan.ok) {
            SkCodecPrintf("Asked to read past end of a memory-mapped stream.\n");
            return false;
        }
        nextInputByte += plan.advance;
        bytesInBuffer = plan.new_bytes_in_buffer;
        return true;
    }
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    const std::vector<SkJpegSegment>& getAllSegments() override {
        if (fScanner) {
            return fScanner->getSegments();
        }
        fScanner = std::make_unique<SkJpegSegmentScanner>(kJpegMarkerEndOfImage);
        fScanner->onBytes(fStream->getMemoryBase(), fStream->getLength());
        return fScanner->getSegments();
    }
    sk_sp<SkData> getSubsetData(size_t offset, size_t size, bool* wasCopied) override {
        if (!skia_rs_jpeg_validate_memory_subset(fStream->getLength(), offset, size)) {
            return nullptr;
        }
        if (wasCopied) {
            *wasCopied = false;
        }
        return SkData::MakeWithoutCopy(
                reinterpret_cast<const uint8_t*>(fStream->getMemoryBase()) + offset, size);
    }
    sk_sp<SkData> getSegmentParameters(const SkJpegSegment& segment) override {
        const auto* base = reinterpret_cast<const uint8_t*>(fStream->getMemoryBase());
        size_t paramOffset = 0;
        size_t paramLen = 0;
        SkiaRsJpegSegment rsSeg{segment.offset, segment.marker, segment.parameterLength};
        if (!skia_rs_jpeg_verify_memory_segment_params(
                    base, fStream->getLength(), rsSeg, &paramOffset, &paramLen)) {
            return nullptr;
        }
        return SkData::MakeWithoutCopy(base + paramOffset, paramLen);
    }
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS
};

class SkJpegBufferedSourceMgr : public SkJpegSourceMgr {
public:
    SkJpegBufferedSourceMgr(SkStream* stream, size_t bufferSize) : SkJpegSourceMgr(stream) {
        fBuffer = SkData::MakeUninitialized(bufferSize);
    }
    ~SkJpegBufferedSourceMgr() override {}

    void initSource(const uint8_t*& nextInputByte, size_t& bytesInBuffer) override {
        nextInputByte = fBuffer->bytes();
        bytesInBuffer = 0;
    }
    bool fillInputBuffer(const uint8_t*& nextInputByte, size_t& bytesInBuffer) override {
        size_t bytesRead = fStream->read(fBuffer->writable_data(), fBuffer->size());
        if (bytesRead == 0) {
            SkCodecPrintf("Hit end of file reading a buffered stream.\n");
            return false;
        }
        nextInputByte = fBuffer->bytes();
        bytesInBuffer = bytesRead;
        return true;
    }
    bool skipInputBytes(size_t bytesToSkip,
                        const uint8_t*& nextInputByte,
                        size_t& bytesInBuffer) override {
        SkiaRsJpegBufferedSkipPlan plan =
                skia_rs_jpeg_plan_buffered_skip(bytesToSkip, bytesInBuffer);
        if (!plan.reset_buffer_ptr) {
            nextInputByte += plan.advance_in_buffer;
            bytesInBuffer = plan.new_bytes_in_buffer;
            return true;
        }
        if (fStream->skip(plan.stream_bytes_to_skip) != plan.stream_bytes_to_skip) {
            SkCodecPrintf("Failed to skip through buffered stream.\n");
            return false;
        }
        bytesInBuffer = 0;
        nextInputByte = fBuffer->bytes();
        return true;
    }
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    const std::vector<SkJpegSegment>& getAllSegments() override {
        if (fScanner) {
            return fScanner->getSegments();
        }
        ScopedSkStreamRestorer streamRestorer(fStream);
        fScanner = std::make_unique<SkJpegSegmentScanner>(kJpegMarkerEndOfImage);
        while (!fScanner->isDone() && !fScanner->hadError()) {
            constexpr size_t kBufferSize = 1024;
            uint8_t buffer[kBufferSize];
            size_t bytesRead = fStream->read(buffer, kBufferSize);
            if (bytesRead == 0) {
                SkCodecPrintf("Unexpected EOF.\n");
                break;
            }
            fScanner->onBytes(buffer, bytesRead);
        }
        return fScanner->getSegments();
    }
    sk_sp<SkData> getSubsetData(size_t offset, size_t size, bool* wasCopied) override {
        ScopedSkStreamRestorer streamRestorer(fStream);
        if (!fStream->seek(offset)) {
            SkCodecPrintf("Failed to seek to subset stream position.\n");
            return nullptr;
        }
        sk_sp<SkData> data = SkData::MakeUninitialized(size);
        if (fStream->read(data->writable_data(), size) != size) {
            SkCodecPrintf("Failed to read subset stream data.\n");
            return nullptr;
        }
        if (wasCopied) {
            *wasCopied = true;
        }
        return data;
    }
    sk_sp<SkData> getSegmentParameters(const SkJpegSegment& segment) override {
        if (segment.parameterLength <= kJpegSegmentParameterLengthSize) {
            return nullptr;
        }
        ScopedSkStreamRestorer streamRestorer(fStream);
        if (!fStream->seek(segment.offset)) {
            SkCodecPrintf("Failed to seek to segment\n");
            return nullptr;
        }
        uint8_t markerCode[kJpegMarkerCodeSize] = {0};
        if (fStream->read(markerCode, kJpegMarkerCodeSize) != kJpegMarkerCodeSize) {
            SkCodecPrintf("Failed to read segment marker code\n");
            return nullptr;
        }
        SkASSERT(markerCode[0] == 0xFF);
        SkASSERT(markerCode[1] == segment.marker);

        uint8_t parameterLength[kJpegSegmentParameterLengthSize] = {0};
        if (fStream->read(parameterLength, kJpegSegmentParameterLengthSize) !=
            kJpegSegmentParameterLengthSize) {
            SkCodecPrintf("Failed to read parameter length\n");
            return nullptr;
        }
        SkASSERT(256 * parameterLength[0] + parameterLength[1] == segment.parameterLength);

        size_t sizeToRead = segment.parameterLength - kJpegSegmentParameterLengthSize;
        auto result = SkData::MakeUninitialized(sizeToRead);
        if (fStream->read(result->writable_data(), sizeToRead) != sizeToRead) {
            return nullptr;
        }
        return result;
    }
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS

private:
    sk_sp<SkData> fBuffer;
};

#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
class SkJpegUnseekableSourceMgr : public SkJpegSourceMgr {
public:
    SkJpegUnseekableSourceMgr(SkStream* stream, size_t bufferSize) : SkJpegSourceMgr(stream) {
        fBuffer = SkData::MakeUninitialized(bufferSize);
        fScanner = std::make_unique<SkJpegSegmentScanner>(kJpegMarkerEndOfImage);
    }
    ~SkJpegUnseekableSourceMgr() override {}

    void initSource(const uint8_t*& nextInputByte, size_t& bytesInBuffer) override {
        nextInputByte = fBuffer->bytes();
        bytesInBuffer = 0;
    }
    bool fillInputBuffer(const uint8_t*& nextInputByte, size_t& bytesInBuffer) override {
        if (!readToBufferAndScan(fBuffer->size())) {
            SkCodecPrintf("Failure filling unseekable input buffer.\n");
            return false;
        }
        nextInputByte = fBuffer->bytes();
        bytesInBuffer = fLastReadSize;
        return true;
    }
    bool skipInputBytes(size_t bytesToSkip,
                        const uint8_t*& nextInputByte,
                        size_t& bytesInBuffer) override {
        SkiaRsJpegBufferedSkipPlan plan =
                skia_rs_jpeg_plan_buffered_skip(bytesToSkip, bytesInBuffer);
        if (!plan.reset_buffer_ptr) {
            nextInputByte += plan.advance_in_buffer;
            bytesInBuffer = plan.new_bytes_in_buffer;
            return true;
        }
        size_t remaining = plan.stream_bytes_to_skip;
        while (remaining > 0) {
            if (!readToBufferAndScan(std::min(remaining, fBuffer->size()))) {
                SkCodecPrintf("Failure filling unseekable input buffer.\n");
                return false;
            }
            remaining -= fLastReadSize;
        }
        bytesInBuffer = 0;
        nextInputByte = fBuffer->bytes();
        return true;
    }
    const std::vector<SkJpegSegment>& getAllSegments() override {
        while (!fScanner->isDone() && !fScanner->hadError()) {
            if (!readToBufferAndScan(fBuffer->size())) {
                SkCodecPrintf("Failure finishing unseekable input buffer.\n");
                break;
            }
        }
        return fScanner->getSegments();
    }
    sk_sp<SkData> getSubsetData(size_t offset, size_t size, bool* wasCopied) override {
        if (!fScanner->isDone()) {
            SkCodecPrintf("getSubsetData is prematurely terminating scan.\n");
        }
        SkiaRsJpegUnseekableSubsetPlan plan =
                skia_rs_jpeg_plan_unseekable_subset(fLastReadOffset, fLastReadSize, offset, size);
        if (!plan.ok) {
            SkCodecPrintf("Requested that is gone.\n");
            return nullptr;
        }

        sk_sp<SkData> subsetData = SkData::MakeUninitialized(size);
        uint8_t* subsetDataCurrent = reinterpret_cast<uint8_t*>(subsetData->writable_data());

        if (plan.copy_from_buffer_len > 0) {
            memcpy(subsetDataCurrent,
                   fBuffer->bytes() + plan.copy_from_buffer_offset,
                   plan.copy_from_buffer_len);
            subsetDataCurrent += plan.copy_from_buffer_len;
        }

        fLastReadOffset = plan.next_last_read_offset;
        fLastReadSize = plan.next_last_read_size;

        size_t bytesToSkip = plan.stream_bytes_to_skip;
        while (bytesToSkip > 0) {
            size_t bytesSkipped = fStream->skip(bytesToSkip);
            if (bytesSkipped == 0) {
                SkCodecPrintf("Failed to skip bytes before subset.\n");
                return nullptr;
            }
            bytesToSkip -= bytesSkipped;
            fLastReadOffset += bytesSkipped;
        }

        size_t bytesRemaining = plan.stream_bytes_to_read;
        while (bytesRemaining > 0) {
            size_t bytesRead = fStream->read(subsetDataCurrent, bytesRemaining);
            if (bytesRead == 0) {
                SkCodecPrintf("Failed to read subset stream data.\n");
                return nullptr;
            }
            bytesRemaining -= bytesRead;
            subsetDataCurrent += bytesRead;
            fLastReadOffset += bytesRead;
        }

        if (wasCopied) {
            *wasCopied = true;
        }
        return subsetData;
    }
    sk_sp<SkData> getSegmentParameters(const SkJpegSegment&) override { return nullptr; }

private:
    bool readToBufferAndScan(size_t bytesToRead) {
        SkASSERT(bytesToRead <= fBuffer->size());
        fLastReadOffset += fLastReadSize;
        fLastReadSize = fStream->read(fBuffer->writable_data(), bytesToRead);
        if (fLastReadSize == 0) {
            SkCodecPrintf("Hit end of file reading an unseekable stream.\n");
            return false;
        }
        fScanner->onBytes(fBuffer->bytes(), fLastReadSize);
        return true;
    }

    sk_sp<SkData> fBuffer;
    size_t fLastReadSize = 0;
    size_t fLastReadOffset = 0;
};
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS

std::unique_ptr<SkJpegSourceMgr> SkJpegSourceMgr::Make(SkStream* stream, size_t bufferSize) {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    constexpr bool kDecodeGainmaps = true;
#else
    constexpr bool kDecodeGainmaps = false;
#endif
    const int32_t kind = skia_rs_jpeg_select_source_mgr(
            kDecodeGainmaps,
            stream->hasPosition(),
            stream->hasLength(),
            stream->getMemoryBase() != nullptr);
    switch (kind) {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
        case 0:
            return std::make_unique<SkJpegUnseekableSourceMgr>(stream, bufferSize);
#endif
        case 1:
            return std::make_unique<SkJpegMemorySourceMgr>(stream);
        default:
            return std::make_unique<SkJpegBufferedSourceMgr>(stream, bufferSize);
    }
}

SkJpegSourceMgr::SkJpegSourceMgr(SkStream* stream) : fStream(stream) {}
SkJpegSourceMgr::~SkJpegSourceMgr() = default;

////////////////////////////////////////////////////////////////////////////////////////////////////
// JpegDecoderMgr

static void print_message(const j_common_ptr info, const char caller[]) {
    [[maybe_unused]] char buffer[JMSG_LENGTH_MAX];
    SkCodecPrintf("libjpeg error %d <%s> from %s\n",
                  info->err->msg_code,
                  (info->err->format_message(info, buffer), buffer),
                  caller);
}

static void output_message(j_common_ptr info) { print_message(info, "output_message"); }

static void progress_monitor(j_common_ptr info) {
    int scan = ((j_decompress_ptr)info)->input_scan_number;
    if (skia_rs_jpeg_should_abort_progress(scan)) {
        skjpeg_err_exit(info);
    }
}

bool JpegDecoderMgr::returnFalse(const char caller[]) {
    print_message((j_common_ptr)&fDInfo, caller);
    return false;
}

SkCodec::Result JpegDecoderMgr::returnFailure(const char caller[], SkCodec::Result result) {
    print_message((j_common_ptr)&fDInfo, caller);
    return result;
}

bool JpegDecoderMgr::getEncodedColor(SkEncodedInfo::Color* outColor) {
    SkiaRsJpegEncodedColorResult res =
            skia_rs_jpeg_encoded_color(static_cast<int32_t>(fDInfo.jpeg_color_space));
    if (!res.valid) {
        return false;
    }
#ifdef SK_BUILD_FOR_ANDROID_FRAMEWORK
    if (res.android_rgb_safety_log) {
        SkAndroidFrameworkUtils::SafetyNetLog("118372692");
    }
#endif
    *outColor = static_cast<SkEncodedInfo::Color>(res.encoded_color);
    return true;
}

SkJpegSourceMgr* JpegDecoderMgr::getSourceMgr() { return fSrcMgr.fSourceMgr.get(); }

JpegDecoderMgr::JpegDecoderMgr(SkStream* stream)
        : fSrcMgr(SkJpegSourceMgr::Make(stream)), fInit(false) {
    fDInfo.err = jpeg_std_error(&fErrorMgr);
    fErrorMgr.error_exit = skjpeg_err_exit;
}

void JpegDecoderMgr::init() {
    jpeg_create_decompress(&fDInfo);
    fInit = true;
    fDInfo.src = &fSrcMgr;
    fDInfo.err->output_message = &output_message;
    fDInfo.progress = &fProgressMgr;
    fProgressMgr.progress_monitor = &progress_monitor;
}

JpegDecoderMgr::~JpegDecoderMgr() {
    if (fInit) {
        jpeg_destroy_decompress(&fDInfo);
    }
}

void JpegDecoderMgr::SourceMgr::InitSource(j_decompress_ptr dinfo) {
    JpegDecoderMgr::SourceMgr* src = (JpegDecoderMgr::SourceMgr*)dinfo->src;
    src->fSourceMgr->initSource(src->next_input_byte, src->bytes_in_buffer);
}

void JpegDecoderMgr::SourceMgr::SkipInputData(j_decompress_ptr dinfo, long num_bytes_long) {
    JpegDecoderMgr::SourceMgr* src = (JpegDecoderMgr::SourceMgr*)dinfo->src;
    size_t num_bytes = static_cast<size_t>(num_bytes_long);
    if (!src->fSourceMgr->skipInputBytes(num_bytes, src->next_input_byte, src->bytes_in_buffer)) {
        SkCodecPrintf("Failure to skip.\n");
        src->next_input_byte = nullptr;
        src->bytes_in_buffer = 0;
        dinfo->err->error_exit((j_common_ptr)dinfo);
    }
}

boolean JpegDecoderMgr::SourceMgr::FillInputBuffer(j_decompress_ptr dinfo) {
    JpegDecoderMgr::SourceMgr* src = (JpegDecoderMgr::SourceMgr*)dinfo->src;
    if (!src->fSourceMgr->fillInputBuffer(src->next_input_byte, src->bytes_in_buffer)) {
        SkCodecPrintf("Failure to fill input buffer.\n");
        src->next_input_byte = nullptr;
        src->bytes_in_buffer = 0;
        return false;
    }
    return true;
}

void JpegDecoderMgr::SourceMgr::TermSource(j_decompress_ptr) {}

JpegDecoderMgr::SourceMgr::SourceMgr(std::unique_ptr<SkJpegSourceMgr> sourceMgr)
        : fSourceMgr(std::move(sourceMgr)) {
    init_source = JpegDecoderMgr::SourceMgr::InitSource;
    fill_input_buffer = JpegDecoderMgr::SourceMgr::FillInputBuffer;
    skip_input_data = JpegDecoderMgr::SourceMgr::SkipInputData;
    resync_to_restart = jpeg_resync_to_restart;
    term_source = JpegDecoderMgr::SourceMgr::TermSource;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegXmp (guarded by SK_CODEC_DECODES_JPEG_GAINMAPS)

#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
std::unique_ptr<SkXmp> SkJpegMakeXmp(const std::vector<sk_sp<const SkData>>& decoderApp1Params) {
    std::vector<SkiaRsJpegSlice> slices;
    slices.reserve(decoderApp1Params.size());
    for (const auto& p : decoderApp1Params) {
        slices.push_back(SkiaRsJpegSlice{p ? p->bytes() : nullptr, p ? p->size() : 0});
    }

    size_t stdIdx = 0;
    size_t stdOff = 0;
    if (!skia_rs_jpeg_find_xmp_standard(slices.data(), slices.size(), &stdIdx, &stdOff)) {
        return nullptr;
    }
    const auto& stdSeg = decoderApp1Params[stdIdx];
    auto xmpStandard = SkData::MakeWithoutCopy(stdSeg->bytes() + stdOff, stdSeg->size() - stdOff);

    std::unique_ptr<SkXmp> xmp = SkXmp::Make(xmpStandard);
    if (!xmp) {
        return nullptr;
    }

    const char* extendedGuid = xmp->getExtendedXmpGuid();
    if (!extendedGuid) {
        return xmp;
    }

    SkiaRsJpegBytes extBytes{};
    if (!skia_rs_jpeg_read_xmp_extended(slices.data(),
                                        slices.size(),
                                        reinterpret_cast<const uint8_t*>(extendedGuid),
                                        strlen(extendedGuid),
                                        &extBytes)) {
        SkCodecPrintf("Extended XMP was indicated but failed to read or validate.\n");
        return xmp;
    }
    auto xmpExtended = SkData::MakeWithCopy(extBytes.ptr, extBytes.len);
    skia_rs_jpeg_free_bytes(extBytes);
    return SkXmp::Make(xmpStandard, xmpExtended);
}
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegMetadataDecoderImpl

#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
std::unique_ptr<SkXmp> SkJpegMetadataDecoderImpl::getXmpMetadata() const {
    std::vector<sk_sp<const SkData>> decoderApp1Params;
    for (const auto& marker : fMarkerList) {
        if (marker.fMarker == kXMPMarker) {
            decoderApp1Params.push_back(marker.fData);
        }
    }
    return SkJpegMakeXmp(decoderApp1Params);
}

static void emit_mp_image_for_meta(void* ctx, SkiaRsJpegMpImage img) {
    auto* out = static_cast<std::vector<SkJpegMultiPictureParameters::Image>*>(ctx);
    out->push_back(SkJpegMultiPictureParameters::Image{img.size, img.data_offset});
}

static std::unique_ptr<SkJpegMultiPictureParameters> find_mp_params(
        const SkJpegMarkerList& markerList,
        SkJpegSourceMgr* sourceMgr,
        SkJpegSegment* outMpParamsSegment) {
    auto views = build_marker_views(markerList);
    auto mpParams = std::make_unique<SkJpegMultiPictureParameters>(0);
    size_t skippedCount = 0;
    if (!skia_rs_jpeg_find_mp_in_markers(
                views.data(), views.size(), &emit_mp_image_for_meta, &mpParams->images, &skippedCount)) {
        return nullptr;
    }
    if (!sourceMgr) {
        SkASSERT(!outMpParamsSegment);
        return mpParams;
    }
    const auto& segs = sourceMgr->getAllSegments();
    SkiaRsJpegSegment rsOut{};
    if (!skia_rs_jpeg_find_mp_segment_by_skip(
                reinterpret_cast<const SkiaRsJpegSegment*>(segs.data()),
                segs.size(),
                skippedCount,
                &rsOut)) {
        return nullptr;
    }
    *outMpParamsSegment = SkJpegSegment{rsOut.offset, rsOut.marker, rsOut.parameter_length};
    return mpParams;
}

static std::pair<sk_sp<const SkData>, SkGainmapInfo> extract_gainmap(
        SkJpegSourceMgr* decoderSource,
        size_t offset,
        size_t size,
        bool baseImageHasIsoVersion,
        bool baseImageHasAdobeXmp,
        std::optional<float> baseImageAppleHdrHeadroom) {
    bool imageDataWasCopied = false;
    auto imageData = decoderSource->getSubsetData(offset, size, &imageDataWasCopied);
    if (!imageData) {
        SkCodecPrintf("Failed to extract MP image.\n");
        return {};
    }

    SkJpegMetadataDecoderImpl metadataDecoder(imageData);

    bool didPopulateInfo = false;
    SkGainmapInfo info;

    if (baseImageHasIsoVersion) {
        didPopulateInfo = SkGainmapInfo::Parse(
                metadataDecoder.getISOGainmapMetadata(/*copyData=*/false).get(), info);
        if (didPopulateInfo && info.fGainmapMathColorSpace) {
            sk_sp<SkColorSpace> imageColorSpace;
            auto iccData = metadataDecoder.getICCProfileData(/*copyData=*/false);
            skcms_ICCProfile iccProfile;
            if (iccData && skcms_Parse(iccData->data(), iccData->size(), &iccProfile)) {
                imageColorSpace = SkColorSpace::Make(iccProfile);
            }
            info.fGainmapMathColorSpace = std::move(imageColorSpace);
        }
    }

    if (!didPopulateInfo) {
        auto xmp = metadataDecoder.getXmpMetadata();
        if (!xmp) {
            return {};
        }
        if (!didPopulateInfo && baseImageHasAdobeXmp) {
            didPopulateInfo = xmp->getGainmapInfoAdobe(&info);
        }
        if (!didPopulateInfo && baseImageAppleHdrHeadroom.has_value()) {
            didPopulateInfo = xmp->getGainmapInfoApple(baseImageAppleHdrHeadroom.value(), &info);
        }
    }

    if (!didPopulateInfo) {
        return {};
    }

    if (imageDataWasCopied) {
        return {imageData, info};
    }
    return {SkData::MakeWithCopy(imageData->data(), imageData->size()), info};
}

struct HeaderScanCtx {
    const SkData* data;
    SkJpegMarkerList* markerList;
};

static void emit_header_segment_cb(void* ctx, SkiaRsJpegSegment rsSeg) {
    auto* c = static_cast<HeaderScanCtx*>(ctx);
    SkJpegSegment seg{rsSeg.offset, rsSeg.marker, rsSeg.parameter_length};
    auto parameters = SkJpegSegmentScanner::GetParameters(c->data, seg);
    if (parameters) {
        c->markerList->emplace_back(seg.marker, std::move(parameters));
    }
}
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS

SkJpegMetadataDecoderImpl::SkJpegMetadataDecoderImpl(SkJpegMarkerList markerList)
        : fMarkerList(std::move(markerList)) {}

SkJpegMetadataDecoderImpl::SkJpegMetadataDecoderImpl(sk_sp<const SkData> data) {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    if (!data) {
        return;
    }
    HeaderScanCtx ctx{data.get(), &fMarkerList};
    skia_rs_jpeg_scan_header_app1_app2(
            data->bytes(), data->size(), &emit_header_segment_cb, &ctx);
#endif
}

sk_sp<const SkData> SkJpegMetadataDecoderImpl::getExifMetadata(bool copyData) const {
    auto views = build_marker_views(fMarkerList);
    return metadata_result_to_skdata(
            fMarkerList, skia_rs_jpeg_read_exif(views.data(), views.size(), copyData));
}

sk_sp<const SkData> SkJpegMetadataDecoderImpl::getICCProfileData(bool copyData) const {
    auto views = build_marker_views(fMarkerList);
    return metadata_result_to_skdata(
            fMarkerList, skia_rs_jpeg_read_icc(views.data(), views.size(), copyData));
}

sk_sp<const SkData> SkJpegMetadataDecoderImpl::getISOGainmapMetadata(bool copyData) const {
    auto views = build_marker_views(fMarkerList);
    return metadata_result_to_skdata(
            fMarkerList, skia_rs_jpeg_read_iso_gainmap(views.data(), views.size(), copyData));
}

sk_sp<const SkData> SkJpegMetadataDecoderImpl::getJUMBFMetadata(bool copyData) const {
    auto views = build_marker_views(fMarkerList);
    return metadata_result_to_skdata(
            fMarkerList, skia_rs_jpeg_read_jumbf(views.data(), views.size(), copyData));
}

bool SkJpegMetadataDecoderImpl::mightHaveGainmapImage() const {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    auto views = build_marker_views(fMarkerList);
    return skia_rs_jpeg_might_have_gainmap(views.data(), views.size());
#else
    return false;
#endif
}

std::pair<sk_sp<const SkData>, SkGainmapInfo> SkJpegMetadataDecoderImpl::findGainmapImage(
        SkJpegSourceMgr* sourceMgr) const {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    SkExif::Metadata baseExif;
    SkExif::Parse(baseExif, getExifMetadata(/*copyData=*/false).get());
    auto xmp = getXmpMetadata();

    bool isoGainmapPresent =
            SkGainmapInfo::ParseVersion(getISOGainmapMetadata(/*copyData=*/false).get());
    bool adobeGainmapPresent = xmp && xmp->getGainmapInfoAdobe(nullptr);

    size_t containerGainmapOffset = 0;
    size_t containerGainmapSize = 0;
    if (xmp && xmp->getContainerGainmapLocation(&containerGainmapOffset, &containerGainmapSize)) {
        const auto& segments = sourceMgr->getAllSegments();
        containerGainmapOffset = skia_rs_jpeg_compute_container_gainmap_offset(
                reinterpret_cast<const SkiaRsJpegSegment*>(segments.data()),
                segments.size(),
                containerGainmapOffset);
    }

    SkJpegSegment mpParamsSegment;
    auto mpParams = find_mp_params(fMarkerList, sourceMgr, &mpParamsSegment);

    if (mpParams) {
        for (size_t mpImageIndex = 1; mpImageIndex < mpParams->images.size(); ++mpImageIndex) {
            size_t mpImageOffset = skia_rs_jpeg_mpf_get_image_absolute_offset(
                    mpParams->images[mpImageIndex].dataOffset, mpParamsSegment.offset);
            size_t mpImageSize = mpParams->images[mpImageIndex].size;

            if (auto [data, info] = extract_gainmap(sourceMgr,
                                                    mpImageOffset,
                                                    mpImageSize,
                                                    isoGainmapPresent,
                                                    adobeGainmapPresent,
                                                    baseExif.fHdrHeadroom);
                data) {
                if (containerGainmapOffset) {
                    SkASSERT(containerGainmapOffset == mpImageOffset);
                    SkASSERT(containerGainmapSize == mpImageSize);
                }
                return {data, info};
            }
        }
    }

    if (containerGainmapOffset) {
        if (auto [data, info] = extract_gainmap(sourceMgr,
                                                containerGainmapOffset,
                                                containerGainmapSize,
                                                /*baseImageHasIsoVersion=*/false,
                                                adobeGainmapPresent,
                                                /*baseImageAppleHdrHeadroom=*/std::nullopt);
            data) {
            return {data, info};
        }
        SkCodecPrintf("Failed to extract container-specified gainmap.\n");
    }
#endif
    return {};
}

std::pair<sk_sp<const SkData>, SkGainmapInfo> SkJpegMetadataDecoderImpl::findGainmapImage(
        sk_sp<const SkData> baseImageData) const {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    auto baseImageStream = SkMemoryStream::Make(baseImageData);
    auto sourceMgr = SkJpegSourceMgr::Make(baseImageStream.get());
    return this->findGainmapImage(sourceMgr.get());
#else
    return {};
#endif
}

std::unique_ptr<SkJpegMetadataDecoder> SkJpegMetadataDecoder::Make(std::vector<Segment> segments) {
    return std::make_unique<SkJpegMetadataDecoderImpl>(std::move(segments));
}

std::unique_ptr<SkJpegMetadataDecoder> SkJpegMetadataDecoder::Make(sk_sp<const SkData> data) {
    return std::make_unique<SkJpegMetadataDecoderImpl>(std::move(data));
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegCodec & SkJpegDecoder

bool SkJpegCodec::IsJpeg(const void* buffer, size_t bytesRead) {
    return skia_rs_jpeg_is_jpeg(static_cast<const uint8_t*>(buffer), bytesRead);
}

static SkJpegMarkerList get_sk_marker_list(jpeg_decompress_struct* dinfo) {
    SkJpegMarkerList markerList;
    for (auto* marker = dinfo->marker_list; marker; marker = marker->next) {
        markerList.emplace_back(marker->marker,
                                SkData::MakeWithoutCopy(marker->data, marker->data_length));
    }
    return markerList;
}

static SkEncodedOrigin get_exif_orientation(sk_sp<const SkData> exifData,
                                            SkEncodedOrigin defaultOrigin) {
    SkEncodedOrigin origin;
    if (exifData && SkParseEncodedOrigin(exifData->bytes(), exifData->size(), &origin)) {
        return origin;
    }
    return defaultOrigin;
}

enum class SaveMarkers : bool {
    kNo = false,
    kYes = true,
};

static std::tuple<SkCodec::Result, std::unique_ptr<JpegDecoderMgr>> read_header(
        SkStream* stream, SaveMarkers saveMarkers) {
    auto decoderMgr = std::make_unique<JpegDecoderMgr>(stream);

    skjpeg_error_mgr::AutoPushJmpBuf jmp(decoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return {decoderMgr->returnFailure("ReadHeader", SkCodec::kInvalidInput), nullptr};
    }

    decoderMgr->init();
    auto* dinfo = decoderMgr->dinfo();

    if (saveMarkers == SaveMarkers::kYes) {
        jpeg_save_markers(dinfo, kExifMarker, 0xFFFF);
        jpeg_save_markers(dinfo, kICCMarker, 0xFFFF);
        jpeg_save_markers(dinfo, kMpfMarker, 0xFFFF);
    }

    switch (jpeg_read_header(dinfo, TRUE)) {
        case JPEG_HEADER_OK:
            break;
        case JPEG_SUSPENDED:
            return {decoderMgr->returnFailure("ReadHeader", SkCodec::kIncompleteInput), nullptr};
        default:
            return {decoderMgr->returnFailure("ReadHeader", SkCodec::kInvalidInput), nullptr};
    }

    return {SkCodec::kSuccess, std::move(decoderMgr)};
}

std::unique_ptr<SkCodec> SkJpegCodec::MakeFromStream(std::unique_ptr<SkStream> stream,
                                                     Result* result) {
    return SkJpegCodec::MakeFromStream(std::move(stream), result, nullptr);
}

std::unique_ptr<SkCodec> SkJpegCodec::MakeFromStream(
        std::unique_ptr<SkStream> stream,
        Result* result,
        std::unique_ptr<SkCodecs::ColorProfile> defaultColorProfile,
        SkEncodedOrigin defaultOrigin) {
    SkASSERT(result);
    if (!stream) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }

    auto [r, decoderMgr] = read_header(stream.get(), SaveMarkers::kYes);
    if (r != SkCodec::kSuccess) {
        *result = r;
        return nullptr;
    }

    SkEncodedInfo::Color color;
    if (!decoderMgr->getEncodedColor(&color)) {
        *result = SkCodec::kInvalidInput;
        return nullptr;
    }

    auto* dinfo = decoderMgr->dinfo();
    auto metadataDecoder = std::make_unique<SkJpegMetadataDecoderImpl>(get_sk_marker_list(dinfo));

    SkEncodedOrigin orientation = get_exif_orientation(
            metadataDecoder->getExifMetadata(/*copyData=*/false), defaultOrigin);

    std::unique_ptr<SkCodecs::ColorProfile> profile;
    if (auto iccProfileData = metadataDecoder->getICCProfileData(/*copyData=*/true)) {
        profile = SkCodecs::ColorProfile::MakeICCProfile(std::move(iccProfileData));
    }
    if (profile && !skia_rs_jpeg_is_icc_compatible(static_cast<int32_t>(dinfo->jpeg_color_space),
                                                   static_cast<int32_t>(profile->dataSpace()))) {
        profile = nullptr;
    }
    if (!profile) {
        profile = std::move(defaultColorProfile);
    }

    SkEncodedInfo info = SkEncodedInfo::Make(dinfo->image_width,
                                             dinfo->image_height,
                                             color,
                                             SkEncodedInfo::kOpaque_Alpha,
                                             8,
                                             std::move(profile));

    *result = SkCodec::kSuccess;
    return std::unique_ptr<SkJpegCodec>(
            new SkJpegCodec(std::move(info), std::move(stream), decoderMgr.release(), orientation));
}

SkJpegCodec::SkJpegCodec(SkEncodedInfo&& info,
                         std::unique_ptr<SkStream> stream,
                         JpegDecoderMgr* decoderMgr,
                         SkEncodedOrigin origin)
        : SkCodec(std::move(info), skcms_PixelFormat_RGBA_8888, std::move(stream), origin)
        , fDecoderMgr(decoderMgr)
        , fReadyState(decoderMgr->dinfo()->global_state) {}

SkJpegCodec::~SkJpegCodec() = default;

static size_t get_row_bytes(const j_decompress_ptr dinfo) {
    return skia_rs_jpeg_get_row_bytes(static_cast<int32_t>(dinfo->out_color_space),
                                      dinfo->output_width,
                                      dinfo->out_color_components);
}

static void calc_output_dimensions(jpeg_decompress_struct* dinfo,
                                   unsigned int num,
                                   unsigned int denom) {
    dinfo->num_components = 0;
    dinfo->scale_num = num;
    dinfo->scale_denom = denom;
    jpeg_calc_output_dimensions(dinfo);
}

SkISize SkJpegCodec::onGetScaledDimensions(float desiredScale) const {
    const unsigned int num = skia_rs_jpeg_scale_numerator(desiredScale);
    constexpr unsigned int denom = 8;

    jpeg_decompress_struct dinfo;
    jpeg_create_decompress(&dinfo);
    dinfo.image_width = this->dimensions().width();
    dinfo.image_height = this->dimensions().height();
    dinfo.global_state = fReadyState;
    calc_output_dimensions(&dinfo, num, denom);
    SkISize outputDimensions = SkISize::Make(dinfo.output_width, dinfo.output_height);
    jpeg_destroy_decompress(&dinfo);

    return outputDimensions;
}

bool SkJpegCodec::onRewind() {
    if (!this->rewindStream()) {
        return false;
    }
    auto [result, decoderMgr] = read_header(this->stream(), SaveMarkers::kNo);
    if (result != kSuccess) {
        return fDecoderMgr->returnFalse("onRewind");
    }
    SkASSERT(decoderMgr);

    if ((int)decoderMgr->dinfo()->image_width != this->getEncodedInfo().width() ||
        (int)decoderMgr->dinfo()->image_height != this->getEncodedInfo().height()) {
        return fDecoderMgr->returnFailure("onRewind: dimensions changed", kInternalError);
    }

    fDecoderMgr = std::move(decoderMgr);
    fSwizzler.reset(nullptr);
    fSwizzleSrcRow = nullptr;
    fColorXformSrcRow = nullptr;
    fStorage.reset();

    return true;
}

bool SkJpegCodec::conversionSupported(const SkImageInfo& dstInfo,
                                      bool srcIsOpaque,
                                      bool needsColorXform) {
    SkASSERT(srcIsOpaque);

    SkiaRsJpegConversionPlan plan =
            skia_rs_jpeg_plan_conversion(static_cast<int32_t>(fDecoderMgr->dinfo()->jpeg_color_space),
                                         static_cast<int32_t>(dstInfo.colorType()),
                                         static_cast<int32_t>(dstInfo.alphaType()),
                                         needsColorXform);
    if (plan.warn_non_opaque) {
        SkCodecPrintf("Warning: an opaque image should be decoded as opaque "
                      "- it is being decoded as non-opaque, which will draw slower\n");
    }
    if (!plan.supported) {
        return false;
    }
    if (plan.set_dither_none) {
        fDecoderMgr->dinfo()->dither_mode = JDITHER_NONE;
    }
    fDecoderMgr->dinfo()->out_color_space = static_cast<J_COLOR_SPACE>(plan.out_color_space);
    return true;
}

bool SkJpegCodec::onDimensionsSupported(const SkISize& size) {
    skjpeg_error_mgr::AutoPushJmpBuf jmp(fDecoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return fDecoderMgr->returnFalse("onDimensionsSupported");
    }

    const unsigned int dstWidth = size.width();
    const unsigned int dstHeight = size.height();

    jpeg_decompress_struct dinfo;
    jpeg_create_decompress(&dinfo);
    dinfo.image_width = this->dimensions().width();
    dinfo.image_height = this->dimensions().height();
    dinfo.global_state = fReadyState;

    unsigned num = 8;
    constexpr unsigned denom = 8;
    calc_output_dimensions(&dinfo, num, denom);
    while (dinfo.output_width != dstWidth || dinfo.output_height != dstHeight) {
        if (1 == num || dstWidth > dinfo.output_width || dstHeight > dinfo.output_height) {
            jpeg_destroy_decompress(&dinfo);
            return false;
        }
        num -= 1;
        calc_output_dimensions(&dinfo, num, denom);
    }
    jpeg_destroy_decompress(&dinfo);

    fDecoderMgr->dinfo()->scale_num = num;
    fDecoderMgr->dinfo()->scale_denom = denom;
    return true;
}

SkCodec::Result SkJpegCodec::readRows(const SkImageInfo& dstInfo,
                                      void* dst,
                                      size_t rowBytes,
                                      int count,
                                      const Options& opts,
                                      int* rowsDecoded) {
    skjpeg_error_mgr::AutoPushJmpBuf jmp(fDecoderMgr->errorMgr());
    if (setjmp(jmp)) {
        *rowsDecoded = 0;
        return kInvalidInput;
    }

    JSAMPLE* decodeDst = (JSAMPLE*)dst;
    uint32_t* swizzleDst = (uint32_t*)dst;
    size_t decodeDstRowBytes = rowBytes;
    size_t swizzleDstRowBytes = rowBytes;
    int dstWidth = opts.fSubset ? opts.fSubset->width() : dstInfo.width();
    if (fSwizzleSrcRow && fColorXformSrcRow) {
        decodeDst = (JSAMPLE*)fSwizzleSrcRow.get();
        swizzleDst = fColorXformSrcRow;
        decodeDstRowBytes = 0;
        swizzleDstRowBytes = 0;
        dstWidth = fSwizzler->swizzleWidth();
    } else if (fColorXformSrcRow) {
        decodeDst = (JSAMPLE*)fColorXformSrcRow.get();
        swizzleDst = fColorXformSrcRow;
        decodeDstRowBytes = 0;
        swizzleDstRowBytes = 0;
    } else if (fSwizzleSrcRow) {
        decodeDst = (JSAMPLE*)fSwizzleSrcRow.get();
        decodeDstRowBytes = 0;
        dstWidth = fSwizzler->swizzleWidth();
    }

    for (int y = 0; y < count; y++) {
        uint32_t lines = jpeg_read_scanlines(fDecoderMgr->dinfo(), &decodeDst, 1);
        if (0 == lines) {
            *rowsDecoded = y;
            return kSuccess;
        }
        if (fSwizzler) {
            fSwizzler->swizzle(swizzleDst, decodeDst);
        }
        if (this->colorXform()) {
            this->applyColorXform(dst, swizzleDst, dstWidth);
            dst = SkTAddOffset<void>(dst, rowBytes);
        }
        decodeDst = SkTAddOffset<JSAMPLE>(decodeDst, decodeDstRowBytes);
        swizzleDst = SkTAddOffset<uint32_t>(swizzleDst, swizzleDstRowBytes);
    }

    *rowsDecoded = count;
    return kSuccess;
}

static inline bool needs_swizzler_to_convert_from_cmyk(J_COLOR_SPACE jpegColorType,
                                                       const SkCodecs::ColorProfile* srcProfile,
                                                       bool hasColorSpaceXform) {
    return skia_rs_jpeg_needs_cmyk_swizzler(
            static_cast<int32_t>(jpegColorType),
            srcProfile != nullptr,
            srcProfile ? static_cast<int32_t>(srcProfile->dataSpace()) : 3,
            hasColorSpaceXform);
}

SkCodec::Result SkJpegCodec::onGetPixels(const SkImageInfo& dstInfo,
                                         void* dst,
                                         size_t dstRowBytes,
                                         const Options& options,
                                         int* rowsDecoded) {
    if (options.fSubset) {
        return kUnimplemented;
    }

    jpeg_decompress_struct* dinfo = fDecoderMgr->dinfo();

    skjpeg_error_mgr::AutoPushJmpBuf jmp(fDecoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return fDecoderMgr->returnFailure("setjmp", kInvalidInput);
    }

    const bool isProgressive = dinfo->progressive_mode;
    if (isProgressive) {
        dinfo->buffered_image = TRUE;
        jpeg_start_decompress(dinfo);
    } else if (!jpeg_start_decompress(dinfo)) {
        return fDecoderMgr->returnFailure("startDecompress", kInvalidInput);
    }

    SkASSERT(1 == dinfo->rec_outbuf_height);

    if (needs_swizzler_to_convert_from_cmyk(dinfo->out_color_space,
                                            this->getEncodedInfo().colorProfile(),
                                            this->colorXform())) {
        this->initializeSwizzler(dstInfo, options, true);
    }

    if (!this->allocateStorage(dstInfo)) {
        return kOutOfMemory;
    }

    const size_t estimatedLibJpegMemory = skia_rs_jpeg_estimate_libjpeg_memory(
            isProgressive, this->dimensions().width(), this->dimensions().height());
    if (!this->allocateFromBudget(estimatedLibJpegMemory)) {
        return kOutOfMemory;
    }

    if (isProgressive) {
        int last_scan_completed = 0;
        while (!jpeg_input_complete(dinfo)) {
            if (dinfo->progress) {
                dinfo->progress->progress_monitor((j_common_ptr)dinfo);
            }
            const int res = jpeg_consume_input(dinfo);
            if (res == JPEG_SUSPENDED) {
                break;
            }
            if (res == JPEG_SCAN_COMPLETED) {
                last_scan_completed = dinfo->input_scan_number;
            }
        }
        if (last_scan_completed <= 0) {
            return fDecoderMgr->returnFailure("Incomplete image data", kIncompleteInput);
        }
        jpeg_start_output(dinfo, last_scan_completed);
        int rows = 0;
        SkCodec::Result readResult =
                this->readRows(dstInfo, dst, dstRowBytes, dstInfo.height(), options, &rows);
        jpeg_finish_output(dinfo);
        if (readResult != kSuccess) {
            return fDecoderMgr->returnFailure("readRows", readResult);
        }
        if (rows < dstInfo.height()) {
            *rowsDecoded = rows;
            return fDecoderMgr->returnFailure("Incomplete image data", kIncompleteInput);
        }
        return kSuccess;
    }

    int rows = 0;
    this->readRows(dstInfo, dst, dstRowBytes, dstInfo.height(), options, &rows);
    if (rows < dstInfo.height()) {
        *rowsDecoded = rows;
        return fDecoderMgr->returnFailure("Incomplete image data", kIncompleteInput);
    }
    return kSuccess;
}

bool SkJpegCodec::allocateStorage(const SkImageInfo& dstInfo) {
    const size_t dinfoRowBytes = fSwizzler ? get_row_bytes(fDecoderMgr->dinfo()) : 0;
    SkiaRsJpegStoragePlan plan = skia_rs_jpeg_plan_storage(
            dstInfo.width(),
            static_cast<size_t>(dstInfo.bytesPerPixel()),
            fSwizzler != nullptr,
            fSwizzler ? fSwizzler->swizzleWidth() : 0,
            dinfoRowBytes,
            this->colorXform());

    SkASSERT(!fSwizzler || !this->colorXform() || SkIsAlign4(plan.swizzle_bytes));

    if (plan.total_bytes > 0) {
        if (!this->allocateFromBudget(plan.total_bytes)) {
            return false;
        }
        fSwizzleSrcRow = nullptr;
        fColorXformSrcRow = nullptr;
        if (!fStorage.reset(plan.total_bytes)) {
            return false;
        }
        fSwizzleSrcRow = (plan.swizzle_bytes > 0) ? fStorage.get() : nullptr;
        fColorXformSrcRow = (plan.xform_bytes > 0)
                                    ? SkTAddOffset<uint32_t>(fStorage.get(), plan.swizzle_bytes)
                                    : nullptr;
    }
    return true;
}

void SkJpegCodec::initializeSwizzler(const SkImageInfo& dstInfo,
                                     const Options& options,
                                     bool needsCMYKToRGB) {
    Options swizzlerOptions = options;
    if (options.fSubset) {
        SkASSERT(!fSwizzlerSubset.isEmpty() && fSwizzlerSubset.x() <= options.fSubset->x() &&
                 fSwizzlerSubset.width() == options.fSubset->width());
        swizzlerOptions.fSubset = &fSwizzlerSubset;
    }

    SkImageInfo swizzlerDstInfo = dstInfo;
    if (this->colorXform()) {
        swizzlerDstInfo = swizzlerDstInfo.makeColorType(kRGBA_8888_SkColorType);
    }

    if (needsCMYKToRGB) {
        auto swizzlerInfo = SkEncodedInfo::Make(
                0, 0, SkEncodedInfo::kInvertedCMYK_Color, SkEncodedInfo::kOpaque_Alpha, 8);
        fSwizzler = SkSwizzler::Make(swizzlerInfo, nullptr, swizzlerDstInfo, swizzlerOptions);
    } else {
        int srcBPP = skia_rs_jpeg_src_bpp_for_out_cs(
                static_cast<int32_t>(fDecoderMgr->dinfo()->out_color_space));
        SkASSERT(srcBPP > 0);
        fSwizzler = SkSwizzler::MakeSimple(srcBPP, swizzlerDstInfo, swizzlerOptions);
    }
    SkASSERT(fSwizzler);
}

SkSampler* SkJpegCodec::getSampler(bool createIfNecessary) {
    if (!createIfNecessary || fSwizzler) {
        SkASSERT(!fSwizzler || (fSwizzleSrcRow && fStorage.get() == fSwizzleSrcRow));
        return fSwizzler.get();
    }

    bool needsCMYKToRGB = needs_swizzler_to_convert_from_cmyk(
            fDecoderMgr->dinfo()->out_color_space,
            this->getEncodedInfo().colorProfile(),
            this->colorXform());
    this->initializeSwizzler(this->dstInfo(), this->options(), needsCMYKToRGB);
    if (!this->allocateStorage(this->dstInfo())) {
        return nullptr;
    }
    return fSwizzler.get();
}

SkCodec::Result SkJpegCodec::onStartScanlineDecode(const SkImageInfo& dstInfo,
                                                   const Options& options) {
    skjpeg_error_mgr::AutoPushJmpBuf jmp(fDecoderMgr->errorMgr());
    if (setjmp(jmp)) {
        SkCodecPrintf("setjmp: Error from libjpeg\n");
        return kInvalidInput;
    }

    if (!jpeg_start_decompress(fDecoderMgr->dinfo())) {
        SkCodecPrintf("start decompress failed\n");
        return kInvalidInput;
    }

    bool needsCMYKToRGB = needs_swizzler_to_convert_from_cmyk(
            fDecoderMgr->dinfo()->out_color_space,
            this->getEncodedInfo().colorProfile(),
            this->colorXform());
    if (options.fSubset) {
        uint32_t startX = options.fSubset->x();
        uint32_t width = options.fSubset->width();
        jpeg_crop_scanline(fDecoderMgr->dinfo(), &startX, &width);

        SkASSERT(startX <= (uint32_t)options.fSubset->x());
        SkASSERT(width >= (uint32_t)options.fSubset->width());
        SkASSERT(startX + width >= (uint32_t)options.fSubset->right());

        SkiaRsJpegSubsetCropPlan cropPlan = skia_rs_jpeg_plan_subset_after_crop(
                options.fSubset->x(),
                options.fSubset->width(),
                options.fSubset->height(),
                startX,
                width);
        fSwizzlerSubset.setXYWH(cropPlan.swizzler_subset_x,
                                cropPlan.swizzler_subset_y,
                                cropPlan.swizzler_subset_w,
                                cropPlan.swizzler_subset_h);

        if (cropPlan.needs_swizzler_for_subset) {
            this->initializeSwizzler(dstInfo, options, needsCMYKToRGB);
        }
    }

    if (!fSwizzler && needsCMYKToRGB) {
        this->initializeSwizzler(dstInfo, options, true);
    }

    if (!this->allocateStorage(dstInfo)) {
        return kOutOfMemory;
    }

    return kSuccess;
}

int SkJpegCodec::onGetScanlines(void* dst, int count, size_t dstRowBytes) {
    int rows = 0;
    this->readRows(this->dstInfo(), dst, dstRowBytes, count, this->options(), &rows);
    if (rows < count) {
        fDecoderMgr->dinfo()->output_scanline = this->dstInfo().height();
    }
    return rows;
}

bool SkJpegCodec::onSkipScanlines(int count) {
    skjpeg_error_mgr::AutoPushJmpBuf jmp(fDecoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return fDecoderMgr->returnFalse("onSkipScanlines");
    }
    return (uint32_t)count == jpeg_skip_scanlines(fDecoderMgr->dinfo(), count);
}

static bool is_yuv_supported(const jpeg_decompress_struct* dinfo,
                             const SkJpegCodec& codec,
                             const SkYUVAPixmapInfo::SupportedDataTypes* supportedDataTypes,
                             SkYUVAPixmapInfo* yuvaPixmapInfo) {
    SkASSERT(dinfo->scale_num == dinfo->scale_denom);
    if (JCS_YCbCr != dinfo->jpeg_color_space) {
        return false;
    }
    SkASSERT(3 == dinfo->num_components);
    SkASSERT(dinfo->comp_info);

    const int32_t hSamp[3] = {
            dinfo->comp_info[0].h_samp_factor,
            dinfo->comp_info[1].h_samp_factor,
            dinfo->comp_info[2].h_samp_factor,
    };
    const int32_t vSamp[3] = {
            dinfo->comp_info[0].v_samp_factor,
            dinfo->comp_info[1].v_samp_factor,
            dinfo->comp_info[2].v_samp_factor,
    };
    const uint32_t widthInBlocks[3] = {
            dinfo->comp_info[0].width_in_blocks,
            dinfo->comp_info[1].width_in_blocks,
            dinfo->comp_info[2].width_in_blocks,
    };
    const bool supportsYuv =
            supportedDataTypes &&
            supportedDataTypes->supported(SkYUVAInfo::PlaneConfig::kY_U_V,
                                          SkYUVAPixmapInfo::DataType::kUnorm8);

    SkiaRsJpegYuvaQueryPlan plan =
            skia_rs_jpeg_query_yuva_info(static_cast<int32_t>(dinfo->jpeg_color_space),
                                         hSamp,
                                         vSamp,
                                         widthInBlocks,
                                         supportedDataTypes != nullptr,
                                         supportsYuv);
    if (!plan.supported) {
        return false;
    }
    if (yuvaPixmapInfo) {
        SkYUVAInfo yuvaInfo(codec.dimensions(),
                            SkYUVAInfo::PlaneConfig::kY_U_V,
                            static_cast<SkYUVAInfo::Subsampling>(plan.subsampling),
                            kJPEG_Full_SkYUVColorSpace,
                            codec.getOrigin(),
                            SkYUVAInfo::Siting::kCentered,
                            SkYUVAInfo::Siting::kCentered);
        *yuvaPixmapInfo =
                SkYUVAPixmapInfo(yuvaInfo, SkYUVAPixmapInfo::DataType::kUnorm8, plan.row_bytes);
    }
    return true;
}

bool SkJpegCodec::onQueryYUVAInfo(const SkYUVAPixmapInfo::SupportedDataTypes& supportedDataTypes,
                                  SkYUVAPixmapInfo* yuvaPixmapInfo) const {
    return is_yuv_supported(fDecoderMgr->dinfo(), *this, &supportedDataTypes, yuvaPixmapInfo);
}

SkCodec::Result SkJpegCodec::onGetYUVAPlanes(const SkYUVAPixmaps& yuvaPixmaps) {
    jpeg_decompress_struct* dinfo = fDecoderMgr->dinfo();
    if (!is_yuv_supported(dinfo, *this, nullptr, nullptr)) {
        return fDecoderMgr->returnFailure("onGetYUVAPlanes", kInvalidInput);
    }

    skjpeg_error_mgr::AutoPushJmpBuf jmp(fDecoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return fDecoderMgr->returnFailure("setjmp", kInvalidInput);
    }

    dinfo->raw_data_out = TRUE;
    if (!jpeg_start_decompress(dinfo)) {
        return fDecoderMgr->returnFailure("startDecompress", kInvalidInput);
    }

    const std::array<SkPixmap, SkYUVAPixmaps::kMaxPlanes>& planes = yuvaPixmaps.planes();

#ifdef SK_DEBUG
    {
        SkYUVAPixmapInfo info;
        SkASSERT(is_yuv_supported(dinfo, *this, nullptr, &info));
        SkASSERT(info.yuvaInfo() == yuvaPixmaps.yuvaInfo());
        for (int i = 0; i < info.numPlanes(); ++i) {
            SkASSERT(info.planeInfo(i) == planes[i].info());
        }
    }
#endif

    SkiaRsJpegYuvaPlanesPlan plan = skia_rs_jpeg_plan_yuva_planes(
            dinfo->output_height,
            dinfo->comp_info[0].v_samp_factor,
            dinfo->comp_info[1].downsampled_height);

    JSAMPARRAY yuv[3];
    std::array<JSAMPROW, 2 * DCTSIZE + DCTSIZE + DCTSIZE> rowptrs;
    yuv[0] = &rowptrs[0];
    yuv[1] = &rowptrs[2 * DCTSIZE];
    yuv[2] = &rowptrs[3 * DCTSIZE];

    const int numYRowsPerBlock = plan.num_y_rows_per_block;
    static_assert(sizeof(JSAMPLE) == 1);
    for (int i = 0; i < numYRowsPerBlock; i++) {
        rowptrs[i] = static_cast<JSAMPLE*>(planes[0].writable_addr()) + i * planes[0].rowBytes();
    }
    for (int i = 0; i < DCTSIZE; i++) {
        rowptrs[i + 2 * DCTSIZE] =
                static_cast<JSAMPLE*>(planes[1].writable_addr()) + i * planes[1].rowBytes();
        rowptrs[i + 3 * DCTSIZE] =
                static_cast<JSAMPLE*>(planes[2].writable_addr()) + i * planes[2].rowBytes();
    }

    const size_t blockIncrementY = numYRowsPerBlock * planes[0].rowBytes();
    const size_t blockIncrementU = DCTSIZE * planes[1].rowBytes();
    const size_t blockIncrementV = DCTSIZE * planes[2].rowBytes();
    const uint32_t numRowsPerBlock = numYRowsPerBlock;

    for (int i = 0; i < plan.num_iters; i++) {
        JDIMENSION linesRead = jpeg_read_raw_data(dinfo, yuv, numRowsPerBlock);
        if (linesRead < numRowsPerBlock) {
            return kInvalidInput;
        }
        for (int j = 0; j < numYRowsPerBlock; j++) {
            rowptrs[j] += blockIncrementY;
        }
        for (int j = 0; j < DCTSIZE; j++) {
            rowptrs[j + 2 * DCTSIZE] += blockIncrementU;
            rowptrs[j + 3 * DCTSIZE] += blockIncrementV;
        }
    }

    SkASSERT(plan.remaining_rows == dinfo->output_height - dinfo->output_scanline);
    if (plan.remaining_rows > 0) {
        AutoTMalloc<JSAMPLE> extraRow(planes[0].rowBytes());
        for (int i = static_cast<int>(plan.remaining_rows); i < numYRowsPerBlock; i++) {
            rowptrs[i] = extraRow.get();
        }
        for (int i = plan.remaining_uv_rows; i < DCTSIZE; i++) {
            rowptrs[i + 2 * DCTSIZE] = extraRow.get();
            rowptrs[i + 3 * DCTSIZE] = extraRow.get();
        }
        JDIMENSION linesRead = jpeg_read_raw_data(dinfo, yuv, numRowsPerBlock);
        if (linesRead < plan.remaining_rows) {
            return kInvalidInput;
        }
    }

    return kSuccess;
}

bool SkJpegCodec::onGetGainmapCodec(SkGainmapInfo* info, std::unique_ptr<SkCodec>* gainmapCodec) {
    std::unique_ptr<SkStream> stream;
    if (!this->onGetGainmapInfo(info, &stream)) {
        return false;
    }
    if (gainmapCodec) {
        Result result;
        *gainmapCodec = MakeFromStream(std::move(stream), &result);
        if (!*gainmapCodec) {
            return false;
        }
    }
    return true;
}

bool SkJpegCodec::onGetGainmapInfo(SkGainmapInfo* info,
                                   std::unique_ptr<SkStream>* gainmapImageStream) {
#ifdef SK_CODEC_DECODES_JPEG_GAINMAPS
    auto metadataDecoder =
            std::make_unique<SkJpegMetadataDecoderImpl>(get_sk_marker_list(fDecoderMgr->dinfo()));
    if (auto [data, gInfo] = metadataDecoder->findGainmapImage(fDecoderMgr->getSourceMgr()); data) {
        *info = gInfo;
        *gainmapImageStream = SkMemoryStream::Make(data);
        return true;
    }
#endif  // SK_CODEC_DECODES_JPEG_GAINMAPS
    return false;
}

namespace SkJpegDecoder {

bool IsJpeg(const void* data, size_t len) { return SkJpegCodec::IsJpeg(data, len); }

std::unique_ptr<SkCodec> Decode(std::unique_ptr<SkStream> stream,
                                SkCodec::Result* outResult,
                                SkCodecs::DecodeContext) {
    SkCodec::Result resultStorage;
    if (!outResult) {
        outResult = &resultStorage;
    }
    return SkJpegCodec::MakeFromStream(std::move(stream), outResult);
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

}  // namespace SkJpegDecoder
