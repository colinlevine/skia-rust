/*
 * Safe Rust bridge for:
 *   src/encode/SkPngEncoderBase.cpp
 * (Phase 2 Module P2.4: skia-rs-codec-png, GN target: png_encode_common).
 */

#include "src/encode/SkPngEncoderBase.h"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <utility>

#include "include/core/SkAlphaType.h"
#include "include/core/SkColorType.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPixmap.h"
#include "include/core/SkSpan.h"
#include "include/private/SkAssert.h"
#include "include/private/SkEncodedInfo.h"
#include "src/core/SkConvertPixels.h"
#include "src/core/SkMSAN.h"

namespace {

static_assert(static_cast<int>(kUnknown_SkColorType) == 0);
static_assert(static_cast<int>(kAlpha_8_SkColorType) == 1);
static_assert(static_cast<int>(kRGB_565_SkColorType) == 2);
static_assert(static_cast<int>(kARGB_4444_SkColorType) == 3);
static_assert(static_cast<int>(kRGBA_8888_SkColorType) == 4);
static_assert(static_cast<int>(kRGB_888x_SkColorType) == 5);
static_assert(static_cast<int>(kBGRA_8888_SkColorType) == 6);
static_assert(static_cast<int>(kRGBA_1010102_SkColorType) == 7);
static_assert(static_cast<int>(kBGRA_1010102_SkColorType) == 8);
static_assert(static_cast<int>(kRGB_101010x_SkColorType) == 9);
static_assert(static_cast<int>(kBGR_101010x_SkColorType) == 10);
static_assert(static_cast<int>(kBGR_101010x_XR_SkColorType) == 11);
static_assert(static_cast<int>(kBGRA_10101010_XR_SkColorType) == 12);
static_assert(static_cast<int>(kRGBA_10x6_SkColorType) == 13);
static_assert(static_cast<int>(kGray_8_SkColorType) == 14);
static_assert(static_cast<int>(kRGBA_F16Norm_SkColorType) == 15);
static_assert(static_cast<int>(kRGBA_F16_SkColorType) == 16);
static_assert(static_cast<int>(kRGB_F16F16F16x_SkColorType) == 17);
static_assert(static_cast<int>(kRGBA_F32_SkColorType) == 18);
static_assert(static_cast<int>(kR16G16B16A16_unorm_SkColorType) == 26);
static_assert(static_cast<int>(kSRGBA_8888_SkColorType) == 27);
static_assert(static_cast<int>(kR8_unorm_SkColorType) == 28);
static_assert(static_cast<int>(kBGR_888x_SkColorType) == 29);

static_assert(static_cast<int>(kUnknown_SkAlphaType) == 0);
static_assert(static_cast<int>(kOpaque_SkAlphaType) == 1);
static_assert(static_cast<int>(kPremul_SkAlphaType) == 2);
static_assert(static_cast<int>(kUnpremul_SkAlphaType) == 3);

static_assert(static_cast<int>(SkEncodedInfo::kGray_Color) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kGrayAlpha_Color) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kRGB_Color) == 5);
static_assert(static_cast<int>(SkEncodedInfo::kRGBA_Color) == 6);

static_assert(static_cast<int>(SkEncodedInfo::kOpaque_Alpha) == 0);
static_assert(static_cast<int>(SkEncodedInfo::kUnpremul_Alpha) == 1);
static_assert(static_cast<int>(SkEncodedInfo::kBinary_Alpha) == 2);

struct SkiaRsPngTargetInfoPlan {
    bool valid;
    bool is_alpha8;
    int32_t dst_encoded_color;
    int32_t dst_encoded_alpha;
    int32_t dst_bits_per_component;
    int32_t dst_row_ct;
    int32_t dst_row_at;
    size_t dst_row_size;
};

extern "C" {
bool skia_rs_png_encoder_get_target_info(int32_t width,
                                         int32_t src_ct,
                                         int32_t src_at,
                                         SkiaRsPngTargetInfoPlan* out_plan);
bool skia_rs_png_encoder_validate_rows_args(int32_t width, int32_t height, int32_t num_rows);
void skia_rs_png_transform_a8_to_gray_alpha(uint8_t* dst, const uint8_t* src, int32_t width);
}

}  // namespace

// static
std::optional<SkPngEncoderBase::TargetInfo> SkPngEncoderBase::getTargetInfo(
        const SkImageInfo& srcInfo) {
    SkiaRsPngTargetInfoPlan plan{};
    if (!skia_rs_png_encoder_get_target_info(srcInfo.width(),
                                             static_cast<int32_t>(srcInfo.colorType()),
                                             static_cast<int32_t>(srcInfo.alphaType()),
                                             &plan) ||
        !plan.valid) {
        return std::nullopt;
    }

    SkEncodedInfo dstInfo = SkEncodedInfo::Make(
            srcInfo.width(),
            srcInfo.height(),
            static_cast<SkEncodedInfo::Color>(plan.dst_encoded_color),
            static_cast<SkEncodedInfo::Alpha>(plan.dst_encoded_alpha),
            plan.dst_bits_per_component);

    if (plan.is_alpha8) {
        return TargetInfo{std::nullopt, std::nullopt, std::move(dstInfo), plan.dst_row_size};
    }

    SkImageInfo dstRowInfo = SkImageInfo::Make(srcInfo.width(),
                                               1,
                                               static_cast<SkColorType>(plan.dst_row_ct),
                                               static_cast<SkAlphaType>(plan.dst_row_at));
    return TargetInfo{srcInfo.makeWH(srcInfo.width(), 1),
                      dstRowInfo,
                      std::move(dstInfo),
                      dstRowInfo.minRowBytes()};
}

SkPngEncoderBase::SkPngEncoderBase(TargetInfo targetInfo, const SkPixmap& src)
        : SkEncoder(src, targetInfo.fDstRowSize), fTargetInfo(std::move(targetInfo)) {
    SkASSERT(src.colorType() == kAlpha_8_SkColorType ||
             (fTargetInfo.fSrcRowInfo && fTargetInfo.fDstRowInfo));
}

bool SkPngEncoderBase::onEncodeRows(int numRows) {
    if (!skia_rs_png_encoder_validate_rows_args(fSrc.width(), fSrc.height(), numRows)) {
        return false;
    }

    while (numRows > 0) {
        if (fCurrRow == fSrc.height()) {
            return false;
        }

        const void* srcRow = fSrc.addr(0, fCurrRow);
        sk_msan_assert_initialized(
                srcRow, static_cast<const uint8_t*>(srcRow) + (fSrc.width() << fSrc.shiftPerPixel()));

        if (fSrc.colorType() == kAlpha_8_SkColorType) {
            skia_rs_png_transform_a8_to_gray_alpha(
                    fStorage.get(), static_cast<const uint8_t*>(srcRow), fSrc.width());
        } else {
            SkASSERT(fSrc.width() == fTargetInfo.fSrcRowInfo->width());
            if (!SkConvertPixels(fTargetInfo.fDstRowInfo.value(),
                                 static_cast<void*>(fStorage.get()),
                                 fTargetInfo.fDstRowSize,
                                 fTargetInfo.fSrcRowInfo.value(),
                                 srcRow,
                                 fTargetInfo.fSrcRowInfo->minRowBytes())) {
                return false;
            }
        }

        SkSpan<const uint8_t> rowToEncode(fStorage.get(), fTargetInfo.fDstRowSize);
        if (!this->onEncodeRow(rowToEncode)) {
            return false;
        }

        fCurrRow++;
        numRows--;
    }

    if (fCurrRow == fSrc.height() && !fFinishedEncoding) {
        fFinishedEncoding = true;
        return this->onFinishEncoding();
    }

    return true;
}
