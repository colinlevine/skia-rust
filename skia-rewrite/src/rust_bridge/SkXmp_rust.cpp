/*
 * Safe Rust bridge for:
 *   src/codec/SkXmp.cpp
 * (Phase 2.2 skia-rs-codec-meta, optional("xml")).
 */

#include "include/core/SkData.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/SkXmp.h"

#include <cstddef>
#include <cstdint>
#include <memory>

namespace {

struct SkiaRsXmp;

struct SkiaRsGainmapInfo {
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
SkiaRsXmp* skia_rs_xmp_make(const uint8_t* std_data,
                            size_t std_len,
                            const uint8_t* ext_data,
                            size_t ext_len);
void skia_rs_xmp_destroy(SkiaRsXmp* xmp);
const char* skia_rs_xmp_get_extended_guid(const SkiaRsXmp* xmp);
bool skia_rs_xmp_get_container_gainmap_location(const SkiaRsXmp* xmp,
                                                size_t* out_offset,
                                                size_t* out_size);
bool skia_rs_xmp_get_gainmap_info_apple(const SkiaRsXmp* xmp,
                                        float exif_hdr_headroom,
                                        SkiaRsGainmapInfo* out_info);
bool skia_rs_xmp_get_gainmap_info_adobe(const SkiaRsXmp* xmp, SkiaRsGainmapInfo* out_info);
}  // extern "C"

class SkXmpRustImpl final : public SkXmp {
public:
    explicit SkXmpRustImpl(SkiaRsXmp* handle) : fHandle(handle) {}
    ~SkXmpRustImpl() override { skia_rs_xmp_destroy(fHandle); }

    bool getGainmapInfoAdobe(SkGainmapInfo* info) const override {
        SkiaRsGainmapInfo raw{};
        if (!skia_rs_xmp_get_gainmap_info_adobe(fHandle, info ? &raw : nullptr)) {
            return false;
        }
        if (!info) {
            return true;
        }
        info->fGainmapRatioMin = {raw.gainmap_ratio_min[0],
                                  raw.gainmap_ratio_min[1],
                                  raw.gainmap_ratio_min[2],
                                  raw.gainmap_ratio_min[3]};
        info->fGainmapRatioMax = {raw.gainmap_ratio_max[0],
                                  raw.gainmap_ratio_max[1],
                                  raw.gainmap_ratio_max[2],
                                  raw.gainmap_ratio_max[3]};
        info->fGainmapGamma = {raw.gainmap_gamma[0],
                               raw.gainmap_gamma[1],
                               raw.gainmap_gamma[2],
                               raw.gainmap_gamma[3]};
        info->fEpsilonSdr = {
                raw.epsilon_sdr[0], raw.epsilon_sdr[1], raw.epsilon_sdr[2], raw.epsilon_sdr[3]};
        info->fEpsilonHdr = {
                raw.epsilon_hdr[0], raw.epsilon_hdr[1], raw.epsilon_hdr[2], raw.epsilon_hdr[3]};
        info->fDisplayRatioSdr = raw.display_ratio_sdr;
        info->fDisplayRatioHdr = raw.display_ratio_hdr;
        info->fBaseImageType = raw.base_image_type == 1 ? SkGainmapInfo::BaseImageType::kHDR
                                                        : SkGainmapInfo::BaseImageType::kSDR;
        return true;
    }

    bool getGainmapInfoApple(float exifHdrHeadroom, SkGainmapInfo* info) const override {
        if (!info) {
            return false;
        }
        SkiaRsGainmapInfo raw{};
        if (!skia_rs_xmp_get_gainmap_info_apple(fHandle, exifHdrHeadroom, &raw)) {
            return false;
        }
        info->fGainmapRatioMin = {raw.gainmap_ratio_min[0],
                                  raw.gainmap_ratio_min[1],
                                  raw.gainmap_ratio_min[2],
                                  raw.gainmap_ratio_min[3]};
        info->fGainmapRatioMax = {raw.gainmap_ratio_max[0],
                                  raw.gainmap_ratio_max[1],
                                  raw.gainmap_ratio_max[2],
                                  raw.gainmap_ratio_max[3]};
        info->fGainmapGamma = {raw.gainmap_gamma[0],
                               raw.gainmap_gamma[1],
                               raw.gainmap_gamma[2],
                               raw.gainmap_gamma[3]};
        info->fEpsilonSdr = {
                raw.epsilon_sdr[0], raw.epsilon_sdr[1], raw.epsilon_sdr[2], raw.epsilon_sdr[3]};
        info->fEpsilonHdr = {
                raw.epsilon_hdr[0], raw.epsilon_hdr[1], raw.epsilon_hdr[2], raw.epsilon_hdr[3]};
        info->fDisplayRatioSdr = raw.display_ratio_sdr;
        info->fDisplayRatioHdr = raw.display_ratio_hdr;
        info->fBaseImageType = SkGainmapInfo::BaseImageType::kSDR;
        info->fType = SkGainmapInfo::Type::kApple;
        return true;
    }

    bool getContainerGainmapLocation(size_t* offset, size_t* size) const override {
        return skia_rs_xmp_get_container_gainmap_location(fHandle, offset, size);
    }

    const char* getExtendedXmpGuid() const override {
        return skia_rs_xmp_get_extended_guid(fHandle);
    }

private:
    SkiaRsXmp* fHandle;
};

}  // namespace

std::unique_ptr<SkXmp> SkXmp::Make(sk_sp<SkData> xmpData) {
    if (!xmpData) {
        return nullptr;
    }
    SkiaRsXmp* handle = skia_rs_xmp_make(xmpData->bytes(), xmpData->size(), nullptr, 0);
    if (!handle) {
        return nullptr;
    }
    return std::make_unique<SkXmpRustImpl>(handle);
}

std::unique_ptr<SkXmp> SkXmp::Make(sk_sp<SkData> xmpStandard, sk_sp<SkData> xmpExtended) {
    if (!xmpStandard) {
        return nullptr;
    }
    SkiaRsXmp* handle = skia_rs_xmp_make(xmpStandard->bytes(),
                                         xmpStandard->size(),
                                         xmpExtended ? xmpExtended->bytes() : nullptr,
                                         xmpExtended ? xmpExtended->size() : 0);
    if (!handle) {
        return nullptr;
    }
    return std::make_unique<SkXmpRustImpl>(handle);
}
