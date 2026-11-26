/*
 * Safe Rust bridge for:
 *   src/core/SkYUVMath.cpp
 *   src/core/SkYUVAInfo.cpp
 *   src/core/SkYUVAPixmaps.cpp
 * (Layer 2.1 skia-rs-color).
 */

#include "include/core/SkAlphaType.h"
#include "include/core/SkColor.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkSpan.h"
#include "include/core/SkYUVAInfo.h"
#include "include/core/SkYUVAPixmaps.h"
#include "include/private/SkAssert.h"
#include "include/private/SkDebug.h"
#include "src/core/SkImageInfoPriv.h"
#include "src/core/SkRectMemcpy.h"
#include "src/core/SkYUVAInfoLocation.h"
#include "src/core/SkYUVMath.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <tuple>
#include <utility>

static_assert(sizeof(SkISize) == 8);
static_assert(sizeof(SkYUVAInfo::YUVALocation) == 8);
static_assert(sizeof(SkYUVAInfo) == 32);

struct SkiaRsValidatedYuvaPixmapInfo {
    int32_t num_planes;
    int32_t data_type;
    SkISize plane_dims[SkYUVAInfo::kMaxPlanes];
    int32_t color_types[SkYUVAInfo::kMaxPlanes];
    size_t row_bytes[SkYUVAInfo::kMaxPlanes];
};

extern "C" {
// SkYUVMath
void skia_rs_yuv_color_matrix_rgb2yuv(int cs, float* m, bool high_precision);
void skia_rs_yuv_color_matrix_yuv2rgb(int cs, float* m, bool high_precision);
bool skia_rs_yuv_color_space_is_limited_range(int cs);

// SkYUVAInfo
void skia_rs_yuva_subsampling_factors(int subsampling, int* out_x, int* out_y);
void skia_rs_yuva_plane_subsampling_factors(int plane_config, int subsampling, int plane_idx,
                                            int* out_x, int* out_y);
int skia_rs_yuva_plane_dimensions(SkISize dims, int plane_config, int subsampling, int origin,
                                  SkISize* out_dims);
void skia_rs_yuva_get_locations(int plane_config, const uint32_t* plane_channel_flags,
                                SkYUVAInfo::YUVALocation* out_locs);
bool skia_rs_yuva_has_alpha(int plane_config);
void skia_rs_yuva_info_init(SkISize dims, int plane_config, int subsampling, int yuv_cs,
                            int origin, int siting_x, int siting_y, SkYUVAInfo* out);
size_t skia_rs_yuva_info_compute_total_bytes(const SkYUVAInfo* info, const size_t* row_bytes,
                                             size_t* plane_sizes);
bool skia_rs_yuva_info_equals(const SkYUVAInfo* a, const SkYUVAInfo* b);

// SkYUVAPixmaps
int skia_rs_yuva_supported_data_type_bit_index(int data_type, int num_channels);
void skia_rs_yuva_num_channels_and_data_type(int ct, int* out_channels, int* out_dt);
bool skia_rs_yuva_validate_pixmap_info(const SkYUVAInfo* info, const int* color_types,
                                       const size_t* row_bytes,
                                       SkiaRsValidatedYuvaPixmapInfo* out);
bool skia_rs_yuva_validate_pixmap_info_from_dt(const SkYUVAInfo* info, int data_type,
                                               const size_t* row_bytes, bool default_red,
                                               SkiaRsValidatedYuvaPixmapInfo* out);
bool skia_rs_yuva_compute_plane_offsets(const SkYUVAInfo* info, const size_t* row_bytes,
                                        size_t* out_offsets, int* out_num_planes);
int skia_rs_yuva_recommended_rgba_color_type(int data_type);
}  // extern "C"

// ============================================================================
// SkYUVMath.cpp
// ============================================================================

#if defined(SK_YUV_COLOR_SPACE_HIGH_PRECISION)
static constexpr bool kYUVHighPrecision = true;
#else
static constexpr bool kYUVHighPrecision = false;
#endif

void SkColorMatrix_RGB2YUV(SkYUVColorSpace cs, SkSpan<float, 20> m) {
    skia_rs_yuv_color_matrix_rgb2yuv(static_cast<int>(cs), m.data(), kYUVHighPrecision);
}

void SkColorMatrix_YUV2RGB(SkYUVColorSpace cs, SkSpan<float, 20> m) {
    skia_rs_yuv_color_matrix_yuv2rgb(static_cast<int>(cs), m.data(), kYUVHighPrecision);
}

bool SkYUVColorSpaceIsLimitedRange(SkYUVColorSpace cs) {
    return skia_rs_yuv_color_space_is_limited_range(static_cast<int>(cs));
}

static void dump_yuv_matrix(const float m[20], const char* name, int bits,
                            const char* range_name, bool rgb2yuv) {
    const char* dirname = rgb2yuv ? "rgb_to_yuv" : "yuv_to_rgb";
    SkDebugf("constexpr float %s_%dbit_%s_%s[] = {\n", name, bits, range_name, dirname);
    SkDebugf("#if defined(SK_YUV_COLOR_SPACE_HIGH_PRECISION)\n");
    for (int i = 0; i < 4; ++i) {
        SkDebugf(" ");
        for (int j = 0; j < 5; ++j) {
            SkDebugf(" %16af,", m[i * 5 + j]);
        }
        SkDebugf("\n");
    }
    SkDebugf("#else\n");
    for (int i = 0; i < 4; ++i) {
        SkDebugf(" ");
        for (int j = 0; j < 5; ++j) {
            SkDebugf(" %16.6ff,", m[i * 5 + j]);
        }
        SkDebugf("\n");
    }
    SkDebugf("#endif\n");
    SkDebugf("};\n");
}

void SkColorMatrix_DumpYUVMatrixTables() {
    struct Entry {
        const char* name;
        int bits;
        SkYUVColorSpace full_cs;
        SkYUVColorSpace limited_cs;
    };
    static constexpr Entry kEntries[] = {
        {"Rec601",    8, kRec601_8bit_Full_SkYUVColorSpace,    kRec601_8bit_Limited_SkYUVColorSpace},
        {"Rec709",    8, kRec709_8bit_Full_SkYUVColorSpace,    kRec709_8bit_Limited_SkYUVColorSpace},
        {"BT2020",    8, kBT2020_8bit_Full_SkYUVColorSpace,    kBT2020_8bit_Limited_SkYUVColorSpace},
        {"BT2020",   10, kBT2020_10bit_Full_SkYUVColorSpace,   kBT2020_10bit_Limited_SkYUVColorSpace},
        {"BT2020",   12, kBT2020_12bit_Full_SkYUVColorSpace,   kBT2020_12bit_Limited_SkYUVColorSpace},
        {"BT2020",   16, kBT2020_16bit_Full_SkYUVColorSpace,   kBT2020_16bit_Limited_SkYUVColorSpace},
        {"FCC",       8, kFCC_8bit_Full_SkYUVColorSpace,       kFCC_8bit_Limited_SkYUVColorSpace},
        {"SMPTE240",  8, kSMPTE240_8bit_Full_SkYUVColorSpace,  kSMPTE240_8bit_Limited_SkYUVColorSpace},
        {"YDZDX",     8, kYDZDX_8bit_Full_SkYUVColorSpace,     kYDZDX_8bit_Limited_SkYUVColorSpace},
        {"GBR",       8, kGBR_8bit_Full_SkYUVColorSpace,       kGBR_8bit_Limited_SkYUVColorSpace},
        {"YCgCo",     8, kYCgCo_8bit_Full_SkYUVColorSpace,     kYCgCo_8bit_Limited_SkYUVColorSpace},
        {"YCgCo",    10, kYCgCo_10bit_Full_SkYUVColorSpace,    kYCgCo_10bit_Limited_SkYUVColorSpace},
        {"YCgCo",    12, kYCgCo_12bit_Full_SkYUVColorSpace,    kYCgCo_12bit_Limited_SkYUVColorSpace},
        {"YCgCo",    16, kYCgCo_16bit_Full_SkYUVColorSpace,    kYCgCo_16bit_Limited_SkYUVColorSpace},
    };
    for (const auto& e : kEntries) {
        float m[20], im[20];
        skia_rs_yuv_color_matrix_rgb2yuv(static_cast<int>(e.full_cs), m, true);
        dump_yuv_matrix(m, e.name, e.bits, "full", true);
        skia_rs_yuv_color_matrix_yuv2rgb(static_cast<int>(e.full_cs), im, true);
        dump_yuv_matrix(im, e.name, e.bits, "full", false);

        skia_rs_yuv_color_matrix_rgb2yuv(static_cast<int>(e.limited_cs), m, true);
        dump_yuv_matrix(m, e.name, e.bits, "limited", true);
        skia_rs_yuv_color_matrix_yuv2rgb(static_cast<int>(e.limited_cs), im, true);
        dump_yuv_matrix(im, e.name, e.bits, "limited", false);
    }
}

// ============================================================================
// SkYUVAInfo.cpp
// ============================================================================

std::tuple<int, int> SkYUVAInfo::SubsamplingFactors(Subsampling subsampling) {
    int x = 0, y = 0;
    skia_rs_yuva_subsampling_factors(static_cast<int>(subsampling), &x, &y);
    return {x, y};
}

std::tuple<int, int> SkYUVAInfo::PlaneSubsamplingFactors(PlaneConfig planeConfig,
                                                         Subsampling subsampling,
                                                         int planeIdx) {
    int x = 0, y = 0;
    skia_rs_yuva_plane_subsampling_factors(
            static_cast<int>(planeConfig), static_cast<int>(subsampling), planeIdx, &x, &y);
    return {x, y};
}

int SkYUVAInfo::PlaneDimensions(SkISize imageDimensions,
                                PlaneConfig planeConfig,
                                Subsampling subsampling,
                                SkEncodedOrigin origin,
                                SkISize planeDimensions[SkYUVAInfo::kMaxPlanes]) {
    return skia_rs_yuva_plane_dimensions(imageDimensions,
                                         static_cast<int>(planeConfig),
                                         static_cast<int>(subsampling),
                                         static_cast<int>(origin),
                                         planeDimensions);
}

SkYUVAInfo::YUVALocations SkYUVAInfo::GetYUVALocations(PlaneConfig config,
                                                       const uint32_t* planeChannelFlags) {
    YUVALocations locs;
    skia_rs_yuva_get_locations(static_cast<int>(config), planeChannelFlags, locs.data());
    return locs;
}

bool SkYUVAInfo::HasAlpha(PlaneConfig planeConfig) {
    return skia_rs_yuva_has_alpha(static_cast<int>(planeConfig));
}

SkYUVAInfo::SkYUVAInfo(SkISize dimensions,
                       PlaneConfig planeConfig,
                       Subsampling subsampling,
                       SkYUVColorSpace yuvColorSpace,
                       SkEncodedOrigin origin,
                       Siting sitingX,
                       Siting sitingY) {
    skia_rs_yuva_info_init(dimensions,
                           static_cast<int>(planeConfig),
                           static_cast<int>(subsampling),
                           static_cast<int>(yuvColorSpace),
                           static_cast<int>(origin),
                           static_cast<int>(sitingX),
                           static_cast<int>(sitingY),
                           this);
}

size_t SkYUVAInfo::computeTotalBytes(const size_t rowBytes[kMaxPlanes],
                                     size_t planeSizes[kMaxPlanes]) const {
    return skia_rs_yuva_info_compute_total_bytes(this, rowBytes, planeSizes);
}

SkYUVAInfo::YUVALocations SkYUVAInfo::toYUVALocations(const uint32_t* channelFlags) const {
    return GetYUVALocations(fPlaneConfig, channelFlags);
}

SkYUVAInfo SkYUVAInfo::makeSubsampling(SkYUVAInfo::Subsampling subsampling) const {
    return {fDimensions, fPlaneConfig, subsampling, fYUVColorSpace, fOrigin, fSitingX, fSitingY};
}

SkYUVAInfo SkYUVAInfo::makeDimensions(SkISize dimensions) const {
    return {dimensions, fPlaneConfig, fSubsampling, fYUVColorSpace, fOrigin, fSitingX, fSitingY};
}

bool SkYUVAInfo::operator==(const SkYUVAInfo& that) const {
    return skia_rs_yuva_info_equals(this, &that);
}

// ============================================================================
// SkYUVAPixmaps.cpp
// ============================================================================

void SkYUVAPixmapInfo::SupportedDataTypes::enableDataType(DataType type, int numChannels) {
    int idx = skia_rs_yuva_supported_data_type_bit_index(static_cast<int>(type), numChannels);
    if (idx >= 0) {
        fDataTypeSupport[static_cast<size_t>(idx)] = true;
    }
}

std::tuple<int, SkYUVAPixmapInfo::DataType> SkYUVAPixmapInfo::NumChannelsAndDataType(
        SkColorType ct) {
    int numChannels = 0;
    int dt = 0;
    skia_rs_yuva_num_channels_and_data_type(static_cast<int>(ct), &numChannels, &dt);
    return {numChannels, static_cast<DataType>(dt)};
}

SkYUVAPixmapInfo::SkYUVAPixmapInfo(const SkYUVAInfo& yuvaInfo,
                                   const SkColorType colorTypes[kMaxPlanes],
                                   const size_t rowBytes[kMaxPlanes]) {
    int cts[kMaxPlanes] = {};
    int numPlanes = yuvaInfo.numPlanes();
    for (int i = 0; i < numPlanes; ++i) {
        cts[i] = static_cast<int>(colorTypes[i]);
    }
    SkiaRsValidatedYuvaPixmapInfo validated = {};
    if (!skia_rs_yuva_validate_pixmap_info(&yuvaInfo, cts, rowBytes, &validated)) {
        *this = {};
        SkASSERT(!this->isValid());
        return;
    }
    fYUVAInfo = yuvaInfo;
    fDataType = static_cast<DataType>(validated.data_type);
    for (int i = 0; i < validated.num_planes; ++i) {
        fRowBytes[i] = validated.row_bytes[i];
        fPlaneInfos[i] = SkImageInfo::Make(validated.plane_dims[i],
                                           static_cast<SkColorType>(validated.color_types[i]),
                                           kUnpremul_SkAlphaType);
    }
    SkASSERT(this->isValid());
}

SkYUVAPixmapInfo::SkYUVAPixmapInfo(const SkYUVAInfo& yuvaInfo,
                                   DataType dataType,
                                   const size_t rowBytes[kMaxPlanes]) {
#if defined(SK_YUVA_DEFAULT_RED)
    constexpr bool kDefaultRed = true;
#else
    constexpr bool kDefaultRed = false;
#endif
    SkiaRsValidatedYuvaPixmapInfo validated = {};
    if (!skia_rs_yuva_validate_pixmap_info_from_dt(
                &yuvaInfo, static_cast<int>(dataType), rowBytes, kDefaultRed, &validated)) {
        *this = {};
        SkASSERT(!this->isValid());
        return;
    }
    fYUVAInfo = yuvaInfo;
    fDataType = static_cast<DataType>(validated.data_type);
    for (int i = 0; i < validated.num_planes; ++i) {
        fRowBytes[i] = validated.row_bytes[i];
        fPlaneInfos[i] = SkImageInfo::Make(validated.plane_dims[i],
                                           static_cast<SkColorType>(validated.color_types[i]),
                                           kUnpremul_SkAlphaType);
    }
    SkASSERT(this->isValid());
}

bool SkYUVAPixmapInfo::operator==(const SkYUVAPixmapInfo& that) const {
    bool result = fYUVAInfo == that.fYUVAInfo && fPlaneInfos == that.fPlaneInfos &&
                  fRowBytes == that.fRowBytes;
    SkASSERT(!result || fDataType == that.fDataType);
    return result;
}

size_t SkYUVAPixmapInfo::computeTotalBytes(size_t planeSizes[kMaxPlanes]) const {
    if (!this->isValid()) {
        if (planeSizes) {
            std::fill_n(planeSizes, kMaxPlanes, 0);
        }
        return 0;
    }
    return fYUVAInfo.computeTotalBytes(fRowBytes.data(), planeSizes);
}

bool SkYUVAPixmapInfo::initPixmapsFromSingleAllocation(void* memory,
                                                       SkPixmap pixmaps[kMaxPlanes]) const {
    if (!this->isValid()) {
        return false;
    }
    SkASSERT(pixmaps);
    size_t offsets[kMaxPlanes] = {};
    int n = 0;
    if (!skia_rs_yuva_compute_plane_offsets(&fYUVAInfo, fRowBytes.data(), offsets, &n)) {
        return false;
    }
    char* base = static_cast<char*>(memory);
    for (int i = 0; i < n; ++i) {
        SkASSERT(fPlaneInfos[i].validRowBytes(fRowBytes[i]));
        pixmaps[i].reset(fPlaneInfos[i], base + offsets[i], fRowBytes[i]);
    }
    for (int i = n; i < kMaxPlanes; ++i) {
        pixmaps[i] = {};
    }
    return true;
}

bool SkYUVAPixmapInfo::isSupported(const SupportedDataTypes& supportedDataTypes) const {
    if (!this->isValid()) {
        return false;
    }
    return supportedDataTypes.supported(fYUVAInfo.planeConfig(), fDataType);
}

SkColorType SkYUVAPixmaps::RecommendedRGBAColorType(DataType dataType) {
    return static_cast<SkColorType>(
            skia_rs_yuva_recommended_rgba_color_type(static_cast<int>(dataType)));
}

SkYUVAPixmaps SkYUVAPixmaps::Allocate(const SkYUVAPixmapInfo& yuvaPixmapInfo) {
    if (!yuvaPixmapInfo.isValid()) {
        return {};
    }
    return SkYUVAPixmaps(yuvaPixmapInfo,
                         SkData::MakeUninitialized(yuvaPixmapInfo.computeTotalBytes()));
}

SkYUVAPixmaps SkYUVAPixmaps::FromData(const SkYUVAPixmapInfo& yuvaPixmapInfo, sk_sp<SkData> data) {
    if (!yuvaPixmapInfo.isValid()) {
        return {};
    }
    if (yuvaPixmapInfo.computeTotalBytes() > data->size()) {
        return {};
    }
    return SkYUVAPixmaps(yuvaPixmapInfo, std::move(data));
}

SkYUVAPixmaps SkYUVAPixmaps::MakeCopy(const SkYUVAPixmaps& src) {
    if (!src.isValid()) {
        return {};
    }
    SkYUVAPixmaps result = Allocate(src.pixmapsInfo());
    int n = result.numPlanes();
    for (int i = 0; i < n; ++i) {
        const SkPixmap& s = src.plane(i);
        const SkPixmap& d = result.plane(i);
        SkRectMemcpy(d.writable_addr(),
                     d.rowBytes(),
                     s.addr(),
                     s.rowBytes(),
                     s.info().minRowBytes(),
                     s.height());
    }
    return result;
}

SkYUVAPixmaps SkYUVAPixmaps::FromExternalMemory(const SkYUVAPixmapInfo& yuvaPixmapInfo,
                                                void* memory) {
    if (!yuvaPixmapInfo.isValid()) {
        return {};
    }
    SkPixmap pixmaps[kMaxPlanes];
    yuvaPixmapInfo.initPixmapsFromSingleAllocation(memory, pixmaps);
    return SkYUVAPixmaps(yuvaPixmapInfo.yuvaInfo(), yuvaPixmapInfo.dataType(), pixmaps);
}

SkYUVAPixmaps SkYUVAPixmaps::FromExternalPixmaps(const SkYUVAInfo& yuvaInfo,
                                                 const SkPixmap pixmaps[kMaxPlanes]) {
    SkColorType colorTypes[kMaxPlanes] = {};
    size_t rowBytes[kMaxPlanes] = {};
    int numPlanes = yuvaInfo.numPlanes();
    for (int i = 0; i < numPlanes; ++i) {
        colorTypes[i] = pixmaps[i].colorType();
        rowBytes[i] = pixmaps[i].rowBytes();
    }
    SkYUVAPixmapInfo yuvaPixmapInfo(yuvaInfo, colorTypes, rowBytes);
    if (!yuvaPixmapInfo.isValid()) {
        return {};
    }
    return SkYUVAPixmaps(yuvaInfo, yuvaPixmapInfo.dataType(), pixmaps);
}

SkYUVAPixmaps::SkYUVAPixmaps(const SkYUVAPixmapInfo& yuvaPixmapInfo, sk_sp<SkData> data)
        : fData(std::move(data))
        , fYUVAInfo(yuvaPixmapInfo.yuvaInfo())
        , fDataType(yuvaPixmapInfo.dataType()) {
    SkASSERT(yuvaPixmapInfo.isValid());
    SkASSERT(yuvaPixmapInfo.computeTotalBytes() <= fData->size());
    SkAssertResult(
            yuvaPixmapInfo.initPixmapsFromSingleAllocation(fData->writable_data(), fPlanes.data()));
}

SkYUVAPixmaps::SkYUVAPixmaps(const SkYUVAInfo& yuvaInfo,
                             DataType dataType,
                             const SkPixmap pixmaps[kMaxPlanes])
        : fYUVAInfo(yuvaInfo), fDataType(dataType) {
    std::copy_n(pixmaps, yuvaInfo.numPlanes(), fPlanes.data());
}

SkYUVAPixmapInfo SkYUVAPixmaps::pixmapsInfo() const {
    if (!this->isValid()) {
        return {};
    }
    SkColorType colorTypes[kMaxPlanes] = {};
    size_t rowBytes[kMaxPlanes] = {};
    int numPlanes = this->numPlanes();
    for (int i = 0; i < numPlanes; ++i) {
        colorTypes[i] = fPlanes[i].colorType();
        rowBytes[i] = fPlanes[i].rowBytes();
    }
    return {fYUVAInfo, colorTypes, rowBytes};
}

SkYUVAInfo::YUVALocations SkYUVAPixmaps::toYUVALocations() const {
    uint32_t channelFlags[] = {SkColorTypeChannelFlags(fPlanes[0].colorType()),
                               SkColorTypeChannelFlags(fPlanes[1].colorType()),
                               SkColorTypeChannelFlags(fPlanes[2].colorType()),
                               SkColorTypeChannelFlags(fPlanes[3].colorType())};
    auto result = fYUVAInfo.toYUVALocations(channelFlags);
    SkDEBUGCODE(int numPlanes;)
    SkASSERT(SkYUVAInfo::YUVALocation::AreValidLocations(result, &numPlanes));
    SkASSERT(numPlanes == this->numPlanes());
    return result;
}
