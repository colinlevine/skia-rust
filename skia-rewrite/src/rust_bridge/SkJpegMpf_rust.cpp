/*
 * Safe Rust bridge for:
 *   src/codec/SkJpegSegmentScan.cpp
 *   src/codec/SkJpegMultiPicture.cpp
 * (Phase 2 Module P2.5: skia-rs-codec-jpeg, GN target: jpeg_mpf).
 *
 * 100% pure safe Rust implementation of the JPEG segment finite-state scanner
 * and CIPA DC-x007-2009 Multi-Picture Format (MPF) parser, serializer, and
 * offset calculator.
 */

#include "src/codec/SkJpegMultiPicture.h"
#include "src/codec/SkJpegSegmentScan.h"

#include "include/core/SkData.h"
#include "src/codec/SkJpegConstants.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

namespace {

struct SkiaRsJpegBytes {
    uint8_t* ptr;
    size_t len;
    size_t cap;
};

struct SkiaRsJpegSegment {
    size_t offset;
    uint8_t marker;
    uint16_t parameter_length;
};

static_assert(sizeof(SkJpegSegment) == sizeof(SkiaRsJpegSegment));
static_assert(offsetof(SkJpegSegment, offset) == offsetof(SkiaRsJpegSegment, offset));
static_assert(offsetof(SkJpegSegment, marker) == offsetof(SkiaRsJpegSegment, marker));
static_assert(offsetof(SkJpegSegment, parameterLength) ==
              offsetof(SkiaRsJpegSegment, parameter_length));

struct SkiaRsJpegScannerState {
    int32_t state;
    uint8_t stop_marker;
    uint8_t segment_param_length_byte0;
    uint8_t current_segment_marker;
    uint8_t _pad;
    size_t offset;
    size_t segment_param_bytes_remaining;
    size_t current_segment_offset;
};

struct SkiaRsJpegMpImage {
    uint32_t size;
    uint32_t data_offset;
};

static_assert(sizeof(SkJpegMultiPictureParameters::Image) == sizeof(SkiaRsJpegMpImage));
static_assert(offsetof(SkJpegMultiPictureParameters::Image, size) ==
              offsetof(SkiaRsJpegMpImage, size));
static_assert(offsetof(SkJpegMultiPictureParameters::Image, dataOffset) ==
              offsetof(SkiaRsJpegMpImage, data_offset));

extern "C" {
void skia_rs_jpeg_free_bytes(SkiaRsJpegBytes buf);

void skia_rs_jpeg_scanner_on_bytes(SkiaRsJpegScannerState* state,
                                   const uint8_t* data,
                                   size_t size,
                                   void (*emit_cb)(void*, SkiaRsJpegSegment),
                                   void* ctx);
void skia_rs_jpeg_scanner_on_byte(SkiaRsJpegScannerState* state,
                                  uint8_t byte,
                                  void (*emit_cb)(void*, SkiaRsJpegSegment),
                                  void* ctx);
void skia_rs_jpeg_scanner_on_marker_second_byte(SkiaRsJpegScannerState* state,
                                                uint8_t byte,
                                                void (*emit_cb)(void*, SkiaRsJpegSegment),
                                                void* ctx);
bool skia_rs_jpeg_segment_param_range(size_t scanned_len,
                                      SkiaRsJpegSegment segment,
                                      size_t* out_offset,
                                      size_t* out_len);

bool skia_rs_jpeg_mpf_parse(const uint8_t* data,
                            size_t len,
                            void (*emit_cb)(void*, SkiaRsJpegMpImage),
                            void* ctx);
SkiaRsJpegBytes skia_rs_jpeg_mpf_serialize(const SkiaRsJpegMpImage* images,
                                           size_t image_count,
                                           uint32_t individual_image_number);
size_t skia_rs_jpeg_mpf_get_image_absolute_offset(uint32_t data_offset, size_t mp_segment_offset);
uint32_t skia_rs_jpeg_mpf_get_image_data_offset(size_t image_absolute_offset,
                                                size_t mp_segment_offset);
}

void emit_segment_cb(void* ctx, SkiaRsJpegSegment seg) {
    auto* out = static_cast<std::vector<SkJpegSegment>*>(ctx);
    out->push_back(SkJpegSegment{seg.offset, seg.marker, seg.parameter_length});
}

void emit_mp_image_cb(void* ctx, SkiaRsJpegMpImage img) {
    auto* out = static_cast<std::vector<SkJpegMultiPictureParameters::Image>*>(ctx);
    out->push_back(SkJpegMultiPictureParameters::Image{img.size, img.data_offset});
}

}  // namespace

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegSegmentScanner

SkJpegSegmentScanner::SkJpegSegmentScanner(uint8_t stopMarker) : fStopMarker(stopMarker) {
    static_assert(static_cast<int32_t>(State::kStartOfImageByte0) == 0);
    static_assert(static_cast<int32_t>(State::kStartOfImageByte1) == 1);
    static_assert(static_cast<int32_t>(State::kSecondMarkerByte0) == 2);
    static_assert(static_cast<int32_t>(State::kSecondMarkerByte1) == 3);
    static_assert(static_cast<int32_t>(State::kSegmentParamLengthByte0) == 4);
    static_assert(static_cast<int32_t>(State::kSegmentParamLengthByte1) == 5);
    static_assert(static_cast<int32_t>(State::kSegmentParam) == 6);
    static_assert(static_cast<int32_t>(State::kEntropyCodedData) == 7);
    static_assert(static_cast<int32_t>(State::kEntropyCodedDataSentinel) == 8);
    static_assert(static_cast<int32_t>(State::kPostEntropyCodedDataFill) == 9);
    static_assert(static_cast<int32_t>(State::kDone) == 10);
    static_assert(static_cast<int32_t>(State::kError) == 11);
}

const std::vector<SkJpegSegment>& SkJpegSegmentScanner::getSegments() const { return fSegments; }

sk_sp<SkData> SkJpegSegmentScanner::GetParameters(const SkData* scannedData,
                                                  const SkJpegSegment& segment) {
    if (!scannedData) {
        return SkData::MakeEmpty();
    }
    SkiaRsJpegSegment rsSeg{segment.offset, segment.marker, segment.parameterLength};
    size_t offset = 0;
    size_t len = 0;
    if (!skia_rs_jpeg_segment_param_range(scannedData->size(), rsSeg, &offset, &len)) {
        return SkData::MakeEmpty();
    }
    return SkData::MakeSubset(scannedData, offset, len);
}

void SkJpegSegmentScanner::onBytes(const void* data, size_t size) {
    SkiaRsJpegScannerState st{
            static_cast<int32_t>(fState),
            fStopMarker,
            fSegmentParamLengthByte0,
            fCurrentSegmentMarker,
            0,
            fOffset,
            fSegmentParamBytesRemaining,
            fCurrentSegmentOffset,
    };
    skia_rs_jpeg_scanner_on_bytes(
            &st, static_cast<const uint8_t*>(data), size, &emit_segment_cb, &fSegments);
    fState = static_cast<State>(st.state);
    fSegmentParamLengthByte0 = st.segment_param_length_byte0;
    fCurrentSegmentMarker = st.current_segment_marker;
    fOffset = st.offset;
    fSegmentParamBytesRemaining = st.segment_param_bytes_remaining;
    fCurrentSegmentOffset = st.current_segment_offset;
}

void SkJpegSegmentScanner::saveCurrentSegment(uint16_t length) {
    fSegments.push_back(SkJpegSegment{fCurrentSegmentOffset, fCurrentSegmentMarker, length});
    fCurrentSegmentMarker = 0;
    fCurrentSegmentOffset = 0;
}

void SkJpegSegmentScanner::onMarkerSecondByte(uint8_t byte) {
    SkiaRsJpegScannerState st{
            static_cast<int32_t>(fState),
            fStopMarker,
            fSegmentParamLengthByte0,
            fCurrentSegmentMarker,
            0,
            fOffset,
            fSegmentParamBytesRemaining,
            fCurrentSegmentOffset,
    };
    skia_rs_jpeg_scanner_on_marker_second_byte(&st, byte, &emit_segment_cb, &fSegments);
    fState = static_cast<State>(st.state);
    fSegmentParamLengthByte0 = st.segment_param_length_byte0;
    fCurrentSegmentMarker = st.current_segment_marker;
    fOffset = st.offset;
    fSegmentParamBytesRemaining = st.segment_param_bytes_remaining;
    fCurrentSegmentOffset = st.current_segment_offset;
}

void SkJpegSegmentScanner::onByte(uint8_t byte) {
    SkiaRsJpegScannerState st{
            static_cast<int32_t>(fState),
            fStopMarker,
            fSegmentParamLengthByte0,
            fCurrentSegmentMarker,
            0,
            fOffset,
            fSegmentParamBytesRemaining,
            fCurrentSegmentOffset,
    };
    skia_rs_jpeg_scanner_on_byte(&st, byte, &emit_segment_cb, &fSegments);
    fState = static_cast<State>(st.state);
    fSegmentParamLengthByte0 = st.segment_param_length_byte0;
    fCurrentSegmentMarker = st.current_segment_marker;
    fOffset = st.offset;
    fSegmentParamBytesRemaining = st.segment_param_bytes_remaining;
    fCurrentSegmentOffset = st.current_segment_offset;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// SkJpegMultiPictureParameters

std::unique_ptr<SkJpegMultiPictureParameters> SkJpegMultiPictureParameters::Make(
        const sk_sp<const SkData>& segmentParameters) {
    if (!segmentParameters) {
        return nullptr;
    }
    auto result = std::make_unique<SkJpegMultiPictureParameters>(0);
    if (!skia_rs_jpeg_mpf_parse(segmentParameters->bytes(),
                                segmentParameters->size(),
                                &emit_mp_image_cb,
                                &result->images)) {
        return nullptr;
    }
    return result;
}

sk_sp<SkData> SkJpegMultiPictureParameters::serialize(uint32_t individualImageNumber) const {
    const auto* imgs = reinterpret_cast<const SkiaRsJpegMpImage*>(images.data());
    SkiaRsJpegBytes bytes =
            skia_rs_jpeg_mpf_serialize(imgs, images.size(), individualImageNumber);
    sk_sp<SkData> data = bytes.len > 0 ? SkData::MakeWithCopy(bytes.ptr, bytes.len)
                                       : SkData::MakeEmpty();
    skia_rs_jpeg_free_bytes(bytes);
    return data;
}

size_t SkJpegMultiPictureParameters::GetImageAbsoluteOffset(uint32_t dataOffset,
                                                            size_t mpSegmentOffset) {
    return skia_rs_jpeg_mpf_get_image_absolute_offset(dataOffset, mpSegmentOffset);
}

uint32_t SkJpegMultiPictureParameters::GetImageDataOffset(size_t imageAbsoluteOffset,
                                                          size_t mpSegmentOffset) {
    return skia_rs_jpeg_mpf_get_image_data_offset(imageAbsoluteOffset, mpSegmentOffset);
}
