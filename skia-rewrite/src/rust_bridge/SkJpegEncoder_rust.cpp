/*
 * Safe Rust bridge for:
 *   src/encode/SkJPEGWriteUtility.cpp
 *   src/encode/SkJpegEncoderImpl.cpp
 *   src/encode/SkJpegGainmapEncoder.cpp
 * (Phase 2 Module P2.5: skia-rs-codec-jpeg, GN target: jpeg_encode).
 *
 * Keeps thin third_party/libjpeg-turbo boundary calls (`jpeg_*` and `setjmp`)
 * while delegating RGB/YUV encode planning, YUV row packing, ICC/XMP/EXIF/ISO
 * segment packetization, HDRGM XMP generation, and MPF splicing to safe Rust.
 */

#include "include/encode/SkJpegEncoder.h"
#include "include/private/SkJpegGainmapEncoder.h"
#include "src/encode/SkJPEGWriteUtility.h"
#include "src/encode/SkJpegEncoderImpl.h"

#include "include/core/SkAlphaType.h"
#include "include/core/SkBitmap.h"
#include "include/core/SkColorType.h"
#include "include/core/SkData.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkRefCnt.h"
#include "include/core/SkSpan.h"
#include "include/core/SkStream.h"
#include "include/core/SkYUVAInfo.h"
#include "include/core/SkYUVAPixmaps.h"
#include "include/encode/SkEncoder.h"
#include "include/encode/SkICC.h"
#include "include/private/SkAssert.h"
#include "include/private/SkAttributes.h"
#include "include/private/SkDebug.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/SkNoncopyable.h"
#include "include/private/SkTemplates.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkJpegConstants.h"
#include "src/codec/SkJpegPriv.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkMSAN.h"
#include "src/encode/SkImageEncoderPriv.h"
#include "src/image/SkImage_Base.h"

#include <setjmp.h>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <optional>
#include <utility>
#include <vector>

class GrDirectContext;
class SkColorSpace;
class SkImage;

extern "C" {
#include "jerror.h"   // NO_G3_REWRITE
#include "jpeglib.h"  // NO_G3_REWRITE
}

namespace {

// Discriminant assertions matching `skia-rs-codec-jpeg` and `libjpeg-turbo`.
static_assert(JCS_UNKNOWN == 0);
static_assert(JCS_GRAYSCALE == 1);
static_assert(JCS_YCbCr == 3);
static_assert(JCS_EXT_RGBX == 7);
static_assert(JCS_EXT_BGRX == 9);
static_assert(JCS_EXT_RGBA == 12);
static_assert(JCS_EXT_BGRA == 13);

static_assert(static_cast<int32_t>(SkJpegEncoder::AlphaOption::kIgnore) == 0);
static_assert(static_cast<int32_t>(SkJpegEncoder::AlphaOption::kBlendOnBlack) == 1);
static_assert(static_cast<int32_t>(SkJpegEncoder::Downsample::k420) == 0);
static_assert(static_cast<int32_t>(SkJpegEncoder::Downsample::k422) == 1);
static_assert(static_cast<int32_t>(SkJpegEncoder::Downsample::k444) == 2);

static_assert(static_cast<int32_t>(kJPEG_Full_SkYUVColorSpace) == 0);
static_assert(static_cast<int32_t>(SkYUVAPixmapInfo::DataType::kUnorm8) == 0);
static_assert(static_cast<int32_t>(SkYUVAInfo::PlaneConfig::kY_U_V) == 1);
static_assert(static_cast<int32_t>(SkYUVAInfo::PlaneConfig::kY_UV) == 3);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k444) == 1);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k422) == 2);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k420) == 3);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k440) == 4);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k411) == 5);
static_assert(static_cast<int32_t>(SkYUVAInfo::Subsampling::k410) == 6);

static_assert(static_cast<int32_t>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int32_t>(kAlpha_8_SkColorType) == 1);
static_assert(static_cast<int32_t>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int32_t>(kRGB_888x_SkColorType) == 5);
static_assert(static_cast<int32_t>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int32_t>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int32_t>(kBGR_888x_SkColorType) == 29);

static_assert(static_cast<int32_t>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int32_t>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int32_t>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int32_t>(kUnpremul_SkAlphaType) == 3);

static_assert(static_cast<int32_t>(SkGainmapInfo::BaseImageType::kSDR) == 0);
static_assert(static_cast<int32_t>(SkGainmapInfo::BaseImageType::kHDR) == 1);
static_assert(static_cast<int32_t>(SkGainmapInfo::Type::kDefault) == 0);
static_assert(static_cast<int32_t>(SkGainmapInfo::Type::kApple) == 1);

struct SkiaRsJpegBytes {
    uint8_t* ptr;
    size_t len;
    size_t cap;
};

struct SkiaRsJpegSlice {
    const uint8_t* ptr;
    size_t len;
};

struct SkiaRsJpegRgbEncodePlan {
    bool valid;
    int32_t jpeg_color_space;
    int32_t num_components;
    bool use_color_xform;
    int32_t xform_dst_ct;
    int32_t xform_dst_at;
    int32_t y_h_samp;
    int32_t y_v_samp;
    size_t storage_bytes;
};

struct SkiaRsJpegYuvEncodePlan {
    bool valid;
    int32_t y_h_samp;
    int32_t y_v_samp;
    size_t storage_bytes;
};

struct SkiaRsJpegGainmapInfo {
    float gainmap_ratio_min[4];
    float gainmap_ratio_max[4];
    float gainmap_gamma[4];
    float epsilon_sdr[4];
    float epsilon_hdr[4];
    float display_ratio_sdr;
    float display_ratio_hdr;
    int32_t base_image_type;
    int32_t gainmap_type;
    bool has_gainmap_math_color_space;
};

extern "C" {
void skia_rs_jpeg_free_bytes(SkiaRsJpegBytes buf);

SkiaRsJpegRgbEncodePlan skia_rs_jpeg_plan_rgb_encode(int32_t src_width,
                                                     int32_t src_ct,
                                                     int32_t src_at,
                                                     int32_t alpha_option,
                                                     int32_t downsample);
SkiaRsJpegYuvEncodePlan skia_rs_jpeg_plan_yuv_encode(int32_t width,
                                                     int32_t yuv_color_space,
                                                     int32_t data_type,
                                                     int32_t plane_config,
                                                     int32_t subsampling);
void skia_rs_jpeg_yuva_copy_row_planar(const uint8_t* y_row,
                                       size_t y_len,
                                       const uint8_t* u_row,
                                       size_t u_len,
                                       const uint8_t* v_row,
                                       size_t v_len,
                                       size_t ss_width_u,
                                       size_t ss_width_v,
                                       uint8_t* dst,
                                       size_t dst_len);
void skia_rs_jpeg_yuva_copy_row_semi_planar(const uint8_t* y_row,
                                            size_t y_len,
                                            const uint8_t* uv_row,
                                            size_t uv_len,
                                            size_t ss_width_uv,
                                            uint8_t* dst,
                                            size_t dst_len);

SkiaRsJpegBytes skia_rs_jpeg_build_icc_segment_params(const uint8_t* icc, size_t len);
SkiaRsJpegBytes skia_rs_jpeg_build_xmp_standard_segment_params(const uint8_t* xmp, size_t len);
bool skia_rs_jpeg_build_origin_exif_segment_params(uint32_t origin, SkiaRsJpegBytes* out_bytes);
SkiaRsJpegBytes skia_rs_jpeg_build_iso_gainmap_segment_params(const uint8_t* data, size_t len);
SkiaRsJpegBytes skia_rs_jpeg_build_minimal_exif_params();
SkiaRsJpegBytes skia_rs_jpeg_build_gainmap_xmp(const SkiaRsJpegGainmapInfo* info);
SkiaRsJpegBytes skia_rs_jpeg_build_base_xmp(size_t gainmap_item_length);
size_t skia_rs_jpeg_mpf_segment_size(size_t image_count, uint32_t image_number);
bool skia_rs_jpeg_assemble_mpf(const SkiaRsJpegSlice* images,
                               size_t count,
                               SkiaRsJpegBytes* out_bytes);
}

sk_sp<SkData> bytes_to_skdata(SkiaRsJpegBytes bytes) {
    sk_sp<SkData> data = bytes.len > 0 ? SkData::MakeWithCopy(bytes.ptr, bytes.len)
                                       : SkData::MakeEmpty();
    skia_rs_jpeg_free_bytes(bytes);
    return data;
}

SkiaRsJpegGainmapInfo to_rs_gainmap_info(const SkGainmapInfo& g) {
    SkiaRsJpegGainmapInfo out{};
    out.gainmap_ratio_min[0] = g.fGainmapRatioMin.fR;
    out.gainmap_ratio_min[1] = g.fGainmapRatioMin.fG;
    out.gainmap_ratio_min[2] = g.fGainmapRatioMin.fB;
    out.gainmap_ratio_min[3] = g.fGainmapRatioMin.fA;
    out.gainmap_ratio_max[0] = g.fGainmapRatioMax.fR;
    out.gainmap_ratio_max[1] = g.fGainmapRatioMax.fG;
    out.gainmap_ratio_max[2] = g.fGainmapRatioMax.fB;
    out.gainmap_ratio_max[3] = g.fGainmapRatioMax.fA;
    out.gainmap_gamma[0] = g.fGainmapGamma.fR;
    out.gainmap_gamma[1] = g.fGainmapGamma.fG;
    out.gainmap_gamma[2] = g.fGainmapGamma.fB;
    out.gainmap_gamma[3] = g.fGainmapGamma.fA;
    out.epsilon_sdr[0] = g.fEpsilonSdr.fR;
    out.epsilon_sdr[1] = g.fEpsilonSdr.fG;
    out.epsilon_sdr[2] = g.fEpsilonSdr.fB;
    out.epsilon_sdr[3] = g.fEpsilonSdr.fA;
    out.epsilon_hdr[0] = g.fEpsilonHdr.fR;
    out.epsilon_hdr[1] = g.fEpsilonHdr.fG;
    out.epsilon_hdr[2] = g.fEpsilonHdr.fB;
    out.epsilon_hdr[3] = g.fEpsilonHdr.fA;
    out.display_ratio_sdr = g.fDisplayRatioSdr;
    out.display_ratio_hdr = g.fDisplayRatioHdr;
    out.base_image_type = static_cast<int32_t>(g.fBaseImageType);
    out.gainmap_type = static_cast<int32_t>(g.fType);
    out.has_gainmap_math_color_space = g.fGainmapMathColorSpace != nullptr;
    return out;
}

}  // namespace

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJPEGWriteUtility

static void sk_init_destination(j_compress_ptr cinfo) {
    skjpeg_destination_mgr* dest = (skjpeg_destination_mgr*)cinfo->dest;
    dest->next_output_byte = dest->fBuffer;
    dest->free_in_buffer = skjpeg_destination_mgr::kBufferSize;
}

static boolean sk_empty_output_buffer(j_compress_ptr cinfo) {
    skjpeg_destination_mgr* dest = (skjpeg_destination_mgr*)cinfo->dest;
    if (!dest->fStream->write(dest->fBuffer, skjpeg_destination_mgr::kBufferSize)) {
        ERREXIT(cinfo, JERR_FILE_WRITE);
        return FALSE;
    }
    dest->next_output_byte = dest->fBuffer;
    dest->free_in_buffer = skjpeg_destination_mgr::kBufferSize;
    return TRUE;
}

static void sk_term_destination(j_compress_ptr cinfo) {
    skjpeg_destination_mgr* dest = (skjpeg_destination_mgr*)cinfo->dest;
    size_t size = skjpeg_destination_mgr::kBufferSize - dest->free_in_buffer;
    if (size > 0) {
        if (!dest->fStream->write(dest->fBuffer, size)) {
            ERREXIT(cinfo, JERR_FILE_WRITE);
            return;
        }
    }
    dest->fStream->flush();
}

skjpeg_destination_mgr::skjpeg_destination_mgr(SkWStream* stream) : fStream(stream) {
    this->init_destination = sk_init_destination;
    this->empty_output_buffer = sk_empty_output_buffer;
    this->term_destination = sk_term_destination;
}

void skjpeg_error_exit(j_common_ptr cinfo) {
    skjpeg_error_mgr* error = static_cast<skjpeg_error_mgr*>(cinfo->err);
    (*error->output_message)(cinfo);
    jpeg_destroy(cinfo);
    if (error->fStack[0] == nullptr) {
        SK_ABORT("JPEG error with no jmp_buf set.");
    }
    longjmp(*error->fStack[0], -1);
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegEncoderMgr & SkJpegEncoderImpl

class SkJpegEncoderMgr final : SkNoncopyable {
public:
    static std::unique_ptr<SkJpegEncoderMgr> Make(SkWStream* stream) {
        return std::unique_ptr<SkJpegEncoderMgr>(new SkJpegEncoderMgr(stream));
    }

    bool initializeRGB(const SkImageInfo&,
                       const SkJpegEncoder::Options&,
                       const SkJpegMetadataEncoder::SegmentList&);
    bool initializeYUV(const SkYUVAPixmapInfo&,
                       const SkJpegEncoder::Options&,
                       const SkJpegMetadataEncoder::SegmentList&);

    jpeg_compress_struct* cinfo() { return &fCInfo; }
    skjpeg_error_mgr* errorMgr() { return &fErrMgr; }
    bool shouldUseColorXform() const { return fUseColorXform; }
    size_t storageBytes() const { return fStorageBytes; }
    bool colorTransformProc(void* dst, const void* src, int width);

    ~SkJpegEncoderMgr() { jpeg_destroy_compress(&fCInfo); }

private:
    SkJpegEncoderMgr(SkWStream* stream) : fDstMgr(stream) {
        fCInfo.err = jpeg_std_error(&fErrMgr);
        fErrMgr.error_exit = skjpeg_error_exit;
        jpeg_create_compress(&fCInfo);
        fCInfo.dest = &fDstMgr;
    }
    void initializeCommon(const SkJpegEncoder::Options&,
                          const SkJpegMetadataEncoder::SegmentList&);

    jpeg_compress_struct fCInfo;
    skjpeg_error_mgr fErrMgr;
    skjpeg_destination_mgr fDstMgr;

    std::optional<SkImageInfo> fSrcInfo;
    std::optional<SkImageInfo> fDstInfo;
    bool fUseColorXform = false;
    size_t fStorageBytes = 0;
};

bool SkJpegEncoderMgr::colorTransformProc(void* dst, const void* src, const int width) {
    SkASSERT(fUseColorXform);
    SkASSERT(fSrcInfo && fDstInfo);
    SkASSERT(width == fSrcInfo->width());
    return SkConvertPixels(fDstInfo.value(),
                           dst,
                           fDstInfo->minRowBytes(),
                           fSrcInfo.value(),
                           src,
                           fSrcInfo->minRowBytes());
}

bool SkJpegEncoderMgr::initializeRGB(const SkImageInfo& srcInfo,
                                     const SkJpegEncoder::Options& options,
                                     const SkJpegMetadataEncoder::SegmentList& metadataSegments) {
    SkiaRsJpegRgbEncodePlan plan =
            skia_rs_jpeg_plan_rgb_encode(srcInfo.width(),
                                         static_cast<int32_t>(srcInfo.colorType()),
                                         static_cast<int32_t>(srcInfo.alphaType()),
                                         static_cast<int32_t>(options.fAlphaOption),
                                         static_cast<int32_t>(options.fDownsample));
    if (!plan.valid) {
        return false;
    }

    fUseColorXform = plan.use_color_xform;
    fStorageBytes = plan.storage_bytes;
    if (fUseColorXform) {
        fSrcInfo = srcInfo.makeWH(srcInfo.width(), 1);
        fDstInfo = SkImageInfo::Make(srcInfo.width(),
                                     1,
                                     static_cast<SkColorType>(plan.xform_dst_ct),
                                     static_cast<SkAlphaType>(plan.xform_dst_at));
    }

    fCInfo.image_width = srcInfo.width();
    fCInfo.image_height = srcInfo.height();
    fCInfo.in_color_space = static_cast<J_COLOR_SPACE>(plan.jpeg_color_space);
    fCInfo.input_components = plan.num_components;
    jpeg_set_defaults(&fCInfo);

    if (plan.num_components != 1) {
        fCInfo.comp_info[0].h_samp_factor = plan.y_h_samp;
        fCInfo.comp_info[0].v_samp_factor = plan.y_v_samp;
    }

    initializeCommon(options, metadataSegments);
    return true;
}

static void yuva_copy_row(const SkYUVAPixmaps& src, int row, uint8_t* dst) {
    const int width = src.plane(0).width();
    const size_t dstLen = static_cast<size_t>(width) * 3;
    switch (src.yuvaInfo().planeConfig()) {
        case SkYUVAInfo::PlaneConfig::kY_U_V: {
            auto [ssWidthU, ssHeightU] = src.yuvaInfo().planeSubsamplingFactors(1);
            auto [ssWidthV, ssHeightV] = src.yuvaInfo().planeSubsamplingFactors(2);
            const auto* srcY = static_cast<const uint8_t*>(src.plane(0).addr(0, row));
            const auto* srcU = static_cast<const uint8_t*>(src.plane(1).addr(0, row / ssHeightU));
            const auto* srcV = static_cast<const uint8_t*>(src.plane(2).addr(0, row / ssHeightV));
            skia_rs_jpeg_yuva_copy_row_planar(srcY,
                                              static_cast<size_t>(width),
                                              srcU,
                                              static_cast<size_t>(src.plane(1).width()),
                                              srcV,
                                              static_cast<size_t>(src.plane(2).width()),
                                              static_cast<size_t>(ssWidthU),
                                              static_cast<size_t>(ssWidthV),
                                              dst,
                                              dstLen);
            break;
        }
        case SkYUVAInfo::PlaneConfig::kY_UV: {
            auto [ssWidthUV, ssHeightUV] = src.yuvaInfo().planeSubsamplingFactors(1);
            const auto* srcY = static_cast<const uint8_t*>(src.plane(0).addr(0, row));
            const auto* srcUV =
                    static_cast<const uint8_t*>(src.plane(1).addr(0, row / ssHeightUV));
            skia_rs_jpeg_yuva_copy_row_semi_planar(srcY,
                                                   static_cast<size_t>(width),
                                                   srcUV,
                                                   static_cast<size_t>(src.plane(1).width()) * 2,
                                                   static_cast<size_t>(ssWidthUV),
                                                   dst,
                                                   dstLen);
            break;
        }
        default:
            break;
    }
}

bool SkJpegEncoderMgr::initializeYUV(const SkYUVAPixmapInfo& srcInfo,
                                     const SkJpegEncoder::Options& options,
                                     const SkJpegMetadataEncoder::SegmentList& metadataSegments) {
    fCInfo.image_width = srcInfo.yuvaInfo().width();
    fCInfo.image_height = srcInfo.yuvaInfo().height();
    fCInfo.in_color_space = JCS_YCbCr;
    fCInfo.input_components = 3;
    jpeg_set_defaults(&fCInfo);

    SkiaRsJpegYuvEncodePlan plan =
            skia_rs_jpeg_plan_yuv_encode(srcInfo.yuvaInfo().width(),
                                         static_cast<int32_t>(srcInfo.yuvColorSpace()),
                                         static_cast<int32_t>(srcInfo.dataType()),
                                         static_cast<int32_t>(srcInfo.yuvaInfo().planeConfig()),
                                         static_cast<int32_t>(srcInfo.yuvaInfo().subsampling()));
    if (!plan.valid) {
        return false;
    }

    fStorageBytes = plan.storage_bytes;
    fCInfo.comp_info[0].h_samp_factor = plan.y_h_samp;
    fCInfo.comp_info[0].v_samp_factor = plan.y_v_samp;

    initializeCommon(options, metadataSegments);
    return true;
}

void SkJpegEncoderMgr::initializeCommon(
        const SkJpegEncoder::Options& options,
        const SkJpegMetadataEncoder::SegmentList& metadataSegments) {
    fCInfo.optimize_coding = TRUE;
    jpeg_set_quality(&fCInfo, options.fQuality, TRUE);
    jpeg_start_compress(&fCInfo, TRUE);

    for (const auto& segment : metadataSegments) {
        jpeg_write_marker(&fCInfo,
                          segment.fMarker,
                          segment.fParameters->bytes(),
                          segment.fParameters->size());
    }
}

std::unique_ptr<SkEncoder> SkJpegEncoderImpl::MakeYUV(
        SkWStream* dst,
        const SkYUVAPixmaps& srcYUVA,
        const SkColorSpace* srcYUVAColorSpace,
        const SkJpegEncoder::Options& options,
        const SkJpegMetadataEncoder::SegmentList& metadataSegments) {
    if (!srcYUVA.isValid()) {
        return nullptr;
    }
    std::unique_ptr<SkJpegEncoderMgr> encoderMgr = SkJpegEncoderMgr::Make(dst);
    skjpeg_error_mgr::AutoPushJmpBuf jmp(encoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return nullptr;
    }

    if (!encoderMgr->initializeYUV(srcYUVA.pixmapsInfo(), options, metadataSegments)) {
        return nullptr;
    }
    return std::unique_ptr<SkJpegEncoderImpl>(
            new SkJpegEncoderImpl(std::move(encoderMgr), srcYUVA));
}

std::unique_ptr<SkEncoder> SkJpegEncoderImpl::MakeRGB(
        SkWStream* dst,
        const SkPixmap& src,
        const SkJpegEncoder::Options& options,
        const SkJpegMetadataEncoder::SegmentList& metadataSegments) {
    if (!SkPixmapIsValid(src)) {
        return nullptr;
    }
    std::unique_ptr<SkJpegEncoderMgr> encoderMgr = SkJpegEncoderMgr::Make(dst);
    skjpeg_error_mgr::AutoPushJmpBuf jmp(encoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return nullptr;
    }

    if (!encoderMgr->initializeRGB(src.info(), options, metadataSegments)) {
        return nullptr;
    }
    return std::unique_ptr<SkJpegEncoderImpl>(new SkJpegEncoderImpl(std::move(encoderMgr), src));
}

SkJpegEncoderImpl::SkJpegEncoderImpl(std::unique_ptr<SkJpegEncoderMgr> encoderMgr,
                                     const SkPixmap& src)
        : SkEncoder(src, encoderMgr->storageBytes()), fEncoderMgr(std::move(encoderMgr)) {}

SkJpegEncoderImpl::SkJpegEncoderImpl(std::unique_ptr<SkJpegEncoderMgr> encoderMgr,
                                     const SkYUVAPixmaps& src)
        : SkEncoder(src.plane(0), encoderMgr->storageBytes())
        , fEncoderMgr(std::move(encoderMgr))
        , fSrcYUVA(src) {}

SkJpegEncoderImpl::~SkJpegEncoderImpl() {}

bool SkJpegEncoderImpl::onEncodeRows(int numRows) {
    skjpeg_error_mgr::AutoPushJmpBuf jmp(fEncoderMgr->errorMgr());
    if (setjmp(jmp)) {
        return false;
    }

    if (fSrcYUVA) {
        for (int i = 0; i < numRows; i++) {
            yuva_copy_row(*fSrcYUVA, fCurrRow + i, fStorage.get());
            JSAMPLE* jpegSrcRow = fStorage.get();
            jpeg_write_scanlines(fEncoderMgr->cinfo(), &jpegSrcRow, 1);
        }
    } else {
        const size_t srcBytes = SkColorTypeBytesPerPixel(fSrc.colorType()) * fSrc.width();
        const size_t jpegSrcBytes = fEncoderMgr->cinfo()->input_components * fSrc.width();
        const void* srcRow = fSrc.addr(0, fCurrRow);
        for (int i = 0; i < numRows; i++) {
            JSAMPLE* jpegSrcRow = (JSAMPLE*)(const_cast<void*>(srcRow));
            if (fEncoderMgr->shouldUseColorXform()) {
                sk_msan_assert_initialized(srcRow, SkTAddOffset<const void>(srcRow, srcBytes));
                if (!fEncoderMgr->colorTransformProc((void*)fStorage.get(), srcRow, fSrc.width())) {
                    return false;
                }
                jpegSrcRow = fStorage.get();
                sk_msan_assert_initialized(jpegSrcRow,
                                           SkTAddOffset<const void>(jpegSrcRow, jpegSrcBytes));
            } else {
                sk_msan_assert_initialized(jpegSrcRow,
                                           SkTAddOffset<const void>(jpegSrcRow, jpegSrcBytes));
            }

            jpeg_write_scanlines(fEncoderMgr->cinfo(), &jpegSrcRow, 1);
            srcRow = SkTAddOffset<const void>(srcRow, fSrc.rowBytes());
        }
    }

    fCurrRow += numRows;
    if (fCurrRow == fSrc.height()) {
        jpeg_finish_compress(fEncoderMgr->cinfo());
    }

    return true;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegEncoder public API

namespace SkJpegEncoder {

bool Encode(SkWStream* dst, const SkPixmap& src, const Options& options) {
    auto encoder = Make(dst, src, options);
    return encoder.get() && encoder->encodeRows(src.height());
}

bool Encode(SkWStream* dst,
            const SkYUVAPixmaps& src,
            const SkColorSpace* srcColorSpace,
            const Options& options) {
    auto encoder = Make(dst, src, srcColorSpace, options);
    return encoder.get() && encoder->encodeRows(src.yuvaInfo().height());
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

std::unique_ptr<SkEncoder> Make(SkWStream* dst, const SkPixmap& src, const Options& options) {
    SkJpegMetadataEncoder::SegmentList metadataSegments;
    SkJpegMetadataEncoder::AppendXMPStandard(metadataSegments, options.xmpMetadata);
    SkJpegMetadataEncoder::AppendICC(metadataSegments, options, src.colorSpace());
    if (options.fOrigin.has_value()) {
        SkJpegMetadataEncoder::AppendOrigin(metadataSegments, options.fOrigin.value());
    }
    return SkJpegEncoderImpl::MakeRGB(dst, src, options, metadataSegments);
}

std::unique_ptr<SkEncoder> Make(SkWStream* dst,
                                const SkYUVAPixmaps& src,
                                const SkColorSpace* srcColorSpace,
                                const Options& options) {
    SkJpegMetadataEncoder::SegmentList metadataSegments;
    SkJpegMetadataEncoder::AppendXMPStandard(metadataSegments, options.xmpMetadata);
    SkJpegMetadataEncoder::AppendICC(metadataSegments, options, srcColorSpace);
    if (options.fOrigin.has_value()) {
        SkJpegMetadataEncoder::AppendOrigin(metadataSegments, options.fOrigin.value());
    }
    return SkJpegEncoderImpl::MakeYUV(dst, src, srcColorSpace, options, metadataSegments);
}

}  // namespace SkJpegEncoder

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegMetadataEncoder

namespace SkJpegMetadataEncoder {

void AppendICC(SegmentList& segmentList,
               const SkJpegEncoder::Options& options,
               const SkColorSpace* colorSpace) {
    sk_sp<SkData> icc = SkWriteICCProfile(colorSpace, options.fHdrMetadata);
    if (!icc) {
        return;
    }
    segmentList.emplace_back(
            kICCMarker,
            bytes_to_skdata(skia_rs_jpeg_build_icc_segment_params(icc->bytes(), icc->size())));
}

void AppendXMPStandard(SegmentList& segmentList, const SkData* xmpMetadata) {
    if (!xmpMetadata) {
        return;
    }
    segmentList.emplace_back(
            kXMPMarker,
            bytes_to_skdata(skia_rs_jpeg_build_xmp_standard_segment_params(
                    xmpMetadata->bytes(), xmpMetadata->size())));
}

void AppendOrigin(SegmentList& segmentList, SkEncodedOrigin origin) {
    SkiaRsJpegBytes bytes{};
    if (!skia_rs_jpeg_build_origin_exif_segment_params(static_cast<uint32_t>(origin), &bytes)) {
        SkDebugf("Origin is not a valid value.\n");
        return;
    }
    segmentList.emplace_back(kExifMarker, bytes_to_skdata(bytes));
}

}  // namespace SkJpegMetadataEncoder

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegGainmapEncoder

static sk_sp<SkData> encode_to_data(const SkPixmap& pm,
                                    const SkJpegEncoder::Options& options,
                                    const SkJpegMetadataEncoder::SegmentList& metadataSegments) {
    SkDynamicMemoryWStream encodeStream;
    auto encoder = SkJpegEncoderImpl::MakeRGB(&encodeStream, pm, options, metadataSegments);
    if (!encoder || !encoder->encodeRows(pm.height())) {
        return nullptr;
    }
    return encodeStream.detachAsData();
}

bool SkJpegGainmapEncoder::EncodeHDRGM(SkWStream* dst,
                                       const SkPixmap& base,
                                       const SkJpegEncoder::Options& baseOptions,
                                       const SkPixmap& gainmap,
                                       const SkJpegEncoder::Options& gainmapOptions,
                                       const SkGainmapInfo& gainmapInfo) {
    const bool includeUltraHDRv1 = gainmapInfo.isUltraHDRv1Compatible();
    sk_sp<SkData> exifParams = bytes_to_skdata(skia_rs_jpeg_build_minimal_exif_params());

    // Encode the gainmap image.
    sk_sp<SkData> gainmapData;
    {
        SkJpegMetadataEncoder::SegmentList metadataSegments;
        metadataSegments.emplace_back(kExifMarker, exifParams);

        if (includeUltraHDRv1) {
            SkiaRsJpegGainmapInfo rsInfo = to_rs_gainmap_info(gainmapInfo);
            sk_sp<SkData> xmp = bytes_to_skdata(skia_rs_jpeg_build_gainmap_xmp(&rsInfo));
            SkJpegMetadataEncoder::AppendXMPStandard(metadataSegments, xmp.get());
        }

        if (gainmapInfo.fGainmapMathColorSpace) {
            SkJpegMetadataEncoder::AppendICC(
                    metadataSegments, gainmapOptions, gainmapInfo.fGainmapMathColorSpace.get());
        }

        sk_sp<SkData> isoBytes = gainmapInfo.serialize();
        metadataSegments.emplace_back(
                kISOGainmapMarker,
                bytes_to_skdata(skia_rs_jpeg_build_iso_gainmap_segment_params(
                        isoBytes->bytes(), isoBytes->size())));

        gainmapData = encode_to_data(gainmap, gainmapOptions, metadataSegments);
        if (!gainmapData) {
            SkCodecPrintf("Failed to encode gainmap image.\n");
            return false;
        }
    }

    // Encode the base image.
    sk_sp<SkData> baseData;
    {
        SkJpegMetadataEncoder::SegmentList metadataSegments;
        metadataSegments.emplace_back(kExifMarker, exifParams);

        if (includeUltraHDRv1) {
            size_t gainmapImageSize =
                    gainmapData->size() + skia_rs_jpeg_mpf_segment_size(2, 1);
            sk_sp<SkData> baseXmp = bytes_to_skdata(
                    skia_rs_jpeg_build_base_xmp(static_cast<int32_t>(gainmapImageSize)));
            SkJpegMetadataEncoder::AppendXMPStandard(metadataSegments, baseXmp.get());
        }

        SkJpegMetadataEncoder::AppendICC(metadataSegments, baseOptions, base.colorSpace());

        sk_sp<SkData> isoVer = SkGainmapInfo::SerializeVersion();
        metadataSegments.emplace_back(
                kISOGainmapMarker,
                bytes_to_skdata(skia_rs_jpeg_build_iso_gainmap_segment_params(
                        isoVer->bytes(), isoVer->size())));

        baseData = encode_to_data(base, baseOptions, metadataSegments);
        if (!baseData) {
            SkCodecPrintf("Failed to encode base image.\n");
            return false;
        }
    }

    const SkData* images[] = {
            baseData.get(),
            gainmapData.get(),
    };
    return MakeMPF(dst, images);
}

bool SkJpegGainmapEncoder::MakeMPF(SkWStream* dst, const SkData** images, size_t imageCount) {
    return MakeMPF(dst, SK_UNSAFE_TODO(SkSpan<const SkData* const>(images, imageCount)));
}

bool SkJpegGainmapEncoder::MakeMPF(SkWStream* dst, SkSpan<const SkData* const> images) {
    if (images.size() < 1) {
        return true;
    }
    std::vector<SkiaRsJpegSlice> slices;
    slices.reserve(images.size());
    for (const SkData* img : images) {
        slices.push_back(SkiaRsJpegSlice{img->bytes(), img->size()});
    }
    SkiaRsJpegBytes assembled{};
    if (!skia_rs_jpeg_assemble_mpf(slices.data(), slices.size(), &assembled)) {
        return false;
    }
    bool ok = dst->write(assembled.ptr, assembled.len);
    skia_rs_jpeg_free_bytes(assembled);
    return ok;
}
