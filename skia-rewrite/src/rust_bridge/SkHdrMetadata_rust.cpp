/*
 * Safe Rust bridge for:
 *   src/codec/SkHdrMetadata.cpp
 *   src/codec/SkHdrAgtm.cpp
 *   src/codec/SkHdrAgtmParse.cpp
 * (Phase 2.2 skia-rs-codec-meta).
 */

#include "include/core/SkBitmap.h"
#include "include/core/SkColor.h"
#include "include/core/SkColorFilter.h"
#include "include/core/SkColorSpace.h"
#include "include/core/SkData.h"
#include "include/core/SkImage.h"
#include "include/core/SkString.h"
#include "include/effects/SkRuntimeEffect.h"
#include "include/private/SkHdrMetadata.h"
#include "modules/skcms/skcms.h"
#include "src/codec/SkHdrAgtmPriv.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace {

struct SkiaRsMetaBytes {
    uint8_t* ptr;
    size_t len;
    size_t cap;
};

struct SkiaRsAgtmAlternateImage {
    float hdr_headroom;
    skhdr::AdaptiveGlobalToneMap::ComponentMixingFunction mix;
    uint32_t raw_num_control_points;
    skhdr::AdaptiveGlobalToneMap::GainCurve::ControlPoint control_points[32];
};

struct SkiaRsAgtmHatm {
    float baseline_hdr_headroom;
    SkColorSpacePrimaries primaries;
    uint32_t raw_num_alternate_images;
    SkiaRsAgtmAlternateImage alternate_images[4];
};

struct SkiaRsAgtm {
    float hdr_reference_white;
    bool has_hatm;
    SkiaRsAgtmHatm hatm;
};

struct SkiaRsColorFilterUniforms {
    float scale_factor;
    float weight[2];
    float mix_rgbx[2][4];
    float mix_mmcx[2][4];
    float curve_texcoord_y[2];
    float curve_n_cp[2];
    skcms_Matrix3x3 gain_application_to_xyzd50;
    bool has_gain_application_space;
};

static_assert(sizeof(skhdr::ContentLightLevelInformation) == 8);
static_assert(sizeof(skhdr::MasteringDisplayColorVolume) == 40);
static_assert(sizeof(skhdr::AdaptiveGlobalToneMap::GainCurve::ControlPoint) == 12);
static_assert(sizeof(skhdr::AdaptiveGlobalToneMap::ComponentMixingFunction) == 24);
static_assert(sizeof(skhdr::AgtmHelpers::Weighting) == 12);
static_assert(sizeof(SkColor4f) == 16);

extern "C" {
void skia_rs_codec_meta_free_bytes(SkiaRsMetaBytes buf);

// CLLI & MDCV
bool skia_rs_hdr_clli_parse(const uint8_t* data,
                            size_t len,
                            skhdr::ContentLightLevelInformation* out);
void skia_rs_hdr_clli_serialize(const skhdr::ContentLightLevelInformation* clli, uint8_t* out4);
bool skia_rs_hdr_clli_parse_png(const uint8_t* data,
                                size_t len,
                                skhdr::ContentLightLevelInformation* out);
void skia_rs_hdr_clli_serialize_png(const skhdr::ContentLightLevelInformation* clli, uint8_t* out8);
void skia_rs_hdr_clli_to_string(const skhdr::ContentLightLevelInformation* clli,
                                SkiaRsMetaBytes* out);
bool skia_rs_hdr_mdcv_parse(const uint8_t* data,
                            size_t len,
                            skhdr::MasteringDisplayColorVolume* out);
void skia_rs_hdr_mdcv_serialize(const skhdr::MasteringDisplayColorVolume* mdcv, uint8_t* out24);
void skia_rs_hdr_mdcv_to_string(const skhdr::MasteringDisplayColorVolume* mdcv,
                                SkiaRsMetaBytes* out);

// AGTM
void skia_rs_agtm_eval_component_mixing(
        const skhdr::AdaptiveGlobalToneMap::ComponentMixingFunction* mix,
        const float* c,
        float* out_c);
float skia_rs_agtm_eval_gain_curve(
        const skhdr::AdaptiveGlobalToneMap::GainCurve::ControlPoint* cps, size_t count, float x);
void skia_rs_agtm_eval_color_gain(const skhdr::AdaptiveGlobalToneMap::ComponentMixingFunction* mix,
                                  const skhdr::AdaptiveGlobalToneMap::GainCurve::ControlPoint* cps,
                                  size_t count,
                                  const float* c,
                                  float* out_c);
void skia_rs_agtm_populate_slope_pchip(
        skhdr::AdaptiveGlobalToneMap::GainCurve::ControlPoint* cps, size_t count);
void skia_rs_agtm_populate_rwtmo(SkiaRsAgtmHatm* inout_hatm);
void skia_rs_agtm_compute_weighting(const SkiaRsAgtmHatm* hatm,
                                    float targeted_hdr_headroom,
                                    skhdr::AgtmHelpers::Weighting* out_weighting);
void skia_rs_agtm_apply_gain(const SkiaRsAgtmHatm* hatm,
                             float* colors,
                             size_t count,
                             float targeted_hdr_headroom);
bool skia_rs_agtm_build_xym_pixels(const SkiaRsAgtmHatm* hatm,
                                   size_t* out_width,
                                   size_t* out_height,
                                   SkiaRsMetaBytes* out_pixels);
void skia_rs_agtm_compute_color_filter_uniforms(const SkiaRsAgtmHatm* hatm,
                                                float targeted_hdr_headroom,
                                                float scale_factor,
                                                SkiaRsColorFilterUniforms* out_uniforms);
bool skia_rs_agtm_populate_tone_map_params(bool has_clli,
                                           const skhdr::ContentLightLevelInformation* clli,
                                           bool has_mdcv,
                                           const skhdr::MasteringDisplayColorVolume* mdcv,
                                           bool has_agtm,
                                           const SkiaRsAgtm* agtm,
                                           bool input_is_pq_or_hlg,
                                           float input_pq_or_hlg_white,
                                           SkiaRsAgtm* out_agtm,
                                           float* out_scale_factor);
bool skia_rs_agtm_validate_agtm(const SkiaRsAgtm* agtm);
bool skia_rs_agtm_validate_hatm(const SkiaRsAgtmHatm* hatm);
bool skia_rs_agtm_parse(const uint8_t* data, size_t len, SkiaRsAgtm* out_agtm);
bool skia_rs_agtm_serialize(const SkiaRsAgtm* agtm, SkiaRsMetaBytes* out_bytes);
void skia_rs_agtm_to_string(const SkiaRsAgtm* agtm, SkiaRsMetaBytes* out_bytes);
void skia_rs_hdr_metadata_to_string(bool has_clli,
                                    const skhdr::ContentLightLevelInformation* clli,
                                    bool has_mdcv,
                                    const skhdr::MasteringDisplayColorVolume* mdcv,
                                    bool has_agtm,
                                    const SkiaRsAgtm* agtm,
                                    SkiaRsMetaBytes* out_bytes);
}  // extern "C"

sk_sp<SkData> take_meta_bytes(SkiaRsMetaBytes buf) {
    if (!buf.ptr) {
        return SkData::MakeEmpty();
    }
    sk_sp<SkData> data = SkData::MakeWithCopy(buf.ptr, buf.len);
    skia_rs_codec_meta_free_bytes(buf);
    return data;
}

SkString take_meta_string(SkiaRsMetaBytes buf) {
    if (!buf.ptr || buf.len == 0) {
        skia_rs_codec_meta_free_bytes(buf);
        return SkString();
    }
    SkString s(reinterpret_cast<const char*>(buf.ptr), buf.len);
    skia_rs_codec_meta_free_bytes(buf);
    return s;
}

SkiaRsAgtmHatm hatm_to_ffi(const skhdr::AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    SkiaRsAgtmHatm out{};
    out.baseline_hdr_headroom = hatm.fBaselineHdrHeadroom;
    out.primaries = hatm.fGainApplicationSpacePrimaries;
    out.raw_num_alternate_images = static_cast<uint32_t>(hatm.fAlternateImages.size());
    size_t altCount = std::min<size_t>(hatm.fAlternateImages.size(), 4);
    for (size_t a = 0; a < altCount; ++a) {
        const auto& src = hatm.fAlternateImages[a];
        auto& dst = out.alternate_images[a];
        dst.hdr_headroom = src.fHdrHeadroom;
        dst.mix = src.fColorGainFunction.fComponentMixing;
        const auto& cps = src.fColorGainFunction.fGainCurve.fControlPoints;
        dst.raw_num_control_points = static_cast<uint32_t>(cps.size());
        size_t cpCount = std::min<size_t>(cps.size(), 32);
        for (size_t c = 0; c < cpCount; ++c) {
            dst.control_points[c] = cps[c];
        }
    }
    return out;
}

void hatm_from_ffi(const SkiaRsAgtmHatm& in,
                   skhdr::AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    hatm.fBaselineHdrHeadroom = in.baseline_hdr_headroom;
    hatm.fGainApplicationSpacePrimaries = in.primaries;
    size_t altCount = std::min<size_t>(in.raw_num_alternate_images, 4);
    hatm.fAlternateImages.resize(altCount);
    for (size_t a = 0; a < altCount; ++a) {
        const auto& src = in.alternate_images[a];
        auto& dst = hatm.fAlternateImages[a];
        dst.fHdrHeadroom = src.hdr_headroom;
        dst.fColorGainFunction.fComponentMixing = src.mix;
        size_t cpCount = std::min<size_t>(src.raw_num_control_points, 32);
        dst.fColorGainFunction.fGainCurve.fControlPoints.assign(src.control_points,
                                                                src.control_points + cpCount);
    }
}

SkiaRsAgtm agtm_to_ffi(const skhdr::AdaptiveGlobalToneMap& agtm) {
    SkiaRsAgtm out{};
    out.hdr_reference_white = agtm.fHdrReferenceWhite;
    out.has_hatm = agtm.fHeadroomAdaptiveToneMap.has_value();
    if (out.has_hatm) {
        out.hatm = hatm_to_ffi(agtm.fHeadroomAdaptiveToneMap.value());
    }
    return out;
}

void agtm_from_ffi(const SkiaRsAgtm& in, skhdr::AdaptiveGlobalToneMap& agtm) {
    agtm.fHdrReferenceWhite = in.hdr_reference_white;
    if (in.has_hatm) {
        auto& hatm = agtm.fHeadroomAdaptiveToneMap.emplace();
        hatm_from_ffi(in.hatm, hatm);
    } else {
        agtm.fHeadroomAdaptiveToneMap.reset();
    }
}

// AGTM tone mapping shader (identical to src/codec/SkHdrAgtm.cpp).
static constexpr char gAgtmSKSL[] =
    "uniform half scale_factor;"
    "uniform shader curve_xym;"
    "uniform half weight_i;"
    "uniform half4 mix_rgbx_i;"
    "uniform half4 mix_Mmcx_i;"
    "uniform half curve_texcoord_y_i;"
    "uniform half curve_N_cp_i;"
    "uniform half weight_j;"
    "uniform half4 mix_rgbx_j;"
    "uniform half4 mix_Mmcx_j;"
    "uniform half curve_texcoord_y_j;"
    "uniform half curve_N_cp_j;"
    "half3 EvalComponentMixing(half3 color, half4 rgbx, half4 Mmcx) {"
      "half common = dot(rgbx.rgb, color) +"
                    "Mmcx[0] * max(max(color.r, color.g), color.b) +"
                    "Mmcx[1] * min(min(color.r, color.g), color.b);"
      "return Mmcx[2] * color + half3(common);"
    "}"
    "half EvalGainCurve(half x, half curve_texcoord_y, half curve_N_cp) {"
      "half c_min = 0.0;"
      "half4 xym_min = curve_xym.eval(half2(c_min + 0.5, curve_texcoord_y));"
      "if (x <= xym_min.x) {"
        "return xym_min.y;"
      "}"
      "half c_max = curve_N_cp - 1.0;"
      "half4 xym_max = curve_xym.eval(half2(c_max + 0.5, curve_texcoord_y));"
      "if (x >= xym_max.x) {"
        "return xym_max.y + log2(xym_max.x / x);"
      "}"
      "for (int step = 0; step < 5; ++step) {"
        "if (c_max - c_min <= 1.0) {"
          "break;"
        "}"
        "half c_mid = ceil(0.5 * (c_min + c_max));"
        "half4 xym_mid = curve_xym.eval(half2(c_mid + 0.5, curve_texcoord_y));"
        "if (x == xym_mid.x) {"
          "return xym_mid.y;"
        "} else if (x < xym_mid.x) {"
        "c_max = c_mid;"
          "xym_max = xym_mid;"
        "} else {"
        "c_min = c_mid;"
          "xym_min = xym_mid;"
        "}"
      "}"
      "half h = xym_max.x - xym_min.x;"
      "half mHat_min = xym_min.z * h;"
      "half mHat_max = xym_max.z * h;"
      "half c3 =  2.0 * xym_min.y + mHat_min - 2.0 * xym_max.y + mHat_max;"
      "half c2 = -3.0 * xym_min.y + 3.0 * xym_max.y - 2.0 * mHat_min - mHat_max;"
      "half c1 = mHat_min;"
      "half c0 = xym_min.y;"
      "half t = (x - xym_min.x) / h;"
      "return ((c3*t + c2)*t + c1)*t + c0;"
    "}"
    "half3 EvalColorGainFunction(half3 color,"
                                "half4 mix_rgbx, half4 mix_Mmcx,"
                                "float curve_texcoord_y, float curve_N_cp) {"
      "half3 M = EvalComponentMixing(color, mix_rgbx, mix_Mmcx);"
      "if (mix_Mmcx.b == 0.0) {"
        "return half3(EvalGainCurve(M.r, curve_texcoord_y, curve_N_cp));"
      "}"
      "return half3(EvalGainCurve(M.r, curve_texcoord_y, curve_N_cp),"
                   "EvalGainCurve(M.g, curve_texcoord_y, curve_N_cp),"
                   "EvalGainCurve(M.b, curve_texcoord_y, curve_N_cp));"
    "}"
    "half4 main(half4 color) {"
      "color.rgb *= scale_factor;"
      "if (weight_i > 0.0) {"
        "float a_inv = (color.a == 0.0) ? 1.0 : 1.0 / color.a;"
        "half3 G = half3(0.0);"
        "G += weight_i * EvalColorGainFunction(color.rgb * a_inv,"
                                              "mix_rgbx_i, mix_Mmcx_i,"
                                              "curve_texcoord_y_i, curve_N_cp_i);"
        "if (weight_j > 0.0) {"
          "G += weight_j * EvalColorGainFunction(color.rgb * a_inv,"
                                                "mix_rgbx_j, mix_Mmcx_j,"
                                                "curve_texcoord_y_j, curve_N_cp_j);"
        "}"
        "color.rgb *= exp2(G);"
      "}"
      "return color;"
    "}";

sk_sp<SkRuntimeEffect> agtm_runtime_effect() {
    auto init_lambda = []() {
        auto result = SkRuntimeEffect::MakeForColorFilter(SkString(gAgtmSKSL), {});
        SkASSERTF(result.effect, "Agtm shader log:\n%s\n", result.errorText.c_str());
        return result.effect.release();
    };
    static SkRuntimeEffect* effect = init_lambda();
    return sk_ref_sp(effect);
}

}  // namespace

namespace skhdr {

// ============================================================================
// SkHdrMetadata.cpp
// ============================================================================

SkString ContentLightLevelInformation::toString() const {
    SkiaRsMetaBytes out{};
    skia_rs_hdr_clli_to_string(this, &out);
    return take_meta_string(out);
}

bool ContentLightLevelInformation::parse(const SkData* data) {
    if (!data) {
        return false;
    }
    return skia_rs_hdr_clli_parse(data->bytes(), data->size(), this);
}

sk_sp<SkData> ContentLightLevelInformation::serialize() const {
    uint8_t buf[4];
    skia_rs_hdr_clli_serialize(this, buf);
    return SkData::MakeWithCopy(buf, sizeof(buf));
}

bool ContentLightLevelInformation::parsePngChunk(const SkData* data) {
    if (!data) {
        return false;
    }
    return skia_rs_hdr_clli_parse_png(data->bytes(), data->size(), this);
}

sk_sp<SkData> ContentLightLevelInformation::serializePngChunk() const {
    uint8_t buf[8];
    skia_rs_hdr_clli_serialize_png(this, buf);
    return SkData::MakeWithCopy(buf, sizeof(buf));
}

bool ContentLightLevelInformation::operator==(const ContentLightLevelInformation& other) const {
    return fMaxCLL == other.fMaxCLL && fMaxFALL == other.fMaxFALL;
}

SkString MasteringDisplayColorVolume::toString() const {
    SkiaRsMetaBytes out{};
    skia_rs_hdr_mdcv_to_string(this, &out);
    return take_meta_string(out);
}

bool MasteringDisplayColorVolume::parse(const SkData* data) {
    if (!data) {
        return false;
    }
    return skia_rs_hdr_mdcv_parse(data->bytes(), data->size(), this);
}

sk_sp<SkData> MasteringDisplayColorVolume::serialize() const {
    uint8_t buf[24];
    skia_rs_hdr_mdcv_serialize(this, buf);
    return SkData::MakeWithCopy(buf, sizeof(buf));
}

bool MasteringDisplayColorVolume::operator==(const MasteringDisplayColorVolume& other) const {
    return fDisplayPrimaries.fRX == other.fDisplayPrimaries.fRX &&
           fDisplayPrimaries.fRY == other.fDisplayPrimaries.fRY &&
           fDisplayPrimaries.fGX == other.fDisplayPrimaries.fGX &&
           fDisplayPrimaries.fGY == other.fDisplayPrimaries.fGY &&
           fDisplayPrimaries.fBX == other.fDisplayPrimaries.fBX &&
           fDisplayPrimaries.fBY == other.fDisplayPrimaries.fBY &&
           fDisplayPrimaries.fWX == other.fDisplayPrimaries.fWX &&
           fDisplayPrimaries.fWY == other.fDisplayPrimaries.fWY &&
           fMaximumDisplayMasteringLuminance == other.fMaximumDisplayMasteringLuminance &&
           fMinimumDisplayMasteringLuminance == other.fMinimumDisplayMasteringLuminance;
}

Metadata Metadata::MakeEmpty() { return Metadata(); }

bool Metadata::getContentLightLevelInformation(ContentLightLevelInformation* clli) const {
    if (!fContentLightLevelInformation.has_value()) {
        return false;
    }
    if (clli) {
        *clli = fContentLightLevelInformation.value();
    }
    return true;
}

bool Metadata::getMasteringDisplayColorVolume(MasteringDisplayColorVolume* mdcv) const {
    if (!fMasteringDisplayColorVolume.has_value()) {
        return false;
    }
    if (mdcv) {
        *mdcv = fMasteringDisplayColorVolume.value();
    }
    return true;
}

bool Metadata::getAdaptiveGlobalToneMap(AdaptiveGlobalToneMap* agtm) const {
    if (!fAdaptiveGlobalToneMap.has_value()) {
        return false;
    }
    if (agtm) {
        *agtm = fAdaptiveGlobalToneMap.value();
    }
    return true;
}

sk_sp<const SkData> Metadata::getSerializedAgtm() const {
    return fAdaptiveGlobalToneMap.has_value() ? fAdaptiveGlobalToneMap->serialize() : nullptr;
}

void Metadata::setMasteringDisplayColorVolume(const MasteringDisplayColorVolume& mdcv) {
    fMasteringDisplayColorVolume = mdcv;
}

void Metadata::setContentLightLevelInformation(const ContentLightLevelInformation& clli) {
    fContentLightLevelInformation = clli;
}

void Metadata::setAdaptiveGlobalToneMap(const AdaptiveGlobalToneMap& agtm) {
    fAdaptiveGlobalToneMap = agtm;
}

void Metadata::setSerializedAgtm(sk_sp<const SkData> agtm_encoded) {
    AdaptiveGlobalToneMap agtm;
    if (agtm.parse(agtm_encoded.get())) {
        fAdaptiveGlobalToneMap = agtm;
    } else {
        fAdaptiveGlobalToneMap.reset();
    }
}

SkString Metadata::toString() const {
    SkiaRsAgtm rawAgtm{};
    if (fAdaptiveGlobalToneMap.has_value()) {
        rawAgtm = agtm_to_ffi(fAdaptiveGlobalToneMap.value());
    }
    SkiaRsMetaBytes out{};
    skia_rs_hdr_metadata_to_string(
            fContentLightLevelInformation.has_value(),
            fContentLightLevelInformation.has_value() ? &fContentLightLevelInformation.value()
                                                      : nullptr,
            fMasteringDisplayColorVolume.has_value(),
            fMasteringDisplayColorVolume.has_value() ? &fMasteringDisplayColorVolume.value()
                                                     : nullptr,
            fAdaptiveGlobalToneMap.has_value(),
            fAdaptiveGlobalToneMap.has_value() ? &rawAgtm : nullptr,
            &out);
    return take_meta_string(out);
}

bool Metadata::operator==(const Metadata& other) const {
    return fContentLightLevelInformation == other.fContentLightLevelInformation &&
           fMasteringDisplayColorVolume == other.fMasteringDisplayColorVolume &&
           fAdaptiveGlobalToneMap == other.fAdaptiveGlobalToneMap;
}

sk_sp<SkColorFilter> Metadata::makeToneMapColorFilter(float targetedHdrHeadroom,
                                                      const SkColorSpace* inputColorSpace) const {
    AdaptiveGlobalToneMap agtm;
    float scaleFactor = 1.f;
    if (!AgtmHelpers::PopulateToneMapAgtmParams(*this, inputColorSpace, &agtm, &scaleFactor)) {
        return nullptr;
    }
    auto& hatm = agtm.fHeadroomAdaptiveToneMap;
    if (!hatm.has_value()) {
        return nullptr;
    }
    return AgtmHelpers::MakeColorFilter(hatm.value(), targetedHdrHeadroom, scaleFactor);
}

// ============================================================================
// SkHdrAgtm.cpp
// ============================================================================

namespace AgtmHelpers {

SkColor4f EvaluateComponentMixingFunction(
        const AdaptiveGlobalToneMap::ComponentMixingFunction& mix, const SkColor4f& c) {
    SkColor4f out{0.f, 0.f, 0.f, 0.f};
    skia_rs_agtm_eval_component_mixing(&mix, c.vec(), out.vec());
    return out;
}

float EvaluateGainCurve(const AdaptiveGlobalToneMap::GainCurve& gainCurve, float x) {
    return skia_rs_agtm_eval_gain_curve(
            gainCurve.fControlPoints.data(), gainCurve.fControlPoints.size(), x);
}

SkColor4f EvaluateColorGainFunction(const AdaptiveGlobalToneMap::ColorGainFunction& gain,
                                    const SkColor4f& c) {
    SkColor4f out{0.f, 0.f, 0.f, 0.f};
    skia_rs_agtm_eval_color_gain(&gain.fComponentMixing,
                                 gain.fGainCurve.fControlPoints.data(),
                                 gain.fGainCurve.fControlPoints.size(),
                                 c.vec(),
                                 out.vec());
    return out;
}

void PopulateSlopeFromPCHIP(AdaptiveGlobalToneMap::GainCurve& gainCurve) {
    skia_rs_agtm_populate_slope_pchip(gainCurve.fControlPoints.data(),
                                      gainCurve.fControlPoints.size());
}

sk_sp<SkImage> MakeGainCurveXYMImage(const AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    SkiaRsAgtmHatm raw = hatm_to_ffi(hatm);
    size_t width = 0;
    size_t height = 0;
    SkiaRsMetaBytes pixels{};
    if (!skia_rs_agtm_build_xym_pixels(&raw, &width, &height, &pixels)) {
        return nullptr;
    }
    SkBitmap bm32;
    bm32.allocPixels(SkImageInfo::Make(
            static_cast<int>(width), static_cast<int>(height), kRGBA_F32_SkColorType, kPremul_SkAlphaType));
    if (pixels.ptr && pixels.len > 0) {
        for (size_t y = 0; y < height; ++y) {
            std::memcpy(bm32.getAddr(0, static_cast<int>(y)),
                        pixels.ptr + y * width * 16,
                        width * 16);
        }
    }
    skia_rs_codec_meta_free_bytes(pixels);

    SkBitmap bm16;
    bm16.allocPixels(bm32.info().makeColorType(kRGBA_F16_SkColorType));
    if (!bm32.readPixels(bm16.pixmap())) {
        return nullptr;
    }
    bm16.setImmutable();
    return SkImages::RasterFromBitmap(bm16);
}

void PopulateUsingRwtmo(AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    SkiaRsAgtmHatm raw = hatm_to_ffi(hatm);
    skia_rs_agtm_populate_rwtmo(&raw);
    hatm_from_ffi(raw, hatm);
}

Weighting ComputeWeighting(const AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm,
                           float targetedHdrHeadroom) {
    SkiaRsAgtmHatm raw = hatm_to_ffi(hatm);
    Weighting result;
    skia_rs_agtm_compute_weighting(&raw, targetedHdrHeadroom, &result);
    return result;
}

void ApplyGain(const AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm,
               SkSpan<SkColor4f> colors,
               float targetedHdrHeadroom) {
    SkASSERT(Validate(hatm));
    if (colors.empty()) {
        return;
    }
    SkiaRsAgtmHatm raw = hatm_to_ffi(hatm);
    skia_rs_agtm_apply_gain(
            &raw, reinterpret_cast<float*>(colors.data()), colors.size(), targetedHdrHeadroom);
}

sk_sp<SkColorSpace> GetGainApplicationSpace(
        const AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    skcms_Matrix3x3 toXYZD50;
    if (!hatm.fGainApplicationSpacePrimaries.toXYZD50(&toXYZD50)) {
        return nullptr;
    }
    return SkColorSpace::MakeRGB(SkNamedTransferFn::kLinear, toXYZD50);
}

sk_sp<SkColorFilter> MakeColorFilter(const AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm,
                                     float targetedHdrHeadroom,
                                     float scaleFactor) {
    auto effect = agtm_runtime_effect();
    if (!effect) {
        return nullptr;
    }

    SkiaRsAgtmHatm raw = hatm_to_ffi(hatm);
    SkiaRsColorFilterUniforms u{};
    skia_rs_agtm_compute_color_filter_uniforms(&raw, targetedHdrHeadroom, scaleFactor, &u);

    SkRuntimeShaderBuilder builder(effect);
    builder.uniform("scale_factor") = u.scale_factor;
    static constexpr std::array<const char*, 2> weight_str = {"weight_i", "weight_j"};
    static constexpr std::array<const char*, 2> mix_rgbx_str = {"mix_rgbx_i", "mix_rgbx_j"};
    static constexpr std::array<const char*, 2> mix_Mmcx_str = {"mix_Mmcx_i", "mix_Mmcx_j"};
    static constexpr std::array<const char*, 2> curve_texcoord_y_str = {"curve_texcoord_y_i",
                                                                        "curve_texcoord_y_j"};
    static constexpr std::array<const char*, 2> curve_N_cp_str = {"curve_N_cp_i", "curve_N_cp_j"};

    for (size_t a = 0; a < 2; ++a) {
        builder.uniform(weight_str[a]) = u.weight[a];
        if (u.weight[a] == 0.f) {
            continue;
        }
        builder.uniform(mix_rgbx_str[a]) =
                SkColor4f{u.mix_rgbx[a][0], u.mix_rgbx[a][1], u.mix_rgbx[a][2], u.mix_rgbx[a][3]};
        builder.uniform(mix_Mmcx_str[a]) =
                SkColor4f{u.mix_mmcx[a][0], u.mix_mmcx[a][1], u.mix_mmcx[a][2], u.mix_mmcx[a][3]};
        builder.uniform(curve_texcoord_y_str[a]) = u.curve_texcoord_y[a];
        builder.uniform(curve_N_cp_str[a]) = u.curve_n_cp[a];
    }

    if (auto gainCurvesXYM = MakeGainCurveXYMImage(hatm)) {
        builder.child("curve_xym") =
                gainCurvesXYM->makeRawShader(SkSamplingOptions(SkFilterMode::kNearest));
    }

    if (!u.has_gain_application_space) {
        return nullptr;
    }
    auto gainApplicationColorSpace =
            SkColorSpace::MakeRGB(SkNamedTransferFn::kLinear, u.gain_application_to_xyzd50);
    if (!gainApplicationColorSpace) {
        return nullptr;
    }

    auto filter = builder.makeColorFilter();
    SkASSERT(filter);
    return filter->makeWithWorkingColorSpace(gainApplicationColorSpace);
}

bool PopulateToneMapAgtmParams(const Metadata& metadata,
                               const SkColorSpace* inputColorSpace,
                               AdaptiveGlobalToneMap* outAgtm,
                               float* outScaleFactor) {
    bool inputIsPqOrHlg = false;
    float inputPqOrHlgWhite = AdaptiveGlobalToneMap::kDefaultHdrReferenceWhite;
    if (inputColorSpace) {
        skcms_TransferFunction trfn;
        inputColorSpace->transferFn(&trfn);
        switch (skcms_TransferFunction_getType(&trfn)) {
            case skcms_TFType_PQ:
            case skcms_TFType_HLG:
                inputIsPqOrHlg = true;
                inputPqOrHlgWhite = trfn.a;
                break;
            default:
                break;
        }
    }

    ContentLightLevelInformation clli;
    bool hasClli = metadata.getContentLightLevelInformation(&clli);
    MasteringDisplayColorVolume mdcv;
    bool hasMdcv = metadata.getMasteringDisplayColorVolume(&mdcv);
    AdaptiveGlobalToneMap agtm;
    bool hasAgtm = metadata.getAdaptiveGlobalToneMap(&agtm);
    SkiaRsAgtm rawIn{};
    if (hasAgtm) {
        rawIn = agtm_to_ffi(agtm);
    }

    SkiaRsAgtm rawOut{};
    float scale = 1.f;
    if (!skia_rs_agtm_populate_tone_map_params(hasClli,
                                               hasClli ? &clli : nullptr,
                                               hasMdcv,
                                               hasMdcv ? &mdcv : nullptr,
                                               hasAgtm,
                                               hasAgtm ? &rawIn : nullptr,
                                               inputIsPqOrHlg,
                                               inputPqOrHlgWhite,
                                               &rawOut,
                                               &scale)) {
        return false;
    }
    if (outAgtm) {
        agtm_from_ffi(rawOut, *outAgtm);
    }
    if (outScaleFactor) {
        *outScaleFactor = scale;
    }
    return true;
}

bool Validate(const AdaptiveGlobalToneMap& agtm) {
    SkiaRsAgtm raw = agtm_to_ffi(agtm);
    return skia_rs_agtm_validate_agtm(&raw);
}

bool Validate(const AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    SkiaRsAgtmHatm raw = hatm_to_ffi(hatm);
    return skia_rs_agtm_validate_hatm(&raw);
}

}  // namespace AgtmHelpers

SkString AdaptiveGlobalToneMap::toString() const {
    SkiaRsAgtm raw = agtm_to_ffi(*this);
    SkiaRsMetaBytes out{};
    skia_rs_agtm_to_string(&raw, &out);
    return take_meta_string(out);
}

bool AdaptiveGlobalToneMap::isValid() const {
    return AgtmHelpers::Validate(*this);
}

// ============================================================================
// SkHdrAgtmParse.cpp
// ============================================================================

bool AdaptiveGlobalToneMap::parse(const SkData* data) {
    if (!data) {
        return false;
    }
    SkiaRsAgtm raw{};
    if (!skia_rs_agtm_parse(data->bytes(), data->size(), &raw)) {
        return false;
    }
    fHdrReferenceWhite = raw.hdr_reference_white;
    if (raw.has_hatm) {
        auto& hatm = fHeadroomAdaptiveToneMap.emplace();
        hatm_from_ffi(raw.hatm, hatm);
    }
    return true;
}

sk_sp<SkData> AdaptiveGlobalToneMap::serialize() const {
    SkiaRsAgtm raw = agtm_to_ffi(*this);
    SkiaRsMetaBytes out{};
    if (!skia_rs_agtm_serialize(&raw, &out)) {
        return nullptr;
    }
    return take_meta_bytes(out);
}

}  // namespace skhdr
