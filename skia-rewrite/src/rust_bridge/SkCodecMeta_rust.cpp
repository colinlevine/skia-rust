/*
 * Safe Rust bridge for:
 *   src/codec/SkTiffUtility.cpp
 *   src/codec/SkExif.cpp
 *   src/codec/SkParseEncodedOrigin.cpp
 *   src/codec/SkCodecColorProfile.cpp
 *   src/codec/SkGainmapInfo.cpp
 * (Phase 2.2 skia-rs-codec-meta).
 */

#include "include/codec/SkEncodedOrigin.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/core/SkRefCnt.h"
#include "include/private/SkAssert.h"
#include "include/private/SkExif.h"
#include "include/private/SkGainmapInfo.h"
#include "include/private/chromium/SkCodecsICCProfileChromium.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkCodecPriv.h"
#include "src/codec/SkParseEncodedOrigin.h"
#include "src/codec/SkTiffUtility.h"

#if defined(SK_CODEC_COLOR_PROFILE_PARSE_WITH_RUST)
#include "src/codec/SkCodecColorProfileRust.h"
#endif

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <utility>

namespace {

struct SkiaRsMetaBytes {
    uint8_t* ptr;
    size_t len;
    size_t cap;
};

struct SkiaRsExifMetadata {
    bool has_origin;
    uint32_t origin;
    bool has_hdr_headroom;
    float hdr_headroom;
    bool has_resolution_unit;
    uint16_t resolution_unit;
    bool has_x_resolution;
    float x_resolution;
    bool has_y_resolution;
    float y_resolution;
    bool has_pixel_x_dimension;
    uint32_t pixel_x_dimension;
    bool has_pixel_y_dimension;
    uint32_t pixel_y_dimension;
};

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
void skia_rs_codec_meta_free_bytes(SkiaRsMetaBytes buf);

// TIFF
bool skia_rs_tiff_is_valid_type(uint16_t ty);
size_t skia_rs_tiff_bytes_for_type(uint16_t ty);
bool skia_rs_tiff_parse_header(const uint8_t* data,
                               size_t len,
                               bool* out_little_endian,
                               uint32_t* out_ifd_offset);
bool skia_rs_tiff_validate_ifd(const uint8_t* data,
                               size_t len,
                               bool little_endian,
                               uint32_t ifd_offset,
                               bool allow_truncated,
                               uint16_t* out_num_entries,
                               uint32_t* out_next_ifd_offset);
uint16_t skia_rs_tiff_get_entry_tag(const uint8_t* data,
                                    size_t len,
                                    bool little_endian,
                                    uint32_t ifd_offset,
                                    uint16_t num_entries,
                                    uint32_t next_ifd_offset,
                                    uint16_t entry_index);
bool skia_rs_tiff_get_entry_raw_data(const uint8_t* data,
                                     size_t len,
                                     bool little_endian,
                                     uint32_t ifd_offset,
                                     uint16_t num_entries,
                                     uint32_t next_ifd_offset,
                                     uint16_t entry_index,
                                     uint16_t* out_tag,
                                     uint16_t* out_type,
                                     uint32_t* out_count,
                                     size_t* out_data_offset,
                                     size_t* out_data_size);
bool skia_rs_tiff_get_entry_values(const uint8_t* data,
                                   size_t len,
                                   bool little_endian,
                                   uint32_t ifd_offset,
                                   uint16_t num_entries,
                                   uint32_t next_ifd_offset,
                                   uint16_t entry_index,
                                   uint16_t expected_type,
                                   uint32_t count,
                                   void* out_values);

// EXIF & EncodedOrigin
void skia_rs_exif_parse(const uint8_t* data, size_t len, SkiaRsExifMetadata* inout_meta);
bool skia_rs_exif_write(const SkiaRsExifMetadata* meta, SkiaRsMetaBytes* out_bytes);
bool skia_rs_parse_encoded_origin(const uint8_t* data, size_t len, uint32_t* out_origin);

// ColorProfile
int32_t skia_rs_color_profile_data_space(uint32_t data_color_space);
bool skia_rs_color_profile_cicp_android(uint8_t color_primaries,
                                        uint8_t transfer_characteristics,
                                        uint8_t matrix_coefficients,
                                        uint8_t full_range_flag,
                                        skcms_TransferFunction* out_trfn,
                                        skcms_Matrix3x3* out_matrix);

// GainmapInfo
bool skia_rs_gainmap_is_ultra_hdr_v1_compatible(int32_t base_image_type, bool has_math_cs);
bool skia_rs_gainmap_parse_version(const uint8_t* data, size_t len);
void skia_rs_gainmap_serialize_version(uint8_t* out4);
bool skia_rs_gainmap_parse(const uint8_t* data, size_t len, SkiaRsGainmapInfo* out_info);
void skia_rs_gainmap_serialize(const SkiaRsGainmapInfo* info, SkiaRsMetaBytes* out_bytes);
}  // extern "C"

sk_sp<SkData> take_meta_bytes(SkiaRsMetaBytes buf) {
    if (!buf.ptr) {
        return SkData::MakeEmpty();
    }
    sk_sp<SkData> data = SkData::MakeWithCopy(buf.ptr, buf.len);
    skia_rs_codec_meta_free_bytes(buf);
    return data;
}

}  // namespace

// ============================================================================
// SkTiffUtility.cpp
// ============================================================================

namespace SkTiff {

bool ImageFileDirectory::IsValidType(uint16_t type) {
    return skia_rs_tiff_is_valid_type(type);
}

size_t ImageFileDirectory::BytesForType(uint16_t type) {
    return skia_rs_tiff_bytes_for_type(type);
}

bool ImageFileDirectory::ParseHeader(const SkData* data,
                                     bool* outLittleEndian,
                                     uint32_t* outIfdOffset) {
    if (!data) {
        return false;
    }
    return skia_rs_tiff_parse_header(data->bytes(), data->size(), outLittleEndian, outIfdOffset);
}

std::unique_ptr<ImageFileDirectory> ImageFileDirectory::MakeFromOffset(sk_sp<SkData> data,
                                                                       bool littleEndian,
                                                                       uint32_t ifdOffset,
                                                                       bool allowTruncated) {
    if (!data) {
        return nullptr;
    }
    uint16_t numEntries = 0;
    uint32_t nextOffset = 0;
    if (!skia_rs_tiff_validate_ifd(data->bytes(),
                                   data->size(),
                                   littleEndian,
                                   ifdOffset,
                                   allowTruncated,
                                   &numEntries,
                                   &nextOffset)) {
        return nullptr;
    }
    return std::unique_ptr<ImageFileDirectory>(new ImageFileDirectory(
            std::move(data), littleEndian, ifdOffset, numEntries, nextOffset));
}

ImageFileDirectory::ImageFileDirectory(sk_sp<SkData> data,
                                       bool littleEndian,
                                       uint32_t offset,
                                       uint16_t numEntries,
                                       uint32_t nextIfdOffset)
        : fData(std::move(data))
        , fLittleEndian(littleEndian)
        , fOffset(offset)
        , fNumEntries(numEntries)
        , fNextIfdOffset(nextIfdOffset) {}

uint16_t ImageFileDirectory::getEntryTag(uint16_t entryIndex) const {
    return skia_rs_tiff_get_entry_tag(fData->bytes(),
                                      fData->size(),
                                      fLittleEndian,
                                      fOffset,
                                      fNumEntries,
                                      fNextIfdOffset,
                                      entryIndex);
}

bool ImageFileDirectory::getEntryRawData(uint16_t entryIndex,
                                         uint16_t* outTag,
                                         uint16_t* outType,
                                         uint32_t* outCount,
                                         const uint8_t** outData,
                                         size_t* outDataSize) const {
    size_t dataOffset = 0;
    size_t dataSize = 0;
    if (!skia_rs_tiff_get_entry_raw_data(fData->bytes(),
                                         fData->size(),
                                         fLittleEndian,
                                         fOffset,
                                         fNumEntries,
                                         fNextIfdOffset,
                                         entryIndex,
                                         outTag,
                                         outType,
                                         outCount,
                                         &dataOffset,
                                         &dataSize)) {
        return false;
    }
    if (outData) {
        *outData = fData->bytes() + dataOffset;
    }
    if (outDataSize) {
        *outDataSize = dataSize;
    }
    return true;
}

sk_sp<SkData> ImageFileDirectory::getEntryUndefinedData(uint16_t entryIndex) const {
    uint16_t type = 0;
    uint32_t count = 0;
    const uint8_t* data = nullptr;
    size_t size = 0;
    if (!getEntryRawData(entryIndex, nullptr, &type, &count, &data, &size)) {
        return nullptr;
    }
    if (type != kTypeUndefined) {
        return nullptr;
    }
    return SkData::MakeSubset(fData.get(), data - fData->bytes(), size);
}

bool ImageFileDirectory::getEntryValuesGeneric(uint16_t entryIndex,
                                               uint16_t type,
                                               uint32_t count,
                                               void* values) const {
    return skia_rs_tiff_get_entry_values(fData->bytes(),
                                         fData->size(),
                                         fLittleEndian,
                                         fOffset,
                                         fNumEntries,
                                         fNextIfdOffset,
                                         entryIndex,
                                         type,
                                         count,
                                         values);
}

}  // namespace SkTiff

// ============================================================================
// SkExif.cpp
// ============================================================================

namespace SkExif {

static SkiaRsExifMetadata to_ffi(const Metadata& m) {
    SkiaRsExifMetadata out{};
    out.has_origin = m.fOrigin.has_value();
    out.origin = m.fOrigin.has_value() ? static_cast<uint32_t>(m.fOrigin.value()) : 0;
    out.has_hdr_headroom = m.fHdrHeadroom.has_value();
    out.hdr_headroom = m.fHdrHeadroom.value_or(0.f);
    out.has_resolution_unit = m.fResolutionUnit.has_value();
    out.resolution_unit = m.fResolutionUnit.value_or(0);
    out.has_x_resolution = m.fXResolution.has_value();
    out.x_resolution = m.fXResolution.value_or(0.f);
    out.has_y_resolution = m.fYResolution.has_value();
    out.y_resolution = m.fYResolution.value_or(0.f);
    out.has_pixel_x_dimension = m.fPixelXDimension.has_value();
    out.pixel_x_dimension = m.fPixelXDimension.value_or(0);
    out.has_pixel_y_dimension = m.fPixelYDimension.has_value();
    out.pixel_y_dimension = m.fPixelYDimension.value_or(0);
    return out;
}

static void from_ffi(const SkiaRsExifMetadata& in, Metadata& m) {
    if (in.has_origin) {
        m.fOrigin = static_cast<SkEncodedOrigin>(in.origin);
    } else {
        m.fOrigin.reset();
    }
    if (in.has_hdr_headroom) {
        m.fHdrHeadroom = in.hdr_headroom;
    } else {
        m.fHdrHeadroom.reset();
    }
    if (in.has_resolution_unit) {
        m.fResolutionUnit = in.resolution_unit;
    } else {
        m.fResolutionUnit.reset();
    }
    if (in.has_x_resolution) {
        m.fXResolution = in.x_resolution;
    } else {
        m.fXResolution.reset();
    }
    if (in.has_y_resolution) {
        m.fYResolution = in.y_resolution;
    } else {
        m.fYResolution.reset();
    }
    if (in.has_pixel_x_dimension) {
        m.fPixelXDimension = in.pixel_x_dimension;
    } else {
        m.fPixelXDimension.reset();
    }
    if (in.has_pixel_y_dimension) {
        m.fPixelYDimension = in.pixel_y_dimension;
    } else {
        m.fPixelYDimension.reset();
    }
}

void Parse(Metadata& metadata, const SkData* data) {
    if (!data) {
        return;
    }
    SkiaRsExifMetadata raw = to_ffi(metadata);
    skia_rs_exif_parse(data->bytes(), data->size(), &raw);
    from_ffi(raw, metadata);
}

sk_sp<SkData> WriteExif(Metadata& metadata) {
    SkiaRsExifMetadata raw = to_ffi(metadata);
    SkiaRsMetaBytes bytes{};
    if (!skia_rs_exif_write(&raw, &bytes)) {
        return nullptr;
    }
    return take_meta_bytes(bytes);
}

}  // namespace SkExif

// ============================================================================
// SkParseEncodedOrigin.cpp
// ============================================================================

bool SkParseEncodedOrigin(const void* data, size_t data_length, SkEncodedOrigin* orientation) {
    SkASSERT(orientation);
    uint32_t origin = 0;
    if (skia_rs_parse_encoded_origin(static_cast<const uint8_t*>(data), data_length, &origin)) {
        *orientation = static_cast<SkEncodedOrigin>(origin);
        return true;
    }
    return false;
}

// ============================================================================
// SkCodecColorProfile.cpp
// ============================================================================

namespace SkCodecs {

std::unique_ptr<ColorProfile> ColorProfile::MakeICCProfileWithSkCMS(sk_sp<const SkData> data) {
    if (data) {
        skcms_ICCProfile profile;
        if (skcms_Parse(data->data(), data->size(), &profile)) {
            return std::unique_ptr<ColorProfile>(new ColorProfile(profile, std::move(data)));
        }
    }
    return nullptr;
}

std::unique_ptr<ColorProfile> ColorProfile::MakeICCProfile(sk_sp<const SkData> data) {
#if defined(SK_CODEC_COLOR_PROFILE_PARSE_WITH_RUST)
    return MakeICCProfileWithRust(data);
#else
    return MakeICCProfileWithSkCMS(data);
#endif
}

std::unique_ptr<ColorProfile> ColorProfile::Make(sk_sp<SkColorSpace> cs) {
    if (!cs) {
        return nullptr;
    }
    skcms_ICCProfile profile;
    cs->toProfile(&profile);
    return std::unique_ptr<ColorProfile>(new ColorProfile(profile));
}

std::unique_ptr<ColorProfile> ColorProfile::Make(const skcms_TransferFunction& trfn,
                                                 const skcms_Matrix3x3& toXYZD50) {
    skcms_ICCProfile profile;
    skcms_Init(&profile);
    skcms_SetTransferFunction(&profile, &trfn);
    skcms_SetXYZD50(&profile, &toXYZD50);
    return std::unique_ptr<ColorProfile>(new ColorProfile(profile));
}

std::unique_ptr<ColorProfile> ColorProfile::MakeCICP(uint8_t color_primaries,
                                                     uint8_t transfer_characteristics,
                                                     uint8_t matrix_coefficients,
                                                     uint8_t full_range_flag) {
    skcms_ICCProfile profile;
    skcms_Init(&profile);
    profile.has_CICP = true;
    profile.CICP.color_primaries = color_primaries;
    profile.CICP.transfer_characteristics = transfer_characteristics;
    profile.CICP.matrix_coefficients = matrix_coefficients;
    profile.CICP.video_full_range_flag = full_range_flag;
    return std::unique_ptr<ColorProfile>(new ColorProfile(profile));
}

std::unique_ptr<ColorProfile> ColorProfile::clone() const {
    auto cloned = std::unique_ptr<ColorProfile>(new ColorProfile(fProfile, fData));
    cloned->fRetainedData = fRetainedData;
    return cloned;
}

ColorProfile::DataSpace ColorProfile::dataSpace() const {
    return static_cast<DataSpace>(skia_rs_color_profile_data_space(fProfile.data_color_space));
}

sk_sp<SkColorSpace> ColorProfile::getExactColorSpace() const {
    return SkColorSpace::Make(fProfile);
}

sk_sp<SkColorSpace> ColorProfile::getAndroidOutputColorSpace() const {
    if (fProfile.has_CICP) {
        skcms_TransferFunction trfn;
        skcms_Matrix3x3 primaries;
        if (skia_rs_color_profile_cicp_android(fProfile.CICP.color_primaries,
                                               fProfile.CICP.transfer_characteristics,
                                               fProfile.CICP.matrix_coefficients,
                                               fProfile.CICP.video_full_range_flag,
                                               &trfn,
                                               &primaries)) {
            if (auto cs = SkColorSpace::MakeRGB(trfn, primaries)) {
                return cs;
            }
        }
    }
    if (auto encodedSpace = SkColorSpace::Make(fProfile)) {
        return encodedSpace;
    }
    if (fProfile.has_toXYZD50) {
        return SkColorSpace::MakeRGB(SkNamedTransferFn::kSRGB, fProfile.toXYZD50);
    }
    return SkColorSpace::MakeSRGB();
}

ColorProfile::ColorProfile(const skcms_ICCProfile& profile, sk_sp<const SkData> data)
        : fProfile(profile), fData(std::move(data)) {}

class ICCProfileChromiumImpl final : public ICCProfileChromium {
public:
    ICCProfileChromiumImpl(std::unique_ptr<ColorProfile> colorProfile)
            : fICCProfile(std::move(colorProfile)) {}

    const skcms_ICCProfile& GetProfile() const override { return *fICCProfile->profile(); }

private:
    std::unique_ptr<ColorProfile> fICCProfile;
};

std::unique_ptr<ICCProfileChromium> ICCProfileChromium::Make(sk_sp<SkData> data) {
    auto profile = ColorProfile::MakeICCProfile(data);
    if (!profile) {
        return nullptr;
    }
    return std::make_unique<ICCProfileChromiumImpl>(std::move(profile));
}

}  // namespace SkCodecs

// ============================================================================
// SkGainmapInfo.cpp
// ============================================================================

bool SkGainmapInfo::isUltraHDRv1Compatible() const {
    return skia_rs_gainmap_is_ultra_hdr_v1_compatible(
            static_cast<int32_t>(fBaseImageType), fGainmapMathColorSpace != nullptr);
}

bool SkGainmapInfo::ParseVersion(const SkData* data) {
    if (!data) {
        return false;
    }
    return skia_rs_gainmap_parse_version(data->bytes(), data->size());
}

bool SkGainmapInfo::Parse(const SkData* data, SkGainmapInfo& info) {
    if (!data) {
        return false;
    }
    SkiaRsGainmapInfo raw{};
    if (!skia_rs_gainmap_parse(data->bytes(), data->size(), &raw)) {
        return false;
    }
    info = SkGainmapInfo();
    info.fGainmapRatioMin = {raw.gainmap_ratio_min[0],
                             raw.gainmap_ratio_min[1],
                             raw.gainmap_ratio_min[2],
                             raw.gainmap_ratio_min[3]};
    info.fGainmapRatioMax = {raw.gainmap_ratio_max[0],
                             raw.gainmap_ratio_max[1],
                             raw.gainmap_ratio_max[2],
                             raw.gainmap_ratio_max[3]};
    info.fGainmapGamma = {raw.gainmap_gamma[0],
                          raw.gainmap_gamma[1],
                          raw.gainmap_gamma[2],
                          raw.gainmap_gamma[3]};
    info.fEpsilonSdr = {
            raw.epsilon_sdr[0], raw.epsilon_sdr[1], raw.epsilon_sdr[2], raw.epsilon_sdr[3]};
    info.fEpsilonHdr = {
            raw.epsilon_hdr[0], raw.epsilon_hdr[1], raw.epsilon_hdr[2], raw.epsilon_hdr[3]};
    info.fDisplayRatioSdr = raw.display_ratio_sdr;
    info.fDisplayRatioHdr = raw.display_ratio_hdr;
    info.fBaseImageType = raw.base_image_type == 1 ? BaseImageType::kHDR : BaseImageType::kSDR;
    info.fType = raw.gainmap_type == 1 ? Type::kApple : Type::kDefault;
    if (raw.has_gainmap_math_color_space) {
        info.fGainmapMathColorSpace = SkColorSpace::MakeSRGB();
    }
    return true;
}

sk_sp<SkData> SkGainmapInfo::SerializeVersion() {
    uint8_t buf[4];
    skia_rs_gainmap_serialize_version(buf);
    return SkData::MakeWithCopy(buf, sizeof(buf));
}

sk_sp<SkData> SkGainmapInfo::serialize() const {
    SkiaRsGainmapInfo raw{};
    raw.gainmap_ratio_min[0] = fGainmapRatioMin.fR;
    raw.gainmap_ratio_min[1] = fGainmapRatioMin.fG;
    raw.gainmap_ratio_min[2] = fGainmapRatioMin.fB;
    raw.gainmap_ratio_min[3] = fGainmapRatioMin.fA;
    raw.gainmap_ratio_max[0] = fGainmapRatioMax.fR;
    raw.gainmap_ratio_max[1] = fGainmapRatioMax.fG;
    raw.gainmap_ratio_max[2] = fGainmapRatioMax.fB;
    raw.gainmap_ratio_max[3] = fGainmapRatioMax.fA;
    raw.gainmap_gamma[0] = fGainmapGamma.fR;
    raw.gainmap_gamma[1] = fGainmapGamma.fG;
    raw.gainmap_gamma[2] = fGainmapGamma.fB;
    raw.gainmap_gamma[3] = fGainmapGamma.fA;
    raw.epsilon_sdr[0] = fEpsilonSdr.fR;
    raw.epsilon_sdr[1] = fEpsilonSdr.fG;
    raw.epsilon_sdr[2] = fEpsilonSdr.fB;
    raw.epsilon_sdr[3] = fEpsilonSdr.fA;
    raw.epsilon_hdr[0] = fEpsilonHdr.fR;
    raw.epsilon_hdr[1] = fEpsilonHdr.fG;
    raw.epsilon_hdr[2] = fEpsilonHdr.fB;
    raw.epsilon_hdr[3] = fEpsilonHdr.fA;
    raw.display_ratio_sdr = fDisplayRatioSdr;
    raw.display_ratio_hdr = fDisplayRatioHdr;
    raw.base_image_type = static_cast<int32_t>(fBaseImageType);
    raw.gainmap_type = static_cast<int32_t>(fType);
    raw.has_gainmap_math_color_space = fGainmapMathColorSpace != nullptr;

    SkiaRsMetaBytes out{};
    skia_rs_gainmap_serialize(&raw, &out);
    return take_meta_bytes(out);
}
